//! Cables: connect a named signal to an input, or disconnect it.
//!
//! In code a cable is a name written as an argument: `{ cutoff: wobble }`, where
//! `const wobble = lfo(...)`. Connecting replaces the value written at a control
//! (`hz(500)`, `0.3`) with such a name; disconnecting replaces the name with the
//! default that the receiving ports declare, never a remembered or guessed value.
//!
//! Each is one `TextEdit` Tx confined to the bytes it writes (WRITE-10), and the planner asserts
//! what it means on the checked candidate, against the graph, not against what the candidate
//! happens to compile to (WRITE-9). Connecting must give exactly the baseline's graph with that
//! value's constants removed and their readers reading the named signal instead. Disconnecting
//! must give a graph that, rewired the same way, is exactly the baseline's. Graphs are compared up
//! to node numbering and source positions, which any insertion or removal shifts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use m2_ir::{Ir, Kernels, Node, Op, Origin, PortKind, Unit, split_ref};
use m2_runtime::PackageSet;
use m2_script::UnitCtor;
use m2_script::component::Spelled;
use m2_script::sites::{Arg, Site};
use m2_session::{Compiled, Session};
use m2_song::Sha;
use m2_song::tx::{Confined, Declared as Ranges, Expect, Range};

use crate::controls::{component_of, resolve, walk};
use crate::plan::{self, path, unmoved};
use crate::sites::{import_insertion, imported_names};
use crate::{EditError, Edited, ready};

/// The start of a cable: a call whose result is bound to a `const`, and is a signal at
/// its module's top level.
#[derive(Clone, Debug, PartialEq)]
pub struct Outlet {
    /// `file#call`: the file and the call's position among its calls. Meaningful
    /// together with `revision`.
    pub id: String,
    pub revision: u64,
    pub file: String,
    pub line: u32,
    /// The `const` it is bound to.
    pub name: String,
    pub module: String,
    /// The call's site in this revision.
    pub site: u32,
    /// Where the declaration's call is.
    start: u32,
    end: u32,
    /// The signal, as a port reference in the module.
    port: String,
}

/// The end of a cable: an outlet's name written as an argument of a call.
#[derive(Clone, Debug, PartialEq)]
pub struct Inlet {
    /// `file#call/arg`, like a control's id.
    pub id: String,
    pub revision: u64,
    pub file: String,
    pub line: u32,
    /// The site of the call it is an argument of, and the argument's path.
    pub site: u32,
    pub path: Vec<String>,
    /// The outlet it reads.
    pub from: String,
    pub name: String,
    start: u32,
    end: u32,
}

impl Outlet {
    /// Whether port reference `r` is this signal.
    pub fn reads(&self, r: &str) -> bool {
        r == self.port
    }
}

impl Inlet {
    /// Where the name is written.
    pub fn start(&self) -> u32 {
        self.start
    }

    pub fn end(&self) -> u32 {
        self.end
    }
}

/// The names written as arguments of `site`, with their argument paths.
fn names(site: &Site) -> Vec<(Vec<String>, u32, u32, String)> {
    fn visit(a: &Arg, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, u32, u32, String)>) {
        match a {
            Arg::Name { start, end, name } => out.push((path.clone(), *start, *end, name.clone())),
            Arg::Object { props, .. } => {
                for (k, v) in props {
                    path.push(k.clone());
                    visit(v, path, out);
                    path.pop();
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    for (i, a) in site.args.iter().enumerate() {
        visit(a, &mut vec![i.to_string()], &mut out);
    }
    out
}

/// The constants written in `file` between `start` and `end`, as (module, node, value).
fn consts_at(compiled: &Compiled, file: &str, start: u32, end: u32) -> Vec<(String, String, f64)> {
    let sites: HashMap<u32, &Site> = compiled.sites.iter().map(|s| (s.id, s)).collect();
    let mut found = vec![];
    for m in &compiled.ir.modules {
        walk(&m.nodes, &mut |n| {
            let (Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin) else { return };
            let Some(site) = sites.get(&o.site).filter(|s| s.file == file) else { return };
            if let Some(Arg::Number { start: at, .. }) = site.arg(&o.arg)
                && start <= *at
                && *at < end
            {
                found.push((m.key.clone(), n.id.clone(), *value));
            }
        });
    }
    found
}

/// The constants of the value at `origin` (every evaluation of an inherited control's call), as
/// (module, node, value).
pub(crate) fn consts_of(compiled: &Compiled, origin: &Origin) -> Vec<(String, String, f64)> {
    let mut found = vec![];
    for m in &compiled.ir.modules {
        walk(&m.nodes, &mut |n| {
            if let (Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin)
                && o.same_value(origin)
            {
                found.push((m.key.clone(), n.id.clone(), *value));
            }
        });
    }
    found
}

/// `ir` with the nodes `removed` (all in `module`) taken out, and everything that read
/// them reading `to` instead.
fn rewire(ir: &Ir, module: &str, removed: &[(String, String, f64)], to: &str) -> Result<Ir, EditError> {
    if removed.is_empty() {
        return Err(EditError::Rejected("the value is no longer in the graph".into()));
    }
    if removed.iter().any(|(m, ..)| m != module) {
        return Err(EditError::Rejected("a cable can only connect signals within one track".into()));
    }
    let ids: BTreeSet<&str> = removed.iter().map(|(_, id, _)| id.as_str()).collect();
    Ok(excise(ir, module, &ids, &|r| ids.contains(split_ref(r).0), to).expect("every reader of a constant reads its output"))
}

/// `ir` with nodes `ids` of `module` taken out, and every reference for which `output`
/// holds reading `to` instead. None if anything else still read a removed node.
pub(crate) fn excise(ir: &Ir, module: &str, ids: &BTreeSet<&str>, output: &dyn Fn(&str) -> bool, to: &str) -> Option<Ir> {
    let mut ir = ir.clone();
    let m = ir.modules.iter_mut().find(|m| m.key == module)?;
    let dangling = std::cell::Cell::new(false);
    let swap = |r: &mut String| {
        if output(r) {
            *r = to.to_string();
        } else if ids.contains(split_ref(r).0) {
            dangling.set(true);
        }
    };
    refs(&mut m.nodes, &mut m.exports, &|ns: &mut Vec<Node>| ns.retain(|n| !ids.contains(n.id.as_str())), &swap);
    (!dangling.get()).then_some(ir)
}

/// Apply `nodes` to every node list (outermost first) and `swap` to every port
/// reference in a module.
pub(crate) fn refs(
    nodes: &mut Vec<Node>,
    exports: &mut BTreeMap<String, String>,
    each: &dyn Fn(&mut Vec<Node>),
    swap: &dyn Fn(&mut String),
) {
    fn visit(nodes: &mut Vec<Node>, each: &dyn Fn(&mut Vec<Node>), swap: &dyn Fn(&mut String)) {
        each(nodes);
        for n in nodes {
            match &mut n.op {
                Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values_mut().for_each(swap),
                Op::Feedback { input, .. } => swap(input),
                Op::Poly { events, glide, body, .. } => {
                    swap(events);
                    glide.iter_mut().for_each(swap);
                    visit(&mut body.nodes, each, swap);
                    swap(&mut body.out);
                }
                Op::Lane { .. } | Op::Time { .. } | Op::Const { .. } | Op::Import { .. } => {}
            }
        }
    }
    visit(nodes, each, swap);
    exports.values_mut().for_each(swap);
}

/// `ir` for comparison: nodes numbered by position, without source positions or frames.
pub(crate) fn canonical(ir: &Ir) -> Ir {
    let mut ir = ir.clone();
    for m in &mut ir.modules {
        m.src = None;
        // Frames are numbered as calls ran, so adding a call renumbers the ones after it.
        m.frames.clear();
        let mut order = vec![];
        walk(&m.nodes, &mut |n| order.push(n.id.clone()));
        let names: HashMap<String, String> = order.into_iter().enumerate().map(|(i, id)| (id, format!("c{i}"))).collect();
        let rename = |r: &mut String| {
            let (id, port) = split_ref(r);
            if let Some(new) = names.get(id) {
                *r = if r.contains('.') { format!("{new}.{port}") } else { new.clone() };
            }
        };
        let strip = |ns: &mut Vec<Node>| {
            for n in ns {
                n.id = names[&n.id].clone();
                n.src = None;
                n.origin = None;
                n.frame = None;
            }
        };
        refs(&mut m.nodes, &mut m.exports, &strip, &rename);
    }
    ir
}

/// The default declared by every port the constants feed, and the ports' unit. None if
/// a reader is not a kernel or plugin port, or the ports don't declare one agreed value.
fn declared_default(ir: &Ir, consts: &[(String, String, f64)], packages: &PackageSet) -> Option<(f64, Option<Unit>)> {
    let ids: BTreeSet<&str> = consts.iter().map(|(_, id, _)| id.as_str()).collect();
    let mut found: Vec<(Option<f64>, Option<Unit>)> = vec![];
    let mut other = false;
    for m in &ir.modules {
        walk(&m.nodes, &mut |n| {
            let reads = |r: &String| ids.contains(split_ref(r).0);
            match &n.op {
                Op::Kernel { kernel, inputs, .. } | Op::Plugin { plugin: kernel, inputs, .. } => {
                    let sig = match &n.op {
                        Op::Kernel { .. } => packages.kernel(kernel),
                        _ => packages.plugin(kernel),
                    };
                    for (port, _) in inputs.iter().filter(|(_, r)| reads(r)) {
                        match sig.and_then(|s| s.inputs.iter().find(|p| p.name == *port)) {
                            Some(p) => found.push((p.default, if let PortKind::Control(u) = p.kind { u } else { None })),
                            None => other = true,
                        }
                    }
                }
                Op::Feedback { input, .. } => other |= reads(input),
                Op::Poly { events, glide, body, .. } => other |= reads(events) || glide.as_ref().is_some_and(reads) || reads(&body.out),
                _ => {}
            }
        });
        other |= m.exports.values().any(|r| ids.contains(split_ref(r).0));
    }
    let (default, unit) = *found.first()?;
    (!other && found.iter().all(|f| *f == (default, unit))).then_some((default?, unit))
}

/// How a default in base units is written when no descriptor spells it (a raw kernel port's):
/// with the unit's canonical constructor (the build's table, PKG-11) if the file imports it, else
/// as a plain number.
fn written(value: f64, unit: Option<Unit>, imported: &[String], units: &[UnitCtor]) -> String {
    let number = if value == 0.0 { "0".to_string() } else { value.to_string() };
    match unit.and_then(|u| units.iter().find(|c| c.unit == u && c.canonical)) {
        Some(c) if imported.contains(&c.name) => format!("{}({number})", c.name),
        _ => number,
    }
}

/// How a descriptor's default is written (6.5, D14): in its spelling `S` as `S(written)` when the
/// file imports `S`; as the plain number when `S` has scale 1 (it equals the base value); else as
/// `S(written)` with `S` imported, so no float noise such as `0.045000000000000005` is ever
/// written. Returns the text and the constructor to import.
pub(crate) fn spelled(d: &Spelled, imported: &[String], units: &[UnitCtor]) -> (String, Option<String>) {
    let number = |x: f64| if x == 0.0 { "0".to_string() } else { x.to_string() };
    match d.spelled.as_deref().and_then(|s| units.iter().find(|c| c.name == s)) {
        Some(c) if imported.contains(&c.name) => (format!("{}({})", c.name, number(d.written)), None),
        Some(c) if c.scale == 1.0 => (number(d.written), None),
        Some(c) => (format!("{}({})", c.name, number(d.written)), Some(c.name.clone())),
        None => (number(d.value), None),
    }
}

/// The default of the declared controls the constants `consts` of `compiled` are (PKG-4), when
/// every one is a declared control and all declare one default.
pub(crate) fn descriptor_default(compiled: &Compiled, consts: &[(String, String, f64)]) -> Option<Spelled> {
    let mut found: Option<Spelled> = None;
    for m in &compiled.ir.modules {
        let mut bad = false;
        walk(&m.nodes, &mut |n| {
            if !consts.iter().any(|(module, id, _)| *module == m.key && *id == n.id) {
                return;
            }
            let d = n.origin.as_ref().and_then(|o| {
                let slot = o.control.as_ref()?;
                component_of(compiled, m, slot.frame)?.control(&slot.slot)?.default.clone()
            });
            match (d, &found) {
                (Some(d), None) => found = Some(d),
                (Some(d), Some(f)) if d == *f => {}
                _ => bad = true,
            }
        });
        if bad {
            return None;
        }
    }
    found
}

/// Every signal bound to a `const` in the project's own files: what a cable can start from.
pub fn outlets(s: &Session) -> Vec<Outlet> {
    let rev = s.accepted();
    rev.compiled
        .sites
        .iter()
        .filter(|x| x.in_project && x.constant)
        .filter_map(|x| {
            let mut outputs = rev.compiled.outputs.iter().filter(|o| o.site == x.id);
            let output = outputs.next().filter(|o| o.top)?;
            // A call that ran more than once doesn't name one signal.
            if outputs.next().is_some() {
                return None;
            }
            Some(Outlet {
                id: format!("{}#{}", x.file, x.ordinal),
                revision: rev.number,
                file: x.file.clone(),
                line: x.line,
                name: x.binding.clone()?,
                module: output.module.clone(),
                site: x.id,
                start: x.start,
                end: x.end,
                port: output.port.clone(),
            })
        })
        .collect()
}

/// Every input connected by a cable: an outlet's name written as an argument after the
/// outlet's declaration, in the same file.
pub fn inlets(s: &Session) -> Vec<Inlet> {
    let rev = s.accepted();
    let outlets = outlets(s);
    let mut found = vec![];
    for site in rev.compiled.sites.iter().filter(|x| x.in_project) {
        for (path, start, end, name) in names(site) {
            // The nearest declaration before it; the build checks that it is the one meant.
            let from = outlets.iter().filter(|o| o.file == site.file && o.name == name && o.end <= start).max_by_key(|o| o.start);
            let Some(from) = from else { continue };
            found.push(Inlet {
                id: format!("{}#{}/{}", site.file, site.ordinal, path.join(".")),
                revision: rev.number,
                file: site.file.clone(),
                line: site.line,
                site: site.id,
                path,
                from: from.id.clone(),
                name,
                start,
                end,
            });
        }
    }
    found
}

/// The site of call `file#ordinal` from revision `at`, as it is in the accepted revision:
/// carried forward only if no change touched it.
pub(crate) fn current_site<'a>(s: &'a Session, at: u64, call: &str) -> Result<&'a Site, EditError> {
    let parse = || {
        let (file, ordinal) = call.rsplit_once('#')?;
        Some((file, ordinal.parse::<u32>().ok()?))
    };
    let (file, ordinal) = parse().ok_or_else(|| EditError::NotFound(call.to_string()))?;
    let accepted = s.accepted();
    let sites = &accepted.compiled.sites;
    if at == accepted.number {
        return sites.iter().find(|x| x.file == file && x.ordinal == ordinal).ok_or_else(|| EditError::NotFound(call.to_string()));
    }
    let carried = || -> Option<&Site> {
        let past = s.past(at)?;
        let old = past.sites().iter().find(|x| x.file == file && x.ordinal == ordinal)?;
        let p = Path::new(file);
        let (was, is) = (past.snapshot.files.get(p)?, accepted.snapshot.files.get(p)?);
        let to = m2_session::text::Edits::between(was, is).carry(old.start as usize..old.end as usize)?.start as u32;
        sites.iter().find(|x| x.file == file && x.start == to && x.end == to + (old.end - old.start))
    };
    carried().ok_or(EditError::Stale { current: accepted.number })
}

fn outlet(s: &Session, at: u64, id: &str) -> Result<Outlet, EditError> {
    let site = current_site(s, at, id)?.id;
    outlets(s)
        .into_iter()
        .find(|o| o.site == site)
        .ok_or_else(|| EditError::Rejected(format!("`{id}` is not a named signal a cable can start from")))
}

/// What a connect or a disconnect may change: the bytes it writes, and nothing else (WRITE-10).
fn confined(file: &str, ranges: &[(u32, u32)]) -> Result<Expect, EditError> {
    let ranges = Ranges::Ranges(ranges.iter().map(|&(a, b)| Range::bytes(a, b)).collect());
    Ok(Expect::Confined(Confined { ranges: [(path(file)?, ranges)].into(), ..Confined::default() }))
}

/// Connect outlet `from` to control `to`: write the outlet's name where the control's value is
/// written, or, for a control its call leaves out, into the call as its first edit would write a
/// value (D5). Ids are from revision `at`.
pub fn connect(s: &mut Session, at: u64, from: &str, to: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let outlet = outlet(s, at, from)?;
    let control = resolve(s, at, to)?;
    if control.file != outlet.file {
        return Err(EditError::Rejected(format!("`{}` is in another file", outlet.name)));
    }
    // The whole written value (the `hz(500)` call for a quantity, else the number); or where
    // the name is inserted.
    let (edits, inserted) = match control.planned(&outlet.name) {
        Some(edits) => (edits, true),
        None => {
            let (start, end) = control.expression().ok_or_else(|| EditError::NotFound(to.to_string()))?;
            (vec![(start, end, outlet.name.clone())], false)
        }
    };
    let (start, end) = (edits[0].0, edits[edits.len() - 1].1);
    if start < outlet.end {
        return Err(EditError::Rejected(format!("`{}` is declared after this value; move it above to connect it", outlet.name)));
    }
    let old = s.accepted().snapshot.files[Path::new(&control.file)].clone();
    // Written into the call, the name is shown with its call (or its dictionary entry), as a value
    // written there is: the bytes alone (`, phase: drop`) say nothing.
    let label = match inserted {
        true => crate::label::inserted(s, &control, &edits),
        false => (old[start as usize..end as usize].to_string(), outlet.name.clone()),
    };
    let file = path(&control.file)?;
    let op = plan::text_edit(&file, &old, &edits);
    // A control the call leaves out is made by the call itself (PKG-3): the call's range is what
    // changes, the name written inside it.
    let site = s.accepted().compiled.sites.iter().find(|x| x.id == control.origin.site).map(|x| (x.start, x.end));
    let range = match (inserted, site) {
        (true, Some(call)) => call,
        _ => (start, end),
    };
    let tx = plan::planned(s, label.clone(), vec![op], confined(&control.file, &[range])?);
    let refused = || EditError::Rejected(format!("connecting `{}` here would change more than this input", outlet.name));
    let said = |why: &str| EditError::Rejected(format!("connecting `{}` here would change more than this input ({why})", outlet.name));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion (WRITE-9): exactly this value's constants are gone, and their
        // readers read the outlet.
        let base = &checked.baseline().compiled;
        let consts = match inserted {
            true => consts_of(base, &control.origin),
            false => consts_at(base, &control.file, start, end),
        };
        let expected = rewire(&base.ir, &outlet.module, &consts, &outlet.port)?;
        match canonical(&checked.candidate().compiled.ir) == canonical(&expected) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: control.file, before: label.0, after: label.1 })
}

/// Disconnect inlet `id`: write the default in place of the name, as the receiving control's
/// descriptor spells it (PKG-4), or else as the ports it feeds declare it; never a remembered or
/// guessed value.
pub fn disconnect(s: &mut Session, at: u64, id: &str) -> Result<Edited, EditError> {
    ready(s)?;
    // `tracks/bass.ts#8/1`: the file may hold `/`, the argument path never does.
    let (call, arg) = id.rsplit_once('/').ok_or_else(|| EditError::NotFound(id.to_string()))?;
    let site = current_site(s, at, call)?.id;
    let inlet = inlets(s).into_iter().find(|i| i.site == site && i.path.join(".") == arg);
    let inlet = inlet.ok_or_else(|| EditError::Rejected(format!("`{id}` is not an input connected by a cable")))?;
    let outlet = outlets(s).into_iter().find(|o| o.id == inlet.from).expect("inlets read outlets");
    let refuse = || EditError::Rejected(format!("this input declares no default to go back to; replace `{}` in the code", inlet.name));
    let file = path(&inlet.file)?;
    let old = s.accepted().snapshot.files[Path::new(&inlet.file)].clone();

    // Which controls or ports the input feeds decides the default: a plain number written there,
    // checked and never committed (R26: a check on `Seen` never refuses on its expectation).
    let probe = plan::planned(
        s,
        (String::new(), String::new()),
        vec![plan::text_edit(&file, &old, &[(inlet.start, inlet.end, "0".into())])],
        Expect::Seen(Sha::of(b"probe")),
    );
    let checked = s.check(&probe).map_err(|r| crate::refused(s, r))?;
    unmoved(s, &checked)?;
    let candidate = &checked.candidate().compiled;
    let consts = consts_at(candidate, &inlet.file, inlet.start, inlet.start + 1);
    if consts.is_empty() {
        return Err(refuse());
    }
    let imported = imported_names(&old, "@m2/core");
    let units = candidate.units.clone();
    let (default, text, import) = match descriptor_default(candidate, &consts) {
        Some(d) => {
            let (text, import) = spelled(&d, &imported, &units);
            (d.value, text, import)
        }
        None => {
            let (default, unit) = declared_default(&candidate.ir, &consts, &s.packages).ok_or_else(refuse)?;
            (default, written(default, unit, &imported, &units), None)
        }
    };
    let mut edits = vec![(inlet.start, inlet.end, text.clone())];
    let import = import.and_then(|name| import_insertion(&old, "@m2/core", &name));
    // Where a position of the file is once the import is in.
    let added = import.as_ref().map(|(at, line)| (*at, line.len() as u32));
    let moved = |pos: u32| pos + added.filter(|(at, _)| *at <= pos).map_or(0, |(_, len)| len);
    let mut ranges = vec![(inlet.start, inlet.end)];
    if let Some((at, line)) = import {
        ranges.push((at, at));
        edits.push((at, at, line));
    }
    edits.sort_by_key(|e| e.0);
    let label = (old[inlet.start as usize..inlet.end as usize].to_string(), text.clone());
    let tx = plan::planned(s, label.clone(), vec![plan::text_edit(&file, &old, &edits)], confined(&inlet.file, &ranges)?);
    let changed = || EditError::Rejected(format!("disconnecting `{}` would change more than this input", inlet.name));
    let said = |why: &str| EditError::Rejected(format!("disconnecting `{}` would change more than this input ({why})", inlet.name));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        let candidate = &checked.candidate().compiled;
        let at = moved(inlet.start);
        let consts = consts_at(candidate, &inlet.file, at, at + text.len() as u32);
        if consts.is_empty() || consts.iter().any(|(.., v)| *v != default) {
            return Err(refuse());
        }
        // The planner's assertion (WRITE-9): rewired back to the outlet, the new constants give the
        // baseline's graph. The outlet is before the edit, so it is where it was, past any import.
        let site = candidate.sites.iter().find(|x| x.file == inlet.file && x.start == moved(outlet.start)).ok_or_else(changed)?;
        let port = candidate.outputs.iter().find(|o| o.site == site.id && o.top).ok_or_else(changed)?.port.clone();
        let back = rewire(&candidate.ir, &outlet.module, &consts, &port)?;
        match canonical(&back) == canonical(&checked.baseline().compiled.ir) {
            true => Ok(()),
            false => Err(changed()),
        }
    })?;
    Ok(Edited { revision: done.number, file: inlet.file, before: label.0, after: label.1 })
}

#[cfg(test)]
#[path = "cable_tests.rs"]
mod tests;

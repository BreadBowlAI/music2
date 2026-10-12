//! Cables: connect a named signal to an input, or disconnect it.
//!
//! In code a cable is a name written as an argument: `{ cutoff: wobble }`, where
//! `const wobble = lfo(...)`. Connecting replaces the value written at a control
//! (`hz(500)`, `0.3`) with such a name; disconnecting replaces the name with the
//! default that the receiving ports declare, never a remembered or guessed value.
//!
//! Both are verified against the graph, not against what the candidate happens to
//! compile to. Connecting must give exactly the old graph with that value's constants
//! removed and their readers reading the named signal instead. Disconnecting must give
//! a graph that, rewired the same way, is exactly the old one. Graphs are compared up
//! to node numbering and source positions, which any insertion or removal shifts.

use std::collections::{BTreeSet, HashMap};

use m2_ir::{Kernels, PortKind, split_ref};
use m2_script::sites::{Arg, imported_names};

use super::*;

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
                found.push((m.name.clone(), n.id.clone(), *value));
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
pub(super) fn excise(ir: &Ir, module: &str, ids: &BTreeSet<&str>, output: &dyn Fn(&str) -> bool, to: &str) -> Option<Ir> {
    let mut ir = ir.clone();
    let m = ir.modules.iter_mut().find(|m| m.name == module)?;
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
pub(super) fn refs(
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
                Op::Poly { events, body, .. } => {
                    swap(events);
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

/// `ir` for comparison: nodes numbered by position, without source positions.
pub(super) fn canonical(ir: &Ir) -> Ir {
    let mut ir = ir.clone();
    for m in &mut ir.modules {
        m.src = None;
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
                n.within.clear();
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
                Op::Kernel { kernel, inputs } | Op::Plugin { plugin: kernel, inputs, .. } => {
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
                Op::Poly { events, body, .. } => other |= reads(events) || reads(&body.out),
                _ => {}
            }
        });
        other |= m.exports.values().any(|r| ids.contains(split_ref(r).0));
    }
    let (default, unit) = *found.first()?;
    (!other && found.iter().all(|f| *f == (default, unit))).then_some((default?, unit))
}

/// How a default is written: with the unit's constructor if the file imports it.
fn written(value: f64, unit: Option<Unit>, imported: &[String]) -> String {
    let number = if value == 0.0 { "0".to_string() } else { value.to_string() };
    let constructor = unit.map(|u| match u {
        Unit::Hz => "hz",
        Unit::Db => "db",
        Unit::Gain => "linear",
        Unit::Semitones => "semis",
        Unit::Seconds => "secs",
        Unit::Beats => "beats",
        Unit::Unit01 => "frac",
        Unit::Bipolar => "bipolar",
        Unit::Scalar => "scalar",
    });
    match constructor {
        Some(c) if imported.iter().any(|n| n == c) => format!("{c}({number})"),
        _ => number,
    }
}

impl Session {
    /// Every signal bound to a `const` in the project's own files: what a cable can start from.
    pub fn outlets(&self) -> Vec<Outlet> {
        let rev = &self.accepted;
        rev.compiled
            .sites
            .iter()
            .filter(|s| s.in_project && s.constant)
            .filter_map(|s| {
                let mut outputs = rev.compiled.outputs.iter().filter(|o| o.site == s.id);
                let output = outputs.next().filter(|o| o.top)?;
                // A call that ran more than once doesn't name one signal.
                if outputs.next().is_some() {
                    return None;
                }
                Some(Outlet {
                    id: format!("{}#{}", s.file, s.ordinal),
                    revision: rev.number,
                    file: s.file.clone(),
                    line: s.line,
                    name: s.binding.clone()?,
                    module: output.module.clone(),
                    site: s.id,
                    start: s.start,
                    end: s.end,
                    port: output.port.clone(),
                })
            })
            .collect()
    }

    /// Every input connected by a cable: an outlet's name written as an argument after
    /// the outlet's declaration, in the same file.
    pub fn inlets(&self) -> Vec<Inlet> {
        let rev = &self.accepted;
        let outlets = self.outlets();
        let mut found = vec![];
        for site in rev.compiled.sites.iter().filter(|s| s.in_project) {
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

    /// The site of call `file#ordinal` from revision `at`, as it is in the accepted
    /// revision: carried forward only if no change touched it.
    pub(super) fn current_site(&self, at: u64, call: &str) -> Result<&Site, EditError> {
        let parse = || {
            let (file, ordinal) = call.rsplit_once('#')?;
            Some((file, ordinal.parse::<u32>().ok()?))
        };
        let (file, ordinal) = parse().ok_or_else(|| EditError::NotFound(call.to_string()))?;
        let sites = &self.accepted.compiled.sites;
        if at == self.accepted.number {
            return sites.iter().find(|s| s.file == file && s.ordinal == ordinal).ok_or_else(|| EditError::NotFound(call.to_string()));
        }
        let carried = || -> Option<&Site> {
            let past = self.past.iter().find(|p| p.number == at)?;
            let old = past.sites.iter().find(|s| s.file == file && s.ordinal == ordinal)?;
            let path = Path::new(file);
            let to = carry(past.snapshot.files.get(path)?, self.accepted.snapshot.files.get(path)?, old.start as usize, old.end as usize)?
                as u32;
            sites.iter().find(|s| s.file == file && s.start == to && s.end == to + (old.end - old.start))
        };
        carried().ok_or(EditError::Stale { current: self.accepted.number })
    }

    fn outlet(&self, at: u64, id: &str) -> Result<Outlet, EditError> {
        let site = self.current_site(at, id)?.id;
        self.outlets().into_iter().find(|o| o.site == site).ok_or_else(|| EditError::Rejected(format!("`{id}` is not a named signal a cable can start from")))
    }

    pub(super) fn ready(&mut self) -> Result<(), EditError> {
        self.refresh();
        if !self.draft_errors.is_empty() {
            return Err(EditError::DraftInvalid(self.draft_errors.clone()));
        }
        Ok(())
    }

    /// Connect outlet `from` to control `to`: write the outlet's name where the
    /// control's value is written. Ids are from revision `at`.
    pub fn connect(&mut self, at: u64, from: &str, to: &str) -> Result<Edited, EditError> {
        self.ready()?;
        let outlet = self.outlet(at, from)?;
        let control = self.resolve(at, to)?;
        let compiled = &self.accepted.compiled;
        let site = compiled.sites.iter().find(|s| s.id == control.origin.site).ok_or_else(|| EditError::NotFound(to.to_string()))?;
        // The whole written value: the `hz(500)` call for a unit value, else the number.
        let (start, end) = if control.unit.is_some() { (site.start, site.end) } else { (control.lens.start, control.lens.end) };
        if control.file != outlet.file {
            return Err(EditError::Rejected(format!("`{}` is in another file", outlet.name)));
        }
        if start < outlet.end {
            return Err(EditError::Rejected(format!("`{}` is declared after this value; move it above to connect it", outlet.name)));
        }
        let consts = consts_at(compiled, &control.file, start, end);
        let expected = rewire(&compiled.ir, &outlet.module, &consts, &outlet.port)?;
        let candidate = self.candidate(Path::new(&control.file), start, end, &outlet.name)?;
        if canonical(&candidate.compiled.ir) != canonical(&expected) {
            return Err(EditError::Rejected(format!("connecting `{}` here would change more than this input", outlet.name)));
        }
        self.write(candidate)
    }

    /// Disconnect inlet `id`: write the default its ports declare in place of the name.
    pub fn disconnect(&mut self, at: u64, id: &str) -> Result<Edited, EditError> {
        self.ready()?;
        // `tracks/bass.ts#8/1`: the file may hold `/`, the argument path never does.
        let (call, path) = id.rsplit_once('/').ok_or_else(|| EditError::NotFound(id.to_string()))?;
        let site = self.current_site(at, call)?.id;
        let inlet = self.inlets().into_iter().find(|i| i.site == site && i.path.join(".") == path);
        let inlet = inlet.ok_or_else(|| EditError::Rejected(format!("`{id}` is not an input connected by a cable")))?;
        let outlet = self.outlets().into_iter().find(|o| o.id == inlet.from).expect("inlets read outlets");
        let file = Path::new(&inlet.file);
        let refuse = || EditError::Rejected(format!("this input declares no default to go back to; replace `{}` in the code", inlet.name));

        // Which ports the input feeds decides the default: try a plain number first.
        let probe = self.candidate(file, inlet.start, inlet.end, "0")?;
        let consts = consts_at(&probe.compiled, &inlet.file, inlet.start, inlet.start + 1);
        if consts.is_empty() {
            return Err(refuse());
        }
        let (default, unit) = declared_default(&probe.compiled.ir, &consts, &self.packages).ok_or_else(refuse)?;
        let text = written(default, unit, &imported_names(&probe.old, "@m2/core"));

        let candidate = self.candidate(file, inlet.start, inlet.end, &text)?;
        let consts = consts_at(&candidate.compiled, &inlet.file, inlet.start, inlet.start + text.len() as u32);
        if consts.iter().any(|(.., v)| *v != default) {
            return Err(refuse());
        }
        // The outlet is before the edit, so it is at the same place in the candidate.
        let changed = || EditError::Rejected(format!("disconnecting `{}` would change more than this input", inlet.name));
        let site = candidate.compiled.sites.iter().find(|s| s.file == inlet.file && s.start == outlet.start).ok_or_else(changed)?;
        let port = candidate.compiled.outputs.iter().find(|o| o.site == site.id && o.top).ok_or_else(changed)?.port.clone();
        let back = rewire(&candidate.compiled.ir, &outlet.module, &consts, &port)?;
        if canonical(&back) != canonical(&self.accepted.compiled.ir) {
            return Err(changed());
        }
        self.write(candidate)
    }

    fn write(&mut self, candidate: Candidate) -> Result<Edited, EditError> {
        let file = candidate.file.display().to_string();
        let (before, after) = (candidate.before.clone(), candidate.after.clone());
        let undo = self.exact(&candidate.file);
        let revision = self.commit(candidate, Some(undo))?;
        Ok(Edited { revision, file, before, after })
    }
}

#[cfg(test)]
#[path = "cable_tests.rs"]
mod tests;

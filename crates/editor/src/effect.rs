//! Effects: put one of the packages' effects after a call's audio, or take it out.
//!
//! An effect is a component whose descriptor has an editor category (PKG-2, PKG-7); the
//! packages' catalog lists them (PKG-11). Adding wraps the call where it is written:
//! `pluck(notes)` becomes `delay(pluck(notes))`, importing `delay` if the file doesn't. That works
//! wherever the call is (an argument, a return, an arrow's body) and needs no new names. The
//! descriptor's `insert` is written after the audio, `lowpass(pluck(notes), 1000)`, so its
//! settings are knobs from the start (and every other control is a knob too, inherited).
//! Removing is the inverse, `delay(x, …)` back to `x`: the effect's own options go with it.
//!
//! A call is an effect by the ComponentId its frame records (PKG-9), never by how its callee is
//! spelled: `echo(x)` after `import { delay as echo }` is `@m2/effects:delay`.
//!
//! Each is one `TextEdit` Tx confined to the call (WRITE-10). Adding is two insertions around the
//! call, `delay(` before it and `, …)` after it, so the wrapped call's characters stay as they are
//! and its nodes keep their names (WRITE-8, R5): the delta is the effect's nodes and the rewired
//! readers. Both are asserted the same way by the planner, against the graph (WRITE-9). Take the
//! effect call's own nodes (the numbers written in its arguments and the controls it leaves out
//! included: they are made in its frame) out of the graph that has it, and make whatever read its
//! output read its input instead: that must be exactly the graph without it.
//!
//! Turning an effect off (Next D1) writes `bypass: true` into its options: its call still runs
//! and makes its nodes, so its knobs stay to set, and gives its audio input on, so nothing reads
//! them and the engine never makes them (Q24). Turning it on takes the property out. Each is one
//! `TextEdit` confined to the call, asserted against the graph as removing is: with the effect's
//! own nodes taken out of the graph that has it turned off, that graph must be exactly the one
//! without it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use m2_ir::{Ir, Module, Node, Op, split_ref};
use m2_script::Descriptor;
use m2_script::component::{PortDesc, PortKind, Spelled};
use m2_script::sites::{Arg, Site};
use m2_session::{Checked, Compiled, Session};
use m2_song::tx::{Confined, Declared as Ranges, Expect, Range};
use serde::Serialize;

use crate::cable::{canonical, current_site, excise, refs};
use crate::controls::{options_of, walk};
use crate::label::shown_call;
use crate::lens::Insertion;
use crate::plan::{self, path, unmoved};
use crate::sites::{import_insertion, imported_names};
use crate::{EditError, Edited, ready};

/// An effect the packages offer, as the effect browser lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EffectInfo {
    /// The ComponentId (`@m2/effects:delay`): what adding it names.
    pub component: String,
    /// What it is called (`Delay`).
    pub label: String,
    /// Where it is imported from and as what: `delay` from `@m2/effects`.
    pub name: String,
    pub module: String,
    pub description: String,
    /// A free string the browser groups by (PKG-7).
    pub category: String,
    /// What adding it writes after the audio, from its descriptor's insert: `, 1000`, or
    /// `, { time: 0.25, feedback: 0.3, mix: 0.3 }`; empty when it inserts nothing.
    pub insert: String,
    /// The pack it is from, with its version; none for std's and the project's (PACK-11).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<crate::packs::From>,
}

/// The component frames of `m` whose chain holds `n`, outermost first: the sites of the
/// component calls it was made inside (ID-15). `key()` frames aren't calls of a component.
pub(crate) fn calls_of<'a>(m: &'a Module, n: &Node) -> impl Iterator<Item = u32> + 'a {
    m.chain(n.frame).into_iter().filter(|f| f.key.is_none()).map(|f| f.site)
}

/// The nodes call `site` made, and the module they are in.
pub(crate) fn made_by(compiled: &Compiled, site: u32) -> Option<(String, BTreeSet<String>)> {
    let output = compiled.outputs.iter().find(|o| o.site == site)?;
    let mut ids = BTreeSet::new();
    let m = compiled.ir.modules.iter().find(|m| m.key == output.module)?;
    walk(&m.nodes, &mut |n| {
        if calls_of(m, n).any(|s| s == site) || n.origin.as_ref().is_some_and(|o| o.site == site) {
            ids.insert(n.id.clone());
        }
    });
    Some((output.module.clone(), ids))
}

/// What call `site` of `c` reads from outside itself (one of these is its input), its own
/// output, its nodes and their module: `None` unless it gave exactly one output it made.
fn reads_of(c: &Compiled, site: u32) -> Option<(String, BTreeSet<String>, String, BTreeSet<String>)> {
    let mut outputs = c.outputs.iter().filter(|o| o.site == site);
    let (Some(output), None) = (outputs.next(), outputs.next()) else { return None };
    let (module, ids) = made_by(c, site)?;
    if !ids.contains(split_ref(&output.port).0) {
        return None;
    }
    // Its nodes may be in a voice (an effect inside `poly`), so every node list is searched,
    // voices included.
    let inputs = std::cell::RefCell::new(BTreeSet::new());
    let m = c.ir.modules.iter().find(|m| m.key == module).expect("the output's module");
    walk(&m.nodes, &mut |n| {
        if !ids.contains(n.id.as_str()) {
            return;
        }
        // The node's own references: a voice's nodes are walked in their turn.
        let mut node = vec![n.clone()];
        if let Op::Poly { body, .. } = &mut node[0].op {
            body.nodes.clear();
        }
        refs(&mut node, &mut BTreeMap::new(), &|_| {}, &|r: &mut String| {
            if !ids.contains(split_ref(r).0) {
                inputs.borrow_mut().insert(r.clone());
            }
        });
    });
    Some((module, ids, output.port.clone(), inputs.into_inner()))
}

/// `ir` (a graph with call `site` of `c` in it, as `c` made it) without that call: its nodes
/// out, and what read its output reading one of what it read instead; one graph for each.
fn unwrapped(c: &Compiled, ir: &Ir, site: u32) -> Vec<Ir> {
    let Some((module, ids, output, inputs)) = reads_of(c, site) else { return vec![] };
    let ids: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    inputs.iter().filter_map(|input| excise(ir, &module, &ids, &|r| r == output, input)).collect()
}

/// Whether `with` is `without` plus effect call `site`: removing the call's nodes and
/// reading what they read in place of their output gives exactly `without`.
fn wraps(with: &Compiled, without: &Ir, site: u32) -> bool {
    let without = canonical(without);
    unwrapped(with, &with.ir, site).iter().any(|ir| canonical(ir) == without)
}

/// How an effect's insert value is written (D11): a number in a spelling of scale 1 (all of
/// std's) as itself, so the call reads as the descriptor wrote it; in another spelling with its
/// constructor (`ms(45)`), never a base value with float noise. The constructors the text needs
/// imported come back with it.
pub(crate) fn insert_text(v: &Spelled, units: &[m2_script::UnitCtor]) -> (String, Option<String>) {
    let number = |x: f64| if x == 0.0 { "0".to_string() } else { x.to_string() };
    match v.spelled.as_deref().and_then(|s| units.iter().find(|u| u.name == s)) {
        Some(u) if u.scale != 1.0 => (format!("{}({})", u.name, number(v.written)), Some(u.name.clone())),
        Some(_) => (number(v.written), None),
        None => (number(v.value), None),
    }
}

/// The arguments adding effect `d` writes after the audio (D11): positional when its insert's
/// slots are its first positional ones in order, else an options object in the insert's order;
/// and the unit constructors that text needs imported.
fn insert_args(d: &Descriptor, units: &[m2_script::UnitCtor]) -> (String, Vec<String>) {
    let values: Vec<(&str, String, Option<String>)> = d
        .editor
        .insert
        .iter()
        .map(|i| {
            let (v, ctor) = insert_text(&i.value, units);
            (i.slot.as_str(), v, ctor)
        })
        .collect();
    let positional = values.len() <= d.positional.len() && values.iter().zip(&d.positional).all(|(v, p)| v.0 == p);
    let text = match () {
        _ if values.is_empty() => String::new(),
        _ if positional => values.iter().map(|v| format!(", {}", v.1)).collect(),
        _ => format!(", {{ {} }}", values.iter().map(|v| format!("{}: {}", v.0, v.1)).collect::<Vec<_>>().join(", ")),
    };
    (text, values.into_iter().filter_map(|v| v.2).collect())
}

/// The effects the packages offer: every catalogued component with an editor category, and
/// where to import it from; in the search's order of packages ([`crate::library::origin`]: std's
/// and music2's own first, std's toolbox before the packs', then the song's, then everyone else's),
/// each in the order its package declares them (by file, then line).
pub fn effects(s: &Session) -> Vec<EffectInfo> {
    let catalog = s.catalog();
    let mut found: Vec<(&Descriptor, EffectInfo)> = catalog
        .effects()
        .filter_map(|d| {
            let export = catalog.exports.get(&d.id)?.iter().find(|e| !e.name.contains('.'))?;
            let info = EffectInfo {
                component: d.id.clone(),
                label: d.label.clone(),
                name: export.name.clone(),
                module: export.module.clone(),
                description: d.editor.description.clone(),
                category: d.editor.category.clone().unwrap_or_default(),
                insert: insert_args(d, &s.accepted().compiled.units).0,
                pack: crate::packs::from(s, &d.id),
            };
            Some((&**d, info))
        })
        .collect();
    // `at` is `file:line:col`.
    let line = |d: &Descriptor| d.at.rsplit(':').nth(1).and_then(|l| l.parse::<u32>().ok()).unwrap_or(0);
    let rank = |d: &Descriptor| {
        let package = d.id.split_once(':').map_or("song", |(p, _)| p);
        let own = s.offered_one(package).is_some_and(|(_, dir)| crate::library::own(package, dir));
        (crate::library::origin(package, own), package != m2_song::pack::STD)
    };
    found.sort_by_cached_key(|(d, _)| (rank(d), d.file.clone(), line(d)));
    found.into_iter().map(|(_, e)| e).collect()
}

/// The effect that call `site` of the accepted revision is, if it is a call in the project of a
/// component with an editor category: by the ComponentId its frames record (PKG-9).
pub(crate) fn effect_at(s: &Session, site: &Site) -> Option<Arc<Descriptor>> {
    if !site.in_project {
        return None;
    }
    let compiled = &s.accepted().compiled;
    let mut ids = compiled.ir.modules.iter().flat_map(|m| &m.frames).filter(|f| f.site == site.id).map(|f| f.component.as_deref());
    // Every evaluation of the call is one component (a call takes only its own site, D17).
    let id = ids.next()??;
    if ids.any(|x| x != Some(id)) {
        return None;
    }
    compiled.components.get(id).filter(|d| d.is_effect()).cloned()
}

/// Insertions joined where they go: two at one place become one edit.
fn joined(mut imports: Vec<(u32, String)>) -> Vec<(u32, String)> {
    imports.sort_by_key(|(at, _)| *at);
    let mut out: Vec<(u32, String)> = vec![];
    for (at, line) in imports {
        match out.last_mut() {
            Some((a, l)) if *a == at => l.push_str(&line),
            _ => out.push((at, line)),
        }
    }
    out
}

/// Where a module reads track `track`'s export `export` (CARD-36): the one module that does, not a
/// track's (a track can't read another), in its file, at the one place that file writes
/// `<handle>.<export>`, `<handle>` the name it imports the track by. As a site of its own, to be
/// wrapped as a call is: the read's text is its callee. Why not, in words.
pub(crate) fn track_read(s: &Session, track: &str, export: &str) -> Result<Site, String> {
    let compiled = &s.accepted().compiled;
    let named = compiled.ir.modules.iter().find(|t| t.key == track).map_or(track.to_string(), |t| t.label().to_string());
    let reads = |m: &Module| {
        let mut found = false;
        walk(&m.nodes, &mut |n| found |= matches!(&n.op, Op::Import { module, export: e } if module == track && e == export));
        found
    };
    let mut readers = compiled.ir.modules.iter().filter(|m| !m.track && reads(m));
    let (Some(m), None) = (readers.next(), readers.next()) else {
        return Err(format!("{named}'s sound isn't read in one place of the song"));
    };
    // Read in one place by the graph: its import feeds one node (a node's input, a voice's
    // capture, an export). The text alone can't tell: `(({ out }) => out)(hook)` reads it too.
    let reads = consumers(m, track, export);
    if reads != 1 {
        return Err(format!("the song reads {named}'s sound in {reads} places, so where it would go isn't known"));
    }
    let (file, from) = (file_of(m).ok_or("its module has no file")?, compiled.ir.modules.iter().find(|t| t.key == track));
    let text = s.accepted().snapshot.files.get(Path::new(file)).ok_or("its file isn't the song's")?;
    let handle = from
        .and_then(file_of)
        .and_then(|t| crate::sites::imports_of(text, file, t).into_iter().find_map(|i| i.default))
        .ok_or_else(|| format!("{file} doesn't import {named} by a name of its own"))?;
    let written = format!("{handle}.{export}");
    let ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    let at: Vec<u32> = crate::sites::references(text, &handle)
        .into_iter()
        .filter(|&a| {
            text[a as usize..].starts_with(&written) && !text[a as usize + written.len()..].starts_with(|c: char| ident(c) || c == '.')
        })
        .collect();
    let start = match at[..] {
        [start] => start,
        [] => return Err(format!("{file} doesn't write {written} by name")),
        _ => return Err(format!("{file} writes {written} in {} places, so which one is {named}'s sound isn't known", at.len())),
    };
    let end = start + written.len() as u32;
    let line = text[..start as usize].matches('\n').count() as u32 + 1;
    Ok(Site {
        id: u32::MAX,
        file: file.to_string(),
        ordinal: u32::MAX,
        start,
        end,
        open: start,
        line,
        callee: written,
        binding: None,
        constant: false,
        in_project: true,
        args: vec![],
    })
}

/// How many times module `m` reads track `track`'s export `export`: the inputs, captures and exports
/// that name one of its imports of it.
fn consumers(m: &Module, track: &str, export: &str) -> usize {
    let mut imports = BTreeSet::new();
    walk(&m.nodes, &mut |n| {
        if matches!(&n.op, Op::Import { module, export: e } if module == track && e == export) {
            imports.insert(n.id.clone());
        }
    });
    let count = std::cell::Cell::new(0);
    let (mut nodes, mut exports) = (m.nodes.clone(), m.exports.clone());
    let swap = |r: &mut String| {
        if imports.contains(split_ref(r).0) {
            count.set(count.get() + 1);
        }
    };
    refs(&mut nodes, &mut exports, &|_| {}, &swap);
    count.get()
}

/// Put effect `component` (a ComponentId, `@m2/effects:delay`, or an installed plugin, `clap:<id>`) after
/// call `call` (`file#ordinal` at revision `at`), or after a track's sound where the song reads it
/// (`import:<track>.<export>`, CARD-36).
pub fn add_effect(s: &mut Session, at: u64, call: &str, component: &str) -> Result<Edited, EditError> {
    if component.starts_with("clap:") {
        return crate::plugins::add_effect(s, at, call, component);
    }
    ready(s)?;
    let effect = effects(s).into_iter().find(|e| e.component == component).ok_or_else(|| EditError::NotFound(component.to_string()))?;
    let desc = s.catalog().components[component].clone();
    let site = match call.strip_prefix("import:").and_then(|r| r.split_once('.')) {
        Some((track, export)) => {
            let site =
                track_read(s, track, export).map_err(|why| EditError::Rejected(format!("{} can't go here: {why}.", effect.label)))?;
            if at != s.accepted().number {
                return Err(EditError::stale(s, at, &site.file));
            }
            site
        }
        None => current_site(s, at, call)?.clone(),
    };
    if !site.in_project {
        return Err(EditError::NotFound(call.to_string()));
    }
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let (args, ctors) = insert_args(&desc, &s.accepted().compiled.units);
    let mut imports = vec![];
    if let Some(i) = import_insertion(&text, &effect.module, &effect.name) {
        imports.push(i);
    }
    for ctor in &ctors {
        if !imported_names(&text, "@m2/core").contains(ctor) && !imports.iter().any(|(_, line)| line.contains(ctor.as_str())) {
            imports.extend(import_insertion(&text, "@m2/core", ctor));
        }
    }
    // A pack's effect brings its `[dependencies]` line and lock entry (PACK-10).
    let also = crate::packs::needs(s, component)?;
    let wrap =
        Wrap { open: format!("{}(", effect.name), close: format!("{args})"), imports, callee: effect.name, label: effect.label, also };
    wrap_call(s, &site, wrap, |_, _, _| Ok(()))
}

/// How a call is wrapped: the text written before it and after it, the imports that text needs,
/// the new call's callee (how the candidate's site is found), and what is added, in words.
pub(crate) struct Wrap {
    pub open: String,
    pub close: String,
    pub imports: Vec<(u32, String)>,
    pub callee: String,
    pub label: String,
    /// What else the Tx writes: a pack's dependency and lock (PACK-10).
    pub also: crate::packs::Needs,
}

/// Wrap call `site` of the accepted revision as `w` says, as one `TextEdit` Tx confined to the call:
/// two insertions around it (R5), so its characters stay as they are, and the imports. The planner
/// asserts `check` on the new call's site in the candidate, then that the new call wraps the old
/// one and nothing else changed (WRITE-9).
pub(crate) fn wrap_call(
    s: &mut Session,
    site: &Site,
    w: Wrap,
    check: impl FnOnce(&Session, &Checked, u32) -> Result<(), EditError>,
) -> Result<Edited, EditError> {
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let written = &text[site.start as usize..site.end as usize];
    let wrapped = format!("{}{written}{}", w.open, w.close);
    let imports = joined(w.imports);
    let shift: u32 = imports.iter().filter(|(at, _)| *at <= site.start).map(|(_, line)| line.len() as u32).sum();
    // Two insertions around the call (R5): an import at the call's own start goes before the
    // wrapping text, in the same edit.
    let mut edits: Vec<(u32, u32, String)> = vec![];
    let mut opening = w.open;
    let mut ranges = vec![Range::bytes(site.start, site.end)];
    for (at, line) in &imports {
        if *at == site.start {
            opening.insert_str(0, line);
        } else {
            edits.push((*at, *at, line.clone()));
            ranges.push(Range::bytes(*at, *at));
        }
    }
    edits.push((site.start, site.start, opening));
    edits.push((site.end, site.end, w.close));
    edits.sort_by_key(|e| e.0);
    let file = path(&site.file)?;
    let mut declared: BTreeMap<_, _> = [(file.clone(), Ranges::Ranges(ranges))].into();
    declared.extend(w.also.ranges);
    let expect = Expect::Confined(Confined { ranges: declared, ..Confined::default() });
    let label = (written.to_string(), wrapped.clone());
    let mut ops = vec![plan::text_edit(&file, &text, &edits)];
    ops.extend(w.also.ops);
    let tx = plan::planned(s, label.clone(), ops, expect);
    let what = w.label;
    let refused = || EditError::Rejected(format!("adding {what} here would change more than putting it after this sound"));
    let said = |why: &str| EditError::Rejected(format!("adding {what} here would change more than putting it after this sound ({why})"));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion (WRITE-9): the new call wraps the old one, and nothing else changed.
        let candidate = &checked.candidate().compiled;
        let new = candidate.sites.iter().find(|x| x.file == site.file && x.start == site.start + shift && x.callee == w.callee);
        let new = new.ok_or_else(refused)?.id;
        check(s, checked, new)?;
        match wraps(candidate, &checked.baseline().compiled.ir, new) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file.clone(), before: label.0, after: label.1 })
}

/// Take out effect call `call`, leaving what it was applied to.
pub fn remove_effect(s: &mut Session, at: u64, call: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let site = current_site(s, at, call)?.clone();
    let effect =
        effect_at(s, &site).ok_or_else(|| EditError::Rejected(format!("`{}` is not an effect that can be removed here", site.callee)))?;
    let refused = || EditError::Rejected(format!("removing {} would change more than taking it out", effect.label));
    let input = site.args.first().ok_or_else(refused)?.span();
    let text = s.accepted().snapshot.files[Path::new(&site.file)].clone();
    let (written, inner) = (text[site.start as usize..site.end as usize].to_string(), text[input.0 as usize..input.1 as usize].to_string());
    // Two deletions around the input, so its characters stay as they are.
    let edits = [(site.start, input.0, String::new()), (input.1, site.end, String::new())];
    let file = path(&site.file)?;
    let ranges = Ranges::Ranges(vec![Range::bytes(site.start, site.end)]);
    let expect = Expect::Confined(Confined { ranges: [(file.clone(), ranges)].into(), ..Confined::default() });
    let tx = plan::planned(s, (written.clone(), inner.clone()), vec![plan::text_edit(&file, &text, &edits)], expect);
    let said = |why: &str| EditError::Rejected(format!("removing {} would change more than taking it out ({why})", effect.label));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion: the baseline is the candidate wrapped in this call.
        match wraps(&checked.baseline().compiled, &checked.candidate().compiled.ir, site.id) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: written, after: inner })
}

/// Whether call `site` is turned off: `bypass: true` in its options when it ran (Next D1).
pub fn is_bypassed(compiled: &Compiled, site: u32) -> bool {
    compiled.ir.modules.iter().flat_map(|m| &m.frames).any(|f| f.site == site && f.component.is_some() && f.bypass)
}

/// Why calls of effect `d` can't be turned off at all, if they can't: the SDK's rule (Q25), its
/// first input and its output audio; and a component with a `bypass` of its own keeps it.
fn unbypassable(d: &Descriptor) -> Option<String> {
    if d.controls.iter().any(|c| c.name == "bypass") || d.settings.iter().any(|x| x.name == "bypass") {
        return Some("It has its own Bypass knob.".into());
    }
    let first = d.inputs.first().filter(|i| i.kind == PortKind::Audio && !i.many && !d.positional.contains(&i.name));
    match (first, &d.output) {
        (Some(_), Some(o)) if o.kind == PortKind::Audio => None,
        _ => Some(format!("{} can't be turned off: only an effect that takes audio and gives audio can.", d.label)),
    }
}

/// Where the property whose value is `value` starts: its key, before the `:`; the value itself
/// for a shorthand (`{ bypass }`).
fn prop_start(text: &str, value: (u32, u32), key: &str) -> Option<u32> {
    let before = text.get(..value.0 as usize)?.trim_end();
    let Some(head) = before.strip_suffix(':') else {
        return (text.get(value.0 as usize..value.1 as usize)? == key).then_some(value.0);
    };
    let head = head.trim_end();
    [key.to_string(), format!("\"{key}\""), format!("'{key}'")].iter().find_map(|k| head.strip_suffix(k.as_str())).map(|h| h.len() as u32)
}

/// The edit that turns call `site` of `text` off (`on`: `bypass: true` written into its options,
/// by [`Insertion::plan`]'s rule) or on again (the property taken out, with its comma, and the
/// options object when it was alone): one edit inside the call. Why not, in words.
fn bypass_edit(s: &Session, site: &Site, d: &Descriptor, text: &str, on: bool) -> Result<(u32, u32, String), String> {
    if let Some(why) = unbypassable(d) {
        return Err(why);
    }
    let what = if on { "off" } else { "on" };
    let in_code = |why: &str| format!("{} can't be turned {what} here: {why}. Turn it {what} in code.", d.label);
    let compiled = &s.accepted().compiled;
    if is_bypassed(compiled, site.id) == on {
        return Err(format!("{} is {what} already.", d.label));
    }
    let arg = |a: &Arg| text.get(a.span().0 as usize..a.span().1 as usize).unwrap_or("");
    if site.args.iter().any(|a| arg(a).trim_start().starts_with("...")) {
        return Err(in_code("its call spreads its arguments"));
    }
    let module = compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site.id));
    let module = module.ok_or_else(|| in_code("it doesn't run"))?;
    let options = options_of(module, site.id).map_err(in_code)?;
    let object = options.and_then(|i| match &site.args.get(i) {
        Some(Arg::Object { props, end, .. }) => Some((i, props, *end)),
        _ => None,
    });
    let written = object.and_then(|(_, props, _)| props.iter().position(|(k, _)| k == "bypass"));
    if on {
        // `bypass: false` written already: it says true now.
        if let (Some((_, props, _)), Some(k)) = (object, written) {
            let (a, b) = props[k].1.span();
            return Ok((a, b, "true".into()));
        }
        let insertion = Insertion::plan(site, d, "bypass", text, options, &compiled.units).map_err(in_code)?;
        return match insertion.edits("true").as_slice() {
            [(a, b, edit)] => Ok((*a, *b, edit.clone())),
            _ => Err(in_code("its options can't take it")),
        };
    }
    let ((i, props, end), k) = object.zip(written).ok_or_else(|| in_code("its call doesn't say `bypass: true` itself"))?;
    let value = props[k].1.span();
    let start = |k: usize| prop_start(text, props[k].1.span(), &props[k].0);
    let range = match (props.len(), k) {
        // Alone: the options object goes, from the argument before it.
        (1, _) => (site.args.get(i.wrapping_sub(1)).map(|a| a.span().1), Some(end)),
        // Last: from the property before it.
        (n, k) if k + 1 == n => (Some(props[k - 1].1.span().1), Some(value.1)),
        // Else up to the next property.
        _ => (start(k), start(k + 1)),
    };
    let (Some(a), Some(b)) = range else { return Err(in_code("its options are written in a way it can't read")) };
    let cut = text.get(a as usize..b as usize).unwrap_or("");
    if cut.contains("//") || cut.contains("/*") {
        return Err(in_code("a comment is written beside `bypass`"));
    }
    Ok((a, b, String::new()))
}

/// The graph of `c` without call `site`'s own nodes, when nothing reads them: a call turned off.
fn stripped(c: &Compiled, site: u32) -> Option<Ir> {
    let (module, ids) = made_by(c, site)?;
    let ids: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    excise(&c.ir, &module, &ids, &|_| false, "")
}

/// Why effect call `site` can't be turned off (`on`) or on here, in words; none when it can.
pub(crate) fn bypass_refused(s: &Session, site: &Site, on: bool) -> Option<String> {
    let d = effect_at(s, site)?;
    let text = s.accepted().snapshot.files.get(Path::new(&site.file))?;
    bypass_edit(s, site, &d, text, on).err()
}

/// Turn effect call `call` off (`on`: `bypass: true` written into its options) or on again (it
/// taken out). One `TextEdit` inside the call. The planner asserts that, with the effect's own
/// nodes taken out of the graph where it is off, that graph is exactly the one where it is on
/// with the effect removed (as [`remove_effect`] asserts), and that its frame says so.
///
/// Its expectation is the delta it was checked as (`Seen`, R26), as a rekey's is: the call
/// written around the effect reads the effect's input instead of its output while the effect's
/// nodes stay as they were, which no `Confined` range can say, and the assertion is exact.
pub fn bypass(s: &mut Session, at: u64, call: &str, on: bool) -> Result<Edited, EditError> {
    ready(s)?;
    let site = current_site(s, at, call)?.clone();
    let what = if on { "off" } else { "on" };
    let effect = effect_at(s, &site)
        .ok_or_else(|| EditError::Rejected(format!("`{}` is not an effect that can be turned {what} here", site.callee)))?;
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let edit = bypass_edit(s, &site, &effect, &text, on).map_err(EditError::Rejected)?;
    let file = path(&site.file)?;
    let label = shown_call(&site, &text, edit.0, edit.1, &edit.2);
    let op = plan::text_edit(&file, &text, std::slice::from_ref(&edit));
    let planned = |s: &Session, expect| plan::planned(s, label.clone(), vec![op.clone()], expect);
    let refused = || EditError::Rejected(format!("turning {} {what} would change more than that", effect.label));
    let said = |why: &str| EditError::Rejected(format!("turning {} {what} would change more than that ({why})", effect.label));
    let assert = |checked: &Checked| {
        let (baseline, candidate) = (&checked.baseline().compiled, &checked.candidate().compiled);
        let new = candidate.sites.iter().find(|x| x.file == site.file && x.start == site.start && x.callee == site.callee);
        let new = new.ok_or_else(refused)?.id;
        let fits = is_bypassed(candidate, new) == on
            && match on {
                true => stripped(candidate, new).is_some_and(|ir| wraps(baseline, &ir, site.id)),
                false => stripped(baseline, site.id).is_some_and(|ir| wraps(candidate, &ir, new)),
            };
        fits.then_some(()).ok_or_else(refused)
    };
    let probe = planned(s, Expect::Seen(m2_song::Sha::of(b"probe")));
    let checked = s.check(&probe).map_err(|r| crate::refused(s, r))?;
    unmoved(s, &checked)?;
    assert(&checked)?;
    let tx = planned(s, Expect::Seen(checked.hash));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        assert(checked)
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: label.0, after: label.1 })
}

/// An effect call and what it is.
type Effect = (Site, Arc<Descriptor>);

/// The effect calls written next to effect call `site` (Next D2): the one written as its first
/// argument (before it in the sound), and the one it is the first argument of (after it).
fn written_next(s: &Session, site: &Site) -> (Option<Effect>, Option<Effect>) {
    let sites = &s.accepted().compiled.sites;
    let effect = |x: &Site| Some((x.clone(), effect_at(s, x)?));
    let span = site.args.first().map(Arg::span);
    let inner = sites.iter().find(|x| x.file == site.file && Some((x.start, x.end)) == span).and_then(effect);
    let outer = sites.iter().find(|x| x.file == site.file && x.args.first().is_some_and(|a| a.span() == (site.start, site.end)));
    (inner, outer.and_then(effect))
}

/// The effects next to effect call `site` in the sound, however the code is written: the call
/// whose output it reads, and the call that reads its output, when each is an effect.
fn chained_next(s: &Session, site: u32) -> (Option<Arc<Descriptor>>, Option<Arc<Descriptor>>) {
    let compiled = &s.accepted().compiled;
    let Some((module, ids, output, inputs)) = reads_of(compiled, site) else { return (None, None) };
    let Some(m) = compiled.ir.modules.iter().find(|m| m.key == module) else { return (None, None) };
    let sites: BTreeMap<u32, &Site> = compiled.sites.iter().map(|x| (x.id, x)).collect();
    // The project's call a node is part of: its outermost one.
    let call = |n: &Node| {
        let site = calls_of(m, n).find(|f| sites.get(f).is_some_and(|x| x.in_project)).or(n.origin.as_ref().map(|o| o.site))?;
        effect_at(s, sites.get(&site)?)
    };
    let mut before = None;
    let mut after = None;
    walk(&m.nodes, &mut |n| {
        if ids.contains(&n.id) {
            return;
        }
        if inputs.iter().any(|r| split_ref(r).0 == n.id) && before.is_none() {
            before = call(n);
        }
        let mut node = vec![n.clone()];
        if let Op::Poly { body, .. } = &mut node[0].op {
            body.nodes.clear();
        }
        let reads = std::cell::Cell::new(false);
        refs(&mut node, &mut BTreeMap::new(), &|_| {}, &|r: &mut String| reads.set(reads.get() || *r == output));
        if reads.get() && after.is_none() {
            after = call(n);
        }
    });
    (before, after)
}

/// Why effect call `site` (`d`) can't swap with the effect before it (`earlier`) or after it
/// here, in words; and the effect it would swap with, when it can.
fn move_plan(s: &Session, site: &Site, d: &Descriptor, earlier: bool) -> Result<Effect, String> {
    let (inner, outer) = written_next(s, site);
    let written = if earlier { inner } else { outer };
    if let Some(other) = written {
        let (outer, inner) = if earlier { (site, &other.0) } else { (&other.0, site) };
        let text = |a: &Arg| {
            s.accepted()
                .snapshot
                .files
                .get(Path::new(&site.file))
                .and_then(|t| t.get(a.span().0 as usize..a.span().1 as usize).map(str::to_string))
        };
        let spread = outer.args.iter().chain(&inner.args).any(|a| text(a).is_some_and(|t| t.trim_start().starts_with("...")));
        if spread || inner.args.is_empty() || !(outer.start < outer.open && inner.start < inner.open) {
            let what = if spread { "one of them spreads its arguments" } else { "they aren't written as plain calls" };
            return Err(format!("{} and {} can't swap here: {what}. Change their order in code.", other.1.label, d.label));
        }
        return Ok(other);
    }
    let (before, after) = chained_next(s, site.id);
    match (earlier, before, after) {
        (true, Some(b), _) => Err(format!(
            "{} and {} aren't next to each other in the code, so they can't swap here. Change their order in code.",
            b.label, d.label
        )),
        (false, _, Some(a)) => Err(format!(
            "{} and {} aren't next to each other in the code, so they can't swap here. Change their order in code.",
            d.label, a.label
        )),
        (true, ..) => Err(format!("{} is the first effect here.", d.label)),
        (false, ..) => Err(format!("{} is the last effect here.", d.label)),
    }
}

/// What the ⋯ menu's Move left (`earlier`) or Move right row of effect call `site` says: the
/// effect it swaps with, or why it can't.
pub(crate) fn move_row(s: &Session, site: &Site, earlier: bool) -> Result<String, String> {
    let d = effect_at(s, site).ok_or_else(|| format!("`{}` is not an effect", site.callee))?;
    move_plan(s, site, &d, earlier).map(|(_, other)| other.label.clone())
}

/// Move effect call `call` before the effect it is written around (`earlier`), or after the one
/// written around it (Next D2): `outer(inner(x, I), O)` becomes `inner(outer(x, O), I)`, each
/// effect keeping its own text. Two edits: the callees' heads swapped, and their tails. Confined
/// to the outer call. The planner asserts that the two now read each other the other way round,
/// and that with both taken out the graph is the same as before. Effects not written one inside
/// the other are refused in words (Q26).
pub fn reorder(s: &mut Session, at: u64, call: &str, earlier: bool) -> Result<Edited, EditError> {
    ready(s)?;
    let site = current_site(s, at, call)?.clone();
    let effect =
        effect_at(s, &site).ok_or_else(|| EditError::Rejected(format!("`{}` is not an effect that can be moved here", site.callee)))?;
    let (other, d) = move_plan(s, &site, &effect, earlier).map_err(EditError::Rejected)?;
    let ((outer, od), (inner, id)) =
        if earlier { ((site.clone(), effect.clone()), (other, d)) } else { ((other, d), (site.clone(), effect.clone())) };
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let t = |a: u32, b: u32| text[a as usize..b as usize].to_string();
    let x = inner.args[0].span();
    // outer( gap inner( gap x itail otail  →  inner( gap outer( gap x otail itail
    let head = format!(
        "{}{}{}{}",
        t(inner.start, inner.open + 1),
        t(outer.open + 1, inner.start),
        t(outer.start, outer.open + 1),
        t(inner.open + 1, x.0)
    );
    let tail = format!("{}{}", t(inner.end, outer.end), t(x.1, inner.end));
    let edits = [(outer.start, x.0, head), (x.1, outer.end, tail)];
    // Where the outer call starts now: after the inner one's head and the gap.
    let shift = inner.open - outer.open;
    let file = path(&site.file)?;
    let ranges = Ranges::Ranges(vec![Range::bytes(outer.start, outer.end)]);
    let expect = Expect::Confined(Confined { ranges: [(file.clone(), ranges)].into(), ..Confined::default() });
    let label = (format!("{}({}(…))", outer.callee, inner.callee), format!("{}({}(…))", inner.callee, outer.callee));
    let tx = plan::planned(s, label.clone(), vec![plan::text_edit(&file, &text, &edits)], expect);
    let refused = || EditError::Rejected(format!("swapping {} and {} would change more than their order", id.label, od.label));
    let said =
        |why: &str| EditError::Rejected(format!("swapping {} and {} would change more than their order ({why})", id.label, od.label));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        let (b, c) = (&checked.baseline().compiled, &checked.candidate().compiled);
        let find = |start: u32, callee: &str| {
            c.sites.iter().find(|x| x.file == outer.file && x.start == start && x.callee == callee).map(|x| x.id)
        };
        let (Some(now_outer), Some(now_inner)) = (find(outer.start, &inner.callee), find(outer.start + shift, &outer.callee)) else {
            return Err(refused());
        };
        // The other way round: the outer call reads the inner one's output, in each (or the
        // inner one is turned off, and gives its input on).
        let reads = |c: &Compiled, reader: u32, read: u32| {
            is_bypassed(c, read) || reads_of(c, reader).zip(reads_of(c, read)).is_some_and(|(r, o)| r.3.contains(&o.2))
        };
        if !reads(b, outer.id, inner.id) || !reads(c, now_outer, now_inner) {
            return Err(refused());
        }
        // With both taken out, the same graph.
        let bare = |c: &Compiled, first: u32, then: u32| -> Vec<Ir> {
            unwrapped(c, &c.ir, first).iter().flat_map(|ir| unwrapped(c, ir, then)).map(|ir| canonical(&ir)).collect()
        };
        let was = bare(b, outer.id, inner.id);
        match bare(c, now_outer, now_inner).iter().any(|ir| was.contains(ir)) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: label.0, after: label.1 })
}

// ---------------------------------------------------------------- what a call listens to (CARD-35)

/// A call that listens: the component call `site` of the accepted revision is, when it is written
/// in the project, with its first sidechain input but its first (PKG-13), and that input's place
/// among the call's arguments. Nothing here is one effect's: `duck`'s side, `compress`'s and
/// `gate`'s keys and any pack's sidechain input are found the same way, from the descriptor.
struct Listener {
    d: Arc<Descriptor>,
    input: PortDesc,
    /// Its argument's index: the inputs `positional` doesn't name, then `positional` (PKG-1).
    at: usize,
}

fn listener(s: &Session, site: &Site) -> Option<Listener> {
    if !site.in_project {
        return None;
    }
    let compiled = &s.accepted().compiled;
    let mut ids = compiled.ir.modules.iter().flat_map(|m| &m.frames).filter(|f| f.site == site.id).map(|f| f.component.as_deref());
    let id = ids.next()??;
    if ids.any(|x| x != Some(id)) {
        return None;
    }
    let d = compiled.components.get(id)?.clone();
    let input = d.inputs.iter().skip(1).find(|i| i.sidechain && !i.many)?.clone();
    let lead = d.inputs.iter().map(|i| i.name.as_str()).filter(|n| !d.positional.iter().any(|p| p == n));
    let at = lead.chain(d.positional.iter().map(String::as_str)).position(|n| n == input.name)?;
    Some(Listener { d, input, at })
}

/// What sidechain input `input` of call `site` received, by its port ref, and the module the call
/// ran in; the ref is none when the call left it out (PKG-18).
fn heard<'a>(compiled: &'a Compiled, site: u32, input: &str) -> Option<(&'a Module, Option<String>)> {
    let m = compiled.ir.modules.iter().find(|m| m.frames.iter().any(|f| f.site == site && f.component.is_some()))?;
    let frame = m.frames.iter().find(|f| f.site == site && f.component.is_some())?;
    Some((m, frame.sidechain.iter().find(|x| x.input == input).map(|x| x.source.clone())))
}

/// The track port reference `r` of module `m` comes from, by key: an import of a track's export
/// (`kick.env`, `kick.out`), or a `follow()` of one (`follow(hats.out)`).
fn track_of(m: &Module, r: &str) -> Option<String> {
    let mut found = None;
    walk(&m.nodes, &mut |n| {
        if n.id == split_ref(r).0 {
            found = Some(n.clone());
        }
    });
    match found?.op {
        Op::Import { module, .. } => Some(module),
        Op::Kernel { kernel, inputs, .. } if kernel == "std:follow" => track_of(m, inputs.get("in")?),
        _ => None,
    }
}

/// Where a module's code is: the file its function is in (`src` is `file:line:column`).
fn file_of(m: &Module) -> Option<&str> {
    m.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2))
}

/// What a card's "Listens to" row shows (CARD-35): the track its input hears now, if it is one;
/// whether the call gives the input at all, and may leave it out; the tracks it can listen to (by
/// key and name), or why it can't be changed here.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Listens {
    /// The card's input row it is, when the card has one for it (the view fills it in): with none,
    /// the row stands alone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<String>,
    /// The input, by its name in the descriptor (`side`, `key`).
    pub input: String,
    /// The track whose sound the call works on, by name, when it is one: what its undo step names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
    /// The track it hears now, by key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track: Option<String>,
    /// Whether the call gives the input: when not, it listens to nothing (None).
    pub given: bool,
    /// Whether the input may be left out (PKG-18), so None is offered.
    pub optional: bool,
    /// Whether it hears a signal at all: a side written as an expression that gives `undefined`
    /// hears nothing, as one left out does.
    pub heard: bool,
    /// What its None is called: for a key left out, a compressor or a gate hears its own sound
    /// ("No other track"); a Duck's side hears nothing ("None").
    pub nothing: String,
    /// What it hears, as a port of the call's module: how the view finds its row.
    #[serde(skip)]
    pub source: Option<String>,
    pub choices: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

/// The tracks a call can listen to, by key, with the name its file imports each by; or why it
/// can't be changed here, in words.
type Choices = Result<Vec<(String, String)>, String>;

/// The tracks call `site` can listen to (or why it can't be changed here: it is in a track's file),
/// and the track whose sound it works on, by key, when it is one.
fn choices_of(s: &Session, site: &Site, l: &Listener) -> Option<(Choices, Option<String>)> {
    let compiled = &s.accepted().compiled;
    let (m, _) = heard(compiled, site.id, &l.input.name)?;
    let text = s.accepted().snapshot.files.get(Path::new(&site.file))?;
    // The track it works on: its file's when it is a track's, else the track whose export its first
    // argument names (`bass.out`), by the name the file imports it by.
    let of = match m.track {
        true => Some(m.key.clone()),
        false => site.args.first().and_then(|a| {
            let (handle, _) = text.get(a.span().0 as usize..a.span().1 as usize)?.split_once('.')?;
            compiled.ir.modules.iter().filter(|t| t.track).find_map(|t| {
                let import = crate::sites::imports_of(text, &site.file, file_of(t)?).into_iter().find_map(|i| i.default)?;
                (import == handle.trim()).then(|| t.key.clone())
            })
        }),
    };
    if m.track {
        let what = format!("{}'s {} is in {}, which can't hear other tracks. Change it in code.", m.label(), l.d.label, site.file);
        return Some((Err(what), of));
    }
    let choices = compiled
        .ir
        .modules
        .iter()
        .filter(|t| t.track && Some(&t.key) != of.as_ref())
        .filter_map(|t| {
            let import = crate::sites::imports_of(text, &site.file, file_of(t)?).into_iter().find_map(|i| i.default)?;
            Some((t.key.clone(), import))
        })
        .collect();
    Some((Ok(choices), of))
}

/// Whether call `site` gives its argument at `at`: written there, and not where its options are.
fn given_at(m: &Module, site: &Site, at: usize, text: &str) -> bool {
    let options = options_of(m, site.id).ok().flatten();
    site.args.get(at).is_some_and(|a| options != Some(at) && text.get(a.span().0 as usize..a.span().1 as usize) != Some("undefined"))
}

/// What call `site`'s "Listens to" row shows; none when it has no input that listens.
pub(crate) fn listens(s: &Session, site: &Site) -> Option<Listens> {
    let l = listener(s, site)?;
    let compiled = &s.accepted().compiled;
    let (m, source) = heard(compiled, site.id, &l.input.name)?;
    let text = s.accepted().snapshot.files.get(Path::new(&site.file))?;
    let track = source.as_deref().and_then(|r| track_of(m, r));
    let label = |key: &str| compiled.ir.modules.iter().find(|t| t.key == key).map_or(key.to_string(), |t| t.label().to_string());
    let (choices, of) = choices_of(s, site, &l)?;
    // With a key left out, an effect that listens to audio hears its own (std's `compress`, a
    // pack's gate): it listens to no other track. A side left out (a Duck's) hears nothing.
    let nothing = if l.input.kind == PortKind::Audio { "No other track" } else { "None" };
    let base = Listens {
        input: l.input.name.clone(),
        of: of.as_deref().map(label),
        track,
        given: given_at(m, site, l.at, text),
        optional: l.input.optional,
        heard: source.is_some(),
        nothing: nothing.into(),
        source,
        ..Listens::default()
    };
    Some(match choices {
        Ok(choices) => {
            // By name, as a list a person reads.
            let mut choices: Vec<(String, String)> = choices.into_iter().map(|(k, _)| (k.clone(), label(&k))).collect();
            choices.sort_by(|a, b| (a.1.to_lowercase(), &a.0).cmp(&(b.1.to_lowercase(), &b.0)));
            Listens { choices, ..base }
        }
        Err(why) => Listens { refused: Some(why), ..base },
    })
}

/// Make call `call` listen to track `track` (CARD-35), or to nothing (`None`, an input a call may
/// leave out). Its argument for the input that listens becomes, by the input's kind, `<handle>.out`
/// for audio; for a control `<handle>.env` when the track exports `env`, else `follow(<handle>.out)`
/// (importing `follow` from `std/dsp` when the file doesn't), where `<handle>` is the name its file
/// imports the track by. Left out, it is written after the argument before it; None takes it out.
/// Refused in words when the call is in a track's file, its file doesn't import the track, or its
/// arguments don't put the input where its order says. One `TextEdit`, confined to the argument
/// and the import line; the planner asserts that the input now hears that track, or nothing.
pub fn sidechain(s: &mut Session, at: u64, call: &str, track: Option<&str>) -> Result<Edited, EditError> {
    ready(s)?;
    let site = current_site(s, at, call)?.clone();
    let l = listener(s, &site).ok_or_else(|| EditError::Rejected(format!("`{}` listens to no other sound", site.callee)))?;
    let (choices, _) = choices_of(s, &site, &l).ok_or_else(|| EditError::NotFound(call.to_string()))?;
    let choices = choices.map_err(EditError::Rejected)?;
    let what = &l.d.label;
    let in_code = |why: &str| EditError::Rejected(format!("{what} can't change what it listens to here: {why}. Change it in code."));
    let compiled = &s.accepted().compiled;
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let (m, _) = heard(compiled, site.id, &l.input.name).ok_or_else(|| EditError::NotFound(call.to_string()))?;
    let given = given_at(m, &site, l.at, &text);
    let named = |key: &str| compiled.ir.modules.iter().find(|t| t.key == key).map_or(key.to_string(), |t| t.label().to_string());
    let arg = |i: usize| site.args.get(i).map(|a| a.span());
    // What its argument is to be, and the import it needs.
    let new = match track {
        Some(track) => {
            let handle = choices.iter().find(|(k, _)| k == track).map(|(_, h)| h.clone()).ok_or_else(|| {
                EditError::Rejected(format!(
                    "{what} can't listen to {} here: {} doesn't import it. Change it in code.",
                    named(track),
                    site.file
                ))
            })?;
            let env = compiled.ir.modules.iter().any(|t| t.key == track && t.exports.contains_key("env"));
            Some(match (l.input.kind, env) {
                (PortKind::Control, true) => (format!("{handle}.env"), None),
                (PortKind::Control, false) => (format!("follow({handle}.out)"), import_insertion(&text, "std/dsp", "follow")),
                _ => (format!("{handle}.out"), None),
            })
        }
        None if !l.input.optional => return Err(in_code(&format!("its {} must listen to something", l.input.name))),
        None => None,
    };
    // The text it replaces: the argument; or, taking it out, it and its own comments with the comma
    // between it and the one before; or, putting one in, nothing, before its options or after the
    // argument before it. Each other argument keeps its text and its comments.
    let span = |a: (u32, u32)| text[a.0 as usize..a.1 as usize].to_string();
    let options = options_of(m, site.id).ok().flatten();
    let (from, to, written, label) = match (&new, given) {
        (Some((n, _)), true) => {
            let (a, b) = arg(l.at).ok_or_else(|| in_code("its input isn't written in its call"))?;
            if text[a as usize..b as usize] == *n {
                return Err(EditError::Rejected(format!("{what} listens to {} already.", named(track.unwrap_or_default()))));
            }
            (a, b, n.clone(), (span((a, b)), n.clone()))
        }
        (Some((n, _)), false) => {
            let before = l.at.checked_sub(1).and_then(arg).ok_or_else(|| in_code("its arguments before it aren't written"))?;
            // Whatever is at its place must be its options, which then come after it, or
            // `undefined`, which it replaces.
            match (arg(l.at), options) {
                (None, _) => (before.1, before.1, format!(", {n}"), (String::new(), n.clone())),
                (Some((a, _)), Some(o)) if o == l.at => (a, a, format!("{n}, "), (String::new(), n.clone())),
                (Some((a, b)), _) if &text[a as usize..b as usize] == "undefined" => (a, b, n.clone(), (span((a, b)), n.clone())),
                _ => return Err(in_code("its arguments don't put it where its order says")),
            }
        }
        (None, false) => return Err(EditError::Rejected(format!("{what} listens to {} already.", nothing_of(&l)))),
        (None, true) => {
            if site.args.len() > l.at + 1 && options != Some(l.at + 1) {
                return Err(in_code("an argument after it would move into its place"));
            }
            let before = arg(l.at - 1).ok_or_else(|| in_code("its arguments before it aren't written"))?;
            let this = arg(l.at).ok_or_else(|| in_code("its input isn't written in its call"))?;
            let (from, to) = taken_out(&text, before.1, this.1, site.args.len() > l.at + 1)
                .ok_or_else(|| in_code("its commas aren't where they should be"))?;
            (from, to, String::new(), (span(this), String::new()))
        }
    };
    let mut edits = vec![(from, to, written)];
    if let Some((at, line)) = new.as_ref().and_then(|(_, i)| i.clone()) {
        edits.push((at, at, line));
    }
    text_tx(s, &site, &text, &edits, track, label)
}

/// What a call that listens to nothing listens to, as a sentence says it: "nothing" for a side
/// (a Duck's), "no other track" for a key (it hears its own sound).
fn nothing_of(l: &Listener) -> &'static str {
    if l.input.kind == PortKind::Audio { "no other track" } else { "nothing" }
}

/// Where trivia (white space and comments) after byte `i` of `text` ends.
fn past_trivia(text: &str, mut i: usize) -> usize {
    loop {
        let rest = &text[i..];
        let t = rest.trim_start();
        i += rest.len() - t.len();
        if t.starts_with("/*") {
            i += t.find("*/").map_or(t.len(), |e| e + 2);
        } else if t.starts_with("//") {
            i += t.find('\n').unwrap_or(t.len());
        } else {
            return i;
        }
    }
}

/// Byte `i` of `text`, or where its next line starts when all that follows it on its line is
/// spaces and a line comment: a comma that ends its line takes that line's end with it.
fn line_end(text: &str, i: usize) -> usize {
    let rest = &text[i..];
    let t = rest.trim_start_matches([' ', '\t']);
    let t = if t.starts_with("//") { &t[t.find('\n').unwrap_or(t.len())..] } else { t };
    match t.strip_prefix("\r\n").or_else(|| t.strip_prefix('\n')) {
        Some(next) => text.len() - next.len(),
        None => i,
    }
}

/// The bytes that take an argument out of a call (CARD-35), the argument before it ending at
/// `before` and it at `end`. When an argument follows (`more`): from after the comma that ends the
/// one before (its line's end too, when the comma ends its line) through the comma that ends it
/// (likewise), so each keeps its own comments. Else from that comma through its own comments.
/// None when a comma isn't where it should be.
fn taken_out(text: &str, before: u32, end: u32, more: bool) -> Option<(u32, u32)> {
    let first = past_trivia(text, before as usize);
    (text.as_bytes().get(first) == Some(&b',')).then_some(())?;
    if !more {
        return Some((first as u32, past_trivia(text, end as usize) as u32));
    }
    let second = past_trivia(text, end as usize);
    (text.as_bytes().get(second) == Some(&b',')).then_some(())?;
    Some((line_end(text, first + 1) as u32, line_end(text, second + 1) as u32))
}

/// Run call `site`'s Listens to edit, `edits` of `text` (each `(from, to, text)`), as one
/// `TextEdit` Tx confined to the call and the import line; the planner asserts that the call's input now hears `track`,
/// or nothing. What it says: the argument before and after (empty when there is none).
fn text_tx(
    s: &mut Session,
    site: &Site,
    text: &str,
    edits: &[(u32, u32, String)],
    track: Option<&str>,
    label: (String, String),
) -> Result<Edited, EditError> {
    let l = listener(s, site).ok_or_else(|| EditError::NotFound(site.file.clone()))?;
    let what = l.d.label.clone();
    let mut edits = edits.to_vec();
    edits.sort_by_key(|e| e.0);
    // Confined to the call (what it makes changes with what it hears: a key's wire, the side a
    // Duck holds at 0 with none) and the import line.
    let mut ranges = vec![Range::bytes(site.start, site.end)];
    ranges.extend(edits.iter().filter(|(a, _, _)| *a < site.start || *a > site.end).map(|(a, b, _)| Range::bytes(*a, *b)));
    // Where the call starts now: after any import put in above it.
    let shift: i64 = edits.iter().filter(|(a, b, _)| *b <= site.start && a == b).map(|(_, _, line)| line.len() as i64).sum::<i64>();
    let file = path(&site.file)?;
    let expect = Expect::Confined(Confined { ranges: [(file.clone(), Ranges::Ranges(ranges))].into(), ..Confined::default() });
    let tx = plan::planned(s, label.clone(), vec![plan::text_edit(&file, text, &edits)], expect);
    let named =
        track.map(|t| s.accepted().compiled.ir.modules.iter().find(|m| m.key == t).map_or(t.to_string(), |m| m.label().to_string()));
    let to = named.unwrap_or_else(|| "nothing".into());
    let refused = || EditError::Rejected(format!("making {what} listen to {to} would change more than that"));
    let said = |why: &str| EditError::Rejected(format!("making {what} listen to {to} would change more than that ({why})"));
    let (file_name, start, callee, input) = (site.file.clone(), site.start as i64 + shift, site.callee.clone(), l.input.name.clone());
    let at = l.at;
    // The call's other arguments, as written: the Tx is confined to the whole call, so the planner
    // checks them itself (CARD-35).
    let others = |text: &str, x: &Site, m: &Module| -> Vec<String> {
        let options = options_of(m, x.id).ok().flatten();
        let skip = (x.args.len() > at && options != Some(at)).then_some(at);
        let args = x.args.iter().enumerate().filter(|(i, _)| Some(*i) != skip);
        args.map(|(_, a)| text[a.span().0 as usize..a.span().1 as usize].to_string()).collect()
    };
    let was = {
        let compiled = &s.accepted().compiled;
        let m = heard(compiled, site.id, &input).ok_or_else(|| EditError::NotFound(site.file.clone()))?.0;
        others(text, site, m)
    };
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        let c = &checked.candidate().compiled;
        let now = c.sites.iter().find(|x| x.file == file_name && x.start as i64 == start && x.callee == callee).ok_or_else(refused)?;
        let (m, source) = heard(c, now.id, &input).ok_or_else(refused)?;
        let written = checked.candidate().snapshot.files.get(Path::new(&file_name)).ok_or_else(refused)?;
        if others(written, now, m) != was {
            return Err(refused());
        }
        match source.as_deref().and_then(|r| track_of(m, r)).as_deref() == track && (track.is_some() == source.is_some()) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file.clone(), before: label.0, after: label.1 })
}

#[cfg(test)]
#[path = "effect_tests.rs"]
mod tests;

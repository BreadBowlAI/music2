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
//! spelled: `echo(x)` after `import { delay as echo }` is `std:delay`.
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
use m2_script::component::{PortKind, Spelled};
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
    /// The ComponentId (`std:delay`): what adding it names.
    pub component: String,
    /// What it is called (`Delay`).
    pub label: String,
    /// Where it is imported from and as what: `delay` from `std/fx`.
    pub name: String,
    pub module: String,
    pub description: String,
    /// A free string the browser groups by (PKG-7).
    pub category: String,
    /// What adding it writes after the audio, from its descriptor's insert: `, 1000`, or
    /// `, { time: 0.25, feedback: 0.3, mix: 0.3 }`; empty when it inserts nothing.
    pub insert: String,
}

/// The component frames of `m` whose chain holds `n`, outermost first: the sites of the
/// component calls it was made inside (ID-15). `key()` frames aren't calls of a component.
pub(crate) fn calls_of<'a>(m: &'a Module, n: &Node) -> impl Iterator<Item = u32> + 'a {
    m.chain(n.frame).into_iter().filter(|f| f.key.is_none()).map(|f| f.site)
}

/// The nodes call `site` made, and the module they are in.
fn made_by(compiled: &Compiled, site: u32) -> Option<(String, BTreeSet<String>)> {
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

/// Whether `with` is `without` plus effect call `site`: removing the call's nodes and
/// reading what they read in place of their output gives exactly `without`.
fn wraps(with: &Compiled, without: &Ir, site: u32) -> bool {
    let mut outputs = with.outputs.iter().filter(|o| o.site == site);
    let (Some(output), None) = (outputs.next(), outputs.next()) else { return false };
    let Some((module, ids)) = made_by(with, site) else { return false };
    let ids: BTreeSet<&str> = ids.iter().map(String::as_str).collect();
    if !ids.contains(split_ref(&output.port).0) {
        return false;
    }
    // What the effect reads from outside itself: one of these is its input. Its nodes may be
    // in a voice (an effect inside `poly`), so every node list is searched, voices included.
    let inputs = std::cell::RefCell::new(BTreeSet::new());
    let m = with.ir.modules.iter().find(|m| m.key == module).expect("the output's module");
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
    let inputs = inputs.into_inner();
    let without = canonical(without);
    inputs.iter().any(|input| excise(&with.ir, &module, &ids, &|r| r == output.port, input).is_some_and(|ir| canonical(&ir) == without))
}

/// How an effect's insert value is written (D11): a number in a spelling of scale 1 (all of
/// std's) as itself, so the call reads as the descriptor wrote it; in another spelling with its
/// constructor (`ms(45)`), never a base value with float noise. The constructors the text needs
/// imported come back with it.
fn insert_text(v: &Spelled, units: &[m2_script::UnitCtor]) -> (String, Option<String>) {
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
/// where to import it from; in the order their packages declare them (by file, then line).
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
            };
            Some((&**d, info))
        })
        .collect();
    // `at` is `file:line:col`.
    let line = |d: &Descriptor| d.at.rsplit(':').nth(1).and_then(|l| l.parse::<u32>().ok()).unwrap_or(0);
    found.sort_by(|(a, _), (b, _)| (&a.file, line(a)).cmp(&(&b.file, line(b))));
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

/// Put effect `component` (a ComponentId, `std:delay`) after call `call` (`file#ordinal` at
/// revision `at`).
pub fn add_effect(s: &mut Session, at: u64, call: &str, component: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let effect = effects(s).into_iter().find(|e| e.component == component).ok_or_else(|| EditError::NotFound(component.to_string()))?;
    let desc = s.catalog().components[component].clone();
    let site = current_site(s, at, call)?.clone();
    if !site.in_project {
        return Err(EditError::NotFound(call.to_string()));
    }
    let text = s.accepted().snapshot.files.get(Path::new(&site.file)).ok_or_else(|| EditError::NotFound(site.file.clone()))?.clone();
    let written = &text[site.start as usize..site.end as usize];
    let (args, ctors) = insert_args(&desc, &s.accepted().compiled.units);
    let wrapped = format!("{}({written}{args})", effect.name);
    let mut imports = vec![];
    if let Some(i) = import_insertion(&text, &effect.module, &effect.name) {
        imports.push(i);
    }
    for ctor in &ctors {
        if !imported_names(&text, "@m2/core").contains(ctor) && !imports.iter().any(|(_, line)| line.contains(ctor.as_str())) {
            imports.extend(import_insertion(&text, "@m2/core", ctor));
        }
    }
    let imports = joined(imports);
    let shift: u32 = imports.iter().filter(|(at, _)| *at <= site.start).map(|(_, line)| line.len() as u32).sum();
    // Two insertions around the call (R5): an import at the call's own start goes before the
    // effect's name, in the same edit.
    let mut edits: Vec<(u32, u32, String)> = vec![];
    let mut opening = format!("{}(", effect.name);
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
    edits.push((site.end, site.end, format!("{args})")));
    edits.sort_by_key(|e| e.0);
    let file = path(&site.file)?;
    let expect = Expect::Confined(Confined { ranges: [(file.clone(), Ranges::Ranges(ranges))].into(), ..Confined::default() });
    let label = (written.to_string(), wrapped.clone());
    let tx = plan::planned(s, label.clone(), vec![plan::text_edit(&file, &text, &edits)], expect);
    let refused = || EditError::Rejected(format!("adding {} here would change more than putting it after this sound", effect.label));
    let said =
        |why: &str| EditError::Rejected(format!("adding {} here would change more than putting it after this sound ({why})", effect.label));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion (WRITE-9): the new call wraps the old one, and nothing else changed.
        let candidate = &checked.candidate().compiled;
        let new = candidate.sites.iter().find(|x| x.file == site.file && x.start == site.start + shift && x.callee == effect.name);
        let new = new.ok_or_else(refused)?.id;
        match wraps(candidate, &checked.baseline().compiled.ir, new) {
            true => Ok(()),
            false => Err(refused()),
        }
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: label.0, after: label.1 })
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

#[cfg(test)]
#[path = "effect_tests.rs"]
mod tests;

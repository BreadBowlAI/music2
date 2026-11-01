//! Change a track's instrument (polish A3, Q5): the one instrument call in a track's code becomes a
//! call of another, reading the same notes, at the new instrument's own settings, as + Track's new
//! track does. One `TextEdit` Tx of the file the call is in, so one undo step. The track keeps its
//! name, colour, lane, notes, mixer channel, effects and mute: only the call and the file's imports
//! change.
//!
//! - The call: `bass(lane.notes("notes"), { cutoff: hz(420) })` → `synth.pad(lane.notes("notes"))`.
//!   Its first argument's bytes stay as written, so the lane it reads keeps its name in the graph.
//! - Its settings go. One that holds anything but a plain value (a lane, an LFO) is refused: the
//!   change would lose what the code does there.
//! - The imports: the new instrument's binding, joining an import from its module when there is
//!   one; and every name only the dropped text read is taken out (`bass`, then `hz`).
//!
//! Refused, in words: a track with no instrument call of its own (a sound built from parts), one
//! with two, and one that plays a plugin.

use std::path::Path;

use m2_ir::{Module, Node, Op};
use m2_script::component::PortKind;
use m2_script::sites::{Arg, Site};
use m2_session::{Revision, Session};
use m2_song::tx::{Confined, Declared, Expect, Range};

use crate::plan::{self, path, unmoved};
use crate::sites::{bound_names, import_insertion, imported_names, specifier, statements, unused_imports};
use crate::tracks::{self, Instrument, instrument_binding};
use crate::{EditError, Edited, ready};

/// Where a module's code is: the file its function is in (`src` is `file:line:column`).
pub(crate) fn file_of(m: &Module) -> Option<&str> {
    m.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2))
}

/// Every node of `nodes`, voices included.
fn each<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        if let Op::Poly { body, .. } = &n.op {
            each(&body.nodes, out);
        }
    }
}

/// What plays a track's notes: its instrument calls (the outermost frames of a component that
/// takes notes first and makes audio, listed or not: @m2/synths' simple `hat` has no
/// category) and the plugins it gives notes to.
pub(crate) struct Players<'a> {
    pub calls: Vec<&'a Site>,
    pub plugins: usize,
}

pub(crate) fn players<'a>(rev: &'a Revision, m: &Module) -> Players<'a> {
    let compiled = &rev.compiled;
    let instrument = |id: Option<&str>| {
        id.and_then(|id| compiled.components.get(id)).is_some_and(|d| {
            d.inputs.first().is_some_and(|i| i.kind == PortKind::Notes) && d.output.as_ref().is_some_and(|o| o.kind == PortKind::Audio)
        })
    };
    let calls = m
        .frames
        .iter()
        .enumerate()
        .filter(|(i, f)| {
            // Outermost: an instrument made inside another is that one's.
            instrument(f.component.as_deref()) && !m.chain(Some(*i as u32)).iter().rev().skip(1).any(|p| instrument(p.component.as_deref()))
        })
        .filter_map(|(_, f)| compiled.sites.iter().find(|s| s.id == f.site))
        .collect();
    let mut nodes = vec![];
    each(&m.nodes, &mut nodes);
    let plugins = nodes.iter().filter(|n| matches!(&n.op, Op::Plugin { inputs, .. } if inputs.contains_key("notes"))).count();
    Players { calls, plugins }
}

/// The track's one instrument call, or why its instrument can't be changed from here.
fn the_call<'a>(rev: &'a Revision, m: &Module) -> Result<&'a Site, EditError> {
    let label = m.label();
    let p = players(rev, m);
    let refused = |why: String| Err(EditError::Rejected(why));
    match (p.calls.as_slice(), p.plugins) {
        ([], 0) => refused(format!("{label} is built from parts, not one instrument. Change it in code.")),
        ([], 1) => refused(format!("{label} plays a plugin. Change it in code.")),
        ([call], 0) if call.in_project && Some(call.file.as_str()) == file_of(m) => Ok(call),
        ([_], 0) => refused(format!("{label}'s instrument is made in other code. Change it in code.")),
        (calls, plugins) => refused(format!("{label} plays {} instruments. Change them in code.", calls.len() + plugins)),
    }
}

/// Whether `text` is a plain value a setting may hold and the change may drop: a number, a unit
/// of one (`hz(420)`, `ms(40)`), a string or a boolean.
fn plain(text: &str, units: &[m2_script::UnitCtor]) -> bool {
    let t = text.trim();
    let number = |t: &str| t.trim().parse::<f64>().is_ok_and(f64::is_finite);
    if number(t) || matches!(t, "true" | "false") {
        return true;
    }
    if t.len() >= 2 && (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\'')) {
        return !t[1..t.len() - 1].contains(['"', '\'', '\\', '`']);
    }
    match t.split_once('(') {
        Some((name, rest)) => units.iter().any(|u| u.name == name.trim()) && rest.strip_suffix(')').is_some_and(number),
        None => false,
    }
}

/// The first setting of `arg` (an argument after the notes) that holds more than a plain value:
/// its name, `None` when every one is plain.
fn computed(text: &str, arg: &Arg, name: &str, units: &[m2_script::UnitCtor]) -> Option<String> {
    match arg {
        Arg::Number { .. } => None,
        Arg::Object { props, .. } => props.iter().find_map(|(k, a)| computed(text, a, k, units)),
        Arg::Name { .. } => Some(name.to_string()),
        Arg::Other { start, end } => (!plain(&text[*start as usize..*end as usize], units)).then(|| name.to_string()),
    }
}

/// The edits of `file`'s import statements that import each binding of `adds` from its module, and
/// take out every name only bytes `gone` read: as one edit of the imports' bytes (trimmed to what
/// differs), so no node's name moves. None when they don't change.
pub(crate) fn import_edits(text: &str, adds: &[(&str, &str)], gone: &[(u32, u32)]) -> Option<(u32, u32, String)> {
    let imports: Vec<(u32, u32)> = statements(text).into_iter().filter(|s| s.2).map(|s| (s.0, s.1)).collect();
    // The imports' bytes, through the end of the last one's line; none: the top of the file.
    let (start, end) = match (imports.first(), imports.last()) {
        (Some(first), Some(last)) => (first.0 as usize, text[last.1 as usize..].find('\n').map_or(text.len(), |i| last.1 as usize + i + 1)),
        _ => (0, 0),
    };
    // Out first; then the import goes in where the file's imports are now.
    let mut out: Vec<(u32, u32)> = unused_imports(text, gone).into_iter().filter(|(a, _)| (*a as usize) < end).collect();
    out.sort();
    let mut after = String::new();
    let mut at = 0;
    for (a, b) in &out {
        after.push_str(&text[at..*a as usize]);
        at = *b as usize;
    }
    after.push_str(&text[at..]);
    for (from, binding) in adds {
        if let Some((i, line)) = import_insertion(&after, from, binding) {
            after.insert_str(i as usize, &line);
        }
    }
    let new = &after[start..after.len() - (text.len() - end)];
    let old = &text[start..end];
    if new == old {
        return None;
    }
    // Only what differs.
    let pre = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let pre = (0..=pre).rev().find(|i| old.is_char_boundary(*i) && new.is_char_boundary(*i)).unwrap_or(0);
    let max = old.len().min(new.len()) - pre;
    let post = old.bytes().rev().zip(new.bytes().rev()).take(max).take_while(|(a, b)| a == b).count();
    let post = (0..=post).rev().find(|i| old.is_char_boundary(old.len() - i) && new.is_char_boundary(new.len() - i)).unwrap_or(0);
    Some(((start + pre) as u32, (end - post) as u32, new[pre..new.len() - post].to_string()))
}

/// Change track `module`'s instrument to `component` (a ComponentId the + Track list offers): its
/// one instrument call becomes `<name>(<its notes>)`. One undo step, labelled "Bass plays Pluck".
pub fn swap(s: &mut Session, module: &str, component: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let inst: Instrument =
        tracks::instruments(s).into_iter().find(|i| i.component == component).ok_or_else(|| EditError::NotFound(component.to_string()))?;
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module && m.track).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    let label = m.label().to_string();
    let call = the_call(rev, m)?;
    let frame = m.frames.iter().find(|f| f.site == call.id).and_then(|f| f.component.clone());
    if frame.as_deref() == Some(component) {
        return Err(EditError::Rejected(format!("{label} plays {} already.", inst.label)));
    }
    let text = rev.snapshot.files.get(Path::new(&call.file)).cloned().ok_or_else(|| EditError::NotFound(call.file.clone()))?;
    let notes = call.args.first().ok_or_else(|| EditError::Rejected(format!("{label}'s instrument reads no notes. Change it in code.")))?;
    // Its settings go: each must be a plain value, or the code would lose what it does.
    let positional = frame.as_deref().and_then(|id| rev.compiled.components.get(id)).map(|d| d.positional.clone()).unwrap_or_default();
    for (i, arg) in call.args.iter().enumerate().skip(1) {
        let name = positional.get(i - 1).cloned().unwrap_or_else(|| "setting".into());
        if let Some(what) = computed(&text, arg, &name, &rev.compiled.units) {
            return Err(EditError::Rejected(format!("{label}'s {what} is set by code here. Change its instrument in the Code view.")));
        }
    }
    // What the file imports it from: a project's own instrument by its path from the file.
    let binding = instrument_binding(&inst).to_string();
    let from = match inst.module.ends_with(".ts") {
        true => specifier(&call.file, &inst.module),
        false => inst.module.clone(),
    };
    if !imported_names(&text, &from).contains(&binding) && bound_names(&text).contains(&binding) {
        return Err(EditError::Rejected(format!("{} already names something else, so change {label}'s instrument in code.", call.file)));
    }

    // The call around its notes: `bass(` → `synth.pad(`, and `, { … })` → `)`.
    let (n0, n1) = notes.span();
    let mut edits = vec![(call.start, n0, format!("{}(", inst.name)), (n1, call.end, ")".to_string())];
    // The names the dropped bytes read: the callee's binding stays when the new one is the same.
    let old_binding = call.callee.split('.').next().unwrap_or(&call.callee);
    let callee_kept = if old_binding == binding { old_binding.len() as u32 } else { 0 };
    let gone = [(call.start + callee_kept, n0), (n1, call.end)];
    let mut ranges = vec![Range::bytes(call.start, call.end)];
    if let Some((a, b, t)) = import_edits(&text, &[(&from, &binding)], &gone) {
        if b > call.start {
            return Err(EditError::Rejected(format!("{}'s imports aren't at its top, so change {label}'s instrument in code.", call.file)));
        }
        ranges.push(Range::bytes(a, b));
        edits.insert(0, (a, b, t));
    }
    let shown = call.file.clone();
    let file = path(&shown)?;
    let mut ops = vec![plan::text_edit(&file, &text, &edits)];
    let mut declared: std::collections::BTreeMap<_, _> = [(file, Declared::Ranges(ranges))].into();
    // A pack's instrument brings its `[dependencies]` line and lock entry (PACK-10).
    let also = crate::packs::needs(s, component)?;
    ops.extend(also.ops);
    declared.extend(also.ranges);
    let expect = Expect::Confined(Confined { ranges: declared, lanes: vec![], modules: vec![] });
    let after = format!("{label} plays {}", inst.label);
    let tx = plan::planned(s, (label.clone(), after.clone()), ops, expect);
    let lanes = m.lanes.clone();
    let said = |why: &str| EditError::Rejected(format!("changing {label}'s instrument would change more than its instrument ({why})"));
    let key = module.to_string();
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion: only the track's own nodes change, it reads the same lanes, and
        // no track comes or goes.
        let d = &checked.delta;
        let mine = |name: &Vec<String>| name.first() == Some(&key);
        let others = d
            .added
            .iter()
            .chain(&d.removed)
            .map(|n| &n.name)
            .chain(d.rewired.iter().map(|w| &w.node))
            .chain(d.values.iter().map(|v| &v.node));
        if let Some(n) = others.into_iter().find(|n| !mine(n)) {
            return Err(said(&format!("{} changes", n.join("/"))));
        }
        let now = checked.candidate().compiled.ir.modules.iter().find(|x| x.key == key).map(|x| &x.lanes);
        match d.modules.is_empty() && d.lanes.is_empty() && now == Some(&lanes) {
            true => Ok(()),
            false => Err(said("its lanes or the tracks change")),
        }
    })?;
    Ok(Edited { revision: done.number, file: shown, before: label, after })
}

#[cfg(test)]
#[path = "swap_tests.rs"]
mod tests;

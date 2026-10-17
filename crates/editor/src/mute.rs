//! Mute in song (MON-5): `muted: true` in a `track()` call's options, saved in code, so every
//! render leaves the track out, export included. Toggling it is one `Confined` Tx on the options of
//! each track it names, declaring each module's `muted` change (WRITE-10); the engine takes it as
//! overlay cell writes, with no new plan. And what a track drives besides its sound (MON-2), which
//! a mute never silences: [`feeds`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use m2_ir::{FOp, Input, Kind, Module, split_ref};
use m2_script::sites::Arg;
use m2_session::{Revision, Session};
use m2_song::ROOT;
use m2_song::tx::{Confined, Declared, Expect, ModuleChange, Range};
use serde::Serialize;

use crate::plan::{self, path};
use crate::rename::track_call;
use crate::sites::properties;
use crate::{EditError, Edited, ready};

/// Labels as words: "Kick", "Kick and Bass", "Kick, Bass and Hats".
fn listed(labels: &[String]) -> String {
    match labels {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The edit that sets `muted` in the options of the `track()` call `site` of `m`, in `text`: none
/// when it says so already.
fn option_edit(text: &str, site: &m2_script::sites::Site, m: &Module, muted: bool) -> Result<Option<(u32, u32, String)>, EditError> {
    let in_code = || {
        EditError::Rejected(format!("{}'s track() call isn't written with its key and options in the code; mute it there", m.label()))
    };
    let key = site.args[0].span();
    Ok(match &site.args[..] {
        // `track("kick", fn)`: options after the key, so the call's first and last characters stay
        // and every node keeps its name.
        [_, _] if muted => Some((key.1, key.1, ", { muted: true }".into())),
        [_, _] => None,
        [_, Arg::Object { start, end, .. }, _] => {
            let props = properties(text, (*start, *end)).ok_or_else(in_code)?;
            let at = props.iter().position(|p| p.key == "muted");
            let written = |i: usize| &text[props[i].value.0 as usize..props[i].value.1 as usize];
            match (at, muted) {
                (Some(i), true) if written(i) == "true" => None,
                (Some(i), true) => Some((props[i].value.0, props[i].value.1, "true".into())),
                // Out: the property goes, and the object too when it held nothing else.
                (Some(_), false) if props.len() == 1 => Some((key.1, *end, String::new())),
                (Some(i), false) => match (i.checked_sub(1).map(|j| &props[j]), props.get(i + 1)) {
                    (Some(prev), _) => Some((prev.end, props[i].end, String::new())),
                    (None, Some(next)) => Some((props[i].start, next.start, String::new())),
                    (None, None) => unreachable!("more than one property"),
                },
                (None, false) => None,
                (None, true) => match props.last() {
                    Some(last) => Some((last.end, last.end, ", muted: true".into())),
                    None => Some((*start, *end, "{ muted: true }".into())),
                },
            }
        }
        _ => return Err(in_code()),
    })
}

/// Mute `modules` in the song, or unmute them: one `Confined` Tx on their `track()` calls'
/// options, one undo step, named "Kick → Kick, muted in song". A track already so is left alone;
/// with none left, nothing is written.
pub fn mute_in_song(s: &mut Session, modules: &[String], muted: bool) -> Result<Edited, EditError> {
    ready(s)?;
    if modules.is_empty() {
        return Err(EditError::Rejected("Name a track to mute in the song.".into()));
    }
    let rev = s.accepted();
    let mut edits: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    let (mut labels, mut changes, mut first) = (vec![], vec![], None);
    for key in modules.iter().collect::<BTreeSet<_>>() {
        if key == ROOT {
            return Err(EditError::Rejected("The song itself can't be muted: mute its tracks.".into()));
        }
        let m = rev.compiled.ir.modules.iter().find(|m| &m.key == key).ok_or_else(|| EditError::NotFound(key.clone()))?;
        if !m.track {
            return Err(EditError::Rejected(format!("{key} isn't a track, so it can't be muted in the song")));
        }
        let in_code = || EditError::Rejected(format!("{}'s track() call isn't written with its key in the code; mute it there", m.label()));
        let site = track_call(&rev.compiled.sites, &rev.snapshot.files, m).ok_or_else(in_code)?;
        let text = &rev.snapshot.files[Path::new(&site.file)];
        first.get_or_insert_with(|| site.file.clone());
        if let Some(e) = option_edit(text, site, m, muted)? {
            edits.entry(site.file.clone()).or_default().push(e);
            changes.push(ModuleChange::Muted { module: m2_song::Key::new(key).map_err(EditError::Rejected)?, muted });
        }
    }
    // Labels in the order asked.
    for key in modules {
        if let Some(m) = rev.compiled.ir.modules.iter().find(|m| &m.key == key)
            && !labels.contains(&m.label().to_string())
        {
            labels.push(m.label().to_string());
        }
    }
    let (plain, said) = (listed(&labels), format!("{}, muted in song", listed(&labels)));
    let (before, after) = if muted { (plain, said) } else { (said, plain) };
    let file = first.unwrap_or_default();
    if edits.is_empty() {
        return Ok(Edited { revision: rev.number, file, before: after.clone(), after });
    }
    let mut ops = vec![];
    let mut ranges = BTreeMap::new();
    for (f, mut e) in edits {
        e.sort_by_key(|x| x.0);
        let p = path(&f)?;
        ops.push(plan::text_edit(&p, &rev.snapshot.files[Path::new(&f)], &e));
        ranges.insert(p, Declared::Ranges(e.iter().map(|x| Range::bytes(x.0, x.1)).collect()));
    }
    let wanted: Vec<String> = changes.iter().filter_map(|c| if let ModuleChange::Muted { module, .. } = c { Some(module.to_string()) } else { None }).collect();
    let tx = plan::planned(s, (before.clone(), after.clone()), ops, Expect::Confined(Confined { ranges, modules: changes, ..Confined::default() }));
    let say = |why: &str| EditError::Rejected(format!("muting in the song would change more than that ({why})"));
    let done = plan::run_as(s, tx, say, |_, checked| {
        // The planner's assertion: each track is as asked in the candidate's graph.
        let now = &checked.candidate().compiled.flat.muted;
        match wanted.iter().all(|k| now.contains(k) == muted) {
            true => Ok(()),
            false => Err(EditError::Rejected("the edit wouldn't mute what it names".into())),
        }
    })?;
    Ok(Edited { revision: done.number, file, before, after })
}

/// What a track drives besides its sound (MON-2): a module and the component there that reads it
/// through a sidechain input or a control export. Muting the track never silences these.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Feed {
    /// The module that reads it: `song` for a node written in `song.ts` (Q6).
    pub module: String,
    /// The reading component's label ("Duck"), or the module's when no component made it.
    pub label: String,
}

/// For each track that drives something besides its sound, what: every sidechain input a
/// component call in another module gave it (PKG-13), and every control output of it another
/// module's node reads (`kick.env`).
pub fn feeds(rev: &Revision) -> BTreeMap<String, Vec<Feed>> {
    let c = &rev.compiled;
    let (ir, flat) = (&c.ir, &c.flat);
    let label_of = |m: &Module, frame: Option<u32>| -> String {
        let component = m.chain(frame).into_iter().rev().find_map(|f| f.component.as_deref());
        component.map_or_else(|| m.label().to_string(), |id| c.components.get(id).map_or_else(|| id.to_string(), |d| d.label.clone()))
    };
    // The checked node of each IR node, by name, and the track of each checked node.
    let index: BTreeMap<&str, usize> = flat.nodes.iter().enumerate().map(|(i, n)| (n.name.as_str(), i)).collect();
    let track = |m: &str| ir.modules.iter().any(|x| x.key == m && x.track);
    let mut out: BTreeMap<String, BTreeSet<Feed>> = BTreeMap::new();
    // The checked node a ref of module `m` reads, following imports.
    let resolve = |m: &Module, r: &str| -> Option<usize> {
        let (mut m, mut id) = (m, split_ref(r).0.to_string());
        for _ in 0..32 {
            let node = m.nodes.iter().find(|n| n.id == id)?;
            let m2_ir::Op::Import { module, export } = &node.op else { return index.get(format!("{}/{id}", m.key).as_str()).copied() };
            m = ir.modules.iter().find(|x| &x.key == module)?;
            id = split_ref(m.exports.get(export)?).0.to_string();
        }
        None
    };
    for m in &ir.modules {
        // Sidechain inputs given another track's signal.
        for (fi, f) in m.frames.iter().enumerate() {
            for sc in &f.sidechain {
                if let Some(i) = resolve(m, &sc.source)
                    && flat.nodes[i].module != m.key
                    && track(&flat.nodes[i].module)
                {
                    let feed = Feed { module: m.key.clone(), label: label_of(m, Some(fi as u32)) };
                    out.entry(flat.nodes[i].module.clone()).or_default().insert(feed);
                }
            }
        }
        // Control outputs of another track read here.
        for n in &m.nodes {
            let Some(&i) = index.get(format!("{}/{}", m.key, n.id).as_str()) else { continue };
            let (FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. }) = &flat.nodes[i].op else { continue };
            for input in inputs {
                let Input::Port { src, .. } = input else { continue };
                let from = &flat.nodes[src.node];
                if from.module != m.key && track(&from.module) && matches!(from.outputs.get(src.port), Some(Kind::Control(_))) {
                    out.entry(from.module.clone()).or_default().insert(Feed { module: m.key.clone(), label: label_of(m, n.frame) });
                }
            }
        }
    }
    out.into_iter().map(|(k, v)| (k, v.into_iter().collect())).collect()
}

#[cfg(test)]
#[path = "mute_tests.rs"]
mod tests;

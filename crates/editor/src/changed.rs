//! What an undo step changed, as the views name it (AGENT-5): the tracks, lanes and knobs the app
//! outlines while a change made elsewhere is new. Read from the delta between the revision before
//! the step and the accepted one (WRITE-8), so a node inserted elsewhere renames nothing.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;

use m2_session::text::Edits;
use m2_session::{Revision, Session};

use crate::controls::{controls_in, site_by_id};

/// What a step changed: tracks by module key, lanes as `[module, lane]`, and knobs by the view's
/// ids in the accepted revision. Each sorted, each once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Changed {
    pub tracks: Vec<String>,
    pub lanes: Vec<(String, String)>,
    pub knobs: Vec<String>,
}

/// What changed from `before` to the session's accepted revision. Nothing when they are one
/// revision (a change to `layout.json` alone, PLANE-3).
pub fn changed(s: &Session, before: &Revision) -> Changed {
    let after = s.accepted();
    if before.number == after.number {
        return Changed::default();
    }
    let d = m2_session::tx::delta_between(before, after);
    let mut tracks = BTreeSet::new();
    let module = |name: &[String]| name.first().cloned();
    tracks.extend(d.added.iter().chain(&d.removed).filter_map(|n| module(&n.name)));
    tracks.extend(d.rewired.iter().filter_map(|w| module(&w.node)));
    tracks.extend(d.values.iter().filter_map(|v| module(&v.node)));
    tracks.extend(d.states.iter().filter_map(|s| s.split('/').next().map(String::from)));
    for m in &d.modules {
        use m2_song::tx::ModuleChange;
        tracks.insert(match m {
            ModuleChange::Added(k) | ModuleChange::Removed(k) | ModuleChange::Muted { module: k, .. } => k.to_string(),
            ModuleChange::Rekeyed { to, .. } => to.to_string(),
        });
    }
    let lanes: BTreeSet<(String, String)> = d.lanes.iter().map(|l| (l.module.to_string(), l.lane.to_string())).collect();
    tracks.extend(lanes.iter().map(|(m, _)| m.clone()));
    // Only modules the accepted revision has: one removed shows nothing.
    tracks.retain(|t| after.compiled.ir.modules.iter().any(|m| m.key.as_str() == t));
    let knobs = if d.values.is_empty() { vec![] } else { knobs(before, after) };
    Changed { tracks: tracks.into_iter().collect(), lanes: lanes.into_iter().collect(), knobs }
}

/// The knobs of `after` whose value differs from what the same knob had in `before`: each of
/// `before`'s carried by its call (WRITE-12's carry), so a call inserted above renames nothing.
fn knobs(before: &Revision, after: &Revision) -> Vec<String> {
    let now: HashMap<String, f64> = controls_in(after).into_iter().map(|c| (c.id, c.value)).collect();
    let mut out = BTreeSet::new();
    for c in controls_in(before) {
        if let Some(id) = carried(before, after, &c.id)
            && now.get(&id).is_some_and(|v| *v != c.value)
        {
            out.insert(id);
        }
    }
    out.into_iter().collect()
}

/// Knob `id` of `before`, as `after` names it: its call found where its first character went, so
/// a change inside the call (its number) keeps it, and one above it renumbers it.
fn carried(before: &Revision, after: &Revision, id: &str) -> Option<String> {
    let (old, arg) = site_by_id(&before.compiled.sites, id)?;
    let file = Path::new(&old.file);
    let (was, is) = (before.snapshot.files.get(file)?, after.snapshot.files.get(file)?);
    let start = old.start as usize;
    let to = Edits::between(was, is).carry(start..start + 1)?.start as u32;
    let site = after.compiled.sites.iter().find(|x| x.file == old.file && x.start == to)?;
    Some(format!("{}#{}/{}", site.file, site.ordinal, arg.join(".")))
}

#[cfg(test)]
#[path = "changed_tests.rs"]
mod tests;

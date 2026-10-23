//! What an undo step changed, as the views name it (AGENT-5): the tracks, lanes and knobs the app
//! outlines while a change made elsewhere is new. Read from the delta between the revision before
//! the step and the accepted one (WRITE-8), so a node inserted elsewhere renames nothing.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use serde::Serialize;

use m2_session::Revision;
use m2_session::text::Edits;

use crate::controls::{controls_in, site_by_id};

/// What a step changed: tracks by module key, lanes as `[module, lane]`, and knobs by the view's
/// ids in the accepted revision. Each sorted, each once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Changed {
    pub tracks: Vec<String>,
    pub lanes: Vec<(String, String)>,
    pub knobs: Vec<String>,
}

/// What changed from `before` to `after`. Nothing when they are one revision (a change to
/// `layout.json` alone, PLANE-3). Pure on the two revisions, so it can be worked out anywhere.
pub fn changed(before: &Revision, after: &Revision) -> Changed {
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

/// The knobs of `after` whose value differs from what the same knob had in `before`. A knob is its
/// call, carried by WRITE-12's carry (so a call inserted above renames nothing), and what in the
/// call it is: its argument path, or for one the call leaves out, its slot (`@slot`). A knob that
/// showed its default and is written now (`@ceiling` to `1.ceiling`), or the other way, is the
/// same knob: they share the slot.
fn knobs(before: &Revision, after: &Revision) -> Vec<String> {
    let mut was: HashMap<(String, String), f64> = HashMap::new();
    for c in controls_in(before) {
        let Some(call) = carried(before, after, &c.id) else { continue };
        let Some((_, what)) = c.id.rsplit_once('/') else { continue };
        was.insert((call.clone(), what.to_string()), c.value);
        for d in &c.declared {
            was.entry((call.clone(), format!("@{}", d.slot))).or_insert(c.value);
        }
    }
    let mut out = BTreeSet::new();
    for c in controls_in(after) {
        let Some((call, what)) = c.id.rsplit_once('/') else { continue };
        let slots = c.declared.iter().map(|d| format!("@{}", d.slot));
        let old = std::iter::once(what.to_string()).chain(slots).find_map(|w| was.get(&(call.to_string(), w)));
        if old.is_some_and(|v| *v != c.value) {
            out.insert(c.id);
        }
    }
    out.into_iter().collect()
}

/// The call of knob `id` of `before`, as `after` names it (`file#ordinal`): found where the `(`
/// that opens its arguments went, which names a call across edits (ID-13), so a change inside it
/// (its number) keeps it, and a call inserted above renumbers it.
fn carried(before: &Revision, after: &Revision, id: &str) -> Option<String> {
    let (old, _) = site_by_id(&before.compiled.sites, id)?;
    let file = Path::new(&old.file);
    let (was, is) = (before.snapshot.files.get(file)?, after.snapshot.files.get(file)?);
    let open = old.open as usize;
    let to = Edits::between(was, is).carry(open..open + 1)?.start as u32;
    let site = after.compiled.sites.iter().find(|x| x.file == old.file && x.open == to)?;
    Some(format!("{}#{}", site.file, site.ordinal))
}

#[cfg(test)]
#[path = "changed_tests.rs"]
mod tests;

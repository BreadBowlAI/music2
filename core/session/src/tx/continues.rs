//! Which nodes of a new revision continue nodes of the one that plays (ID-8, ID-13, ID-15): what
//! the engine may keep the instances of when it activates the new revision's plan. The runtime then
//! keeps a node only when its shape matches too.
//!
//! Two passes, the second over the nodes the first didn't claim, on both sides:
//!
//! 1. A plugin with a state path continues the plugin of the same module with the same path,
//!    wherever either is (ID-8): `plugin.state-collision` makes the pair unique on each side.
//! 2. Every other node continues the node with the same name and op in the comparison step 6's
//!    delta makes (`delta_between`), while the counts its name's occurrences come from are the
//!    same (ID-14): untouched files keep every name, and so does every call of an edited file
//!    whose `(` no change touched, a wrapped one included (ID-13). A plugin given a key here
//!    continues by its name, though its op's key changed.

use std::collections::HashMap;

use m2_ir::{FNode, FOp};
use m2_runtime::Continues;
use m2_song::Snapshot;
use serde_json::Value;

use super::coords_between;
use super::delta::{Positions, Side};
use crate::Compiled;
use crate::session::Revision;

/// Two ops the same but for a plugin's key and frames, which make its state path: a plugin given a
/// key, or another one, continues by its name (ID-8), and its shape judges its state.
fn alike(x: &Value, y: &Value) -> bool {
    let bare = |v: &Value| {
        let mut v = v.clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("key");
            o.remove("frames");
        }
        v
    };
    x == y || bare(x) == bare(y)
}

/// For each top-level node of `new`'s graph, the node of `old`'s graph it continues.
pub fn continues(old: &Revision, new: &Revision) -> Continues {
    between((&old.compiled, &old.snapshot), (&new.compiled, &new.snapshot))
}

/// [`continues`], from what plays (a build and the snapshot it was read from) to another.
pub fn between(old: (&Compiled, &Snapshot), new: (&Compiled, &Snapshot)) -> Continues {
    let (a, b) = (&old.0.flat, &new.0.flat);
    let mut top = vec![None; b.nodes.len()];
    let mut claimed = vec![false; a.nodes.len()];
    // Each node on either side in one pair at most.
    let pair = |i: usize, j: usize, top: &mut [Option<u32>], claimed: &mut [bool]| {
        if top[i].is_none() && !claimed[j] {
            (top[i], claimed[j]) = (Some(j as u32), true);
        }
    };
    // 1. Keyed plugins, by module and state path.
    fn path(n: &FNode) -> Option<(&str, &[String])> {
        match &n.op {
            FOp::Plugin { state: Some(p), .. } => Some((n.module.as_str(), p.as_slice())),
            _ => None,
        }
    }
    let by_path: HashMap<(&str, &[String]), usize> = a.nodes.iter().enumerate().filter_map(|(j, n)| Some((path(n)?, j))).collect();
    for (i, n) in b.nodes.iter().enumerate() {
        if let Some(&j) = path(n).and_then(|k| by_path.get(&k)) {
            pair(i, j, &mut top, &mut claimed);
        }
    }
    // 2. Names, where both revisions have code: the comparison the delta makes.
    if !std::sync::Arc::ptr_eq(old.0.code(), new.0.code()) || a != b {
        let coords = coords_between(old.1, new.1);
        let side =
            |c: &Compiled, s: &Snapshot, n: bool| Side { compiled: c, snapshot: s, at: Positions { coords: &coords, new: n } }.named();
        let ((ga, was), (gb, is)) = (side(old.0, old.1, false), side(new.0, new.1, true));
        let by_name: HashMap<_, usize> =
            a.nodes.iter().enumerate().filter(|(j, _)| !claimed[*j]).filter_map(|(j, n)| Some((was.get(&n.name)?, j))).collect();
        for (i, n) in b.nodes.iter().enumerate() {
            let Some((name, &j)) = is.get(&n.name).and_then(|name| Some((name, by_name.get(name)?))) else { continue };
            // A name whose op changed is a node removed and one added, as the delta says; one whose
            // loop or group grew or shrank is ambiguous, and resets (ID-14).
            let (x, y) = (&ga.nodes[name], &gb.nodes[name]);
            if alike(&x.sig, &y.sig) && x.counts == y.counts {
                pair(i, j, &mut top, &mut claimed);
            }
        }
    } else {
        for i in 0..b.nodes.len() {
            pair(i, i, &mut top, &mut claimed);
        }
    }
    Continues { top }
}

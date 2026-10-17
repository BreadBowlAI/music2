//! What a notes lane's notes are called (polish A4, Q7): the names the instrument that plays the
//! lane gives its notes in its descriptor (`editor.notes`, PKG-9), so the piano roll's rows say
//! "Kick" and "Snare" on a drum kit's lane where other lanes say C2, D2.

use std::collections::BTreeMap;

use m2_ir::{Node, Op, split_ref};
use m2_session::Revision;

/// Every node of `nodes`, voices included.
fn each<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        if let Op::Poly { body, .. } = &n.op {
            each(&body.nodes, out);
        }
    }
}

/// The names of the notes of lane `lane` of module `module` in revision `rev`: what the component
/// whose call reads the lane calls them, found by the frames the lane's readers were made in (the
/// outermost one that names its notes). Empty when nothing reads the lane, when what reads it
/// names none, or when its readers are of different components.
pub fn of(rev: &Revision, module: &str, lane: &str) -> BTreeMap<u8, String> {
    let compiled = &rev.compiled;
    let Some(m) = compiled.ir.modules.iter().find(|m| m.key == module) else { return BTreeMap::new() };
    // The lane's nodes: a module's own (in a voice, a `lane` node is a note's field).
    let lanes: Vec<&str> = m.nodes.iter().filter(|n| matches!(&n.op, Op::Lane { lane: l } if l == lane)).map(|n| n.id.as_str()).collect();
    let reads = |r: &str| lanes.contains(&split_ref(r).0);
    let mut nodes = vec![];
    each(&m.nodes, &mut nodes);
    let readers: Vec<&Node> = nodes
        .into_iter()
        .filter(|n| match &n.op {
            Op::Poly { events, .. } => reads(events),
            Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values().any(|r| reads(r)),
            _ => false,
        })
        .collect();
    let named: Vec<&str> = readers
        .iter()
        .filter_map(|n| {
            m.chain(n.frame)
                .into_iter()
                .find_map(|f| f.component.as_deref().filter(|id| compiled.components.get(*id).is_some_and(|d| !d.editor.notes.is_empty())))
        })
        .collect();
    match named.first() {
        Some(id) if named.len() == readers.len() && named.iter().all(|x| x == id) => compiled.components[*id].editor.notes.clone(),
        _ => BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "note_names_tests.rs"]
mod tests;

//! Sounds that stopped while the song played (RENDER-25): the session tells the user once for each,
//! by the sound and its track, and what starts it again. The audio thread marks them in the plan's
//! [`m2_runtime::Stopped`]; the actor looks before each job, as it does for plugins (HOST-5).

use m2_ir::FOp;
use m2_runtime::Why;
use m2_session::Revision;

use super::Inner;
use super::actor::Open;

/// Tell what stopped in the plan sent last since the last look, each sound once. A plan's marks are
/// its own: in a new plan the same sound can be told again.
pub(super) fn heard(inner: &Inner, open: Option<&Open>) {
    let Some(p) = open.and_then(|o| o.plays.as_ref()) else { return };
    let said: Vec<String> = p.info.stopped.news().into_iter().map(|(node, why)| said(&p.revision, node, why)).collect();
    if !said.is_empty() {
        inner.notices.lock().extend(said);
    }
}

/// What the user is told when node `node` of `rev`'s graph stopped, for `why`. No key: the menu row
/// shows the platform's own, ⌘. on a Mac and Ctrl+. elsewhere (PLAT-2).
fn said(rev: &Revision, node: usize, why: Why) -> String {
    let what = match why {
        Why::Crashed => "stopped working",
        Why::OutOfMemory => "ran out of memory (a sound may use up to 32 MB)",
    };
    format!(
        "{} {what}, so music2 made it silent. The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound.",
        sound_named(rev, node)
    )
}

/// How the user knows node `node` of `rev`'s graph: what it is, and where it is ("The delay on
/// Bass"). A kernel by its name, a `poly` as the instrument, anything else by its id; on a track by
/// its label, in the song's own module as "in the song".
fn sound_named(rev: &Revision, node: usize) -> String {
    let Some(n) = rev.compiled.flat.nodes.get(node) else { return "A sound".into() };
    let what = match &n.op {
        FOp::Poly { .. } => "instrument",
        FOp::Kernel { kernel, .. } => kernel.split_once(':').map_or(kernel.as_str(), |(_, name)| name),
        _ => n.name.rsplit_once('/').map_or(n.name.as_str(), |(_, id)| id),
    };
    let ir = &rev.compiled.ir;
    let place = match ir.modules.iter().find(|m| m.key == n.module) {
        Some(m) if m.track => format!("on {}", m.label()),
        _ if n.module == ir.root.module => "in the song".to_string(),
        Some(m) => format!("in {}", m.label()),
        None => format!("in {}", n.module),
    };
    format!("The {what} {place}")
}

#[cfg(test)]
#[path = "stopped_tests.rs"]
mod tests;

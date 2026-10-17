//! A commit's preview (PLANE-5): once a data Tx passed its checks and its files still read as their
//! bases, the engine hears its lanes while the session makes the commit durable. The swaps are the
//! ones a settle sends (RENDER-26, under RENDER-26a's sounding-note rules), sent from the session's
//! hook between the commit's pre-check and its journal (`Session::before_write`).
//!
//! What plays is then the candidate. The settle that follows every edit decides by the delta from
//! what plays, as always: after a commit that was made durable it is empty (the accepted revision
//! is the candidate's data, shared), so nothing more is sent; after one that failed it is the
//! candidate's lanes, so the accepted data is swapped back. Nothing is accepted before it is
//! durable: the session's revision, its `data_rev` and its undo step come after the renames.

use std::sync::Arc;

use m2_runtime::PlanInfo;
use m2_session::{BeforeWrite, Revision, Session, Writing};
use parking_lot::Mutex;

use super::Inner;
use super::actor::{Open, reach_to};

/// What plays, as the hook may know it for one edit: the actor's own record stays where it is.
struct View {
    epoch: u64,
    generation: u64,
    revision: Arc<Revision>,
    info: Arc<PlanInfo>,
}

/// A preview sent: its last message, when the session's checks were done and when it was queued,
/// and what it plays.
#[derive(Clone)]
struct Sent {
    seq: u64,
    ready_ns: u64,
    sent_ns: u64,
    candidate: Arc<Revision>,
    epoch: u64,
    generation: u64,
}

/// Shared by the actor and the session's hook for one edit.
pub(super) struct Lent {
    view: Option<View>,
    sent: Option<Sent>,
    /// A send failed: the player stopped taking messages, and is let go at the next settle.
    broken: bool,
}

/// What an edit's preview did, once the edit returned.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Heard {
    /// Its message, when the session's checks were done, and when it was queued: None when
    /// nothing was previewed.
    pub(super) previewed: Option<(u64, u64, u64)>,
    /// The transport played while it was previewed: the listener may have heard it. Stopped,
    /// nothing was heard, and nothing is said of it.
    pub(super) played: bool,
    /// The commit failed and its rollback did too: its journal says what is half done.
    pub(super) interrupted: bool,
    /// How long the disk took, when the user was told it isn't saved yet (`Graph::expire`).
    pub(super) slow: Option<std::time::Duration>,
}

impl Heard {
    /// What a failed edit says: one that was heard says it was taken back, or, when its rollback
    /// failed too, where to finish or undo it (the app shows this as a message).
    pub(super) fn said(&self, e: &str) -> String {
        let played = self.previewed.is_some() && self.played;
        let then = match (played, self.interrupted) {
            (true, false) => "It played for a moment and was taken back.",
            (true, true) => "It played for a moment; Needs attention says how to finish or undo it.",
            (false, true) => "Needs attention says how to finish or undo it.",
            (false, false) => return e.to_string(),
        };
        format!("{}. {then}", e.trim_end_matches('.'))
    }

    /// What the app is told once the edit returned, whoever sent it, when its user may have heard
    /// it or was told it isn't saved yet: that it is saved now, or (`failed`, as [`Heard::said`]
    /// words it) that it wasn't. The window that sent it shows its own error too: the same words,
    /// which the app shows once.
    pub(super) fn told(&self, failed: Option<&str>) -> Option<String> {
        match failed {
            None => self.slow.map(|took| format!("The last edit is saved now: the disk took {:.1} s.", took.as_secs_f64())),
            Some(said) if self.previewed.is_some() && (self.played || self.slow.is_some()) => Some(said.to_string()),
            Some(_) => None,
        }
    }
}

/// How many commits the session reports interrupted: one more after an edit is one whose
/// rollback failed.
fn interrupted(s: &Session) -> usize {
    s.problems().iter().filter(|d| d.code == "tx.interrupted").count()
}

/// What plays, lent to the hook for the edit about to run. Only `Arc`s are copied: the actor's
/// record stays, so nothing is lost when the edit panics.
pub(super) fn lend(o: &Open) -> (Arc<Mutex<Lent>>, usize) {
    // Not while a tempo drag plays another tempo map than the candidate's (RENDER-27a).
    let plays = o.plays.as_ref().filter(|p| p.tempo.is_none());
    let view = plays.map(|p| View { epoch: p.epoch, generation: p.generation, revision: p.revision.clone(), info: p.info.clone() });
    (Arc::new(Mutex::new(Lent { view, sent: None, broken: false })), interrupted(&o.session))
}

/// The session's hook: told of a commit that passed every check and whose files read as their
/// bases, before it is journaled, it sends the candidate's lanes to the engine when the commit is
/// data only, what plays is the accepted revision (so the way back is the same swaps), the
/// project plays in the player there is, and each lane fits its scheduler. Anything else is heard
/// once the commit is accepted, as before.
pub(super) fn hook(inner: Arc<Inner>, lent: Arc<Mutex<Lent>>) -> BeforeWrite {
    Box::new(move |w: &Writing| {
        let ready_ns = inner.now_ns();
        let mut l = lent.lock();
        let Some(v) = l.view.as_ref() else { return };
        if !w.data_only() || !Arc::ptr_eq(&v.revision, &w.accepted) {
            return;
        }
        // Never into a player that stopped taking messages: the commit mustn't wait for it.
        let device = inner.current_device().filter(|d| d.epoch == v.epoch && !d.player.stalled() && !d.player.crashed());
        let Some(d) = device else { return };
        let Some(reach) = reach_to(&v.revision, &v.info, &w.candidate) else { return };
        // Lanes alone: a lane no node reads, or a commit that changes nothing that plays, sends
        // nothing and says nothing of a preview.
        if !reach.cells.is_empty() || reach.swaps.is_empty() {
            return;
        }
        let (epoch, generation) = (v.epoch, v.generation);
        let mut seq = None;
        {
            // Under the lock a settle's first phase takes, so a lease's sends and these reach the
            // queue in order.
            let _cp = inner.control.lock();
            for (slot, data) in reach.swaps {
                match d.player.swap_lane(generation, slot, data) {
                    Ok(s) => seq = Some(s),
                    Err(_) => {
                        l.broken = true;
                        break;
                    }
                }
            }
        }
        if let Some(seq) = seq {
            l.sent = Some(Sent { seq, ready_ns, sent_ns: inner.now_ns(), candidate: w.candidate.clone(), epoch, generation });
            // Heard, not yet saved: a disk slow to make it durable is told (`Graph::expire`).
            let played = d.player.status().playing;
            *inner.saving.lock() = Some(super::Saving { since: std::time::Instant::now(), said: false, played });
        }
    })
}

/// Take back what the hook did, once the edit returned (or panicked): when it sent a preview,
/// what plays is the candidate and the settle must bring the engine to the accepted revision.
/// `before` is [`lend`]'s count of interrupted commits.
pub(super) fn give_back(inner: &Inner, lent: &Mutex<Lent>, o: &mut Open, before: usize) -> Heard {
    let saving = inner.saving.lock().take();
    let slow = saving.filter(|s| s.said).map(|s| s.since.elapsed());
    // Played when it was sent, while it was saved (as `Graph::expire` saw), or now.
    let played = saving.is_some_and(|s| s.played) || inner.current_device().is_some_and(|d| d.player.status().playing);
    let mut l = lent.lock();
    let sent = l.sent.take();
    if let Some(s) = &sent {
        if let Some(p) = o.plays.as_mut().filter(|p| p.epoch == s.epoch && p.generation == s.generation) {
            p.revision = s.candidate.clone();
        }
        o.playing = None;
        o.sent = Some(s.seq);
    }
    if l.broken {
        o.playing = None;
    }
    let interrupted = sent.is_some() && interrupted(&o.session) > before;
    let played = sent.is_some() && played;
    Heard { previewed: sent.map(|s| (s.seq, s.ready_ns, s.sent_ns)), played, interrupted, slow }
}

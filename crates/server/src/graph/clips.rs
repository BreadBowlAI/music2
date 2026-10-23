//! Clip gestures on the open project (8b.5): each one Tx through the session, which a lane edit
//! takes as the data path (PLANE-1): no QuickJS run, a `SwapLane` for notes and automation, heard
//! before it is durable (PLANE-5). And Remove automation, which writes code.

use std::sync::Arc;

use anyhow::Result;
use m2_editor::EditError;
use m2_editor::clips::Gesture;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Graph;

/// How a gesture ended: 200 with `{file, before, after, revision, made: [{module, lane, id}]}`, or
/// 409 with `{error, changed: true}` when a clip it names changed since the window drew it
/// (WRITE-12): the window shows the clip as it is now, and the producer tries again.
pub enum Answer {
    Done(Value),
    Changed(String),
}

impl Graph {
    /// Apply a clip gesture ([`m2_editor::clips::edit`]): one Tx, one undo step.
    pub fn clips(&self, token: u64, gesture: Gesture) -> Result<Answer> {
        self.clips_as(token, gesture, None)
    }

    /// [`Graph::clips`] as the Tx with id `tx_id` when given: a lane lease's commit, whose new
    /// clips have the ids its previews played (WRITE-22a, Q21).
    pub(super) fn clips_as(&self, token: u64, gesture: Gesture, tx_id: Option<String>) -> Result<Answer> {
        match self.clips_why(token, gesture, tx_id) {
            (Ok(v), _) => Ok(Answer::Done(v)),
            (Err(e), Some(Refused::Changed)) => Ok(Answer::Changed(format!("{e:#}"))),
            (Err(e), _) => Err(e),
        }
    }

    /// [`Graph::clips_as`]'s Tx, and why the editor refused it when it did: a take says each kind
    /// in its own words (the review's R1).
    pub(super) fn clips_why(&self, token: u64, gesture: Gesture, tx_id: Option<String>) -> (Result<Value>, Option<Refused>) {
        let why = Arc::new(Mutex::new(None));
        let saw = why.clone();
        let tx_id = tx_id.unwrap_or_else(m2_song::tx::random_id);
        let done = self.call(move |a| {
            a.edit_and(token, |s| match m2_editor::clips::edit_as(s, &gesture, &tx_id) {
                Ok(done) => {
                    let made: Vec<Value> =
                        done.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                    Ok((done.edited, json!({"made": made})))
                }
                Err(e) => {
                    *saw.lock() = Some(Refused::of(&e));
                    Err(e)
                }
            })
        });
        let why = *why.lock();
        (done, why)
    }
}

/// Why the editor refused a clip gesture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Refused {
    /// A clip it names changed since (WRITE-12).
    Changed,
    /// Its track or lane isn't there.
    Missing,
    /// The code doesn't compile.
    Draft,
    /// The song is open read-only here (WRITE-17).
    ReadOnly,
    Other,
}

impl Refused {
    pub(super) fn of(e: &EditError) -> Refused {
        match e {
            EditError::Changed(_) => Refused::Changed,
            EditError::Missing(_) => Refused::Missing,
            EditError::DraftInvalid(_) => Refused::Draft,
            EditError::ReadOnly(_) => Refused::ReadOnly,
            _ => Refused::Other,
        }
    }
}

impl Graph {
    /// Remove automation (HOME-2's inverse): the lane's read becomes a literal of its value at
    /// `beat` (the playhead; the fallback when none), and its file goes. One Tx, one undo step.
    pub fn remove_automation(&self, token: u64, module: String, lane: String, beat: Option<f64>) -> Result<Value> {
        self.edit(token, move |s| m2_editor::automate::remove(s, &module, &lane, beat))
    }
}

#[cfg(test)]
#[path = "clips_tests.rs"]
mod tests;

//! Clip gestures on the open project (8b.5): each one Tx through the session, which a lane edit
//! takes as the data path (PLANE-1): no QuickJS run, a `SwapLane` for notes and automation, heard
//! before it is durable (PLANE-5). And Remove automation, which writes code.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use m2_editor::EditError;
use m2_editor::clips::Gesture;
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
        let changed = Arc::new(AtomicBool::new(false));
        let saw = changed.clone();
        let tx_id = tx_id.unwrap_or_else(m2_song::tx::random_id);
        let done = self.call(move |a| {
            a.edit_and(token, |s| match m2_editor::clips::edit_as(s, &gesture, &tx_id) {
                Ok(done) => {
                    let made: Vec<Value> =
                        done.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                    Ok((done.edited, json!({"made": made})))
                }
                Err(e) => {
                    saw.store(matches!(e, EditError::Changed(_)), Ordering::Relaxed);
                    Err(e)
                }
            })
        });
        match done {
            Ok(v) => Ok(Answer::Done(v)),
            Err(e) if changed.load(Ordering::Relaxed) => Ok(Answer::Changed(format!("{e:#}"))),
            Err(e) => Err(e),
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

//! Clip gestures on the open project (8b.5): each one Tx through the session, which a lane edit
//! takes as the data path (PLANE-1): no QuickJS run, a `SwapLane` for notes and automation, heard
//! before it is durable (PLANE-5).

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
        let changed = Arc::new(AtomicBool::new(false));
        let saw = changed.clone();
        let done = self.call(move |a| {
            a.edit_and(token, |s| match m2_editor::clips::edit(s, &gesture) {
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

#[cfg(test)]
#[path = "clips_tests.rs"]
mod tests;

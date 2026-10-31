//! Section gestures on the open project (gap #23, ARR-2): each one Tx through the session, on the
//! data path (PLANE-1), answered as a clip gesture is.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_editor::sections::Gesture;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Graph;
use super::clips::{Answer, Refused};

impl Graph {
    /// Apply a section gesture ([`m2_editor::sections::edit`]): one Tx, one undo step. Answers as
    /// [`Graph::clips`] does: `{file, before, after, revision, made}`, or a conflict when a file it
    /// writes changed since (WRITE-12).
    pub fn sections(&self, token: u64, gesture: Gesture) -> Result<Answer> {
        let why = Arc::new(Mutex::new(None));
        let saw = why.clone();
        let done = self.call(move |a| {
            a.edit_and(token, |s| match m2_editor::sections::edit(s, &gesture) {
                Ok(done) => {
                    let made: Vec<_> =
                        done.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                    Ok((done.edited, json!({"made": made})))
                }
                Err(e) => {
                    *saw.lock() = Some(Refused::of(&e));
                    Err(e)
                }
            })
        });
        match (done, *why.lock()) {
            (Ok(v), _) => Ok(Answer::Done(v)),
            (Err(e), Some(Refused::Changed)) => Ok(Answer::Changed(format!("{e:#}"))),
            (Err(e), _) => Err(e),
        }
    }
}

impl Graph {
    /// A copy of bars `start` to `end` of the open song (ARR-5): every lane's kind and the clips that
    /// overlap them, as `{section}`. Only a read: it works in a read-only window too.
    pub fn section_copy(&self, token: u64, start: f64, end: f64) -> Result<Value> {
        self.call(move |a| match a.open.as_ref().filter(|o| o.token == token) {
            Some(o) => {
                let section = m2_editor::sections::section(o.session.accepted(), start, end).map_err(|e| anyhow!("{e}"))?;
                Ok(json!({ "section": section }))
            }
            None => bail!("the open project changed; refresh the graph before copying"),
        })
    }
}

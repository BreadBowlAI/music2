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

    /// [`Graph::clips`] for a gesture that goes from revision `since` (AGENT-2): a clip it names, or
    /// the Beat grid's lane, changed after it is refused, saying who changed it.
    pub fn clips_since(&self, token: u64, gesture: Gesture, since: Option<u64>) -> Result<Answer> {
        let (gesture, name) = self.measured(token, gesture)?;
        let answer = match self.clips_why_since(token, gesture, None, since) {
            (Ok(v), _) => Answer::Done(v),
            (Err(e), Some(Refused::Changed)) => return Ok(Answer::Changed(format!("{e:#}"))),
            (Err(e), _) => return Err(e),
        };
        // Find notes' step is named for what it did, as a comp's is (VOX-43).
        if let (Answer::Done(v), Some(name)) = (&answer, name)
            && let Some(step) = v["step"].as_u64()
            && let Err(err) = self.name_step_by(token, None, Some(step), name, false)
        {
            eprintln!("music2: couldn't name Find notes' undo step: {err:#}");
        }
        Ok(answer)
    }

    /// What measure `measure` finds in the open song's asset `asset` (VOX-41): `{measure, asset,
    /// hop, rate, outputs, values}`, each output's numbers a list in `values`, in the order of
    /// `outputs`. Measured off the session's lock, and kept in the cache folder.
    pub fn measure(&self, token: u64, asset: String, measure: String) -> Result<Value> {
        let file = self.asset_file(token, &asset)?;
        let plain = |e: EditError| anyhow::anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
        let m = measure.clone();
        let packages = self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow::anyhow!("no project is open"))?;
            o.check_token(token)?;
            m2_editor::sung::packages_measuring(&o.session, &m).map_err(plain)
        })?;
        let found = m2_runtime::measure::measure(&packages, &measure, &file, &[])?;
        Ok(
            json!({"measure": measure, "asset": asset, "hop": found.hop, "rate": found.rate, "outputs": found.outputs, "values": found.values}),
        )
    }

    /// `gesture`, a Find notes (VOX-43) given what its measure found in the clip's file, and its
    /// step's name ("Find notes on Vox"); any other as it is. The measure runs here, off the
    /// session's lock: the session lends the packages it runs from and the file, then is free.
    fn measured(&self, token: u64, gesture: Gesture) -> Result<(Gesture, Option<String>)> {
        let Gesture::FindNotes { clip, measure, render, found: None } = gesture else { return Ok((gesture, None)) };
        let plain = |e: EditError| anyhow::anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
        let (m, r, c) = (measure.clone(), render.clone(), clip.clone());
        let (packages, file) = self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow::anyhow!("no project is open"))?;
            o.check_token(token)?;
            let s = &o.session;
            let packages = m2_editor::sung::packages_for(s, &m, &r).map_err(plain)?;
            Ok((packages, m2_editor::sung::file_of(s, &c).map_err(plain)?))
        })?;
        let found = m2_runtime::measure::measure(&packages, &measure, &file, &[])?;
        let name = {
            let cp = self.inner.control.lock();
            format!("Find notes on {}", super::record::label_of(&cp, clip.lane.module.as_str()))
        };
        Ok((Gesture::FindNotes { clip, measure, render, found: Some(Arc::new(found)) }, Some(name)))
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
        self.clips_why_since(token, gesture, tx_id, None)
    }

    /// [`Graph::clips_why`] from revision `since`, when the gesture says one.
    pub(super) fn clips_why_since(
        &self,
        token: u64,
        gesture: Gesture,
        tx_id: Option<String>,
        since: Option<u64>,
    ) -> (Result<Value>, Option<Refused>) {
        let why = Arc::new(Mutex::new(None));
        let saw = why.clone();
        let tx_id = tx_id.unwrap_or_else(m2_song::tx::random_id);
        let done = self.call(move |a| {
            a.edit_and(token, |s| match m2_editor::clips::edit_since(s, &gesture, &tx_id, since) {
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

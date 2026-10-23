//! The tempo lease (RENDER-27a, 8b.7): the tempo field dragged is heard as it goes, playing or not
//! (the time readout follows), and written once, on release, as 8b.2's tempo edit.
//!
//! - **begin** names the tempo entry in effect at a beat (the playhead's), as the accepted
//!   revision has it. One drag at a time: the window that holds it may begin again; another is
//!   refused.
//! - **update** keeps the newest value and queues one actor job, if none is queued (Q22): the job
//!   settles with the arrangement the drag plays (`DataSet::with_arrangement`), an identity
//!   activation prepared against the plan sent last, where the engine is (RENDER-27). While it
//!   runs, newer values wait; the newest is played next, so one prepare is in flight and values
//!   in between are skipped, never queued. A drag held during a long compile is heard after it.
//!   The field never waits.
//! - **While it lives**, every settle prepares with its arrangement (`Plays.tempo`): an edit made
//!   meanwhile plays at the dragged tempo, and nothing reaches the engine but an activation.
//!   `Timing` (seek, the loop, the status's beat and `bpm`) follows the map that plays.
//! - **commit** is one Tx writing `data/arrangement.json` (8b.2's builder, as `Graph::tempo` runs
//!   it); refused, 409, when the arrangement changed since the drag began (X24). Until it lands,
//!   every settle plays the value let go, so a job ahead of it never plays the old tempo; once it
//!   lands, the accepted arrangement is that value.
//! - **cancel**, an idle TTL (10 s; the page sends a keepalive every 2 s) and the window closing
//!   end it: the next settle plays the accepted arrangement again.
//! - **A song with an audio lane** previews nothing (Q23's fallback, X3): its renders are timed in
//!   the accepted map, so the drag is heard when it is let go, as before.
//!
//! Exports render the accepted revision offline: a drag never reaches one.

use std::collections::VecDeque;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_session::Revision;
use m2_song::Arrangement;
use serde::Serialize;
use serde_json::Value;

use super::actor::Job;
use super::lease::{MAX_TTL, TTL};
use super::{ControlPlane, Graph, Inner};

/// Ended tempo leases remembered, so a late update says why its lease ended.
const ENDED: usize = 16;

/// The tempo field, dragged.
pub(super) struct TempoLease {
    id: u64,
    /// The open project it belongs to.
    token: u64,
    /// The window (its WebSocket's `client`): when that closes, the lease ends.
    client: Option<String>,
    ttl: Duration,
    began: Instant,
    touched: Instant,
    /// The beat of the tempo entry it drags: the one in effect where it began.
    at: f64,
    /// The accepted arrangement it began on: a commit after another writer changed it is refused.
    began_on: Arc<Arrangement>,
    /// The newest value, and its update's number.
    bpm: f64,
    seq: u64,
    /// The value the plan sent last plays, and how many plans played its values.
    heard: Option<f64>,
    previews: u64,
    /// An actor job is queued or running for it.
    queued: bool,
    /// Its commit is on the actor: settles play the value let go (`bpm`) until it lands.
    closing: bool,
    /// A lane of what plays is audio: nothing is previewed (Q23).
    silent: bool,
}

/// The open project's tempo lease, on the control plane.
#[derive(Default)]
pub(super) struct TempoLeases {
    live: Option<TempoLease>,
    ended: VecDeque<(u64, String)>,
}

impl TempoLeases {
    /// Lease `id`, live, or why it isn't.
    fn get(&mut self, id: u64) -> Result<&mut TempoLease> {
        match self.live.as_mut().filter(|l| l.id == id) {
            Some(l) => Ok(l),
            None => match self.ended.iter().find(|e| e.0 == id) {
                Some((_, why)) => Err(anyhow!("{why}")),
                None => Err(anyhow!("no tempo drag {id}")),
            },
        }
    }

    /// End the live lease, remembering why: it ends with whatever it was.
    fn end(&mut self, why: impl Into<String>) -> Option<TempoLease> {
        let l = self.live.take()?;
        self.ended.push_back((l.id, why.into()));
        while self.ended.len() > ENDED {
            self.ended.pop_front();
        }
        Some(l)
    }
}

/// What `begin` answers: the lease, and the entry it drags.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TempoBegun {
    pub lease: u64,
    pub at: f64,
    pub bpm: f64,
    /// Whether it is heard as it goes: false in a song with an audio lane (heard on release).
    pub heard: bool,
}

/// A tempo lease as `GET /api/graph/leases` lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TempoLeaseInfo {
    pub kind: &'static str,
    pub lease: u64,
    pub client: Option<String>,
    pub state: &'static str,
    pub at: f64,
    pub bpm: f64,
    /// The value what plays plays, and how many plans played its values.
    pub heard: Option<f64>,
    pub previews: u64,
    pub age_ms: u64,
    pub idle_ms: u64,
}

/// How a commit ended: written (the tempo edit's answer), or refused because another writer
/// changed the tempo since the drag began (409).
pub enum TempoCommitted {
    Done(Value),
    Changed(String),
}

/// `a` with the tempo in effect at `at` set to `bpm`, as 8b.2's builder sets it.
fn retempo(a: &Arrangement, at: f64, bpm: f64) -> Arrangement {
    let mut a = a.clone();
    let i = a.tempo.partition_point(|t| t.beat <= at).saturating_sub(1);
    if let Some(t) = a.tempo.get_mut(i) {
        t.bpm = bpm;
    }
    a
}

/// What a settle of project `token` plays in place of the accepted arrangement, and the value it
/// carries: the drag's, unless it plays nothing new or a lane plays `audio` (Q23's fallback: heard
/// on release). A drag being written plays the value let go until its commit lands; then the
/// accepted arrangement is that value, so nothing is played in its place.
pub(super) fn wanted(cp: &mut ControlPlane, token: u64, accepted: &Revision, audio: bool) -> Option<(Arc<Arrangement>, f64)> {
    let l = cp.tempo.live.as_mut().filter(|l| l.token == token)?;
    l.silent = audio;
    let a = retempo(&accepted.project.arrangement, l.at, l.bpm);
    (!audio && a != accepted.project.arrangement).then(|| (Arc::new(a), l.bpm))
}

/// Whether the musician drags project `token`'s tempo now (AGENT-3).
pub(super) fn held(cp: &ControlPlane, token: u64) -> bool {
    cp.tempo.live.as_ref().is_some_and(|l| l.token == token)
}

/// A plan playing the drag's value `bpm` was sent.
pub(super) fn heard(cp: &mut ControlPlane, token: u64, bpm: f64) {
    if let Some(l) = cp.tempo.live.as_mut().filter(|l| l.token == token) {
        l.heard = Some(bpm);
        l.previews += 1;
    }
}

/// The project closes: its drag ends with it.
pub(super) fn end_all(cp: &mut ControlPlane) {
    cp.tempo.end("The song was closed.");
}

/// The actor's job for project `token`'s drag: play its newest value (or, once it ended, the
/// accepted arrangement); then, while a newer value waits and the last one was sent, again, as a
/// job of its own, behind whatever else was asked meanwhile.
fn job(inner: Arc<Inner>, jobs: mpsc::Sender<Job>, token: u64) -> Job {
    Box::new(move |a| {
        let before = inner.control.lock().tempo.live.as_ref().and_then(|l| l.heard);
        a.tempo_preview(token);
        let mut cp = inner.control.lock();
        let Some(l) = cp.tempo.live.as_mut().filter(|l| l.token == token) else { return };
        // Sent, and moved on since: the newest value next. Not sent (an export holds playback, no
        // player): the next update, or the export's end, plays it.
        let again = !l.closing && !l.silent && l.heard != before && l.heard != Some(l.bpm);
        l.queued = again;
        if again {
            drop(cp);
            let _ = jobs.send(job(inner.clone(), jobs.clone(), token));
        }
    })
}

impl Graph {
    /// Queue the drag's job, once.
    fn tempo_job(&self, token: u64) {
        let _ = self.jobs.send(job(self.inner.clone(), self.jobs.clone(), token));
    }

    /// Begin a drag of the tempo in effect at beat `at` (RENDER-27a), as the accepted revision has
    /// it. Never waits for the session. The window that holds a drag may begin again; another
    /// window's begin is refused.
    pub fn tempo_begin(&self, token: u64, at: f64, client: Option<String>, ttl: Option<Duration>) -> Result<TempoBegun> {
        super::busy::no_lease(&crate::caller::now())?;
        if !at.is_finite() {
            bail!("can't drag the tempo at beat {at}");
        }
        let mut cp = self.inner.control.lock();
        let p = cp.open.as_ref().context("no project is open")?;
        if p.engine.token != token {
            bail!("the open project changed; refresh the graph before editing");
        }
        let accepted = p.engine.accepted.clone();
        let audio = p.engine.plays.as_ref().is_some_and(|p| p.info.lanes.iter().any(|l| matches!(l.lane.kind, m2_ir::Kind::Audio(_))));
        if let Some(l) = cp.tempo.live.as_ref().filter(|l| l.token == token) {
            if l.closing || l.client.is_none() || l.client != client {
                bail!("The tempo is being dragged in another window.");
            }
            // The same window took hold again: the drag before ends, and this one goes on from it.
            cp.tempo.end("A new drag of the tempo began.");
        }
        let a = &accepted.project.arrangement;
        let i = a.tempo.partition_point(|t| t.beat <= at.max(0.0)).saturating_sub(1);
        let entry = a.tempo.get(i).cloned().unwrap_or(m2_song::Tempo { beat: 0.0, bpm: 120.0 });
        let id = cp.leases.next_id();
        let now = Instant::now();
        cp.tempo.live = Some(TempoLease {
            id,
            token,
            client,
            ttl: ttl.unwrap_or(TTL).min(MAX_TTL),
            began: now,
            touched: now,
            at: entry.beat,
            began_on: Arc::new(a.clone()),
            bpm: entry.bpm,
            seq: 0,
            heard: None,
            previews: 0,
            queued: false,
            closing: false,
            silent: audio,
        });
        Ok(TempoBegun { lease: id, at: entry.beat, bpm: entry.bpm, heard: !audio })
    }

    /// Play `bpm` for drag `lease`: kept as its newest value, played by the actor's job at the
    /// first block after its plan is prepared. Never waits. An update older than one taken is
    /// dropped (Ok(None)); without `seq`, it follows the last. Returns the update's number.
    pub fn tempo_update(&self, lease: u64, seq: Option<u64>, bpm: f64) -> Result<Option<u64>> {
        if !(20.0..=999.0).contains(&bpm) {
            bail!("A tempo is 20 to 999 BPM, not {bpm}.");
        }
        let (token, seq) = {
            let mut cp = self.inner.control.lock();
            let l = cp.tempo.get(lease)?;
            if l.closing {
                bail!("this tempo drag is being written");
            }
            let seq = match seq {
                Some(s) if s <= l.seq => return Ok(None),
                Some(s) => s,
                None => l.seq + 1,
            };
            (l.seq, l.bpm, l.touched) = (seq, bpm, Instant::now());
            if l.silent || l.queued {
                return Ok(Some(seq));
            }
            l.queued = true;
            (l.token, seq)
        };
        self.tempo_job(token);
        Ok(Some(seq))
    }

    /// Keep drag `lease` alive while the pointer holds still.
    pub fn tempo_keepalive(&self, lease: u64) -> Result<()> {
        self.inner.control.lock().tempo.get(lease)?.touched = Instant::now();
        Ok(())
    }

    /// End drag `lease` without a write: the accepted tempo plays again, from the actor's next
    /// job. Never waits.
    pub fn tempo_cancel(&self, lease: u64) -> Result<()> {
        let token = {
            let mut cp = self.inner.control.lock();
            let Ok(l) = cp.tempo.get(lease) else { return Ok(()) };
            if l.closing {
                bail!("this tempo drag is being written; the write decides");
            }
            cp.tempo.end("This tempo drag was cancelled.").map(|l| l.token)
        };
        if let Some(t) = token {
            self.tempo_job(t);
        }
        Ok(())
    }

    /// Write drag `lease` at `bpm`: one Tx (8b.2's tempo edit), one undo step, answered as
    /// `POST /api/graph/tempo` answers. Changed: another writer changed the tempo since the drag
    /// began.
    pub fn tempo_commit(&self, lease: u64, bpm: f64) -> Result<TempoCommitted> {
        let (token, at, changed) = {
            let mut cp = self.inner.control.lock();
            let accepted = cp.open.as_ref().map(|p| p.engine.accepted.clone()).context("no project is open")?;
            let l = cp.tempo.get(lease)?;
            if l.closing {
                bail!("this tempo drag is being written");
            }
            // What was let go plays on until the commit lands.
            (l.closing, l.bpm) = (true, bpm);
            (l.token, l.at, accepted.project.arrangement != *l.began_on)
        };
        if changed {
            let why = "The tempo changed elsewhere: your drag was dropped.";
            self.inner.control.lock().tempo.end(why);
            self.tempo_job(token);
            return Ok(TempoCommitted::Changed(why.into()));
        }
        // Its settle finds the accepted arrangement holds the value let go: nothing in its place.
        let done = self.tempo(token, at, Some(bpm), false, false, false);
        let mut cp = self.inner.control.lock();
        if cp.tempo.live.as_ref().is_some_and(|l| l.id == lease) {
            cp.tempo.end(match &done {
                Ok(_) => "This tempo drag was written.".to_string(),
                Err(e) => format!("{e:#}"),
            });
        }
        drop(cp);
        // Not written: the accepted tempo plays again.
        if done.is_err() {
            self.tempo_job(token);
        }
        done.map(TempoCommitted::Done)
    }

    /// End a drag idle past its TTL (the server's `music2-tell` thread, every 300 ms).
    pub(super) fn tempo_expire(&self, now: Instant) {
        let token = {
            let mut cp = self.inner.control.lock();
            match cp.tempo.live.as_ref() {
                Some(l) if !l.closing && now.duration_since(l.touched) > l.ttl => {
                    let why = format!("This tempo drag ended: nothing was heard from it for {} s.", l.ttl.as_secs_f64().round());
                    cp.tempo.end(why).map(|l| l.token)
                }
                _ => None,
            }
        };
        if let Some(t) = token {
            self.tempo_job(t);
        }
    }

    /// The window `client` closed: its drag ends.
    pub(super) fn tempo_gone(&self, client: &str) {
        let token = {
            let mut cp = self.inner.control.lock();
            match cp.tempo.live.as_ref() {
                Some(l) if !l.closing && l.client.as_deref() == Some(client) => {
                    cp.tempo.end("The window that owned this tempo drag closed.").map(|l| l.token)
                }
                _ => None,
            }
        };
        if let Some(t) = token {
            self.tempo_job(t);
        }
    }

    /// The tempo drag in progress, as `GET /api/graph/leases` lists it.
    pub fn tempo_leases(&self) -> Vec<TempoLeaseInfo> {
        let now = Instant::now();
        let cp = self.inner.control.lock();
        cp.tempo
            .live
            .iter()
            .map(|l| TempoLeaseInfo {
                kind: "tempo",
                lease: l.id,
                client: l.client.clone(),
                state: if l.closing { "committing" } else { "live" },
                at: l.at,
                bpm: l.bpm,
                heard: l.heard,
                previews: l.previews,
                age_ms: now.duration_since(l.began).as_millis() as u64,
                idle_ms: now.duration_since(l.touched).as_millis() as u64,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "tempo_lease_tests.rs"]
mod tests;

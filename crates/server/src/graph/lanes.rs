//! Lane leases (WRITE-22a, 8b.7): a clip, a note or an automation point dragged while the song
//! plays is heard where it is before it is let go, and written once, on release, as one Tx.
//!
//! - **begin** checks the gesture on the accepted revision the control plane last published
//!   (`EngineView.accepted`), never waiting for the session (RENDER-29). Only notes and automation
//!   gestures (`move`, `resize`, `notes`, `points`, `create`) are previewed: a gesture on an audio
//!   lane needs renders, so it answers `previewable: false` and is written on release, as before.
//! - **update** applies the newest gesture with `clips::apply_as` (pure, the commit's own ids,
//!   Q21) and swaps each lane it edits into the renderer that plays (`SwapLane`, RENDER-26, under
//!   RENDER-26a's rules for sounding notes). A notes lane whose preview needs more room than its
//!   scheduler has is heard at the commit, which grows it. A lane an earlier update reached and
//!   this one doesn't gets its accepted data back. Updates older than one taken are dropped.
//! - **commit** is one `Clips` Tx under the lease's Tx id (`clips::edit_as`), through the session
//!   as any clip gesture is: its PLANE-5 preview is what the lease played, so nothing heard
//!   changes. Until it lands, a new plan plays what was let go. A refused commit puts the accepted
//!   data back and says why.
//! - **cancel**, an idle TTL (10 s; the page sends a keepalive every 2 s) and the window closing
//!   put the accepted data back.
//! - **Each settle** (`settled`) carries the live leases to what it accepted: a new plan, or new
//!   data for a lane a lease plays, gets the preview again, applied on the new revision; a change
//!   to a clip the lease drags ends it, and its window hears why at its next request.
//!
//! Exports render the accepted revision offline: a lease never reaches one.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::EditError;
use m2_editor::clips::{self, Gesture};
use m2_runtime::{LaneSwap, NotesData};
use m2_session::Revision;
use m2_song::{LaneAddr, LaneData, LaneKind};
use serde::Serialize;
use serde_json::{Value, json};

use super::clips::Answer;
use super::lease::{MAX_TTL, PlaysView, TTL};
use super::{ControlPlane, Device, Graph};

/// Ended lane leases remembered, so a late update says why its lease ended.
const ENDED: usize = 32;

/// Where a lease's preview of a lane went, and what it played there.
#[derive(Clone)]
pub(super) struct Sent {
    lane: LaneAddr,
    epoch: u64,
    generation: u64,
    data: Arc<LaneData>,
}

/// A drag on clips, heard while it lasts.
pub(super) struct LaneLease {
    pub(super) id: u64,
    /// The open project it belongs to.
    token: u64,
    /// The window (its WebSocket's `client`): when that closes, the lease ends.
    client: Option<String>,
    ttl: Duration,
    began: Instant,
    touched: Instant,
    /// The Tx id its commit has, whose ids the previews mint (Q21).
    tx_id: String,
    /// The newest gesture, each clip it names pinned to the hash it had when seen.
    gesture: Gesture,
    seq: u64,
    /// The accepted revision its preview was applied on.
    basis: Arc<Revision>,
    /// Where its preview plays, lane by lane.
    sent: Vec<Sent>,
    /// Its commit is on the actor, which decides: updates and cancels are refused.
    closing: bool,
}

/// The lane leases of the open project, on the control plane.
#[derive(Default)]
pub(super) struct LaneLeases {
    live: Vec<LaneLease>,
    ended: VecDeque<(u64, String)>,
}

impl LaneLeases {
    fn index(&self, id: u64) -> Result<usize> {
        self.live.iter().position(|l| l.id == id).ok_or_else(|| match self.ended.iter().find(|e| e.0 == id) {
            Some((_, why)) => anyhow!("{why}"),
            None => anyhow!("no drag lease {id}"),
        })
    }

    /// Take lease `index` out, remembering why it ended.
    fn end(&mut self, index: usize, why: impl Into<String>) -> LaneLease {
        let l = self.live.remove(index);
        self.ended.push_back((l.id, why.into()));
        while self.ended.len() > ENDED {
            self.ended.pop_front();
        }
        l
    }
}

/// What `begin` answers.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaneBegun {
    /// None when the gesture isn't previewed: it is written on release, as before.
    pub lease: Option<u64>,
    pub previewable: bool,
    /// The lanes the gesture names.
    pub lanes: Vec<Value>,
    /// Whether what plays reads any of them now.
    pub heard: bool,
    /// The accepted revision the gesture was checked on.
    pub revision: u64,
}

/// A lane lease as `GET /api/graph/leases` lists it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaneLeaseInfo {
    pub kind: &'static str,
    pub lease: u64,
    pub client: Option<String>,
    /// `live` or `committing`.
    pub state: &'static str,
    pub lanes: Vec<Value>,
    pub seq: u64,
    pub revision: u64,
    pub age_ms: u64,
    pub idle_ms: u64,
}

/// `module`'s lane `lane` as JSON.
fn lane_json(a: &LaneAddr) -> Value {
    json!({"module": a.module.as_str(), "lane": a.lane.as_str()})
}

/// The lanes `g` names: the clips' lanes, and a move's target.
fn named(g: &Gesture) -> Vec<LaneAddr> {
    let mut out: Vec<LaneAddr> = vec![];
    let mut add = |a: &LaneAddr| {
        if !out.contains(a) {
            out.push(a.clone());
        }
    };
    match g {
        Gesture::Create { lane, .. } => add(lane),
        Gesture::Move { clips, to, .. } => {
            clips.iter().for_each(|c| add(&c.lane));
            to.iter().for_each(&mut add);
        }
        Gesture::Resize { clip, .. } | Gesture::Notes { clip, .. } | Gesture::Points { clip, .. } | Gesture::Audio { clip, .. } => {
            add(&clip.lane)
        }
        Gesture::Split { clips, .. }
        | Gesture::Join { clips }
        | Gesture::Duplicate { clips, .. }
        | Gesture::Delete { clips }
        | Gesture::Quantize { clips, .. } => clips.iter().for_each(|c| add(&c.lane)),
        Gesture::Paste { clips, to, .. } => {
            clips.iter().for_each(|c| add(&c.from));
            to.iter().for_each(&mut add);
        }
        Gesture::Set { lane, .. } => add(lane),
        Gesture::Record { takes } => takes.iter().for_each(|t| add(&t.lane)),
    }
    out
}

/// Why `g` isn't previewed on `rev`, if it isn't: only a drag of notes or automation is (design-8b
/// §9.1); a gesture on an audio lane needs renders.
fn unpreviewable(rev: &Revision, g: &Gesture) -> Option<&'static str> {
    if !matches!(g, Gesture::Move { .. } | Gesture::Resize { .. } | Gesture::Notes { .. } | Gesture::Points { .. } | Gesture::Create { .. })
    {
        return Some("only a drag is previewed");
    }
    let audio = named(g).iter().any(|a| {
        let m = rev.compiled.ir.modules.iter().find(|m| m.key == a.module.as_str());
        let decl = m.and_then(|m| m.lanes.iter().find(|l| l.key == a.lane.as_str()));
        decl.is_some_and(|d| m2_runtime::lane_kind(d.kind) == LaneKind::Audio)
    });
    audio.then_some("an audio lane is heard once it is let go")
}

/// What `plays` hears of lane `lane` holding `data`: its slot and the swap, when it reads it and
/// the data fits its room (a notes lane that needs more is heard once a commit grows it). Notes are
/// timed in the tempo map that plays: a tempo drag's, while one plays (NX-S4).
fn swap_of(plays: &PlaysView, rev: &Revision, lane: &LaneAddr, data: &LaneData) -> Option<(u32, LaneSwap)> {
    let (module, key) = (lane.module.as_str(), lane.lane.as_str());
    let slot = plays.info.lanes.iter().position(|s| s.lane.module == module && s.lane.lane == key)?;
    let swap = match data {
        LaneData::Notes(clips) => {
            let tempo = plays.tempo.as_deref().unwrap_or(rev.compiled.data.tempo());
            let notes = NotesData::build(clips, tempo, rev.project.sample_rate() as f64);
            if !plays.info.lanes[slot].caps?.fits(notes.needs()) {
                return None;
            }
            LaneSwap::Notes(Arc::new(notes))
        }
        LaneData::Control { clips, .. } => LaneSwap::Control(Arc::new(m2_song::lane::ControlLane::new(clips.clone()))),
        // Names become places here, off the audio thread (CARD-15).
        LaneData::Steps { of, clips } => LaneSwap::Control(Arc::new(m2_song::lane::ControlLane::steps(clips, of))),
        LaneData::Audio(_) => return None,
    };
    Some((slot as u32, swap))
}

/// The lanes `g` makes on `rev`, as Tx `tx_id` would write them.
fn previewed(rev: &Revision, g: &Gesture, tx_id: &str) -> Result<Vec<(LaneAddr, Arc<LaneData>)>, EditError> {
    Ok(clips::apply_as(rev, g, tx_id)?.into_iter().map(|e| (e.lane, Arc::new(e.data))).collect())
}

/// What plays in `device`, if it is the player `plays` was sent to.
fn playing<'a>(plays: Option<&'a PlaysView>, device: Option<&'a Device>) -> Option<(&'a PlaysView, &'a Device)> {
    let (p, d) = (plays?, device?);
    (p.epoch == d.epoch).then_some((p, d))
}

/// Play `lanes` (each lane's data) for lease `l` in what plays, and give the accepted data back to
/// the lanes it reached before and doesn't now. Returns the last engine message's number.
fn send(
    l: &mut LaneLease,
    lanes: &[(LaneAddr, Arc<LaneData>)],
    accepted: &Revision,
    at: Option<(&PlaysView, &Device)>,
) -> Result<Option<u64>> {
    let mut seq = None;
    let mut sent = vec![];
    if let Some((p, d)) = at {
        for (lane, data) in lanes {
            if let Some((slot, swap)) = swap_of(p, accepted, lane, data) {
                seq = Some(d.player.swap_lane(p.generation, slot, swap)?);
                sent.push(Sent { lane: lane.clone(), epoch: p.epoch, generation: p.generation, data: data.clone() });
            }
        }
    }
    let gone: Vec<Sent> = l.sent.iter().filter(|s| !sent.iter().any(|n| n.lane == s.lane)).cloned().collect();
    l.sent = sent;
    seq = put_back(&gone, accepted, at).or(seq);
    Ok(seq)
}

/// Give lanes `sent` their accepted data back where the preview went to the plan that plays: a
/// newer plan was built from the accepted data already, and a lane whose preview the accepted
/// revision now holds (a commit) plays it already.
fn put_back(sent: &[Sent], accepted: &Revision, at: Option<(&PlaysView, &Device)>) -> Option<u64> {
    let (p, d) = at?;
    let mut seq = None;
    for s in sent.iter().filter(|s| (s.epoch, s.generation) == (p.epoch, p.generation)) {
        let now = accepted.compiled.data.lane(s.lane.module.as_str(), s.lane.lane.as_str());
        if now.is_some_and(|d| Arc::ptr_eq(d, &s.data) || **d == *s.data) {
            continue;
        }
        // A lane with no file plays nothing.
        let data = now.map_or_else(|| Arc::new(LaneData::empty(&s.data.kind())), Arc::clone);
        if let Some((slot, swap)) = swap_of(p, accepted, &s.lane, &data) {
            match d.player.swap_lane(p.generation, slot, swap) {
                Ok(n) => seq = Some(n),
                Err(e) => eprintln!("music2: couldn't put a dragged lane back: {e:#}"),
            }
        }
    }
    seq
}

/// End lease `index` of `cp`, its lanes given their accepted data back.
fn end(cp: &mut ControlPlane, index: usize, why: impl Into<String>, device: Option<&Device>) {
    let l = cp.lanes.end(index, why);
    if let Some(p) = cp.open.as_ref().filter(|p| p.engine.token == l.token) {
        put_back(&l.sent, &p.engine.accepted, playing(p.engine.plays.as_ref(), device));
    }
}

/// What the user is told when a drag ends because what it drags changed: "Bass changed
/// elsewhere: your drag was dropped."
fn dropped(rev: &Revision, g: &Gesture) -> String {
    let lane = named(g).into_iter().next();
    let track = lane.as_ref().and_then(|a| rev.compiled.ir.modules.iter().find(|m| m.key == a.module.as_str()));
    let who = track.map_or("The clip", |m| m.label());
    // A sentence starts with a capital: a track named only by its key (`bass`) is "Bass".
    let mut chars = who.chars();
    let who: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
    format!("{who} changed elsewhere: your drag was dropped.")
}

/// Carry every live lane lease of project `token` to what a settle just published (its first
/// phase, under the control plane's lock, after its own sends): a new plan, or new data for a lane
/// a lease names, gets the preview again, applied on the new accepted revision, so it plays from
/// the new plan's first block or the one after (the queue is FIFO); a change to a clip a lease
/// drags ends it, the accepted data playing. A lease whose commit is on the actor plays what was
/// let go in a new plan, until the commit lands; if its lanes changed meanwhile, the commit decides.
pub(super) fn settled(cp: &mut ControlPlane, token: u64, device: Option<&Device>) {
    let Some(engine) = cp.open.as_ref().filter(|p| p.engine.token == token).map(|p| p.engine.clone()) else { return };
    let at = playing(engine.plays.as_ref(), device);
    let mut i = 0;
    while i < cp.lanes.live.len() {
        let l = &mut cp.lanes.live[i];
        if l.token != token {
            i += 1;
            continue;
        }
        let accepted = &engine.accepted;
        let lanes = named(&l.gesture).into_iter().chain(l.sent.iter().map(|s| s.lane.clone()));
        let moved = !Arc::ptr_eq(&l.basis, accepted)
            && lanes.into_iter().any(|a| {
                let (m, k) = (a.module.as_str(), a.lane.as_str());
                match (l.basis.compiled.data.lane(m, k), accepted.compiled.data.lane(m, k)) {
                    (Some(x), Some(y)) => !Arc::ptr_eq(x, y),
                    (None, None) => false,
                    _ => true,
                }
            });
        let fresh = at.is_some_and(|(p, _)| l.sent.iter().any(|s| (s.epoch, s.generation) != (p.epoch, p.generation)));
        if l.closing {
            // Its lanes as the lease saw them (the same data, though perhaps read again).
            let kept = named(&l.gesture).iter().chain(l.sent.iter().map(|s| &s.lane)).all(|a| {
                let (m, k) = (a.module.as_str(), a.lane.as_str());
                match (l.basis.compiled.data.lane(m, k), accepted.compiled.data.lane(m, k)) {
                    (Some(x), Some(y)) => Arc::ptr_eq(x, y) || x == y,
                    (x, y) => x.is_none() && y.is_none(),
                }
            });
            if fresh && kept {
                let lanes: Vec<(LaneAddr, Arc<LaneData>)> = l.sent.iter().map(|s| (s.lane.clone(), s.data.clone())).collect();
                if let Err(e) = send(l, &lanes, accepted, at) {
                    eprintln!("music2: couldn't carry a drag being written to the new plan: {e:#}");
                }
            }
            i += 1;
            continue;
        }
        if !moved && !fresh {
            l.basis = accepted.clone();
            i += 1;
            continue;
        }
        match carried(l, accepted) {
            Ok(lanes) => {
                if let Err(e) = send(l, &lanes, accepted, at) {
                    eprintln!("music2: couldn't carry a drag to the new revision: {e:#}");
                }
                l.basis = accepted.clone();
                i += 1;
            }
            Err(why) => end(cp, i, why, device),
        }
    }
}

/// Whether lease `l` goes on in revision `rev`: its gesture, applied there, as it then plays; else
/// why it ends (a clip it drags changed, or its lane is gone). What decides a drag's fate at each
/// settle, and what an agent's change is checked by (AGENT-3).
fn carried(l: &LaneLease, rev: &Revision) -> Result<Vec<(LaneAddr, Arc<LaneData>)>, String> {
    previewed(rev, &l.gesture, &l.tx_id).map_err(|e| match e {
        EditError::Changed(_) | EditError::Missing(_) => dropped(&l.basis, &l.gesture),
        e => format!("Your drag was dropped: {e}"),
    })
}

/// The musician's drags on clips in project `token` that revision `rev` would end, whatever it
/// changed (AGENT-3): each one its gesture can't be carried to. One being written counts too: its
/// commit would meet the same.
pub(super) fn ending(cp: &ControlPlane, token: u64, rev: &Arc<Revision>) -> Vec<u64> {
    let ends = |l: &&LaneLease| l.token == token && !Arc::ptr_eq(&l.basis, rev) && carried(l, rev).is_err();
    cp.lanes.live.iter().filter(ends).map(|l| l.id).collect()
}

/// The lanes the musician's live clip drags hold in project `token` (AGENT-3), each by its drag.
pub(super) fn held(cp: &ControlPlane, token: u64) -> Vec<(u64, LaneAddr)> {
    let mut out: Vec<(u64, LaneAddr)> = vec![];
    for l in cp.lanes.live.iter().filter(|l| l.token == token) {
        for a in named(&l.gesture).into_iter().chain(l.sent.iter().map(|s| s.lane.clone())) {
            if !out.contains(&(l.id, a.clone())) {
                out.push((l.id, a));
            }
        }
    }
    out
}

/// End every lane lease idle past its TTL (the server's `music2-tell` thread, every 300 ms).
pub(super) fn expire(cp: &mut ControlPlane, device: Option<&Device>, now: Instant) {
    let mut i = 0;
    while i < cp.lanes.live.len() {
        let l = &cp.lanes.live[i];
        if !l.closing && now.duration_since(l.touched) > l.ttl {
            let why = format!("This drag ended: nothing was heard from it for {} s.", l.ttl.as_secs_f64().round());
            end(cp, i, why, device);
        } else {
            i += 1;
        }
    }
}

/// The window `client` closed: its lane leases end, the accepted data playing again.
pub(super) fn gone(cp: &mut ControlPlane, client: &str, device: Option<&Device>) {
    let mut i = 0;
    while i < cp.lanes.live.len() {
        let l = &cp.lanes.live[i];
        if !l.closing && l.client.as_deref() == Some(client) {
            end(cp, i, "The window that owned this drag closed.", device);
        } else {
            i += 1;
        }
    }
}

/// The project closes: every lane lease ends. A fresh engine plays what opens next.
pub(super) fn end_all(cp: &mut ControlPlane) {
    while !cp.lanes.live.is_empty() {
        cp.lanes.end(0, "The song was closed.");
    }
}

impl Graph {
    /// Begin a drag on clips (WRITE-22a): the gesture checked on the accepted revision as the control
    /// plane published it, never waiting for the session, then heard as `update` hears it. A
    /// gesture that isn't previewed (an audio lane's, or one that isn't a drag) begins nothing, and
    /// is written on release as before. Changed: a clip it names changed since the window drew it.
    pub fn lane_begin(
        &self,
        token: u64,
        gesture: Gesture,
        client: Option<String>,
        ttl: Option<Duration>,
    ) -> Result<Result<LaneBegun, String>> {
        super::busy::no_lease(&crate::caller::now())?;
        let engine = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            p.engine.clone()
        };
        let rev = &engine.accepted;
        let lanes: Vec<Value> = named(&gesture).iter().map(lane_json).collect();
        let pinned = match clips::pinned(rev, &gesture) {
            Ok(g) => g,
            Err(EditError::Changed(f)) => return Ok(Err(EditError::Changed(f).to_string())),
            Err(e) => bail!("{e}"),
        };
        if unpreviewable(rev, &pinned).is_some() {
            return Ok(Ok(LaneBegun { lease: None, previewable: false, lanes, heard: false, revision: rev.number }));
        }
        let tx_id = m2_song::tx::random_id();
        if let Err(e) = previewed(rev, &pinned, &tx_id) {
            return match e {
                EditError::Changed(_) => Ok(Err(e.to_string())),
                e => Err(anyhow!("{e}")),
            };
        }
        let heard = engine.plays.as_ref().is_some_and(|p| {
            named(&pinned).iter().any(|a| p.info.lanes.iter().any(|s| s.lane.module == a.module.as_str() && s.lane.lane == a.lane.as_str()))
        });
        let id = {
            let mut cp = self.inner.control.lock();
            let id = cp.leases.next_id();
            let now = Instant::now();
            cp.lanes.live.push(LaneLease {
                id,
                token,
                client,
                ttl: ttl.unwrap_or(TTL).min(MAX_TTL),
                began: now,
                touched: now,
                tx_id,
                gesture: pinned.clone(),
                seq: 0,
                basis: engine.accepted.clone(),
                sent: vec![],
                closing: false,
            });
            id
        };
        self.lane_update(id, Some(1), pinned)?;
        Ok(Ok(LaneBegun { lease: Some(id), previewable: true, lanes, heard, revision: rev.number }))
    }

    /// Hear lease `lease`'s newest gesture: each lane it edits swapped into the renderer that plays
    /// (RENDER-26), from the control plane, never through the session (RENDER-29). An update older
    /// than one taken is dropped (Ok(None)); without `seq`, it follows the last. Returns the last
    /// engine message's number, None when nothing was sent.
    pub fn lane_update(&self, lease: u64, seq: Option<u64>, gesture: Gesture) -> Result<Option<u64>> {
        // Applied off the lock, on the revision published; again if a settle published another
        // meanwhile.
        for _ in 0..4 {
            let (engine, tx_id, seq, basis) = {
                let cp = self.inner.control.lock();
                let l = &cp.lanes.live[cp.lanes.index(lease)?];
                if l.closing {
                    bail!("this drag is being written");
                }
                let seq = match seq {
                    Some(s) if s <= l.seq => return Ok(None),
                    Some(s) => s,
                    None => l.seq + 1,
                };
                let p = cp.open.as_ref().filter(|p| p.engine.token == l.token).context("the song this drag was in is closed")?;
                (p.engine.clone(), l.tx_id.clone(), seq, l.basis.clone())
            };
            // A clip the window names without its hash is pinned to the one the lease saw.
            let pinned = clips::pinned(&basis, &gesture).map_err(|e| anyhow!("{e}"))?;
            let lanes = previewed(&engine.accepted, &pinned, &tx_id).map_err(|e| anyhow!("{e}"))?;
            let device = self.inner.current_device();
            let mut cp = self.inner.control.lock();
            if !cp.open.as_ref().is_some_and(|p| Arc::ptr_eq(&p.engine, &engine)) {
                continue;
            }
            let i = cp.lanes.index(lease)?;
            let l = &mut cp.lanes.live[i];
            if l.closing {
                bail!("this drag is being written");
            }
            if seq <= l.seq {
                return Ok(None);
            }
            let sent = send(l, &lanes, &engine.accepted, playing(engine.plays.as_ref(), device.as_ref()));
            (l.seq, l.gesture, l.basis, l.touched) = (seq, pinned, engine.accepted.clone(), Instant::now());
            return sent;
        }
        bail!("the song kept changing under this drag; try again")
    }

    /// Keep lease `lease` alive while the pointer holds still.
    pub fn lane_keepalive(&self, lease: u64) -> Result<()> {
        let mut cp = self.inner.control.lock();
        let i = cp.lanes.index(lease)?;
        cp.lanes.live[i].touched = Instant::now();
        Ok(())
    }

    /// End lease `lease` without a write: the accepted data plays again. Never waits for the
    /// session. During a commit, the commit decides.
    pub fn lane_cancel(&self, lease: u64) -> Result<()> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let Ok(i) = cp.lanes.index(lease) else { return Ok(()) };
        if cp.lanes.live[i].closing {
            bail!("this drag is being written; the write decides");
        }
        end(&mut cp, i, "This drag was cancelled.", device.as_ref());
        Ok(())
    }

    /// Write lease `lease`'s gesture (`gesture`, the window's last): one `Clips` Tx under the
    /// lease's Tx id, one undo step, answered as `POST /api/graph/clips` answers. Its preview is
    /// what the lease played, so nothing heard changes; refused, the accepted data plays again.
    pub fn lane_commit(&self, lease: u64, gesture: Gesture) -> Result<Answer> {
        let (token, tx_id, gesture) = {
            let mut cp = self.inner.control.lock();
            let i = cp.lanes.index(lease)?;
            let l = &mut cp.lanes.live[i];
            if l.closing {
                bail!("this drag is being written");
            }
            l.closing = true;
            let g = clips::pinned(&l.basis, &gesture).unwrap_or(gesture);
            (l.token, l.tx_id.clone(), g)
        };
        let done = self.clips_as(token, gesture, Some(tx_id));
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        if let Ok(i) = cp.lanes.index(lease) {
            let why = match &done {
                Ok(Answer::Done(_)) => "This drag was written.".to_string(),
                Ok(Answer::Changed(e)) => e.clone(),
                Err(e) => format!("{e:#}"),
            };
            end(&mut cp, i, why, device.as_ref());
        }
        done
    }

    /// The drags on clips in progress, as `GET /api/graph/leases` lists them beside the knobs'.
    pub fn lane_leases(&self) -> Vec<LaneLeaseInfo> {
        let now = Instant::now();
        let cp = self.inner.control.lock();
        cp.lanes
            .live
            .iter()
            .map(|l| LaneLeaseInfo {
                kind: "lane",
                lease: l.id,
                client: l.client.clone(),
                state: if l.closing { "committing" } else { "live" },
                lanes: named(&l.gesture).iter().map(lane_json).collect(),
                seq: l.seq,
                revision: l.basis.number,
                age_ms: now.duration_since(l.began).as_millis() as u64,
                idle_ms: now.duration_since(l.touched).as_millis() as u64,
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "lanes_tests.rs"]
mod tests;

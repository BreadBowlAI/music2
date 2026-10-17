//! Audition (MON-7, 8b.6): a window plays notes on a lane while you draw or click them.
//!
//! - **Sources:** each window (its WebSocket's `client`) is a source, numbered here. It names its
//!   own notes by handle; the engine keeps each source's apart.
//! - **The way in:** `{"module", "lane", "on": [{h, pitch, velocity}], "off": [h], "keep": [h]}`,
//!   over the window's WebSocket or `POST /api/graph/audition`. Offs go first, then Ons. The lane
//!   is found in the plan last sent, and the note sent to it as `Live`, under the control plane's
//!   lock, so it queues behind that plan's activation and reaches it. Never through the session
//!   (RENDER-29), so a compile never holds it up, and a read-only window plays too.
//! - **A lane nothing reads** is silent, and the answer says so, to that window only.
//! - **What ends a window's notes:** their Offs (a handle struck again is let go first); its
//!   WebSocket closing; or the watchdog, when it said nothing for [`QUIET`] (the window sends a
//!   keepalive every 2 s while a key is held). The last two let go of all its notes at once.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_runtime::LiveEv;
use serde::Deserialize;
use serde_json::{Value, json};

use super::lease::PlaysView;
use super::{ControlPlane, Device, Graph};

/// How long a window may hold notes without a word before they are let go.
pub const QUIET: Duration = Duration::from_secs(10);

/// What a window sends.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Audition {
    /// The lane the Ons play: needed with any On.
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub on: Vec<Strike>,
    /// Handles let go.
    #[serde(default)]
    pub off: Vec<u32>,
    /// Handles still held: a keepalive.
    #[serde(default)]
    pub keep: Vec<u32>,
}

/// A key struck: the window's handle for it, its pitch (0 to 127, fractions tune it) and its
/// velocity (0 to 1).
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Strike {
    pub h: u32,
    pub pitch: f64,
    #[serde(default = "Strike::velocity")]
    pub velocity: f64,
}

impl Strike {
    fn velocity() -> f64 {
        0.8
    }
}

/// The windows that play notes, on the control plane.
#[derive(Default)]
pub(super) struct Sources {
    by_client: HashMap<String, Source>,
    next: u32,
}

struct Source {
    id: u32,
    /// Each handle held, and the lane it sounds on, found again in each new plan.
    held: BTreeMap<u32, (String, String)>,
    /// When the window last said anything.
    heard: Instant,
}

/// Lane `(module, lane)`'s slot in what plays, if it is a notes lane there.
fn slot_of(plays: &PlaysView, module: &str, lane: &str) -> Option<u32> {
    let at = plays.info.lanes.iter().position(|l| l.lane.module == module && l.lane.lane == lane && l.caps.is_some())?;
    Some(at as u32)
}

/// What plays in `device`, if it is the player `plays` was sent to.
fn playing<'a>(plays: Option<&'a PlaysView>, device: Option<&'a Device>) -> Option<(&'a PlaysView, &'a Device)> {
    let (p, d) = (plays?, device?);
    (p.epoch == d.epoch).then_some((p, d))
}

/// Let go of every note `source` holds: one release, which reaches every lane.
fn release(plays: Option<&PlaysView>, device: Option<&Device>, source: &mut Source) {
    if std::mem::take(&mut source.held).is_empty() {
        return;
    }
    if let Some((p, d)) = playing(plays, device)
        && let Err(e) = d.player.live(p.generation, 0, LiveEv::Release { source: source.id })
    {
        eprintln!("music2: couldn't let go of a window's notes: {e:#}");
    }
}

impl Graph {
    /// Play or let go of notes from the window `client` (MON-7): Offs first, then Ons, on the plan
    /// last sent. Never waits for the session. Answers `{"t": "audition", "silent": …}` when the
    /// lane named plays nothing.
    pub fn audition(&self, client: &str, a: Audition) -> Result<Option<Value>> {
        for s in &a.on {
            if !(0.0..=127.0).contains(&s.pitch) {
                bail!("A note's pitch is a number from 0 to 127, not {}.", s.pitch);
            }
            if !(0.0..=1.0).contains(&s.velocity) {
                bail!("A note's velocity is a number from 0 to 1, not {}.", s.velocity);
            }
        }
        let lane = match (&a.module, &a.lane) {
            (Some(m), Some(l)) => Some((m.clone(), l.clone())),
            _ if a.on.is_empty() => None,
            _ => bail!("Say which lane plays the notes: its module and its lane."),
        };
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let ControlPlane { open, audition, .. } = &mut *cp;
        let open = open.as_ref().context("no project is open")?;
        let plays = playing(open.engine.plays.as_ref(), device.as_ref());
        let next = &mut audition.next;
        let source = audition.by_client.entry(client.to_string()).or_insert_with(|| {
            *next += 1;
            Source { id: *next, held: BTreeMap::new(), heard: Instant::now() }
        });
        source.heard = Instant::now();
        let id = source.id;
        // A handle struck again is let go first, wherever it sounds.
        for h in a.off.iter().chain(a.on.iter().map(|s| &s.h)) {
            let Some((m, l)) = source.held.remove(h) else { continue };
            if let Some((p, d)) = plays
                && let Some(slot) = slot_of(p, &m, &l)
            {
                d.player.live(p.generation, slot, LiveEv::Off { source: id, handle: *h })?;
            }
        }
        let Some((m, l)) = lane.filter(|_| !a.on.is_empty()) else { return Ok(None) };
        let Some((p, d, slot)) = plays.and_then(|(p, d)| Some((p, d, slot_of(p, &m, &l)?))) else {
            return Ok(Some(json!({"t": "audition", "module": m, "lane": l, "silent": "Nothing plays this lane"})));
        };
        for s in &a.on {
            let ev = LiveEv::On { source: id, handle: s.h, pitch: s.pitch as f32, velocity: s.velocity as f32 };
            d.player.live(p.generation, slot, ev)?;
            source.held.insert(s.h, (m.clone(), l.clone()));
        }
        Ok(None)
    }

    /// The window `client` closed: every note it holds is let go.
    pub(super) fn audition_gone(&self, client: &str) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let ControlPlane { open, audition, .. } = &mut *cp;
        if let Some(mut s) = audition.by_client.remove(client) {
            release(open.as_ref().and_then(|o| o.engine.plays.as_ref()), device.as_ref(), &mut s);
        }
    }

    /// The watchdog: let go of the notes of each window that held some and said nothing for
    /// [`QUIET`]; then it is forgotten. The server's `music2-tell` thread calls it every 300 ms.
    pub fn expire_auditions(&self) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let ControlPlane { open, audition, .. } = &mut *cp;
        let plays = open.as_ref().and_then(|o| o.engine.plays.as_ref());
        let now = Instant::now();
        audition.by_client.retain(|_, s| {
            let quiet = now.duration_since(s.heard) > QUIET;
            if quiet {
                release(plays, device.as_ref(), s);
            }
            !quiet
        });
    }
}

#[cfg(test)]
#[path = "audition_tests.rs"]
mod tests;

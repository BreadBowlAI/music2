//! Meters, loudness and the DSP load (8b.8, TAP-4 to TAP-6, TAP-8), for every window, about 30
//! times a second while the engine runs.
//!
//! - **The message:** `{"t": "meters", "tracks": {"kick": [l, r], …}, "channels": {"kick": [l,
//!   r], …}, "out": [l, r], "loudness": {"short": -14.2, "true_peak": -1.3}, "scopes": [{"module",
//!   "node", "frames": [[…], […]]}], "dsp": {"load": 0.23, "dropouts": 0}}`. Peaks are linear (1.0
//!   is 0 dBFS), each channel's largest `|x|` since the last message. A channel of the song's std
//!   mixer is read at its fader, after its pan and gain (TAP-5); a track at its channel's when the
//!   channel is its own, else where its audio enters the song, after mute and solo, before any
//!   fader (its overlay sites, the louder). `out` is the song's output. A track may be called
//!   `out` (ID-1), so the output has a field of its own. `loudness` is the output's (TAP-6):
//!   `short` the last 3 s in LUFS, `true_peak` the largest of the last 3 s in dBTP, each null
//!   until there is one. `scopes` are the last [`super::scope::SCOPE`] frames of each scope asked
//!   (TAP-8), a list per channel. While a take records on an audio lane, `"input": [x]` is the
//!   sound input's loudest sample since the last message, both sides (the io plan's slice 3: the
//!   take's level bar), read from the take's ring at a place of its own.
//! - **When:** the server's `music2-meters` thread calls [`Graph::meters`] every 33 ms and sends what
//!   it gives on a `watch` channel, never the event broadcast (M1): a window that falls behind skips
//!   to the latest meters, and loses no `graph` event for them. Nothing is sent while no window
//!   listens (TAP-4: the taps are off), while no project plays, nor while the engine idles
//!   (RENDER-6a), but one last message as it starts to, so the meters fall to the silence it ended
//!   in. Nothing either while a new plan waits to be taken: its taps read nothing yet.
//! - **The load:** the longest callback of the last second (the last 30 reads), as a share of the
//!   device's period (Q3). `dropouts` are the device's xruns and the late callbacks since the
//!   player started: a window lights its warning when the count grows. The meters thread is the
//!   only reader of [`m2_play::Player::load`] (M3); the status reads its figure ([`Meters::load`]).

use std::collections::VecDeque;
use std::sync::Arc;

use m2_editor::listen::Meter;
use m2_editor::mix::{self, Mixing};
use m2_play::Load;
use m2_play::ring::Ring;
use m2_runtime::{RING, TapAt, TapSlots};
use m2_session::Revision;
use m2_song::ROOT;
use serde_json::{Map, Value, json};

use super::{ControlPlane, Graph};

/// Reads kept for the load: a second's, at 30 a second.
const LOADS: usize = 30;
/// The input's frames a level reads at most: the latest.
const LEVEL: u64 = 4800;
/// The ring that copies the song's output for its loudness, while the meters are watched (TAP-6).
pub(super) const LOUD: usize = 0;
/// The true peak is the largest of this many seconds, as the short-term loudness is.
const WINDOW: f64 = 3.0;

/// Where a plan's meters read (TAP-5): each channel's and each track's meter, by its place among
/// the meters [`m2_runtime::Renderer::tap`] was given.
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Each channel of the song's one std mixer, by key.
    channels: Vec<(String, usize)>,
    /// Each track something reads, by key: its channel's meter when the channel is its own.
    tracks: Vec<(String, usize)>,
}

/// The meters of `rev`'s plans (TAP-5): each channel of its one std mixer at its fader, then each
/// track whose channel isn't its own at its overlay sites, the louder of them. What to tap, and
/// whose each meter is.
pub(crate) fn layout(rev: &Revision) -> (Vec<Vec<TapAt>>, Layout) {
    let flat = &rev.compiled.flat;
    let (mut taps, mut layout) = (vec![], Layout::default());
    let mixer = match mix::mixer(rev) {
        Mixing::One(m) => Some(m),
        _ => None,
    };
    if let Some(mx) = &mixer {
        for c in &mx.channels {
            if let Some(fader) = mix::fader(rev, mx, &c.key) {
                layout.channels.push((c.key.clone(), taps.len()));
                taps.push(vec![TapAt::Node(fader)]);
            }
        }
    }
    for m in rev.compiled.ir.modules.iter().filter(|m| m.track && m.key != ROOT) {
        let own = mixer
            .as_ref()
            .and_then(|mx| mx.channel_of(&m.key))
            .filter(|c| c.tracks.len() == 1)
            .and_then(|c| layout.channels.iter().find(|x| x.0 == c.key))
            .map(|x| x.1);
        let place = match own {
            Some(at) => at,
            None => {
                let sites: Vec<TapAt> = (0..flat.overlays.len()).filter(|&k| flat.overlays[k].module == m.key).map(TapAt::Site).collect();
                if sites.is_empty() {
                    // Nothing reads it: it has no meter (Q21).
                    continue;
                }
                taps.push(sites);
                taps.len() - 1
            }
        };
        layout.tracks.push((m.key.clone(), place));
    }
    (taps, layout)
}

/// The output's loudness, worked out from its ring off the audio thread (TAP-6).
struct Loud {
    meter: Meter,
    /// The ring read (by its taps' address) and its next frame.
    at: (usize, u64),
    /// The true peak of each read in the last [`WINDOW`]: its frames and its peak, linear.
    peaks: VecDeque<(u64, f64)>,
    frames: Vec<[f32; 2]>,
    sides: [Vec<f32>; 2],
}

impl Loud {
    fn new(rate: u32) -> Loud {
        Loud { meter: Meter::recent(rate), at: (0, 0), peaks: VecDeque::new(), frames: vec![], sides: [vec![], vec![]] }
    }

    /// Read the output's frames since the last read, and say the loudness of the last 3 s and the
    /// largest true peak in them.
    fn read(&mut self, taps: &Arc<TapSlots>, rate: u32) -> Value {
        let id = Arc::as_ptr(taps) as usize;
        let from = if self.at.0 == id { self.at.1 } else { 0 };
        if let Some(first) = taps.frames(LOUD, from, RING, &mut self.frames) {
            self.at = (id, first + self.frames.len() as u64);
            let [l, r] = &mut self.sides;
            (l.clear(), r.clear());
            l.extend(self.frames.iter().map(|f| f[0]));
            r.extend(self.frames.iter().map(|f| f[1]));
            self.meter.push(l, r);
            let peak = self.meter.take_true_peak().map_or(0.0, |db| 10f64.powf(db / 20.0));
            self.peaks.push_back((self.frames.len() as u64, peak));
            // The reads of the last 3 s, the newest kept whatever its length.
            let window = (WINDOW * rate as f64) as u64;
            while self.peaks.len() > 1 && self.peaks.iter().skip(1).map(|p| p.0).sum::<u64>() >= window {
                self.peaks.pop_front();
            }
        }
        let top = self.peaks.iter().fold(0.0f64, |m, p| m.max(p.1));
        let db = |x: Option<f64>| x.map(|x| (x * 100.0).round() / 100.0);
        json!({"short": db(self.meter.short_term()), "true_peak": db((top > 0.0).then(|| 20.0 * top.log10()))})
    }
}

/// What the meters keep between reads, on the control plane.
#[derive(Default)]
pub(super) struct Meters {
    /// The player read from, and its last loads.
    epoch: u64,
    loads: VecDeque<f32>,
    late: u64,
    /// The engine idled at the last read: its last message was sent.
    idle: bool,
    /// Where the input's level was last read: its ring (by address) and the frame.
    input_at: (usize, u64),
    /// A window listens for meters, so the taps run (TAP-4).
    pub(super) watched: bool,
    /// The taps were turned on since the last read: they have heard no block yet, so that read
    /// sends nothing rather than silence.
    started: bool,
    /// The output's loudness, while the meters are watched and the engine runs.
    loud: Option<Loud>,
}

impl Meters {
    /// The longest callback of the last second, as a share of the device's period.
    pub(super) fn load(&self) -> f32 {
        self.loads.iter().fold(0.0, |m, &x| m.max(x))
    }

    /// Callbacks that took longer than their period, since the player started.
    pub(super) fn late(&self) -> u64 {
        self.late
    }

    /// The input's loudest sample since the last read, of its latest [`LEVEL`] frames.
    fn input_level(&mut self, ring: &Ring) -> f32 {
        let (id, head) = (ring as *const Ring as usize, ring.head());
        let from = if self.input_at.0 == id { self.input_at.1 } else { 0 }.max(head.saturating_sub(LEVEL));
        self.input_at = (id, head);
        let mut frames = vec![[0.0f32; 2]; (head - from.min(head)) as usize];
        let got = ring.read(from, &mut frames);
        frames[..got.frames].iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
    }

    fn read(&mut self, epoch: u64, l: Load) {
        if epoch != self.epoch {
            *self = Meters { epoch, watched: self.watched, ..Meters::default() };
        }
        if self.loads.len() == LOADS {
            self.loads.pop_front();
        }
        self.loads.push_back(l.longest);
        self.late = l.late;
    }
}

/// Watch `taps` as the meters are watched: its peaks, and the output's ring for its loudness
/// (TAP-4, TAP-6). What every new plan is given before it is sent (TAP-9).
pub(super) fn watch(taps: &TapSlots, on: bool) {
    taps.watch(on);
    // The output is always a point.
    let _ = taps.point(LOUD, on.then_some(TapAt::Out));
}

/// `x` to a millionth (−120 dB), so the message stays short.
fn short(x: f32) -> f64 {
    (x as f64 * 1e6).round() / 1e6
}

/// Each meter of `places`, by key, from `peaks`.
fn by_key(places: &[(String, usize)], peaks: &[Vec<f32>]) -> Map<String, Value> {
    places.iter().filter_map(|(k, at)| Some((k.clone(), json!(peaks.get(*at)?.iter().map(|&x| short(x)).collect::<Vec<_>>())))).collect()
}

impl Graph {
    /// Whether a window listens for meters (TAP-4): the taps of what plays run only then. The
    /// meters thread says so at each read; every new plan is given it before it is sent.
    pub fn watch_meters(&self, on: bool) {
        let mut cp = self.inner.control.lock();
        if on && !std::mem::replace(&mut cp.meters.watched, on) {
            cp.meters.started = true;
        }
        if !on {
            (cp.meters.watched, cp.meters.loud) = (false, None);
        }
        if let Some(taps) = cp.open.as_ref().and_then(|o| o.engine.plays.as_ref()).and_then(|p| p.info.taps.as_ref()) {
            watch(taps, on);
        }
    }

    /// The meters message, if one is due: a window watches, a project plays, and the engine runs
    /// or has just begun to idle. Reads and resets the taps of the plan that plays, reads its rings
    /// (the output's loudness, each scope), ends the scopes no one asked for in 10 s, and reads the
    /// load.
    pub fn meters(&self) -> Option<Value> {
        let device = self.inner.current_device();
        let (token, peaks, layout, loudness, shown, load, dropouts, input) = {
            let mut cp = self.inner.control.lock();
            let ControlPlane { open, meters, record, scopes, .. } = &mut *cp;
            let (Some(open), Some(device)) = (open.as_ref(), device) else {
                *meters = Meters { watched: meters.watched, ..Meters::default() };
                return None;
            };
            let player = &device.player;
            meters.read(device.epoch, player.load());
            let plays = open.engine.plays.as_ref().filter(|p| p.epoch == device.epoch);
            scopes.expire(std::time::Instant::now(), plays.and_then(|p| p.info.taps.as_deref()));
            if !meters.watched || std::mem::take(&mut meters.started) {
                return None;
            }
            let idle = player.idle();
            if std::mem::replace(&mut meters.idle, idle) && idle {
                // The loudness starts again with the next sound.
                meters.loud = None;
                return None;
            }
            let plays = plays?;
            if player.status().generation != plays.generation {
                return None;
            }
            let taps = plays.info.taps.as_ref()?;
            let rate = plays.info.rate;
            let loudness = meters.loud.get_or_insert_with(|| Loud::new(rate)).read(taps, rate);
            let input = record.input().map(|ring| meters.input_level(ring));
            let dropouts = player.device_errors().xruns + meters.late;
            (open.engine.token, taps.take(), plays.meters.clone(), loudness, scopes.frames(taps), meters.load(), dropouts, input)
        };
        let mut m = json!({
            "t": "meters",
            "session": token,
            "tracks": by_key(&layout.tracks, &peaks.meters),
            "channels": by_key(&layout.channels, &peaks.meters),
            "out": peaks.out.map(short),
            "loudness": loudness,
            "dsp": {"load": (load as f64 * 1e4).round() / 1e4, "dropouts": dropouts},
        });
        if !shown.is_empty() {
            m["scopes"] = shown.into_iter().map(|s| s.json()).collect();
        }
        if let Some(x) = input {
            m["input"] = json!([short(x)]);
        }
        Some(m)
    }
}

#[cfg(test)]
#[path = "meters_tests.rs"]
mod tests;

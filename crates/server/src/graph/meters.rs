//! Meters, loudness and the DSP load (8b.8, TAP-4 to TAP-6, TAP-8), for every socket that listens
//! for meters, about 30 times a second while the engine runs.
//!
//! - **The message:** `{"t": "meters", "tracks": {"kick": [l, r], …}, "channels": {"kick": [l,
//!   r], …}, "out": [l, r], "loudness": {"short": -14.2, "true_peak": -1.3}, "dsp": {"load": 0.23,
//!   "dropouts": 0}}`. Peaks are linear (1.0 is 0 dBFS), each channel's largest `|x|` since the
//!   last message. A channel of the song's std mixer is read at its fader, after its pan and gain
//!   (TAP-5); a track at its channel's when the channel is its own, else where its audio enters the
//!   song, after mute and solo, before any fader (its overlay sites, the louder). `out` is the
//!   song's output. A track may be called `out` (ID-1), so the output has a field of its own.
//!   `loudness`, while a client asks for it (TAP-6), is the output's: `short` the last 3 s in LUFS,
//!   `true_peak` the largest of the last 3 s in dBTP, each null until there is one. While a take
//!   records on an audio lane, `"input": [x]` is the sound input's loudest sample since the last
//!   message, both sides (the io plan's slice 3: the take's level bar), read from the take's ring
//!   at a place of its own.
//! - **Each window's scopes:** `{"t": "scopes", "scopes": [{"module", "node", "frames": [[…],
//!   …]}]}`, the last [`super::scope::SCOPE`] frames of each scope it asked for, a list per channel
//!   (`why` in place of `frames` while it can't show), to that window's socket alone (TAP-8).
//! - **When:** the server's `music2-meters` thread calls [`Graph::meters`] every 33 ms and sends what
//!   it gives on a `watch` channel, never the event broadcast (M1): a window that falls behind skips
//!   to the latest meters, and loses no `graph` event for them. Nothing is sent while no socket
//!   listens (TAP-4: the taps are off; a window's always listens, an agent's when it asks), while no
//!   project plays, nor while the engine idles (RENDER-6a), but one last message as it starts to,
//!   so the meters fall to the silence it ended in. Nothing either while a new plan waits to be
//!   taken: its taps read nothing yet.
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
/// The ring that copies the song's output for its loudness, while a client asks for it (TAP-6).
pub(super) const LOUD: usize = 0;
/// The true peak is the largest of this many seconds, as the short-term loudness is.
const WINDOW: f64 = 3.0;

/// Where a plan's meters read (TAP-5): each channel's and each track's meter, by its place among
/// the meters [`m2_runtime::Renderer::tap`] was given, and why they read before the faders when
/// they couldn't be laid out after them.
#[derive(Debug, Default)]
pub(crate) struct Layout {
    /// Each channel of the song's one std mixer, by key.
    channels: Vec<(String, usize)>,
    /// Each track something reads, by key: its channel's meter when the channel is its own.
    tracks: Vec<(String, usize)>,
    /// What the status says while the meters read before the faders though the song has them.
    pub(super) why: Option<String>,
}

/// Songs whose meters can't be laid out after the faders, by folder: the tests' way to make the
/// refusal a bug would be.
#[cfg(test)]
pub(super) static BREAK: parking_lot::Mutex<Vec<std::path::PathBuf>> = parking_lot::Mutex::new(vec![]);

/// The meters of `rev`'s plans (TAP-5): each channel of its one std mixer at its fader, then each
/// track whose channel isn't its own at its overlay sites, the louder of them. With `faders` false,
/// every track at its sites: what can't fail. What to tap, and whose each meter is.
fn layout(rev: &Revision, faders: bool) -> (Vec<Vec<TapAt>>, Layout) {
    let flat = &rev.compiled.flat;
    let (mut taps, mut layout) = (vec![], Layout::default());
    let mixer = match mix::mixer(rev) {
        Mixing::One(m) if faders => Some(m),
        _ => None,
    };
    if let Some(mx) = &mixer {
        for c in &mx.channels {
            if let Some(fader) = mix::fader(rev, mx, &c.key) {
                layout.channels.push((c.key.clone(), taps.len()));
                taps.push(vec![TapAt::Node(fader)]);
            }
        }
        #[cfg(test)]
        if BREAK.lock().contains(&rev.project.dir) {
            taps.push(vec![TapAt::Node(usize::MAX)]);
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

/// Lay out the meters of `r`, a plan of `rev` (TAP-5): after the faders; refused, which is a bug,
/// each track before its fader, which can't fail, and the layout says why for the status.
pub(super) fn lay_out(r: &mut m2_runtime::Renderer, rev: &Revision) -> Arc<Layout> {
    let (taps, layout) = self::layout(rev, true);
    let Err(e) = r.tap(&taps) else { return Arc::new(layout) };
    let (taps, mut layout) = self::layout(rev, false);
    layout.why = Some(format!(
        "The meters show each track before its fader for now: music2 couldn't place them after the faders, so a fader \
         doesn't move its meter. Please report this. (In detail: {e:#})"
    ));
    if let Err(e) = r.tap(&taps) {
        layout = Layout {
            why: Some(format!("The meters show nothing for now: music2 couldn't place them. Please report this. (In detail: {e:#})")),
            ..Layout::default()
        };
    }
    Arc::new(layout)
}

/// The song's loudness, worked out from the output's ring off the audio thread (TAP-6).
pub(super) struct Loud {
    pub(super) meter: Meter,
    /// The rate it measures at: the song's.
    rate: u32,
    /// The plan whose ring it reads (by its id: unique, never reused) and its next frame there.
    at: (u64, u64),
    /// The true peak of each read in the last [`WINDOW`]: its frames and its peak, linear.
    peaks: VecDeque<(u64, f64)>,
    frames: Vec<[f32; 2]>,
    sides: [Vec<f32>; 2],
}

impl Loud {
    fn new(rate: u32) -> Loud {
        Loud { meter: Meter::recent(rate), rate, at: (0, 0), peaks: VecDeque::new(), frames: vec![], sides: [vec![], vec![]] }
    }

    /// Read the output's frames of plan `plan` since the last read, and say the loudness of the
    /// last 3 s and the largest true peak in them.
    fn read(&mut self, taps: &TapSlots, plan: u64) -> Value {
        let from = if self.at.0 == plan { self.at.1 } else { 0 };
        if let Some(first) = taps.frames(LOUD, from, RING, &mut self.frames) {
            self.at = (plan, first + self.frames.len() as u64);
            let [l, r] = &mut self.sides;
            l.clear();
            r.clear();
            l.extend(self.frames.iter().map(|f| f[0]));
            r.extend(self.frames.iter().map(|f| f[1]));
            self.meter.push(l, r);
            let peak = self.meter.take_true_peak().map_or(0.0, |db| 10f64.powf(db / 20.0));
            self.peaks.push_back((self.frames.len() as u64, peak));
            // The reads of the last 3 s, the newest kept whatever its length.
            let window = (WINDOW * self.rate as f64) as u64;
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
    /// A socket listens for meters, so the meters' taps run (TAP-4).
    pub(super) watched: bool,
    /// The taps were turned on since the last read: they have heard no block yet, so that read
    /// sends nothing rather than silence.
    started: bool,
    /// The song's loudness, while a client asks for it and the engine runs, and what it last said.
    pub(super) loud: Option<Loud>,
    said: Option<Value>,
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

    /// The song's loudness as last measured: `{short, true_peak}`, each null until there is one.
    pub(super) fn loudness(&self) -> Value {
        self.said.clone().unwrap_or_else(|| json!({"short": null, "true_peak": null}))
    }

    /// The input's loudest sample since the last read, of its latest [`LEVEL`] frames.
    /// Through `channels`: one channel alone reads only it (MON-13).
    fn input_level(&mut self, ring: &Ring, channels: m2_play::Channels) -> f32 {
        let (id, head) = (ring as *const Ring as usize, ring.head());
        let from = if self.input_at.0 == id { self.input_at.1 } else { 0 }.max(head.saturating_sub(LEVEL));
        self.input_at = (id, head);
        let mut frames = vec![[0.0f32; 2]; (head - from.min(head)) as usize];
        let got = ring.read(from, &mut frames);
        frames[..got.frames].iter().map(|f| channels.pick(*f)).fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
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

    /// The loudness of plan `plan` at `rate`, from its ring: the meter is made anew for another
    /// rate (a song's rate changed), and goes on across plans of the same one.
    fn hear(&mut self, taps: &TapSlots, plan: u64, rate: u32) {
        let loud = match &mut self.loud {
            Some(l) if l.rate == rate => l,
            l => l.insert(Loud::new(rate)),
        };
        self.said = Some(loud.read(taps, plan));
    }

    /// No loudness: no one asks, or the engine went quiet; it starts again from nothing.
    fn quiet(&mut self) {
        (self.loud, self.said) = (None, None);
    }
}

/// What a meters read sends (TAP-4, TAP-8): the message every listener gets, and each window's
/// own, `{"t": "scopes", "scopes": […]}`, for the windows that asked for scopes.
#[derive(Debug)]
pub struct MetersOut {
    pub all: Value,
    pub scopes: Vec<(String, Value)>,
}

/// Watch `taps`'s meters as they are watched: their peaks are raised only then (TAP-4). What every
/// new plan is given before it is sent (TAP-9).
pub(super) fn watch(taps: &TapSlots, on: bool) {
    taps.watch(on);
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
    /// Whether a socket listens for meters (TAP-4): the meters of what plays run only then. The
    /// meters thread says so at each read; every new plan is given it before it is sent.
    pub fn watch_meters(&self, on: bool) {
        let mut cp = self.inner.control.lock();
        if on && !std::mem::replace(&mut cp.meters.watched, on) {
            cp.meters.started = true;
        }
        cp.meters.watched = on;
        if let Some(taps) = cp.open.as_ref().and_then(|o| o.engine.plays.as_ref()).and_then(|p| p.info.taps.as_ref()) {
            watch(taps, on);
        }
    }

    /// The meters, if they are due: a socket listens, a project plays, and the engine runs or has
    /// just begun to idle. Reads and resets the meters of the plan that plays, reads its rings (the
    /// song's loudness, while asked for, and each scope), ends what no one asked for in 10 s, and
    /// reads the load. The loudness is measured while asked for, whoever listens.
    pub fn meters(&self) -> Option<MetersOut> {
        let device = self.inner.current_device();
        let (token, peaks, layout, loudness, shown, load, dropouts, input) = {
            let mut cp = self.inner.control.lock();
            let ControlPlane { open, meters, record, asks, .. } = &mut *cp;
            let (Some(open), Some(device)) = (open.as_ref(), device) else {
                *meters = Meters { watched: meters.watched, ..Meters::default() };
                return None;
            };
            let player = &device.player;
            meters.read(device.epoch, player.load());
            let plays = open.engine.plays.as_ref().filter(|p| p.epoch == device.epoch);
            asks.expire(std::time::Instant::now(), plays.and_then(|p| p.info.taps.as_deref()));
            let idle = player.idle();
            // The loudness, while asked for, whoever listens (TAP-6): from the plan the engine plays;
            // while it idles, nothing (it starts again with the next sound).
            let current = plays.filter(|p| player.status().generation == p.generation);
            match (asks.loudness(), current.and_then(|p| Some((p.info.taps.as_ref()?, p)))) {
                (true, Some((taps, p))) if !idle => meters.hear(taps, p.info.id, p.info.rate),
                (true, Some(_)) => meters.quiet(),
                (true, None) => {}
                (false, _) => meters.quiet(),
            }
            // The first read since the meters' taps started reads what they heard of no block.
            if !meters.watched || std::mem::take(&mut meters.started) {
                return None;
            }
            if std::mem::replace(&mut meters.idle, idle) && idle {
                return None;
            }
            let plays = current?;
            let taps = plays.info.taps.as_ref()?;
            let loudness = asks.loudness().then(|| meters.loudness());
            let input = record.input().map(|(ring, ch)| meters.input_level(ring, ch));
            let dropouts = player.device_errors().xruns + meters.late;
            (open.engine.token, taps.take(), plays.meters.clone(), loudness, asks.frames(taps), meters.load(), dropouts, input)
        };
        let mut all = json!({
            "t": "meters",
            "session": token,
            "tracks": by_key(&layout.tracks, &peaks.meters),
            "channels": by_key(&layout.channels, &peaks.meters),
            "out": peaks.out.map(short),
            "dsp": {"load": (load as f64 * 1e4).round() / 1e4, "dropouts": dropouts},
        });
        if let Some(l) = loudness {
            all["loudness"] = l;
        }
        if let Some(x) = input {
            all["input"] = json!([short(x)]);
        }
        // Each window's scopes, to it alone.
        let mut windows: Vec<(String, Vec<Value>)> = vec![];
        for s in &shown {
            let v = s.json();
            for c in &s.clients {
                match windows.iter_mut().find(|w| &w.0 == c) {
                    Some(w) => w.1.push(v.clone()),
                    None => windows.push((c.clone(), vec![v.clone()])),
                }
            }
        }
        let scopes = windows.into_iter().map(|(c, list)| (c, json!({"t": "scopes", "session": token, "scopes": list}))).collect();
        Some(MetersOut { all, scopes })
    }
}

#[cfg(test)]
#[path = "meters_tests.rs"]
mod tests;

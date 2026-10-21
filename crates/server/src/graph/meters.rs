//! Meters and the DSP load (8b.8), for every window, about 30 times a second while the engine runs.
//!
//! - **The message:** `{"t": "meters", "tracks": {"kick": [l, r], …}, "out": [l, r], "dsp": {"load":
//!   0.23, "dropouts": 0}}`. Peaks are linear (1.0 is 0 dBFS), each channel's largest `|x|` since
//!   the last message. A track's is read where its audio enters another module, after mute and
//!   solo, before what it feeds (Q2): its overlay. `out` is the song's output. A track may be called
//!   `out` (ID-1), so the output has a field of its own. While a take records on an audio lane,
//!   `"input": [x]` is the sound input's loudest sample since the last message, both sides (the io
//!   plan's slice 3: the take's level bar), read from the take's ring at a place of its own.
//! - **When:** the server's `music2-meters` thread calls [`Graph::meters`] every 33 ms and sends what
//!   it gives on a `watch` channel, never the event broadcast (M1): a window that falls behind skips
//!   to the latest meters, and loses no `graph` event for them. Nothing is sent while no project
//!   plays, nor while the engine idles (RENDER-6a), but one last message as it starts to, so the
//!   meters fall to the silence it ended in. Nothing either while a new plan waits to be taken: its
//!   taps read nothing yet.
//! - **The load:** the longest callback of the last second (the last 30 reads), as a share of the
//!   device's period (Q3). `dropouts` are the device's xruns and the late callbacks since the
//!   player started: a window lights its warning when the count grows. The meters thread is the
//!   only reader of [`m2_play::Player::load`] (M3); the status reads its figure ([`Meters::load`]).

use std::collections::VecDeque;

use m2_play::Load;
use m2_play::ring::Ring;
use serde_json::{Map, Value, json};

use super::{ControlPlane, Graph};

/// Reads kept for the load: a second's, at 30 a second.
const LOADS: usize = 30;
/// The input's frames a level reads at most: the latest.
const LEVEL: u64 = 4800;

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
            *self = Meters { epoch, ..Meters::default() };
        }
        if self.loads.len() == LOADS {
            self.loads.pop_front();
        }
        self.loads.push_back(l.longest);
        self.late = l.late;
    }
}

/// `x` to a millionth (−120 dB), so the message stays short.
fn short(x: f32) -> f64 {
    (x as f64 * 1e6).round() / 1e6
}

impl Graph {
    /// The meters message, if one is due: a project plays, and the engine runs or has just begun to
    /// idle. Reads and resets the taps of the plan that plays, and reads the load.
    pub fn meters(&self) -> Option<Value> {
        let device = self.inner.current_device();
        let (peaks, load, dropouts, input) = {
            let mut cp = self.inner.control.lock();
            let ControlPlane { open, meters, record, .. } = &mut *cp;
            let (Some(open), Some(device)) = (open.as_ref(), device) else {
                *meters = Meters::default();
                return None;
            };
            let player = &device.player;
            meters.read(device.epoch, player.load());
            let idle = player.idle();
            if std::mem::replace(&mut meters.idle, idle) && idle {
                return None;
            }
            let plays = open.engine.plays.as_ref().filter(|p| p.epoch == device.epoch)?;
            if player.status().generation != plays.generation {
                return None;
            }
            let input = record.input().map(|ring| meters.input_level(ring));
            (plays.info.taps.as_ref()?.take(), meters.load(), player.device_errors().xruns + meters.late, input)
        };
        let tracks: Map<String, Value> =
            peaks.tracks.iter().map(|(k, p)| (k.clone(), json!(p.iter().map(|&x| short(x)).collect::<Vec<_>>()))).collect();
        let mut m = json!({
            "t": "meters",
            "tracks": tracks,
            "out": peaks.out.map(short),
            "dsp": {"load": (load as f64 * 1e4).round() / 1e4, "dropouts": dropouts},
        });
        if let Some(x) = input {
            m["input"] = json!([short(x)]);
        }
        Some(m)
    }
}

#[cfg(test)]
#[path = "meters_tests.rs"]
mod tests;

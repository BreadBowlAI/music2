//! The transport clock (RENDER-6, RENDER-9a): the song position, in integer samples at the
//! project's rate. Beats are derived from it through the tempo map, and a loop's frames are
//! computed from its beats, never by adding up lap lengths, so a loop never drifts off the grid.
//!
//! Each renderer owns one. Commands land at the next block line, in order; each block is then
//! one or two [`Run`]s of transport time: a wrap falls at its exact frame, mid-block if need be.
//! Offline the transport plays from 0 and never loops, so a block is always one run whose
//! frames are the block's own.

use m2_song::TempoMap;

use crate::BLOCK;

/// Why a run starts somewhere other than where the last one ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    None,
    /// Playing after being stopped.
    Start,
    /// The first block of a renderer just activated (RENDER-25a).
    Enter,
    Seek,
    /// A loop wrapped.
    Wrap,
    /// Stopped after playing.
    Stop,
}

/// A stretch of a block that plays transport frames `at..at + n` (or holds `at`, stopped) in
/// the block's frames `off..off + n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub off: u32,
    pub n: u32,
    pub at: u64,
    pub moving: bool,
    pub jump: Jump,
}

impl Run {
    const EMPTY: Run = Run { off: 0, n: 0, at: 0, moving: false, jump: Jump::None };

    /// The transport frame of the block's frame `f`, which is in this run.
    pub fn frame(&self, f: u32) -> u64 {
        if self.moving { self.at + (f - self.off) as u64 } else { self.at }
    }
}

/// A change at frame 0 and at most one wrap: loops are at least two blocks long.
pub const MAX_RUNS: usize = 2;

/// A block's runs, in order, covering its frames exactly.
#[derive(Clone, Copy, Debug)]
pub struct Runs {
    runs: [Run; MAX_RUNS],
    len: usize,
}

impl Runs {
    pub fn one(r: Run) -> Runs {
        Runs { runs: [r, Run::EMPTY], len: 1 }
    }

    /// False when full; callers never push more than [`MAX_RUNS`].
    pub fn push(&mut self, r: Run) -> bool {
        if self.len == MAX_RUNS {
            return false;
        }
        self.runs[self.len] = r;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[Run] {
        &self.runs[..self.len]
    }

    pub fn first(&self) -> &Run {
        &self.runs[0]
    }

    /// The run holding the block's frame `f`.
    pub fn at(&self, f: u32) -> &Run {
        self.as_slice().iter().rev().find(|r| r.off <= f).unwrap_or(&self.runs[0])
    }

    /// The transport frame of the block's frame `f`.
    pub fn frame(&self, f: u32) -> u64 {
        self.at(f).frame(f)
    }

    /// The same runs `by` frames later in the transport (earlier when negative, never below 0).
    pub(crate) fn shifted(&self, by: i64) -> Runs {
        let mut r = *self;
        for run in &mut r.runs[..r.len] {
            run.at = run.at.saturating_add_signed(by);
        }
        r
    }
}

/// A transport command. Seek targets and loops are beats, so a command means the same in any
/// tempo map: the one the engine holds when it applies it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    Play,
    Stop,
    Seek(f64),
    Loop(Option<(f64, f64)>),
}

/// A loop: its beats, and their frames in the map it was last validated in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loop {
    pub start_beat: f64,
    pub end_beat: f64,
    start: u64,
    end: u64,
}

impl Loop {
    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.end
    }
}

/// The frame of `beat`: the formula notes use, so loop ends land on the notes' grid.
pub fn frame_of(tempo: &TempoMap, beat: f64, rate: f64) -> u64 {
    (tempo.seconds_at(beat) * rate).round().max(0.0) as u64
}

/// The furthest a seek goes: 24 hours.
pub fn max_frames(rate: f64) -> u64 {
    (24.0 * 3600.0 * rate) as u64
}

#[derive(Clone, Copy, Debug)]
pub struct Transport {
    pos: u64,
    playing: bool,
    /// The play state at the last block line: commands net out to one jump per line.
    was_playing: bool,
    looping: Option<Loop>,
    seeked: bool,
    /// A wrap that fell exactly on the block line, carried to the next block.
    wrapped: bool,
    enter: bool,
    /// The last loop asked for was refused, or a tempo change made it too short.
    loop_refused: bool,
}

impl Transport {
    /// Playing from `pos`, with no jump: offline renders.
    pub fn new(pos: u64, playing: bool) -> Transport {
        Transport { pos, playing, was_playing: playing, looping: None, seeked: false, wrapped: false, enter: false, loop_refused: false }
    }

    /// Starting to play at `pos` with the next block (a range render, RENDER-4).
    pub fn starting(pos: u64) -> Transport {
        Transport { was_playing: false, ..Transport::new(pos, true) }
    }

    /// The shortest loop: two blocks, and never shorter than an audio lane's crossfade.
    pub fn min_loop(rate: f64) -> u64 {
        (2 * BLOCK as u64).max((0.005 * rate).round() as u64)
    }

    pub fn pos(&self) -> u64 {
        self.pos
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn looping(&self) -> Option<Loop> {
        self.looping
    }

    pub fn loop_refused(&self) -> bool {
        self.loop_refused
    }

    /// Where the transport is, in beats of `tempo`.
    pub fn beat(&self, tempo: &TempoMap, rate: f64) -> f64 {
        tempo.beat_at(self.pos as f64 / rate)
    }

    /// Apply a command at the block line. Never allocates: the audio thread calls it.
    pub fn apply(&mut self, cmd: Cmd, tempo: &TempoMap, rate: f64) {
        match cmd {
            Cmd::Play => self.playing = true,
            Cmd::Stop => self.playing = false,
            // The server refuses a non-finite target first; one that gets here is ignored.
            Cmd::Seek(beat) if beat.is_finite() => {
                self.pos = frame_of(tempo, beat.max(0.0), rate).min(max_frames(rate));
                self.seeked = true;
                self.wrapped = false;
            }
            Cmd::Seek(_) => {}
            Cmd::Loop(None) => {
                self.looping = None;
                self.loop_refused = false;
            }
            Cmd::Loop(Some((start, end))) => self.set_loop(start, end, tempo, rate),
        }
    }

    /// A loop from `start` to `end` beats, when it is long enough at `rate` in `tempo`; otherwise
    /// no loop, and [`Self::loop_refused`] says so.
    fn set_loop(&mut self, start_beat: f64, end_beat: f64, tempo: &TempoMap, rate: f64) {
        let valid = start_beat.is_finite() && end_beat.is_finite() && 0.0 <= start_beat && start_beat < end_beat;
        let (start, end) = (frame_of(tempo, start_beat, rate), frame_of(tempo, end_beat, rate));
        if valid && end >= start + Transport::min_loop(rate) {
            self.looping = Some(Loop { start_beat, end_beat, start, end });
            self.loop_refused = false;
        } else {
            self.looping = None;
            self.loop_refused = true;
        }
    }

    /// The next block is a renderer's first: its cursors seek, and nothing is chased.
    pub fn enter(&mut self) {
        self.enter = true;
    }

    /// Keep the beat while the tempo map or the rate changes (RENDER-27): the position and the
    /// loop's frames are recomputed from their beats, and a loop the change makes too short is
    /// turned off.
    pub fn retime(&mut self, old: &TempoMap, old_rate: f64, new: &TempoMap, new_rate: f64) {
        if old == new && old_rate == new_rate {
            return;
        }
        let beat = old.beat_at(self.pos as f64 / old_rate);
        self.pos = frame_of(new, beat, new_rate).min(max_frames(new_rate));
        if let Some(l) = self.looping {
            self.set_loop(l.start_beat, l.end_beat, new, new_rate);
        }
    }

    /// The next block's runs, and the transport moved past them. A start, stop, seek or wrap on a
    /// renderer's first block line is that jump, not `Enter`: it ends the notes the renderer took
    /// over and declicks as on any block, and for what is new it seeks just as `Enter` does
    /// (RENDER-9, RENDER-26a).
    pub fn advance(&mut self) -> Runs {
        let jump = if !self.was_playing && self.playing {
            Jump::Start
        } else if self.was_playing && !self.playing {
            Jump::Stop
        } else if self.playing && self.seeked {
            Jump::Seek
        } else if self.playing && self.wrapped {
            Jump::Wrap
        } else if self.enter {
            Jump::Enter
        } else {
            Jump::None
        };
        (self.enter, self.seeked, self.wrapped, self.was_playing) = (false, false, false, self.playing);
        let block = BLOCK as u64;
        if !self.playing {
            return Runs::one(Run { off: 0, n: BLOCK as u32, at: self.pos, moving: false, jump });
        }
        let first = Run { off: 0, n: BLOCK as u32, at: self.pos, moving: true, jump };
        match self.looping {
            // A position at or past the end plays on.
            Some(l) if self.pos < l.end && l.end <= self.pos + block => {
                let w = l.end - self.pos;
                if w == block {
                    self.pos = l.start;
                    self.wrapped = true;
                    Runs::one(first)
                } else {
                    let mut runs = Runs::one(Run { n: w as u32, ..first });
                    let pushed = runs.push(Run { off: w as u32, n: (block - w) as u32, at: l.start, moving: true, jump: Jump::Wrap });
                    debug_assert!(pushed);
                    self.pos = l.start + (block - w);
                    runs
                }
            }
            _ => {
                self.pos += block;
                Runs::one(first)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use m2_song::{Arrangement, TempoMap};

    use super::*;

    const RATE: f64 = 48_000.0;

    fn at(bpm: f64) -> TempoMap {
        TempoMap::constant(bpm)
    }

    // RENDER-9a: an offline transport is the block grid: one run per block, no jump.
    #[test]
    fn offline_is_one_run_per_block() {
        let mut t = Transport::new(0, true);
        for k in 0..4u64 {
            let runs = t.advance();
            assert_eq!(runs.as_slice(), [Run { off: 0, n: 128, at: k * 128, moving: true, jump: Jump::None }]);
        }
    }

    // RENDER-6a: stopped, the transport holds its position; commands net out to one jump.
    #[test]
    fn commands_land_on_the_block_line_as_one_jump() {
        let tempo = at(120.0);
        let mut t = Transport::new(0, false);
        assert_eq!(t.advance().as_slice()[0], Run { off: 0, n: 128, at: 0, moving: false, jump: Jump::None });
        t.apply(Cmd::Seek(2.0), &tempo, RATE);
        assert_eq!(t.advance().first().at, 48_000, "a seek while stopped moves at once, with no jump");
        t.apply(Cmd::Play, &tempo, RATE);
        assert_eq!(*t.advance().first(), Run { off: 0, n: 128, at: 48_000, moving: true, jump: Jump::Start });
        t.apply(Cmd::Seek(4.0), &tempo, RATE);
        assert_eq!(t.advance().first().jump, Jump::Seek);
        t.apply(Cmd::Stop, &tempo, RATE);
        t.apply(Cmd::Play, &tempo, RATE);
        assert_eq!(t.advance().first().jump, Jump::None, "stop then play within a block is nothing");
        t.apply(Cmd::Stop, &tempo, RATE);
        let stop = *t.advance().first();
        assert_eq!((stop.moving, stop.jump, stop.at), (false, Jump::Stop, 96_000 + 256));
        assert_eq!(t.advance().first().at, 96_000 + 256, "frozen while stopped");
        t.apply(Cmd::Seek(f64::NAN), &tempo, RATE);
        t.apply(Cmd::Seek(-3.0), &tempo, RATE);
        assert_eq!(t.pos(), 0, "a negative seek clamps to 0, and a NaN is ignored");
        t.enter();
        assert_eq!(t.advance().first().jump, Jump::Enter, "a renderer's first block is Enter");
        t.enter();
        t.apply(Cmd::Play, &tempo, RATE);
        assert_eq!(t.advance().first().jump, Jump::Start, "unless a real jump shares its line");
    }

    // RENDER-9, RENDER-26a: on a renderer's first block line a start, stop, seek or block-line wrap
    // wins over Enter, so the notes it took over end and its audio declicks; alone, Enter it is.
    #[test]
    fn a_jump_on_an_entry_line_wins() {
        let tempo = at(120.0);
        let jump = |t: &mut Transport, cmd: Option<Cmd>| {
            t.enter();
            if let Some(c) = cmd {
                t.apply(c, &tempo, RATE);
            }
            t.advance().first().jump
        };
        let mut t = Transport::new(0, true);
        assert_eq!(jump(&mut t, None), Jump::Enter);
        assert_eq!(jump(&mut t, Some(Cmd::Seek(2.0))), Jump::Seek);
        assert_eq!(jump(&mut t, Some(Cmd::Stop)), Jump::Stop);
        assert_eq!(jump(&mut t, Some(Cmd::Play)), Jump::Start);
        // A loop of two blocks wraps on the block line: carried to the entry's line, it is a Wrap.
        let mut t = Transport::new(0, true);
        t.apply(Cmd::Loop(Some((0.0, 256.0 / 24_000.0))), &tempo, RATE);
        t.advance();
        t.advance();
        assert_eq!(jump(&mut t, None), Jump::Wrap);
        assert_eq!(t.pos(), 128, "and plays the loop's start");
    }

    // RENDER-9a: a wrap falls at the loop end's exact frame, mid-block, and one on the block
    // line is carried to the next block.
    #[test]
    fn wraps_fall_on_their_frame() {
        let tempo = at(120.0);
        let mut t = Transport::new(0, true);
        // Beats 0..1 at 120 bpm: frames 0..24000, 187.5 blocks.
        t.apply(Cmd::Loop(Some((0.0, 1.0))), &tempo, RATE);
        for _ in 0..187 {
            assert_eq!(t.advance().as_slice().len(), 1);
        }
        let runs = t.advance();
        assert_eq!(
            runs.as_slice(),
            [
                Run { off: 0, n: 64, at: 23_936, moving: true, jump: Jump::None },
                Run { off: 64, n: 64, at: 0, moving: true, jump: Jump::Wrap }
            ]
        );
        assert_eq!(t.pos(), 64);
        // A loop of exactly 2 blocks wraps on the block line.
        let mut t = Transport::new(0, true);
        t.apply(Cmd::Loop(Some((0.0, 256.0 / 24_000.0))), &tempo, RATE);
        assert_eq!(t.looping().unwrap().end(), 256);
        t.advance();
        assert_eq!(t.advance().as_slice().len(), 1);
        assert_eq!(*t.advance().first(), Run { off: 0, n: 128, at: 0, moving: true, jump: Jump::Wrap });
        // Past the end, it plays on.
        let mut t = Transport::new(30_000, true);
        t.apply(Cmd::Loop(Some((0.0, 1.0))), &tempo, RATE);
        assert_eq!(t.advance().as_slice(), [Run { off: 0, n: 128, at: 30_000, moving: true, jump: Jump::None }]);
    }

    // RENDER-9a, step 3 gate (G2a): a 4-bar loop at 127 bpm for 10 minutes from 1:00. Every
    // wrap falls at E0 + k·L, the loop end is the frame computed from its beat, and nothing
    // drifts.
    #[test]
    fn a_loop_at_127_bpm_runs_10_minutes_without_drift() {
        let arr: Arrangement = serde_json::from_value(serde_json::json!({"tempo": [{"beat": 0, "bpm": 127}]})).unwrap();
        let tempo = arr.tempo_map();
        let mut t = Transport::new(0, false);
        t.apply(Cmd::Seek(tempo.beat_at(60.0)), &tempo, RATE);
        t.apply(Cmd::Play, &tempo, RATE);
        t.apply(Cmd::Loop(Some((128.0, 144.0))), &tempo, RATE);
        let l = t.looping().unwrap();
        assert_eq!((l.start(), l.end()), (2_902_677, 3_265_512));
        assert_eq!(l.end(), frame_of(&tempo, 144.0, RATE));
        let len = l.end() - l.start();
        let (mut engine, mut wraps) = (0u64, vec![]);
        let mut expect = t.pos();
        while engine < (600.0 * RATE) as u64 {
            let runs = t.advance();
            for r in runs.as_slice() {
                if r.jump == Jump::Wrap {
                    wraps.push(engine + r.off as u64);
                    expect = l.start();
                }
                // The transport is continuous between wraps.
                assert_eq!(r.at, expect, "at engine frame {engine}");
                expect = r.at + r.n as u64;
                assert!(r.at + r.n as u64 <= l.end());
            }
            engine += 128;
        }
        assert!(wraps.len() > 70, "{} wraps", wraps.len());
        for (k, w) in wraps.iter().enumerate() {
            assert_eq!(*w, wraps[0] + k as u64 * len, "wrap {k}");
        }
    }

    // RENDER-9a, RENDER-27: a tempo change keeps the beat; a loop it makes too short is turned
    // off and reported.
    #[test]
    fn retiming_keeps_the_beat_and_checks_the_loop() {
        let (slow, fast) = (at(60.0), at(240.0));
        let mut t = Transport::new(0, true);
        t.apply(Cmd::Seek(8.0), &slow, RATE);
        t.apply(Cmd::Loop(Some((8.0, 8.02))), &slow, RATE);
        assert!(t.looping().is_some(), "0.02 beats at 60 bpm is 960 frames");
        t.retime(&slow, RATE, &fast, RATE);
        assert_eq!(t.pos(), 96_000);
        assert_eq!(t.beat(&fast, RATE), 8.0);
        assert!(t.looping().is_none() && t.loop_refused(), "240 frames is under two blocks");
        // A rate change keeps the beat too.
        t.retime(&fast, RATE, &fast, 96_000.0);
        assert_eq!(t.pos(), 192_000);
        // Too short, backwards, or not finite: refused.
        for (a, b) in [(0.0, 0.001), (4.0, 2.0), (f64::NAN, 1.0), (-1.0, 2.0)] {
            let mut t = Transport::new(0, true);
            t.apply(Cmd::Loop(Some((a, b))), &slow, RATE);
            assert!(t.looping().is_none() && t.loop_refused(), "{a}..{b}");
            t.apply(Cmd::Loop(None), &slow, RATE);
            assert!(!t.loop_refused());
        }
        assert_eq!(Transport::min_loop(48_000.0), 256);
        assert_eq!(Transport::min_loop(96_000.0), 480);
    }
}

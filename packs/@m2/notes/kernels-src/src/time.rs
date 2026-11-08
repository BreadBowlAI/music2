//! Where the song is, as the Arpeggiator and Note echo read it (PACK-30, PACK-32): the song's beat
//! (`time.beats`) while it plays, and whether it started, stopped or jumped between two frames; the
//! tempo (`time.tempo`), by which each keeps time while the song is stopped; and the note lengths
//! their menus name.

/// The beats of each of the menus' rates, `1/4`, `1/8`, `1/8T`, `1/16`, `1/16T` and `1/32`: a T is
/// two thirds of its plain one.
pub const RATES: [f64; 6] = [1.0, 0.5, 1.0 / 3.0, 0.25, 1.0 / 6.0, 0.125];

/// How far the song's beat may move on in one frame and still be playing on: further is a seek
/// (PACK-30). At 999 bpm and 8 kHz a frame is 0.002 beats.
pub const LEAP: f32 = 1.0 / 64.0;

/// The place a choice's signal holds, from 0 to `n` − 1: the nearest, or `rest` for one that isn't
/// a number.
pub fn place(v: f32, n: usize, rest: usize) -> usize {
    if v.is_finite() { (v.round().max(0.0) as usize).min(n - 1) } else { rest }
}

/// `v` within `lo..=hi`, or `rest` when it isn't a number.
pub fn within(v: f32, lo: f32, hi: f32, rest: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { rest }
}

/// The beats one frame lasts at `tempo` (beats per minute; one that isn't a tempo is 120).
pub fn beats_per_frame(tempo: f32, sample_rate: f32) -> f64 {
    let bpm = if tempo.is_finite() && tempo > 0.0 { tempo as f64 } else { 120.0 };
    bpm / 60.0 / sample_rate.max(1.0) as f64
}

/// The song at one frame, as the kernels read it: whether it plays (`time.playing`), and its beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Song {
    pub playing: bool,
    pub beats: f32,
}

impl Song {
    pub fn at(playing: f32, beats: f32) -> Song {
        Song { playing: playing >= 0.5, beats: if beats.is_finite() { beats } else { 0.0 } }
    }

    /// Whether the song started, stopped or jumped since `last`, the frame before (PACK-30): Play, a
    /// Stop, a seek, a loop's wrap; and the first frame a kernel sees. Its beat going back, or on by
    /// more than [`LEAP`] in a frame, is a jump while it plays; any move is one while it is stopped,
    /// where the beat holds.
    pub fn moved(self, last: Option<Song>) -> bool {
        match last {
            None => true,
            Some(l) if l.playing != self.playing => true,
            Some(l) if self.playing => self.beats < l.beats || self.beats > l.beats + LEAP,
            Some(l) => self.beats != l.beats,
        }
    }
}

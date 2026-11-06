//! `@m2/vocals:voice`: a clip's edited notes, played (VOX-45, RENDER-30b). An offline kernel: the
//! host gives it one phrase of a clip to render, and it writes that phrase's frames of the clip's
//! output. TD-PSOLA (Moulines and Charpentier, 1990) laid out by the clip's time map.
//!
//! **What it is given.** Its source is the clip's file (asset 0). `values` are the phrase's first
//! and last frame in the clip's output (the song's rate), its lead-in in seconds, then each note's
//! five numbers: `at`, `length` (seconds of the file), `pitch`, `move` and `straighten` (VOX-40).
//! Its time map is the clip's pairs (file seconds, output seconds from the clip's start) that reach
//! the phrase. It starts the lead-in before the phrase, so its pulses and pitch have settled where
//! the phrase begins, and writes the phrase alone.
//!
//! **What plays.** At each moment of a note, the pitch line plus `move`, pulled toward
//! `pitch + move` by `straighten`: the voice moves by `shift = move + straighten × (pitch − p)`, `p`
//! the pitch line there, within −7 and 12 semitones (as far as `shift` reaches, VOX-23). Elsewhere,
//! and where the voice has no pitch, it moves by 0, and the sound passes through.
//!
//! **The pitch line** is `pitch`'s reading ([`crate::yin`]) at the hops of the measure
//! `@m2/vocals:notes` ([`crate::measure`]), of the window centred on each, between two hops a straight
//! line: the very numbers the window draws, so the line it moves is the line the musician sees.
//!
//! **Pulses** are marked in the mono sum low-passed at 1 kHz, as `shift` marks them ([`crate::psola`]):
//! each the largest peak within a quarter period of where the last mark plus the period says, an
//! octave error taken as the nearer period.
//!
//! **Grains** are raised cosines read at the file's own speed, so the voice keeps its colour. A
//! *voiced* grain is read around the pulse nearest the place the time map gives its centre, a
//! period each side and at most 10 ms; the next is laid that pulse's period over `2^(shift/12)`
//! later along the output, so a higher voice lays them closer. A voiced run starts on a pulse, laid
//! where the time map puts it. Where no pulse is near, grains of 10 ms each side are read where the
//! time map puts them. Each sample out is the sum of the grains over it divided by the sum of their
//! windows. Grain centres aren't rounded to a frame: each frame of a grain reads the file at the
//! grain's source point plus its distance from the centre, so with no shift and the map at speed 1
//! every grain reads the very point the normal path reads, and the voice gives back the file.
//!
//! Every buffer is sized at birth; nothing is taken while it runs.

use m2_kernel::{Cfg, Io, Kernel};

use crate::aim::VOICED;
use crate::file::File;
use crate::measure::HOP;
use crate::psola::{DOWN, LEAP, Lowpass, PASS, UP};
use crate::yin::{HIGHEST, LOWEST, Pitch};

/// The most notes it reads, and the longest lead-in, in seconds.
const NOTES: usize = 4096;
const LEAD: f64 = 1.0;
/// Marks kept, and readings kept, each a ring.
const MARKS: usize = 256;
const READINGS: usize = 1024;
/// The fastest a time map may read its file: more, and it is taken as speed 1.
const FASTEST: f64 = 16.0;
/// Points of the half window's table.
const TABLE: usize = 1024;
/// What a sample weighs where no grain is over it: the file as the time map reads it.
const EPS: f32 = 1e-4;
/// The longest a phrase may be, in frames: three hours at 384 kHz.
const LONGEST: f64 = 3.0 * 3600.0 * 384_000.0;

#[derive(Clone, Copy, Default)]
struct Mark {
    at: i64,
    period: f32,
    said: f32,
    odd: u8,
}

#[derive(Clone, Copy)]
enum Last {
    Pass { at: f64, half: f64 },
    Voiced { at: f64, mark: usize },
}

/// One grain: its centre and half length in output frames, and the file frame its centre reads.
#[derive(Clone, Copy)]
struct Grain {
    at: f64,
    half: f64,
    read: f64,
}

/// A note as given: `at`, its end, `pitch`, `move`, `straighten`.
#[derive(Clone, Copy, Default)]
struct Note {
    at: f64,
    end: f64,
    pitch: f32,
    by: f32,
    straighten: f32,
}

pub struct Voice {
    file: File,
    pitch: Pitch,
    /// The output's rate, the file's, and file frames per output frame at the file's own speed.
    rate: f64,
    src_rate: f64,
    step: f64,
    map: Vec<[f64; 2]>,
    notes: Vec<Note>,
    /// The note a shift was last found in.
    near: usize,
    /// The phrase's frames, and where its lead-in starts.
    from: i64,
    to: i64,
    start: i64,
    /// Readings by hop: a ring of (hop, hz, clarity).
    readings: Vec<(i64, f32, f32)>,
    /// The pulses: the low-passed mono sum by file frame (a ring), the next frame to look at, the
    /// marks found (mark `j` is `marks[j % MARKS]`), and the mark a chain goes on from.
    lp: Vec<f32>,
    lp_mask: usize,
    filter: Lowpass,
    looked: i64,
    first: i64,
    marks: [Mark; MARKS],
    count: usize,
    chain: Option<Mark>,
    /// The output being laid (a ring by output frame), its windows' sum, the next frame written,
    /// and the grain laid last.
    acc: [Vec<f32>; 2],
    wsum: Vec<f32>,
    mask: usize,
    hann: Vec<f32>,
    emit: i64,
    last: Last,
    /// Half a grain that passes the sound, and the most half a grain may be, in output frames.
    pass: f64,
    most: f64,
    /// The longest period, in file frames.
    longest: usize,
    channels: usize,
}

/// `x` when it is a finite number, else `or`.
fn finite(x: f64, or: f64) -> f64 {
    if x.is_finite() { x } else { or }
}

/// Seconds of `map`'s first side at seconds `x` of its second (`by` 1), or the other way (`by` 0):
/// straight between two pairs, the nearest line beyond, speed 1 through one pair, the same with none.
fn through(map: &[[f64; 2]], x: f64, by: usize) -> f64 {
    let other = 1 - by;
    match map.len() {
        0 => x,
        1 => map[0][other] + (x - map[0][by]),
        n => {
            let i = map.partition_point(|p| p[by] <= x).clamp(1, n - 1);
            let (a, b) = (map[i - 1], map[i]);
            a[other] + (x - a[by]) * (b[other] - a[other]) / (b[by] - a[by])
        }
    }
}

impl Voice {
    /// The file frame output frame `z` reads, through the time map.
    fn src(&self, z: f64) -> f64 {
        through(&self.map, z / self.rate, 1) * self.src_rate
    }

    /// The output frame where file frame `f` plays.
    fn out(&self, f: f64) -> f64 {
        through(&self.map, f / self.src_rate, 0) * self.rate
    }

    /// The reading of hop `k`: `hz` and `clarity`.
    fn reading(&mut self, k: i64) -> (f32, f32) {
        let slot = k.rem_euclid(READINGS as i64) as usize;
        if self.readings[slot].0 != k {
            let (span, half) = (self.pitch.span() as i64, self.pitch.window() as i64 / 2);
            let end = k * HOP as i64 + half;
            self.file.hold(end - span, end);
            let (hz, clarity, _) = self.pitch.read_window(self.file.mono(end - span, end));
            self.readings[slot] = (k, hz, clarity);
        }
        (self.readings[slot].1, self.readings[slot].2)
    }

    /// Whether the voice has a pitch at file frame `f`, and what it is.
    fn sung_at(&mut self, f: f64) -> Option<f32> {
        let (hz, clarity) = self.reading((f / HOP as f64).round() as i64);
        (clarity >= VOICED && (LOWEST..=HIGHEST).contains(&hz)).then_some(hz)
    }

    /// The pitch line at file frame `f`, as a MIDI number: between two voiced hops a straight line.
    fn line(&mut self, f: f64) -> Option<f32> {
        let midi = |hz: f32| 69.0 + 12.0 * (hz / 440.0).log2();
        let k = (f / HOP as f64).floor() as i64;
        let voiced = |r: (f32, f32)| (r.1 >= VOICED && (LOWEST..=HIGHEST).contains(&r.0)).then_some(r.0);
        let (a, b) = (voiced(self.reading(k)), voiced(self.reading(k + 1)));
        let t = (f / HOP as f64 - k as f64) as f32;
        match (a, b) {
            (Some(a), Some(b)) => Some(midi(a) + t * (midi(b) - midi(a))),
            (Some(a), None) if t < 0.5 => Some(midi(a)),
            (None, Some(b)) if t >= 0.5 => Some(midi(b)),
            _ => None,
        }
    }

    /// How many semitones the voice moves at file frame `f`.
    fn shift(&mut self, f: f64) -> f32 {
        let t = f / self.src_rate;
        while self.near > 0 && self.notes[self.near].at > t {
            self.near -= 1;
        }
        while self.near + 1 < self.notes.len() && self.notes[self.near].end <= t {
            self.near += 1;
        }
        let Some(&n) = self.notes.get(self.near) else { return 0.0 };
        if !(n.at <= t && t < n.end) || (n.by == 0.0 && n.straighten == 0.0) {
            return 0.0;
        }
        let pull = match (n.straighten, self.line(f)) {
            (s, Some(p)) if s > 0.0 => s * (n.pitch - p),
            _ => 0.0,
        };
        (n.by + pull).clamp(DOWN, UP)
    }

    fn mark(&self, j: usize) -> Option<Mark> {
        (j < self.count && j + MARKS > self.count).then(|| self.marks[j % MARKS])
    }

    fn push(&mut self, m: Mark) {
        self.marks[self.count % MARKS] = m;
        self.count += 1;
        self.chain = Some(m);
    }

    /// The largest peak of the low-passed sum over frames `from..=to`.
    fn peak(&self, from: i64, to: i64) -> (i64, f32) {
        let mut best = (from, f32::NEG_INFINITY);
        for i in from..=to {
            let v = self.lp[(i as usize) & self.lp_mask];
            if v > best.1 {
                best = (i, v);
            }
        }
        best
    }

    /// Look for pulses up to file frame `to`.
    fn look(&mut self, to: i64) {
        while self.looked < to {
            let n = self.looked;
            self.file.hold(n, n + 1);
            let x = self.file.mono_at(n);
            self.lp[(n as usize) & self.lp_mask] = self.filter.tick(x);
            let said = self.sung_at(n as f64);
            self.track(n, said);
            self.looked += 1;
        }
    }

    /// Follow the pulses at file frame `n`, given the pitch there (`shift`'s rule, VOX-23).
    fn track(&mut self, n: i64, hz: Option<f32>) {
        let Some(hz) = hz else {
            self.chain = None;
            return;
        };
        let said = self.src_rate as f32 / hz;
        let newest = self.mark(self.count.wrapping_sub(1)).map(|m| m.at);
        match self.chain {
            None => {
                let (from, to) = ((n as f32 - 1.25 * said).round() as i64, (n as f32 - 0.25 * said).round() as i64);
                let from = newest.map_or(from, |m| from.max(m + (0.5 * said) as i64)).max(self.first);
                if from <= to {
                    let (at, v) = self.peak(from, to);
                    if v > 0.0 {
                        self.push(Mark { at, period: said, said, odd: 0 });
                    }
                }
            }
            Some(last) => {
                let ratio = said / last.said;
                let (period, odd) = match ratio {
                    r if (1.7..2.3).contains(&r) && last.odd < LEAP => (said / 2.0, last.odd + 1),
                    r if (0.43..0.59).contains(&r) && last.odd < LEAP => (said * 2.0, last.odd + 1),
                    _ => (said, 0),
                };
                let expect = last.at as f32 + period;
                let to = (expect + 0.25 * period).round() as i64;
                if n < to {
                    return;
                }
                let from = ((expect - 0.25 * period).round() as i64).max(last.at + 1);
                let (at, v) = self.peak(from, to);
                match v > 0.0 {
                    true => self.push(Mark { at, period: (at - last.at) as f32, said: period, odd }),
                    false => self.chain = None,
                }
            }
        }
    }

    /// The period around mark `j`, in file frames: to the mark after it once known, else its own.
    fn period_at(&self, j: usize) -> f64 {
        let Some(here) = self.mark(j) else { return self.longest as f64 };
        self.mark(j + 1).map_or(here.period as f64, |n| (n.at - here.at) as f64)
    }

    /// The mark nearest file frame `f`, among those kept.
    fn nearest(&self, f: f64) -> Option<(usize, Mark)> {
        let mut best: Option<(usize, Mark)> = None;
        let mut j = self.count;
        while j > 0 && j + MARKS > self.count {
            j -= 1;
            let m = self.marks[j % MARKS];
            if best.is_none_or(|(_, b)| (m.at as f64 - f).abs() < (b.at as f64 - f).abs()) {
                best = Some((j, m));
            }
            if (m.at as f64) < f - 4.0 * self.longest as f64 {
                break;
            }
        }
        best
    }

    /// The first mark that plays after output frame `after` and by `by`.
    fn first_between(&self, after: f64, by: f64) -> Option<(usize, Mark, f64)> {
        let mut found = None;
        let mut j = self.count;
        while j > 0 && j + MARKS > self.count {
            j -= 1;
            let m = self.marks[j % MARKS];
            let z = self.out(m.at as f64);
            if z <= after {
                break;
            }
            if z <= by {
                found = Some((j, m, z));
            }
        }
        found
    }

    /// The next grain after the last, and what it makes the last.
    fn next(&mut self) -> (Grain, Last) {
        match self.last {
            Last::Pass { at, half } => {
                let next = at + half;
                // A voiced run starts on a pulse, laid where the time map puts it: the first after
                // this grain's centre and by the next.
                let ahead = self.src(next + self.most).ceil() as i64 + 2 * self.longest as i64;
                self.look(ahead);
                if let Some((j, m, z)) = self.first_between(at, next)
                    && self.sung_at(m.at as f64).is_some()
                {
                    return self.voiced(z, j, m);
                }
                (Grain { at: next, half: self.pass, read: self.src(next) }, Last::Pass { at: next, half: self.pass })
            }
            Last::Voiced { at, mark } => {
                let period = self.period_at(mark) / self.step;
                let semis = self.shift(self.src(at));
                let next = at + (period / 2f64.powf(semis as f64 / 12.0)).max(1.0);
                let f = self.src(next);
                self.look(f.ceil() as i64 + 2 * self.longest as i64);
                match self.nearest(f) {
                    Some((j, m)) if self.sung_at(f).is_some() && (m.at as f64 - f).abs() <= 0.5 * m.period as f64 + 1.0 => {
                        self.voiced(next, j, m)
                    }
                    _ => (Grain { at: next, half: self.pass, read: f }, Last::Pass { at: next, half: self.pass }),
                }
            }
        }
    }

    /// A voiced grain centred at output frame `at`, read around mark `j` (`m`).
    fn voiced(&self, at: f64, j: usize, m: Mark) -> (Grain, Last) {
        let half = (self.period_at(j) / self.step).min(self.most).max(1.0);
        (Grain { at, half, read: m.at as f64 }, Last::Voiced { at, mark: j })
    }

    /// The window at `d` frames from a grain's centre, `half` to its edge.
    fn window(&self, d: f64, half: f64) -> f32 {
        let u = ((d.abs() / half).min(1.0) * TABLE as f64) as f32;
        let i = (u as usize).min(TABLE - 1);
        let f = u - i as f32;
        self.hann[i] + f * (self.hann[i + 1] - self.hann[i])
    }

    /// Lay `g` on the output.
    fn lay(&mut self, g: Grain) {
        let (lo, hi) = ((g.at - g.half).ceil() as i64, (g.at + g.half).floor() as i64);
        let reach = (g.half * self.step).ceil() as i64 + 4;
        self.file.hold(g.read.floor() as i64 - reach, g.read.floor() as i64 + reach);
        for z in lo.max(self.emit)..=hi {
            let d = z as f64 - g.at;
            let w = self.window(d, g.half);
            let pos = g.read + d * self.step;
            let i = (z as usize) & self.mask;
            for c in 0..self.channels {
                self.acc[c][i] += w * self.file.read(c, pos);
            }
            self.wsum[i] += w;
        }
    }

    /// Lay every grain that reaches output frame `z`.
    fn lay_to(&mut self, z: i64) {
        loop {
            let (g, last) = self.next();
            if g.at - self.most > z as f64 {
                return;
            }
            self.lay(g);
            self.last = last;
        }
    }

    /// Output frame `emit`, a sample a channel, and its room cleared.
    fn take(&mut self) -> [f32; 2] {
        let z = self.emit;
        let i = (z as usize) & self.mask;
        let f = self.src(z as f64);
        self.file.hold(f.floor() as i64 - 2, f.floor() as i64 + 3);
        let w = self.wsum[i];
        let mut out = [0.0; 2];
        for (c, o) in out.iter_mut().enumerate().take(self.channels) {
            let direct = self.file.read(c, f);
            *o = (self.acc[c][i] + EPS * direct) / (w + EPS);
            self.acc[c][i] = 0.0;
        }
        self.wsum[i] = 0.0;
        self.emit += 1;
        out
    }
}

impl Kernel for Voice {
    const INPUTS: usize = 0;
    const OUTPUTS: usize = 1;

    fn new(cfg: &Cfg) -> Self {
        let asset = cfg.assets.first().copied().unwrap_or_default();
        let rate = (cfg.sample_rate as f64).clamp(8_000.0, 384_000.0);
        let src_rate = if asset.sample_rate > 0.0 { asset.sample_rate as f64 } else { rate }.clamp(8_000.0, 384_000.0);
        let v = cfg.values;
        let value = |i: usize, or: f64| finite(v.get(i).copied().unwrap_or(or), or);
        let from = value(0, 0.0).clamp(0.0, LONGEST).round() as i64;
        let to = value(1, 0.0).clamp(from as f64, from as f64 + LONGEST).round() as i64;
        let lead = (value(2, 0.1).clamp(0.0, LEAD) * rate).round() as i64;
        let mut notes: Vec<Note> = v
            .get(3..)
            .unwrap_or(&[])
            .as_chunks::<5>()
            .0
            .iter()
            .take(NOTES)
            .filter(|n| n.iter().all(|x| x.is_finite()) && n[1] > 0.0)
            .map(|n| Note {
                at: n[0],
                end: n[0] + n[1],
                pitch: n[2].clamp(0.0, 127.0) as f32,
                by: n[3].clamp(DOWN as f64, UP as f64) as f32,
                straighten: n[4].clamp(0.0, 1.0) as f32,
            })
            .collect();
        notes.sort_by(|a, b| a.at.total_cmp(&b.at));
        // A map that doesn't run forward in both, or reads too fast, is taken as speed 1.
        let given = cfg.time_map();
        let fine = given.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[0].abs() < 1e7 && p[1].abs() < 1e7)
            && given.windows(2).all(|w| w[1][0] > w[0][0] && w[1][1] > w[0][1] && (w[1][0] - w[0][0]) / (w[1][1] - w[0][1]) <= FASTEST);
        let map = if fine { given.to_vec() } else { vec![] };
        let pitch = Pitch::new(&Cfg::new(src_rate as f32, cfg.max_block, 1));
        let longest = (src_rate / LOWEST as f64).ceil() as usize;
        let step = src_rate / rate;
        let most = (PASS as f64 * rate).max(1.0);
        let pass = most;
        // The file's window spans what the grains of a call read, the pulses ahead of them and a
        // reading's samples.
        let block = cfg.max_block.max(1) as f64;
        let spread = (block + 6.0 * most) * FASTEST * step + (4 * pitch.span() + 6 * longest + 2 * HOP) as f64;
        let cap = (2.0 * spread) as usize + 4096;
        let file = File::new(asset, cap, cap / 2);
        let size = ((block + 6.0 * most) as usize + 64).next_power_of_two();
        let lp_size = (16 * longest + 64).next_power_of_two();
        let hann = (0..=TABLE).map(|i| (0.5 + 0.5 * (std::f64::consts::PI * i as f64 / TABLE as f64).cos()) as f32).collect();
        let start = from - lead;
        let mut voice = Voice {
            file,
            pitch,
            rate,
            src_rate,
            step,
            map,
            notes,
            near: 0,
            from,
            to,
            start,
            readings: vec![(i64::MIN, 0.0, 0.0); READINGS],
            lp: vec![0.0; lp_size],
            lp_mask: lp_size - 1,
            filter: Lowpass::new(src_rate, 1000.0),
            looked: 0,
            first: 0,
            marks: [Mark::default(); MARKS],
            count: 0,
            chain: None,
            acc: [vec![0.0; size], vec![0.0; size]],
            wsum: vec![0.0; size],
            mask: size - 1,
            hann,
            emit: start,
            last: Last::Pass { at: 0.0, half: pass },
            pass,
            most,
            longest,
            channels: cfg.channels.clamp(1, 2),
        };
        voice.begin();
        voice
    }

    /// As `new` leaves it, cleared in place: a reset takes no memory.
    fn reset(&mut self, _: &Cfg) {
        for a in &mut self.acc {
            a.fill(0.0);
        }
        self.wsum.fill(0.0);
        self.lp.fill(0.0);
        self.readings.fill((i64::MIN, 0.0, 0.0));
        self.filter.clear();
        (self.count, self.chain, self.near) = (0, None, 0);
        self.begin();
    }

    fn process(&mut self, io: &mut Io) {
        let channels = io.channels.clamp(1, 2);
        // The lead-in first, laid and let go.
        while self.emit < self.from {
            self.lay_to(self.emit);
            self.take();
        }
        for t in 0..io.frames {
            let out = if self.emit < self.to {
                self.lay_to(self.emit);
                self.take()
            } else {
                self.emit += 1;
                [0.0; 2]
            };
            for c in 0..2 {
                io.output_ch(0, c)[t] = out[c.min(channels - 1)];
            }
        }
    }
}

impl Voice {
    /// Where a render starts: its lead-in's first frame, a pulse search a few periods before.
    fn begin(&mut self) {
        self.emit = self.start;
        self.last = Last::Pass { at: self.start as f64 - self.pass, half: self.pass };
        let f = self.src(self.start as f64 - 2.0 * self.most).floor() as i64;
        self.first = f - 4 * self.longest as i64;
        self.looked = self.first - 4 * self.longest as i64;
        let _ = self.to;
    }
}

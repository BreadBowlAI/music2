//! What the gates measure in the sound itself: levels, steps, onsets, tails cut at a jump and
//! tails that freeze. Pure functions over stereo frames, so each detector is tested on made-up
//! signals before it judges an engine.

/// Root mean square over both channels.
pub fn rms(x: &[[f32; 2]]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|f| f[0] as f64 * f[0] as f64 + f[1] as f64 * f[1] as f64).sum::<f64>() / (2 * x.len()) as f64).sqrt()
}

/// The largest magnitude on either channel.
pub fn peak(x: &[[f32; 2]]) -> f64 {
    x.iter().fold(0.0f64, |m, f| m.max(f[0].abs() as f64).max(f[1].abs() as f64))
}

/// A level in dBFS, floored at -240 so silence still compares.
pub fn db(level: f64) -> f64 {
    20.0 * level.max(1e-12).log10()
}

/// The largest sample-to-sample step on either channel, and the frame it lands on.
pub fn max_step(x: &[[f32; 2]]) -> (f64, usize) {
    let mut best = (0.0, 0);
    for (i, w) in x.windows(2).enumerate() {
        let d = ((w[1][0] - w[0][0]).abs() as f64).max((w[1][1] - w[0][1]).abs() as f64);
        if d > best.0 {
            best = (d, i + 1);
        }
    }
    best
}

/// `x[from..to]`, clamped to what exists.
pub fn span(x: &[[f32; 2]], from: u64, to: u64) -> &[[f32; 2]] {
    let to = (to as usize).min(x.len());
    &x[(from as usize).min(to)..to]
}

/// The level just before and just after a jump: RMS over `window` frames each side of `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Around {
    pub at: u64,
    pub before: f64,
    pub after: f64,
}

impl Around {
    pub fn of(x: &[[f32; 2]], at: u64, window: u64) -> Around {
        Around { at, before: rms(span(x, at.saturating_sub(window), at)), after: rms(span(x, at, at + window)) }
    }

    /// After over before: about 1 when a tail rings through, about 0 when it was cut.
    pub fn ratio(&self) -> f64 {
        if self.before == 0.0 { f64::INFINITY } else { self.after / self.before }
    }
}

/// RMS of consecutive `window`-frame windows.
pub fn levels(x: &[[f32; 2]], window: usize) -> Vec<f64> {
    x.chunks_exact(window.max(1)).map(rms).collect()
}

/// Peak of consecutive `window`-frame windows (one echo period each, for a delay's tail).
pub fn peaks(x: &[[f32; 2]], window: usize) -> Vec<f64> {
    x.chunks_exact(window.max(1)).map(peak).collect()
}

/// The first window that rises more than `tolerance_db` above the quietest one before it,
/// ignoring windows quieter than `floor_db` (numerical dust). None: it only decays.
pub fn first_rise(levels: &[f64], tolerance_db: f64, floor_db: f64) -> Option<(usize, f64)> {
    let mut lowest = f64::INFINITY;
    for (i, l) in levels.iter().map(|l| db(*l)).enumerate() {
        if l < floor_db {
            continue;
        }
        if l > lowest + tolerance_db {
            return Some((i, l - lowest));
        }
        lowest = lowest.min(l);
    }
    None
}

/// The first of two consecutive `block`-frame blocks that are identical and not silent: an
/// engine that stopped processing and repeats its last output.
pub fn frozen(x: &[[f32; 2]], block: usize) -> Option<usize> {
    let blocks: Vec<&[[f32; 2]]> = x.chunks_exact(block).collect();
    blocks.windows(2).position(|w| w[0] == w[1] && peak(w[0]) > 0.0).map(|i| i * block)
}

/// Frames where the sound starts: the first frame at or above `threshold` after at least
/// `gap` frames below it.
pub fn onsets(x: &[[f32; 2]], threshold: f64, gap: usize) -> Vec<u64> {
    let mut out = vec![];
    let mut quiet = gap;
    for (i, f) in x.iter().enumerate() {
        if (f[0].abs() as f64).max(f[1].abs() as f64) >= threshold {
            if quiet >= gap {
                out.push(i as u64);
            }
            quiet = 0;
        } else {
            quiet += 1;
        }
    }
    out
}

/// Frames where a sound starts over another that goes on (a kick over a held chord): the start of
/// each `window`-frame window whose level is at least `jump_db` above the loudest of the `back`
/// windows before it, and above `floor_db`, at least `gap` frames after the last one found. The
/// loudest, not the last: a sound that beats (detuned voices) dips and comes back to where it
/// was, which isn't a start.
pub fn rises(x: &[[f32; 2]], window: usize, back: usize, jump_db: f64, floor_db: f64, gap: usize) -> Vec<u64> {
    let l: Vec<f64> = levels(x, window).into_iter().map(db).collect();
    let mut out: Vec<u64> = vec![];
    for i in back.max(1)..l.len() {
        let (at, before) = ((i * window) as u64, l[i - back.max(1)..i].iter().copied().fold(f64::MIN, f64::max));
        if l[i] >= floor_db && l[i] - before >= jump_db && out.last().is_none_or(|&o| at - o >= gap as u64) {
            out.push(at);
        }
    }
    out
}

/// The first frame from which the level stays below `db_floor` (RMS over `window`-frame
/// windows) to the end, or None when it never settles.
pub fn settles(x: &[[f32; 2]], window: usize, db_floor: f64) -> Option<u64> {
    let l = levels(x, window);
    let loud = l.iter().rposition(|v| db(*v) >= db_floor);
    match loud {
        None => Some(0),
        Some(i) if i + 1 < l.len() => Some(((i + 1) * window) as u64),
        Some(_) => None,
    }
}

/// p50, p95 and max of some measurements, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
}

impl Summary {
    pub fn of(values: &[f64]) -> Summary {
        if values.is_empty() {
            return Summary::default();
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        // Nearest rank: the smallest value at or above the share asked for.
        let at = |p: f64| v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
        Summary { n: v.len(), p50: at(0.5), p95: at(0.95), max: v[v.len() - 1] }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms (n={})", self.p50, self.p95, self.max, self.n)
    }
}

/// A fixed-seed generator (xorshift64*), so a run's random seeks and clocks repeat.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(n: usize, hz: f64, gain: f64) -> Vec<[f32; 2]> {
        (0..n).map(|i| [(gain * (std::f64::consts::TAU * hz * i as f64 / 48_000.0).sin()) as f32; 2]).collect()
    }

    #[test]
    fn levels_and_steps_of_a_sine() {
        let x = sine(48_000, 440.0, 1.0);
        assert!((rms(&x) - 0.5f64.sqrt()).abs() < 1e-3);
        assert!((db(rms(&x)) + 3.01).abs() < 0.01);
        // A full-scale 440 Hz sine's own largest step: about 2π·440/48000.
        let (step, _) = max_step(&x);
        assert!((step - 0.0576).abs() < 1e-3, "{step}");
        let mut cut = x.clone();
        cut[1000] = [0.9, 0.9];
        assert_eq!(max_step(&cut).1, 1000);
    }

    #[test]
    fn a_tail_cut_at_a_jump_is_seen_and_one_that_rings_through_is_not() {
        let tail: Vec<[f32; 2]> =
            (0..48_000).map(|i| [(0.5 * (-(i as f64) / 20_000.0).exp() * (i as f64 * 0.05).sin()) as f32; 2]).collect();
        let rang = Around::of(&tail, 24_000, 2_400);
        assert!(rang.ratio() > 0.8 && rang.ratio() < 1.0, "{rang:?}");
        let mut cut = tail.clone();
        cut[24_000..].fill([0.0; 2]);
        assert_eq!(Around::of(&cut, 24_000, 2_400).ratio(), 0.0);
    }

    #[test]
    fn a_decay_passes_and_a_rise_or_a_freeze_does_not() {
        let decay: Vec<f64> = (0..30).map(|i| 0.5 * 0.8f64.powi(i)).collect();
        assert_eq!(first_rise(&decay, 1.0, -120.0), None);
        let mut ghost = decay.clone();
        ghost[20] = 0.3;
        assert_eq!(first_rise(&ghost, 1.0, -120.0).map(|r| r.0), Some(20));
        // Dust under the floor never counts.
        let mut dust = decay.clone();
        dust.extend([1e-9, 1e-8, 1e-9]);
        assert_eq!(first_rise(&dust, 1.0, -150.0), None);
        let mut x = sine(1024, 440.0, 0.5);
        assert_eq!(frozen(&x, 128), None);
        let block: Vec<[f32; 2]> = x[256..384].to_vec();
        x[384..512].copy_from_slice(&block);
        assert_eq!(frozen(&x, 128), Some(256));
        assert_eq!(frozen(&[[0.0; 2]; 512], 128), None, "silence repeats, and that is fine");
    }

    #[test]
    fn onsets_are_found_after_quiet_and_not_inside_a_sound() {
        let mut x = vec![[0.0f32; 2]; 10_000];
        for start in [1000, 5000, 5100] {
            for (k, f) in x[start..start + 50].iter_mut().enumerate() {
                *f = [0.5 * (k as f32 * 0.3).sin(); 2];
            }
        }
        // 5100 follows 5000's burst after a gap of 50 frames: under the gap asked for.
        let found = onsets(&x, 0.01, 200);
        assert_eq!(found.len(), 2);
        assert!((1000..1010).contains(&found[0]) && (5000..5010).contains(&found[1]), "{found:?}");
        assert_eq!(settles(&x, 100, -80.0), Some(5200));
        assert_eq!(settles(&sine(1000, 440.0, 0.5), 100, -80.0), None);
    }

    #[test]
    fn rises_are_found_over_a_sound_that_goes_on() {
        // A sine at -26 dBFS that beats (dipping 20 dB every 2,400 frames), and a burst 16 dB louder
        // every 24,000 frames: two of them 100 frames apart, under the gap asked for.
        let mut x = sine(100_000, 440.0, 0.05);
        for (i, f) in x.iter_mut().enumerate() {
            let g = if (i / 1_200) % 2 == 1 { 0.1 } else { 1.0 };
            *f = [f[0] * g, f[1] * g];
        }
        for start in [10_000, 34_000, 34_100, 58_000] {
            for (k, f) in x[start..start + 2_000].iter_mut().enumerate() {
                *f = [f[0] + 0.3 * (k as f32 * 0.2).sin(); 2];
            }
        }
        let found = rises(&x, 240, 10, 6.0, -40.0, 4_800);
        assert_eq!(found.len(), 3, "{found:?}");
        for (f, want) in found.iter().zip([10_000u64, 34_000, 58_000]) {
            assert!(f.abs_diff(want) <= 480, "{found:?}");
        }
        let naive = rises(&x, 240, 2, 6.0, -40.0, 4_800);
        assert!(naive.len() > 10, "against the level just before, the beating starts too: {naive:?}");
        assert!(rises(&sine(50_000, 440.0, 0.5), 240, 10, 6.0, -40.0, 4_800).is_empty(), "a steady sound starts nowhere");
    }

    #[test]
    fn summaries_use_the_nearest_rank() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(Summary::of(&v), Summary { n: 100, p50: 50.0, p95: 95.0, max: 100.0 });
        assert_eq!(Summary::of(&[7.0]).p95, 7.0);
        let mut a = Rng::new(3);
        let mut b = Rng::new(3);
        let x: Vec<f64> = (0..5).map(|_| a.range(0.0, 10.0)).collect();
        assert!(x.iter().all(|v| (0.0..10.0).contains(v)));
        assert_eq!(x, (0..5).map(|_| b.range(0.0, 10.0)).collect::<Vec<_>>());
    }
}

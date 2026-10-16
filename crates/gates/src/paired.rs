//! G7f's verdicts from paired runs (RENDER-32). One run's p99 varies far more between runs than the
//! rule's bound (on the Mac, ±200 µs a block and ±1.5 ms a callback), so a single run judged against
//! a record decides nothing. Instead each run measures the app's layout and S, the commit before
//! step 7, in turns over the same music, so what disturbs one disturbs the other alike, and takes
//! the median of the pairs' differences. S plays in a runner of its own (`baseline`), and the two
//! take turns over a pipe as players in one process do. Step 7's first layout, P1, takes its turns
//! here too, so each run also says what step 7 costs over S with wasmtime's default layout, and
//! what the owner's layout costs over P1 (printed, not judged).
//!
//! Even paired, the p99 moves too much to settle in the pairs the Mac's time allows: on Linux the
//! p99's differences spread over hundreds of µs (a block) to thousands (a callback) between pairs on
//! a quiet machine, while the mean's stay within about ±12 µs (a block) and ±100 µs (a callback).
//! So the mean is judged against the rule's bound, and the p99's difference is printed beside it.

use std::fmt;

use anyhow::{Result, bail};

use crate::baseline::wire::Heard;

/// The median: the middle value, or the mean of the two middle ones; 0 when there is none.
pub fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

/// Nearest rank: the smallest value at or above share `p` of `v`; 0 when it is empty.
pub fn rank(v: &[f64], p: f64) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(((p * v.len() as f64).ceil() as usize).clamp(1, v.len().max(1)) - 1).copied().unwrap_or(0.0)
}

/// One statistic's differences, pair by pair: one side minus another (the app's layout minus S), in µs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diffs(pub Vec<f64>);

impl Diffs {
    /// The pairs' differences of `stat` over each side's runs.
    pub fn of(one: &[Vec<f64>], other: &[Vec<f64>], stat: impl Fn(&[f64]) -> f64) -> Diffs {
        Diffs(one.iter().zip(other).map(|(a, b)| stat(a) - stat(b)).collect())
    }

    pub fn median(&self) -> f64 {
        median(&self.0)
    }
}

impl fmt::Display for Diffs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (lo, hi) = (rank(&self.0, 0.0), rank(&self.0, 1.0));
        write!(f, "{:+.0} µs ({} pairs, {lo:+.0} to {hi:+.0})", self.median(), self.0.len())
    }
}

/// A line `<tag> key=value … machine=<machine>` of `text`: its values and machine, which is last
/// on the line since it holds spaces.
pub fn line(text: &str, tag: &str) -> Option<(Vec<(String, f64)>, String)> {
    let line = text.lines().find_map(|l| l.trim().strip_prefix(tag)?.strip_prefix(' '))?;
    let (line, machine) = line.split_once(" machine=")?;
    let machine = Some(machine.trim().to_string()).filter(|m| !m.is_empty())?;
    let values = line.split_whitespace().map(|w| w.split_once('=').and_then(|(k, v)| Some((k.to_string(), v.parse().ok()?))));
    Some((values.collect::<Option<_>>()?, machine))
}

/// The value of `key` among `values`.
pub fn value(values: &[(String, f64)], key: &str) -> Option<f64> {
    values.iter().find(|(k, _)| k == key).map(|(_, v)| *v)
}

/// The judged line of a G7f part: the median of the pairs' mean differences from S against
/// `bound`, then the p99's, printed. The gates' summary keeps 220 characters of a line.
pub fn judge([mean, p99]: [&Diffs; 2], bound: f64) -> (bool, String) {
    (mean.median() <= bound, format!("{mean} over S, measured here (bound {bound:.0}). p99, not judged: {p99}"))
}

/// One side of the turns: a player or a renderer, here or in S's runner.
pub trait Side {
    /// `n` callbacks or blocks, not timed.
    fn warm(&mut self, n: usize) -> Result<()>;
    /// `n` more, each one's time in µs; what each played is added to the side's [`Heard`] off the
    /// clock.
    fn turn(&mut self, n: usize) -> Result<Vec<f64>>;
    /// What the timed ones played so far.
    fn heard(&mut self) -> Result<Heard>;
}

/// Every order `k` sides can take a turn in, so that over the whole cycle each side is first,
/// second … as often as every other.
pub fn orders(k: usize) -> Vec<Vec<usize>> {
    if k <= 1 {
        return vec![(0..k).collect()];
    }
    let mut all = vec![];
    for first in 0..k {
        for rest in orders(k - 1) {
            all.push(std::iter::once(first).chain(rest.into_iter().map(|i| if i >= first { i + 1 } else { i })).collect());
        }
    }
    all
}

/// Each side's times over `n` callbacks or blocks, after `warm` untimed ones each: in turns of
/// `turn`, each turn in the next of [`orders`], round `round` starting `round` orders on, so what
/// disturbs one side disturbs the others alike and none always meets caches another left cold.
///
/// A side in S's runner takes its turns in another process, so on another thread. That adds no
/// bias of its own: with the app's own engine on both sides, here and in a runner, both processes
/// held on one die of the Linux machine's CPU, they differed by -4 µs a block (5 pairs, -5 to +1)
/// and -10 µs a callback (-14 to -3). Not held, where the scheduler puts the two processes moves
/// that by up to about 100 µs a callback from one run to the next.
pub fn in_turns(sides: &mut [&mut dyn Side], warm: usize, n: usize, turn: usize, round: usize) -> Result<Vec<Vec<f64>>> {
    if turn == 0 {
        bail!("a turn of no steps");
    }
    for side in sides.iter_mut() {
        side.warm(warm)?;
    }
    let orders = orders(sides.len());
    let mut times = vec![Vec::with_capacity(n); sides.len()];
    for (t, from) in (0..n).step_by(turn).enumerate() {
        let m = turn.min(n - from);
        for &i in &orders[(t + round) % orders.len()] {
            let got = sides[i].turn(m)?;
            if got.len() != m {
                bail!("a turn of {m} timed {}", got.len());
            }
            times[i].extend(got);
        }
    }
    Ok(times)
}

/// Whether every side in every round played the same bits (the app's layout, P1 and S, in that
/// order), and that said: the comparison means something only then.
pub fn same_music(heard: &[[Heard; 3]]) -> (bool, String) {
    let first = heard.first().map(|h| h[0]).unwrap_or_default();
    let names = ["the app's layout", "P1", "S"];
    let other = heard.iter().enumerate().find_map(|(i, h)| (0..3).find(|&k| h[k] != first).map(|k| (i, k, h[k])));
    match other {
        None if first.samples == 0 => (false, "nothing was heard".to_string()),
        None => {
            (true, format!("the same {} samples (hash {:016x}) from the app's layout, P1 and S in every round", first.samples, first.hash))
        }
        Some((i, k, h)) => (
            false,
            format!(
                "round {}: {} played {} samples (hash {:016x}), the app's layout in round 1 {} ({:016x})",
                i + 1,
                names[k],
                h.samples,
                h.hash,
                first.samples,
                first.hash
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-32: the median of the pairs' differences, so one disturbed pair moves nothing.
    #[test]
    fn the_median_of_the_pairs_decides() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
        let d = Diffs::of(&[vec![10.0], vec![12.0], vec![900.0]], &[vec![0.0], vec![0.0], vec![0.0]], |v| v[0]);
        assert_eq!(d.median(), 12.0, "{d}");
        // G7f part 1: the app's layout's mean +72 over S, measured in the same run, is inside 80;
        // +81 isn't. The p99's difference is printed, whatever it is, and never judged.
        let p99 = Diffs(vec![900.0, -400.0, 300.0]);
        let (pass, said) = judge([&Diffs(vec![72.0, 70.0, 77.0]), &p99], 80.0);
        assert!(pass, "{said}");
        assert_eq!(said, "+72 µs (3 pairs, +70 to +77) over S, measured here (bound 80). p99, not judged: +300 µs (3 pairs, -400 to +900)");
        assert!(!judge([&Diffs(vec![81.0, 82.0, 47.0]), &p99], 80.0).0);
        assert!(judge([&Diffs(vec![80.0, 300.0, -20.0]), &Diffs(vec![5_000.0; 3])], 80.0).0, "the bound itself passes");
        // G7f part 2: the Mac's run at ed7fc8e, had S been measured there: its +328 fails 320.
        assert!(!judge([&Diffs(vec![328.0, 300.0, 364.0]), &p99], 320.0).0);
    }

    /// A side whose every step takes `us` and plays its own number, logging each call in `log`.
    struct Fake<'a> {
        n: usize,
        us: f64,
        log: &'a std::cell::RefCell<Vec<String>>,
        heard: Heard,
    }

    impl Side for Fake<'_> {
        fn warm(&mut self, n: usize) -> Result<()> {
            self.log.borrow_mut().push(format!("warm {} {n}", self.n));
            Ok(())
        }
        fn turn(&mut self, n: usize) -> Result<Vec<f64>> {
            self.log.borrow_mut().push(format!("{} {n}", self.n));
            self.heard.add(&vec![self.n as f32; n]);
            Ok(vec![self.us; n])
        }
        fn heard(&mut self) -> Result<Heard> {
            Ok(self.heard)
        }
    }

    // RENDER-32: the comparison counts only when the three played the same bits in every round,
    // and says which played something else, and where.
    #[test]
    fn the_same_music_is_the_same_bits() {
        let (mut a, mut b) = (Heard::default(), Heard::default());
        a.add(&[0.25; 8]);
        b.add(&[0.25; 7]);
        b.add(&[f32::from_bits(0.25f32.to_bits() + 1)]);
        let (same, said) = same_music(&[[a; 3], [a; 3]]);
        assert!(same, "{said}");
        assert_eq!(said, format!("the same 8 samples (hash {:016x}) from the app's layout, P1 and S in every round", a.hash));
        let (same, said) = same_music(&[[a; 3], [a, a, b]]);
        assert!(!same);
        assert!(said.starts_with("round 2: S played 8 samples (hash "), "{said}");
        assert_eq!(same_music(&[[Heard::default(); 3]]), (false, "nothing was heard".to_string()));
        assert!(!same_music(&[]).0);
    }

    // RENDER-32: every order sides can take, each side as often in each place.
    #[test]
    fn every_order_is_taken() {
        assert_eq!(orders(2), [[0, 1], [1, 0]]);
        let three = orders(3);
        assert_eq!(three, [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]);
        for place in 0..3 {
            for side in 0..3 {
                assert_eq!(three.iter().filter(|o| o[place] == side).count(), 2, "side {side} in place {place}");
            }
        }
        assert_eq!(orders(1), [[0]]);
    }

    // RENDER-32: the sides warm up, then take turns of `turn` steps, each turn in the next order
    // (each round starting one order on), the last turn short; each side's times in its own list,
    // all `n` of them. Two sides take turns as G7f's pairs did: the first first in even turns of
    // even rounds.
    #[test]
    fn the_sides_take_turns() {
        let log = std::cell::RefCell::new(vec![]);
        let side = |n, us| Fake { n, us, log: &log, heard: Heard::default() };
        let (mut a, mut b, mut c) = (side(0, 1.0), side(1, 2.0), side(2, 3.0));
        let times = in_turns(&mut [&mut a, &mut b, &mut c], 5, 7, 3, 1).unwrap();
        assert_eq!(times, [vec![1.0; 7], vec![2.0; 7], vec![3.0; 7]]);
        assert_eq!(
            log.borrow().join(", "),
            "warm 0 5, warm 1 5, warm 2 5, 0 3, 2 3, 1 3, 1 3, 0 3, 2 3, 1 1, 2 1, 0 1",
            "round 1 starts at the second order"
        );
        let mut want = Heard::default();
        want.add(&[1.0; 7]);
        assert_eq!(b.heard().unwrap(), want);
        log.borrow_mut().clear();
        let (mut a, mut b) = (side(0, 1.0), side(1, 2.0));
        in_turns(&mut [&mut a, &mut b], 0, 6, 2, 0).unwrap();
        assert_eq!(log.borrow().join(", "), "warm 0 0, warm 1 0, 0 2, 1 2, 1 2, 0 2, 0 2, 1 2");
        assert!(in_turns(&mut [&mut a], 0, 6, 0, 0).is_err(), "a turn of no steps");
    }

    // A side that times fewer steps than it was asked for stops the run, never shifts the pairs.
    #[test]
    fn a_short_turn_is_an_error() {
        struct Short;
        impl Side for Short {
            fn warm(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn turn(&mut self, n: usize) -> Result<Vec<f64>> {
                Ok(vec![1.0; n - 1])
            }
            fn heard(&mut self) -> Result<Heard> {
                Ok(Heard::default())
            }
        }
        let e = in_turns(&mut [&mut Short], 0, 4, 2, 0).unwrap_err();
        assert_eq!(e.to_string(), "a turn of 2 timed 1");
    }

    // RENDER-32, PLAT-1: a recorded estimate reads back with its machine, and needs one.
    #[test]
    fn a_recorded_estimate_names_its_machine() {
        let text = "noise\nstore-p1 p99_us=35.000 raise_us=0.000 machine=MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads\n";
        let (values, machine) = line(text, "store-p1").unwrap();
        assert_eq!((value(&values, "p99_us"), value(&values, "raise_us")), (Some(35.0), Some(0.0)));
        assert_eq!(machine, "MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads");
        assert_eq!(line("store-p1 p99_us=35.000", "store-p1"), None, "a record names its machine");
        assert_eq!(line("store-p1s p99_us=1 machine=x", "store-p1"), None, "another tag");
    }
}

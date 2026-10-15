//! G7f's verdicts from paired runs (RENDER-32). One run's p99 varies far more between runs than the
//! rule's bound (on the Mac, ±200 µs a block and ±1.5 ms a callback), so a single run judged against
//! a record decides nothing. Instead the gate measures the app's layout and P1 in turns in one
//! process, over the same music, so what disturbs one disturbs the other alike, and takes the
//! median of the pairs' differences. P1 against the baseline (S, or HEAD's callback) is what the
//! reference Mac's own alternated campaign recorded; the two add up to the app's layout against the
//! baseline.
//!
//! Even paired, the p99 moves too much to settle in the pairs the Mac's time allows: on Linux the
//! p99's differences spread over hundreds of µs (a block) to thousands (a callback) between pairs on
//! a quiet machine, while the mean's stay within about ±12 µs (a block) and ±100 µs (a callback).
//! So the mean is judged against the rule's bound, and the p99's estimate is printed beside it.

use std::fmt;

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

/// One statistic's differences, pair by pair: the app's layout minus P1, in µs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diffs(pub Vec<f64>);

impl Diffs {
    /// The pairs' differences of `stat` over each side's runs.
    pub fn of(app: &[Vec<f64>], p1: &[Vec<f64>], stat: impl Fn(&[f64]) -> f64) -> Diffs {
        Diffs(app.iter().zip(p1).map(|(a, b)| stat(a) - stat(b)).collect())
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

/// The app's layout against the baseline: what it costs over P1 here (the pairs' median), plus what
/// P1 cost over the baseline on the reference Mac (`recorded`), and that said briefly: the gates'
/// summary keeps 220 characters of a line.
pub fn estimate(here: &Diffs, recorded: f64) -> (f64, String) {
    let total = here.median() + recorded;
    (total, format!("{total:+.0} µs: {here} here over P1, and P1 {recorded:+.0} on the Mac"))
}

/// The judged line of a G7f part: the mean's estimate against `bound`, then the p99's, printed.
pub fn judge([mean, p99]: [&Diffs; 2], [mean_p1, p99_p1]: [f64; 2], bound: f64) -> (bool, String) {
    let ((total, said), (_, p99_said)) = (estimate(mean, mean_p1), estimate(p99, p99_p1));
    (total <= bound, format!("{said} (bound {bound:.0}). p99, not judged: {p99_said}"))
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
        // G7f part 1: the app's layout's mean +40 here and P1's +32 on the Mac is +72, inside 80;
        // +50 isn't. The p99's estimate is printed, whatever it is.
        let p99 = Diffs(vec![900.0, -400.0, 300.0]);
        let (pass, said) = judge([&Diffs(vec![40.0, 38.0, 45.0]), &p99], [32.0, 35.0], 80.0);
        assert!(pass, "{said}");
        assert_eq!(
            said,
            "+72 µs: +40 µs (3 pairs, +38 to +45) here over P1, and P1 +32 on the Mac (bound 80). p99, not judged: +335 µs: +300 µs (3 pairs, -400 to +900) here over P1, and P1 +35 on the Mac"
        );
        assert!(!judge([&Diffs(vec![50.0, 52.0, 47.0]), &p99], [32.0, 35.0], 80.0).0);
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

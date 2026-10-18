//! G7f's verdicts from paired runs (RENDER-32). One run's p99 varies far more between runs than the
//! rule's bound (on the Mac, ±200 µs a block and ±1.5 ms a callback), so a single run judged against
//! a record decides nothing. Instead each run measures the app's layout and S, the commit before
//! step 7, in turns over the same music, so what disturbs one disturbs the other alike, and takes
//! the median of the pairs' differences. S plays in a runner of its own (`baseline`), and the two
//! take turns over a pipe as players in one process do. A second player of the app's layout, its
//! A/A control, takes its turns here too, so each run also says what noise alone gives (printed,
//! not judged), and three sides take turns (`cycle`).
//!
//! Even paired, the p99 moves too much to settle in the pairs the Mac's time allows: on Linux the
//! p99's differences spread over hundreds of µs (a block) to thousands (a callback) between pairs on
//! a quiet machine, while the mean's stay within about ±12 µs (a block) and ±100 µs (a callback).
//! So the mean is judged against the rule's bound, and the p99's difference is printed beside it.

use std::fmt;

use anyhow::{Result, bail};

use crate::baseline::wire::Heard;
use crate::report::{Outcome, same_machine};

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
/// `bound`, then the p99's, printed. The gates' summary keeps 220 bytes of a line, so whatever
/// [`timed`] adds, wherever it is, comes after it.
pub fn judge([mean, p99]: [&Diffs; 2], bound: f64) -> (bool, String) {
    (mean.median() <= bound, format!("{mean} over S, measured here (bound {bound:.0}). p99, not judged: {p99}"))
}

/// Where a G7f timing is judged (PLAT-1).
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// The reference Mac, the machine S's records name: judged.
    Reference,
    /// A Mac those records don't name, and why: printed, not judged, as on any machine but the
    /// reference Mac (PLAT-1), saying what to do if it is the reference Mac now. ci/check.sh then
    /// marks the gates row PART, and its first line PART, not PASS, with exit status 1: so a Mac's
    /// run never passes with G7f unjudged.
    OtherMac(&'static str),
    /// Any other machine: printed with what they would say, not judged.
    Elsewhere,
}

impl Place {
    /// Where a run on machine `now` stands, given the machine S's record names (`then`, none
    /// without a record). Both are `m2_os::process::machine()` lines, whose second part is the OS.
    pub fn of(then: Option<&str>, now: &str) -> Place {
        let mac = now.split(", ").nth(1).is_some_and(|os| os.starts_with("macOS "));
        match then {
            Some(then) if same_machine(then, now) => Place::Reference,
            Some(_) if mac => Place::OtherMac("not the Mac S's records name"),
            None if mac => Place::OtherMac("no record of S"),
            _ => Place::Elsewhere,
        }
    }
}

/// What a check says on a Mac S's records don't name (`why`). ci/check.sh looks for its first words.
pub fn not_the_reference(why: &str) -> String {
    format!(
        "not judged on this Mac (PLAT-1), {why}: if this is the reference Mac, copy its machine line into crates/gates/store-S.txt and callback-S.txt"
    )
}

/// A G7f timing check (PLAT-1): judged on the reference Mac, the machine S's records name;
/// elsewhere printed with its numbers and what it would say, not judged, and on another Mac with
/// what to do if it is the reference Mac now. Whether it passed.
pub fn timed(out: &mut Outcome, name: &str, place: &Place, (pass, detail): (bool, String)) -> bool {
    let would = if pass { "would pass" } else { "would fail" };
    match place {
        Place::Reference => out.check(name, pass, detail),
        Place::OtherMac(why) => {
            out.skip(name, format!("{would}: {detail}; {}", not_the_reference(why)));
            false
        }
        Place::Elsewhere => {
            out.skip(name, format!("{would}: {detail}; not judged off the reference Mac (PLAT-1)"));
            false
        }
    }
}

/// One side of the turns: a player or a renderer, here or in S's runner.
pub trait Side {
    /// `n` callbacks or blocks, not timed.
    fn warm(&mut self, n: usize) -> Result<()>;
    /// `lead` more, then `n` more: each one's time in µs, the lead's first. What each of the `n`
    /// played is added to the side's [`Heard`] off the clock; what the lead played is not.
    fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>>;
    /// What the `n` steps of every turn played so far.
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

/// Every one of [`orders`], arranged so that no side takes two turns in a row, from the last order
/// back to the first too: every turn starts just after another side's. Where one order ends and
/// the next starts, each side follows each other side as often, so each starts its turns after
/// each other side as often. None for fewer than three sides, which can't do that.
///
/// And the first side and the last, the two G7f compares, wait alike for their turns ([`waits`]):
/// as many turns of one as of the other come with one other turn between it and its own before,
/// as many with two, and so on. One side must come back sooner than the others (with three sides,
/// four of its six turns with one other turn between), and so it is the middle one, the A/A control
/// in G7f, which isn't judged. The sides are renamed to get that, which keeps all of the above, where some
/// naming can (with three sides, one can).
pub fn cycle(k: usize) -> Option<Vec<Vec<usize>>> {
    /// Extend `path` (indices into `all`) to the whole cycle, `joins[a][b]` counting where `b`
    /// starts an order just after `a` ends one, at most `most` each.
    fn next(all: &[Vec<usize>], path: &mut Vec<usize>, joins: &mut [Vec<usize>], most: usize) -> bool {
        let last = *all[*path.last().unwrap()].last().unwrap();
        if path.len() == all.len() {
            let first = all[path[0]][0];
            return first != last && joins[last][first] < most;
        }
        for j in 0..all.len() {
            let first = all[j][0];
            if !path.contains(&j) && first != last && joins[last][first] < most {
                joins[last][first] += 1;
                path.push(j);
                if next(all, path, joins, most) {
                    return true;
                }
                path.pop();
                joins[last][first] -= 1;
            }
        }
        false
    }
    if k < 3 {
        return None;
    }
    let all = orders(k);
    // k! joins over k(k-1) pairs of sides.
    let most = all.len() / (k * (k - 1));
    let mut path = vec![0];
    if !next(&all, &mut path, &mut vec![vec![0; k]; k], most) {
        return None;
    }
    let found: Vec<Vec<usize>> = path.iter().map(|&j| all[j].clone()).collect();
    // Side i named to[i]: every order is still there once, and every join as often.
    let named = |to: &Vec<usize>| -> Vec<Vec<usize>> { found.iter().map(|o| o.iter().map(|&i| to[i]).collect()).collect() };
    let alike = orders(k).iter().map(named).find(|c| waits(c, 0) == waits(c, k - 1));
    Some(alike.unwrap_or(found))
}

/// How many other turns come between each turn of `side` and its own turn before it, in `cycle`
/// played round and round, smallest first: how a side waits for its turns.
fn waits(cycle: &[Vec<usize>], side: usize) -> Vec<usize> {
    let played: Vec<usize> = cycle.iter().flatten().copied().collect();
    let n = played.len();
    let mut out: Vec<usize> =
        (0..n).filter(|&i| played[i] == side).map(|i| (1..=n).find(|&d| played[(i + n - d) % n] == side).unwrap_or(n) - 1).collect();
    out.sort_unstable();
    out
}

/// Each side's times from [`in_turns`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Turns {
    /// Each side's judged steps, in the order played.
    pub timed: Vec<Vec<f64>>,
    /// Each side's lead steps, `lead` at the start of each turn, timed but not judged.
    pub lead: Vec<Vec<f64>>,
    /// Lead steps a turn, and judged steps a full turn.
    pub leads: usize,
    pub turn: usize,
}

impl Turns {
    /// What starting a turn costs `side`, in µs: the mean of each turn's first step (judged when
    /// there is no lead) over the mean of its judged steps; and what of it is left in the judged
    /// steps, the mean of the first half of each turn's judged steps over the mean of the second
    /// half's.
    pub fn starts(&self, side: usize) -> (f64, f64) {
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let timed = &self.timed[side];
        let first: Vec<f64> = if self.leads == 0 {
            timed.iter().step_by(self.turn.max(1)).copied().collect()
        } else {
            self.lead[side].iter().step_by(self.leads).copied().collect()
        };
        let (mut early, mut late) = (vec![], vec![]);
        for t in timed.chunks(self.turn.max(1)).filter(|t| t.len() >= 2) {
            let (a, b) = t.split_at(t.len() / 2);
            early.extend_from_slice(a);
            late.extend_from_slice(b);
        }
        (mean(&first) - mean(timed), mean(&early) - mean(&late))
    }
}

/// Each side's times over `n` callbacks or blocks, after `warm` untimed ones each: in turns of
/// `lead` steps then `turn`, each turn in the next order of [`cycle`], round `round` starting
/// `round` orders on, so what disturbs one side disturbs the others alike. Only the `turn` steps
/// are judged.
///
/// No side takes two turns in a row, so each starts every turn on the caches another side left.
/// That costs each a different amount, far more than the bounds: on Linux a turn's first callback
/// took 4 to 7 ms more than the rest, and its first block 2 to 3 ms. Most of it is gone after a few
/// steps, but not all. In the one look by position, held on the 96 MB-cache die of the Linux
/// machine's CPU, the app's layout's cost over S, which its bounds checks make larger once the
/// caches are warm, rose from about 0 to +60 µs a callback over a turn's first 50 callbacks, and to
/// +11 µs a block over its first 200 blocks. So each turn's `lead` steps, about half a second, are
/// played but left out, and timed apart to show what starting a turn costs. Half a second was sized
/// from that look, and at first shown enough only there: it wasn't tried on the 32 MB die, and not
/// held, as `ci/check.sh` and the Mac run, S's first judged callback was still 42 to 121 µs slower
/// against its mean than the others' (three runs). So each run also says whether any start is left
/// in its judged steps ([`Turns::starts`], [`starting`]): on Linux, held on either die or not, none
/// was seen beyond about 25 µs a callback (`crates/gates/s/README.md`). Before, every step was
/// judged and the orders were played one after another: S never followed itself, while the app's
/// layout and P1 each did one turn in six. That favoured the app's layout by about 20 µs a callback
/// on Linux, and made most of the p99's difference.
///
/// A side in S's runner takes its turns in another process. With the app's own engine on both
/// sides (the A/A control in `crates/gates/s/README.md`), held on one die of the Linux machine's
/// CPU, a block came out at 0 within a few µs, but a callback anywhere from -145 to +38 µs from one
/// run to the next: each process runs a little faster or slower as a whole, all run long, which the
/// rounds can't average out. With 2 s turns after the half second (98fc493 to 651cad4), two runs
/// gave +9 and +14. Not measured on the Mac.
pub fn in_turns(sides: &mut [&mut dyn Side], warm: usize, n: usize, turn: usize, lead: usize, round: usize) -> Result<Turns> {
    if turn == 0 {
        bail!("a turn of no steps");
    }
    let Some(cycle) = cycle(sides.len()) else { bail!("taking turns needs three sides or more, so that none follows itself") };
    // In the order before the first turn's, so that the first turn follows another side too.
    for &i in &cycle[(round + cycle.len() - 1) % cycle.len()] {
        sides[i].warm(warm)?;
    }
    let mut out = Turns { timed: vec![Vec::with_capacity(n); sides.len()], lead: vec![vec![]; sides.len()], leads: lead, turn };
    for (t, from) in (0..n).step_by(turn).enumerate() {
        let m = turn.min(n - from);
        for &i in &cycle[(t + round) % cycle.len()] {
            let got = sides[i].turn(lead, m)?;
            if got.len() != lead + m {
                bail!("a turn of {lead} and {m} steps timed {}", got.len());
            }
            out.lead[i].extend_from_slice(&got[..lead]);
            out.timed[i].extend_from_slice(&got[lead..]);
        }
    }
    Ok(out)
}

/// What starting a turn cost each side ([`Turns::starts`], one a round: the app's layout, its A/A
/// control and S),
/// said in a line: the medians over the rounds of what is left of it in the judged steps, each
/// turn's first half over its second, the app's layout's and its control's against S's; and of each turn's
/// first step, in µs over the side's mean. The music changes from step to step, by far more than
/// that, but alike on every side, so only against S's does it tell. If a side's start lasted into
/// its judged steps, its first half would be slower than its second: a value under 0 says S's
/// start lasted longer than that side's, so the judged means favour that side, by about half the
/// value when the start is gone by the second half. One step can't say how long a start lasts;
/// the halves, a thousand steps or more each, can. They come first, as the gates' summary keeps
/// 220 bytes of a line. `lead` is the steps each turn starts with, not judged, and `step` what a
/// step is.
pub fn starting(starts: &[Vec<(f64, f64)>; 3], lead: usize, step: &str) -> String {
    let first = |k: usize| median(&starts[k].iter().map(|s| s.0).collect::<Vec<_>>());
    let left = |k: usize| median(&starts[k].iter().zip(&starts[2]).map(|(a, s)| a.1 - s.1).collect::<Vec<_>>());
    format!(
        "of the judged ones, each turn's first half over its second, against S's: {:+.0} µs (the app's layout), {:+.0} (its control); each turn's first {step} over the mean: {:+.0} µs, {:+.0}, {:+.0} (S); {lead} a turn not judged; medians of {}",
        left(0),
        left(1),
        first(0),
        first(1),
        first(2),
        starts[0].len()
    )
}

/// Output at or under this is silence (-80 dB): three sides that all played silence would play the
/// same bits and time nothing worth comparing.
const QUIET: f32 = 1e-4;

/// Whether every side in every round played the same bits (the app's layout, its A/A control and S, in that
/// order), and not silence, the app's layout's loudest sample being `peak`; and that said: the
/// comparison means something only then. The same bits, so S was as loud.
pub fn same_music(heard: &[[Heard; 3]], peak: f32) -> (bool, String) {
    let first = heard.first().map(|h| h[0]).unwrap_or_default();
    let names = ["the app's layout", "its A/A control", "S"];
    let other = heard.iter().enumerate().find_map(|(i, h)| (0..3).find(|&k| h[k] != first).map(|k| (i, k, h[k])));
    let db = 20.0 * peak.max(1e-12).log10();
    match other {
        None if first.samples == 0 => (false, "nothing was heard".to_string()),
        None if peak <= QUIET => (false, format!("the same {} samples from all three, but silence (peak {db:.0} dB)", first.samples)),
        None => (
            true,
            format!(
                "the same {} samples (hash {:016x}) from the app's layout, its control and S in every round, peak {db:.1} dB",
                first.samples, first.hash
            ),
        ),
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

    /// A side whose every judged step takes `us` and plays its own number, and every lead step
    /// `us` + 100, logging each call in `log`.
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
        fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
            self.log.borrow_mut().push(format!("{} {lead}+{n}", self.n));
            self.heard.add(&vec![self.n as f32; n]);
            Ok([vec![self.us + 100.0; lead], vec![self.us; n]].concat())
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
        let (same, said) = same_music(&[[a; 3], [a; 3]], 0.25);
        assert!(same, "{said}");
        assert_eq!(
            said,
            format!("the same 8 samples (hash {:016x}) from the app's layout, its control and S in every round, peak -12.0 dB", a.hash)
        );
        let (same, said) = same_music(&[[a; 3], [a, a, b]], 0.25);
        assert!(!same);
        assert!(said.starts_with("round 2: S played 8 samples (hash "), "{said}");
        assert_eq!(same_music(&[[Heard::default(); 3]], 0.25), (false, "nothing was heard".to_string()));
        assert!(!same_music(&[], 0.25).0);
        // Three sides that all played silence play the same bits, and time nothing worth comparing.
        let mut quiet = Heard::default();
        quiet.add(&[0.0; 8]);
        assert_eq!(same_music(&[[quiet; 3]], 0.0), (false, "the same 8 samples from all three, but silence (peak -240 dB)".to_string()));
        assert!(!same_music(&[[a; 3]], 1e-4).0, "-80 dB is silence");
    }

    // PLAT-1, RENDER-32: off the reference Mac a G7f timing is printed with its numbers and what it
    // would say, and judged nowhere else; there, it is judged. On a Mac S's records don't name it is
    // printed too, as PLAT-1 says, never passed, and says how to make that Mac the reference.
    #[test]
    fn a_timing_is_judged_on_the_reference_mac_only() {
        use crate::report::Verdict;
        let mut out = Outcome::new("G7f", "RENDER-32", "a test");
        assert!(!timed(&mut out, "block", &Place::Elsewhere, (false, "+109 µs".into())));
        assert!(!timed(&mut out, "build", &Place::Elsewhere, (true, "220 ms".into())));
        assert!(!timed(&mut out, "callback", &Place::Reference, (false, "+356 µs".into())));
        assert!(timed(&mut out, "callback", &Place::Reference, (true, "+200 µs".into())));
        assert!(!timed(&mut out, "callback", &Place::OtherMac("not the Mac S's records name"), (true, "+20 µs".into())));
        let said: Vec<(Verdict, &str)> = out.checks.iter().map(|c| (c.verdict.clone(), c.detail.as_str())).collect();
        assert_eq!(
            said,
            [
                (Verdict::Skip, "would fail: +109 µs; not judged off the reference Mac (PLAT-1)"),
                (Verdict::Skip, "would pass: 220 ms; not judged off the reference Mac (PLAT-1)"),
                (Verdict::Fail, "+356 µs"),
                (Verdict::Pass, "+200 µs"),
                (
                    Verdict::Skip,
                    "would pass: +20 µs; not judged on this Mac (PLAT-1), not the Mac S's records name: if this is the reference Mac, copy its machine line into crates/gates/store-S.txt and callback-S.txt"
                ),
            ]
        );
        // The summary keeps 220 bytes of a line: the numbers come first, then where it isn't judged,
        // on another Mac and elsewhere alike. The numbers of the Linux check at a894448 lost their
        // end when "not judged off the reference Mac" came first.
        let judged = judge([&Diffs(vec![97.0, 56.0, 201.0, 90.0, 110.0]), &Diffs(vec![-614.0, -797.0, 6.0, -700.0, -500.0])], 320.0);
        for place in [Place::OtherMac("not the Mac S's records name"), Place::Elsewhere] {
            let mut out = Outcome::new("G7f", "RENDER-32", "a test");
            timed(&mut out, "callback within S's + 320 µs, by the mean", &place, judged.clone());
            let line = format!("{out}").lines().nth(1).unwrap().to_string();
            let kept = String::from_utf8_lossy(&line.as_bytes()[..220.min(line.len())]).to_string();
            let numbers = "would pass: +97 µs (5 pairs, +56 to +201) over S, measured here (bound 320). p99, not judged: -614 µs (5 pairs, -797 to +6)";
            assert!(kept.contains(numbers), "{place:?}: {kept}");
        }
    }

    // PLAT-1: the reference Mac is the machine S's records name, through an OS update too; a Mac they
    // don't name, or with no record, is another Mac; anything else is elsewhere.
    #[test]
    fn the_reference_mac_is_the_one_the_records_name() {
        let mac = "MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads";
        let linux = "ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D 16-Core Processor, x86_64, 32 threads";
        assert_eq!(Place::of(Some(mac), mac), Place::Reference);
        assert_eq!(Place::of(Some(mac), &mac.replace("26.6.2", "26.7")), Place::Reference, "an OS update");
        let m2 = "Mac14,2, macOS 26.6.2, Apple M2, aarch64, 8 threads";
        assert_eq!(Place::of(Some(mac), m2), Place::OtherMac("not the Mac S's records name"));
        assert_eq!(Place::of(Some(mac), &mac.replace("10 threads", "8 threads")), Place::OtherMac("not the Mac S's records name"));
        assert_eq!(Place::of(None, mac), Place::OtherMac("no record of S"));
        assert_eq!(Place::of(Some(mac), linux), Place::Elsewhere);
        assert_eq!(Place::of(None, linux), Place::Elsewhere);
        assert_eq!(Place::of(Some(linux), linux), Place::Reference, "a record made here judges here");
    }

    // RENDER-32: every order sides can take, each side as often in each place; and all of them in
    // a cycle where no side takes two turns in a row, nor from the cycle's end back to its start.
    // With three sides, each also follows each other side as often.
    #[test]
    fn every_order_is_taken() {
        assert_eq!(orders(2), [[0, 1], [1, 0]]);
        let three = orders(3);
        assert_eq!(three, [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]]);
        assert_eq!(orders(1), [[0]]);
        let cycle3 = cycle(3).unwrap();
        assert_eq!(cycle3, [[0, 2, 1], [0, 1, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0], [1, 0, 2]]);
        // The app's layout and S, the two compared, wait alike between their turns. One side must
        // come back sooner, four of its six turns with one other turn between: the A/A control, not judged.
        assert_eq!(waits(&cycle3, 0), [1, 1, 1, 2, 3, 4]);
        assert_eq!(waits(&cycle3, 2), waits(&cycle3, 0));
        assert_eq!(waits(&cycle3, 1), [1, 1, 1, 1, 4, 4]);
        assert_eq!(waits(&[vec![0, 1], vec![1, 0]], 1), [0, 2], "a side that follows itself waits for no turn");
        for place in 0..3 {
            for side in 0..3 {
                assert_eq!(cycle3.iter().filter(|o| o[place] == side).count(), 2, "side {side} in place {place}");
            }
        }
        // Played round and round: the cycle, then its first side again.
        let played: Vec<usize> = cycle3.iter().flatten().chain([&cycle3[0][0]]).copied().collect();
        for (a, b) in [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)] {
            assert_eq!(played.windows(2).filter(|w| w == &[a, b]).count(), 3, "{b} after {a}");
        }
        assert!(played.windows(2).all(|w| w[0] != w[1]));
        let cycle4 = cycle(4).unwrap();
        let mut sorted = cycle4.clone();
        sorted.sort();
        assert_eq!(sorted, orders(4), "every order once");
        let played: Vec<usize> = cycle4.iter().flatten().chain([&cycle4[0][0]]).copied().collect();
        assert!(played.windows(2).all(|w| w[0] != w[1]));
        assert_eq!((cycle(2), cycle(1), cycle(0)), (None, None, None), "two sides can't take turns without one following itself");
    }

    // RENDER-32: the sides warm up in the order before the first turn's, then take turns of `lead`
    // steps and `turn` more, each turn in the next order of the cycle (each round starting one order
    // on), the last turn short; each side's judged times and lead times apart, all `n` judged ones,
    // and only those heard.
    #[test]
    fn the_sides_take_turns() {
        let log = std::cell::RefCell::new(vec![]);
        let side = |n, us| Fake { n, us, log: &log, heard: Heard::default() };
        let (mut a, mut b, mut c) = (side(0, 1.0), side(1, 2.0), side(2, 3.0));
        let t = in_turns(&mut [&mut a, &mut b, &mut c], 5, 7, 3, 2, 1).unwrap();
        assert_eq!(t.timed, [vec![1.0; 7], vec![2.0; 7], vec![3.0; 7]]);
        assert_eq!(t.lead, [vec![101.0; 6], vec![102.0; 6], vec![103.0; 6]]);
        assert_eq!((t.leads, t.turn), (2, 3));
        assert_eq!(
            log.borrow().join(", "),
            "warm 0 5, warm 2 5, warm 1 5, 0 2+3, 1 2+3, 2 2+3, 1 2+3, 2 2+3, 0 2+3, 2 2+1, 0 2+1, 1 2+1",
            "round 1 starts at the cycle's second order"
        );
        assert_eq!(t.starts(1), (100.0, 0.0));
        let mut want = Heard::default();
        want.add(&[1.0; 7]);
        assert_eq!(b.heard().unwrap(), want);
        log.borrow_mut().clear();
        let (mut a, mut b, mut c) = (side(0, 1.0), side(1, 2.0), side(2, 3.0));
        let t = in_turns(&mut [&mut a, &mut b, &mut c], 0, 3, 3, 0, 0).unwrap();
        assert_eq!(log.borrow().join(", "), "warm 1 0, warm 0 0, warm 2 0, 0 0+3, 2 0+3, 1 0+3", "round 0 warms in the cycle's last order");
        assert_eq!((t.starts(2), t.lead), ((0.0, 0.0), vec![vec![]; 3]));
        assert!(in_turns(&mut [&mut a, &mut b, &mut c], 0, 6, 0, 1, 0).is_err(), "a turn of no steps");
        let e = in_turns(&mut [&mut a, &mut b], 0, 6, 2, 1, 0).unwrap_err();
        assert_eq!(e.to_string(), "taking turns needs three sides or more, so that none follows itself");
    }

    /// One side of a machine whose caches hold one side's work: a step costs `us`, and more on the
    /// first steps after another side's (`cold`, one a step).
    struct Cached<'a> {
        n: usize,
        us: f64,
        cold: Vec<f64>,
        /// The side that played last, and how many steps in a row.
        last: &'a std::cell::Cell<(usize, usize)>,
    }

    impl Cached<'_> {
        fn step(&mut self) -> f64 {
            let (side, run) = self.last.get();
            let run = if side == self.n { run } else { 0 };
            self.last.set((self.n, run + 1));
            self.us + self.cold.get(run).copied().unwrap_or(0.0)
        }
    }

    impl Side for Cached<'_> {
        fn warm(&mut self, n: usize) -> Result<()> {
            for _ in 0..n {
                self.step();
            }
            Ok(())
        }
        fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
            Ok((0..lead + n).map(|_| self.step()).collect())
        }
        fn heard(&mut self) -> Result<Heard> {
            Ok(Heard::default())
        }
    }

    // RENDER-32: three sides that cost the same once their caches are warm come out the same, in
    // every round, though starting a turn costs each a different amount, as it did on Linux (the
    // first callback after another side's turn 4.4 ms over the rest for the app's layout, 5.3 for
    // P1 and 6.3 for S): the lead takes those steps out, and shows what they cost. Judged, they
    // would make S seem 21 µs slower a callback, which is about what the Linux runs leaned.
    #[test]
    fn starting_a_turn_favours_no_side() {
        let last = std::cell::Cell::new((usize::MAX, 0));
        let cold = [vec![4_400.0, 25.0], vec![5_300.0, 355.0], vec![6_300.0, 93.0]];
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        let mut starts: [Vec<(f64, f64)>; 3] = Default::default();
        for round in 0..6 {
            for lead in [2, 0] {
                let mut sides: Vec<Cached> = (0..3).map(|n| Cached { n, us: 7_000.0, cold: cold[n].clone(), last: &last }).collect();
                let [a, b, c] = &mut sides[..] else { unreachable!() };
                let t = in_turns(&mut [a, b, c], 94, 2_820, 94, lead, round).unwrap();
                let means: Vec<f64> = t.timed.iter().map(|v| mean(v)).collect();
                if lead == 2 {
                    assert_eq!(means, [7_000.0; 3], "round {round}");
                    assert_eq!([t.starts(0), t.starts(1), t.starts(2)], [(4_400.0, 0.0), (5_300.0, 0.0), (6_300.0, 0.0)]);
                    (0..3).for_each(|k| starts[k].push(t.starts(k)));
                } else {
                    assert!((means[2] - means[0] - (6_393.0 - 4_425.0) / 94.0).abs() < 1e-6, "round {round}: {means:?}");
                }
            }
        }
        assert_eq!(
            starting(&starts, 2, "callback"),
            "of the judged ones, each turn's first half over its second, against S's: +0 µs (the app's layout), +0 (its control); each turn's first callback over the mean: +4400 µs, +5300, +6300 (S); 2 a turn not judged; medians of 6"
        );
        // But when S's start lasts past the lead, its first 20 judged callbacks 40 µs slow a turn,
        // S's first half comes out 17 µs slower than its second, against the others' none: twice
        // what it takes off the app's layout's cost over S by the mean, as it is gone by the second
        // half. One step can't tell how long it lasts; the halves do.
        let cold = [vec![4_400.0, 25.0], vec![5_300.0, 355.0], [vec![6_300.0, 93.0], vec![40.0; 20]].concat()];
        let mut sides: Vec<Cached> = (0..3).map(|n| Cached { n, us: 7_000.0, cold: cold[n].clone(), last: &last }).collect();
        let [a, b, c] = &mut sides[..] else { unreachable!() };
        let t = in_turns(&mut [a, b, c], 94, 2_820, 94, 2, 0).unwrap();
        let lean = mean(&t.timed[0]) - mean(&t.timed[2]);
        assert!((lean + 20.0 * 40.0 / 94.0).abs() < 1e-6, "{lean}");
        assert!((t.starts(2).1 - 20.0 * 40.0 / 47.0).abs() < 1e-6, "{:?}", t.starts(2));
        let said = starting(&[vec![t.starts(0)], vec![t.starts(1)], vec![t.starts(2)]], 2, "callback");
        assert!(
            said.starts_with(
                "of the judged ones, each turn's first half over its second, against S's: -17 µs (the app's layout), -17 (its control); "
            ),
            "{said}"
        );
        // The gates' summary keeps 220 bytes of a line: the halves, which say whether the lead was
        // enough, come first, so they are kept whole even far from Linux's numbers.
        let far = [vec![(16_182.0, -108.0)], vec![(17_494.0, -123.0)], vec![(16_786.0, 0.0)]];
        let mut out = Outcome::new("G7f", "RENDER-32", "a test");
        out.info("callback, starting a turn", starting(&far, 47, "callback"));
        let line = format!("{out}").lines().nth(1).unwrap().to_string();
        let kept = String::from_utf8_lossy(&line.as_bytes()[..220.min(line.len())]).to_string();
        assert!(
            kept.contains("against S's: -108 µs (the app's layout), -123 (its control); each turn's first callback over the mean: +16182 µs"),
            "{kept}"
        );
    }

    // A side that times fewer steps than it was asked for stops the run, never shifts the pairs.
    #[test]
    fn a_short_turn_is_an_error() {
        struct Short;
        impl Side for Short {
            fn warm(&mut self, _: usize) -> Result<()> {
                Ok(())
            }
            fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
                Ok(vec![1.0; lead + n - 1])
            }
            fn heard(&mut self) -> Result<Heard> {
                Ok(Heard::default())
            }
        }
        let e = in_turns(&mut [&mut Short, &mut Short, &mut Short], 0, 4, 2, 1, 0).unwrap_err();
        assert_eq!(e.to_string(), "a turn of 1 and 2 steps timed 2");
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

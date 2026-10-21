//! G12's offline part (8a): how a song renders on more threads, measured on the runtime itself,
//! with no device and no server. Its live part is [`live`].
//!
//! - **Set-up**: this machine's cores, its load, and whether workers got the background priority.
//! - **Sweep**: each thread count in the list renders the same 20 s of the reference project from
//!   3:00, twice (the second pass in reverse order), back to back: mean, p50, p99 and max block,
//!   the speed-up against one thread, the block's passes, the share of tasks run on workers, and
//!   how long threads waited for a CPU (Linux).
//! - **Bits** (judged): every count renders the same bits as one thread; and the five pure
//!   fixtures, rendered through `Render` on the sweep's largest count, match `e2e/renders.sha256`.
//! - **Paced**: one thread and four, a block every 2,667 µs (128 frames at 48 kHz), the workers
//!   sent to sleep after each, as live playback will: mean, p99, and blocks over the period.
//! - **Partitions**: each partition run alone, timed: their count and width, the work in all, the
//!   critical path, the bound on 2, 4 and 8 threads, the ten slowest, and the song's tail.
//! - **The executor's own cost**: on one thread, step 7's loop, the whole order as one partition,
//!   and the plan's partitions, in turns over the same music (`paired::in_turns`), by the mean.
//! - **Tails** (OS-49): a long delay's and a long reverb's mean block 1 s and 5 min after their note.
//!   What makes subnormals is counted by a runtime test; these times are information.
//!
//! Only the bits are judged. Timings are judged only on the reference Mac (PLAT-1), and none is
//! judged here yet: the owner's Mac run decides what 8a does next.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_editor::render::{Length, Render, Threads};
use m2_runtime::gauge::{How, Phases};
use m2_runtime::{BLOCK, Pool, Renderer, Runner, Transport};

use crate::baseline::wire::Heard;
use crate::gates::Opts;
use crate::paired::{Diffs, Side, in_turns, median, rank};
use crate::report::Outcome;
use crate::{Lab, Song, songs};

pub mod live;

/// Where every timed run starts: 3:00, as G7f's.
const FROM_SECONDS: f64 = 180.0;
/// Blocks rendered before the timed ones: one second.
const WARM: usize = 375;
/// A block's period at 48 kHz: what live playback has for each.
const PERIOD: Duration = Duration::from_nanos(2_666_667);
/// Blocks timed alone, partition by partition.
const PROFILED: usize = 2_000;
/// The executor's cost in turns: each turn this many blocks, after `LEAD` not judged (G7f's).
const TURN: usize = 625;
const LEAD: usize = 188;

/// What `realtime` is asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    /// The thread counts, 1 first.
    pub counts: Vec<usize>,
    /// Seconds timed for each count, each pass.
    pub seconds: f64,
}

impl Default for Sweep {
    fn default() -> Sweep {
        Sweep { counts: vec![1, 2, 3, 4, 6, 8], seconds: 20.0 }
    }
}

/// A `--sweep` list: thread counts from 1 to 64, comma separated, each once; 1 is added (every
/// count is held against it).
pub fn counts(list: &str) -> Result<Vec<usize>> {
    let mut counts = vec![1];
    for part in list.split(',') {
        match part.trim().parse::<usize>() {
            Ok(n @ 1..=64) => counts.push(n),
            _ => bail!("--sweep takes thread counts from 1 to 64, like 1,2,4,8: not `{}`", part.trim()),
        }
    }
    counts.sort_unstable();
    counts.dedup();
    Ok(counts)
}

/// The judged bits line: every count's hash against the first's (one thread's).
pub fn same_bits(hashes: &[(usize, Heard)]) -> (bool, String) {
    let Some(&(_, one)) = hashes.first() else { return (false, "nothing rendered".into()) };
    let differ: Vec<String> = hashes.iter().filter(|(_, h)| *h != one).map(|(n, h)| format!("{n} threads: {:016x}", h.hash)).collect();
    match differ.is_empty() {
        true => (true, format!("every count rendered {:016x} ({} samples), as one thread did", one.hash, one.samples)),
        false => (false, format!("one thread rendered {:016x}; {}", one.hash, differ.join(", "))),
    }
}

/// µs statistics of blocks.
#[derive(Clone, Copy, Debug, Default)]
struct Stats {
    mean: f64,
    p50: f64,
    p99: f64,
    max: f64,
}

impl Stats {
    fn of(us: &[f64]) -> Stats {
        let mean = us.iter().sum::<f64>() / us.len().max(1) as f64;
        Stats { mean, p50: rank(us, 0.5), p99: rank(us, 0.99), max: rank(us, 1.0) }
    }
}

/// A renderer of `song` from 3:00, live (non-finite samples silenced, as the engine plays).
fn from_three(lab: &Lab, song: &Song) -> Result<Renderer> {
    let mut r = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    r.set_strict(false);
    r.set_transport(Transport::starting((FROM_SECONDS * song.rate as f64).round() as u64));
    Ok(r)
}

/// `threads` threads: none but this one for 1.
fn threads(n: usize, background: bool) -> Result<(Runner, Option<Pool>)> {
    if n == 1 {
        return Ok((Runner::one(), None));
    }
    let (pool, runner) = Pool::new(n, m2_runtime::threads::setup(background)).context("starting the pool's threads")?;
    Ok((runner, Some(pool)))
}

/// One count's back-to-back pass.
struct Pass {
    blocks: Vec<f64>,
    phases: Phases,
    heard: Heard,
    /// The share of tasks run on workers.
    on_workers: f64,
    /// µs a block that the process's threads waited for a CPU (Linux).
    waited: Option<f64>,
}

fn pass(lab: &Lab, song: &Song, n: usize, blocks: usize) -> Result<Pass> {
    let mut r = from_three(lab, song)?;
    let (mut runner, _pool) = threads(n, false)?;
    let mut k = 0;
    for _ in 0..WARM {
        r.gauge_block((k * BLOCK) as u64, &mut runner, How::Parts)?;
        k += 1;
    }
    let tasks = |runner: &Runner| {
        let c = runner.counters();
        (c.iter().map(|c| c.tasks).sum::<u64>(), c.iter().skip(1).map(|c| c.tasks).sum::<u64>())
    };
    let (all0, workers0) = tasks(&runner);
    let waited0 = m2_os::process::waited();
    let mut out =
        Pass { blocks: Vec::with_capacity(blocks), phases: Phases::default(), heard: Heard::default(), on_workers: 0.0, waited: None };
    for _ in 0..blocks {
        let t = Instant::now();
        let p = r.gauge_block((k * BLOCK) as u64, &mut runner, How::Parts)?;
        out.blocks.push(t.elapsed().as_secs_f64() * 1e6);
        (out.phases.pre, out.phases.graph, out.phases.post) =
            (out.phases.pre + p.pre, out.phases.graph + p.graph, out.phases.post + p.post);
        out.heard.add_frames(r.last_block());
        k += 1;
    }
    let (all, workers) = tasks(&runner);
    out.on_workers = (workers - workers0) as f64 / (all - all0).max(1) as f64;
    out.waited = waited0.zip(m2_os::process::waited()).map(|(a, b)| b.saturating_sub(a).as_secs_f64() * 1e6 / blocks as f64);
    runner.rest();
    Ok(out)
}

/// One count's paced run: a block every [`PERIOD`], the workers sent to sleep after each.
fn paced(lab: &Lab, song: &Song, n: usize, blocks: usize) -> Result<(Stats, usize)> {
    let mut r = from_three(lab, song)?;
    let (mut runner, _pool) = threads(n, false)?;
    for k in 0..WARM {
        r.gauge_block((k * BLOCK) as u64, &mut runner, How::Parts)?;
    }
    let mut us = Vec::with_capacity(blocks);
    let start = Instant::now();
    for k in 0..blocks {
        let at = start + PERIOD * k as u32;
        if let Some(wait) = at.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        let t = Instant::now();
        r.gauge_block(((WARM + k) * BLOCK) as u64, &mut runner, How::Parts)?;
        us.push(t.elapsed().as_secs_f64() * 1e6);
        runner.rest();
    }
    let over = us.iter().filter(|&&u| u > PERIOD.as_secs_f64() * 1e6).count();
    Ok((Stats::of(&us), over))
}

/// One side of the executor's cost: a renderer whose graph runs `how`, on this thread.
struct Gauged {
    r: Renderer,
    how: How,
    k: usize,
    heard: Heard,
}

impl Side for Gauged {
    fn warm(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.r.gauge_block((self.k * BLOCK) as u64, &mut Runner::one(), self.how)?;
            self.k += 1;
        }
        Ok(())
    }

    fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
        let mut times = Vec::with_capacity(lead + n);
        for i in 0..lead + n {
            let t = Instant::now();
            self.r.gauge_block((self.k * BLOCK) as u64, &mut Runner::one(), self.how)?;
            times.push(t.elapsed().as_secs_f64() * 1e6);
            if i >= lead {
                self.heard.add_frames(self.r.last_block());
            }
            self.k += 1;
        }
        Ok(times)
    }

    fn heard(&mut self) -> Result<Heard> {
        Ok(self.heard)
    }
}

/// G12, offline: the sweep, the bits, paced blocks, the partitions, the executor's own cost and
/// the tails, on `o.reference` (p50 unless asked).
pub fn realtime(lab: &mut Lab, o: &Opts, sweep: &Sweep) -> Result<Outcome> {
    let mut out = Outcome::new("G12 offline", "RENDER-2, RENDER-2a", &format!("the runtime on {}", o.reference.name()));
    let cores = m2_os::process::cores();
    out.info(
        "cores",
        format!("{} fast ({} per cache, {} per last cache), {} slow", cores.fast, cores.per_cache, cores.per_last_cache, cores.slow),
    );
    out.info("load", load());
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let blocks = (sweep.seconds * song.rate as f64 / BLOCK as f64).round().max(1.0) as usize;

    // The sweep: every count, then again in reverse.
    let order: Vec<usize> = sweep.counts.iter().chain(sweep.counts.iter().rev()).copied().collect();
    let mut runs: Vec<(usize, Pass)> = vec![];
    for &n in &order {
        runs.push((n, pass(lab, &song, n, blocks)?));
    }
    let of = |n: usize| runs.iter().filter(move |(m, _)| *m == n).map(|(_, p)| p);
    let one: Vec<Stats> = of(1).map(|p| Stats::of(&p.blocks)).collect();
    let (one_mean, one_p99) =
        (median(&one.iter().map(|s| s.mean).collect::<Vec<_>>()), median(&one.iter().map(|s| s.p99).collect::<Vec<_>>()));
    for &n in &sweep.counts {
        let passes: Vec<&Pass> = of(n).collect();
        let stats: Vec<Stats> = passes.iter().map(|p| Stats::of(&p.blocks)).collect();
        let med = |f: fn(&Stats) -> f64| median(&stats.iter().map(f).collect::<Vec<_>>());
        let (mean, p99) = (med(|s| s.mean), med(|s| s.p99));
        let phase = |f: fn(&Phases) -> u64| median(&passes.iter().map(|p| f(&p.phases) as f64 / 1e3 / blocks as f64).collect::<Vec<_>>());
        let waited = passes.iter().filter_map(|p| p.waited).collect::<Vec<_>>();
        out.info(
            format!("{n} thread{}", if n == 1 { "" } else { "s" }),
            format!(
                "mean {mean:.0} µs, p50 {:.0}, p99 {p99:.0}, max {:.0} (medians of 2 passes of {blocks} blocks); {:.2}x by the mean, {:.2}x by the p99; pre {:.1}, graph {:.0}, post {:.1} µs; {:.0}% of tasks on workers{}",
                med(|s| s.p50),
                med(|s| s.max),
                one_mean / mean,
                one_p99 / p99,
                phase(|p| p.pre),
                phase(|p| p.graph),
                phase(|p| p.post),
                100.0 * median(&passes.iter().map(|p| p.on_workers).collect::<Vec<_>>()),
                if waited.is_empty() { String::new() } else { format!("; waited for a CPU {:.1} µs a block", median(&waited)) }
            ),
        );
    }
    let hashes: Vec<(usize, Heard)> = runs.iter().map(|(n, p)| (*n, p.heard)).collect();
    let (same, said) = same_bits(&hashes);
    out.check("bits: every count renders the same bits as one thread", same, said);
    let most = *sweep.counts.last().expect("1 at least");
    let (same, said) = fixtures(lab, most)?;
    out.check(format!("bits: the pure fixtures on {most} threads match e2e/renders.sha256"), same, said);

    // Paced, as live will run.
    for n in [1, 4] {
        let (s, over) = paced(lab, &song, n, blocks)?;
        out.info(
            format!("paced, {n} thread{}", if n == 1 { "" } else { "s" }),
            format!(
                "a block every 2,667 µs: mean {:.0} µs, p99 {:.0}, max {:.0}; {over} of {blocks} over the period",
                s.mean, s.p99, s.max
            ),
        );
    }

    partitions(lab, &song, &mut out)?;
    cost(lab, &song, blocks, &mut out)?;
    tails(lab, &mut out)?;
    out.info("background priority", if m2_runtime::threads::background_refused() { "refused to a worker" } else { "given (or not asked)" });
    Ok(out)
}

/// The 1, 5 and 15 minute load averages.
fn load() -> String {
    m2_os::process::load_average().map_or_else(|| "not known".into(), |l| format!("{:.2}, {:.2}, {:.2}", l[0], l[1], l[2]))
}

/// The five pure fixtures, rendered through `Render` on `n` threads as `e2e/baseline.sh` renders
/// them (whole, and 6 s from 4 s in), each file's SHA-256 against `e2e/renders.sha256`.
fn fixtures(lab: &mut Lab, n: usize) -> Result<(bool, String)> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let record = std::fs::read_to_string(repo.join("e2e/renders.sha256")).context("reading e2e/renders.sha256")?;
    let want = |file: &str| record.lines().find_map(|l| l.split_once("  ").filter(|(_, f)| f.trim() == file).map(|(h, _)| h.to_string()));
    let out = lab.dir("fixtures");
    std::fs::create_dir_all(&out)?;
    let (mut matched, mut differ) = (0, vec![]);
    for name in ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter", "templates/empty"] {
        let song = lab.compile(&repo.join(name))?;
        for (from, length, suffix) in [(0.0, Length::Song, ""), (4.0, Length::Seconds(6.0), "-from4-for6")] {
            let file = format!("{}{suffix}.wav", name.replace('/', "-"));
            let path = out.join(&file);
            Render::new(std::sync::Arc::new(song.clone()), &lab.packages, song.rate, from, length, Threads::Exactly(n))?
                .write(&path, |_| {})?;
            let got = m2_song::Sha::of_file(&path)?.hex();
            match want(&file) {
                Some(w) if w == got => matched += 1,
                w => differ.push(format!("{file}: {got}, the record {}", w.unwrap_or_else(|| "none".into()))),
            }
        }
    }
    Ok(match differ.is_empty() {
        true => (true, format!("{matched} of {matched} files match")),
        false => (false, format!("{matched} match; {}", differ.join("; "))),
    })
}

/// Each partition run alone for [`PROFILED`] blocks: their shape, the work and the bounds.
fn partitions(lab: &Lab, song: &Song, out: &mut Outcome) -> Result<()> {
    let mut r = from_three(lab, song)?;
    let parts = r.parts();
    for k in 0..WARM {
        r.gauge_block((k * BLOCK) as u64, &mut Runner::one(), How::Parts)?;
    }
    let mut ns: Vec<Vec<f64>> = vec![Vec::with_capacity(PROFILED); parts.len()];
    for k in 0..PROFILED {
        for (p, t) in r.profile_block(((WARM + k) * BLOCK) as u64)?.into_iter().enumerate() {
            ns[p].push(t as f64 / 1e3);
        }
    }
    let means: Vec<f64> = ns.iter().map(|v| v.iter().sum::<f64>() / v.len().max(1) as f64).collect();
    let work: f64 = means.iter().sum();
    // The longest path, by mean: partitions are numbered by label, so walk them in an order where
    // each comes after what it reads.
    let mut finish = vec![f64::NAN; parts.len()];
    while finish.iter().any(|f| f.is_nan()) {
        for (q, p) in parts.iter().enumerate() {
            if finish[q].is_nan() && p.reads.iter().all(|&o| !finish[o].is_nan()) {
                finish[q] = means[q] + p.reads.iter().map(|&o| finish[o]).fold(0.0, f64::max);
            }
        }
    }
    let critical = finish.iter().copied().fold(0.0, f64::max);
    let bound = |n: f64| (work / n).max(critical);
    out.info(
        "partitions",
        format!(
            "{} partitions, {} at once at most; work {work:.0} µs a block, critical path {critical:.0}; bound on 2, 4 and 8 threads {:.0}, {:.0}, {:.0} µs (each alone, {PROFILED} blocks)",
            parts.len(),
            r.width(),
            bound(2.0),
            bound(4.0),
            bound(8.0)
        ),
    );
    let mut slow: Vec<usize> = (0..parts.len()).collect();
    slow.sort_by(|&a, &b| means[b].total_cmp(&means[a]));
    let named = |q: usize| match parts[q].depth {
        0 => parts[q].label.clone(),
        d => format!("{} (depth {d})", parts[q].label),
    };
    let ten: Vec<String> = slow.iter().take(10).map(|&q| format!("{} {:.0}/{:.0}", named(q), means[q], rank(&ns[q], 0.99))).collect();
    out.info("the ten slowest partitions (mean/p99 µs)", ten.join(", "));
    let tail: Vec<usize> = (0..parts.len()).filter(|&q| parts[q].label == "song").collect();
    let tail_us: f64 = tail.iter().map(|&q| means[q]).sum();
    out.info(
        "the song's tail",
        format!(
            "{tail_us:.0} µs a block ({} nodes): {:.0}% of the work",
            tail.iter().map(|&q| parts[q].nodes.len()).sum::<usize>(),
            100.0 * tail_us / work.max(1.0)
        ),
    );
    Ok(())
}

/// The executor's own cost on one thread: step 7's loop, one partition, and the plan's partitions,
/// in turns over the same music.
fn cost(lab: &Lab, song: &Song, blocks: usize, out: &mut Outcome) -> Result<()> {
    let side = |how: How| -> Result<Gauged> { Ok(Gauged { r: from_three(lab, song)?, how, k: 0, heard: Heard::default() }) };
    let (mut old, mut one, mut parts) = (side(How::Old)?, side(How::One)?, side(How::Parts)?);
    let turns = in_turns(&mut [&mut old, &mut one, &mut parts], WARM, blocks, TURN, LEAD, 0)?;
    let heard = [old.heard()?, one.heard()?, parts.heard()?];
    out.check(
        "bits: step 7's loop, one partition and the partitions render the same",
        heard.iter().all(|h| *h == heard[0]),
        format!("{:016x}", heard[0].hash),
    );
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let t = &turns.timed;
    let pairs = |a: usize, b: usize| {
        let (x, y): (Vec<Vec<f64>>, Vec<Vec<f64>>) =
            (t[a].chunks(TURN).map(<[f64]>::to_vec).collect(), t[b].chunks(TURN).map(<[f64]>::to_vec).collect());
        Diffs::of(&x, &y, mean)
    };
    out.info(
        "the executor's own cost, one thread",
        format!(
            "step 7's loop {:.0} µs a block; the partitions {}, one partition {} over it; the partitions over one partition {} (turn by turn, by the mean)",
            mean(&t[0]),
            pairs(2, 0),
            pairs(1, 0),
            pairs(2, 1)
        ),
    );
    Ok(())
}

/// OS-49's tails: a long delay's and a long reverb's mean block 1 s and 5 min after their note.
fn tails(lab: &mut Lab, out: &mut Outcome) -> Result<()> {
    for (fx, name) in [(songs::Fx::LongDelay, "long delay"), (songs::Fx::LongReverb, "long reverb")] {
        let song = lab.project("tail", &songs::tail(fx)).and_then(|d| lab.compile(&d))?;
        let mut r = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
        r.set_strict(false);
        let block = |s: f64| (s * song.rate as f64 / BLOCK as f64) as usize;
        let end = song.data.tempo().seconds_at(song.data.end());
        let windows = [(block(end + 1.0), "1 s"), (block(end + 300.0), "5 min")];
        let last = windows[1].0 + block(1.0);
        let mut times = [vec![], vec![]];
        for k in 0..last {
            let t = Instant::now();
            r.render_block((k * BLOCK) as u64)?;
            let us = t.elapsed().as_secs_f64() * 1e6;
            for (w, (from, _)) in windows.iter().enumerate() {
                if (*from..from + block(1.0)).contains(&k) {
                    times[w].push(us);
                }
            }
        }
        let (a, b) = (Stats::of(&times[0]).mean, Stats::of(&times[1]).mean);
        out.info(
            format!("tail, {name}"),
            format!(
                "{a:.1} µs a block {} after the note, {b:.1} {} after: {:.2}x (OS-49; information)",
                windows[0].1,
                windows[1].1,
                b / a.max(1e-9)
            ),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // G12 (8a): the bits line fails when any count renders other bits than one thread, and names it.
    #[test]
    fn the_sweep_fails_when_a_count_renders_other_bits() {
        let h = |hash: u64| Heard { hash, samples: 100 };
        let (ok, said) = same_bits(&[(1, h(7)), (2, h(7)), (8, h(7)), (1, h(7))]);
        assert!(ok, "{said}");
        let (ok, said) = same_bits(&[(1, h(7)), (2, h(7)), (4, h(9)), (8, h(7))]);
        assert!(!ok && said.contains("4 threads: 0000000000000009") && !said.contains("8 threads"), "{said}");
        assert!(!same_bits(&[]).0, "nothing rendered passes nothing");
    }

    // G12 (8a): `--sweep` reads thread counts from 1 to 64, always with 1 to hold the others
    // against, and refuses anything else in plain words.
    #[test]
    fn a_sweep_list_is_read_and_nonsense_refused() {
        assert_eq!(counts("1,2,4,8").unwrap(), [1, 2, 4, 8]);
        assert_eq!(counts("8, 4,4").unwrap(), [1, 4, 8], "1 is added; repeats count once");
        for bad in ["0", "65", "x", "", "1,,2", "2.5"] {
            let e = counts(bad).unwrap_err().to_string();
            assert!(e.contains("from 1 to 64"), "{bad}: {e}");
        }
    }
}

//! G7f, part 1 (step 7's design, §11): what one wasmtime `Store` per kernel instance costs at p50,
//! measured on the runtime itself, with no server and no device, and the store layout judged by a
//! rule written down before anything was measured (RENDER-32).
//!
//! It uses only what HEAD `8aa301e` already had (`Renderer::new`, `set_transport`, `render_block`,
//! `Drop`, and the `Flat` for counting), so this same file ran in a worktree at HEAD to record
//! layout S, one `Store` per renderer. S's record is read from `store-S.txt` beside this crate's
//! manifest, or from `$M2_STORE_S`; every run prints its own record line (`store-record …`), which
//! is what that file holds. A record names its machine, and the rule compares only with S's record
//! from the same machine (PLAT-1).
//!
//! The app's layout is the owner's (`m2_runtime::packages::Layout::P2`: 32 MiB reserved per
//! memory, bounds-checked). Its build and memory are measured once. Its block time and its drop are
//! measured in pairs with step 7's first layout, P1 (4 GiB reserved per memory, unchecked), in turns
//! in one process (`paired`), and judged as their difference plus P1's cost over S on the
//! reference Mac, recorded in `store-S.txt` from the campaign that measured P1.
//!
//! The rule (§11.3), against S:
//! - the block time is worse by no more than 80 µs, 3% of a 2.67 ms block. The rule names the p99,
//!   but even paired it moves by more than that between pairs, so the mean is judged and the p99's
//!   estimate printed;
//! - the footprint is worse by no more than 64 MB;
//! - at least 2 renderers build side by side, and the address space the process holds with 2
//!   renderers is at most 25% of its limit;
//! - p50's full build, what opening a project costs, is worse by no more than 50%.
//!
//! The two memory rules fail when the OS doesn't say what the process holds: they never pass
//! unmeasured (`m2_os::process::memory`). Batched drops are needed only if the app's layout's drop
//! raises an audio-like thread's p99 more than S's drop does, in every pair.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use m2_ir::{FNode, FOp};
use m2_os::process::{self, Memory};
use m2_runtime::packages::Layout;
use m2_runtime::{BLOCK, Renderer, Transport};

use crate::gates::Opts;
use crate::paired::Diffs;
use crate::report::{Outcome, same_machine};
use crate::{Lab, songs};

/// What the contract's acceptance gate 7 names as p50's instance count.
const CONTRACT_COUNT: usize = 6_176;
/// The block time measured: 30 s from 3:00.
const FROM_SECONDS: f64 = 180.0;
const BLOCKS: usize = 11_250;
/// The most p50 renderers built side by side.
const SIDE_BY_SIDE: usize = 4;
/// Pairs of runs, each the app's layout and P1 over the same 30 s in turns (`paired`): about 5
/// minutes on the reference Mac. On a quiet Linux machine the pairs' mean differences stayed
/// within ±12 µs, so their median moves by a few µs from run to run.
const PAIRS: usize = 5;
/// Blocks one layout renders before the other's turn: 1 s, so a change in the machine's load
/// lasting longer than that meets both alike.
const TURN: usize = 375;
/// Why the memory rules fail where the OS doesn't say what the process holds.
const UNMEASURED: &str = "not measured: this OS doesn't say what the process holds";

/// One layout's numbers, and the machine that measured them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub count: usize,
    /// `Renderer::new` on the reference project.
    pub build_ms: f64,
    /// Physical footprint and address space one renderer adds.
    pub footprint_mb: f64,
    pub reserved_gib: f64,
    /// Address space the whole process holds while two renderers live (a pool reserves its slots
    /// when its engine is made, so only the total compares across layouts), and its limit.
    pub reserved2_gib: f64,
    pub limit_gib: f64,
    pub side_by_side: usize,
    pub mean_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    /// The renderer's drop, and an audio-like thread's p99 block before and during it.
    pub drop_ms: f64,
    pub audio_base_us: f64,
    pub audio_drop_us: f64,
    /// `m2_os::process::machine()`: last on the line, since it holds spaces.
    pub machine: String,
}

/// `Record`'s fields as `key=value` words, in this order.
const KEYS: [&str; 13] = [
    "count",
    "build_ms",
    "footprint_mb",
    "reserved_gib",
    "reserved2_gib",
    "limit_gib",
    "side_by_side",
    "mean_us",
    "p99_us",
    "max_us",
    "drop_ms",
    "audio_base_us",
    "audio_drop_us",
];

impl Record {
    fn values(&self) -> [f64; 13] {
        [
            self.count as f64,
            self.build_ms,
            self.footprint_mb,
            self.reserved_gib,
            self.reserved2_gib,
            self.limit_gib,
            self.side_by_side as f64,
            self.mean_us,
            self.p99_us,
            self.max_us,
            self.drop_ms,
            self.audio_base_us,
            self.audio_drop_us,
        ]
    }

    pub fn line(&self) -> String {
        let words: Vec<String> = KEYS.iter().zip(self.values()).map(|(k, v)| format!("{k}={v:.3}")).collect();
        format!("store-record {} machine={}", words.join(" "), self.machine)
    }

    /// The first `store-record` line of `text`.
    pub fn parse(text: &str) -> Option<Record> {
        let line = text.lines().find_map(|l| l.trim().strip_prefix("store-record "))?;
        let (line, machine) = line.split_once(" machine=")?;
        let machine = machine.trim().to_string();
        if machine.is_empty() {
            return None;
        }
        let mut v = [f64::NAN; 13];
        for w in line.split_whitespace() {
            let (k, x) = w.split_once('=')?;
            if let Some(i) = KEYS.iter().position(|key| *key == k) {
                v[i] = x.parse().ok()?;
            }
        }
        if v.iter().any(|x| x.is_nan()) {
            return None;
        }
        Some(Record {
            count: v[0] as usize,
            build_ms: v[1],
            footprint_mb: v[2],
            reserved_gib: v[3],
            reserved2_gib: v[4],
            limit_gib: v[5],
            side_by_side: v[6] as usize,
            mean_us: v[7],
            p99_us: v[8],
            max_us: v[9],
            drop_ms: v[10],
            audio_base_us: v[11],
            audio_drop_us: v[12],
            machine,
        })
    }
}

/// Kernel instances a renderer of `nodes` builds: top-level kernel nodes, and for each `poly` its
/// voices times its voice graph's kernel nodes. Only nodes in the processing order are built.
fn instances(nodes: &[FNode], order: &[usize]) -> usize {
    order
        .iter()
        .map(|&i| match &nodes[i].op {
            FOp::Kernel { .. } => 1,
            FOp::Poly { voices, voice, .. } => *voices as usize * instances(&voice.nodes, &voice.order),
            _ => 0,
        })
        .sum()
}

/// P1 against S on the reference Mac, from its campaign of alternated runs (RENDER-32): what
/// P1's mean and p99 block, and its drop's raise of another thread's p99, cost over S's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct P1VsS {
    pub mean_us: f64,
    pub p99_us: f64,
    pub raise_us: f64,
}

impl P1VsS {
    /// The first `store-p1` line of `text`, whose machine must be S's record's.
    pub fn parse(text: &str) -> Option<P1VsS> {
        let (values, machine) = crate::paired::line(text, "store-p1")?;
        let s = Record::parse(text)?;
        same_machine(&s.machine, &machine).then_some(())?;
        let get = |k| crate::paired::value(&values, k);
        Some(P1VsS { mean_us: get("mean_us")?, p99_us: get("p99_us")?, raise_us: get("raise_us")? })
    }
}

/// Whether batched drops are needed (§11.3): only if the app's layout's drop raised the audio-like
/// thread's p99 more than S's would, P1's estimated as its own plus `recorded`, in every pair. A
/// raise of a few µs either way is the thread's own jitter, and every pair agreeing by chance is
/// a 1 in 32 event with 5 pairs.
fn needed(raises: &Diffs, recorded: f64) -> bool {
    !raises.0.is_empty() && raises.0.iter().all(|r| r + recorded > 0.0)
}

/// The rule (§11.3) on `rec`, and on `[mean, p99]`, the app's layout's block times against P1's
/// pair by pair: each check into `out`, and whether all passed. A comparison with S needs S's
/// records from this machine, else it is skipped with why (PLAT-1). The memory rules fail when the
/// memory wasn't measured (`memory`: before and after one renderer; `with_two`: with two
/// renderers), whatever S is: they never pass unmeasured.
fn rule(
    out: &mut Outcome,
    rec: &Record,
    [mean, p99]: [&Diffs; 2],
    s: &Result<(Record, P1VsS), String>,
    memory: bool,
    with_two: bool,
) -> bool {
    let against_s = |out: &mut Outcome, name: &str, judge: &dyn Fn(&Record, &P1VsS) -> (bool, String)| match s {
        Ok((s, p1)) => {
            let (ok, detail) = judge(s, p1);
            out.check(name, ok, detail)
        }
        Err(why) => {
            out.skip(name, why);
            false
        }
    };
    // The rule bounds the p99 block, but even the pairs' p99 moves by more than 80 µs: the mean is
    // judged with the same bound, and the p99's estimate printed (`paired`).
    let mut pass =
        against_s(out, "block within S + 80 µs, by the mean", &|_, p1| crate::paired::judge([mean, p99], [p1.mean_us, p1.p99_us], 80.0));
    let footprint = "footprint within S + 64 MB";
    pass &= if memory {
        against_s(out, footprint, &|s, _| {
            (rec.footprint_mb <= s.footprint_mb + 64.0, format!("{:.0} MB against S's {:.0} MB", rec.footprint_mb, s.footprint_mb))
        })
    } else {
        out.check(footprint, false, UNMEASURED)
    };
    // Against the process's own limit, so judged on any machine.
    let side = "2 renderers side by side, reserving at most 25% of the address space";
    pass &= match (rec.side_by_side >= 2, with_two) {
        (false, _) => out.check(side, false, format!("{} built", rec.side_by_side)),
        (true, false) => out.check(side, false, format!("{} built; {UNMEASURED}", rec.side_by_side)),
        (true, true) => {
            let of_s = s.as_ref().map_or(String::new(), |(s, _)| format!(" (S: {:.1}%)", 100.0 * s.reserved2_gib / s.limit_gib));
            let held = 100.0 * rec.reserved2_gib / rec.limit_gib;
            out.check(side, rec.reserved2_gib <= 0.25 * rec.limit_gib, format!("{} built; {held:.2}%{of_s}", rec.side_by_side))
        }
    };
    pass &= against_s(out, "full build within 1.5 × S", &|s, _| {
        (rec.build_ms <= 1.5 * s.build_ms, format!("{:.0} ms against S's {:.0} ms", rec.build_ms, s.build_ms))
    });
    pass
}

const MB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MB;

/// G7f, part 1: the layout this tree builds, measured on `o.reference` (p50 for the gate).
pub fn store(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let song = lab.reference(&o.reference).and_then(|d| lab.compile(&d))?;
    // A sine into a delay: the same small cost every block, so what moves it during the drop is the drop.
    let small = lab.project("sine", &songs::sine(true)).and_then(|d| lab.compile(&d))?;
    let mut out = Outcome::new("G7f store", "RENDER-32", &format!("the runtime on {}", o.reference.name()));
    let mut rec = Record { count: instances(&song.flat.nodes, &song.flat.order), machine: process::machine(), ..Record::default() };
    out.info("kernel instances", format!("{} (acceptance gate 7 names {CONTRACT_COUNT})", rec.count));

    // Build: one renderer, then more side by side.
    let before = process::memory();
    let t = Instant::now();
    let first = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    rec.build_ms = t.elapsed().as_secs_f64() * 1e3;
    let one = process::memory();
    let mut more = vec![first];
    rec.side_by_side = 1;
    let mut two = None;
    let mut failed = None;
    while rec.side_by_side < SIDE_BY_SIDE {
        match Renderer::new(&song.flat, &song.data, &lab.packages, song.rate) {
            Ok(r) => more.push(r),
            Err(e) => {
                failed = Some(format!("{e:#}"));
                break;
            }
        }
        rec.side_by_side += 1;
        if rec.side_by_side == 2 {
            two = process::memory();
        }
    }
    drop(more);
    // Without the process's memory these stay 0, and the memory rules below fail.
    let measured: Option<(Memory, Memory)> = before.zip(one);
    if let Some((b, a)) = measured {
        rec.footprint_mb = a.footprint.saturating_sub(b.footprint) as f64 / MB;
        rec.reserved_gib = a.mapped.saturating_sub(b.mapped) as f64 / GIB;
        rec.limit_gib = a.address_space as f64 / GIB;
    }
    if let Some(a) = two {
        rec.reserved2_gib = a.mapped as f64 / GIB;
    }
    out.info(
        "build",
        format!(
            "{:.0} ms for p50's renderer, {:.1} µs per instance; {} side by side{}",
            rec.build_ms,
            rec.build_ms * 1e3 / rec.count.max(1) as f64,
            rec.side_by_side,
            failed.map_or(String::new(), |e| format!(" (the next failed: {e})"))
        ),
    );
    match (measured, two) {
        (Some(_), Some(_)) => out.info(
            "memory",
            format!(
                "one renderer adds {:.0} MB of footprint and reserves {:.0} GiB; with two the process holds {:.0} GiB, {:.1}% of its {:.0} GiB",
                rec.footprint_mb,
                rec.reserved_gib,
                rec.reserved2_gib,
                100.0 * rec.reserved2_gib / rec.limit_gib.max(1.0),
                rec.limit_gib
            ),
        ),
        (Some(_), None) => out.info(
            "memory",
            format!(
                "one renderer adds {:.0} MB of footprint and reserves {:.0} GiB, of the process's {:.0} GiB",
                rec.footprint_mb, rec.reserved_gib, rec.limit_gib
            ),
        ),
        (None, _) => out.info("memory", UNMEASURED),
    }

    // Block time and the drop, in pairs: the app's layout and P1 in turns over the same music, so
    // what disturbs one disturbs the other alike (RENDER-32).
    let p1 = crate::load_std_laid_out(Layout::P1)?;
    let pairs = PAIRS;
    let (mut runs, mut drops): ([Vec<Vec<f64>>; 2], [Vec<Dropped>; 2]) = Default::default();
    let mut class = None;
    for i in 0..pairs {
        let app = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
        let old = Renderer::new(&song.flat, &song.data, &p1, song.rate)?;
        // Each layout first in every other pair.
        let mut both = [app, old];
        let [a, b] = in_turns(&mut both, i % 2 == 0, song.rate)?;
        out.info(format!("pair {}", i + 1), format!("the app's layout {}; P1 {}", Run::of(&a), Run::of(&b)));
        runs[0].push(a);
        runs[1].push(b);
        // Each layout's drop first in every other pair too.
        let [app, old] = both;
        let (d_app, d_old) = if i % 2 == 0 {
            let d_old = dropped(old, lab, &small)?;
            (dropped(app, lab, &small)?, d_old)
        } else {
            let d_app = dropped(app, lab, &small)?;
            (d_app, dropped(old, lab, &small)?)
        };
        class.get_or_insert(d_app.class.clone());
        drops[0].push(d_app);
        drops[1].push(d_old);
    }
    let (app, old) = (Run::median(&runs[0]), Run::median(&runs[1]));
    (rec.mean_us, rec.p99_us, rec.max_us) = (app.mean, app.p99, app.max);
    out.info(format!("block, the app's layout (medians of {pairs} runs)"), format!("{app} over {BLOCKS} blocks from 3:00"));
    out.info(format!("block, P1 (medians of {pairs} runs)"), format!("{old}"));
    let p99 = Diffs::of(&runs[0], &runs[1], |v| crate::paired::rank(v, 0.99));
    let mean = Diffs::of(&runs[0], &runs[1], |v| v.iter().sum::<f64>() / v.len().max(1) as f64);
    out.info("block, the app's layout against P1, pair by pair", format!("mean {mean}; p99 {p99}"));
    let med = |f: fn(&Dropped) -> f64, d: &[Dropped]| crate::paired::median(&d.iter().map(f).collect::<Vec<_>>());
    (rec.drop_ms, rec.audio_base_us, rec.audio_drop_us) =
        (med(|d| d.ms, &drops[0]), med(|d| d.base_us, &drops[0]), med(|d| d.during_us, &drops[0]));
    let raises = Diffs(drops[0].iter().zip(&drops[1]).map(|(a, b)| a.raise() - b.raise()).collect());
    out.info(
        "drop",
        format!(
            "{:.1} ms (P1 {:.1}); the audio-like thread's p99 block {:.0} µs during it, {:.0} µs before (medians of {pairs}); {}",
            rec.drop_ms,
            med(|d| d.ms, &drops[1]),
            rec.audio_drop_us,
            rec.audio_base_us,
            match class.unwrap_or(Ok(())) {
                Ok(()) => "that thread marked latency-sensitive".to_string(),
                Err(e) => format!("that thread at its default priority ({e})"),
            }
        ),
    );
    out.info("record", rec.line());

    // The rule (§11.3). Its comparisons with S are judged only against S's records from this same
    // machine (PLAT-1); its memory rules fail, never pass, when the memory wasn't measured.
    let path =
        std::env::var_os("M2_STORE_S").map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt"), Into::into);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let s = match (Record::parse(&text), P1VsS::parse(&text)) {
        (None, _) => Err(format!("no record of layout S in {}", path.display())),
        (_, None) => Err(format!("no record of P1 against S in {}", path.display())),
        (Some(s), _) if !same_machine(&s.machine, &rec.machine) => {
            out.info("machines", format!("S's record is from {}; this run is on {}", s.machine, rec.machine));
            Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())
        }
        (Some(s), Some(p)) => Ok((s, p)),
    };
    let pass = rule(&mut out, &rec, [&mean, &p99], &s, measured.is_some(), measured.is_some() && two.is_some());
    let Ok((_, p1_s)) = s else {
        out.info("batched drops", format!("the drop raises the audio-like thread's p99 {raises} more than P1's; not compared with S here"));
        out.info("layout", "not judged here: the rule needs S's records from this machine (§11.3, PLAT-1)");
        return Ok(out);
    };
    out.info(
        "batched drops",
        format!(
            "{}: the drop raises the audio-like thread's p99 {raises} more than P1's here, and P1's {:+.0} µs more than S's on the reference Mac; needed only if more than S's in every pair",
            if needed(&raises, p1_s.raise_us) { "needed" } else { "not needed" },
            p1_s.raise_us
        ),
    );
    out.info(
        "layout",
        if pass { "the app's layout passes the rule (RENDER-32)" } else { "the app's layout fails the rule (§11.3): the owner decides" },
    );
    Ok(out)
}

/// One run's block times: mean, p99 and max, in µs.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    mean: f64,
    p99: f64,
    max: f64,
}

impl Run {
    fn of(times: &[f64]) -> Run {
        let mean = times.iter().sum::<f64>() / times.len().max(1) as f64;
        Run { mean, p99: crate::paired::rank(times, 0.99), max: crate::paired::rank(times, 1.0) }
    }

    /// Each statistic's median over `runs`.
    fn median(runs: &[Vec<f64>]) -> Run {
        let each: Vec<Run> = runs.iter().map(|t| Run::of(t)).collect();
        let m = |f: fn(&Run) -> f64| crate::paired::median(&each.iter().map(f).collect::<Vec<_>>());
        Run { mean: m(|r| r.mean), p99: m(|r| r.p99), max: m(|r| r.max) }
    }
}

impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "mean {:.0} µs, p99 {:.0}, max {:.0}", self.mean, self.p99, self.max)
    }
}

/// The block times of both renderers over the same `BLOCKS` from 3:00, rendered in turns of `TURN`
/// blocks, the first first in even turns when `first_first`, else in odd ones: each as often
/// first, so neither always meets caches the other left cold.
fn in_turns(both: &mut [Renderer; 2], first_first: bool, rate: u32) -> Result<[Vec<f64>; 2]> {
    for r in both.iter_mut() {
        // Live: non-finite samples silenced, as the engine plays.
        r.set_strict(false);
        r.set_transport(Transport::starting((FROM_SECONDS * rate as f64).round() as u64));
    }
    let mut times = [Vec::with_capacity(BLOCKS), Vec::with_capacity(BLOCKS)];
    for (n, from) in (0..BLOCKS).step_by(TURN).enumerate() {
        let order = if (n % 2 == 0) == first_first { [0, 1] } else { [1, 0] };
        for i in order {
            for k in from..(from + TURN).min(BLOCKS) {
                let t = Instant::now();
                both[i].render_block((k * BLOCK) as u64)?;
                times[i].push(t.elapsed().as_secs_f64() * 1e6);
            }
        }
    }
    Ok(times)
}

/// A renderer's drop, off an audio-like thread that renders a small song every 2.67 ms meanwhile.
struct Dropped {
    ms: f64,
    /// That thread's p99 block over the second before the drop, and during it.
    base_us: f64,
    during_us: f64,
    class: std::result::Result<(), String>,
}

impl Dropped {
    /// What the drop adds to the thread's p99 over its own before; none when it adds nothing.
    fn raise(&self) -> f64 {
        (self.during_us - self.base_us).max(0.0)
    }
}

/// Drop `r` while an audio-like thread renders `small`.
fn dropped(r: Renderer, lab: &Lab, small: &crate::Song) -> Result<Dropped> {
    let mut player = Renderer::new(&small.flat, &small.data, &lab.packages, small.rate)?;
    player.set_strict(false);
    let stop = Arc::new(AtomicBool::new(false));
    let period = Duration::from_secs_f64(BLOCK as f64 / small.rate as f64);
    let audio = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            // Marked latency-sensitive, as an audio thread is (on macOS, kept on a performance
            // core), so its blocks time the drop rather than the scheduler. Linux can't: reported.
            let class = m2_os::thread::interactive().map_err(|e| e.to_string());
            let mut blocks: Vec<(Instant, f64)> = Vec::with_capacity(4096);
            let (mut next, mut clock) = (Instant::now(), 0u64);
            while !stop.load(Ordering::Relaxed) {
                let t = Instant::now();
                let ok = player.render_block(clock).is_ok();
                blocks.push((t, t.elapsed().as_secs_f64() * 1e6));
                clock += BLOCK as u64;
                if !ok || blocks.len() == blocks.capacity() {
                    break;
                }
                next += period;
                match next.checked_duration_since(Instant::now()) {
                    Some(d) => std::thread::sleep(d),
                    None => next = Instant::now(),
                }
            }
            (class, blocks)
        })
    };
    std::thread::sleep(Duration::from_secs(3));
    let t0 = Instant::now();
    drop(r);
    let t1 = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::Relaxed);
    let Ok((class, blocks)) = audio.join() else { bail!("the audio-like thread panicked") };
    let base: Vec<f64> = blocks.iter().filter(|b| b.0 < t0 && b.0 + Duration::from_secs(1) >= t0).map(|b| b.1).collect();
    let during: Vec<f64> = blocks.iter().filter(|b| b.0 + Duration::from_secs_f64(b.1 / 1e6) >= t0 && b.0 <= t1).map(|b| b.1).collect();
    let p99 = |v: &[f64]| crate::paired::rank(v, 0.99);
    Ok(Dropped { ms: (t1 - t0).as_secs_f64() * 1e3, base_us: p99(&base), during_us: p99(&during), class })
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-32, PLAT-1: a record line reads back as itself, its machine included.
    #[test]
    fn a_record_reads_back() {
        let machine = "MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads".to_string();
        let r =
            Record { count: 6_060, build_ms: 151.5, side_by_side: 4, p99_us: 812.25, limit_gib: 131_072.0, machine, ..Record::default() };
        assert_eq!(Record::parse(&format!("noise\n{}\n", r.line())), Some(r.clone()));
        assert_eq!(Record::parse("store-record count=1"), None, "every field is needed");
        let unnamed = r.line().split(" machine=").next().unwrap().to_string();
        assert_eq!(Record::parse(&unnamed), None, "a record names its machine");
    }

    // OS-82, RENDER-32: unmeasured, the memory rules fail, never pass, also where S's record is
    // from another machine; measured, the same rules are judged on the numbers.
    #[test]
    fn the_memory_rules_fail_unmeasured() {
        use crate::report::Verdict;
        let here = "a model, an OS, a CPU, aarch64, 8 threads".to_string();
        let rec = Record { side_by_side: 4, machine: here.clone(), ..Record::default() };
        let s = Record {
            p99_us: 3_000.0,
            footprint_mb: 554.0,
            build_ms: 100.0,
            reserved2_gib: 49_636.0,
            limit_gib: 131_068.0,
            machine: here,
            ..Record::default()
        };
        let memory = ["footprint within S + 64 MB", "2 renderers side by side, reserving at most 25% of the address space"];
        let p1 = P1VsS { mean_us: 32.0, p99_us: 35.0, raise_us: 0.0 };
        let (mean, p99) = (Diffs(vec![10.0, 12.0, 8.0]), Diffs(vec![-300.0, 20.0, 250.0]));
        for s in [Ok((s.clone(), p1)), Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())] {
            let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
            assert!(!rule(&mut out, &rec, [&mean, &p99], &s, false, false));
            let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
            assert_eq!(failed, memory, "{out}");
            assert_eq!(out.verdict(), Verdict::Fail);
        }
        let measured =
            Record { p99_us: 3_050.0, footprint_mb: 600.0, build_ms: 120.0, reserved2_gib: 30_000.0, limit_gib: 131_072.0, ..rec.clone() };
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(rule(&mut out, &measured, [&mean, &p99], &Ok((s.clone(), p1)), true, true), "{out}");
        assert_eq!(out.checks.iter().filter(|c| c.verdict == Verdict::Pass).count(), 4, "{out}");

        // RENDER-32, rg-1: the pairs' mean is judged, the app's layout's cost over P1 here plus P1's
        // over S on the Mac: +10 and +32 pass the 80 µs bound, +50 and +32 don't. The p99's estimate
        // is printed, never judged: one far over the bound fails nothing.
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(rule(&mut out, &measured, [&mean, &Diffs(vec![900.0; 3])], &Ok((s.clone(), p1)), true, true), "{out}");
        assert!(out.checks.iter().any(|c| c.verdict == Verdict::Pass && c.detail.contains("p99, not judged: +935 µs")), "{out}");
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(!rule(&mut out, &measured, [&Diffs(vec![50.0, 48.0, 55.0]), &p99], &Ok((s, p1)), true, true), "{out}");
        let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
        assert_eq!(failed, ["block within S + 80 µs, by the mean"], "{out}");
    }

    // RENDER-32 (§11.3), rg-1: batched drops are needed only when the app's layout's drop raises the
    // other thread's p99 more than S's in every pair, not on one pair's jitter.
    #[test]
    fn batched_drops_need_every_pair() {
        assert!(!needed(&Diffs(vec![0.0, 0.0, 1.0, -2.0, 0.0]), 0.0));
        assert!(!needed(&Diffs(vec![5.0, 6.0, 7.0, 8.0, -1.0]), 0.0));
        assert!(needed(&Diffs(vec![5.0, 6.0, 7.0, 8.0, 1.0]), 0.0));
        assert!(!needed(&Diffs(vec![]), 0.0));
    }

    // PLAT-1: S's own record, as checked in, names the reference Mac.
    #[test]
    fn the_checked_in_record_of_s_names_its_machine() {
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt")).unwrap();
        let s = Record::parse(&text).expect("store-S.txt holds a record");
        assert!(s.machine.starts_with("MacBookPro18,1, macOS "), "{}", s.machine);
        // RENDER-32: and P1's cost over S that the paired verdict adds, from the same machine.
        assert_eq!(P1VsS::parse(&text), Some(P1VsS { mean_us: 32.0, p99_us: 35.0, raise_us: 0.0 }));
    }
}

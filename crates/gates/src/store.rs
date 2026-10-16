//! G7f, part 1 (step 7's design, §11): what one wasmtime `Store` per kernel instance costs at p50,
//! measured on the runtime itself, with no server and no device, and the store layout judged by a
//! rule written down before anything was measured (RENDER-32).
//!
//! The app's layout is the owner's (`m2_runtime::packages::Layout::P2`: 32 MiB reserved per
//! memory, bounds-checked). Its build and memory are measured once. Its block time is measured in 5
//! rounds of three renderers taking turns over the same music: the app's layout and step 7's first
//! layout, P1 (4 GiB reserved per memory, unchecked), in this process, and S, HEAD `8aa301e`, one
//! `Store` per renderer, in its runner's (`baseline`), which also times S's first build. No renderer
//! takes two turns in a row, and each turn's first blocks, which meet the caches the renderer before
//! left, are timed apart and not judged. Each round, the three render the same bits, and not
//! silence, which the gate checks. The drop is measured in
//! each round for the app's layout and P1, each while an audio-like thread renders beside it.
//!
//! What the runner can't measure, S's memory, comes from S's record, `store-S.txt` beside this
//! crate's manifest or `$M2_STORE_S`, with P1's drop against S's from the same campaign; every run
//! prints its own record line (`store-record …`), which is what that file holds. A record names its
//! machine, and is used only on the same machine (PLAT-1). The timings are judged only there, the
//! reference Mac; elsewhere they are printed, and on another Mac they also say what to do if it is
//! the reference Mac now.
//!
//! The rule (§11.3), against S:
//! - the block time is worse by no more than 80 µs, 3% of a 2.67 ms block, measured here. The rule
//!   names the p99, but even paired it moves by more than that between pairs, so the mean is judged
//!   and the p99's difference printed;
//! - the footprint is worse by no more than 64 MB, against S's record;
//! - at least 2 renderers build side by side, and the address space the process holds with 2
//!   renderers is at most 25% of its limit;
//! - p50's full build, what opening a project costs, is worse by no more than 50%, each process's
//!   first build, measured here.
//!
//! The two memory rules fail when the OS doesn't say what the process holds: they never pass
//! unmeasured (`m2_os::process::memory`). Batched drops are needed only if the app's layout's drop
//! raises an audio-like thread's p99 more than S's drop does, in every pair: the app's layout's
//! raise over P1's here, plus P1's over S's as recorded.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use m2_ir::{FNode, FOp};
use m2_os::process::{self, Memory};
use m2_runtime::packages::Layout;
use m2_runtime::{BLOCK, Renderer, Transport};

use crate::baseline::{Runner, wire::Heard};
use crate::gates::Opts;
use crate::paired::{Diffs, Place, Side, in_turns, judge, not_the_reference, timed};
use crate::report::{Outcome, same_machine};
use crate::{Lab, songs};

/// What the contract's acceptance gate 7 names as p50's instance count.
const CONTRACT_COUNT: usize = 6_176;
/// The block time measured: 30 s of blocks from 3:01 on, after a second from 3:00 untimed, each turn's
/// lead (not judged) between them.
const FROM_SECONDS: f64 = 180.0;
const BLOCKS: usize = 11_250;
const WARM: usize = 375;
/// The most p50 renderers built side by side.
const SIDE_BY_SIDE: usize = 4;
/// Rounds of the three renderers, each over the same 30 s in turns (`paired`), each with two drops:
/// about 8 minutes on the reference Mac (an estimate: three renderers of about 2.2 ms a block, 38.5 s
/// each a round). On a
/// quiet Linux machine the pairs' mean differences stayed within ±12 µs, so their median moves by a
/// few µs from run to run.
const ROUNDS: usize = 5;
/// Blocks one renderer renders, and judges, before the next one's turn: about 1.67 s, so a change in
/// the machine's load lasting longer than that meets all alike. 11,250 blocks make 18 turns of each
/// renderer, three whole cycles of the orders (`paired::cycle`), so within each round every renderer
/// follows each other one as often.
const TURN: usize = 625;
/// Blocks each turn starts with, before those: half a second, timed but not judged, as they meet the
/// caches the other renderers left (`paired::in_turns`). On Linux the app's layout's cost over S
/// rose from about 0 to +11 µs over a turn's first 200 blocks.
const LEAD: usize = 188;
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

/// What P1's drop raised an audio-like thread's p99 more than S's did, in µs, from the campaign
/// of alternated runs on the machine of S's record (`store-p1` in `text`, RENDER-32): the estimate
/// of S's drop the batched-drops verdict adds to what the app's layout's drop costs over P1's here.
fn p1_raise(text: &str) -> Option<f64> {
    let (values, machine) = crate::paired::line(text, "store-p1")?;
    same_machine(&Record::parse(text)?.machine, &machine).then_some(())?;
    crate::paired::value(&values, "raise_us")
}

/// Whether batched drops are needed (§11.3): only if the app's layout's drop raised the audio-like
/// thread's p99 more than S's would, P1's estimated as its own plus `recorded`, in every pair. A
/// raise of a few µs either way is the thread's own jitter, and every pair agreeing by chance is
/// a 1 in 32 event with 5 pairs.
fn needed(raises: &Diffs, recorded: f64) -> bool {
    !raises.0.is_empty() && raises.0.iter().all(|r| r + recorded > 0.0)
}

/// S where this run judges against it: its first build, measured here, in ms, and its record from
/// this machine for what its runner can't measure, its memory, or why there is none (PLAT-1). The
/// record names the reference Mac: only there are the timings judged (`place`).
struct OfS {
    build_ms: f64,
    record: Result<Record, String>,
    place: Place,
}

/// The rule (§11.3) on `rec`, and on `[mean, p99]`, the app's layout's block times against S's,
/// pair by pair: each check into `out`, and whether all passed. The block time and the build are
/// judged only on the machine S's record names, the reference Mac, and printed elsewhere; the
/// footprint is compared with that record only there too, else skipped with why (PLAT-1). On a Mac
/// the record doesn't name, all three also say what to do if it is the reference Mac now. The
/// memory rules fail when the memory wasn't measured (`memory`: before and after one renderer;
/// `with_two`: with two renderers), whatever S is: they never pass unmeasured.
fn rule(out: &mut Outcome, rec: &Record, [mean, p99]: [&Diffs; 2], s: &OfS, memory: bool, with_two: bool) -> bool {
    // The rule bounds the p99 block, but even the pairs' p99 moves by more than 80 µs: the mean is
    // judged with the same bound, and the p99's difference printed (`paired`).
    let mut pass = timed(out, "block within S + 80 µs, by the mean", &s.place, judge([mean, p99], 80.0));
    let footprint = "footprint within S + 64 MB";
    pass &= match (memory, &s.record) {
        (false, _) => out.check(footprint, false, UNMEASURED),
        (true, Ok(r)) => out.check(
            footprint,
            rec.footprint_mb <= r.footprint_mb + 64.0,
            format!("{:.0} MB against S's {:.0} MB, as recorded on this machine", rec.footprint_mb, r.footprint_mb),
        ),
        (true, Err(why)) => {
            out.skip(footprint, if let Place::OtherMac(other) = s.place { not_the_reference(other) } else { why.clone() });
            false
        }
    };
    // Against the process's own limit, so judged on any machine.
    let side = "2 renderers side by side, reserving at most 25% of the address space";
    pass &= match (rec.side_by_side >= 2, with_two) {
        (false, _) => out.check(side, false, format!("{} built", rec.side_by_side)),
        (true, false) => out.check(side, false, format!("{} built; {UNMEASURED}", rec.side_by_side)),
        (true, true) => {
            let of_s =
                s.record.as_ref().map_or(String::new(), |r| format!(" (S, as recorded: {:.1}%)", 100.0 * r.reserved2_gib / r.limit_gib));
            let held = 100.0 * rec.reserved2_gib / rec.limit_gib;
            out.check(side, rec.reserved2_gib <= 0.25 * rec.limit_gib, format!("{} built; {held:.2}%{of_s}", rec.side_by_side))
        }
    };
    let build = format!("{:.0} ms against S's {:.0} ms, each its process's first, measured here", rec.build_ms, s.build_ms);
    pass &= timed(out, "full build within 1.5 × S", &s.place, (rec.build_ms <= 1.5 * s.build_ms, build));
    pass
}

const MB: f64 = 1024.0 * 1024.0;
const GIB: f64 = 1024.0 * MB;

/// G7f, part 1: the layout this tree builds, measured on `o.reference` (p50 for the gate).
pub fn store(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
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
            "{:.0} ms for {}'s renderer, {:.1} µs per instance; {} side by side{}",
            rec.build_ms,
            o.reference.name(),
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

    // S's first build, in its own process, as the app's above was in this one; then S is played in
    // the rounds below, beside the app's layout.
    let mut s = Runner::start(&lab.work)?;
    out.info("S", s.about());
    let first = s.renderer(&dir, FROM_SECONDS)?;
    s.drop_it()?;
    out.info(
        "S's build",
        format!(
            "{:.0} ms for {}'s renderer, its runner's first; {} kernel instances by the same count",
            first.build_ms,
            o.reference.name(),
            first.count
        ),
    );

    // Block time and the drop, in rounds: the app's layout and P1 here and S in its runner, in turns
    // over the same music, so what disturbs one disturbs the others alike (RENDER-32).
    let p1 = crate::load_std_laid_out(Layout::P1)?;
    let (mut runs, mut heard): ([Vec<Vec<f64>>; 3], Vec<[Heard; 3]>) = Default::default();
    let mut peak = 0.0f32;
    let mut drops: [Vec<Dropped>; 2] = Default::default();
    let (mut s_drops, mut class) = (vec![], None);
    let mut starts: [Vec<(f64, f64)>; 3] = Default::default();
    for i in 0..ROUNDS {
        let mut app = Blocks::new(&lab.packages, &song)?;
        let mut old = Blocks::new(&p1, &song)?;
        s.renderer(&dir, FROM_SECONDS)?;
        let turns = in_turns(&mut [&mut app, &mut old, &mut s], WARM, BLOCKS, TURN, LEAD, i)?;
        for (k, at) in starts.iter_mut().enumerate() {
            at.push(turns.starts(k));
        }
        let times = turns.timed;
        heard.push([app.heard()?, old.heard()?, s.heard()?]);
        peak = peak.max(app.peak);
        s_drops.push(s.drop_it()?);
        out.info(
            format!("round {}", i + 1),
            format!("the app's layout {}; P1 {}; S {}", Run::of(&times[0]), Run::of(&times[1]), Run::of(&times[2])),
        );
        for (run, t) in runs.iter_mut().zip(times) {
            run.push(t);
        }
        // Each layout's drop first in every other round.
        let (app, old) = (app.r, old.r);
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
    drop(s);
    let (same, said) = crate::paired::same_music(&heard, peak);
    out.check("the same music: every renderer's timed blocks the same bits", same, said);
    let app = Run::median(&runs[0]);
    (rec.mean_us, rec.p99_us, rec.max_us) = (app.mean, app.p99, app.max);
    out.info(format!("block, the app's layout (medians of {ROUNDS} runs)"), format!("{app} over {BLOCKS} blocks from 3:01"));
    out.info(format!("block, P1 (medians of {ROUNDS} runs)"), format!("{}", Run::median(&runs[1])));
    out.info(format!("block, S (medians of {ROUNDS} runs)"), format!("{}", Run::median(&runs[2])));
    let against = |a: usize, b: usize| {
        let mean = Diffs::of(&runs[a], &runs[b], |v| v.iter().sum::<f64>() / v.len().max(1) as f64);
        (mean, Diffs::of(&runs[a], &runs[b], |v| crate::paired::rank(v, 0.99)))
    };
    let (mean, p99) = against(0, 2);
    let ((p1_mean, p1_99), (o1_mean, o1_99)) = (against(1, 2), against(0, 1));
    out.info("block, the app's layout against S, pair by pair", format!("mean {mean}; p99 {p99}"));
    out.info("block, P1 against S (step 7 with wasmtime's default layout)", format!("mean {p1_mean}; p99 {p1_99}"));
    out.info("block, the app's layout against P1 (the owner's layout alone)", format!("mean {o1_mean}; p99 {o1_99}"));
    out.info("block, starting a turn", crate::paired::starting(&starts, LEAD, "block"));
    let med = |f: fn(&Dropped) -> f64, d: &[Dropped]| crate::paired::median(&d.iter().map(f).collect::<Vec<_>>());
    (rec.drop_ms, rec.audio_base_us, rec.audio_drop_us) =
        (med(|d| d.ms, &drops[0]), med(|d| d.base_us, &drops[0]), med(|d| d.during_us, &drops[0]));
    let raises = Diffs(drops[0].iter().zip(&drops[1]).map(|(a, b)| a.raise() - b.raise()).collect());
    out.info(
        "drop",
        format!(
            "{:.1} ms (P1 {:.1}, S {:.1} with no thread beside it); the audio-like thread's p99 block {:.0} µs during it, {:.0} µs before (medians of {ROUNDS}); {}",
            rec.drop_ms,
            med(|d| d.ms, &drops[1]),
            crate::paired::median(&s_drops),
            rec.audio_drop_us,
            rec.audio_base_us,
            match class.unwrap_or(Ok(())) {
                Ok(()) => "that thread marked latency-sensitive".to_string(),
                Err(e) => format!("that thread at its default priority ({e})"),
            }
        ),
    );
    out.info("record", rec.line());

    // The rule (§11.3): the block and the build against S measured here, and the footprint against
    // S's record, judged only on the reference Mac, the machine that record names (PLAT-1). Its
    // memory rules fail, never pass, when the memory wasn't measured.
    let path =
        std::env::var_os("M2_STORE_S").map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt"), Into::into);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let place = Place::of(Record::parse(&text).map(|r| r.machine).as_deref(), &rec.machine);
    let record = match Record::parse(&text) {
        None => Err(format!("no record of layout S in {}", path.display())),
        Some(r) if !same_machine(&r.machine, &rec.machine) => {
            out.info("machines", format!("S's record is from {}; this run is on {}", r.machine, rec.machine));
            Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())
        }
        Some(r) => Ok(r),
    };
    let recorded = record.is_ok();
    let line = layout(
        &mut out,
        &rec,
        [&mean, &p99],
        &OfS { build_ms: first.build_ms, record, place },
        measured.is_some(),
        measured.is_some() && two.is_some(),
        same,
    );
    match p1_raise(&text).filter(|_| recorded) {
        Some(raise) => out.info(
            "batched drops",
            format!(
                "{}: the drop raises the audio-like thread's p99 {raises} more than P1's here, and P1's {raise:+.0} µs more than S's as recorded on this machine; needed only if more than S's in every pair",
                if needed(&raises, raise) { "needed" } else { "not needed" },
            ),
        ),
        None => out.info("batched drops", format!("the drop raises the audio-like thread's p99 {raises} more than P1's; not compared with S here")),
    }
    out.info("layout", line);
    Ok(out)
}

/// The rule's checks into `out` ([`rule`]), and its verdict on the layout in words: from the rule's
/// own checks, and none when the three sides didn't play the same music (`same`), as then nothing
/// was compared. A check that failed fails the rule wherever it ran: on a Mac S's records don't
/// name, the 25% rule and an unmeasured footprint are still judged.
fn layout(out: &mut Outcome, rec: &Record, diffs: [&Diffs; 2], s: &OfS, memory: bool, with_two: bool, same: bool) -> String {
    let before = out.checks.len();
    let pass = rule(out, rec, diffs, s, memory, with_two);
    let failed = out.checks[before..].iter().any(|c| c.verdict == crate::report::Verdict::Fail);
    match (same, pass, failed, &s.place) {
        (false, ..) => "not judged: the three didn't play the same music (see above), so nothing was compared".to_string(),
        (true, true, ..) => "the app's layout passes the rule (RENDER-32)".to_string(),
        (true, false, true, _) => "the app's layout fails the rule (§11.3): the owner decides".to_string(),
        (true, false, false, Place::OtherMac(why)) => not_the_reference(why),
        (true, false, false, _) => {
            "not judged in full here: only the reference Mac, which S's record names, judges it (§11.3, PLAT-1)".to_string()
        }
    }
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

/// A renderer here, its kernels built by one layout's packages, its transport at 3:00: one side of
/// the turns, as S's runner renders S on another, with the same calls.
struct Blocks {
    r: Renderer,
    /// The next block's index: the engine clock is `k * BLOCK`.
    k: usize,
    heard: Heard,
    /// The loudest sample of its timed blocks.
    peak: f32,
}

impl Blocks {
    fn new(packages: &m2_runtime::PackageSet, song: &crate::Song) -> Result<Blocks> {
        let mut r = Renderer::new(&song.flat, &song.data, packages, song.rate)?;
        // Live: non-finite samples silenced, as the engine plays.
        r.set_strict(false);
        r.set_transport(Transport::starting((FROM_SECONDS * song.rate as f64).round() as u64));
        Ok(Blocks { r, k: 0, heard: Heard::default(), peak: 0.0 })
    }
}

impl Side for Blocks {
    fn warm(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.r.render_block((self.k * BLOCK) as u64)?;
            self.k += 1;
        }
        Ok(())
    }

    fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
        let mut times = Vec::with_capacity(lead + n);
        for _ in 0..lead {
            let t = Instant::now();
            self.r.render_block((self.k * BLOCK) as u64)?;
            times.push(t.elapsed().as_secs_f64() * 1e6);
            self.k += 1;
        }
        for _ in 0..n {
            let t = Instant::now();
            let out = self.r.render_block((self.k * BLOCK) as u64)?;
            times.push(t.elapsed().as_secs_f64() * 1e6);
            self.heard.add_frames(out);
            self.peak = out.iter().fold(self.peak, |m, f| m.max(f[0].abs()).max(f[1].abs()));
            self.k += 1;
        }
        Ok(times)
    }

    fn heard(&mut self) -> Result<Heard> {
        Ok(self.heard)
    }
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
        let of_s = |record: Result<Record, String>| {
            let place = if record.is_ok() { Place::Reference } else { Place::Elsewhere };
            OfS { build_ms: 100.0, record, place }
        };
        let memory = ["footprint within S + 64 MB", "2 renderers side by side, reserving at most 25% of the address space"];
        let (mean, p99) = (Diffs(vec![10.0, 12.0, 8.0]), Diffs(vec![-300.0, 20.0, 250.0]));
        for record in [Ok(s.clone()), Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())] {
            let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
            assert!(!rule(&mut out, &rec, [&mean, &p99], &of_s(record), false, false));
            let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
            assert_eq!(failed, memory, "{out}");
            assert_eq!(out.verdict(), Verdict::Fail);
        }
        let measured =
            Record { p99_us: 3_050.0, footprint_mb: 600.0, build_ms: 120.0, reserved2_gib: 30_000.0, limit_gib: 131_072.0, ..rec.clone() };
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(rule(&mut out, &measured, [&mean, &p99], &of_s(Ok(s.clone())), true, true), "{out}");
        assert_eq!(out.checks.iter().filter(|c| c.verdict == Verdict::Pass).count(), 4, "{out}");
        // PLAT-1: off the reference Mac, which S's record names, the block time and the build are
        // printed with what they would say, and the footprint can't be compared: none of the three
        // is judged, and the rule isn't passed in full. The 25% rule is the process's own: judged.
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        let slow = Record { build_ms: 151.0, ..measured.clone() };
        assert!(!rule(&mut out, &slow, [&Diffs(vec![109.0; 3]), &p99], &of_s(Err("another machine".into())), true, true), "{out}");
        let skipped: Vec<(&str, &str)> =
            out.checks.iter().filter(|c| c.verdict == Verdict::Skip).map(|c| (c.name.as_str(), c.detail.as_str())).collect();
        assert_eq!(skipped.len(), 3, "{out}");
        assert_eq!(skipped[0].0, "block within S + 80 µs, by the mean");
        assert!(
            skipped[0].1.starts_with("would fail: +109 µs (3 pairs, +109 to +109)")
                && skipped[0].1.ends_with("; not judged off the reference Mac (PLAT-1)"),
            "{out}"
        );
        assert_eq!(skipped[1], ("footprint within S + 64 MB", "another machine"));
        assert_eq!(skipped[2].0, "full build within 1.5 × S");
        assert_eq!(
            skipped[2].1,
            "would fail: 151 ms against S's 100 ms, each its process's first, measured here; not judged off the reference Mac (PLAT-1)",
            "{out}"
        );
        assert_eq!(out.checks.iter().filter(|c| c.verdict == Verdict::Pass).count(), 1, "{out}");
        assert_eq!(out.verdict(), Verdict::Pass, "nothing failed: {out}");
        // PLAT-1: on a Mac S's record doesn't name, the same three are printed, not judged, as on any
        // machine but the reference Mac, never passed, and say how to make that Mac the reference;
        // the 25% rule is still judged.
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        let other = OfS { build_ms: 100.0, record: Err("another machine".into()), place: Place::OtherMac("not the Mac S's records name") };
        assert!(!rule(&mut out, &measured, [&mean, &p99], &other, true, true), "{out}");
        let skipped: Vec<(&str, &str)> =
            out.checks.iter().filter(|c| c.verdict == Verdict::Skip).map(|c| (c.name.as_str(), c.detail.as_str())).collect();
        assert_eq!(skipped.len(), 3, "{out}");
        assert_eq!(skipped[0].0, "block within S + 80 µs, by the mean");
        assert!(skipped[0].1.starts_with("would pass: +10 µs (3 pairs, +8 to +12)"), "{out}");
        assert!(
            skipped[0].1.contains("; not judged on this Mac (PLAT-1), not the Mac S's records name: if this is the reference Mac, copy "),
            "{out}"
        );
        assert_eq!(skipped[1].0, "footprint within S + 64 MB");
        assert!(skipped[1].1.starts_with("not judged on this Mac (PLAT-1)"), "{out}");
        assert_eq!(skipped[2].0, "full build within 1.5 × S");
        assert_eq!(out.checks.iter().filter(|c| c.verdict == Verdict::Pass).count(), 1, "the 25% rule: {out}");
        assert_eq!(out.verdict(), Verdict::Pass, "nothing failed, and ci/check.sh marks the row PART: {out}");

        // RENDER-32, rg-1: the pairs' mean is judged, the app's layout's cost over S, both measured
        // in this run: +10 passes the 80 µs bound, +81 doesn't. The p99's difference is printed,
        // never judged: one far over the bound fails nothing.
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(rule(&mut out, &measured, [&mean, &Diffs(vec![900.0; 3])], &of_s(Ok(s.clone())), true, true), "{out}");
        assert!(out.checks.iter().any(|c| c.verdict == Verdict::Pass && c.detail.contains("p99, not judged: +900 µs")), "{out}");
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(!rule(&mut out, &measured, [&Diffs(vec![81.0, 48.0, 85.0]), &p99], &of_s(Ok(s.clone())), true, true), "{out}");
        let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
        assert_eq!(failed, ["block within S + 80 µs, by the mean"], "{out}");

        // RENDER-32: the build against S's first build in its runner, measured here: 151 ms against
        // 100 fails, whatever S's record says.
        let slow = Record { build_ms: 151.0, ..measured.clone() };
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(!rule(&mut out, &slow, [&mean, &p99], &of_s(Ok(s)), true, true), "{out}");
        let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
        assert_eq!(failed, ["full build within 1.5 × S"], "{out}");
        assert!(out.checks.iter().any(|c| c.detail == "151 ms against S's 100 ms, each its process's first, measured here"), "{out}");
    }

    // RENDER-32: the layout's verdict comes from the rule's own checks. When the three didn't play
    // the same music nothing was compared, so it says so, never "fails the rule: the owner decides"
    // (a bug to fix, not the owner's call), nor "passes".
    #[test]
    fn the_layout_is_judged_only_on_the_same_music() {
        use crate::report::Verdict;
        let here = "a model, an OS, a CPU, aarch64, 8 threads".to_string();
        let s = Record { footprint_mb: 554.0, machine: here.clone(), ..Record::default() };
        let rec = Record {
            side_by_side: 4,
            footprint_mb: 600.0,
            build_ms: 120.0,
            reserved2_gib: 30_000.0,
            limit_gib: 131_072.0,
            machine: here,
            ..Record::default()
        };
        let (mean, p99) = (Diffs(vec![10.0, 12.0, 8.0]), Diffs(vec![-300.0, 20.0, 250.0]));
        let of_s = |record: Result<Record, String>, place| OfS { build_ms: 100.0, record, place };
        let said = |same, record, place| {
            let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
            out.check("the same music: every renderer's timed blocks the same bits", same, "a test");
            let line = layout(&mut out, &rec, [&mean, &p99], &of_s(record, place), true, true, same);
            (line, out.verdict())
        };
        let music = "not judged: the three didn't play the same music (see above), so nothing was compared".to_string();
        assert_eq!(said(false, Ok(s.clone()), Place::Reference), (music.clone(), Verdict::Fail), "the reference Mac");
        assert_eq!(said(false, Err("another machine".into()), Place::Elsewhere), (music, Verdict::Fail), "Linux");
        assert_eq!(said(true, Ok(s.clone()), Place::Reference).0, "the app's layout passes the rule (RENDER-32)");
        let elsewhere = said(true, Err("another machine".into()), Place::Elsewhere);
        assert_eq!(elsewhere.0, "not judged in full here: only the reference Mac, which S's record names, judges it (§11.3, PLAT-1)");
        let slow = Record { build_ms: 151.0, ..rec.clone() };
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert_eq!(
            layout(&mut out, &slow, [&mean, &p99], &of_s(Ok(s), Place::Reference), true, true, true),
            "the app's layout fails the rule (§11.3): the owner decides"
        );
        // PLAT-1: on a Mac S's records don't name, the timings aren't judged, so the line says what
        // to do then. But the checks judged on any machine still judge: when one fails there (one
        // renderer built, or two holding over 25% of the address space), the line says it fails,
        // not "copy its machine line".
        let other = Place::OtherMac("not the Mac S's records name");
        let unnamed = said(true, Err("another machine".into()), other.clone());
        assert_eq!(unnamed, (not_the_reference("not the Mac S's records name"), Verdict::Pass));
        for bad in [Record { side_by_side: 1, ..rec.clone() }, Record { reserved2_gib: 40_000.0, ..rec.clone() }] {
            let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
            let line = layout(&mut out, &bad, [&mean, &p99], &of_s(Err("another machine".into()), other.clone()), true, true, true);
            assert_eq!(line, "the app's layout fails the rule (§11.3): the owner decides", "{out}");
            assert_eq!(out.verdict(), Verdict::Fail, "{out}");
        }
    }

    // RENDER-32: a round is whole cycles of the turns' orders, all its turns as long, so within each
    // pair every renderer follows each other one as often, and waits for its turns as the cycle says.
    #[test]
    fn a_round_is_whole_cycles_of_turns() {
        let orders = BLOCKS.div_ceil(TURN);
        assert_eq!(
            (BLOCKS % TURN, orders % crate::paired::cycle(3).unwrap().len()),
            (0, 0),
            "{BLOCKS} blocks in turns of {TURN}: {orders} orders"
        );
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
        // RENDER-32: and what P1's drop raised another thread's p99 over S's, from the same machine,
        // which the batched-drops verdict adds.
        assert_eq!(p1_raise(&text), Some(0.0));
        let s_line = text.lines().find(|l| l.starts_with("store-record ")).unwrap();
        let other = format!("{s_line}\nstore-p1 raise_us=9.000 machine=Mac14,2, macOS 26.6.2, Apple M2, aarch64, 8 threads\n");
        assert_eq!(p1_raise(&other), None, "only from S's record's machine");
    }
}

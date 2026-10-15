//! G7f, part 1 (step 7's design, §11): what one wasmtime `Store` per kernel instance costs at p50,
//! measured on the runtime itself, with no server and no device, and the store layout chosen by a
//! rule written down before anything was measured (RENDER-32).
//!
//! It uses only what HEAD `8aa301e` already had (`Renderer::new`, `set_transport`, `render_block`,
//! `Drop`, and the `Flat` for counting), so this same file ran in a worktree at HEAD to record
//! layout S, one `Store` per renderer, and measures whatever layout the tree it is built in has.
//! S's record is read from `store-S.txt` beside this crate's manifest, or from `$M2_STORE_S`; every
//! run prints its own record line (`store-record …`), which is what that file holds. A record names
//! its machine, and the rule compares only with S's record from the same machine (PLAT-1).
//!
//! The rule (§11.3): choose one `Store` per kernel instance (P1) unless one of these fails
//! against S:
//! - the p99 block time is worse by more than 80 µs, 3% of a 2.67 ms block;
//! - the footprint is worse by more than 64 MB;
//! - fewer than 2 renderers build side by side, or the address space the process holds with 2
//!   renderers exceeds 25% of its limit;
//! - p50's full build, what opening a project costs, is worse by more than 50%.
//!
//! The two memory rules fail when the OS doesn't say what the process holds: they never pass
//! unmeasured (`m2_os::process::memory`).
//!
//! If P1 fails, wasmtime's pooling allocator (P3) is measured, then a smaller memory reservation
//! (P2), and the first that passes is chosen; a shared store never is. Batched drops are built
//! only if the chosen layout's drop raises an audio-like thread's p99 more than S's drop does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use m2_ir::{FNode, FOp};
use m2_os::process::{self, Memory};
use m2_runtime::{BLOCK, Renderer, Transport};

use crate::gates::Opts;
use crate::report::{Outcome, same_machine};
use crate::{Lab, songs};

/// What the contract's acceptance gate 7 names as p50's instance count.
const CONTRACT_COUNT: usize = 6_176;
/// The block time measured: 30 s from 3:00.
const FROM_SECONDS: f64 = 180.0;
const BLOCKS: usize = 11_250;
/// The most p50 renderers built side by side.
const SIDE_BY_SIDE: usize = 4;
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

/// The rule (§11.3) on `rec`: each check into `out`, and whether all passed. A comparison with S
/// needs S's record from this machine, else it is skipped with why (PLAT-1). The memory rules fail
/// when the memory wasn't measured (`memory`: before and after one renderer; `with_two`: with two
/// renderers), whatever S is: they never pass unmeasured.
fn rule(out: &mut Outcome, rec: &Record, s: &Result<Record, String>, memory: bool, with_two: bool) -> bool {
    let against_s = |out: &mut Outcome, name: &str, judge: &dyn Fn(&Record) -> (bool, String)| match s {
        Ok(s) => {
            let (ok, detail) = judge(s);
            out.check(name, ok, detail)
        }
        Err(why) => {
            out.skip(name, why);
            false
        }
    };
    let mut pass = against_s(out, "p99 block within S + 80 µs", &|s| {
        (rec.p99_us <= s.p99_us + 80.0, format!("{:.0} µs against S's {:.0} µs", rec.p99_us, s.p99_us))
    });
    let footprint = "footprint within S + 64 MB";
    pass &= if memory {
        against_s(out, footprint, &|s| {
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
            let of_s = s.as_ref().map_or(String::new(), |s| format!(" (S: {:.1}%)", 100.0 * s.reserved2_gib / s.limit_gib));
            let held = 100.0 * rec.reserved2_gib / rec.limit_gib;
            out.check(side, rec.reserved2_gib <= 0.25 * rec.limit_gib, format!("{} built; {held:.1}%{of_s}", rec.side_by_side))
        }
    };
    pass &= against_s(out, "full build within 1.5 × S", &|s| {
        (rec.build_ms <= 1.5 * s.build_ms, format!("{:.0} ms against S's {:.0} ms", rec.build_ms, s.build_ms))
    });
    pass
}

/// Nearest rank: the smallest value at or above share `p`.
fn rank(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((p * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1]
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
    let mut first = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    rec.build_ms = t.elapsed().as_secs_f64() * 1e3;
    let one = process::memory();
    let mut more = vec![];
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

    // Block time: 30 s from 3:00, live (non-finite samples silenced, as the engine plays).
    first.set_strict(false);
    first.set_transport(Transport::starting((FROM_SECONDS * song.rate as f64).round() as u64));
    let mut times = Vec::with_capacity(BLOCKS);
    for k in 0..BLOCKS {
        let t = Instant::now();
        first.render_block((k * BLOCK) as u64)?;
        times.push(t.elapsed().as_secs_f64() * 1e6);
    }
    rec.mean_us = times.iter().sum::<f64>() / times.len() as f64;
    rec.p99_us = rank(&mut times, 0.99);
    rec.max_us = rank(&mut times, 1.0);
    let (p50, p90, p999) = (rank(&mut times, 0.5), rank(&mut times, 0.9), rank(&mut times, 0.999));
    out.info(
        "block",
        format!(
            "mean {:.0} µs; p50 {p50:.0}, p90 {p90:.0}, p99 {:.0}, p99.9 {p999:.0}, max {:.0} µs over {BLOCKS} blocks from 3:00",
            rec.mean_us, rec.p99_us, rec.max_us
        ),
    );

    // Drop: off an audio-like thread that renders a small song every 2.67 ms meanwhile.
    let mut player = Renderer::new(&small.flat, &small.data, &lab.packages, small.rate)?;
    player.set_strict(false);
    let stop = Arc::new(AtomicBool::new(false));
    let period = Duration::from_secs_f64(BLOCK as f64 / small.rate as f64);
    let audio = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            // Marked latency-sensitive, as an audio thread is (on macOS, kept on a performance
            // core), so its blocks time the drop rather than the scheduler. Linux can't: reported.
            let class = m2_os::thread::interactive();
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
    drop(first);
    let t1 = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::Relaxed);
    let Ok((class, blocks)) = audio.join() else { bail!("the audio-like thread panicked") };
    rec.drop_ms = (t1 - t0).as_secs_f64() * 1e3;
    let mut base: Vec<f64> = blocks.iter().filter(|b| b.0 < t0 && b.0 + Duration::from_secs(1) >= t0).map(|b| b.1).collect();
    let mut during: Vec<f64> = blocks.iter().filter(|b| b.0 + Duration::from_secs_f64(b.1 / 1e6) >= t0 && b.0 <= t1).map(|b| b.1).collect();
    rec.audio_base_us = rank(&mut base, 0.99);
    rec.audio_drop_us = rank(&mut during, 0.99);
    out.info(
        "drop",
        format!(
            "{:.1} ms; the audio-like thread's p99 block {:.0} µs during it ({} blocks), {:.0} µs before; {}",
            rec.drop_ms,
            rec.audio_drop_us,
            during.len(),
            rec.audio_base_us,
            match class {
                Ok(()) => "that thread marked latency-sensitive".to_string(),
                Err(e) => format!("that thread at its default priority ({e})"),
            }
        ),
    );
    out.info("record", rec.line());

    // The rule (§11.3). Its comparisons with S are judged only against S's record from this same
    // machine (PLAT-1); its memory rules fail, never pass, when the memory wasn't measured.
    let path =
        std::env::var_os("M2_STORE_S").map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt"), Into::into);
    let s = match std::fs::read_to_string(&path).ok().and_then(|t| Record::parse(&t)) {
        None => Err(format!("no record of layout S in {}", path.display())),
        Some(s) if !same_machine(&s.machine, &rec.machine) => {
            out.info("machines", format!("S's record is from {}; this run is on {}", s.machine, rec.machine));
            Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())
        }
        Some(s) => Ok(s),
    };
    let pass = rule(&mut out, &rec, &s, measured.is_some(), measured.is_some() && two.is_some());
    // What the drop adds to the audio-like thread's p99 over its own before; none when it adds nothing.
    let raise = |r: &Record| (r.audio_drop_us - r.audio_base_us).max(0.0);
    let Ok(s) = s else {
        out.info(
            "batched drops",
            format!("the drop raises the audio-like thread's p99 by {:.0} µs; not compared with S here", raise(&rec)),
        );
        out.info("layout", "not chosen here: the rule needs S's record from this machine (§11.3, PLAT-1)");
        return Ok(out);
    };
    out.info(
        "batched drops",
        format!(
            "{}: the drop raises the audio-like thread's p99 by {:.0} µs, S's by {:.0} µs",
            if raise(&rec) > raise(&s) { "needed" } else { "not needed" },
            raise(&rec),
            raise(&s)
        ),
    );
    out.info("layout", if pass { "one Store per kernel instance (P1)" } else { "P1 fails the rule: measure P3, then P2 (§11.3)" });
    Ok(out)
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
        for s in [Ok(s.clone()), Err("recorded, not judged: S's record is from another machine (PLAT-1)".to_string())] {
            let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
            assert!(!rule(&mut out, &rec, &s, false, false));
            let failed: Vec<&str> = out.checks.iter().filter(|c| c.verdict == Verdict::Fail).map(|c| c.name.as_str()).collect();
            assert_eq!(failed, memory, "{out}");
            assert_eq!(out.verdict(), Verdict::Fail);
        }
        let measured =
            Record { p99_us: 3_050.0, footprint_mb: 600.0, build_ms: 120.0, reserved2_gib: 30_000.0, limit_gib: 131_072.0, ..rec.clone() };
        let mut out = Outcome::new("G7f store", "RENDER-32", "a test");
        assert!(rule(&mut out, &measured, &Ok(s), true, true), "{out}");
        assert_eq!(out.checks.iter().filter(|c| c.verdict == Verdict::Pass).count(), 4, "{out}");
    }

    // PLAT-1: S's own record, as checked in, names the reference Mac.
    #[test]
    fn the_checked_in_record_of_s_names_its_machine() {
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt")).unwrap();
        let s = Record::parse(&text).expect("store-S.txt holds a record");
        assert!(s.machine.starts_with("MacBookPro18,1, macOS "), "{}", s.machine);
    }
}

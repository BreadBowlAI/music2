//! G7f, part 1 (step 7's design, §11): what one wasmtime `Store` per kernel instance costs at p50,
//! measured on the runtime itself, with no server and no device, and the store layout chosen by a
//! rule written down before anything was measured (RENDER-32).
//!
//! It uses only what HEAD `8aa301e` already had (`Renderer::new`, `set_transport`, `render_block`,
//! `Drop`, and the `Flat` for counting), so this same file ran in a worktree at HEAD to record
//! layout S, one `Store` per renderer, and measures whatever layout the tree it is built in has.
//! S's record is read from `store-S.txt` beside this crate's manifest, or from `$M2_STORE_S`; every
//! run prints its own record line (`store-record …`), which is what that file holds.
//!
//! The rule (§11.3): choose one `Store` per kernel instance (P1) unless one of these fails
//! against S:
//! - the p99 block time is worse by more than 80 µs, 3% of a 2.67 ms block;
//! - the footprint is worse by more than 64 MB;
//! - fewer than 2 renderers build side by side, or the address space the process holds with 2
//!   renderers exceeds 25% of its limit;
//! - p50's full build, what opening a project costs, is worse by more than 50%.
//!
//! If P1 fails, wasmtime's pooling allocator (P3) is measured, then a smaller memory reservation
//! (P2), and the first that passes is chosen; a shared store never is. Batched drops are built
//! only if the chosen layout's drop raises an audio-like thread's p99 more than S's drop does.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use m2_ir::{FNode, FOp};
use m2_runtime::{BLOCK, Renderer, Transport};

use crate::gates::Opts;
use crate::report::Outcome;
use crate::{Lab, songs};

/// What the contract's acceptance gate 7 names as p50's instance count.
const CONTRACT_COUNT: usize = 6_176;
/// The block time measured: 30 s from 3:00.
const FROM_SECONDS: f64 = 180.0;
const BLOCKS: usize = 11_250;
/// The most p50 renderers built side by side.
const SIDE_BY_SIDE: usize = 4;

/// One layout's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
        format!("store-record {}", words.join(" "))
    }

    /// The first `store-record` line of `text`.
    pub fn parse(text: &str) -> Option<Record> {
        let line = text.lines().find_map(|l| l.trim().strip_prefix("store-record "))?;
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

/// The process's memory, from `task_info(TASK_VM_INFO)`: physical footprint, address space in use,
/// and the address-space limit, in bytes.
#[cfg(target_os = "macos")]
fn memory() -> Option<(u64, u64, u64)> {
    // `task_vm_info` through rev2 (<mach/task_info.h>).
    #[repr(C)]
    #[derive(Default)]
    struct VmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        sizes: [u64; 16],
        phys_footprint: u64,
        min_address: u64,
        max_address: u64,
    }
    const TASK_VM_INFO: u32 = 22;
    let mut info = VmInfo::default();
    let mut count = (std::mem::size_of::<VmInfo>() / 4) as u32;
    // SAFETY: `info` is a `task_vm_info` through rev2 and `count` its size in `natural_t`s.
    #[allow(deprecated)]
    let kr = unsafe { libc::task_info(libc::mach_task_self(), TASK_VM_INFO, &mut info as *mut VmInfo as libc::task_info_t, &mut count) };
    (kr == 0).then_some((info.phys_footprint, info.virtual_size, info.max_address - info.min_address))
}

#[cfg(not(target_os = "macos"))]
fn memory() -> Option<(u64, u64, u64)> {
    None
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
    let mut rec = Record { count: instances(&song.flat.nodes, &song.flat.order), ..Record::default() };
    out.info("kernel instances", format!("{} (acceptance gate 7 names {CONTRACT_COUNT})", rec.count));

    // Build: one renderer, then more side by side.
    let before = memory();
    let t = Instant::now();
    let mut first = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    rec.build_ms = t.elapsed().as_secs_f64() * 1e3;
    let one = memory();
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
            two = memory();
        }
    }
    drop(more);
    if let (Some(b), Some(a)) = (before, one) {
        rec.footprint_mb = a.0.saturating_sub(b.0) as f64 / MB;
        rec.reserved_gib = a.1.saturating_sub(b.1) as f64 / GIB;
        rec.limit_gib = a.2 as f64 / GIB;
    }
    if let Some(a) = two {
        rec.reserved2_gib = a.1 as f64 / GIB;
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
    out.info(
        "memory",
        format!(
            "one renderer adds {:.0} MB of footprint and reserves {:.0} GiB; with two the process holds {:.0} GiB, {:.1}% of its {:.0} GiB",
            rec.footprint_mb,
            rec.reserved_gib,
            rec.reserved2_gib,
            100.0 * rec.reserved2_gib / rec.limit_gib.max(1.0),
            rec.limit_gib
        ),
    );

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
            // Kept on a performance core, as an audio thread is, so its blocks time the drop rather
            // than the scheduler.
            // SAFETY: sets this thread's own QoS class.
            #[cfg(target_os = "macos")]
            unsafe {
                libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0);
            }
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
            blocks
        })
    };
    std::thread::sleep(Duration::from_secs(3));
    let t0 = Instant::now();
    drop(first);
    let t1 = Instant::now();
    std::thread::sleep(Duration::from_millis(300));
    stop.store(true, Ordering::Relaxed);
    let Ok(blocks) = audio.join() else { bail!("the audio-like thread panicked") };
    rec.drop_ms = (t1 - t0).as_secs_f64() * 1e3;
    let mut base: Vec<f64> = blocks.iter().filter(|b| b.0 < t0 && b.0 + Duration::from_secs(1) >= t0).map(|b| b.1).collect();
    let mut during: Vec<f64> = blocks.iter().filter(|b| b.0 + Duration::from_secs_f64(b.1 / 1e6) >= t0 && b.0 <= t1).map(|b| b.1).collect();
    rec.audio_base_us = rank(&mut base, 0.99);
    rec.audio_drop_us = rank(&mut during, 0.99);
    out.info(
        "drop",
        format!(
            "{:.1} ms; the audio-like thread's p99 block {:.0} µs during it ({} blocks), {:.0} µs before",
            rec.drop_ms,
            rec.audio_drop_us,
            during.len(),
            rec.audio_base_us
        ),
    );
    out.info("record", rec.line());

    // The rule, against S.
    let path =
        std::env::var_os("M2_STORE_S").map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("store-S.txt"), Into::into);
    let Some(s) = std::fs::read_to_string(&path).ok().and_then(|t| Record::parse(&t)) else {
        out.skip("the rule (§11.3)", format!("no record of layout S in {}", path.display()));
        return Ok(out);
    };
    let checks = [
        ("p99 block within S + 80 µs", rec.p99_us <= s.p99_us + 80.0, format!("{:.0} µs against S's {:.0} µs", rec.p99_us, s.p99_us)),
        (
            "footprint within S + 64 MB",
            rec.footprint_mb <= s.footprint_mb + 64.0,
            format!("{:.0} MB against S's {:.0} MB", rec.footprint_mb, s.footprint_mb),
        ),
        (
            "2 renderers side by side, reserving at most 25% of the address space",
            rec.side_by_side >= 2 && rec.reserved2_gib <= 0.25 * rec.limit_gib,
            format!(
                "{} built; {:.1}% (S: {:.1}%)",
                rec.side_by_side,
                100.0 * rec.reserved2_gib / rec.limit_gib,
                100.0 * s.reserved2_gib / s.limit_gib
            ),
        ),
        ("full build within 1.5 × S", rec.build_ms <= 1.5 * s.build_ms, format!("{:.0} ms against S's {:.0} ms", rec.build_ms, s.build_ms)),
    ];
    let mut pass = true;
    for (name, ok, detail) in checks {
        pass &= out.check(name, ok, detail);
    }
    // What the drop adds to the audio-like thread's p99 over its own before; none when it adds nothing.
    let raise = |r: &Record| (r.audio_drop_us - r.audio_base_us).max(0.0);
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

    // RENDER-32: a record line reads back as itself.
    #[test]
    fn a_record_reads_back() {
        let r = Record { count: 6_060, build_ms: 151.5, side_by_side: 4, p99_us: 812.25, limit_gib: 131_072.0, ..Record::default() };
        assert_eq!(Record::parse(&format!("noise\n{}\n", r.line())), Some(r));
        assert_eq!(Record::parse("store-record count=1"), None, "every field is needed");
    }
}

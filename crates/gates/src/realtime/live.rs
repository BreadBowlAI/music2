//! G12's live part (8a's live half; design-8a §9.2): a song plays live on the player's engine
//! threads, muted, and every callback is judged.
//!
//! In short:
//! - **On a Mac** it opens the output device whatever `M2_AUDIO` says, at `--frames` (128), and
//!   judges: no dropout from the first callback after Play (the device's overloads, late
//!   callbacks, gaps), the callback's p99 at most 70% of its period from 5 s on, and every engine
//!   thread set up as designed (real time, in the output's workgroup, at 48 kHz and the frames
//!   asked). A Mac that isn't a base M1 is a stand-in: its verdict is provisional for gate 8.
//! - **Elsewhere** it plays on the silent clock, never a device: the timings and the set-up are
//!   information (PLAT-1).
//! - **Everywhere** it judges: no crash and no non-finite sample, and the engine rendered as many
//!   frames as the device took.
//! - **A run**: open the player and wait until its callback took its engine threads; open the song,
//!   looping it whole; let it sit stopped 2 s (what no one hears isn't counted); Play. Dropouts
//!   count from the first callback after Play, callback times from 5 s after it.
//! - **`--sweep` and `--between`** play each count and each way to wait; then a table: each
//!   count's live p99 beside its back-to-back p99 (offline, plain threads), and how late workers
//!   came into a block. The owner's Mac run picks the live count and `BETWEEN` from it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_play::{BETWEEN, BUCKET_NS, Between, EngineThreads, LiveThreads, Player, Timing};
use m2_runtime::{BLOCK, Cmd, Counters, Renderer};

use super::{Stats, load, pass};
use crate::gates::Opts;
use crate::report::{Check, Outcome, Verdict};
use crate::{Lab, Song};

/// The share of its period a callback may take at p99 (design-8a §9.2).
const P99_SHARE: f64 = 0.70;
/// The rate the reference set-up plays at.
const RATE: u32 = 48_000;
/// Before Play: what no one hears, not counted.
const STOPPED: Duration = Duration::from_secs(2);
/// After Play: callback times count from here.
const SETTLE: Duration = Duration::from_secs(5);
/// Seconds of each count's back-to-back pass.
const BACK_TO_BACK: f64 = 10.0;

/// What a live run is asked (`m2-gates realtime` without `--offline`).
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    /// The device's buffer, in frames.
    pub frames: u32,
    /// The thread counts to play (`--sweep`, `--threads`); None: the live rule's.
    pub counts: Option<Vec<usize>>,
    /// How workers wait between callbacks, each played (`--between`).
    pub between: Vec<Between>,
    /// Heard (`--audible`), or zeros to the device.
    pub audible: bool,
    /// Another song than the reference project (`--song`).
    pub song: Option<PathBuf>,
    /// G13 (`--beside agents`): agents listen and edit beside the run ([`super::beside`]).
    pub beside: bool,
}

impl Default for Asked {
    fn default() -> Asked {
        Asked { frames: 128, counts: None, between: vec![BETWEEN], audible: false, song: None, beside: false }
    }
}

/// `--beside`: who works beside the live run; `agents` only (G13).
pub fn beside(text: &str) -> Result<bool> {
    match text.trim() {
        "agents" => Ok(true),
        other => bail!("--beside takes agents: not `{other}`"),
    }
}

/// Why G13's timings aren't judged on a Mac yet (the agents design, section 7).
const G13_RECORDED: &str = "recorded, not judged yet: G13's first runs on the Mac are recorded, then judged (the agents design, section 7)";

/// `--frames`: a device buffer of 16 to 4096 frames.
pub fn frames(text: &str) -> Result<u32> {
    match text.trim().parse::<u32>() {
        Ok(n @ 16..=4096) => Ok(n),
        _ => bail!("--frames takes a buffer size from 16 to 4096 frames, like 128: not `{}`", text.trim()),
    }
}

/// `--threads`: one count, 1 to 64.
pub fn threads(text: &str) -> Result<usize> {
    m2_runtime::threads::parse(text).context("--threads")
}

/// `--between`: park, spin, or both (`park,spin`), each once.
pub fn between(text: &str) -> Result<Vec<Between>> {
    let mut ways = vec![];
    for part in text.split(',') {
        let way = match part.trim() {
            "park" => Between::Park,
            "spin" => Between::Spin,
            other => bail!("--between takes park, spin or park,spin: not `{other}`"),
        };
        if !ways.contains(&way) {
            ways.push(way);
        }
    }
    Ok(ways)
}

/// `--minutes`: how long each run plays, more than 0.
pub fn minutes(text: &str) -> Result<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|m| *m > 0.0 && m.is_finite())
        .with_context(|| format!("--minutes takes how many minutes to play, more than 0, like 1 or 0.5: not `{}`", text.trim()))
}

/// Where a live run plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The output device, whatever `M2_AUDIO` says: on a Mac, where timings are judged.
    Device,
    /// The silent clock: anywhere else (PLAT-1). It never opens a device.
    Clock,
}

/// Where a run on machine `machine` (`m2_os::process::machine()`) plays: a Mac's output device,
/// else the silent clock. A Mac with no output fails, as the run can't open one.
pub fn place(machine: &str) -> Place {
    match machine.split(", ").nth(1).is_some_and(|os| os.starts_with("macOS ")) {
        true => Place::Device,
        false => Place::Clock,
    }
}

/// What one live run found.
#[derive(Clone, Debug)]
pub struct Found {
    pub threads: EngineThreads,
    /// The device's rate, and its callbacks' frames.
    pub rate: u32,
    pub frames: u32,
    /// The song's rate: what the engine clock counts in.
    pub song_rate: u32,
    /// From the first callback after Play to the end; before Play (information).
    pub whole: Timing,
    pub before: Timing,
    /// From 5 s after Play to the end: the callback times.
    pub steady: Timing,
}

impl Found {
    /// The device's period: its callbacks' frames at its rate.
    fn period(&self) -> Duration {
        Duration::from_secs_f64(f64::from(self.frames) / f64::from(self.rate.max(1)))
    }
}

/// G12's live verdict, each check (a pure function of the run). `judged` (a Mac's run): the
/// set-up, the callback's p99 and the dropouts are bounds; elsewhere they are information
/// (PLAT-1). Crashes, non-finite samples and frames are judged everywhere. `asked`: the frames
/// asked for.
pub fn verdict(f: &Found, asked: u32, judged: bool) -> Vec<Check> {
    let mut checks = vec![];
    let mut timed = |name: &str, pass: bool, detail: String| {
        let (verdict, detail) = match judged {
            true => (if pass { Verdict::Pass } else { Verdict::Fail }, detail),
            false => (Verdict::Info, format!("{detail}; not judged on Linux (PLAT-1)")),
        };
        checks.push(Check { name: name.into(), verdict, detail });
    };
    // The set-up: every worker real time and in the workgroup, at 48 kHz and the frames asked.
    let t = &f.threads;
    let workers = t.count - 1;
    let mut wrong = vec![];
    if f.rate != RATE {
        wrong.push(format!("the output plays at {} Hz: set it to 48 kHz in Audio MIDI Setup", f.rate));
    }
    if f.frames != asked {
        wrong.push(format!("its callbacks ask {} frames, not {asked}: the device didn't take {asked}", f.frames));
    }
    if t.real_time < workers || t.joined < workers {
        wrong.push(format!("workers real time {} of {workers}, in the workgroup {} of {workers}", t.real_time, t.joined));
    }
    if !t.device.real_time || !t.device.workgroup {
        wrong.push(format!(
            "the device's thread {}real time, {}in the workgroup",
            if t.device.real_time { "" } else { "not " },
            if t.device.workgroup { "" } else { "not " }
        ));
    }
    if let Some(m) = &t.missing {
        wrong.push(m.clone());
    }
    let setup = format!(
        "{} thread{} ({}), {}; workers real time {}/{workers}, in the workgroup {}/{workers}; the device's thread {}; {} Hz, {} frames; its FP register {:#x}",
        t.count,
        if t.count == 1 { "" } else { "s" },
        t.advice.map_or_else(|| "no workgroup's advice".into(), |n| format!("the workgroup advises {n}")),
        way(t.between),
        t.real_time,
        t.joined,
        match (t.device.real_time, t.device.workgroup) {
            (true, true) => "real time, in the workgroup",
            (true, false) => "real time",
            (false, _) => "not real time",
        },
        f.rate,
        f.frames,
        t.device_mode
    );
    let designed = wrong.is_empty();
    timed(
        "set-up",
        designed,
        match designed {
            true => setup,
            false => format!("{setup}: not the designed set-up: {}", wrong.join("; ")),
        },
    );
    // The callback, from 5 s after Play.
    let period = f.period().as_secs_f64() * 1e6;
    let s = &f.steady;
    let us = |q: f64| s.quantile(q).map_or(f64::NAN, |d| d.as_secs_f64() * 1e6);
    let (p99, bound) = (us(0.99), P99_SHARE * period);
    let share = |x: f64| 100.0 * x / period;
    let (top, max) = (past(s), longest(s));
    timed(
        "callback",
        s.callbacks > 0 && p99 <= bound,
        format!(
            "p50 {} µs, p99 {} ({}% of the period), p99.9 {}, max {} ({}%) in {} callbacks from 5 s on; bound p99 {bound:.0} µs (70% of {period:.0})",
            shown(us(0.5), top),
            shown(p99, top),
            shown(share(p99), share(top)),
            shown(us(0.999), top),
            known(max, top),
            known(max.map(share), share(top)),
            s.callbacks
        ),
    );
    // Dropouts, from the first callback after Play.
    let w = &f.whole;
    timed(
        "dropouts",
        w.dropouts() == 0,
        format!(
            "{} from Play: the device's {}, late {}, gaps {} in {} callbacks (before Play, not counted: late {}, gaps {} in {})",
            w.dropouts(),
            w.xruns,
            w.late,
            w.gaps,
            w.callbacks,
            f.before.late,
            f.before.gaps,
            f.before.callbacks
        ),
    );
    // Judged everywhere: no crash, no non-finite sample, and the engine as far as the device.
    let ok = !w.crashed && w.crashes == 0 && w.non_finite == 0;
    checks.push(Check {
        name: "crashes and non-finite samples".into(),
        verdict: if ok { Verdict::Pass } else { Verdict::Fail },
        detail: format!(
            "{}crashes {}, non-finite samples {}",
            if w.crashed { "the player crashed; " } else { "" },
            w.crashes,
            w.non_finite
        ),
    });
    let ratio = f64::from(f.song_rate) / f64::from(f.rate.max(1));
    let played = w.frames as f64 * ratio;
    let slack = BLOCK as f64 + f64::from(f.frames) * ratio + 8.0;
    let off = played - w.clock as f64;
    checks.push(Check {
        name: "frames".into(),
        verdict: if w.frames > 0 && off.abs() <= slack { Verdict::Pass } else { Verdict::Fail },
        detail: format!(
            "the device took {} frames ({played:.0} at the song's rate), the engine rendered {}: {off:+.0}, within {slack:.0}",
            w.frames, w.clock
        ),
    });
    checks
}

/// Where a window's histogram ends, in µs: a quantile past it is only known to be over it.
fn past(t: &Timing) -> f64 {
    (t.hist.len() as u64 * BUCKET_NS) as f64 / 1e3
}

/// A window's longest callback in µs: None when it is only known to be past the histogram.
fn longest(t: &Timing) -> Option<f64> {
    t.max_ns.map(|ns| ns as f64 / 1e3)
}

/// `x` as printed, or "over `top`" when it isn't known.
fn known(x: Option<f64>, top: f64) -> String {
    x.map_or_else(|| format!("over {top:.0}"), |x| format!("{x:.0}"))
}

/// `x` as printed, or "over `top`" when it is past the histogram.
fn shown(x: f64, top: f64) -> String {
    match x > top {
        true => format!("over {top:.0}"),
        false => format!("{x:.0}"),
    }
}

fn way(b: Between) -> &'static str {
    match b {
        Between::Park => "park",
        Between::Spin => "spin",
    }
}

/// Wait until `ok`, at most `up_to`; else say what didn't come.
fn until(what: &str, up_to: Duration, mut ok: impl FnMut() -> bool) -> Result<()> {
    let t = Instant::now();
    while !ok() {
        if t.elapsed() > up_to {
            bail!("{what} didn't come within {} s", up_to.as_secs());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

/// One live run of `song`, on `live`'s threads, for `minutes`.
fn play(lab: &Lab, song: &Song, place: Place, asked: &Asked, live: LiveThreads, minutes: f64) -> Result<Found> {
    let player = match place {
        Place::Device => Player::start_device(asked.frames, live).context("G12 plays on this Mac's output, and it didn't open")?,
        Place::Clock => Player::silent_with(asked.frames, live)?,
    };
    player.mute_output(!asked.audible);
    until("the callback taking its engine threads", Duration::from_secs(5), || player.engine_threads().taken)?;
    player.open(Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?)?;
    player.set_loop(Some((0.0, song.data.end())))?;
    let opened = player.timing();
    std::thread::sleep(STOPPED);
    let seq = player.transport(Cmd::Play)?;
    until("Play", Duration::from_secs(5), || player.applied(seq).is_some())?;
    let t0 = player.timing();
    std::thread::sleep(SETTLE);
    let t1 = player.timing();
    std::thread::sleep(Duration::from_secs_f64((minutes * 60.0 - SETTLE.as_secs_f64()).max(1.0)));
    let t2 = player.timing();
    let found = Found {
        threads: player.engine_threads(),
        rate: player.device_rate,
        frames: player.frames().unwrap_or(0),
        song_rate: song.rate,
        whole: t2.since(&t0),
        before: t0.since(&opened),
        steady: t2.since(&t1),
    };
    player.stop();
    player.unload()?;
    let freed = player.freed();
    drop(player);
    // The next run opens the device again: its stream stopped and its workers joined first.
    if !freed.wait(Duration::from_secs(5)) {
        bail!("the player let go didn't free its device within 5 s");
    }
    Ok(found)
}

/// The workers' entries into blocks, summed: the bucket below which a share `q` came.
fn entry(threads: &[Counters], q: f64) -> Option<String> {
    let mut sum = [0u64; m2_runtime::ENTRY_BUCKETS];
    for c in threads.iter().skip(1) {
        for (s, n) in sum.iter_mut().zip(c.entry) {
            *s += n;
        }
    }
    let n: u64 = sum.iter().sum();
    let want = ((q * n as f64).ceil() as u64).max(1);
    let mut seen = 0;
    let k = sum.iter().position(|&c| {
        seen += c;
        seen >= want
    })?;
    Some(match k {
        0 => "under 1 µs".into(),
        k if k == m2_runtime::ENTRY_BUCKETS - 1 => format!("{} ms or more", (1u64 << (k - 1)) / 1000),
        k => format!("under {} µs", 1u64 << k),
    })
}

/// The pool's counts over a window, in words.
fn engines_said(t: &Timing) -> String {
    if t.threads.is_empty() {
        return "one thread: every block on the device's own".into();
    }
    let all: u64 = t.threads.iter().map(|c| c.tasks).sum();
    let tasks: Vec<String> = t.threads.iter().map(|c| c.tasks.to_string()).collect();
    let workers = &t.threads[1..];
    let sum = |f: fn(&Counters) -> u64| workers.iter().map(f).sum::<u64>();
    let c = &t.threads[0];
    format!(
        "tasks {} ({:.0}% on workers); workers parked {} times, signalled {}; entered blocks {}, p50 {}, p99 {}; the device's thread dozed {} times ({} rung)",
        tasks.join("/"),
        100.0 * (all - c.tasks) as f64 / all.max(1) as f64,
        sum(|c| c.parks),
        sum(|c| c.signals),
        sum(|c| c.entry.iter().sum()),
        entry(&t.threads, 0.5).unwrap_or_else(|| "-".into()),
        entry(&t.threads, 0.99).unwrap_or_else(|| "-".into()),
        c.dozes,
        c.rung
    )
}

/// A base M1 judges gate 8; any other Mac stands in, provisionally (design-8a Q1).
fn stand_in(machine: &str) -> Option<String> {
    let parts: Vec<&str> = machine.split(", ").collect();
    let (model, cpu) = (parts.first().copied().unwrap_or("?"), parts.get(2).copied().unwrap_or("?"));
    (cpu != "Apple M1").then(|| format!("stand-in ({model}, {cpu}): provisional for gate 8 (design-8a Q1)"))
}

/// G12, live: each count and way to wait plays `o.loop_minutes` of the reference project (or
/// `--song`), and is judged.
pub fn live(lab: &mut Lab, o: &Opts, asked: &Asked) -> Result<Outcome> {
    let machine = m2_os::process::machine();
    let place = place(&machine);
    let dir = match &asked.song {
        Some(d) => d.clone(),
        None => lab.reference(&o.reference)?,
    };
    let name = asked.song.as_ref().map_or_else(|| o.reference.name(), |d| d.display().to_string());
    let rig = match place {
        Place::Device => format!("the output device on {name}, {} frames", asked.frames),
        Place::Clock => format!("the silent clock on {name}, {} frames", asked.frames),
    };
    let mut out = match asked.beside {
        false => Outcome::new("G12 live", "RENDER-28, RENDER-28a", &rig),
        true => Outcome::new("G13", "AGENT-7, RENDER-28", &format!("{rig}, agents beside it")),
    };
    let cores = m2_os::process::cores();
    out.info("cores", format!("{} fast ({} per cache), {} slow", cores.fast, cores.per_cache, cores.slow));
    out.info("load", load());
    if place == Place::Device
        && let Some(s) = stand_in(&machine)
    {
        out.info("gate 8", s);
    }
    let song = lab.compile(&dir)?;
    if Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?.hosts_plugins() {
        out.info("plugins", "the song hosts plugins: every block renders on the device's thread (HOST-2)");
    }
    // Off a device the rule would say 1: play the count a device would get, so the engine
    // threads run here too.
    let rule = match place {
        Place::Device => None,
        Place::Clock => Some(m2_play::live_count(None, &cores, None, true)),
    };
    let counts: Vec<Option<usize>> = asked.counts.as_ref().map_or_else(|| vec![rule], |c| c.iter().map(|&n| Some(n)).collect());
    // G13: the agents start before the first run and stop after the last.
    let agents = match asked.beside {
        true => Some(super::beside::start(&dir, &lab.work)?),
        false => None,
    };
    if let Some(a) = &agents {
        out.info(
            "agents",
            format!("m2 listen back to back on bars {}; m2 set {} forwarded to the app, one after another", super::beside::BARS, a.knob),
        );
    }
    let mut rows = vec![];
    for count in counts {
        let ways = if count == Some(1) { &asked.between[..1] } else { &asked.between[..] };
        for &between in ways {
            let f = play(lab, &song, place, asked, LiveThreads { count, between }, o.loop_minutes)?;
            let label = format!("{} thread{}, {}", f.threads.count, if f.threads.count == 1 { "" } else { "s" }, way(between));
            for mut c in verdict(&f, asked.frames, place == Place::Device && !asked.beside) {
                if asked.beside && place == Place::Device {
                    c.detail = c.detail.replace("not judged on Linux (PLAT-1)", G13_RECORDED);
                }
                out.checks.push(Check { name: format!("{label}: {}", c.name), ..c });
            }
            out.info(format!("{label}: engine threads"), engines_said(&f.steady));
            rows.push(f);
        }
    }
    if let Some(a) = agents {
        let did = a.stop();
        let landed = |runs: &[super::beside::Ran]| runs.iter().any(|r| r.ok) && runs.iter().all(|r| r.ok);
        out.checks.push(Check {
            name: "listens beside it".into(),
            verdict: if landed(&did.listens) { Verdict::Pass } else { Verdict::Fail },
            detail: super::beside::Did::said(&did.listens),
        });
        out.checks.push(Check {
            name: "edits forwarded beside it".into(),
            verdict: if landed(&did.edits) { Verdict::Pass } else { Verdict::Fail },
            detail: super::beside::Did::said(&did.edits),
        });
        let background = did.listens.iter().filter(|r| r.ok).all(|r| r.background == Some(true));
        out.checks.push(Check {
            name: "listens at background priority".into(),
            verdict: if background && !did.listens.is_empty() { Verdict::Pass } else { Verdict::Fail },
            detail: format!(
                "{} of {} said their threads ran at background priority",
                did.listens.iter().filter(|r| r.background == Some(true)).count(),
                did.listens.iter().filter(|r| r.ok).count()
            ),
        });
    }
    // Each count's back-to-back p99 (offline, plain threads) beside its live one: what pacing and
    // waking cost.
    let blocks = (BACK_TO_BACK * song.rate as f64 / BLOCK as f64).round() as usize;
    let mut b2b: Vec<(usize, f64)> = vec![];
    for f in &rows {
        let n = f.threads.count;
        if !b2b.iter().any(|(m, _)| *m == n) {
            b2b.push((n, Stats::of(&pass(lab, &song, n, blocks)?.blocks).p99));
        }
    }
    let table: Vec<String> = rows
        .iter()
        .map(|f| {
            let s = &f.steady;
            let p99 = shown(s.quantile(0.99).map_or(f64::NAN, |d| d.as_secs_f64() * 1e6), past(s));
            let back = b2b.iter().find(|(n, _)| *n == f.threads.count).map_or(f64::NAN, |(_, p)| *p);
            format!(
                "{} {}: p99 {p99} µs (back to back {back:.0}), max {}, dropouts {}, workers came p99 {}",
                f.threads.count,
                way(f.threads.between),
                known(longest(s), past(s)),
                f.whole.dropouts(),
                entry(&s.threads, 0.99).unwrap_or_else(|| "-".into())
            )
        })
        .collect();
    out.info("live against back to back (10 s each count, offline, plain threads, from 3:00)", table.join("; "));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

    use m2_os::rt::Joined;
    use m2_runtime::Trip;

    use super::*;

    /// A run with nothing wrong: 4 threads at 48 kHz and 128 frames, every worker joined, a minute
    /// of callbacks well under the period.
    fn clean() -> Found {
        let threads = EngineThreads {
            count: 4,
            real_time: 3,
            joined: 3,
            device: Joined { real_time: true, workgroup: true, cancelled: false },
            missing: None,
            between: Between::Park,
            device_mode: 0,
            taken: true,
            advice: Some(4),
        };
        let mut hist = vec![0; 2048];
        hist[200] = 22_000;
        let t = Timing { callbacks: 22_000, hist, max_ns: Some(804_000), frames: 22_000 * 128, clock: 22_000 * 128, ..Timing::default() };
        Found { threads, rate: 48_000, frames: 128, song_rate: 48_000, whole: t.clone(), before: Timing::default(), steady: t }
    }

    fn failed(f: &Found, judged: bool) -> Vec<String> {
        verdict(f, 128, judged).into_iter().filter(|c| c.verdict == Verdict::Fail).map(|c| format!("{}: {}", c.name, c.detail)).collect()
    }

    // G12 (8a's live half): a clean run passes, judged or not.
    #[test]
    fn a_clean_record_passes() {
        assert!(failed(&clean(), true).is_empty(), "{:?}", failed(&clean(), true));
        let judged = verdict(&clean(), 128, true);
        assert!(judged.iter().all(|c| c.verdict == Verdict::Pass));
        assert!(judged[0].detail.starts_with("4 threads (the workgroup advises 4), park; workers real time 3/3"), "{}", judged[0].detail);
        let linux = verdict(&clean(), 128, false);
        assert!(linux.iter().filter(|c| c.verdict == Verdict::Info).all(|c| c.detail.ends_with("not judged on Linux (PLAT-1)")));
        assert_eq!(linux.iter().filter(|c| c.verdict == Verdict::Pass).count(), 2, "crashes and frames, judged everywhere");
    }

    // G12 (8a's live half; design-8a §9.2): each cause alone fails the judged verdict, and is named.
    #[test]
    fn each_cause_fails_the_live_verdict() {
        let with = |change: &dyn Fn(&mut Found)| {
            let mut f = clean();
            change(&mut f);
            f
        };
        let causes: Vec<(&str, Found, bool)> = vec![
            ("dropouts: 1 from Play: the device's 1", with(&|f| f.whole.xruns = 1), false),
            ("dropouts: 1 from Play: the device's 0, late 1", with(&|f| f.whole.late = 1), false),
            ("gaps 1", with(&|f| f.whole.gaps = 1), false),
            ("callback: p50 1868 µs, p99 1868", with(&|f| f.steady.hist[466] = std::mem::take(&mut f.steady.hist[200])), false),
            ("workers real time 2 of 3", with(&|f| f.threads.real_time = 2), false),
            ("workers real time 3 of 3, in the workgroup 2 of 3", with(&|f| f.threads.joined = 2), false),
            ("the device's thread not real time", with(&|f| f.threads.device.real_time = false), false),
            ("set it to 48 kHz", with(&|f| (f.rate, f.song_rate) = (44_100, 44_100)), false),
            ("its callbacks ask 256 frames, not 128", with(&|f| f.frames = 256), false),
            ("no workgroup", with(&|f| f.threads.missing = Some("no workgroup: the output has none".into())), false),
            ("the player crashed", with(&|f| f.whole.crashed = true), true),
            ("crashes 1", with(&|f| f.whole.crashes = 1), true),
            ("non-finite samples 3", with(&|f| f.whole.non_finite = 3), true),
            ("frames: the device took 2815600", with(&|f| f.whole.frames -= 400), true),
        ];
        let past = with(&|f| (f.steady.over, f.steady.max_ns) = (std::mem::take(&mut f.steady.hist[200]), Some(9_000_000)));
        assert!(
            failed(&past, true)[0].contains("p99 over 8192 (over 307% of the period), p99.9 over 8192, max 9000 (337%)"),
            "{:?}",
            failed(&past, true)
        );
        // The longest before the window: the window's own is only known to be past the histogram.
        let before = with(&|f| (f.steady.over, f.steady.max_ns) = (std::mem::take(&mut f.steady.hist[200]), None));
        assert!(failed(&before, true)[0].contains("max over 8192 (over 307%)"), "{:?}", failed(&before, true));
        for (named, f, everywhere) in causes {
            let judged = failed(&f, true);
            assert!(judged.len() == 1 && judged[0].contains(named), "{named}: {judged:#?}");
            assert_eq!(failed(&f, false).len(), usize::from(everywhere), "{named}: judged on Linux too: {everywhere}");
        }
    }

    // G12 (8a's live half, D17): a Mac plays its output device (none: the run fails); anything
    // else the silent clock, never a device.
    #[test]
    fn no_device_fails_on_a_mac_and_plays_the_clock_on_linux() {
        assert_eq!(place("MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads"), Place::Device);
        assert_eq!(place("ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D, x86_64, 32 threads"), Place::Clock);
        assert_eq!(place("unknown"), Place::Clock);
        assert_eq!(stand_in("MacBookAir10,1, macOS 26.6.2, Apple M1, aarch64, 8 threads"), None, "a base M1 judges gate 8");
        assert!(stand_in("MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads").is_some_and(|s| s.contains("provisional")));
    }

    // G12 (8a's live half): the live options are read, and nonsense refused in plain words.
    #[test]
    fn live_options_are_read_and_nonsense_refused() {
        assert_eq!(frames("128").unwrap(), 128);
        assert_eq!(threads("4").unwrap(), 4);
        assert_eq!(between("park,spin").unwrap(), [Between::Park, Between::Spin]);
        assert_eq!(between("spin, spin").unwrap(), [Between::Spin]);
        assert_eq!(minutes("0.5").unwrap(), 0.5);
        for (bad, says) in [
            (minutes("0").unwrap_err(), "more than 0"),
            (minutes("x").unwrap_err(), "more than 0"),
            (frames("0").unwrap_err(), "from 16 to 4096"),
            (frames("8192").unwrap_err(), "from 16 to 4096"),
            (between("x").unwrap_err(), "park, spin or park,spin"),
            (threads("65").unwrap_err(), "from 1 to 64"),
        ] {
            assert!(format!("{bad:#}").contains(says), "{bad:#}");
        }
    }

    // G12 (8a's live half): a block held 50 ms on a player paced in real time, a second of 128-frame
    // callbacks, is counted late (and not every callback is), and fails the judged verdict.
    #[test]
    fn a_slow_block_is_counted_late() {
        let work = std::env::temp_dir().join(format!("m2-gates-g12-late-{}", std::process::id()));
        let mut lab = Lab::new(&work).unwrap();
        let song = lab.project("sine", &crate::songs::sine(false)).and_then(|d| lab.compile(&d)).unwrap();
        let (player, mut d) = Player::driven(RATE);
        player.open(Renderer::new(&song.flat, &song.data, &lab.packages, song.rate).unwrap()).unwrap();
        player.play();
        let mut buf = vec![0.0f32; 2 * 128];
        d.fill(&mut buf, 2);
        let before = player.timing();
        let period = Duration::from_secs_f64(128.0 / f64::from(RATE));
        let start = Instant::now();
        for k in 0..375u32 {
            if k == 100 {
                let open = Arc::new(AtomicBool::new(false));
                let at = d.runner().runs() + 1;
                d.runner().trip(at, Trip::Hold(open.clone()));
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    open.store(true, SeqCst);
                });
            }
            std::thread::sleep((start + period * k).saturating_duration_since(Instant::now()));
            d.fill(&mut buf, 2);
        }
        let t = player.timing().since(&before);
        assert!((1..100).contains(&t.late) && t.callbacks == 375, "late {} of {}", t.late, t.callbacks);
        let f = Found { whole: t.clone(), steady: t, ..clean() };
        let late = format!("dropouts: {} from Play: the device's 0, late {},", f.whole.dropouts(), f.whole.late);
        assert!(failed(&f, true).iter().any(|c| c.starts_with(&late)), "{late}: {:?}", failed(&f, true));
        let _ = std::fs::remove_dir_all(&work);
    }
}

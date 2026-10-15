//! G7f, part 2 (step 7's design, §11.3): what the arena costs the audio thread, measured on p50
//! through the player's own callback, in device buffers of 512 frames at 48 kHz, with no server.
//!
//! - **The callback, playing:** 30 s from `o.at` (3:00 for the recorded run) with no edit, every
//!   callback timed. [`callbacks`] uses only what HEAD `8aa301e` had (`Player::driven`,
//!   `Renderer::new`, `open`, `seek`, `play`, `Driver::fill`), so the same function, built in a
//!   worktree at HEAD, recorded HEAD's numbers in `callback-S.txt` beside this crate's manifest (or
//!   `$M2_CALLBACK_S`); every run prints its own record line (`callback-record …`). A record names
//!   its machine, and a run is compared only with HEAD's record from the same machine (PLAT-1).
//! - **The activation block:** ten edits, a delay added after t02's pluck and taken out again,
//!   each prepared against what plays as the server prepares it. The callback that takes each one
//!   in is one block long (128 frames), so its time is the activation block's own, and the ordinary
//!   block just before it is timed the same way beside it.
//!
//! The rule, written before measuring: the p99 callback within HEAD's + 80 µs (3% of a 2.67 ms
//! block), and the slowest activation block under half the device buffer's period (5.3 ms), printed
//! against the run's p99 callback. Both go into RENDER-32's Today line. The bound is on the block, not
//! the 512-frame callback around it: on p50 a plain callback at HEAD already takes about 8.8 ms.

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use m2_editor::Gestures;
use m2_play::{Driver, Player};
use m2_runtime::{BLOCK, Basis, Keep, Renderer};

use crate::gates::Opts;
use crate::report::{Outcome, same_machine};
use crate::signal::Summary;
use crate::{Lab, Song};

/// The device buffer the callback is timed in: 512 frames at 48 kHz, 10.7 ms.
const FRAMES: usize = 512;
const SECONDS: f64 = 30.0;

/// One callback's frames through `d`, timed in microseconds.
fn fill(d: &mut Driver, buf: &mut [f32]) -> f64 {
    let t = Instant::now();
    d.fill(buf, 2);
    let us = t.elapsed().as_secs_f64() * 1e6;
    drop(d.drain_retired());
    us
}

/// Every callback's time over `seconds` of `song` played from `from` seconds, after a second of
/// warm-up: HEAD's API only.
pub fn callbacks(lab: &Lab, song: &Song, from: f64, seconds: f64) -> Result<Vec<f64>> {
    let mut r = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate)?;
    r.set_strict(false);
    let (player, mut d) = Player::driven(song.rate);
    player.open(r)?;
    player.seek(song.beat((from * song.rate as f64) as u64))?;
    player.play();
    let mut buf = vec![0.0f32; 2 * FRAMES];
    let per_second = song.rate as usize / FRAMES;
    for _ in 0..per_second {
        fill(&mut d, &mut buf);
    }
    Ok((0..(seconds * per_second as f64) as usize).map(|_| fill(&mut d, &mut buf)).collect())
}

/// Nearest rank: the smallest value at or above share `p`.
fn rank(v: &[f64], p: f64) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(((p * v.len() as f64).ceil() as usize).clamp(1, v.len().max(1)) - 1).copied().unwrap_or(0.0)
}

/// The callback record: its p99, mean and max, in µs.
fn record(times: &[f64]) -> (f64, f64, f64) {
    (rank(times, 0.99), times.iter().sum::<f64>() / times.len().max(1) as f64, rank(times, 1.0))
}

/// The first `callback-record` line of `text`: its p99 and mean, and the machine that measured
/// them, which is last on the line since it holds spaces.
fn parse(text: &str) -> Option<(f64, f64, String)> {
    let line = text.lines().find_map(|l| l.trim().strip_prefix("callback-record "))?;
    let (line, machine) = line.split_once(" machine=")?;
    let machine = Some(machine.trim().to_string()).filter(|m| !m.is_empty())?;
    let get = |k: &str| line.split_whitespace().find_map(|w| w.strip_prefix(k)?.strip_prefix('=')?.parse().ok());
    Some((get("p99_us")?, get("mean_us")?, machine))
}

/// G7f, part 2, on `o.reference` (p50 for the gate) from `o.at`.
pub fn arena(lab: &mut Lab, o: &Opts) -> Result<Outcome> {
    let dir = lab.reference(&o.reference)?;
    let song = lab.compile(&dir)?;
    let mut out = Outcome::new(
        "G7f the arena's callback",
        "RENDER-32, RENDER-25",
        &format!("the player on {} from {:.0} s", o.reference.name(), o.at),
    );
    let times = callbacks(lab, &song, o.at, SECONDS)?;
    let (p99, mean, max) = record(&times);
    out.info(
        "callback, playing",
        format!("mean {mean:.0} µs, p99 {p99:.0}, max {max:.0} over {} callbacks of {FRAMES} frames", times.len()),
    );
    let machine = m2_os::process::machine();
    out.info("record", format!("callback-record p99_us={p99:.3} mean_us={mean:.3} max_us={max:.3} machine={machine}"));
    let path = std::env::var_os("M2_CALLBACK_S")
        .map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("callback-S.txt"), Into::into);
    let name = "the p99 callback within HEAD's + 80 µs";
    match std::fs::read_to_string(&path).ok().and_then(|t| parse(&t)) {
        // Only against HEAD's record from this same machine (PLAT-1).
        Some((head, head_mean, then)) if same_machine(&then, &machine) => {
            out.check(name, p99 <= head + 80.0, format!("{p99:.0} µs against HEAD's {head:.0} µs (means {mean:.0} and {head_mean:.0})"));
        }
        Some((.., then)) => {
            out.info("machines", format!("HEAD's record is from {then}; this run is on {machine}"));
            out.skip(name, "recorded, not judged: HEAD's record is from another machine (PLAT-1)");
        }
        None => out.skip(name, format!("no record of HEAD in {}", path.display())),
    }

    // Ten edits on t02 while it plays, each prepared against what plays.
    let mut session = lab.session(&dir)?;
    let site = session.accepted().compiled.sites.iter().find(|x| x.in_project && x.file == "tracks/t02.ts" && x.callee == "pluck").cloned();
    let call = site.map(|x| format!("{}#{}", x.file, x.ordinal)).context("no pluck in t02")?;
    let rev = session.accepted_arc();
    let mut cache = m2_runtime::audio::Cache::default();
    let owner = m2_plugins::Owner::discard();
    let mut r = Renderer::prepare(&rev.compiled.flat, &rev.compiled.data, &session.packages, song.rate, &mut cache, &owner, Basis::Fresh)?;
    r.set_strict(false);
    let (mut plays, mut info) = (rev, r.info());
    let (player, mut d) = Player::driven(song.rate);
    player.open(r)?;
    player.seek(song.beat((o.at * song.rate as f64) as u64))?;
    player.play();
    let mut buf = vec![0.0f32; 2 * FRAMES];
    for _ in 0..song.rate as usize / FRAMES {
        fill(&mut d, &mut buf);
    }
    let (mut taken, mut before, mut prepared, mut named) = (vec![], vec![], vec![], vec![]);
    let restarts = std::collections::HashSet::new();
    for k in 0..10 {
        if k % 2 == 0 {
            session.add_effect(session.accepted().number, &call, "std:delay").map_err(|e| anyhow!("{e}"))?;
        } else {
            m2_editor::undo(&mut session).map_err(|e| anyhow!("{e}"))?;
        }
        let next = session.accepted_arc();
        let t = Instant::now();
        let cont = m2_session::tx::continues::continues(&plays, &next);
        named.push(t.elapsed().as_secs_f64() * 1e3);
        let keep = Keep { old: &plays.compiled.flat, old_data: &plays.compiled.data, info: &info, cont: &cont, restarts: &restarts };
        let t = Instant::now();
        let mut r = Renderer::prepare(
            &next.compiled.flat,
            &next.compiled.data,
            &session.packages,
            song.rate,
            &mut cache,
            &owner,
            Basis::Keep(keep),
        )?;
        cache.prune();
        prepared.push(t.elapsed().as_secs_f64() * 1e3);
        r.set_strict(false);
        let generation = r.generation();
        (plays, info) = (next, r.info());
        // One ordinary block, then the block that takes the activation in, each its own callback;
        // a block first that isn't timed, as the prepare just ran on this thread and left its caches cold.
        fill(&mut d, &mut buf[..2 * BLOCK]);
        before.push(fill(&mut d, &mut buf[..2 * BLOCK]));
        player.activate(r)?;
        taken.push(fill(&mut d, &mut buf[..2 * BLOCK]));
        let e = d.engine().context("an engine")?;
        if e.current().map(|c| c.generation()) != Some(generation) || e.refused_activations() > 0 {
            return Err(anyhow!("edit {k} wasn't taken in the callback after it was sent"));
        }
        for _ in 0..song.rate as usize / FRAMES / 2 {
            fill(&mut d, &mut buf);
        }
    }
    out.info("naming what continues (continues)", Summary::of(&named));
    out.info("preparing the plan", Summary::of(&prepared));
    let half = FRAMES as f64 / song.rate as f64 / 2.0 * 1e6;
    let slowest = taken.iter().copied().fold(0.0, f64::max);
    out.check(
        "the slowest activation block under half the device buffer's period",
        slowest <= half,
        format!(
            "{slowest:.0} µs, p50 {:.0}; the block before it p50 {:.0}, max {:.0} (bound {half:.0} µs; the run's p99 callback {p99:.0})",
            rank(&taken, 0.5),
            rank(&before, 0.5),
            rank(&before, 1.0),
        ),
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::parse;

    // RENDER-32, PLAT-1: a callback record names its machine, and HEAD's, as checked in, the
    // reference Mac.
    #[test]
    fn a_callback_record_names_its_machine() {
        let line = "callback-record p99_us=12909.604 mean_us=8935.735 max_us=17022.895 machine=Mac14,2, macOS 26.6.2, Apple M2, aarch64, 8 threads";
        assert_eq!(
            parse(&format!("noise\n{line}\n")),
            Some((12909.604, 8935.735, "Mac14,2, macOS 26.6.2, Apple M2, aarch64, 8 threads".into()))
        );
        assert_eq!(parse(line.split(" machine=").next().unwrap()), None, "a record names its machine");
        let text = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("callback-S.txt")).unwrap();
        let (.., machine) = parse(&text).expect("callback-S.txt holds a record");
        assert!(machine.starts_with("MacBookPro18,1, macOS "), "{machine}");
    }
}

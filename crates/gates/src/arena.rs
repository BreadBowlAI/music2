//! G7f, part 2 (step 7's design, §11.3): what the arena costs the audio thread, measured on p50
//! through the player's own callback, in device buffers of 512 frames at 48 kHz, with no server.
//!
//! - **The callback, playing:** 30 s from `o.at` (3:00 for the recorded run) with no edit, every
//!   callback timed, in pairs: the app's layout (`m2_runtime::packages::Layout::P2`) and step 7's
//!   first, P1, in turns in one process over the same music (`paired`). [`callbacks`] uses only
//!   what HEAD `8aa301e` had (`Player::driven`, `Renderer::new`, `open`, `seek`, `play`,
//!   `Driver::fill`), so the same code, built in a worktree at HEAD, recorded HEAD's numbers in
//!   `callback-S.txt` beside this crate's manifest (or `$M2_CALLBACK_S`), with P1's cost over HEAD
//!   from the campaign that measured P1. Every run prints its own record line (`callback-record …`),
//!   the app's layout's medians. A record names its machine, and a run is compared only with HEAD's
//!   records from the same machine (PLAT-1).
//! - **The activation block:** ten edits, a delay added after t02's pluck and taken out again,
//!   each prepared against what plays as the server prepares it. The callback that takes each one
//!   in is one block long (128 frames), so its time is the activation block's own, and the ordinary
//!   block just before it is timed the same way beside it.
//!
//! The rule: the callback within HEAD's + 320 µs (3% of a 10.7 ms callback, the owner's bound), the
//! app's layout's cost over P1 here plus P1's over HEAD on the reference Mac. The rule names the
//! p99, but even paired it moves by far more than that between pairs, so the mean is judged and the
//! p99's estimate printed. And the slowest activation block under half the device buffer's period
//! (5.3 ms), printed against the run's p99 callback. The bound is on the block, not the 512-frame
//! callback around it: on p50 a plain callback at HEAD already takes about 8.8 ms.

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use m2_editor::Gestures;
use m2_play::{Driver, Player};
use m2_runtime::packages::Layout;
use m2_runtime::{BLOCK, Basis, Keep, PackageSet, Renderer};

use crate::gates::Opts;
use crate::paired::{Diffs, rank};
use crate::report::{Outcome, same_machine};
use crate::signal::Summary;
use crate::{Lab, Song};

/// The device buffer the callback is timed in: 512 frames at 48 kHz, 10.7 ms.
const FRAMES: usize = 512;
const SECONDS: f64 = 30.0;
/// Pairs of runs, each the app's layout and P1 over the same 30 s in turns (`paired`): about 4
/// minutes on the reference Mac. On a quiet Linux machine the pairs' mean differences spread over
/// about 200 µs, so their median moves by a few tens of µs from run to run.
const PAIRS: usize = 5;
/// Callbacks one layout plays before the other's turn: about 1 s, so a change in the machine's
/// load lasting longer than that meets both alike.
const TURN: usize = 94;

/// One callback's frames through `d`, timed in microseconds.
fn fill(d: &mut Driver, buf: &mut [f32]) -> f64 {
    let t = Instant::now();
    d.fill(buf, 2);
    let us = t.elapsed().as_secs_f64() * 1e6;
    drop(d.drain_retired());
    us
}

/// Every callback's time over `seconds` of `song` played from `from` seconds, once with its kernels
/// built by each of `packages`, after a second of warm-up each: the two players take turns of
/// `TURN` callbacks over the same music, the first first in even turns when `first_first`, else in
/// odd ones, so what disturbs one disturbs the other alike. HEAD's API only.
pub fn callbacks(packages: [&PackageSet; 2], song: &Song, from: f64, seconds: f64, first_first: bool) -> Result<[Vec<f64>; 2]> {
    let mut decks = vec![];
    for p in packages {
        let mut r = Renderer::new(&song.flat, &song.data, p, song.rate)?;
        r.set_strict(false);
        let (player, d) = Player::driven(song.rate);
        player.open(r)?;
        player.seek(song.beat((from * song.rate as f64) as u64))?;
        player.play();
        decks.push((player, d));
    }
    let mut buf = vec![0.0f32; 2 * FRAMES];
    let per_second = song.rate as usize / FRAMES;
    for (_, d) in decks.iter_mut() {
        for _ in 0..per_second {
            fill(d, &mut buf);
        }
    }
    let n = (seconds * per_second as f64) as usize;
    let mut times = [Vec::with_capacity(n), Vec::with_capacity(n)];
    for (k, from) in (0..n).step_by(TURN).enumerate() {
        let order = if (k % 2 == 0) == first_first { [0, 1] } else { [1, 0] };
        for i in order {
            times[i].extend((from..(from + TURN).min(n)).map(|_| fill(&mut decks[i].1, &mut buf)));
        }
    }
    Ok(times)
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

/// P1 with the arena against HEAD's callback on the reference Mac, from its campaign of
/// alternated runs (RENDER-32), paired: its p99 and mean callback's cost, in µs, and the machine.
fn parse_p1(text: &str) -> Option<(f64, f64, String)> {
    let (values, machine) = crate::paired::line(text, "callback-p1")?;
    Some((crate::paired::value(&values, "p99_us")?, crate::paired::value(&values, "mean_us")?, machine))
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
    // The callback, playing, in pairs: the app's layout and P1 in turns over the same music
    // (RENDER-32). One run's p99 varies by far more than the bound between runs.
    let p1 = crate::load_std_laid_out(Layout::P1)?;
    let pairs = PAIRS;
    let (mut app, mut old) = (vec![], vec![]);
    for i in 0..pairs {
        let [a, b] = callbacks([&lab.packages, &p1], &song, o.at, SECONDS, i % 2 == 0)?;
        let ((pa, ma, _), (pb, mb, _)) = (record(&a), record(&b));
        out.info(format!("pair {}", i + 1), format!("the app's layout mean {ma:.0} µs, p99 {pa:.0}; P1 mean {mb:.0}, p99 {pb:.0}"));
        app.push(a);
        old.push(b);
    }
    let med =
        |runs: &[Vec<f64>], f: fn((f64, f64, f64)) -> f64| crate::paired::median(&runs.iter().map(|t| f(record(t))).collect::<Vec<_>>());
    let (p99, mean, max) = (med(&app, |r| r.0), med(&app, |r| r.1), med(&app, |r| r.2));
    out.info(
        format!("callback, playing, the app's layout (medians of {pairs} runs)"),
        format!("mean {mean:.0} µs, p99 {p99:.0}, max {max:.0} over {} callbacks of {FRAMES} frames", app[0].len()),
    );
    out.info(
        format!("callback, playing, P1 (medians of {pairs} runs)"),
        format!("mean {:.0} µs, p99 {:.0}", med(&old, |r| r.1), med(&old, |r| r.0)),
    );
    let (d99, dmean) = (Diffs::of(&app, &old, |t| record(t).0), Diffs::of(&app, &old, |t| record(t).1));
    out.info("callback, the app's layout against P1, pair by pair", format!("mean {dmean}; p99 {d99}"));
    let machine = m2_os::process::machine();
    out.info("record", format!("callback-record p99_us={p99:.3} mean_us={mean:.3} max_us={max:.3} machine={machine}"));
    let path = std::env::var_os("M2_CALLBACK_S")
        .map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("callback-S.txt"), Into::into);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    // The rule bounds the p99 callback, 3% of it (O2), but even the pairs' p99 moves by more than
    // that: the mean is judged with the same bound, and the p99's estimate printed (`paired`).
    let name = "callback within HEAD's + 320 µs, by the mean";
    match (parse(&text), parse_p1(&text)) {
        // Only against HEAD's records from this same machine (PLAT-1).
        (Some((.., then)), Some((p1_99, p1_mean, p1_then))) if same_machine(&then, &machine) && same_machine(&p1_then, &machine) => {
            let (ok, detail) = crate::paired::judge([&dmean, &d99], [p1_mean, p1_99], 320.0);
            out.check(name, ok, detail);
        }
        (Some((.., then)), _) if !same_machine(&then, &machine) => {
            out.info("machines", format!("HEAD's record is from {then}; this run is on {machine}"));
            out.skip(name, "recorded, not judged: HEAD's record is from another machine (PLAT-1)");
        }
        (None, _) => out.skip(name, format!("no record of HEAD in {}", path.display())),
        (Some(_), _) => out.skip(name, format!("no record of P1 against HEAD from this machine in {}", path.display())),
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

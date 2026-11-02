//! G7f, part 2 (step 7's design, §11.3): what step 7 costs the audio thread, measured on p50
//! through the player's own callback, in device buffers of 512 frames at 48 kHz, with no server.
//!
//! - **The callback, playing:** 30 s from `o.at` (3:00 for the recorded run, after a second of
//!   warm-up) with no edit, every callback timed, in 5 rounds of three players taking turns over
//!   the same music: the app's layout (one sandbox a box, RENDER-32) and a second player of it,
//!   its A/A control, printed only, in this process, and S, HEAD `8aa301e`, the commit before step 7, in its runner's
//!   (`baseline`). No player takes two turns in a row, and each turn's first callbacks, which meet
//!   the caches the player before left, are timed apart and not judged. Each round, the three play
//!   the same bits, and not silence, which the gate checks. Every run prints its own record line (`callback-record …`, the app's layout's
//!   medians), and S's as recorded in `callback-S.txt` on the same machine beside S's fresh numbers.
//! - **The activation block:** ten edits, a delay added after t02's pluck and taken out again,
//!   each prepared against what plays as the server prepares it. The callback that takes each one
//!   in is one block long (128 frames), so its time is the activation block's own, and the ordinary
//!   block just before it is timed the same way beside it.
//!
//! The rule: the callback within S's + 320 µs (3% of a 10.7 ms callback, the owner's bound), the
//! median of the rounds' differences, the app's layout minus S, both measured here, and judged only
//! on the reference Mac, the machine `callback-S.txt` names (PLAT-1); elsewhere it is printed, and on
//! another Mac it also says what to do if it is the reference Mac now. The rule names the p99, but even paired it moves by far
//! more than that between pairs, so the mean is judged and the p99's difference printed. The app's
//! layout against its control, what noise alone gives, is printed, not judged: the app is judged
//! against S only. And the slowest activation block under half the device
//! buffer's period (5.3 ms), printed against the run's p99 callback. The bound is on the block, not
//! the 512-frame callback around it: on p50 a plain callback at S already takes about 8.8 ms.

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use m2_editor::Gestures;
use m2_play::{Driver, Player};
use m2_runtime::{BLOCK, Basis, Keep, PackageSet, Renderer};

use crate::baseline::{Runner, wire::Heard};
use crate::gates::Opts;
use crate::paired::{Diffs, Place, Side, in_turns, rank};
use crate::report::{Outcome, same_machine};
use crate::signal::Summary;
use crate::{Lab, Song};

/// The device buffer the callback is timed in: 512 frames at 48 kHz, 10.7 ms.
const FRAMES: usize = 512;
const SECONDS: f64 = 30.0;
/// Rounds of the three players, each over the same 30 s in turns (`paired`): about 8 minutes on the
/// reference Mac (an estimate: three players of about 9 ms a callback, 38.5 s each a round). On a quiet Linux machine the
/// pairs' mean differences spread over about 200 µs, so their median moves by a few tens of µs from
/// run to run.
const ROUNDS: usize = 5;
/// Callbacks one player plays, and judges, before the next one's turn: about 1.65 s, so a change in
/// the machine's load lasting longer than that meets all alike. 2,790 callbacks a round make 18
/// turns of each player, three whole cycles of the orders (`paired::cycle`), so within each round
/// every player follows each other one as often.
const TURN: usize = 155;
/// Callbacks each turn starts with, before those: about half a second, timed but not judged, as
/// they meet the caches the other players left (`paired::in_turns`). On Linux, held on the CPU's
/// 96 MB-cache die, the app's layout's cost over S rose from about 0 to +60 µs over a turn's first
/// 50 callbacks. Each run prints whether any start is left in the judged callbacks.
const LEAD: usize = 47;

/// One callback's frames through `d`, timed in microseconds.
fn fill(d: &mut Driver, buf: &mut [f32]) -> f64 {
    let t = Instant::now();
    d.fill(buf, 2);
    let us = t.elapsed().as_secs_f64() * 1e6;
    drop(d.drain_retired());
    us
}

/// A player here, its kernels built by one layout's packages: one side of the turns, as S's runner
/// plays S on another, with the same calls.
struct Here {
    _player: Player,
    d: Driver,
    buf: Vec<f32>,
    heard: Heard,
    /// The loudest sample of its timed callbacks.
    peak: f32,
}

impl Here {
    /// `song`, playing from `from` seconds.
    fn new(packages: &PackageSet, song: &Song, from: f64) -> Result<Here> {
        let mut r = Renderer::new(&song.flat, &song.data, packages, song.rate)?;
        r.set_strict(false);
        let (player, d) = Player::driven(song.rate);
        player.open(r)?;
        player.seek(song.beat((from * song.rate as f64) as u64))?;
        player.play();
        Ok(Here { _player: player, d, buf: vec![0.0; 2 * FRAMES], heard: Heard::default(), peak: 0.0 })
    }
}

impl Side for Here {
    fn warm(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            fill(&mut self.d, &mut self.buf);
        }
        Ok(())
    }

    fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
        let mut times = Vec::with_capacity(lead + n);
        for _ in 0..lead {
            times.push(fill(&mut self.d, &mut self.buf));
        }
        for _ in 0..n {
            times.push(fill(&mut self.d, &mut self.buf));
            self.heard.add(&self.buf);
            self.peak = self.buf.iter().fold(self.peak, |m, x| m.max(x.abs()));
        }
        Ok(times)
    }

    fn heard(&mut self) -> Result<Heard> {
        Ok(self.heard)
    }
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
    // The callback, playing, in rounds: the app's layout and its A/A control here, and S in its
    // runner, in turns over the same music (RENDER-32). One run's p99 varies by far more than the
    // bound between runs. The control, a second player of the app's layout, is printed only.
    let control = crate::load_std()?;
    let mut s = Runner::start(&lab.work)?;
    out.info("S", s.about());
    let per_second = song.rate as usize / FRAMES;
    let n = (SECONDS * per_second as f64) as usize;
    let (mut runs, mut heard): ([Vec<Vec<f64>>; 3], Vec<[Heard; 3]>) = Default::default();
    let mut peak = 0.0f32;
    let mut starts: [Vec<(f64, f64)>; 3] = Default::default();
    for i in 0..ROUNDS {
        let (mut app, mut old) = (Here::new(&lab.packages, &song, o.at)?, Here::new(&control, &song, o.at)?);
        s.player(&dir, o.at, FRAMES)?;
        let turns = in_turns(&mut [&mut app, &mut old, &mut s], per_second, n, TURN, LEAD, i)?;
        for (k, at) in starts.iter_mut().enumerate() {
            at.push(turns.starts(k));
        }
        let times = turns.timed;
        heard.push([app.heard()?, old.heard()?, s.heard()?]);
        peak = peak.max(app.peak);
        s.drop_it()?;
        let r: Vec<(f64, f64, f64)> = times.iter().map(|t| record(t)).collect();
        out.info(
            format!("round {}", i + 1),
            format!(
                "the app's layout mean {:.0} µs, p99 {:.0}; its control mean {:.0}, p99 {:.0}; S mean {:.0}, p99 {:.0}",
                r[0].1, r[0].0, r[1].1, r[1].0, r[2].1, r[2].0
            ),
        );
        for (run, t) in runs.iter_mut().zip(times) {
            run.push(t);
        }
    }
    drop(s);
    let (same, said) = crate::paired::same_music(&heard, peak);
    out.check("the same music: every player's timed callbacks the same bits", same, said);
    let med =
        |runs: &[Vec<f64>], f: fn((f64, f64, f64)) -> f64| crate::paired::median(&runs.iter().map(|t| f(record(t))).collect::<Vec<_>>());
    let (p99, mean, max) = (med(&runs[0], |r| r.0), med(&runs[0], |r| r.1), med(&runs[0], |r| r.2));
    out.info(
        format!("callback, playing, the app's layout (medians of {ROUNDS} runs)"),
        format!("mean {mean:.0} µs, p99 {p99:.0}, max {max:.0} over {} callbacks of {FRAMES} frames", runs[0][0].len()),
    );
    for (k, name) in [(1, "its A/A control"), (2, "S")] {
        out.info(
            format!("callback, playing, {name} (medians of {ROUNDS} runs)"),
            format!("mean {:.0} µs, p99 {:.0}, max {:.0}", med(&runs[k], |r| r.1), med(&runs[k], |r| r.0), med(&runs[k], |r| r.2)),
        );
    }
    let against = |a: usize, b: usize| (Diffs::of(&runs[a], &runs[b], |t| record(t).1), Diffs::of(&runs[a], &runs[b], |t| record(t).0));
    let (dmean, d99) = against(0, 2);
    let (aa_mean, aa_99) = against(0, 1);
    out.info("callback, the app's layout against S, pair by pair", format!("mean {dmean}; p99 {d99}"));
    out.info("callback, the app's layout against its A/A control (noise alone)", format!("mean {aa_mean}; p99 {aa_99}"));
    out.info("callback, starting a turn", crate::paired::starting(&starts, LEAD, "callback"));
    let machine = m2_os::process::machine();
    out.info("record", format!("callback-record p99_us={p99:.3} mean_us={mean:.3} max_us={max:.3} machine={machine}"));
    // S as its campaign recorded it on this machine, beside S measured now: printed, not judged.
    let path = std::env::var_os("M2_CALLBACK_S")
        .map_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("callback-S.txt"), Into::into);
    // That record names the reference Mac: only there is the callback judged (PLAT-1).
    let record = parse(&std::fs::read_to_string(&path).unwrap_or_default());
    let place = Place::of(record.as_ref().map(|(.., then)| then.as_str()), &machine);
    if let Some((.., then)) = record.as_ref().filter(|(.., then)| !same_machine(then, &machine)) {
        out.info("machines", format!("S's record is from {then}; this run is on {machine}"));
    }
    if let Some((then99, then_mean, _)) = record.filter(|_| place == Place::Reference) {
        out.info(
            "S as recorded",
            format!(
                "mean {then_mean:.0} µs, p99 {then99:.0} in {}; measured here, mean {:.0}, p99 {:.0}",
                path.display(),
                med(&runs[2], |r| r.1),
                med(&runs[2], |r| r.0)
            ),
        );
    }
    // The rule bounds the p99 callback, 3% of it (O2), but even the pairs' p99 moves by more than
    // that: the mean is judged with the same bound, and the p99's difference printed (`paired`).
    let judged = crate::paired::judge([&dmean, &d99], 320.0);
    crate::paired::timed(&mut out, "callback within S's + 320 µs, by the mean", &place, judged);

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
            session.add_effect(session.accepted().number, &call, "@m2/effects:delay").map_err(|e| anyhow!("{e}"))?;
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
    use super::{FRAMES, SECONDS, TURN, parse};

    // RENDER-32: a round is whole cycles of the turns' orders, all its turns as long, so within each
    // pair every player follows each other one as often, and waits for its turns as the cycle says.
    #[test]
    fn a_round_is_whole_cycles_of_turns() {
        let n = (SECONDS * (48_000 / FRAMES) as f64) as usize;
        let orders = n.div_ceil(TURN);
        assert_eq!(
            (n % TURN, orders % crate::paired::cycle(3).unwrap().len()),
            (0, 0),
            "{n} callbacks in turns of {TURN}: {orders} orders"
        );
    }

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

//! `m2-gates`: measure step 3's acceptance gates and print what they found.
//!
//!   cargo run -p m2-gates --release -- [--rig replay|engine] [--server] [--quick] [gate ...]
//!
//! See the crate's README for what each gate does and how to read its numbers.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use m2_gates::gates::{self, GATES, Opts};
use m2_gates::report::{self, Outcome, Verdict};
use m2_gates::rig::Kind;
use m2_gates::{Lab, server};
use m2_session::reference::Reference;

const USAGE: &str = "m2-gates: measure step 3's acceptance gates (docs/architecture.md, Acceptance gates)

USAGE:
  cargo run -p m2-gates --release -- [options] [gate ...]

GATES (default: all):
  seek        G1  a seek is heard within 50 ms, resets nothing, plays every note from its target
                  on and chases nothing
  loop        G2  a 4-bar loop at 127 bpm: no tail cut at a wrap, no drift against the grid
  note-edit   G3  a note edit is heard within compile + 20 ms, by SwapLane, with no tail cut
  stuck       G4  deleting, shortening, stopping, wrapping or seeking through a note leaves none stuck
  add-effect  G5  an added effect is heard within compile + 100 ms, with no jump back; through the
                  server, G7a too: ten edits adding and taking out a delay on t02, each heard within
                  200 ms of its request (step 7)
  tails       G7b the tails of untouched tracks are intact through an edit: bit for bit against a
                  twin, every stateful node outside the edited track kept (step 7)
  step        G6  activations while a sine plays make no step over twice the sine's own, and keep
                  the beat
  tail-stop   G7  after stop, reverb and delay tails decay instead of freezing
  range       G10 range render timing, and whether it says it is exact
  plugins     G1-G5 and G7 on examples/with-plugins (Surge XT and Airwindows on the CLAP path),
                  with the plugin instances carried across rebuilds (step 3b, HOST-5)
  previews    G8  (--server only) a knob drag's updates are heard as fast while the session
                  compiles a slow save (WRITE-22, RENDER-29)
  drag        G9  (--server only) a knob drag goes on through an agent's saves to other files
                  and to its own, and its commit writes its file alone (WRITE-22)
  tx-data     G11 (--server only) piano-roll writes as journaled Tx, each heard as its preview:
                  from the request to the block that plays it, under 20 ms at p95, and apart until
                  the commit is durable, on an idle disk, a busy one and with 5,000 assets; a
                  held arrow key's nudges; a refused write taken back; the barrier's durable
                  sync alone (F_FULLFSYNC on macOS, fsync on Linux) (PLANE-5)
  watcher     G11 (--server only) the watcher with 5,000 files in assets/: the CPU it uses while
                  idle, under 1% of a core, and an outside lane write from its last byte to the
                  block that plays it; the full stat walk (WRITE-19)
  export      G7e (--server only) an export of examples/with-plugins (Surge XT and
                  Airwindows on the CLAP path) pauses playback, reuses its plugin instances, and
                  playback resumes where it was with their state intact (step 7, RENDER-1)
  arena       G7f part 2 (named only) step 7's cost to the player's callback, by the mean against
                  S measured in the same run (S: 8aa301e, the commit before step 7), and the
                  callback that takes in each of ten activations (step 7). Needs S's runner:
                  $M2_GATES_S, from crates/gates/s/build.sh
  store       G7f part 1 (named only) what one wasmtime Store per kernel instance costs: build,
                  memory, address space, block time and drop; the block time (by the mean) and the
                  build against S measured in the same run (RENDER-32). Needs $M2_GATES_S too

OPTIONS:
  --rig R          engine: step 3a's live engine (needs --features engine); replay: HEAD's
                   rebuild-and-replay engine, the negative reference (default: engine when built
                   with it, else replay)
  --server         seek, note-edit and add-effect through the app's server in real time, on the
                   silent player, instead of on a rig
  --no-arena       the engine rig keeps no instance across an activation: every one is new and the
                   whole old graph plays out as ghosts, the negative reference for the arena
                   (RENDER-25, HOST-5)
  --quick          CI sizes: a 3-track reference project, 1 minute of loop, 8 seeks
  --reference N    the reference project the timed gates play: 16 (p16, the default) or 50 (p50)
  --stems N        add N one-minute stems on audio lanes (the audio-heavy variant)
  --minutes M      how long the loop gate loops (default 10)
  --seeks N        seeks for G1 (default 50; 5 on the replay rig, where each one replays)
  --activations N  activations per song for G6 (default 50)
  --edits N        edits G3 and G5 time through the server (default 10)
  --at S           where G5, G7a and G7b edit, in seconds of the song (default 60; 180 for step 7's
                   recorded runs on p50)
  --seed N         for the random seeks and clocks (default 3)
  --work DIR       where projects are generated (default: a new folder under the temp dir)
  --keep           keep the work folder afterwards
";

fn main() {
    // Before any thread starts: server runs must never open a real device, and the silent player
    // times with 512-frame buffers (step 3a's design, §15).
    // SAFETY: nothing else runs yet, so nothing reads the environment concurrently.
    unsafe {
        std::env::set_var("M2_AUDIO", "none");
        if std::env::var_os("M2_AUDIO_FRAMES").is_none() {
            std::env::set_var("M2_AUDIO_FRAMES", "512");
        }
    }
    // HOST-2: the gates run on a worker while the main thread serves plugins as their main thread,
    // and destroys what they created before the process exits.
    let code = m2_plugins::run_main(|| match run() {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(e) => {
            eprintln!("m2-gates: {e:#}");
            2
        }
    });
    std::process::exit(code);
}

fn run() -> Result<bool> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut o = Opts::recorded();
    let mut kind = if cfg!(feature = "engine") { Kind::Engine } else { Kind::Replay };
    let (mut via_server, mut keep, mut work, mut seeks, mut arena) = (false, false, None, None, true);
    let mut names = vec![];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(true);
            }
            "--rig" => {
                kind = match value()?.as_str() {
                    "replay" => Kind::Replay,
                    "engine" => Kind::Engine,
                    other => bail!("no rig `{other}`: replay or engine"),
                }
            }
            "--server" => via_server = true,
            "--no-arena" => arena = false,
            "--quick" => {
                let q = Opts::quick();
                (o.reference, o.loop_minutes, o.seeks, o.activations, o.edits) =
                    (q.reference, q.loop_minutes, q.seeks, q.activations, q.edits);
            }
            "--reference" => {
                o.reference = Reference { tracks: value()?.trim_start_matches('p').parse().context("--reference")?, ..o.reference }
            }
            "--stems" => o.reference.stems = value()?.parse().context("--stems")?,
            "--minutes" => o.loop_minutes = value()?.parse().context("--minutes")?,
            "--seeks" => seeks = Some(value()?.parse().context("--seeks")?),
            "--activations" => o.activations = value()?.parse().context("--activations")?,
            "--edits" => o.edits = value()?.parse::<usize>().context("--edits")?.max(1),
            "--at" => o.at = value()?.parse().context("--at")?,
            "--seed" => o.seed = value()?.parse().context("--seed")?,
            "--work" => work = Some(PathBuf::from(value()?)),
            "--keep" => keep = true,
            g if !g.starts_with("--") => names.push(g.to_string()),
            other => bail!("unknown option {other}\n\n{USAGE}"),
        }
    }
    // HEAD replays the song for every seek: a few make the point.
    o.seeks = seeks.unwrap_or(if kind == Kind::Replay && !via_server { 5.min(o.seeks) } else { o.seeks });
    let all = names.is_empty() || names.iter().any(|n| n == "all");
    if all {
        names = GATES.iter().map(|g| g.0.to_string()).collect();
    }
    if via_server {
        // Step 3b's gates exist only through the server.
        if all {
            names.extend(["previews", "drag", "tx-data", "watcher", "export"].map(String::from));
        }
        names.retain(|n| ["seek", "note-edit", "add-effect", "previews", "drag", "tx-data", "watcher", "export"].contains(&n.as_str()));
    }
    let made = work.is_none();
    let work = work.unwrap_or_else(|| std::env::temp_dir().join(format!("m2-gates-{}", std::process::id())));
    let mut lab = Lab::new(&work)?;
    lab.arena = arena;
    println!("m2-gates: {}", report::profile());
    // Every run names its machine: only the reference Mac's timings are judged (PLAT-1).
    println!("machine: {}", m2_os::process::machine());
    println!(
        "rig: {}{}; reference {}; work in {}",
        if via_server { "server, real time" } else { kind.name() },
        if arena { "" } else { ", no instance kept" },
        o.reference.name(),
        work.display()
    );
    let (mut failed, mut skipped) = (0, 0);
    for name in &names {
        let t = Instant::now();
        let outcome: Result<Outcome> = match (via_server, name.as_str()) {
            (true, "seek") => server::seek(&mut lab, &o),
            (true, "note-edit") => server::note_edit(&mut lab, &o),
            (true, "add-effect") => server::add_effect(&mut lab, &o),
            (true, "previews") => server::previews(&mut lab, &o),
            (true, "drag") => server::drag(&mut lab, &o),
            (true, "tx-data") => server::tx_data(&mut lab, &o),
            (true, "watcher") => server::watcher(&mut lab, &o),
            (true, "export") => server::export(&mut lab, &o),
            (false, "store") => m2_gates::store::store(&mut lab, &o),
            (false, "arena") => m2_gates::arena::arena(&mut lab, &o),
            _ => gates::run(name, &mut lab, kind, &o),
        };
        match outcome {
            Ok(outcome) => {
                match outcome.verdict() {
                    Verdict::Fail => failed += 1,
                    Verdict::Skip => skipped += 1,
                    _ => {}
                }
                print!("\n{outcome}");
            }
            Err(e) => {
                failed += 1;
                println!("\nFAIL  {name}: couldn't run: {e:#}");
            }
        }
        println!("      ({:.1} s)", t.elapsed().as_secs_f64());
    }
    if made && !keep {
        let _ = std::fs::remove_dir_all(&work);
    }
    println!("\n{} gates: {} passed, {failed} failed, {skipped} not measured here", names.len(), names.len() - failed - skipped);
    Ok(failed == 0)
}

#[cfg(test)]
mod tests {
    use super::USAGE;

    // RENDER-32: --help says what G7f judges, against S measured in the same run, and that both
    // parts need S's runner, which they can't run without.
    #[test]
    fn the_help_says_what_g7f_needs() {
        for gate in ["arena", "store"] {
            let mut lines = USAGE.lines().skip_while(|l| !l.starts_with(&format!("  {gate} ")));
            let first = lines.next().unwrap();
            let rest = lines.take_while(|l| l.starts_with(&" ".repeat(18)));
            let entry = std::iter::once(first).chain(rest).flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ");
            assert!(entry.contains("by the mean"), "{gate}: {entry}");
            assert!(entry.contains("against S measured in the same run"), "{gate}: {entry}");
            assert!(entry.contains("$M2_GATES_S"), "{gate}: {entry}");
        }
        assert!(!USAGE.contains("p99 playing"), "the p99 is printed, not judged");
    }
}

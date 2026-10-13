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
  add-effect  G5  an added effect is heard within compile + 100 ms, with no jump back
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

OPTIONS:
  --rig R          engine: step 3a's live engine (needs --features engine); replay: HEAD's
                   rebuild-and-replay engine, the negative reference (default: engine when built
                   with it, else replay)
  --server         seek, note-edit and add-effect through the app's server in real time, on the
                   silent player, instead of on a rig
  --no-carry       the engine rig builds every plugin instance anew with each renderer, as step 3a
                   did: the negative reference for carrying them across rebuilds (HOST-5)
  --quick          CI sizes: a 3-track reference project, 1 minute of loop, 8 seeks
  --reference N    the reference project the timed gates play: 16 (p16, the default) or 50 (p50)
  --stems N        add N one-minute stems on audio lanes (the audio-heavy variant)
  --minutes M      how long the loop gate loops (default 10)
  --seeks N        seeks for G1 (default 50; 5 on the replay rig, where each one replays)
  --activations N  activations per song for G6 (default 50)
  --edits N        edits G3 and G5 time through the server (default 10; G5 adds an effect to up to
                   N tracks)
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
    let (mut via_server, mut keep, mut work, mut seeks, mut carry) = (false, false, None, None, true);
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
            "--no-carry" => carry = false,
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
            names.extend(["previews".to_string(), "drag".to_string()]);
        }
        names.retain(|n| ["seek", "note-edit", "add-effect", "previews", "drag"].contains(&n.as_str()));
    }
    let made = work.is_none();
    let work = work.unwrap_or_else(|| std::env::temp_dir().join(format!("m2-gates-{}", std::process::id())));
    let mut lab = Lab::new(&work)?;
    lab.carry = carry;
    println!("m2-gates: {}", report::profile());
    println!(
        "rig: {}{}; reference {}; work in {}",
        if via_server { "server, real time" } else { kind.name() },
        if carry { "" } else { ", plugin instances never carried" },
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

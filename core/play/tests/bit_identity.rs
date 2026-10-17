//! RENDER-33: live playback from transport 0, on a fresh engine, with no edits, no jumps and no
//! overlays, driven with arbitrary device buffer sizes, gives the same `f32` bits as the offline
//! render. Step 3's gate, run on every pure fixture from step 3 on.
//!
//! "Fresh" matters less since the sounds merge (std 0.2). Stopped, the engine runs its kernels on
//! (RENDER-6a), but std's chorus, phaser and tremolo wait while the song is stopped
//! (`time.playing`), the bitcrusher's clock is put back at Play, and a drum's seed is its lane's
//! hit count: so every pure fixture plays the export after the engine sat stopped too
//! (`std_plays_the_export_after_the_engine_sat_stopped`). It still matters to free-running sources
//! at the top level (std's oscillators and `noise`), which an engine that sat stopped plays from
//! another phase, and to a smoother that sets itself from its first input while stopped.

mod common;

use std::sync::Arc;

use common::*;
use m2_play::Player;
use m2_runtime::{BLOCK, Cmd, Continues, LaneSwap, PlanInfo, Renderer, notes_data};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

/// `song` live through the player, from 0 for `frames`, with a no-op swap of every lane at
/// frame `swap_at` when given.
fn live(song: &Song, frames: usize, swap_at: Option<usize>) -> Vec<[f32; 2]> {
    let r = song.renderer();
    let generation = r.generation();
    let slots = r.lane_slots().to_vec();
    let (player, mut d) = playing(r, song.rate);
    let mut swapped = false;
    hear(&mut d, frames, &mut |_, done| {
        if swap_at.is_some_and(|at| done >= at) && !swapped {
            swapped = true;
            for (i, s) in slots.iter().enumerate() {
                let data = match s.kind {
                    m2_ir::Kind::Events(_) => LaneSwap::Notes(Arc::new(notes_data(&song.data, &s.module, &s.lane, song.rate as f64))),
                    m2_ir::Kind::Control(_) => LaneSwap::Control(Arc::new(song.data.control(&s.module, &s.lane))),
                    m2_ir::Kind::Audio(_) => continue,
                };
                player.swap_lane(generation, i as u32, data).unwrap();
            }
        }
    })
}

fn offline(song: &Song, frames: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    song.renderer().process(&mut out).unwrap();
    out
}

fn check(name: &str, song: &Song, frames: usize, swap_at: Option<usize>) {
    let export = offline(song, frames);
    let heard = live(song, frames, swap_at);
    assert!(peak(&export) > 0.0 || name.contains("empty"), "{name} makes sound");
    assert_eq!(first_diff(&heard, &export), None, "{name}: live differs from the export");
}

// RENDER-33 on every pure fixture: their whole song and a second past it.
#[test]
fn live_is_the_export_on_every_pure_fixture() {
    for name in ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter", "templates/empty"] {
        let song = fixture(name);
        let frames = (song.seconds() * song.rate as f64) as usize;
        check(name, &song, frames, None);
    }
}

// RENDER-26, RENDER-26a: a swap of the same data, while notes sound, changes nothing at all.
#[test]
fn a_no_op_swap_changes_no_bit() {
    let song = fixture("examples/first-song");
    check("first-song with a swap at 5 s", &song, 8 * SR as usize, Some(5 * SR as usize));
    // Equal keys: duplicate notes in a clip, layered clips with one id, anonymous clips at one start.
    let dup = |len: f64| json!({"pitch": 60, "start": 0, "length": len});
    let lane = json!({"clips": [
        {"id": "a", "start": 0, "length": 16, "notes": [dup(8.0), dup(12.0), {"pitch": 64, "start": 2, "length": 10}]},
        {"id": "l", "start": 4, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 8}]},
        {"id": "l", "start": 4, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 8}]},
        {"start": 6, "length": 8, "notes": [{"pitch": 72, "start": 0, "length": 6}]},
        {"start": 6, "length": 8, "notes": [{"pitch": 72, "start": 0, "length": 6}]}]});
    let song = synth(0.1, json!([]), "v", lane, at_bpm(120.0));
    check("duplicates with a swap at 3.5 s", &song, 9 * SR as usize, Some(7 * SR as usize / 2));
}

/// `song` live through the player after it sat stopped for `idle` frames, then played from 0
/// for `frames`.
fn live_after_idle(song: &Song, idle: usize, frames: usize) -> Vec<[f32; 2]> {
    let (player, mut d) = Player::driven(song.rate);
    player.open(song.renderer()).unwrap();
    assert_eq!(peak(&hear(&mut d, idle, &mut |_, _| {})), 0.0, "silence while stopped");
    let seq = player.transport(Cmd::Play).unwrap();
    // Played from the block line after the frames heard.
    let heard = hear(&mut d, frames + BLOCK, &mut |_, _| {});
    let at = player.applied(seq).expect("the play applied").clock as usize - idle;
    assert!(at < BLOCK, "played from {at} frames after the play was sent");
    heard[at..at + frames].to_vec()
}

/// A beat on std's drums in a temp project: the kit on one lane (every kind of drum, and the
/// hats choking each other), a snare roll and the hats on lanes of their own, at 120 bpm.
fn drum_beat() -> Song {
    let lane = |id: &str, notes: Vec<Value>| json!({"clips": [{"id": id, "start": 0, "length": 8, "notes": notes}]}).to_string();
    let note = |pitch: u32, start: f64, velocity: f64| json!({"pitch": pitch, "start": start, "length": 0.1, "velocity": velocity});
    // In 8th notes: kick, snare, clap, closed, pedal and open hats, toms, rim, cowbell, crash, ride, shaker.
    let mut kit = Vec::new();
    for (k, pitch) in [36, 42, 38, 42, 36, 46, 39, 44, 45, 47, 37, 56, 49, 51, 69, 42].into_iter().enumerate() {
        kit.push(note(pitch, k as f64 * 0.5, 0.6 + 0.02 * k as f64));
    }
    // A 32nd-note snare roll through the second half, rising; 16th-note hats, open every half bar.
    let roll = (0..32).map(|k| note(38, 4.0 + k as f64 / 8.0, 0.4 + 0.015 * k as f64)).collect();
    let hats = (0..32).map(|k| note(if k % 8 == 6 { 46 } else { 42 }, k as f64 / 4.0, 0.7)).collect();
    project(
        "drums",
        &[
            (
                "song.ts",
                r#"import { song } from "@m2/core";
import { mixer } from "std/mix";
import drums from "./tracks/drums";

export default song(() => mixer({ drums: drums.out }));
"#,
            ),
            (
                "tracks/drums.ts",
                r#"import { track } from "@m2/core";
import { drums } from "std/drums";
import { mixer } from "std/mix";

export default track("drums", ({ lane }) =>
  mixer({
    kit: drums.kit(lane.notes("kit")),
    snare: drums.snare(lane.notes("snare")),
    hats: drums.hats(lane.notes("hats")),
  }),
);
"#,
            ),
            ("data/arrangement.json", &json!({"tempo": [{"beat": 0, "bpm": 120}], "meter": []}).to_string()),
            ("data/drums/kit.json", &lane("kitbeat1", kit)),
            ("data/drums/snare.json", &lane("snaroll1", roll)),
            ("data/drums/hats.json", &lane("hatline1", hats)),
        ],
    )
}

/// A chord held for 3 seconds at 120 bpm, on std's pad, through `effect`, a call of one of
/// std/fx's effects on `x`: one song per effect, so each is judged on its own.
fn held(name: &str, effect: &str) -> Song {
    let chord: Vec<Value> = [48, 55, 60, 64, 67].iter().map(|p| json!({"pitch": p, "start": 0, "length": 6})).collect();
    let track = format!(
        "import {{ track }} from \"@m2/core\";\nimport {{ pad }} from \"std/instruments\";\nimport {{ {name} }} from \"std/fx\";\n\n\
         export default track(\"held\", ({{ lane }}) => {{\n  const x = pad(lane.notes(\"notes\"));\n  return {effect};\n}});\n"
    );
    project(
        name,
        &[
            (
                "song.ts",
                "import { song } from \"@m2/core\";\nimport held from \"./tracks/held\";\n\nexport default song(() => held.out);\n",
            ),
            ("tracks/held.ts", &track),
            ("data/arrangement.json", &json!({"tempo": [{"beat": 0, "bpm": 120}], "meter": []}).to_string()),
            ("data/held/notes.json", &json!({"clips": [{"id": "chord001", "start": 0, "length": 8, "notes": chord}]}).to_string()),
        ],
    )
}

/// The songs played after the engine sat stopped: std's drums, each of std's effects that follow
/// the song alone, at rates that make a difference plain, and every pure fixture.
fn after_idle_songs() -> Vec<(String, Song)> {
    let mut out = vec![("a drum beat".to_string(), drum_beat())];
    for (name, effect) in [
        ("chorus", "chorus(x, { rate: 1.7, depth: 1, mix: 0.5 })"),
        ("phaser", "phaser(x, { rate: 1.3, depth: 1, feedback: 0.6, mix: 0.5 })"),
        ("tremolo", "tremolo(x, { rate: 3.3, depth: 1 })"),
        ("bitcrush", "bitcrush(x, { bits: 6, rate: 7000, mix: 1 })"),
    ] {
        out.push((format!("a held chord through {name}"), held(name, effect)));
    }
    for name in ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter", "templates/empty"] {
        out.push((name.to_string(), fixture(name)));
    }
    out
}

// RENDER-33 after the engine sat stopped: a song opened and played a few seconds later (or a few
// blocks), from 0, is the export bit for bit. Stopped, the engine runs its kernels on so tails
// decay (RENDER-6a), so nothing of std's effects or drums may follow how long the engine has run.
// A drum's hits take their seed from how many hits its lane has played (std:hitcount), never from
// the samples processed. Chorus, phaser and tremolo step their LFOs, and the bitcrusher its clock,
// only while the song plays (`time.playing`). Every song is judged, and every difference named.
// None of these songs has a free-running source at its top level (the module doc).
#[test]
fn std_plays_the_export_after_the_engine_sat_stopped() {
    let mut differs = vec![];
    for (name, song) in after_idle_songs() {
        let frames = (song.seconds() * song.rate as f64) as usize;
        let export = offline(&song, frames);
        assert!(peak(&export) > 0.01 || name.contains("empty"), "{name} makes sound");
        for idle in [2 * SR as usize, 5 * BLOCK] {
            let heard = live_after_idle(&song, idle, frames);
            if let Some(at) = first_diff(&heard, &export) {
                differs.push(format!("{name}, played after {idle} frames stopped: from frame {at}"));
            }
        }
    }
    assert!(differs.is_empty(), "live differs from the export:\n{}", differs.join("\n"));
}

/// The 16-track reference project (p16), written into a temp folder.
fn p16() -> Song {
    reference(&m2_session::reference::Reference::P16)
}

/// A reference project, written into a temp folder.
fn reference(r: &m2_session::reference::Reference) -> Song {
    let dir = temp(&r.name());
    std::fs::remove_dir(&dir).unwrap();
    m2_session::reference::write(&dir, r).unwrap();
    compile(dir)
}

// RENDER-33 on p16: its first 30 seconds in CI.
#[test]
fn live_is_the_export_on_p16() {
    check("p16, 30 s", &p16(), 30 * SR as usize, None);
}

// RENDER-33 on all of p16 (4:02), recorded: `cargo test -p m2-play --release -- --ignored`.
#[test]
#[ignore = "about a minute in release"]
fn live_is_the_export_on_all_of_p16() {
    let song = p16();
    let frames = (song.seconds() * SR as f64) as usize;
    check("p16", &song, frames, None);
}

/// `song` live from 0 for `frames`, with a plan activated at each of `at`, each made by `next`
/// against the plan sent before it; every activation must keep every instance and fade nothing.
fn activated(song: &Song, frames: usize, at: &[usize], next: &dyn Fn(&PlanInfo) -> Renderer) -> Vec<[f32; 2]> {
    let r = song.renderer();
    let mut info = r.info();
    let (player, mut d) = playing(r, song.rate);
    let mut sent = 0;
    let mut stats = vec![];
    let out = hear(&mut d, frames, &mut |d, done| {
        // Each activation's stats, once it plays.
        let cur = d.engine().and_then(|e| e.current()).filter(|c| sent > stats.len() && c.generation() == info.id);
        if let Some(c) = cur {
            stats.push(c.stats().clone());
        }
        if at.get(sent).is_some_and(|&a| done >= a) && stats.len() == sent {
            let r = next(&info);
            info = r.info();
            player.activate(r).unwrap();
            sent += 1;
        }
    });
    assert_eq!((sent, stats.len()), (at.len(), at.len()));
    for s in &stats {
        assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.resets), (0, 0, 0, 0), "{s:?}");
        assert!(s.kept.total() > 0);
    }
    out
}

/// Three points to activate at, from 5 s on, while notes sound.
fn points(song: &Song) -> Vec<usize> {
    let end = (song.seconds() * song.rate as f64) as usize;
    [5 * SR as usize, 5 * SR as usize + 77_777, 5 * SR as usize + 151_515].into_iter().filter(|&a| a + SR as usize <= end).collect()
}

/// The fixtures G7c plays: every pure one but the silent templates/empty, and p16.
fn g7c() -> Vec<(String, Song)> {
    let mut out: Vec<(String, Song)> = ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter"]
        .into_iter()
        .map(|n| (n.to_string(), fixture(n)))
        .collect();
    out.push(("p16".into(), p16()));
    out
}

// RENDER-33, RENDER-25 (G7c): a plan of the same revision activated while playing, three times,
// mid-note, keeps every instance (schedulers, voice pools, rings, audio players, cells) and
// changes no bit of the export.
#[test]
fn a_no_op_activation_changes_no_bit() {
    for (name, song) in g7c() {
        let frames = 10 * SR as usize;
        let export = offline(&song, frames);
        let heard = activated(&song, frames, &points(&song), &|info| song.against(&song, info, &Continues::identity(&song.flat)));
        assert_eq!(first_diff(&heard, &export), None, "{name}: a no-op activation changed the output");
    }
}

// RENDER-27, RENDER-33 (G7c): an activation whose every lane comes with its data rebuilt (new data
// and renders, the same contents) swaps every lane and crossfades every audio lane, and changes no
// bit: equal sides stay equal through the crossfade.
#[test]
fn an_activation_with_rebuilt_lane_data_changes_no_bit() {
    for (name, song) in g7c() {
        let frames = 10 * SR as usize;
        let export = offline(&song, frames);
        let as_new = song.with_data(DataSet::empty());
        // Against no data: every lane compares changed, so each moves in with its data again.
        let heard = activated(&song, frames, &points(&song), &|info| song.against(&as_new, info, &Continues::identity(&song.flat)));
        assert_eq!(first_diff(&heard, &export), None, "{name}: rebuilt lane data changed the output");
    }
}

// RENDER-27, RENDER-33 (G7c): an arrangement read again (an activation of the same structure with
// equal tables and renders, from a new read of the project) changes no bit.
#[test]
fn an_identity_arrangement_activation_changes_no_bit() {
    let song = fixture("examples/resample");
    let frames = 10 * SR as usize;
    let export = offline(&song, frames);
    let decls = m2_session::compile(&Project::open(&song.dir).unwrap(), &mut std_packages()).unwrap().decls.clone();
    let reread = song.with_data(Project::open(&song.dir).unwrap().data(&decls, None).0);
    let heard = activated(&song, frames, &points(&song), &|info| reread.against(&song, info, &Continues::identity(&song.flat)));
    assert_eq!(first_diff(&heard, &export), None, "an arrangement read again changed the output");
}

// RENDER-33 (G7c) on all of p50 (4:02), recorded: `cargo test -p m2-play --release --test
// bit_identity -- --ignored`.
#[test]
#[ignore = "about five minutes in release"]
fn live_is_the_export_on_p50() {
    let song = reference(&m2_session::reference::Reference::P50);
    let frames = (song.seconds() * SR as f64) as usize;
    check("p50", &song, frames, None);
}

// RENDER-33, RENDER-25 (G7c) on p50, recorded: a plan of the same revision activated three times
// mid-note keeps every one of its instances and changes no bit of the export.
#[test]
#[ignore = "about half a minute in release"]
fn a_no_op_activation_on_p50() {
    let song = reference(&m2_session::reference::Reference::P50);
    let frames = 10 * SR as usize;
    let export = offline(&song, frames);
    let heard = activated(&song, frames, &points(&song), &|info| song.against(&song, info, &Continues::identity(&song.flat)));
    assert_eq!(first_diff(&heard, &export), None, "p50: a no-op activation changed the output");
}

/// A track `t` playing a 440 Hz sine through `through` (`x` → what plays), then down by 1e-30 and
/// 1e-10 (about 1e-40: subnormal) and back up by 1e30 and 1e10: near the sine again where
/// subnormals are kept, silence where they are flushed to zero. Its probe, keyed `fz`, loads a state
/// tagged `sets-fz` (`m2_plugins::probe`).
fn subnormal_chain(what: &str, through: &str) -> Song {
    let track = format!(
        "import {{ kernel, plugin, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\n\n\
         const chain = (x: any) => [1e-30, 1e-10, 1e30, 1e10].reduce((y, by) => kernel(\"std:scale\", {{ in: y, by }}), x);\n\
         const through = (x: any) => {through};\n\n\
         export default track(\"t\", () => chain(through(osc.sine(440))));\n"
    );
    project(
        what,
        &[
            ("song.ts", "import { song } from \"@m2/core\";\nimport t from \"./tracks/t\";\n\nexport default song(() => t.out);\n"),
            ("tracks/t.ts", &track),
            ("data/t/fz.state", "sets-fz"),
            ("data/t/fz.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#),
        ],
    )
}

// RENDER-2a: a plugin that leaves its thread flushing subnormals to zero changes no bit of what
// runs after it on that thread: the engine sets the one mode again after every plugin call. The
// probe passes the sine through exactly (gain 1), so the song is the chain's without it.
#[test]
fn a_flush_to_zero_left_by_a_plugin_changes_no_bit() {
    let with = subnormal_chain("fz-probe", "plugin(\"clap:m2.probe\", { key: \"fz\", in: x, gain_1: 1 })");
    let without = subnormal_chain("fz-none", "x");
    let frames = SR as usize / 10;
    let (a, b) = (offline(&with, frames), offline(&without, frames));
    assert!(peak(&b) > 0.5, "the chain gives the sine back: {}", peak(&b));
    assert!(m2_plugins::probe::tagged("sets-fz").iter().any(|s| s.process.load(std::sync::atomic::Ordering::Relaxed) > 0), "the probe ran");
    assert_eq!(first_diff(&a, &b), None, "the same bits after a plugin that sets flush-to-zero");
}

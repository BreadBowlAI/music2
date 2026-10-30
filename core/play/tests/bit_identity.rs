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
use m2_runtime::{BLOCK, Cmd, Continues, Copying, LaneSwap, PlanInfo, Pool, Renderer, Runner, Stats, TapAt, notes_data};
use m2_song::{DataSet, Project};
use serde_json::{Value, json};

/// The fixtures made only of kernels, which render the same bits every time (RENDER-2).
const PURE: [&str; 7] = [
    "examples/first-song",
    "examples/resample",
    "examples/sounds",
    "examples/sampler",
    "templates/starter",
    "templates/empty",
    "core/session/tests/fixtures/sampler-regions",
];

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
    for name in PURE {
        let song = fixture(name);
        let frames = (song.seconds() * song.rate as f64) as usize;
        check(name, &song, frames, None);
    }
}

// AGENT-7: copy taps change no bit of what renders. Every pure fixture renders the same bits with a
// copy tap on the output, on every node with an audio output and on every overlay site, from its
// first frame; and the output's copy is what was rendered, block by block.
#[test]
fn copy_taps_change_no_bit() {
    for name in PURE {
        let song = fixture(name);
        let frames = ((song.seconds().min(20.0) * song.rate as f64) as usize).div_ceil(BLOCK) * BLOCK;
        let export = offline(&song, frames);
        let mut at = vec![TapAt::Out];
        at.extend((0..song.flat.overlays.len()).map(TapAt::Site));
        let audio = |i: usize| matches!(song.flat.nodes[i].outputs.first(), Some(m2_ir::Kind::Audio(_)));
        at.extend((0..song.flat.nodes.len()).filter(|&i| audio(i)).map(TapAt::Node));
        let mut r = Copying::new(song.renderer(), &at, 0).unwrap();
        let (mut tapped, mut copied) = (vec![[0.0f32; 2]; frames], vec![]);
        for chunk in tapped.chunks_mut(4096) {
            r.process_on(chunk, &mut Runner::one()).unwrap();
            let c = r.copies();
            assert_eq!(c.taps(), at.len(), "{name}: every tap");
            for k in 0..c.blocks().len() {
                assert_eq!(c.blocks()[k], (copied.len() / BLOCK * BLOCK) as u64, "{name}: block {k} in order");
                for t in 0..c.taps() {
                    assert_eq!(c.block(t, k).len(), c.channels(t) * BLOCK, "{name}: tap {t} copied whole");
                }
                let out = c.block(0, k);
                copied.extend((0..BLOCK).map(|f| [out[f], out[BLOCK + f]]));
            }
            c.clear();
        }
        assert!(peak(&export) > 0.0 || name.contains("empty"), "{name} makes sound");
        assert_eq!(first_diff(&tapped, &export), None, "{name}: a copy tap changed what renders");
        assert_eq!(first_diff(&copied[..frames], &export), None, "{name}: the output's copy is what rendered");
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
    for name in PURE {
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

// PKG-16, RENDER-33: std's Sampler takes a region's turn from how many notes its lane has started
// (std:hitcount, outside the voices), never from engine time or from which voice is free. Four C4s
// a beat apart on two takes play take 1, 2, 1, 2 in the export; and live after the engine sat
// stopped they play the same, bit for bit.
#[test]
fn round_robin_takes_turns_by_the_notes_live_as_in_the_export() {
    let keys = "import { track } from \"@m2/core\";\nimport { sampler } from \"std/sampler\";\n\
                export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => sampler(lane.notes(\"notes\"), { regions: [\
                { file: \"soft-1.wav\", round_robin: [1, 2] }, { file: \"soft-2.wav\", round_robin: [2, 2] }] }));\n";
    let notes = "{\"clips\": [{\"id\": \"rr4c4s00\", \"start\": 0, \"length\": 4, \"notes\": [\
                 {\"pitch\": \"C4\", \"start\": 0, \"length\": 0.5, \"velocity\": 0.5}, {\"pitch\": \"C4\", \"start\": 1, \"length\": 0.5, \"velocity\": 0.5},\
                 {\"pitch\": \"C4\", \"start\": 2, \"length\": 0.5, \"velocity\": 0.5}, {\"pitch\": \"C4\", \"start\": 3, \"length\": 0.5, \"velocity\": 0.5}]}]}";
    let song = fixture_with("core/session/tests/fixtures/sampler-regions", &[("tracks/keys.ts", keys), ("data/keys/notes.json", notes)]);
    let frames = (song.seconds() * song.rate as f64) as usize;
    let export = offline(&song, frames);
    // A beat is 0.5 s at 120 bpm; each take is 0.12 s, so each note's sound is its own.
    let beat = song.rate as usize / 2;
    let note = |k: usize| &export[k * beat..k * beat + beat / 2];
    assert!(peak(note(0)) > 0.05 && peak(note(1)) > 0.05, "the notes sound");
    assert!(note(0) == note(2) && note(1) == note(3), "every second note plays the same take");
    assert!(note(0) != note(1), "and the two takes take turns");
    for idle in [2 * SR as usize, 5 * BLOCK] {
        assert_eq!(first_diff(&live_after_idle(&song, idle, frames), &export), None, "played after {idle} frames stopped");
    }
}

// PKG-16.4, RENDER-16: a release region plays when its note ends, wherever on the block grid. A
// poly frees a voice whose note's Off lands on a block line before the voice runs that block, when
// its last block was silent and its kernels say they aren't active; std:regions says it is active
// while a note a release region matches is held. Here the attack file (0.12 s) ends long before
// each note, so each voice is silent when its note ends: at 120 bpm the second note ends on frame
// 96 000 (750 blocks), the first mid-block; at 125 bpm a beat is 23 040 frames (180 blocks), so
// every note that ends on a beat ends on a block line.
#[test]
fn a_release_region_sounds_when_its_note_ends_on_a_block_line() {
    let keys = "import { track } from \"@m2/core\";\nimport { sampler } from \"std/sampler\";\n\
                export default track(\"keys\", { name: \"Keys\" }, ({ lane }) => sampler(lane.notes(\"notes\"), { regions: [\
                { file: \"soft-1.wav\", keys: [48, 72] }, { file: \"release.wav\", keys: [48, 72], trigger: \"release\" }] }));\n";
    let notes = "{\"clips\": [{\"id\": \"relgrid1\", \"start\": 0, \"length\": 6, \"notes\": [\
                 {\"pitch\": \"C4\", \"start\": 0, \"length\": 1, \"velocity\": 0.5}, {\"pitch\": \"C4\", \"start\": 2, \"length\": 2, \"velocity\": 0.5}]}]}";
    for (bpm, ends) in [(120, [24_000, 96_000]), (125, [23_040, 92_160])] {
        let tempo = format!("{{\"tempo\": [{{\"beat\": 0, \"bpm\": {bpm}}}], \"meter\": []}}");
        let files = [("tracks/keys.ts", keys), ("data/keys/notes.json", notes), ("data/arrangement.json", tempo.as_str())];
        let song = fixture_with("core/session/tests/fixtures/sampler-regions", &files);
        let frames = (song.seconds() * song.rate as f64) as usize;
        let export = offline(&song, frames);
        for end in ends {
            let line = end % BLOCK == 0;
            assert!(peak(&export[end - 4_800..end]) < 1e-4, "{bpm} bpm: the voice is silent before its note ends at {end}");
            assert!(peak(&export[end..end + 1_920]) > 0.05, "{bpm} bpm: the release sounds at {end} (on a block line: {line})");
        }
        assert_eq!(first_diff(&live(&song, frames, None), &export), None, "{bpm} bpm: live is the export");
    }
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
    let mut out: Vec<(String, Song)> =
        ["examples/first-song", "examples/resample", "examples/sounds", "examples/sampler", "templates/starter"]
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

/// One pool at a time in this test binary (8a): each spins its workers.
static POOLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `song` offline for `frames`, on `runner`.
fn offline_on(song: &Song, frames: usize, runner: &mut Runner) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    song.renderer().process_on(&mut out, runner).unwrap();
    out
}

/// What each thread count renders of `song` for `frames` against one thread: the first frame that
/// differs, the tasks run on workers, and the reads that crossed threads. In spread mode: every
/// worker woken at each block, the caller kept off the first tasks.
fn on_threads(song: &Song, frames: usize, counts: &[usize], one: &[[f32; 2]]) -> Vec<(usize, Option<usize>, u64, u64)> {
    let _one_at_a_time = POOLS.lock().unwrap_or_else(|e| e.into_inner());
    counts
        .iter()
        .map(|&n| {
            let (_pool, mut runner) = Pool::new(n, m2_runtime::threads::setup(false)).unwrap();
            runner.spread(true);
            let out = offline_on(song, frames, &mut runner);
            let s = runner.spread_report();
            (n, first_diff(&out, one), s.tasks_on_workers, s.edges_across)
        })
        .collect()
}

// RENDER-2 (8a): every pure fixture, its whole song and a second past it, offline, renders the same
// bits on 2, 3, 4 and 7 threads as on one. Each render ran tasks on worker threads; over all of
// them, some read a producer that ran on another thread.
#[test]
fn every_pure_fixture_renders_the_same_bits_on_1_and_n_threads() {
    let t = std::time::Instant::now();
    let mut across = 0;
    for name in PURE {
        let song = fixture(name);
        let frames = (song.seconds() * song.rate as f64) as usize;
        let one = offline(&song, frames);
        for (n, diff, on_workers, crossed) in on_threads(&song, frames, &[2, 3, 4, 7], &one) {
            assert_eq!(diff, None, "{name} on {n} threads: the same bits");
            assert!(on_workers > 0, "{name} on {n} threads: tasks ran on workers");
            across += crossed;
        }
    }
    assert!(across > 0, "some reads crossed threads");
    eprintln!("every pure fixture on 1, 2, 3, 4 and 7 threads: {:.1} s", t.elapsed().as_secs_f64());
}

// RENDER-2 (track ab): std's sampler reads its files through the kernels' asset import, in the
// voices of two tracks: one file for each note on the drums, one 44.1 kHz file pitched to every note
// on the keys. Its example renders the same bits on 2, 3, 4 and 7 threads as on one, and each of its
// tracks sounds on its own.
#[test]
fn the_sampler_example_gives_the_same_bits_on_1_to_7_threads() {
    let song = fixture("examples/sampler");
    let frames = (song.seconds() * song.rate as f64) as usize;
    let one = offline(&song, frames);
    assert!(peak(&one) > 0.1, "the sampler example sounds");
    for (n, diff, on_workers, _) in on_threads(&song, frames, &[2, 3, 4, 7], &one) {
        assert_eq!(diff, None, "the sampler example on {n} threads: the same bits");
        assert!(on_workers > 0, "the sampler example on {n} threads: tasks ran on workers");
    }
    for track in ["drums", "keys"] {
        let alone = format!(
            "import {{ song }} from \"@m2/core\";\nimport {{ master, mixer }} from \"std/mix\";\nimport t from \"./tracks/{track}\";\n\
             export default song(() => master(mixer({{ t: t.out }})));\n"
        );
        let song = fixture_with("examples/sampler", &[("song.ts", &alone)]);
        assert!(peak(&offline(&song, frames)) > 0.05, "the {track} sound on their own");
    }
}

// RENDER-2, PACK-5 (packs): a song that plays a pack from its folder, its keys on std's synth and
// its sampler on the pack's own recording, renders the same bits on 4 threads as on one, and
// sounds.
#[test]
fn a_song_using_a_pack_gives_the_same_bits_on_1_and_4_threads() {
    let root = temp("pack");
    copy(&repo().join("core/session/tests/fixtures/packs"), &root);
    let song = compile(root.join("song-path"));
    let frames = (song.seconds() * song.rate as f64) as usize;
    let one = offline(&song, frames);
    assert!(peak(&one) > 0.05, "the pack's song sounds");
    for (n, diff, on_workers, _) in on_threads(&song, frames, &[4], &one) {
        assert_eq!(diff, None, "the pack's song on {n} threads: the same bits");
        assert!(on_workers > 0, "the pack's song on {n} threads: tasks ran on workers");
    }
    let _ = std::fs::remove_dir_all(&root);
}

// RENDER-2 (track sx): examples/resample's warped clips are stretched by std's `std:stretch`, made
// before the render starts, so the example renders the same bits on 2, 3, 4 and 7 threads as on
// one; and its chops track alone sounds in both warped clips (3 to 5 s, and from 6 s).
#[test]
fn the_resample_example_gives_the_same_bits_on_1_to_7_threads() {
    let song = fixture("examples/resample");
    let frames = (song.seconds() * song.rate as f64) as usize;
    let one = offline(&song, frames);
    for (n, diff, on_workers, _) in on_threads(&song, frames, &[2, 3, 4, 7], &one) {
        assert_eq!(diff, None, "the resample example on {n} threads: the same bits");
        assert!(on_workers > 0, "the resample example on {n} threads: tasks ran on workers");
    }
    let alone = "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport chops from \"./tracks/chops\";\n\
                 export default song(() => master(mixer({ chops: chops.out })));\n";
    let chops = offline(&fixture_with("examples/resample", &[("song.ts", alone)]), frames);
    let at = |s: f64| (s * song.rate as f64) as usize;
    assert!(peak(&chops[at(3.0)..at(5.0)]) > 0.05 && peak(&chops[at(6.0)..at(8.0)]) > 0.05, "the warped clips sound");
}

// RENDER-2 (8a): 30 s of p16 render the same bits on 4 threads as on one, with tasks on workers
// and reads across threads.
#[test]
fn p16_renders_the_same_bits_on_1_and_4_threads() {
    let song = p16();
    let frames = 30 * song.rate as usize;
    let one = offline(&song, frames);
    let [(_, diff, on_workers, crossed)] = on_threads(&song, frames, &[4], &one)[..] else { unreachable!() };
    assert_eq!(diff, None, "the same bits on 4 threads");
    assert!(on_workers > 0 && crossed > 0, "{on_workers} tasks on workers, {crossed} reads across threads");
}

// RENDER-2 (8a): the same for 30 s of p50 on 8 threads. Long in a debug build: run by hand
// (`--ignored`), and recorded in the build notes.
#[test]
#[ignore]
fn p50_renders_the_same_bits_on_1_and_8_threads() {
    let song = reference(&m2_session::reference::Reference::P50);
    let frames = 30 * song.rate as usize;
    let one = offline(&song, frames);
    let [(_, diff, on_workers, crossed)] = on_threads(&song, frames, &[8], &one)[..] else { unreachable!() };
    assert_eq!(diff, None, "the same bits on 8 threads");
    assert!(on_workers > 0 && crossed > 0, "{on_workers} tasks on workers, {crossed} reads across threads");
}

// RENDER-2 (8a, R1): p16's song partition holds only the sum (its adds) and the master's stereo,
// gain and limiter: each channel's fader runs with its track, and constants run nowhere.
#[test]
fn the_song_tail_is_the_sum_and_the_master() {
    let song = p16();
    let f = &song.flat;
    let r = song.renderer();
    let kernel = |i: usize| match f.nodes.get(i).map(|n| &n.op) {
        Some(m2_ir::FOp::Kernel { kernel, .. }) => kernel.as_str(),
        _ => "not a kernel",
    };
    let parts = r.parts();
    let tail: Vec<usize> = parts.iter().filter(|p| p.label == "song").flat_map(|p| p.nodes.iter().copied()).collect();
    let mut kinds: Vec<&str> = tail.iter().map(|&i| kernel(i)).collect();
    kinds.sort_unstable();
    let tracks = parts.iter().filter(|p| p.label != "song").count();
    assert_eq!(tracks, 16, "a partition a track");
    let mut want = vec!["std:add"; tracks - 1];
    want.extend(["std:gain", "std:limit", "std:stereo"]);
    assert_eq!(kinds, want, "the song's tail");
    // The song's own faders: one a channel, and the master's. Only the master's is in the song.
    let label = |i: usize| parts.iter().find(|p| p.nodes.contains(&i)).map_or("none", |p| p.label.as_str());
    let faders: Vec<usize> = (0..f.nodes.len()).filter(|&i| f.nodes[i].module == "song" && kernel(i) == "std:gain").collect();
    assert_eq!(faders.len(), tracks + 1);
    assert_eq!(faders.iter().filter(|&&i| label(i) == "song").count(), 1, "each channel's fader runs with its track");
}

/// A live player's engine threads for tests: `n` threads set up as a player's (plain threads, as
/// no device plays here), in spread mode: every worker woken each block and the device's thread
/// kept off the roots, so tasks surely run on workers.
fn engine_threads(n: usize) -> (Pool, Runner) {
    let group = Arc::new(m2_os::rt::Group::none(Some(std::time::Duration::from_micros(2667))));
    let (pool, mut runner) = Pool::new(n, m2_runtime::threads::live(group, false, None).0).unwrap();
    runner.spread(true);
    (pool, runner)
}

/// What a player driven on its engine threads heard, and what its workers did: tasks they ran and
/// reads across threads, summed over the run; and its activation's stats, if it had one.
struct Heard {
    out: Vec<[f32; 2]>,
    on_workers: u64,
    across: u64,
    stats: Option<Stats>,
}

/// A plan to activate at a frame, made against the one that plays.
type At<'a> = (usize, &'a dyn Fn(&PlanInfo) -> Renderer);

/// `song` live on `runner` at `rate`, from 0 for `frames`, with `at`'s plan activated when given.
fn live_on(song: &Song, rate: u32, frames: usize, runner: Runner, at: Option<At>) -> Heard {
    let r = song.renderer();
    let info = r.info();
    let (player, mut d) = Player::driven_on(rate, runner);
    player.open(r).unwrap();
    player.play();
    let mut sent = None;
    let out = hear(&mut d, frames, &mut |_, done| {
        if let Some((frame, next)) = &at
            && done >= *frame
            && sent.is_none()
        {
            let r = next(&info);
            sent = Some(r.generation());
            player.activate(r).unwrap();
        }
    });
    let stats = sent.map(|id| {
        let cur = d.engine().and_then(|e| e.current()).filter(|c| c.generation() == id).expect("the activation plays");
        cur.stats().clone()
    });
    let s = d.runner().spread_report();
    Heard { out, on_workers: s.tasks_on_workers, across: s.edges_across, stats }
}

// RENDER-33, RENDER-2 (8a's live half): live playback on the player's engine threads is the
// export on one thread, bit for bit: every pure fixture, its whole song and a second, on 2, 4 and
// 7 threads, in device buffers of mixed sizes. Each fixture of two partitions or more ran tasks on
// workers; over all of them, some reads crossed threads.
#[test]
fn live_on_n_threads_is_the_export_on_1() {
    let _one_at_a_time = POOLS.lock().unwrap_or_else(|e| e.into_inner());
    let t = std::time::Instant::now();
    let mut across = 0;
    for name in PURE {
        let song = fixture(name);
        let frames = (song.seconds() * song.rate as f64) as usize;
        let export = offline(&song, frames);
        let parts = song.renderer().parts().len();
        for n in [2, 4, 7] {
            let (_pool, runner) = engine_threads(n);
            let heard = live_on(&song, song.rate, frames, runner, None);
            assert_eq!(first_diff(&heard.out, &export), None, "{name} live on {n} threads: the export's bits");
            assert!(parts < 2 || heard.on_workers > 0, "{name} on {n} threads: tasks ran on workers");
            across += heard.across;
        }
    }
    assert!(across > 0, "some reads crossed threads");
    eprintln!("every pure fixture live on 2, 4 and 7 threads: {:.1} s", t.elapsed().as_secs_f64());
}

// RENDER-25, RENDER-2 (8a's live half): activations on the engine threads give one thread's bits:
// G7c's three no-op activations on first-song and p16 (the export's bits), and an edit with a ghost
// and fades on first-song (4 threads against 1, sample for sample, through the window).
#[test]
fn edits_on_engine_threads_match_one_thread() {
    let _one_at_a_time = POOLS.lock().unwrap_or_else(|e| e.into_inner());
    for (name, song) in [("first-song".to_string(), fixture("examples/first-song")), ("p16".into(), p16())] {
        let frames = 10 * SR as usize;
        let export = offline(&song, frames);
        let (_pool, runner) = engine_threads(4);
        let (player, mut d) = Player::driven_on(song.rate, runner);
        let r = song.renderer();
        let mut info = r.info();
        player.open(r).unwrap();
        player.play();
        let at = points(&song);
        let mut sent = 0;
        let heard = hear(&mut d, frames, &mut |_, done| {
            if at.get(sent).is_some_and(|&a| done >= a) {
                let r = song.against(&song, &info, &Continues::identity(&song.flat));
                info = r.info();
                player.activate(r).unwrap();
                sent += 1;
            }
        });
        assert_eq!(sent, at.len());
        assert_eq!(first_diff(&heard, &export), None, "{name}: no-op activations on 4 threads changed the output");
        assert!(d.runner().spread_report().tasks_on_workers > 0, "{name}: tasks ran on workers");
    }
    let (song, edited) = (fixture("examples/first-song"), first_song_edited());
    let cont = continues(&song, &edited);
    let next = |info: &PlanInfo| edited.against(&song, info, &cont);
    let frames = 8 * SR as usize;
    let one = live_on(&song, song.rate, frames, Runner::one(), Some((3 * SR as usize, &next)));
    let (_pool, runner) = engine_threads(4);
    let four = live_on(&song, song.rate, frames, runner, Some((3 * SR as usize, &next)));
    let s = four.stats.as_ref().expect("an activation");
    assert!(s.ghosts >= 1 && s.fades >= 2, "a ghost, and fades: {s:?}");
    assert_eq!(one.stats.as_ref().map(|o| o.kept.total()), Some(s.kept.total()));
    assert!(peak(&four.out[3 * SR as usize..]) > 0.05, "the edit plays");
    assert_eq!(first_diff(&four.out, &one.out), None, "the edit on 4 threads: one thread's bits");
    assert!(four.on_workers > 0 && four.across > 0, "tasks on workers ({}), reads across threads ({})", four.on_workers, four.across);
}

// RENDER-31, RENDER-2 (8a's live half): a song at 48 kHz on a 44.1 kHz device (the resampler pulls
// engine frames in pieces, so some callbacks render two blocks) plays the same on 4 threads as on 1.
#[test]
fn a_resampled_song_on_n_threads_matches_one_thread() {
    let _one_at_a_time = POOLS.lock().unwrap_or_else(|e| e.into_inner());
    let song = fixture("examples/first-song");
    let frames = 6 * 44_100;
    let one = live_on(&song, 44_100, frames, Runner::one(), None);
    let (_pool, runner) = engine_threads(4);
    let four = live_on(&song, 44_100, frames, runner, None);
    assert!(peak(&one.out) > 0.1);
    assert_eq!(first_diff(&four.out, &one.out), None, "resampled on 4 threads: one thread's bits");
    assert!(four.on_workers > 0);
}

/// The host process, as `m2-plugin-host` (built beside the tests).
fn remote() -> Arc<dyn m2_plugins::PluginHost> {
    Arc::new(m2_plugins::Remote::for_tests())
}

// HOST-3, RENDER-2a: an offline render through the host process is the in-process render, bit for
// bit: a probe instrument on notes, its gain from a lane and its level a literal, beside a probe
// tagged `sets-fz` in the subnormal chain, which leaves its thread flushing after every call (the
// host process puts the one mode back, as the engine does). Red with that reset removed there.
// With Surge XT and Airwindows in CLAP_PATH, `examples/with-plugins` too, each second of the song
// within 1 dB (G7e's rule: Surge XT renders other bits each run, in either host).
#[test]
fn renders_through_the_host_process_are_the_same_bits() {
    let track = "import { kernel, plugin, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { sum } from \"std/math\";\n\n\
                 const down = (x: any) => [1e-30, 1e-10].reduce((y, by) => kernel(\"std:scale\", { in: y, by }), x);\n\
                 const up = (x: any) => [1e30, 1e10].reduce((y, by) => kernel(\"std:scale\", { in: y, by }), x);\n\n\
                 export default track(\"t\", ({ lane }) => sum(\n  \
                 up(plugin(\"clap:m2.probe\", { key: \"fz\", in: down(osc.sine(440)), gain_1: 1 })),\n  \
                 plugin(\"clap:m2.probe\", { key: \"a\", notes: lane.notes(\"n\"), gain_1: lane.control(\"g\", \"scalar\", 0.5), level_2: 0.2 }),\n));\n";
    let notes = json!({"clips": [{"id": "a", "start": 0, "length": 8, "notes": (0..16).map(|k| json!({"pitch": 57 + k % 5, "start": k as f64 * 0.25, "length": 0.5})).collect::<Vec<_>>()}]}).to_string();
    let gain = r#"{"unit": "scalar", "clips": [{"id": "r", "start": 0, "length": 8, "points": [[0, 0.2], [2, 0.9], [4, 0.4]]}]}"#;
    let files = [
        ("song.ts", "import { song } from \"@m2/core\";\nimport t from \"./tracks/t\";\n\nexport default song(() => t.out);\n"),
        ("tracks/t.ts", track),
        ("data/t/fz.state", "sets-fz"),
        ("data/t/fz.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#),
        ("data/t/n.json", notes.as_str()),
        ("data/t/g.json", gain),
    ];
    let frames = SR as usize;
    let here = offline(&project("hp-here", &files), frames);
    let there = offline(&project_on("hp-there", &files, Some(remote())), frames);
    assert!(peak(&here) > 0.5, "the notes and the chain play: {}", peak(&here));
    assert_eq!(first_diff(&here, &there), None, "the host process renders the same bits");

    let clap = std::env::var_os("CLAP_PATH").map(std::path::PathBuf::from);
    if !clap.is_some_and(|c| c.join("Surge XT.clap").exists() && c.join("Airwindows Consolidated.clap").exists()) {
        eprintln!(
            "renders_through_the_host_process_are_the_same_bits: no Surge XT and Airwindows in CLAP_PATH, so with-plugins wasn't rendered"
        );
        return;
    }
    let render = |host: Option<Arc<dyn m2_plugins::PluginHost>>| {
        let dir = temp("hp-with-plugins");
        copy(&repo().join("examples/with-plugins"), &dir);
        let song = compile_on(dir, host);
        offline(&song, (song.seconds() * SR as f64) as usize)
    };
    let (a, b) = (render(None), render(Some(remote())));
    assert_eq!(a.len(), b.len());
    let level =
        |x: &[[f32; 2]]| (x.iter().map(|f| (f[0] as f64).powi(2) + (f[1] as f64).powi(2)).sum::<f64>() / x.len() as f64).sqrt() + 1e-9;
    // Each whole second of the song; the second past its end is a tail, Surge XT's own noise.
    let song = a.len() / SR as usize - 1;
    for (s, (x, y)) in a.chunks(SR as usize).zip(b.chunks(SR as usize)).take(song).enumerate() {
        let db = 20.0 * (level(x) / level(y)).log10();
        assert!(db.abs() < 1.0, "with-plugins, second {s}: {db:.2} dB apart");
    }
}

// RENDER-18: a song with no latency gets no delay, so its plan, and its bits, are what they were:
// every fixture, p16, and `examples/with-plugins` when Surge XT and Airwindows are in CLAP_PATH
// (neither has latency). Red with a 1-frame latency forced on every kernel.
#[test]
fn no_fixture_gets_a_delay() {
    let mut songs: Vec<(String, Song)> =
        ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter", "templates/empty"]
            .map(|n| (n.to_string(), fixture(n)))
            .into();
    songs.push(("p16".into(), p16()));
    let clap = std::env::var_os("CLAP_PATH").map(std::path::PathBuf::from);
    match clap.is_some_and(|c| c.join("Surge XT.clap").exists() && c.join("Airwindows Consolidated.clap").exists()) {
        true => songs.push(("examples/with-plugins".into(), fixture("examples/with-plugins"))),
        false => eprintln!("no_fixture_gets_a_delay: no Surge XT and Airwindows in CLAP_PATH, so with-plugins wasn't planned"),
    }
    for (name, song) in &songs {
        let r = song.renderer();
        let info = r.info();
        assert_eq!((r.latency(), r.stats().delays, info.played.is_some()), (0, 0, false), "{name} has no latency, so no delay");
    }
}

/// The session's bus fixture: first-song's tracks into a drum group, a reverb and a delay that
/// sends on to the reverb (RENDER-34).
const BUSES: &str = "core/session/tests/fixtures/buses";

// RENDER-2, RENDER-33, RENDER-34: the bus fixture, a song and buses that read each other both ways,
// renders the same bits on 2, 3, 4 and 7 threads as on one, with tasks on workers, and plays live
// what it exports.
#[test]
fn buses_give_the_same_bits_on_any_thread_count() {
    let song = fixture(BUSES);
    assert!(song.flat.routes.iter().any(|(t, b)| t == "keys" && b == "echo"), "the keys feed the delay: {:?}", song.flat.routes);
    let frames = (song.seconds() * song.rate as f64) as usize;
    let one = offline(&song, frames);
    assert!(peak(&one) > 0.1);
    for (n, diff, on_workers, _) in on_threads(&song, frames, &[2, 3, 4, 7], &one) {
        assert_eq!(diff, None, "the bus fixture on {n} threads: the same bits");
        assert!(on_workers > 0, "the bus fixture on {n} threads: tasks ran on workers");
    }
    check("buses", &song, song.rate as usize * 4, None);
}

// RENDER-17 to RENDER-19, RENDER-34: a latent plugin on a bus stays in time with the dry path. The
// fixture's keys go to its delay bus (`to`), which plays them through the probe 100 frames late,
// beside the keys inverted: the planner delays the dry path by 100 frames, so the two cancel to
// exactly 0, offline and live.
#[test]
fn a_latent_plugin_on_a_bus_stays_aligned() {
    let late = "import { bus, plugin } from \"@m2/core\";\n\
                export default bus(\"echo\", ({ input }) => plugin(\"clap:m2.probe\", { key: \"late\", in: input, gain_1: 1 }));\n";
    let song = |mix: &str| {
        let code = format!(
            "import {{ kernel, song }} from \"@m2/core\";\nimport {{ mixer }} from \"std/mix\";\n\
             import echo from \"./tracks/echo\";\nimport keys from \"./tracks/keys\";\n\
             export default song(() => mixer({mix}));\n"
        );
        fixture_with(
            BUSES,
            &[
                ("song.ts", &code),
                ("tracks/echo.ts", late),
                ("data/echo/late.state", "latency-100"),
                ("data/echo/late.state.json", r#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#),
            ],
        )
    };
    let cancel = song("{ keys: { in: keys.out, to: \"echo\" }, inv: kernel(\"std:scale\", { in: keys.out, by: -1 }), echo: echo.out }");
    let wet = song("{ keys: { in: keys.out, to: \"echo\" }, echo: echo.out }");
    let seconds = cancel.seconds();
    let mut r = cancel.renderer();
    assert_eq!(r.latency(), 100, "the probe's latency is the song's");
    let out = r.render(seconds).unwrap();
    assert!(peak(&wet.renderer().render(seconds).unwrap()) > 0.1, "the bus plays the keys");
    assert_eq!(peak(&out), 0.0, "offline, the bus and the dry path inverted cancel exactly");
    let heard = live(&cancel, cancel.rate as usize * 4, None);
    assert_eq!(peak(&heard), 0.0, "live, the same");
}

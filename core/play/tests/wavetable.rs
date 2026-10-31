//! PACK-27, RENDER-2, RENDER-33: a song that plays `@m2/wavetable` (two tracks: a pad on two
//! built-in tables with five unison voices and the LFO, a lead that plays a WAV of its own as its
//! table, one note at a time with glide) renders the same bits on 2, 3, 4 and 7 threads as on one,
//! and plays live the bits it exports.

mod common;

use common::*;
use m2_runtime::{Pool, Runner};

fn wavetable_song() -> Song {
    let dir = temp("wavetable");
    let files = [
        (
            "music2.toml",
            "[project]\nname = \"Wavetable\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\
             \"@m2/wavetable\" = \"1\"\n",
        ),
        (
            "song.ts",
            "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport pad from \"./tracks/pad\";\n\
             import lead from \"./tracks/lead\";\n\nexport default song(() => master(mixer({ pad: pad.out, lead: lead.out })));\n",
        ),
        (
            "tracks/pad.ts",
            "import { track } from \"@m2/core\";\nimport { wavetable } from \"@m2/wavetable\";\n\n\
             export default track(\"pad\", { name: \"Pad\" }, ({ lane }) =>\n  wavetable(lane.notes(\"notes\"), { tableA: \"vowels\", \
             tableB: \"harmonics\", levelB: 0.5, unison: 5, detune: 0.2, positionA: 0.4, lfoPosition: 0.3, lfoRate: 0.5, filterEnv: 2 }),\n);\n",
        ),
        (
            "tracks/lead.ts",
            "import { track } from \"@m2/core\";\nimport { wavetable } from \"@m2/wavetable\";\n\n\
             export default track(\"lead\", { name: \"Lead\" }, ({ lane }) =>\n  wavetable(lane.notes(\"notes\"), { fileA: \"table.wav\", \
             positionA: 0.6, voices: 1, glide: 0.08, cutoff: 900, filterEnv: 3, filter: \"bandpass\" }),\n);\n",
        ),
        (
            "data/pad/notes.json",
            r#"{"clips": [{"id": "pad00001", "start": 0, "length": 8, "notes": [
  {"pitch": "C3", "start": 0, "length": 4, "velocity": 0.7}, {"pitch": "E3", "start": 0, "length": 4, "velocity": 0.7},
  {"pitch": "G3", "start": 0.01, "length": 4, "velocity": 0.6}, {"pitch": "A2", "start": 4, "length": 4, "velocity": 0.8},
  {"pitch": "C4", "start": 4.25, "length": 3.5, "velocity": 0.5}, {"pitch": "E5", "start": 5, "length": 2, "velocity": 0.9}
]}]}"#,
        ),
        (
            "data/lead/notes.json",
            r#"{"clips": [{"id": "lead0001", "start": 0, "length": 8, "notes": [
  {"pitch": "G4", "start": 0.5, "length": 1, "velocity": 0.8}, {"pitch": "C6", "start": 1.25, "length": 0.75, "velocity": 0.9},
  {"pitch": "A5", "start": 2, "length": 1.5, "velocity": 0.7}, {"pitch": "C8", "start": 3.25, "length": 0.5, "velocity": 1}
]}]}"#,
        ),
    ];
    for (rel, text) in files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), text).unwrap();
    }
    // 8 frames: each a cycle of a saw with one more harmonic than the one before.
    let table: Vec<f32> = (0..8 * 2048)
        .map(|i| {
            let (frame, t) = (i / 2048, (i % 2048) as f64 / 2048.0);
            (1..=frame + 1).map(|h| (std::f64::consts::TAU * h as f64 * t).sin() / h as f64).sum::<f64>() as f32 * 0.6
        })
        .collect();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    m2_runtime::audio::write_wav(&dir.join("assets/table.wav"), &[table], SR).unwrap();
    compile(dir)
}

fn offline_on(song: &Song, frames: usize, runner: Option<&mut Runner>) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    match runner {
        Some(runner) => song.renderer().process_on(&mut out, runner).unwrap(),
        None => song.renderer().process(&mut out).unwrap(),
    }
    out
}

#[test]
fn a_wavetable_song_gives_the_same_bits_on_1_to_7_threads() {
    let song = wavetable_song();
    let frames = (song.seconds() * song.rate as f64) as usize;
    let one = offline_on(&song, frames, None);
    assert!(peak(&one) > 0.05, "the wavetable song sounds: {}", peak(&one));
    for n in [2, 3, 4, 7] {
        let (_pool, mut runner) = Pool::new(n, m2_runtime::threads::setup(false)).unwrap();
        runner.spread(true);
        let out = offline_on(&song, frames, Some(&mut runner));
        assert_eq!(first_diff(&out, &one), None, "the wavetable song on {n} threads: the same bits");
        assert!(runner.spread_report().tasks_on_workers > 0, "on {n} threads: tasks ran on workers");
    }
    let (_player, mut d) = playing(song.renderer(), song.rate);
    let heard = hear(&mut d, frames, &mut |_, _| {});
    assert_eq!(first_diff(&heard, &one), None, "the wavetable song live: the bits it exports");
}

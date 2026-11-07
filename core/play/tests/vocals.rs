//! VOX-20, VOX-24, RENDER-2, RENDER-18, RENDER-33: songs that play `@m2/vocals`' Tune. One with two
//! voices tuned to its key, Robot and Natural, renders the same bits on 2, 3, 4 and 7 threads as on
//! one, and plays live the bits it exports. Tune is 15 ms late, and the rest of the song is late with
//! it: a voice through Tune at Amount 0 against the same voice dry, upside down, is silence.

mod common;

use common::*;
use m2_runtime::{Pool, Runner};

/// A song asking for `@m2/vocals`, at 48 kHz, its key C major, with `tracks` (key, code).
fn vocal_song(what: &str, tracks: &[(&str, &str)]) -> Song {
    let dir = temp(what);
    let names: Vec<&str> = tracks.iter().map(|t| t.0).collect();
    let imports: String = names.iter().map(|n| format!("import {n} from \"./tracks/{n}\";\n")).collect();
    let mix: Vec<String> = names.iter().map(|n| format!("{n}: {n}.out")).collect();
    let song = format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ mixer }} from \"std/mix\";\n{imports}\nexport default song(() => mixer({{ {} }}));\n",
        mix.join(", ")
    );
    let mut files = vec![
        (
            "music2.toml".to_string(),
            "[project]\nname = \"Vocals\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.10\"\n\"@m2/vocals\" = \"1\"\n"
                .to_string(),
        ),
        ("song.ts".to_string(), song),
        (
            "data/arrangement.json".to_string(),
            "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": [{\"beat\": 0, \"beats\": 4, \"unit\": 4}], \"key\": {\"root\": \"C\", \"scale\": \"major\"}}\n"
                .to_string(),
        ),
    ];
    for (name, body) in tracks {
        files.push((
            format!("tracks/{name}.ts"),
            format!(
                "import {{ kernel, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ tune }} from \"@m2/vocals\";\n\n\
                 export default track(\"{name}\", {{ name: \"{name}\" }}, () => {body});\n"
            ),
        ));
    }
    for (rel, text) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), text).unwrap();
    }
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
fn a_tuned_song_gives_the_same_bits_on_1_to_7_threads() {
    // A buzzy voice a little flat of A, at Robot; a brighter one sharp of E, at Natural.
    let song = vocal_song(
        "tuned",
        &[
            ("robot", "tune(kernel(\"std:svf\", { in: osc.saw(216), cutoff: 1500 }).port(\"lp\"), { glide: 0 })"),
            ("natural", "tune(osc.square(337), { glide: 0.1, amount: 0.8 })"),
        ],
    );
    assert_eq!(song.renderer().latency(), 720, "Tune is 15 ms late, and so is the song");
    let frames = 3 * song.rate as usize;
    let one = offline_on(&song, frames, None);
    assert!(peak(&one) > 0.05, "the song sounds: {}", peak(&one));
    for n in [2, 3, 4, 7] {
        let (_pool, mut runner) = Pool::new(n, m2_runtime::threads::setup(false)).unwrap();
        runner.spread(true);
        let out = offline_on(&song, frames, Some(&mut runner));
        assert_eq!(first_diff(&out, &one), None, "on {n} threads: the same bits");
        assert!(runner.spread_report().tasks_on_workers > 0, "on {n} threads: tasks ran on workers");
    }
    let (_player, mut d) = playing(song.renderer(), song.rate);
    let heard = hear(&mut d, frames + 720, &mut |_, _| {});
    assert_eq!(first_diff(&heard[720..], &one), None, "live, 15 ms late: the bits it exports");
}

// RENDER-18, VOX-24: the song is as late as Tune, so a voice through Tune at Amount 0 (shift then
// gives it back as it came, 15 ms late) and the same voice dry, upside down, cancel to silence.
#[test]
fn the_rest_of_a_tuned_song_waits_for_tune() {
    let voice = "kernel(\"std:svf\", { in: osc.saw(203), cutoff: 1200 }).port(\"lp\")";
    let song = vocal_song(
        "aligned",
        &[("tuned", &format!("tune({voice}, {{ amount: 0 }})")), ("dry", &format!("kernel(\"std:scale\", {{ in: {voice}, by: -1 }})"))],
    );
    let out = offline_on(&song, 2 * song.rate as usize, None);
    let alone = vocal_song("alone", &[("dry", &format!("kernel(\"std:scale\", {{ in: {voice}, by: -1 }})"))]);
    let dry = offline_on(&alone, 2 * alone.rate as usize, None);
    assert!(peak(&dry) > 0.1, "the voice sounds: {}", peak(&dry));
    assert!(peak(&out) < 1e-4, "the two cancel: what is left peaks at {}", peak(&out));
}

/// `clock − pos` of `Player::heard_frame` after each of `n` device buffers of `size` frames.
fn heard_offsets(song: &Song, size: usize, n: usize) -> Vec<i64> {
    let (player, mut d) = playing(song.renderer(), song.rate);
    let mut data = vec![0.0f32; 2 * size];
    let mut out = vec![];
    for _ in 0..n {
        d.fill(&mut data, 2);
        drop(d.drain_retired());
        let (clock, pos) = player.heard_frame().unwrap();
        out.push(clock as i64 - pos as i64);
    }
    out
}

// RENDER-19, VOX-25 (the review's aside): `Player::heard_frame` pairs the engine clock and the
// transport's frame at one moment, so while the song plays on, the one less the other never moves:
// with buffers of 100 frames the engine renders blocks of 128 ahead, and what is left of a block
// must count the same on both sides. A song with Tune (15 ms late) and one without.
#[test]
fn heard_frame_pairs_one_moment_late_or_not() {
    let tuned = vocal_song("heard", &[("vox", "tune(osc.saw(216), { glide: 0 })")]);
    let dry = vocal_song("heard-dry", &[("vox", "osc.saw(216)")]);
    for (name, song) in [("dry", &dry), ("tuned", &tuned)] {
        let offsets = heard_offsets(song, 100, 60);
        let settled = &offsets[20..];
        assert!(settled.iter().all(|o| *o == settled[0]), "{name}: clock less frame moves while it plays: {settled:?}");
    }
}

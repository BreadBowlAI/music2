//! VOX-20, VOX-22, VOX-24, RENDER-2, RENDER-18, RENDER-33: songs that play `@m2/vocals`' Tune. One
//! with two voices tuned to its key, Robot and Natural, renders the bits `@m2/vocals` 1.1.0 rendered,
//! the same on 2, 3, 4 and 7 threads as on one, and plays live the bits it exports. Tune is 15 ms late, and the rest of the song is late with
//! it: a voice through Tune at Amount 0 against the same voice dry, upside down, is silence.

mod common;

use common::*;
use m2_runtime::{Pool, Runner};

/// The imports of each track of [`vocal_song`].
const TRACK_IMPORTS: &str =
    "import { kernel, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { tune } from \"@m2/vocals\";\n";

/// A song asking for `@m2/vocals`, at 48 kHz, its key C major, with `tracks` (key, code).
fn vocal_song(what: &str, tracks: &[(&str, &str)]) -> Song {
    vocal_song_with(what, TRACK_IMPORTS, tracks)
}

/// [`vocal_song`], each track importing `track_imports`.
fn vocal_song_with(what: &str, track_imports: &str, tracks: &[(&str, &str)]) -> Song {
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
            format!("{track_imports}\nexport default track(\"{name}\", {{ name: \"{name}\" }}, () => {body});\n"),
        ));
    }
    for (rel, text) in &files {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), text).unwrap();
    }
    compile(dir)
}

/// The SHA-256 of a render's samples, as little-endian `f32` bits.
fn bits(frames: &[[f32; 2]]) -> String {
    let bytes: Vec<u8> = frames.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    m2_song::Sha::of(&bytes).hex()
}

/// What the tuned song below renders, its first 3 s on one thread, as `@m2/vocals` 1.1.0's module
/// played it (main at 6658f1da, before snap gave its meters): Tune's sound, pinned (VOX-22, track
/// meters2's review T4). A song with Tune can't be a pure fixture of `renders.json`: after the
/// engine sat stopped it reads at other moments of the song (RENDER-33).
const TUNED: &str = "eede586e411217d9195c1a6b2c2ad59d673279eec610bcf3946cc6ac7c0d9b21";

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
    // VOX-22: its sound is what it was before snap gave its meters. A change to Tune's kernels that
    // changes it moves @m2/vocals' version (PACK-17) and this hash with it.
    assert_eq!(bits(&one), TUNED, "Tune renders other bits than @m2/vocals 1.1.0 did");
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

// ---------------------------------------------------------------- a voice's colour (VOX-20, VOX-28)

/// The imports of a track that sings [`vowel`] through Tune or the Voice changer.
const VOWEL_IMPORTS: &str = "import { track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { bandpass } from \"std/filter\";\n\
                             import { scale, sum } from \"std/math\";\nimport { tune, voiceChanger } from \"@m2/vocals\";\n";

/// A buzzy vowel at `hz`: a saw through three resonances near an "ah"'s formants (700, 1220 and
/// 2600 Hz), the hills of its colour, which a formant shift moves and a pitch shift keeps.
fn vowel(hz: f64) -> String {
    format!("sum(bandpass(osc.saw({hz}), 700, 4), bandpass(osc.saw({hz}), 1220, 5), scale(bandpass(osc.saw({hz}), 2600, 6), 0.5))")
}

/// The power of `x`'s first 2^k samples under a Hann window, bin by bin, with each bin's frequency.
fn spectrum(x: &[f32], sr: f64) -> Vec<(f64, f64)> {
    use std::f64::consts::TAU;
    let n = 1usize << (usize::BITS - 1 - x.len().leading_zeros());
    let mut re: Vec<f64> = (0..n).map(|i| x[i] as f64 * (0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())).collect();
    let mut im = vec![0.0; n];
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((-TAU * k as f64 / len as f64).cos(), (-TAU * k as f64 / len as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                (re[b], im[b]) = (re[a] - xr, im[a] - xi);
                re[a] += xr;
                im[a] += xi;
            }
        }
        len <<= 1;
    }
    (0..n / 2).map(|k| (k as f64 * sr / n as f64, re[k] * re[k] + im[k] * im[k])).collect()
}

/// The strongest frequency of `x` from `lo` to `hi` Hz, between bins by a parabola through the log
/// powers: its note.
fn note_of(x: &[f32], lo: f64, hi: f64) -> f64 {
    let s = spectrum(x, 48_000.0);
    let k = (1..s.len() - 1).filter(|&k| (lo..=hi).contains(&s[k].0)).max_by(|&a, &b| s[a].1.total_cmp(&s[b].1)).unwrap();
    let (a, b, c) = (s[k - 1].1.ln(), s[k].1.ln(), s[k + 1].1.ln());
    (k as f64 + 0.5 * (a - c) / (a - 2.0 * b + c)) * s[1].0
}

/// A voice's colour: the centre of its spectrum's power from 300 Hz to 8 kHz, where its vowel's
/// hills are, moved up by half again (Chipmunk's) too.
fn colour_of(x: &[f32]) -> f64 {
    let band: Vec<(f64, f64)> = spectrum(x, 48_000.0).into_iter().filter(|s| (300.0..=8_000.0).contains(&s.0)).collect();
    band.iter().map(|s| s.0 * s.1).sum::<f64>() / band.iter().map(|s| s.1).sum::<f64>()
}

fn cents(a: f64, b: f64) -> f64 {
    1200.0 * (a / b).log2()
}

/// A song singing `call` (a track's code) on one track: its left channel from 1 s to about 2.4 s,
/// and its latency.
fn sung(what: &str, call: &str) -> (Vec<f32>, u32) {
    let song = vocal_song_with(what, VOWEL_IMPORTS, &[("vox", call)]);
    let out = offline_on(&song, 48_000 + (1 << 16), None);
    (out[48_000..].iter().map(|f| f[0]).collect(), song.renderer().latency())
}

// VOX-20: Tune's Formant moves the voice's colour and not its notes: a buzzy vowel a third of a
// semitone flat of A, tuned at Robot, comes out on A at Formant 0, 4 and -4, its colour moved up
// and down by about 2^(4/12).
#[test]
fn tunes_formant_moves_the_colour_and_keeps_the_notes() {
    let heard = |formant: i32| sung(&format!("formant{formant}"), &format!("tune({}, {{ glide: 0, formant: {formant} }})", vowel(216.0))).0;
    let at0 = heard(0);
    let (note, plain) = (note_of(&at0, 200.0, 240.0), colour_of(&at0));
    assert!(cents(note, 220.0).abs() < 5.0, "Tune puts it on A: {note:.2} Hz");
    for formant in [4, -4] {
        let x = heard(formant);
        let (n, c) = (note_of(&x, 200.0, 240.0), colour_of(&x));
        let want = 2f64.powf(formant as f64 / 12.0);
        println!("Tune at Formant {formant}: note {n:.2} Hz, colour {c:.0} Hz ({:.3} of {plain:.0} Hz, {want:.3} asked)", c / plain);
        assert!(cents(n, note).abs() < 5.0, "Formant {formant} keeps the note: {n:.2} Hz, {note:.2} at 0");
        assert!((c / plain / want - 1.0).abs() < 0.1, "Formant {formant} moves the colour by {:.3}, not about {want:.3}", c / plain);
    }
}

/// The values of the Voice changer's preset `name`, as the pack ships it: Pitch and Formant.
fn preset(name: &str) -> (f64, f64) {
    let path = repo().join(format!("packs/@m2/vocals/presets/voice-changer/{name}.json"));
    let p: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(p["sound"], "@m2/vocals:voice-changer", "{name}");
    (p["values"]["pitch"].as_f64().unwrap(), p["values"]["formant"].as_f64().unwrap())
}

// VOX-28: the Voice changer moves the notes by Pitch and keeps the colour, moves the colour by
// Formant and keeps the notes; Deep and Chipmunk move both, by their values; and the song is 15 ms
// late, as with Tune.
#[test]
fn the_voice_changer_moves_the_notes_and_the_colour_on_their_own() {
    let heard = |pitch: f64, formant: f64| {
        let (x, late) = sung(
            &format!("changed{pitch}-{formant}"),
            &format!("voiceChanger({}, {{ pitch: {pitch}, formant: {formant} }})", vowel(220.0)),
        );
        assert_eq!(late, 720, "15 ms late at 48 kHz, as Tune is");
        let want = 220.0 * 2f64.powf(pitch / 12.0);
        (note_of(&x, want / 1.06, want * 1.06), colour_of(&x))
    };
    let (note, plain) = heard(0.0, 0.0);
    assert!(cents(note, 220.0).abs() < 5.0, "at 0 and 0 the voice as it came: {note:.2} Hz");
    let (deep, chipmunk) = (preset("Deep"), preset("Chipmunk"));
    assert!(deep.0 < 0.0 && deep.1 < 0.0 && chipmunk.0 > 0.0 && chipmunk.1 > 0.0, "Deep moves both down, Chipmunk both up");
    for (pitch, formant, what) in [
        (5.0, 0.0, "up 5"),
        (-5.0, 0.0, "down 5"),
        (0.0, 4.0, "colour up 4"),
        (deep.0, deep.1, "Deep"),
        (chipmunk.0, chipmunk.1, "Chipmunk"),
    ] {
        let (n, c) = heard(pitch, formant);
        let (want_note, want_colour) = (220.0 * 2f64.powf(pitch / 12.0), 2f64.powf(formant / 12.0));
        println!("{what}: note {n:.2} Hz ({want_note:.2} asked), colour {:.3} of as sung ({want_colour:.3} asked)", c / plain);
        assert!(cents(n, want_note).abs() < 5.0, "{what}: the note at {n:.2} Hz, not {want_note:.2}");
        let tolerance = if formant == 0.0 { 0.05 } else { 0.1 };
        assert!(
            (c / plain / want_colour - 1.0).abs() < tolerance,
            "{what}: the colour moved by {:.3}, not about {want_colour:.3}",
            c / plain
        );
    }
}

/// What the changed song below renders, its first 3 s on one thread: the Voice changer's sound,
/// pinned, as Tune's is (`TUNED`). It can't be a pure fixture: its kernels are Tune's (RENDER-33).
const CHANGED: &str = "d72f22d276bda00bbede55a1028856e05e9e5eb3b44cd9bf16c5ec7114d92d11";

// VOX-28, RENDER-2: a song with two voices through the Voice changer, one at Deep's values and one
// at Chipmunk's, renders the bits recorded for it, the same on 2, 3, 4 and 7 threads as on one,
// and plays live the bits it exports, 15 ms late.
#[test]
fn a_changed_voice_gives_the_same_bits_on_1_to_7_threads() {
    let song = vocal_song_with(
        "changed",
        VOWEL_IMPORTS,
        &[
            ("deep", &format!("voiceChanger({}, {{ pitch: -5, formant: -5 }})", vowel(220.0))),
            ("high", "voiceChanger(osc.square(330), { pitch: 7, formant: 7 })"),
        ],
    );
    assert_eq!(song.renderer().latency(), 720, "15 ms late, and so is the song");
    let frames = 3 * song.rate as usize;
    let one = offline_on(&song, frames, None);
    assert!(peak(&one) > 0.05, "the song sounds: {}", peak(&one));
    assert_eq!(bits(&one), CHANGED, "the Voice changer renders other bits than were recorded for it");
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

// ---------------------------------------------------------------- the Vocoder (VOX-29)

/// The imports of a track that plays the Vocoder.
const VOCODER_IMPORTS: &str = "import { track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { vocoder } from \"@m2/vocals\";\n";

// VOX-29, PKG-19: a Vocoder with no voice, as + Effect adds it, plays its input bit for bit, whatever
// its knobs, and with a voice written as `undefined` too: it hears nothing.
#[test]
fn a_vocoder_with_no_voice_plays_its_input_bit_for_bit() {
    let frames = 48_000;
    let dry = offline_on(&vocal_song_with("dry", VOCODER_IMPORTS, &[("synth", "osc.saw(110)")]), frames, None);
    assert!(peak(&dry) > 0.1, "the saw plays: {}", peak(&dry));
    for call in [
        "vocoder(osc.saw(110))",
        "vocoder(osc.saw(110), { clarity: 0.5, smooth: 0.05, formant: 0 })",
        "vocoder(osc.saw(110), { clarity: 1, smooth: 0.2, formant: 7 })",
        "vocoder(osc.saw(110), undefined, { formant: -12 })",
    ] {
        let out = offline_on(&vocal_song_with("vocoded", VOCODER_IMPORTS, &[("synth", call)]), frames, None);
        assert_eq!(first_diff(&out, &dry), None, "{call} changes its input");
    }
}

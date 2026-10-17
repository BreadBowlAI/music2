//! Mute through the player (MON-1, MON-5, RENDER-22a, gate 8): toggling a monitoring mute while a
//! sine plays makes no click, at any device buffer size; and a monitoring mute live is mute in song
//! offline.

mod common;

use std::sync::Arc;

use common::*;
use m2_runtime::{CellId, CellTarget};

/// A song of one `tone` track, a sine at `hz` at full scale, mixed by std's mixer.
fn tone(hz: f64) -> Song {
    project(
        "tone",
        &[(
            "song.ts",
            &format!(
                "import {{ song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ mixer }} from \"std/mix\";\n\
                 const tone = track(\"tone\", () => osc.sine({hz}));\nexport default song(() => mixer({{ tone: tone.out }}));\n"
            ),
        )],
    )
}

/// `frames` frames through `d` in device buffers of `size`, calling `between` before each with the
/// frames done so far.
fn fill_by(d: &mut m2_play::Driver, size: usize, frames: usize, between: &mut dyn FnMut(usize)) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(frames);
    let mut data = vec![0.0f32; 2 * size];
    while out.len() < frames {
        let n = size.min(frames - out.len());
        between(out.len());
        d.fill(&mut data[..2 * n], 2);
        out.extend(data[..2 * n].chunks(2).map(|f| [f[0], f[1]]));
        drop(d.drain_retired());
    }
    out
}

// Gate 8, RENDER-22a: toggling mute while a sine plays makes no sample-to-sample step larger than
// twice the sine's own largest (2π f / rate), at 440 Hz (the gate) and 50 Hz (a bass note, where a
// one-block ramp would fail), with device buffers of 128, 37 and 512 frames, so the toggles land
// mid-block. Each toggle silences the sine, or brings it back whole.
#[test]
fn toggling_mute_makes_no_step_larger_than_twice_the_sines() {
    for hz in [440.0, 50.0] {
        let song = tone(hz);
        assert_eq!(song.flat.overlays.len(), 1, "the tone into the mixer");
        for size in [128, 37, 512] {
            let r = song.renderer();
            let cells: Arc<[CellId]> = r.info().overlays.iter().map(|o| o.2).collect();
            let target = CellTarget { slots: r.cell_slots(), cells };
            let (player, mut d) = playing(r, SR);
            // A toggle every 0.1 s plus 61 frames, from 0.25 s on.
            let every = SR as usize / 10 + 61;
            let (mut on, mut next) = (true, SR as usize / 4);
            let out = fill_by(&mut d, size, SR as usize * 2, &mut |done| {
                if done >= next {
                    on = !on;
                    next += every;
                    player.set_cells(&target, if on { 1.0 } else { 0.0 });
                }
            });
            let own = std::f32::consts::TAU * hz as f32 / SR as f32;
            let step = max_step(&out);
            assert!(step <= 2.0 * own, "{hz} Hz, buffers of {size}: a step of {step}, the sine's own is {own}");
            // The toggles did something: silence, and the whole sine again.
            let windows: Vec<f32> = out.chunks(every / 8).map(peak).collect();
            assert!(windows.contains(&0.0), "{hz} Hz, {size}: muted somewhere");
            assert!(windows.iter().skip(1).any(|p| *p > 0.99), "{hz} Hz, {size}: heard whole");
        }
    }
}

/// Kick and bass tracks, mixed; the kick muted in the song with `muted`.
fn kick_and_bass(muted: bool) -> Song {
    let opts = if muted { "{ muted: true }, " } else { "" };
    project(
        "kick-bass",
        &[(
            "song.ts",
            &format!(
                "import {{ song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ mixer }} from \"std/mix\";\n\
                 const kick = track(\"kick\", {opts}() => osc.sine(55));\nconst bass = track(\"bass\", () => osc.saw(110));\n\
                 export default song(() => mixer({{ kick: kick.out, bass: bass.out }}));\n"
            ),
        )],
    )
}

// MON-4, MON-5, A9: the kick muted while listening, live, gives the samples of the kick muted in
// the song, offline (compared as numbers: a zero's sign may differ).
#[test]
fn a_monitoring_mute_live_equals_mute_in_song_offline() {
    let (heard, saved) = (kick_and_bass(false), kick_and_bass(true));
    let frames = SR as usize;
    let mut r = heard.renderer();
    let values: Vec<f32> = r.info().overlays.iter().map(|o| if o.0 == "kick" { 0.0 } else { 1.0 }).collect();
    assert!(values.contains(&0.0) && values.contains(&1.0), "{:?}", r.info().overlays);
    r.set_overlays(&values);
    let (_player, mut d) = playing(r, SR);
    let live = hear(&mut d, frames, &mut |_, _| {});
    let mut offline = vec![[0.0; 2]; frames];
    saved.renderer().process(&mut offline).unwrap();
    assert!(peak(&offline) > 0.1, "the bass plays");
    assert!(live.iter().zip(&offline).all(|(a, b)| a[0] == b[0] && a[1] == b[1]), "live with the kick muted is the song with it muted");
    let mut unmuted = vec![[0.0; 2]; frames];
    heard.renderer().process(&mut unmuted).unwrap();
    assert!(unmuted.iter().zip(&offline).any(|(a, b)| a[0] != b[0]), "the control: the kick is in the song unmuted");
}

/// The sidechain fixture `name` (`core/session/tests/fixtures/sidechain/`), its `song.ts` with each
/// `(from, to)` replaced.
fn sidechain(name: &str, edits: &[(&str, &str)]) -> Song {
    let rel = format!("core/session/tests/fixtures/sidechain/{name}");
    let mut song = std::fs::read_to_string(repo().join(&rel).join("song.ts")).unwrap();
    for (from, to) in edits {
        assert!(song.contains(from), "{name}: no `{from}`");
        song = song.replace(from, to);
    }
    fixture_with(&rel, &[("song.ts", &song)])
}

fn offline_render(song: &Song, frames: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    song.renderer().process(&mut out).unwrap();
    out
}

/// Equal as numbers, sample by sample (A9: a zero's sign may differ).
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x[0] == y[0] && x[1] == y[1])
}

const MUTED: (&str, &str) = ("track(\"kick\", { name: \"Kick\" }", "track(\"kick\", { name: \"Kick\", muted: true }");
const OUT_OF_THE_MIX: (&str, &str) = ("mixer({ kick: kick.out, bass:", "mixer({ bass:");

/// Each fixture, and the sidechain its duck reads.
const SIDECHAINS: [(&str, &str); 2] = [("env", "kick.env"), ("follow", "follow(kick.out, { release: ms(150) })")];

// Gate 8, MON-2: muting the kick in the song keeps the bass ducked, whether the duck reads the
// kick's envelope (`kick.env`) or `follow(kick.out)` written in the song: the song is the one with
// the kick taken out of the mix and its sidechain kept, sample for sample.
#[test]
fn muting_the_kick_keeps_the_bass_ducked() {
    let frames = 3 * SR as usize;
    for (name, _) in SIDECHAINS {
        let muted = offline_render(&sidechain(name, &[MUTED]), frames);
        let out = offline_render(&sidechain(name, &[OUT_OF_THE_MIX]), frames);
        assert!(peak(&muted) > 0.1, "{name}: the bass plays");
        assert!(same(&muted, &out), "{name}: the kick muted is the kick out of the mix, its sidechain kept");
    }
}

// The gate's negative control: with the sidechain gone too, the bass changes, so the gate above
// would see a mute that reached the sidechain.
#[test]
fn without_the_sidechain_the_bass_changes() {
    let frames = 3 * SR as usize;
    for (name, side) in SIDECHAINS {
        let muted = offline_render(&sidechain(name, &[MUTED]), frames);
        let undocked = offline_render(&sidechain(name, &[MUTED, (side, "0")]), frames);
        assert!(!same(&muted, &undocked), "{name}: the duck follows the kick");
    }
}

// MON-4, MON-5 on the gate's fixtures: the kick muted while listening, live, is the kick muted in
// the song, offline.
#[test]
fn a_monitoring_mute_of_the_kick_live_is_its_mute_in_song() {
    let frames = 3 * SR as usize;
    for (name, _) in SIDECHAINS {
        let heard = sidechain(name, &[]);
        let mut r = heard.renderer();
        let values: Vec<f32> = r.info().overlays.iter().map(|o| if o.0 == "kick" { 0.0 } else { 1.0 }).collect();
        r.set_overlays(&values);
        let (_player, mut d) = playing(r, SR);
        let live = hear(&mut d, frames, &mut |_, _| {});
        assert!(same(&live, &offline_render(&sidechain(name, &[MUTED]), frames)), "{name}");
    }
}

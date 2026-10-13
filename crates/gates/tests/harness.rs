//! The gate harness checked against what it should find: every probe song plays as written, a 5 ms
//! crossfade meets the step bound, HEAD's replay engine fails the gates step 3 exists for, and,
//! with the `engine` feature, step 3a's live engine passes every gate.

use std::path::PathBuf;

use m2_gates::gates::{self, GATES, Opts};
use m2_gates::report::{Outcome, Verdict};
use m2_gates::rig::Kind;
use m2_gates::signal;
use m2_gates::songs::{self, Fx, Pad};
use m2_gates::{Lab, Song};
use m2_runtime::Renderer;

struct Temp(PathBuf);

impl Temp {
    fn new(tag: &str) -> Temp {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Temp(std::env::temp_dir().join(format!("m2-gates-test-{tag}-{}-{n}", std::process::id())))
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn offline(lab: &Lab, song: &Song, frames: usize) -> Vec<[f32; 2]> {
    let mut r = Renderer::new(&song.flat, &song.data, &lab.packages, song.rate).unwrap();
    let mut out = vec![[0.0; 2]; frames];
    r.process(&mut out).unwrap();
    out
}

fn quick() -> Opts {
    Opts { loop_minutes: 0.3, seeks: 2, activations: 8, ..Opts::quick() }
}

fn failed(o: &Outcome, check: &str) -> bool {
    o.checks.iter().any(|c| c.verdict == Verdict::Fail && c.name.contains(check))
}

#[test]
fn every_probe_song_compiles_and_sounds() {
    let t = Temp::new("songs");
    let mut lab = Lab::new(&t.0).unwrap();
    // Each with a beat by which it has sounded: the tail songs' note is at beat 132.
    let all = [
        (songs::tail(Fx::LongReverb), 133.0),
        (songs::tail(Fx::LongDelay), 133.0),
        (songs::tail(Fx::Reverb), 133.0),
        (songs::tail(Fx::Delay), 133.0),
        (songs::tail(Fx::VoiceDelay), 133.0),
        (songs::clicks(), 2.0),
        (songs::sine(false), 2.0),
        (songs::sine(true), 2.0),
        (songs::poly_sine(false), 2.0),
        (songs::poly_sine(true), 2.0),
        (songs::with_tempo(songs::poly_sine(false), 90.0), 2.0),
        (songs::pad(Pad::One), 8.0),
        (songs::pad(Pad::Twice), 8.0),
        (songs::pad(Pad::Layered), 8.0),
        (songs::pad(Pad::Anonymous), 8.0),
    ];
    for (files, by) in all {
        let song = lab.project("song", &files).and_then(|d| lab.compile(&d)).unwrap();
        let out = offline(&lab, &song, song.frame(by) as usize);
        assert!(signal::db(signal::peak(&out)) > -40.0, "{} is silent", song.dir.display());
    }
}

// RENDER-25a: a 5 ms linear crossfade between a sine and the same sine through a delay keeps every
// step within twice the sine's own, wherever it lands: the bound G6 holds an engine to is reachable.
#[test]
fn a_five_ms_crossfade_meets_the_step_bound() {
    let t = Temp::new("fade");
    let mut lab = Lab::new(&t.0).unwrap();
    let a = lab.project("a", &songs::sine(false)).and_then(|d| lab.compile(&d)).unwrap();
    let b = lab.project("b", &songs::sine(true)).and_then(|d| lab.compile(&d)).unwrap();
    let (a, b) = (offline(&lab, &a, 96_000), offline(&lab, &b, 96_000));
    let own = signal::max_step(&a).0;
    let fade = 240;
    let mut rng = signal::Rng::new(7);
    let mut worst_cut = 0.0f64;
    for _ in 0..20 {
        let at = (rng.range(0.1, 1.9) * 48_000.0) as usize / 128 * 128;
        let mixed: Vec<[f32; 2]> = (0..a.len())
            .map(|i| {
                let g = ((i as f32 - at as f32 + 1.0) / fade as f32).clamp(0.0, 1.0);
                [a[i][0] * (1.0 - g) + b[i][0] * g, a[i][1] * (1.0 - g) + b[i][1] * g]
            })
            .collect();
        let (step, frame) = signal::max_step(&mixed);
        assert!(step <= 2.0 * own, "a fade at {at}: step {step} at {frame}, bound {}", 2.0 * own);
        let cut: Vec<[f32; 2]> = (0..a.len()).map(|i| if i < at { a[i] } else { b[i] }).collect();
        worst_cut = worst_cut.max(signal::max_step(&cut).0);
    }
    // Switching in one sample is what the bound exists to catch.
    assert!(worst_cut > 2.0 * own, "a cut's worst step {worst_cut}");
}

// The negative reference: HEAD's engine replays and stops dead, so the gates for what step 3
// changes must fail on it, or they measure nothing (RENDER-6a, RENDER-9, RENDER-25a).
#[test]
fn heads_replay_engine_fails_the_gates_step_3_exists_for() {
    let t = Temp::new("replay");
    let mut lab = Lab::new(&t.0).unwrap();
    let o = quick();
    let tails = gates::tail_stop(&mut lab, Kind::Replay, &o).unwrap();
    assert!(failed(&tails, "rings on through the stop"), "{tails}");
    let step = gates::step(&mut lab, Kind::Replay, &o).unwrap();
    assert!(failed(&step, "sine ↔ delay(sine)"), "{step}");
    let looped = gates::looping(&mut lab, Kind::Replay, &o).unwrap();
    assert!(failed(&looped, "no tail cut at a wrap"), "{looped}");
    // What HEAD gets right, the harness must not call wrong: wraps on the grid, nothing stuck.
    assert!(!failed(&looped, "drift"), "{looped}");
    assert!(!failed(&looped, "negative control"), "{looped}");
    let stuck = gates::stuck(&mut lab, Kind::Replay, &o).unwrap();
    for c in stuck.checks.iter().filter(|c| c.verdict == Verdict::Fail) {
        assert!(c.name.ends_with("Same"), "{stuck}");
    }
}

// Step 3a's live engine passes every gate at CI sizes.
#[cfg(feature = "engine")]
#[test]
fn the_live_engine_passes_every_gate() {
    let t = Temp::new("engine");
    let mut lab = Lab::new(&t.0).unwrap();
    let o = Opts { loop_minutes: 1.0, ..Opts::quick() };
    for (name, _) in GATES {
        let outcome = gates::run(name, &mut lab, Kind::Engine, &o).unwrap();
        assert_ne!(outcome.verdict(), Verdict::Fail, "{outcome}");
    }
}

#[test]
fn every_gate_is_named() {
    let t = Temp::new("names");
    let mut lab = Lab::new(&t.0).unwrap();
    assert!(gates::run("nope", &mut lab, Kind::Replay, &quick()).is_err());
    assert_eq!(GATES.len(), 9);
}

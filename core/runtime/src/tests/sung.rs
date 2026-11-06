//! A voice's notes in the host (VOX-41, VOX-45, VOX-47, RENDER-30b): a measure run over a file and
//! kept, a clip's edited phrases played through `@m2/vocals:voice` and joined to its normal path,
//! live as offline, and a timing drag's markers.

use super::*;
use crate::measure::{self, Measured};

/// A made-up singer at `rate` for `secs`: a buzzy tone (a pulse train through a resonance near
/// "ah"'s first formant) at MIDI `midi(t)`, its level `level(t)`.
fn singer(rate: u32, secs: f64, midi: &dyn Fn(f64) -> f64, level: &dyn Fn(f64) -> f64) -> Vec<f32> {
    let sr = rate as f64;
    let (mut phase, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64);
    let (r, th) = ((-std::f64::consts::PI * 150.0 / sr).exp(), std::f64::consts::TAU * 700.0 / sr);
    (0..(secs * sr) as usize)
        .map(|i| {
            let t = i as f64 / sr;
            let f = 440.0 * 2f64.powf((midi(t) - 69.0) / 12.0);
            phase += f / sr;
            let x = if phase >= 1.0 {
                phase -= 1.0;
                1.0
            } else {
                0.0
            };
            let y = x + 2.0 * r * th.cos() * y1 - r * r * y2;
            (y2, y1) = (y1, y);
            (0.08 * y * level(t)) as f32
        })
        .collect()
}

/// Silent outside `a..b` seconds, with 5 ms fades.
fn sung_between(a: f64, b: f64) -> impl Fn(f64) -> f64 {
    move |t| ((t - a).min(b - t) / 0.005).clamp(0.0, 1.0)
}

// VOX-41: a measure writes ⌈frames / hop⌉ frames, each its outputs' numbers at the file's frame
// k × hop, at the file's own rate; it is kept in the cache folder by what it was given, so the
// second look reads it back, the same numbers, and a measure of another file is another; a kernel
// that isn't a measure is refused in words.
#[test]
fn a_measure_writes_its_numbers_each_hop_and_keeps_them() {
    let dir = project(&[]);
    let rate = 44_100;
    let x = singer(rate, 1.0, &|_| 57.0, &sung_between(0.2, 0.8));
    audio::write_wav(&dir.join("a.wav"), std::slice::from_ref(&x), rate).unwrap();
    audio::write_wav(&dir.join("b.wav"), &[x.iter().map(|v| v * 0.5).collect()], rate).unwrap();
    let pk = own_packages();
    let folder = dir.join("measures");
    let first = measure::measure_in(Some(&folder), &pk, "@m2/vocals:notes", &dir.join("a.wav"), &[]).unwrap();
    assert_eq!((first.hop, first.rate), (256, rate as f64));
    assert_eq!(first.outputs, ["hz", "clarity", "level", "note", "centre"]);
    assert!(first.values.iter().all(|v| v.len() == x.len().div_ceil(256)), "a frame each hop");
    let kept: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(kept.len(), 1, "kept once, no part file left: {kept:?}");
    let notes = first.output("note").unwrap();
    assert!(notes.iter().any(|&n| n == 1.0) && notes.iter().all(|&n| n == 0.0 || n == 1.0), "one note");
    let at = notes.iter().position(|&n| n == 1.0).unwrap() as f64 * 256.0 / rate as f64;
    assert!((at - 0.2).abs() < 0.02, "it starts where it was sung: {at}");
    let centre = first.output("centre").unwrap().iter().cloned().fold(0.0f32, f32::max);
    assert!((centre - 57.0).abs() < 0.1, "at its pitch: {centre}");
    // Kept: the second look reads the file back, even with the kernel's module gone.
    std::fs::write(&kept[0], std::fs::read(&kept[0]).unwrap()).unwrap();
    let again = measure::measure_in(Some(&folder), &pk, "@m2/vocals:notes", &dir.join("a.wav"), &[]).unwrap();
    assert_eq!(again, first);
    let other = measure::measure_in(Some(&folder), &pk, "@m2/vocals:notes", &dir.join("b.wav"), &[]).unwrap();
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2, "another file is another measure");
    assert_ne!(other.output("level"), first.output("level"));
    let none = measure::measure_in(None, &pk, "@m2/vocals:notes", &dir.join("a.wav"), &[]).unwrap();
    assert_eq!(none, first, "with no cache folder, made in memory: the same");
    let e = measure::measure_in(Some(&folder), &pk, "std:stretch", &dir.join("a.wav"), &[]).unwrap_err();
    assert!(format!("{e:#}").contains("isn't a measure"), "{e:#}");
    let e = measure::measure_in(Some(&folder), &pk, "@m2/vocals:pitch", &dir.join("a.wav"), &[]).unwrap_err();
    assert!(format!("{e:#}").contains("isn't a measure"), "{e:#}");
    let _: Option<&Measured> = None;
}

// VOX-41: a measure is an offline kernel with a `hop` and controls for outputs; a `hop` on a live
// kernel, audio from a measure, or controls from an offline kernel with no hop are refused as the
// package loads, in words.
#[test]
fn a_measure_is_offline_and_controls_only() {
    let k = r#"(func (export "k_new") (param i32) (result i32) (i32.const 4096))
        (func (export "k_ports") (result i32) (i32.const 2))
        (func (export "k_reset") (param i32))
        (func (export "k_active") (param i32) (result i32) (i32.const 0))
        (func (export "k_process") (param i32 i32 i32 i32))"#;
    let load = |ports: &str| {
        let dir = package_with(&module_v1(k), ports);
        let mut pk = PackageSet::default();
        let got = pk.load(&dir).map(drop).map(|()| pk.offline_decl("@test/t:k").cloned());
        std::fs::remove_dir_all(dir).ok();
        got.map_err(|e| format!("{e:#}"))
    };
    let ok = load("offline = true\nhop = 128\noutputs = [\"a: control<hz>\", \"b: control<scalar>\"]").unwrap();
    assert_eq!(ok, Some(packages::OfflineDecl { hop: 128, outputs: vec!["a".into(), "b".into()] }));
    let audio = load("offline = true\noutputs = [\"out: audio<*>\"]").unwrap();
    assert_eq!(audio.map(|d| d.hop), Some(0), "an offline kernel that makes audio");
    for (ports, says) in [
        ("offline = true\nhop = 128\noutputs = [\"out: audio<*>\"]", "its outputs are controls"),
        ("hop = 128\noutputs = [\"a: control<hz>\"]", "only a measure has"),
        ("offline = true\noutputs = [\"a: control<hz>\"]", "has one audio output, or, a measure with a `hop`, controls"),
    ] {
        let e = load(ports).unwrap_err();
        assert!(e.contains(says), "{ports}: {e}");
    }
}

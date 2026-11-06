//! A voice's notes in the host (VOX-41, VOX-45, VOX-47, RENDER-30b): a measure run over a file and
//! kept, a clip's edited phrases played through `@m2/vocals:voice` and joined to its normal path,
//! live as offline, and a timing drag's markers.

use super::*;
use crate::fault::Code;
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
    assert!(notes.contains(&1.0) && notes.iter().all(|&n| n == 0.0 || n == 1.0), "one note");
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

// ---------------------------------------------------------------- RENDER-30b

const RATE: u32 = 48_000;
/// The clip's 3 s, in beats at 120 bpm.
const BEATS: f64 = 6.0;

/// A sung file at 48 kHz, 3 s: E3 from 0.3 to 0.8 s, G3 from 1.0 to 1.5, A3 from 1.8 to 2.4, with
/// a little vibrato; silence between.
fn sung_file(dir: &Path) {
    let midi = |t: f64| {
        (if t < 0.9 {
            52.0
        } else if t < 1.65 {
            55.0
        } else {
            57.0
        }) + 0.3 * (std::f64::consts::TAU * 5.5 * t).sin()
    };
    let level = |t: f64| sung_between(0.3, 0.8)(t) + sung_between(1.0, 1.5)(t) + sung_between(1.8, 2.4)(t);
    audio::write_wav(&dir.join("a.wav"), &[singer(RATE, 3.0, &midi, &level)], RATE).unwrap();
}

fn audio_clip(v: Value) -> m2_song::AudioClip {
    serde_json::from_value(v).unwrap()
}

/// The clip over the whole file, with these warp markers, `render` and sung notes.
fn the_clip(warp: Value, sung: Option<Value>) -> m2_song::AudioClip {
    let mut v = json!({"id": "c1", "start": 0, "length": BEATS, "asset": "a.wav", "warp": warp});
    if let Some(n) = sung {
        v["render"] = json!("@m2/vocals:voice");
        v["sung"] = n;
    }
    audio_clip(v)
}

/// Its three notes: the second moved by `second`, the first straightened by `first`.
fn notes(first: f64, second: f64) -> Value {
    json!([
        {"at": 0.3, "length": 0.5, "pitch": 52, "move": 0, "straighten": first},
        {"at": 1.0, "length": 0.5, "pitch": 55, "move": second, "straighten": 0},
        {"at": 1.8, "length": 0.6, "pitch": 57, "move": 0, "straighten": 0}
    ])
}

/// What `lane` plays over its frames `0..len`, its chunks as its ring holds them: none worked out
/// here, so a resident lane plays what its build and refreshes put there.
fn copied(lane: &audio::AudioLane, len: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    for at in (0..len).step_by(BLOCK) {
        let n = BLOCK.min(len - at);
        lane.fill(&mut out[at..at + n], n, 0, at as u64, n);
    }
    out
}

/// What `lane` plays over `0..len` as an offline render does: chunks worked out, then copied.
fn played(lane: &audio::AudioLane, len: usize) -> Vec<f32> {
    lane.ensure(0, len as u64, &[]);
    let mut out = vec![0.0f32; len];
    for at in (0..len).step_by(BLOCK) {
        let n = BLOCK.min(len - at);
        lane.ensure(at as u64, n as u64, &[]);
        lane.fill(&mut out[at..at + n], n, 0, at as u64, n);
    }
    out
}

/// The frames of `a` and `b` whose bits differ.
fn differ(a: &[f32], b: &[f32]) -> Vec<usize> {
    a.iter().zip(b).enumerate().filter(|(_, (x, y))| x.to_bits() != y.to_bits()).map(|(i, _)| i).collect()
}

fn secs(f: usize) -> f64 {
    f as f64 / RATE as f64
}

// RENDER-30b: a clip with edits plays the bits of the clip with no `render` everywhere but its
// edited phrases (an edited note, 50 ms each side): an unedited note's included, plain or warped.
// Its one phrase is the second note's; it is heard there, moved.
#[test]
fn an_unedited_phrase_plays_the_bits_of_the_clip_with_no_render() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    for warp in [json!([]), json!([[0, 0], [3, BEATS]])] {
        let lane = |c: m2_song::AudioClip| {
            let mut cache = audio::Cache::with_renders_in(Some(dir.join("renders")));
            let l = audio::AudioLane::build_in(&[c], &|a| Some(dir.join(a)), &mut cache, &tempo, RATE as f64, 1, makers, None).unwrap();
            played(&l, len)
        };
        let plain = lane(the_clip(warp.clone(), None));
        let edited = lane(the_clip(warp.clone(), Some(notes(0.0, 1.0))));
        let unedited = lane(the_clip(warp.clone(), Some(notes(0.0, 0.0))));
        assert!(differ(&plain, &unedited).is_empty(), "{warp}: no edit, the same bits");
        let d = differ(&plain, &edited);
        let (first, last) = (d[0], d[d.len() - 1]);
        assert!(secs(first) >= 0.95 - 1e-9 && secs(last) < 1.55, "{warp}: only the phrase changed: {}..{} s", secs(first), secs(last));
        assert!(d.len() > RATE as usize / 4, "{warp}: the edited note is heard moved: {} frames", d.len());
        assert!(plain.iter().any(|v| v.abs() > 0.01), "it plays");
    }
}

/// A legato file (the review's S1): one voice from 0.3 to 2.4 s, E3, G3 from 1.0, A3 from 1.5, no
/// silence between the notes.
fn legato_file(dir: &Path) {
    let midi = |t: f64| {
        (if t < 1.0 {
            52.0
        } else if t < 1.5 {
            55.0
        } else {
            57.0
        }) + 0.3 * (std::f64::consts::TAU * 5.5 * t).sin()
    };
    audio::write_wav(&dir.join("a.wav"), &[singer(RATE, 3.0, &midi, &sung_between(0.3, 2.4))], RATE).unwrap();
}

fn legato_notes(second: f64) -> Value {
    json!([
        {"at": 0.3, "length": 0.7, "pitch": 52, "move": 0, "straighten": 0},
        {"at": 1.0, "length": 0.5, "pitch": 55, "move": second, "straighten": 0},
        {"at": 1.5, "length": 0.9, "pitch": 57, "move": 0, "straighten": 0}
    ])
}

// RENDER-30b, VOX-45 (the review's S1): a note edited in a line sung legato, its neighbours
// touching it, joins the clip's normal path with no dip: over each of the phrase's 10 ms
// crossfades the lane plays the clip as it was within a thousandth of the voice's peak, for a move
// up, a small one and one down; and its bits are the clip's everywhere but its phrase.
#[test]
fn an_edited_note_sung_legato_joins_without_a_dip() {
    let dir = project(&[]);
    legato_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let lane = |c: m2_song::AudioClip| {
        let mut cache = audio::Cache::with_renders_in(None);
        played(&audio::AudioLane::build_in(&[c], &|a| Some(dir.join(a)), &mut cache, &tempo, RATE as f64, 1, makers, None).unwrap(), len)
    };
    let plain = lane(the_clip(json!([]), None));
    let peak = plain.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let fade = (0.01 * RATE as f64) as usize;
    for by in [2.0, 0.5, -3.0] {
        let c = the_clip(json!([]), Some(legato_notes(by)));
        let edited = lane(c.clone());
        let ph = audio::phrases_of(&c, &tempo, RATE as f64);
        assert_eq!(ph.len(), 1);
        let p = &ph[0];
        for (edge, a, b) in [("start", p.from, p.from + fade), ("end", p.to - fade, p.to)] {
            let worst = (a..b).map(|i| (edited[i] - plain[i]).abs()).fold(0.0f32, f32::max) / peak;
            assert!(worst < 1e-3, "moved {by}: the phrase's {edge} crossfade is the clip as it was, but for {worst} of its peak");
        }
        let d = differ(&plain, &edited);
        assert!(d[0] >= p.from && *d.last().unwrap() < p.to, "moved {by}: only the phrase changed");
    }
}

// RENDER-30b: live, after any order of edits, once their renders are made, the lane plays the bits
// an export plays, for a plain clip and a warped one (the review's S12). While a phrase's render is held, the lane plays what it played there before (for
// a first edit, the clip with no render), with every chunk there and no miss; once whole, the disk
// thread's work again puts the new phrase in, a chunk after an old one fading in first.
#[test]
fn after_any_order_of_edits_the_live_lane_plays_the_exports_bits() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let path = |a: &str| Some(dir.join(a));
    for (w, warp) in [json!([]), json!([[0, 0], [3, BEATS]])].into_iter().enumerate() {
        let plain = {
            let mut cache = audio::Cache::with_renders_in(None);
            played(
                &audio::AudioLane::build_in(&[the_clip(warp.clone(), None)], &path, &mut cache, &tempo, RATE as f64, 1, makers, None)
                    .unwrap(),
                len,
            )
        };
        let (a, b, c) = (notes(0.0, 1.0), notes(0.5, 2.0), notes(0.0, -1.5));
        for (n, order) in [vec![&a, &b], vec![&b, &a, &c], vec![&a, &c, &a]].into_iter().enumerate() {
            let mut live = audio::Cache::live_in(dir.join(format!("renders-{w}-{n}")));
            // The clip as it was, made whole first: a warped one's stretch too.
            let base = audio::AudioLane::build_in(&[the_clip(warp.clone(), None)], &path, &mut live, &tempo, RATE as f64, 1, makers, None)
                .unwrap();
            assert!(live.settle_renders(std::time::Duration::from_secs(20)));
            for k in 0..(len as u64).div_ceil(base.chunk()) {
                base.fill_chunk(k, &|_| true);
            }
            assert!(differ(&copied(&base, len), &plain).is_empty(), "warp {w}: the clip as it was plays its export's bits");
            let (mut was, mut before): (Option<Arc<audio::AudioLane>>, Vec<f32>) = (Some(base), plain.clone());
            let mut faded = false;
            for (e, sung) in order.iter().enumerate() {
                live.hold_renders(Some(0));
                let clips = [the_clip(warp.clone(), Some((*sung).clone()))];
                let lane = audio::AudioLane::build_in(&clips, &path, &mut live, &tempo, RATE as f64, 1, makers, was.as_ref()).unwrap();
                live.prune();
                assert!(live.settle_renders(std::time::Duration::from_secs(20)));
                let now = copied(&lane, len);
                assert_eq!(lane.misses(), 0, "order {n}, edit {e}: every chunk there while its render is made");
                let phrases = audio::phrases_of(&clips[0], &tempo, RATE as f64);
                // An edit back to one made before finds its renders whole in the cache folder: it plays
                // them at once.
                if order[..e].contains(sung) {
                    let mut export = audio::Cache::with_renders_in(None);
                    let lane = audio::AudioLane::build_in(&clips, &path, &mut export, &tempo, RATE as f64, 1, makers, None).unwrap();
                    assert!(differ(&now, &played(&lane, len)).is_empty(), "order {n}, edit {e}: made before, it plays at once");
                }
                for (i, (x, y)) in now.iter().zip(&before).enumerate().filter(|_| !order[..e].contains(sung)) {
                    let inside = phrases.iter().any(|p| (p.from..p.to).contains(&i));
                    let want = if inside { *y } else { plain[i] };
                    assert_eq!(
                        x.to_bits(),
                        want.to_bits(),
                        "order {n}, edit {e}: frame {i} ({} s) plays what it played before (inside {inside}; plain {}, before {y}, now {x})",
                        secs(i),
                        plain[i]
                    );
                }
                // Made, then worked out again from the last chunk back: each new chunk after an old one
                // fades in, and is worked out once more when the old one before it is gone.
                live.hold_renders(None);
                assert!(live.settle_renders(std::time::Duration::from_secs(20)));
                let chunks = (len as u64).div_ceil(lane.chunk());
                for round in 0..4 {
                    let stale: Vec<u64> = (0..chunks).rev().filter(|&k| lane.stale(k)).collect();
                    if round > 0 && !stale.is_empty() {
                        faded = true;
                    }
                    for k in stale {
                        lane.refresh(k);
                    }
                }
                assert!((0..chunks).all(|k| !lane.stale(k)), "order {n}, edit {e}: nothing left to work out");
                before = copied(&lane, len);
                assert_eq!(lane.misses(), 0);
                was = Some(lane);
            }
            let mut export = audio::Cache::with_renders_in(None);
            let last = [the_clip(warp.clone(), Some((*order.last().unwrap()).clone()))];
            let exported =
                played(&audio::AudioLane::build_in(&last, &path, &mut export, &tempo, RATE as f64, 1, makers, None).unwrap(), len);
            let d = differ(&before, &exported);
            assert!(d.is_empty(), "order {n}: the live lane plays the export's bits but at {} frames, from {} s", d.len(), secs(d[0]));
            assert!(differ(&exported, &plain).len() > RATE as usize / 4, "order {n}: the edits are heard");
            assert!(faded, "order {n}: a chunk after an old one faded in, and was worked out again");
        }
    }
}

// VOX-47: a timing drag writes markers at the note's start and end where it is dragged to, and
// keeps markers where they play at the end of the note before and the start of the note after
// (the clip's start and end too, on a clip with none): only the note and its two gaps stretch.
// Here the second note is dragged 0.1 s later. Measured on the sound: the first and last notes play
// where they played, within the 8 ms std:stretch may take a frame from where the map says (its
// search, 384 frames: WSOLA lays the sound out again, so alike, not the same bits), and the second
// starts and ends 0.1 s later.
#[test]
fn a_timing_drag_stretches_only_its_gaps() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let lane = |warp: Value| {
        let mut cache = audio::Cache::with_renders_in(None);
        let c = the_clip(warp, Some(notes(0.0, 0.0)));
        played(&audio::AudioLane::build_in(&[c], &|a| Some(dir.join(a)), &mut cache, &tempo, RATE as f64, 1, makers, None).unwrap(), len)
    };
    let before = lane(json!([]));
    // As the page writes them (`retimed`, web/src/lib/sung.ts), in beats at 120 bpm.
    let after = lane(json!([[0, 0], [0.8, 1.6], [1.0, 2.2], [1.5, 3.2], [1.8, 3.6], [3, 6]]));
    let f = |s: f64| (s * RATE as f64) as usize;
    // The lag (in frames, within 10 ms) at which `after` over `a..b` s is most like `before`, and how
    // alike.
    let alike = |a: f64, b: f64| -> (i64, f64) {
        let m = f(0.010) as i64;
        (-m..=m)
            .map(|lag| {
                let x = &before[f(a)..f(b)];
                let y = &after[(f(a) as i64 + lag) as usize..(f(b) as i64 + lag) as usize];
                let xy: f64 = x.iter().zip(y).map(|(p, q)| (p * q) as f64).sum();
                let (xx, yy): (f64, f64) = (x.iter().map(|p| (p * p) as f64).sum(), y.iter().map(|q| (q * q) as f64).sum());
                (lag, xy / (xx * yy).sqrt().max(1e-30))
            })
            .max_by(|p, q| p.1.total_cmp(&q.1))
            .unwrap()
    };
    for (a, b) in [(0.32, 0.78), (1.82, 2.38)] {
        let (lag, r) = alike(a, b);
        assert!(lag.abs() <= 384 && r > 0.95, "{a}..{b} s plays where it played: most alike {lag} frames off ({r:.4})");
    }
    // Where the second note sounds: its first and last 5 ms over a tenth of its level.
    let loud = |x: &[f32], at: usize| x[at..at + f(0.005)].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let level = loud(&before, f(1.2));
    let edges = |x: &[f32]| {
        let on: Vec<usize> = (f(0.85)..f(1.75)).step_by(f(0.001)).filter(|&i| loud(x, i) > 0.1 * level).collect();
        (secs(on[0]), secs(*on.last().unwrap()))
    };
    let ((s0, e0), (s1, e1)) = (edges(&before), edges(&after));
    assert!((s1 - s0 - 0.1).abs() < 0.012, "it starts 0.1 s later: {s0:.3} then {s1:.3}");
    assert!((e1 - e0 - 0.1).abs() < 0.012, "it ends 0.1 s later: {e0:.3} then {e1:.3}");
}

// RENDER-30b (the review's S4): live, a timing edit while the song plays never goes silent. Its
// new stretch is made from the clip's start; till it has made a chunk's frames, the lane plays what
// the clip it replaces played there (the same clip: its file and id), with no miss; once made, the
// disk thread's work again puts the new timing in, and the lane plays the export's bits.
#[test]
fn a_timing_edit_plays_the_old_stretch_till_the_new_one_is_made() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let path = |a: &str| Some(dir.join(a));
    let (before, after) = (json!([[0, 0], [3, BEATS]]), json!([[0, 0], [0.8, 1.6], [1.0, 2.2], [1.5, 3.2], [1.8, 3.6], [3, BEATS]]));
    let mut live = audio::Cache::live_in(dir.join("renders"));
    let first = audio::AudioLane::build_in(&[the_clip(before, None)], &path, &mut live, &tempo, RATE as f64, 1, makers, None).unwrap();
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    let chunks = (len as u64).div_ceil(first.chunk());
    for k in 0..chunks {
        first.fill_chunk(k, &|_| true);
    }
    let was = copied(&first, len);
    assert!(was.iter().any(|v| v.abs() > 0.01) && first.misses() == 0);
    live.hold_renders(Some(0));
    let clips = [the_clip(after.clone(), None)];
    let lane = audio::AudioLane::build_in(&clips, &path, &mut live, &tempo, RATE as f64, 1, makers, Some(&first)).unwrap();
    live.prune();
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    let held = copied(&lane, len);
    assert_eq!(lane.misses(), 0, "held back, the new stretch's chunks are all there");
    assert!(differ(&held, &was).is_empty(), "and play what the old timing played");
    live.hold_renders(None);
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    for _ in 0..3 {
        for k in (0..chunks).rev().filter(|&k| lane.stale(k)) {
            lane.refresh(k);
        }
    }
    let now = copied(&lane, len);
    let mut export = audio::Cache::with_renders_in(None);
    let exported = played(&audio::AudioLane::build_in(&clips, &path, &mut export, &tempo, RATE as f64, 1, makers, None).unwrap(), len);
    assert!(differ(&now, &exported).is_empty(), "made, the lane plays the export's bits");
    assert_eq!(lane.misses(), 0);
}

// RENDER-30b (the review's S5): the disk thread's work again checks, as it stores the chunk, that
// the transport isn't about to read it: one it is about to read is left as it was, old, to be
// worked out again when it may.
#[test]
fn a_chunk_the_transport_reaches_is_left_as_it_was() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let path = |a: &str| Some(dir.join(a));
    let mut live = audio::Cache::live_in(dir.join("renders"));
    live.hold_renders(Some(0));
    let lane =
        audio::AudioLane::build_in(&[the_clip(json!([]), Some(notes(0.0, 1.0)))], &path, &mut live, &tempo, RATE as f64, 1, makers, None)
            .unwrap();
    let before = copied(&lane, len);
    live.hold_renders(None);
    assert!(live.settle_renders(std::time::Duration::from_secs(20)));
    let chunks = (len as u64).div_ceil(lane.chunk());
    let stale: Vec<u64> = (0..chunks).filter(|&k| lane.stale(k)).collect();
    assert!(!stale.is_empty());
    for &k in &stale {
        lane.refresh_unless(k, &|_| true);
    }
    assert!(differ(&copied(&lane, len), &before).is_empty(), "each chunk the transport reaches is left as it was");
    assert!(stale.iter().all(|&k| lane.stale(k)), "and stays old");
    for &k in &stale {
        lane.refresh_unless(k, &|_| false);
    }
    assert!(stale.iter().all(|&k| !lane.stale(k)) && !differ(&copied(&lane, len), &before).is_empty());
}

// RENDER-30b (the review's S8, S9): a clip whose edited notes can't play plays as sung, and says
// so, once a build, `sung.unheard`, naming the clip: played backwards, at another pitch, or naming
// a `render` the song's packs don't have, or one of std's (no pack's voice renderer). A clip with
// edits that play says nothing.
#[test]
fn edits_that_cant_play_say_so() {
    let dir = project(&[]);
    sung_file(&dir);
    let pk = own_packages();
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let makers = audio::Makers { stretch: Some(&k), packages: Some(&pk) };
    let tempo = m2_song::TempoMap::constant(120.0);
    let len = 3 * RATE as usize;
    let plain = {
        let mut cache = audio::Cache::with_renders_in(None);
        played(
            &audio::AudioLane::build_in(
                &[the_clip(json!([]), None)],
                &|a| Some(dir.join(a)),
                &mut cache,
                &tempo,
                RATE as f64,
                1,
                makers,
                None,
            )
            .unwrap(),
            len,
        )
    };
    let told = |c: m2_song::AudioClip| {
        let mut cache = audio::Cache::with_renders_in(None);
        let l = audio::AudioLane::build_in(&[c], &|a| Some(dir.join(a)), &mut cache, &tempo, RATE as f64, 1, makers, None).unwrap();
        let said: Vec<_> = cache.faults().drain().into_iter().map(|f| (f.code, f.subject)).collect();
        (said, played(&l, len))
    };
    let clip = crate::fault::Subject::Clip { lane: 0, clip: 0 };
    let edited = the_clip(json!([]), Some(notes(0.0, 1.0)));
    for (why, c) in [
        ("backwards", m2_song::AudioClip { reverse: true, ..edited.clone() }),
        ("std's", m2_song::AudioClip { render: Some("std:stretch".into()), ..edited.clone() }),
        ("not the song's", m2_song::AudioClip { render: Some("@nobody/none:voice".into()), ..edited.clone() }),
    ] {
        let (said, out) = told(c.clone());
        assert_eq!(said, [(Code::NotesUnheard, clip)], "{why}: said, once");
        let as_sung = told(m2_song::AudioClip { sung: vec![], render: None, ..c }).1;
        assert!(differ(&out, &as_sung).is_empty(), "{why}: it plays as sung");
    }
    let pitched = m2_song::AudioClip { pitch: 2.0, ..edited.clone() };
    assert_eq!(told(pitched).0, [(Code::NotesUnheard, clip)], "at another pitch: said");
    let (said, out) = told(edited);
    assert!(said.is_empty() && !differ(&out, &plain).is_empty(), "edits that play say nothing");
}

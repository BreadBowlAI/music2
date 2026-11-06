//! Curves measured from the sound itself (CARD-81 to CARD-84): a filter's response against the
//! textbook's, a drum's hit against a render of one note, bit for bit, and what each refuses.

use m2_runtime::Renderer;
use m2_session::Session;
use serde_json::json;

use super::*;
use crate::testing::{Fixture, MANIFEST};

/// A lowpass after a saw, and a drum on its notes lane.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { drums } from '@m2/drums';\nimport { lowpass } from 'std/filter';\n\
                    import { osc } from 'std/dsp';\nimport { sum } from 'std/math';\n\
                    const lead = track('lead', () => lowpass(osc.saw(110), { cutoff: 2400, q: 2 }));\n\
                    const kick = track('kick', ({ lane }) => drums.drum(lane.notes('hits'), { decay: 0.5 }));\n\
                    export default song(() => sum(lead.out, kick.out));\n";

fn session() -> (Fixture, Session) {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("song.ts", SONG);
    let s = f.open();
    (f, s)
}

/// The view's id for the call of `callee` in `song.ts`.
fn id(s: &Session, callee: &str) -> String {
    let site = s.accepted().compiled.sites.iter().find(|x| x.file == "song.ts" && x.callee == callee).unwrap();
    format!("{}#{}", site.file, site.ordinal)
}

fn values(v: serde_json::Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// The cookbook's lowpass (Robert Bristow-Johnson's), in dB at `hz`: the bilinear transform of a
/// two-pole lowpass, its cutoff prewarped.
fn textbook(cutoff: f64, q: f64, hz: f64, rate: f64) -> f64 {
    let w0 = std::f64::consts::TAU * cutoff / rate;
    let (cos, alpha) = (w0.cos(), w0.sin() / (2.0 * q));
    let (b0, b1, b2) = ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0);
    let (a0, a1, a2) = (1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    let w = std::f64::consts::TAU * hz / rate;
    let at = |c: [f64; 3]| {
        let re = c[0] + c[1] * w.cos() + c[2] * (2.0 * w).cos();
        let im = -c[1] * w.sin() - c[2] * (2.0 * w).sin();
        re * re + im * im
    };
    10.0 * (at([b0, b1, b2]) / at([a0, a1, a2])).log10()
}

const HZ: Span = Span { log: true, lo: 20.0, hi: 20_000.0 };

// CARD-81, CARD-82: a lowpass's response, measured from an offline copy of its call, is the
// textbook's curve within 0.5 dB at every point from 20 Hz to 20 kHz where it is above −60 dB: at
// the value its code writes, and at the ones a picture gives (a lease's, as it is dragged), which
// the code doesn't, a low one ringing long among them.
#[test]
fn a_measured_lowpass_is_the_textbook_curve() {
    let (_f, s) = session();
    let rev = s.accepted();
    let node = id(&s, "lowpass");
    for (cutoff, q, given) in [
        (2400.0, 2.0, json!({})),
        (300.0, 0.707, json!({"cutoff": 300, "q": 0.707})),
        (8000.0, 4.0, json!({"cutoff": 8000, "q": 4})),
        (60.0, 4.0, json!({"cutoff": 60, "q": 4})),
    ] {
        let copy = copy(rev, &s.packages, "lead", &node, &values(given), Layer::Response).unwrap();
        let step = copy.render(&s.packages).unwrap();
        let points = response(&step, copy.rate(), HZ);
        assert_eq!(points.len(), 2 * POINTS, "every point is under half the rate");
        let shown = points.chunks(2).map(|p| (p[1], textbook(cutoff, q, p[0], 48_000.0))).filter(|(_, t)| *t > -60.0);
        let worst = shown.map(|(m, t)| (m - t).abs()).fold(0.0, f64::max);
        assert!(worst < 0.5, "at {cutoff} Hz, q {q}: {worst} dB from the textbook");
        let peak = points.chunks(2).map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!(q < 1.0 || peak > 3.0, "a resonance at q {q} rises above 0 dB: {peak}");
    }
}

// CARD-83: a drum's hit, from the offline copy of its call, is a render of the song with one note
// on its lane at sample 0, bit for bit, on both sides.
#[test]
fn a_hit_is_a_render_of_one_note_bit_for_bit() {
    let (_f, s) = session();
    let rev = s.accepted();
    let node = id(&s, "drums.drum");
    let seconds = 0.5;
    let copy = copy(rev, &s.packages, "kick", &node, &Map::new(), Layer::Hit { note: 36, seconds }).unwrap();
    let hit = copy.frames(&s.packages).unwrap();
    // The song itself, its lead aside, with that one note on the drum's lane.
    let solo = SONG.replace("sum(lead.out, kick.out)", "kick.out");
    let (f2, s2) = {
        let f = Fixture::new();
        f.write("music2.toml", MANIFEST);
        f.write("song.ts", &solo);
        let s = f.open();
        (f, s)
    };
    let rev2 = s2.accepted();
    let beats = rev2.tempo().beat_at(seconds);
    let one = m2_song::lane::LaneData::Notes(vec![m2_song::lane::NoteClip {
        id: None,
        other_id: None,
        start: 0.0,
        length: beats,
        notes: vec![m2_song::lane::Note { pitch: 36.0, start: 0.0, length: beats, velocity: m2_song::lane::VELOCITY }],
    }]);
    let addr = m2_song::LaneAddr::new("kick", "hits").unwrap();
    let data = rev2.compiled.data.seeded(&addr, &m2_song::lane::LaneKind::Notes, std::sync::Arc::from(""), std::sync::Arc::new(one));
    let mut r = Renderer::new(&rev2.compiled.flat, &data, &s2.packages, 48_000).unwrap();
    let mut song = vec![[0.0f32; 2]; hit.len()];
    r.process(&mut song).unwrap();
    drop(f2);
    assert_eq!(hit.len(), 24_000);
    assert!(hit.iter().any(|x| x[0].abs() > 0.1), "the kick sounds");
    let bits = |v: &[[f32; 2]]| v.iter().flat_map(|x| [x[0].to_bits(), x[1].to_bits()]).collect::<Vec<_>>();
    assert!(bits(&hit) == bits(&song), "the hit is the song's note, bit for bit");
    // As a picture draws it: the highest sample of each column, then the lowest, back.
    let mid: Vec<f64> = hit.iter().map(|[l, r]| 0.5 * (*l as f64 + *r as f64)).collect();
    let shape = super::hit(&mid, seconds);
    assert_eq!(shape.len(), 4 * COLUMNS);
    let top = mid[..24_000 / COLUMNS].iter().copied().fold(f64::MIN, f64::max);
    assert_eq!((shape[0], shape[1]), (0.5 / COLUMNS as f64 * seconds, (top * 1e6).round() / 1e6));
}

// CARD-81: a copy is refused in words where its layer can't be measured: a response of a sound
// that takes no sound in, a hit of one that plays no notes, a call that isn't one.
#[test]
fn a_copy_is_refused_in_words() {
    let (_f, s) = session();
    let rev = s.accepted();
    let why = |module: &str, callee: &str, layer| copy(rev, &s.packages, module, &id(&s, callee), &Map::new(), layer).err().unwrap();
    assert_eq!(why("kick", "drums.drum", Layer::Response), "it takes no sound in, so p.response has nothing to play through it");
    assert_eq!(why("lead", "lowpass", Layer::Hit { note: 60, seconds: 1.0 }), "it plays no notes, so p.hit has no note to play");
    let gone = copy(rev, &s.packages, "lead", "song.ts#99", &Map::new(), Layer::Response).err().unwrap();
    assert_eq!(gone, "there's no call song.ts#99 in lead now");
}

// CARD-84: a copy is named by what it is made from: the same call at the same values is the same
// key, at another value another, and the other layer another.
#[test]
fn a_copy_is_named_by_what_it_is_made_from() {
    let (_f, s) = session();
    let rev = s.accepted();
    let node = id(&s, "lowpass");
    let key = |v: serde_json::Value, layer| copy(rev, &s.packages, "lead", &node, &values(v), layer).unwrap().key;
    let at = key(json!({"cutoff": 300}), Layer::Response);
    assert_eq!(at, key(json!({"cutoff": 300}), Layer::Response));
    assert_ne!(at, key(json!({"cutoff": 301}), Layer::Response));
    assert_eq!(key(json!({}), Layer::Response), key(json!({"cutoff": 2400}), Layer::Response), "the code's own value");
    let kick = id(&s, "drums.drum");
    let hit = |seconds| copy(rev, &s.packages, "kick", &kick, &Map::new(), Layer::Hit { note: 36, seconds }).unwrap().key;
    assert_ne!(hit(0.5), hit(0.25));
}

// CARD-85: a measure is cheap enough to follow a drag: a lowpass's response, and half a second of
// the Drum's hit, each copied, rendered and drawn at a new value each time, under 4 ms at p99. It
// judges speed, so it runs in the nightly tier (ci/nightly-tests.txt).
#[test]
fn a_measure_follows_a_drag_inside_its_budget() {
    let (_f, s) = session();
    let rev = s.accepted();
    let (lowpass, kick) = (id(&s, "lowpass"), id(&s, "drums.drum"));
    let time = |module: &str, node: &str, layer: Layer, at: &dyn Fn(usize) -> serde_json::Value| {
        let mut took: Vec<std::time::Duration> = (0..200)
            .map(|i| {
                let t = std::time::Instant::now();
                let c = copy(rev, &s.packages, module, node, &values(at(i)), layer).unwrap();
                let sound = c.render(&s.packages).unwrap();
                match layer {
                    Layer::Response => assert_eq!(response(&sound, c.rate(), HZ).len(), 2 * POINTS),
                    Layer::Hit { seconds, .. } => assert_eq!(hit(&sound, seconds).len(), 4 * COLUMNS),
                }
                t.elapsed()
            })
            .collect();
        took.sort();
        (took[took.len() / 2], took[took.len() * 99 / 100])
    };
    let response = time("lead", &lowpass, Layer::Response, &|i| json!({"cutoff": 200 + 40 * i, "q": 0.5 + (i % 7) as f64}));
    let hit = time("kick", &kick, Layer::Hit { note: 36, seconds: 0.5 }, &|i| json!({"decay": 0.2 + 0.01 * i as f64}));
    eprintln!("a response: median {:?}, p99 {:?}; a hit of 0.5 s: median {:?}, p99 {:?}", response.0, response.1, hit.0, hit.1);
    assert!(response.1 < std::time::Duration::from_millis(4), "a response's p99 {:?}: over the 4 ms budget", response.1);
    assert!(hit.1 < std::time::Duration::from_millis(4), "a hit's p99 {:?}: over the 4 ms budget", hit.1);
}

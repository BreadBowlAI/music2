//! Curves measured from the sound itself (CARD-81 to CARD-85): filters and an EQ against the
//! textbook's, at three rates; a delay against a long listen; a drum's hit against a render of one
//! note, bit for bit; what each refuses, in words; and a measure's own budget.

use std::time::{Duration, Instant};

use m2_runtime::Renderer;
use m2_session::Session;
use serde_json::json;

use super::*;
use crate::testing::Fixture;

/// The packs these songs read: the tests' manifest's, and `@m2/mixing`'s EQ.
fn manifest(rate: u32, extra: &str) -> String {
    format!(
        "[project]\nname = \"t\"\nformat = 1\nsample_rate = {rate}\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.10\"\n\"@m2/drums\" = \"2\"\n\
         \"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n\"@m2/mixing\" = \"1\"\n{extra}"
    )
}

/// What every song here imports.
const HEAD: &str = "import { song, track, time, plugin } from '@m2/core';\nimport { drums } from '@m2/drums';\n\
                    import { lowpass, highpass, notch } from 'std/filter';\nimport { osc, mono } from 'std/dsp';\nimport { sum, xrange } from 'std/math';\n\
                    import { lfo } from 'std/control';\nimport { eq } from '@m2/mixing';\nimport { drive, chorus, delay, reverb } from '@m2/effects';\n\
                    import { compress } from 'std/fx';\n";

/// A lowpass after a saw, and a drum on its notes lane.
const SONG: &str = "const lead = track('lead', () => lowpass(osc.saw(110), { cutoff: 2400, q: 2 }));\n\
                    const kick = track('kick', ({ lane }) => drums.drum(lane.notes('hits'), { decay: 0.5 }));\n\
                    export default song(() => sum(lead.out, kick.out));\n";

fn open_at(rate: u32, body: &str) -> (Fixture, Session) {
    let f = Fixture::new();
    f.write("music2.toml", manifest(rate, ""));
    f.write("song.ts", format!("{HEAD}{body}"));
    let s = f.open();
    (f, s)
}

fn session() -> (Fixture, Session) {
    open_at(48_000, SONG)
}

/// The view's id for the call of `callee` in `song.ts` that track `module` makes.
fn id_in(s: &Session, module: &str, callee: &str) -> String {
    let rev = s.accepted();
    let c = &rev.compiled;
    let site = c
        .sites
        .iter()
        .find(|x| x.file == "song.ts" && x.callee == callee && c.outputs.iter().any(|o| o.site == x.id && o.module == module))
        .unwrap_or_else(|| panic!("no {callee} in {module}"));
    format!("{}#{}", site.file, site.ordinal)
}

fn id(s: &Session, callee: &str) -> String {
    let site = s.accepted().compiled.sites.iter().find(|x| x.file == "song.ts" && x.callee == callee).unwrap();
    format!("{}#{}", site.file, site.ordinal)
}

fn values(v: serde_json::Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

/// A measure's budget, roomy for a test on a busy machine: what is tested here is the sound.
const ROOMY: Duration = Duration::from_secs(5);

/// The response of `module`'s call of `callee` at `v`, measured as a picture's is.
fn measured(s: &Session, module: &str, callee: &str, v: serde_json::Value) -> Result<Vec<f64>, Failed> {
    let c = copy(s.accepted(), &s.packages, module, &id_in(s, module, callee), &values(v), Layer::Response { level: LEVEL })
        .map_err(Failed::Cant)?;
    c.measure(&s.packages, HZ, ROOMY, Pass::Rest)
}

/// The cookbook's biquads (Robert Bristow-Johnson's), in dB at `hz`: the bilinear transform of an
/// analog prototype, its frequency prewarped.
fn cookbook(kind: &str, f0: f64, q: f64, gain: f64, hz: f64, rate: f64) -> f64 {
    let w0 = std::f64::consts::TAU * f0 / rate;
    let (c, al) = (w0.cos(), w0.sin() / (2.0 * q));
    let a = 10f64.powf(gain / 40.0);
    let (b, d) = match kind {
        "lp" => ([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al]),
        "hp" => ([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al]),
        "bell" => ([1.0 + al * a, -2.0 * c, 1.0 - al * a], [1.0 + al / a, -2.0 * c, 1.0 - al / a]),
        _ => unreachable!(),
    };
    let w = std::f64::consts::TAU * hz / rate;
    let at = |x: [f64; 3]| {
        let re = x[0] + x[1] * w.cos() + x[2] * (2.0 * w).cos();
        let im = -x[1] * w.sin() - x[2] * (2.0 * w).sin();
        re * re + im * im
    };
    10.0 * (at(b) / at(d)).log10()
}

fn textbook(cutoff: f64, q: f64, hz: f64, rate: f64) -> f64 {
    cookbook("lp", cutoff, q, 0.0, hz, rate)
}

/// How far `points` are from `t` at most, where `t` is above −60 dB.
fn off(points: &[f64], t: &dyn Fn(f64) -> f64) -> f64 {
    points.chunks(2).filter(|p| t(p[0]) > -60.0).map(|p| (p[1] - t(p[0])).abs()).fold(0.0, f64::max)
}

/// A curve the textbook gives, in dB at a frequency.
type Curve = Box<dyn Fn(f64) -> f64>;

const HZ: Span = Span { log: true, lo: 20.0, hi: 20_000.0 };

// CARD-81, CARD-82: a lowpass's response, measured from an offline copy of its call, is the
// textbook's curve within 0.5 dB at every point from 20 Hz to 20 kHz where it is above −60 dB: at
// the value its code writes, and at the ones a picture gives (a lease's, as it is dragged), which
// the code doesn't, a low one ringing long among them.
#[test]
fn a_measured_lowpass_is_the_textbook_curve() {
    let (_f, s) = session();
    for (cutoff, q, given) in [
        (2400.0, 2.0, json!({})),
        (300.0, 0.707, json!({"cutoff": 300, "q": 0.707})),
        (8000.0, 4.0, json!({"cutoff": 8000, "q": 4})),
        (60.0, 4.0, json!({"cutoff": 60, "q": 4})),
    ] {
        let points = measured(&s, "lead", "lowpass", given).unwrap();
        assert_eq!(points.len(), 2 * POINTS, "every point is under half the rate");
        let worst = off(&points, &|f| textbook(cutoff, q, f, 48_000.0));
        assert!(worst < 0.5, "at {cutoff} Hz, q {q}: {worst} dB from the textbook");
        let peak = points.chunks(2).map(|p| p[1]).fold(f64::MIN, f64::max);
        assert!(q < 1.0 || peak > 3.0, "a resonance at q {q} rises above 0 dB: {peak}");
    }
}

// CARD-82 (review M1): the copy listens until its click has died away, however long that is at the
// song's rate, up to its cap: a lowpass at 25 Hz ringing at q 10, a highpass at 30 Hz, and the EQ's
// sharp bell at 40 Hz are the textbook's at 44.1, 48 and 96 kHz; a short delay's echoes are what a
// listen of 2^19 frames hears; and a delay or a reverb that rings past the cap is refused, in
// words, never drawn short.
#[test]
fn a_response_listens_until_its_click_dies_away() {
    let body = "const l = track('l', () => lowpass(osc.saw(110), { cutoff: 25, q: 10 }));\n\
                const h = track('h', () => highpass(osc.saw(110), { cutoff: 30, q: 8 }));\n\
                const e = track('e', () => eq(osc.saw(110), { freq4: 40, gain4: 12, q4: 18 }));\n\
                const d = track('d', () => delay(osc.saw(110), { time: 0.1, feedback: 0.3, mix: 0.5 }));\n\
                const dl = track('dl', () => delay(osc.saw(110), { time: 0.5, feedback: 0.6, mix: 0.5 }));\n\
                const rv = track('rv', () => reverb(osc.saw(110), { size: 1, mix: 1 }));\n\
                export default song(() => sum(l.out, h.out, e.out, d.out, dl.out, rv.out));\n";
    for rate in [44_100u32, 48_000, 96_000] {
        let (_f, s) = open_at(rate, body);
        let r = rate as f64;
        let cases: [(&str, &str, Curve); 3] = [
            ("l", "lowpass", Box::new(move |f| cookbook("lp", 25.0, 10.0, 0.0, f, r))),
            ("h", "highpass", Box::new(move |f| cookbook("hp", 30.0, 8.0, 0.0, f, r))),
            ("e", "eq", Box::new(move |f| cookbook("bell", 40.0, 18.0, 12.0, f, r))),
        ];
        for (m, callee, t) in cases {
            let points = measured(&s, m, callee, json!({})).unwrap_or_else(|e| panic!("{rate} {callee}: {e:?}"));
            let worst = off(&points, &*t);
            assert!(worst < 0.5, "{callee} at {rate} Hz: {worst} dB from the textbook");
        }
        // The short delay against a fixed listen of 2^19 frames, worked out the same way.
        let c = copy(s.accepted(), &s.packages, "d", &id_in(&s, "d", "delay"), &Map::new(), Layer::Response { level: LEVEL }).unwrap();
        let points = c.measure(&s.packages, HZ, ROOMY, Pass::Rest).unwrap();
        let long = response(&c.step(&s.packages, 1 << 19).unwrap(), rate, HZ);
        let worst =
            points.chunks(2).zip(long.chunks(2)).filter(|(_, l)| l[1] > -60.0).map(|(p, l)| (p[1] - l[1]).abs()).fold(0.0, f64::max);
        assert!(worst < 0.5, "the delay's echoes at {rate} Hz: {worst} dB from a long listen");
        let ys = || points.chunks(2).map(|p| p[1]);
        let comb = ys().fold(f64::MIN, f64::max) - ys().fold(f64::MAX, f64::min);
        assert!(comb > 3.0, "its echoes comb the curve: {comb} dB from top to bottom");
        let rings =
            Err(Failed::Cant("it rings longer than music2 listens (2 s), so music2 can't measure its curve: its pack must draw it".into()));
        for (m, callee) in [("dl", "delay"), ("rv", "reverb")] {
            assert_eq!(measured(&s, m, callee, json!({})), rings, "{callee} at {rate} Hz");
        }
    }
}

// CARD-82 (review M2): a response is drawn only for a sound that stays the same at every level and
// at every time: a compressor and a drive, which change with how loud they are, and a chorus, which
// changes over time, are refused in words, never drawn wrong.
#[test]
fn a_sound_that_changes_with_level_or_time_is_refused() {
    let body = "const d = track('d', () => drive(osc.saw(110), { amount: 24 }));\n\
                const c = track('c', () => compress(osc.saw(110), { threshold: -30, ratio: 10, makeup: 0 }));\n\
                const ch = track('ch', () => chorus(osc.saw(110)));\n\
                export default song(() => sum(d.out, c.out, ch.out));\n";
    let (_f, s) = open_at(48_000, body);
    let level = Err(Failed::Cant("its sound changes with how loud it is, so music2 can't measure its curve: its pack must draw it".into()));
    let time = Err(Failed::Cant("its sound changes over time, so music2 can't measure its curve: its pack must draw it".into()));
    assert_eq!(measured(&s, "d", "drive", json!({})), level, "drive");
    assert_eq!(measured(&s, "c", "compress", json!({})), level, "compress");
    assert_eq!(measured(&s, "ch", "chorus", json!({})), time, "chorus");
}

// CARD-80, CARD-82, CARD-84: a response's click may be quieter, down to −80 dBFS. std's compress at
// a threshold of −30 dB and a ratio of 10 is refused at −20 dBFS, its sound changing with how loud
// it is; at −80 dBFS, under its threshold, it leaves the sound as it is: flat at 0 dB, passing the
// checks at a 32nd of the level at rest. The click's level is part of what the copy is made from.
#[test]
fn a_quiet_click_measures_a_sound_below_its_threshold() {
    let body = "const c = track('c', () => compress(osc.saw(110), { threshold: -30, ratio: 10, makeup: 0 }));\n\
                export default song(() => c.out);\n";
    let (_f, s) = open_at(48_000, body);
    let at = |level: f64| {
        let c = copy(s.accepted(), &s.packages, "c", &id_in(&s, "c", "compress"), &Map::new(), Layer::Response { level }).unwrap();
        (c.key.clone(), c.measure(&s.packages, HZ, ROOMY, Pass::Rest))
    };
    let (loud, refused) = at(LEVEL);
    assert!(matches!(&refused, Err(Failed::Cant(w)) if w.starts_with("its sound changes with how loud it is")), "at −20 dBFS: {refused:?}");
    let (quiet, measured) = at(-80.0);
    let points = measured.unwrap();
    assert_eq!(points.len(), 2 * POINTS);
    let worst = points.chunks(2).map(|p| p[1].abs()).fold(0.0, f64::max);
    assert!(worst < 0.001, "under its threshold it leaves the sound as it is: {worst} dB from 0 dB at most");
    assert_ne!(loud, quiet, "a copy is kept by its click's level too");
}

// CARD-85: a drag's frame measures the click alone, so it stays smooth; the checks of level and
// time run at rest. A compressor's curve is drawn while its knob moves, and greyed at rest.
#[test]
fn a_drag_frame_measures_the_click_alone() {
    let body = "const c = track('c', () => compress(osc.saw(110), { threshold: -30, ratio: 10, makeup: 0 }));\n\
                const l = track('l', () => lowpass(osc.saw(110), { cutoff: 2400, q: 2 }));\n\
                export default song(() => sum(c.out, l.out));\n";
    let (_f, s) = open_at(48_000, body);
    let measure = |module: &str, callee: &str, pass| {
        let c = copy(s.accepted(), &s.packages, module, &id_in(&s, module, callee), &Map::new(), Layer::Response { level: LEVEL }).unwrap();
        c.measure(&s.packages, HZ, ROOMY, pass)
    };
    assert_eq!(measure("c", "compress", Pass::Drag).map(|p| p.len()), Ok(2 * POINTS), "while dragged: its click's curve");
    assert!(matches!(measure("c", "compress", Pass::Rest), Err(Failed::Cant(w)) if w.starts_with("its sound changes with how loud it is")));
    assert_eq!(measure("l", "lowpass", Pass::Drag), measure("l", "lowpass", Pass::Rest), "a linear sound's curve is the same both ways");
}

// CARD-85: a drag's frame of the eight-band EQ's response stays under 8 ms of the thread's CPU at
// p99 at 48 kHz. It judges speed, so it runs in the nightly tier (ci/nightly-tests.txt).
#[test]
fn a_drag_frame_of_the_eq_stays_under_8_ms() {
    let (_f, s) = open_at(48_000, "const e = track('e', () => eq(osc.saw(110)));\nexport default song(() => e.out);\n");
    let rev = s.accepted();
    let node = id_in(&s, "e", "eq");
    let mut took: Vec<Duration> = (0..200)
        .map(|i| {
            let t = m2_os::thread::cpu_time();
            let packages = PackageSet::default().viewing(&rev.compiled.view).into_owned().with_offline_limit(BUDGET);
            let c =
                copy(rev, &packages, "e", &node, &values(json!({"gain4": (i % 48) as f64 / 2.0 - 12.0})), Layer::Response { level: LEVEL })
                    .unwrap();
            assert_eq!(c.measure(&packages, HZ, BUDGET, Pass::Drag).unwrap().len(), 2 * POINTS);
            m2_os::thread::cpu_time() - t
        })
        .collect();
    took.sort();
    let (median, p99) = (took[took.len() / 2], took[took.len() * 99 / 100]);
    eprintln!("a drag's frame of the EQ: median {median:?}, p99 {p99:?}");
    assert!(p99 < Duration::from_millis(8), "p99 {p99:?} (median {median:?}): over 8 ms");
}

// CARD-83 (review M6): a drum's hit, from the offline copy of its call, is a render of the song with
// one note on its lane at sample 0, bit for bit, on both sides, at 44.1 and 48 kHz.
#[test]
fn a_hit_is_a_render_of_one_note_bit_for_bit() {
    for rate in [44_100u32, 48_000] {
        let (_f, s) = open_at(rate, SONG);
        let rev = s.accepted();
        let node = id(&s, "drums.drum");
        let seconds = 0.5;
        let copy = copy(rev, &s.packages, "kick", &node, &Map::new(), Layer::Hit { note: 36, seconds }).unwrap();
        let hit = copy.frames(&s.packages, ROOMY).unwrap();
        // The song itself, its lead aside, with that one note on the drum's lane.
        let (_f2, s2) = open_at(rate, &SONG.replace("sum(lead.out, kick.out)", "kick.out"));
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
        let mut r = Renderer::new(&rev2.compiled.flat, &data, &s2.packages, rev2.project.sample_rate()).unwrap();
        let mut song = vec![[0.0f32; 2]; hit.len()];
        r.process(&mut song).unwrap();
        assert_eq!(hit.len(), (seconds * rate as f64).round() as usize);
        assert!(hit.iter().any(|x| x[0].abs() > 0.1), "the kick sounds");
        let bits = |v: &[[f32; 2]]| v.iter().flat_map(|x| [x[0].to_bits(), x[1].to_bits()]).collect::<Vec<_>>();
        assert!(bits(&hit) == bits(&song), "the hit is the song's note, bit for bit, at {rate} Hz");
        // As a picture draws it: the highest sample of each column, then the lowest, back.
        let mid: Vec<f64> = hit.iter().map(|[l, r]| 0.5 * (*l as f64 + *r as f64)).collect();
        let shape = super::hit(&mid, seconds);
        assert_eq!(shape.len(), 4 * COLUMNS);
        let top = mid[..hit.len() / COLUMNS].iter().copied().fold(f64::MIN, f64::max);
        assert_eq!((shape[0], shape[1]), (0.5 / COLUMNS as f64 * seconds, (top * 1e6).round() / 1e6));
    }
}

// CARD-81 (review M2, M5): a copy is refused in words where its layer can't be measured: a response
// of a sound that takes no sound in, a hit of one that plays no notes, a call that isn't there, is
// called more than once, plays inside an instrument's voices, gives no sound, hosts a plugin, reads
// a value from another track; and a response of one whose values move as the song plays.
#[test]
fn a_copy_is_refused_in_words() {
    m2_plugins::probe::install();
    let body = "import { component, port } from '@m2/core';\n\
                const wrap = component({ id: 'wrap', label: 'Wrap', inputs: { x: port.audio() }, output: port.audio() }, (x) => plugin('clap:m2.probe', { in: x, key: 'p' }));\n\
                const lead = track('lead', () => lowpass(osc.saw(110), { cutoff: 2400, q: 2 }));\n\
                const kick = track('kick', ({ lane }) => drums.drum(lane.notes('hits'), { decay: 0.5 }));\n\
                const m = track('m', () => sum(...[110, 220].map((f) => notch(osc.saw(f), { cutoff: 900 }))));\n\
                const v = track('v', ({ lane }) => mono(lane.notes('n'), (n) => highpass(osc.saw(110), { cutoff: 300 })));\n\
                const src = track('src', () => ({ out: osc.sine(1), cut: xrange(lfo(1), 200, 4000) }));\n\
                const o = track('o', () => notch(osc.saw(110), { cutoff: src.cut }));\n\
                const w = track('w', () => lowpass(osc.saw(110), { cutoff: xrange(lfo(time.beats, { period: 0.25 }), 200, 4000), q: 4 }));\n\
                const ln = track('ln', ({ lane }) => highpass(osc.saw(110), { cutoff: lane.control('cut', 'hz', 300) }));\n\
                const g = track('g', () => osc.sine(xrange(lfo(1), 200, 400)));\n\
                const p = track('p', () => wrap(osc.saw(110)));\n\
                export default song(() => sum(lead.out, kick.out, m.out, v.out, src.out, o.out, w.out, ln.out, g.out, p.out));\n";
    let (_f, s) = open_at(48_000, body);
    let rev = s.accepted();
    let why =
        |module: &str, callee: &str, layer| copy(rev, &s.packages, module, &id_in(&s, module, callee), &Map::new(), layer).err().unwrap();
    assert_eq!(
        why("kick", "drums.drum", Layer::Response { level: LEVEL }),
        "it takes no sound in, so p.response has nothing to play through it"
    );
    assert_eq!(why("lead", "lowpass", Layer::Hit { note: 60, seconds: 1.0 }), "it plays no notes, so p.hit has no note to play");
    let gone = copy(rev, &s.packages, "lead", "song.ts#999", &Map::new(), Layer::Response { level: LEVEL }).err().unwrap();
    assert_eq!(gone, "there's no call song.ts#999 in lead now");
    assert_eq!(why("m", "notch", Layer::Response { level: LEVEL }), "it is called more than once");
    assert_eq!(why("v", "highpass", Layer::Response { level: LEVEL }), "it plays inside an instrument's voices");
    assert_eq!(why("o", "notch", Layer::Response { level: LEVEL }), "one of its values comes from another track");
    assert_eq!(why("p", "wrap", Layer::Response { level: LEVEL }), "it hosts a plugin, which music2 can't measure yet");
    assert_eq!(why("g", "xrange", Layer::Response { level: LEVEL }), "it gives no sound");
    let moves = "one of its values moves as the song plays (a lane, a cable or the song's beat drives it), so it has no one curve";
    assert_eq!(why("w", "lowpass", Layer::Response { level: LEVEL }), moves, "a cable from the beat");
    assert_eq!(why("ln", "highpass", Layer::Response { level: LEVEL }), moves, "a lane");
}

// CARD-84 (review M7): a copy is named by what it plays: the same call at the same values is the
// same key, at another value another, and the other layer another; an edit that moves the call down
// its file, but leaves its sound, keeps the key.
#[test]
fn a_copy_is_named_by_what_it_is_made_from() {
    let (_f, s) = session();
    let rev = s.accepted();
    let node = id(&s, "lowpass");
    let key = |v: serde_json::Value, layer| copy(rev, &s.packages, "lead", &node, &values(v), layer).unwrap().key;
    let at = key(json!({"cutoff": 300}), Layer::Response { level: LEVEL });
    assert_eq!(at, key(json!({"cutoff": 300}), Layer::Response { level: LEVEL }));
    assert_ne!(at, key(json!({"cutoff": 301}), Layer::Response { level: LEVEL }));
    assert_eq!(
        key(json!({}), Layer::Response { level: LEVEL }),
        key(json!({"cutoff": 2400}), Layer::Response { level: LEVEL }),
        "the code's own value"
    );
    let kick = id(&s, "drums.drum");
    let hit = |seconds| copy(rev, &s.packages, "kick", &kick, &Map::new(), Layer::Hit { note: 36, seconds }).unwrap().key;
    assert_ne!(hit(0.5), hit(0.25));
    let (_f2, moved) = open_at(48_000, &format!("// a line above it\n\n{SONG}"));
    let there =
        copy(moved.accepted(), &moved.packages, "lead", &id(&moved, "lowpass"), &Map::new(), Layer::Response { level: LEVEL }).unwrap().key;
    assert_eq!(there, key(json!({}), Layer::Response { level: LEVEL }), "the same sound, lines further down");
}

/// A song that plays the spin pack's two sounds (`core/session/tests/fixtures/packs/spin`).
fn spinning() -> (Fixture, Session) {
    let pack =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/packs/spin").canonicalize().unwrap();
    let f = Fixture::new();
    f.write("music2.toml", manifest(48_000, &format!("\"@test/spin\" = {{ path = {:?} }}\n", pack.display().to_string())));
    f.write(
        "song.ts",
        "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { sum } from 'std/math';\nimport { spin, slow } from '@test/spin';\n\
         const a = track('a', () => spin(osc.saw(110)));\nconst b = track('b', () => slow(osc.saw(110)));\nexport default song(() => sum(a.out, b.out));\n",
    );
    let s = f.open();
    (f, s)
}

// CARD-85 (review M3): a measure has a budget of its own, and each call into a pack's kernel the
// measure's limit: a kernel that never returns once it hears the step, and one that is slow at each
// call, are each stopped within a few tens of milliseconds, said as too slow, never drawn.
#[test]
fn a_measure_is_stopped_past_its_budget() {
    let (_f, s) = spinning();
    let rev = s.accepted();
    for (m, callee) in [("a", "spin"), ("b", "slow")] {
        let packages = s.packages.clone().with_offline_limit(BUDGET);
        let c = copy(rev, &packages, m, &id(&s, callee), &Map::new(), Layer::Response { level: LEVEL }).unwrap();
        let t = Instant::now();
        assert_eq!(c.measure(&packages, HZ, BUDGET, Pass::Rest), Err(Failed::Slow), "{callee}");
        assert!(t.elapsed() < Duration::from_millis(250), "{callee} stopped at {:?}", t.elapsed());
    }
}

// CARD-85: a measure is cheap enough to follow a drag: a lowpass's and the EQ's responses, and a
// second of the Drum's hit, each copied, measured and drawn at a new value each time, at 48 and 96
// kHz, inside its budget (20 ms at 48 kHz, 40 ms at 96) of the thread's CPU at p99. It judges
// speed, so it runs in the nightly tier (ci/nightly-tests.txt).
#[test]
fn a_measure_follows_a_drag_inside_its_budget() {
    let body = "const lead = track('lead', () => lowpass(osc.saw(110), { cutoff: 2400, q: 2 }));\n\
                const e = track('e', () => eq(osc.saw(110)));\n\
                const kick = track('kick', ({ lane }) => drums.drum(lane.notes('hits'), { decay: 0.5 }));\n\
                export default song(() => sum(lead.out, e.out, kick.out));\n";
    for rate in [48_000u32, 96_000] {
        let most = budget(rate);
        let (_f, s) = open_at(rate, body);
        let rev = s.accepted();
        let time = |module: &str, callee: &str, layer: Layer, at: &dyn Fn(usize) -> serde_json::Value| {
            let node = id_in(&s, module, callee);
            let mut took: Vec<Duration> = (0..100)
                .map(|i| {
                    // The work it takes, as the thread's CPU time, so a busy machine doesn't count.
                    let t = m2_os::thread::cpu_time();
                    let packages = PackageSet::default().viewing(&rev.compiled.view).into_owned().with_offline_limit(most);
                    let c = copy(rev, &packages, module, &node, &values(at(i)), layer).unwrap();
                    let span = if matches!(layer, Layer::Response { .. }) { HZ } else { Span { log: false, lo: 0.0, hi: 1.0 } };
                    assert!(!c.measure(&packages, span, most, Pass::Rest).unwrap().is_empty());
                    m2_os::thread::cpu_time() - t
                })
                .collect();
            took.sort();
            (took[took.len() / 2], took[took.len() * 99 / 100])
        };
        let lowpass =
            time("lead", "lowpass", Layer::Response { level: LEVEL }, &|i| json!({"cutoff": 200 + 40 * i, "q": 0.5 + (i % 7) as f64}));
        let eq = time("e", "eq", Layer::Response { level: LEVEL }, &|i| json!({"gain4": (i % 24) as f64 - 12.0}));
        let hit = time("kick", "drums.drum", Layer::Hit { note: 36, seconds: 1.0 }, &|i| json!({"decay": 0.2 + 0.01 * i as f64}));
        eprintln!("at {rate} Hz: a lowpass {lowpass:?}, the EQ {eq:?}, a hit of 1 s {hit:?} (median, p99)");
        for (what, (_, p99)) in [("a lowpass's response", lowpass), ("the EQ's response", eq), ("a hit of 1 s", hit)] {
            assert!(p99 < most, "{what} at {rate} Hz: p99 {p99:?}, over {most:?}");
        }
    }
}

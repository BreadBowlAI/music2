//! Comping (VOX-1 to VOX-6): a song of one Vox track at 120 bpm (a beat is 0.5 s), playing a take
//! over a loop of 8 beats: one file of three passes of 4 s each, its note listing them.

use m2_session::Session;
use m2_song::{AudioClip, LaneAddr, LaneData};

use crate::clips::{ClipRef, Gesture, edit};
use crate::testing::{Fixture, MANIFEST};

const RATE: u32 = 48_000;
/// Where each pass starts in the file, in seconds.
const TAKES: [f64; 3] = [0.0, 4.0, 8.0];

/// A mono float WAV of `passes`, each 4 s, sample `i` of a pass from `f(pass, i)`.
fn wav(f: &Fixture, name: &str, sample: impl Fn(usize, usize) -> f32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let path = f.0.join("assets").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for pass in 0..TAKES.len() {
        for i in 0..4 * RATE as usize {
            w.write_sample(sample(pass, i)).unwrap();
        }
    }
    w.finalize().unwrap();
}

/// The song, its Vox lane holding `clips`; each pass of the take plays `sample`.
fn song(clips: &str, sample: impl Fn(usize, usize) -> f32) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nconst vox = track(\"vox\", { name: \"Vox\" }, ({ lane }) => lane.audio(\"clips\"));\n\
         export default song(() => vox.out);\n",
    );
    f.write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    f.write("data/vox/clips.json", format!("{{\"clips\": [{clips}]}}\n"));
    wav(&f, "vox take.wav", sample);
    f.write("assets/vox take.wav.json", "{\"takes\": [0, 4, 8]}\n");
    f
}

/// The loop's clip, over 8 beats, playing the third pass.
const ONE: &str = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8}";

/// Each pass at its own level: 0.1, 0.2, 0.3.
fn levels(pass: usize, _: usize) -> f32 {
    0.1 * (pass + 1) as f32
}

fn vox() -> LaneAddr {
    LaneAddr::new("vox", "clips").unwrap()
}

fn clips(s: &Session) -> Vec<AudioClip> {
    match s.accepted().compiled.data.lane("vox", "clips").map(|d| (**d).clone()) {
        Some(LaneData::Audio(c)) => c,
        other => panic!("an audio lane: {other:?}"),
    }
}

/// Take `take` (from 0) over `from` to `to`, on the comp of clip `id`.
fn comp(id: &str, take: usize, from: f64, to: f64) -> Gesture {
    let clip = ClipRef { lane: vox(), id: Some(id.into()), at: None, sha: None };
    Gesture::Comp { clip, takes: TAKES.to_vec(), take, from, to }
}

fn go(s: &mut Session, g: Gesture) -> crate::clips::ClipsEdited {
    edit(s, &g).unwrap_or_else(|e| panic!("{g:?}: {e}"))
}

fn refused(s: &mut Session, g: Gesture) -> String {
    match edit(s, &g) {
        Err(e) => e.to_string(),
        Ok(done) => panic!("{g:?} was accepted: {done:?}"),
    }
}

/// Each clip's start, length, offset, fade in and fade out.
fn spans(s: &Session) -> Vec<[f64; 5]> {
    clips(s).iter().map(|c| [c.start, c.length, c.offset, c.fade_in, c.fade_out]).collect()
}

fn near(a: &[[f64; 5]], b: &[[f64; 5]]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.iter().zip(y).all(|(p, q)| (p - q).abs() < 1e-9))
}

/// The song, `seconds` of it, at the file's rate: its left channel.
fn render(s: &Session, seconds: f64) -> Vec<f32> {
    let rev = s.accepted();
    let out = m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, RATE).unwrap().render(seconds).unwrap();
    out.iter().map(|x| x[0]).collect()
}

/// The middle value of `x` from `a` to `b` seconds.
fn level(x: &[f32], a: f64, b: f64) -> f32 {
    let mut v: Vec<f32> = x[(a * RATE as f64) as usize..(b * RATE as f64) as usize].to_vec();
    v.sort_by(f32::total_cmp);
    v[v.len() / 2]
}

// VOX-1, VOX-3, VOX-4: Take 2 swiped over beats 2 to 4 plays there; the rest plays the third pass
// as before. Three plain clips, overlapping by 10 ms (0.02 beats at 120 bpm) with 10 ms fades, each
// reading the same place of its pass; the first keeps the clip's id. One Tx, and undo takes it back.
#[test]
fn a_swipe_plays_its_take_over_its_range_only() {
    let f = song(ONE, levels);
    let mut s = f.open();
    let before = clips(&s);
    go(&mut s, comp("vox00001", 1, 2.0, 4.0));
    let want = [[0.0, 2.01, 8.0, 0.0, 0.01], [1.99, 2.02, 4.995, 0.01, 0.01], [3.99, 4.01, 9.995, 0.01, 0.0]];
    assert!(near(&spans(&s), &want), "{:?}", spans(&s));
    let c = clips(&s);
    assert_eq!(c[0].id.as_deref(), Some("vox00001"), "the first keeps its id");
    assert!(c[1].id.is_some() && c[2].id.is_some() && c[1].id != c[2].id, "the new pieces get their own");
    assert!(c.iter().all(|c| c.asset == "vox take.wav"), "all of one file");
    let out = render(&s, 4.0);
    assert!((level(&out, 0.1, 0.9) - 0.3).abs() < 1e-6, "the third pass before the range");
    assert!((level(&out, 1.1, 1.9) - 0.2).abs() < 1e-6, "Take 2 over beats 2 to 4 (1 s to 2 s)");
    assert!((level(&out, 2.1, 3.9) - 0.3).abs() < 1e-6, "the third pass after it");
    s.undo().unwrap();
    assert_eq!(clips(&s), before, "one undo step takes it all back");
}

// VOX-4: the crossfades add up to 1. With every pass the same sound, a comp plays the same sound as
// the clip it was made from, through each join.
#[test]
fn the_joins_crossfade_to_one() {
    let sine = |_: usize, i: usize| (0.5 * (std::f64::consts::TAU * 220.0 * i as f64 / RATE as f64).sin()) as f32;
    let f = song(ONE, sine);
    let mut s = f.open();
    let plain = render(&s, 4.0);
    go(&mut s, comp("vox00001", 0, 1.0, 3.0));
    go(&mut s, comp("vox00001", 1, 2.0, 5.0));
    assert_eq!(clips(&s).len(), 4, "three joins: {:?}", spans(&s));
    let comped = render(&s, 4.0);
    let worst = plain.iter().zip(&comped).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    assert!(worst < 1e-5, "no dip or bump at a join: {worst}");
}

// VOX-5: swiping the third pass back over the range leaves the one clip it started as; comping the
// take that already plays there changes nothing; a piece a comp doesn't reach keeps its line.
#[test]
fn a_comp_back_to_one_take_is_one_clip_again() {
    let f = song(ONE, levels);
    let mut s = f.open();
    let was = clips(&s);
    let done = go(&mut s, comp("vox00001", 2, 5.0, 6.0));
    assert_eq!(done.edited.before, "no change", "the take that plays there already");
    go(&mut s, comp("vox00001", 1, 2.0, 4.0));
    let first = clips(&s)[0].clone();
    go(&mut s, comp("vox00001", 0, 6.0, 7.0));
    assert_eq!(clips(&s).len(), 5, "{:?}", spans(&s));
    assert_eq!(clips(&s)[0], first, "the piece before beat 2 is as it was");
    // Back to the third pass, over everything comped: one clip again, as it was.
    go(&mut s, comp("vox00001", 2, 1.0, 7.5));
    assert_eq!(clips(&s), was, "one clip, every field as before");
}

// VOX-2: a comp is the clips of one file in a row. A copy further on is another comp, which a swipe
// leaves alone; the range is cut to the comp; a range outside it is refused; a cut within 10 ms of
// the comp's edge moves onto it.
#[test]
fn a_comp_is_the_clips_of_one_file_in_a_row() {
    let copy = "{\"id\": \"vox00002\", \"start\": 16, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8}";
    let f = song(&format!("{ONE}, {copy}"), levels);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 0, 6.0, 20.0));
    let c = clips(&s);
    assert_eq!(c.len(), 3, "{:?}", spans(&s));
    assert!((c[1].start + c[1].length - 8.0).abs() < 1e-12, "cut to the comp's end: {:?}", spans(&s));
    assert_eq!(c[2].id.as_deref(), Some("vox00002"), "the copy is another comp");
    assert_eq!((c[2].start, c[2].length, c[2].offset), (16.0, 8.0, 8.0), "left alone");
    let why = refused(&mut s, comp("vox00001", 0, 9.0, 12.0));
    assert!(why.contains("nothing of"), "{why}");
    // 0.01 beats is 5 ms from the comp's start: the cut moves onto it.
    go(&mut s, comp("vox00002", 1, 16.01, 18.0));
    let c: Vec<_> = clips(&s).into_iter().filter(|c| c.start >= 16.0 - 0.1).collect();
    assert_eq!(c.len(), 2, "no piece of 5 ms: {:?}", c.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>());
    assert_eq!((c[0].start, c[0].offset), (16.0, 4.0), "Take 2 from the comp's start");
}

// VOX-6: a clip that plays backwards, or whose pitch is changed, isn't comped; nor is a range that
// isn't two beats in order, or a take its file doesn't have.
#[test]
fn a_comp_refuses_what_it_cant_cut() {
    let back = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8, \"reverse\": true}";
    let f = song(back, levels);
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00001", 0, 2.0, 4.0));
    assert!(why.contains("backwards"), "{why}");
    let pitched = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8, \"pitch\": 2}";
    f.write("data/vox/clips.json", format!("{{\"clips\": [{pitched}]}}\n"));
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00001", 0, 2.0, 4.0));
    assert!(why.contains("pitch"), "{why}");
    f.write("data/vox/clips.json", format!("{{\"clips\": [{ONE}]}}\n"));
    let mut s = f.open();
    for (from, to) in [(4.0, 2.0), (2.0, 2.0), (f64::NAN, 2.0)] {
        let why = refused(&mut s, comp("vox00001", 0, from, to));
        assert!(why.contains("after"), "{from} to {to}: {why}");
    }
    let why = refused(&mut s, comp("vox00001", 3, 2.0, 4.0));
    assert!(why.contains("1 to 3"), "{why}");
    assert_eq!(clips(&s).len(), 1, "nothing changed");
}

// VOX-3: a piece that follows the tempo reads by its warp markers, which move with it: Take 2 over
// beats 2 to 4 plays there.
#[test]
fn a_comp_that_follows_the_tempo_moves_its_markers() {
    let warped =
        "{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8, \"warp\": [[8, 0], [12, 8]]}";
    let f = song(warped, levels);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 1, 2.0, 4.0));
    let c = clips(&s);
    assert_eq!(c.len(), 3, "{:?}", spans(&s));
    // At 120 bpm, beat b of the song is b / 2 seconds into a pass.
    let reads = |c: &AudioClip, pass: f64| c.warp.iter().all(|m| (m.0 - (pass + (m.1 + c.start) * 0.5)).abs() < 1e-9);
    assert!(reads(&c[1], 4.0), "Take 2's moments: {:?}", c[1].warp);
    assert!(reads(&c[2], 8.0) && reads(&c[0], 8.0), "the third's: {:?}", (&c[0].warp, &c[2].warp));
    let out = render(&s, 4.0);
    assert!((level(&out, 1.1, 1.9) - 0.2).abs() < 1e-3, "Take 2 plays there: {}", level(&out, 1.1, 1.9));
    assert!((level(&out, 2.1, 3.9) - 0.3).abs() < 1e-3, "the third after it: {}", level(&out, 2.1, 3.9));
}

/// The lines of the lane file, one per clip.
fn lines(f: &Fixture) -> Vec<String> {
    f.read("data/vox/clips.json").lines().filter(|l| l.contains("\"asset\"")).map(|l| l.trim().trim_end_matches(',').to_string()).collect()
}

/// Clip `id` of the lane, as it reads.
fn by(s: &Session, id: &str) -> AudioClip {
    clips(s).into_iter().find(|c| c.id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no clip {id}"))
}

/// The line of clip `id` in the lane file.
fn line(f: &Fixture, id: &str) -> String {
    lines(f).into_iter().find(|l| l.contains(&format!("\"id\": \"{id}\""))).unwrap_or_else(|| panic!("no clip {id}: {:?}", lines(f)))
}

// VOX-5 (review P1): two pieces that follow the tempo at different speeds, meeting at beat 4. A
// comp over beats 0 to 1 changes nothing from beat 1 on: what beat 3 and beat 6 read stays, and
// the second piece keeps its line byte for byte.
#[test]
fn a_comp_leaves_what_its_range_doesnt_reach() {
    let a = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 8, \"warp\": [[8, 0], [9, 2]]}";
    let b =
        "{\"id\": \"vox00002\", \"start\": 4, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 10, \"warp\": [[10.5, 2], [11, 4]]}";
    let f = song(&format!("{a}, {b}"), levels);
    let mut s = f.open();
    let t = s.accepted().compiled.data.tempo().clone();
    let b = by(&s, "vox00002");
    let was = clips(&s);
    let read = |c: &[AudioClip], beat: f64| {
        let c = c.iter().rev().find(|c| c.start <= beat && beat < c.start + c.length).unwrap();
        super::reads(c, beat, &t)
    };
    let (r3, r6) = (read(&was, 3.0), read(&was, 6.0));
    go(&mut s, comp("vox00001", 0, 0.0, 1.0));
    let now = clips(&s);
    assert!((read(&now, 3.0) - r3).abs() < 1e-9, "beat 3 reads {} s, as before {r3}", read(&now, 3.0));
    assert!((read(&now, 6.0) - r6).abs() < 1e-9, "beat 6 reads {} s, as before {r6}", read(&now, 6.0));
    assert_eq!(by(&s, "vox00002"), b, "the second piece as it was");
}

// VOX-4, VOX-5 (review P3): a split the musician made, with fades for a breath at the cut, or
// nudged 30 ms earlier, is left as it is by a comp away from it: its pieces keep their lines.
#[test]
fn a_comp_never_merges_or_remakes_the_musicians_split() {
    let a = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 8, \"fade_out\": 0.3}";
    let b = "{\"id\": \"vox00002\", \"start\": 4, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 10, \"fade_in\": 0.3}";
    let f = song(&format!("{a}, {b}"), levels);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 1, 7.0, 8.0));
    let first = line(&f, "vox00001");
    let plain = render(&s, 4.0);
    go(&mut s, comp("vox00001", 0, 6.0, 7.0));
    assert_eq!(line(&f, "vox00001"), first, "the piece before the split, byte for byte");
    assert!(
        clips(&s).iter().any(|c| c.id.as_deref() == Some("vox00002") && c.fade_in == 0.3),
        "the breath's fade in stays: {:?}",
        spans(&s)
    );
    let comped = render(&s, 4.0);
    assert!((comped[96_000 - 10] - plain[96_000 - 10]).abs() < 1e-6, "the dip at 2 s stays");
    // Nudged: the second half 30 ms earlier, its audio kept.
    let b = "{\"id\": \"vox00002\", \"start\": 3.94, \"length\": 4.06, \"asset\": \"vox take.wav\", \"offset\": 10}";
    f.write("data/vox/clips.json", format!("{{\"clips\": [{ONE}, {b}]}}\n").replace("\"length\": 8", "\"length\": 4"));
    drop(s);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 1, 6.0, 7.0));
    let first = line(&f, "vox00001");
    go(&mut s, comp("vox00001", 0, 5.0, 6.0));
    assert_eq!(line(&f, "vox00001"), first, "the nudged split stays as it was");
}

// VOX-4 (review P3): a cut inside a piece's own fade is refused in words.
#[test]
fn a_comp_never_cuts_inside_a_fade() {
    let one = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8, \"fade_in\": 2}";
    let f = song(one, levels);
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00001", 0, 1.0, 2.0));
    assert!(why.contains("fade"), "{why}");
    go(&mut s, comp("vox00001", 0, 5.0, 6.0));
    assert_eq!(clips(&s)[0].fade_in, 2.0, "outside the fade, the fade stays");
}

// VOX-2 (review P2, P14): a copy of the loop straight after the first is another comp: a swipe in
// the first leaves it as it is, and Take 1 plays at its start. A clip 40 ms over the one before, at
// the same place in its pass, is in the comp; one 60 ms over isn't.
#[test]
fn a_comp_keeps_one_passs_timeline() {
    let copy = "{\"id\": \"vox00002\", \"start\": 8, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8}";
    let f = song(&format!("{ONE}, {copy}"), levels);
    let mut s = f.open();
    let second = by(&s, "vox00002");
    go(&mut s, comp("vox00001", 1, 2.0, 3.0));
    assert_eq!(by(&s, "vox00002"), second, "the copy as it was");
    go(&mut s, comp("vox00002", 0, 8.0, 12.0));
    let c = clips(&s).into_iter().find(|c| c.id.as_deref() == Some("vox00002")).unwrap();
    assert_eq!((c.start, c.offset), (8.0, 0.0), "Take 1 from the copy's start");
    drop(s);
    for (over, same) in [(0.04, true), (0.06, false)] {
        // `over` seconds over the clip before (2 beats a second), reading the same place in its pass.
        let a = "{\"id\": \"vox00001\", \"start\": 0, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 8}";
        let b = format!(
            "{{\"id\": \"vox00002\", \"start\": {}, \"length\": {}, \"asset\": \"vox take.wav\", \"offset\": {}}}",
            4.0 - 2.0 * over,
            4.0 + 2.0 * over,
            10.0 - over
        );
        f.write("data/vox/clips.json", format!("{{\"clips\": [{a}, {b}]}}\n"));
        let mut s = f.open();
        let second = by(&s, "vox00002");
        go(&mut s, comp("vox00001", 0, 0.0, 8.0));
        assert_eq!(by(&s, "vox00002") != second, same, "{over} s over: in the comp {same}");
    }
}

// VOX-3 (review P5): the last pass, cut short by the stop, has no sound over beats 4 to 8: Take 3
// there is refused, saying where it stops.
#[test]
fn a_take_with_no_sound_there_is_refused() {
    let f = song(ONE, levels);
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(f.0.join("assets/vox take.wav"), spec).unwrap();
    for (pass, n) in [(0, 4 * RATE as usize), (1, 4 * RATE as usize), (2, 2 * RATE as usize)] {
        for i in 0..n {
            w.write_sample(levels(pass, i)).unwrap();
        }
    }
    w.finalize().unwrap();
    f.write(
        "data/vox/clips.json",
        "{\"clips\": [{\"id\": \"vox00001\", \"start\": 0, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 4}]}\n",
    );
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00001", 2, 6.0, 8.0));
    assert!(why.contains("Take 3 stops at bar 2"), "{why}");
    go(&mut s, comp("vox00001", 2, 1.0, 3.0));
    assert_eq!(clips(&s).len(), 3, "where it has sound, it plays");
}

// VOX-2 (review P7, P11, P14): a range under 10 ms is refused; so is one outside the comp, in
// bars as the window shows them; and a file with fewer than two takes.
#[test]
fn a_range_too_short_or_outside_is_refused_in_bars() {
    let f = song(ONE, levels);
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00001", 0, 2.0, 2.004));
    assert!(why.contains("too short"), "{why}");
    let why = refused(&mut s, comp("vox00001", 0, 9.0, 12.0));
    assert!(why.contains("bar 3"), "{why}");
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    let why = refused(&mut s, Gesture::Comp { clip, takes: vec![0.0], take: 0, from: 2.0, to: 4.0 });
    assert!(why.contains("no takes"), "{why}");
    assert_eq!(clips(&s).len(), 1, "nothing changed");
}

// VOX-2, VOX-6 (review P6): a reversed swell made from the take, ending where the vocal starts, is
// no piece of its comp: the vocal is comped, the swell left as it is.
#[test]
fn a_reversed_clip_beside_a_comp_isnt_part_of_it() {
    let swell = "{\"id\": \"vox00009\", \"start\": 0, \"length\": 4, \"asset\": \"vox take.wav\", \"offset\": 8, \"reverse\": true}";
    let main = "{\"id\": \"vox00001\", \"start\": 4, \"length\": 8, \"asset\": \"vox take.wav\", \"offset\": 8}";
    let f = song(&format!("{swell}, {main}"), levels);
    let mut s = f.open();
    let why = refused(&mut s, comp("vox00009", 2, 1.0, 2.0));
    assert!(why.contains("backwards"), "the swell itself isn't comped: {why}");
    let swell = by(&s, "vox00009");
    go(&mut s, comp("vox00001", 1, 6.0, 8.0));
    assert_eq!(by(&s, "vox00009"), swell, "the swell as it was");
    assert_eq!(clips(&s).len(), 4, "{:?}", spans(&s));
}

// VOX-1 (review P4): a comp goes from a revision; another writer's change to the lane after it
// refuses it, though the clip it names didn't change.
#[test]
fn a_comp_from_before_anothers_change_is_refused() {
    let f = song(ONE, levels);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 1, 2.0, 4.0));
    let base = s.accepted().number;
    go(&mut s, comp("vox00001", 0, 6.0, 7.0));
    let late = crate::clips::edit_since(&mut s, &comp("vox00001", 0, 5.0, 8.0), "tx-late", Some(base));
    assert!(matches!(late, Err(crate::EditError::Changed(_))), "{:?}", late.map(|d| d.edited));
}

// Review P13: the lane file has no float noise: 2.01, not 2.0099999999999998.
#[test]
fn a_comps_numbers_are_written_clean() {
    let f = song(ONE, levels);
    let mut s = f.open();
    go(&mut s, comp("vox00001", 1, 2.0, 4.0));
    let text = f.read("data/vox/clips.json");
    assert!(!text.contains("9999") && !text.contains("0000000"), "{text}");
}

// VOX-3: a pass is a whole number of samples, so the loop's beats may reach a fraction of a sample
// past it: a take there still plays.
#[test]
fn a_take_a_sample_short_of_the_loop_still_plays() {
    let f = song(ONE, levels);
    let mut s = f.open();
    let clip = ClipRef { lane: vox(), id: Some("vox00001".into()), at: None, sha: None };
    go(&mut s, Gesture::Comp { clip, takes: vec![0.0, 3.99999, 7.99998], take: 0, from: 6.0, to: 8.0 });
    assert_eq!(clips(&s).len(), 2, "{:?}", spans(&s));
}

//! The metronome on the live engine (gaps #19): where its clicks land, and that off it changes no
//! bit. Songs here are built from IR without TypeScript.

use std::collections::VecDeque;
use std::path::Path;

use m2_ir::{Ir, check};
use m2_song::{Arrangement, DataSet};
use serde_json::{Value, json};

use crate::engine::{CallbackStamp, Engine, Envelope, Msg, Retired};
use crate::{Cmd, PackageSet, Renderer, Transport};

const SR: u32 = 48_000;
/// A blip's length in frames: 30 ms.
const BLIP: usize = 1440;

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).expect("std loads");
    p
}

/// A 220 Hz sine at `level`: 0 is silent from its first frame (a sine times 0).
fn sine(level: f64) -> Ir {
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "song", "nodes": [
            {"id": "f", "op": "const", "value": 220, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "z", "op": "const", "value": level},
            {"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "z"}}],
         "exports": {"out": "out"}}]}))
    .unwrap()
}

fn arrangement(v: Value) -> Arrangement {
    Arrangement::read(&v.to_string()).unwrap()
}

fn at_bpm(bpm: f64) -> Arrangement {
    arrangement(json!({"tempo": [{"beat": 0, "bpm": bpm}]}))
}

/// `ir` in `arr`, as the offline render makes it: playing from 0.
fn renderer(ir: &Ir, arr: &Arrangement) -> Renderer {
    let pk = std_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    Renderer::new(&flat, &DataSet::with_lanes(arr.clone(), []), &pk, SR).unwrap()
}

/// An engine driven by hand, playing (or stopped at) 0.
struct Rig {
    e: Engine,
    inbox: VecDeque<Envelope>,
    seq: u64,
}

impl Rig {
    fn new(ir: &Ir, arr: &Arrangement, playing: bool, click: bool) -> Rig {
        let mut r = renderer(ir, arr);
        r.set_strict(false);
        r.set_transport(Transport::new(0, playing));
        let mut e = Engine::with(Box::new(r));
        e.set_click(click);
        Rig { e, inbox: VecDeque::new(), seq: 0 }
    }

    fn send(&mut self, msg: Msg) {
        self.seq += 1;
        self.inbox.push_back(Envelope { seq: self.seq, msg });
    }

    fn run(&mut self, frames: usize) -> Vec<[f32; 2]> {
        let mut out = vec![[0.0; 2]; frames];
        let inbox = &mut self.inbox;
        self.e.process(
            &mut out,
            &mut || inbox.pop_front(),
            &mut |r: Retired| {
                drop(r);
                Ok(())
            },
            CallbackStamp::default(),
        );
        out
    }
}

fn frames(seconds: f64) -> usize {
    (seconds * SR as f64).round() as usize
}

/// Where blips start in a silent song: a frame of exact silence after at least 64 more, with sound
/// right after it (a blip's sine starts at 0).
fn onsets(x: &[[f32; 2]]) -> Vec<usize> {
    let mut found = vec![];
    let mut quiet = 64;
    for f in 0..x.len().saturating_sub(1) {
        if x[f][0] == 0.0 {
            if quiet >= 64 && x[f + 1][0] != 0.0 {
                found.push(f);
            }
            quiet += 1;
        } else {
            quiet = 0;
        }
    }
    found
}

/// A blip's peak, and how often its first 10 ms cross zero.
fn blip(x: &[[f32; 2]], at: usize) -> (f32, usize) {
    let b = &x[at..(at + BLIP).min(x.len())];
    let peak = b.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
    let ten = &b[1..480.min(b.len())];
    (peak, ten.windows(2).filter(|w| (w[0][0] > 0.0) != (w[1][0] > 0.0)).count())
}

/// The onsets that are accents: louder (−12 dBFS against −16) and higher (1,500 Hz against 1,000).
fn accents(x: &[[f32; 2]]) -> Vec<usize> {
    let mut v = vec![];
    for at in onsets(x) {
        let (peak, crossings) = blip(x, at);
        let accent = peak > 0.2;
        assert_eq!(accent, crossings > 25, "a blip at {at} is loud or high but not both: peak {peak}, {crossings} crossings");
        assert!(peak > 0.1 && peak < 0.26, "a blip at {at} peaks at {peak}");
        if accent {
            v.push(at);
        }
    }
    v
}

fn beats(of: &[f64], seconds_per_beat: f64) -> Vec<usize> {
    of.iter().map(|b| frames(b * seconds_per_beat)).collect()
}

#[test]
fn the_click_sounds_on_each_beat_louder_on_the_bar() {
    // 120 bpm in 4/4, two bars and the next downbeat, over a silent song.
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), true, true);
    let out = rig.run(frames(4.25));
    let every: Vec<f64> = (0..=8).map(f64::from).collect();
    assert_eq!(onsets(&out), beats(&every, 0.5), "a click on each beat, at its frame");
    assert_eq!(accents(&out), beats(&[0.0, 4.0, 8.0], 0.5), "the bar's first beat is the accent");
    // Silence between the blips.
    for at in beats(&every, 0.5) {
        let end = (at + 24_000).min(out.len());
        assert!(out[at + BLIP..end].iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "a blip at {at} lasts 30 ms");
        assert_eq!(out[at + 1][0], out[at + 1][1], "the same in both channels");
    }
}

#[test]
fn the_click_follows_the_meter_and_a_tempo_change() {
    // 3/4 at 120 bpm for two bars, then 6/8 at 60 bpm: quarters, then eighths, each bar's first
    // the accent.
    let arr = arrangement(json!({
        "tempo": [{"beat": 0, "bpm": 120}, {"beat": 6, "bpm": 60}],
        "meter": [{"beat": 0, "beats": 3, "unit": 4}, {"beat": 6, "beats": 6, "unit": 8}]}));
    let mut rig = Rig::new(&sine(0.0), &arr, true, true);
    let out = rig.run(frames(6.2));
    // Beats 0 to 6 are half a second apart; then eighths, half a second apart at 60 bpm.
    let expected: Vec<usize> = (0..=6).map(|b| frames(b as f64 * 0.5)).chain((1..=6).map(|k| frames(3.0 + k as f64 * 0.5))).collect();
    assert_eq!(onsets(&out), expected);
    // Bars start at beats 0 and 3 (3/4), then 6 and 9 (6/8 is three quarter notes a bar).
    assert_eq!(accents(&out), vec![frames(0.0), frames(1.5), frames(3.0), frames(6.0)]);
}

#[test]
fn the_click_lands_on_a_loop_wrap() {
    // A loop of beats 1 to 3 at 120 bpm: frames 24,000 to 72,000. The wrap lands on beat 1, mid
    // block (72,000 is 562.5 blocks), and clicks it once; the loop's end, beat 3, never clicks.
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), true, true);
    rig.send(Msg::Transport(Cmd::Loop(Some((1.0, 3.0)))));
    let out = rig.run(frames(3.1));
    assert_eq!(onsets(&out), vec![0, 24_000, 48_000, 72_000, 96_000, 120_000, 144_000]);
    assert_eq!(accents(&out), vec![0], "beat 1 is no bar's first");
}

#[test]
fn no_click_while_stopped_or_held() {
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, true);
    assert!(rig.run(frames(1.0)).iter().all(|f| f[0] == 0.0), "stopped: no click");
    rig.send(Msg::Transport(Cmd::Play));
    let played = rig.run(frames(0.25));
    assert_eq!(onsets(&played), vec![0], "Play clicks the beat it starts on");
    // Held for an export: the output fades out over 5 ms, then nothing, though a beat comes.
    rig.send(Msg::Hold(true));
    let held = rig.run(frames(2.0));
    assert!(rig.e.is_held());
    assert!(held[600..].iter().all(|f| f[0] == 0.0 && f[1] == 0.0), "held: no click");
}

#[test]
fn the_click_off_changes_no_bit() {
    let (ir, arr) = (sine(0.5), at_bpm(120.0));
    let n = frames(2.0);
    let export = renderer(&ir, &arr).render(2.0).unwrap();
    assert_eq!(export.len(), n);
    let same = |a: &[[f32; 2]], b: &[[f32; 2]]| {
        a.iter().zip(b).position(|(x, y)| x[0].to_bits() != y[0].to_bits() || x[1].to_bits() != y[1].to_bits())
    };
    // Never on: the export's bits.
    let mut never = Rig::new(&ir, &arr, true, false);
    assert_eq!(same(&never.run(n), &export), None, "off, the engine plays the export");
    // On for two beats, then off: the blips differ, and from the last one's end every bit is the
    // export's again.
    let mut once = Rig::new(&ir, &arr, true, true);
    let mut out = once.run(frames(0.6));
    once.e.set_click(false);
    out.extend(once.run(n - out.len()));
    assert!(same(&out[..BLIP], &export[..BLIP]).is_some(), "on, it clicks");
    assert_eq!(same(&out[24_000 + BLIP..], &export[24_000 + BLIP..]), None, "off again, not a bit changes");
}

// Lane B: the count-in. One bar of clicks from the engine's own frames, the transport stopped,
// then the song plays from where it stood, at the block line after the bar.

#[test]
fn a_count_in_clicks_one_bar_then_plays() {
    // 120 bpm in 4/4, stopped at bar 3 (beat 8, frame 192,000), the metronome on: four clicks
    // half a second apart from the first, the first accented; then the song plays from beat 8 at
    // frame 96,000 (750 blocks: a block line), and its downbeat clicks there, accented.
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, true);
    rig.send(Msg::Transport(Cmd::Seek(8.0)));
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
    let mut out = rig.run(96_000);
    let st = rig.e.status();
    assert!(st.playing, "playing once the bar is over");
    assert_eq!(st.pos, 192_000, "from the start point, not a frame on");
    out.extend(rig.run(24_000));
    assert_eq!(onsets(&out), vec![0, 24_000, 48_000, 72_000, 96_000], "four clicks, then the song's first beat a bar after the first");
    assert_eq!(accents(&out), vec![0, 96_000], "the count's first click, and the song's downbeat");
    assert_eq!(rig.e.status().pos, 192_000 + 24_000, "the song moves from then on");
}

#[test]
fn a_count_in_follows_the_meter() {
    // 3/4 at 120 bpm: three quarters, 72,000 frames, which is 562.5 blocks; so the first click
    // comes 64 frames into the first block, and the bar ends on the line at 72,064.
    let arr = arrangement(json!({"tempo": [{"beat": 0, "bpm": 120}], "meter": [{"beat": 0, "beats": 3, "unit": 4}]}));
    let mut rig = Rig::new(&sine(0.0), &arr, false, false);
    rig.send(Msg::CountIn { beats: 3, frames_per_beat: 24_000.0 });
    let out = rig.run(72_064);
    assert_eq!(onsets(&out), vec![64, 24_064, 48_064]);
    assert!(rig.e.status().playing && rig.e.status().pos == 0, "plays from the block line after the bar");
    // 6/8 counts six eighths: 12,000 frames apart at 120 quarter notes a minute; the same bar.
    let mut rig = Rig::new(&sine(0.0), &arr, false, false);
    rig.send(Msg::CountIn { beats: 6, frames_per_beat: 12_000.0 });
    let out = rig.run(72_064);
    assert_eq!(onsets(&out), (0..6).map(|k| 64 + k * 12_000).collect::<Vec<_>>());
    assert!(rig.e.status().playing);
    // A fraction of a frame between clicks: click k at round(k × step).
    let mut rig = Rig::new(&sine(0.0), &arr, false, false);
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 20_000.4 });
    let out = rig.run(80_128);
    let bar = 80_002;
    let pad = 80_128 - bar;
    assert_eq!(onsets(&out), vec![pad, pad + 20_000, pad + 40_001, pad + 60_001]);
}

#[test]
fn the_count_accents_its_first_click() {
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, false);
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
    let out = rig.run(96_000);
    assert_eq!(onsets(&out).len(), 4);
    assert_eq!(accents(&out), vec![0], "higher and louder, as a bar's first beat");
    // The metronome off, the song plays on with no click.
    assert_eq!(onsets(&rig.run(48_000)), Vec::<usize>::new());
}

#[test]
fn stop_cancels_a_count_in() {
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, false);
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
    let mut out = rig.run(30_000);
    rig.send(Msg::Transport(Cmd::Stop));
    out.extend(rig.run(100_000));
    assert_eq!(onsets(&out), vec![0, 24_000], "no click after the stop");
    assert!(!rig.e.status().playing && rig.e.status().pos == 0, "and nothing plays");
    // A seek cancels it too, and a hold for an export.
    for cancel in [Msg::Transport(Cmd::Seek(2.0)), Msg::Hold(true)] {
        let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, false);
        rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
        let mut out = rig.run(30_000);
        rig.send(cancel);
        out.extend(rig.run(70_000));
        rig.send(Msg::Hold(false));
        out.extend(rig.run(30_000));
        assert_eq!(onsets(&out), vec![0, 24_000]);
        assert!(!rig.e.status().playing);
    }
}

#[test]
fn play_while_counting_cancels_the_count() {
    // Space during the count-in: the song plays at once, the count's clicks stop, and its end
    // sends no second Play.
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, false);
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
    let mut out = rig.run(30_000);
    rig.send(Msg::Transport(Cmd::Play));
    out.extend(rig.run(30_000));
    assert!(rig.e.status().playing);
    rig.send(Msg::Transport(Cmd::Stop));
    out.extend(rig.run(70_000));
    assert_eq!(onsets(&out), vec![0, 24_000], "no click after the Play");
    assert!(!rig.e.status().playing, "stopped stays stopped: the count's end plays nothing");
}

#[test]
fn a_count_in_wakes_an_idle_engine() {
    let mut rig = Rig::new(&sine(0.0), &at_bpm(120.0), false, false);
    assert!(rig.run(48_000).iter().all(|f| f[0] == 0.0));
    assert!(rig.e.is_idle(), "stopped and silent: idle");
    rig.send(Msg::CountIn { beats: 4, frames_per_beat: 24_000.0 });
    let out = rig.run(96_000);
    assert_eq!(onsets(&out), vec![0, 24_000, 48_000, 72_000], "every click, though silence lies between them");
    assert!(rig.e.status().playing);
}

// Guard: with no count-in, or one cancelled before its first click, the engine plays the export's
// bits.
#[test]
fn no_count_in_changes_no_bit() {
    let (ir, arr) = (sine(0.5), at_bpm(120.0));
    let n = frames(2.0);
    let export = renderer(&ir, &arr).render(2.0).unwrap();
    let same = |a: &[[f32; 2]], b: &[[f32; 2]]| {
        a.iter().zip(b).position(|(x, y)| x[0].to_bits() != y[0].to_bits() || x[1].to_bits() != y[1].to_bits())
    };
    let mut never = Rig::new(&ir, &arr, true, false);
    assert_eq!(same(&never.run(n), &export), None);
    // 3/4: the first click would come 64 frames in; a Play read in the same block cancels it.
    let mut cancelled = Rig::new(&ir, &arr, false, false);
    cancelled.send(Msg::CountIn { beats: 3, frames_per_beat: 24_000.0 });
    cancelled.send(Msg::Transport(Cmd::Play));
    assert_eq!(same(&cancelled.run(n), &export), None);
}

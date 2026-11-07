//! Native tests of the pack's note effects, each named by the rule it checks (PACK-29 to PACK-33):
//! the Arpeggiator's every rule in the events note's §5, Chord's shapes in and out of a key, Note
//! echo's echoes on the beat, Lock to key's nearest note and shift; then what all four keep: the
//! controllers passed, no note the guard would drop, nothing taken while they play, a reset that
//! starts over. Chord against the roll's own chord stamps is in core/runtime, which has the table
//! (`the_chord_is_the_rolls_stamp_in_every_seven_note_key`).
//!
//! Each runs a kernel as the host does: whole blocks of 128 frames, its notes as a lane gives them
//! (at one frame the controllers, then the Offs, then the Ons), its time inputs frame by frame, at
//! 120 bpm and 48 kHz (a beat is 24,000 frames, a sixteenth 6,000) unless a test says otherwise.

use m2_dsp::testing::defaults_in;
use m2_kernel::{Cfg, Event, Host, Kernel, LIVE, live};

use super::*;

const MANIFEST: &str = include_str!("../../m2pkg.toml");
const SR: f32 = 48_000.0;
const BLOCK: u64 = 128;
/// A beat at 120 bpm, in frames.
const BEAT: u64 = 24_000;
/// A sixteenth (`1/16`, a step of a quarter of a beat) at 120 bpm.
const SIXTEENTH: u64 = BEAT / 4;
/// C major, as `time.key` gives it (VOX-12).
const C_MAJOR: f32 = (1 | 1 << 2 | 1 << 4 | 1 << 5 | 1 << 7 | 1 << 9 | 1 << 11) as f32;
/// C major pentatonic (C D E G A) and C blues (C E♭ F F♯ G B♭).
const C_PENTATONIC: f32 = (1 | 1 << 2 | 1 << 4 | 1 << 7 | 1 << 9) as f32;
const C_BLUES: f32 = (1 | 1 << 3 | 1 << 5 | 1 << 6 | 1 << 7 | 1 << 10) as f32;
const EVERY: f32 = 4095.0;

const ARP: [&str; 10] = ["notes", "beats", "playing", "tempo", "style", "rate", "octaves", "gate", "hold", "swing"];
const CHORD: [&str; 5] = ["notes", "key", "shape", "in_key", "inversion"];
const ECHO: [&str; 8] = ["notes", "beats", "playing", "tempo", "time", "repeats", "fall", "pitch"];
const LOCK: [&str; 3] = ["notes", "key", "shift"];

// ---------------------------------------------------------------- a run, as the host plays one

/// A note of a run's input: from frame `on` to frame `off` of the run, at `pitch`, as hard as
/// `velocity`; a clip's, or a key played live, whose id has the live bit (RENDER-14).
#[derive(Clone, Copy, Debug)]
struct N {
    on: u64,
    off: u64,
    pitch: f32,
    velocity: f32,
    live: bool,
}

fn n(on: u64, off: u64, pitch: f32) -> N {
    N { on, off, pitch, velocity: 0.8, live: false }
}

/// `notes` as keys played live.
fn keys(notes: &[N]) -> Vec<N> {
    notes.iter().map(|x| N { live: true, ..*x }).collect()
}

/// What a kernel gave, each at its frame from the run's start, with the kernel's own id.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Gave {
    On { at: u64, id: u32, pitch: f32, velocity: f32 },
    Off { at: u64, id: u32 },
    Midi { at: u64, bytes: [u8; 3] },
}

/// The song at a run's frame: whether it plays, and its beat.
type Transport<'a> = &'a dyn Fn(u64) -> (bool, f64);

/// Playing from beat 0 at 120 bpm.
fn from_0(f: u64) -> (bool, f64) {
    (true, f as f64 / BEAT as f64)
}

/// Stopped at beat 0.
fn stopped(_: u64) -> (bool, f64) {
    (false, 0.0)
}

/// A run of kernel `K`, named `kernel` in the manifest with ports `names`: `set` gives some of them
/// values (the others play their defaults), `song` says where the song is at each frame, and the
/// notes and controllers come as a lane gives them. Its seed is `values[0]`.
struct Run<'a> {
    kernel: &'a str,
    names: &'a [&'a str],
    set: Vec<(&'a str, f32)>,
    song: Transport<'a>,
    tempo: f32,
    notes: Vec<N>,
    midi: Vec<(u64, [u8; 3])>,
    frames: u64,
    seed: f64,
}

impl<'a> Run<'a> {
    fn new(kernel: &'a str, names: &'a [&'a str], frames: u64) -> Self {
        Run { kernel, names, set: vec![], song: &from_0, tempo: 120.0, notes: vec![], midi: vec![], frames, seed: 1.0 }
    }

    fn with(mut self, name: &'a str, v: f32) -> Self {
        self.set.push((name, v));
        self
    }

    fn notes(mut self, notes: &[N]) -> Self {
        self.notes = notes.to_vec();
        self
    }

    fn song(mut self, song: Transport<'a>) -> Self {
        self.song = song;
        self
    }
}

/// A host for `K`, its seed `seed`, its notes output roomy.
fn host<K: Kernel>(seed: f64) -> Host<K> {
    let values: &'static [f64] = Box::leak(vec![seed].into_boxed_slice());
    let mut h = Host::<K>::of(Cfg { values, ..Cfg::new(SR, BLOCK as usize, 1) });
    h.gives(0, 4096);
    h
}

/// The events of block `b`'s frames, as a lane gives them: the notes' ids are their place + 1, with
/// the live bit for a key played live.
fn block_events(r: &Run, b: u64) -> Vec<Event> {
    let (from, to) = (b * BLOCK, (b + 1) * BLOCK);
    let mut ev: Vec<(u64, u8, Event)> = vec![];
    for &(at, bytes) in &r.midi {
        if (from..to).contains(&at) {
            ev.push((at, 0, Event::Midi { frame: (at - from) as usize, bytes }));
        }
    }
    for (i, x) in r.notes.iter().enumerate() {
        let id = (i as u32 + 1) | if x.live { LIVE } else { 0 };
        if (from..to).contains(&x.off) {
            ev.push((x.off, 1, Event::Off { frame: (x.off - from) as usize, id, velocity: 0.0 }));
        }
        if (from..to).contains(&x.on) {
            ev.push((x.on, 2, Event::On { frame: (x.on - from) as usize, id, pitch: x.pitch, velocity: x.velocity, from_pitch: None }));
        }
    }
    ev.sort_by_key(|(at, kind, _)| (*at, *kind));
    ev.into_iter().map(|(_, _, e)| e).collect()
}

/// Plays one block of `r` on `h`: its events and inputs written, called, what it gave read, each at
/// its frame of the run.
fn play_block<K: Kernel>(h: &mut Host<K>, r: &Run, b: u64, out: &mut Vec<Gave>) {
    let defaults = defaults_in(MANIFEST, r.kernel, r.names);
    h.events(0, &block_events(r, b));
    for (p, name) in r.names.iter().enumerate().skip(1) {
        let fixed = r.set.iter().rev().find(|(k, _)| k == name).map(|(_, v)| *v).unwrap_or(defaults[p]);
        for k in 0..BLOCK as usize {
            let (playing, beats) = (r.song)(b * BLOCK + k as u64);
            h.port_mut(p)[k] = match *name {
                "beats" => beats as f32,
                "playing" => playing as u32 as f32,
                "tempo" => r.tempo,
                _ => fixed,
            };
        }
    }
    h.process(BLOCK as u32);
    for e in h.given(0) {
        let at = b * BLOCK + e.frame() as u64;
        out.push(match e {
            Event::On { id, pitch, velocity, .. } => Gave::On { at, id, pitch, velocity },
            Event::Off { id, .. } => Gave::Off { at, id },
            Event::Midi { bytes, .. } => Gave::Midi { at, bytes },
            other => panic!("it gave {other:?}"),
        });
    }
}

/// Everything `K` gave in the run's frames, and its meter at the end of each block (when it has one).
fn run<K: Kernel>(r: &Run) -> (Vec<Gave>, Vec<f32>) {
    let mut h = host::<K>(r.seed);
    let (mut out, mut meter) = (vec![], vec![]);
    for b in 0..r.frames.div_ceil(BLOCK) {
        play_block(&mut h, r, b, &mut out);
        if K::OUTPUTS > 1 {
            meter.push(h.port_mut(K::INPUTS + 1)[BLOCK as usize - 1]);
        }
    }
    let at = |e: &Gave| match *e {
        Gave::On { at, .. } | Gave::Off { at, .. } | Gave::Midi { at, .. } => at,
    };
    out.retain(|e| at(e) < r.frames);
    (out, meter)
}

/// Each On it gave: its frame and pitch.
fn ons(g: &[Gave]) -> Vec<(u64, f32)> {
    g.iter().filter_map(|e| if let Gave::On { at, pitch, .. } = e { Some((*at, *pitch)) } else { None }).collect()
}

/// The pitches of the Ons it gave, in order.
fn pitches(g: &[Gave]) -> Vec<f32> {
    ons(g).into_iter().map(|(_, p)| p).collect()
}

/// Each note it gave as (start, end, pitch): an On and the Off of its id after it.
fn spans(g: &[Gave]) -> Vec<(u64, Option<u64>, f32)> {
    g.iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let Gave::On { at, id, pitch, .. } = *e else { return None };
            let end = g[i + 1..].iter().find_map(|x| match *x {
                Gave::Off { at, id: o } if o == id => Some(at),
                _ => None,
            });
            Some((at, end, pitch))
        })
        .collect()
}

/// Every On it gave is a note the guard plays, and every note it started it ended by the run's end
/// (a note an Off never ends would hang until the guard ended it).
fn all_playable_and_ended(g: &[Gave]) {
    for e in g {
        if let Gave::On { pitch, velocity, .. } = e {
            assert!((0.0..=127.0).contains(pitch) && (0.0..=1.0).contains(velocity), "{e:?}");
        }
    }
    for (at, end, p) in spans(g) {
        assert!(end.is_some_and(|e| e > at), "the note at {at} ({p}) never ended");
    }
}

/// C, E and G (C4, E4, G4) held from frame `on` to frame `off`.
fn c_major(on: u64, off: u64) -> Vec<N> {
    vec![n(on, off, 60.0), n(on, off, 64.0), n(on, off, 67.0)]
}

/// The Arpeggiator over `notes`, with `set`, for `frames`, playing from beat 0.
fn arp(notes: &[N], set: &[(&'static str, f32)], frames: u64) -> Vec<Gave> {
    let mut r = Run::new("arp", &ARP, frames).notes(notes);
    r.set = set.to_vec();
    run::<Arp>(&r).0
}

// ---------------------------------------------------------------- the Arpeggiator (PACK-30)

// PACK-30, Order: Up by pitch; Down backwards, from the top octave down; Up and down without the
// top or the bottom twice; each octave up `octaves` times. A step each sixteenth, on the grid.
#[test]
fn the_order_is_by_style_and_octaves() {
    let held = c_major(0, 6 * BEAT);
    let frames = 24 * SIXTEENTH;
    let up = [60.0, 64.0, 67.0, 72.0, 76.0, 79.0];
    for (style, want) in [
        (0.0, up.to_vec()),
        (1.0, up.iter().rev().copied().collect::<Vec<_>>()),
        (2.0, vec![60.0, 64.0, 67.0, 72.0, 76.0, 79.0, 76.0, 72.0, 67.0, 64.0]),
    ] {
        let g = arp(&held, &[("style", style), ("octaves", 1.0)], frames);
        let o = ons(&g);
        assert_eq!(
            o.iter().map(|x| x.0).collect::<Vec<_>>(),
            (0..24).map(|k| k * SIXTEENTH).collect::<Vec<_>>(),
            "style {style}: a step each sixteenth"
        );
        let turn: Vec<f32> = want.iter().cycle().take(24).copied().collect();
        assert_eq!(pitches(&g), turn, "style {style}");
        all_playable_and_ended(&g);
    }
    // Four octaves of C E G, Up: twelve notes, then again.
    let g = arp(&held, &[("octaves", 3.0)], 13 * SIXTEENTH);
    assert_eq!(pitches(&g), [60.0, 64.0, 67.0, 72.0, 76.0, 79.0, 84.0, 88.0, 91.0, 96.0, 100.0, 103.0, 60.0]);
}

// PACK-30, Order: As played is by when each note started, notes that start on one frame by pitch;
// a note that joins later restarts the pattern (Retrigger) at the next step, past the catch.
#[test]
fn as_played_is_by_when_each_note_started() {
    let held = [n(0, 4 * BEAT, 67.0), n(0, 4 * BEAT, 64.0), n(2000, 4 * BEAT, 60.0)];
    let g = arp(&held, &[("style", 3.0), ("octaves", 1.0)], 8 * SIXTEENTH);
    // Step 0: E then G (one frame, by pitch), so E; C joins 2,000 frames in, past the catch: the
    // pattern restarts at step 1, E G C E' G' C'.
    assert_eq!(pitches(&g), [64.0, 64.0, 67.0, 60.0, 76.0, 79.0, 72.0, 64.0]);
}

// PACK-30, Which note: Random is the hash(seed, n − n₀) mod L th note of Up's order, so a step
// plays the same note each time the song passes it, and another seed picks others.
#[test]
fn random_picks_by_the_seed_and_the_step() {
    let held = c_major(0, 8 * BEAT);
    let up = [60.0, 64.0, 67.0, 72.0, 76.0, 79.0];
    let picked = |seed: f64| {
        let mut r = Run::new("arp", &ARP, 32 * SIXTEENTH).notes(&held).with("style", 4.0).with("octaves", 1.0);
        r.seed = seed;
        pitches(&run::<Arp>(&r).0)
    };
    let one = picked(1.0);
    let want: Vec<f32> = (0..32).map(|k| up[arpeggio::pick(1, k, up.len())]).collect();
    assert_eq!(one, want, "from Up's order, by the hash");
    assert_eq!(picked(1.0), one, "the same every time");
    assert_ne!(picked(2.0), one, "another seed picks others");
    let kinds: std::collections::BTreeSet<u32> = one.iter().map(|p| *p as u32).collect();
    assert!(kinds.len() >= 5, "it picks from every note: {one:?}");
}

// PACK-30, Steps: step n of the song's grid starts at n × rate from beat 0; a chord held from
// beat 0.6 (past the catch of a quarter-note step) starts at the next step, on beat 1.
#[test]
fn steps_land_on_the_songs_grid() {
    let held = c_major(14_400, 6 * BEAT);
    let g = arp(&held, &[("rate", 0.0)], 5 * BEAT);
    assert_eq!(ons(&g), [(BEAT, 60.0), (2 * BEAT, 64.0), (3 * BEAT, 67.0), (4 * BEAT, 60.0)]);
    // Eighths and thirty-seconds: every half and eighth of a beat.
    let g = arp(&c_major(0, 2 * BEAT), &[("rate", 1.0)], BEAT + 1);
    assert_eq!(ons(&g).iter().map(|x| x.0).collect::<Vec<_>>(), [0, BEAT / 2, BEAT]);
    let g = arp(&c_major(0, 2 * BEAT), &[("rate", 5.0)], BEAT / 2 + 1);
    assert_eq!(ons(&g).iter().map(|x| x.0).collect::<Vec<_>>(), (0..=4).map(|k| k * BEAT / 8).collect::<Vec<_>>());
    // Triplets: three to a beat, within a frame of where the grid puts them.
    let g = arp(&c_major(0, 2 * BEAT), &[("rate", 2.0)], BEAT + 1);
    let at: Vec<u64> = ons(&g).iter().map(|x| x.0).collect();
    assert_eq!(at.len(), 4);
    for (k, a) in at.iter().enumerate() {
        assert!(a.abs_diff(k as u64 * 8000) <= 1, "triplet {k} at {a}");
    }
}

// PACK-30, Retrigger: a note that joins restarts the pattern; the step it joins on is step 0.
#[test]
fn a_note_that_joins_restarts_the_pattern() {
    let mut held = c_major(0, 4 * BEAT);
    held.push(n(3 * SIXTEENTH, 4 * BEAT, 59.0));
    let g = arp(&held, &[], 8 * SIXTEENTH);
    assert_eq!(pitches(&g), [60.0, 64.0, 67.0, 59.0, 60.0, 64.0, 67.0, 59.0]);
}

// PACK-30, Catch: a note that joins within a quarter of its step after the step starts plays that
// step at once, at its own frame, ending the note the step was playing; later, it waits.
#[test]
fn a_note_just_late_plays_its_step_at_once_and_a_later_one_waits() {
    let g = arp(&[n(1000, BEAT, 60.0)], &[], 2 * SIXTEENTH + 1);
    assert_eq!(ons(&g), [(1000, 60.0), (SIXTEENTH, 60.0), (2 * SIXTEENTH, 60.0)], "1,000 frames late: at once");
    let g = arp(&[n(2000, BEAT, 60.0)], &[], 2 * SIXTEENTH + 1);
    assert_eq!(ons(&g), [(SIXTEENTH, 60.0), (2 * SIXTEENTH, 60.0)], "2,000 frames late, past 1,500: the next step");
    // C held; A3 joins 1,000 frames into step 1: step 1 plays again, from A3, and its C ends there.
    let g = arp(&[n(0, BEAT, 60.0), n(SIXTEENTH + 1000, BEAT, 57.0)], &[], 3 * SIXTEENTH + 1);
    assert_eq!(ons(&g), [(0, 60.0), (SIXTEENTH, 60.0), (SIXTEENTH + 1000, 57.0), (2 * SIXTEENTH, 60.0), (3 * SIXTEENTH, 57.0)]);
    let s = spans(&g);
    assert_eq!(s[1], (SIXTEENTH, Some(SIXTEENTH + 1000), 60.0), "the step's C ends as it plays again");
    // At least 2 frames: at 100,000 bpm a quarter of a thirty-second is under 2 frames.
    let fast = |f: u64| (true, f as f64 * 100_000.0 / 60.0 / 48_000.0);
    let late = 2; // frames after a grid line: 0.069 beats, past a quarter step (0.031) but within 2 frames
    let at = (0.125 * 60.0 * 48_000.0 / 100_000.0f64).ceil() as u64 + late - 1;
    let mut r = Run::new("arp", &ARP, at + 10).notes(&[n(at, at + 100, 60.0)]).with("rate", 5.0).song(&fast);
    r.tempo = 100_000.0;
    assert_eq!(ons(&run::<Arp>(&r).0).first().map(|x| x.0), Some(at), "within 2 frames of a line: at once");
}

// PACK-30, Gate: a step's note sounds gate × its step, from its step's start: under 1 choppy, over
// 1 overlapping the next; letting go of its key never cuts it.
#[test]
fn each_step_sounds_gate_times_its_step() {
    for (gate, len) in [(0.5, SIXTEENTH / 2), (0.8, 4800), (1.5, 9000), (2.0, 2 * SIXTEENTH)] {
        let g = arp(&c_major(0, BEAT), &[("gate", gate)], 2 * BEAT);
        let s = spans(&g);
        assert_eq!(s.len(), 4, "gate {gate}: four steps in a beat");
        for (k, (at, end, _)) in s.iter().enumerate() {
            assert_eq!((*at, *end), (k as u64 * SIXTEENTH, Some(k as u64 * SIXTEENTH + len)), "gate {gate}, step {k}");
        }
    }
    // Let go 1,000 frames into a step: its note still sounds its gate.
    let g = arp(&c_major(0, SIXTEENTH + 1000), &[], BEAT);
    assert_eq!(spans(&g).last().copied(), Some((SIXTEENTH, Some(SIXTEENTH + 4800), 64.0)));
}

// PACK-30, Steps: swing moves the second step of each pair later, as the roll's swing; each step's
// gate is of its own length.
#[test]
fn swing_moves_the_second_step_of_each_pair() {
    let g = arp(&c_major(0, BEAT), &[("swing", 0.6)], BEAT);
    // Pairs of 12,000 frames: the second step at 60 % of the pair, 7,200 in.
    let s = spans(&g);
    assert_eq!(s.iter().map(|x| x.0).collect::<Vec<_>>(), [0, 7200, 12_000, 19_200]);
    assert_eq!(s[0].1, Some(5760), "0.8 of its 7,200 frames");
    assert_eq!(s[1].1, Some(7200 + 3840), "0.8 of its 4,800");
    // 50 % is straight.
    let g = arp(&c_major(0, BEAT), &[("swing", 0.5)], BEAT);
    assert_eq!(ons(&g).iter().map(|x| x.0).collect::<Vec<_>>(), [0, 6000, 12_000, 18_000]);
}

// PACK-30, Velocity: each step plays as hard as its note.
#[test]
fn each_step_plays_as_hard_as_its_note() {
    let held = [N { velocity: 0.3, ..n(0, BEAT, 60.0) }, N { velocity: 0.6, ..n(0, BEAT, 64.0) }, N { velocity: 0.9, ..n(0, BEAT, 67.0) }];
    let g = arp(&held, &[], 3 * SIXTEENTH);
    let v: Vec<f32> = g.iter().filter_map(|e| if let Gave::On { velocity, .. } = e { Some(*velocity) } else { None }).collect();
    assert_eq!(v, [0.3, 0.6, 0.9]);
}

// PACK-30, One key held: repeated at the rate, note repeat.
#[test]
fn one_key_held_repeats_at_the_rate() {
    let g = arp(&[n(0, 2 * BEAT, 62.0)], &[("rate", 1.0)], 2 * BEAT);
    assert_eq!(ons(&g), (0..4).map(|k| (k * BEAT / 2, 62.0)).collect::<Vec<_>>());
}

// PACK-30, Stopped: the steps run on its own clock from the first key, at time.tempo; with no key
// held the clock stops, and the next key starts it again at 0.
#[test]
fn stopped_its_steps_run_at_the_tempo_from_the_first_key() {
    let held = [c_major(1000, 20_000), c_major(30_000, 45_000)].concat();
    let r = Run::new("arp", &ARP, 45_000).notes(&held).song(&stopped);
    let g = run::<Arp>(&r).0;
    assert_eq!(ons(&g), [(1000, 60.0), (7000, 64.0), (13_000, 67.0), (19_000, 60.0), (30_000, 60.0), (36_000, 64.0), (42_000, 67.0)]);
    // At 60 bpm a sixteenth is twice as long.
    let mut r = Run::new("arp", &ARP, 26_000).notes(&c_major(1000, 30_000)).song(&stopped);
    r.tempo = 60.0;
    assert_eq!(ons(&run::<Arp>(&r).0).iter().map(|x| x.0).collect::<Vec<_>>(), [1000, 13_000, 25_000]);
}

// PACK-30, Play pressed while keys are held: the note it plays ends, and the pattern restarts on the
// grid, its first step at the first grid line at or after where the song starts (within the catch,
// at once).
#[test]
fn play_pressed_while_keys_are_held_restarts_on_the_grid() {
    let at = 50_000;
    for (start, first) in [(0.0, at), (0.1, at + 3600)] {
        let song = move |f: u64| if f < at { (false, start) } else { (true, start + (f - at) as f64 / BEAT as f64) };
        let r = Run::new("arp", &ARP, at + 2 * SIXTEENTH).notes(&c_major(0, 2 * at)).song(&song);
        let g = run::<Arp>(&r).0;
        let before: Vec<Gave> = g.iter().filter(|e| matches!(e, Gave::On { at: a, .. } if *a < at)).copied().collect();
        assert_eq!(before.len(), 9, "stopped, a step each 6,000 frames from the first key");
        let after: Vec<(u64, f32)> = ons(&g).into_iter().filter(|x| x.0 >= at).collect();
        assert_eq!(after[0], (first, 60.0), "from beat {start}: step 0 of the pattern at {first}");
        let s = spans(&g);
        assert_eq!(s[8].1, Some(at), "the note it was playing ends at Play");
    }
}

// PACK-30, Play, a Stop, a seek: a loop's wrap (the beat going back) ends the note it plays and
// restarts the pattern on the grid, at once; so does a seek forward.
#[test]
fn a_seek_or_a_wrap_ends_its_note_and_restarts_on_the_grid() {
    let wrap = |f: u64| (true, if f < 30_000 { f } else { f - 30_000 } as f64 / BEAT as f64);
    let r = Run::new("arp", &ARP, 40_000).notes(&c_major(0, 2 * BEAT)).song(&wrap);
    let g = run::<Arp>(&r).0;
    assert_eq!(ons(&g), [(0, 60.0), (6000, 64.0), (12_000, 67.0), (18_000, 60.0), (24_000, 64.0), (30_000, 60.0), (36_000, 64.0)]);
    assert_eq!(spans(&g)[4].1, Some(28_800), "its gate ended it before the wrap");
    let short = Run::new("arp", &ARP, 40_000).notes(&c_major(0, 2 * BEAT)).song(&wrap).with("gate", 1.5);
    assert_eq!(spans(&run::<Arp>(&short).0)[4].1, Some(30_000), "the wrap ends the note it plays");
    // A seek forward by two beats, 100 frames past a grid line: at once (within the catch).
    let seek = |f: u64| {
        (true, if f < 20_000 { f as f64 } else { f as f64 + 2.0 * BEAT as f64 + 100.0 - (20_000 % SIXTEENTH) as f64 } / BEAT as f64)
    };
    let g = run::<Arp>(&Run::new("arp", &ARP, 22_000).notes(&c_major(0, 4 * BEAT)).song(&seek)).0;
    assert_eq!(ons(&g).last().copied(), Some((20_000, 60.0)), "{g:?}");
}

// PACK-30: a bar plays the same from any bar. Played from a seek to beat 2 and a little, the
// pattern of the chord that starts on beat 4 is the export's, note for note, at the same places.
#[test]
fn a_bar_plays_the_same_from_any_bar() {
    let chords =
        [c_major(0, 4 * BEAT), vec![n(4 * BEAT, 8 * BEAT, 57.0), n(4 * BEAT, 8 * BEAT, 60.0), n(4 * BEAT, 8 * BEAT, 64.0)]].concat();
    let export = arp(&chords, &[("octaves", 1.0), ("swing", 0.6)], 8 * BEAT);
    // From beat 2 and 37 frames: the run's frame f is the song's frame f + 2 beats + 37.
    let skip = 2 * BEAT + 37;
    let later: Vec<N> = chords.iter().filter(|x| x.on >= skip).map(|x| N { on: x.on - skip, off: x.off - skip, ..*x }).collect();
    let song = move |f: u64| (true, (f + skip) as f64 / BEAT as f64);
    let r = Run::new("arp", &ARP, 8 * BEAT - skip).notes(&later).with("octaves", 1.0).with("swing", 0.6).song(&song);
    let heard: Vec<(u64, f32)> = ons(&run::<Arp>(&r).0).into_iter().map(|(at, p)| (at + skip, p)).collect();
    let want: Vec<(u64, f32)> = ons(&export).into_iter().filter(|x| x.0 >= 4 * BEAT).collect();
    assert_eq!(want.len(), 16, "a step each sixteenth from beat 4: {want:?}");
    assert_eq!(heard, want);
}

// PACK-30, Hold: while the song is stopped, keys played live and let go keep playing until a new key
// (a new chord), and Play, a seek, a reset (Stop pressed again) or Hold turned off let go of them.
#[test]
fn hold_latches_keys_while_stopped_until_a_new_key_play_or_a_seek() {
    let held = keys(&[c_major(0, 10_000), vec![n(40_000, 45_000, 65.0)]].concat());
    // Stopped: C E G let go at 10,000 keep playing; F at 40,000 is a new chord; Play at 60,000 lets go.
    let song = |f: u64| if f < 60_000 { (false, 0.0) } else { (true, (f - 60_000) as f64 / BEAT as f64) };
    let r = Run::new("arp", &ARP, 80_000).notes(&held).with("hold", 1.0).song(&song);
    let o = ons(&run::<Arp>(&r).0);
    assert_eq!(o.iter().filter(|x| x.0 < 40_000).map(|x| x.1).collect::<Vec<_>>(), [60.0, 64.0, 67.0, 60.0, 64.0, 67.0, 60.0]);
    // F joins 4,000 frames into a step of the clock that runs on: past the catch, it starts at the next.
    let f: Vec<(u64, f32)> = o.iter().filter(|x| (40_000..60_000).contains(&x.0)).copied().collect();
    assert_eq!(f, [(42_000, 65.0), (48_000, 65.0), (54_000, 65.0)], "F alone, a new chord");
    assert!(o.iter().all(|x| x.0 < 60_000), "Play let go of it: {o:?}");
    // A seek while stopped lets go too; so does Hold turned off.
    let seek = |f: u64| (false, if f < 20_000 { 0.0 } else { 3.0 });
    let r = Run::new("arp", &ARP, 40_000).notes(&keys(&c_major(0, 10_000))).with("hold", 1.0).song(&seek);
    assert!(ons(&run::<Arp>(&r).0).iter().all(|x| x.0 < 20_000), "a seek let go");
    let mut h = host::<Arp>(1.0);
    let mut r = Run::new("arp", &ARP, 40_000).notes(&keys(&c_major(0, 10_000))).with("hold", 1.0).song(&stopped);
    let mut g = vec![];
    for b in 0..100 {
        play_block(&mut h, &r, b, &mut g);
    }
    r = r.with("hold", 0.0);
    play_block(&mut h, &r, 100, &mut g);
    let turned = g.len();
    for b in 101..300 {
        play_block(&mut h, &r, b, &mut g);
    }
    assert!(g[turned..].iter().all(|e| !matches!(e, Gave::On { .. })), "Hold off let go");
    // A reset (Stop pressed while stopped) forgets the latch.
    let mut h = host::<Arp>(1.0);
    let r = Run::new("arp", &ARP, 40_000).notes(&keys(&c_major(0, 10_000))).with("hold", 1.0).song(&stopped);
    let mut g = vec![];
    for b in 0..100 {
        play_block(&mut h, &r, b, &mut g);
    }
    h.reset();
    let kept = g.len();
    for b in 100..300 {
        play_block(&mut h, &r, b, &mut g);
    }
    assert!(g[kept..].is_empty(), "a reset let go: {:?}", &g[kept..]);
}

// PACK-30, Hold: a clip's notes never latch, so a clip's chord ends with its notes and an export
// never plays a latched chord.
#[test]
fn hold_never_latches_a_clips_notes() {
    let g = arp(&c_major(0, BEAT), &[("hold", 1.0)], 3 * BEAT);
    assert_eq!(ons(&g).len(), 4, "the chord's beat, and no more");
}

// PACK-30, Hold: while the song plays it latches keys played live: let go, they keep playing until a
// new key starts a new chord; a seek lets go of them.
#[test]
fn hold_latches_live_keys_while_the_song_plays() {
    let held = keys(&[c_major(0, BEAT), vec![n(2 * BEAT, 2 * BEAT + 1000, 65.0)]].concat());
    let g = arp(&held, &[("hold", 1.0)], 3 * BEAT);
    let o = ons(&g);
    let early: Vec<f32> = o.iter().filter(|x| x.0 < 2 * BEAT).map(|x| x.1).collect();
    assert_eq!(early, [60.0, 64.0, 67.0, 60.0, 64.0, 67.0, 60.0, 64.0], "C E G, latched past beat 1");
    let late: Vec<(u64, f32)> = o.iter().filter(|x| x.0 >= 2 * BEAT).copied().collect();
    assert_eq!(late, [(48_000, 65.0), (54_000, 65.0), (60_000, 65.0), (66_000, 65.0)], "F alone, a new chord, latched too");
    all_playable_and_ended(&g);
    let seek = |f: u64| (true, if f < BEAT + BEAT / 2 { f } else { f + 10 * BEAT } as f64 / BEAT as f64);
    let r = Run::new("arp", &ARP, 3 * BEAT).notes(&keys(&c_major(0, BEAT))).with("hold", 1.0).song(&seek);
    assert!(ons(&run::<Arp>(&r).0).iter().all(|x| x.0 < BEAT + BEAT / 2), "a seek let go");
}

// PACK-30, Hold: nothing latches at the frame where the song stops: keys played live and let go at
// a Stop's own frame are let go, not held.
#[test]
fn keys_let_go_at_a_stops_frame_are_not_latched() {
    let stop: u64 = 235 * BLOCK;
    let song = move |f: u64| if f < stop { (true, f as f64 / BEAT as f64) } else { (false, stop as f64 / BEAT as f64) };
    let r = Run::new("arp", &ARP, stop + 4 * BEAT).notes(&keys(&c_major(0, stop))).with("hold", 1.0).song(&song);
    let g = run::<Arp>(&r).0;
    assert!(ons(&g).iter().all(|x| x.0 < stop), "{:?}", ons(&g));
}

// PACK-30, ABI-15: each keeps a live key's bit on the notes it makes from it, so an Arpeggiator after
// it can hold them; the notes it makes from a clip's have none.
#[test]
fn each_keeps_the_live_bit() {
    for is_live in [false, true] {
        let notes = |ns: &[N]| if is_live { keys(ns) } else { ns.to_vec() };
        let runs = [
            ("arp", run::<Arp>(&Run::new("arp", &ARP, BEAT).notes(&notes(&c_major(0, BEAT)))).0),
            ("chord", run::<Chord>(&Run::new("chord", &CHORD, 30_000).notes(&notes(&[n(0, 20_000, 60.0)]))).0),
            ("echo", run::<Echo>(&Run::new("echo", &ECHO, 3 * BEAT).notes(&notes(&[n(0, 6000, 60.0)]))).0),
            ("lock", run::<LockToKey>(&Run::new("lock", &LOCK, 30_000).notes(&notes(&[n(0, 20_000, 61.0)])).with("key", C_MAJOR)).0),
        ];
        for (name, g) in runs {
            assert!(g.len() >= 2, "{name}");
            for e in &g {
                if let Gave::On { id, .. } | Gave::Off { id, .. } = *e {
                    assert_eq!(live(id), is_live, "{name}: {e:?}");
                }
            }
        }
    }
}

// PACK-30, Hold: nothing latches at the frame where the song stops. The lane ends a clip's chord at
// a Stop's own frame (the stopped block's first, where `playing` is already 0): it is let go there,
// not held, so the arpeggio stops with the song.
#[test]
fn hold_lets_go_of_a_clips_chord_at_a_stop() {
    let stop: u64 = 235 * BLOCK; // a block line, as a Stop always is
    let song = move |f: u64| if f < stop { (true, f as f64 / BEAT as f64) } else { (false, stop as f64 / BEAT as f64) };
    let r = Run::new("arp", &ARP, stop + 4 * BEAT).notes(&c_major(0, stop)).with("hold", 1.0).song(&song);
    let g = run::<Arp>(&r).0;
    let after: Vec<(u64, f32)> = ons(&g).into_iter().filter(|x| x.0 >= stop).collect();
    assert!(after.is_empty(), "it played on after the Stop: {after:?}");
    assert!(ons(&g).len() > 4, "it played before it");
    all_playable_and_ended(&g);
}

// PACK-30, Play, a Stop: keys held live go on through a Stop, their pattern started again from it
// on its own clock; the note it played ends at the Stop.
#[test]
fn keys_held_through_a_stop_start_again_from_it() {
    let song = |f: u64| if f < 30_000 { (true, f as f64 / BEAT as f64) } else { (false, 30_000.0 / BEAT as f64) };
    let r = Run::new("arp", &ARP, 45_000).notes(&c_major(0, 2 * BEAT)).song(&song).with("gate", 1.5);
    let g = run::<Arp>(&r).0;
    assert_eq!(ons(&g).iter().filter(|x| x.0 >= 30_000).copied().collect::<Vec<_>>(), [(30_000, 60.0), (36_000, 64.0), (42_000, 67.0)]);
    assert_eq!(spans(&g)[4].1, Some(30_000), "the note it played ends at the Stop");
}

// PACK-30, Held notes: 64 at most; a note past that is left out.
#[test]
fn at_most_64_notes_are_held() {
    let held: Vec<N> = (0..65).map(|i| n(0, 100 * BEAT, 40.0 + i as f32)).collect();
    let p = pitches(&arp(&held, &[("rate", 5.0)], 66 * BEAT / 8));
    assert_eq!(p[..64], (0..64).map(|i| 40.0 + i as f32).collect::<Vec<_>>(), "Up through the 64 held");
    assert_eq!(p[64], 40.0, "then the first again: the order is 64 long");
    assert!(!p.contains(&104.0), "the 65th, 104, is left out");
}

// PACK-30, Steps: a new rate starts the pattern again at its next step.
#[test]
fn a_new_rate_starts_the_pattern_again_at_its_next_step() {
    let mut h = host::<Arp>(1.0);
    let sixteenths = Run::new("arp", &ARP, 0).notes(&c_major(0, 4 * BEAT));
    let eighths = Run::new("arp", &ARP, 0).notes(&c_major(0, 4 * BEAT)).with("rate", 1.0);
    // Sixteenths, then eighths from beat 1.1: the next eighth, beat 1.5, plays the order's first note.
    let turn = (1.1 * BEAT as f64 / BLOCK as f64) as u64;
    let mut g = vec![];
    for b in 0..3 * BEAT / BLOCK {
        play_block(&mut h, if b < turn { &sixteenths } else { &eighths }, b, &mut g);
    }
    let want = [(0, 60.0), (6000, 64.0), (12_000, 67.0), (18_000, 60.0), (24_000, 64.0), (36_000, 60.0), (48_000, 64.0), (60_000, 67.0)];
    assert_eq!(ons(&g), want);
}

// PACK-30, Steps: a swing that moves while it plays never starts a step twice.
#[test]
fn a_moving_swing_never_starts_a_step_twice() {
    let mut h = host::<Arp>(1.0);
    let swung = Run::new("arp", &ARP, 0).notes(&c_major(0, 4 * BEAT)).with("swing", 0.71);
    let straight = Run::new("arp", &ARP, 0).notes(&c_major(0, 4 * BEAT)).with("swing", 0.5);
    let mut g = vec![];
    for b in 0..4 * BEAT / BLOCK {
        play_block(&mut h, if b % 2 == 0 { &swung } else { &straight }, b, &mut g);
    }
    let steps: Vec<u64> = ons(&g).iter().map(|x| x.0 / SIXTEENTH).collect();
    let mut once = steps.clone();
    once.dedup();
    assert_eq!((steps.len(), once.len()), (16, 16), "each of 16 sixteenths once: {:?}", ons(&g));
}

// PACK-30, Its step: the meter says where the steps picture draws the step playing (the middle of
// its box, in beats), and −1 while no note is held.
#[test]
fn the_step_meter_says_where_the_picture_draws_it() {
    let r = Run::new("arp", &ARP, BEAT).notes(&c_major(0, 3 * SIXTEENTH));
    let (_, meter) = run::<Arp>(&r);
    let in_step = |k: u64| meter[((k * SIXTEENTH + 3000) / BLOCK) as usize];
    for k in 0..3 {
        let want = arpeggio::place(k as usize, 0.25, 0.5, 0.8) as f32;
        assert_eq!(in_step(k), want, "step {k}");
    }
    assert_eq!(arpeggio::place(1, 0.25, 0.5, 0.8), 0.25 + 0.5 * 0.8 * 0.25, "the middle of its box");
    assert_eq!(arpeggio::place(1, 0.25, 0.6, 2.0), 0.3 + 0.5 * 0.2, "swung, its gate past 1 counted as 1");
    assert_eq!(*meter.last().unwrap(), -1.0, "no note held");
}

// PACK-30, Order: a note an octave up past 127 is left out, so no step is a note the guard drops.
#[test]
fn a_note_an_octave_past_127_is_left_out() {
    let g = arp(&[n(0, BEAT, 120.0), n(0, BEAT, 124.0)], &[("octaves", 3.0)], BEAT);
    assert_eq!(pitches(&g), [120.0, 124.0, 120.0, 124.0]);
    all_playable_and_ended(&g);
}

// ---------------------------------------------------------------- Chord (PACK-31)

/// What Chord gives for each note of `played` (each a beat long, a beat apart), with `set`.
fn chords(played: &[f32], set: &[(&'static str, f32)]) -> Vec<Vec<f32>> {
    let notes: Vec<N> = played.iter().enumerate().map(|(i, p)| n(i as u64 * BEAT, i as u64 * BEAT + BEAT / 2, *p)).collect();
    let mut r = Run::new("chord", &CHORD, played.len() as u64 * BEAT).notes(&notes);
    r.set = set.to_vec();
    let g = run::<Chord>(&r).0;
    all_playable_and_ended(&g);
    let mut out = vec![vec![]; played.len()];
    for (at, p) in ons(&g) {
        out[(at / BEAT) as usize].push(p);
    }
    out
}

// PACK-31: In key, the note goes to the key's nearest note (a tie going down), then the chord
// stacks the key's notes: in C major on D, D minor; on B, B diminished; C♯ goes down to C.
#[test]
fn in_key_a_chord_stacks_the_keys_notes_from_the_nearest_note() {
    let c = chords(&[62.0, 71.0, 61.0, 63.0], &[("key", C_MAJOR)]);
    assert_eq!(c, [vec![62.0, 65.0, 69.0], vec![71.0, 74.0, 77.0], vec![60.0, 64.0, 67.0], vec![62.0, 65.0, 69.0]]);
    // Each shape on C, in C major.
    let shapes = [
        vec![60.0, 72.0],
        vec![60.0, 67.0],
        vec![60.0, 64.0, 67.0],
        vec![60.0, 64.0, 67.0, 71.0],
        vec![60.0, 62.0, 67.0],
        vec![60.0, 65.0, 67.0],
        vec![60.0, 64.0, 67.0, 74.0],
    ];
    for (s, want) in shapes.iter().enumerate() {
        assert_eq!(chords(&[60.0], &[("key", C_MAJOR), ("shape", s as f32)])[0], *want, "shape {s}");
    }
}

// PACK-31: in a pentatonic or blues key, the chord stacks the key's own five or six notes, and
// nothing is refused.
#[test]
fn in_a_pentatonic_or_blues_key_the_chord_stacks_its_own_notes() {
    // C major pentatonic, C D E G A: a triad on C is C E A, on D is D G C, on E is E A D.
    assert_eq!(
        chords(&[60.0, 62.0, 64.0], &[("key", C_PENTATONIC)]),
        [vec![60.0, 64.0, 69.0], vec![62.0, 67.0, 72.0], vec![64.0, 69.0, 74.0]]
    );
    // C blues, C E♭ F F♯ G B♭: a triad on C is C F G; a 7th on E♭ is E♭ F♯ B♭ and E♭ an octave up.
    assert_eq!(chords(&[60.0], &[("key", C_BLUES)])[0], [60.0, 65.0, 67.0]);
    assert_eq!(chords(&[63.0], &[("key", C_BLUES), ("shape", 3.0)])[0], [63.0, 66.0, 70.0, 75.0]);
}

// PACK-31: with no key, a chromatic one, or In key off, the shapes are fixed: a triad is major, a
// 7th a dominant 7th, on the note as it is.
#[test]
fn with_no_key_or_in_key_off_the_shapes_are_fixed() {
    let fixed = [
        vec![0.0, 12.0],
        vec![0.0, 7.0],
        vec![0.0, 4.0, 7.0],
        vec![0.0, 4.0, 7.0, 10.0],
        vec![0.0, 2.0, 7.0],
        vec![0.0, 5.0, 7.0],
        vec![0.0, 4.0, 7.0, 14.0],
    ];
    for (s, steps) in fixed.iter().enumerate() {
        for set in [vec![("key", EVERY)], vec![("key", C_MAJOR), ("in_key", 0.0)]] {
            let mut set = set.clone();
            set.push(("shape", s as f32));
            let want: Vec<f32> = steps.iter().map(|o| 61.5 + o).collect();
            assert_eq!(chords(&[61.5], &set)[0], want, "shape {s}, {set:?}");
        }
    }
}

// PACK-31: inversion k moves the lowest note up an octave k times; a pitch twice plays once; a
// note past 127 is left out.
#[test]
fn inversions_move_the_lowest_note_up_an_octave() {
    let on_c = |inversion: f32, shape: f32| chords(&[60.0], &[("key", C_MAJOR), ("shape", shape), ("inversion", inversion)]).remove(0);
    assert_eq!(on_c(1.0, 2.0), [64.0, 67.0, 72.0]);
    assert_eq!(on_c(2.0, 2.0), [67.0, 72.0, 76.0]);
    assert_eq!(on_c(3.0, 2.0), [72.0, 76.0, 79.0], "past a triad's last inversion, it goes on up");
    assert_eq!(on_c(3.0, 3.0), [71.0, 72.0, 76.0, 79.0], "a 7th's third");
    assert_eq!(on_c(1.4, 2.0), [64.0, 67.0, 72.0], "the nearest whole number");
    assert_eq!(on_c(1.0, 0.0), [72.0], "an octave turned over is one note");
    assert_eq!(chords(&[120.0], &[("key", C_MAJOR), ("shape", 3.0)])[0], [120.0, 124.0, 127.0], "B9 is past 127");
}

// PACK-31: a chord's notes are as hard as the note, start with it and end with its Off.
#[test]
fn a_chords_notes_start_and_end_with_its_note() {
    let r = Run::new("chord", &CHORD, BEAT).notes(&[N { velocity: 0.45, ..n(100, 900, 60.0) }]).with("key", C_MAJOR);
    let g = run::<Chord>(&r).0;
    let s = spans(&g);
    assert_eq!(s, [(100, Some(900), 60.0), (100, Some(900), 64.0), (100, Some(900), 67.0)]);
    assert!(g.iter().all(|e| !matches!(e, Gave::On { velocity, .. } if *velocity != 0.45)));
}

// ---------------------------------------------------------------- Note echo (PACK-32)

fn echo(notes: &[N], set: &[(&'static str, f32)], frames: u64, song: Transport) -> Vec<Gave> {
    let mut r = Run::new("echo", &ECHO, frames).notes(notes).song(song);
    r.set = set.to_vec();
    run::<Echo>(&r).0
}

// PACK-32: each note, then `repeats` echoes, `time` apart, each (1 − fall) times as hard as the
// last and `pitch` higher, each as long as the note.
#[test]
fn echoes_repeat_time_apart_each_quieter_and_higher() {
    let g = echo(&[N { velocity: 1.0, ..n(0, 3000, 60.0) }], &[("pitch", 12.0)], 2 * BEAT, &from_0);
    let eighth = BEAT / 2;
    assert_eq!(spans(&g), (0..4).map(|k| (k * eighth, Some(k * eighth + 3000), 60.0 + 12.0 * k as f32)).collect::<Vec<_>>());
    let v: Vec<f32> = g.iter().filter_map(|e| if let Gave::On { velocity, .. } = e { Some(*velocity) } else { None }).collect();
    for (k, x) in v.iter().enumerate() {
        assert!((x - 0.7f32.powi(k as i32)).abs() < 1e-6, "echo {k}: {x}");
    }
    // Sixteenths, 5 repeats, the pitch the nearest whole semitone.
    let g = echo(&[n(0, 1000, 60.0)], &[("time", 3.0), ("repeats", 5.0), ("pitch", -2.4)], 2 * BEAT, &from_0);
    assert_eq!(ons(&g), (0..6).map(|k| (k * SIXTEENTH, 60.0 - 2.0 * k as f32)).collect::<Vec<_>>());
    all_playable_and_ended(&g);
}

// PACK-32: echoes are placed from each note's start on the song's beat; while the song is stopped,
// they keep time by time.tempo.
#[test]
fn echoes_keep_the_beat_and_while_stopped_the_tempo() {
    let g = echo(&[n(2400, 4000, 60.0)], &[("repeats", 2.0)], 2 * BEAT, &from_0);
    assert_eq!(ons(&g).iter().map(|x| x.0).collect::<Vec<_>>(), [2400, 14_400, 26_400]);
    let mut r = Run::new("echo", &ECHO, 3 * BEAT).notes(&[n(500, 900, 60.0)]).song(&stopped).with("repeats", 2.0);
    r.tempo = 60.0;
    assert_eq!(ons(&run::<Echo>(&r).0).iter().map(|x| x.0).collect::<Vec<_>>(), [500, 24_500, 48_500], "an eighth at 60 bpm");
}

// PACK-32: Play, a Stop, a seek and a loop's wrap end the echoes sounding and forget those to come;
// a note itself goes on while it is held.
#[test]
fn play_a_stop_and_a_seek_end_the_echoes() {
    let stop = |f: u64| if f < 15_000 { (true, f as f64 / BEAT as f64) } else { (false, 15_000.0 / BEAT as f64) };
    let g = echo(&[n(0, 6000, 60.0)], &[], 3 * BEAT, &stop);
    assert_eq!(spans(&g), [(0, Some(6000), 60.0), (12_000, Some(15_000), 60.0)], "echo 1 ends at the Stop; 2 and 3 never start");
    let seek = |f: u64| (true, if f < 15_000 { f } else { f + 10 * BEAT } as f64 / BEAT as f64);
    assert_eq!(spans(&echo(&[n(0, 6000, 60.0)], &[], 3 * BEAT, &seek)).len(), 2, "a seek");
    let play = |f: u64| if f < 15_000 { (false, 0.0) } else { (true, (f - 15_000) as f64 / BEAT as f64) };
    let held = echo(&[n(0, 2 * BEAT, 60.0)], &[], 3 * BEAT, &play);
    assert_eq!(spans(&held), [(0, Some(2 * BEAT), 60.0), (12_000, Some(15_000), 60.0)], "Play; the note itself goes on");
}

// PACK-32: an echo that would be silent, or go past 127, is left out.
#[test]
fn an_echo_past_127_or_silent_is_left_out() {
    let g = echo(&[n(0, 1000, 110.0)], &[("pitch", 7.0)], 2 * BEAT, &from_0);
    assert_eq!(pitches(&g), [110.0, 117.0, 124.0], "131 is past 127");
    all_playable_and_ended(&g);
    let g = echo(&[n(0, 1000, 60.0)], &[("fall", 1.0)], 2 * BEAT, &from_0);
    assert_eq!(pitches(&g), [60.0], "a fall of 1 leaves only the note");
}

// PACK-32: it echoes 256 notes at once at most: a note past that plays alone. A place comes free at
// the frame the last echo of its note ends, so a note after that frame is echoed, even in that block.
#[test]
fn it_echoes_256_notes_at_once_and_a_note_past_that_plays_alone() {
    // One echo each, a 1/32 (3,000 frames) after its note. The first note's echo ends at 3,100.
    let mut notes = vec![n(0, 100, 60.0)];
    notes.extend((1..echoes::ROOM as u64).map(|i| n(i, 10_000, 60.0)));
    notes.push(n(1000, 1100, 72.0));
    notes.push(n(3150, 3250, 74.0));
    let g = echo(&notes, &[("time", 5.0), ("repeats", 1.0)], BEAT, &from_0);
    let at = |pitch: f32| ons(&g).iter().filter(|x| x.1 == pitch).map(|x| x.0).collect::<Vec<_>>();
    assert_eq!(at(72.0), [1000], "the 257th plays alone");
    assert_eq!(at(74.0), [3150, 6150], "a place came free at 3,100, in the block of 3,150");
    all_playable_and_ended(&g);
}

// PACK-32: a place comes free after the frame where its note's echoes are over, as if each frame
// were played in turn: a note let go at a frame, none of its echoes to play, leaves its place to
// a note a frame later, not to one at that frame.
#[test]
fn a_place_comes_free_after_its_frame() {
    // Each echo a semitone up: the first note, at 127, has none to play (128 is past 127).
    let mut notes = vec![n(0, 5000, 127.0)];
    notes.extend((1..echoes::ROOM as u64).map(|i| n(i, 20_000, 60.0)));
    notes.push(n(5000, 5100, 72.0));
    notes.push(n(5001, 5101, 74.0));
    let g = echo(&notes, &[("time", 5.0), ("repeats", 1.0), ("pitch", 1.0)], BEAT, &from_0);
    let at = |pitch: f32| ons(&g).iter().filter(|x| x.1 == pitch).map(|x| x.0).collect::<Vec<_>>();
    assert_eq!((at(72.0), at(73.0)), (vec![5000], vec![]), "at the frame the first note is let go: alone");
    assert_eq!((at(74.0), at(75.0)), (vec![5001], vec![8001]), "a frame later: echoed");
    all_playable_and_ended(&g);
}

// PACK-32, ABI-15: a note and its echoes are 8 at most, its kernel's fan: repeats run 1 to 7, and
// 8, past the range, plays as 7.
#[test]
fn a_note_and_its_echoes_are_its_fan_at_most() {
    let g = echo(&[n(0, 2 * BEAT, 60.0)], &[("time", 5.0), ("repeats", 8.0), ("fall", 0.1)], 4 * BEAT, &from_0);
    assert_eq!(spans(&g).len(), 8, "the note and 7 echoes: {:?}", spans(&g));
}

// PACK-32, PACK-29: a note given past 0 to 127 is left out, and so are its echoes, even ones its
// pitch would bring back between them.
#[test]
fn a_note_given_past_127_or_below_0_is_left_out_with_its_echoes() {
    for pitch in [-12.0, 12.0] {
        let g = echo(&[n(0, 1000, 130.0), n(100, 1100, -2.0)], &[("pitch", pitch)], 2 * BEAT, &from_0);
        assert!(g.iter().all(|e| !matches!(e, Gave::On { .. })), "pitch {pitch}: {g:?}");
    }
}

// ---------------------------------------------------------------- Lock to key (PACK-33)

/// What Lock to key gives for each note of `played`, with `set`.
fn locked(played: &[f32], set: &[(&'static str, f32)]) -> Vec<f32> {
    let notes: Vec<N> = played.iter().enumerate().map(|(i, p)| n(i as u64 * 1000, i as u64 * 1000 + 500, *p)).collect();
    let mut r = Run::new("lock", &LOCK, played.len() as u64 * 1000).notes(&notes);
    r.set = set.to_vec();
    let g = run::<LockToKey>(&r).0;
    all_playable_and_ended(&g);
    pitches(&g)
}

// PACK-33: each note goes to the key's nearest note, a tie going down, as Lock to key does.
#[test]
fn lock_to_key_moves_each_note_to_the_nearest_note_of_the_key() {
    let all: Vec<f32> = (60..72).map(|p| p as f32).collect();
    assert_eq!(locked(&all, &[("key", C_MAJOR)]), [60.0, 60.0, 62.0, 62.0, 64.0, 65.0, 65.0, 67.0, 67.0, 69.0, 69.0, 71.0]);
    assert_eq!(locked(&[61.4, 61.6], &[("key", C_MAJOR)]), [60.0, 62.0], "rounded first");
}

// PACK-33: then `shift` notes of the key: +2 in C major turns C E G into E G B.
#[test]
fn lock_to_key_shifts_by_notes_of_the_key() {
    assert_eq!(locked(&[60.0, 64.0, 67.0], &[("key", C_MAJOR), ("shift", 2.0)]), [64.0, 67.0, 71.0]);
    assert_eq!(locked(&[60.0, 61.0], &[("key", C_MAJOR), ("shift", -1.0)]), [59.0, 59.0]);
    assert_eq!(locked(&[60.0], &[("key", C_MAJOR), ("shift", 7.0)]), [72.0], "seven notes of the key: an octave");
    assert_eq!(locked(&[60.0], &[("key", C_PENTATONIC), ("shift", 5.0)]), [72.0], "five of a pentatonic");
    assert_eq!(locked(&[127.0, 60.0], &[("key", C_MAJOR), ("shift", 1.0)]), [62.0], "past 127, left out");
}

// PACK-33: with no key (or a chromatic one), every note plays as it is, whatever the shift.
#[test]
fn with_no_key_lock_to_key_plays_notes_as_they_are() {
    assert_eq!(locked(&[61.0, 61.5, 66.0], &[("key", EVERY), ("shift", 3.0)]), [61.0, 61.5, 66.0]);
}

// ---------------------------------------------------------------- what all four keep (PACK-29)

// PACK-29: the wheels and the pedal pass as they come, at their own frame, through each.
#[test]
fn controllers_pass_as_they_come() {
    let midi = vec![(5, [0xB0, 64, 127]), (300, [0xE0, 0, 80]), (700, [0xB0, 1, 20])];
    let want: Vec<Gave> = midi.iter().map(|&(at, bytes)| Gave::Midi { at, bytes }).collect();
    let wheels = |g: Vec<Gave>| g.into_iter().filter(|e| matches!(e, Gave::Midi { .. })).collect::<Vec<_>>();
    let mut r = Run::new("arp", &ARP, 1024);
    r.midi = midi.clone();
    assert_eq!(wheels(run::<Arp>(&r).0), want, "arp");
    let mut r = Run::new("chord", &CHORD, 1024);
    r.midi = midi.clone();
    assert_eq!(wheels(run::<Chord>(&r).0), want, "chord");
    let mut r = Run::new("echo", &ECHO, 1024);
    r.midi = midi.clone();
    assert_eq!(wheels(run::<Echo>(&r).0), want, "echo");
    let mut r = Run::new("lock", &LOCK, 1024);
    r.midi = midi;
    assert_eq!(wheels(run::<LockToKey>(&r).0), want, "lock");
}

/// A thread's count of heap operations, for the tests that say none happen.
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        static HERE: Cell<u64> = const { Cell::new(0) };
    }

    fn count() {
        let _ = HERE.try_with(|c| c.set(c.get() + 1));
    }

    struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count();
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            count();
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count();
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    /// What this thread has allocated and freed so far.
    pub fn here() -> u64 {
        HERE.with(Cell::get)
    }
}

/// 2,000 blocks of `K` over a busy part: chords that come and go, and the pedal, playing and
/// stopped, with wraps; the heap operations its calls made.
fn heap_ops<K: Kernel>(names: &[&str], set: &[(usize, f32)]) -> u64 {
    let mut h = host::<K>(7.0);
    let events: Vec<Vec<Event>> = (0..64)
        .map(|b| {
            let on = |k: u32| Event::On {
                frame: (k as usize * 17) % 128,
                id: b * 8 + k + 1,
                pitch: 40.0 + (k * 7 + b) as f32 % 60.0,
                velocity: 0.7,
                from_pitch: None,
            };
            let off = |k: u32| Event::Off { frame: 120, id: b * 8 + k + 1, velocity: 0.0 };
            let mut e: Vec<Event> = (0..4).map(on).collect();
            e.sort_by_key(Event::frame);
            if b % 3 == 2 {
                e.extend((0..4).map(off));
            }
            e.push(Event::Midi { frame: 127, bytes: [0xB0, 64, (b * 2) as u8] });
            e
        })
        .collect();
    let mut ops = 0;
    for (b, e) in events.iter().cycle().take(2000).enumerate() {
        h.events(0, e);
        let playing = (b / 300) % 2 == 0;
        for (p, name) in names.iter().enumerate().skip(1) {
            let v = match *name {
                "beats" => ((b % 700) * 128) as f32 / BEAT as f32,
                "playing" => playing as u32 as f32,
                "tempo" => 133.0,
                _ => set.iter().find(|x| x.0 == p).map_or(0.5, |x| x.1),
            };
            h.port_mut(p)[..BLOCK as usize].fill(v);
        }
        let before = counting::here();
        h.process(BLOCK as u32);
        ops += counting::here() - before;
        h.given(0);
    }
    ops
}

// PACK-29, ABI-5: once born, each allocates nothing while it plays.
#[test]
fn the_four_allocate_nothing_while_they_play() {
    assert_eq!(heap_ops::<Arp>(&ARP, &[(4, 2.0), (6, 3.0), (7, 1.5), (8, 1.0)]), 0, "arp");
    assert_eq!(heap_ops::<Chord>(&CHORD, &[(1, C_MAJOR), (2, 3.0), (3, 1.0), (4, 2.0)]), 0, "chord");
    assert_eq!(heap_ops::<Echo>(&ECHO, &[(4, 5.0), (5, 8.0), (6, 0.1), (7, 5.0)]), 0, "echo");
    assert_eq!(heap_ops::<LockToKey>(&LOCK, &[(1, C_MAJOR), (2, 3.0)]), 0, "lock");
}

// PACK-29, ABI-5: a reset starts over: what a kernel gives after it is what a new one gives.
#[test]
fn a_reset_starts_over() {
    fn again<K: Kernel>(r: &Run) {
        let mut h = host::<K>(r.seed);
        let mut first = vec![];
        for b in 0..200 {
            play_block(&mut h, r, b, &mut first);
        }
        h.reset();
        let mut second = vec![];
        for b in 0..200 {
            play_block(&mut h, r, b, &mut second);
        }
        let ids = |g: &[Gave]| {
            g.iter()
                .map(|e| if let Gave::On { at, pitch, velocity, .. } = e { (*at, *pitch, *velocity) } else { (0, 0.0, 0.0) })
                .collect::<Vec<_>>()
        };
        assert!(!first.is_empty() && ids(&first) == ids(&second), "{}: {first:?}\n{second:?}", r.kernel);
    }
    let held = c_major(100, 20_000);
    again::<Arp>(&Run::new("arp", &ARP, 0).notes(&held).with("style", 4.0));
    again::<Chord>(&Run::new("chord", &CHORD, 0).notes(&held).with("key", C_MAJOR));
    again::<Echo>(&Run::new("echo", &ECHO, 0).notes(&held));
    again::<LockToKey>(&Run::new("lock", &LOCK, 0).notes(&held).with("key", C_MAJOR).with("shift", 1.0));
}

// PACK-29: whatever they are given (values that aren't numbers, or far past their ranges, notes
// at the ends of the keyboard and past them), each gives only notes the guard plays, and ends each.
#[test]
fn nothing_makes_them_give_a_note_the_guard_drops() {
    // A lane takes any number as a clip note's pitch: 130 and -2 too.
    let edge =
        [n(0, 30_000, 0.0), n(0, 30_000, 127.0), n(500, 31_000, 126.5), n(9000, 40_000, 3.0), n(0, 30_000, 130.0), n(100, 31_000, -2.0)];
    let frames = 100_000;
    for v in [f32::NAN, -1e9, 1e9, f32::INFINITY, -0.4, 0.49] {
        let all = |names: &'static [&'static str]| {
            names[1..].iter().filter(|x| !["beats", "playing", "tempo"].contains(x)).map(|x| (*x, v)).collect::<Vec<_>>()
        };
        let mut r = Run::new("arp", &ARP, frames).notes(&edge);
        r.set = all(&ARP);
        all_playable_and_ended(&run::<Arp>(&r).0);
        let mut r = Run::new("chord", &CHORD, frames).notes(&edge);
        r.set = all(&CHORD);
        all_playable_and_ended(&run::<Chord>(&r).0);
        let mut r = Run::new("echo", &ECHO, frames).notes(&edge);
        r.set = all(&ECHO);
        all_playable_and_ended(&run::<Echo>(&r).0);
        let mut r = Run::new("lock", &LOCK, frames).notes(&edge);
        r.set = all(&LOCK);
        all_playable_and_ended(&run::<LockToKey>(&r).0);
    }
}

//! PACK-29 to PACK-33: `@m2/notes`' four kernels in wasm, as music2's own, run in the lane pass behind
//! the guard (RENDER-41, RENDER-42), their notes read as an instrument reads them: Chord against
//! the roll's own chord stamps in every key of seven notes (NOTE-4), the Arpeggiator over a held
//! chord on the grid, Note echo on the beat, Lock to key onto the song's key. Their rules one by one are
//! the kernels' native tests (`packs/@m2/notes/kernels-src/src/tests.rs`).

use std::collections::BTreeMap;

use m2_ir::Flat;
use m2_song::scales::{Chord, TABLE};
use m2_song::songkey::SongKey;
use m2_song::{Arrangement, LaneData, LaneKind};

use super::goldens::addr;
use super::*;
use crate::event::{Event, LIVE_BIT};

/// A poly of sine voices playing `events`: what reads a note effect's notes, as an instrument does.
fn voice(id: &str, events: &str) -> Value {
    json!({"id": id, "op": "poly", "events": events, "voices": 4, "body": {"out": "amp", "nodes": [
        {"id": "p", "op": "lane", "lane": "pitch"},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]}})
}

/// A song of the notes lane `notes` and note effects `effects` (`(id, kernel, inputs)`, each reading
/// the lane), each played by voices of its own, summed; and `more` nodes before them.
fn song_of(effects: &[(String, &str, Value)], more: Value) -> Ir {
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "key", "op": "time", "of": "key"}),
        json!({"id": "beats", "op": "time", "of": "beats"}),
        json!({"id": "playing", "op": "time", "of": "playing"}),
        json!({"id": "tempo", "op": "time", "of": "tempo"}),
    ];
    nodes.extend(more.as_array().unwrap().iter().cloned());
    let mut sum: Option<String> = None;
    for (id, kernel, inputs) in effects {
        let mut ins = inputs.clone();
        ins["notes"] = json!("n");
        nodes.push(json!({"id": id, "op": "kernel", "kernel": format!("@m2/notes:{kernel}"), "inputs": ins}));
        nodes.push(voice(&format!("v{id}"), id));
        sum = Some(match sum {
            None => format!("v{id}"),
            Some(acc) => {
                nodes.push(
                    json!({"id": format!("a{id}"), "op": "kernel", "kernel": "std:add", "inputs": {"a": acc, "b": format!("v{id}")}}),
                );
                format!("a{id}")
            }
        });
    }
    song(Value::Array(nodes), &sum.expect("a note effect"), json!([{"key": "notes", "kind": "events<note>"}]))
}

/// The lane's notes, at 120 bpm (a beat is 24,000 frames), in the key `key` (`[root, scale]`).
fn data(notes: Value, key: Option<[&str; 2]>) -> DataSet {
    let mut a = json!({"tempo": [{"beat": 0, "bpm": 120}]});
    if let Some([root, scale]) = key {
        a["key"] = json!({"root": root, "scale": scale});
    }
    let arrangement: Arrangement = serde_json::from_value(a).unwrap();
    DataSet::with_lanes(arrangement, [(addr("notes"), LaneData::read(&notes.to_string(), &LaneKind::Notes).unwrap())])
}

/// Each note effect's slot, by its id in the song.
fn slots(r: &Renderer, flat: &Flat) -> BTreeMap<String, usize> {
    r.info().effects.iter().map(|&(node, slot)| (flat.nodes[node].name.trim_start_matches("song/").to_string(), slot)).collect()
}

/// The Ons of a block's events, as (frame, pitch), and how many Offs.
fn ons(events: &[Event]) -> (Vec<(u32, f32)>, usize) {
    let on = events.iter().filter_map(|e| if let Event::On { frame, pitch, .. } = e { Some((*frame, *pitch)) } else { None }).collect();
    (on, events.iter().filter(|e| matches!(e, Event::Off { .. })).count())
}

/// A choice constant of Chord's shape: its place.
fn shape(id: &str, place: u32) -> Value {
    json!({"id": id, "op": "const", "value": place, "unit": "choice<octave|fifth|triad|7th|sus2|sus4|add9>"})
}

// PACK-31, NOTE-4: in every key of seven notes, from each of the twelve roots, Chord plays the
// roll's own stamp of each kind, in each of its inversions, on each note: the twelve notes from C4
// played in turn, each first moved to the key's nearest note (a tie going down), as the roll moves
// a stamp's row.
#[test]
fn the_chord_is_the_rolls_stamp_in_every_seven_note_key() {
    let kinds = [(Chord::Triad, 2), (Chord::Seventh, 3), (Chord::Sus2, 4), (Chord::Sus4, 5), (Chord::Add9, 6)];
    let mut effects = vec![];
    let mut more = vec![json!({"id": "on", "op": "const", "value": 1, "unit": "switch"})];
    for (kind, place) in kinds {
        for inversion in 0..kind.inversions() {
            let id = format!("c{place}{inversion}");
            more.push(shape(&format!("s{id}"), place));
            more.push(json!({"id": format!("i{id}"), "op": "const", "value": inversion}));
            effects.push((
                id.clone(),
                "chord",
                json!({"key": "key", "shape": format!("s{id}"), "in_key": "on", "inversion": format!("i{id}")}),
            ));
        }
    }
    assert_eq!(effects.len(), 16, "every kind, every inversion");
    let ir = song_of(&effects, Value::Array(more));
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    // The twelve notes from C4, one a block, each half a block long.
    let block = BLOCK as f64 / 24_000.0;
    let notes: Vec<Value> = (0..12).map(|k| json!({"pitch": 60 + k, "start": k as f64 * block, "length": block / 2.0})).collect();
    let notes = json!({"clips": [{"start": 0, "length": 1, "notes": notes}]});
    let (mut keys, mut chords) = (0, 0);
    for scale in TABLE.iter().filter(|r| r.steps.len() == 7) {
        for root in m2_song::songkey::ROOTS {
            let key = SongKey::new(root, scale.name).unwrap();
            let mut r = Renderer::new(&flat, &data(notes.clone(), Some([root, scale.name])), &pk, SR).unwrap();
            let at = slots(&r, &flat);
            for k in 0..12 {
                r.render_block(r.clock()).unwrap();
                let played = key.snap(60.0 + k as f64);
                for (kind, place) in kinds {
                    for inversion in 0..kind.inversions() {
                        let stamp = key.stamp(played, kind, inversion).unwrap_or_else(|e| panic!("{root} {}: {e}", scale.name));
                        let (on, offs) = ons(r.lane_events(at[&format!("c{place}{inversion}")]));
                        let mut heard: Vec<f64> = on.iter().map(|x| x.1 as f64).collect();
                        heard.sort_by(f64::total_cmp);
                        assert_eq!(heard, stamp, "{root} {}: {kind:?} inversion {inversion} on {}", scale.name, 60 + k);
                        assert!(on.iter().all(|x| x.0 == on[0].0) && offs == stamp.len(), "at once, and ended with the note");
                        chords += 1;
                    }
                }
            }
            keys += 1;
        }
    }
    assert_eq!((keys, chords), (9 * 12, 9 * 12 * 12 * 16), "nine scales of seven notes, from each root");
}

// PACK-30: the Arpeggiator, in wasm, over a C major chord held from beat 0: a step each sixteenth
// (6,000 frames at 120 bpm), C E G C' E' G' for two octaves, each 0.8 of its step, to the frame.
#[test]
fn the_arpeggiator_plays_a_held_chord_in_wasm() {
    let more = json!([
        {"id": "style", "op": "const", "value": 0, "unit": "choice<up|down|updown|played|random>"},
        {"id": "rate", "op": "const", "value": 3, "unit": "choice<1/4|1/8|1/8T|1/16|1/16T|1/32>"},
        {"id": "oct", "op": "const", "value": 1, "unit": "choice<1|2|3|4>"},
    ]);
    let inputs = json!({"beats": "beats", "playing": "playing", "tempo": "tempo", "style": "style", "rate": "rate", "octaves": "oct"});
    let ir = song_of(&[("a".to_string(), "arp", inputs)], more);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let chord = json!({"clips": [{"start": 0, "length": 4, "notes": [
        {"pitch": "C4", "start": 0, "length": 2}, {"pitch": "E4", "start": 0, "length": 2}, {"pitch": "G4", "start": 0, "length": 2}]}]});
    let mut r = Renderer::new(&flat, &data(chord, None), &pk, SR).unwrap();
    let slot = slots(&r, &flat)["a"];
    let (mut heard, mut offs) = (vec![], vec![]);
    for b in 0..(2 * 24_000 / BLOCK as u64) {
        r.render_block(r.clock()).unwrap();
        for e in r.lane_events(slot) {
            match *e {
                Event::On { frame, pitch, .. } => heard.push((b * BLOCK as u64 + frame as u64, pitch)),
                Event::Off { frame, .. } => offs.push(b * BLOCK as u64 + frame as u64),
                _ => {}
            }
        }
    }
    let up = [60.0, 64.0, 67.0, 72.0, 76.0, 79.0];
    assert_eq!(heard, (0..8).map(|k| (k * 6000, up[k as usize % 6])).collect::<Vec<_>>());
    assert_eq!(offs, (0..8).map(|k| k * 6000 + 4800).collect::<Vec<_>>());
}

// PACK-32: Note echo, in wasm: a note at beat 0.1, then three echoes an eighth apart on the beat
// (12,000 frames), each 0.7 times as hard.
#[test]
fn the_echo_plays_its_echoes_on_the_beat_in_wasm() {
    let inputs = json!({"beats": "beats", "playing": "playing", "tempo": "tempo"});
    let ir = song_of(&[("e".to_string(), "echo", inputs)], json!([]));
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let note = json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "A4", "start": 0.1, "length": 0.1, "velocity": 1}]}]});
    let mut r = Renderer::new(&flat, &data(note, None), &pk, SR).unwrap();
    let slot = slots(&r, &flat)["e"];
    let mut heard = vec![];
    for b in 0..(2 * 24_000 / BLOCK as u64) {
        r.render_block(r.clock()).unwrap();
        for e in r.lane_events(slot) {
            if let Event::On { frame, pitch, velocity, .. } = *e {
                heard.push((b * BLOCK as u64 + frame as u64, pitch, (velocity * 1000.0).round() / 1000.0));
            }
        }
    }
    assert_eq!(heard, [(2400, 69.0, 1.0), (14_400, 69.0, 0.7), (26_400, 69.0, 0.49), (38_400, 69.0, 0.343)]);
}

// PACK-33: Lock to key, in wasm, in the song's key of D minor: C♯ goes down to C, E♭ down to D; shifted
// up 2 notes of the key, D goes to F.
#[test]
fn lock_to_key_moves_notes_onto_the_songs_key_in_wasm() {
    let more = json!([{"id": "two", "op": "const", "value": 2}]);
    let effects = [("s".to_string(), "lock", json!({"key": "key"})), ("t".to_string(), "lock", json!({"key": "key", "shift": "two"}))];
    let ir = song_of(&effects, more);
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let notes = json!({"clips": [{"start": 0, "length": 1, "notes": [
        {"pitch": 61, "start": 0, "length": 0.001}, {"pitch": 63, "start": 0.01, "length": 0.001}, {"pitch": 62, "start": 0.02, "length": 0.001}]}]});
    let mut r = Renderer::new(&flat, &data(notes.clone(), Some(["D", "minor"])), &pk, SR).unwrap();
    let at = slots(&r, &flat);
    let (mut s, mut t) = (vec![], vec![]);
    for _ in 0..12 {
        r.render_block(r.clock()).unwrap();
        s.extend(ons(r.lane_events(at["s"])).0.into_iter().map(|x| x.1));
        t.extend(ons(r.lane_events(at["t"])).0.into_iter().map(|x| x.1));
    }
    assert_eq!(s, [60.0, 62.0, 62.0], "C♯ down to C, E♭ down to D, D as it is");
    assert_eq!(t, [64.0, 65.0, 65.0], "and two notes of D minor up: E, F, F");
    // With no key, notes play as they are.
    let mut r = Renderer::new(&flat, &data(notes, None), &pk, SR).unwrap();
    let mut s = vec![];
    for _ in 0..12 {
        r.render_block(r.clock()).unwrap();
        s.extend(ons(r.lane_events(at["s"])).0.into_iter().map(|x| x.1));
    }
    assert_eq!(s, [61.0, 63.0, 62.0]);
}

// PACK-30, ABI-13, TAP-15: the Arpeggiator's meter `step`, which a note effect's kernel writes in the
// lane pass, reaches a look as the steps picture's live layer reads it: where the picture draws the
// step playing (the middle of its box, in beats from the turn's start), and -1 once no note is held.
#[test]
fn the_arpeggiators_step_reaches_a_look() {
    let inputs = json!({"beats": "beats", "playing": "playing", "tempo": "tempo"});
    let ir = song_of(&[("a".to_string(), "arp", inputs)], json!([]));
    let pk = own_packages();
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let chord = json!({"clips": [{"start": 0, "length": 4, "notes": [
        {"pitch": "C4", "start": 0, "length": 1}, {"pitch": "E4", "start": 0, "length": 1}, {"pitch": "G4", "start": 0, "length": 1}]}]});
    let mut r =
        Renderer::prepare(&flat, &data(chord, None), &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh)
            .unwrap();
    r.set_transport(Transport::new(0, true));
    let taps = r.info().taps.clone().expect("a live plan has taps");
    let node = flat.nodes.iter().position(|n| n.name == "song/a").expect("the arpeggiator's node");
    taps.point_meter(0, Some(TapAt::Port(node, 1))).unwrap();
    // A sixteenth is 6,000 frames at 120 bpm: block 70 ends in step 1, block 160 in step 3.
    let mut read = |until: usize| {
        while (r.clock() as usize) < until * BLOCK {
            r.render_block(r.clock()).unwrap();
        }
        taps.meter(0).expect("a reading").now
    };
    assert_eq!(read(70), step_place(1), "step 1");
    assert_eq!(read(160), step_place(3), "step 3: the turn's first again");
    assert_eq!(read(400), -1.0, "the chord let go at beat 1");
}

/// Where the steps picture draws step `k` of a sixteenth's turn at gate 0.8 and no swing, in beats:
/// its box starts at k × 0.25 and is 0.8 of it long, and the line is at its middle (PACK-30).
fn step_place(k: usize) -> f32 {
    ((k % 3) as f64 * 0.25 + 0.5 * 0.8 * 0.25) as f32
}

/// The Arpeggiator with Hold on, reading the lane.
fn held_arp() -> Ir {
    let more = json!([{"id": "hold", "op": "const", "value": 1, "unit": "switch"}]);
    song_of(&[("a".to_string(), "arp", json!({"beats": "beats", "playing": "playing", "tempo": "tempo", "hold": "hold"}))], more)
}

/// A clip of C, E and G from beat 0, `beats` long.
fn c_chord(beats: f64) -> Value {
    json!({"clips": [{"start": 0, "length": 16, "notes": [
        {"pitch": "C4", "start": 0, "length": beats}, {"pitch": "E4", "start": 0, "length": beats}, {"pitch": "G4", "start": 0, "length": beats}]}]})
}

/// `ir` over `data`, live and playing from beat 0, and its note effects' slots.
fn playing(ir: &Ir, data: &DataSet) -> (Renderer, BTreeMap<String, usize>) {
    let pk = own_packages();
    let flat = check(ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::new(&flat, data, &pk, SR).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, true));
    r.live = true;
    let at = slots(&r, &flat);
    (r, at)
}

/// The ids of the Ons slot `slot` gives over the next `blocks` blocks.
fn ons_of(r: &mut Renderer, slot: usize, blocks: usize) -> Vec<u32> {
    let mut out = vec![];
    for _ in 0..blocks {
        r.render_block(r.clock()).unwrap();
        out.extend(r.lane_events(slot).iter().filter_map(|e| if let Event::On { id, .. } = *e { Some(id) } else { None }));
    }
    out
}

/// A key played live on the lane, and let go.
fn key(handle: u32, pitch: f32) -> LiveEv {
    LiveEv::On { source: 1, handle, pitch, velocity: 0.8 }
}
fn let_go(handle: u32) -> LiveEv {
    LiveEv::Off { source: 1, handle }
}

/// Four beats of blocks.
const FOUR_BEATS: usize = 4 * 24_000 / BLOCK;

// PACK-30, Hold, RENDER-9: in the engine, a Stop ends a clip's arpeggio with Hold on. The lane ends
// the chord at the Stop's own frame, where `time.playing` is already 0, and nothing latches there.
#[test]
fn a_stop_ends_a_clips_arpeggio_with_hold_on_in_wasm() {
    let (mut r, at) = playing(&held_arp(), &data(c_chord(8.0), None));
    assert!(!ons_of(&mut r, at["a"], 200).is_empty(), "it plays the chord");
    r.transport_cmd(Cmd::Stop);
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS), Vec::<u32>::new(), "no step after the Stop");
}

// PACK-30, Hold, RENDER-14: in the engine, Hold latches keys played live while the song plays: let
// go, they keep playing, until a Stop lets go of them.
#[test]
fn hold_latches_live_keys_while_the_song_plays_in_wasm() {
    let (mut r, at) = playing(&held_arp(), &data(json!({"clips": []}), None));
    for (h, p) in [(1, 60.0), (2, 64.0), (3, 67.0)] {
        assert!(r.live(0, key(h, p)));
    }
    assert!(!ons_of(&mut r, at["a"], 100).is_empty(), "the keys play");
    for h in 1..=3 {
        r.live(0, let_go(h));
    }
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS).len(), 16, "let go, the chord plays on: 16 sixteenths in 4 beats");
    r.transport_cmd(Cmd::Stop);
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS), Vec::<u32>::new(), "a Stop lets go of it");
}

// PACK-30, Hold: in the engine, a clip's chord never latches, while the song plays too.
#[test]
fn hold_never_latches_a_clips_chord_in_wasm() {
    let (mut r, at) = playing(&held_arp(), &data(c_chord(1.0), None));
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS / 2).len(), 4, "the chord's beat, and no more");
}

// PACK-30, RENDER-42, RENDER-14: a key played live stays live through a Chord: the guard keeps the
// bit on the notes it gives from it, so the Arpeggiator after holds the chord one finger played.
#[test]
fn a_live_key_stays_live_through_a_chord_in_wasm() {
    let nodes = json!([
        {"id": "n", "op": "lane", "lane": "notes"},
        {"id": "key", "op": "time", "of": "key"},
        {"id": "beats", "op": "time", "of": "beats"},
        {"id": "playing", "op": "time", "of": "playing"},
        {"id": "tempo", "op": "time", "of": "tempo"},
        {"id": "hold", "op": "const", "value": 1, "unit": "switch"},
        {"id": "c", "op": "kernel", "kernel": "@m2/notes:chord", "inputs": {"notes": "n", "key": "key"}},
        {"id": "a", "op": "kernel", "kernel": "@m2/notes:arp", "inputs": {"notes": "c", "beats": "beats", "playing": "playing", "tempo": "tempo", "hold": "hold"}},
        voice("v", "a"),
    ]);
    let ir = song(nodes, "v", json!([{"key": "notes", "kind": "events<note>"}]));
    let (mut r, at) = playing(&ir, &data(json!({"clips": []}), None));
    assert!(r.live(0, key(1, 60.0)));
    let chord = ons_of(&mut r, at["c"], 1);
    assert_eq!(chord.len(), 3, "C E G from one key");
    assert!(chord.iter().all(|id| id & LIVE_BIT != 0), "the chord's notes are live: {chord:?}");
    ons_of(&mut r, at["a"], 99);
    r.live(0, let_go(1));
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS).len(), 16, "let go, the chord plays on");
}

// RENDER-14, MON-7: a key played live has the live bit on its id, and a clip's note hasn't.
#[test]
fn a_live_keys_id_has_the_live_bit_and_a_clips_note_hasnt() {
    let clip = json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "C4", "start": 0, "length": 1}]}]});
    let (mut r, _) = playing(&song_of(&[("s".to_string(), "lock", json!({"key": "key"}))], json!([])), &data(clip, None));
    assert!(r.live(0, key(1, 67.0)));
    r.render_block(r.clock()).unwrap();
    let ons: Vec<(f32, u32)> =
        r.lane_events(0).iter().filter_map(|e| if let Event::On { pitch, id, .. } = *e { Some((pitch, id)) } else { None }).collect();
    assert_eq!(ons.len(), 2, "{ons:?}");
    for (pitch, id) in ons {
        assert_eq!(id & LIVE_BIT != 0, pitch == 67.0, "{pitch}: {id:#x}");
    }
}

// RENDER-14: the engine's one counter wraps below the live bit, so only a key played live has it.
#[test]
fn the_note_counter_wraps_below_the_live_bit() {
    let mut next = LIVE_BIT - 1;
    assert_eq!(crate::event::take_id(&mut next), LIVE_BIT - 1);
    assert_eq!((crate::event::take_id(&mut next), next), (1, 2), "then 1 again");
}

// PACK-30, Hold: in the engine, a loop's wrap keeps the keys Hold latched: let go, the chord plays on
// every lap of a 2-beat loop.
#[test]
fn hold_keeps_its_latch_through_a_loops_wrap_in_wasm() {
    let (mut r, at) = playing(&held_arp(), &data(json!({"clips": []}), None));
    r.transport_cmd(Cmd::Loop(Some((0.0, 2.0))));
    for (h, p) in [(1, 60.0), (2, 64.0), (3, 67.0)] {
        assert!(r.live(0, key(h, p)));
    }
    assert!(!ons_of(&mut r, at["a"], 50).is_empty(), "the keys play");
    for h in 1..=3 {
        r.live(0, let_go(h));
    }
    // Two beats are 375 blocks: the laps end at blocks 375 and 750, counted from the first.
    let laps: Vec<usize> = (0..3).map(|lap| ons_of(&mut r, at["a"], if lap == 0 { 325 } else { 375 }).len()).collect();
    assert_eq!(laps, [6, 8, 8], "every lap");
}

// RENDER-14, RENDER-42, ABI-15: offline, as an export renders, the guard gives no note the live bit,
// whatever a kernel gives: a stranger's note effect that numbers a clip's chord from 1 << 31 can't
// make the Arpeggiator after it, Hold on, play the chord on after it ends.
#[test]
fn an_export_never_latches_a_chord_a_kernel_marked_live() {
    let raw = |kind: u32, id: u32, v: [f32; 3]| [kind, 0, id, v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
    let b = LIVE_BIT;
    let mut script: Vec<Vec<[u32; 6]>> = vec![[1, 2, 3].map(|n| raw(0, b | n, [56.0 + 4.0 * n as f32, 0.8, f32::NAN])).to_vec()];
    script.extend((1..10).map(|_| vec![]));
    script.push([1, 2, 3].map(|n| raw(1, b | n, [0.0; 3])).to_vec());
    let (pk, pkg) = super::events::with_effects(&script);
    let nodes = json!([
        {"id": "n", "op": "lane", "lane": "notes"},
        {"id": "beats", "op": "time", "of": "beats"},
        {"id": "playing", "op": "time", "of": "playing"},
        {"id": "tempo", "op": "time", "of": "tempo"},
        {"id": "hold", "op": "const", "value": 1, "unit": "switch"},
        {"id": "s", "op": "kernel", "kernel": "@test/t:script", "inputs": {"notes": "n"}},
        {"id": "a", "op": "kernel", "kernel": "@m2/notes:arp", "inputs": {"notes": "s", "beats": "beats", "playing": "playing", "tempo": "tempo", "hold": "hold"}},
        voice("v", "a"),
    ]);
    let ir = song(nodes, "v", json!([{"key": "notes", "kind": "events<note>"}]));
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::new(&flat, &data(json!({"clips": []}), None), &pk, SR).unwrap();
    let at = slots(&r, &flat);
    assert!(!ons_of(&mut r, at["a"], 60).is_empty(), "the chord plays while it sounds");
    assert_eq!(ons_of(&mut r, at["a"], FOUR_BEATS), Vec::<u32>::new(), "no latched chord after it ends");
    std::fs::remove_dir_all(pkg).ok();
}

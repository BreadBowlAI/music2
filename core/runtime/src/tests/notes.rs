//! PACK-29 to PACK-33: `@m2/notes`' four kernels in wasm, as music2's own, run in the lane pass behind
//! the guard (RENDER-41, RENDER-42), their notes read as an instrument reads them: Chord against
//! the roll's own chord stamps in every key of seven notes (NOTE-4), the Arpeggiator over a held
//! chord on the grid, Note echo on the beat, Scale onto the song's key. Their rules one by one are
//! the kernels' native tests (`packs/@m2/notes/kernels-src/src/tests.rs`).

use std::collections::BTreeMap;

use m2_ir::Flat;
use m2_song::scales::{Chord, TABLE};
use m2_song::songkey::SongKey;
use m2_song::{Arrangement, LaneData, LaneKind};

use super::goldens::addr;
use super::*;
use crate::event::Event;

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
                nodes.push(json!({"id": format!("a{id}"), "op": "kernel", "kernel": "std:add", "inputs": {"a": acc, "b": format!("v{id}")}}));
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
            effects.push((id.clone(), "chord", json!({"key": "key", "shape": format!("s{id}"), "in_key": "on", "inversion": format!("i{id}")})));
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

// PACK-33: Scale, in wasm, in the song's key of D minor: C♯ goes down to C, E♭ down to D; shifted
// up 2 notes of the key, D goes to F.
#[test]
fn the_scale_moves_notes_onto_the_songs_key_in_wasm() {
    let more = json!([{"id": "two", "op": "const", "value": 2}]);
    let effects = [("s".to_string(), "scale", json!({"key": "key"})), ("t".to_string(), "scale", json!({"key": "key", "shift": "two"}))];
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

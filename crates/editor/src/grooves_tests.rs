//! Grooves (NOTE-7): each one's exact places and velocities, a groove applied twice, and which
//! notes a groove leaves alone.

use m2_song::Note;

use super::*;

/// Notes a step of `every` beats apart from 0, `n` of them, each a step long, at velocity 0.8.
fn run(every: f64, n: usize) -> Vec<Note> {
    (0..n).map(|k| Note { pitch: 60.0, start: k as f64 * every, length: every, velocity: 0.8 }).collect()
}

/// `notes` after `g`, every note picked, in a clip that starts at `start`.
fn grooved(notes: &[Note], start: f64, g: Groove) -> Vec<Note> {
    let mut out = notes.to_vec();
    let all: Vec<usize> = (0..notes.len()).collect();
    apply(&mut out, &all, start, &g).unwrap();
    out
}

fn starts(notes: &[Note]) -> Vec<f64> {
    notes.iter().map(|n| n.start).collect()
}

fn lengths(notes: &[Note]) -> Vec<f64> {
    notes.iter().map(|n| n.length).collect()
}

fn velocities(notes: &[Note]) -> Vec<f64> {
    notes.iter().map(|n| n.velocity).collect()
}

// NOTE-7: each groove's places, to a millionth of a beat, and what it does to lengths and
// velocities.
#[test]
fn each_groove_moves_notes_to_exact_places() {
    let sixteenths = run(0.25, 8);
    let eighths = run(0.5, 4);
    // Swing 60% on sixteenths: the second of each pair at 60% of the pair (0.3), keeping its end.
    let g = grooved(&sixteenths, 0.0, Groove::Swing { amount: 0.6, step: 0.25 });
    assert_eq!(starts(&g), [0.0, 0.3, 0.5, 0.8, 1.0, 1.3, 1.5, 1.8]);
    assert_eq!(lengths(&g), [0.25, 0.2, 0.25, 0.2, 0.25, 0.2, 0.25, 0.2]);
    assert_eq!(velocities(&g), [0.8; 8]);
    // Swing 55% and 71% on eighths: the sixteenths between them stay where they are.
    assert_eq!(starts(&grooved(&eighths, 0.0, Groove::Swing { amount: 0.55, step: 0.5 })), [0.0, 0.55, 1.0, 1.55]);
    assert_eq!(
        starts(&grooved(&sixteenths, 0.0, Groove::Swing { amount: 0.71, step: 0.5 })),
        [0.0, 0.25, 0.71, 0.75, 1.0, 1.25, 1.71, 1.75]
    );
    // 50% is straight: it puts swung notes back.
    let swung = grooved(&eighths, 0.0, Groove::Swing { amount: 0.66, step: 0.5 });
    assert_eq!(starts(&swung), [0.0, 0.66, 1.0, 1.66]);
    assert_eq!(starts(&grooved(&swung, 0.0, Groove::Swing { amount: 0.5, step: 0.5 })), [0.0, 0.5, 1.0, 1.5]);
    // Shuffle: the second eighth of each beat at 2/3 of the beat, a triplet.
    let g = grooved(&eighths, 0.0, Groove::Shuffle);
    assert_eq!(starts(&g), [0.0, 0.666667, 1.0, 1.666667]);
    assert_eq!(lengths(&g), [0.5, 0.333333, 0.5, 0.333333]);
    // Laid back and Pushed: the three sixteenths after each beat 1/32 of a beat late, or early.
    assert_eq!(starts(&grooved(&sixteenths, 0.0, Groove::LaidBack)), [0.0, 0.28125, 0.53125, 0.78125, 1.0, 1.28125, 1.53125, 1.78125]);
    let pushed = grooved(&sixteenths, 0.0, Groove::Pushed);
    assert_eq!(starts(&pushed), [0.0, 0.21875, 0.46875, 0.71875, 1.0, 1.21875, 1.46875, 1.71875]);
    assert_eq!(lengths(&pushed), [0.25, 0.28125, 0.28125, 0.28125, 0.25, 0.28125, 0.28125, 0.28125], "each moved note keeps its end");
    // Accents: velocities only: the beat 0.9, the "and" 0.75, the other two 0.6.
    let g = grooved(&sixteenths, 0.0, Groove::Accents);
    assert_eq!(starts(&g), starts(&sixteenths));
    assert_eq!(velocities(&g), [0.9, 0.6, 0.75, 0.6, 0.9, 0.6, 0.75, 0.6]);
    // Beats are counted from the song's start: a clip that starts on an off-beat has its first
    // note swung.
    assert_eq!(starts(&grooved(&run(0.5, 2), 0.5, Groove::Swing { amount: 0.6, step: 0.5 })), [0.1, 0.5]);
    // A note between two steps (a 32nd) is left as it is; one a little off a step is moved.
    let off =
        [Note { pitch: 60.0, start: 0.125, length: 0.125, velocity: 0.5 }, Note { pitch: 62.0, start: 0.27, length: 0.23, velocity: 0.5 }];
    let g = grooved(&off, 0.0, Groove::LaidBack);
    assert_eq!((starts(&g), lengths(&g)), (vec![0.125, 0.28125], vec![0.125, 0.21875]));
    // Only the notes picked.
    let mut some = sixteenths.clone();
    apply(&mut some, &[1, 2], 0.0, &Groove::Accents).unwrap();
    assert_eq!(velocities(&some), [0.8, 0.6, 0.75, 0.8, 0.8, 0.8, 0.8, 0.8]);
    // A swing past 71%, or of steps other than eighths and sixteenths, is refused in words.
    let mut n = sixteenths.clone();
    assert_eq!(apply(&mut n, &[0], 0.0, &Groove::Swing { amount: 0.75, step: 0.25 }).unwrap_err(), "Swing goes from 50% to 71%, not 75%");
    assert_eq!(
        apply(&mut n, &[0], 0.0, &Groove::Swing { amount: 0.6, step: 1.0 }).unwrap_err(),
        "Swing pairs eighths (0.5) or sixteenths (0.25), not 1 beat"
    );
    assert_eq!(n, sixteenths);
}

// NOTE-7: a groove sets notes to its places, so twice is once; and a groove that times the same
// steps as the one before sets them to its own places, from wherever the first put them.
#[test]
fn a_groove_twice_is_a_groove_once() {
    let sixteenths = run(0.25, 8);
    let swing = |amount: f64, step: f64| Groove::Swing { amount, step };
    let all = [swing(0.55, 0.25), swing(0.71, 0.25), swing(0.66, 0.5), Groove::Shuffle, Groove::LaidBack, Groove::Pushed, Groove::Accents];
    for g in all {
        let once = grooved(&sixteenths, 0.0, g);
        assert_eq!(grooved(&once, 0.0, g), once, "{g:?} twice");
    }
    // Grooves that time the same steps: the swings of a step (and Shuffle, an eighths' swing),
    // and Laid back and Pushed.
    let families = [
        vec![swing(0.5, 0.25), swing(0.55, 0.25), swing(0.6, 0.25), swing(0.66, 0.25), swing(0.71, 0.25)],
        vec![swing(0.5, 0.5), swing(0.55, 0.5), swing(0.6, 0.5), swing(0.66, 0.5), swing(0.71, 0.5), Groove::Shuffle],
        vec![Groove::LaidBack, Groove::Pushed],
    ];
    for family in families {
        for g in &family {
            for h in &family {
                let after = grooved(&grooved(&sixteenths, 0.0, *g), 0.0, *h);
                assert_eq!(starts(&after), starts(&grooved(&sixteenths, 0.0, *h)), "{h:?} after {g:?} puts the notes at {h:?}'s places");
            }
        }
    }
    // Light after Heavy: Light's place.
    let heavy = grooved(&sixteenths, 0.0, Groove::Swing { amount: 0.71, step: 0.25 });
    assert_eq!(starts(&grooved(&heavy, 0.0, Groove::Swing { amount: 0.55, step: 0.25 }))[..4], [0.0, 0.275, 0.5, 0.775]);
}

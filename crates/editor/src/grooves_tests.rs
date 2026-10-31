//! Grooves (NOTE-7): each one's exact places and velocities, steps counted from each bar, a groove
//! applied twice, and the notes a groove leaves alone.

use m2_song::{Arrangement, Meter, Note};

use super::*;

/// Notes a step of `every` beats apart from 0, `n` of them, each a step long, at velocity 0.8.
fn run(every: f64, n: usize) -> Vec<Note> {
    (0..n).map(|k| Note { pitch: 60.0, start: k as f64 * every, length: every, velocity: 0.8 }).collect()
}

/// `notes` after `g`, every note picked, in a clip that starts at `start`, in 4/4.
fn grooved(notes: &[Note], start: f64, g: Groove) -> Vec<Note> {
    grooved_in(notes, start, &Arrangement::default(), g)
}

/// `notes` after `g` in a song of arrangement `arr`.
fn grooved_in(notes: &[Note], start: f64, arr: &Arrangement, g: Groove) -> Vec<Note> {
    let mut out = notes.to_vec();
    let all: Vec<usize> = (0..notes.len()).collect();
    apply(&mut out, &all, start, arr, &g).unwrap();
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

fn swing(amount: f64, step: Step) -> Groove {
    Groove::Swing { amount, step }
}

// NOTE-7: each groove's places, to a millionth of a beat, and what it does to lengths and
// velocities.
#[test]
fn each_groove_moves_notes_to_exact_places() {
    let sixteenths = run(0.25, 8);
    let eighths = run(0.5, 4);
    // Swing 60% on sixteenths: the second of each pair at 60% of the pair (0.3), keeping its end.
    let g = grooved(&sixteenths, 0.0, swing(0.6, Step::Sixteenths));
    assert_eq!(starts(&g), [0.0, 0.3, 0.5, 0.8, 1.0, 1.3, 1.5, 1.8]);
    assert_eq!(lengths(&g), [0.25, 0.2, 0.25, 0.2, 0.25, 0.2, 0.25, 0.2]);
    assert_eq!(velocities(&g), [0.8; 8]);
    // Swing 55% and 71% on eighths.
    assert_eq!(starts(&grooved(&eighths, 0.0, swing(0.55, Step::Eighths))), [0.0, 0.55, 1.0, 1.55]);
    assert_eq!(starts(&grooved(&eighths, 0.0, swing(0.71, Step::Eighths))), [0.0, 0.71, 1.0, 1.71]);
    // 50% is straight: it puts swung notes back.
    let swung = grooved(&eighths, 0.0, swing(0.66, Step::Eighths));
    assert_eq!(starts(&swung), [0.0, 0.66, 1.0, 1.66]);
    assert_eq!(starts(&grooved(&swung, 0.0, swing(0.5, Step::Eighths))), [0.0, 0.5, 1.0, 1.5]);
    // Laid back and Pushed: the three sixteenths after each beat 1/32 of a beat late, or early.
    assert_eq!(starts(&grooved(&sixteenths, 0.0, Groove::LaidBack)), [0.0, 0.28125, 0.53125, 0.78125, 1.0, 1.28125, 1.53125, 1.78125]);
    let pushed = grooved(&sixteenths, 0.0, Groove::Pushed);
    assert_eq!(starts(&pushed), [0.0, 0.21875, 0.46875, 0.71875, 1.0, 1.21875, 1.46875, 1.71875]);
    assert_eq!(lengths(&pushed), [0.25, 0.28125, 0.28125, 0.28125, 0.25, 0.28125, 0.28125, 0.28125], "each moved note keeps its end");
    // Accents: velocities only: the beat 0.9, the "and" 0.75, the other two 0.6.
    let g = grooved(&sixteenths, 0.0, Groove::Accents);
    assert_eq!(starts(&g), starts(&sixteenths));
    assert_eq!(velocities(&g), [0.9, 0.6, 0.75, 0.6, 0.9, 0.6, 0.75, 0.6]);
    // A clip that starts on an off-beat has its first note swung: steps are the song's.
    assert_eq!(starts(&grooved(&run(0.5, 2), 0.5, swing(0.6, Step::Eighths))), [0.1, 0.5]);
    // A note a little off a step is moved.
    let off = [Note { pitch: 62.0, start: 0.27, length: 0.23, velocity: 0.5 }];
    let g = grooved(&off, 0.0, Groove::LaidBack);
    assert_eq!((starts(&g), lengths(&g)), (vec![0.28125], vec![0.21875]));
    // Only the notes picked.
    let mut some = sixteenths.clone();
    apply(&mut some, &[1, 2], 0.0, &Arrangement::default(), &Groove::Accents).unwrap();
    assert_eq!(velocities(&some), [0.8, 0.6, 0.75, 0.8, 0.8, 0.8, 0.8, 0.8]);
    // A swing past 71% is refused in words, changing nothing.
    let mut n = sixteenths.clone();
    let e = apply(&mut n, &[0], 0.0, &Arrangement::default(), &swing(0.75, Step::Sixteenths)).unwrap_err();
    assert_eq!(e, "Swing goes from 50% to 71%, not 75%");
    assert_eq!(n, sixteenths);
}

// NOTE-7 (review N3): a groove moves only the notes on its own grid. A beat (a pair, for a swing)
// that holds a picked note off the grid (a triplet, a sixteenth under an eighths' swing, a 32nd)
// is left whole; the other beats are grooved.
#[test]
fn a_groove_leaves_another_subdivision_alone() {
    // Six triplet eighths under a sixteenths' swing: untouched (the second triplet isn't swung
    // early, the third isn't left behind).
    let triplets: Vec<Note> = (0..6).map(|k| Note { pitch: 64.0, start: k as f64 / 3.0, length: 1.0 / 3.0, velocity: 0.8 }).collect();
    assert_eq!(grooved(&triplets, 0.0, swing(0.6, Step::Sixteenths)), triplets);
    assert_eq!(grooved(&triplets, 0.0, Groove::Accents), triplets);
    // Sixteenths under an eighths' swing: untouched, so the "and" never runs into the "a".
    let sixteenths = run(0.25, 8);
    assert_eq!(grooved(&sixteenths, 0.0, swing(0.66, Step::Eighths)), sixteenths);
    // Eighths under a sixteenths' swing: on its grid, on steps it doesn't move.
    assert_eq!(grooved(&run(0.5, 8), 0.0, swing(0.66, Step::Sixteenths)), run(0.5, 8));
    // One clip: two beats of sixteenths, then two of triplets (the review's case). The sixteenths
    // are swung; the triplets stay.
    let mut mixed = run(0.25, 8);
    mixed.extend((0..6).map(|k| Note { pitch: 64.0, start: 2.0 + k as f64 / 3.0, length: 1.0 / 3.0, velocity: 0.8 }));
    let g = grooved(&mixed, 0.0, swing(0.6, Step::Sixteenths));
    assert_eq!(starts(&g)[..8], [0.0, 0.3, 0.5, 0.8, 1.0, 1.3, 1.5, 1.8]);
    assert_eq!(g[8..], mixed[8..]);
    // A 32nd note in a beat leaves that beat alone, not the next.
    let mut fill = run(0.25, 8);
    fill.push(Note { pitch: 60.0, start: 0.125, length: 0.125, velocity: 0.8 });
    let g = grooved(&fill, 0.0, Groove::LaidBack);
    assert_eq!(starts(&g), [0.0, 0.25, 0.5, 0.75, 1.0, 1.28125, 1.53125, 1.78125, 0.125]);
}

// NOTE-7 (review N3): steps count from each bar's start, in the song's time signature. In 7/8 a
// bar is 3.5 beats: the second bar starts on what would be an "and" from the song's start, and is
// its own downbeat.
#[test]
fn steps_count_from_each_bar() {
    let seven_eight = Arrangement { meter: vec![Meter { beat: 0.0, beats: 7, unit: 8 }], ..Arrangement::default() };
    // Eighths through two bars: each bar's pairs from its start; its seventh eighth is a pair's first.
    let g = grooved_in(&run(0.5, 10), 0.0, &seven_eight, swing(0.6, Step::Eighths));
    assert_eq!(starts(&g), [0.0, 0.6, 1.0, 1.6, 2.0, 2.6, 3.0, 3.5, 4.1, 4.5]);
    // Accents: the second bar's downbeat (beat 3.5) is a beat, 0.9.
    let g = grooved_in(&run(0.25, 16), 0.0, &seven_eight, Groove::Accents);
    assert_eq!(velocities(&g)[12..], [0.9, 0.6, 0.9, 0.6]);
    // A note pushed early across a bar line is the next bar's downbeat, which no groove times.
    let early = [Note { pitch: 60.0, start: 3.45, length: 0.3, velocity: 0.8 }];
    assert_eq!(grooved_in(&early, 0.0, &seven_eight, Groove::LaidBack), early);
}

// NOTE-7: a groove sets notes to its places, so twice is once; and a groove that times the same
// steps as the one before sets them to its own places, from wherever the first put them.
#[test]
fn a_groove_twice_is_a_groove_once() {
    let sixteenths = run(0.25, 8);
    let eighths = run(0.5, 8);
    let all = [
        (swing(0.55, Step::Sixteenths), &sixteenths),
        (swing(0.71, Step::Sixteenths), &sixteenths),
        (swing(0.66, Step::Eighths), &eighths),
        (Groove::LaidBack, &sixteenths),
        (Groove::Pushed, &sixteenths),
        (Groove::Accents, &sixteenths),
    ];
    for (g, notes) in all {
        let once = grooved(notes, 0.0, g);
        assert_eq!(grooved(&once, 0.0, g), once, "{g:?} twice");
    }
    // Grooves that time the same steps: the swings of a step, and Laid back and Pushed.
    let amounts = [0.5, 0.55, 0.6, 0.66, 0.71];
    let families = [
        (amounts.map(|a| swing(a, Step::Sixteenths)).to_vec(), &sixteenths),
        (amounts.map(|a| swing(a, Step::Eighths)).to_vec(), &eighths),
        (vec![Groove::LaidBack, Groove::Pushed], &sixteenths),
    ];
    for (family, notes) in families {
        for g in &family {
            for h in &family {
                let after = grooved(&grooved(notes, 0.0, *g), 0.0, *h);
                assert_eq!(starts(&after), starts(&grooved(notes, 0.0, *h)), "{h:?} after {g:?} puts the notes at {h:?}'s places");
            }
        }
    }
    // 55% after 71%: 55%'s place.
    let heavy = grooved(&sixteenths, 0.0, swing(0.71, Step::Sixteenths));
    assert_eq!(starts(&grooved(&heavy, 0.0, swing(0.55, Step::Sixteenths)))[..4], [0.0, 0.275, 0.5, 0.775]);
}

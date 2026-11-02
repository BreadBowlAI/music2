//! The scale table (NOTE-1), snapping to a key (NOTE-3) and chord stamps (NOTE-4), checked
//! against what a musician would write out by hand.

use super::*;

fn key(root: &str, scale: &str) -> SongKey {
    SongKey::new(root, scale).unwrap()
}

/// A key's notes by name, from its root up: `A B C D E F G`.
fn named(k: &SongKey) -> String {
    k.names().join(" ")
}

// NOTE-1: each scale's notes in C, as written out by hand, and the same shape from every root.
#[test]
fn every_scale_has_its_notes_in_every_key() {
    let in_c = [
        ("major", "C D E F G A B"),
        ("minor", "C D E♭ F G A♭ B♭"),
        ("dorian", "C D E♭ F G A B♭"),
        ("phrygian", "C D♭ E♭ F G A♭ B♭"),
        ("lydian", "C D E F♯ G A B"),
        ("mixolydian", "C D E F G A B♭"),
        ("locrian", "C D♭ E♭ F G♭ A♭ B♭"),
        ("harmonic-minor", "C D E♭ F G A♭ B"),
        ("melodic-minor", "C D E♭ F G A B"),
        ("major-pentatonic", "C D E G A"),
        ("minor-pentatonic", "C E♭ F G B♭"),
        ("blues", "C E♭ F F♯ G B♭"),
        ("chromatic", "C C♯ D E♭ E F F♯ G A♭ A B♭ B"),
    ];
    assert_eq!(in_c.len(), TABLE.len(), "every scale of the table is written out here");
    for (row, (name, notes)) in TABLE.iter().zip(in_c) {
        assert_eq!(row.name, name, "the table's order");
        assert_eq!(named(&key("C", name)), notes, "{name} in C");
        let c = key("C", name).notes();
        for (root, spelled) in ROOTS.iter().enumerate() {
            let k = key(spelled, name);
            let up: Vec<u8> = c.iter().map(|pc| (pc + root as u8) % 12).collect();
            assert_eq!(k.notes(), up, "{name} from {spelled}: the same steps");
            for p in 0..128 {
                assert_eq!(k.has(p), up.contains(&((p % 12) as u8)), "{name} from {spelled}: pitch {p}");
            }
        }
    }
    // A few keys a musician knows by heart.
    for (root, scale, notes) in [
        ("A", "minor", "A B C D E F G"),
        ("D", "dorian", "D E F G A B C"),
        ("E", "phrygian", "E F G A B C D"),
        ("F", "lydian", "F G A B C D E"),
        ("G", "mixolydian", "G A B C D E F"),
        ("B", "locrian", "B C D E F G A"),
        ("A", "harmonic-minor", "A B C D E F G♯"),
        ("A", "melodic-minor", "A B C D E F♯ G♯"),
        ("G", "major-pentatonic", "G A B D E"),
        ("A", "minor-pentatonic", "A C D E G"),
        ("A", "blues", "A C D E♭ E G"),
        ("Eb", "major", "E♭ F G A♭ B♭ C D"),
        ("F#", "major", "F♯ G♯ A♯ B C♯ D♯ E♯"),
        ("Bb", "minor", "B♭ C D♭ E♭ F G♭ A♭"),
    ] {
        assert_eq!(named(&key(root, scale)), notes, "{root} {scale}");
    }
    // What a key is called, as the app says it.
    assert_eq!(key("Eb", "major").name(), "E♭ major");
    assert_eq!(key("A", "harmonic-minor").name(), "A harmonic minor");
    assert_eq!(key("D", "dorian").name(), "D Dorian");
    assert_eq!(key("F#", "minor-pentatonic").name(), "F♯ minor pentatonic");
    // A scale's third, for the library's keys.
    let minor: Vec<Option<bool>> = TABLE.iter().map(|r| r.scale.minor()).collect();
    let (y, n) = (Some(true), Some(false));
    assert_eq!(minor, [n, y, y, y, n, n, y, y, y, n, y, y, None]);
}

// NOTE-1: a file or a request may spell a scale in any case, with spaces; the file gets its name.
#[test]
fn a_scale_is_read_by_any_spelling_and_written_by_its_name() {
    for text in ["harmonic-minor", "Harmonic Minor", "HARMONIC_MINOR", " harmonic minor "] {
        assert_eq!(Scale::parse(text), Some(Scale::HarmonicMinor), "{text:?}");
    }
    assert_eq!(Scale::parse("bebop"), None);
    let k: SongKey = serde_json::from_str(r#"{"root": "D♭", "scale": "Major Pentatonic"}"#).unwrap();
    assert_eq!(serde_json::to_string(&k).unwrap(), r#"{"root":"C#","scale":"major-pentatonic"}"#);
    let e = serde_json::from_str::<SongKey>(r#"{"root": "A", "scale": "bebop"}"#).unwrap_err().to_string();
    assert!(e.contains("a key's scale is one of major, minor, dorian, phrygian, lydian, mixolydian, locrian, harmonic-minor"), "{e}");
    let e = serde_json::from_str::<SongKey>(r#"{"root": "H", "scale": "minor"}"#).unwrap_err().to_string();
    assert!(e.contains("H isn't a key's root"), "{e}");
}

// NOTE-3: the nearest note of the key; a pitch exactly between two goes down; never past 0 or 127.
#[test]
fn snapping_goes_to_the_nearest_note_and_a_tie_goes_down() {
    let c = key("C", "major");
    // C♯4 lies between C4 and D4: down to C4. So D♯ to D, F♯ to F, G♯ to G, A♯ to A.
    let snapped: Vec<f64> = (60..72).map(|p| c.snap(p as f64)).collect();
    assert_eq!(snapped, [60., 60., 62., 62., 64., 65., 65., 67., 67., 69., 69., 71.]);
    assert_eq!(c.snap_table(), [0, -1, 0, -1, 0, 0, -1, 0, -1, 0, -1, 0]);
    // A minor pentatonic (A C D E G) has gaps of three (E to G, A to C), with no middle: the
    // nearer note wins. A gap of two (C to D) has one: C♯ goes down to C.
    let am = key("A", "minor-pentatonic");
    let snapped: Vec<f64> = (69..81).map(|p| am.snap(p as f64)).collect();
    assert_eq!(snapped, [69., 69., 72., 72., 72., 74., 74., 76., 76., 79., 79., 79.]);
    assert_eq!(am.snap_table(), [0, -1, 0, -1, 0, -1, 1, 0, -1, 0, -1, 1]);
    // Every pitch of a chromatic key is its own.
    assert!(key("C", "chromatic").snap_table().iter().all(|d| *d == 0));
    // A pitch that isn't whole is rounded first; the ends stay on the keyboard.
    assert_eq!(c.snap(61.4), 60.0);
    assert_eq!(key("A", "major").snap(127.0), 126.0, "G9 isn't in A major and G♯9 is past the top: F♯9");
    assert_eq!(key("D", "major").snap(0.0), 1.0, "C-1 isn't in D major and the B below it is past the bottom: C♯-1");
    // The table is what the timeline publishes.
    assert_eq!(c.view().snap, c.snap_table());
}

/// Each chord kind on each note of `k`: `numeral name name name name name` and its notes, by
/// pitch from the root on `octave`, in every inversion.
fn stamps(k: &SongKey) -> Vec<String> {
    k.chords()
        .iter()
        .map(|d| {
            let names: Vec<String> = Chord::ALL.iter().map(|c| d.kinds.get(c).map_or("-".to_string(), |k| k.name.clone())).collect();
            format!("{} {}", d.numeral.as_deref().unwrap_or("-"), names.join(" "))
        })
        .collect()
}

/// Chord `c` on root `root` (a pitch) in every inversion, by note names.
fn voicings(k: &SongKey, root: &str, c: Chord) -> Vec<String> {
    let p = m2_song::parse_pitch(root).unwrap();
    (0..c.inversions())
        .map(|inv| k.stamp(p, c, inv).unwrap().iter().map(|n| m2_song::lane::pitch_name(*n).unwrap()).collect::<Vec<_>>().join(" "))
        .collect()
}

// NOTE-4: every chord on every degree of C major, named and numbered as a chord chart would.
#[test]
fn chord_stamps_in_c_major() {
    let c = key("C", "major");
    assert_eq!(
        stamps(&c),
        [
            "I C Cmaj7 Csus2 Csus4 Cadd9",
            "ii Dm Dm7 Dsus2 Dsus4 Dm(add9)",
            "iii Em Em7 Esus♭2 Esus4 Em(add♭9)",
            "IV F Fmaj7 Fsus2 Fsus♯4 Fadd9",
            "V G G7 Gsus2 Gsus4 Gadd9",
            "vi Am Am7 Asus2 Asus4 Am(add9)",
            "vii° Bdim Bm7♭5 Bsus♭2♭5 Bsus4♭5 Bdim(add♭9)",
        ]
    );
    // Each chord's notes, in every inversion: the k lowest notes up an octave.
    assert_eq!(voicings(&c, "C4", Chord::Triad), ["C4 E4 G4", "E4 G4 C5", "G4 C5 E5"]);
    assert_eq!(voicings(&c, "D4", Chord::Triad), ["D4 F4 A4", "F4 A4 D5", "A4 D5 F5"]);
    assert_eq!(voicings(&c, "G3", Chord::Seventh), ["G3 B3 D4 F4", "B3 D4 F4 G4", "D4 F4 G4 B4", "F4 G4 B4 D5"]);
    assert_eq!(voicings(&c, "B3", Chord::Triad), ["B3 D4 F4", "D4 F4 B4", "F4 B4 D5"]);
    assert_eq!(voicings(&c, "A3", Chord::Sus2), ["A3 B3 E4", "B3 E4 A4", "E4 A4 B4"]);
    assert_eq!(voicings(&c, "F3", Chord::Sus4), ["F3 B3 C4", "B3 C4 F4", "C4 F4 B4"]);
    assert_eq!(voicings(&c, "C4", Chord::Add9), ["C4 E4 G4 D5", "E4 G4 C5 D5", "G4 C5 D5 E5"]);
    // Root positions of every triad and 7th: the shapes the timeline publishes, from the root.
    let shapes: Vec<(u8, Vec<u8>, Vec<u8>)> =
        c.chords().iter().map(|d| (d.root, d.kinds[&Chord::Triad].shapes[0].clone(), d.kinds[&Chord::Seventh].shapes[0].clone())).collect();
    assert_eq!(
        shapes,
        [
            (0, vec![0, 4, 7], vec![0, 4, 7, 11]),
            (2, vec![0, 3, 7], vec![0, 3, 7, 10]),
            (4, vec![0, 3, 7], vec![0, 3, 7, 10]),
            (5, vec![0, 4, 7], vec![0, 4, 7, 11]),
            (7, vec![0, 4, 7], vec![0, 4, 7, 10]),
            (9, vec![0, 3, 7], vec![0, 3, 7, 10]),
            (11, vec![0, 3, 6], vec![0, 3, 6, 10]),
        ]
    );
    // A root out of the key is refused, in words; so is an inversion a chord hasn't, or a chord
    // that would go above G9.
    let e = c.stamp(61.0, Chord::Triad, 0).unwrap_err();
    assert_eq!(e, "C♯ isn't in C major: a chord is built on one of its notes (C, D, E, F, G, A, B)");
    assert_eq!(c.stamp(60.0, Chord::Triad, 3).unwrap_err(), "C has 3 inversions, 0 to 2, not 3");
    assert!(c.stamp(124.0, Chord::Triad, 0).unwrap_err().contains("above the highest note"));
}

// NOTE-4: A minor's chords, its relative major's from another root; its harmonic minor's V is major.
#[test]
fn chord_stamps_in_a_minor() {
    let am = key("A", "minor");
    assert_eq!(
        stamps(&am),
        [
            "i Am Am7 Asus2 Asus4 Am(add9)",
            "ii° Bdim Bm7♭5 Bsus♭2♭5 Bsus4♭5 Bdim(add♭9)",
            "III C Cmaj7 Csus2 Csus4 Cadd9",
            "iv Dm Dm7 Dsus2 Dsus4 Dm(add9)",
            "v Em Em7 Esus♭2 Esus4 Em(add♭9)",
            "VI F Fmaj7 Fsus2 Fsus♯4 Fadd9",
            "VII G G7 Gsus2 Gsus4 Gadd9",
        ]
    );
    assert_eq!(voicings(&am, "A3", Chord::Triad), ["A3 C4 E4", "C4 E4 A4", "E4 A4 C5"]);
    assert_eq!(voicings(&am, "E3", Chord::Seventh), ["E3 G3 B3 D4", "G3 B3 D4 E4", "B3 D4 E4 G4", "D4 E4 G4 B4"]);
    // Harmonic minor raises the seventh: its V is E major, E7, its III augmented, its vii° a dim7.
    let hm = key("A", "harmonic-minor");
    let numerals: Vec<String> =
        hm.chords().iter().map(|d| format!("{} {}", d.numeral.as_deref().unwrap(), d.kinds[&Chord::Seventh].name)).collect();
    assert_eq!(numerals, ["i Am(maj7)", "ii° Bm7♭5", "III+ Caug(maj7)", "iv Dm7", "V E7", "VI Fmaj7", "vii° G♯dim7"]);
    // A pentatonic key stamps its minor's chords on its own five notes, only those whose notes are
    // all in it (NOTE-4, review N1); the blues' blue note has none; a chromatic key has no chords.
    assert_eq!(
        stamps(&key("A", "minor-pentatonic")),
        ["i Am Am7 - Asus4 -", "III C - Csus2 - Cadd9", "iv - - Dsus2 Dsus4 -", "v - - - - -", "VII - - Gsus2 Gsus4 -"]
    );
    let blues = key("A", "blues");
    assert_eq!(blues.chords().len(), 6, "a place for each of its notes");
    assert_eq!(blues.stamp(63.0, Chord::Triad, 0).unwrap_err(), "A blues has no chord on E♭, its blue note");
    assert!(key("C", "chromatic").chords().is_empty());
    assert_eq!(
        key("C", "chromatic").stamp(60.0, Chord::Triad, 0).unwrap_err(),
        "C chromatic has no chords to stamp: pick a key with a scale of seven notes"
    );
}

// NOTE-4 (review N1): in a pentatonic or blues key a stamp offers only the chords whose notes are
// all in the key, and refuses the others in words, naming the notes that aren't; every chord it
// does place is notes of the key, in every inversion, from every root.
#[test]
fn a_pentatonic_or_blues_key_stamps_only_chords_all_in_it() {
    let c = key("C", "major-pentatonic");
    assert_eq!(stamps(&c), ["I C - Csus2 - Cadd9", "ii - - Dsus2 Dsus4 -", "iii - - - - -", "V - - Gsus2 Gsus4 -", "vi Am Am7 - Asus4 -"]);
    assert_eq!(
        c.stamp(62.0, Chord::Triad, 0).unwrap_err(),
        "Dm has F, which isn't in C major pentatonic: a stamp places only chords whose notes are all in the key"
    );
    assert_eq!(
        c.stamp(64.0, Chord::Seventh, 0).unwrap_err(),
        "Em7 has B, which isn't in C major pentatonic: a stamp places only chords whose notes are all in the key"
    );
    assert_eq!(
        c.stamp(67.0, Chord::Seventh, 0).unwrap_err(),
        "G7 has B and F, which aren't in C major pentatonic: a stamp places only chords whose notes are all in the key"
    );
    // The published refusals are the stamp's.
    let d = c.chords().into_iter().find(|d| d.root == 2).unwrap();
    assert_eq!(d.refused[&Chord::Triad], c.stamp(62.0, Chord::Triad, 0).unwrap_err());
    assert!(!d.refused.contains_key(&Chord::Sus2) && d.kinds.contains_key(&Chord::Sus2));
    for scale in ["major-pentatonic", "minor-pentatonic", "blues"] {
        for root in ROOTS {
            let k = key(root, scale);
            for d in k.chords() {
                for (chord, stamp) in &d.kinds {
                    for inv in 0..stamp.shapes.len() {
                        let p = 48.0 + d.root as f64;
                        for n in k.stamp(p, *chord, inv).unwrap() {
                            assert!(k.has(n as i32), "{} {scale}: {} inversion {inv} has {n}, out of the key", root, stamp.name);
                        }
                    }
                }
                assert_eq!(d.kinds.len() + d.refused.len(), Chord::ALL.len(), "{root} {scale} on {}: each kind offered or refused", d.name);
            }
        }
    }
}

// NOTE-4 (review N2): a sus chord is named by its second or fourth as it is: ♭2, 2, ♯2, ♭4, 4, ♯4;
// every kind on every degree of the harmonic and melodic minors and a mode.
#[test]
fn every_chord_of_the_minors_and_a_mode_is_named_as_it_sounds() {
    assert_eq!(
        stamps(&key("C", "harmonic-minor")),
        [
            "i Cm Cm(maj7) Csus2 Csus4 Cm(add9)",
            "ii° Ddim Dm7♭5 Dsus♭2♭5 Dsus4♭5 Ddim(add♭9)",
            "III+ E♭aug E♭aug(maj7) E♭sus2♯5 E♭sus4♯5 E♭aug(add9)",
            "iv Fm Fm7 Fsus2 Fsus♯4 Fm(add9)",
            "V G G7 Gsus♭2 Gsus4 Gadd♭9",
            "VI A♭ A♭maj7 A♭sus♯2 A♭sus♯4 A♭add♯9",
            "vii° Bdim Bdim7 Bsus♭2♭5 Bsus♭4♭5 Bdim(add♭9)",
        ]
    );
    assert_eq!(
        stamps(&key("C", "melodic-minor")),
        [
            "i Cm Cm(maj7) Csus2 Csus4 Cm(add9)",
            "ii Dm Dm7 Dsus♭2 Dsus4 Dm(add♭9)",
            "III+ E♭aug E♭aug(maj7) E♭sus2♯5 E♭sus♯4♯5 E♭aug(add9)",
            "IV F F7 Fsus2 Fsus♯4 Fadd9",
            "V G G7 Gsus2 Gsus4 Gadd9",
            "vi° Adim Am7♭5 Asus2♭5 Asus4♭5 Adim(add9)",
            "vii° Bdim Bm7♭5 Bsus♭2♭5 Bsus♭4♭5 Bdim(add♭9)",
        ]
    );
    assert_eq!(
        stamps(&key("C", "dorian")),
        [
            "i Cm Cm7 Csus2 Csus4 Cm(add9)",
            "ii Dm Dm7 Dsus♭2 Dsus4 Dm(add♭9)",
            "III E♭ E♭maj7 E♭sus2 E♭sus♯4 E♭add9",
            "IV F F7 Fsus2 Fsus4 Fadd9",
            "v Gm Gm7 Gsus2 Gsus4 Gm(add9)",
            "vi° Adim Am7♭5 Asus♭2♭5 Asus4♭5 Adim(add♭9)",
            "VII B♭ B♭maj7 B♭sus2 B♭sus4 B♭add9",
        ]
    );
    // The sus2 of A♭ in C harmonic minor has B, an augmented second, and no D.
    assert_eq!(voicings(&key("C", "harmonic-minor"), "G#3", Chord::Sus2)[0], "G#3 B3 D#4");
}

// Review N10: no note of any key is spelled with a double sharp or a double flat, which many fonts
// lack: where its own letter would need one, a note is spelled on its own, as a root is.
#[test]
fn no_note_is_spelled_with_a_double_sharp_or_flat() {
    for row in TABLE.iter() {
        for root in ROOTS {
            let k = key(root, row.name);
            let names: Vec<String> = k
                .names()
                .into_iter()
                .chain(
                    k.chords()
                        .into_iter()
                        .flat_map(|d| std::iter::once(d.name.clone()).chain(d.kinds.into_values().map(|s| s.name)).collect::<Vec<_>>()),
                )
                .collect();
            assert!(names.iter().all(|n| !n.contains('𝄪') && !n.contains('𝄫')), "{}: {names:?}", k.name());
        }
    }
    assert_eq!(key("Eb", "locrian").names().join(" "), "E♭ F♭ G♭ A♭ A C♭ D♭");
    assert_eq!(key("C#", "lydian").names().join(" "), "C♯ D♯ E♯ G G♯ A♯ B♯");
}

// Review N12: a chord's root is a pitch on the keyboard; one that would go past an end is refused,
// naming the root as a pitch.
#[test]
fn a_stamp_stays_on_the_keyboard() {
    let c = key("C", "major");
    assert_eq!(c.stamp(-5.0, Chord::Triad, 0).unwrap_err(), "A chord's root is a pitch from 0 (C-1) to 127 (G9), not -5");
    assert_eq!(c.stamp(128.0, Chord::Triad, 0).unwrap_err(), "A chord's root is a pitch from 0 (C-1) to 127 (G9), not 128");
    assert_eq!(c.stamp(127.4, Chord::Triad, 0).unwrap_err(), "G from G9 goes above the highest note, G9");
    assert_eq!(c.stamp(0.0, Chord::Triad, 0).unwrap(), [0.0, 4.0, 7.0]);
}

// LIB-1, review N7: a sample fits a song whose key has the notes of the sample's key: a mode by the
// major key with its notes (or that key's relative minor), a minor scale by its minor, a
// pentatonic or the blues by its major or minor; a chromatic key fits no key.
#[test]
fn a_sample_fits_by_the_keys_own_notes() {
    let fits = |root: &str, scale: &str| {
        key(root, scale).sample_key().map(|(r, minor)| format!("{} {}", pc_name(r), if minor { "minor" } else { "major" }))
    };
    assert_eq!(fits("C", "major").as_deref(), Some("C major"));
    assert_eq!(fits("A", "minor").as_deref(), Some("A minor"));
    assert_eq!(fits("D", "dorian").as_deref(), Some("C major"));
    assert_eq!(fits("E", "phrygian").as_deref(), Some("C major"));
    assert_eq!(fits("F", "lydian").as_deref(), Some("C major"));
    assert_eq!(fits("G", "mixolydian").as_deref(), Some("C major"));
    assert_eq!(fits("B", "locrian").as_deref(), Some("C major"));
    assert_eq!(fits("F#", "dorian").as_deref(), Some("E major"));
    assert_eq!(fits("A", "harmonic-minor").as_deref(), Some("A minor"));
    assert_eq!(fits("A", "melodic-minor").as_deref(), Some("A minor"));
    assert_eq!(fits("G", "major-pentatonic").as_deref(), Some("G major"));
    assert_eq!(fits("E", "minor-pentatonic").as_deref(), Some("E minor"));
    assert_eq!(fits("E", "blues").as_deref(), Some("E minor"));
    assert_eq!(fits("C", "chromatic"), None);
}

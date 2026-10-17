use super::*;

/// The bytes of `hex`, spaces and newlines aside.
fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()).collect()
}

/// A file: its header (format, ticks a beat) and its tracks' event bytes, each closed with an
/// End of Track.
fn file(format: u16, ppq: u16, tracks: &[&str]) -> Vec<u8> {
    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(format.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend(ppq.to_be_bytes());
    for t in tracks {
        let mut body = hex(t);
        body.extend(hex("00 FF 2F 00"));
        out.extend(b"MTrk");
        out.extend((body.len() as u32).to_be_bytes());
        out.extend(body);
    }
    out
}

fn note(channel: u8, key: u8, velocity: u8, start: u64, end: u64) -> SmfNote {
    SmfNote { channel, key, velocity, start, end }
}

/// SMF 1.0's own example (its Appendix 1.1): the same three notes and three programs as format 0
/// and as format 1, 96 ticks a beat, 4/4 at 120 bpm.
const SPEC_0: &str = "
    4D 54 68 64 00 00 00 06 00 00 00 01 00 60
    4D 54 72 6B 00 00 00 3B
    00 FF 58 04 04 02 18 08
    00 FF 51 03 07 A1 20
    00 C0 05
    00 C1 2E
    00 C2 46
    00 92 30 60
    00 3C 60
    60 91 43 40
    60 90 4C 20
    81 40 82 30 40
    00 3C 40
    00 81 43 40
    00 80 4C 40
    00 FF 2F 00";

const SPEC_1: &str = "
    4D 54 68 64 00 00 00 06 00 01 00 04 00 60
    4D 54 72 6B 00 00 00 14
    00 FF 58 04 04 02 18 08
    00 FF 51 03 07 A1 20
    83 00 FF 2F 00
    4D 54 72 6B 00 00 00 10
    00 C0 05
    81 40 90 4C 20
    81 40 4C 00
    00 FF 2F 00
    4D 54 72 6B 00 00 00 0F
    00 C1 2E
    60 91 43 40
    82 20 43 00
    00 FF 2F 00
    4D 54 72 6B 00 00 00 15
    00 C2 46
    00 92 30 60
    00 3C 60
    83 00 30 00
    00 3C 00
    00 FF 2F 00";

/// The example's notes, as the spec lists them: by start.
fn spec_notes() -> Vec<SmfNote> {
    vec![note(2, 48, 96, 0, 384), note(2, 60, 96, 0, 384), note(1, 67, 64, 96, 384), note(0, 76, 32, 192, 384)]
}

#[test]
fn reads_the_spec_example() {
    let zero = read(&hex(SPEC_0)).unwrap();
    assert_eq!((zero.format, zero.ppq, zero.tracks.len()), (0, 96, 1));
    let t = &zero.tracks[0];
    assert_eq!(t.events[..2], [(0, Event::Meter { beats: 4, unit_pow: 2 }), (0, Event::Tempo(500_000))]);
    let programs: Vec<_> = t.events.iter().filter(|e| matches!(e.1, Event::Program { .. })).copied().collect();
    assert_eq!(
        programs,
        [
            (0, Event::Program { channel: 0, program: 5 }),
            (0, Event::Program { channel: 1, program: 46 }),
            (0, Event::Program { channel: 2, program: 70 })
        ]
    );
    assert_eq!(t.notes(), spec_notes());
    assert_eq!(t.end, 384);

    let one = read(&hex(SPEC_1)).unwrap();
    assert_eq!((one.format, one.ppq, one.tracks.len()), (1, 96, 4));
    assert_eq!(one.tracks[0].events, [(0, Event::Meter { beats: 4, unit_pow: 2 }), (0, Event::Tempo(500_000))]);
    assert_eq!(one.tracks[0].end, 384);
    let mut all: Vec<SmfNote> = one.tracks.iter().flat_map(SmfTrack::notes).collect();
    all.sort_by_key(|n| (n.start, n.channel, n.key));
    let mut want = spec_notes();
    want.sort_by_key(|n| (n.start, n.channel, n.key));
    assert_eq!(all, want, "the same notes as format 0");
}

#[test]
fn reads_format_0_and_1() {
    // Format 0: one track, two channels.
    let f = file(0, 480, &["00 FF 03 04 4A 61 6D 21  00 90 3C 64  00 99 24 7F  83 60 80 3C 00  00 89 24 00"]);
    let smf = read(&f).unwrap();
    assert_eq!((smf.format, smf.ppq), (0, 480));
    assert_eq!(smf.tracks[0].name.as_deref(), Some("Jam!"));
    assert_eq!(smf.tracks[0].notes(), [note(0, 60, 100, 0, 480), note(9, 36, 127, 0, 480)]);
    // Format 1: a tempo track and two named tracks.
    let f = file(1, 960, &["00 FF 51 03 07 A1 20", "00 FF 03 04 42 61 73 73  00 90 28 50  87 40 80 28 40", "00 FF 03 04 4B 65 79 73"]);
    let smf = read(&f).unwrap();
    assert_eq!(smf.tracks.iter().map(|t| t.name.as_deref()).collect::<Vec<_>>(), [None, Some("Bass"), Some("Keys")]);
    assert_eq!(smf.tracks[1].notes(), [note(0, 40, 80, 0, 960)]);
    assert!(smf.tracks[2].notes().is_empty());
    // A chunk that isn't a track is skipped.
    let mut f = file(0, 96, &["00 90 3C 40 60 80 3C 40"]);
    let at = 14;
    f.splice(at..at, hex("58 59 5A 21 00 00 00 03 01 02 03"));
    assert_eq!(read(&f).unwrap().tracks[0].notes(), [note(0, 60, 64, 0, 96)]);
}

#[test]
fn running_status_and_sysex() {
    // Running status across a sysex and a meta event, as players read it.
    let f = file(0, 96, &["00 90 3C 40  00 F0 03 7E 7F F7  00 3E 40  00 FF 01 02 68 69  00 40 40  60 80 3C 00  00 3E 00  00 40 00"]);
    let notes = read(&f).unwrap().tracks[0].notes();
    assert_eq!(notes, [note(0, 60, 64, 0, 96), note(0, 62, 64, 0, 96), note(0, 64, 64, 0, 96)]);
    // Controllers, pitch bend and aftertouch are read past, and keep running status too.
    let f = file(0, 96, &["00 B0 07 64  00 0A 40  00 E0 00 40  00 D0 10  00 90 3C 40  60 3C 00"]);
    assert_eq!(read(&f).unwrap().tracks[0].notes(), [note(0, 60, 64, 0, 96)]);
}

#[test]
fn velocity_0_ends_a_note() {
    let f = file(0, 96, &["00 90 3C 40 60 90 3C 00 00 90 3E 20 30 90 3E 00"]);
    let t = &read(&f).unwrap().tracks[0];
    assert_eq!(t.events[1], (96, Event::NoteOff { channel: 0, key: 60 }));
    assert_eq!(t.notes(), [note(0, 60, 64, 0, 96), note(0, 62, 32, 96, 144)]);
}

#[test]
fn a_note_without_an_off_ends_at_the_next_on() {
    // C4 struck twice with no Off between, then never let go: the first ends at the second, the
    // second at the track's end. An Off with no On is ignored.
    let f = file(0, 96, &["00 90 3C 40  60 90 3C 50  00 80 3E 00  83 00 FF 01 00"]);
    let t = &read(&f).unwrap().tracks[0];
    assert_eq!(t.end, 480);
    assert_eq!(t.notes(), [note(0, 60, 64, 0, 96), note(0, 60, 80, 96, 480)]);
    // The same pitch on another channel is another note.
    let f = file(0, 96, &["00 90 3C 40  00 91 3C 40  60 80 3C 00"]);
    assert_eq!(read(&f).unwrap().tracks[0].notes(), [note(0, 60, 64, 0, 96), note(1, 60, 64, 0, 96)]);
    // A tap, its Off at its own tick, stays; an On repeated at its own tick doesn't.
    let f = file(0, 96, &["00 90 3C 40  00 80 3C 00  00 90 3E 40  00 90 3E 41  60 80 3E 00"]);
    assert_eq!(read(&f).unwrap().tracks[0].notes(), [note(0, 60, 64, 0, 0), note(0, 62, 65, 0, 96)]);
}

#[test]
fn tempo_and_meter_events() {
    let f = file(1, 96, &["00 FF 51 03 07 A1 20  00 FF 58 04 03 02 18 08  83 00 FF 51 03 09 27 C0  00 FF 58 04 06 03 18 08"]);
    let t = &read(&f).unwrap().tracks[0];
    assert_eq!(
        t.events,
        [
            (0, Event::Tempo(500_000)),
            (0, Event::Meter { beats: 3, unit_pow: 2 }),
            (384, Event::Tempo(600_000)),
            (384, Event::Meter { beats: 6, unit_pow: 3 })
        ]
    );
    // A short tempo or meter event is skipped, not misread.
    let f = file(0, 96, &["00 FF 51 02 07 A1  00 FF 58 01 04"]);
    assert!(read(&f).unwrap().tracks[0].events.is_empty());
}

#[test]
fn an_smpte_division_is_refused_in_words() {
    let mut f = file(0, 96, &[""]);
    f[12..14].copy_from_slice(&[0xe7, 0x28]);
    let e = read(&f).unwrap_err().to_string();
    assert_eq!(e, "counts time in SMPTE frames, which music2 doesn't read. Save it with beats (ticks per quarter note) instead.");
    let e = read(&file(0, 0, &[""])).unwrap_err().to_string();
    assert_eq!(e, "doesn't read as a MIDI file: its header says a beat has 0 ticks.");
}

#[test]
fn format_2_is_refused() {
    let e = read(&file(2, 96, &[""])).unwrap_err().to_string();
    assert_eq!(e, "is a format 2 MIDI file (a set of patterns), which music2 doesn't read. Save it as format 1.");
    assert!(read(&file(7, 96, &[""])).unwrap_err().to_string().contains("format 7"));
    let e = read(b"RIFF....WAVEfmt ").unwrap_err().to_string();
    assert_eq!(e, "isn't a MIDI file: it doesn't start with MThd.");
}

#[test]
fn a_cut_short_file_says_where() {
    let f = file(0, 96, &["00 90 3C 40 60 80 3C 40"]);
    // The chunk's own length says more than the file holds.
    let e = read(&f[..f.len() - 3]).unwrap_err().to_string();
    assert_eq!(e, "doesn't read as a MIDI file: a chunk at byte 14 runs past the file's end.");
    // An event cut off inside its chunk.
    let mut short = f.clone();
    short[21] = 6;
    short.truncate(22 + 6);
    let e = read(&short).unwrap_err().to_string();
    assert_eq!(e, "doesn't read as a MIDI file: it ends early, at byte 28.");
    // A header cut off.
    assert_eq!(read(&f[..10]).unwrap_err().to_string(), "doesn't read as a MIDI file: it ends early, at byte 10.");
    // A length of 5 bytes, and data with no status before it.
    let e = read(&file(0, 96, &["FF FF FF FF 7F 90 3C 40"])).unwrap_err().to_string();
    assert_eq!(e, "doesn't read as a MIDI file: a length at byte 22 is longer than 4 bytes.");
    let e = read(&file(0, 96, &["00 3C 40"])).unwrap_err().to_string();
    assert_eq!(e, "doesn't read as a MIDI file: the data at byte 23 has no event before it.");
}

#[test]
fn too_big_or_too_many_notes_is_refused_in_words() {
    let big = vec![0u8; MAX_BYTES + 1];
    assert_eq!(read(&big).unwrap_err().to_string(), "is over 16 MB: music2 brings in MIDI files up to 16 MB.");
    let mut many = String::new();
    for _ in 0..=MAX_NOTES / 2 {
        many.push_str("00 90 3C 40 00 3E 40 00 80 3C 00 00 80 3E 00 ");
    }
    let e = read(&file(0, 96, &[&many])).unwrap_err().to_string();
    assert_eq!(e, "has over 200,000 notes: music2 brings in 200,000 at most.");
}

#[test]
fn writes_what_it_reads() {
    for spec in [SPEC_0, SPEC_1] {
        let smf = read(&hex(spec)).unwrap();
        let again = read(&write(&smf)).unwrap();
        assert_eq!(again, smf);
    }
    // At one tick: tempo and meter, programs, Offs, then Ons (D37); the name first; full status.
    let smf = Smf {
        format: 1,
        ppq: 960,
        tracks: vec![SmfTrack {
            name: Some("Bass".into()),
            events: vec![
                (960, Event::NoteOn { channel: 1, key: 40, velocity: 100 }),
                (0, Event::NoteOn { channel: 1, key: 40, velocity: 90 }),
                (960, Event::NoteOff { channel: 1, key: 40 }),
                (0, Event::Program { channel: 1, program: 38 }),
                (1920, Event::NoteOff { channel: 1, key: 40 }),
            ],
            end: 0,
        }],
    };
    let bytes = write(&smf);
    assert_eq!(
        bytes[22..],
        hex("00 FF 03 04 42 61 73 73  00 C1 26  00 91 28 5A  87 40 81 28 40  00 91 28 64  87 40 81 28 40  00 FF 2F 00")[..]
    );
    assert_eq!(read(&bytes).unwrap().tracks[0].notes(), [note(1, 40, 90, 0, 960), note(1, 40, 100, 960, 1920)]);
    // A long gap's length takes 4 bytes; a tempo of 10 bpm and a 6/8 meter as they are.
    let smf = Smf {
        format: 0,
        ppq: 96,
        tracks: vec![SmfTrack {
            name: None,
            events: vec![(0, Event::Tempo(6_000_000)), (0, Event::Meter { beats: 6, unit_pow: 3 })],
            end: 0x0fff_ffff,
        }],
    };
    assert_eq!(read(&write(&smf)).unwrap(), smf);
}

/// A tiny xorshift: the same bytes every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn a_mangled_file_never_panics() {
    let good = hex(SPEC_1);
    // Every prefix: words or a file, never a panic.
    for n in 0..good.len() {
        let _ = read(&good[..n]).map(|s| s.tracks.iter().map(SmfTrack::notes).count());
    }
    // 2,000 files with a few bytes flipped.
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..2000 {
        let mut f = good.clone();
        for _ in 0..1 + rng.next() % 4 {
            let at = (rng.next() % f.len() as u64) as usize;
            f[at] = rng.next() as u8;
        }
        if let Ok(smf) = read(&f) {
            for t in &smf.tracks {
                let _ = t.notes();
            }
            let _ = read(&write(&smf));
        }
    }
}

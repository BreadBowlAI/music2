//! Samples (LIB-4, LIB-5, LIB-7): the guess, `samples.toml`, measuring and indexing, and notes.

use std::path::{Path, PathBuf};

use super::*;

/// A scratch folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("m2-samples-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 16-bit WAV of `seconds` of a sine at `amplitude`, in `channels` channels at 48 kHz.
/// A FLAC fixture's bytes, as libsndfile made it (`core/runtime/tests/fixtures/flac/make.py`).
pub(crate) fn flac(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/runtime/tests/fixtures/flac").join(name)).unwrap()
}

pub(crate) fn wav(path: &Path, channels: u16, seconds: f64, amplitude: f64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec = hound::WavSpec { channels, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..(seconds * 48_000.0) as usize {
        let x = amplitude * (std::f64::consts::TAU * 220.0 * i as f64 / 48_000.0).sin();
        for _ in 0..channels {
            w.write_sample((x * 32767.0).round() as i16).unwrap();
        }
    }
    w.finalize().unwrap();
}

fn key(text: &str) -> Option<Key> {
    key_said(text)
}

/// A path, and what it says: its type, instrument, tempo and key.
type Case = (&'static str, SampleType, Option<&'static str>, Option<f64>, Option<&'static str>);

// LIB-4: one function guesses a tempo, a key, loop or one-shot and an instrument from a path, as
// sample packs in the wild name their files.
#[test]
fn a_name_says_its_tempo_key_type_and_instrument() {
    use SampleType::{Loop, OneShot};
    let cases: &[Case] = &[
        ("loops/dusty keys 90 Am.wav", Loop, Some("keys"), Some(90.0), Some("Am")),
        ("Drums/Kicks/Kick 01.wav", OneShot, Some("kick"), None, None),
        ("OS_KICK_808_Punchy.wav", OneShot, Some("kick"), None, None),
        ("120bpm_Am_PianoChords.wav", Loop, Some("keys"), Some(120.0), Some("Am")),
        ("Loop 128bpm F#min.wav", Loop, None, Some(128.0), Some("F#m")),
        ("Vocal Chop - F# minor - 140 BPM.wav", Loop, Some("vocal"), Some(140.0), Some("F#m")),
        ("Hats/Closed Hat 3.wav", OneShot, Some("hat"), None, None),
        ("hi-hat_open_02.WAV", OneShot, Some("hat"), None, None),
        ("Bass Drum 1.wav", OneShot, Some("kick"), None, None),
        ("808 Bass C.wav", OneShot, Some("bass"), None, None),
        ("Synth Lead Bb maj 128.wav", Loop, Some("lead"), Some(128.0), Some("Bb")),
        ("Guitar Strum Loop 95 C#m.wav", Loop, Some("guitar"), Some(95.0), Some("C#m")),
        ("FX/Riser 8 bars.wav", OneShot, Some("fx"), None, None),
        ("Snare 100.wav", OneShot, Some("snare"), None, None),
        ("One Shots/Keys/Rhodes Chord Am.wav", OneShot, Some("keys"), None, Some("Am")),
        ("Clap_09.wav", OneShot, Some("clap"), None, None),
        ("Ride Cymbal.wav", OneShot, Some("cymbal"), None, None),
        ("Shaker Loop 100 BPM.wav", Loop, Some("perc"), Some(100.0), None),
        ("Kick 045.wav", OneShot, Some("kick"), None, None),
        ("Breakbeat 170 loop.wav", Loop, None, Some(170.0), None),
        ("Pad Ebm 80bpm.wav", Loop, Some("pad"), Some(80.0), Some("Ebm")),
        ("Texture - vinyl crackle.wav", OneShot, Some("texture"), None, None),
        ("Strings Ensemble D major.wav", OneShot, Some("strings"), None, Some("D")),
        ("brass stab Gm.wav", OneShot, Some("brass"), None, Some("Gm")),
        ("Funk Guitar 92 Gmin.wav", Loop, Some("guitar"), Some(92.0), Some("Gm")),
        ("TR-808 Cowbell.wav", OneShot, Some("perc"), None, None),
        ("foley/footsteps gravel.wav", OneShot, Some("foley"), None, None),
        ("Lead 128 BPM A minor.wav", Loop, Some("lead"), Some(128.0), Some("Am")),
        ("SubBass_Loop_C#_140.wav", Loop, Some("bass"), Some(140.0), Some("C#")),
        ("Loops/Drums/Groove 2.wav", Loop, None, None, None),
        ("pads/warm 70.wav", OneShot, Some("pad"), None, None),
        ("Loops/120 BPM/Top Loop.wav", Loop, None, Some(120.0), None),
        ("one-shots/stab 120 Am.wav", OneShot, Some("synth"), None, Some("Am")),
        ("Amp Noise.wav", OneShot, Some("texture"), None, None),
        ("kick -6dB.wav", OneShot, Some("kick"), None, None),
        ("Fm Synth Bell.wav", OneShot, Some("synth"), None, Some("Fm")),
        ("FM Synth Bell.wav", OneShot, Some("synth"), None, None),
        ("bass am.wav", OneShot, Some("bass"), None, None),
        ("bass c#m 90.wav", Loop, Some("bass"), Some(90.0), Some("C#m")),
        ("A minor chords loop 85.wav", Loop, None, Some(85.0), Some("Am")),
    ];
    for &(path, kind, instrument, bpm, k) in cases {
        let g = guess(path);
        assert_eq!((g.kind, g.instrument, g.bpm, g.key), (kind, instrument, bpm, k.and_then(key)), "{path}");
    }
}

// LIB-4: what a key typed in samples.toml may be.
#[test]
fn a_key_in_samples_toml_reads_as_written() {
    for (text, words) in
        [("Am", "A minor"), ("F#", "F# major"), ("Bb minor", "Bb minor"), ("C", "C major"), ("Ebm", "Eb minor"), ("G maj", "G major")]
    {
        assert_eq!(key_said(text).map(|k| k.words()), Some(words.to_string()), "{text}");
    }
    for text in ["", "H", "am", "A dorian", "C sharp"] {
        assert_eq!(key_said(text), None, "{text}");
    }
}

// LIB-4: samples.toml is read strictly, each problem in words.
#[test]
fn samples_toml_is_read_strictly() {
    let good = "[folder.\"drums/kicks\"]\ninstrument = \"kick\"\ntags = [\"punchy\"]\n\n\
                [file.\"loops/dusty keys 90 Am.wav\"]\ntype = \"loop\"\ninstrument = \"keys\"\nbpm = 90\nkey = \"Am\"\ntags = [\"lofi\"]\n";
    let t = SamplesToml::parse(good).unwrap();
    assert_eq!(t.folder["drums/kicks"].instrument.as_deref(), Some("kick"));
    assert_eq!(t.file["loops/dusty keys 90 Am.wav"].bpm, Some(90.0));
    let unknown = SamplesToml::parse("[file.\"a.wav\"]\ncolour = \"red\"\n").unwrap_err();
    assert!(unknown[0].contains("unknown field `colour`"), "{unknown:?}");
    let table = SamplesToml::parse("[files.\"a.wav\"]\ntype = \"loop\"\n").unwrap_err();
    assert!(table[0].contains("unknown field `files`"), "{table:?}");
    let bad = SamplesToml::parse(
        "[file.\"a.wav\"]\ntype = \"loops\"\ninstrument = \"cowbell\"\nbpm = 400\nkey = \"H minor\"\n\n[folder.\"b\"]\nbpm = 10\n",
    )
    .unwrap_err();
    assert_eq!(bad.len(), 5, "{bad:#?}");
    for (said, what) in [
        (0, "[folder.\"b\"]: bpm is 20 to 300, not 10"),
        (1, "type is loop or one-shot, not loops"),
        (2, "instrument cowbell isn't one of kick"),
    ] {
        assert!(bad[said].contains(what), "{bad:#?}");
    }
    assert!(bad[3].contains("bpm is 20 to 300, not 400") && bad[4].contains("key H minor doesn't read"), "{bad:#?}");
}

/// A pack named `@alice/loops` in `dir`, version 1.2.0, with a licence and an author.
fn pack(dir: &Path, samples_toml: Option<&str>) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"@alice/loops\"\nversion = \"1.2.0\"\nlicense = \"CC-BY-4.0\"\nauthors = [\"Alice Example\"]\n",
    )
    .unwrap();
    if let Some(t) = samples_toml {
        std::fs::write(dir.join("samples.toml"), t).unwrap();
    }
}

fn sample_of<'a>(found: &'a Indexed, id: &str) -> &'a Sample {
    let ids: Vec<&str> = found.items.iter().map(|i| i.id.as_str()).collect();
    found.items.iter().find(|i| i.id == id).and_then(|i| i.sample.as_ref()).unwrap_or_else(|| panic!("no {id} in {ids:?}"))
}

// LIB-4: what samples.toml says wins over the guess, a file's table over its folders', the deepest
// folder over the shallower; what it doesn't say is guessed; what is measured comes from the file.
#[test]
fn samples_toml_wins_over_the_guess() {
    let t = Scratch::new("toml");
    pack(
        &t.0,
        Some(
            "[folder.\"drums\"]\ninstrument = \"perc\"\ntags = [\"dry\"]\n\n[folder.\"drums/kicks\"]\ninstrument = \"kick\"\n\n\
             [file.\"loops/dusty keys 90 Am.wav\"]\nbpm = 91\ntags = [\"lofi\"]\n\n[file.\"drums/kicks/boom.wav\"]\ntype = \"loop\"\nbpm = 120\n",
        ),
    );
    wav(&t.0.join("samples/loops/dusty keys 90 Am.wav"), 2, 2.0, 0.5);
    wav(&t.0.join("samples/drums/kicks/boom.wav"), 1, 0.5, 0.25);
    wav(&t.0.join("samples/drums/snap 1.wav"), 1, 0.25, 1.0);
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    assert_eq!((found.problems.clone(), found.not_wav, found.unreadable.clone()), (vec![], 0, vec![]));
    let keys = sample_of(&found, "sample:@alice/loops:samples/loops/dusty keys 90 Am.wav");
    assert_eq!((keys.kind, keys.bpm, keys.key, keys.instrument.as_deref()), (SampleType::Loop, Some(91.0), key("Am"), Some("keys")));
    assert_eq!((keys.channels, keys.rate, keys.seconds), (2, 48_000, Some(2.0)));
    assert!((keys.peak - 0.5).abs() < 0.001, "{}", keys.peak);
    assert_eq!(keys.hash, m2_song::Sha::of_file(&t.0.join("samples/loops/dusty keys 90 Am.wav")).unwrap().hex());
    assert_eq!(
        keys.from,
        json!({"pack": "@alice/loops", "version": "1.2.0", "file": "samples/loops/dusty keys 90 Am.wav", "author": "Alice Example", "license": "CC-BY-4.0"})
    );
    let boom = sample_of(&found, "sample:@alice/loops:samples/drums/kicks/boom.wav");
    assert_eq!((boom.kind, boom.bpm, boom.instrument.as_deref(), boom.channels), (SampleType::Loop, Some(120.0), Some("kick"), 1));
    let snap = sample_of(&found, "sample:@alice/loops:samples/drums/snap 1.wav");
    assert_eq!((snap.kind, snap.instrument.as_deref()), (SampleType::OneShot, Some("perc")), "its folder says perc, over the guess (clap)");
    let item = found.items.iter().find(|i| i.label == "dusty keys 90 Am").unwrap();
    assert_eq!(
        (item.kind, item.package.as_str(), item.component.as_str()),
        (Kind::Sample, "@alice/loops", "@alice/loops:samples/loops/dusty keys 90 Am.wav")
    );
    assert!(item.tags.contains(&"lofi".to_string()) && item.tags.contains(&"loops".to_string()), "{:?}", item.tags);
    assert_eq!(item.description, "A loop, 91 bpm, A minor, 2.0 s, stereo");
    // A path it names that isn't there is said; a problem in it leaves the guess.
    pack(&t.0, Some("[file.\"gone.wav\"]\ninstrument = \"kick\"\n[folder.\"nowhere\"]\ninstrument = \"pad\"\n"));
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    assert_eq!(found.problems.len(), 2, "{:?}", found.problems);
    assert!(found.problems.iter().any(|p| p.contains("there is no WAV or FLAC file samples/gone.wav")), "{:?}", found.problems);
    assert!(found.items.is_empty(), "it names none of the files there: they are its instruments' recordings");
    // One that doesn't read lists nothing until it is fixed.
    pack(&t.0, Some("[file.\"drums/snap 1.wav\"]\nbpm = 500\n"));
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    assert!(found.problems[0].contains("bpm is 20 to 300") && found.items.is_empty(), "{:?}", found.problems);
    // Only what it names: a folder, or a file.
    pack(&t.0, Some("[folder.\"drums\"]\n"));
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    let labels: Vec<&str> = found.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["boom", "snap 1"]);
    assert_eq!(sample_of(&found, "sample:@alice/loops:samples/drums/snap 1.wav").instrument.as_deref(), Some("clap"), "guessed");
    // Without samples.toml, a pack's samples/ is its instruments' recordings alone.
    std::fs::remove_file(t.0.join("samples.toml")).unwrap();
    assert!(index(Root::Package { name: "@alice/loops", dir: &t.0 }, None).items.is_empty());
}

// LIB-4, FMT-24: only WAV and FLAC files are samples, in any capitals; other audio files are
// counted; a file that doesn't read is named, a FLAC that isn't one too; hidden folders and links
// are skipped.
#[test]
fn only_wav_and_flac_files_are_samples_and_the_rest_are_counted() {
    let t = Scratch::new("wav-only");
    wav(&t.0.join("Kick.WAV"), 1, 0.1, 0.5);
    wav(&t.0.join("sub/snare.wav"), 1, 0.1, 0.5);
    wav(&t.0.join(".hidden/clap.wav"), 1, 0.1, 0.5);
    std::fs::write(t.0.join("Pad.FLAC"), flac("16bit-stereo.flac")).unwrap();
    for f in ["loop.flac", "vox.mp3", "pad.aiff", "readme.txt"] {
        std::fs::write(t.0.join(f), b"not audio").unwrap();
    }
    std::fs::write(t.0.join("broken.wav"), b"RIFF....WAVE").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(t.0.join("sub"), t.0.join("again")).unwrap();
    let found = index(Root::Folder(&t.0), None);
    let labels: Vec<&str> = found.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Kick", "Pad", "snare"]);
    assert_eq!(found.not_wav, 2, "mp3 and aiff; not the text file, and FLAC is read");
    assert_eq!(found.unreadable.len(), 2, "{:?}", found.unreadable);
    assert!(found.unreadable[0].starts_with("broken.wav doesn't read as a WAV file"), "{:?}", found.unreadable);
    assert_eq!(found.unreadable[1], "loop.flac isn't a WAV or FLAC file", "told by its first bytes");
    let kick = &found.items[0];
    assert_eq!((kick.package.as_str(), kick.component.as_str()), ("yours", ""));
    assert_eq!(kick.id, format!("sample:folder:{}", t.0.join("Kick.WAV").display()));
    assert_eq!(kick.sample.as_ref().unwrap().from, json!({"folder": true}), "no path in a note");
    assert_eq!(kick.sample.as_ref().unwrap().file, t.0.join("Kick.WAV"));
}

// LIB-5: what is measured is kept in the cache by path, size and modified time: a folder indexed
// again reads no file, a changed file is read again, and a gone one leaves the cache.
#[test]
fn a_folder_indexed_again_reads_no_file() {
    let t = Scratch::new("cache");
    let (folder, cache) = (t.0.join("mine"), t.0.join("cache"));
    for i in 0..20 {
        wav(&folder.join(format!("kick {i}.wav")), 1, 0.05, 0.5);
    }
    let first = index(Root::Folder(&folder), Some(&cache));
    assert_eq!((first.items.len(), first.measured), (20, 20));
    let again = index(Root::Folder(&folder), Some(&cache));
    assert_eq!((again.items.clone(), again.measured), (first.items.clone(), 0), "nothing read the second time");
    // One file changed (another size), one gone.
    wav(&folder.join("kick 3.wav"), 2, 0.05, 0.5);
    std::fs::remove_file(folder.join("kick 4.wav")).unwrap();
    let changed = index(Root::Folder(&folder), Some(&cache));
    assert_eq!((changed.items.len(), changed.measured), (19, 1));
    assert_eq!(changed.items.iter().find(|i| i.label == "kick 3").unwrap().sample.as_ref().unwrap().channels, 2);
    assert_eq!(index(Root::Folder(&folder), Some(&cache)).measured, 0);
    let kept: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(kept.len(), 1, "one file per folder, no part file left: {kept:?}");
}

// LIB-7: a note is written beside its asset, create-only: one already there is kept.
#[test]
fn a_note_is_written_once_beside_its_asset() {
    let t = Scratch::new("note");
    std::fs::create_dir_all(t.0.join("assets")).unwrap();
    let s = Sample { from: json!({"pack": "std", "version": "0.5.0", "file": "samples/kit/kick.wav"}), ..Sample::default() };
    assert!(write_note(&t.0, "kick.wav", &note(&s)).unwrap());
    let text = std::fs::read_to_string(t.0.join("assets/kick.wav.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"from": {"pack": "std", "version": "0.5.0", "file": "samples/kit/kick.wav"}})
    );
    let other = Sample { from: json!({"folder": true}), ..Sample::default() };
    assert!(!write_note(&t.0, "kick.wav", &note(&other)).unwrap(), "kept");
    assert_eq!(std::fs::read_to_string(t.0.join("assets/kick.wav.json")).unwrap(), text);
}

// LIB-7 (review D5): a loop follows the song's tempo whenever its own is known, at the song's own
// tempo too (so a later tempo change moves it); its beats are its exact length, a whole number
// only within one sample of one: a tail never pulls its beats off the song's.
#[test]
fn a_loop_follows_with_its_exact_beats() {
    let looped = Sample { kind: SampleType::Loop, bpm: Some(90.0), ..Sample::default() };
    assert_eq!(follows(&looped), Some(90.0));
    assert_eq!(follows(&Sample { bpm: None, ..looped.clone() }), None);
    assert_eq!(follows(&Sample { kind: SampleType::OneShot, ..looped }), None);
    assert_eq!(loop_beats(10.0 + 2.0 / 3.0, 90.0, 48_000), 16.0);
    assert_eq!(loop_beats(512_000.0 / 48_000.0 + 1.0 / 48_000.0, 90.0, 48_000), 16.0, "one sample over 16 beats is 16");
    assert_eq!(loop_beats(8.3, 120.0, 48_000), 8.3 * 2.0, "a 16-beat loop with a tail of 0.6 beats is 16.6 beats, not 17");
    assert_eq!(loop_beats(10.75, 90.0, 48_000), 10.75 * 1.5);
    assert_eq!(loop_beats(1.0, 90.0, 48_000), 1.5);
}

/// A WAV's bytes from its `fmt ` chunk's body and its data, as a crafted file may have them.
fn raw_wav(fmt: &[u8], data: &[u8]) -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    body.extend(b"fmt ");
    body.extend((fmt.len() as u32).to_le_bytes());
    body.extend(fmt);
    body.extend(b"data");
    body.extend((data.len() as u32).to_le_bytes());
    body.extend(data);
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A `fmt ` chunk's body: its format, channels, rate, bytes a second, a frame's bytes and bits.
fn fmt_of(format: u16, channels: u16, rate: u32, bytes_a_second: u32, align: u16, bits: u16) -> Vec<u8> {
    let mut f = vec![];
    for (v, n) in [(format as u32, 2), (channels as u32, 2), (rate, 4), (bytes_a_second, 4), (align as u32, 2), (bits as u32, 2)] {
        f.extend(&v.to_le_bytes()[..n]);
    }
    f
}

/// The labels of what `found` lists, and their types.
fn kinds(found: &Indexed) -> Vec<(String, SampleType, Option<f64>)> {
    found.items.iter().map(|i| (i.label.clone(), i.sample.as_ref().unwrap().kind, i.sample.as_ref().unwrap().bpm)).collect()
}

// LIB-4 (review D4): a loop needs evidence of its own. A tempo in a folder's name (a construction
// kit's) gives a loop its tempo but makes no file a loop, and a file named with a tempo is a loop
// only if it lasts a bar at it: the kicks of a kit at 140 are one-shots, never warped.
#[test]
fn a_loop_needs_evidence_of_its_own() {
    use SampleType::{Loop, OneShot};
    let t = Scratch::new("evidence");
    for (path, seconds) in [
        ("Trap Kit 140 BPM/Kicks/Kick 1.wav", 0.43),
        ("Construction Kit 128 BPM/Drum Hits/Kick.wav", 0.3),
        ("Construction Kit 128 BPM/Loops/Drums.wav", 1.9),
        ("Trap Kick 140 BPM.wav", 0.43),
        ("Chord Stab Em 124.wav", 0.5),
        ("Keys 90 Am.wav", 2.7),
        ("Fill 120 bpm.wav", 2.0),
        ("Kick Loop 140.wav", 0.43),
    ] {
        wav(&t.0.join(path), 1, seconds, 0.5);
    }
    let found = index(Root::Folder(&t.0), None);
    let want = [
        ("Chord Stab Em 124", OneShot, None),
        ("Drums", Loop, Some(128.0)),
        ("Fill 120 bpm", Loop, Some(120.0)),
        ("Keys 90 Am", Loop, Some(90.0)),
        ("Kick", OneShot, None),
        ("Kick 1", OneShot, None),
        ("Kick Loop 140", Loop, Some(140.0)),
        ("Trap Kick 140 BPM", OneShot, None),
    ];
    let mut got = kinds(&found);
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got, want.map(|(l, k, b)| (l.to_string(), k, b)));
    for path in ["Trap Kit 140 BPM/Kicks/Kick 1.wav", "Kit 128 BPM/Drum Hits/Snare.wav", "Loops/One Shots/Kick 3.wav"] {
        assert_eq!(guess(path).kind, OneShot, "{path}");
    }
    // The nearest type word decides: the name's before its folders'.
    assert_eq!(guess("Kit 128 BPM/Loops/Groove.wav").kind, Loop);
    assert_eq!(guess("One Shots/Fx Loop 1.wav").kind, Loop);
}

// LIB-4 (review D13): words of a sentence and numbers with a unit are no keys or tempos.
#[test]
fn words_and_units_are_no_keys_or_tempos() {
    use SampleType::{Loop, OneShot};
    let cases: &[Case] = &[
        ("I Am Free 120.wav", OneShot, None, None, None),
        ("Vocal I Am 100.wav", OneShot, Some("vocal"), None, None),
        ("FmPiano 90.wav", OneShot, Some("keys"), None, None),
        ("Top Loop 24bit 96k.wav", Loop, None, None, None),
        ("Loop 100 Hz Sine.wav", Loop, None, None, None),
        ("90s Drum Loop.wav", Loop, None, None, None),
        ("Drums/808/01.wav", OneShot, None, None, None),
        // Still read as before.
        ("Amen Loop 170.wav", Loop, None, Some(170.0), None),
        ("808 Bass C.wav", OneShot, Some("bass"), None, None),
        ("Piano Am Loop 90.wav", Loop, Some("keys"), Some(90.0), Some("Am")),
        ("Fm Synth Bell.wav", OneShot, Some("synth"), None, Some("Fm")),
    ];
    for &(path, kind, instrument, bpm, k) in cases {
        let g = guess(path);
        assert_eq!((g.kind, g.instrument, g.bpm, g.key), (kind, instrument, bpm, k.and_then(key)), "{path}");
    }
}

// LIB-4 (review D14): samples.toml takes back a guess with `none`.
#[test]
fn samples_toml_takes_back_a_guess() {
    let t = Scratch::new("none");
    pack(&t.0, Some("[folder.\"loops\"]\ninstrument = \"none\"\n\n[file.\"loops/Kick Groove Am 120.wav\"]\nkey = \"none\"\n"));
    wav(&t.0.join("samples/loops/Kick Groove Am 120.wav"), 1, 2.0, 0.5);
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    assert_eq!(found.problems, Vec::<String>::new());
    let s = sample_of(&found, "sample:@alice/loops:samples/loops/Kick Groove Am 120.wav");
    assert_eq!((s.kind, s.bpm, s.key, s.instrument.as_deref()), (SampleType::Loop, Some(120.0), None, None));
    assert!(SamplesToml::parse("[file.\"a.wav\"]\nkey = \"nothing\"\n").is_err(), "only none");
}

// LIB-5 (review D2): a folder that isn't there for a while (a drive unplugged) keeps its cache,
// so nothing is measured again when it is back.
#[test]
fn a_missing_folder_keeps_its_cache() {
    let t = Scratch::new("missing");
    let (folder, away, cache) = (t.0.join("drive"), t.0.join("unplugged"), t.0.join("cache"));
    for i in 0..10 {
        wav(&folder.join(format!("kick {i}.wav")), 1, 0.05, 0.5);
    }
    assert_eq!(index(Root::Folder(&folder), Some(&cache)).measured, 10);
    std::fs::rename(&folder, &away).unwrap();
    let gone = index(Root::Folder(&folder), Some(&cache));
    assert!(gone.missing && gone.items.is_empty());
    std::fs::rename(&away, &folder).unwrap();
    let back = index(Root::Folder(&folder), Some(&cache));
    assert_eq!((back.items.len(), back.measured), (10, 0), "nothing measured again");
}

// LIB-5 (review D3): a file music2 can't play from the library (0 Hz, a sample that isn't a
// number) is unreadable, not listed; and a bad entry in the cache costs only itself.
#[test]
fn a_bad_file_costs_only_itself() {
    let t = Scratch::new("bad");
    let (folder, cache) = (t.0.join("mine"), t.0.join("cache"));
    wav(&folder.join("Kick 01.wav"), 1, 0.1, 0.5);
    wav(&folder.join("Snare 02.wav"), 1, 0.1, 0.5);
    let tone: Vec<u8> = (0..4800).flat_map(|i| (((i as f32 / 10.0).sin() * 8000.0) as i16).to_le_bytes()).collect();
    std::fs::write(folder.join("Zero Rate Snare.wav"), raw_wav(&fmt_of(1, 1, 0, 0, 2, 16), &tone)).unwrap();
    let inf: Vec<u8> = [0.5f32, f32::INFINITY, -0.25].iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(folder.join("Inf Hat.wav"), raw_wav(&fmt_of(3, 1, 48_000, 192_000, 4, 32), &inf)).unwrap();
    let found = index(Root::Folder(&folder), Some(&cache));
    let labels: Vec<&str> = found.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Kick 01", "Snare 02"]);
    assert_eq!(found.unreadable.len(), 2, "{:?}", found.unreadable);
    assert!(found.unreadable.iter().any(|u| u.starts_with("Zero Rate Snare.wav") && u.contains("0 Hz")), "{:?}", found.unreadable);
    assert!(found.unreadable.iter().any(|u| u.starts_with("Inf Hat.wav") && u.contains("isn't a number")), "{:?}", found.unreadable);
    assert_eq!(index(Root::Folder(&folder), Some(&cache)).measured, 2, "the two that don't read, again; the rest from the cache");
    // One entry spoilt by hand: only it is measured again.
    let file = cache_file(&cache, &folder);
    let text = std::fs::read_to_string(&file).unwrap();
    let at = text.find("\"seconds\":").unwrap();
    let spoilt = format!("{}\"seconds\":null,\"was\":{}", &text[..at], &text[at + "\"seconds\":".len()..]);
    std::fs::write(&file, spoilt).unwrap();
    assert_eq!(index(Root::Folder(&folder), Some(&cache)).measured, 3, "one kept file, and the two that don't read");
}

// LIB-5 (review D10): only plain files are read: a pipe named like a WAV never blocks the index.
#[cfg(unix)]
#[test]
fn only_plain_files_are_read() {
    let t = Scratch::new("fifo");
    wav(&t.0.join("Kick.wav"), 1, 0.1, 0.5);
    let made = std::process::Command::new("mkfifo").arg(t.0.join("trap.wav")).status().unwrap();
    assert!(made.success());
    let dir = t.0.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(index(Root::Folder(&dir), None)).unwrap());
    let found = rx.recv_timeout(std::time::Duration::from_secs(20)).expect("the index never ends: it opened the pipe");
    let labels: Vec<&str> = found.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!((labels, found.unreadable.len()), (vec!["Kick"], 0));
}

// LIB-4 (review D20): a sample of yours is named by its exact path: two names that differ only in
// bytes that aren't UTF-8 are two samples.
#[cfg(target_os = "linux")]
#[test]
fn ids_are_the_exact_paths() {
    use std::os::unix::ffi::OsStrExt;
    let t = Scratch::new("bytes");
    for name in [&b"kick \xff.wav"[..], &b"kick \xfe.wav"[..]] {
        wav(&t.0.join(std::ffi::OsStr::from_bytes(name)), 1, 0.05, 0.5);
    }
    wav(&t.0.join("snare.wav"), 1, 0.05, 0.5);
    let found = index(Root::Folder(&t.0), None);
    let ids: std::collections::BTreeSet<&str> = found.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(ids.contains(format!("sample:folder:{}", t.0.join("snare.wav").display()).as_str()), "a UTF-8 path is itself: {ids:?}");
    let again = index(Root::Folder(&t.0), None);
    assert_eq!(again.items.iter().map(|i| i.id.as_str()).collect::<std::collections::BTreeSet<_>>(), ids, "and stays so");
}

// LIB-5 (review D11): a crafted header (more valid bits than its samples hold) is unreadable, never
// a panic.
#[test]
fn a_crafted_header_is_unreadable() {
    let t = Scratch::new("crafted");
    let mut fmt = fmt_of(0xFFFE, 1, 48_000, 96_000, 2, 16);
    fmt.extend(22u16.to_le_bytes());
    fmt.extend(100u16.to_le_bytes());
    fmt.extend(4u32.to_le_bytes());
    fmt.extend([1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
    let data: Vec<u8> = (0..480i16).flat_map(|i| (i * 10).to_le_bytes()).collect();
    std::fs::write(t.0.join("Odd.wav"), raw_wav(&fmt, &data)).unwrap();
    let found = index(Root::Folder(&t.0), None);
    assert!(found.items.is_empty() && found.unreadable.len() == 1, "{:?}", found.unreadable);
}

// LIB-4, FMT-24: a FLAC sample is measured as the WAV of its samples is: its length, channels, rate
// and peak; its hash is the FLAC's own bytes'. Its rate and length are checked before it is decoded.
#[test]
fn a_flac_sample_is_measured_as_its_wav() {
    let t = Scratch::new("flac-measure");
    std::fs::create_dir_all(&t.0).unwrap();
    for name in ["24bit-stereo", "16bit-3ch", "8bit-mono"] {
        let (f, w) = (t.0.join(format!("{name}.flac")), t.0.join(format!("{name}.wav")));
        std::fs::write(&f, flac(&format!("{name}.flac"))).unwrap();
        std::fs::write(&w, flac(&format!("{name}.wav"))).unwrap();
        let (mf, mw) = (measure(&f).unwrap(), measure(&w).unwrap());
        assert_eq!((mf.seconds, mf.channels, mf.rate, mf.peak), (mw.seconds, mw.channels, mw.rate, mw.peak), "{name}");
        assert_eq!(mf.hash, m2_song::Sha::of(&std::fs::read(&f).unwrap()).hex(), "{name}");
    }
    let mut slow = flac("16bit-mono.flac");
    // STREAMINFO's rate, 20 bits from byte 18: 4,000 Hz.
    let rate: u32 = 4_000;
    slow[18] = (rate >> 12) as u8;
    slow[19] = (rate >> 4) as u8;
    slow[20] = (slow[20] & 0x0F) | ((rate as u8 & 0x0F) << 4);
    std::fs::write(t.0.join("slow.flac"), slow).unwrap();
    let why = measure(&t.0.join("slow.flac")).unwrap_err();
    assert_eq!(why, "doesn't read as a FLAC file (it plays at 4000 Hz: music2 takes files from 8,000 to 384,000 Hz)");
}

// LIB-7, FMT-24: a FLAC's note says what its WAV was made from; a note of yours with that, or an
// import's with only that, still credits no one, so a pack's note for the same bytes replaces it.
#[test]
fn a_flacs_note_of_yours_gives_way_to_a_packs() {
    let t = Scratch::new("flac-credit");
    std::fs::create_dir_all(t.0.join("assets")).unwrap();
    let made = json!({"file": "kick.flac", "format": "FLAC", "sha256": "00"});
    let packs = json!({"from": {"pack": "@alice/loops", "version": "1.2.0", "license": "CC-BY-4.0"}, "made_from": made});
    for yours in [json!({"from": {"folder": true}, "made_from": made}), json!({"made_from": made})] {
        let _ = std::fs::remove_file(t.0.join("assets/kick.wav.json"));
        assert!(write_note(&t.0, "kick.wav", &yours).unwrap());
        assert!(write_note(&t.0, "kick.wav", &packs).unwrap(), "the pack's credit replaces {yours}");
        let read = serde_json::from_slice::<Value>(&std::fs::read(t.0.join("assets/kick.wav.json")).unwrap()).unwrap();
        assert_eq!(read, packs);
    }
    // A note someone wrote by hand is theirs: kept.
    std::fs::write(t.0.join("assets/snare.wav.json"), r#"{"from": {"author": "me"}}"#).unwrap();
    assert!(!write_note(&t.0, "snare.wav", &packs).unwrap());
}

// LIB-7 (review D15): a note of yours, which credits no one, gives way to a pack's note for the
// same bytes; a pack's note is never replaced.
#[test]
fn a_folders_note_gives_way_to_a_packs() {
    let t = Scratch::new("credit");
    std::fs::create_dir_all(t.0.join("assets")).unwrap();
    let yours = Sample { from: json!({"folder": true}), ..Sample::default() };
    let packs = Sample { from: json!({"pack": "@alice/loops", "version": "1.2.0", "license": "CC-BY-4.0"}), ..Sample::default() };
    assert!(write_note(&t.0, "kick.wav", &note(&yours)).unwrap());
    assert!(write_note(&t.0, "kick.wav", &note(&packs)).unwrap(), "the pack's credit replaces it");
    let read = || serde_json::from_slice::<Value>(&std::fs::read(t.0.join("assets/kick.wav.json")).unwrap()).unwrap();
    assert_eq!(read(), note(&packs));
    assert!(!write_note(&t.0, "kick.wav", &note(&yours)).unwrap());
    let other = Sample { from: json!({"pack": "std", "version": "0.5.0"}), ..Sample::default() };
    assert!(!write_note(&t.0, "kick.wav", &note(&other)).unwrap(), "a pack's credit stays");
    assert_eq!(read(), note(&packs));
    let names: Vec<String> =
        std::fs::read_dir(t.0.join("assets")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["kick.wav.json"], "no part file left");
}

// LIB-5 (review D16): an index asked to stop ends between two files and writes no cache; the
// cache files of a folder removed, of a package's folder gone, and of none are pruned.
#[test]
fn a_stopped_index_writes_nothing_and_caches_are_pruned() {
    let t = Scratch::new("prune");
    let (mine, other, cache) = (t.0.join("mine"), t.0.join("other"), t.0.join("cache"));
    wav(&mine.join("kick.wav"), 1, 0.05, 0.5);
    wav(&other.join("snare.wav"), 1, 0.05, 0.5);
    let stopped = index_until(Root::Folder(&mine), Some(&cache), &AtomicBool::new(true));
    assert!(stopped.stopped && stopped.items.is_empty());
    assert!(!cache.exists(), "nothing written");
    index(Root::Folder(&mine), Some(&cache));
    index(Root::Folder(&other), Some(&cache));
    let pack_dir = t.0.join("pack");
    pack(&pack_dir, Some("[folder.\"kicks\"]\n"));
    wav(&pack_dir.join("samples/kicks/boom.wav"), 1, 0.05, 0.5);
    index(Root::Package { name: "@alice/loops", dir: &pack_dir }, Some(&cache));
    std::fs::write(cache.join("0123456789abcdef.json"), "{\"files\": {}}").unwrap();
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 4);
    assert_eq!(prune(&cache, &[mine.clone(), other.clone()]), 1, "only the one that names no folder");
    std::fs::remove_dir_all(&pack_dir).unwrap();
    assert_eq!(prune(&cache, std::slice::from_ref(&mine)), 2, "the other folder, and the pack gone");
    let left: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().map(|e| e.path()).collect();
    assert_eq!(left, [cache_file(&cache, &mine)]);
}

// LIB-5 (review D12): a package whose `samples` is a link lists none: it could point anywhere.
#[cfg(unix)]
#[test]
fn a_packages_samples_through_a_link_list_none() {
    let t = Scratch::new("link");
    let (dir, elsewhere) = (t.0.join("pack"), t.0.join("elsewhere"));
    pack(&dir, Some("[folder.\"kicks\"]\n"));
    wav(&elsewhere.join("kicks/boom.wav"), 1, 0.05, 0.5);
    std::os::unix::fs::symlink(&elsewhere, dir.join("samples")).unwrap();
    let found = index(Root::Package { name: "@alice/loops", dir: &dir }, None);
    assert!(found.items.is_empty(), "{:?}", found.items);
    assert!(found.problems.iter().any(|p| p.contains("is a link")), "{:?}", found.problems);
}

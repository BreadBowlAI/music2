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
    assert!(found.problems.iter().any(|p| p.contains("there is no WAV file samples/gone.wav")), "{:?}", found.problems);
    pack(&t.0, Some("[file.\"drums/snap 1.wav\"]\nbpm = 500\n"));
    let found = index(Root::Package { name: "@alice/loops", dir: &t.0 }, None);
    assert!(found.problems[0].contains("bpm is 20 to 300"), "{:?}", found.problems);
    assert_eq!(sample_of(&found, "sample:@alice/loops:samples/drums/snap 1.wav").instrument.as_deref(), Some("clap"), "guessed");
}

// LIB-4: only WAV files are samples, in any capitals; other audio files are counted; a WAV that
// doesn't read is named; hidden folders and links are skipped.
#[test]
fn only_wav_files_are_samples_and_the_rest_are_counted() {
    let t = Scratch::new("wav-only");
    wav(&t.0.join("Kick.WAV"), 1, 0.1, 0.5);
    wav(&t.0.join("sub/snare.wav"), 1, 0.1, 0.5);
    wav(&t.0.join(".hidden/clap.wav"), 1, 0.1, 0.5);
    for f in ["loop.flac", "vox.mp3", "pad.aiff", "readme.txt"] {
        std::fs::write(t.0.join(f), b"not audio").unwrap();
    }
    std::fs::write(t.0.join("broken.wav"), b"RIFF....WAVE").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(t.0.join("sub"), t.0.join("again")).unwrap();
    let found = index(Root::Folder(&t.0), None);
    let labels: Vec<&str> = found.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Kick", "snare"]);
    assert_eq!(found.not_wav, 3, "flac, mp3 and aiff; not the text file");
    assert!(
        found.unreadable.len() == 1 && found.unreadable[0].starts_with("broken.wav doesn't read as a WAV file"),
        "{:?}",
        found.unreadable
    );
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

// LIB-7: a loop follows the song's tempo when its own is known and differs; its beats are a whole
// number when within 5% of one.
#[test]
fn a_loop_at_another_tempo_follows_with_whole_beats() {
    let looped = Sample { kind: SampleType::Loop, bpm: Some(90.0), ..Sample::default() };
    assert_eq!(follow_at(&looped, 120.0), Some(90.0));
    assert_eq!(follow_at(&looped, 90.0), None, "the song's own tempo: a plain clip");
    assert_eq!(follow_at(&Sample { bpm: None, ..looped.clone() }, 120.0), None);
    assert_eq!(follow_at(&Sample { kind: SampleType::OneShot, ..looped }, 120.0), None);
    assert_eq!(loop_beats(10.0 + 2.0 / 3.0, 90.0), 16.0);
    assert_eq!(loop_beats(10.75, 90.0), 16.0, "a rounded tempo still lands on 16 beats");
    assert_eq!(loop_beats(1.0, 90.0), 1.5, "1.5 beats is no whole number");
}

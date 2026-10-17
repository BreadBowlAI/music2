//! Bringing audio files in (A2): the copy (create-only, a numbered name, the same bytes used again,
//! refusals in words that leave nothing), and the placing (one Tx, the file taken in first).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use m2_session::Session;
use m2_song::{LaneData, Stored};

use super::*;
use crate::testing::{Fixture, fixture};

/// What a test runs mid-copy.
type During = Option<Box<dyn Fn(&Path)>>;

thread_local! {
    /// Run with the part file, after it is filled and pushed and before it is decoded and named.
    pub(super) static DURING: RefCell<During> = const { RefCell::new(None) };
}

/// A 16-bit WAV of `channels` at `rate`, `seconds` long, each sample from `f(frame, channel)`.
fn wav_bytes(channels: u16, rate: u32, seconds: f64, f: impl Fn(usize, u16) -> f32) -> Vec<u8> {
    let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..(seconds * rate as f64).round() as usize {
        for c in 0..channels {
            w.write_sample((f(i, c).clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
    }
    w.finalize().unwrap();
    out.into_inner()
}

/// A tone: a file whose bytes differ by `seed`.
fn tone(seconds: f64, seed: f32) -> Vec<u8> {
    wav_bytes(1, 48_000, seconds, |i, _| 0.5 * (i as f32 * 0.01 + seed).sin())
}

/// A file `name` holding `bytes` in a folder of its own, outside the song.
fn outside(name: &str, bytes: &[u8]) -> (Fixture, PathBuf) {
    let f = Fixture::new();
    f.write(name, bytes);
    let p = f.0.join(name);
    (f, p)
}

/// The names in `assets/`, dot files too, sorted.
fn assets(f: &Fixture) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(f.0.join("assets")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn bytes(name: &str, b: Vec<u8>) -> Source {
    Source::Bytes { name: name.into(), bytes: b }
}

fn refusal(r: Result<(Imported, Option<m2_runtime::audio::Asset>), EditError>) -> String {
    match r {
        // The page says a refusal without its "edit rejected: ".
        Err(e) => e.to_string().trim_start_matches("edit rejected: ").to_string(),
        Ok((i, _)) => panic!("imported: {i:?}"),
    }
}

#[test]
fn an_import_copies_the_file_create_only() {
    let song = fixture("examples/resample");
    let wav = tone(1.0, 0.0);
    let (_out, path) = outside("Lead Vox.WAV", &wav);
    let (i, decoded) = audio(&song.0, Source::Path(path.clone())).unwrap();
    assert_eq!(i.asset.as_str(), "Lead Vox.wav");
    assert_eq!((i.name.as_str(), i.dropped_channels, i.reused), ("Lead Vox.WAV", 0, false));
    assert_eq!(i.hash, m2_song::Sha::of(&wav));
    assert!((i.seconds - 1.0).abs() < 1e-9, "{}", i.seconds);
    assert_eq!(std::fs::read(song.0.join("assets/Lead Vox.wav")).unwrap(), wav, "a copy, byte for byte");
    assert_eq!(decoded.unwrap().len(), 48_000, "decoded once, for its peaks");
    assert_eq!(assets(&song), ["Lead Vox.wav", "keys-bounce.wav", "keys-bounce.wav.json"], "no part file left");
    assert!(path.exists(), "the file itself stays where it was");
    // A song with no assets/ yet gets one.
    let empty = fixture("examples/first-song");
    let (i, _) = audio(&empty.0, bytes("kick.wav", tone(0.5, 1.0))).unwrap();
    assert_eq!((i.asset.as_str(), assets(&empty)), ("kick.wav", vec!["kick.wav".to_string()]));
}

#[test]
fn a_taken_name_gets_a_number() {
    let song = fixture("examples/resample");
    let (a, _) = audio(&song.0, bytes("keys-bounce.wav", tone(0.5, 2.0))).unwrap();
    let (b, _) = audio(&song.0, bytes("KEYS-BOUNCE.WAV", tone(0.5, 3.0))).unwrap();
    assert_eq!((a.asset.as_str(), b.asset.as_str()), ("keys-bounce 2.wav", "KEYS-BOUNCE 3.wav"), "taken in any capitals");
    assert_eq!(std::fs::read(song.0.join("assets/keys-bounce.wav")).unwrap(), std::fs::read(fixture("examples/resample").0.join("assets/keys-bounce.wav")).unwrap(), "never replaced");
}

#[test]
fn the_same_bytes_use_the_asset_there() {
    let song = fixture("examples/resample");
    let was = std::fs::read(song.0.join("assets/keys-bounce.wav")).unwrap();
    let (_out, path) = outside("my keys.wav", &was);
    let (i, decoded) = audio(&song.0, Source::Path(path)).unwrap();
    assert_eq!((i.asset.as_str(), i.reused), ("keys-bounce.wav", true));
    assert!(decoded.is_none(), "nothing copied, nothing decoded");
    assert_eq!(assets(&song), ["keys-bounce.wav", "keys-bounce.wav.json"]);
    // In a folder of assets/ too.
    song.write("assets/loops/drums.wav", tone(0.25, 9.0));
    let (i, _) = audio(&song.0, bytes("again.wav", tone(0.25, 9.0))).unwrap();
    assert_eq!((i.asset.as_str(), i.reused), ("loops/drums.wav", true));
}

#[test]
fn a_name_is_made_safe_and_nfc() {
    let song = fixture("examples/resample");
    let (i, _) = audio(&song.0, bytes(".take:1/cafe\u{301}.Wav", tone(0.25, 4.0))).unwrap();
    assert_eq!(i.asset.as_str(), "take-1-café.wav");
    assert!(song.0.join("assets/take-1-café.wav").exists());
}

#[test]
fn a_file_that_isnt_wav_is_refused_in_words() {
    let song = fixture("examples/resample");
    let mp3 = b"ID3\x04\x00\x00\x00\x00\x00\x00some mp3 frames".to_vec();
    let words = refusal(audio(&song.0, bytes("song.mp3", mp3)));
    assert_eq!(words, "song.mp3 isn't a WAV file. music2 reads WAV files. Save it as a WAV first: most audio apps can (Export, then WAV).");
    // Taken as WAV by its header, not its name.
    let named = refusal(audio(&song.0, bytes("fake.wav", b"just some words, not sound".to_vec())));
    assert!(named.starts_with("fake.wav isn't a WAV file."), "{named}");
    let (i, _) = audio(&song.0, bytes("real.mp3", tone(0.25, 5.0))).unwrap();
    assert_eq!(i.asset.as_str(), "real.mp3", "a WAV with another name is a WAV");
    let silent = refusal(audio(&song.0, bytes("empty.wav", wav_bytes(1, 48_000, 0.0, |_, _| 0.0))));
    assert_eq!(silent, "empty.wav has no sound in it.");
    let slow = refusal(audio(&song.0, bytes("slow.wav", wav_bytes(1, 4_000, 0.1, |_, _| 0.0))));
    assert_eq!(slow, "slow.wav plays at 4000 Hz: music2 takes files from 8,000 to 384,000 Hz.");
    assert_eq!(assets(&song), ["keys-bounce.wav", "keys-bounce.wav.json", "real.mp3"], "nothing else written");
}

#[test]
fn a_wav_that_doesnt_decode_leaves_nothing() {
    let song = fixture("examples/resample");
    // Its header promises a second, its data stops after a tenth.
    let mut cut = tone(1.0, 6.0);
    cut.truncate(44 + 9_600);
    let words = refusal(audio(&song.0, bytes("cut.wav", cut)));
    assert!(words.starts_with("cut.wav doesn't read as a WAV file ("), "{words}");
    assert_eq!(assets(&song), ["keys-bounce.wav", "keys-bounce.wav.json"], "no part, no file");
}

#[test]
fn a_file_over_15_minutes_is_refused() {
    let song = fixture("examples/resample");
    // A header that says 16 minutes of 8 kHz mono 8-bit; the bytes themselves are few.
    let frames: u32 = 16 * 60 * 8_000;
    let mut b = vec![];
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + frames).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8_000u32.to_le_bytes());
    b.extend_from_slice(&8_000u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&frames.to_le_bytes());
    b.extend_from_slice(&[128; 64]);
    let words = refusal(audio(&song.0, bytes("long.wav", b)));
    assert_eq!(words, "long.wav is longer than 15 minutes: music2 keeps audio in memory for now, so it takes files up to 15 minutes.");
    assert_eq!(assets(&song), ["keys-bounce.wav", "keys-bounce.wav.json"]);
}

// Q1: more than two channels: the first two play, and the import says so.
#[test]
fn extra_channels_are_counted() {
    let song = fixture("examples/resample");
    let (i, decoded) = audio(&song.0, bytes("six.wav", wav_bytes(6, 48_000, 0.1, |_, c| c as f32 / 10.0))).unwrap();
    assert_eq!(i.dropped_channels, 4);
    assert_eq!(decoded.unwrap().channels.len(), 2);
}

// D2: a part file is locked from the moment it is made: a render's or another import's sweep of
// `assets/` never takes it halfway.
#[test]
fn a_part_being_copied_is_never_swept() {
    let song = fixture("examples/resample");
    let folder = song.0.join("assets");
    DURING.with(|d| {
        *d.borrow_mut() = Some(Box::new(move |part: &Path| {
            crate::render::sweep(&folder);
            assert!(part.exists(), "the sweep left the part being copied");
        }))
    });
    let done = audio(&song.0, bytes("vox.wav", tone(0.5, 7.0)));
    DURING.with(|d| *d.borrow_mut() = None);
    assert_eq!(done.unwrap().0.asset.as_str(), "vox.wav");
    // A part nobody holds (a killed import's) is swept by the next.
    std::fs::write(song.0.join("assets/.old.wav.1.part"), b"half").unwrap();
    audio(&song.0, bytes("next.wav", tone(0.5, 8.0))).unwrap();
    assert!(!assets(&song).iter().any(|n| n.ends_with(".part")), "{:?}", assets(&song));
}

#[test]
fn two_imports_of_one_name_both_land() {
    let song = fixture("examples/resample");
    let a = audio(&song.0, bytes("vox.wav", tone(0.5, 10.0))).unwrap().0;
    let b = audio(&song.0, bytes("vox.wav", tone(0.5, 11.0))).unwrap().0;
    assert_eq!((a.asset.as_str(), b.asset.as_str()), ("vox.wav", "vox 2.wav"));
    // The name taken between the part's copy and its naming: the next number.
    let folder = song.0.join("assets");
    DURING.with(|d| *d.borrow_mut() = Some(Box::new(move |_: &Path| std::fs::write(folder.join("drum.wav"), b"there first").unwrap())));
    let c = audio(&song.0, bytes("drum.wav", tone(0.5, 12.0)));
    DURING.with(|d| *d.borrow_mut() = None);
    assert_eq!(c.unwrap().0.asset.as_str(), "drum 2.wav");
    assert_eq!(std::fs::read(song.0.join("assets/drum.wav")).unwrap(), b"there first", "never replaced");
}

fn chops(s: &Session) -> Vec<m2_song::AudioClip> {
    match s.accepted().compiled.data.lane("chops", "clips").map(|d| (**d).clone()) {
        Some(LaneData::Audio(c)) => c,
        other => panic!("{other:?}"),
    }
}

fn lane_target(beat: f64) -> Target {
    Target::Lane { module: "chops".into(), lane: "clips".into(), beat }
}

#[test]
fn a_drop_on_an_audio_lane_is_one_clips_tx() {
    let song = fixture("examples/resample");
    let mut s = song.open();
    let before = chops(&s).len();
    let a = audio(&song.0, bytes("one.wav", tone(1.0, 13.0))).unwrap().0;
    let b = audio(&song.0, bytes("two.wav", tone(0.5, 14.0))).unwrap().0;
    let placed = place(&mut s, &[a.clone(), b.clone()], &lane_target(16.0)).unwrap();
    let now = chops(&s);
    assert_eq!(now.len(), before + 2);
    let made: Vec<&m2_song::AudioClip> = now.iter().filter(|c| placed.made.iter().any(|(_, id)| c.id.as_deref() == Some(id))).collect();
    assert_eq!(made.len(), 2, "{placed:?}");
    // One after another: 1 s is 2 beats at 120 BPM, then half a second.
    assert_eq!((made[0].asset.as_str(), made[0].start, made[0].length), ("one.wav", 16.0, 2.0));
    assert_eq!((made[1].asset.as_str(), made[1].start, made[1].length), ("two.wav", 18.0, 1.0));
    assert_eq!(made[0].hash.as_deref(), Some(a.hash.pin().as_str()));
    assert!((made[0].offset, made[0].gain_db, made[0].fade_in, made[0].reverse) == (0.0, 0.0, 0.0, false));
    let text = song.read("data/chops/clips.json");
    let data = LaneData::read(&text, &m2_song::LaneKind::Audio).unwrap();
    assert_eq!(m2_song::canonical(&data), text, "the lane file is canonical");
    // One undo takes both back; the files stay (Q4).
    crate::undo(&mut s).unwrap();
    assert_eq!(chops(&s).len(), before);
    assert!(song.0.join("assets/one.wav").exists() && song.0.join("assets/two.wav").exists());
    // Not onto a notes lane, in words.
    let why = place(&mut s, &[a], &Target::Lane { module: "keys".into(), lane: "chords".into(), beat: 0.0 }).unwrap_err().to_string();
    assert!(why.contains("drop audio on an audio track"), "{why}");
}

// D1 (A.2): right after the import the accepted revision lists the file by its hash and no lane
// says it is missing, so the clip plays it at once.
#[test]
fn an_import_commits_a_revision_that_plays_the_asset() {
    let song = fixture("examples/resample");
    let mut s = song.open();
    let i = audio(&song.0, bytes("fresh.wav", tone(1.0, 15.0))).unwrap().0;
    place(&mut s, std::slice::from_ref(&i), &lane_target(20.0)).unwrap();
    let rev = s.accepted();
    assert_eq!(rev.snapshot.binaries.get(Path::new("assets/fresh.wav")), Some(&Stored::Hash(i.hash)));
    let missing: Vec<_> = rev.compiled.problems.iter().filter(|d| d.code == "asset.missing").collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn the_clip_is_the_files_length_at_the_tempo_there() {
    let song = fixture("examples/resample");
    // 120 BPM to beat 4, then 60.
    song.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 60}], "meter": []}"#);
    let mut s = song.open();
    let i = audio(&song.0, bytes("two.wav", tone(2.0, 16.0))).unwrap().0;
    place(&mut s, std::slice::from_ref(&i), &lane_target(0.0)).unwrap();
    place(&mut s, std::slice::from_ref(&i), &lane_target(2.0)).unwrap();
    let ours: Vec<(f64, f64)> = chops(&s).iter().filter(|c| c.asset == "two.wav").map(|c| (c.start, c.length)).collect();
    // From 0: 2 s at 120 is 4 beats. From 2: 1 s takes it to beat 4, the next second at 60 one beat more.
    assert_eq!(ours, [(0.0, 4.0), (2.0, 3.0)]);
    assert_eq!(length_at(&TempoMap::constant(120.0), 8.0, 2.0), 4.0);
}

#[test]
fn a_file_name_makes_a_track_name() {
    let i = |name: &str| Imported { asset: AssetName::new("x.wav").unwrap(), name: name.into(), hash: m2_song::Sha::of(b""), seconds: 1.0, dropped_channels: 0, reused: false };
    assert_eq!(track_label(&i("lead vox.wav")), "Lead vox");
    assert_eq!(track_label(&i("ベース.wav")), "ベース");
    assert_eq!(track_label(&i(".wav")), "Audio");
}

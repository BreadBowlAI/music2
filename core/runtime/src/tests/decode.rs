//! FMT-24: WAV and FLAC through one door, told apart by their first bytes, a FLAC giving the bits of
//! the WAV of its samples. The fixtures are libsndfile's (`tests/fixtures/flac/make.py`): each FLAC
//! beside the WAV it wrote from the same numbers.

use std::path::{Path, PathBuf};

use crate::decode::{self, Flac, Kind};

/// The fixtures: each FLAC and the WAV of its samples, as `(bits, channels, name)`.
const PAIRS: [(u16, u16, &str); 6] = [
    (8, 1, "8bit-mono"),
    (16, 1, "16bit-mono"),
    (16, 2, "16bit-stereo"),
    (24, 1, "24bit-mono"),
    (24, 2, "24bit-stereo"),
    (16, 3, "16bit-3ch"),
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flac").join(name)
}

/// A scratch folder of this test's own.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("m2-decode-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Each channel's samples as their bits.
fn bits(a: &crate::audio::Asset) -> Vec<Vec<u32>> {
    a.channels.iter().map(|c| c.iter().map(|s| s.to_bits()).collect()).collect()
}

/// The FLAC `name` with its bytes changed by `f`, written to `dir`.
fn patched(dir: &Path, name: &str, f: impl FnOnce(&mut Vec<u8>)) -> PathBuf {
    let mut b = std::fs::read(fixture(name)).unwrap();
    f(&mut b);
    let out = dir.join(name);
    std::fs::write(&out, b).unwrap();
    out
}

/// STREAMINFO (bytes 8 to 42): its bits in a sample, 5 bits less one, from byte 20's last bit.
fn set_bits(b: &mut [u8], bits: u8) {
    let v = bits - 1;
    b[20] = (b[20] & 0xFE) | (v >> 4);
    b[21] = (b[21] & 0x0F) | ((v & 0x0F) << 4);
}

/// STREAMINFO's frames: 36 bits, from byte 21's low half.
fn set_frames(b: &mut [u8], frames: u64) {
    b[21] = (b[21] & 0xF0) | ((frames >> 32) as u8 & 0x0F);
    b[22..26].copy_from_slice(&(frames as u32).to_be_bytes());
}

// FMT-24: a FLAC decodes to the very bits of the WAV of the same samples, at 8, 16 and 24 bits,
// mono, stereo and 3 channels (its first two kept, as a WAV's), whole and block by block.
#[test]
fn a_flac_gives_the_bits_of_the_wav_of_its_samples() {
    for (b, ch, name) in PAIRS {
        let flac = decode::load(&fixture(&format!("{name}.flac"))).unwrap();
        let wav = decode::load(&fixture(&format!("{name}.wav"))).unwrap();
        assert_eq!(flac.sample_rate, 48_000.0, "{name}");
        assert_eq!(flac.channels.len(), ch.min(2) as usize, "{name}: its first two channels");
        assert_eq!(flac.len(), 4_800, "{name}");
        assert!(bits(&flac) == bits(&wav), "{name}: the WAV's bits");
        // The edges of the scale: the loudest and quietest numbers the bits hold.
        assert_eq!(flac.channels[0][10], 1.0 - decode::scale(b), "{name}");
        assert_eq!(flac.channels[0][20], -1.0, "{name}");
        let mut f = Flac::new(std::fs::File::open(fixture(&format!("{name}.flac"))).unwrap()).unwrap();
        assert_eq!((f.bits, f.channels, f.rate, f.frames), (b, ch, 48_000, 4_800), "{name}: its header");
        let (mut block, mut all) = (vec![], vec![vec![]; ch as usize]);
        while f.next(&mut block).unwrap() > 0 {
            for (a, c) in all.iter_mut().zip(&block) {
                a.extend(c.iter().map(|&v| v as f32 * decode::scale(b)));
            }
        }
        assert!(all[..ch.min(2) as usize] == flac.channels[..], "{name}: block by block, the same");
    }
}

// FMT-24: the kind is the first bytes', never the name's: a FLAC named `.wav` is a FLAC, a WAV
// named `.flac` a WAV, and anything else is neither.
#[test]
fn the_kind_is_told_by_the_first_bytes() {
    assert_eq!(decode::kind(b"RIFF\0\0\0\0WAVEfmt "), Some(Kind::Wav));
    assert_eq!(decode::kind(b"fLaC\0\0\0\x22"), Some(Kind::Flac));
    for not in [&b"RIFF\0\0\0\0AVI "[..], b"ID3\x04", b"fLa", b"OggS", b""] {
        assert_eq!(decode::kind(not), None, "{not:?}");
    }
    let d = scratch("kind");
    std::fs::copy(fixture("16bit-stereo.flac"), d.join("named.wav")).unwrap();
    std::fs::copy(fixture("16bit-stereo.wav"), d.join("named.flac")).unwrap();
    assert_eq!(decode::kind_of(&d.join("named.wav")).unwrap(), Some(Kind::Flac));
    assert_eq!(decode::kind_of(&d.join("named.flac")).unwrap(), Some(Kind::Wav));
    let want = bits(&decode::load(&fixture("16bit-stereo.wav")).unwrap());
    assert!(bits(&decode::load(&d.join("named.wav")).unwrap()) == want);
    assert!(bits(&decode::load(&d.join("named.flac")).unwrap()) == want);
    std::fs::write(d.join("song.mp3"), b"ID3\x04\0\0\0\0\0\0 not a wav or a flac").unwrap();
    let e = format!("{:#}", decode::load(&d.join("song.mp3")).unwrap_err());
    assert!(e.contains("isn't a WAV or FLAC file"), "{e}");
    std::fs::remove_dir_all(d).ok();
}

// FMT-24: a FLAC of 12, 20 or 32 bits, or one whose header doesn't say how long it is, is refused
// in words before a sample is decoded.
#[test]
fn a_flac_music2_cant_read_is_refused_in_words() {
    let d = scratch("refused");
    for b in [12, 20, 32, 4] {
        let f = patched(&d, "16bit-mono.flac", |x| set_bits(x, b));
        let e = format!("{:#}", decode::load(&f).unwrap_err());
        assert!(e.contains(&format!("it has {b} bits in each sample: music2 reads FLAC files of 8, 16 or 24 bits")), "{e}");
    }
    let f = patched(&d, "16bit-mono.flac", |x| set_frames(x, 0));
    let e = format!("{:#}", decode::load(&f).unwrap_err());
    assert!(e.contains("its header doesn't say how long it is"), "{e}");
    std::fs::remove_dir_all(d).ok();
}

// FMT-24: a damaged FLAC is refused, never played: cut short, a byte changed, more or fewer
// samples than its header says, a block with other channels.
#[test]
fn a_damaged_flac_is_refused_never_played() {
    let d = scratch("damaged");
    let cut = patched(&d, "16bit-stereo.flac", |x| x.truncate(x.len() / 2));
    assert!(decode::load(&cut).is_err(), "cut short");
    // A byte in the audio changed: the frame's check (its CRC) fails.
    let flipped = patched(&d, "16bit-stereo.flac", |x| {
        let at = x.len() - 100;
        x[at] ^= 0x5A;
    });
    assert!(decode::load(&flipped).is_err(), "a byte changed");
    let more = patched(&d, "16bit-stereo.flac", |x| set_frames(x, 4_000));
    let e = format!("{:#}", decode::load(&more).unwrap_err());
    assert!(e.contains("it holds more samples than its header says"), "{e}");
    let fewer = patched(&d, "16bit-stereo.flac", |x| set_frames(x, 5_000));
    let e = format!("{:#}", decode::load(&fewer).unwrap_err());
    assert!(e.contains("it holds 4800 of the 5000 frames its header says"), "{e}");
    // A stereo file whose header says mono: its first block has two channels.
    let channels = patched(&d, "16bit-stereo.flac", |x| x[20] &= !0x0E);
    let e = format!("{:#}", decode::load(&channels).unwrap_err());
    assert!(e.contains("a block of it has 2 channels, not 1"), "{e}");
    // A mono 16-bit file whose header says 8 bits: its samples don't fit them.
    let wider = patched(&d, "16bit-mono.flac", |x| set_bits(x, 8));
    let e = format!("{:#}", decode::load(&wider).unwrap_err());
    assert!(e.contains("a sample is outside its 8 bits"), "{e}");
    std::fs::remove_dir_all(d).ok();
}

// FMT-24: a WAV's limits hold before a sample is decoded: frames × channels × bytes must fit a WAV.
#[test]
fn a_flac_too_big_for_a_wav_is_refused_before_decoding() {
    let d = scratch("big");
    let f = patched(&d, "24bit-stereo.flac", |x| set_frames(x, (decode::WAV_BYTES / 6) + 1));
    let e = format!("{:#}", Flac::new(std::fs::File::open(&f).unwrap()).err().unwrap());
    assert!(e.contains("as a WAV it would be over 4 GiB"), "{e}");
    let fits = patched(&d, "24bit-stereo.flac", |x| set_frames(x, decode::WAV_BYTES / 6));
    assert!(Flac::new(std::fs::File::open(&fits).unwrap()).is_ok(), "just fits: decoding is what finds it short");
    std::fs::remove_dir_all(d).ok();
}

// FMT-24: a FLAC written as a WAV keeps its samples and its bits: the WAV libsndfile wrote from the
// same numbers, sample for sample, and the bits of the FLAC's own decode.
#[test]
fn a_flac_written_as_a_wav_keeps_its_samples_and_bits() {
    let d = scratch("write");
    for (b, ch, name) in PAIRS {
        let out = d.join(format!("{name}.wav"));
        let flac = Flac::new(std::fs::File::open(fixture(&format!("{name}.flac"))).unwrap()).unwrap();
        decode::write_wav(flac, std::io::BufWriter::new(std::fs::File::create(&out).unwrap())).unwrap();
        let mine = hound::WavReader::open(&out).unwrap();
        let theirs = hound::WavReader::open(fixture(&format!("{name}.wav"))).unwrap();
        let spec = mine.spec();
        assert_eq!((spec.bits_per_sample, spec.channels, spec.sample_rate), (b, ch, 48_000), "{name}");
        assert_eq!(spec.sample_format, hound::SampleFormat::Int, "{name}");
        let samples = |r: hound::WavReader<_>| r.into_samples::<i32>().map(Result::unwrap).collect::<Vec<_>>();
        assert!(samples(mine) == samples(theirs), "{name}: libsndfile's numbers");
        assert!(bits(&decode::load(&out).unwrap()) == bits(&decode::load(&fixture(&format!("{name}.flac"))).unwrap()), "{name}");
    }
    std::fs::remove_dir_all(d).ok();
}

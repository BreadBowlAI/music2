//! Waveform peaks (Q9): min and max per 256 frames, made once per content, kept on disk.

use std::path::{Path, PathBuf};

use super::*;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("m2-peaks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A 32-bit float WAV of `channels`, interleaved from `frame(i)`.
fn wav(path: &Path, channels: u16, frames: usize, frame: impl Fn(usize) -> Vec<f32>) {
    let spec = hound::WavSpec { channels, sample_rate: 48_000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        for s in frame(i) {
            w.write_sample(s).unwrap();
        }
    }
    w.finalize().unwrap();
}

/// The pair of bucket `b`, channel `c`.
fn pair(p: &Peaks, b: usize, c: usize) -> (i8, i8) {
    let i = (b * p.channels as usize + c) * 2;
    (p.data[i], p.data[i + 1])
}

#[test]
fn peaks_are_min_and_max_per_256_frames() {
    let d = Dir::new("minmax");
    // Left: a ramp from -1 up to 1 over 1,024 frames; right: a sine at half level, 1 cycle per 256.
    let file = d.0.join("ramp.wav");
    let ramp = |i: usize| -1.0 + 2.0 * i as f32 / 1023.0;
    let sine = |i: usize| 0.5 * (std::f32::consts::TAU * i as f32 / 256.0).sin();
    wav(&file, 2, 1_000, |i| vec![ramp(i), sine(i)]);
    let p = Store::new(None).of(&file).unwrap();
    assert_eq!((p.channels, p.rate, p.frames, p.bucket), (2, 48_000, 1_000, 256));
    assert_eq!(p.data.len(), 4 * 2 * 2, "4 buckets (the last one short), 2 channels, a pair each");
    let q = |x: f32| (x * 127.0).round() as i8;
    for b in 0..4 {
        let last = (b * 256 + 255).min(999);
        assert_eq!(pair(&p, b, 0), (q(ramp(b * 256)), q(ramp(last))), "the ramp's bucket {b}");
        let (lo, hi) = (b * 256..=last).map(sine).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert_eq!(pair(&p, b, 1), (q(lo), q(hi)), "the sine's bucket {b}");
        assert!((q(hi) - 64).abs() <= 1 && (q(lo) + 64).abs() <= 1, "half level: {lo} {hi}");
    }
    assert_eq!(pair(&p, 0, 0), (-127, q(ramp(255))));
    // Over full scale is drawn at full scale.
    let loud = d.0.join("loud.wav");
    wav(&loud, 1, 10, |_| vec![3.0]);
    assert_eq!(pair(&Store::new(None).of(&loud).unwrap(), 0, 0), (127, 127));
}

#[test]
fn a_mono_file_has_one_channel() {
    let d = Dir::new("mono");
    let file = d.0.join("mono.wav");
    wav(&file, 1, 600, |i| vec![if i < 256 { 0.0 } else { 0.25 }]);
    let p = Store::new(None).of(&file).unwrap();
    assert_eq!((p.channels, p.frames, p.data.len()), (1, 600, 3 * 2));
    assert_eq!(pair(&p, 0, 0), (0, 0), "silence is a flat line");
    assert_eq!(pair(&p, 1, 0), (32, 32));
}

#[test]
fn peaks_are_made_once_and_kept_by_content() {
    let d = Dir::new("once");
    let cache = d.0.join("cache/peaks");
    let file = d.0.join("vox.wav");
    wav(&file, 2, 5_000, |i| vec![(i as f32 / 5_000.0).sin(), 0.1]);
    let store = Store::new(Some(cache.clone()));
    let first = store.of(&file).unwrap();
    assert_eq!(store.decodes(), 1);
    let again = store.of(&file).unwrap();
    assert!(Arc::ptr_eq(&first, &again), "the second ask is the first's");
    // A copy under another name is the same content: the same entry.
    let copy = d.0.join("copy of vox.wav");
    std::fs::copy(&file, &copy).unwrap();
    assert_eq!(*store.of(&copy).unwrap(), *first);
    assert_eq!(store.decodes(), 1, "a copy decodes nothing");
    // Kept on disk by content: a new store (the next launch) reads it, decoding nothing.
    let sha = HashCache::global().hash(&file).unwrap();
    let on_disk = cache.join(format!("{}.m2pk", sha.hex()));
    assert_eq!(std::fs::read(&on_disk).unwrap(), first.bytes());
    let next = Store::new(Some(cache.clone()));
    assert_eq!(*next.of(&file).unwrap(), *first);
    assert_eq!(next.decodes(), 0, "read from the cache folder");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1, "no part file left behind");
}

#[test]
fn a_cache_file_that_doesnt_read_is_made_again() {
    let d = Dir::new("bad");
    let cache = d.0.join("peaks");
    let file = d.0.join("kick.wav");
    wav(&file, 1, 1_000, |i| vec![if i % 2 == 0 { 0.5 } else { -0.5 }]);
    let made = Store::new(Some(cache.clone())).of(&file).unwrap();
    let on_disk = cache.join(format!("{}.m2pk", HashCache::global().hash(&file).unwrap().hex()));
    for bad in [b"M2PK".to_vec(), made.bytes()[..made.bytes().len() - 1].to_vec(), b"junk junk junk junk junk junk".to_vec()] {
        std::fs::write(&on_disk, &bad).unwrap();
        let store = Store::new(Some(cache.clone()));
        assert_eq!(*store.of(&file).unwrap(), *made, "made again from the file");
        assert_eq!(store.decodes(), 1);
        assert_eq!(std::fs::read(&on_disk).unwrap(), made.bytes(), "and written whole again");
    }
}

#[test]
fn a_file_that_doesnt_decode_says_why() {
    let d = Dir::new("why");
    let file = d.0.join("notes.wav");
    std::fs::write(&file, "these are words, not sound").unwrap();
    let e = Store::new(None).of(&file).unwrap_err().to_string();
    assert!(e.starts_with("notes.wav doesn't read as a WAV file ("), "{e}");
    let gone = Store::new(None).of(&d.0.join("gone.wav")).unwrap_err().to_string();
    assert!(gone.contains("gone.wav"), "{gone}");
}

// PLAT-7: the server keeps peaks beside the hash cache, under `$HOME`; only a program keeps them.
#[test]
fn the_cache_is_under_home() {
    let home = Path::new("/h");
    let cache = m2_os::dirs::cache_of(home);
    let dir = folder_in(&cache);
    assert!(dir.starts_with(home) && dir.ends_with("peaks"), "{}", dir.display());
    assert_eq!(dir.parent(), Some(cache.as_path()));
    assert_eq!(disk(), None, "tests keep peaks in memory");
}

#[test]
fn peaks_read_back() {
    let p = Peaks { channels: 2, rate: 44_100, frames: 300, bucket: 256, data: vec![-3, 4, -5, 6, 0, 1, -1, 0] };
    assert_eq!(Peaks::read(&p.bytes()).unwrap(), p);
    let mut other = p.bytes();
    other[4] = 9;
    assert!(Peaks::read(&other).is_err(), "another version is made again");
}

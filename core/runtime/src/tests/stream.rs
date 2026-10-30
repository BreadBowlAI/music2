//! RENDER-30: audio lanes stream from the disk, and every sample keeps the bits the whole render had.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::{AudioClip, LaneData, LaneKind, TempoMap};

use crate::audio::{self, AudioLane, Cache};
use crate::offline::{self, Kernel};
use crate::stream::{Making, READ, SLOTS, chunk_frames};

const SR: f64 = 48_000.0;

thread_local! {
    /// The largest read this thread made since its test began, and how many it made.
    static READS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

fn watch(_: &std::fs::File, n: usize) -> std::io::Result<()> {
    let _ = READS.try_with(|r| r.set((r.get().0.max(n), r.get().1 + 1)));
    Ok(())
}

/// What this thread read since the last call.
fn reads() -> (usize, usize) {
    m2_os::fs::watch_reads(watch);
    READS.with(|r| r.replace((0, 0)))
}

fn resample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample")
}

/// std's `std:stretch`.
pub(super) fn stretch() -> Kernel {
    Kernel::of(&super::std_packages(), offline::STRETCH).expect("std has std:stretch")
}

/// The old way: every clip rendered whole (a warped one by the stretch kernel's whole run), then
/// the old copy's sum (by clip, in order, mono-down per clip), over `0..len`.
fn rendered(clips: &[AudioClip], assets: &Path, tempo: &TempoMap, channels: usize, len: usize) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0.0f32; len]; channels];
    let k = stretch();
    for c in clips {
        let asset = Arc::new(crate::decode::load(&assets.join(&c.asset)).unwrap());
        let data = audio::render_clip(c, &asset, tempo, SR, Some(&k)).unwrap();
        let at = (tempo.seconds_at(c.start) * SR).round() as usize;
        for (ch, o) in out.iter_mut().enumerate() {
            let mono_down = channels == 1 && data.len() > 1;
            for i in 0..data[0].len() {
                let v = if mono_down { 0.5 * (data[0][i] + data[1][i]) } else { data[ch.min(data.len() - 1)][i] };
                if at + i < len {
                    o[at + i] += v;
                }
            }
        }
    }
    out
}

/// Play `lane` from `0..len` as an offline render does, in runs of odd sizes across chunk edges:
/// the missing chunks worked out, then copied.
fn streamed(lane: &AudioLane, channels: usize, len: usize) -> Vec<Vec<f32>> {
    let mut out = vec![Vec::with_capacity(len); channels];
    let sizes = [1usize, 127, 128, 333, 4097, 12_001, 11_999, 64, 7];
    let mut buf = vec![0.0f32; channels * 12_001];
    let (mut at, mut k) = (0usize, 0);
    while at < len {
        let n = sizes[k % sizes.len()].min(len - at);
        k += 1;
        lane.ensure(at as u64, n as u64, &[]);
        lane.fill(&mut buf, n, 0, at as u64, n);
        for (c, o) in out.iter_mut().enumerate() {
            o.extend_from_slice(&buf[c * n..c * n + n]);
        }
        at += n;
    }
    out
}

// RENDER-30, RENDER-1: every clip of examples/resample (plain, pitched, offset, reversed, faded,
// warped), streamed in runs of odd sizes across chunk edges, is the old whole render bit for bit,
// in a stereo lane and a mono one; no block missed.
#[test]
fn a_streamed_lane_is_the_rendered_lane() {
    let text = std::fs::read_to_string(resample().join("data/chops/clips.json")).unwrap();
    let LaneData::Audio(clips) = LaneData::read(&text, &LaneKind::Audio).unwrap() else { panic!("an audio lane") };
    let kinds = [
        clips.iter().any(|c| c.pitch != 0.0 && c.warp.is_empty()),
        clips.iter().any(|c| c.reverse),
        clips.iter().any(|c| c.offset > 0.0),
        clips.iter().any(|c| c.fade_in > 0.0),
        clips.iter().any(|c| c.fade_out > 0.0),
        clips.iter().any(|c| !c.warp.is_empty()),
    ];
    assert!(kinds.iter().all(|&k| k), "every kind of clip");
    let tempo = TempoMap::constant(120.0);
    let assets = resample().join("assets");
    let len = (tempo.seconds_at(clips.iter().map(|c| c.start + c.length).fold(0.0, f64::max)) * SR) as usize + 1000;
    for channels in [2, 1] {
        let lane = AudioLane::build_with(&clips, &|a| Some(assets.join(a)), &mut Cache::default(), &tempo, SR, channels, Some(&stretch()))
            .unwrap();
        assert!(!lane.resident(), "the lane streams: more chunks than slots");
        let (want, got) = (rendered(&clips, &assets, &tempo, channels, len), streamed(&lane, channels, len));
        for c in 0..channels {
            let first = (0..len).find(|&i| want[c][i].to_bits() != got[c][i].to_bits());
            assert_eq!(first, None, "{channels} channels: channel {c} differs from frame {first:?}");
        }
        assert!(want[0].iter().any(|&x| x != 0.0), "it plays something");
        assert_eq!(lane.misses(), 0);
    }
}

/// A WAV of `seconds` of stereo 32-bit float at 48 kHz, written fast: its header, then its frames.
fn long_wav(path: &Path, seconds: usize) {
    let frames = seconds * SR as usize;
    let data = (frames * 8) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&(48_000u32 * 8).to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let x = ((i % 480) as f32 / 480.0 - 0.5) * 0.5;
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&(-x).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

// RENDER-30: a 3-minute file plays from the disk: the lane holds no more than its slots and its
// filler's buffers, and no read is over one chunk's bytes.
#[test]
fn a_long_file_never_lives_in_memory() {
    let dir = std::env::temp_dir().join(format!("m2-stream-long-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    long_wav(&dir.join("long.wav"), 180);
    let file = std::fs::metadata(dir.join("long.wav")).unwrap().len() as usize;
    let tempo = TempoMap::constant(120.0);
    let clip: AudioClip = serde_json::from_value(serde_json::json!({"start": 0, "length": 360, "asset": "long.wav"})).unwrap();
    let lane = AudioLane::build_with(&[clip], &|a| Some(dir.join(a)), &mut Cache::default(), &tempo, SR, 2, None).unwrap();
    let chunk_bytes = chunk_frames(SR) as usize * 2 * 4;
    let slots = SLOTS * chunk_bytes;
    reads();
    let mut buf = vec![0.0f32; 2 * 4096];
    let (mut most, mut heard) = (0, 0.0f32);
    for b in 0..180 * SR as u64 / 4096 {
        lane.ensure(b * 4096, 4096, &[]);
        lane.fill(&mut buf, 4096, 0, b * 4096, 4096);
        heard = heard.max(buf[0].abs());
        most = most.max(lane.memory());
    }
    let (largest, n) = reads();
    assert!(heard > 0.2, "it plays the file");
    assert!(most <= slots + 8 * chunk_bytes, "the lane held {most} bytes: its slots are {slots}, the file {file}");
    assert!(largest <= chunk_bytes.min(READ), "a read of {largest} bytes, over a chunk's {chunk_bytes}");
    assert!(n * largest >= file, "{n} reads read the file once");
    std::fs::remove_dir_all(&dir).ok();
}

// RENDER-30: the ring and its copies never let a torn chunk through. A filler rewrites slots with
// chunks whose every sample says which chunk it is, while a reader copies: each copy either holds
// one chunk's samples only, or is silence counted as a miss.
#[test]
fn a_torn_chunk_is_never_heard() {
    let c = chunk_frames(SR) as usize;
    // 40 chunks, each its own number throughout.
    let render: Vec<f32> = (0..40 * c).map(|i| (i / c) as f32 + 1.0).collect();
    let lane = Arc::new(AudioLane::of_renders(vec![(0, Arc::new(vec![render]))], 1, SR));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let filler = {
        let (lane, stop) = (lane.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut k = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                lane.fill_chunk(k % 40, &|_| false);
                k += 7;
            }
        })
    };
    let mut out = vec![0.0f32; 256];
    let (mut heard, mut missed) = (0, 0);
    for i in 0..200_000u64 {
        let k = i % 40;
        let before = lane.misses();
        lane.fill(&mut out, 256, 0, k * c as u64 + 100, 256);
        if lane.misses() > before {
            missed += 1;
            assert!(out.iter().all(|&x| x == 0.0), "a miss is silence");
        } else {
            heard += 1;
            assert!(out.iter().all(|&x| x == k as f32 + 1.0), "chunk {k} whole: {:?}", &out[..4]);
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    filler.join().unwrap();
    assert!(heard > 0 && missed > 0, "{heard} heard, {missed} missed");
}

// The sx plan, slice 2 (RENDER-30): a warped clip's render is made in pieces. Its source is read
// from the disk a piece at a time, about once, and never decoded whole; the render goes to its file
// in the cache folder a piece at a time. No block the build takes from the heap comes near the size
// of either file.
#[test]
fn a_warped_render_holds_neither_file_whole() {
    let dir = std::env::temp_dir().join(format!("m2-stream-warp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    long_wav(&dir.join("long.wav"), 20);
    let file = std::fs::metadata(dir.join("long.wav")).unwrap().len() as usize;
    let tempo = TempoMap::constant(120.0);
    // 20 s of source laid out over 50 beats, 25 s: a render of 9.6 MB.
    let clip: AudioClip =
        serde_json::from_value(serde_json::json!({"start": 0, "length": 50, "asset": "long.wav", "warp": [[0, 0], [20, 50]]})).unwrap();
    let mut cache = Cache::with_renders_in(Some(dir.join("renders")));
    let k = stretch();
    reads();
    let (lane, most) = super::alloc_count::largest(|| {
        AudioLane::build_with(&[clip], &|a| Some(dir.join(a)), &mut cache, &tempo, SR, 2, Some(&k)).unwrap()
    });
    let (largest, n) = reads();
    let render = 25 * SR as usize * 2 * 4;
    assert_eq!(cache.work().1, 1, "one render made");
    assert!(most < 1 << 20, "a block of {most} bytes taken while the source is {file} bytes and its render {render}");
    assert!(
        largest <= READ && n * largest >= file && n * largest < 2 * file,
        "{n} reads of up to {largest} bytes read the source about once"
    );
    let rendered =
        std::fs::read_dir(dir.join("renders")).unwrap().map(|e| e.unwrap().metadata().unwrap().len() as usize).collect::<Vec<_>>();
    assert_eq!(rendered, [render], "its render is in the cache folder, whole");
    lane.ensure(10 * SR as u64, 4096, &[]);
    let mut buf = vec![0.0f32; 2 * 4096];
    lane.fill(&mut buf, 4096, 0, 10 * SR as u64, 4096);
    assert!(buf.iter().any(|&x| x.abs() > 0.1), "the lane plays it");
    std::fs::remove_dir_all(&dir).ok();
}

/// A new folder for one test.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-stream-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A clip of `json`.
fn clip(json: serde_json::Value) -> AudioClip {
    serde_json::from_value(json).unwrap()
}

/// Build `clips` on another thread: the lane and the cache back, or None when it took over 20 s.
fn built(clips: Vec<AudioClip>, dir: &Path, cache: Cache, tempo: &TempoMap) -> Option<(Arc<AudioLane>, Cache)> {
    let (dir, tempo, (tx, rx)) = (dir.to_path_buf(), tempo.clone(), std::sync::mpsc::channel());
    std::thread::spawn(move || {
        let mut cache = cache;
        let lane = AudioLane::build_with(&clips, &|a| Some(dir.join(a)), &mut cache, &tempo, SR, 2, Some(&stretch())).unwrap();
        drop(tx.send((lane, cache)));
    });
    rx.recv_timeout(std::time::Duration::from_secs(20)).ok()
}

// The sx plan, slice 3 (RENDER-30, D10): a live cache never waits for a stretch. With its stretch
// thread held back, the lane of a newly warped clip (what a tempo change asks for) is built at
// once. Where the clip is it plays silence, each block counted as a miss, never a wrong sample;
// the plain clip before it plays. Let go, the render is made and the lane plays it: the offline
// render, bit for bit.
#[test]
fn a_tempo_change_never_waits_for_a_stretch() {
    let dir = folder("held");
    long_wav(&dir.join("a.wav"), 4);
    let tempo = TempoMap::constant(120.0);
    // At 120 BPM: the plain clip plays 0 s to 1 s, the warped one 4 s of the file over 5 s from 1 s.
    let clips = vec![
        clip(serde_json::json!({"start": 0, "length": 2, "asset": "a.wav"})),
        clip(serde_json::json!({"start": 2, "length": 10, "asset": "a.wav", "warp": [[0, 0], [4, 10]]})),
    ];
    let cache = Cache::live_in(dir.join("renders"));
    cache.hold_renders(Some(0));
    let (lane, cache) = built(clips.clone(), &dir, cache, &tempo).expect("the build never waits for the stretch");
    assert!(cache.settle_renders(std::time::Duration::from_secs(20)), "the stretch waits at the hold");
    let len = 7 * SR as usize;
    let want = rendered(&clips, &dir, &tempo, 2, len);
    let got = streamed(&lane, 2, len);
    let clip_at = SR as usize;
    for i in 0..len {
        let w = if i < clip_at { [want[0][i], want[1][i]] } else { [0.0; 2] };
        assert_eq!([got[0][i].to_bits(), got[1][i].to_bits()], w.map(f32::to_bits), "frame {i}: the plain clip, then silence");
    }
    assert!(want[0][..clip_at].iter().any(|&x| x != 0.0) && want[0][clip_at..].iter().any(|&x| x != 0.0), "both clips sound");
    assert!(lane.misses() > 0, "the silence is counted");
    cache.hold_renders(None);
    assert!(cache.settle_renders(std::time::Duration::from_secs(60)), "the stretch is made");
    let misses = lane.misses();
    let got = streamed(&lane, 2, len);
    for c in 0..2 {
        let first = (0..len).find(|&i| want[c][i].to_bits() != got[c][i].to_bits());
        assert_eq!(first, None, "channel {c}: the lane plays the render made, as the offline render");
    }
    assert_eq!(lane.misses(), misses, "no miss once made");
    drop((lane, cache));
    std::fs::remove_dir_all(&dir).ok();
}

// The sx plan, slice 3 (RENDER-30, D10): a lane plays its render as it is made. A chunk fills only
// once every frame of the render it reads is done: then it is the offline render's, bit for bit;
// till then it stays empty. A short lane, filled whole as it is built, has holes where the render
// isn't made, filled again later as the disk thread does. A reversed clip reads its render from
// the end, so its chunks wait for the render's end.
#[test]
fn a_lane_plays_its_render_as_it_is_made() {
    for reverse in [false, true] {
        let dir = folder(&format!("made-{reverse}"));
        long_wav(&dir.join("a.wav"), 2);
        let tempo = TempoMap::constant(120.0);
        // From 0.65 s, mid-chunk, 2 s of the file over 2.5 s: 120 000 frames.
        let c = clip(serde_json::json!({"start": 1.3, "length": 5, "asset": "a.wav", "warp": [[0, 0], [2, 5]], "reverse": reverse}));
        let cache = Cache::live_in(dir.join("renders"));
        cache.hold_renders(Some(0));
        let (lane, cache) = built(vec![c.clone()], &dir, cache, &tempo).expect("built at once");
        assert!(lane.resident(), "a short lane");
        let (at, n) = ((0.65 * SR) as usize, 120_000);
        let len = at + n + 1000;
        let want = rendered(std::slice::from_ref(&c), &dir, &tempo, 2, len);
        let chunk = lane.chunk() as usize;
        let mut filled = vec![];
        for made in [0, 30_000, 61_000, 119_000, n] {
            cache.hold_renders(Some(made));
            assert!(cache.settle_renders(std::time::Duration::from_secs(60)));
            let mut ready = 0;
            for k in (0..len.div_ceil(chunk) as u64).filter(|&k| lane.has_audio(k)) {
                lane.fill_chunk(k, &|_| true);
                // The render's frames this chunk reads end at `needs`.
                let (i0, i1) = ((k as usize * chunk).max(at) - at, ((k as usize + 1) * chunk).min(at + n) - at);
                let needs = if reverse { n - i0 } else { i1 };
                if needs <= made {
                    assert!(lane.ready(k), "{reverse}: chunk {k} is made by {made} frames: it fills");
                } else if needs > made + crate::BLOCK {
                    assert!(!lane.ready(k), "{reverse}: chunk {k} needs frames to {needs}, {made} made: it waits");
                }
                if lane.ready(k) {
                    ready += 1;
                    let mut out = vec![0.0f32; 2 * chunk];
                    lane.fill(&mut out, chunk, 0, k * chunk as u64, chunk);
                    for ch in 0..2 {
                        let from = k as usize * chunk;
                        let first =
                            (0..chunk).find(|&i| out[ch * chunk + i].to_bits() != want[ch].get(from + i).copied().unwrap_or(0.0).to_bits());
                        assert_eq!(first, None, "{reverse}: chunk {k} filled with {made} frames made is the offline render's");
                    }
                }
            }
            filled.push(ready);
        }
        assert!(
            filled.windows(2).all(|w| w[0] <= w[1]) && filled[0] < filled[2] && filled[2] < filled[4],
            "{reverse}: chunks fill as it is made: {filled:?}"
        );
        assert_eq!(streamed(&lane, 2, len), want, "{reverse}: the whole lane is the offline render");
        drop((lane, cache));
        std::fs::remove_dir_all(&dir).ok();
    }
}

// Review sx-r1 (RENDER-30): a disk that fills while a render is made never cuts it short. What is in
// its part file so far goes to memory, the rest is made there, and the part file goes. A reader
// reads every frame as it was put.
#[test]
fn a_render_whose_disk_fills_goes_on_in_memory() {
    let dir = folder("full");
    let frames = 3 * READ;
    let x = |c: usize, i: usize| (2 * i + c) as f32 * 1e-4;
    let mut m = Making::new(Some(&dir.join("r.f32")), 2, frames);
    let raw = m.raw().clone();
    let put = |m: &mut Making, from: usize, to: usize| {
        let b: [Vec<f32>; 2] = [0, 1].map(|c| (from..to).map(|i| x(c, i)).collect());
        m.put(&[&b[0], &b[1]]).unwrap();
    };
    put(&mut m, 0, frames / 2);
    m.flush().unwrap();
    raw.made(m.written());
    assert_eq!(m.written(), frames / 2, "half of it is in its part file");
    m.break_disk();
    put(&mut m, frames / 2, frames);
    m.finish().unwrap();
    assert!(raw.whole());
    let (mut buf, mut out) = (vec![], vec![vec![]; 2]);
    raw.read(0, frames, &mut buf, &mut out).unwrap();
    for (c, o) in out.iter().enumerate() {
        assert_eq!(o.iter().enumerate().position(|(i, &v)| v != x(c, i)), None, "channel {c}: every frame as put");
    }
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "its part file went, and no render is named");
    std::fs::remove_dir_all(&dir).ok();
}

// FMT-24, RENDER-1: a clip whose file in assets/ is a FLAC (a song's clips are WAV: an import
// writes a FLAC as one) fails an export, naming the clip, with a fault of its own: the session says
// it isn't a WAV, not that it may be damaged. Live it is silent, said the same way.
#[test]
fn an_export_of_a_clip_on_a_flac_fails_naming_it() {
    use crate::fault::{Code, Failed, Subject};
    let (ir, _, dir) = super::audio_song(&[], serde_json::json!([{"start": 0, "length": 4, "asset": "loop.flac"}]));
    std::fs::write(dir.join("assets/loop.flac"), include_bytes!("../../tests/fixtures/flac/16bit-stereo.flac")).unwrap();
    let tl = super::data(&ir, &dir);
    let pk = super::std_packages();
    let flat = super::check(&ir, &pk).unwrap();
    let mut r = crate::Renderer::new(&flat, &tl, &pk, SR as u32).unwrap();
    let e = r.render(1.0).map(drop).expect_err("the render stops");
    let fault = e.downcast_ref::<Failed>().map(|f| (f.fault.code, f.fault.subject));
    assert_eq!(fault, Some((Code::NotWav, Subject::Clip { lane: 0, clip: 0 })), "{e:#}");
    assert_eq!(Code::NotWav.diag(), "asset.unreadable");
    std::fs::remove_dir_all(dir).ok();
}

// FMT-24 (review F3): only a clip on a FLAC (behind an ID3v2 tag too) is told it isn't a WAV; a file
// that is empty, cut inside its header or of another kind (AIFF) can't be read, as before: it may be
// damaged, or still being written.
#[test]
fn only_a_clip_on_a_flac_is_told_it_isnt_a_wav() {
    use crate::fault::{Code, Failed};
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flac");
    let flac = std::fs::read(fixtures.join("16bit-stereo.flac")).unwrap();
    let tagged = std::fs::read(fixtures.join("id3v2-start.flac")).unwrap();
    for (name, bytes, code) in [
        ("empty.wav", &b""[..], Code::Unreadable),
        ("cut.wav", b"RIFF\0\0", Code::Unreadable),
        ("x.aif", b"FORM\0\0\0\x20AIFFCOMM a sound", Code::Unreadable),
        ("loop.flac", &flac[..], Code::NotWav),
        ("tagged.flac", &tagged[..], Code::NotWav),
    ] {
        let (ir, _, dir) = super::audio_song(&[], serde_json::json!([{"start": 0, "length": 4, "asset": name}]));
        std::fs::write(dir.join("assets").join(name), bytes).unwrap();
        let tl = super::data(&ir, &dir);
        let pk = super::std_packages();
        let flat = super::check(&ir, &pk).unwrap();
        let mut r = crate::Renderer::new(&flat, &tl, &pk, SR as u32).unwrap();
        let e = r.render(1.0).map(drop).expect_err("the render stops");
        assert_eq!(e.downcast_ref::<Failed>().map(|f| f.fault.code), Some(code), "{name}: {e:#}");
        std::fs::remove_dir_all(dir).ok();
    }
}

// io r8, RENDER-1 (the ex plan's slice 2): an export never finishes with a clip silent. A streamed
// file cut short after the build can't be read where it ends: the render stops there, its error the
// fault `asset.unreadable` about that clip (lane 0, its first clip), which the session names.
// Before, that chunk played silence and the render said nothing.
#[test]
fn an_export_of_a_streamed_file_cut_short_fails_naming_it() {
    use crate::fault::{Code, Failed, Subject};
    // Six seconds: more chunks than a lane's slots, so it is read as it plays.
    let (ir, tl, dir) = super::audio_song(
        &[("long.wav", vec![super::sine(220.0, 6.0, SR as u32); 2], SR as u32)],
        serde_json::json!([{"start": 0, "length": 12, "asset": "long.wav"}]),
    );
    let pk = super::std_packages();
    let flat = super::check(&ir, &pk).unwrap();
    let mut r = crate::Renderer::new(&flat, &tl, &pk, SR as u32).unwrap();
    let file = std::fs::OpenOptions::new().write(true).open(dir.join("assets/long.wav")).unwrap();
    file.set_len(file.metadata().unwrap().len() / 2).unwrap();
    let e = r.render(6.0).map(drop).expect_err("the render stops");
    let fault = e.downcast_ref::<Failed>().map(|f| (f.fault.code, f.fault.subject));
    assert_eq!(fault, Some((Code::Unreadable, Subject::Clip { lane: 0, clip: 0 })), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}

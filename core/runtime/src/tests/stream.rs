//! RENDER-30: audio lanes stream from the disk, and every sample keeps the bits the whole render had.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::{AudioClip, LaneData, LaneKind, TempoMap};

use crate::audio::{self, AudioLane, Cache};
use crate::stream::{READ, SLOTS, chunk_frames};

const SR: f64 = 48_000.0;

thread_local! {
    /// The largest read this thread made since its test began, and how many it made.
    static READS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

fn watch(n: usize) {
    let _ = READS.try_with(|r| r.set((r.get().0.max(n), r.get().1 + 1)));
}

/// What this thread read since the last call.
fn reads() -> (usize, usize) {
    m2_os::fs::watch_reads(watch);
    READS.with(|r| r.replace((0, 0)))
}

fn resample() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/resample")
}

/// The old way: every clip rendered whole, then the old copy's sum (by clip, in order, mono-down
/// per clip), over `0..len`.
fn rendered(clips: &[AudioClip], assets: &Path, tempo: &TempoMap, channels: usize, len: usize) -> Vec<Vec<f32>> {
    let mut out = vec![vec![0.0f32; len]; channels];
    for c in clips {
        let asset = audio::load_wav(&assets.join(&c.asset)).unwrap();
        let data = audio::render_clip(c, &asset, tempo, SR);
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
        let lane = AudioLane::build_with(&clips, &|a| Some(assets.join(a)), &mut Cache::default(), &tempo, SR, channels);
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
    let lane = AudioLane::build_with(&[clip], &|a| Some(dir.join(a)), &mut Cache::default(), &tempo, SR, 2);
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

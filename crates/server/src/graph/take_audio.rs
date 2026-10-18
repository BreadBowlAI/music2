//! A take on an audio lane (the io plan's D17 to D22): what the input played, written into the
//! song's `assets/` as it records, then placed as one clip.
//!
//! - **The writer** ([`Writer`]): one `m2-take` thread a take. Every 50 ms it moves what the input's
//!   take ring holds into the take's hidden part file ([`Growing`]), from the frame heard at the
//!   take's start: the engine clock where the transport was at its start (its anchor), less the
//!   input's stamp (D20). Nothing before it is kept, so a count-in's sound never is.
//! - **A full ring** (D22): frames the ring lost before the writer moved them are written as
//!   silence, so everything after stays where it was heard; the take says "The disk was too slow:
//!   part of the take is silent."
//! - **The end**: told how many frames the take has, the writer waits for the input to deliver
//!   them (at most a second, then silence), finishes the file (header, push, name, folder sync) and
//!   hands it back. Only then does the take thread plan its Tx ([`Graph::place_take`]): one `Clips`
//!   Tx (`Gesture::Set`) on the lane, the clip from the take's start as long as the take, pinned by
//!   its hash (WRITE-1, FMT-14).
//! - **Let go** (cancelled, or its song closed): the thread stops and the part file goes.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use m2_editor::import::{self, Imported, Target};
use m2_editor::take::Growing;
use m2_play::ring::Ring;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Graph;
use super::clips::Refused;
use crate::peaks::{Peaks, Store};

/// How often the writer moves what the ring holds into the file (D17).
const EVERY: Duration = Duration::from_millis(50);
/// Frames moved at a time.
const PIECE: usize = 4096;
/// How long the end waits for the input to deliver the take's last frames.
const LAST: Duration = Duration::from_secs(1);

/// What the windows hear when part of a take is silence (D22).
pub const TOO_SLOW: &str = "The disk was too slow: part of the take is silent.";
/// What they hear when the sound input went away while it recorded (the review's io-r2).
pub const UNPLUGGED: &str = "The sound input was unplugged: the take was kept up to here.";

/// What the writer and the take share.
#[derive(Default)]
struct Shared {
    /// The take's anchor plus one; 0 while unknown.
    anchor: AtomicU64,
    /// The take's length in frames plus one, once it ended; 0 before.
    end: AtomicU64,
    cancel: AtomicBool,
    /// Frames the file holds, from the take's start.
    written: AtomicU64,
    /// The ring's head when the writer last moved frames.
    seen: AtomicU64,
    /// The writer moves nothing while a test fills its ring past what it holds.
    #[cfg(test)]
    paused: AtomicBool,
}

/// A take's file as it is written, and its writer thread.
pub(super) struct Writer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<Result<Option<Written>>>>,
    poke: mpsc::Sender<()>,
}

/// A take's file, written: the asset, and how many of its frames are silence for want of the disk.
pub(super) type Written = (Imported, u64);

impl Writer {
    /// Start writing the input's `ring` into `file` once the anchor is known; `name` is what the
    /// file is called once named (the take's).
    pub(super) fn start(ring: Arc<Ring>, file: Growing, name: String, channels: u16) -> Result<Writer> {
        let shared = Arc::new(Shared::default());
        let s = shared.clone();
        let (poke, poked) = mpsc::channel();
        let thread = std::thread::Builder::new().name("m2-take".into()).spawn(move || write(&ring, file, &name, channels, &s, &poked))?;
        Ok(Writer { shared, thread: Some(thread), poke })
    }

    /// The engine clock where the transport is at the take's start (its anchor).
    pub(super) fn anchor(&self, clock: u64) {
        self.shared.anchor.store(clock + 1, Ordering::Release);
    }

    /// The take ended, `frames` long: the writer finishes the file.
    pub(super) fn end(&self, frames: u64) {
        self.shared.end.store(frames + 1, Ordering::Release);
        let _ = self.poke.send(());
    }

    /// The file written and named, or None for a take with no frame. Waits for the writer.
    pub(super) fn done(mut self) -> Result<Option<Written>> {
        let t = self.thread.take().ok_or_else(|| anyhow!("the take was written already"))?;
        t.join().map_err(|_| anyhow!("the recording's writer stopped"))?
    }

    /// Wait until the writer has moved everything the ring holds now (tests: a driven player runs
    /// far faster than the writer's 50 ms).
    #[cfg(test)]
    pub(super) fn settle(&self, ring: &Ring) {
        if self.shared.paused.load(Ordering::Relaxed) {
            return;
        }
        let head = ring.head();
        let t = Instant::now();
        while self.shared.anchor.load(Ordering::Acquire) > 0
            && self.shared.seen.load(Ordering::Acquire) < head
            && t.elapsed() < Duration::from_secs(5)
        {
            let _ = self.poke.send(());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(test)]
impl Writer {
    /// The writer moves nothing, or moves again (`a_full_take_ring_writes_silence_and_keeps_time`).
    pub(super) fn pause(&self, on: bool) {
        self.shared.paused.store(on, Ordering::Relaxed);
        let _ = self.poke.send(());
    }
}

impl Drop for Writer {
    /// Let go before it ended: the take was dropped, and its part file goes with the thread.
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.shared.cancel.store(true, Ordering::Release);
            let _ = self.poke.send(());
        }
    }
}

/// The writer's thread: from the frame heard at the anchor, move what the ring holds into `file`
/// every 50 ms; at the end, finish it.
fn write(ring: &Ring, mut file: Growing, name: &str, channels: u16, s: &Shared, poked: &mpsc::Receiver<()>) -> Result<Option<Written>> {
    // The ring's number for the take's first frame: below 0 when it was heard before the input's
    // first frame (it opened a moment after).
    let mut start: Option<i64> = None;
    let (mut at, mut gone, mut peaks) = (0u64, 0u64, Peaks::builder());
    let mut buf = vec![[0.0f32; 2]; PIECE];
    let mut planar = vec![Vec::with_capacity(PIECE); channels as usize];
    let mut ended: Option<Instant> = None;
    loop {
        let _ = poked.recv_timeout(EVERY);
        if s.cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        #[cfg(test)]
        if s.paused.load(Ordering::Relaxed) {
            continue;
        }
        let end = s.end.load(Ordering::Acquire).checked_sub(1);
        if start.is_none()
            && let (Some(anchor), Some(offset)) = (s.anchor.load(Ordering::Acquire).checked_sub(1), ring.offset())
        {
            let first = anchor as i64 - offset;
            let before = vec![[0.0f32; 2]; (-first).max(0) as usize];
            file.append(&before).map_err(|e| anyhow!("{e}"))?;
            add(&mut peaks, &mut planar, &before);
            (start, at) = (Some(first), first.max(0) as u64);
        }
        let Some(first) = start else {
            if end.is_some() {
                // Ended before it began: nothing to keep.
                return Ok(None);
            }
            continue;
        };
        let head = ring.head();
        let until = end.map(|n| (first + n as i64).max(0) as u64);
        let to = until.map_or(head, |u| u.min(head));
        while at < to {
            let n = ((to - at) as usize).min(PIECE);
            let got = ring.read(at, &mut buf[..n]);
            gone += got.gone as u64;
            file.append(&buf[..got.frames]).map_err(|e| anyhow!("{e}"))?;
            add(&mut peaks, &mut planar, &buf[..got.frames]);
            at += got.frames as u64;
        }
        s.written.store((at as i64 - first) as u64, Ordering::Release);
        s.seen.store(head, Ordering::Release);
        let Some(until) = until else { continue };
        // The input delivers the last frames a buffer late; a second on, silence stands in.
        let since = *ended.get_or_insert_with(Instant::now);
        if at < until && since.elapsed() >= LAST {
            let silence = vec![[0.0f32; 2]; (until - at) as usize];
            file.append(&silence).map_err(|e| anyhow!("{e}"))?;
            add(&mut peaks, &mut planar, &silence);
            at = until;
        }
        if at >= until {
            if file.frames() == 0 {
                return Ok(None);
            }
            let rate = file.rate();
            let made = file.finish(name).map_err(|e| anyhow!("{e}"))?;
            // Its waveform, from the frames as written (D31: no second decode).
            drop(Store::global().put(made.hash, peaks.done(rate)));
            return Ok(Some((made, gone)));
        }
    }
}

/// Frames into the waveform's builder, as its channels.
fn add(peaks: &mut crate::peaks::Builder, planar: &mut [Vec<f32>], frames: &[[f32; 2]]) {
    for (c, p) in planar.iter_mut().enumerate() {
        p.clear();
        p.extend(frames.iter().map(|f| f[c]));
    }
    peaks.add(planar);
}

impl Graph {
    /// Plan a written take's Tx (WRITE-1): its file, already durable, taken in, and one clip on
    /// lane `(module, lane)` from beat `from`, as long as the file. Answers the edit, or why it was
    /// refused.
    pub(super) fn place_take(&self, token: u64, f: &Imported, module: &str, lane: &str, from: f64) -> (Result<Value>, Option<Refused>) {
        let why = Arc::new(Mutex::new(None));
        let saw = why.clone();
        let (f, target) = (f.clone(), Target::Lane { module: module.into(), lane: lane.into(), beat: from });
        let done = self.call(move |a| {
            a.edit_and(token, |s| {
                let folder: PathBuf = s.accepted().project.dir.join(m2_song::path::ASSETS);
                m2_editor::take::seen(&folder, "tx");
                match import::place(s, &[f], &target) {
                    Ok(p) => {
                        let made: Vec<Value> =
                            p.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                        Ok((p.edited, json!({"made": made})))
                    }
                    Err(e) => {
                        *saw.lock() = Some(Refused::of(&e));
                        Err(e)
                    }
                }
            })
        });
        let why = *why.lock();
        (done, why)
    }
}

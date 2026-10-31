//! HEAD's engine, as the app drives it before step 3 (`crates/server/src/graph.rs`,
//! `core/play/src/lib.rs` at `0ff97b4`): a seek, an edit of any kind and a rebuild all build a
//! new renderer and replay the song from 0 to the playhead (RENDER-8's "Today"), the player swaps
//! it in at its next buffer, and a stopped player outputs zeros without processing. HEAD has no
//! loop; this rig loops the only way that engine could, by replaying to the loop's start at every
//! wrap, with the wrap's frame computed from its beat (RENDER-9a), so it never drifts.
//!
//! It is the gates' negative reference: it cuts tails at every jump and stop, and its seeks and
//! edits take as long as the replay. A gate that passes on it doesn't measure what it says.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use m2_runtime::{PackageSet, Renderer};

use crate::Song;
use crate::rig::{Change, Cmd, Probe, Rig, Sent};

enum Msg {
    Play,
    Stop,
    Loop(Option<(u64, u64)>),
    /// A renderer built and replayed to where the player was.
    Load {
        renderer: Box<Renderer>,
        song: Option<Box<Song>>,
        reset: bool,
    },
}

pub struct Replay {
    packages: Arc<PackageSet>,
    song: Option<Song>,
    renderer: Option<Box<Renderer>>,
    playing: bool,
    clock: u64,
    inbox: Vec<(u64, Msg)>,
    applied: HashMap<u64, u64>,
    next: u64,
    looping: Option<(u64, u64)>,
    resets: u64,
    generation: u64,
    prepared: Option<(Msg, Duration)>,
    /// Wall time spent replaying at loop wraps, where the audio thread would have waited for it.
    pub stalled: Duration,
}

impl Replay {
    pub fn new(packages: Arc<PackageSet>) -> Replay {
        Replay {
            packages,
            song: None,
            renderer: None,
            playing: false,
            clock: 0,
            inbox: vec![],
            applied: HashMap::new(),
            next: 0,
            looping: None,
            resets: 0,
            generation: 0,
            prepared: None,
            stalled: Duration::ZERO,
        }
    }

    /// HEAD's `renderer()`: a new renderer for `song`, replayed from 0 to `at`, in live mode.
    fn build(&self, song: &Song, at: u64) -> Result<(Box<Renderer>, Duration)> {
        let t = Instant::now();
        // The kernels the song was built with: its own view of the packages (PACK-9).
        let mut r = Renderer::new(&song.flat, &song.data, &self.packages.viewing(&song.code.0.view), song.rate)?;
        r.set_strict(false);
        r.seek(at)?;
        Ok((Box::new(r), t.elapsed()))
    }

    fn position(&self) -> u64 {
        self.renderer.as_ref().map_or(0, |r| r.position())
    }

    fn queue(&mut self, msg: Msg, prepare: Duration) -> Sent {
        self.next += 1;
        self.inbox.push((self.next, msg));
        Sent { id: self.next, at: self.clock, prepare, swap: false }
    }

    fn song(&self) -> Result<&Song> {
        self.song.as_ref().ok_or_else(|| anyhow::anyhow!("nothing is open"))
    }

    /// HEAD's rebuild: it reads where the player is, then replays to there while the old renderer
    /// plays on, so the new one starts behind by however long that took.
    fn rebuild(&mut self, song: &Song, reset: bool) -> Result<(Msg, Duration)> {
        let (renderer, took) = self.build(song, self.position())?;
        let song = (!reset).then(|| Box::new(song.clone()));
        Ok((Msg::Load { renderer, song, reset }, took))
    }
}

impl Rig for Replay {
    fn name(&self) -> String {
        "replay (HEAD)".into()
    }

    fn rate(&self) -> u32 {
        self.song.as_ref().map_or(48_000, |s| s.rate)
    }

    fn open(&mut self, song: &Song) -> Result<()> {
        let (renderer, _) = self.build(song, 0)?;
        *self = Replay::new(self.packages.clone());
        self.renderer = Some(renderer);
        self.song = Some(song.clone());
        Ok(())
    }

    fn fill(&mut self, out: &mut [[f32; 2]]) -> Result<()> {
        for (id, msg) in std::mem::take(&mut self.inbox) {
            self.applied.insert(id, self.clock);
            match msg {
                Msg::Play => self.playing = true,
                Msg::Stop => self.playing = false,
                Msg::Loop(l) => self.looping = l,
                Msg::Load { renderer, song, reset } => {
                    self.renderer = Some(renderer);
                    if let Some(song) = song {
                        self.song = Some(*song);
                        self.generation += 1;
                    }
                    self.resets += reset as u64;
                }
            }
        }
        self.clock += out.len() as u64;
        if !self.playing || self.renderer.is_none() {
            out.fill([0.0; 2]);
            return Ok(());
        }
        let mut done = 0;
        while done < out.len() {
            let pos = self.position();
            let mut take = out.len() - done;
            let wrap = match self.looping {
                Some((start, end)) if pos < end && end - pos <= take as u64 => {
                    take = (end - pos) as usize;
                    Some(start)
                }
                _ => None,
            };
            if self.renderer.as_mut().expect("checked").process(&mut out[done..done + take]).is_err() {
                // HEAD's player stops rather than make noise when a kernel crashes.
                self.playing = false;
                out[done..].fill([0.0; 2]);
                return Ok(());
            }
            done += take;
            if let Some(start) = wrap {
                let song = self.song()?.clone();
                let (renderer, took) = self.build(&song, start)?;
                self.renderer = Some(renderer);
                self.resets += 1;
                self.stalled += took;
            }
        }
        Ok(())
    }

    fn send(&mut self, cmd: Cmd) -> Result<Sent> {
        Ok(match cmd {
            Cmd::Play => self.queue(Msg::Play, Duration::ZERO),
            Cmd::Stop => self.queue(Msg::Stop, Duration::ZERO),
            Cmd::Loop(l) => {
                let song = self.song()?;
                let frames = l.map(|(a, b)| (song.frame(a), song.frame(b)));
                self.queue(Msg::Loop(frames), Duration::ZERO)
            }
            Cmd::Seek(beat) => {
                // HEAD's `Graph::seek`: a renderer at the beat's second, replayed to there.
                let song = self.song()?.clone();
                let (renderer, took) = self.build(&song, song.frame(beat.max(0.0)))?;
                self.queue(Msg::Load { renderer, song: None, reset: true }, took)
            }
            Cmd::Panic => {
                let song = self.song()?.clone();
                let (msg, took) = self.rebuild(&song, true)?;
                self.queue(msg, took)
            }
        })
    }

    fn prepare(&mut self, song: &Song, _: &Change) -> Result<Duration> {
        // HEAD rebuilds for every accepted change, a lane edit included (PLANE-1's "Today").
        let (msg, took) = self.rebuild(song, false)?;
        self.prepared = Some((msg, took));
        Ok(took)
    }

    fn commit(&mut self) -> Result<Sent> {
        let (msg, took) = self.prepared.take().ok_or_else(|| anyhow::anyhow!("nothing prepared"))?;
        Ok(self.queue(msg, took))
    }

    fn applied(&mut self, sent: &Sent) -> Option<u64> {
        self.applied.get(&sent.id).copied()
    }

    fn probe(&mut self) -> Probe {
        Probe {
            clock: self.clock,
            transport: Some(self.position()),
            playing: Some(self.playing),
            resets: Some(self.resets),
            generation: Some(self.generation),
            held_notes: None,
        }
    }
}

//! Step 3's live engine as a rig: `m2_play::Player::driven`, filled by the gate in simulated
//! time, and fed what the server feeds it (step 3a's design, §9.1): a renderer built without
//! seeking and activated, or for a data edit that leaves the graph as it was, `SwapLane` for each
//! notes or control lane whose data changed and fits the running scheduler. Renderers are built as
//! the server builds them (step 3b): plugin instances carried from the renderer last sent, unless
//! the rig was made not to carry (`--no-carry`, the negative reference).
//!
//! Every call into the engine is in this file, so following its interface as it changes happens
//! here; the README lists the calls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_ir::Kind;
use m2_play::{Driver, Player};
use m2_plugins::{HostEvent, Owner};
use m2_runtime::{Carry, Caps, Fresh, LaneSwap, PackageSet, Renderer, TraceKind, audio, notes_data};
use m2_song::LaneAddr;

use crate::Song;
use crate::rig::{Change, Cmd, NoteOn, Plugins, Probe, Rig, Sent};

/// Trace entries each renderer keeps: ten minutes of the loop gate's clicks, on and off.
const TRACE: usize = 1 << 16;

enum Made {
    Renderer(Box<Renderer>),
    /// `(slot, data)` for each lane to swap.
    Swaps(Vec<(u32, LaneSwap)>),
}

pub struct Live {
    packages: Arc<PackageSet>,
    player: Player,
    driver: Driver,
    rate: u32,
    song: Option<Song>,
    /// The playing renderer's generation, which `swap_lane` addresses, its lane slots, and the
    /// room each notes slot has (a swap that needs more activates instead, §4.4).
    generation: u64,
    slots: Vec<(LaneAddr, Kind)>,
    caps: Vec<Option<Caps>>,
    /// Shared by every build, as the server keeps one per open project (§6.3).
    cache: audio::Cache,
    made: Option<(Made, Duration, Song)>,
    seqs: HashMap<u64, u64>,
    next: u64,
    /// Device frames taken so far.
    delivered: u64,
    /// The last note-on read from the trace, by engine frame.
    read_to: Option<u64>,
    buf: Vec<f32>,
    /// Who its plugin instances report to, and what they reported (a restart).
    owner: Owner,
    events: m2_plugins::HostEvents,
    /// The instances of the renderer last sent, for the next build, and whether to use them.
    carry: Carry,
    carrying: bool,
    carry_epoch: u64,
    idled: bool,
}

/// One engine per `open`: what a carry is tagged with, so it never reaches another.
static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Live {
    pub fn new(packages: Arc<PackageSet>, carrying: bool) -> Result<Live> {
        Ok(Live::at(packages, 48_000, carrying))
    }

    fn at(packages: Arc<PackageSet>, rate: u32, carrying: bool) -> Live {
        let (player, driver) = Player::driven(rate);
        let (owner, events) = Owner::new();
        Live {
            packages,
            player,
            driver,
            rate,
            song: None,
            generation: 0,
            slots: vec![],
            caps: vec![],
            cache: audio::Cache::default(),
            made: None,
            seqs: HashMap::new(),
            next: 0,
            delivered: 0,
            read_to: None,
            buf: vec![],
            owner,
            events,
            carry: Carry::none(),
            carrying,
            carry_epoch: EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            idled: false,
        }
    }

    fn epoch(&self) -> u64 {
        self.carry_epoch
    }

    /// A renderer for `song` as the server builds one: not sought, sharing the audio cache, its
    /// plugin instances carried from the renderer last sent (unless `fresh`, or the rig doesn't
    /// carry), and keeping a trace for the gates.
    fn build(&mut self, song: &Song, fresh: bool) -> Result<Box<Renderer>> {
        let epoch = self.epoch();
        let source: &mut dyn m2_runtime::InstanceSource = if fresh || !self.carrying { &mut Fresh } else { self.carry.source(epoch) };
        let mut r = Renderer::build_live(&song.flat, &song.data, &self.packages, song.rate, &mut self.cache, &self.owner, source)?;
        // The cache keeps what this build used, as the server's does: what the renderer it
        // replaces played alone goes with it.
        self.cache.prune();
        r.trace(TRACE);
        Ok(Box::new(r))
    }

    /// Remember the lane slots and room of `r`, about to play.
    fn adopt(&mut self, r: &Renderer) -> Result<()> {
        self.slots.clear();
        for s in r.lane_slots() {
            self.slots.push((LaneAddr::new(&s.module, &s.lane).map_err(anyhow::Error::msg)?, s.kind));
        }
        self.caps = (0..self.slots.len()).map(|s| r.caps(s)).collect();
        self.generation = r.generation();
        Ok(())
    }

    fn stamp(&mut self, seq: u64, prepare: Duration, swap: bool) -> Sent {
        self.next += 1;
        self.seqs.insert(self.next, seq);
        Sent { id: self.next, at: self.delivered, prepare, swap }
    }

    /// The swaps that turn what plays into `song`: when the graph and rate are the same and every
    /// changed lane is a notes lane that fits its scheduler or a control lane (§9.1, path 4).
    fn swaps(&self, song: &Song, lanes: &[LaneAddr]) -> Option<Vec<(u32, LaneSwap)>> {
        let now = self.song.as_ref()?;
        if now.flat != song.flat || now.rate != song.rate {
            return None;
        }
        let mut out = vec![];
        for addr in lanes {
            let slot = self.slots.iter().position(|(a, _)| a == addr)?;
            let (module, lane) = (addr.module.as_str(), addr.lane.as_str());
            let swap = match self.slots[slot].1 {
                Kind::Events(_) => {
                    let notes = notes_data(&song.data, module, lane, song.rate as f64);
                    if !self.caps[slot]?.fits(notes.needs()) {
                        return None;
                    }
                    LaneSwap::Notes(Arc::new(notes))
                }
                Kind::Control(_) => LaneSwap::Control(Arc::new(song.data.control(module, lane))),
                _ => return None,
            };
            out.push((slot as u32, swap));
        }
        Some(out)
    }
}

impl Rig for Live {
    fn name(&self) -> String {
        if self.carrying { "engine (step 3)".into() } else { "engine (step 3, plugin instances never carried)".into() }
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn open(&mut self, song: &Song) -> Result<()> {
        // A fresh engine every time: RENDER-33's, and every gate's, starting point. Its plugin
        // instances are new too: a carry never reaches another engine.
        *self = Live::at(self.packages.clone(), song.rate, self.carrying);
        let r = self.build(song, true)?;
        self.adopt(&r)?;
        self.carry = Carry::of(&r, self.epoch());
        self.player.open(*r)?;
        self.song = Some(song.clone());
        Ok(())
    }

    fn fill(&mut self, out: &mut [[f32; 2]]) -> Result<()> {
        self.buf.resize(out.len() * 2, 0.0);
        self.driver.fill(&mut self.buf, 2);
        for (o, f) in out.iter_mut().zip(self.buf.as_chunks::<2>().0) {
            *o = *f;
        }
        self.delivered += out.len() as u64;
        // What the audio thread retired is dropped here, off it: plugin instances no renderer
        // holds any more are destroyed on the plugin main thread.
        drop(self.driver.drain_retired());
        self.idled |= self.driver.engine().is_some_and(|e| e.is_idle());
        for e in self.events.try_iter() {
            if let HostEvent::Restart(h) = e {
                self.carry.restart(h);
            }
        }
        if self.player.crashed() {
            return Err(anyhow!("the engine panicked on the audio thread"));
        }
        Ok(())
    }

    fn send(&mut self, cmd: Cmd) -> Result<Sent> {
        let t = Instant::now();
        let seq = match cmd {
            Cmd::Play => self.player.transport(m2_runtime::Cmd::Play)?,
            Cmd::Stop => self.player.transport(m2_runtime::Cmd::Stop)?,
            Cmd::Seek(beat) => self.player.seek(beat)?,
            Cmd::Loop(l) => self.player.set_loop(l)?,
            Cmd::Panic => self.player.panic()?,
        };
        Ok(self.stamp(seq, t.elapsed(), false))
    }

    fn prepare(&mut self, song: &Song, change: &Change) -> Result<Duration> {
        let t = Instant::now();
        let swaps = match change {
            Change::Data(lanes) => self.swaps(song, lanes),
            Change::Structure => None,
        };
        let made = match swaps {
            Some(s) => Made::Swaps(s),
            None => Made::Renderer(self.build(song, false)?),
        };
        let took = t.elapsed();
        self.made = Some((made, took, song.clone()));
        Ok(took)
    }

    fn commit(&mut self) -> Result<Sent> {
        let (made, took, song) = self.made.take().context("nothing prepared")?;
        let t = Instant::now();
        let sent = match made {
            Made::Swaps(swaps) => {
                let mut seq = 0;
                for (slot, data) in swaps {
                    seq = self.player.swap_lane(self.generation, slot, data)?;
                }
                self.stamp(seq, took + t.elapsed(), true)
            }
            Made::Renderer(r) => {
                self.adopt(&r)?;
                // The next build carries from what this one holds (HOST-5).
                self.carry = Carry::of(&r, self.epoch());
                let (_, seq) = self.player.activate(*r)?;
                self.stamp(seq, took + t.elapsed(), false)
            }
        };
        self.song = Some(song);
        Ok(sent)
    }

    fn applied(&mut self, sent: &Sent) -> Option<u64> {
        self.player.applied(*self.seqs.get(&sent.id)?).map(|a| a.clock)
    }

    fn probe(&mut self) -> Probe {
        let s = self.player.status();
        let engine = self.driver.engine();
        let (resets, activations, held) = match engine {
            Some(e) => (Some(e.resets()), Some(e.activations()), e.current().map(|r| r.held_notes())),
            None => (None, None, None),
        };
        Probe { clock: self.delivered, transport: Some(s.pos), playing: Some(s.playing), resets, generation: activations, held_notes: held }
    }

    fn audio_work(&self) -> Option<(usize, usize)> {
        Some(self.cache.work())
    }

    fn plugins(&mut self) -> Option<Plugins> {
        let (created, destroyed) = m2_plugins::InProcess::counts();
        let nodes = self.driver.engine()?.current()?.plugins();
        Some(Plugins { nodes, created, destroyed, idled: self.idled, owner: Some(self.owner.clone()) })
    }

    fn ons(&mut self) -> Option<Vec<NoteOn>> {
        let entries = self.driver.engine()?.current()?.trace_entries();
        let from = self.read_to;
        let ons: Vec<NoteOn> = entries
            .iter()
            .filter(|e| from.is_none_or(|f| e.engine > f))
            .filter_map(|e| match e.kind {
                TraceKind::On { pitch } => Some(NoteOn { engine: e.engine, transport: e.transport, pitch }),
                _ => None,
            })
            .collect();
        if let Some(last) = entries.last() {
            self.read_to = Some(last.engine);
        }
        Some(ons)
    }
}

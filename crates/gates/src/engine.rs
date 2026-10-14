//! Step 3's live engine as a rig: `m2_play::Player::driven`, filled by the gate in simulated
//! time, and fed what the server feeds it (step 3a's design, §9.1): a plan prepared against the
//! one last sent and activated, or for a data edit that leaves the graph as it was, `SwapLane` for
//! each notes or control lane whose data changed and fits the running scheduler. Plans are
//! prepared as the server prepares them (step 7): every instance that continues, named as the
//! session names them, moves into the next plan, unless the rig was made to keep nothing
//! (`--no-arena`, the negative reference).
//!
//! Every call into the engine is in this file, so following its interface as it changes happens
//! here; the README lists the calls.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use m2_ir::Kind;
use m2_play::{Driver, Player};
use m2_plugins::{Handle, HostEvent, Owner};
use m2_runtime::{Basis, Continues, Keep, LaneSwap, PackageSet, PlanInfo, Renderer, Stats, TraceKind, audio, notes_data};

use crate::Song;
use crate::rig::{Arena, Change, Cmd, NoteOn, Plugins, Probe, Rig, Sent};

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
    /// The plan last sent: what the next is prepared against, its generation, which `swap_lane`
    /// addresses, its lanes, and the room each notes lane has (a swap that needs more activates
    /// instead, §4.4).
    info: Option<Arc<PlanInfo>>,
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
    /// Instances that asked to be restarted: the next plan makes them anew.
    restarts: HashSet<Handle>,
    /// Whether the next plan keeps what continues (else `--no-arena`: nothing).
    arena: bool,
    idled: bool,
}

impl Live {
    pub fn new(packages: Arc<PackageSet>, arena: bool) -> Result<Live> {
        Ok(Live::at(packages, 48_000, arena))
    }

    fn at(packages: Arc<PackageSet>, rate: u32, arena: bool) -> Live {
        let (player, driver) = Player::driven(rate);
        let (owner, events) = Owner::new();
        Live {
            packages,
            player,
            driver,
            rate,
            song: None,
            info: None,
            cache: audio::Cache::default(),
            made: None,
            seqs: HashMap::new(),
            next: 0,
            delivered: 0,
            read_to: None,
            buf: vec![],
            owner,
            events,
            restarts: HashSet::new(),
            arena,
            idled: false,
        }
    }

    /// A plan for `song` as the server prepares one: not sought, sharing the audio cache, against
    /// the plan last sent (unless `fresh`), keeping every instance that continues (none without
    /// the arena), and keeping a trace for the gates.
    fn build(&mut self, song: &Song, fresh: bool) -> Result<Box<Renderer>> {
        let Live { packages, cache, owner, song: old, info, restarts, arena, .. } = self;
        let cont = match (old.as_ref(), fresh, *arena) {
            (None, ..) | (_, true, _) => None,
            (Some(_), false, false) => Some(Continues::nothing(&song.flat)),
            (Some(old), false, true) if Arc::ptr_eq(old.code.0.code(), song.code.0.code()) => Some(Continues::identity(&song.flat)),
            (Some(old), false, true) => Some(song.continues(old)),
        };
        let basis = match (old.as_ref(), info.as_deref(), &cont) {
            (Some(old), Some(info), Some(cont)) => Basis::Keep(Keep { old: &old.flat, old_data: &old.data, info, cont, restarts }),
            _ => Basis::Fresh,
        };
        let mut r = Renderer::prepare(&song.flat, &song.data, packages, song.rate, cache, owner, basis)?;
        // The cache keeps what this build used, as the server's does: what the plan it replaces
        // played alone goes with it.
        cache.prune();
        r.trace(TRACE);
        Ok(Box::new(r))
    }

    fn stamp(&mut self, seq: u64, prepare: Duration, swap: bool) -> Sent {
        self.next += 1;
        self.seqs.insert(self.next, seq);
        Sent { id: self.next, at: self.delivered, prepare, swap }
    }

    /// The swaps that turn what plays into `song`: when the graph and rate are the same and every
    /// changed lane is a notes lane that fits its scheduler or a control lane (§9.1, path 4).
    fn swaps(&self, song: &Song, lanes: &[m2_song::LaneAddr]) -> Option<Vec<(u32, LaneSwap)>> {
        let (now, info) = (self.song.as_ref()?, self.info.as_ref()?);
        if now.flat != song.flat || now.rate != song.rate {
            return None;
        }
        let mut out = vec![];
        for addr in lanes {
            let (module, lane) = (addr.module.as_str(), addr.lane.as_str());
            let slot = info.lanes.iter().position(|l| l.lane.module == module && l.lane.lane == lane)?;
            let swap = match info.lanes[slot].lane.kind {
                Kind::Events(_) => {
                    let notes = notes_data(&song.data, module, lane, song.rate as f64);
                    if !info.lanes[slot].caps?.fits(notes.needs()) {
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
        if self.arena { "engine (step 7)".into() } else { "engine (step 7, no instance kept)".into() }
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn open(&mut self, song: &Song) -> Result<()> {
        // A fresh engine every time: RENDER-33's, and every gate's, starting point. Its plugin
        // instances are new too: a carry never reaches another engine.
        *self = Live::at(self.packages.clone(), song.rate, self.arena);
        let r = self.build(song, true)?;
        self.info = Some(r.info());
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
                self.restarts.insert(h);
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
                let generation = self.info.as_ref().map_or(0, |i| i.id);
                let mut seq = 0;
                for (slot, data) in swaps {
                    seq = self.player.swap_lane(generation, slot, data)?;
                }
                self.stamp(seq, took + t.elapsed(), true)
            }
            Made::Renderer(r) => {
                // The next plan is prepared against this one.
                self.info = Some(r.info());
                self.restarts.clear();
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

    fn arena(&mut self) -> Option<Arena> {
        let e = self.driver.engine()?;
        let stats: Stats = e.current()?.stats().clone();
        Some(Arena { stats, refused: e.refused_activations(), leaked: e.leaked(), window: e.in_window() })
    }

    fn output_of(&mut self, node: usize) -> Option<Vec<f32>> {
        Some(self.driver.engine()?.current()?.output_of(node).to_vec())
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

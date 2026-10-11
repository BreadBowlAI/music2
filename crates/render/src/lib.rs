//! L3 — song → audio. One code path for realtime playback and offline export.
//!
//! A [`Renderer`] is compiled from an immutable song snapshot (off the audio thread).
//! When the song changes, a new renderer is compiled and the audio thread calls
//! [`Renderer::adopt`], which moves live device state (ringing voices, reverb tails)
//! over from tracks whose device chain didn't structurally change.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use m2_dsp::{Frame, Smooth, db_to_gain, pan_gains};
use m2_graph::{BLOCK, Ctx, DeviceRt, MacroValues, Registry, SampleData, resolve_macros};
use m2_project::{AutoTarget, ClipContent, Device, Id, PPQ, Song, Tempo, Tick, TrackKind};

/// Decoded audio for every asset, keyed by asset id.
#[derive(Clone, Default)]
pub struct SampleBank {
    pub samples: HashMap<Id, Arc<SampleData>>,
}

impl SampleBank {
    pub fn get(&self, id: Id) -> Option<&Arc<SampleData>> {
        self.samples.get(&id)
    }
}

/// Decode a WAV file to stereo f32 frames.
pub fn load_wav(path: &Path, root: u8) -> Result<SampleData> {
    let mut reader = hound::WavReader::open(path).with_context(|| format!("opening {}", path.display()))?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let frames = raw.chunks(ch).map(|c| if ch == 1 { [c[0], c[0]] } else { [c[0], c[1]] }).collect();
    Ok(SampleData { frames, sr: spec.sample_rate as f32, root })
}

pub fn write_wav(path: &Path, frames: &[Frame], sr: u32) -> Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate: sr, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    let scale = ((1 << 23) - 1) as f32;
    for f in frames {
        for s in f {
            w.write_sample((s.clamp(-1.0, 1.0) * scale) as i32)?;
        }
    }
    w.finalize()?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct NoteEvent {
    /// Absolute sample position (precomputed so scheduling is independent of block size).
    sample: i64,
    on: bool,
    pitch: u8,
    velocity: u8,
}

struct AudioClipRt {
    start: Tick,
    end: Tick,
    data: Arc<SampleData>,
    offset_secs: f64,
    gain: f32,
}

struct DeviceSlot {
    id: Id,
    /// Structural identity (plugin + asset bindings). Macro changes don't change it.
    key: String,
    macros: MacroValues,
    rt: DeviceRt,
    /// Index of the track feeding this device's sidechain input.
    sidechain: Option<usize>,
}

enum LaneTarget {
    Volume,
    Pan,
    /// `None` slot = the instrument; `Some(i)` = effect `i`.
    Macro {
        slot: Option<usize>,
        macro_id: String,
    },
}

struct LaneRt {
    target: LaneTarget,
    /// (sample, value), sorted.
    points: Vec<(i64, f32)>,
}

impl LaneRt {
    fn value_at(&self, s: i64) -> Option<f32> {
        let p = &self.points;
        let first = p.first()?;
        if s <= first.0 {
            return Some(first.1);
        }
        let i = p.partition_point(|x| x.0 <= s);
        if i >= p.len() {
            return Some(p[p.len() - 1].1);
        }
        let (a, b) = (p[i - 1], p[i]);
        Some(a.1 + (b.1 - a.1) * ((s - a.0) as f32 / (b.0 - a.0).max(1) as f32))
    }
}

struct TrackRt {
    id: Id,
    is_bus: bool,
    audible: bool,
    volume_db: f32,
    pan: f32,
    instrument: Option<DeviceSlot>,
    effects: Vec<DeviceSlot>,
    events: Vec<NoteEvent>,
    audio: Vec<AudioClipRt>,
    /// Index of the bus this track feeds (`None` = master).
    output: Option<usize>,
    /// (bus index, gain, pre-fader)
    sends: Vec<(usize, f32, bool)>,
    lanes: Vec<LaneRt>,
    gain: Smooth,
    peak: f32,
}

/// The project graph, compiled: tracks processed in routing order so every sidechain
/// source and every bus input is ready before it's read.
pub struct Renderer {
    song: Arc<Song>,
    sr: f32,
    tempo: Tempo,
    tracks: Vec<TrackRt>,
    order: Vec<usize>,
    /// Per track: post-effects, pre-fader signal (sidechain sources, pre-fader sends).
    pre: Vec<Vec<Frame>>,
    /// Per track: post-fader signal.
    post: Vec<Vec<Frame>>,
    /// Per bus: sum of everything routed into it this block.
    bus_in: Vec<Vec<Frame>>,
    master: Vec<DeviceSlot>,
    master_gain: f32,
    mix_buf: Vec<Frame>,
    pub master_peak: f32,
    /// Problems found while compiling (missing plugins, …). Shown in the UI.
    pub warnings: Vec<String>,
}

fn device_key(d: &Device) -> String {
    format!("{}|{:?}|{}", d.plugin, d.assets, d.bypass)
}

fn build_device(
    d: &Device,
    reg: &Registry,
    bank: &SampleBank,
    sr: f32,
    index_of: &dyn Fn(Id) -> Option<usize>,
    warnings: &mut Vec<String>,
) -> Option<DeviceSlot> {
    if d.bypass {
        return None;
    }
    let Some(def) = reg.get(&d.plugin) else {
        warnings.push(format!("missing plugin `{}`", d.plugin));
        return None;
    };
    let macros = resolve_macros(&def, &d.macros);
    let samples: BTreeMap<String, Arc<SampleData>> =
        d.assets.iter().filter_map(|(slot, id)| bank.get(*id).map(|s| (slot.clone(), s.clone()))).collect();
    match DeviceRt::new(&def, &macros, &samples, sr) {
        Ok(rt) => Some(DeviceSlot { id: d.id, key: device_key(d), macros, rt, sidechain: d.sidechain.and_then(index_of) }),
        Err(e) => {
            warnings.push(format!("{}: {e:#}", d.plugin));
            None
        }
    }
}

impl Renderer {
    pub fn new(song: Arc<Song>, reg: &Registry, bank: &SampleBank, sr: f32) -> Renderer {
        let tempo = song.tempo();
        let mut warnings = Vec::new();
        let index_of = |id: Id| song.tracks.iter().position(|t| t.id == id);
        let at = |t: Tick| tempo.tick_to_sample(t, sr as f64);
        let tracks: Vec<TrackRt> = song
            .tracks
            .iter()
            .map(|t| {
                let mut events = Vec::new();
                let mut audio = Vec::new();
                for c in &t.clips {
                    match &c.content {
                        ClipContent::Midi { notes } => {
                            for n in notes {
                                if n.start >= c.length || n.start < 0 {
                                    continue;
                                }
                                let end = n.end().min(c.length);
                                events.push(NoteEvent { sample: at(c.start + n.start), on: true, pitch: n.pitch, velocity: n.velocity });
                                events.push(NoteEvent { sample: at(c.start + end), on: false, pitch: n.pitch, velocity: 0 });
                            }
                        }
                        ClipContent::Audio { asset, offset, gain_db } => match bank.get(*asset) {
                            Some(data) => audio.push(AudioClipRt {
                                start: c.start,
                                end: c.end(),
                                data: data.clone(),
                                offset_secs: *offset,
                                gain: db_to_gain(*gain_db),
                            }),
                            None => warnings.push(format!("audio for clip `{}` is missing", c.name)),
                        },
                    }
                }
                // Note-offs sort before note-ons at the same tick so repeated notes retrigger.
                events.sort_by_key(|e| (e.sample, e.on));
                let instrument = t.instrument.as_ref().and_then(|d| build_device(d, reg, bank, sr, &index_of, &mut warnings));
                let effects: Vec<DeviceSlot> =
                    t.effects.iter().filter_map(|d| build_device(d, reg, bank, sr, &index_of, &mut warnings)).collect();
                let lanes = t
                    .automation
                    .iter()
                    .filter_map(|l| {
                        let target = match &l.target {
                            AutoTarget::Volume => LaneTarget::Volume,
                            AutoTarget::Pan => LaneTarget::Pan,
                            AutoTarget::Macro { device, macro_id } => {
                                let slot = if instrument.as_ref().is_some_and(|d| d.id == *device) {
                                    None
                                } else {
                                    Some(effects.iter().position(|d| d.id == *device)?)
                                };
                                LaneTarget::Macro { slot, macro_id: macro_id.clone() }
                            }
                        };
                        Some(LaneRt { target, points: l.points.iter().map(|p| (at(p.tick), p.value)).collect() })
                    })
                    .collect();
                let audible = song.is_audible(t);
                TrackRt {
                    id: t.id,
                    is_bus: t.kind == TrackKind::Bus,
                    audible,
                    volume_db: t.volume_db,
                    pan: t.pan,
                    instrument,
                    effects,
                    events,
                    audio,
                    output: t.output.and_then(index_of),
                    sends: t.sends.iter().filter_map(|x| Some((index_of(x.to)?, db_to_gain(x.gain_db), x.pre_fader))).collect(),
                    lanes,
                    gain: Smooth::new(if audible { db_to_gain(t.volume_db) } else { 0.0 }, 0.02, sr),
                    peak: 0.0,
                }
            })
            .collect();
        let order = song.routing_order().unwrap_or_else(|e| {
            warnings.push(format!("routing: {e:#}"));
            (0..song.tracks.len()).collect()
        });
        let master = song.master.effects.iter().filter_map(|d| build_device(d, reg, bank, sr, &index_of, &mut warnings)).collect();
        let n = tracks.len();
        Renderer {
            master_gain: db_to_gain(song.master.volume_db),
            song,
            sr,
            tempo,
            tracks,
            order,
            pre: vec![vec![[0.0; 2]; BLOCK]; n],
            post: vec![vec![[0.0; 2]; BLOCK]; n],
            bus_in: vec![vec![[0.0; 2]; BLOCK]; n],
            master,
            mix_buf: vec![[0.0; 2]; BLOCK],
            master_peak: 0.0,
            warnings,
        }
    }

    pub fn song(&self) -> &Arc<Song> {
        &self.song
    }

    pub fn sample_rate(&self) -> f32 {
        self.sr
    }

    pub fn tempo(&self) -> Tempo {
        self.tempo
    }

    /// Move live device state from `old` into `self` wherever the structure matches.
    pub fn adopt_from(&mut self, old: &mut Renderer) {
        fn take(new: &mut DeviceSlot, old: &mut DeviceSlot) {
            if new.key == old.key {
                std::mem::swap(&mut new.rt, &mut old.rt);
                if new.macros != old.macros {
                    new.rt.set_macros(&new.macros);
                }
            }
        }
        for t in &mut self.tracks {
            let Some(o) = old.tracks.iter_mut().find(|o| o.id == t.id) else { continue };
            if let (Some(n), Some(o)) = (&mut t.instrument, &mut o.instrument) {
                take(n, o);
            }
            for n in &mut t.effects {
                if let Some(o) = o.effects.iter_mut().find(|o| o.id == n.id) {
                    take(n, o);
                }
            }
            t.gain.value = o.gain.value;
        }
        for n in &mut self.master {
            if let Some(o) = old.master.iter_mut().find(|o| o.id == n.id) {
                take(n, o);
            }
        }
    }

    /// Silence everything (seek / stop). `hard` also cuts tails.
    pub fn all_notes_off(&mut self, hard: bool) {
        for t in &mut self.tracks {
            if let Some(i) = &mut t.instrument {
                i.rt.all_notes_off(hard);
            }
        }
    }

    /// Play a note on a track right now (piano-roll clicks, computer keyboard).
    pub fn live_note(&mut self, track: Id, pitch: u8, velocity: u8, on: bool) {
        if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track)
            && let Some(i) = &mut t.instrument
        {
            if on {
                i.rt.note_on(pitch, velocity);
            } else {
                i.rt.note_off(pitch);
            }
        }
    }

    pub fn track_peaks(&self) -> Vec<(Id, f32)> {
        self.tracks.iter().map(|t| (t.id, t.peak)).collect()
    }

    /// Render `out.len()` frames starting at sample `pos`, with timeline events when
    /// `playing`. The caller guarantees continuity (call `all_notes_off` after jumps).
    pub fn render(&mut self, pos: i64, playing: bool, out: &mut [Frame]) {
        let mut done = 0;
        while done < out.len() {
            let n = (out.len() - done).min(BLOCK);
            self.render_block(pos + done as i64, playing, &mut out[done..done + n]);
            done += n;
        }
    }

    fn render_block(&mut self, pos: i64, playing: bool, out: &mut [Frame]) {
        let n = out.len();
        let sr = self.sr;
        let tempo = self.tempo;
        let t0 = tempo.sample_to_tick_f(pos, sr as f64);
        let base = Ctx { sr, bpm: tempo.bpm as f32, beat_pos: t0 / PPQ as f64, note_freq: 0.0, velocity: 0.0, gate: false };
        let bps = base.beats_per_sample();
        out.fill([0.0; 2]);
        for b in &mut self.bus_in {
            b[..n].fill([0.0; 2]);
        }
        let mut master_peak: f32 = 0.0;

        for &ti in &self.order {
            let t = &mut self.tracks[ti];
            let mut buf = std::mem::take(&mut self.pre[ti]);
            let work = &mut buf[..n];
            work.fill([0.0; 2]);

            // Automation (control rate: evaluated once per block, smoothed by the fader).
            let mut volume_db = t.volume_db;
            let mut pan = t.pan;
            for lane in &t.lanes {
                let Some(v) = lane.value_at(pos) else { continue };
                match &lane.target {
                    LaneTarget::Volume => volume_db = v,
                    LaneTarget::Pan => pan = v.clamp(-1.0, 1.0),
                    LaneTarget::Macro { slot, macro_id } => {
                        let dev = match slot {
                            None => t.instrument.as_mut(),
                            Some(i) => t.effects.get_mut(*i),
                        };
                        if let Some(d) = dev
                            && d.macros.get(macro_id).is_some_and(|cur| (cur - v).abs() > 1e-4)
                        {
                            d.macros.insert(macro_id.clone(), v.clamp(0.0, 1.0));
                            d.rt.set_macros(&d.macros);
                        }
                    }
                }
            }

            // Source: instrument (split at note events), audio clips, or the bus's inputs.
            if let Some(inst) = &mut t.instrument {
                let mut cursor = 0usize;
                if playing {
                    let first = t.events.partition_point(|e| e.sample < pos);
                    for e in &t.events[first..] {
                        if e.sample >= pos + n as i64 {
                            break;
                        }
                        let at = (e.sample - pos) as usize;
                        if at > cursor {
                            let ctx = Ctx { beat_pos: base.beat_pos + cursor as f64 * bps, ..base };
                            inst.rt.process(&ctx, &mut work[cursor..at], at - cursor);
                            cursor = at;
                        }
                        if e.on {
                            inst.rt.note_on(e.pitch, e.velocity);
                        } else {
                            inst.rt.note_off(e.pitch);
                        }
                    }
                }
                if cursor < n {
                    let ctx = Ctx { beat_pos: base.beat_pos + cursor as f64 * bps, ..base };
                    inst.rt.process(&ctx, &mut work[cursor..n], n - cursor);
                }
            }
            if playing {
                for clip in &t.audio {
                    mix_audio_clip(clip, &tempo, sr, pos, work);
                }
            }
            if t.is_bus {
                for (o, x) in work.iter_mut().zip(&self.bus_in[ti][..n]) {
                    o[0] += x[0];
                    o[1] += x[1];
                }
            }

            // Effects, with sidechains read from already-processed tracks.
            for fx in &mut t.effects {
                let sc = fx.sidechain.map(|j| &self.pre[j][..n]);
                fx.rt.process_sc(&base, work, sc, n);
            }

            // Pre-fader sends, then fader + pan into this track's post buffer.
            for &(bus, g, pre) in &t.sends {
                if pre {
                    for (o, x) in self.bus_in[bus][..n].iter_mut().zip(work.iter()) {
                        o[0] += x[0] * g;
                        o[1] += x[1] * g;
                    }
                }
            }
            let target = if t.audible { db_to_gain(volume_db) } else { 0.0 };
            let (pl, pr) = pan_gains(pan);
            let post = &mut self.post[ti][..n];
            let mut peak: f32 = 0.0;
            for (o, x) in post.iter_mut().zip(work.iter()) {
                let g = t.gain.next(target);
                *o = [x[0] * g * pl, x[1] * g * pr];
                peak = peak.max(o[0].abs()).max(o[1].abs());
            }
            t.peak = peak.max(t.peak * 0.9);

            // Route: into a bus or the master mix, plus post-fader sends.
            let dest = match t.output {
                Some(bus) => &mut self.bus_in[bus][..n],
                None => &mut out[..],
            };
            for (o, x) in dest.iter_mut().zip(post.iter()) {
                o[0] += x[0];
                o[1] += x[1];
            }
            for &(bus, g, pre) in &t.sends {
                if !pre {
                    for (o, x) in self.bus_in[bus][..n].iter_mut().zip(post.iter()) {
                        o[0] += x[0] * g;
                        o[1] += x[1] * g;
                    }
                }
            }
            self.pre[ti] = buf;
        }

        let mix = &mut self.mix_buf[..n];
        mix.copy_from_slice(out);
        for fx in &mut self.master {
            fx.rt.process(&base, mix, n);
        }
        for (o, x) in out.iter_mut().zip(mix.iter()) {
            o[0] = x[0] * self.master_gain;
            o[1] = x[1] * self.master_gain;
            master_peak = master_peak.max(o[0].abs()).max(o[1].abs());
        }
        self.master_peak = master_peak.max(self.master_peak * 0.9);
    }
}

fn mix_audio_clip(clip: &AudioClipRt, tempo: &Tempo, sr: f32, pos: i64, buf: &mut [Frame]) {
    let n = buf.len() as i64;
    let start = tempo.tick_to_sample(clip.start, sr as f64);
    let end = tempo.tick_to_sample(clip.end, sr as f64);
    if pos + n <= start || pos >= end {
        return;
    }
    let ratio = clip.data.sr as f64 / sr as f64;
    let offset = clip.offset_secs * clip.data.sr as f64;
    let len = clip.data.frames.len();
    for (i, o) in buf.iter_mut().enumerate() {
        let s = pos + i as i64;
        if s < start || s >= end {
            continue;
        }
        let src = offset + (s - start) as f64 * ratio;
        let idx = src.floor() as usize;
        if idx + 1 >= len {
            break;
        }
        let fr = (src - idx as f64) as f32;
        let a = clip.data.frames[idx];
        let b = clip.data.frames[idx + 1];
        // Short fades at clip edges avoid clicks.
        let edge = ((s - start).min(end - s) as f32 / (0.003 * sr)).min(1.0);
        let g = clip.gain * edge;
        o[0] += (a[0] + (b[0] - a[0]) * fr) * g;
        o[1] += (a[1] + (b[1] - a[1]) * fr) * g;
    }
}

/// Offline render of the whole song (plus a reverb tail).
pub fn render_song(song: Arc<Song>, reg: &Registry, bank: &SampleBank, sr: u32, tail_secs: f32) -> Vec<Frame> {
    let tempo = song.tempo();
    let total = tempo.tick_to_sample(song.end_tick(), sr as f64) + (tail_secs * sr as f32) as i64;
    let mut r = Renderer::new(song, reg, bank, sr as f32);
    let mut out = vec![[0.0f32; 2]; total.max(0) as usize];
    r.render(0, true, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use m2_project::{Clip, Note, Track, TrackKind};

    fn song_with_bass() -> Song {
        let mut s = Song::default();
        let tid = s.alloc_id();
        let cid = s.alloc_id();
        s.tracks.push(Arc::new(Track {
            id: tid,
            name: "Bass".into(),
            color: [255, 0, 0],
            kind: TrackKind::Instrument,
            instrument: Some(Device::new("m2/bass")),
            effects: vec![Device::new("m2/space")],
            clips: vec![Arc::new(Clip {
                id: cid,
                name: "c".into(),
                start: 0,
                length: 4 * PPQ,
                content: ClipContent::Midi {
                    notes: (0..4).map(|i| Note { id: 100 + i as u64, pitch: 45, start: i * PPQ, length: PPQ / 2, velocity: 100 }).collect(),
                },
            })],
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: vec![],
            automation: vec![],
        }));
        s.assign_device_ids();
        s
    }

    fn rms(x: &[Frame]) -> f32 {
        (x.iter().map(|f| f[0] * f[0] + f[1] * f[1]).sum::<f32>() / (2 * x.len().max(1)) as f32).sqrt()
    }

    /// A long sustained bass note, so level changes come from routing, not the envelope.
    fn pad_song() -> Song {
        let mut s = song_with_bass();
        // No master chain: levels in these tests should be pure routing math.
        s.master.effects.clear();
        let note_id = s.alloc_id();
        let t = s.track_at_mut(0);
        t.effects.clear();
        let clip = Arc::make_mut(&mut t.clips[0]);
        *clip.notes_mut().unwrap() = vec![Note { id: note_id, pitch: 45, start: 0, length: 4 * PPQ, velocity: 100 }];
        s
    }

    fn render_secs(song: Song, secs: f32) -> Vec<Frame> {
        let reg = Registry::with_builtins();
        let mut r = Renderer::new(Arc::new(song), &reg, &SampleBank::default(), 48_000.0);
        let mut out = vec![[0.0f32; 2]; (secs * 48_000.0) as usize];
        r.render(0, true, &mut out);
        out
    }

    fn bus(s: &mut Song, name: &str) -> Id {
        let id = s.alloc_id();
        s.tracks.push(Arc::new(Track {
            id,
            name: name.into(),
            color: [0; 3],
            kind: TrackKind::Bus,
            instrument: None,
            effects: vec![],
            clips: vec![],
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: vec![],
            automation: vec![],
        }));
        id
    }

    #[test]
    fn bus_routing_and_sends() {
        let dry = rms(&render_secs(pad_song(), 1.0));
        // Through a bus at -inf: silence. Through a bus at 0 dB: same level.
        let mut s = pad_song();
        let b = bus(&mut s, "Bus");
        s.track_at_mut(0).output = Some(b);
        s.track_at_mut(1).volume_db = -96.0;
        assert!(rms(&render_secs(s.clone(), 1.0)) < dry * 0.01, "track goes through the bus");
        s.track_at_mut(1).volume_db = 0.0;
        let via_bus = rms(&render_secs(s, 1.0));
        assert!((via_bus / dry - 1.0).abs() < 0.05, "unity bus is transparent ({via_bus} vs {dry})");
        // A 0 dB post-fader send adds a second copy: louder than dry.
        let mut s = pad_song();
        let b = bus(&mut s, "Verb");
        s.track_at_mut(0).sends.push(m2_project::Send { to: b, gain_db: 0.0, pre_fader: false });
        let sent = rms(&render_secs(s, 1.0));
        assert!((sent / dry - 2.0).abs() < 0.1, "a unity send doubles the signal ({sent} vs {dry})");
    }

    #[test]
    fn sidechain_ducks_on_kick() {
        // Bass with m2/duck keyed from a drum track playing kicks on every beat.
        let mut s = pad_song();
        let kick_id = s.alloc_id();
        let clip_id = s.alloc_id();
        let mut notes = vec![];
        for i in 0..4 {
            notes.push(Note { id: s.alloc_id(), pitch: 36, start: i * PPQ, length: PPQ / 4, velocity: 127 });
        }
        s.tracks.push(Arc::new(Track {
            id: kick_id,
            name: "Kick".into(),
            color: [0; 3],
            kind: TrackKind::Instrument,
            instrument: Some(Device::new("m2/drums")),
            effects: vec![],
            clips: vec![Arc::new(Clip { id: clip_id, name: "k".into(), start: 0, length: 4 * PPQ, content: ClipContent::Midi { notes } })],
            volume_db: 0.0,
            pan: 0.0,
            mute: true, // ghost kick: muted, but still keys the duck
            solo: false,
            output: None,
            sends: vec![],
            automation: vec![],
        }));
        let mut duck = Device::new("m2/duck");
        duck.sidechain = Some(kick_id);
        duck.macros.insert("depth".into(), 1.0);
        s.track_at_mut(0).effects.push(duck);
        s.assign_device_ids();
        s.validate().unwrap();
        let out = render_secs(s, 2.0);
        // Compare bass level just after a kick vs just before the next one (beat = 0.49 s at 122 bpm).
        let beat = (60.0 / 122.0 * 48_000.0) as usize;
        let after = rms(&out[beat + 200..beat + 2400]);
        let before = rms(&out[2 * beat - 4000..2 * beat - 400]);
        assert!(after < before * 0.6, "ducked after the kick: {after} vs {before}");
    }

    #[test]
    fn volume_automation_ramps() {
        let mut s = pad_song();
        let lane_id = s.alloc_id();
        s.track_at_mut(0).automation.push(m2_project::AutomationLane {
            id: lane_id,
            target: AutoTarget::Volume,
            points: vec![m2_project::AutoPoint { tick: 0, value: -40.0 }, m2_project::AutoPoint { tick: 4 * PPQ, value: 0.0 }],
        });
        let out = render_secs(s, 1.9);
        let early = rms(&out[4_800..14_400]);
        let late = rms(&out[76_800..86_400]);
        assert!(late > early * 8.0, "fade-in: {early} → {late}");
    }

    #[test]
    fn macro_automation_changes_sound() {
        // Automate an effect knob (compressor makeup, 0 → +9 dB): the level rises.
        let mut s = pad_song();
        let mut glue = Device::new("m2/glue");
        glue.macros.insert("amount".into(), 0.0);
        s.track_at_mut(0).effects.push(glue);
        s.assign_device_ids();
        let dev = s.tracks[0].effects[0].id;
        let lane_id = s.alloc_id();
        s.track_at_mut(0).automation.push(m2_project::AutomationLane {
            id: lane_id,
            target: AutoTarget::Macro { device: dev, macro_id: "makeup".into() },
            points: vec![m2_project::AutoPoint { tick: 0, value: 0.0 }, m2_project::AutoPoint { tick: 4 * PPQ, value: 1.0 }],
        });
        let out = render_secs(s, 1.9);
        let (early, late) = (rms(&out[9_600..19_200]), rms(&out[76_800..86_400]));
        assert!(late > early * 2.0, "makeup automation raises the level: {early} → {late}");
    }

    #[test]
    fn renders_audio_and_respects_mute() {
        let reg = Registry::with_builtins();
        let mut song = song_with_bass();
        let out = render_song(Arc::new(song.clone()), &reg, &SampleBank::default(), 48_000, 0.5);
        let peak = out.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
        assert!(peak > 0.05 && peak <= 1.0, "peak {peak}");
        song.track_at_mut(0).mute = true;
        let out = render_song(Arc::new(song), &reg, &SampleBank::default(), 48_000, 0.5);
        let peak = out.iter().skip(4800).fold(0.0f32, |m, f| m.max(f[0].abs()));
        assert!(peak < 1e-3, "muted peak {peak}");
    }

    #[test]
    fn block_size_does_not_change_output() {
        let reg = Registry::with_builtins();
        let song = Arc::new(song_with_bass());
        let mut a = Renderer::new(song.clone(), &reg, &SampleBank::default(), 48_000.0);
        let mut b = Renderer::new(song, &reg, &SampleBank::default(), 48_000.0);
        let mut oa = vec![[0.0f32; 2]; 48_000];
        let mut ob = vec![[0.0f32; 2]; 48_000];
        a.render(0, true, &mut oa);
        let mut pos = 0;
        for chunk in ob.chunks_mut(37) {
            b.render(pos, true, chunk);
            pos += chunk.len() as i64;
        }
        let diff = oa.iter().zip(&ob).fold(0.0f32, |m, (x, y)| m.max((x[0] - y[0]).abs()));
        assert!(diff < 1e-4, "diff {diff}");
    }
}

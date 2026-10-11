//! Built-in node types. A graph can only wire these — loading a plugin never runs code.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use m2_dsp::*;

/// Decoded sample data bound to a plugin's asset slot.
#[derive(Debug)]
pub struct SampleData {
    pub frames: Vec<Frame>,
    pub sr: f32,
    pub root: u8,
}

/// Per-call context. Everything here is constant for the span being processed —
/// the renderer splits blocks at note events.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub sr: f32,
    pub bpm: f32,
    /// Song position (in beats) of the first frame.
    pub beat_pos: f64,
    pub note_freq: f32,
    pub velocity: f32,
    pub gate: bool,
}

impl Ctx {
    pub fn beats_per_sample(&self) -> f64 {
        self.bpm as f64 / 60.0 / self.sr as f64
    }
}

pub struct BuildEnv<'a> {
    pub sr: f32,
    pub samples: &'a BTreeMap<String, Arc<SampleData>>,
}

pub trait Node: Send {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]);
    /// Set a numeric parameter. Returns false for unknown names.
    fn set(&mut self, param: &str, v: f32) -> bool;
    fn gate(&mut self, _on: bool) {}
    /// Clear internal state (called when a silent voice is reused).
    fn reset(&mut self) {}
    /// `Some(true)` while this node is still producing sound after note-off (envelopes,
    /// one-shot samples). `None` if the node doesn't affect voice lifetime.
    fn activity(&self) -> Option<bool> {
        None
    }
}

/// Input slot names per node type. Multiple refs wired to one input are summed.
pub fn input_names(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "osc" => &["freq", "fm"],
        "noise" | "env" | "lfo" | "const" => &[],
        "vca" => &["in", "cv"],
        "mix" | "gain" | "pan" | "eq" | "drive" | "delay" | "reverb" | "chorus" | "pump" | "limit" => &["in"],
        "map" => &["in"],
        "filter" => &["in", "mod"],
        "comp" => &["in", "sidechain"],
        "sampler" => &["freq"],
        _ => return None,
    })
}

pub const NODE_TYPES: &[&str] = &[
    "osc", "noise", "env", "lfo", "const", "vca", "mix", "map", "filter", "eq", "drive", "delay", "reverb", "chorus", "comp", "pump",
    "gain", "pan", "limit", "sampler",
];

pub fn make_node(kind: &str, strs: &BTreeMap<String, String>, env: &BuildEnv) -> Result<Box<dyn Node>> {
    let s = |k: &str| strs.get(k).map(|s| s.as_str());
    let sr = env.sr;
    let node: Box<dyn Node> = match kind {
        "osc" => Box::new(OscNode::new(Wave::parse(s("wave").unwrap_or("saw")))),
        "noise" => Box::new(NoiseNode { n: Noise::new(0x1234_5678), level: 1.0, hits: 0 }),
        "env" => Box::new(EnvNode { env: Adsr::new(0.005, 0.2, 0.7, 0.2), amount: 1.0 }),
        "lfo" => Box::new(LfoNode {
            osc: Osc::new(0.0),
            wave: Wave::parse(s("wave").unwrap_or("sine")),
            rate: 1.0,
            depth: 1.0,
            offset: 0.0,
            sync: 0.0,
        }),
        "const" => Box::new(ConstNode(0.0)),
        "vca" => Box::new(VcaNode { gain: 1.0, velocity: 0.7 }),
        "mix" | "gain" => Box::new(GainNode { db: 0.0, smooth: Smooth::new(1.0, 0.01, sr) }),
        "map" => Box::new(MapNode { mul: 1.0, add: 0.0 }),
        "filter" => Box::new(FilterNode {
            svf: Svf::default(),
            mode: FilterMode::parse(s("mode").unwrap_or("lp")),
            cutoff: 2000.0,
            q: 0.707,
            mod_amount: 0.0,
            keytrack: 0.0,
        }),
        "eq" => Box::new(EqNode::new(sr)),
        "drive" => Box::new(DriveNode { drive: 6.0, mix: 1.0, out_db: 0.0 }),
        "delay" => Box::new(DelayNode {
            line: DelayLine::new((sr * 4.0) as usize),
            time: 0.75,
            feedback: 0.35,
            mix: 0.25,
            pingpong: 1.0,
            tone: Svf::default(),
        }),
        "reverb" => Box::new(ReverbNode { rev: Reverb::new(sr), size: 0.7, damp: 0.5, mix: 0.25, pre: Svf::default() }),
        "chorus" => Box::new(ChorusNode { line: DelayLine::new((sr * 0.05) as usize), phase: 0.0, rate: 0.6, depth: 4.0, mix: 0.4 }),
        "comp" => {
            Box::new(CompNode { comp: Compressor::default(), threshold: -18.0, ratio: 4.0, attack: 0.005, release: 0.12, makeup: 0.0 })
        }
        "pump" => Box::new(PumpNode { depth: 0.6, release: 0.5, rate: 1.0, smooth: Smooth::new(1.0, 0.002, sr) }),
        "pan" => Box::new(PanNode { pan: 0.0 }),
        "limit" => Box::new(LimitNode { comp: Compressor::default(), ceiling: -0.5 }),
        "sampler" => {
            let slot = s("asset").unwrap_or("sample");
            Box::new(SamplerNode { data: env.samples.get(slot).cloned(), pos: 0.0, playing: false, level: 1.0, start: 0.0, looped: 0.0 })
        }
        other => return Err(anyhow!("unknown node type `{other}` (known: {})", NODE_TYPES.join(", "))),
    };
    Ok(node)
}

/// Which string params a node accepts (others must be numbers or macro bindings).
pub fn is_string_param(kind: &str, param: &str) -> bool {
    matches!((kind, param), ("osc", "wave") | ("lfo", "wave") | ("filter", "mode") | ("sampler", "asset"))
}

fn input(ins: &[Option<&[Frame]>], slot: usize, i: usize) -> Frame {
    ins.get(slot).copied().flatten().map(|b| b[i]).unwrap_or([0.0; 2])
}

fn has_input(ins: &[Option<&[Frame]>], slot: usize) -> bool {
    ins.get(slot).copied().flatten().is_some()
}

// ---------------------------------------------------------------- sources

struct OscNode {
    wave: Wave,
    oscs: [Osc; 7],
    unison: usize,
    detune: f32,
    spread: f32,
    octave: f32,
    semi: f32,
    fine: f32,
    level: f32,
    fixed: f32,
    fm_amount: f32,
}

impl OscNode {
    fn new(wave: Wave) -> Self {
        let oscs = std::array::from_fn(|i| Osc::new(i as f32 * 0.137 + if i > 0 { 0.31 } else { 0.0 }));
        Self { wave, oscs, unison: 1, detune: 10.0, spread: 0.6, octave: 0.0, semi: 0.0, fine: 0.0, level: 1.0, fixed: 0.0, fm_amount: 0.0 }
    }
}

impl Node for OscNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let n = self.unison.clamp(1, 7);
        let transpose = 2f32.powf(self.octave + (self.semi + self.fine / 100.0) / 12.0);
        let norm = self.level / (n as f32).sqrt();
        let has_freq = has_input(ins, 0);
        let has_fm = has_input(ins, 1);
        for (i, o) in out.iter_mut().enumerate() {
            let mut f = if self.fixed > 0.0 {
                self.fixed
            } else if has_freq {
                input(ins, 0, i)[0]
            } else {
                ctx.note_freq
            };
            f *= transpose;
            if has_fm {
                f *= 2f32.powf(input(ins, 1, i)[0] * self.fm_amount / 12.0);
            }
            let mut acc = [0.0f32; 2];
            for (k, osc) in self.oscs.iter_mut().take(n).enumerate() {
                // Symmetric detune and alternating stereo placement.
                let pos = if n == 1 { 0.0 } else { k as f32 / (n - 1) as f32 * 2.0 - 1.0 };
                let cents = pos * self.detune;
                let s = osc.next(self.wave, f * 2f32.powf(cents / 1200.0), ctx.sr);
                let p = if n == 1 { 0.0 } else { pos * self.spread * if k % 2 == 0 { 1.0 } else { -1.0 } };
                acc[0] += s * (1.0 - p.max(0.0));
                acc[1] += s * (1.0 + p.min(0.0));
            }
            *o = [acc[0] * norm, acc[1] * norm];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "unison" => self.unison = v.round().clamp(1.0, 7.0) as usize,
            "detune" => self.detune = v,
            "spread" => self.spread = v.clamp(0.0, 1.0),
            "octave" => self.octave = v,
            "semi" => self.semi = v,
            "fine" => self.fine = v,
            "level" => self.level = v,
            "fixed" => self.fixed = v,
            "fm_amount" => self.fm_amount = v,
            _ => return false,
        }
        true
    }

    fn gate(&mut self, on: bool) {
        if on {
            // Key-synced phases: consistent attacks and deterministic renders.
            for (i, o) in self.oscs.iter_mut().enumerate() {
                o.phase = if i == 0 { if self.fixed > 0.0 { 0.25 } else { 0.0 } } else { (i as f32 * 0.137 + 0.31).fract() };
            }
        }
    }
}

struct NoiseNode {
    n: Noise,
    level: f32,
    hits: u32,
}

impl Node for NoiseNode {
    fn process(&mut self, _: &Ctx, _: &[Option<&[Frame]>], out: &mut [Frame]) {
        for o in out {
            *o = [self.n.next() * self.level, self.n.next() * self.level];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        if p == "level" {
            self.level = v;
            true
        } else {
            false
        }
    }

    fn gate(&mut self, on: bool) {
        if on {
            // Reseed per hit: every hit differs, but renders are reproducible.
            self.hits = self.hits.wrapping_add(1);
            self.n = Noise::new(0x1234_5678 ^ self.hits.wrapping_mul(0x9E37_79B9));
        }
    }
}

struct EnvNode {
    env: Adsr,
    amount: f32,
}

impl Node for EnvNode {
    fn process(&mut self, ctx: &Ctx, _: &[Option<&[Frame]>], out: &mut [Frame]) {
        for o in out {
            let v = self.env.next(ctx.sr) * self.amount;
            *o = [v, v];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "a" | "attack" => self.env.a = v,
            "d" | "decay" => self.env.d = v,
            "s" | "sustain" => self.env.s = v.clamp(0.0, 1.0),
            "r" | "release" => self.env.r = v,
            "amount" => self.amount = v,
            _ => return false,
        }
        true
    }

    fn gate(&mut self, on: bool) {
        self.env.gate(on);
    }

    fn activity(&self) -> Option<bool> {
        Some(!self.env.is_idle())
    }
}

struct LfoNode {
    osc: Osc,
    wave: Wave,
    rate: f32,
    depth: f32,
    offset: f32,
    /// If > 0, rate is cycles per `sync` beats instead of Hz.
    sync: f32,
}

impl Node for LfoNode {
    fn process(&mut self, ctx: &Ctx, _: &[Option<&[Frame]>], out: &mut [Frame]) {
        let hz = if self.sync > 0.0 { ctx.bpm / 60.0 / self.sync } else { self.rate };
        for o in out {
            let v = self.osc.next(self.wave, hz, ctx.sr) * self.depth + self.offset;
            *o = [v, v];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "rate" => self.rate = v,
            "depth" => self.depth = v,
            "offset" => self.offset = v,
            "sync" => self.sync = v,
            _ => return false,
        }
        true
    }

    fn gate(&mut self, on: bool) {
        if on {
            self.osc.phase = 0.0;
        }
    }
}

struct ConstNode(f32);

impl Node for ConstNode {
    fn process(&mut self, _: &Ctx, _: &[Option<&[Frame]>], out: &mut [Frame]) {
        out.fill([self.0; 2]);
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        if p == "value" {
            self.0 = v;
            true
        } else {
            false
        }
    }
}

struct SamplerNode {
    data: Option<Arc<SampleData>>,
    pos: f64,
    playing: bool,
    level: f32,
    start: f32,
    looped: f32,
}

impl Node for SamplerNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let Some(data) = &self.data else {
            out.fill([0.0; 2]);
            return;
        };
        let len = data.frames.len();
        let root_hz = m2_theory::midi_to_hz(data.root as f32);
        let has_freq = has_input(ins, 0);
        for (i, o) in out.iter_mut().enumerate() {
            if !self.playing || len < 2 {
                *o = [0.0; 2];
                continue;
            }
            let f = if has_freq { input(ins, 0, i)[0] } else { ctx.note_freq };
            let step = (f / root_hz) as f64 * (data.sr / ctx.sr) as f64;
            let idx = self.pos.floor() as usize;
            if idx + 1 >= len {
                if self.looped > 0.5 {
                    self.pos = (self.start as f64 * len as f64).min(len as f64 - 2.0);
                } else {
                    self.playing = false;
                }
                *o = [0.0; 2];
                continue;
            }
            let fr = (self.pos - idx as f64) as f32;
            let a = data.frames[idx];
            let b = data.frames[idx + 1];
            *o = [(a[0] + (b[0] - a[0]) * fr) * self.level, (a[1] + (b[1] - a[1]) * fr) * self.level];
            self.pos += step;
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "level" => self.level = v,
            "start" => self.start = v.clamp(0.0, 0.99),
            "loop" => self.looped = v,
            _ => return false,
        }
        true
    }

    fn gate(&mut self, on: bool) {
        if on {
            let len = self.data.as_ref().map(|d| d.frames.len()).unwrap_or(0);
            self.pos = self.start as f64 * len as f64;
            self.playing = true;
        }
    }

    fn activity(&self) -> Option<bool> {
        Some(self.playing && self.data.is_some())
    }
}

// ---------------------------------------------------------------- processors

struct VcaNode {
    gain: f32,
    velocity: f32,
}

impl Node for VcaNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let vel = 1.0 - self.velocity + self.velocity * ctx.velocity;
        let g = self.gain * vel;
        let has_cv = has_input(ins, 1);
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            let cv = if has_cv { input(ins, 1, i) } else { [1.0, 1.0] };
            *o = [x[0] * cv[0] * g, x[1] * cv[1] * g];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "gain" => self.gain = v,
            "velocity" => self.velocity = v.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }
}

struct GainNode {
    db: f32,
    smooth: Smooth,
}

impl Node for GainNode {
    fn process(&mut self, _: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let target = db_to_gain(self.db);
        for (i, o) in out.iter_mut().enumerate() {
            let g = self.smooth.next(target);
            let x = input(ins, 0, i);
            *o = [x[0] * g, x[1] * g];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        if p == "db" || p == "gain" {
            self.db = v;
            true
        } else {
            false
        }
    }
}

struct MapNode {
    mul: f32,
    add: f32,
}

impl Node for MapNode {
    fn process(&mut self, _: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            *o = [x[0] * self.mul + self.add, x[1] * self.mul + self.add];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "mul" => self.mul = v,
            "add" => self.add = v,
            _ => return false,
        }
        true
    }
}

struct FilterNode {
    svf: Svf,
    mode: FilterMode,
    cutoff: f32,
    q: f32,
    /// Octaves of cutoff shift per unit of `mod` input.
    mod_amount: f32,
    /// 0..1: how much the cutoff follows the played note (relative to C4).
    keytrack: f32,
}

impl Node for FilterNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let key = if self.keytrack > 0.0 && ctx.note_freq > 0.0 { (ctx.note_freq / 261.63).powf(self.keytrack) } else { 1.0 };
        let has_mod = has_input(ins, 1);
        for (i, o) in out.iter_mut().enumerate() {
            let m = if has_mod { input(ins, 1, i)[0] * self.mod_amount } else { 0.0 };
            let fc = self.cutoff * key * 2f32.powf(m);
            *o = self.svf.process(input(ins, 0, i), self.mode, fc, self.q, ctx.sr);
        }
    }

    fn reset(&mut self) {
        self.svf = Svf::default();
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "cutoff" => self.cutoff = v,
            "q" | "resonance" => self.q = v,
            "mod_amount" => self.mod_amount = v,
            "keytrack" => self.keytrack = v,
            _ => return false,
        }
        true
    }
}

struct EqNode {
    low: Biquad,
    mid: Biquad,
    high: Biquad,
    hp: Biquad,
    vals: [f32; 5],
    sr: f32,
    dirty: bool,
}

impl EqNode {
    fn new(sr: f32) -> Self {
        Self {
            low: Biquad::new(BiquadKind::LowShelf, 120.0, 0.7, 0.0, sr),
            mid: Biquad::new(BiquadKind::Peak, 1000.0, 0.8, 0.0, sr),
            high: Biquad::new(BiquadKind::HighShelf, 8000.0, 0.7, 0.0, sr),
            hp: Biquad::new(BiquadKind::Highpass, 20.0, 0.7, 0.0, sr),
            // low db, mid db, mid freq, high db, low cut hz
            vals: [0.0, 0.0, 1000.0, 0.0, 20.0],
            sr,
            dirty: false,
        }
    }
}

impl Node for EqNode {
    fn process(&mut self, _: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        if self.dirty {
            let [l, m, mf, h, lc] = self.vals;
            self.low.set(BiquadKind::LowShelf, 120.0, 0.7, l, self.sr);
            self.mid.set(BiquadKind::Peak, mf, 0.8, m, self.sr);
            self.high.set(BiquadKind::HighShelf, 8000.0, 0.7, h, self.sr);
            self.hp.set(BiquadKind::Highpass, lc, 0.7, 0.0, self.sr);
            self.dirty = false;
        }
        for (i, o) in out.iter_mut().enumerate() {
            let x = self.hp.process(input(ins, 0, i));
            *o = self.high.process(self.mid.process(self.low.process(x)));
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        let idx = match p {
            "low" => 0,
            "mid" => 1,
            "mid_freq" => 2,
            "high" => 3,
            "low_cut" => 4,
            _ => return false,
        };
        self.vals[idx] = v;
        self.dirty = true;
        true
    }
}

struct DriveNode {
    drive: f32,
    mix: f32,
    out_db: f32,
}

impl Node for DriveNode {
    fn process(&mut self, _: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let pre = db_to_gain(self.drive);
        // Rough loudness compensation so turning up drive doesn't just get louder.
        let post = db_to_gain(self.out_db) / pre.sqrt().max(1.0);
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            let w = [soft_clip(x[0] * pre) * post, soft_clip(x[1] * pre) * post];
            *o = [x[0] + (w[0] - x[0]) * self.mix, x[1] + (w[1] - x[1]) * self.mix];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "drive" | "amount" => self.drive = v,
            "mix" => self.mix = v.clamp(0.0, 1.0),
            "out" => self.out_db = v,
            _ => return false,
        }
        true
    }
}

struct DelayNode {
    line: DelayLine,
    /// Delay time in beats.
    time: f32,
    feedback: f32,
    mix: f32,
    pingpong: f32,
    tone: Svf,
}

impl Node for DelayNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let d = self.time * 60.0 / ctx.bpm.max(1.0) * ctx.sr;
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            let wet = self.line.read(d);
            let fb = self.tone.process(wet, FilterMode::Lowpass, 5000.0, 0.6, ctx.sr);
            let feed = if self.pingpong > 0.5 {
                // Mono input into the left, then the echoes bounce between sides.
                [(x[0] + x[1]) * 0.5 + fb[1] * self.feedback, fb[0] * self.feedback]
            } else {
                [x[0] + fb[0] * self.feedback, x[1] + fb[1] * self.feedback]
            };
            self.line.write(feed);
            *o = [x[0] + wet[0] * self.mix, x[1] + wet[1] * self.mix];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "time" => self.time = v.max(0.01),
            "feedback" => self.feedback = v.clamp(0.0, 0.95),
            "mix" => self.mix = v.clamp(0.0, 1.0),
            "pingpong" => self.pingpong = v,
            _ => return false,
        }
        true
    }
}

struct ReverbNode {
    rev: Reverb,
    size: f32,
    damp: f32,
    mix: f32,
    pre: Svf,
}

impl Node for ReverbNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            // High-pass the send so the low end stays tight.
            let send = self.pre.process(x, FilterMode::Highpass, 250.0, 0.7, ctx.sr);
            let w = self.rev.process(send, self.size, self.damp);
            *o = [x[0] + w[0] * self.mix, x[1] + w[1] * self.mix];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "size" => self.size = v,
            "damp" => self.damp = v,
            "mix" => self.mix = v.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }
}

struct ChorusNode {
    line: DelayLine,
    phase: f32,
    rate: f32,
    /// Modulation depth in ms.
    depth: f32,
    mix: f32,
}

impl Node for ChorusNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let base = 0.012 * ctx.sr;
        let depth = self.depth * 0.001 * ctx.sr;
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            self.line.write(x);
            self.phase = (self.phase + self.rate / ctx.sr).fract();
            let l = self.line.read(base + depth * (0.5 + 0.5 * (self.phase * std::f32::consts::TAU).sin()));
            let r = self.line.read(base + depth * (0.5 + 0.5 * (self.phase * std::f32::consts::TAU).cos()));
            *o = [x[0] * (1.0 - self.mix * 0.5) + l[0] * self.mix, x[1] * (1.0 - self.mix * 0.5) + r[1] * self.mix];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "rate" => self.rate = v,
            "depth" => self.depth = v.clamp(0.0, 15.0),
            "mix" => self.mix = v.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }
}

struct CompNode {
    comp: Compressor,
    threshold: f32,
    ratio: f32,
    attack: f32,
    release: f32,
    makeup: f32,
}

impl Node for CompNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let mk = db_to_gain(self.makeup);
        let sc = has_input(ins, 1);
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            let det = if sc { input(ins, 1, i) } else { x };
            let level = det[0].abs().max(det[1].abs());
            let g = self.comp.gain(level, self.threshold, self.ratio, self.attack, self.release, ctx.sr) * mk;
            *o = [x[0] * g, x[1] * g];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "threshold" => self.threshold = v,
            "ratio" => self.ratio = v,
            "attack" => self.attack = v,
            "release" => self.release = v,
            "makeup" => self.makeup = v,
            _ => return false,
        }
        true
    }
}

/// Tempo-synced ducking: the "sidechained to the kick" pump without needing a kick.
struct PumpNode {
    depth: f32,
    /// Fraction of the period it takes to recover.
    release: f32,
    /// Pumps per beat.
    rate: f32,
    smooth: Smooth,
}

impl Node for PumpNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let bps = ctx.beats_per_sample();
        for (i, o) in out.iter_mut().enumerate() {
            let phase = ((ctx.beat_pos + i as f64 * bps) * self.rate as f64).rem_euclid(1.0) as f32;
            let rec = (phase / self.release.max(0.05)).min(1.0);
            let target = 1.0 - self.depth * (1.0 - rec).powi(2);
            let g = self.smooth.next(target);
            let x = input(ins, 0, i);
            *o = [x[0] * g, x[1] * g];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        match p {
            "depth" => self.depth = v.clamp(0.0, 1.0),
            "release" => self.release = v,
            "rate" => self.rate = v.max(0.125),
            _ => return false,
        }
        true
    }
}

struct PanNode {
    pan: f32,
}

impl Node for PanNode {
    fn process(&mut self, _: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let (l, r) = pan_gains(self.pan);
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            *o = [x[0] * l, x[1] * r];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        if p == "pan" {
            self.pan = v;
            true
        } else {
            false
        }
    }
}

/// Fast compressor into a soft clipper. Not a true-peak limiter, but keeps exports sane.
struct LimitNode {
    comp: Compressor,
    ceiling: f32,
}

impl Node for LimitNode {
    fn process(&mut self, ctx: &Ctx, ins: &[Option<&[Frame]>], out: &mut [Frame]) {
        let ceil = db_to_gain(self.ceiling);
        for (i, o) in out.iter_mut().enumerate() {
            let x = input(ins, 0, i);
            let level = x[0].abs().max(x[1].abs());
            let g = self.comp.gain(level, self.ceiling - 3.0, 10.0, 0.001, 0.08, ctx.sr);
            *o = [soft_clip(x[0] * g / ceil) * ceil, soft_clip(x[1] * g / ceil) * ceil];
        }
    }

    fn set(&mut self, p: &str, v: f32) -> bool {
        if p == "ceiling" {
            self.ceiling = v.min(0.0);
            true
        } else {
            false
        }
    }
}

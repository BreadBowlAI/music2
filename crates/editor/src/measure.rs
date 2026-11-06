//! Curves measured from the sound itself (CARD-80 to CARD-86): a picture's `p.response()` and
//! `p.hit()`, worked out from an offline copy of its call, never drawn from a pack's own maths.
//!
//! - **The copy** ([`copy`]): the call's own nodes, as the accepted revision built them (its frame
//!   and every frame inside it, ID-15), each control the picture reads at its value now (written, the
//!   default, or a lease's), in a song of its own. What the call reads from outside it is put in
//!   place: its sound in, a step from sample 0 (the first difference of what comes out is the
//!   click's, its impulse response); its notes in, one note from sample 0; a sound or notes it only
//!   listens to (a sidechain), silence; a value a lane drives, the lane's fallback; one its track
//!   works out, that work, copied. The same copy measures both layers; only its notes differ.
//! - **The render**: a renderer of its own, from sample 0 at the song's rate and tempo, off the
//!   audio thread: it never touches what plays (RENDER-33, TAP-2).
//! - **The layers**: [`response`], the click's spectrum in dB at points across the picture's x
//!   axis, each worked out exactly at its frequency (a Goertzel filter); [`hit`], the note's sound
//!   as the lowest and highest sample in each of [`COLUMNS`] columns, closed into one shape.
//! - **What it is made from** ([`Copy::key`]): the copy itself, the code of every kernel it runs,
//!   the recordings they read, the rate, the tempo and the layer's options. The server keeps each
//!   measure by it, so a picture drawn again at the same values measures nothing.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use m2_ir::{Body, EventType, Ir, Kind, LaneDecl, Module, Node, Op, Root, split_ref};
use m2_runtime::{PackageSet, Renderer};
use m2_session::Revision;
use m2_song::lane::{LaneData, LaneKind, Note, NoteClip, VELOCITY};
use m2_song::{LaneAddr, ROOT};
use serde_json::{Map, Value};

use crate::scope::{Inside, port_of};

/// The frames a response plays: 16,384, 0.34 s at 48 kHz.
pub const FRAMES: usize = 1 << 14;
/// The points a response gives across its axis, and the columns a hit's sound is drawn in.
pub const POINTS: usize = 256;
pub const COLUMNS: usize = 256;
/// The longest hit, in seconds.
pub const LONGEST: f64 = 4.0;
/// What a response says of no sound: −120 dB.
const FLOOR: f64 = -120.0;

/// Which layer is measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    /// The click's spectrum.
    Response,
    /// One note of `note`, `seconds` long.
    Hit { note: u8, seconds: f64 },
}

/// An axis as a measure lays its points on: log or not, from `lo` to `hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub log: bool,
    pub lo: f64,
    pub hi: f64,
}

/// A call's offline copy (CARD-81): its graph and the data it plays, at the song's rate.
pub struct Copy {
    ir: Ir,
    data: m2_song::DataSet,
    rate: u32,
    frames: usize,
    /// What it was made from (CARD-84).
    pub key: String,
}

/// The lanes the copy plays its notes from: the hit's note, the click's, and one never played.
const NOTE: &str = "m2-note";
const CLICK: &str = "m2-click";
const NONE: &str = "m2-none";

/// What a reference from outside the call becomes in the copy.
#[derive(Clone, Copy, PartialEq)]
enum Put {
    /// The sound the call takes in: the step.
    Sound,
    /// A sound it only listens to: silence.
    Quiet,
    /// The notes it plays.
    Notes,
    /// Notes it only listens to: none.
    NoNotes,
}

impl Put {
    /// The node of the copy that stands in for it, and how it is made.
    fn node(self) -> (&'static str, Op) {
        let step = |events: &str| Op::Poly {
            events: events.into(),
            voices: 1,
            alloc: m2_ir::Alloc::default(),
            glide: None,
            // A note's key count is 1 on each of its samples, from its first (RENDER-39): a step.
            body: Body {
                nodes: vec![Node { id: "k".into(), op: Op::Lane { lane: "keyCount".into() }, src: None, origin: None, frame: None }],
                out: "k".into(),
            },
        };
        match self {
            Put::Sound => ("m2_in", step("m2_click")),
            Put::Quiet => ("m2_quiet", step("m2_none")),
            Put::Notes => ("m2_note", Op::Lane { lane: NOTE.into() }),
            Put::NoNotes => ("m2_none", Op::Lane { lane: NONE.into() }),
        }
    }
}

/// Every reference node `n` reads, each with the ids of the `poly` body it is read in, if any (a
/// body's own node shadows the module's); one a body inside that body names as its own isn't given.
fn reads(n: &Node, f: &mut dyn FnMut(&str, Option<&HashSet<String>>)) {
    match &n.op {
        Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values().for_each(|r| f(r, None)),
        Op::Feedback { input, .. } => f(input, None),
        Op::Poly { events, glide, body, .. } => {
            f(events, None);
            if let Some(g) = glide {
                f(g, None);
            }
            let own: HashSet<String> = body.nodes.iter().map(|b| b.id.clone()).collect();
            // A body's own node is the body's; a body inside it shadows it in turn.
            for b in &body.nodes {
                reads(b, &mut |r, inner| {
                    if !inner.is_some_and(|i| i.contains(split_ref(r).0)) {
                        f(r, Some(&own))
                    }
                });
            }
        }
        _ => {}
    }
}

/// [`reads`], each reference given back to be written again.
fn rewrite(n: &mut Node, f: &mut dyn FnMut(&mut String, Option<&HashSet<String>>)) {
    match &mut n.op {
        Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values_mut().for_each(|r| f(r, None)),
        Op::Feedback { input, .. } => f(input, None),
        Op::Poly { events, glide, body, .. } => {
            f(events, None);
            if let Some(g) = glide {
                f(g, None);
            }
            let own: HashSet<String> = body.nodes.iter().map(|b| b.id.clone()).collect();
            for b in &mut body.nodes {
                rewrite(b, &mut |r, inner| {
                    if !inner.is_some_and(|i| i.contains(split_ref(r).0)) {
                        f(r, Some(&own))
                    }
                });
            }
        }
        _ => {}
    }
}

/// `r` with its port written out: `n3` is `n3.out`.
fn whole(r: &str) -> String {
    let (n, p) = split_ref(r);
    format!("{n}.{p}")
}

/// The offline copy of call `node` (its id in the view, `file#ordinal`) of module `module` in
/// `rev`, for `layer` (CARD-81): each control `values` names (a picture's `v`, by name) at that
/// value. Refused in words that follow "can't measure its sound: ".
pub fn copy(
    rev: &Revision,
    kernels: &PackageSet,
    module: &str,
    node: &str,
    values: &Map<String, Value>,
    layer: Layer,
) -> Result<Copy, String> {
    let compiled = &rev.compiled;
    let gone = || format!("there's no call {node} in {module} now");
    let (file, ordinal) = node.rsplit_once('#').ok_or_else(gone)?;
    let ordinal: u32 = ordinal.parse().map_err(|_| gone())?;
    let site = compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal).ok_or_else(gone)?;
    let outputs: Vec<_> = compiled.outputs.iter().filter(|o| o.site == site.id && o.module == module).collect();
    let output = match outputs[..] {
        [one] if one.top => one,
        [_] => return Err("it plays inside an instrument's voices".into()),
        [] => return Err("it gives no sound".into()),
        _ => return Err("it is called more than once".into()),
    };
    let m = compiled.ir.modules.iter().find(|x| x.key == module).ok_or_else(gone)?;
    let Some(Inside::Frame(frame)) = Inside::of(m, site.id) else { return Err("it is called more than once".into()) };
    let inside = Inside::Frame(frame);
    let by_id: BTreeMap<&str, &Node> = m.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    // What its sidechain inputs hear, by reference: only listened to (TAP-11's rule).
    let side: HashSet<String> = (m.frames.iter().enumerate())
        .filter(|(i, _)| inside.frame(m, Some(*i as u32)))
        .flat_map(|(_, f)| &f.sidechain)
        .map(|x| whole(&x.source))
        .collect();

    // The call's nodes, and each control its track works out for it, copied; what it reads from
    // outside, sorted into what stands in for it.
    let mut keep: HashSet<&str> = m.nodes.iter().filter(|n| inside.has(m, n)).map(|n| n.id.as_str()).collect();
    let mut todo: Vec<(&Node, bool)> = m.nodes.iter().filter(|n| inside.has(m, n)).map(|n| (n, true)).collect();
    // Each outside reference: its kind, and whether the call itself (not a value's work) reads it.
    let mut outside: Vec<(String, Kind, bool)> = vec![];
    let mut fallbacks: BTreeMap<String, (f64, Kind)> = BTreeMap::new();
    let mut refused: Option<String> = None;
    while let Some((n, own)) = todo.pop() {
        if matches!(n.op, Op::Plugin { .. }) {
            return Err("it hosts a plugin, which music2 can't measure yet".into());
        }
        let mut found = vec![];
        reads(n, &mut |r, body| {
            let id = split_ref(r).0;
            if body.is_some_and(|b| b.contains(id)) || keep.contains(id) {
                return;
            }
            found.push(r.to_string());
        });
        for r in found {
            let id = split_ref(&r).0;
            let Some(&x) = by_id.get(id) else { continue };
            let id = x.id.as_str();
            let kind = match &x.op {
                Op::Lane { lane } => m.lanes.iter().find(|l| &l.key == lane).map(|l| (l.kind, l.default)),
                Op::Const { .. } | Op::Time { .. } => None,
                _ => port_of(rev, kernels, module, &r)
                    .and_then(|(i, p)| compiled.flat.nodes.get(i)?.outputs.get(p).copied())
                    .map(|k| (k, None)),
            };
            match (&x.op, kind) {
                (Op::Const { .. } | Op::Time { .. }, _) => {
                    keep.insert(id);
                }
                (Op::Lane { .. }, Some((k @ Kind::Control(_), default))) => {
                    fallbacks.insert(id.to_string(), (default.unwrap_or(0.0), k));
                    keep.insert(id);
                }
                (_, Some((k @ (Kind::Audio(_) | Kind::Events(_)), _))) => outside.push((whole(&r), k, own)),
                (Op::Import { .. }, Some(_)) => {
                    refused.get_or_insert_with(|| "one of its values comes from another track".to_string());
                }
                (_, Some((Kind::Control(_), _))) => {
                    keep.insert(id);
                    todo.push((x, false));
                }
                (_, None) => return Err(gone()),
            }
        }
    }
    if let Some(why) = refused {
        return Err(why);
    }

    // The sound and the notes it takes in: what the call reads that its sidechains alone don't.
    let mine = |audio: bool| -> HashSet<String> {
        let all: Vec<&(String, Kind, bool)> = outside.iter().filter(|(_, k, own)| *own && matches!(k, Kind::Audio(_)) == audio).collect();
        let only: HashSet<String> = all.iter().filter(|(r, ..)| !side.contains(r)).map(|(r, ..)| r.clone()).collect();
        if only.is_empty() { all.iter().map(|(r, ..)| r.clone()).collect() } else { only }
    };
    let (sound, notes) = (mine(true), mine(false));
    match layer {
        Layer::Response if sound.is_empty() => return Err("it takes no sound in, so p.response has nothing to play through it".into()),
        Layer::Hit { .. } if notes.is_empty() => return Err("it plays no notes, so p.hit has no note to play".into()),
        _ => {}
    }
    let put: BTreeMap<String, Put> = outside
        .iter()
        .map(|(r, k, _)| {
            let p = match k {
                Kind::Audio(_) if sound.contains(r) => Put::Sound,
                Kind::Audio(_) => Put::Quiet,
                _ if notes.contains(r) => Put::Notes,
                _ => Put::NoNotes,
            };
            (r.clone(), p)
        })
        .collect();

    // Only what its output plays: a value its sound doesn't read (a dot's own) changes no copy.
    let mut plays: HashSet<&str> = HashSet::new();
    let mut next = vec![split_ref(&output.port).0];
    while let Some(id) = next.pop() {
        if !keep.contains(id) || !plays.insert(id) {
            continue;
        }
        let Some(&n) = by_id.get(id) else { continue };
        reads(n, &mut |r, body| {
            let id = split_ref(r).0;
            if !body.is_some_and(|b| b.contains(id))
                && let Some(&x) = by_id.get(id)
            {
                next.push(x.id.as_str());
            }
        });
    }

    // The copy's module: the nodes it plays, in their order, its values at the picture's.
    let mut nodes: Vec<Node> = m.nodes.iter().filter(|n| plays.contains(n.id.as_str())).cloned().collect();
    let mut used: Vec<Put> = vec![];
    for n in &mut nodes {
        if let Some((value, kind)) = fallbacks.get(&n.id) {
            let unit = match kind {
                Kind::Control(u) => *u,
                _ => None,
            };
            n.op = Op::Const { value: *value, unit };
        }
        set_values(n, frame, values);
        rewrite(n, &mut |r, body| {
            if body.is_some_and(|b| b.contains(split_ref(r).0)) {
                return;
            }
            if let Some(&p) = put.get(&whole(r)) {
                *r = p.node().0.to_string();
                if !used.contains(&p) {
                    used.push(p);
                }
            }
        });
    }
    // What stands in for each, and the lanes their notes come from.
    let mut extra: Vec<Node> = vec![];
    let mut add = |id: &str, op: Op| {
        if !extra.iter().any(|n| n.id == id) {
            extra.push(Node { id: id.into(), op, src: None, origin: None, frame: None });
        }
    };
    for p in &used {
        let (id, op) = p.node();
        add(id, op);
        match p {
            Put::Sound => add("m2_click", Op::Lane { lane: CLICK.into() }),
            Put::Quiet => add("m2_none", Op::Lane { lane: NONE.into() }),
            _ => {}
        }
    }
    let lanes: Vec<LaneDecl> = extra
        .iter()
        .filter_map(|n| match &n.op {
            Op::Lane { lane } => Some(LaneDecl { key: lane.clone(), kind: Kind::Events(EventType::Note), default: None, label: None }),
            _ => None,
        })
        .collect();
    nodes.extend(extra);
    // Each frame's sidechains as the copy has them: what stands in for each, and none for a node
    // it doesn't play.
    let ids: HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
    let mut frames = m.frames.clone();
    for (i, f) in frames.iter_mut().enumerate() {
        if !inside.frame(m, Some(i as u32)) {
            f.sidechain.clear();
        }
        for s in &mut f.sidechain {
            if let Some(p) = put.get(&whole(&s.source)) {
                s.source = p.node().0.to_string();
            }
        }
        f.sidechain.retain(|s| ids.contains(split_ref(&s.source).0));
    }
    let export = if module == ROOT { "out" } else { "measure" };
    let copy = Module {
        key: m.key.clone(),
        label: m.label.clone(),
        track: m.track,
        muted: false,
        bus: false,
        lanes,
        nodes,
        exports: [(export.to_string(), output.port.clone())].into(),
        src: m.src.clone(),
        frames,
    };
    let mut modules = vec![copy];
    if module != ROOT {
        let import = Node {
            id: "out".into(),
            op: Op::Import { module: module.into(), export: export.into() },
            src: None,
            origin: None,
            frame: None,
        };
        let root = Module {
            key: ROOT.into(),
            label: None,
            track: false,
            muted: false,
            bus: false,
            lanes: vec![],
            nodes: vec![import],
            exports: [("out".to_string(), "out".to_string())].into(),
            src: None,
            frames: vec![],
        };
        modules.insert(0, root);
    }
    let ir = Ir { ir: m2_ir::IR_VERSION, modules, root: Root { module: ROOT.into(), export: "out".into() } };

    // Its notes: the hit's one note, or the click's, each from sample 0.
    let rate = rev.project.sample_rate();
    let tempo = rev.tempo();
    let (frames, beats, note) = match layer {
        Layer::Response => (FRAMES, tempo.beat_at((FRAMES + 1) as f64 / rate as f64 + 1.0), None),
        Layer::Hit { note, seconds } => {
            let frames = (seconds.clamp(0.0, LONGEST) * rate as f64).round() as usize;
            (frames, tempo.beat_at(seconds.clamp(0.0, LONGEST)), Some(note))
        }
    };
    let clip = |pitch: f64| {
        LaneData::Notes(vec![NoteClip {
            id: None,
            other_id: None,
            start: 0.0,
            length: beats,
            notes: vec![Note { pitch, start: 0.0, length: beats, velocity: VELOCITY }],
        }])
    };
    let mut data = compiled.data.clone();
    for (lane, d) in [(NOTE, note.map(|n| clip(n as f64))), (CLICK, (layer == Layer::Response).then(|| clip(60.0)))] {
        let (Some(d), Ok(addr)) = (d, LaneAddr::new(module, lane)) else { continue };
        data = data.seeded(&addr, &LaneKind::Notes, Arc::from(""), Arc::new(d));
    }

    // What it is made from (CARD-84): the copy, its kernels' code, the recordings they read, the
    // rate, the tempo and the layer.
    let mut key = <sha2::Sha256 as sha2::Digest>::new();
    let text = serde_json::to_string(&ir).map_err(|e| e.to_string())?;
    sha2::Digest::update(&mut key, format!("{text}\0{rate}\0{layer:?}\0{:?}\0", tempo.segments()));
    for m in &ir.modules {
        made_of(&m.nodes, &mut key, kernels, &compiled.data);
    }
    let key: String = sha2::Digest::finalize(key).iter().map(|b| format!("{b:02x}")).collect();
    Ok(Copy { ir, data, rate, frames, key })
}

/// Into `key`, each kernel of `nodes` (its voices' too) by the hash of its code as `kernels` loaded
/// it, and each recording it reads by the hash `data` read it at.
fn made_of(nodes: &[Node], key: &mut sha2::Sha256, kernels: &PackageSet, data: &m2_song::DataSet) {
    for n in nodes {
        match &n.op {
            Op::Kernel { kernel, assets, .. } => {
                let code = kernels.module_sha(kernel).map(|s| s.hex()).unwrap_or_default();
                sha2::Digest::update(key, format!("{kernel}\0{code}\0"));
                for a in assets {
                    let sha = data.kernel_assets().get(a).map(|s| s.hex()).unwrap_or_default();
                    sha2::Digest::update(key, format!("{a}\0{sha}\0"));
                }
            }
            Op::Poly { body, .. } => made_of(&body.nodes, key, kernels, data),
            _ => {}
        }
    }
}

/// Each control of frame `frame` node `n` is (its voices' too), at its value in `values`: a number
/// as it is, a choice's name as its place, a switch's true or false as 1 or 0. One `values`
/// leaves out, or gives null, plays as the revision built it.
fn set_values(n: &mut Node, frame: u32, values: &Map<String, Value>) {
    if let Op::Poly { body, .. } = &mut n.op {
        body.nodes.iter_mut().for_each(|b| set_values(b, frame, values));
        return;
    }
    let Some(slot) = n.origin.as_ref().and_then(|o| o.control.as_ref()).filter(|c| c.frame == frame).map(|c| c.slot.clone()) else {
        return;
    };
    let Op::Const { value, unit } = &mut n.op else { return };
    let v = match values.get(&slot) {
        Some(Value::Number(x)) => x.as_f64(),
        Some(Value::Bool(b)) => Some(*b as u8 as f64),
        Some(Value::String(name)) => match unit {
            Some(m2_ir::Unit::Choice(o)) => o.place(name).map(|p| p as f64),
            _ => None,
        },
        _ => None,
    };
    if let Some(v) = v.filter(|v| v.is_finite()) {
        *value = v;
    }
}

impl Copy {
    /// Its sound, both sides, `frames` frames from sample 0, in a renderer of its own, its kernels
    /// the revision's (`packages` seeing them). Refused in words.
    pub fn frames(&self, packages: &PackageSet) -> Result<Vec<[f32; 2]>, String> {
        let flat = m2_ir::check(&self.ir, packages).map_err(|d| {
            let first = d.first().map(|x| x.message.clone()).unwrap_or_default();
            format!("its copy doesn't build ({first})")
        })?;
        let mut r = Renderer::new(&flat, &self.data, packages, self.rate).map_err(|e| format!("its copy doesn't play ({e:#})"))?;
        let mut out = vec![[0.0f32; 2]; self.frames];
        r.process(&mut out).map_err(|e| format!("its copy stopped ({e:#})"))?;
        Ok(out)
    }

    /// Its sound as a layer reads it: each frame the mean of its two sides.
    pub fn render(&self, packages: &PackageSet) -> Result<Vec<f64>, String> {
        Ok(self.frames(packages)?.iter().map(|[l, r]| 0.5 * (*l as f64 + *r as f64)).collect())
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

/// The response (CARD-82): the click's spectrum at [`POINTS`] points across `x` (evenly on its
/// scale), each in dB, `[hz, db, …]`, from `step`, the copy's sound after a step from sample 0.
/// Points at or above half the rate are left out.
pub fn response(step: &[f64], rate: u32, x: Span) -> Vec<f64> {
    // The click's sound: the step's first difference.
    let mut h: Vec<f64> = step.iter().scan(0.0, |was, &y| Some(y - std::mem::replace(was, y))).collect();
    // Past its last sample a millionth of its loudest, it adds nothing a picture shows.
    let peak = h.iter().fold(0.0f64, |a, &x| a.max(x.abs()));
    let last = h.iter().rposition(|&x| x.abs() > peak * 1e-7).map_or(0, |i| i + 1);
    h.truncate(last);
    let nyquist = rate as f64 / 2.0;
    let hz: Vec<f64> = (0..POINTS)
        .map(|i| {
            let t = i as f64 / (POINTS - 1) as f64;
            match x.log {
                true => x.lo * (x.hi / x.lo).powf(t),
                false => x.lo + (x.hi - x.lo) * t,
            }
        })
        .filter(|f| *f > 0.0 && *f < nyquist)
        .collect();
    let mut out = Vec::with_capacity(2 * hz.len());
    for group in hz.chunks(WIDE) {
        let mut w = [0.0; WIDE];
        for (w, f) in w.iter_mut().zip(group) {
            *w = std::f64::consts::TAU * f / rate as f64;
        }
        let power = goertzel(&h, &w);
        for (f, p) in group.iter().zip(power) {
            out.extend([*f, db(p)]);
        }
    }
    out
}

/// The frequencies worked out together in one pass over the click's sound: eight independent sums
/// keep the processor busy where one would wait on itself.
const WIDE: usize = 8;

/// |H(e^{jw})|² of `h` at each of `w`, in radians a sample: a Goertzel filter for each.
fn goertzel(h: &[f64], w: &[f64; WIDE]) -> [f64; WIDE] {
    let c = w.map(|w| 2.0 * w.cos());
    let (mut s1, mut s2) = ([0.0; WIDE], [0.0; WIDE]);
    for &x in h {
        for k in 0..WIDE {
            let s = x + c[k] * s1[k] - s2[k];
            s2[k] = s1[k];
            s1[k] = s;
        }
    }
    // The last step's phase aside, the power at each w (the Goertzel filter's result).
    std::array::from_fn(|k| (s1[k] * s1[k] + s2[k] * s2[k] - c[k] * s1[k] * s2[k]).max(0.0))
}

/// A power in dB, never under [`FLOOR`].
fn db(power: f64) -> f64 {
    let d = 10.0 * power.log10();
    let d = if d.is_finite() { d.max(FLOOR) } else { FLOOR };
    (d * 1000.0).round() / 1000.0
}

/// The hit (CARD-83): the note's `sound` over `seconds`, as one closed shape: the highest sample
/// of each of [`COLUMNS`] columns, left to right, then the lowest, right to left, each at its
/// column's middle in seconds.
pub fn hit(sound: &[f64], seconds: f64) -> Vec<f64> {
    let n = sound.len();
    let mut hi = Vec::with_capacity(COLUMNS);
    let mut lo = Vec::with_capacity(COLUMNS);
    for c in 0..COLUMNS {
        let (a, b) = (c * n / COLUMNS, ((c + 1) * n / COLUMNS).max(c * n / COLUMNS + 1).min(n));
        let part = &sound[a.min(n)..b.max(a.min(n))];
        let at = (c as f64 + 0.5) / COLUMNS as f64 * seconds;
        let top = part.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let bottom = part.iter().copied().fold(f64::INFINITY, f64::min);
        let (top, bottom) = if part.is_empty() { (0.0, 0.0) } else { (top, bottom) };
        hi.extend([at, round(top)]);
        lo.push([at, round(bottom)]);
    }
    hi.extend(lo.iter().rev().flatten());
    hi
}

/// A sample's value, kept to six places: what a picture shows, and short on the socket.
fn round(x: f64) -> f64 {
    if x.is_finite() { (x * 1e6).round() / 1e6 } else { 0.0 }
}

#[cfg(test)]
#[path = "measure_tests.rs"]
mod tests;

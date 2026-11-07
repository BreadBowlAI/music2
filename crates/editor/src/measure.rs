//! Curves measured from the sound itself (CARD-80 to CARD-85): a picture's `p.response()` and
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
use std::time::Duration;

use m2_ir::{Body, EventType, Ir, Kind, LaneDecl, Module, Node, Op, Root, split_ref};
use m2_runtime::{BLOCK, PackageSet, Renderer};
use m2_session::Revision;
use m2_song::lane::{LaneData, LaneKind, Note, NoteClip, VELOCITY};
use m2_song::{LaneAddr, ROOT};
use serde_json::{Map, Value};

use crate::scope::{Inside, port_of};

/// The step's level, in dBFS, when a draw gives none: −20 (0.1), quiet enough to keep most sounds in
/// their straight-line range. A draw may give a quieter one, down to −80, so a sound with
/// thresholds rests under them (CARD-80). Each is checked against one 32 times quieter (CARD-82).
pub const LEVEL: f64 = m2_script::pictures::CLICK.1;
/// How much quieter the step it is checked against is.
const SOFTER: f64 = 32.0;
/// How long the click must stay quiet, in seconds, for the copy to stop listening; and the longest
/// it may ring before it falls quiet.
pub const QUIET: f64 = 0.5;
pub const LISTEN: f64 = 2.0;
/// Quiet: a ten-thousandth of the click's loudest and a little more (−90 dB), or a millionth of the
/// step (the floats' own floor).
const QUIETER: f64 = 3.162_277_660_168_379_4e-5;
const FLOATS: f64 = 1e-6;
/// The stretch of the click compared at the two levels and the two start times, and how much later
/// the second start is, in seconds; and how far apart, in dB, they may be at a point.
const CHECK: f64 = 0.05;
const LATER: f64 = 0.1;
const APART: f64 = 0.5;
/// The points a response gives across its axis, and the columns a hit's sound is drawn in.
pub const POINTS: usize = 256;
pub const COLUMNS: usize = 256;
/// The longest hit, in seconds (CARD-80).
pub const LONGEST: f64 = 1.0;
/// What a whole measure may take at 48 kHz (CARD-85), and each call into a pack's kernel in it.
pub const BUDGET: Duration = Duration::from_millis(20);

/// A measure's budget at `rate`: [`BUDGET`], more as the rate is higher than 48 kHz, whose
/// renders have that many more frames.
pub fn budget(rate: u32) -> Duration {
    BUDGET.mul_f64((rate as f64 / 48_000.0).max(1.0))
}
/// The frames played between looks at the clock.
const CHUNK: usize = 1024;
/// A click longer than this is worked out by an FFT; a shorter one point by point.
const SHORT: usize = 1 << 14;
/// What a response says of no sound: −120 dB.
const FLOOR: f64 = -120.0;

/// Which measure a draw asks for (CARD-85): a drag's frame, the click alone, or at rest, the click
/// and the checks that it stays the same at every level and every time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Drag,
    Rest,
}

/// Why a measure gave no points (CARD-81, CARD-85).
#[derive(Clone, Debug, PartialEq)]
pub enum Failed {
    /// It can't be measured, in words that follow "can't measure its sound: ": the same each time
    /// for the same copy.
    Cant(String),
    /// It took longer than its budget, or a call into a kernel did: it may be a busy computer.
    Slow,
}

/// Which layer is measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    /// The click's spectrum, its step at `level` dBFS.
    Response { level: f64 },
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
    /// What it plays: the hit's note on its lane; a response's click is put in as it is played.
    data: m2_song::DataSet,
    rate: u32,
    layer: Layer,
    module: String,
    tempo: m2_song::TempoMap,
    /// What it was made from (CARD-84).
    pub key: String,
}

/// The lanes the copy plays its notes from: the hit's note, the click's, and one never played.
const NOTE: &str = "m2-note";
const CLICK: &str = "m2-click";
const NONE: &str = "m2-none";
/// The node that plays the step, and the one inside it that is its level.
const STEP: &str = "m2_in";
const LEVEL_NODE: &str = "l";

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
        let node = |id: &str, op: Op| Node { id: id.into(), op, src: None, origin: None, frame: None };
        let step = |events: &str, level: bool| Op::Poly {
            events: events.into(),
            voices: 1,
            alloc: m2_ir::Alloc::default(),
            glide: None,
            // A note's key count is 1 on each of its samples, from its first (RENDER-39): a step, at
            // the level the measure plays it (std's scale, whose `by` the measure sets).
            body: match level {
                true => Body {
                    nodes: vec![
                        node("k", Op::Lane { lane: "keyCount".into() }),
                        node(LEVEL_NODE, Op::Const { value: amplitude(LEVEL), unit: None }),
                        node(
                            "g",
                            Op::Kernel {
                                kernel: "std:scale".into(),
                                inputs: [("in".into(), "k".into()), ("by".into(), LEVEL_NODE.into())].into(),
                                assets: vec![],
                                values: vec![],
                            },
                        ),
                    ],
                    out: "g".into(),
                },
                false => Body { nodes: vec![node("k", Op::Lane { lane: "keyCount".into() })], out: "k".into() },
            },
        };
        match self {
            Put::Sound => (STEP, step("m2_click", true)),
            Put::Quiet => ("m2_quiet", step("m2_none", false)),
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
    // What it gives must be sound: a control or notes has no curve (review M5).
    let gives = port_of(rev, kernels, module, &output.port).and_then(|(i, p)| compiled.flat.nodes.get(i)?.outputs.get(p).copied());
    if gives.is_some_and(|k| !matches!(k, Kind::Audio(_))) {
        return Err("it gives no sound".into());
    }
    let m = compiled.ir.modules.iter().find(|x| x.key == module).ok_or_else(gone)?;
    let frame = match Inside::of(m, site.id) {
        Some(Inside::Frame(f)) => f,
        Some(Inside::Site(_)) => return Err("it is no sound's own call (a component's)".into()),
        None => return Err("it is called more than once".into()),
    };
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
    // Whether a value of it moves as the song plays: a lane drives it, or its track works it out from
    // the song's beat, its tempo (which changes along the song) or from a sound (review M2). A
    // response has no one curve then.
    let mut moves = false;
    while let Some((n, own)) = todo.pop() {
        if matches!(n.op, Op::Plugin { .. }) {
            return Err("it hosts a plugin, which music2 can't measure yet".into());
        }
        if matches!(n.op, Op::Time { of: m2_ir::TimeOf::Beats | m2_ir::TimeOf::Seconds | m2_ir::TimeOf::Tempo }) {
            moves = true;
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
                    moves |= matches!(x.op, Op::Time { of: m2_ir::TimeOf::Beats | m2_ir::TimeOf::Seconds | m2_ir::TimeOf::Tempo });
                    keep.insert(id);
                }
                (Op::Lane { .. }, Some((k @ Kind::Control(_), default))) => {
                    moves = true;
                    fallbacks.insert(id.to_string(), (default.unwrap_or(0.0), k));
                    keep.insert(id);
                }
                (_, Some((k @ (Kind::Audio(_) | Kind::Events(_)), _))) => {
                    moves |= !own;
                    outside.push((whole(&r), k, own))
                }
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
    if moves && matches!(layer, Layer::Response { .. }) {
        return Err(
            "one of its values moves as the song plays (a lane, a cable or the song's beat drives it), so it has no one curve".into()
        );
    }

    // The sound and the notes it takes in: what the call reads that its sidechains alone don't.
    let mine = |audio: bool| -> HashSet<String> {
        let all: Vec<&(String, Kind, bool)> = outside.iter().filter(|(_, k, own)| *own && matches!(k, Kind::Audio(_)) == audio).collect();
        let only: HashSet<String> = all.iter().filter(|(r, ..)| !side.contains(r)).map(|(r, ..)| r.clone()).collect();
        if only.is_empty() { all.iter().map(|(r, ..)| r.clone()).collect() } else { only }
    };
    let (sound, notes) = (mine(true), mine(false));
    match layer {
        Layer::Response { .. } if sound.is_empty() => {
            return Err("it takes no sound in, so p.response has nothing to play through it".into());
        }
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

    // A hit's note, from sample 0; a response's click is put in as it is played.
    let rate = rev.project.sample_rate();
    let tempo = rev.tempo();
    let mut data = compiled.data.clone();
    if let Layer::Hit { note, seconds } = layer
        && let Ok(addr) = LaneAddr::new(module, NOTE)
    {
        data = data.seeded(
            &addr,
            &LaneKind::Notes,
            Arc::from(""),
            Arc::new(one_note(note as f64, 0.0, tempo.beat_at(seconds.clamp(0.0, LONGEST)))),
        );
    }

    // What it is made from (CARD-84): what the copy plays, its kernels' code, the recordings they
    // read, the rate, the tempo and the layer. Where it is written isn't: a line added above it
    // moves its nodes' places in the code, not its sound (review M7).
    let mut plain = ir.clone();
    for m in &mut plain.modules {
        (m.label, m.src) = (None, None);
        unwritten(&mut m.nodes);
        for f in &mut m.frames {
            (f.site, f.occurrence, f.options) = (0, 0, None);
        }
    }
    let mut key = <sha2::Sha256 as sha2::Digest>::new();
    let text = serde_json::to_string(&plain).map_err(|e| e.to_string())?;
    sha2::Digest::update(&mut key, format!("{text}\0{rate}\0{layer:?}\0{:?}\0", tempo.segments()));
    for m in &ir.modules {
        made_of(&m.nodes, &mut key, kernels, &compiled.data);
    }
    let key: String = sha2::Digest::finalize(key).iter().map(|b| format!("{b:02x}")).collect();
    Ok(Copy { ir, data, rate, layer, module: module.to_string(), tempo, key })
}

/// One note of `pitch` from beat `from` to `to`, at the default velocity: a lane's notes.
fn one_note(pitch: f64, from: f64, to: f64) -> LaneData {
    LaneData::Notes(vec![NoteClip {
        id: None,
        other_id: None,
        start: 0.0,
        length: to,
        notes: vec![Note { pitch, start: from, length: to - from, velocity: VELOCITY }],
    }])
}

/// `nodes` (their voices' too) with no word of where they are written: their source places and
/// origins, which only name the code, never change the sound.
fn unwritten(nodes: &mut [Node]) {
    for n in nodes {
        (n.src, n.origin) = (None, None);
        if let Op::Poly { body, .. } = &mut n.op {
            unwritten(&mut body.nodes);
        }
    }
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

/// What a measure has spent of its budget: the CPU time of the thread it runs on, so a busy computer
/// that gives it the CPU late never stops it (CARD-85).
struct Spent {
    from: Duration,
    most: Duration,
}

impl Spent {
    fn new(most: Duration) -> Spent {
        Spent { from: m2_os::thread::cpu_time(), most }
    }

    fn over(&self) -> bool {
        m2_os::thread::cpu_time().saturating_sub(self.from) > self.most
    }
}

/// What hears each chunk a copy plays, and says whether it has heard enough.
type Heard<'a> = dyn FnMut(&[[f32; 2]]) -> Result<bool, Failed> + 'a;

impl Copy {
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Frames in `seconds` at its rate.
    fn frames_in(&self, seconds: f64) -> usize {
        (seconds * self.rate as f64).round() as usize
    }

    /// How loud its step is: its response's level (CARD-80), as a sample's size.
    fn loudness(&self) -> f64 {
        match self.layer {
            Layer::Response { level } => amplitude(level),
            Layer::Hit { .. } => amplitude(LEVEL),
        }
    }

    /// Its graph with the step at `level`, and its data with the click's note from frame `start`
    /// (a response's), held past anything it plays.
    fn click(&self, level: f64, start: usize) -> (Ir, m2_song::DataSet) {
        let mut ir = self.ir.clone();
        for m in &mut ir.modules {
            for n in m.nodes.iter_mut().filter(|n| n.id == STEP) {
                if let Op::Poly { body, .. } = &mut n.op {
                    for b in body.nodes.iter_mut().filter(|b| b.id == LEVEL_NODE) {
                        b.op = Op::Const { value: level, unit: None };
                    }
                }
            }
        }
        let from = self.tempo.beat_at(start as f64 / self.rate as f64);
        let to = self.tempo.beat_at(start as f64 / self.rate as f64 + LISTEN + QUIET + 1.0);
        let data = match LaneAddr::new(&self.module, CLICK) {
            Ok(addr) => self.data.seeded(&addr, &LaneKind::Notes, Arc::from(""), Arc::new(one_note(60.0, from, to))),
            Err(_) => self.data.clone(),
        };
        (ir, data)
    }

    /// Play `ir` over `data` from sample 0 in a renderer of its own, its kernels the revision's
    /// (`packages` seeing them), [`CHUNK`] frames at a time, each chunk's frames given to `heard`
    /// until it says it has heard enough, or `most` frames. Stopped past `until` (CARD-85).
    fn play(
        &self,
        packages: &PackageSet,
        (ir, data): (Ir, m2_song::DataSet),
        most: usize,
        until: &Spent,
        heard: &mut Heard,
    ) -> Result<(), Failed> {
        let flat = m2_ir::check(&ir, packages).map_err(|d| {
            let first = d.first().map(|x| x.message.clone()).unwrap_or_default();
            Failed::Cant(format!("its copy doesn't build ({first})"))
        })?;
        let ran = |e: anyhow::Error| match format!("{e:#}").contains("took too long") {
            true => Failed::Slow,
            false => Failed::Cant("it stopped as it played: one of its kernels failed".into()),
        };
        let mut r = Renderer::new(&flat, &data, packages, self.rate).map_err(ran)?;
        let mut out = vec![[0.0f32; 2]; CHUNK];
        let mut done = 0;
        while done < most {
            if until.over() {
                return Err(Failed::Slow);
            }
            let n = CHUNK.min(most - done);
            r.process(&mut out[..n]).map_err(ran)?;
            done += n;
            if heard(&out[..n])? {
                break;
            }
        }
        if until.over() {
            return Err(Failed::Slow);
        }
        Ok(())
    }

    /// A hit's sound, both sides: its note's `seconds` (CARD-83), stopped past `budget`.
    pub fn frames(&self, packages: &PackageSet, budget: Duration) -> Result<Vec<[f32; 2]>, Failed> {
        let Layer::Hit { seconds, .. } = self.layer else { return Err(Failed::Cant("it is a response's copy, not a hit's".into())) };
        let frames = self.frames_in(seconds.clamp(0.0, LONGEST));
        let mut got = Vec::with_capacity(frames);
        self.play(packages, (self.ir.clone(), self.data.clone()), frames, &Spent::new(budget), &mut |f| {
            got.extend_from_slice(f);
            Ok(false)
        })?;
        Ok(got)
    }

    /// The click's sound, `frames` of it, as a step at its level from sample 0 gives it: each
    /// sample the step's first difference, the mean of its two sides, over the level.
    pub fn step(&self, packages: &PackageSet, frames: usize) -> Result<Vec<f64>, Failed> {
        self.clicked(packages, self.loudness(), 0, frames, &Spent::new(Duration::from_secs(600)), &mut |_| Ok(false))
    }

    /// The click's sound from a step at `level` from frame `start`, from that frame on, `frames` at
    /// most, until `enough` says so of what it has.
    fn clicked(
        &self,
        packages: &PackageSet,
        level: f64,
        start: usize,
        frames: usize,
        until: &Spent,
        enough: &mut dyn FnMut(&[f64]) -> Result<bool, Failed>,
    ) -> Result<Vec<f64>, Failed> {
        let (mut h, mut was, mut at) = (Vec::with_capacity(frames.min(1 << 20)), 0.0, 0usize);
        self.play(packages, self.click(level, start), start + frames, until, &mut |f| {
            for [l, r] in f {
                let y = 0.5 * (*l as f64 + *r as f64);
                if at >= start {
                    h.push((y - was) / level);
                }
                (was, at) = (y, at + 1);
            }
            if h.is_empty() { Ok(false) } else { enough(&h) }
        })?;
        Ok(h)
    }

    /// Its layer's points (CARD-82, CARD-83): a response across `x`, or a hit's columns. Refused in
    /// words, or stopped past `budget` (CARD-85). A drag's frame (`pass`) listens to the click alone;
    /// at rest, the response is checked at a quieter level and a later start too.
    pub fn measure(&self, packages: &PackageSet, x: Span, budget: Duration, pass: Pass) -> Result<Vec<f64>, Failed> {
        let until = &Spent::new(budget);
        if let Layer::Hit { seconds, .. } = self.layer {
            let sound: Vec<f64> = self.frames(packages, budget)?.iter().map(|[l, r]| 0.5 * (*l as f64 + *r as f64)).collect();
            return Ok(hit(&sound, seconds.clamp(0.0, LONGEST)));
        }
        // Listen until the click has stayed quiet for QUIET, as long as it rings up to LISTEN.
        let (quiet, cap) = (self.frames_in(QUIET), self.frames_in(LISTEN));
        let level = self.loudness();
        let mut end = 0;
        let mut h = self.clicked(packages, level, 0, cap + quiet, until, &mut |h| {
            let peak = h.iter().fold(0.0f64, |a, x| a.max(x.abs()));
            let floor = (peak * QUIETER).max(FLOATS);
            end = h.iter().rposition(|x| x.abs() > floor).map_or(0, |i| i + 1);
            if end > cap {
                return Err(Failed::Cant(format!(
                    "it rings longer than music2 listens ({} s), so music2 can't measure its curve: its pack must draw it",
                    LISTEN
                )));
            }
            Ok(h.len() - end >= quiet)
        })?;
        if h.len() - end < quiet {
            return Err(Failed::Cant(format!(
                "it rings longer than music2 listens ({LISTEN} s), so music2 can't measure its curve: its pack must draw it"
            )));
        }
        h.truncate(end.max(1));
        if pass == Pass::Drag {
            let points = response(&h, self.rate, x);
            return if until.over() { Err(Failed::Slow) } else { Ok(points) };
        }
        // The same at a 32nd of the level, and from a later start: a sound that stays the same at
        // every level and every time gives the same click (review M2).
        let check = self.frames_in(CHECK);
        let later = self.frames_in(LATER).div_ceil(BLOCK) * BLOCK;
        let soft = self.clicked(packages, level / SOFTER, 0, check, until, &mut |_| Ok(false))?;
        let moved = self.clicked(packages, level, later, check, until, &mut |_| Ok(false))?;
        let mut own = h.clone();
        own.resize(check, 0.0);
        let at = |v: &[f64]| response(v, self.rate, x);
        let (own, soft, moved) = (at(&own), at(&soft), at(&moved));
        let apart =
            |other: &[f64]| own.chunks(2).zip(other.chunks(2)).filter(|(a, _)| a[1] > -60.0).any(|(a, b)| (a[1] - b[1]).abs() > APART);
        if apart(&soft) {
            return Err(Failed::Cant(
                "its sound changes with how loud it is, so music2 can't measure its curve: its pack must draw it".into(),
            ));
        }
        if apart(&moved) {
            return Err(Failed::Cant("its sound changes over time, so music2 can't measure its curve: its pack must draw it".into()));
        }
        let points = response(&h, self.rate, x);
        if until.over() {
            return Err(Failed::Slow);
        }
        Ok(points)
    }
}

/// The response (CARD-82): the click's sound `h`'s spectrum at [`POINTS`] points across `x` (evenly
/// on its scale), each in dB, `[hz, db, …]`. Points at or above half the rate are left out. A
/// click up to [`SHORT`] frames is worked out at each point's own frequency (a Goertzel filter); a
/// longer one by an FFT of twice its length or more, between its two nearest bins.
pub fn response(h: &[f64], rate: u32, x: Span) -> Vec<f64> {
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
    if h.len() > SHORT {
        let n = (2 * h.len()).next_power_of_two();
        let bins = spectrum(h, n);
        for f in hz {
            let b = f * n as f64 / rate as f64;
            let (k, t) = (b.floor() as usize, b - b.floor());
            let (a, c) = (bins[k.min(n / 2)], bins[(k + 1).min(n / 2)]);
            let (re, im) = (a.0 + (c.0 - a.0) * t, a.1 + (c.1 - a.1) * t);
            out.extend([f, db(re * re + im * im)]);
        }
        return out;
    }
    for group in hz.chunks(WIDE) {
        let mut w = [0.0; WIDE];
        for (w, f) in w.iter_mut().zip(group) {
            *w = std::f64::consts::TAU * f / rate as f64;
        }
        let power = goertzel(h, &w);
        for (f, p) in group.iter().zip(power) {
            out.extend([*f, db(p)]);
        }
    }
    out
}

/// The spectrum of real `h`, silence after it to `n` frames (a power of two), bins 0 to `n / 2`:
/// an FFT of half the length, of pairs of frames, its halves taken apart after.
fn spectrum(h: &[f64], n: usize) -> Vec<(f64, f64)> {
    let m = n / 2;
    let (mut re, mut im) = (vec![0.0; m], vec![0.0; m]);
    for (i, &v) in h.iter().enumerate() {
        match i % 2 {
            0 => re[i / 2] = v,
            _ => im[i / 2] = v,
        }
    }
    fft(&mut re, &mut im);
    (0..=m)
        .map(|k| {
            let (a, j) = (k % m, (m - k % m) % m);
            let (zr, zi, cr, ci) = (re[a], im[a], re[j], -im[j]);
            let (er, ei) = ((zr + cr) / 2.0, (zi + ci) / 2.0);
            let (or, oi) = ((zi - ci) / 2.0, (cr - zr) / 2.0);
            let w = -std::f64::consts::TAU * k as f64 / n as f64;
            let (c, s) = (w.cos(), w.sin());
            (er + or * c - oi * s, ei + or * s + oi * c)
        })
        .collect()
}

/// An FFT in place, of a power of two, in f64.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        let turns: Vec<(f64, f64)> =
            (0..half).map(|k| (-std::f64::consts::TAU * k as f64 / len as f64).sin_cos()).map(|(s, c)| (c, s)).collect();
        for start in (0..n).step_by(len) {
            for (k, &(wr, wi)) in turns.iter().enumerate() {
                let (x, y) = (start + k, start + k + half);
                let (tr, ti) = (re[y] * wr - im[y] * wi, re[y] * wi + im[y] * wr);
                (re[y], im[y]) = (re[x] - tr, im[x] - ti);
                re[x] += tr;
                im[x] += ti;
            }
        }
        len <<= 1;
    }
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

/// A level in dBFS as a sample's size: −20 is 0.1.
fn amplitude(db: f64) -> f64 {
    10f64.powf(db / 20.0)
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

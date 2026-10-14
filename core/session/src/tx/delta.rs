//! `GraphDelta` (WRITE-8): what changed from one revision's graph to another's, computed by core
//! itself, with nodes named so that inserting one can't rename another.
//!
//! A node made at a site is named by its module key, its frame chain (each frame its ComponentId
//! and its call, nothing positional above a `key()` frame), what it is and the call that made it,
//! and its occurrence among nodes otherwise named alike. A call is the point of the `(` that
//! opens its arguments (ID-13), in each file's reference coordinates (`text::Coords`): an offset
//! in the reference text, or a byte of what a change made. So nothing an insertion elsewhere does
//! can shift a name, a wrap keeps what it wraps, and two processes checking one Tx name every
//! node alike (the step 6 design's 5.2, C17). Source positions, labels, node ids, site numbers and
//! whether a control was written or inherited are no part of a node, so they never differ
//! (WRITE-6).
//!
//! The delta drives the engine and judges expectations; its hash is SHA-256 of its JSON in JCS
//! form, every list sorted by name.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use m2_ir::{Module, Node, Op, split_ref};
use m2_script::sites::{Arg, Site};
use m2_song::tx::ModuleChange;
use m2_song::{DataPath, Key, LaneAddr, Sha, Snapshot, Stored};
use serde_json::{Value, json};

use crate::Compiled;
use crate::text::{Coords, Point};

/// A node's name: its module key, then segments.
pub type Name = Vec<String>;

/// An input's source: a node's output.
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    pub node: Name,
    pub port: String,
}

/// A node added or removed: what it is, and what it reads.
#[derive(Clone, Debug, PartialEq)]
pub struct DNode {
    pub name: Name,
    /// The op without its inputs: its kernel or plugin id, a constant's value and unit, a lane's
    /// key and declaration.
    pub op: Value,
    pub inputs: BTreeMap<String, Ref>,
}

/// An input of a node both graphs have that reads something else now.
#[derive(Clone, Debug, PartialEq)]
pub struct Rewire {
    pub node: Name,
    pub input: String,
    pub from: Option<Ref>,
    pub to: Option<Ref>,
}

/// A constant both graphs have whose value changed.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueChange {
    pub node: Name,
    pub from: f64,
    pub to: f64,
}

/// What changed from one revision to another (WRITE-8).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphDelta {
    pub added: Vec<DNode>,
    pub removed: Vec<DNode>,
    pub rewired: Vec<Rewire>,
    pub values: Vec<ValueChange>,
    /// Lanes declared in both whose data plays differently.
    pub lanes: Vec<LaneAddr>,
    /// Plugin states loaded differently, as `module/frames/key`.
    pub states: Vec<String>,
    /// Assets a clip plays whose hash changed.
    pub assets: Vec<String>,
    pub arrangement: bool,
    pub modules: Vec<ModuleChange>,
}

impl GraphDelta {
    /// Whether the graph's structure changed: nodes, wiring or modules.
    pub fn structural(&self) -> bool {
        !(self.added.is_empty() && self.removed.is_empty() && self.rewired.is_empty() && self.modules.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        !self.structural()
            && self.values.is_empty()
            && self.lanes.is_empty()
            && self.states.is_empty()
            && self.assets.is_empty()
            && !self.arrangement
    }

    pub fn to_json(&self) -> Value {
        let r = |r: &Option<Ref>| r.as_ref().map_or(Value::Null, ref_json);
        let node = |n: &DNode| {
            let inputs: serde_json::Map<String, Value> = n.inputs.iter().map(|(k, v)| (k.clone(), ref_json(v))).collect();
            json!({"name": n.name, "op": n.op, "inputs": inputs})
        };
        let rewire = |w: &Rewire| json!({"node": w.node, "input": w.input, "from": r(&w.from), "to": r(&w.to)});
        json!({
            "added": self.added.iter().map(node).collect::<Vec<_>>(),
            "removed": self.removed.iter().map(node).collect::<Vec<_>>(),
            "rewired": self.rewired.iter().map(rewire).collect::<Vec<_>>(),
            "values": self.values.iter().map(|v| json!({"node": v.node, "from": v.from, "to": v.to})).collect::<Vec<_>>(),
            "lanes": self.lanes.iter().map(|l| json!([l.module.as_str(), l.lane.as_str()])).collect::<Vec<_>>(),
            "states": self.states,
            "assets": self.assets,
            "arrangement": self.arrangement,
            "modules": self.modules.iter().map(m2_song::tx::module_json).collect::<Vec<_>>(),
        })
    }

    /// SHA-256 of the delta's JSON in JCS form (WRITE-8).
    pub fn hash(&self) -> Sha {
        m2_song::jcs::hash(&self.to_json())
    }
}

fn ref_json(r: &Ref) -> Value {
    json!({"node": r.node, "port": r.port})
}

/// Names by the UTF-16 code units of their segments, then by how many there are.
pub fn by_name(a: &Name, b: &Name) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match x.encode_utf16().cmp(y.encode_utf16()) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    a.len().cmp(&b.len())
}

/// Where a position of a file is named: a file's coordinates (the files a change touched), and
/// which side of the change a position is on.
pub struct Positions<'a> {
    pub coords: &'a BTreeMap<String, Coords>,
    /// The new side: positions of the candidate (or the newer revision).
    pub new: bool,
}

impl Positions<'_> {
    pub fn point(&self, file: &str, at: usize) -> Point {
        match self.coords.get(file) {
            None => Point::Base(at),
            Some(c) if self.new => c.candidate(at),
            Some(c) => c.current(at),
        }
    }

    fn spell(&self, file: &str, p: &Point) -> String {
        match self.coords.get(file) {
            Some(c) => c.spell(p),
            None => Coords::same().spell(p),
        }
    }
}

/// Where something was made: a file, and the points of its first and last characters.
#[derive(Clone, Debug, PartialEq)]
pub struct At {
    pub file: String,
    pub start: Point,
    pub end: Point,
}

/// What a node is, for comparing and for judging expectations.
#[derive(Clone, Debug)]
pub struct Facts {
    pub sig: Value,
    pub inputs: BTreeMap<String, Ref>,
    /// A constant's value.
    pub value: Option<f64>,
    /// Its origin (the argument's range, else its call's) and each call of its frame chain.
    pub spans: Vec<At>,
    /// A constant's number, as written at its origin's argument.
    pub written: Option<At>,
    /// A declared control's constant: its instance's call and its slot (PKG-3).
    pub slot: Option<(At, String)>,
    /// How many times each positional segment's call ran in its parent, then how many nodes share
    /// the name but for its occurrence: what is kept by index only while it is the same (ID-14).
    pub counts: Vec<u32>,
}

/// Where a node was made, as [`Facts`] keeps it.
struct Places {
    spans: Vec<At>,
    written: Option<At>,
    slot: Option<(At, String)>,
    counts: Vec<u32>,
}

/// A module as its names read it: the calls by site, and how many times each call ran in each
/// parent frame (ID-14).
struct Within<'a> {
    m: &'a Module,
    sites: &'a HashMap<u32, &'a Site>,
    runs: HashMap<(Option<u32>, u32), u32>,
}

/// A module's nodes with its key taken off every name, to tell a module re-keyed.
type Bare = Vec<(Name, Value, Vec<(String, Name, String)>)>;

/// One side's graph, by name.
#[derive(Default)]
pub struct Graph {
    pub nodes: BTreeMap<Name, Facts>,
    pub exports: BTreeMap<(String, String), Ref>,
    pub root: (String, String),
    pub modules: BTreeSet<String>,
}

/// A revision as a delta reads it.
pub struct Side<'a> {
    pub compiled: &'a Compiled,
    pub snapshot: &'a Snapshot,
    pub at: Positions<'a>,
}

fn op_name(op: &Op) -> &'static str {
    op.name()
}

/// `file:line:col` split up.
fn parse_src(src: &str) -> Option<(&str, usize, usize)> {
    let mut it = src.rsplitn(3, ':');
    let col = it.next()?.parse().ok()?;
    let line = it.next()?.parse().ok()?;
    Some((it.next()?, line, col))
}

/// `file:line:col` (1-based, the column in characters) as a byte offset of `text`.
fn offset(text: &str, line: usize, col: usize) -> Option<usize> {
    let start = if line <= 1 { 0 } else { text.match_indices('\n').nth(line - 2)?.0 + 1 };
    let rest = &text[start..];
    let col = rest.char_indices().nth(col.saturating_sub(1)).map_or(rest.len(), |(i, _)| i);
    Some(start + col)
}

impl Side<'_> {
    fn sites(&self) -> HashMap<u32, &Site> {
        self.compiled.sites.iter().map(|s| (s.id, s)).collect()
    }

    /// `[start, end)` of `file` as a place.
    fn place(&self, file: &str, start: u32, end: u32) -> At {
        let (s, e) = (start as usize, (end.max(start + 1) - 1) as usize);
        At { file: file.to_string(), start: self.at.point(file, s), end: self.at.point(file, e) }
    }

    /// `@file:point`: a call named by the point of its `(` (ID-13).
    fn called(&self, s: &Site) -> String {
        format!("@{}:{}", s.file, self.at.spell(&s.file, &self.at.point(&s.file, s.open as usize)))
    }

    /// The chain segments of `frame`, each frame's call as a place, and how many times each
    /// positional segment's call ran in its parent (ID-14). A `key()` frame is `key:<k>`, a
    /// component frame its ComponentId, its call and its occurrence (ID-15). Keys are unique per
    /// parent key frame, so nothing positional above the innermost one names anything (ID-7).
    fn chain(&self, w: &Within, frame: Option<u32>) -> (Vec<String>, Vec<At>, Vec<u32>) {
        let frames = w.m.chain(frame);
        let pinned = frames.iter().rposition(|f| f.key.is_some());
        let (mut segs, mut places, mut counts) = (vec![], vec![], vec![]);
        for (i, f) in frames.iter().enumerate() {
            let site = w.sites.get(&f.site);
            places.extend(site.map(|s| self.place(&s.file, s.start, s.end)));
            match &f.key {
                Some(k) => segs.push(format!("key:{k}")),
                None if pinned.is_some_and(|p| i < p) => {}
                None => {
                    let call = site.map_or("@?".to_string(), |s| self.called(s));
                    segs.push(format!("{}{call}#{}", f.component.as_deref().unwrap_or(""), f.occurrence));
                    counts.push(w.runs.get(&(f.parent, f.site)).copied().unwrap_or(1));
                }
            }
        }
        (segs, places, counts)
    }

    /// A node's name segments after its module key (before its occurrence), and what it is.
    fn segments(&self, w: &Within, n: &Node) -> (Vec<String>, Places) {
        let (m, sites) = (w.m, w.sites);
        let site = n.origin.as_ref().and_then(|o| sites.get(&o.site).copied());
        let call = site.map(|s| self.place(&s.file, s.start, s.end));
        let called = site.map(|s| self.called(s)).unwrap_or_default();
        let mut spans = vec![];
        let mut written = None;
        let mut slot = None;
        let mut counts = vec![];
        if let (Some(o), Some(s)) = (&n.origin, site) {
            match s.arg(&o.arg).filter(|_| !o.arg.is_empty()) {
                Some(a) => {
                    let (x, y) = a.span();
                    let place = self.place(&s.file, x, y);
                    if matches!(a, Arg::Number { .. }) {
                        written = Some(place.clone());
                    }
                    spans.push(place);
                }
                None => spans.extend(call.clone()),
            }
        }
        let segs = match (&n.op, &n.origin) {
            (Op::Const { .. }, Some(o)) if o.control.is_some() => {
                let c = o.control.as_ref().expect("checked");
                let (mut segs, places, runs) = self.chain(w, Some(c.frame));
                if let Some(instance) = m.frames.get(c.frame as usize).and_then(|f| sites.get(&f.site)) {
                    slot = Some((self.place(&instance.file, instance.start, instance.end), c.slot.clone()));
                }
                spans.extend(places);
                counts = runs;
                segs.push(format!("@{}", c.slot));
                segs
            }
            (Op::Lane { lane }, _) => vec![format!("lane:{lane}{called}")],
            (Op::Time { of }, _) => {
                vec![format!("time:{}", serde_json::to_value(of).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default())]
            }
            (Op::Import { module, export }, _) => vec![format!("import:{module}.{export}")],
            (op, Some(o)) => {
                let (mut segs, places, runs) = self.chain(w, n.frame);
                spans.extend(places);
                counts = runs;
                segs.push(format!("{}{called}", op_name(op)));
                if matches!(op, Op::Const { .. }) && !o.arg.is_empty() {
                    segs.push(format!("[{}]", o.arg.join(".")));
                }
                segs
            }
            (op, None) => {
                let (mut segs, places, runs) = self.chain(w, n.frame);
                spans.extend(places);
                counts = runs;
                let id = match op {
                    Op::Kernel { kernel, .. } => kernel.as_str(),
                    Op::Plugin { plugin, .. } => plugin.as_str(),
                    _ => "",
                };
                segs.push(format!("{}~{id}", op_name(op)));
                segs
            }
        };
        // A node made with no origin and no frame: where its source says it is.
        if spans.is_empty()
            && let Some((file, line, col)) = n.src.as_deref().and_then(parse_src)
            && let Some(text) = self.snapshot.files.get(Path::new(file))
            && let Some(at) = offset(text, line, col)
        {
            let p = self.at.point(file, at);
            spans.push(At { file: file.to_string(), start: p, end: p });
        }
        (segs, Places { spans, written, slot, counts })
    }

    /// The node's op, without its inputs.
    fn sig(m: &Module, n: &Node, in_voice: bool) -> Value {
        match &n.op {
            Op::Const { unit, .. } => json!({"op": "const", "unit": unit}),
            Op::Lane { lane } if in_voice => json!({"op": "lane", "lane": lane}),
            Op::Lane { lane } => {
                let decl = m.lanes.iter().find(|l| l.key == *lane);
                json!({"op": "lane", "lane": lane, "kind": decl.map(|d| d.kind.to_string()), "default": decl.and_then(|d| d.default)})
            }
            Op::Time { of } => json!({"op": "time", "of": of}),
            Op::Kernel { kernel, .. } => json!({"op": "kernel", "kernel": kernel}),
            Op::Plugin { plugin, key, frames, .. } => json!({"op": "plugin", "plugin": plugin, "key": key, "frames": frames}),
            Op::Feedback { kind, .. } => json!({"op": "feedback", "kind": kind.to_string()}),
            Op::Poly { voices, .. } => json!({"op": "poly", "voices": voices}),
            Op::Import { module, export } => json!({"op": "import", "module": module, "export": export}),
        }
    }

    /// Name every node of `nodes` (one scope: a module's top level, or a voice) under `prefix`.
    fn name_all(&self, w: &Within, nodes: &[Node], prefix: &Name) -> Vec<(Name, Places)> {
        let made: Vec<(Vec<String>, Places)> = nodes.iter().map(|n| self.segments(w, n)).collect();
        // For each name but its occurrence: how many share it, and the next occurrence.
        let mut groups: HashMap<Vec<String>, (u32, u32)> = HashMap::new();
        for (segs, _) in &made {
            groups.entry(segs.clone()).or_default().0 += 1;
        }
        made.into_iter()
            .map(|(segs, mut places)| {
                let (size, next) = groups.get_mut(&segs).expect("counted");
                places.counts.push(*size);
                let mut name = prefix.clone();
                name.extend(segs);
                name.push(format!("#{next}"));
                *next += 1;
                (name, places)
            })
            .collect()
    }

    /// Every node of one scope (a module's top level, or a voice, which reads its own nodes
    /// first, then the module's), and the voices inside it. Returns the scope's names by id.
    fn scope(
        &self,
        w: &Within,
        nodes: &[Node],
        prefix: &Name,
        outer: Option<&HashMap<String, Name>>,
        g: &mut Graph,
    ) -> HashMap<String, Name> {
        let (m, named) = (w.m, self.name_all(w, nodes, prefix));
        let mut ids: HashMap<String, Name> = outer.cloned().unwrap_or_default();
        for (n, (name, ..)) in nodes.iter().zip(&named) {
            ids.insert(n.id.clone(), name.clone());
        }
        let resolve = |ids: &HashMap<String, Name>, r: &str| {
            let (id, port) = split_ref(r);
            Ref { node: ids.get(id).cloned().unwrap_or_else(|| vec![m.key.clone(), format!("?{id}")]), port: port.to_string() }
        };
        for (n, (name, Places { spans, written, slot, counts })) in nodes.iter().zip(named) {
            let mut inputs = BTreeMap::new();
            match &n.op {
                Op::Kernel { inputs: i, .. } | Op::Plugin { inputs: i, .. } => {
                    for (port, r) in i {
                        inputs.insert(port.clone(), resolve(&ids, r));
                    }
                }
                Op::Feedback { input, .. } => {
                    inputs.insert("input".into(), resolve(&ids, input));
                }
                Op::Poly { events, body, .. } => {
                    inputs.insert("events".into(), resolve(&ids, events));
                    let mut voice = name.clone();
                    voice.push("voice".into());
                    let local = self.scope(w, &body.nodes, &voice, Some(&ids), g);
                    inputs.insert("out".into(), resolve(&local, &body.out));
                }
                _ => {}
            }
            let value = match &n.op {
                Op::Const { value, .. } => Some(*value),
                _ => None,
            };
            let sig = Side::sig(m, n, outer.is_some());
            debug_assert!(!g.nodes.contains_key(&name), "a name built twice: {name:?}");
            g.nodes.insert(name, Facts { sig, inputs, value, spans, written, slot, counts });
        }
        ids
    }

    /// The whole graph, by name.
    pub fn graph(&self) -> Graph {
        self.walk(None)
    }

    /// The whole graph, and each top-level node's name by its checked name (`<module>/<node id>`,
    /// `FNode::name`): what `continues` joins a revision's nodes by.
    pub fn named(&self) -> (Graph, HashMap<String, Name>) {
        let mut top = HashMap::new();
        (self.walk(Some(&mut top)), top)
    }

    fn walk(&self, mut top: Option<&mut HashMap<String, Name>>) -> Graph {
        let sites = self.sites();
        let mut g = Graph::default();
        let ir = &self.compiled.ir;
        for m in &ir.modules {
            g.modules.insert(m.key.clone());
            let mut runs = HashMap::new();
            for f in &m.frames {
                *runs.entry((f.parent, f.site)).or_default() += 1;
            }
            let ids = self.scope(&Within { m, sites: &sites, runs }, &m.nodes, &vec![m.key.clone()], None, &mut g);
            if let Some(top) = top.as_deref_mut() {
                top.extend(ids.iter().map(|(id, name)| (format!("{}/{id}", m.key), name.clone())));
            }
            for (e, r) in &m.exports {
                let (id, port) = split_ref(r);
                let node = ids.get(id).cloned().unwrap_or_else(|| vec![m.key.clone(), format!("?{id}")]);
                g.exports.insert((m.key.clone(), e.clone()), Ref { node, port: port.to_string() });
            }
        }
        g.root = (ir.root.module.clone(), ir.root.export.clone());
        g
    }
}

/// The node a delta lists as added or removed.
fn dnode(name: &Name, f: &Facts) -> DNode {
    let mut op = f.sig.clone();
    if let Some(v) = f.value {
        op["value"] = json!(v);
    }
    DNode { name: name.clone(), op, inputs: f.inputs.clone() }
}

/// The delta from `old` to `new`, and both graphs (for judging expectations). When the two share
/// one build (a data revision), the graph lists are empty without a walk.
pub fn compare(old: &Side, new: &Side) -> (GraphDelta, Graph, Graph) {
    let mut d = GraphDelta::default();
    let same_code = std::sync::Arc::ptr_eq(old.compiled.code(), new.compiled.code());
    let (a, b) = if same_code { (Graph::default(), Graph::default()) } else { (old.graph(), new.graph()) };
    if !same_code {
        for (name, f) in &b.nodes {
            match a.nodes.get(name) {
                None => d.added.push(dnode(name, f)),
                Some(g) if g.sig != f.sig => {
                    d.removed.push(dnode(name, g));
                    d.added.push(dnode(name, f));
                }
                Some(g) => {
                    let ports: BTreeSet<&String> = g.inputs.keys().chain(f.inputs.keys()).collect();
                    for p in ports {
                        let (x, y) = (g.inputs.get(p), f.inputs.get(p));
                        if x != y {
                            d.rewired.push(Rewire { node: name.clone(), input: p.clone(), from: x.cloned(), to: y.cloned() });
                        }
                    }
                    if let (Some(x), Some(y)) = (g.value, f.value)
                        && x != y
                    {
                        d.values.push(ValueChange { node: name.clone(), from: x, to: y });
                    }
                }
            }
        }
        for (name, g) in &a.nodes {
            if !b.nodes.contains_key(name) {
                d.removed.push(dnode(name, g));
            }
        }
        // A module's exports are its own: an added or removed module's come and go with it.
        let both = |m: &String| a.modules.contains(m) && b.modules.contains(m);
        let keys: BTreeSet<&(String, String)> = a.exports.keys().chain(b.exports.keys()).filter(|k| both(&k.0)).collect();
        for k in keys {
            let (x, y) = (a.exports.get(k), b.exports.get(k));
            if x != y {
                d.rewired.push(Rewire { node: vec![k.0.clone(), "exports".into()], input: k.1.clone(), from: x.cloned(), to: y.cloned() });
            }
        }
        if a.root != b.root {
            let r = |(m, e): &(String, String)| Some(Ref { node: vec![m.clone(), "exports".into()], port: e.clone() });
            d.rewired.push(Rewire { node: vec!["root".into()], input: "out".into(), from: r(&a.root), to: r(&b.root) });
        }
        d.modules = modules(&a, &b);
    }
    data(old, new, &mut d);
    d.added.sort_by(|x, y| by_name(&x.name, &y.name));
    d.removed.sort_by(|x, y| by_name(&x.name, &y.name));
    d.rewired.sort_by(|x, y| by_name(&x.node, &y.node).then_with(|| x.input.encode_utf16().cmp(y.input.encode_utf16())));
    d.values.sort_by(|x, y| by_name(&x.node, &y.node));
    (d, a, b)
}

/// Modules added and removed; a removed and an added one whose graphs are the same but for the
/// key are one re-keyed.
fn modules(a: &Graph, b: &Graph) -> Vec<ModuleChange> {
    let key = |k: &str| Key::new(k).ok();
    let mut added: Vec<&String> = b.modules.difference(&a.modules).collect();
    let mut removed: Vec<&String> = a.modules.difference(&b.modules).collect();
    // A module's graph with its key taken off every name.
    let bare = |g: &Graph, m: &str| -> Bare {
        let strip = |n: &Name| if n.first().map(String::as_str) == Some(m) { n[1..].to_vec() } else { n.clone() };
        g.nodes
            .iter()
            .filter(|(n, _)| n.first().map(String::as_str) == Some(m))
            .map(|(n, f)| (strip(n), f.sig.clone(), f.inputs.iter().map(|(p, r)| (p.clone(), strip(&r.node), r.port.clone())).collect()))
            .collect()
    };
    let mut out = vec![];
    removed.retain(|r| {
        let mine = bare(a, r);
        match added.iter().position(|x| bare(b, x) == mine) {
            Some(i) => {
                let to = added.remove(i);
                out.extend(key(r).zip(key(to)).map(|(from, to)| ModuleChange::Rekeyed { from, to }));
                false
            }
            None => true,
        }
    });
    out.extend(added.into_iter().filter_map(|k| key(k).map(ModuleChange::Added)));
    out.extend(removed.into_iter().filter_map(|k| key(k).map(ModuleChange::Removed)));
    out.sort();
    out
}

/// Lanes, plugin states, assets and the arrangement.
fn data(old: &Side, new: &Side, d: &mut GraphDelta) {
    let (a, b) = (&old.compiled.data, &new.compiled.data);
    let lanes = |c: &Compiled| -> BTreeSet<LaneAddr> { c.decls.lanes.iter().map(|(l, _)| l.clone()).collect() };
    for addr in lanes(old.compiled).intersection(&lanes(new.compiled)) {
        let (x, y) = (a.lane(addr.module.as_str(), addr.lane.as_str()), b.lane(addr.module.as_str(), addr.lane.as_str()));
        let differ = match (x, y) {
            (Some(p), Some(q)) => !std::sync::Arc::ptr_eq(p, q) && p != q,
            (None, None) => false,
            _ => true,
        };
        if differ {
            d.lanes.push(addr.clone());
        }
    }
    let states = |c: &Compiled| -> BTreeSet<(String, Vec<String>)> {
        c.decls.states.iter().map(|s| (s.module.to_string(), s.path.iter().map(|k| k.to_string()).collect())).collect()
    };
    for (m, p) in states(old.compiled).intersection(&states(new.compiled)) {
        let path: Vec<&str> = p.iter().map(String::as_str).collect();
        let present = |c: &Compiled| c.data.state(m, &path).is_some();
        if a.state_hash(m, &path) != b.state_hash(m, &path) || present(old.compiled) != present(new.compiled) {
            d.states.push(format!("{m}/{}", p.join("/")));
        }
    }
    // Assets some audio clip plays, on either side.
    let mut named = BTreeSet::new();
    for c in [old.compiled, new.compiled] {
        for (addr, _) in c.decls.lanes.iter().filter(|(_, k)| *k == m2_song::LaneKind::Audio) {
            for clip in c.data.audio(addr.module.as_str(), addr.lane.as_str()) {
                named.insert(clip.asset.clone());
            }
        }
    }
    // An asset no lane of a side names is only listed there: there, it is known to exist, not
    // what it holds. So a clip that starts or stops playing an asset whose bytes never changed
    // doesn't count as the asset changing (WRITE-6's rule 5 would refuse it as unwritten); the
    // clip's own change is in `lanes`.
    for asset in named {
        let Ok(name) = m2_song::AssetName::new(&asset) else { continue };
        let p = DataPath::Asset(name).path();
        let changed = match (old.snapshot.binaries.get(&p), new.snapshot.binaries.get(&p)) {
            (Some(Stored::Listed), Some(Stored::Listed | Stored::Hash(_))) | (Some(Stored::Hash(_)), Some(Stored::Listed)) => false,
            (Some(Stored::Hash(a)), Some(Stored::Hash(b))) => a != b,
            (None, None) => false,
            // Made, gone, or unreadable on one side.
            (a, b) => a != b,
        };
        if changed {
            d.assets.push(asset);
        }
    }
    d.arrangement = *a.arrangement != *b.arrangement;
}

//! The graph the canvas shows: the calls written in the project, not the kernels they
//! expand into.
//!
//! Every node in the IR is assigned to the call it came from in the project's own
//! code: the outermost component call it was made inside (`bass(...)` for everything
//! the bass instrument builds, by its frames, ID-15), or else its own call (`kernel(...)`,
//! `lane.notes(...)`). Constants become knobs on the innermost call that contains where they
//! were written; a declared control a call leaves out is a knob on that call too, showing its
//! default until its first edit writes it in (PKG-3). A knob's range is the one its
//! component's descriptor declares (PKG-8): core guesses none from units (P10). Edges are the
//! connections between those calls. The view is derived from the accepted revision every time;
//! nothing about it is saved.
//!
//! Each node also has ports: an input for every knob, cable and signal that enters
//! the call, named where it is written (`cutoff`, `hats.in`) or else by the kernel port
//! it feeds, and an output for every signal that leaves it. Port edges connect those,
//! typed as the checked graph types the signal.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use m2_ir::{FNode, FOp, Kernels, Kind, Node, Op, Origin, PortKind, Unit, split_ref};
use m2_script::sites::{Arg, Site};
use serde::Serialize;

use crate::edit::effect::calls_of;
use crate::{Declared, Home};
use crate::edit::{Control, Inlet, Outlet, Session, declared, not_insertable};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GraphView {
    /// The accepted revision this view shows; edits name it.
    pub revision: u64,
    pub modules: Vec<ModuleView>,
    /// Why the files on disk don't compile right now (the view shows the last
    /// revision that did).
    pub draft_errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ModuleView {
    /// The module's key: what edits, selection and the timeline name it by (ID-11).
    pub key: String,
    /// What to show for it, never an address (ID-10).
    pub label: String,
    pub track: bool,
    pub nodes: Vec<ViewNode>,
    /// `(from, to)` node ids.
    pub edges: Vec<(String, String)>,
    /// The same connections, from an output port to an input port.
    pub port_edges: Vec<PortEdge>,
}

/// A connection from one node's output to another's input.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PortEdge {
    pub from: String,
    pub from_port: String,
    pub to: String,
    pub to_port: String,
    /// `audio`, `control` or `notes`.
    pub kind: &'static str,
    /// Read one block late, by a `feedback()`: the edge that closes a loop.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub late: bool,
}

/// Where a signal or value comes into a node.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct InputPort {
    /// Unique within its node.
    pub id: String,
    /// Where it is written in the call (`cutoff`, `hats.in`), or the port it feeds.
    pub label: String,
    /// `audio`, `control` or `notes`.
    pub kind: &'static str,
    /// The knob that sets it, when a number is written there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knob: Option<String>,
    /// The cable connected to it, by its inlet's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inlet: Option<String>,
    /// The value written there that can only be edited in code: its index in `readonly`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<usize>,
}

impl InputPort {
    fn new(id: String, label: &str, kind: &'static str) -> InputPort {
        InputPort { id, label: label.to_string(), kind, knob: None, inlet: None, readonly: None }
    }
}

/// A signal a node gives to others.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct OutputPort {
    /// Unique within its node.
    pub id: String,
    /// The `const` it is bound to, or else its kind; another port of the call by its name.
    pub label: String,
    /// `audio`, `control` or `notes`.
    pub kind: &'static str,
    /// Whether it is the node's outlet, where a cable can start.
    pub outlet: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ViewNode {
    pub id: String,
    pub label: String,
    /// `call`, `lane`, `time`, `import`, `value` or `export`.
    pub kind: &'static str,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub knobs: Vec<Knob>,
    /// Values that are valid code but have no supported source edit.
    pub readonly: Vec<ReadOnly>,
    /// The `const` this node's signal is bound to, if a cable can start here. Cables
    /// name the node by its id.
    pub outlet: Option<String>,
    /// Inputs connected by a cable.
    pub inlets: Vec<InletView>,
    /// Whether its result is audio, which an effect can go after.
    pub audio: bool,
    /// The ComponentId of the component the call is an instance of (`std:delay`), whatever the
    /// name it is called by (PKG-9).
    pub component: Option<String>,
    /// The effect this node is, by its descriptor's label (`Delay`), if it can be removed.
    pub effect: Option<String>,
    /// Its inputs in the order they are written: knobs, cables and signals.
    pub inputs: Vec<InputPort>,
    /// Its outputs, its own result first.
    pub outputs: Vec<OutputPort>,
}

/// An input connected by a cable, as the canvas shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct InletView {
    /// What disconnecting names.
    pub id: String,
    pub label: String,
    /// The name written there, and the node it comes from.
    pub name: String,
    pub from: String,
}

/// A value written in the code that has no supported source edit.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReadOnly {
    pub label: String,
    pub expression: String,
    pub reason: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// Where the value lives (HOME-1).
    pub home: Home,
}

/// A control as the canvas shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Knob {
    pub id: String,
    pub label: String,
    pub value: f64,
    /// The number written in the code; empty while the control is inherited.
    pub literal: String,
    /// The constant's unit: a quantity's own (`semis(60)` is semitones wherever it goes), else
    /// the declared one.
    pub unit: Option<&'static str>,
    /// The range its descriptor declares for that unit, in it (a hint, never a clamp, PKG-8); a
    /// raw `kernel()` input's port range. None when nothing declares one for this unit, or the
    /// controls it feeds disagree: the UI picks its own span.
    pub range: Option<[f64; 2]>,
    /// Drag on a logarithmic scale.
    pub log: bool,
    /// Written in the code; false while it plays its declared default (inherited), until its
    /// first edit writes it into the call.
    pub written: bool,
    /// The declared default, when the controls it feeds declare one in its unit.
    pub default: Option<f64>,
    /// The declared control it is (`cutoff`, `channels.hats.db`) and its component
    /// (`std:lowpass`), when it feeds exactly one.
    pub slot: Option<String>,
    pub component: Option<String>,
    pub origin: Origin,
    pub file: String,
    pub line: u32,
    pub uses: usize,
    /// Where the value lives (HOME-1).
    pub home: Home,
}

/// What every declared control a value feeds says of `f`, when they all agree.
fn agreed<T: PartialEq>(declared: &[Declared], f: impl Fn(&Declared) -> T) -> Option<T> {
    let first = f(declared.first()?);
    declared.iter().skip(1).all(|d| f(d) == first).then_some(first)
}

/// How a value is keyed, as its control's id is: a written number by its site and argument
/// path, an inherited control by its call and `@slot`. None for a constant that is neither.
fn value_key(o: &Origin) -> Option<(u32, Vec<String>)> {
    match o.slot() {
        _ if !o.arg.is_empty() => Some((o.site, o.arg.clone())),
        Some(slot) if o.inherited() => Some((o.site, vec![format!("@{slot}")])),
        _ => None,
    }
}

/// The argument path (`["0", "hats", "db"]`) at which a call's source span `start..end`
/// is written, if it is one of the call's arguments or nested object properties.
fn path_in(site: &Site, start: u32, end: u32) -> Option<Vec<String>> {
    fn find(a: &Arg, start: u32, end: u32, path: &mut Vec<String>) -> bool {
        let (s, e) = a.span();
        if (s, e) == (start, end) {
            return true;
        }
        if let Arg::Object { props, .. } = a {
            for (k, v) in props {
                path.push(k.clone());
                if find(v, start, end, path) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    site.args.iter().enumerate().find_map(|(i, a)| {
        let mut path = vec![i.to_string()];
        find(a, start, end, &mut path).then_some(path)
    })
}

/// The argument path (`["1", "cutoff"]`) of a call written around source offset `at`:
/// the innermost argument or object property containing it.
fn path_around(site: &Site, at: u32) -> Option<Vec<String>> {
    fn find(a: &Arg, at: u32, path: &mut Vec<String>) -> bool {
        let (s, e) = a.span();
        if !(s <= at && at < e) {
            return false;
        }
        if let Arg::Object { props, .. } = a {
            for (k, v) in props {
                path.push(k.clone());
                if find(v, at, path) {
                    return true;
                }
                path.pop();
            }
        }
        true
    }
    site.args.iter().enumerate().find_map(|(i, a)| {
        let mut path = vec![i.to_string()];
        find(a, at, &mut path).then_some(path)
    })
}

/// Where something is written: a file and a byte offset in it.
type Written = (String, u32);

/// A node's `src` (`file:line:col`, both 1-based) as where it is written.
fn locate(src: &str, files: &BTreeMap<std::path::PathBuf, String>) -> Option<Written> {
    let mut parts = src.rsplitn(3, ':');
    let (col, line, file) = (parts.next()?.parse::<usize>().ok()?, parts.next()?.parse::<usize>().ok()?, parts.next()?);
    let text = files.get(Path::new(file))?;
    let start = if line <= 1 { 0 } else { text.match_indices('\n').nth(line - 2)?.0 + 1 };
    let offset = text[start..].char_indices().nth(col.checked_sub(1)?).map_or(text.len(), |(i, _)| start + i);
    Some((file.to_string(), offset as u32))
}

/// How a port's signal is shown: `audio`, `control` or `notes`.
fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Audio(_) => "audio",
        Kind::Control(_) => "control",
        Kind::Events(_) => "notes",
    }
}

/// The checked node named `name`, voices included.
fn checked<'a>(nodes: &'a [FNode], name: &str) -> Option<&'a FNode> {
    nodes.iter().find_map(|n| match &n.op {
        _ if n.name == name => Some(n),
        FOp::Poly { voice, .. } => checked(&voice.nodes, name),
        _ => None,
    })
}

impl Session {
    /// The signal call `site` returned, if exactly one: its module and port reference.
    fn call_output(&self, site: u32) -> Option<(&str, &str)> {
        let mut outputs = self.accepted().compiled.outputs.iter().filter(|o| o.site == site);
        let (Some(output), None) = (outputs.next(), outputs.next()) else { return None };
        Some((&output.module, &output.port))
    }

    /// What port reference `r` of `module` carries, as the checked graph types it.
    /// An import is the export it names.
    fn kind_of(&self, module: &str, r: &str) -> Option<Kind> {
        let compiled = &self.accepted().compiled;
        let (mut module, mut r) = (module, r);
        // Imports can't go round in a circle in a checked graph; the bound is a guard.
        for _ in 0..32 {
            let (id, port) = split_ref(r);
            let m = compiled.ir.modules.iter().find(|m| m.key == module)?;
            let mut found = None;
            walk_nodes(&m.nodes, &mut |n| {
                if n.id == id {
                    found = Some(n);
                }
            });
            let node = found?;
            let index = match &node.op {
                Op::Import { module: target, export } => {
                    r = compiled.ir.modules.iter().find(|m| &m.key == target)?.exports.get(export)?;
                    module = target;
                    continue;
                }
                Op::Kernel { kernel, .. } => self.packages.kernel(kernel)?.output_index(port)?,
                Op::Plugin { plugin, .. } => self.packages.plugin(plugin)?.output_index(port)?,
                _ => 0,
            };
            return checked(&compiled.flat.nodes, &format!("{module}/{id}"))?.outputs.get(index).copied();
        }
        None
    }

    /// Whether call `site` returned one audio signal, as the checked graph types it.
    fn audio_out(&self, site: u32) -> bool {
        matches!(self.call_output(site).and_then(|(m, r)| self.kind_of(m, r)), Some(Kind::Audio(_)))
    }
}

/// Every node, voices included.
fn walk_nodes<'a>(ns: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk_nodes(&body.nodes, f);
        }
    }
}

/// The smallest call among `groups` whose source contains the inlet's name.
fn innermost(groups: &[Group], sites: &HashMap<u32, &Site>, inlet: &Inlet) -> Option<Group> {
    groups
        .iter()
        .filter_map(|g| match g {
            Group::Site(s) => {
                sites.get(s).filter(|x| x.file == inlet.file && x.start <= inlet.start() && inlet.end() <= x.end).map(|x| (x.end - x.start, g))
            }
            _ => None,
        })
        .min_by_key(|(len, _)| *len)
        .map(|(_, g)| g.clone())
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Group {
    Site(u32),
    Time(String),
    Import(String, String),
    /// The note a voice of `poly` call `site` plays: its pitch, gate, velocity, ….
    Voice(u32),
}

impl Session {
    /// The canvas view of the accepted revision.
    pub fn view(&self) -> GraphView {
        let rev = self.accepted();
        let sites: HashMap<u32, &Site> = rev.compiled.sites.iter().map(|s| (s.id, s)).collect();
        let in_project = |id: u32| sites.get(&id).is_some_and(|s| s.in_project);
        let controls: HashMap<(u32, Vec<String>), Control> =
            self.controls().into_iter().filter_map(|c| Some((value_key(&c.origin)?, c))).collect();
        let outlets = self.outlets();
        let inlets = self.inlets();

        let mut modules = vec![];
        for m in &rev.compiled.ir.modules {
            // Every node of the module, voices included, by id.
            let mut all: Vec<&Node> = vec![];
            walk_nodes(&m.nodes, &mut |n| all.push(n));
            let by_id: HashMap<&str, &Node> = all.iter().map(|n| (n.id.as_str(), *n)).collect();
            // The `poly` call each voice's nodes are in.
            let mut voice_of: HashMap<&str, &Node> = HashMap::new();
            for n in &all {
                if let Op::Poly { body, .. } = &n.op {
                    voice_of.extend(body.nodes.iter().map(|b| (b.id.as_str(), *n)));
                }
            }

            let call_group = |n: &Node| -> Option<Group> {
                if matches!(n.op, Op::Const { .. }) {
                    return None;
                }
                if let Some(f) = calls_of(m, n).find(|&f| in_project(f)) {
                    return Some(Group::Site(f));
                }
                match &n.op {
                    Op::Time { of } => Some(Group::Time(format!("{of:?}").to_lowercase())),
                    Op::Import { module, export } => Some(Group::Import(module.clone(), export.clone())),
                    // The note a voice plays, which `poly` gives it: inside a component, part
                    // of it; else a node of its own beside the `poly` call.
                    Op::Lane { .. } if n.origin.is_none() && voice_of.contains_key(n.id.as_str()) => {
                        let p = voice_of[n.id.as_str()];
                        match calls_of(m, p).find(|&f| in_project(f)) {
                            Some(f) => Some(Group::Site(f)),
                            None => p.origin.as_ref().filter(|o| in_project(o.site)).map(|o| Group::Voice(o.site)),
                        }
                    }
                    _ => n.origin.as_ref().filter(|o| in_project(o.site)).map(|o| Group::Site(o.site)),
                }
            };

            // A direct value belongs to the innermost containing call. A shared
            // source gets its own node, so consumers cannot look independently editable.
            let mut knobs: BTreeMap<Group, Vec<Knob>> = BTreeMap::new();
            let mut readonly: BTreeMap<Group, Vec<ReadOnly>> = BTreeMap::new();
            let mut homes: HashMap<(u32, Vec<String>), Group> = HashMap::new();
            // Where each knob (by id) and read-only value is written, to order a node's inputs.
            let mut knob_at: HashMap<String, Written> = HashMap::new();
            let mut readonly_at: BTreeMap<Group, Vec<Written>> = BTreeMap::new();
            let groups: Vec<Group> = {
                let mut g: Vec<Group> = all.iter().filter_map(|n| call_group(n)).collect();
                g.sort();
                g.dedup();
                g
            };
            let mut seen = HashSet::new();
            for n in &all {
                let (Op::Const { value, .. }, Some(o)) = (&n.op, &n.origin) else { continue };
                let Some(key) = value_key(o) else { continue };
                let Some(at) = sites.get(&o.site).filter(|s| s.in_project) else { continue };
                if !seen.insert(key.clone()) {
                    continue;
                }
                let inherited = o.arg.is_empty();
                let control = controls.get(&key);
                let home = if control.is_some_and(|c| c.uses > 1) {
                    Group::Site(o.site)
                } else {
                    groups
                        .iter()
                        .filter_map(|g| match g {
                            Group::Site(s) => sites
                                .get(s)
                                .filter(|x| x.file == at.file && x.start <= at.start && at.end <= x.end)
                                .map(|x| (x.end - x.start, g)),
                            _ => None,
                        })
                        .min_by_key(|(len, _)| *len)
                        .map(|(_, g)| g.clone())
                        .unwrap_or(Group::Site(o.site))
                };
                homes.insert(key, home.clone());
                // Name it after its property, or the port it feeds, or its call.
                let consumer = all.iter().find_map(|k| match &k.op {
                    Op::Kernel { kernel, inputs } => {
                        inputs.iter().find(|(_, r)| split_ref(r).0 == n.id).map(|(p, _)| (self.packages.kernel(kernel), p.clone()))
                    }
                    Op::Plugin { plugin, inputs, .. } => {
                        inputs.iter().find(|(_, r)| split_ref(r).0 == n.id).map(|(p, _)| (self.packages.plugin(plugin), p.clone()))
                    }
                    _ => None,
                });
                // A unit value like `db(-6)` written as a property is named by where it
                // is written in its home call: `hats.db`.
                let written_at = match &home {
                    Group::Site(h) if *h != o.site => path_in(sites[h], at.start, at.end),
                    _ => None,
                };
                let path = written_at.as_ref().unwrap_or(&o.arg);
                // A declared control is named by its declaration: `cutoff`, or `hats.db` in a dictionary.
                let declared = declared(&rev.compiled, m, o, None);
                let label = if let Some(d) = declared.as_ref().filter(|_| inherited) {
                    d.label.clone()
                } else if path.len() > 1 {
                    path[1..].join(".")
                } else {
                    consumer.as_ref().map(|(_, p)| p.clone()).unwrap_or_else(|| at.callee.clone())
                };
                let Some(c) = control else {
                    let (expression, reason, start) = if inherited {
                        // A control the call leaves out that can't be written into it.
                        let why = not_insertable(rev, at, m, o).unwrap_or("it can't be written into the call");
                        (value.to_string(), format!("Plays its default: {why}. Write it in the code to change it."), at.end)
                    } else {
                        // For an ambiguous object, the whole argument is the smallest
                        // expression we can honestly identify. Never guess a property.
                        let exact = at.arg(&o.arg);
                        let arg = exact.or_else(|| o.arg.first()?.parse::<usize>().ok().and_then(|i| at.args.get(i)));
                        let Some(arg) = arg else { continue };
                        let (start, end) = arg.span();
                        let expression = rev
                            .snapshot
                            .files
                            .get(Path::new(&at.file))
                            .and_then(|text| text.get(start as usize..end as usize))
                            .unwrap_or("")
                            .to_string();
                        let reason = if exact.is_none() {
                            "This property has no unambiguous literal in the source object."
                        } else {
                            "Only a directly written number can be edited here."
                        };
                        (expression, reason.to_string(), start)
                    };
                    readonly_at.entry(home.clone()).or_default().push((at.file.clone(), start));
                    readonly.entry(home).or_default().push(ReadOnly {
                        label,
                        expression,
                        reason,
                        file: Some(at.file.clone()),
                        line: Some(at.line),
                        home: Home::Code,
                    });
                    continue;
                };
                let port = consumer.and_then(|(sig, p)| sig?.inputs.iter().find(|i| i.name == p));
                let port_unit = port.and_then(|p| match p.kind {
                    PortKind::Control(u) => u,
                    _ => None,
                });
                let unit = c.unit.or(port_unit);
                // The declared range and default when every control it feeds agrees (D4); a
                // raw kernel input's port range (never a log scale: nothing declares one).
                let (range, log) = match agreed(&c.declared, |d| (d.range, d.log)) {
                    _ if c.declared.is_empty() => (port.and_then(|p| p.range), false),
                    Some((Some(range), log)) => (Some(range), log),
                    _ => (None, false),
                };
                let default = agreed(&c.declared, |d| d.default).flatten();
                let single = c.declared.first().filter(|_| c.declared.len() == 1);
                let label = match single {
                    Some(d) => d.label.clone(),
                    None => label,
                };
                // Written values in the order the call writes them; the ones it leaves out after.
                let place = if inherited { at.end } else { at.arg(&o.arg).map_or(at.start, |a| a.span().0) };
                knob_at.insert(c.id.clone(), (at.file.clone(), place));
                knobs.entry(home).or_default().push(Knob {
                    id: c.id.clone(),
                    label,
                    value: c.value,
                    literal: c.literal.clone(),
                    unit: unit.map(Unit::name),
                    range: range.map(|(lo, hi)| [lo, hi]),
                    log,
                    written: c.written,
                    default,
                    slot: single.map(|d| d.slot.clone()),
                    component: single.map(|d| d.component.clone()),
                    origin: c.origin.clone(),
                    file: c.file.clone(),
                    line: c.line,
                    uses: c.uses,
                    home: c.home,
                });
            }
            // Cables end on the innermost call containing the name, like knobs.
            let mut connected: BTreeMap<Group, Vec<InletView>> = BTreeMap::new();
            let mut cabled: HashMap<String, (&Inlet, &Outlet)> = HashMap::new();
            for inlet in &inlets {
                let Some(outlet) = outlets.iter().find(|o| o.id == inlet.from && o.module == m.key) else { continue };
                cabled.insert(inlet.id.clone(), (inlet, outlet));
                let home = innermost(&groups, &sites, inlet).unwrap_or(Group::Site(inlet.site));
                // Name it after its property, or else the one port it feeds in that call.
                let label = if inlet.path.len() > 1 {
                    inlet.path[1..].join(".")
                } else {
                    let port = all
                        .iter()
                        .filter(|k| call_group(k).as_ref() == Some(&home))
                        .flat_map(|k| match &k.op {
                            Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => {
                                inputs.iter().filter(|(_, r)| outlet.reads(r)).map(|(p, _)| p.clone()).collect()
                            }
                            _ => vec![],
                        })
                        .collect::<Vec<_>>();
                    match port.as_slice() {
                        [one] => one.clone(),
                        _ => format!("input {}", inlet.path[0].parse::<usize>().map_or(0, |i| i + 1)),
                    }
                };
                connected.entry(home).or_default().push(InletView {
                    id: inlet.id.clone(),
                    label,
                    name: inlet.name.clone(),
                    from: outlet.id.clone(),
                });
            }

            let group_of = |n: &Node| -> Option<Group> {
                if matches!(n.op, Op::Const { .. }) {
                    return n.origin.as_ref().and_then(value_key).and_then(|k| homes.get(&k).cloned());
                }
                call_group(n)
            };

            // Nodes, in order of first appearance.
            let mut nodes: Vec<ViewNode> = vec![];
            let mut ids: HashMap<Group, String> = HashMap::new();
            let mut add = |g: &Group, n: Option<&Node>, nodes: &mut Vec<ViewNode>| -> String {
                if let Some(id) = ids.get(g) {
                    return id.clone();
                }
                let (id, label, kind, file, line) = match g {
                    Group::Site(s) => {
                        let site = sites[s];
                        let kind = if !groups.contains(g) {
                            "value"
                        } else if matches!(n.map(|n| &n.op), Some(Op::Lane { .. }))
                            && n.is_some_and(|n| calls_of(m, n).next().is_none() && n.origin.is_some())
                        {
                            "lane"
                        } else {
                            "call"
                        };
                        let label = match n.map(|n| &n.op) {
                            // A lane shows its label; it is addressed by its key (ID-10).
                            Some(Op::Lane { lane }) if kind == "lane" => {
                                m.lanes.iter().find(|l| &l.key == lane).map_or(lane.as_str(), |l| l.label()).to_string()
                            }
                            _ if kind == "value" => site.binding.clone().unwrap_or_else(|| site.callee.clone()),
                            _ => site.callee.clone(),
                        };
                        (format!("{}#{}", site.file, site.ordinal), label, kind, Some(site.file.clone()), Some(site.line))
                    }
                    Group::Time(of) => (format!("time:{of}"), format!("time.{of}"), "time", None, None),
                    Group::Import(module, export) => {
                        // Named by the imported module's label, addressed by its key.
                        let label = rev.compiled.ir.modules.iter().find(|x| &x.key == module).map_or(module.as_str(), |x| x.label());
                        (format!("import:{module}.{export}"), format!("{label}.{export}"), "import", None, None)
                    }
                    Group::Voice(s) => {
                        let site = sites[s];
                        let id = format!("voice:{}#{}", site.file, site.ordinal);
                        (id, "note".to_string(), "voice", Some(site.file.clone()), Some(site.line))
                    }
                };
                ids.insert(g.clone(), id.clone());
                let outlet = match g {
                    Group::Site(s) => outlets.iter().find(|o| o.site == *s && o.module == m.key).map(|o| o.name.clone()),
                    _ => None,
                };
                let (audio, effect) = match g {
                    Group::Site(s) if kind == "call" => (self.audio_out(*s), self.effect_at(sites[s]).map(|e| e.label.clone())),
                    _ => (false, None),
                };
                // The ComponentId its frames record, whatever it is called by.
                let component = match g {
                    Group::Site(s) => m.frames.iter().find(|f| f.site == *s && f.component.is_some()).and_then(|f| f.component.clone()),
                    _ => None,
                };
                nodes.push(ViewNode {
                    id: id.clone(),
                    label,
                    kind,
                    file,
                    line,
                    knobs: knobs.get(g).cloned().unwrap_or_default(),
                    readonly: readonly.get(g).cloned().unwrap_or_default(),
                    outlet,
                    inlets: connected.get(g).cloned().unwrap_or_default(),
                    audio,
                    component,
                    effect,
                    inputs: vec![],
                    outputs: vec![],
                });
                id
            };
            for n in &all {
                if let Some(g) = call_group(n) {
                    add(&g, Some(n), &mut nodes);
                }
            }
            // Values whose source call made no node of its own.
            for g in knobs.keys().chain(readonly.keys()).chain(connected.keys()) {
                add(g, None, &mut nodes);
            }

            // Edges between groups.
            let mut edges: Vec<(String, String)> = vec![];
            let link = |from: Option<Group>, to: Option<Group>, edges: &mut Vec<(String, String)>| {
                if let (Some(a), Some(b)) = (from, to)
                    && a != b
                    && let (Some(x), Some(y)) = (ids.get(&a), ids.get(&b))
                {
                    let e = (x.clone(), y.clone());
                    if !edges.contains(&e) {
                        edges.push(e);
                    }
                }
            };
            let source = |r: &str| by_id.get(split_ref(r).0).copied();
            // The reads that make them: (reading group, the port it reads into, the
            // reference, the source's group).
            let mut reads: Vec<(Group, &str, &str, Group)> = vec![];
            // The reads a `feedback()` makes, one block late: where its loop closes.
            let mut late: HashSet<(Group, &str)> = HashSet::new();
            for n in &all {
                let to = group_of(n);
                let refs: Vec<(&str, &str)> = match &n.op {
                    Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.iter().map(|(p, r)| (p.as_str(), r.as_str())).collect(),
                    Op::Poly { events, body, .. } => vec![("notes", events.as_str()), ("voice", body.out.as_str())],
                    Op::Feedback { input, .. } => vec![("feedback", input.as_str())],
                    _ => vec![],
                };
                for (port, r) in refs {
                    if let Some(src) = source(r) {
                        let from = group_of(src);
                        link(from.clone(), to.clone(), &mut edges);
                        if let (Some(from), Some(to)) = (from, &to)
                            && from != *to
                            && ids.contains_key(&from)
                            && ids.contains_key(to)
                        {
                            if matches!(n.op, Op::Feedback { .. }) {
                                late.insert((to.clone(), r));
                            }
                            reads.push((to.clone(), port, r, from));
                        }
                    }
                }
            }
            let mut exported: Vec<(String, &str, Group)> = vec![];
            for (name, r) in &m.exports {
                let id = format!("export:{name}");
                nodes.push(ViewNode {
                    id: id.clone(),
                    label: name.clone(),
                    kind: "export",
                    file: None,
                    line: None,
                    knobs: vec![],
                    readonly: vec![],
                    outlet: None,
                    inlets: vec![],
                    audio: false,
                    component: None,
                    effect: None,
                    inputs: vec![],
                    outputs: vec![],
                });
                if let Some(g) = source(r).and_then(group_of)
                    && let Some(from) = ids.get(&g)
                {
                    edges.push((from.clone(), id.clone()));
                    exported.push((id, r, g));
                }
            }

            // Ports. A node's inputs are its knobs, cables and read-only values, and each
            // other signal that enters it (one input however many of its kernels read it).
            // Its outputs are its own result, then anything else read from it.
            let kind = |r: &str| self.kind_of(&m.key, r).map_or("control", kind_name);
            // A port is a signal, or a written value however many constants it made.
            let canon = |r: &str| match source(r).filter(|n| matches!(n.op, Op::Const { .. })).and_then(|n| value_key(n.origin.as_ref()?)) {
                Some((site, path)) => format!("value:{site}/{}", path.join(".")),
                None => {
                    let (id, port) = split_ref(r);
                    format!("{id}.{port}")
                }
            };
            let output_label = |r: &str| match (split_ref(r).1, source(r).map(|n| &n.op)) {
                // A voice's note: `pitch`, `gate`, ….
                ("out", Some(Op::Lane { lane })) if voice_of.contains_key(split_ref(r).0) => lane.clone(),
                ("out", _) => kind(r).to_string(),
                (port, _) => port.to_string(),
            };
            let mut inputs: HashMap<String, Vec<(InputPort, Option<Written>)>> = HashMap::new();
            // The argument each signal input is written at, by node and port id: inputs that
            // would be named alike (the `b` of each `add` a `sum` makes) go by position instead.
            let mut written_at: HashMap<(String, String), usize> = HashMap::new();
            let mut outputs: HashMap<String, Vec<OutputPort>> = HashMap::new();
            let mut ordered: Vec<(&Group, &String)> = ids.iter().collect();
            ordered.sort();
            for &(g, id) in &ordered {
                let row = inputs.entry(id.clone()).or_default();
                for k in knobs.get(g).into_iter().flatten() {
                    let port = InputPort { knob: Some(k.id.clone()), ..InputPort::new(format!("knob:{}", k.id), &k.label, "control") };
                    row.push((port, knob_at.get(&k.id).cloned()));
                }
                for (i, (value, at)) in readonly.get(g).into_iter().flatten().zip(readonly_at.get(g).into_iter().flatten()).enumerate() {
                    let port = InputPort { readonly: Some(i), ..InputPort::new(format!("value:{i}"), &value.label, "control") };
                    row.push((port, Some(at.clone())));
                }
                for c in connected.get(g).into_iter().flatten() {
                    let (inlet, outlet) = cabled[&c.id];
                    let kind = self.call_output(outlet.site).and_then(|(module, r)| self.kind_of(module, r)).map_or("control", kind_name);
                    let port = InputPort { inlet: Some(c.id.clone()), ..InputPort::new(format!("inlet:{}", c.id), &c.label, kind) };
                    if let Some(i) = inlet.path.first().and_then(|i| i.parse().ok()) {
                        written_at.insert((id.clone(), port.id.clone()), i);
                    }
                    row.push((port, Some((inlet.file.clone(), inlet.start()))));
                }
                if let Group::Site(s) = g
                    && let Some((module, r)) = self.call_output(*s)
                    && module == m.key
                {
                    // Named by its `const`, or else what it is: `svf.lowpass` gives audio, not `lp`.
                    let outlet = outlets.iter().find(|o| o.site == *s && o.module == m.key);
                    let label = outlet.map_or_else(|| kind(r).to_string(), |o| o.name.clone());
                    let port = OutputPort { id: canon(r), label, kind: kind(r), outlet: outlet.is_some() };
                    outputs.entry(id.clone()).or_default().push(port);
                }
            }
            // The ports each signal enters a node at, for naming its input.
            let mut ports: HashMap<(&Group, String), Vec<&str>> = HashMap::new();
            for (to, port, r, _) in &reads {
                let at = ports.entry((to, canon(r))).or_default();
                if !at.contains(port) {
                    at.push(port);
                }
            }
            let mut port_edges: Vec<PortEdge> = vec![];
            let mut wire = |edge: PortEdge| {
                if !port_edges.contains(&edge) {
                    port_edges.push(edge);
                }
            };
            let mut give = |from: &str, r: &str| {
                let outs = outputs.entry(from.to_string()).or_default();
                if !outs.iter().any(|o| o.id == canon(r)) {
                    outs.push(OutputPort { id: canon(r), label: output_label(r), kind: kind(r), outlet: false });
                }
                canon(r)
            };
            // A name written at several inputs of one call (`add(dry, dry)`, a mixer's two
            // channels) is a cable at each: the call's reads of it take them in turn, in the
            // order it reads its arguments. Otherwise a read takes the cable named after its port.
            let mut turns: HashMap<(&Group, &str), usize> = HashMap::new();
            let mut cables: Vec<Option<&InletView>> = vec![];
            for (to, port, r, _) in &reads {
                let mut at: Vec<&InletView> = connected.get(to).into_iter().flatten().filter(|c| cabled[&c.id].1.reads(r)).collect();
                at.sort_by_key(|c| cabled[&c.id].0.start());
                let count = reads.iter().filter(|(t, _, x, _)| t == to && x == r).count();
                cables.push(if at.len() > 1 && at.len() == count {
                    let turn = turns.entry((to, r)).or_default();
                    *turn += 1;
                    Some(at[*turn - 1])
                } else {
                    at.into_iter().min_by_key(|c| c.label != *port)
                });
            }
            for ((to, _, r, from), cable) in reads.iter().zip(cables) {
                let (to_id, from_id) = (&ids[to], &ids[from]);
                let from_port = give(from_id, r);
                let row = inputs.entry(to_id.clone()).or_default();
                let to_port = match cable {
                    Some(c) => format!("inlet:{}", c.id),
                    None => {
                        let id = format!("in:{from_port}");
                        if !row.iter().any(|(p, _)| p.id == id) {
                            // Named where the signal is written in the call, like knobs.
                            let site = match to {
                                Group::Site(s) => sites.get(s).copied(),
                                _ => None,
                            };
                            let written = match from {
                                Group::Site(s) => sites.get(s).map(|x| (x.file.clone(), x.start)),
                                _ => source(r).and_then(|n| n.src.as_deref()).and_then(|src| locate(src, &rev.snapshot.files)),
                            };
                            let inside = written.filter(|(f, at)| site.is_some_and(|x| &x.file == f && x.start <= *at && *at < x.end));
                            let path = site.zip(inside.as_ref()).and_then(|(x, (_, at))| path_around(x, *at));
                            let label = match (&path, ports[&(to, from_port.clone())].as_slice()) {
                                (Some(p), _) if p.len() > 1 => p[1..].join("."),
                                (_, [one]) => one.to_string(),
                                (Some(p), _) => format!("input {}", p[0].parse::<usize>().map_or(1, |i| i + 1)),
                                (None, _) => "in".to_string(),
                            };
                            let port = InputPort::new(id.clone(), &label, kind(r));
                            if let Some(i) = path.as_ref().and_then(|p| p[0].parse().ok()) {
                                written_at.insert((to_id.clone(), id.clone()), i);
                            }
                            row.push((port, inside));
                        }
                        id
                    }
                };
                let late = late.contains(&(to.clone(), *r));
                wire(PortEdge { from: from_id.clone(), from_port, to: to_id.clone(), to_port, kind: kind(r), late });
            }
            for (to_id, r, from) in &exported {
                let from_port = give(&ids[from], r);
                let port = InputPort::new("in".into(), kind(r), kind(r));
                inputs.entry(to_id.clone()).or_default().push((port, None));
                wire(PortEdge { from: ids[from].clone(), from_port, to: to_id.clone(), to_port: "in".into(), kind: kind(r), late: false });
            }
            // Inputs in the order they are written in the call; signals from elsewhere first.
            let files: HashMap<&String, &str> = ordered
                .iter()
                .filter_map(|(g, id)| match g {
                    Group::Site(s) => sites.get(s).map(|x| (*id, x.file.as_str())),
                    _ => None,
                })
                .collect();
            for n in &mut nodes {
                let mut row = inputs.remove(&n.id).unwrap_or_default();
                row.sort_by_key(|(_, at)| at.as_ref().filter(|(f, _)| files.get(&n.id) == Some(&f.as_str())).map(|(_, o)| *o));
                n.inputs = row.into_iter().map(|(p, _)| p).collect();
                n.outputs = outputs.remove(&n.id).unwrap_or_default();
                let mut names = HashSet::new();
                if !n.inputs.iter().all(|p| names.insert(p.label.clone())) {
                    for p in &mut n.inputs {
                        let Some(i) = written_at.get(&(n.id.clone(), p.id.clone())) else { continue };
                        p.label = format!("input {}", i + 1);
                        if let Some(c) = n.inlets.iter_mut().find(|c| Some(&c.id) == p.inlet.as_ref()) {
                            c.label = p.label.clone();
                        }
                    }
                }
            }
            modules.push(ModuleView { key: m.key.clone(), label: m.label().to_string(), track: m.track, nodes, edges, port_edges });
        }
        GraphView { revision: rev.number, modules, draft_errors: self.draft_errors.iter().map(|d| d.to_string()).collect() }
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;

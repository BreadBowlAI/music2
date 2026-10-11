//! The graph the canvas shows: the calls written in the project, not the kernels they
//! expand into.
//!
//! Every node in the IR is assigned to the call it came from in the project's own
//! code: the outermost component call it was made inside (`bass(...)` for everything
//! the bass instrument builds), or else its own call (`kernel(...)`, `lane.notes(...)`).
//! Constants become knobs on the innermost call that contains where they were written.
//! Edges are the connections between those calls. The view is derived from the
//! accepted revision every time; nothing about it is saved.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use m2_ir::{Kernels, Node, Op, PortKind, Unit, split_ref};
use m2_script::sites::{Arg, Site};
use serde::Serialize;

use crate::edit::{Control, Inlet, Session};

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
    pub name: String,
    pub track: bool,
    pub nodes: Vec<ViewNode>,
    /// `(from, to)` node ids.
    pub edges: Vec<(String, String)>,
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

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReadOnly {
    pub label: String,
    pub expression: String,
    pub reason: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// A control as the canvas shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Knob {
    pub id: String,
    pub label: String,
    pub value: f64,
    pub literal: String,
    pub unit: Option<&'static str>,
    pub min: f64,
    pub max: f64,
    /// Drag on a logarithmic scale (frequencies, times).
    pub log: bool,
    pub origin: m2_ir::Origin,
    pub file: String,
    pub line: u32,
    pub uses: usize,
}

/// A sensible knob range for a unit, widened to include the current value.
fn range(unit: Option<Unit>, v: f64) -> (f64, f64, bool) {
    let (lo, hi, log) = match unit {
        Some(Unit::Hz) => (20.0, 20_000.0, true),
        Some(Unit::Db) => (-60.0, 12.0, false),
        Some(Unit::Seconds) => (0.001, 10.0, true),
        Some(Unit::Beats) => (1.0 / 16.0, 16.0, true),
        Some(Unit::Unit01) => (0.0, 1.0, false),
        Some(Unit::Gain) => (0.0, 2.0, false),
        Some(Unit::Bipolar) => (-1.0, 1.0, false),
        Some(Unit::Semitones) => (v - 24.0, v + 24.0, false),
        Some(Unit::Scalar) | None => {
            if v.abs() <= 1.0 {
                (if v < 0.0 { -1.0 } else { 0.0 }, 1.0, false)
            } else {
                (v.min(0.0) * 2.0, v.max(0.0) * 2.0, false)
            }
        }
    };
    (lo.min(v), hi.max(v), log && v > 0.0)
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
}

impl Session {
    /// The canvas view of the accepted revision.
    pub fn view(&self) -> GraphView {
        let rev = self.accepted();
        let sites: HashMap<u32, &Site> = rev.compiled.sites.iter().map(|s| (s.id, s)).collect();
        let in_project = |id: u32| sites.get(&id).is_some_and(|s| s.in_project);
        let controls: HashMap<(u32, Vec<String>), Control> =
            self.controls().into_iter().map(|c| ((c.origin.site, c.origin.arg.clone()), c)).collect();
        let outlets = self.outlets();
        let inlets = self.inlets();

        let mut modules = vec![];
        for m in &rev.compiled.ir.modules {
            // Every node of the module, voices included, by id.
            let mut all: Vec<&Node> = vec![];
            fn collect<'a>(ns: &'a [Node], out: &mut Vec<&'a Node>) {
                for n in ns {
                    out.push(n);
                    if let Op::Poly { body, .. } = &n.op {
                        collect(&body.nodes, out);
                    }
                }
            }
            collect(&m.nodes, &mut all);
            let by_id: HashMap<&str, &Node> = all.iter().map(|n| (n.id.as_str(), *n)).collect();

            let call_group = |n: &Node| -> Option<Group> {
                if matches!(n.op, Op::Const { .. }) {
                    return None;
                }
                if let Some(&f) = n.within.iter().find(|&&f| in_project(f)) {
                    return Some(Group::Site(f));
                }
                match &n.op {
                    Op::Time { of } => Some(Group::Time(format!("{of:?}").to_lowercase())),
                    Op::Import { module, export } => Some(Group::Import(module.clone(), export.clone())),
                    _ => n.origin.as_ref().filter(|o| in_project(o.site)).map(|o| Group::Site(o.site)),
                }
            };

            // A direct value belongs to the innermost containing call. A shared
            // source gets its own node, so consumers cannot look independently editable.
            let mut knobs: BTreeMap<Group, Vec<Knob>> = BTreeMap::new();
            let mut readonly: BTreeMap<Group, Vec<ReadOnly>> = BTreeMap::new();
            let mut homes: HashMap<(u32, Vec<String>), Group> = HashMap::new();
            let groups: Vec<Group> = {
                let mut g: Vec<Group> = all.iter().filter_map(|n| call_group(n)).collect();
                g.sort();
                g.dedup();
                g
            };
            let mut seen = HashSet::new();
            for n in &all {
                let (Op::Const { .. }, Some(o)) = (&n.op, &n.origin) else { continue };
                let key = (o.site, o.arg.clone());
                let Some(at) = sites.get(&o.site).filter(|s| s.in_project && !o.arg.is_empty()) else { continue };
                if !seen.insert(key.clone()) {
                    continue;
                }
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
                let label = if path.len() > 1 {
                    path[1..].join(".")
                } else {
                    consumer.as_ref().map(|(_, p)| p.clone()).unwrap_or_else(|| at.callee.clone())
                };
                let Some(c) = control else {
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
                    readonly.entry(home).or_default().push(ReadOnly {
                        label,
                        expression,
                        reason: reason.into(),
                        file: Some(at.file.clone()),
                        line: Some(at.line),
                    });
                    continue;
                };
                let port_unit = consumer.and_then(|(sig, p)| match sig?.inputs.iter().find(|i| i.name == p)?.kind {
                    PortKind::Control(u) => u,
                    _ => None,
                });
                let unit = c.unit.or(port_unit);
                let (min, max, log) = range(unit, c.value);
                knobs.entry(home).or_default().push(Knob {
                    id: c.id.clone(),
                    label,
                    value: c.value,
                    literal: c.literal.clone(),
                    unit: unit.map(Unit::name),
                    min,
                    max,
                    log,
                    origin: c.origin.clone(),
                    file: c.file.clone(),
                    line: c.line,
                    uses: c.uses,
                });
            }
            // Cables end on the innermost call containing the name, like knobs.
            let mut connected: BTreeMap<Group, Vec<InletView>> = BTreeMap::new();
            for inlet in &inlets {
                let Some(outlet) = outlets.iter().find(|o| o.id == inlet.from && o.module == m.name) else { continue };
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
                    return n.origin.as_ref().and_then(|o| homes.get(&(o.site, o.arg.clone())).cloned());
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
                        } else if matches!(n.map(|n| &n.op), Some(Op::Lane { .. })) && n.is_some_and(|n| n.within.is_empty()) {
                            "lane"
                        } else {
                            "call"
                        };
                        let label = match n.map(|n| &n.op) {
                            Some(Op::Lane { lane }) if kind == "lane" => lane.clone(),
                            _ if kind == "value" => site.binding.clone().unwrap_or_else(|| site.callee.clone()),
                            _ => site.callee.clone(),
                        };
                        (format!("{}#{}", site.file, site.ordinal), label, kind, Some(site.file.clone()), Some(site.line))
                    }
                    Group::Time(of) => (format!("time:{of}"), format!("time.{of}"), "time", None, None),
                    Group::Import(module, export) => {
                        (format!("import:{module}.{export}"), format!("{module}.{export}"), "import", None, None)
                    }
                };
                ids.insert(g.clone(), id.clone());
                let outlet = match g {
                    Group::Site(s) => outlets.iter().find(|o| o.site == *s && o.module == m.name).map(|o| o.name.clone()),
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
            for n in &all {
                let to = group_of(n);
                let refs: Vec<&str> = match &n.op {
                    Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. } => inputs.values().map(String::as_str).collect(),
                    Op::Poly { events, body, .. } => std::iter::once(events.as_str()).chain(std::iter::once(body.out.as_str())).collect(),
                    Op::Feedback { input, .. } => vec![input.as_str()],
                    _ => vec![],
                };
                for r in refs {
                    if let Some(src) = source(r) {
                        link(group_of(src), to.clone(), &mut edges);
                    }
                }
            }
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
                });
                if let Some(from) = source(r).and_then(group_of).and_then(|g| ids.get(&g).cloned()) {
                    edges.push((from, id));
                }
            }
            modules.push(ModuleView { name: m.name.clone(), track: m.track, nodes, edges });
        }
        GraphView { revision: rev.number, modules, draft_errors: self.draft_errors.iter().map(|d| d.to_string()).collect() }
    }
}

//! Where a look reads (TAP-8, TAP-11): a box of the graph view, by its id, as a point of a plan: the
//! call's output, its input, or one of its sound's meters. The id is the view's for a call
//! (`tracks/bass.ts#3`: its file and its place among the file's calls). An edit may give a call
//! another place, so the server follows the call itself into each new revision, as ID-13 names a
//! call, and takes the id the new view gives it ([`carry`], TAP-9).

use std::collections::HashSet;

use m2_ir::{FOp, Kernels, Kind, Module, Node, Op, PortKind, split_ref};
use m2_runtime::TapAt;
use m2_session::Revision;
use m2_session::text::Edits;

/// Which point of a call a look reads (TAP-11): its output, its input, or a meter by its name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum At {
    Out,
    In,
    Meter(String),
}

impl At {
    /// `out`, `in`, or a meter's name, as an ask writes it.
    pub fn parse(at: &str) -> At {
        match at {
            "out" => At::Out,
            "in" => At::In,
            meter => At::Meter(meter.to_string()),
        }
    }

    /// As an ask writes it.
    pub fn name(&self) -> &str {
        match self {
            At::Out => "out",
            At::In => "in",
            At::Meter(m) => m,
        }
    }
}

/// Where a look reads in a plan, and what it reads there: sound of `channels`, or a meter of `unit`.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub at: TapAt,
    pub channels: u8,
    /// A meter's unit (`db`); None for sound.
    pub unit: Option<String>,
}

/// Where a scope on box `node` of module `module` reads in `rev`: the call's own output, at the top
/// of its track, as sound; and its channels. Refused in words when it isn't one. `kernels` names
/// each kernel's and plugin's outputs, as the revision was built with them.
pub fn point(rev: &Revision, kernels: &dyn Kernels, module: &str, node: &str) -> Result<(TapAt, u8), String> {
    look(rev, kernels, module, node, &At::Out).map(|p| (p.at, p.channels))
}

/// Where a look at point `at` of box `node` of module `module` reads in `rev` (TAP-11): the call's
/// own output, as [`point`] says; the one sound that comes into it from outside, its sidechain
/// inputs aside; or its meter of that name, given by one kernel the call is made of. Refused in
/// words where it doesn't fit.
pub fn look(rev: &Revision, kernels: &dyn Kernels, module: &str, node: &str, at: &At) -> Result<Point, String> {
    let gone = || format!("There's no box {node} in {module} now, so there's nothing to show.");
    let (file, ordinal) = node.rsplit_once('#').ok_or_else(gone)?;
    let ordinal: u32 = ordinal.parse().map_err(|_| gone())?;
    let compiled = &rev.compiled;
    let site = compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal).ok_or_else(gone)?;
    let outputs: Vec<_> = compiled.outputs.iter().filter(|o| o.site == site.id).collect();
    if outputs.is_empty() {
        return Err(format!("{} gives no sound to show.", site.callee));
    }
    let outputs: Vec<_> = outputs.into_iter().filter(|o| o.module == module).collect();
    let output = match outputs[..] {
        [] => return Err(gone()),
        [one] => one,
        _ => return Err(format!("{} is called more than once, so a scope can't tell which one to show.", site.callee)),
    };
    if !output.top {
        return Err(format!("{} plays inside an instrument's voices, one for each note: put the scope on the instrument.", site.callee));
    }
    let callee = &site.callee;
    let m = compiled.ir.modules.iter().find(|m| m.key == output.module).ok_or_else(gone)?;
    // A call turned off passes its input on (Next D1): what plays through it is its input.
    let off = Inside::of(m, site.id).is_some_and(|i| i.bypassed(m));
    match at {
        At::Out if off => look(rev, kernels, module, node, &At::In),
        At::Out => {
            let (at, channels, index) = resolve(rev, kernels, &output.module, &output.port).ok_or_else(gone)?;
            match (index, at) {
                (0, Some(at)) => Ok(Point { at, channels, unit: None }),
                (_, None) => Err(format!("{callee} gives a control or notes, not sound: a scope shows sound.")),
                (i, Some(_)) => Err(format!("{callee} gives its sound on its output {}: a scope shows a box's first output.", i + 1)),
            }
        }
        At::In => {
            let inside = Inside::of(m, site.id).ok_or_else(|| twice(callee))?;
            // What its sidechain inputs hear, and those of the calls inside it, isn't its input.
            let side: HashSet<(usize, usize)> = (m.frames.iter().enumerate())
                .filter(|(i, _)| inside.frame(m, Some(*i as u32)))
                .flat_map(|(_, f)| &f.sidechain)
                .filter_map(|x| port_of(rev, kernels, &m.key, &x.source))
                .collect();
            let mut feeds = vec![];
            for n in m.nodes.iter().filter(|n| inside.has(m, n)) {
                let (Op::Kernel { inputs, .. } | Op::Plugin { inputs, .. }) = &n.op else { continue };
                for r in inputs.values() {
                    let from = m.nodes.iter().find(|x| x.id == split_ref(r).0);
                    if from.is_none_or(|x| inside.has(m, x)) {
                        continue;
                    }
                    if let Some(p) = port_of(rev, kernels, &m.key, r).filter(|p| !feeds.contains(p)) {
                        feeds.push(p);
                    }
                }
            }
            let audio = |&(n, p): &(usize, usize)| match compiled.flat.nodes[n].outputs.get(p) {
                Some(&Kind::Audio(c)) => Some((n, p, c)),
                _ => None,
            };
            let all: Vec<(usize, usize, u8)> = feeds.iter().filter_map(audio).collect();
            // A sound its sidechain alone hears isn't its input; one it is keyed by and plays (a
            // compressor keyed by its own input) still is.
            let mine: Vec<(usize, usize, u8)> = all.iter().copied().filter(|&(n, p, _)| !side.contains(&(n, p))).collect();
            let sound = if mine.is_empty() { all } else { mine };
            match sound[..] {
                [] => Err(format!("{callee} takes no sound in, so it has no input to show.")),
                [(n, p, channels)] => Ok(Point { at: TapAt::Port(n, p), channels, unit: None }),
                _ => Err(format!("{callee} takes more than one sound in, so a look at its input can't tell which one to show.")),
            }
        }
        At::Meter(name) => {
            let inside = Inside::of(m, site.id).ok_or_else(|| twice(callee))?;
            let mut given = vec![];
            let mut all: Vec<&str> = vec![];
            for n in m.nodes.iter().filter(|n| inside.has(m, n)) {
                let Op::Kernel { kernel, .. } = &n.op else { continue };
                let Some(sig) = kernels.kernel(kernel) else { continue };
                all.extend(sig.meters.iter().map(|x| x.name.as_str()).filter(|x| !all.contains(x)).collect::<Vec<_>>());
                if let Some(k) = sig.meter_index(name) {
                    let unit = match sig.meters[k].kind {
                        PortKind::Control(Some(u)) => Some(u.to_string()),
                        _ => None,
                    };
                    given.push((format!("{}/{}", m.key, n.id), sig.outputs.len() + k, unit));
                }
            }
            match &given[..] {
                [] if all.is_empty() => Err(format!("{callee} gives no meters.")),
                [] if all.len() == 1 => Err(format!("{callee} gives no meter {name}: its meter is {}.", all[0])),
                [] => Err(format!("{callee} gives no meter {name}: its meters are {}.", all.join(", "))),
                [(flat, port, unit)] => {
                    let n = compiled.flat.nodes.iter().position(|x| &x.name == flat).ok_or_else(gone)?;
                    Ok(Point { at: TapAt::Port(n, *port), channels: 1, unit: unit.clone() })
                }
                _ => Err(format!("{callee} is made of more than one part that gives {name}, so a look can't tell which one to show.")),
            }
        }
    }
}

/// The nodes a call is made of: those made inside its frame (ID-15), or, for a call with no frame
/// (a kernel's own, a plain function's), those it made itself.
enum Inside {
    Frame(u32),
    Site(u32),
}

impl Inside {
    /// The call at site `site` in module `m`: None when it ran more than once there.
    fn of(m: &Module, site: u32) -> Option<Inside> {
        let frames: Vec<usize> = (0..m.frames.len()).filter(|&i| m.frames[i].site == site && m.frames[i].component.is_some()).collect();
        match frames[..] {
            [] => Some(Inside::Site(site)),
            [f] => Some(Inside::Frame(f as u32)),
            _ => None,
        }
    }

    /// Whether the call is turned off, `bypass: true` (Next D1).
    fn bypassed(&self, m: &Module) -> bool {
        matches!(*self, Inside::Frame(f) if m.frames.get(f as usize).is_some_and(|x| x.bypass))
    }

    /// Whether frame `frame` is the call's, or inside it.
    fn frame(&self, m: &Module, mut frame: Option<u32>) -> bool {
        let Inside::Frame(of) = *self else { return false };
        while let Some(i) = frame {
            if i == of {
                return true;
            }
            // A parent is always an earlier frame (the checker holds IRs to it), so this ends.
            frame = m.frames.get(i as usize).and_then(|f| f.parent).filter(|p| *p < i);
        }
        false
    }

    /// Whether node `n` is part of the call.
    fn has(&self, m: &Module, n: &Node) -> bool {
        match *self {
            Inside::Frame(_) => self.frame(m, n.frame),
            Inside::Site(site) => n.origin.as_ref().is_some_and(|o| o.site == site),
        }
    }
}

/// Why a look can't tell which call it is.
fn twice(callee: &str) -> String {
    format!("{callee} is called more than once, so a look can't tell which one to show.")
}

/// The flat node and the output that reference `r` of module `module` names in `rev`.
fn port_of(rev: &Revision, kernels: &dyn Kernels, module: &str, r: &str) -> Option<(usize, usize)> {
    let (module, r) = follow(rev, module, r)?;
    let (id, port) = split_ref(r);
    let compiled = &rev.compiled;
    let m = compiled.ir.modules.iter().find(|m| m.key == module)?;
    let n = m.nodes.iter().find(|n| n.id == id)?;
    let index = match &n.op {
        Op::Kernel { kernel, .. } => kernels.kernel(kernel).and_then(|k| k.output_index(port))?,
        Op::Plugin { plugin, .. } => kernels.plugin(plugin).and_then(|k| k.output_index(port))?,
        _ => 0,
    };
    let name = format!("{module}/{id}");
    Some((compiled.flat.nodes.iter().position(|n| n.name == name)?, index))
}

/// Reference `r` of `module`, an import followed to the export it names, in its own module.
fn follow<'a>(rev: &'a Revision, module: &'a str, r: &'a str) -> Option<(&'a str, &'a str)> {
    let (mut module, mut r) = (module, r);
    for _ in 0..32 {
        let m = rev.compiled.ir.modules.iter().find(|m| m.key == module)?;
        let n = m.nodes.iter().find(|n| n.id == split_ref(r).0)?;
        match &n.op {
            Op::Import { module: target, export } => {
                r = rev.compiled.ir.modules.iter().find(|m| &m.key == target).and_then(|m| m.exports.get(export))?;
                module = target;
            }
            _ => return Some((module, r)),
        }
    }
    None
}

/// The id `now`'s view gives the call `node` names in `was` (its view's id there): the same call,
/// as ID-13 says, by its file and the `(` of its arguments carried through the edits between the
/// two (`Edits::between`), wherever the edits around it moved it. None when that `(` was edited
/// away: the call is gone (TAP-9).
pub fn carry(was: &Revision, now: &Revision, node: &str) -> Option<String> {
    let (file, ordinal) = node.rsplit_once('#')?;
    let ordinal: u32 = ordinal.parse().ok()?;
    let site = was.compiled.sites.iter().find(|s| s.file == file && s.ordinal == ordinal)?;
    let path = std::path::Path::new(file);
    let (before, after) = (was.snapshot.files.get(path)?, now.snapshot.files.get(path)?);
    let open = site.open as usize;
    let to = Edits::between(before, after).carry(open..open + 1)?.start as u32;
    let found = now.compiled.sites.iter().find(|s| s.file == file && s.open == to)?;
    Some(format!("{}#{}", found.file, found.ordinal))
}

/// What output reference `r` of `module` is in `rev`'s flat graph: the node, when it is sound, its
/// channels, and the output's place among its node's. None when it isn't found.
fn resolve(rev: &Revision, kernels: &dyn Kernels, module: &str, r: &str) -> Option<(Option<TapAt>, u8, usize)> {
    let compiled = &rev.compiled;
    // An import is the export it names, in its own module.
    let (mut module, mut r) = (module, r);
    for _ in 0..32 {
        let (id, port) = split_ref(r);
        let m = compiled.ir.modules.iter().find(|m| m.key == module)?;
        let n = m.nodes.iter().find(|n| n.id == id)?;
        let index = match &n.op {
            Op::Import { module: target, export } => {
                r = compiled.ir.modules.iter().find(|m| &m.key == target).and_then(|m| m.exports.get(export))?;
                module = target;
                continue;
            }
            Op::Kernel { kernel, .. } => kernels.kernel(kernel).and_then(|k| k.output_index(port))?,
            Op::Plugin { plugin, .. } => kernels.plugin(plugin).and_then(|k| k.output_index(port))?,
            _ => 0,
        };
        let name = format!("{module}/{id}");
        let at = compiled.flat.nodes.iter().position(|n| n.name == name)?;
        let flat = &compiled.flat.nodes[at];
        return Some(match flat.outputs.get(index) {
            Some(&Kind::Audio(c)) if !matches!(flat.op, FOp::Const(_)) => (Some(TapAt::Port(at, 0)), c, index),
            _ => (None, 0, index),
        });
    }
    None
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;

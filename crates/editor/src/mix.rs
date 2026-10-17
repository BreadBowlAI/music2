//! The song's mixer and its channels (ID-18, PKG-6): where a track goes into the mix, as the track
//! builders edit it.
//!
//! The mixer is the `song` module's frame whose ComponentId is `std:mixer`, whatever it is
//! imported as (PKG-9). Its channels are the properties of the object its call is given, in the
//! project's code. A track is in the channel its audio reaches, by the graph (design-8b §2.3): from
//! each of its audio exports the song reads, downstream over audio edges, through nodes outside the
//! mixer (a duck, a compress), to the first node inside it. That node is the channel's panner or
//! its gain, and reads the channel's `pan` or `db` (`channels.kick.db`). Since 8b.3 every channel
//! has a gain (design-8b §5.1), so each has such a node. An edge into a sidechain input (the
//! duck's `kick.env`, PKG-13) only listens, so the walk doesn't take it.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;

use m2_ir::{FNode, FOp, Input, Kind, Module, Op, split_ref};
use m2_script::sites::{Arg, Site};
use m2_session::Revision;
use m2_song::ROOT;

use crate::sites::{Property, properties};

/// A read of another module's export inside the song: which, and where it is written.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportRead {
    /// The node in `song` that reads it.
    pub node: String,
    pub module: String,
    pub export: String,
    /// Its byte offset in the mixer's file: where the reading expression (`kick.out`) starts.
    pub at: u32,
    /// A sidechain input receives it: it only listens.
    pub sidechain: bool,
    /// The export is audio.
    pub audio: bool,
}

/// One channel: its key, its property's bytes, and the tracks whose audio it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub key: String,
    pub property: Property,
    /// Every read of another module's export inside the property.
    pub reads: Vec<ExportRead>,
    /// The tracks whose audio goes in here, by the graph: in the order the property reads them,
    /// then any it doesn't read by name (`const k = kick.out` above the mixer), in build order.
    pub tracks: Vec<String>,
}

/// The song's one mixer, written in a project file.
#[derive(Clone, Debug, PartialEq)]
pub struct Mixer {
    pub file: String,
    /// The mixer call's site and bytes.
    pub site: u32,
    pub call: (u32, u32),
    /// The channels object's bytes, braces included.
    pub object: (u32, u32),
    pub channels: Vec<Channel>,
}

/// What mixes the song.
#[derive(Clone, Debug, PartialEq)]
pub enum Mixing {
    One(Mixer),
    /// No `std:mixer` in the song.
    None,
    /// Several.
    Several(usize),
    /// One whose channels aren't an object written in the project's code.
    Unwritten,
}

impl Mixing {
    /// Why a builder can't add a channel here, in words.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Mixing::One(_) => None,
            Mixing::Several(n) => {
                Some(format!("This song has {n} mixers, so a new track would have no one place to go. Add it in the Code view."))
            }
            Mixing::None | Mixing::Unwritten => {
                Some("This song mixes its tracks its own way, so a new track would have nowhere to go. Add it in the Code view.".into())
            }
        }
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

/// Whether export `export` of `module` is audio, as the checker typed it; an export it can't find
/// counts as audio, so a channel is never missed.
fn audio(rev: &Revision, module: &str, export: &str) -> bool {
    let Some(m) = rev.compiled.ir.modules.iter().find(|m| m.key == module) else { return true };
    let Some(r) = m.exports.get(export) else { return true };
    let (node, _) = split_ref(r);
    match checked(&rev.compiled.flat.nodes, &format!("{module}/{node}")) {
        Some(n) if !n.outputs.is_empty() => n.outputs.iter().any(|k| matches!(k, Kind::Audio(_))),
        _ => true,
    }
}

/// Every node of `song` that reads another module's export, with where it is written in `file`.
pub fn reads(rev: &Revision, song: &Module, file: &str) -> Vec<ExportRead> {
    let side: Vec<&str> = song.frames.iter().flat_map(|f| &f.sidechain).map(|s| split_ref(&s.source).0).collect();
    song.nodes
        .iter()
        .filter_map(|n| {
            let Op::Import { module, export } = &n.op else { return None };
            let (f, at) = crate::view::locate(n.src.as_deref()?, &rev.snapshot.files)?;
            (f == file).then(|| ExportRead {
                node: n.id.clone(),
                module: module.clone(),
                export: export.clone(),
                at,
                sidechain: side.contains(&n.id.as_str()),
                audio: audio(rev, module, export),
            })
        })
        .collect()
}

/// The song's mixer in the accepted revision `rev`.
pub fn mixer(rev: &Revision) -> Mixing {
    let Some(song) = rev.compiled.ir.modules.iter().find(|m| m.key == ROOT) else { return Mixing::None };
    let mut sites: Vec<u32> = song.frames.iter().filter(|f| f.component.as_deref() == Some("std:mixer")).map(|f| f.site).collect();
    sites.dedup();
    let site = match sites[..] {
        [] => return Mixing::None,
        [one] => one,
        _ => return Mixing::Several(sites.len()),
    };
    // One call, evaluated once: a mixer made in a loop isn't one place.
    if song.frames.iter().filter(|f| f.site == site).count() != 1 {
        return Mixing::Several(song.frames.iter().filter(|f| f.site == site).count());
    }
    let frame = song.frames.iter().position(|f| f.site == site).expect("its frame") as u32;
    let Some(call) = rev.compiled.sites.iter().find(|s| s.id == site && s.in_project) else { return Mixing::Unwritten };
    let Some(text) = rev.snapshot.files.get(Path::new(&call.file)) else { return Mixing::Unwritten };
    let Some(Arg::Object { start, end, .. }) = call.args.first() else { return Mixing::Unwritten };
    let Some(props) = properties(text, (*start, *end)) else { return Mixing::Unwritten };
    let all = reads(rev, song, &call.file);
    let Reached { mut channels, unnamed } = reached(rev, song, frame);
    let built: Vec<&str> = rev.compiled.ir.modules.iter().filter(|m| m.track).map(|m| m.key.as_str()).collect();
    let channels = props
        .into_iter()
        .map(|p| {
            let reads: Vec<ExportRead> = all.iter().filter(|r| p.value.0 <= r.at && r.at < p.value.1).cloned().collect();
            let mut found = channels.remove(&p.key).unwrap_or_default();
            // A strip that reads none of its channel's controls (its `db` a signal, no `pan`): the
            // read's place says the channel.
            for r in reads.iter().filter(|r| r.audio && !r.sidechain && unnamed.contains(&r.module)) {
                found.insert(r.module.clone());
            }
            // In the order the property reads them, then the build's.
            let mut tracks: Vec<String> = vec![];
            for t in reads.iter().map(|r| r.module.as_str()).chain(built.iter().copied()) {
                if found.contains(t) && !tracks.iter().any(|x| x == t) {
                    tracks.push(t.to_string());
                }
            }
            Channel { key: p.key.clone(), property: p, reads, tracks }
        })
        .collect();
    Mixing::One(Mixer { file: call.file.clone(), site, call: (call.start, call.end), object: (*start, *end), channels })
}

/// Where the tracks' audio reaches the mixer.
struct Reached {
    /// By channel key, the tracks that reach it.
    channels: BTreeMap<String, BTreeSet<String>>,
    /// Tracks that reach a strip which reads none of its channel's controls.
    unnamed: BTreeSet<String>,
}

/// Follow each track's audio through the song to the mixer at `frame` of `song` (ID-18): from each
/// node of the track that the song reads, over edges whose output is audio and that don't go into
/// a sidechain input, through song nodes outside the mixer, to the first nodes inside it. A node
/// there names its channel by the channel control it reads: `channels.<key>.pan` on a panner,
/// `channels.<key>.db` on a gain. One that reads neither (a panner whose `pan` is a signal) passes
/// the walk on, inside the mixer, to the gain after it. Only the song's own nodes are walked: a
/// track read by another (a bus) reaches the mixer as that one.
fn reached(rev: &Revision, song: &Module, frame: u32) -> Reached {
    let flat = &rev.compiled.flat;
    let at: HashMap<&str, usize> = flat.nodes.iter().enumerate().map(|(i, n)| (n.name.as_str(), i)).collect();
    let ir: HashMap<&str, &m2_ir::Node> = song.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    // Whether the frame chain from `f` up holds `target`.
    let within = |mut f: Option<u32>, target: u32| {
        while let Some(x) = f {
            if x == target {
                return true;
            }
            f = song.frames.get(x as usize).and_then(|fr| fr.parent).filter(|p| *p < x);
        }
        false
    };
    let frame_of = |i: usize| -> Option<Option<u32>> {
        let n = &flat.nodes[i];
        let id = n.name.strip_prefix(ROOT).and_then(|x| x.strip_prefix('/'));
        (n.module == ROOT).then(|| id.and_then(|id| ir.get(id)).and_then(|n| n.frame))
    };
    let inside = |i: usize| frame_of(i).is_some_and(|f| within(f, frame));
    // The flat node an import of `module`'s `export` reads, through re-exports.
    let export = |module: &str, export: &str| -> Option<usize> {
        let (mut module, mut export) = (module.to_string(), export.to_string());
        for _ in 0..32 {
            let m = rev.compiled.ir.modules.iter().find(|m| m.key == module)?;
            let (id, _) = split_ref(m.exports.get(&export)?);
            match &m.nodes.iter().find(|n| n.id == id)?.op {
                Op::Import { module: to, export: e } => (module, export) = (to.clone(), e.clone()),
                _ => return at.get(format!("{module}/{id}").as_str()).copied(),
            }
        }
        None
    };
    // A song ref's flat node: an import's source, or the node itself.
    let source = |r: &str| -> Option<usize> {
        let (id, _) = split_ref(r);
        match &ir.get(id)?.op {
            Op::Import { module, export: e } => export(module, e),
            _ => at.get(format!("{ROOT}/{id}").as_str()).copied(),
        }
    };
    // What each frame's sidechain inputs received.
    let side: Vec<(u32, usize)> = song
        .frames
        .iter()
        .enumerate()
        .flat_map(|(f, fr)| fr.sidechain.iter().map(move |s| (f as u32, s.source.as_str())))
        .filter_map(|(f, r)| Some((f, source(r)?)))
        .collect();
    // The song's nodes that read each node, over edges from an audio output that aren't sidechains.
    let mut readers: Vec<Vec<usize>> = vec![vec![]; flat.nodes.len()];
    for (j, n) in flat.nodes.iter().enumerate() {
        let Some(f) = frame_of(j) else { continue };
        let srcs: Vec<m2_ir::PortRef> = match &n.op {
            FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. } => inputs
                .iter()
                .filter_map(|i| match i {
                    Input::Port { src, .. } => Some(*src),
                    _ => None,
                })
                .collect(),
            FOp::Feedback { input, .. } => vec![*input],
            _ => vec![],
        };
        for src in srcs {
            let audio = matches!(flat.nodes[src.node].outputs.get(src.port), Some(Kind::Audio(_)));
            let sidechain = side.iter().any(|(sf, s)| *s == src.node && within(f, *sf));
            if audio && !sidechain && !readers[src.node].contains(&j) {
                readers[src.node].push(j);
            }
        }
    }
    // The channel a node inside the mixer names by the channel control it reads.
    let names = |i: usize| -> Option<String> {
        let (FOp::Kernel { inputs, .. } | FOp::Plugin { inputs, .. }) = &flat.nodes[i].op else { return None };
        inputs.iter().find_map(|x| {
            let Input::Port { src, .. } = x else { return None };
            let c = &flat.nodes[src.node];
            let slot = c.origin.as_ref()?.control.as_ref().filter(|s| c.module == ROOT && s.frame == frame)?;
            let (key, control) = slot.slot.strip_prefix("channels.")?.rsplit_once('.')?;
            (matches!(c.op, FOp::Const(_)) && (control == "db" || control == "pan")).then(|| key.to_string())
        })
    };
    let mut out = Reached { channels: BTreeMap::new(), unnamed: BTreeSet::new() };
    for n in song.nodes.iter() {
        let Op::Import { module, export: e } = &n.op else { continue };
        if !rev.compiled.ir.modules.iter().any(|m| m.key == *module && m.track) {
            continue;
        }
        let Some(from) = export(module, e) else { continue };
        let mut seen: BTreeSet<usize> = BTreeSet::new();
        let mut queue: VecDeque<usize> = readers[from].iter().copied().collect();
        while let Some(i) = queue.pop_front() {
            if !seen.insert(i) {
                continue;
            }
            if !inside(i) {
                queue.extend(&readers[i]);
                continue;
            }
            match names(i) {
                Some(key) => drop(out.channels.entry(key).or_default().insert(module.clone())),
                None => {
                    let next: Vec<usize> = readers[i].iter().copied().filter(|&j| inside(j)).collect();
                    if next.is_empty() {
                        out.unnamed.insert(module.clone());
                    }
                    queue.extend(next);
                }
            }
        }
    }
    out
}

impl Mixer {
    /// The channel track `module` goes into, when it is exactly one.
    pub fn channel_of(&self, module: &str) -> Option<&Channel> {
        let mut found = self.channels.iter().filter(|c| c.tracks.iter().any(|t| t == module));
        let one = found.next()?;
        found.next().is_none().then_some(one)
    }

    /// The call, as the site list has it.
    pub fn site<'a>(&self, rev: &'a Revision) -> Option<&'a Site> {
        rev.compiled.sites.iter().find(|s| s.id == self.site)
    }
}

#[cfg(test)]
#[path = "mix_tests.rs"]
mod tests;

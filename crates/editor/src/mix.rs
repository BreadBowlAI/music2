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
//!
//! Here too: the Mix view's strips ([`view`], design-8b §5.4), and a channel's first pan
//! ([`insert`], §5.3).

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::Path;

use m2_ir::{FNode, FOp, Input, Kind, Module, Op, split_ref};
use m2_script::sites::{Arg, Site};
use m2_session::{Checked, Revision, Session};
use m2_song::ROOT;
use m2_song::tx::{Confined, Declared, Expect, Range};
use serde::Serialize;

use crate::controls::{controls, walk};
use crate::layout::LayoutView;
use crate::lens::{self, Insertion};
use crate::plan::{self, path, unmoved};
use crate::sites::{Property, properties};
use crate::view::LaneRef;
use crate::{EditError, Edited, ready};

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
    /// Its instance: an index into the song's `frames`.
    pub frame: u32,
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
    /// Why the levels are set in code, not in the Mix view, in words: when there isn't one mixer
    /// whose channels are written in the project's code.
    pub fn levels(&self) -> Option<String> {
        match self {
            Mixing::One(_) => None,
            Mixing::None => Some("This song doesn't use std's mixer: its levels are in code.".into()),
            Mixing::Several(n) => Some(format!("This song has {n} mixers: set its levels in code.")),
            Mixing::Unwritten => Some("This song's mixer isn't written in a way this view can change: set its levels in code.".into()),
        }
    }

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
    Mixing::One(Mixer { frame, file: call.file.clone(), site, call: (call.start, call.end), object: (*start, *end), channels })
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
            // A channel's own fader or pan; a send's level (`channels.vox.sends.verb.db`) names no channel here.
            let (key, control) = slot.slot.strip_prefix("channels.")?.split_once('.')?;
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

/// The Mix view (design-8b §5.4): a strip for each track in the track order, then one for each
/// channel no track reaches, and std's master.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MixView {
    /// `ok`; `none` when the song uses no std mixer, `several` when it has more than one, and
    /// `unwritten` when its channels aren't an object written in the project's code.
    pub mixer: &'static str,
    /// Why the levels are set in code, when the mixer isn't `ok`: what the view says.
    pub message: Option<String>,
    pub strips: Vec<Strip>,
    /// std's master, when the song has exactly one.
    pub master: Option<MasterStrip>,
}

/// One strip: a channel and the tracks it plays, or a track with no channel of its own.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Strip {
    /// The tracks it plays, by key, in the order its channel reads them; none for a channel no
    /// track reaches.
    pub modules: Vec<String>,
    /// Its channel's key, when it has one.
    pub channel: Option<String>,
    /// What it is called: its tracks' names ("Kick + Snare"), else its channel's key.
    pub label: String,
    /// Its first track's colour by name, else graphite.
    pub colour: String,
    /// The fader's knob id, as the graph view names it: none when code sets the level (a signal).
    pub db: Option<String>,
    /// The pan's knob id, when the channel writes a number there.
    pub pan: Option<String>,
    /// The channel writes no pan, and its first pan can write one (`/api/graph/mix/insert`).
    pub pan_insert: bool,
    /// Why a track has no fader: "Not in the mixer", or "In 2 channels: set its level in code".
    pub why: Option<String>,
    /// The automation lane its level follows, when one is read there (taste.md §2.10): the fader
    /// shows it, read only, with Show lane and Remove automation (KN-2).
    pub db_lane: Option<LaneRef>,
    /// The automation lane its pan follows, likewise.
    pub pan_lane: Option<LaneRef>,
    /// It is a bus's strip (RENDER-34): these come after the tracks'.
    pub bus: bool,
    /// Its sends to bus channels, as its entry writes them.
    pub sends: Vec<StripSend>,
    /// The bus channel it goes to instead of the master; none for the master.
    pub output: Option<String>,
    /// The bus channels it may send to or go to without a loop, by key, in the mixer's order.
    pub may_use: Vec<String>,
}

/// A send to a bus channel, on a strip.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StripSend {
    /// The bus channel's key.
    pub bus: String,
    /// Its level's knob id (`channels.vox.sends.verb.db`): none when code sets it (a signal).
    pub knob: Option<String>,
    /// Before the fader, or after it.
    pub pre: bool,
    /// The automation lane its level follows, when one is read there.
    pub lane: Option<LaneRef>,
}

/// std's master: the knob ids of its fader and its ceiling.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MasterStrip {
    pub db: Option<String>,
    pub ceiling: Option<String>,
    /// The automation lane its level follows, when one is read there.
    pub db_lane: Option<LaneRef>,
}

/// The automation lane read as `arg`, a mixer or master argument written in `file`: a
/// `lane.control("hats-volume", …)` call written there, of a control lane the song declares.
fn lane_read(rev: &Revision, song: &Module, file: &str, arg: Option<&Arg>) -> Option<LaneRef> {
    let (start, end) = arg?.span();
    let text = rev.snapshot.files.get(Path::new(file))?;
    let read = rev.compiled.sites.iter().find(|x| x.file == file && x.start == start && x.end == end && x.callee.ends_with(".control"))?;
    let (a, b) = read.args.first()?.span();
    let quoted = text.get(a as usize..b as usize)?.trim();
    let key = ['"', '\''].iter().find_map(|q| quoted.strip_prefix(*q)?.strip_suffix(*q))?;
    let declared = song.lanes.iter().any(|l| l.key == key && matches!(l.kind, Kind::Control(_)));
    declared.then(|| LaneRef { module: song.key.clone(), lane: key.to_string() })
}

/// The knob id of each declared control of the song's frames `frames`, by frame and slot.
fn knobs(s: &Session, song: &Module, frames: &[u32]) -> BTreeMap<(u32, String), String> {
    let all = controls(s);
    let mut out = BTreeMap::new();
    walk(&song.nodes, &mut |n| {
        let (Op::Const { .. }, Some(o)) = (&n.op, &n.origin) else { return };
        let Some(slot) = o.control.as_ref().filter(|c| frames.contains(&c.frame)) else { return };
        if let Some(c) = all.iter().find(|c| c.origin.same_value(o)) {
            out.insert((slot.frame, slot.slot.clone()), c.id.clone());
        }
    });
    out
}

/// The Mix view of the accepted revision, with the tracks in `layout`'s order and colours.
pub fn view(s: &Session, layout: &LayoutView) -> MixView {
    let rev = s.accepted();
    let Some(song) = rev.compiled.ir.modules.iter().find(|m| m.key == ROOT) else {
        return MixView { mixer: "none", message: None, strips: vec![], master: None };
    };
    let label = |k: &str| rev.compiled.ir.modules.iter().find(|m| m.key == k).map_or(k, |m| m.label()).to_string();
    let colour = |k: &str| layout.colours.get(k).map_or("graphite", |c| c).to_string();
    let masters: Vec<u32> =
        (0..song.frames.len() as u32).filter(|&f| song.frames[f as usize].component.as_deref() == Some("std:master")).collect();
    let mixing = mixer(rev);
    let mut frames = masters.clone();
    if let Mixing::One(m) = &mixing {
        frames.push(m.frame);
    }
    let ids = knobs(s, song, &frames);
    let master = match masters[..] {
        [f] => {
            let db = ids.get(&(f, "db".into())).cloned();
            let frame = &song.frames[f as usize];
            let call = rev.compiled.sites.iter().find(|x| x.id == frame.site && x.in_project);
            let options = call.zip(frame.options).and_then(|(c, o)| Some((c, c.args.get(o as usize)?)));
            let db_lane = db.is_none().then(|| options.and_then(|(c, o)| lane_read(rev, song, &c.file, o.at(&["db".to_string()])))).flatten();
            Some(MasterStrip { db, ceiling: ids.get(&(f, "ceiling".into())).cloned(), db_lane })
        }
        _ => None,
    };
    let is_bus = |k: &str| rev.compiled.ir.modules.iter().any(|m| m.key == k && m.bus);
    let alone = |k: &str, why: Option<String>| Strip {
        modules: vec![k.to_string()],
        channel: None,
        label: label(k),
        colour: colour(k),
        db: None,
        pan: None,
        pan_insert: false,
        why,
        db_lane: None,
        pan_lane: None,
        bus: is_bus(k),
        sends: vec![],
        output: None,
        may_use: vec![],
    };
    let word = match &mixing {
        Mixing::One(_) => "ok",
        Mixing::None => "none",
        Mixing::Several(_) => "several",
        Mixing::Unwritten => "unwritten",
    };
    let message = mixing.levels();
    let Mixing::One(m) = &mixing else {
        let (buses, mut strips): (Vec<Strip>, Vec<Strip>) = layout.order.iter().map(|k| alone(k, None)).partition(|x| x.bus);
        strips.extend(buses);
        return MixView { mixer: word, message, strips, master };
    };
    let buses = crate::bus::bus_channels(rev, m);
    let routes = crate::bus::routing(rev, m);
    let feeds = crate::bus::feeds(rev);
    let channels = m.site(rev).and_then(|x| x.args.first());
    let lane = |c: &Channel, control: &str| lane_read(rev, song, &m.file, channels.and_then(|o| o.at(&[c.key.clone(), control.to_string()])));
    let strip = |c: &Channel| {
        let db = ids.get(&(m.frame, format!("channels.{}.db", c.key))).cloned();
        let pan = ids.get(&(m.frame, format!("channels.{}.pan", c.key))).cloned();
        let db_lane = db.is_none().then(|| lane(c, "db")).flatten();
        let pan_lane = pan.is_none().then(|| lane(c, "pan")).flatten();
        let pan_insert = pan.is_none() && planned(rev, m, &c.key, "pan").is_ok();
        let label = match c.tracks.is_empty() {
            true => c.key.clone(),
            false => c.tracks.iter().map(|t| label(t)).collect::<Vec<_>>().join(" + "),
        };
        let colour = c.tracks.first().map_or_else(|| "graphite".to_string(), |t| colour(t));
        let route = routes.get(&c.key).cloned().unwrap_or_default();
        let sends = route
            .sends
            .iter()
            .map(|(b, pre)| {
                let knob = ids.get(&(m.frame, format!("channels.{}.sends.{b}.db", c.key))).cloned();
                // A bare send is its level; an object's is its `db`.
                let written = channels.and_then(|o| o.at(&[c.key.clone(), "sends".into(), b.clone()]));
                let level = match written {
                    Some(Arg::Object { .. }) => written.and_then(|x| x.at(&["db".to_string()])),
                    other => other,
                };
                let lane = knob.is_none().then(|| lane_read(rev, song, &m.file, level)).flatten();
                StripSend { bus: b.clone(), knob, pre: *pre, lane }
            })
            .collect();
        let may_use = m
            .channels
            .iter()
            .filter_map(|x| buses.get(&x.key).map(|b| (&x.key, b)))
            .filter(|(_, b)| crate::bus::looped(rev, &feeds, c, b, false).is_none())
            .map(|(k, _)| k.clone())
            .collect();
        Strip {
            modules: c.tracks.clone(),
            channel: Some(c.key.clone()),
            label,
            colour,
            db,
            pan,
            pan_insert,
            why: None,
            db_lane,
            pan_lane,
            bus: buses.contains_key(&c.key),
            sends,
            output: route.to,
            may_use,
        }
    };
    // The tracks' strips, then the channels no track reaches, then the buses' (RENDER-34).
    let (mut strips, mut later) = (vec![], vec![]);
    let mut shown: BTreeSet<&str> = BTreeSet::new();
    for k in &layout.order {
        let reached: Vec<&Channel> = m.channels.iter().filter(|c| c.tracks.contains(k)).collect();
        let x = match reached[..] {
            [c] if shown.insert(&c.key) => strip(c),
            [_] => continue,
            [] => alone(k, Some("Not in the mixer".into())),
            _ => alone(k, Some(format!("In {} channels: set its level in code", reached.len()))),
        };
        if x.bus { later.push(x) } else { strips.push(x) }
    }
    strips.extend(m.channels.iter().filter(|c| c.tracks.is_empty()).map(strip));
    strips.extend(later);
    MixView { mixer: word, message, strips, master }
}

/// Plan writing optional control `control` (`pan`) into channel `key`'s entry of mixer `m`.
pub(crate) fn planned(rev: &Revision, m: &Mixer, key: &str, control: &str) -> Result<Insertion, String> {
    let song = rev.compiled.ir.modules.iter().find(|x| x.key == ROOT).ok_or("the song has no code")?;
    let frame = &song.frames[m.frame as usize];
    let desc = frame.component.as_deref().and_then(|c| rev.compiled.components.get(c)).ok_or("the mixer has no descriptor")?;
    let site = m.site(rev).ok_or("the mixer's call is gone")?;
    let text = rev.snapshot.files.get(Path::new(&m.file)).ok_or("the mixer's file is gone")?;
    let slot = format!("channels.{key}.{control}");
    let declared = desc.control(&slot).ok_or_else(|| format!("a channel has no {control}"))?;
    if !declared.optional {
        return Err(format!("a channel always has its {control}, so turning it writes it"));
    }
    let options = frame.options.map(|o| o as usize);
    Insertion::plan(site, desc, &slot, text, options, &rev.compiled.units).map_err(str::to_string)
}

/// A channel's first pan, written: the edit, and the new pan's knob id.
#[derive(Clone, Debug, PartialEq)]
pub struct Inserted {
    pub edited: Edited,
    pub control: String,
}

/// Each channel control of the mixer at song frame `frame`, as built: `(key, control)` → its
/// value and the kernels that read it, by id.
fn strips(rev: &Revision, frame: u32) -> BTreeMap<(String, String), (f64, Vec<String>)> {
    let flat = &rev.compiled.flat;
    let mut at: HashMap<usize, (String, String)> = HashMap::new();
    let mut out: BTreeMap<(String, String), (f64, Vec<String>)> = BTreeMap::new();
    for (i, n) in flat.nodes.iter().enumerate() {
        let FOp::Const(v) = n.op else { continue };
        let Some(slot) = n.origin.as_ref().and_then(|o| o.control.as_ref()).filter(|c| n.module == ROOT && c.frame == frame) else {
            continue;
        };
        // A channel's own controls; a send's (`channels.vox.sends.verb.db`) waits for the Mix view's sends.
        let Some((key, control)) = slot.slot.strip_prefix("channels.").and_then(|r| r.split_once('.')).filter(|(_, c)| !c.contains('.')) else {
            continue;
        };
        at.insert(i, (key.to_string(), control.to_string()));
        out.insert((key.to_string(), control.to_string()), (v, vec![]));
    }
    for n in &flat.nodes {
        let FOp::Kernel { kernel, inputs, .. } = &n.op else { continue };
        for x in inputs {
            if let Input::Port { src, .. } = x
                && let Some(k) = at.get(&src.node)
            {
                out.get_mut(k).expect("listed").1.push(kernel.clone());
            }
        }
    }
    out.values_mut().for_each(|v| v.1.sort());
    out
}

/// Write optional control `control` (a channel's first `pan`) of channel `key` of the song's mixer
/// as `value` (design-8b §5.3): into its entry, wrapping a bare one (`kick: { in: kick.out, pan: 0 }`).
/// One `Confined` Tx with the mixer's whole call as its range: the new panner is std's, made inside
/// the call, and has no other place in the project. The planner's assertion is by channel, through
/// [`mixer`] (lead decision T4): a panner's name counts the panned channels before it (ID-14), so a
/// pan on the first channel renames every later one's. Every channel keeps its tracks and its
/// controls' values and readers, and this one gains only the control, read by its new node: the
/// graph gains that constant and that node, and nothing else.
pub fn insert(s: &mut Session, key: &str, control: &str, value: f64) -> Result<Inserted, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let m = match mixer(rev) {
        Mixing::One(m) => m,
        other => return Err(EditError::Rejected(other.levels().unwrap_or_default())),
    };
    let channel = m.channels.iter().find(|c| c.key == key).ok_or_else(|| EditError::NotFound(format!("channel {key}")))?;
    let name = match channel.tracks.as_slice() {
        [] => key.to_string(),
        tracks => tracks
            .iter()
            .map(|t| rev.compiled.ir.modules.iter().find(|x| x.key == *t).map_or(t.as_str(), |x| x.label()))
            .collect::<Vec<_>>()
            .join(" + "),
    };
    let insertion =
        planned(rev, &m, key, control).map_err(|why| EditError::Rejected(format!("Can't add a {control} to {name}: {why}.")))?;
    let edit = lens::propose(value, 1.0)?;
    let edits = insertion.edits(&insertion.number(&edit.text));
    let old = rev.snapshot.files[Path::new(&m.file)].clone();
    let entry = insertion.entry.as_ref().expect("a dictionary entry's");
    let label = crate::label::shown_entry(&old, entry, &edits);
    let file = path(&m.file)?;
    let op = plan::text_edit(&file, &old, &edits);
    let expect = Expect::Confined(Confined {
        ranges: [(file, Declared::Ranges(vec![Range::bytes(m.call.0, m.call.1)]))].into(),
        ..Confined::default()
    });
    let tx = plan::planned(s, label.clone(), vec![op], expect);
    let before = (channels(&m), strips(s.accepted(), m.frame));
    let said = |why: &str| EditError::Rejected(format!("adding a {control} to {name} would change more than its strip ({why})"));
    let (k, c) = (key.to_string(), control.to_string());
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        one_more(checked, &before, &k, &c, edit.value)
            .map_err(|why| EditError::Rejected(format!("adding a {control} to {name} would {why}")))
    })?;
    let id = match mixer(s.accepted()) {
        Mixing::One(now) => {
            let song = s.accepted().compiled.ir.modules.iter().find(|x| x.key == ROOT).expect("a song");
            knobs(s, song, &[now.frame]).remove(&(now.frame, format!("channels.{key}.{control}")))
        }
        _ => None,
    };
    let id = id.ok_or_else(|| EditError::Rejected(format!("{name}'s new {control} is written, but isn't a knob")))?;
    let edited = Edited { revision: done.number, file: m.file.clone(), before: label.0, after: label.1 };
    Ok(Inserted { edited, control: id })
}

/// Each channel's key and tracks.
fn channels(m: &Mixer) -> Vec<(String, Vec<String>)> {
    m.channels.iter().map(|c| (c.key.clone(), c.tracks.clone())).collect()
}

/// Each channel's tracks, and each channel control's value and readers.
type Before = (Vec<(String, Vec<String>)>, BTreeMap<(String, String), (f64, Vec<String>)>);

/// The first pan's assertion on the checked candidate: as `before` with only `key`'s `control`
/// added, at `value` and read by its own new nodes; the delta adds that constant and those nodes,
/// net, and changes no value, module or data.
fn one_more(checked: &Checked, before: &Before, key: &str, control: &str, value: f64) -> Result<(), String> {
    let d = &checked.delta;
    if !(d.values.is_empty() && d.modules.is_empty() && d.lanes.is_empty() && d.states.is_empty() && d.assets.is_empty() && !d.arrangement)
    {
        return Err("change a value or the song's data too".into());
    }
    let Mixing::One(m) = mixer(checked.candidate()) else { return Err("lose the mixer".into()) };
    if channels(&m) != before.0 {
        return Err("move a track to another channel".into());
    }
    let mut after = strips(checked.candidate(), m.frame);
    let Some((v, readers)) = after.remove(&(key.to_string(), control.to_string())) else {
        return Err(format!("not give the channel a {control}"));
    };
    if v != value || readers.is_empty() {
        return Err(format!("give the channel a {control} of {v} read by {readers:?}"));
    }
    if after != before.1 {
        return Err("change another channel's strip".into());
    }
    // Net, by what each node is: the constant and its readers, nothing else.
    let mut net: BTreeMap<String, i64> = BTreeMap::new();
    for n in &d.added {
        *net.entry(n.op.to_string()).or_default() += 1;
    }
    for n in &d.removed {
        *net.entry(n.op.to_string()).or_default() -= 1;
    }
    net.retain(|_, n| *n != 0);
    let mut want: BTreeMap<String, i64> = BTreeMap::new();
    let named = format!("@channels.{key}.{control}");
    let Some(made) = d.added.iter().find(|n| n.name.contains(&named)) else { return Err(format!("not make the {control}'s constant")) };
    *want.entry(made.op.to_string()).or_default() += 1;
    for r in &readers {
        *want.entry(serde_json::json!({"op": "kernel", "kernel": r}).to_string()).or_default() += 1;
    }
    match net == want {
        true => Ok(()),
        false => Err(format!("add or take out more than the {control} ({net:?})")),
    }
}

#[cfg(test)]
#[path = "mix_tests.rs"]
mod tests;

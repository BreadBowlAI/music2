//! The song's mixer and its channels (ID-18, PKG-6): where a track goes into the mix, as the track
//! builders edit it.
//!
//! The mixer is the `song` module's frame whose ComponentId is `std:mixer`, whatever it is
//! imported as (PKG-9). Its channels are the properties of the object its call is given, in the
//! project's code. A track is in a channel when an audio export of it is read inside that
//! channel's property: the build's import nodes say where each read is, and a read a frame records
//! as a sidechain input (the duck's `kick.env`, PKG-13) only listens, so it isn't the track's way
//! into the mix. Following the graph alone can't tell a bare `kick: kick.out` from its neighbours
//! until every channel has a gain (design-8b §5.1), so the read's place says which channel it is.

use std::path::Path;

use m2_ir::{FNode, FOp, Kind, Module, Op, split_ref};
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
    /// The tracks whose audio goes in here.
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
    let Some(call) = rev.compiled.sites.iter().find(|s| s.id == site && s.in_project) else { return Mixing::Unwritten };
    let Some(text) = rev.snapshot.files.get(Path::new(&call.file)) else { return Mixing::Unwritten };
    let Some(Arg::Object { start, end, .. }) = call.args.first() else { return Mixing::Unwritten };
    let Some(props) = properties(text, (*start, *end)) else { return Mixing::Unwritten };
    let all = reads(rev, song, &call.file);
    let channels = props
        .into_iter()
        .map(|p| {
            let reads: Vec<ExportRead> = all.iter().filter(|r| p.value.0 <= r.at && r.at < p.value.1).cloned().collect();
            let mut tracks: Vec<String> = vec![];
            for r in reads.iter().filter(|r| r.audio && !r.sidechain) {
                if !tracks.contains(&r.module) {
                    tracks.push(r.module.clone());
                }
            }
            Channel { key: p.key.clone(), property: p, reads, tracks }
        })
        .collect();
    Mixing::One(Mixer { file: call.file.clone(), site, call: (call.start, call.end), object: (*start, *end), channels })
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

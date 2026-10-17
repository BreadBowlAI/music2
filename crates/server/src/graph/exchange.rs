//! Files from and for other music apps (studio lane C): the song or one clip as a MIDI file's bytes
//! ([`m2_editor::midifile`]), and its stems' renders, each made on the actor from the accepted
//! revision, as every view is; files are written off the actor, by the route or by `crate::stems`'
//! thread. And a MIDI file brought in: read off the actor, then one actor job.

use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_editor::midifile::ClipAt;
use m2_editor::render::{Length, Render, Threads};
use m2_ir::{FOp, Flat};
use m2_session::Revision;
use serde_json::{Value, json};

use super::Graph;

impl Graph {
    /// The open song as a MIDI file, or `clip` alone, and the name its file takes: the song's
    /// ("first-song"), or the song's and the clip's track's ("first-song Bass"). Reads only.
    pub fn midi_file(&self, token: u64, clip: Option<ClipAt>) -> Result<(Vec<u8>, String)> {
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            let song = o.session.accepted().project.manifest.project.name.clone();
            let plain = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            match clip {
                Some(c) => {
                    let (bytes, track) = m2_editor::midifile::export_clip(&o.session, &c.module, &c.lane, &c.id).map_err(plain)?;
                    Ok((bytes, format!("{song} {track}")))
                }
                None => Ok((m2_editor::midifile::export_song(&o.session).map_err(plain)?, song)),
            }
        })
    }
}

/// The stems of the accepted revision (C3, Q39): each track's own output, in the tracks' order,
/// tracks muted in the song left out, checked as graphs of their own; rendered one at a time
/// ([`Graph::stem_render`]), all from the revision they were planned on.
pub struct StemsPlan {
    pub song: String,
    /// Each stem's track label and graph.
    pub stems: Vec<(String, Flat)>,
    rev: Arc<Revision>,
    token: u64,
}

/// Why a song's stems can't be made now, in words, or None: a song that plays plugins (Q40).
fn stems_refused(rev: &Revision, packages: &m2_runtime::PackageSet) -> Option<String> {
    let mut names: Vec<String> = vec![];
    for n in &rev.compiled.flat.nodes {
        if let FOp::Plugin { plugin, .. } = &n.op {
            // One that isn't installed by its id.
            let id = || plugin.strip_prefix("clap:").unwrap_or(plugin).to_string();
            let name = packages.plugin_info(plugin).map_or_else(id, |i| i.found.name.clone());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let listed = match names.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    Some(format!("Not yet for a song that plays plugins ({listed})."))
}

impl Graph {
    /// Why the open song's stems can't be made now, in words, or None (the Export dialog greys its
    /// Stems row with it). Reads only.
    pub fn stems_refused(&self, token: u64) -> Result<Option<String>> {
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            Ok(stems_refused(o.session.accepted(), &o.session.packages))
        })
    }

    /// The open song's stems, planned on the accepted revision (see [`StemsPlan`]). Refused in
    /// words for a song that plays plugins, and one with no track to export.
    pub fn stems(&self, token: u64) -> Result<StemsPlan> {
        self.call(move |a| {
            let o = a.open.as_ref().ok_or_else(|| anyhow!("no project is open"))?;
            if o.token != token {
                return Err(anyhow!("the open project changed; refresh the graph before editing"));
            }
            let rev = o.session.accepted_arc();
            if let Some(why) = stems_refused(&rev, &o.session.packages) {
                bail!("{why}");
            }
            let modules = &rev.compiled.ir.modules;
            let plays = |key: &str| modules.iter().any(|m| m.key == key && !m.muted && m.exports.contains_key("out"));
            let mut stems = vec![];
            for t in m2_editor::timeline::timeline(&o.session).tracks.into_iter().filter(|t| t.track && plays(&t.module)) {
                let ir = m2_editor::render::rooted(&rev.compiled.ir, &t.module, "out")?;
                let flat = m2_ir::check(&ir, &o.session.packages).map_err(|d| {
                    let why = d.first().map_or_else(String::new, |d| format!(": {}", d.message));
                    anyhow!("{}'s stem doesn't build{why}", t.label)
                })?;
                stems.push((t.label, flat));
            }
            if stems.is_empty() {
                bail!("The song has no tracks to export as stems: each is muted in the song.");
            }
            Ok(StemsPlan { song: rev.project.manifest.project.name.clone(), stems, rev, token })
        })
    }

    /// Stem `i` of `plan`, ready to render: as `m2 bounce --track` renders it, from 0 to 2 s past
    /// the song's last clip, on an export's threads. Only building it waits for the session.
    pub fn stem_render(&self, plan: &StemsPlan, i: usize) -> Result<Render> {
        let (rev, flat, token) = (plan.rev.clone(), plan.stems[i].1.clone(), plan.token);
        let idle = self.inner.current_device().is_none_or(|d| d.player.idle());
        let threads = crate::export::export_threads(Threads::from_env()?, idle, false);
        self.call(move |a| {
            let o = a.open.as_ref().filter(|o| o.token == token).ok_or_else(|| anyhow!("the song was closed"))?;
            let data = &rev.compiled.data;
            let seconds = m2_editor::render::bounce_seconds(data);
            // Offline renders run at the project's rate (RENDER-31a).
            Render::new(&flat, data, &o.session.packages, rev.project.sample_rate(), 0.0, Length::Seconds(seconds), threads)
                .map_err(super::plainly)
        })
    }
}

/// A MIDI file to bring in: its path on this computer (the app), or its bytes (a browser), with
/// its name.
pub enum MidiSource {
    Path(std::path::PathBuf),
    Bytes { name: String, bytes: Vec<u8> },
}

/// Where a MIDI file goes (Q36): into a notes lane at a beat (a one-part file dropped on a notes
/// track's row, D11).
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum MidiTarget {
    Lane { module: String, lane: String, beat: f64 },
}

/// A MIDI file's name and bytes, read: refused in words over 16 MB (D10), before reading it whole.
fn midi_bytes(source: MidiSource) -> Result<(String, Vec<u8>)> {
    let (name, bytes) = match source {
        MidiSource::Path(p) => {
            let name = p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
            let size = std::fs::metadata(&p).map_err(|e| anyhow!("{name} can't be read: {e}"))?.len();
            if size > m2_editor::smf::MAX_BYTES as u64 {
                bail!("{name} is over 16 MB: music2 brings in MIDI files up to 16 MB.");
            }
            let bytes = std::fs::read(&p).map_err(|e| anyhow!("{name} can't be read: {e}"))?;
            (name, bytes)
        }
        MidiSource::Bytes { name, bytes } => (name, bytes),
    };
    Ok((name, bytes))
}

impl Graph {
    /// Bring a MIDI file into the open song at `target` (Q36, D11): read on the calling thread
    /// (refused in words when it isn't one music2 reads), then one actor job, one undo step named
    /// "importing groove.mid" on the server. Answers the edit with `made` (the clip, to select),
    /// `name` and `undo_name`.
    pub fn import_midi(&self, token: u64, source: MidiSource, target: MidiTarget) -> Result<Value> {
        let (name, bytes) = midi_bytes(source)?;
        let file = m2_editor::smf::read(&bytes).map_err(|e| anyhow!("{name} {e}"))?;
        let step = format!("importing {name}");
        let mut done = self.call(move |a| {
            a.edit_and(token, |s| match &target {
                MidiTarget::Lane { module, lane, beat } => {
                    let done = m2_editor::midifile::import_into(s, &file, &name, module, lane, *beat)?;
                    let made: Vec<Value> =
                        done.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                    Ok((done.edited, json!({"made": made, "tracks": []})))
                }
            })
        })?;
        if let Some(revision) = done["revision"].as_u64() {
            let named = self.name_step(token, revision, step.clone(), false)?;
            done["undo_name"] = named["undo_name"].clone();
        }
        done["name"] = json!(step);
        Ok(done)
    }
}

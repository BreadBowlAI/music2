//! Audio files in the open song (lane A): where an asset's file is, for its waveform's peaks
//! (A1); and bringing files in (A2): copied into `assets/` on the calling thread, their peaks made
//! from that one decode, then placed by one actor job as one undo step, named on the server.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use m2_editor::import::{self, Imported, Source, Target};
use m2_song::AssetName;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Graph;
use crate::peaks::{Peaks, Store};

/// One import at a time in this server: two never share a part file (D2).
static IMPORTING: Mutex<()> = Mutex::new(());

/// What an import of `files` is called as an undo step (Q16).
pub fn step_name(files: &[Imported]) -> String {
    match files {
        [one] => format!("importing {}", one.name),
        many => format!("importing {} files", many.len()),
    }
}

impl Graph {
    /// The file asset `name` of the open song (session `token`) is: under its `assets/`. Reads the
    /// control plane only, never the session, so it never waits for a compile.
    pub fn asset_file(&self, token: u64, name: &str) -> Result<PathBuf> {
        let name = AssetName::new(name).map_err(anyhow::Error::msg)?;
        let cp = self.inner.control.lock();
        let p = cp.open.as_ref().context("no project is open")?;
        if p.engine.token != token {
            bail!("the open project changed; refresh the graph");
        }
        Ok(p.shown.dir.join(m2_song::path::ASSETS).join(name.as_str()))
    }

    /// The open song's folder, once what an import of `target` needs is known to be there: the song
    /// is `token`'s and writable, the lane is an audio lane, and a new track can be added (the code
    /// compiles). Checked before anything is copied (D16).
    fn import_dir(&self, token: u64, target: &Target) -> Result<PathBuf> {
        let cp = self.inner.control.lock();
        let p = cp.open.as_ref().context("no project is open")?;
        if p.engine.token != token {
            bail!("the open project changed; refresh the graph before editing");
        }
        if let Some(why) = &p.shown.read_only {
            bail!("{why}");
        }
        let compiles = p.shown.state["view"]["draft_errors"].as_array().is_none_or(Vec::is_empty);
        match target {
            Target::Lane { module, lane, .. } => {
                let l = p.shown.timeline.lane(module, lane).with_context(|| format!("There's no lane `{lane}` on `{module}` now."))?;
                if l.json["kind"] != "audio" {
                    bail!("That lane plays notes or automation: drop audio on an audio track.");
                }
                if !compiles {
                    bail!("The code doesn't compile, so nothing can be added now. Fix the code first.");
                }
            }
            Target::Track { .. } | Target::Below { .. } if !compiles => {
                bail!("The code doesn't compile, so no track can be added now. Drop it on an audio track, or fix the code first.")
            }
            _ => {}
        }
        Ok(p.shown.dir.clone())
    }

    /// Bring `sources` into the open song and place them at `target` (Q6, Q7): each copied into
    /// `assets/` (or the same bytes there used again) and its peaks made, here; then one actor job
    /// places their clips (one Tx, one undo step named "importing vox.wav"). Answers the edit with
    /// `made` (the clips, to select), `tracks` (any added) and `notes` (what to tell: a file whose
    /// extra channels don't play). A file refused says why: nothing is added to the song, and any
    /// file copied before it stays in `assets/` (D16).
    pub fn import_audio(&self, token: u64, sources: Vec<Source>, target: Target) -> Result<Value> {
        if sources.is_empty() {
            bail!("No file to bring in.");
        }
        let dir = self.import_dir(token, &target)?;
        let files = {
            let _one = IMPORTING.lock();
            let mut files = vec![];
            for src in sources {
                let mut peaks = Peaks::builder();
                let f = import::audio(&dir, src, &mut |piece| peaks.add(piece)).map_err(|e| anyhow::anyhow!("{}", words(&e)))?;
                // The peaks from the decode the import made (D31); a file used again has its own.
                match f.reused {
                    false => drop(Store::global().put(f.hash, peaks.done(f.rate))),
                    true => drop(Store::global().of(&dir.join(m2_song::path::ASSETS).join(f.asset.as_str()))),
                }
                files.push(f);
            }
            files
        };
        let name = step_name(&files);
        let structural = !matches!(target, Target::Lane { .. });
        let notes: Vec<String> = files
            .iter()
            .filter(|f| f.dropped_channels > 0)
            .map(|f| format!("{} has {} channels: music2 plays the first two.", f.name, f.dropped_channels + 2))
            .collect();
        let placing = files.clone();
        let mut done = self.call(move |a| {
            a.edit_and(token, |s| {
                let p = import::place(s, &placing, &target)?;
                let made: Vec<Value> =
                    p.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                let tracks: Vec<Value> = p.tracks.iter().map(|(m, l)| json!({"module": m, "label": l})).collect();
                Ok((p.edited, json!({"made": made, "tracks": tracks})))
            })
        })?;
        if let Some(revision) = done["revision"].as_u64() {
            let named = self.name_step(token, revision, name.clone(), structural)?;
            done["undo_name"] = named["undo_name"].clone();
        }
        // A new track under a key a gone track's mute or solo was kept for starts heard (Q7).
        let keys: Vec<String> =
            done["tracks"].as_array().into_iter().flatten().filter_map(|t| t["module"].as_str().map(String::from)).collect();
        if !keys.is_empty() {
            self.forget_overlays(&keys);
        }
        done["name"] = json!(name);
        done["notes"] = json!(notes);
        done["assets"] = json!(files.iter().map(|f| f.asset.as_str()).collect::<Vec<_>>());
        Ok(done)
    }
}

impl Graph {
    /// A WAV dropped on a sampler's card (track ab): `source` is copied into the open song's
    /// `assets/` (or the same bytes there used again), here, then one actor job writes its name into
    /// sampler call `call` of revision `at` (one Tx, one undo step, "setting Sampler to kick.wav").
    /// Undo takes the code back; the file stays. A file that isn't a WAV is refused in words, and
    /// nothing is copied. Answers the edit with `name` (the step's) and `asset`.
    pub fn set_sample(&self, token: u64, at: u64, call: String, source: Source) -> Result<Value> {
        let dir = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            if let Some(why) = &p.shown.read_only {
                bail!("{why}");
            }
            if !p.shown.state["view"]["draft_errors"].as_array().is_none_or(Vec::is_empty) {
                bail!("The code doesn't compile, so the Sampler's file can't be set now. Fix the code first.");
            }
            p.shown.dir.clone()
        };
        let file = {
            let _one = IMPORTING.lock();
            let mut peaks = Peaks::builder();
            let f = import::audio(&dir, source, &mut |piece| peaks.add(piece)).map_err(|e| anyhow::anyhow!("{}", words(&e)))?;
            match f.reused {
                false => drop(Store::global().put(f.hash, peaks.done(f.rate))),
                true => drop(Store::global().of(&dir.join(m2_song::path::ASSETS).join(f.asset.as_str()))),
            }
            f
        };
        let name = format!("setting Sampler to {}", file.asset.as_str());
        let asset = file.asset.as_str().to_string();
        let mut done = self.call(move |a| {
            a.edit_and(token, |s| {
                let e = m2_editor::sampler::set_sample(s, at, &call, &file)?;
                Ok((e, json!({})))
            })
        })?;
        if let Some(revision) = done["revision"].as_u64() {
            let named = self.name_step(token, revision, name.clone(), false)?;
            done["undo_name"] = named["undo_name"].clone();
        }
        done["name"] = json!(name);
        done["asset"] = json!(asset);
        Ok(done)
    }

    /// The + Track list's Audio track (Q8): an audio track named `label` (else "Audio"), after
    /// track `after` (else last), as one undo step named "adding Audio". Answers `{module, label,
    /// revision}`.
    pub fn add_audio_track(&self, token: u64, label: Option<String>, after: Option<String>) -> Result<Value> {
        let added = self.call(move |a| {
            a.edit_and(token, |s| {
                let new = m2_editor::tracks::NewTrack { play: m2_editor::tracks::Play::Audio, label, key: None, clips: vec![] };
                let made = m2_editor::tracks::add_tracks(s, &[new], after.as_deref(), None)?.remove(0);
                Ok((made.edited, json!({"module": made.module, "label": made.label})))
            })
        })?;
        if let (Some(revision), Some(label)) = (added["revision"].as_u64(), added["label"].as_str()) {
            self.name_step(token, revision, format!("adding {label}"), true)?;
        }
        if let Some(k) = added["module"].as_str() {
            self.forget_overlays(&[k.to_string()]);
        }
        Ok(added)
    }
}

/// An import's refusal as the page says it: without "edit rejected: ".
fn words(e: &m2_editor::EditError) -> String {
    e.to_string().trim_start_matches("edit rejected: ").to_string()
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

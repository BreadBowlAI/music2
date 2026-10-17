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
use crate::peaks::Store;

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
                let (f, decoded) = import::audio(&dir, src).map_err(|e| anyhow::anyhow!("{}", words(&e)))?;
                // The peaks from the decode the import made (D31); a file used again has its own.
                match decoded {
                    Some(a) => drop(Store::global().put(f.hash, &a)),
                    None => drop(Store::global().of(&dir.join(m2_song::path::ASSETS).join(f.asset.as_str()))),
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
                let made: Vec<Value> = p.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                let tracks: Vec<Value> = p.tracks.iter().map(|(m, l)| json!({"module": m, "label": l})).collect();
                Ok((p.edited, json!({"made": made, "tracks": tracks})))
            })
        })?;
        if let Some(revision) = done["revision"].as_u64() {
            let named = self.name_step(token, revision, name.clone(), structural)?;
            done["undo_name"] = named["undo_name"].clone();
        }
        done["name"] = json!(name);
        done["notes"] = json!(notes);
        done["assets"] = json!(files.iter().map(|f| f.asset.as_str()).collect::<Vec<_>>());
        Ok(done)
    }
}

/// An import's refusal as the page says it: without "edit rejected: ".
fn words(e: &m2_editor::EditError) -> String {
    e.to_string().trim_start_matches("edit rejected: ").to_string()
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

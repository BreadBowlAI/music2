//! Audio files in the open song (lane A): where an asset's file is, for its waveform's peaks
//! (A1); and bringing files in (A2): copied on the calling thread to a hidden part file in
//! `assets/`, their peaks made from that one decode, then named and placed by one actor job as one
//! undo step, named on the server (by an agent's words when it gives some, AGENT-2). An agent's is
//! checked before anything is copied, and again in that job: refused, nothing of it is left
//! (AGENT-3).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::import::{self, Imported, Source, Staged, Target};
use m2_song::AssetName;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::Graph;
use crate::peaks::{Peaks, Store};

/// One import at a time in this server, from its copy to the job that names its files: two never
/// share a part file (D2).
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

    /// AGENT-3: whether an agent's write may begin, before anything of it is made off the session's
    /// thread (an import's copy): busy while a take records or the Compare pill is open. The job
    /// that lands it checks again.
    fn arriving(&self, token: u64) -> Result<()> {
        if !crate::caller::now().is_agent() {
            return Ok(());
        }
        self.call(move |a| {
            a.open.as_ref().context("no project is open")?.check_token(token)?;
            a.arrive(None)
        })
    }

    /// `sources`, each copied to a hidden part file in `dir`'s `assets/` (or the same bytes there
    /// found), its peaks made from that one decode (D31): none is in the song yet. One refused says
    /// why, and those staged before it go. The caller holds [`IMPORTING`] until they are named.
    fn stage(dir: &Path, sources: Vec<Source>) -> Result<Vec<Staged>> {
        let mut staged = vec![];
        for src in sources {
            let mut peaks = Peaks::builder();
            let f = import::stage(dir, src, &mut |piece| peaks.add(piece)).map_err(|e| anyhow!("{}", words(&e)))?;
            let file = f.file();
            // A file used again has its own peaks.
            match file.reused {
                false => drop(Store::global().put(file.hash, peaks.done(file.rate))),
                true => drop(Store::global().of(&dir.join(m2_song::path::ASSETS).join(file.asset.as_str()))),
            }
            staged.push(f);
        }
        Ok(staged)
    }

    /// Bring `sources` into the open song and place them at `target` (Q6, Q7): each copied and its
    /// peaks made, here ([`Graph::stage`]); then one actor job names them into `assets/` and places
    /// their clips (one Tx, one undo step named "importing vox.wav", or by an agent's words).
    /// Answers the edit with `made` (the clips, to select), `tracks` (any added), `assets` and
    /// `reused` (each file's asset, and whether its bytes were there already) and `notes` (what to
    /// tell: a file whose extra channels don't play). A file refused says why, and nothing is added
    /// to the song; nor is anything while the musician holds the song (AGENT-3), which is checked
    /// before anything is copied, and again when the files would be named.
    pub fn import_audio(&self, token: u64, sources: Vec<Source>, target: Target) -> Result<Value> {
        if sources.is_empty() {
            bail!("No file to bring in.");
        }
        let dir = self.import_dir(token, &target)?;
        self.arriving(token)?;
        let assets = dir.join(m2_song::path::ASSETS);
        let _one = IMPORTING.lock();
        let made_folder = !assets.exists();
        let done = self.import_staged(token, &dir, sources, target);
        if done.is_err() && made_folder {
            // The folder this import made for files that never came in.
            let _ = std::fs::remove_dir(&assets);
        }
        let done = done?;
        // A new track under a key a gone track's mute or solo was kept for starts heard (Q7).
        let keys: Vec<String> =
            done["tracks"].as_array().into_iter().flatten().filter_map(|t| t["module"].as_str().map(String::from)).collect();
        if !keys.is_empty() {
            self.forget_overlays(&keys);
        }
        Ok(done)
    }

    /// [`Graph::import_audio`]'s copy, then its job.
    fn import_staged(&self, token: u64, dir: &Path, sources: Vec<Source>, target: Target) -> Result<Value> {
        let staged = Graph::stage(dir, sources)?;
        let files: Vec<Imported> = staged.iter().map(|f| f.file().clone()).collect();
        // AGENT-2: an agent's step is named by its words.
        let name = crate::caller::now().why().unwrap_or_else(|| step_name(&files));
        let structural = !matches!(target, Target::Lane { .. });
        let notes: Vec<String> = files
            .iter()
            .filter(|f| f.dropped_channels > 0)
            .map(|f| format!("{} has {} channels: music2 plays the first two.", f.name, f.dropped_channels + 2))
            .collect();
        let named = name.clone();
        let mut done = self.call(move |a| {
            a.edit_and(token, |s| {
                // Into the song only now, after this job's check (AGENT-3).
                let files = staged.into_iter().map(Staged::name).collect::<Result<Vec<_>, _>>()?;
                let p = import::place(s, &files, &target)?;
                s.name_step(p.edited.revision, &named, structural);
                let made: Vec<Value> =
                    p.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                let tracks: Vec<Value> = p.tracks.iter().map(|(m, l)| json!({"module": m, "label": l})).collect();
                let assets: Vec<&str> = files.iter().map(|f| f.asset.as_str()).collect();
                let reused: Vec<bool> = files.iter().map(|f| f.reused).collect();
                Ok((p.edited, json!({"made": made, "tracks": tracks, "assets": assets, "reused": reused})))
            })
        })?;
        done["name"] = json!(name);
        done["notes"] = json!(notes);
        Ok(done)
    }
}

impl Graph {
    /// A WAV dropped on a sampler's card (track ab): `source` is copied here, as an import's file is
    /// ([`Graph::stage`]), then one actor job names it into the open song's `assets/` (or the same
    /// bytes there are used again) and writes its name into sampler call `call` of revision `at`
    /// (one Tx, one undo step, "setting Sampler to kick.wav", or an agent's words). Undo takes the
    /// code back; the file stays. A file that isn't a WAV is refused in words, and nothing is
    /// copied; nor is anything while the musician holds the song (AGENT-3). Answers the edit with
    /// `name` (the step's) and `asset`.
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
        self.arriving(token)?;
        let assets = dir.join(m2_song::path::ASSETS);
        let _one = IMPORTING.lock();
        let made_folder = !assets.exists();
        let words = crate::caller::now().why();
        let done = Graph::stage(&dir, vec![source]).map(|mut s| s.remove(0)).and_then(|staged| {
            self.call(move |a| {
                a.edit_and(token, |s| {
                    // Into the song only now, after this job's check (AGENT-3).
                    let file = staged.name()?;
                    let e = m2_editor::sampler::set_sample(s, at, &call, &file)?;
                    // AGENT-2: an agent's step is named by its words.
                    let name = words.unwrap_or_else(|| format!("setting Sampler to {}", file.asset.as_str()));
                    s.name_step(e.revision, &name, false);
                    Ok((e, json!({"name": name, "asset": file.asset.as_str()})))
                })
            })
        });
        if done.is_err() && made_folder {
            let _ = std::fs::remove_dir(&assets);
        }
        done
    }

    /// The + Track list's Audio track (Q8): an audio track named `label` (else "Audio"), after
    /// track `after` (else last), as one undo step named "adding Audio" (or by an agent's words,
    /// AGENT-2). Answers `{module, label, revision}`.
    pub fn add_audio_track(&self, token: u64, label: Option<String>, after: Option<String>) -> Result<Value> {
        let words = crate::caller::now().why();
        let added = self.call(move |a| {
            a.edit_and(token, |s| {
                let new = m2_editor::tracks::NewTrack { play: m2_editor::tracks::Play::Audio, label, key: None, clips: vec![] };
                let made = m2_editor::tracks::add_tracks(s, &[new], after.as_deref(), None)?.remove(0);
                s.name_step(made.edited.revision, &words.unwrap_or_else(|| format!("adding {}", made.label)), true);
                Ok((made.edited, json!({"module": made.module, "label": made.label})))
            })
        })?;
        if let Some(k) = added["module"].as_str() {
            self.forget_overlays(&[k.to_string()]);
        }
        Ok(added)
    }
}

impl Graph {
    /// Add sample `sample` from the library to the open song (LIB-7): its file copied here, as an
    /// import's is ([`Graph::stage`]), then one actor job names it into `assets/` (or the same bytes
    /// there are used again), writes its note beside it (create-only, before the clip names it) and
    /// places its clip at `target`, with Follow tempo for a loop whose tempo differs from the
    /// song's there: one Tx, one undo step, "adding kick.wav" (or an agent's words). Answers the
    /// edit with `made`, `tracks`, `asset`, `reused`, `noted` (the note was written now) and
    /// `follows` (the loop's tempo it follows, or null).
    pub fn add_sample(&self, token: u64, sample: m2_editor::library::Sample, target: Target) -> Result<Value> {
        let dir = self.import_dir(token, &target)?;
        self.arriving(token)?;
        let assets = dir.join(m2_song::path::ASSETS);
        let _one = IMPORTING.lock();
        let made_folder = !assets.exists();
        let words = crate::caller::now().why();
        let structural = !matches!(target, Target::Lane { .. });
        let done = Graph::stage(&dir, vec![Source::Path(sample.file.clone())]).map(|mut s| s.remove(0)).and_then(|staged| {
            self.call(move |a| {
                a.edit_and(token, |s| {
                    // Into the song only now, after this job's check (AGENT-3); its note before
                    // the clip names it (WRITE-1).
                    let file = staged.name_noted()?;
                    let noted =
                        m2_editor::library::samples::write_note(&dir, file.asset.as_str(), &m2_editor::library::samples::note(&sample))
                            .map_err(|e| m2_editor::EditError::Io(format!("couldn't write the note of {}: {e}", file.asset)))?;
                    let song_bpm = s.accepted().compiled.data.tempo().bpm_at(target.beat().max(0.0));
                    let follows = m2_editor::library::samples::follow_at(&sample, song_bpm);
                    let p = import::place_with(s, std::slice::from_ref(&file), &target, &[follows])?;
                    let name = words.unwrap_or_else(|| format!("adding {}", file.asset.as_str()));
                    s.name_step(p.edited.revision, &name, structural);
                    let made: Vec<Value> =
                        p.made.iter().map(|(l, id)| json!({"module": l.module.as_str(), "lane": l.lane.as_str(), "id": id})).collect();
                    let tracks: Vec<Value> = p.tracks.iter().map(|(m, l)| json!({"module": m, "label": l})).collect();
                    let answer = json!({
                        "made": made, "tracks": tracks, "name": name, "asset": file.asset.as_str(), "reused": file.reused,
                        "noted": noted, "follows": follows, "song_bpm": song_bpm,
                    });
                    Ok((p.edited, answer))
                })
            })
        });
        if done.is_err() && made_folder {
            let _ = std::fs::remove_dir(&assets);
        }
        let done = done?;
        let keys: Vec<String> =
            done["tracks"].as_array().into_iter().flatten().filter_map(|t| t["module"].as_str().map(String::from)).collect();
        if !keys.is_empty() {
            self.forget_overlays(&keys);
        }
        Ok(done)
    }

    /// Sample `sample` from the library dropped on sampler call `call` of revision `at` (LIB-7):
    /// copied and noted as [`Graph::add_sample`] does, then it becomes the Sampler's `sample`, as a
    /// WAV dropped there does ([`Graph::set_sample`]): one Tx, one undo step.
    pub fn sample_to_sampler(&self, token: u64, at: u64, call: String, sample: m2_editor::library::Sample) -> Result<Value> {
        let dir = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            if let Some(why) = &p.shown.read_only {
                bail!("{why}");
            }
            p.shown.dir.clone()
        };
        self.arriving(token)?;
        let assets = dir.join(m2_song::path::ASSETS);
        let _one = IMPORTING.lock();
        let made_folder = !assets.exists();
        let words = crate::caller::now().why();
        let done = Graph::stage(&dir, vec![Source::Path(sample.file.clone())]).map(|mut s| s.remove(0)).and_then(|staged| {
            self.call(move |a| {
                a.edit_and(token, |s| {
                    let file = staged.name_noted()?;
                    m2_editor::library::samples::write_note(&dir, file.asset.as_str(), &m2_editor::library::samples::note(&sample))
                        .map_err(|e| m2_editor::EditError::Io(format!("couldn't write the note of {}: {e}", file.asset)))?;
                    let e = m2_editor::sampler::set_sample(s, at, &call, &file)?;
                    let name = words.unwrap_or_else(|| format!("setting Sampler to {}", file.asset.as_str()));
                    s.name_step(e.revision, &name, false);
                    Ok((e, json!({"name": name, "asset": file.asset.as_str()})))
                })
            })
        });
        if done.is_err() && made_folder {
            let _ = std::fs::remove_dir(&assets);
        }
        done
    }
}

/// An import's refusal as the page says it: without "edit rejected: ".
fn words(e: &m2_editor::EditError) -> String {
    e.to_string().trim_start_matches("edit rejected: ").to_string()
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

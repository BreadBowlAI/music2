//! L4 — the engine. A [`Session`] owns the current song, history and assets. Every
//! change goes through [`Session::execute`] with a command id and JSON params, so the
//! UI, the CLI and anything driving the HTTP API share one registry and one undo stack.

pub mod commands;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use m2_graph::Registry;
use m2_project::{Asset, Id, Song, assets_dir_for};
use m2_render::{SampleBank, load_wav};
use serde_json::Value;

pub use commands::{COMMANDS, CommandDef};

pub const HISTORY_LIMIT: usize = 200;

struct Entry {
    label: String,
    song: Arc<Song>,
}

struct Group {
    label: String,
    before: Arc<Song>,
    depth: usize,
}

/// A step in a multi-command recipe. A `"track": "$new"` param refers to the track
/// created by the most recent `track.add` / `sampler.load` / `audio.place` in the recipe.
pub type Step = (String, Value);

pub struct Session {
    song: Arc<Song>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    group: Option<Group>,
    path: Option<PathBuf>,
    revision: u64,
    saved_revision: u64,
    pub registry: Arc<Registry>,
    bank: SampleBank,
    /// (asset id, root pitch) loaded into the bank, to detect when to reload.
    bank_keys: Vec<(Id, u8)>,
    scratch: PathBuf,
    notices: Vec<String>,
}

impl Session {
    pub fn new(registry: Arc<Registry>) -> Session {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Session {
            song: Arc::new(Song::default()),
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
            path: None,
            revision: 1,
            saved_revision: 1,
            registry,
            bank: SampleBank::default(),
            bank_keys: Vec::new(),
            scratch: music2_home().join("scratch").join(stamp.to_string()),
            notices: Vec::new(),
        }
    }

    /// Registry with built-ins plus everything in `~/.music2/plugins`.
    pub fn default_registry() -> (Registry, Vec<String>) {
        let mut reg = Registry::with_builtins();
        let errors = reg.load_dir(&plugins_dir());
        (reg, errors)
    }

    pub fn song(&self) -> &Arc<Song> {
        &self.song
    }

    /// Increments on every change (edit, undo, open). Frontends compare it to know when
    /// to rebuild their renderer.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn bank(&self) -> &SampleBank {
        &self.bank
    }

    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    pub fn notice(&mut self, msg: impl Into<String>) {
        self.notices.push(msg.into());
    }

    // ------------------------------------------------------------ commands

    pub fn execute(&mut self, id: &str, params: Value) -> Result<Value> {
        let cmd = commands::find(id).ok_or_else(|| anyhow!("unknown command `{id}`"))?;
        let mut next = (*self.song).clone();
        let out = {
            let mut ctx = commands::CmdCtx { song: &mut next, registry: &self.registry };
            (cmd.run)(&mut ctx, &params).with_context(|| cmd.id)?
        };
        if cmd.mutates && next != *self.song {
            next.assign_device_ids();
            next.validate()?;
            self.commit(cmd.label, next);
        }
        Ok(out)
    }

    /// Run a recipe as one undo step.
    pub fn execute_steps(&mut self, label: &str, steps: &[Step]) -> Result<()> {
        let before = self.song.clone();
        let outer = self.group.is_some();
        self.begin_group(label);
        let mut last_track: Option<u64> = None;
        let res = (|| {
            for (id, params) in steps {
                let out = self.execute(id, substitute(params, last_track))?;
                if let Some(t) = out.get("track_id").and_then(|v| v.as_u64()) {
                    last_track = Some(t);
                }
            }
            Ok(())
        })();
        self.end_group();
        if res.is_err() && !Arc::ptr_eq(&before, &self.song) {
            // Don't leave half a recipe behind.
            if !outer && self.undo.last().is_some_and(|e| Arc::ptr_eq(&e.song, &before)) {
                self.undo.pop();
            }
            self.song = before;
            self.revision += 1;
            self.refresh_bank();
        }
        res
    }

    fn commit(&mut self, label: &str, next: Song) {
        let prev = std::mem::replace(&mut self.song, Arc::new(next));
        if self.group.is_none() {
            self.push_undo(label.to_string(), prev);
        }
        self.revision += 1;
        self.refresh_bank();
    }

    fn push_undo(&mut self, label: String, song: Arc<Song>) {
        self.undo.push(Entry { label, song });
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Collapse everything until the matching `end_group` into one undo step.
    pub fn begin_group(&mut self, label: &str) {
        match &mut self.group {
            Some(g) => g.depth += 1,
            None => self.group = Some(Group { label: label.to_string(), before: self.song.clone(), depth: 1 }),
        }
    }

    pub fn end_group(&mut self) {
        let Some(g) = &mut self.group else { return };
        g.depth -= 1;
        if g.depth == 0 {
            let g = self.group.take().unwrap();
            if *g.before != *self.song {
                self.push_undo(g.label, g.before);
            }
        }
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    pub fn undo(&mut self) -> Option<String> {
        while self.group.is_some() {
            self.end_group();
        }
        let e = self.undo.pop()?;
        let current = std::mem::replace(&mut self.song, e.song);
        self.redo.push(Entry { label: e.label.clone(), song: current });
        self.revision += 1;
        self.refresh_bank();
        Some(e.label)
    }

    pub fn redo(&mut self) -> Option<String> {
        let e = self.redo.pop()?;
        let current = std::mem::replace(&mut self.song, e.song);
        self.undo.push(Entry { label: e.label.clone(), song: current });
        self.revision += 1;
        self.refresh_bank();
        Some(e.label)
    }

    // ------------------------------------------------------------ files

    pub fn new_song(&mut self, song: Song) {
        *self = Session::new(self.registry.clone());
        self.song = Arc::new(song);
        self.revision = 2;
        self.saved_revision = 1;
    }

    pub fn open(&mut self, path: &Path) -> Result<()> {
        let song = Song::load(path)?;
        *self = Session::new(self.registry.clone());
        self.song = Arc::new(song);
        self.path = Some(path.to_path_buf());
        self.revision = 2;
        self.saved_revision = 2;
        self.refresh_bank();
        let missing: Vec<String> = self
            .song
            .tracks
            .iter()
            .flat_map(|t| t.instrument.iter().chain(t.effects.iter()))
            .filter(|d| self.registry.get(&d.plugin).is_none())
            .map(|d| d.plugin.clone())
            .collect();
        if !missing.is_empty() {
            self.notice(format!("Missing plugins: {} — install with `music2-cli plugin add`", missing.join(", ")));
        }
        Ok(())
    }

    /// Save to `path`. Assets still in the scratch area move into `<song>.assets/`.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        let dir = assets_dir_for(path);
        let mut song = (*self.song).clone();
        for a in &mut song.assets {
            let src = self.asset_path(a);
            let target = dir.join(file_name(&src));
            let is_relative = Path::new(&a.file).is_relative();
            if !is_relative || self.path.as_deref() != Some(path) {
                if src != target && src.exists() {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::copy(&src, &target).with_context(|| format!("copying {}", src.display()))?;
                }
                a.file = file_name(&target);
            }
        }
        song.save(path)?;
        self.song = Arc::new(song);
        self.path = Some(path.to_path_buf());
        self.saved_revision = self.revision;
        Ok(())
    }

    // ------------------------------------------------------------ assets

    pub fn assets_dir(&self) -> PathBuf {
        match &self.path {
            Some(p) => assets_dir_for(p),
            None => self.scratch.clone(),
        }
    }

    pub fn asset_path(&self, a: &Asset) -> PathBuf {
        let p = Path::new(&a.file);
        if p.is_absolute() { p.to_path_buf() } else { self.assets_dir().join(p) }
    }

    /// Store WAV bytes as a new asset (one undo step). Returns the asset id.
    pub fn import_wav_bytes(&mut self, name: &str, wav: &[u8], prompt: Option<String>) -> Result<Id> {
        let dir = self.assets_dir();
        std::fs::create_dir_all(&dir)?;
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join("-");
        let mut path = dir.join(format!("{slug}.wav"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{slug}-{n}.wav"));
            n += 1;
        }
        std::fs::write(&path, wav)?;
        self.import_path(name, &path, prompt)
    }

    /// Copy a WAV file into the song's assets and register it.
    pub fn import_file(&mut self, src: &Path) -> Result<Id> {
        let dir = self.assets_dir();
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(file_name(src));
        if target != src {
            std::fs::copy(src, &target)?;
        }
        let name = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Sound".into());
        self.import_path(&name, &target, None)
    }

    fn import_path(&mut self, name: &str, path: &Path, prompt: Option<String>) -> Result<Id> {
        let data = load_wav(path, 60)?;
        if data.frames.is_empty() {
            bail!("{} contains no audio", path.display());
        }
        let duration = data.frames.len() as f32 / data.sr;
        let mut next = (*self.song).clone();
        let id = next.alloc_id();
        let file = if self.path.is_some() { file_name(path) } else { path.to_string_lossy().to_string() };
        next.assets.push(Asset { id, name: name.to_string(), file, prompt, root_pitch: 60, duration });
        self.bank.samples.insert(id, Arc::new(data));
        self.bank_keys.push((id, 60));
        self.commit("Import sound", next);
        Ok(id)
    }

    /// Decode any assets that aren't in the bank yet (after open/undo/root change).
    fn refresh_bank(&mut self) {
        let wanted: Vec<(Id, u8)> = self.song.assets.iter().map(|a| (a.id, a.root_pitch)).collect();
        if wanted == self.bank_keys {
            return;
        }
        let assets = self.song.assets.clone();
        for a in &assets {
            let have = self.bank.samples.get(&a.id).map(|s| s.root);
            if have == Some(a.root_pitch) {
                continue;
            }
            match load_wav(&self.asset_path(a), a.root_pitch) {
                Ok(d) => {
                    self.bank.samples.insert(a.id, Arc::new(d));
                }
                Err(e) => self.notices.push(format!("Couldn't load sound `{}`: {e:#}", a.name)),
            }
        }
        self.bank_keys = wanted;
    }
}

fn substitute(params: &Value, last_track: Option<u64>) -> Value {
    let mut p = params.clone();
    if p.get("track").and_then(|v| v.as_str()) == Some("$new")
        && let Some(t) = last_track
    {
        p["track"] = Value::from(t);
    }
    p
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// `~/.music2`: installed plugins and scratch files.
pub fn music2_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".music2")
}

pub fn plugins_dir() -> PathBuf {
    music2_home().join("plugins")
}

/// Download a plugin from GitHub (`github:owner/repo[/path][@ref]`), validate it, save it
/// under `~/.music2/plugins/`, and register it. The plugin's id is rewritten to the
/// GitHub source so projects can find it again.
pub fn install_github_plugin(registry: &mut Registry, spec: &str) -> Result<String> {
    let (url, id) = m2_graph::github_source(spec)?;
    let mut resp = ureq::get(&url).call().with_context(|| format!("fetching {url}"))?;
    let text = resp.body_mut().read_to_string()?;
    let mut def = m2_graph::parse(&text)?;
    def.id = id.clone();
    m2_graph::validate(&def)?;
    let dir = plugins_dir();
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.m2plug.json", id.replace([':', '/'], "_")));
    std::fs::write(&file, serde_json::to_string_pretty(&def)?)?;
    registry.add(def)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session() -> Session {
        Session::new(Arc::new(Registry::with_builtins()))
    }

    #[test]
    fn execute_undo_redo() {
        let mut s = session();
        let r = s.execute("track.add", json!({"name": "Bass", "instrument": "m2/bass"})).unwrap();
        let tid = r["track_id"].as_u64().unwrap();
        let c = s
            .execute(
                "clip.add",
                json!({"track": "bass", "start_bar": 1, "length_bars": 2,
            "notes": [{"pitch": "A1", "start": 0, "length": 1}, {"pitch": 33, "start": 1, "length": 0.5}]}),
            )
            .unwrap();
        let cid = c["clip_id"].as_u64().unwrap();
        assert_eq!(s.song().clip(cid).unwrap().notes().len(), 2);
        assert_eq!(s.song().track(tid).unwrap().clips.len(), 1);
        s.execute("notes.transpose", json!({"clip": cid, "semitones": 12})).unwrap();
        assert_eq!(s.song().clip(cid).unwrap().notes()[0].pitch, 45);
        assert_eq!(s.undo().as_deref(), Some("Transpose"));
        assert_eq!(s.song().clip(cid).unwrap().notes()[0].pitch, 33);
        s.redo();
        assert_eq!(s.song().clip(cid).unwrap().notes()[0].pitch, 45);
        // Read-only commands don't create history.
        let before = s.undo_label().map(str::to_string);
        s.execute("song.describe", json!({})).unwrap();
        assert_eq!(s.undo_label().map(str::to_string), before);
    }

    #[test]
    fn groups_and_failed_recipes() {
        let mut s = session();
        s.execute_steps(
            "Add drums",
            &[
                ("track.add".into(), json!({"instrument": "m2/drums"})),
                (
                    "clip.add".into(),
                    json!({"track": "$new", "start_bar": 1, "length_bars": 1, "notes": [{"pitch": 36, "start": 0, "length": 0.5}]}),
                ),
            ],
        )
        .unwrap();
        assert_eq!(s.song().tracks.len(), 1);
        assert_eq!(s.undo_label(), Some("Add drums"));
        let err = s.execute_steps(
            "bad",
            &[("track.add".into(), json!({})), ("clip.add".into(), json!({"track": "nope", "start_bar": 1, "length_bars": 1}))],
        );
        assert!(err.is_err());
        assert_eq!(s.song().tracks.len(), 1, "failed recipe is rolled back");
        assert!(s.undo().is_some());
        assert_eq!(s.song().tracks.len(), 0);
    }

    #[test]
    fn routing_commands() {
        let mut s = session();
        s.execute("track.add", json!({"name": "Drums", "instrument": "m2/drums"})).unwrap();
        s.execute("track.add", json!({"name": "Bass", "instrument": "m2/bass"})).unwrap();
        s.execute("track.add", json!({"name": "Verb", "instrument": "bus"})).unwrap();
        s.execute("track.add", json!({"name": "Music", "instrument": "bus"})).unwrap();
        s.execute("effect.add", json!({"track": "Verb", "plugin": "m2/space"})).unwrap();
        s.execute("track.set", json!({"track": "Bass", "output": "Music"})).unwrap();
        s.execute("send.set", json!({"track": "Bass", "to": "Verb", "gain_db": -9})).unwrap();
        s.execute("effect.add", json!({"track": "Bass", "plugin": "m2/duck"})).unwrap();
        s.execute("device.setSidechain", json!({"track": "Bass", "device": 0, "source": "Drums"})).unwrap();
        s.execute(
            "automation.set",
            json!({"track": "Bass", "param": "macro", "device": "instrument", "macro": "tone",
            "points": [{"bar": 1, "value": 0.1}, {"bar": 5, "value": 0.9}]}),
        )
        .unwrap();
        s.execute(
            "automation.set",
            json!({"track": "Music", "param": "volume", "points": [{"bar": 1, "value": -20}, {"bar": 3, "value": 0}]}),
        )
        .unwrap();
        let song = s.song().clone();
        let bass = &song.tracks[1];
        assert_eq!(bass.output, Some(song.tracks[3].id));
        assert_eq!(bass.sends[0].gain_db, -9.0);
        assert_eq!(bass.effects[0].sidechain, Some(song.tracks[0].id));
        assert_eq!(bass.automation.len(), 1);
        // Loops and bad targets are rejected without changing the song.
        assert!(s.execute("track.set", json!({"track": "Music", "output": "Verb"})).is_ok());
        assert!(s.execute("track.set", json!({"track": "Verb", "output": "Music"})).unwrap_err().to_string().contains("loop"));
        assert!(s.execute("track.set", json!({"track": "Bass", "output": "Drums"})).is_err());
        assert!(s.execute("clip.add", json!({"track": "Verb", "start_bar": 1, "length_bars": 1})).is_err());
        // Deleting a bus cleans up everything that pointed at it.
        s.execute("track.remove", json!({"track": "Verb"})).unwrap();
        let bass = s.song().tracks.iter().find(|t| t.name == "Bass").unwrap().clone();
        assert!(bass.sends.is_empty());
        // Renders.
        let r = m2_render::render_song(s.song().clone(), &s.registry, s.bank(), 48_000, 0.1);
        assert!(r.iter().all(|f| f[0].is_finite()));
    }

    #[test]
    fn every_command_has_valid_schema() {
        for c in COMMANDS {
            let schema = (c.schema)();
            assert_eq!(schema["type"], "object", "{}", c.id);
            assert!(c.id.replace('.', "_").chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_'));
        }
    }
}

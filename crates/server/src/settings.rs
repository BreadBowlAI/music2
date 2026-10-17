//! This machine's settings (gaps #31, next lane C2): `~/.music2/settings.json`, never a song's,
//! with no undo. The Settings window changes them, and each change applies at once.
//!
//! ```json
//! {"output": {"device": "coreaudio:AppleUSBAudioEngine:…", "name": "Scarlett 2i2", "frames": 256}, "songs": "/Users/me/Songs"}
//! ```
//! - `output`: the sound output by its id (`m2_play::Output::id`), its name as it was listed when
//!   chosen (shown while it isn't connected), and its buffer size in samples; null for the device
//!   and the size is the system's default. A device that is gone, or won't open, plays on the
//!   default.
//! - `songs`: where new songs go (and Exports with them); null is `~/Music/music2`.
//! - `record` (lane B, Q18): `{"count_in": true, "quantize": false}`: Record counts in one bar first,
//!   and snaps the take's notes to Notes' grid. Left out while both are the defaults.
//! - `midi` (lane B, Q28): `{"off": ["Keystation 49"]}`: the MIDI keyboards turned off, by the names
//!   `m2_os::midi` gives; they neither play nor record. Left out while none is.
//!
//! Written whole, through a temporary file renamed over it, one change at a time. A file that
//! doesn't read is the defaults. The file is given, never found here: tests pass a temporary one,
//! so none of them writes the real `~/.music2`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use m2_play::Choice;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What the file holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    #[serde(default)]
    pub output: OutputKept,
    /// Where new songs go; None: the default.
    #[serde(default)]
    pub songs: Option<PathBuf>,
    /// Record's options (lane B).
    #[serde(default, skip_serializing_if = "RecordKept::is_default")]
    pub record: RecordKept,
    /// The MIDI keyboards turned off (lane B).
    #[serde(default, skip_serializing_if = "MidiKept::is_default")]
    pub midi: MidiKept,
}

/// Record's options: count in one bar first (on by default), and snap the take's notes to the
/// grid (off by default).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordKept {
    #[serde(default = "yes")]
    pub count_in: bool,
    #[serde(default)]
    pub quantize: bool,
}

fn yes() -> bool {
    true
}

impl Default for RecordKept {
    fn default() -> RecordKept {
        RecordKept { count_in: true, quantize: false }
    }
}

impl RecordKept {
    fn is_default(&self) -> bool {
        *self == RecordKept::default()
    }
}

/// The MIDI keyboards turned off in Settings, by name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiKept {
    #[serde(default)]
    pub off: Vec<String>,
}

impl MidiKept {
    fn is_default(&self) -> bool {
        self.off.is_empty()
    }
}

/// The sound output chosen, and its buffer size: None for each is the system's default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputKept {
    /// Its id: what opens it, never its name (NX-S1).
    #[serde(default)]
    pub device: Option<String>,
    /// Its name as it was listed when chosen: what Settings says of it while it isn't connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub frames: Option<u32>,
}

impl OutputKept {
    /// What a player opens.
    pub fn choice(&self) -> Choice {
        Choice { device: self.device.clone(), frames: self.frames }
    }
}

/// The settings in their file.
pub struct Settings {
    file: PathBuf,
    lock: Mutex<()>,
}

/// Each write's temporary file has a name of its own.
static TEMP: AtomicU64 = AtomicU64::new(0);

impl Settings {
    pub fn new(file: PathBuf) -> Settings {
        Settings { file, lock: Mutex::new(()) }
    }

    /// The file they are kept in.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// What the file says; the defaults when it isn't there or doesn't read.
    pub fn read(&self) -> Kept {
        std::fs::read(&self.file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write(&self, kept: &Kept) -> Result<()> {
        let dir = self.file.parent().context("the settings file has no folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        let tmp = dir.join(format!("settings.json.{}.{}.tmp", std::process::id(), TEMP.fetch_add(1, Ordering::Relaxed)));
        let text = serde_json::to_string_pretty(kept).expect("JSON") + "\n";
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.file).with_context(|| format!("writing {}", self.file.display()))
    }

    /// Change the settings, one change at a time; written only when they changed. Returns them.
    pub fn change(&self, f: impl FnOnce(&mut Kept)) -> Result<Kept> {
        let _one = self.lock.lock();
        let mut kept = self.read();
        let before = kept.clone();
        f(&mut kept);
        if kept != before {
            self.write(&kept)?;
        }
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Temp {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir =
                std::env::temp_dir().join(format!("m2-settings-{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // gaps #31: the three settings are kept in their file, each change at once, and read back
    // whole by the next launch; a change to one keeps the others.
    #[test]
    fn settings_are_kept_in_their_file() {
        let t = Temp::new("kept");
        let file = t.0.join(".music2/settings.json");
        let s = Settings::new(file.clone());
        assert_eq!(s.read(), Kept::default(), "no file: the defaults");
        s.change(|k| (k.output.device, k.output.name) = (Some("coreaudio:USB".into()), Some("Headphones".into()))).unwrap();
        s.change(|k| k.output.frames = Some(256)).unwrap();
        s.change(|k| k.songs = Some("/Users/me/Songs".into())).unwrap();
        let again = Settings::new(file.clone()).read();
        assert_eq!(again.output.choice(), Choice { device: Some("coreaudio:USB".into()), frames: Some(256) }, "opened by its id");
        assert_eq!(again.songs.as_deref(), Some(Path::new("/Users/me/Songs")));
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({"output": {"device": "coreaudio:USB", "name": "Headphones", "frames": 256}, "songs": "/Users/me/Songs"}));
        // Back to the defaults: nulls, and nothing else is left in the folder.
        s.change(|k| *k = Kept::default()).unwrap();
        assert_eq!(s.read(), Kept::default());
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["settings.json"], "no temporary file is left");
    }

    // Lane B: Record's options and the keyboards turned off are kept with the rest, each change at
    // once, and left out of the file while they are the defaults.
    #[test]
    fn record_options_and_keyboards_are_kept() {
        let t = Temp::new("record");
        let file = t.0.join("settings.json");
        let s = Settings::new(file.clone());
        assert_eq!(s.read().record, RecordKept { count_in: true, quantize: false }, "counts in, doesn't snap");
        assert!(s.read().midi.off.is_empty());
        s.change(|k| k.songs = Some("/x".into())).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({"output": {"device": null, "frames": null}, "songs": "/x"}), "the defaults aren't written");
        s.change(|k| (k.record.count_in, k.record.quantize) = (false, true)).unwrap();
        s.change(|k| k.midi.off = vec!["Keystation 49".into()]).unwrap();
        let again = Settings::new(file.clone()).read();
        assert_eq!(again.record, RecordKept { count_in: false, quantize: true });
        assert_eq!(again.midi.off, ["Keystation 49"]);
        assert_eq!(again.songs.as_deref(), Some(Path::new("/x")), "the others are kept");
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(json["record"], serde_json::json!({"count_in": false, "quantize": true}));
        assert_eq!(json["midi"], serde_json::json!({"off": ["Keystation 49"]}));
        // Half a record object reads with the other half's default.
        std::fs::write(&file, r#"{"record": {"quantize": true}}"#).unwrap();
        assert_eq!(s.read().record, RecordKept { count_in: true, quantize: true });
    }

    // A file that doesn't read (half written by an older build, or edited by hand) is the
    // defaults, and the next change writes it whole again.
    #[test]
    fn a_bad_settings_file_is_the_defaults() {
        let t = Temp::new("bad");
        let file = t.0.join("settings.json");
        std::fs::write(&file, "{\"output\": {\"device\": ").unwrap();
        let s = Settings::new(file.clone());
        assert_eq!(s.read(), Kept::default());
        std::fs::write(&file, r#"{"songs": "/x", "theme": "dark"}"#).unwrap();
        assert_eq!(s.read(), Kept { songs: Some("/x".into()), ..Kept::default() }, "what it doesn't know is skipped");
        std::fs::write(&file, "[]").unwrap();
        let kept = s.change(|k| k.output.frames = Some(128)).unwrap();
        assert_eq!(kept.output.frames, Some(128));
        assert_eq!(Settings::new(file).read(), kept);
    }
}

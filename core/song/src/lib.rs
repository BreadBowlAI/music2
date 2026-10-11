//! A music2 project folder.
//!
//! ```text
//! my-song/
//!   music2.toml         name (and later: dependencies, locks)
//!   song.ts             root module
//!   tracks/*.ts         modules
//!   timeline/song.json  tempo map
//!   timeline/<track>.<lane>.json   clips for one lane
//! ```
//!
//! Timeline data is plain JSON so it diffs well. Times are in beats; a clip's notes
//! and points are relative to the clip's start.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Manifest {
    #[serde(default)]
    pub project: ProjectInfo,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectInfo {
    #[serde(default = "default_name")]
    pub name: String,
    /// The root module, relative to the project folder.
    #[serde(default = "default_entry")]
    pub entry: String,
}

impl Default for ProjectInfo {
    fn default() -> Self {
        ProjectInfo { name: default_name(), entry: default_entry() }
    }
}

fn default_name() -> String {
    "untitled".into()
}

fn default_entry() -> String {
    "song.ts".into()
}

pub struct Project {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub timeline: Timeline,
}

impl Project {
    pub fn open(dir: &Path) -> Result<Project> {
        let dir = dir.canonicalize().with_context(|| format!("opening project {}", dir.display()))?;
        let manifest_path = dir.join("music2.toml");
        let manifest: Manifest = if manifest_path.exists() {
            toml::from_str(&std::fs::read_to_string(&manifest_path)?).with_context(|| format!("reading {}", manifest_path.display()))?
        } else {
            Manifest::default()
        };
        if !dir.join(&manifest.project.entry).exists() {
            bail!("{} has no {}", dir.display(), manifest.project.entry);
        }
        let timeline = Timeline::load(&dir.join("timeline"))?;
        Ok(Project { dir, manifest, timeline })
    }

    pub fn entry(&self) -> PathBuf {
        self.dir.join(&self.manifest.project.entry)
    }
}

/// The file stem used for a module's lane files: `"Lead Vox"` → `lead-vox`.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

// ---------------------------------------------------------------- tempo

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct TempoSegment {
    pub beat: f64,
    pub bpm: f64,
}

/// Piecewise-constant tempo: song time in beats ↔ seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    /// Sorted, the first at beat 0, each with the seconds at its start.
    segs: Vec<(TempoSegment, f64)>,
}

impl TempoMap {
    pub fn new(mut segments: Vec<TempoSegment>) -> Result<TempoMap> {
        if segments.is_empty() {
            segments.push(TempoSegment { beat: 0.0, bpm: 120.0 });
        }
        segments.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        if segments[0].beat != 0.0 {
            bail!("the tempo map must start at beat 0");
        }
        let mut segs = Vec::with_capacity(segments.len());
        let mut secs = 0.0;
        for (i, s) in segments.iter().enumerate() {
            if !(20.0..=999.0).contains(&s.bpm) {
                bail!("tempo {} bpm at beat {} is out of range (20..999)", s.bpm, s.beat);
            }
            if i > 0 {
                let prev = segments[i - 1];
                secs += (s.beat - prev.beat) * 60.0 / prev.bpm;
            }
            segs.push((*s, secs));
        }
        Ok(TempoMap { segs })
    }

    pub fn constant(bpm: f64) -> TempoMap {
        TempoMap::new(vec![TempoSegment { beat: 0.0, bpm }]).expect("valid tempo")
    }

    pub fn seconds_at(&self, beat: f64) -> f64 {
        let i = self.segs.partition_point(|(s, _)| s.beat <= beat).saturating_sub(1);
        let (s, at) = self.segs[i];
        at + (beat - s.beat) * 60.0 / s.bpm
    }

    pub fn beat_at(&self, seconds: f64) -> f64 {
        let i = self.segs.partition_point(|(_, at)| *at <= seconds).saturating_sub(1);
        let (s, at) = self.segs[i];
        s.beat + (seconds - at) * s.bpm / 60.0
    }

    /// Tempo at a beat.
    pub fn bpm_at(&self, beat: f64) -> f64 {
        let i = self.segs.partition_point(|(s, _)| s.beat <= beat).saturating_sub(1);
        self.segs[i].0.bpm
    }

    pub fn segments(&self) -> Vec<TempoSegment> {
        self.segs.iter().map(|(s, _)| *s).collect()
    }
}

// ---------------------------------------------------------------- timeline

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SongData {
    #[serde(default)]
    pub tempo: Vec<TempoSegment>,
    #[serde(default = "four")]
    pub beats_per_bar: u32,
}

fn four() -> u32 {
    4
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct NoteClip {
    pub start: f64,
    pub length: f64,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Note {
    /// Semitones (60 = middle C), or a name like `"A1"` or `"C#4"`.
    #[serde(deserialize_with = "pitch")]
    pub pitch: f64,
    pub start: f64,
    pub length: f64,
    #[serde(default = "default_velocity")]
    pub velocity: f64,
}

fn default_velocity() -> f64 {
    0.8
}

fn pitch<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum P {
        N(f64),
        S(String),
    }
    match P::deserialize(d)? {
        P::N(n) => Ok(n),
        P::S(s) => parse_pitch(&s).ok_or_else(|| serde::de::Error::custom(format!("bad pitch `{s}` (try 60 or \"C4\")"))),
    }
}

/// `"C4"` → 60, `"A#1"` → 34, `"Bb3"` → 58.
pub fn parse_pitch(s: &str) -> Option<f64> {
    let s = s.trim();
    let mut chars = s.chars();
    let base = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (acc, oct) = if let Some(r) = rest.strip_prefix('#') {
        (1, r)
    } else if let Some(r) = rest.strip_prefix('b') {
        (-1, r)
    } else {
        (0, rest)
    };
    let octave: i32 = oct.parse().ok()?;
    Some(((octave + 1) * 12 + base + acc) as f64)
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ControlClip {
    pub start: f64,
    pub length: f64,
    /// `[beat, value]` pairs, linear in between, held flat before the first and after the last.
    pub points: Vec<(f64, f64)>,
}

/// A clip on an audio lane: part of an asset, placed on the timeline.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct AudioClip {
    /// Position and length on the timeline, in beats.
    pub start: f64,
    pub length: f64,
    /// A file in the project's `assets/` folder (WAV).
    pub asset: String,
    /// Where in the asset the clip starts, in seconds.
    #[serde(default)]
    pub offset: f64,
    #[serde(default)]
    pub gain_db: f64,
    /// Fade lengths in seconds.
    #[serde(default)]
    pub fade_in: f64,
    #[serde(default)]
    pub fade_out: f64,
    /// Play the clip backwards (the audio as placed, reversed).
    #[serde(default)]
    pub reverse: bool,
    /// Transpose in semitones. Without warp markers this is varispeed (faster and
    /// higher together); with warp markers, timing follows the markers and only the
    /// pitch changes.
    #[serde(default)]
    pub pitch: f64,
    /// Warp markers `[source_seconds, beat]`, beats relative to the clip start. The
    /// audio between markers is time-stretched (keeping its pitch) so each marked
    /// moment lands on its beat, following the tempo map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warp: Vec<(f64, f64)>,
}

/// A note in song time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbsNote {
    pub start: f64,
    pub end: f64,
    pub pitch: f64,
    pub velocity: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Timeline {
    pub song: Option<SongData>,
    /// Raw lane files by `(slug, lane)`; parsed when a lane of known kind asks.
    files: BTreeMap<(String, String), (PathBuf, serde_json::Value)>,
}

#[derive(Deserialize)]
struct LaneFile<T> {
    clips: Vec<T>,
}

impl Timeline {
    pub fn load(dir: &Path) -> Result<Timeline> {
        let mut t = Timeline::default();
        let Ok(entries) = std::fs::read_dir(dir) else { return Ok(t) };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
        paths.sort();
        for path in paths {
            let text = std::fs::read_to_string(&path)?;
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            if stem == "song" {
                t.song = Some(serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?);
                continue;
            }
            let Some((module, lane)) = stem.split_once('.') else {
                bail!("{}: lane files are named <track>.<lane>.json", path.display());
            };
            let v = serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
            t.files.insert((module.to_string(), lane.to_string()), (path, v));
        }
        Ok(t)
    }

    pub fn tempo(&self) -> Result<TempoMap> {
        TempoMap::new(self.song.as_ref().map(|s| s.tempo.clone()).unwrap_or_default())
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.song.as_ref().map_or(4, |s| s.beats_per_bar.max(1))
    }

    fn clips<T: for<'de> Deserialize<'de>>(&self, module: &str, lane: &str) -> Result<Vec<T>> {
        match self.files.get(&(slug(module), lane.to_string())) {
            None => Ok(vec![]),
            Some((path, v)) => Ok(serde_json::from_value::<LaneFile<T>>(v.clone()).with_context(|| format!("reading {}", path.display()))?.clips),
        }
    }

    /// The notes of a notes lane in song time, cut to their clips, sorted by start.
    pub fn notes(&self, module: &str, lane: &str) -> Result<Vec<AbsNote>> {
        let mut out = vec![];
        for c in self.clips::<NoteClip>(module, lane)? {
            for n in &c.notes {
                let (s, e) = (n.start.max(0.0), (n.start + n.length).min(c.length));
                if e > s {
                    out.push(AbsNote { start: c.start + s, end: c.start + e, pitch: n.pitch, velocity: n.velocity.clamp(0.0, 1.0) });
                }
            }
        }
        out.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.pitch.total_cmp(&b.pitch)));
        Ok(out)
    }

    /// The clips of an audio lane, sorted by start, with warp markers sorted.
    pub fn audio(&self, module: &str, lane: &str) -> Result<Vec<AudioClip>> {
        let mut clips = self.clips::<AudioClip>(module, lane)?;
        for c in &mut clips {
            if c.length <= 0.0 || c.offset < 0.0 || c.fade_in < 0.0 || c.fade_out < 0.0 {
                bail!("clip at beat {}: length must be positive, and offset and fades can't be negative", c.start);
            }
            if c.asset.contains("..") || Path::new(&c.asset).is_absolute() {
                bail!("clip at beat {}: assets are files inside the project's assets/ folder, not `{}`", c.start, c.asset);
            }
            c.warp.sort_by(|a, b| a.1.total_cmp(&b.1));
            if c.warp.windows(2).any(|w| w[1].0 <= w[0].0 || w[1].1 <= w[0].1) {
                bail!("clip at beat {}: warp markers must move forward in both source time and beats", c.start);
            }
        }
        clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        Ok(clips)
    }

    pub fn control(&self, module: &str, lane: &str) -> Result<ControlLane> {
        let mut clips = self.clips::<ControlClip>(module, lane)?;
        for c in &mut clips {
            c.points.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        Ok(ControlLane { clips })
    }

    /// The file holding a lane's clips, if there is one.
    pub fn lane_file(&self, module: &str, lane: &str) -> Option<&Path> {
        self.files.get(&(slug(module), lane.to_string())).map(|(p, _)| p.as_path())
    }

    /// Lane files that no declared lane uses.
    pub fn unused(&self, used: &[(String, String)]) -> Vec<PathBuf> {
        self.files.iter().filter(|(k, _)| !used.iter().any(|(m, l)| slug(m) == k.0 && *l == k.1)).map(|(_, (p, _))| p.clone()).collect()
    }

    /// The beat where the last clip ends.
    pub fn end(&self) -> f64 {
        #[derive(Deserialize)]
        struct Span {
            start: f64,
            length: f64,
        }
        self.files
            .values()
            .filter_map(|(_, v)| serde_json::from_value::<LaneFile<Span>>(v.clone()).ok())
            .flat_map(|f| f.clips)
            .map(|c| c.start + c.length)
            .fold(0.0, f64::max)
    }
}

/// A control lane's clips. Where clips overlap, the one that starts later wins.
#[derive(Clone, Debug, Default)]
pub struct ControlLane {
    clips: Vec<ControlClip>,
}

impl ControlLane {
    pub fn value_at(&self, beat: f64, default: f64) -> f64 {
        let clip = self.clips.iter().filter(|c| beat >= c.start && beat < c.start + c.length).max_by(|a, b| a.start.total_cmp(&b.start));
        let Some(c) = clip else { return default };
        let t = beat - c.start;
        let p = &c.points;
        match p.iter().position(|(b, _)| *b > t) {
            None => p.last().map_or(default, |x| x.1),
            Some(0) => p[0].1,
            Some(i) => {
                let ((b0, v0), (b1, v1)) = (p[i - 1], p[i]);
                if b1 > b0 { v0 + (v1 - v0) * (t - b0) / (b1 - b0) } else { v1 }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_map_round_trips() {
        let t = TempoMap::new(vec![TempoSegment { beat: 0.0, bpm: 120.0 }, TempoSegment { beat: 8.0, bpm: 60.0 }]).unwrap();
        assert_eq!(t.seconds_at(4.0), 2.0);
        assert_eq!(t.seconds_at(8.0), 4.0);
        assert_eq!(t.seconds_at(10.0), 6.0);
        for b in [0.0, 3.5, 8.0, 12.25] {
            assert!((t.beat_at(t.seconds_at(b)) - b).abs() < 1e-9);
        }
        assert!(TempoMap::new(vec![TempoSegment { beat: 1.0, bpm: 120.0 }]).is_err());
    }

    #[test]
    fn pitches() {
        assert_eq!(parse_pitch("C4"), Some(60.0));
        assert_eq!(parse_pitch("A1"), Some(33.0));
        assert_eq!(parse_pitch("Bb3"), Some(58.0));
        assert_eq!(parse_pitch("C#-1"), Some(1.0));
        assert_eq!(parse_pitch("H2"), None);
    }

    #[test]
    fn control_lane_interpolates_and_later_clip_wins() {
        let lane = ControlLane {
            clips: vec![
                ControlClip { start: 0.0, length: 8.0, points: vec![(0.0, 0.0), (4.0, 1.0)] },
                ControlClip { start: 6.0, length: 2.0, points: vec![(0.0, 5.0)] },
            ],
        };
        assert_eq!(lane.value_at(2.0, 9.0), 0.5);
        assert_eq!(lane.value_at(5.0, 9.0), 1.0);
        assert_eq!(lane.value_at(7.0, 9.0), 5.0);
        assert_eq!(lane.value_at(8.0, 9.0), 9.0);
    }

    #[test]
    fn audio_clips_parse_with_defaults_and_validate() {
        let mut t = Timeline::default();
        let file = |v: serde_json::Value| (PathBuf::from("x.json"), v);
        t.files.insert(("vox".into(), "takes".into()), file(serde_json::json!({"clips": [
            {"start": 4, "length": 2, "asset": "vox.wav", "warp": [[1.0, 1], [0.0, 0]]},
            {"start": 0, "length": 1, "asset": "vox.wav", "reverse": true, "pitch": -12}]})));
        let c = t.audio("Vox", "takes").unwrap();
        assert_eq!(c[0].start, 0.0);
        assert!(c[0].reverse && c[0].pitch == -12.0 && c[0].gain_db == 0.0);
        assert_eq!(c[1].warp, vec![(0.0, 0.0), (1.0, 1.0)]);
        t.files.insert(("vox".into(), "bad".into()), file(serde_json::json!({"clips": [{"start": 0, "length": 1, "asset": "../secret.wav"}]})));
        assert!(t.audio("Vox", "bad").is_err());
        t.files.insert(("vox".into(), "bad".into()), file(serde_json::json!({"clips": [{"start": 0, "length": 1, "asset": "a.wav", "warp": [[0, 0], [1, 0]]}]})));
        assert!(t.audio("Vox", "bad").is_err());
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Lead Vox"), "lead-vox");
        assert_eq!(slug("Kick"), "kick");
    }
}

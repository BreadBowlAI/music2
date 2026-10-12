//! The timeline the editor shows: the lanes the accepted revision declares, with their
//! clips as written (notes and points relative to their clip).
//!
//! Like the graph view, it is derived from the accepted revision every time, so the
//! clips shown are the ones that play.

use m2_ir::{Kind, LaneDecl, Unit};
use m2_song::{AudioClip, ControlClip, DataSet, LaneData, Meter, NoteClip, Tempo, TempoMap};
use serde::Serialize;

use crate::edit::{Revision, Session};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineView {
    pub tempo: Vec<Tempo>,
    /// The time signatures (FMT-13); empty is 4/4 throughout.
    pub meter: Vec<Meter>,
    /// Where the last clip ends, in beats; at least one bar.
    pub end: f64,
    /// Modules with rows: tracks, and any other module that declares a lane.
    pub tracks: Vec<TimelineTrack>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineTrack {
    /// The module's key, as in the graph view: what edits and selection name it by (ID-11).
    pub module: String,
    /// What to show for it, never an address (ID-10).
    pub label: String,
    pub track: bool,
    /// In the order they are declared.
    pub lanes: Vec<TimelineLane>,
}

/// A lane and its clips, in canonical order. `key` addresses it within its module (ID-6),
/// `label` is what to show; each clip carries its id when it has one (FMT-12).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TimelineLane {
    Notes {
        key: String,
        label: String,
        clips: Vec<NoteClip>,
    },
    Control {
        key: String,
        label: String,
        unit: Option<&'static str>,
        default: Option<f64>,
        /// A range to draw the values in.
        min: f64,
        max: f64,
        clips: Vec<ControlClip>,
    },
    Audio {
        key: String,
        label: String,
        channels: u8,
        clips: Vec<AudioClip>,
    },
}

impl TimelineLane {
    /// The lane's key.
    pub fn key(&self) -> &str {
        match self {
            TimelineLane::Notes { key, .. } | TimelineLane::Control { key, .. } | TimelineLane::Audio { key, .. } => key,
        }
    }
}

impl Revision {
    /// The tempo map, which reading the project checked.
    pub fn tempo(&self) -> TempoMap {
        self.compiled.data.tempo().clone()
    }
}

/// A range that holds a control lane's values: the unit's own, if it has one, widened
/// to the default and every point, and never empty.
fn range(unit: Option<Unit>, default: Option<f64>, clips: &[ControlClip]) -> (f64, f64) {
    let own = match unit {
        Some(Unit::Unit01) => (0.0, 1.0),
        Some(Unit::Bipolar) => (-1.0, 1.0),
        _ => (f64::INFINITY, f64::NEG_INFINITY),
    };
    let values = default.into_iter().chain(clips.iter().flat_map(|c| c.points.iter().map(|p| p.1)));
    match values.fold(own, |(lo, hi), v| (lo.min(v), hi.max(v))) {
        (lo, hi) if lo < hi => (lo, hi),
        (lo, hi) if lo == hi => (lo - 1.0, hi + 1.0),
        _ => (-1.0, 1.0),
    }
}

impl Session {
    /// The timeline of the accepted revision.
    pub fn timeline(&self) -> TimelineView {
        let rev = self.accepted();
        let data = &rev.compiled.data;
        let tracks = rev.compiled.ir.modules.iter().filter(|m| m.track || !m.lanes.is_empty());
        let tracks = tracks
            .map(|m| TimelineTrack {
                module: m.key.clone(),
                label: m.label().to_string(),
                track: m.track,
                lanes: m.lanes.iter().map(|l| lane(data, &m.key, l)).collect(),
            })
            .collect();
        let a = &rev.project.arrangement;
        let bar = a.meter_at(0.0).bar();
        TimelineView { tempo: a.tempo.clone(), meter: a.meter.clone(), end: data.end().max(bar), tracks }
    }
}

/// A declared lane and the clips it plays, in canonical order (by start, then id), which is
/// also the order they win in where they overlap, so the last is drawn on top. A lane whose
/// file failed shows what it plays: its last accepted data, or nothing (FMT-9).
fn lane(data: &DataSet, module: &str, decl: &LaneDecl) -> TimelineLane {
    let (key, label) = (decl.key.clone(), decl.label().to_string());
    let lane = data.lane(module, &key).map(|d| &**d);
    match decl.kind {
        Kind::Events(_) => {
            let clips = if let Some(LaneData::Notes(c)) = lane { c.clone() } else { vec![] };
            TimelineLane::Notes { key, label, clips }
        }
        Kind::Control(unit) => {
            let clips = if let Some(LaneData::Control { clips, .. }) = lane { clips.clone() } else { vec![] };
            let (min, max) = range(unit, decl.default, &clips);
            TimelineLane::Control { key, label, unit: unit.map(Unit::name), default: decl.default, min, max, clips }
        }
        Kind::Audio(channels) => {
            let clips = data.audio(module, &key).to_vec();
            TimelineLane::Audio { key, label, channels, clips }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use m2_runtime::PackageSet;
    use serde_json::json;

    use super::*;

    const SONG: &str = r#"import { hz, song, track } from "@m2/core";
import { bass } from "std/instruments";
import { mixer } from "std/mix";
const b = track("bass", { name: "Bass" }, ({ lane }) => {
  const notes = lane.notes("notes", { name: "Notes" });
  const cutoff = lane.control("cutoff", "hz", hz(500));
  lane.mask("drop"); // declared, with no file yet
  return bass(notes, { cutoff });
});
const v = track("vox", ({ lane }) => lane.audio("vox"));
export default song(() => mixer({ bass: b.out, vox: v.out }));
"#;

    fn project() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "m2-timeline-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let files = [
            ("music2.toml", crate::tests::MANIFEST),
            ("song.ts", SONG),
            ("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 90}], "meter": [{"beat": 0, "beats": 3, "unit": 4}]}"#),
            (
                "data/bass/notes.json",
                r#"{"clips": [{"id": "b", "start": 8, "length": 4, "notes": [{"pitch": "A1", "start": 0.5, "length": 1}]},
                              {"id": "a", "start": 0, "length": 4, "notes": [{"pitch": 45, "start": 0, "length": 0.5, "velocity": 0.5}]}]}"#,
            ),
            (
                "data/bass/cutoff.json",
                r#"{"unit": "hz", "clips": [{"id": "c", "start": 0, "length": 8, "points": [[8, 2200], [0, 300]]}]}"#,
            ),
            (
                "data/vox/vox.json",
                r#"{"clips": [{"id": "e", "start": 2, "length": 1, "asset": "vox.wav"}, {"id": "d", "start": 0, "length": 1, "asset": "vox.wav", "gain_db": -6}]}"#,
            ),
        ];
        for (name, text) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        m2_runtime::audio::write_wav(&dir.join("assets/vox.wav"), &[vec![0.0; 4800], vec![0.0; 4800]], 48_000).unwrap();
        dir
    }

    fn open(dir: &Path) -> Session {
        let mut packages = PackageSet::default();
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        Session::open(dir, packages).unwrap_or_else(|d| panic!("{d:?}"))
    }

    #[test]
    fn shows_every_declared_lane_with_its_clips_as_written() {
        let dir = project();
        let mut s = open(&dir);
        let v = serde_json::to_value(s.timeline()).unwrap();
        assert_eq!((&v["tempo"], &v["end"]), (&json!([{"beat": 0.0, "bpm": 90.0}]), &json!(12.0)));
        // FMT-13: bars are counted from the meter alone; there is no second spelling of it.
        assert!(v.get("beats_per_bar").is_none());
        assert_eq!(v["meter"], json!([{"beat": 0.0, "beats": 3, "unit": 4}]));
        // The song module has no lanes and isn't a track.
        // ID-10, ID-11: tracks and lanes are addressed by key and shown by label.
        let modules: Vec<_> = v["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["module"].as_str().unwrap(), t["label"].as_str().unwrap(), t["track"].as_bool().unwrap()))
            .collect();
        assert_eq!(modules, [("bass", "Bass", true), ("vox", "vox", true)]);
        assert_eq!(
            v["tracks"][0]["lanes"],
            json!([
                {"kind": "notes", "key": "notes", "label": "Notes", "clips": [
                    {"id": "a", "start": 0.0, "length": 4.0, "notes": [{"pitch": 45.0, "start": 0.0, "length": 0.5, "velocity": 0.5}]},
                    {"id": "b", "start": 8.0, "length": 4.0, "notes": [{"pitch": 33.0, "start": 0.5, "length": 1.0, "velocity": 0.8}]},
                ]},
                {"kind": "control", "key": "cutoff", "label": "cutoff", "unit": "hz", "default": 500.0, "min": 300.0, "max": 2200.0,
                 "clips": [
                    {"id": "c", "start": 0.0, "length": 8.0, "points": [[0.0, 300.0], [8.0, 2200.0]]},
                ]},
                {"kind": "control", "key": "drop", "label": "drop", "unit": "unit01", "default": 0.0, "min": 0.0, "max": 1.0, "clips": []},
            ])
        );
        assert_eq!(
            v["tracks"][1]["lanes"],
            json!([{"kind": "audio", "key": "vox", "label": "vox", "channels": 2, "clips": [
                {"id": "d", "start": 0.0, "length": 1.0, "asset": "vox.wav", "offset": 0.0, "gain_db": -6.0, "fade_in": 0.0, "fade_out": 0.0, "reverse": false, "pitch": 0.0, "warp": []},
                {"id": "e", "start": 2.0, "length": 1.0, "asset": "vox.wav", "offset": 0.0, "gain_db": 0.0, "fade_in": 0.0, "fade_out": 0.0, "reverse": false, "pitch": 0.0, "warp": []},
            ]}])
        );

        // With no clips at all, the song is still a bar long.
        for f in ["bass/notes", "bass/cutoff", "vox/vox"] {
            std::fs::remove_file(dir.join(format!("data/{f}.json"))).unwrap();
        }
        assert!(s.refresh() && s.draft_errors.is_empty());
        let t = s.timeline();
        assert_eq!(t.end, 3.0);
        assert!(t.tracks.iter().flat_map(|t| &t.lanes).all(|l| match l {
            TimelineLane::Notes { clips, .. } => clips.is_empty(),
            TimelineLane::Control { clips, .. } => clips.is_empty(),
            TimelineLane::Audio { clips, .. } => clips.is_empty(),
        }));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn control_ranges_hold_every_value() {
        let clip = |points: Vec<(f64, f64)>| ControlClip { id: None, other_id: None, start: 0.0, length: 4.0, points };
        assert_eq!(range(Some(Unit::Unit01), Some(0.0), &[]), (0.0, 1.0));
        assert_eq!(range(Some(Unit::Bipolar), Some(0.0), &[clip(vec![(0.0, 1.5)])]), (-1.0, 1.5));
        assert_eq!(range(Some(Unit::Db), Some(-6.0), &[clip(vec![(0.0, -24.0), (1.0, 0.0)])]), (-24.0, 0.0));
        assert_eq!(range(Some(Unit::Hz), Some(500.0), &[]), (499.0, 501.0));
        assert_eq!(range(None, None, &[]), (-1.0, 1.0));
    }
}

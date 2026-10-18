//! The timeline the editor shows: the lanes the accepted revision declares, with their
//! clips as written (notes and points relative to their clip).
//!
//! Like the graph view, it is derived from the accepted revision every time, so the
//! clips shown are the ones that play.

use std::collections::BTreeMap;

use m2_ir::{Kind, LaneDecl, Unit};
use m2_song::{AudioClip, ControlClip, DataSet, LaneData, LaneKind, Meter, NoteClip, Project, Tempo};
use serde::Serialize;

use m2_session::{Home, Revision, Session};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineView {
    pub tempo: Vec<Tempo>,
    /// The time signatures (FMT-13); empty is 4/4 throughout.
    pub meter: Vec<Meter>,
    /// Where the tempo and the time signatures live (HOME-1): `data/arrangement.json`.
    pub home: Home,
    /// Where the last clip ends, in beats; at least one bar.
    pub end: f64,
    /// Modules with rows: tracks, and any other module that declares a lane.
    pub tracks: Vec<TimelineTrack>,
    /// The ruler's markers, by beat, from `layout.json` (Q13): none while it doesn't read.
    pub markers: Vec<crate::markers::Marker>,
    /// The song's key, from `layout.json` (the polish plan's D2): None when it says none.
    pub key: Option<crate::songkey::SongKey>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TimelineTrack {
    /// The module's key, as in the graph view: what edits and selection name it by (ID-11).
    pub module: String,
    /// What to show for it, never an address (ID-10).
    pub label: String,
    pub track: bool,
    /// It is a bus (RENDER-34): a track with no notes or audio lanes, which plays what the song
    /// feeds it. Written only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bus: bool,
    /// Its colour by palette name (taste.md §2.7), from `layout.json` or by its place; `graphite`
    /// for a module that isn't a track (the song's own lanes).
    pub colour: String,
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
        /// What the instrument that plays it calls its notes, by MIDI note (`36: "Kick"`): the
        /// piano roll's row names (polish A4). Empty when it names none.
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        names: BTreeMap<u8, String>,
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

/// The timeline of the accepted revision.
pub fn timeline(s: &Session) -> TimelineView {
    let rev = s.accepted();
    if !rev.has_code() {
        return last_known(rev);
    }
    let data = &rev.compiled.data;
    let layout = crate::layout::view(s);
    let tracks = rev.compiled.ir.modules.iter().filter(|m| m.track || !m.lanes.is_empty());
    let mut tracks: Vec<TimelineTrack> = tracks
        .map(|m| TimelineTrack {
            module: m.key.clone(),
            label: m.label().to_string(),
            track: m.track,
            bus: m.bus,
            colour: colour_of(&layout, &m.key, m.track),
            lanes: m.lanes.iter().map(|l| lane(rev, data, &m.key, l)).collect(),
        })
        .collect();
    in_order(&mut tracks, &layout);
    let a = &rev.project.arrangement;
    let bar = a.meter_at(0.0).bar();
    let markers = crate::markers::on_disk(&rev.project.dir);
    let key = crate::songkey::of(&rev.project.dir);
    TimelineView { tempo: a.tempo.clone(), meter: a.meter.clone(), home: Home::Arrangement, end: data.end().max(bar), tracks, markers, key }
}

/// A module's colour by name: a track's from the layout, else graphite.
fn colour_of(layout: &crate::layout::LayoutView, key: &str, track: bool) -> String {
    match (track, layout.colours.get(key)) {
        (true, Some(c)) => c.to_string(),
        _ => "graphite".into(),
    }
}

/// `tracks` in the layout's order (FMT-19, taste.md §3.4): the modules it doesn't list (the song's
/// own lanes) after, as they were.
fn in_order(tracks: &mut [TimelineTrack], layout: &crate::layout::LayoutView) {
    tracks.sort_by_key(|t| layout.order.iter().position(|k| *k == t.module).unwrap_or(usize::MAX));
}

/// A project opened with code that doesn't compile (WRITE-20) declares nothing yet, but its files
/// still say what the song is: a track for each folder of lanes under `data/`, each lane read as
/// what its clips hold, and the tempo and meter of `data/arrangement.json`. The app shows them
/// greyed until the code compiles, rather than an empty song at 120 BPM (taste.md TL-13, D2).
/// Nothing plays or can be edited from it.
fn last_known(rev: &Revision) -> TimelineView {
    let Ok(project) = Project::from_snapshot(&rev.snapshot) else {
        let a = &rev.project.arrangement;
        let markers = crate::markers::on_disk(&rev.project.dir);
        let key = crate::songkey::of(&rev.project.dir);
        return TimelineView {
            tempo: a.tempo.clone(),
            meter: a.meter.clone(),
            home: Home::Arrangement,
            end: a.meter_at(0.0).bar(),
            tracks: vec![],
            markers,
            key,
        };
    };
    let mut modules: BTreeMap<String, Vec<TimelineLane>> = BTreeMap::new();
    let mut end: f64 = 0.0;
    for (addr, _, text) in project.lane_files() {
        // No code says the labels yet: the keys, in sentence case, as a lane without a label reads.
        let (key, label) = (addr.lane.to_string(), sentence(addr.lane.as_str()));
        // Read as notes, as automation in any unit, then as audio: the first that fits is what it holds.
        let lane = if let Ok(LaneData::Notes(clips)) = LaneData::read(text, &LaneKind::Notes) {
            TimelineLane::Notes { key, label, clips, names: BTreeMap::new() }
        } else if let Ok(LaneData::Control { unit, clips }) = LaneData::read(text, &LaneKind::Control { unit: None }) {
            let unit = unit.and_then(|u| u.parse::<Unit>().ok());
            let (min, max) = range(unit, None, &clips);
            TimelineLane::Control { key, label, unit: unit.map(Unit::name), default: None, min, max, clips }
        } else if let Ok(LaneData::Audio(clips)) = LaneData::read(text, &LaneKind::Audio) {
            TimelineLane::Audio { key, label, channels: 2, clips }
        } else {
            continue;
        };
        end = end.max(lane_end(&lane));
        modules.entry(addr.module.to_string()).or_default().push(lane);
    }
    // Its colours and order as the layout says, over the folders there are.
    let keys: Vec<String> = modules.keys().filter(|k| *k != m2_song::ROOT).cloned().collect();
    let layout = crate::layout::effective(&keys, &crate::layout::read_disk(&rev.project.dir));
    let mut tracks: Vec<TimelineTrack> = modules
        .into_iter()
        .map(|(module, lanes)| {
            let track = module != m2_song::ROOT;
            TimelineTrack { label: sentence(&module), track, bus: false, colour: colour_of(&layout, &module, track), module, lanes }
        })
        .collect();
    in_order(&mut tracks, &layout);
    let a = &project.arrangement;
    let markers = crate::markers::on_disk(&rev.project.dir);
    let key = crate::songkey::of(&rev.project.dir);
    TimelineView {
        tempo: a.tempo.clone(),
        meter: a.meter.clone(),
        home: Home::Arrangement,
        end: end.max(a.meter_at(0.0).bar()),
        tracks,
        markers,
        key,
    }
}

/// `kick` → `Kick`.
fn sentence(key: &str) -> String {
    let words = key.replace('-', " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Where a lane's last clip ends, in beats.
fn lane_end(lane: &TimelineLane) -> f64 {
    let ends: Vec<f64> = match lane {
        TimelineLane::Notes { clips, .. } => clips.iter().map(|c| c.start + c.length).collect(),
        TimelineLane::Control { clips, .. } => clips.iter().map(|c| c.start + c.length).collect(),
        TimelineLane::Audio { clips, .. } => clips.iter().map(|c| c.start + c.length).collect(),
    };
    ends.into_iter().fold(0.0, f64::max)
}

/// A declared lane and the clips it plays, in canonical order (by start, then id), which is
/// also the order they win in where they overlap, so the last is drawn on top. A lane whose
/// file failed shows what it plays: its last accepted data, or nothing (FMT-9).
fn lane(rev: &Revision, data: &DataSet, module: &str, decl: &LaneDecl) -> TimelineLane {
    let (key, label) = (decl.key.clone(), decl.label().to_string());
    let lane = data.lane(module, &key).map(|d| &**d);
    match decl.kind {
        Kind::Events(_) => {
            let clips = if let Some(LaneData::Notes(c)) = lane { c.clone() } else { vec![] };
            let names = crate::note_names::of(rev, module, &key);
            TimelineLane::Notes { key, label, clips, names }
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
    use crate::Gestures;

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
            ("music2.toml", crate::testing::MANIFEST),
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

    /// D2: opened with code that doesn't compile, the timeline is what `data/` holds, not an empty song at 120 BPM.
    #[test]
    fn code_that_never_compiled_shows_the_data_and_the_arrangement() {
        let dir = project();
        std::fs::write(dir.join("song.ts"), format!("{SONG}\nconst broken = ;\n")).unwrap();
        let s = open(&dir);
        assert!(!s.accepted().has_code());
        let v = serde_json::to_value(s.timeline()).unwrap();
        assert_eq!((&v["tempo"], &v["meter"][0]["beats"], &v["end"]), (&json!([{"beat": 0.0, "bpm": 90.0}]), &json!(3), &json!(12.0)));
        let tracks: Vec<(&str, &str, bool)> = v["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (t["module"].as_str().unwrap(), t["label"].as_str().unwrap(), t["track"].as_bool().unwrap()))
            .collect();
        assert_eq!(tracks, [("bass", "Bass", true), ("vox", "Vox", true)]);
        let lanes = |m: usize| -> Vec<(String, String, usize)> {
            v["tracks"][m]["lanes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| (l["key"].as_str().unwrap().into(), l["kind"].as_str().unwrap().into(), l["clips"].as_array().unwrap().len()))
                .collect()
        };
        // Each read as what its clips hold: notes, automation in Hz, audio.
        assert_eq!(lanes(0), [("cutoff".into(), "control".into(), 1), ("notes".into(), "notes".into(), 2)]);
        assert_eq!(v["tracks"][0]["lanes"][0]["unit"], json!("hz"));
        assert_eq!(lanes(1), [("vox".into(), "audio".into(), 2)]);
        let _ = std::fs::remove_dir_all(&dir);
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

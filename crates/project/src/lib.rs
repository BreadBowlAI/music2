//! L1 — the song model. Pure data, serde JSON (`.m2song`). Source assets (sample files)
//! are separate from their uses on the timeline, like FilmCraft's media vs clip instances.
//!
//! Tracks and clips are behind `Arc`s: an edit copies only the path it touches
//! (`Arc::make_mut`), so snapshots for undo share almost everything and diffing two
//! songs can skip any subtree that is pointer-equal. Every object, notes included, has a
//! stable id.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use m2_theory::Key;
pub use m2_time::{PPQ, Tempo, Tick, TimeSig};
use serde::{Deserialize, Serialize};

pub type Id = u64;

/// 2: notes carry ids. 3: buses, sends, sidechains, automation; devices carry ids.
pub const FORMAT_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Song {
    pub format: u32,
    pub title: String,
    pub bpm: f64,
    pub time_sig: TimeSig,
    pub key: Key,
    pub tracks: Vec<Arc<Track>>,
    pub master: Master,
    #[serde(default)]
    pub assets: Vec<Asset>,
    pub next_id: Id,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Master {
    pub volume_db: f32,
    #[serde(default)]
    pub effects: Vec<Device>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Instrument,
    Audio,
    /// No clips: sums whatever is routed into it (outputs and sends), then processes it.
    Bus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: Id,
    pub name: String,
    pub color: [u8; 3],
    pub kind: TrackKind,
    /// Instrument tracks only.
    pub instrument: Option<Device>,
    #[serde(default)]
    pub effects: Vec<Device>,
    #[serde(default)]
    pub clips: Vec<Arc<Clip>>,
    pub volume_db: f32,
    pub pan: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    /// Bus this track feeds; `None` = master.
    #[serde(default)]
    pub output: Option<Id>,
    #[serde(default)]
    pub sends: Vec<Send>,
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
}

/// A copy of a track's signal into a bus (reverb/delay returns, parallel compression…).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Send {
    pub to: Id,
    pub gain_db: f32,
    /// Tap before the track's fader (level independent of the track volume).
    #[serde(default)]
    pub pre_fader: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationLane {
    pub id: Id,
    pub target: AutoTarget,
    /// Sorted by tick. Values: volume in dB, pan -1..1, macros 0..1. Linear in between.
    pub points: Vec<AutoPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "param", rename_all = "snake_case")]
pub enum AutoTarget {
    Volume,
    Pan,
    Macro {
        device: Id,
        #[serde(rename = "macro")]
        macro_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AutoPoint {
    pub tick: Tick,
    pub value: f32,
}

impl AutomationLane {
    pub fn value_at(&self, tick: f64) -> Option<f32> {
        let p = &self.points;
        let first = p.first()?;
        if tick <= first.tick as f64 {
            return Some(first.value);
        }
        let i = p.partition_point(|x| (x.tick as f64) <= tick);
        if i >= p.len() {
            return Some(p[p.len() - 1].value);
        }
        let (a, b) = (p[i - 1], p[i]);
        let f = ((tick - a.tick as f64) / (b.tick - a.tick).max(1) as f64) as f32;
        Some(a.value + (b.value - a.value) * f)
    }
}

/// An instance of an m2plug plugin: which plugin, and its macro/asset settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// Stable id (automation targets refer to it). 0 = not yet assigned.
    #[serde(default)]
    pub id: Id,
    pub plugin: String,
    #[serde(default)]
    pub macros: BTreeMap<String, f32>,
    #[serde(default)]
    pub assets: BTreeMap<String, Id>,
    #[serde(default)]
    pub bypass: bool,
    /// Track whose signal feeds this device's `sidechain` input (pre-fader, so a muted
    /// "ghost kick" can still drive ducking).
    #[serde(default)]
    pub sidechain: Option<Id>,
}

impl Device {
    /// A device with id 0; `Song::add_device_ids` (or `Song::device`) assigns one.
    pub fn new(plugin: impl Into<String>) -> Self {
        Self { id: 0, plugin: plugin.into(), macros: BTreeMap::new(), assets: BTreeMap::new(), bypass: false, sidechain: None }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub id: Id,
    pub name: String,
    pub start: Tick,
    pub length: Tick,
    pub content: ClipContent,
}

impl Clip {
    pub fn end(&self) -> Tick {
        self.start + self.length
    }

    pub fn notes(&self) -> &[Note] {
        match &self.content {
            ClipContent::Midi { notes } => notes,
            ClipContent::Audio { .. } => &[],
        }
    }

    pub fn notes_mut(&mut self) -> Option<&mut Vec<Note>> {
        match &mut self.content {
            ClipContent::Midi { notes } => Some(notes),
            ClipContent::Audio { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClipContent {
    Midi {
        notes: Vec<Note>,
    },
    /// Plays `asset` from `offset` seconds into the file.
    Audio {
        asset: Id,
        offset: f64,
        gain_db: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// Unique within the song. 0 = not yet assigned (format-1 files, new notes in commands).
    #[serde(default)]
    pub id: Id,
    pub pitch: u8,
    /// Ticks from the clip start.
    pub start: Tick,
    pub length: Tick,
    pub velocity: u8,
}

impl Note {
    pub fn end(&self) -> Tick {
        self.start + self.length
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub id: Id,
    pub name: String,
    /// Absolute, or relative to the song's assets directory.
    pub file: String,
    /// The text prompt it was generated from, if any.
    #[serde(default)]
    pub prompt: Option<String>,
    /// Pitch the sample plays at unshifted (sampler instrument).
    pub root_pitch: u8,
    pub duration: f32,
}

pub const TRACK_COLORS: [[u8; 3]; 8] = [
    [255, 94, 135],  // hot pink
    [255, 159, 67],  // tangerine
    [255, 214, 92],  // sunshine
    [94, 214, 140],  // mint
    [72, 196, 230],  // sky
    [124, 131, 253], // periwinkle
    [196, 113, 237], // lilac
    [240, 240, 240], // white
];

impl Default for Song {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            title: "Untitled".into(),
            bpm: 122.0,
            time_sig: TimeSig::default(),
            key: Key::default(),
            tracks: Vec::new(),
            master: Master { volume_db: 0.0, effects: vec![Device { id: 1, ..Device::new("m2/master") }] },
            assets: Vec::new(),
            next_id: 2,
        }
    }
}

/// Refer to a track by id or (case-insensitive) name. Commands accept either, which makes
/// the API friendly for both UIs and language models.
#[derive(Debug, Clone, PartialEq)]
pub enum TrackRef {
    Id(Id),
    Name(String),
}

impl Song {
    pub fn tempo(&self) -> Tempo {
        Tempo::new(self.bpm, self.time_sig)
    }

    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn next_color(&self) -> [u8; 3] {
        TRACK_COLORS[self.tracks.len() % TRACK_COLORS.len()]
    }

    pub fn track_index(&self, r: &TrackRef) -> Option<usize> {
        match r {
            TrackRef::Id(id) => self.tracks.iter().position(|t| t.id == *id),
            TrackRef::Name(n) => {
                let n = n.trim().to_lowercase();
                self.tracks
                    .iter()
                    .position(|t| t.name.to_lowercase() == n)
                    .or_else(|| self.tracks.iter().position(|t| t.name.to_lowercase().starts_with(&n)))
            }
        }
    }

    pub fn track(&self, id: Id) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id).map(|t| &**t)
    }

    /// Copy-on-write access to a track (clones it only if a snapshot still shares it).
    pub fn track_mut(&mut self, id: Id) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id).map(Arc::make_mut)
    }

    pub fn track_at_mut(&mut self, index: usize) -> &mut Track {
        Arc::make_mut(&mut self.tracks[index])
    }

    /// (track index, clip index) of a clip id.
    pub fn clip_location(&self, clip: Id) -> Option<(usize, usize)> {
        self.tracks.iter().enumerate().find_map(|(ti, t)| t.clips.iter().position(|c| c.id == clip).map(|ci| (ti, ci)))
    }

    pub fn clip(&self, id: Id) -> Option<&Clip> {
        self.clip_location(id).map(|(t, c)| &*self.tracks[t].clips[c])
    }

    pub fn clip_mut(&mut self, id: Id) -> Option<&mut Clip> {
        let (t, c) = self.clip_location(id)?;
        Some(Arc::make_mut(&mut self.track_at_mut(t).clips[c]))
    }

    /// Give every note with id 0 (or a duplicate id) a fresh one.
    pub fn assign_note_ids(&mut self, notes: &mut [Note]) {
        let mut seen = std::collections::HashSet::new();
        for n in notes {
            if n.id == 0 || n.id >= self.next_id || !seen.insert(n.id) {
                n.id = self.alloc_id();
                seen.insert(n.id);
            }
        }
    }

    /// Upgrade older formats in place.
    pub fn migrate(&mut self) {
        if self.format < 2 {
            for ti in 0..self.tracks.len() {
                for ci in 0..self.tracks[ti].clips.len() {
                    let mut notes = self.tracks[ti].clips[ci].notes().to_vec();
                    if notes.is_empty() {
                        continue;
                    }
                    for n in &mut notes {
                        n.id = 0;
                    }
                    self.assign_note_ids(&mut notes);
                    let clip = Arc::make_mut(&mut Arc::make_mut(&mut self.tracks[ti]).clips[ci]);
                    *clip.notes_mut().unwrap() = notes;
                }
            }
        }
        self.assign_device_ids();
        self.format = FORMAT_VERSION;
    }

    pub fn asset(&self, id: Id) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// End of the last clip (at least one bar).
    pub fn end_tick(&self) -> Tick {
        self.tracks.iter().flat_map(|t| t.clips.iter().map(|c| c.end())).max().unwrap_or(0).max(self.time_sig.ticks_per_bar())
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.solo)
    }

    /// Soloing a track keeps the buses it feeds audible.
    pub fn is_audible(&self, track: &Track) -> bool {
        if track.mute {
            return false;
        }
        if !self.any_solo() || track.solo {
            return true;
        }
        track.kind == TrackKind::Bus && self.tracks.iter().any(|t| t.solo && self.feeds(t, track.id))
    }

    /// Whether `from` reaches bus `to` through outputs or sends.
    pub fn feeds(&self, from: &Track, to: Id) -> bool {
        let mut stack = vec![from.id];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(t) = self.track(id) else { continue };
            for next in t.output.iter().chain(t.sends.iter().map(|s| &s.to)) {
                if *next == to {
                    return true;
                }
                stack.push(*next);
            }
        }
        false
    }

    /// Every device in the song, with its track (None = master).
    pub fn devices(&self) -> impl Iterator<Item = (Option<Id>, &Device)> {
        self.tracks
            .iter()
            .flat_map(|t| t.instrument.iter().chain(t.effects.iter()).map(move |d| (Some(t.id), d)))
            .chain(self.master.effects.iter().map(|d| (None, d)))
    }

    /// Track indices in processing order: sidechain sources before the devices they key,
    /// and every track before the bus it feeds. Errors on routing loops.
    pub fn routing_order(&self) -> Result<Vec<usize>> {
        let n = self.tracks.len();
        let index = |id: Id| self.tracks.iter().position(|t| t.id == id);
        let mut edges: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut indeg = vec![0usize; n];
        for (i, t) in self.tracks.iter().enumerate() {
            let mut add = |from: usize, to: usize| {
                edges[from].push(to);
                indeg[to] += 1;
            };
            for dest in t.output.iter().chain(t.sends.iter().map(|s| &s.to)) {
                let j = index(*dest).ok_or_else(|| anyhow!("`{}` routes to a missing track", t.name))?;
                if self.tracks[j].kind != TrackKind::Bus {
                    return Err(anyhow!("`{}` can only route into a bus, not `{}`", t.name, self.tracks[j].name));
                }
                add(i, j);
            }
            for d in t.instrument.iter().chain(t.effects.iter()) {
                if let Some(src) = d.sidechain {
                    let j = index(src).ok_or_else(|| anyhow!("sidechain source {src} is missing"))?;
                    add(j, i);
                }
            }
        }
        let mut order = Vec::with_capacity(n);
        let mut ready: Vec<usize> = (0..n).rev().filter(|i| indeg[*i] == 0).collect();
        while let Some(i) = ready.pop() {
            order.push(i);
            for &j in edges[i].iter().rev() {
                indeg[j] -= 1;
                if indeg[j] == 0 {
                    ready.push(j);
                }
            }
        }
        if order.len() != n {
            return Err(anyhow!("that would create a routing loop"));
        }
        Ok(order)
    }

    /// Give every device with id 0 a fresh id.
    pub fn assign_device_ids(&mut self) {
        let mut next = self.next_id;
        let mut fix = |d: &mut Device| {
            if d.id == 0 {
                d.id = next;
                next += 1;
            }
        };
        for t in &mut self.tracks {
            if t.instrument.iter().chain(t.effects.iter()).any(|d| d.id == 0) {
                let t = Arc::make_mut(t);
                t.instrument.iter_mut().chain(t.effects.iter_mut()).for_each(&mut fix);
            }
        }
        self.master.effects.iter_mut().for_each(&mut fix);
        self.next_id = next;
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(s: &str) -> Result<Song> {
        let mut song: Song = serde_json::from_str(s).context("not a valid .m2song file")?;
        if song.format > FORMAT_VERSION {
            return Err(anyhow!("song was made by a newer music2 (format {})", song.format));
        }
        song.migrate();
        song.validate()?;
        Ok(song)
    }

    /// Invariants: unique ids, ids below `next_id`, non-negative lengths, clip references resolve.
    pub fn validate(&self) -> Result<()> {
        self.routing_order()?;
        let mut seen = std::collections::HashSet::new();
        let mut check = |id: Id, what: &str| -> Result<()> {
            if id >= self.next_id || !seen.insert(id) {
                return Err(anyhow!("invalid or duplicate {what} id {id}"));
            }
            Ok(())
        };
        for a in &self.assets {
            check(a.id, "asset")?;
        }
        for (_, d) in self.devices() {
            check(d.id, "device")?;
        }
        for t in &self.tracks {
            check(t.id, "track")?;
            for lane in &t.automation {
                check(lane.id, "automation lane")?;
                if let AutoTarget::Macro { device, .. } = &lane.target
                    && !t.instrument.iter().chain(t.effects.iter()).any(|d| d.id == *device)
                {
                    return Err(anyhow!("automation on `{}` targets a missing device", t.name));
                }
            }
            for c in &t.clips {
                check(c.id, "clip")?;
                if c.length <= 0 || c.start < 0 {
                    return Err(anyhow!("clip {} has invalid range", c.id));
                }
                if let ClipContent::Audio { asset, .. } = c.content
                    && self.asset(asset).is_none()
                {
                    return Err(anyhow!("clip {} references missing asset {asset}", c.id));
                }
                for n in c.notes() {
                    check(n.id, "note")?;
                }
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Song> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Song::from_json(&text)
    }

    /// Atomic save: write a sibling temp file, then rename over the target.
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("m2song.tmp");
        std::fs::write(&tmp, self.to_json()?).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// `foo.m2song` → `foo.assets/`.
pub fn assets_dir_for(song_path: &Path) -> PathBuf {
    let stem = song_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "song".into());
    song_path.with_file_name(format!("{stem}.assets"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let mut s = Song::default();
        let id = s.alloc_id();
        let clip_id = s.alloc_id();
        let note_id = s.alloc_id();
        s.tracks.push(Arc::new(Track {
            id,
            name: "Bass".into(),
            color: s.next_color(),
            kind: TrackKind::Instrument,
            instrument: Some(Device::new("m2/bass")),
            effects: vec![],
            clips: vec![Arc::new(Clip {
                id: clip_id,
                name: "Bass".into(),
                start: 0,
                length: 4 * PPQ,
                content: ClipContent::Midi { notes: vec![Note { id: note_id, pitch: 45, start: 0, length: PPQ, velocity: 100 }] },
            })],
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: vec![],
            automation: vec![],
        }));
        s.assign_device_ids();
        let back = Song::from_json(&s.to_json().unwrap()).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.track_index(&TrackRef::Name("bass".into())), Some(0));
        assert_eq!(back.clip_location(clip_id), Some((0, 0)));
    }

    #[test]
    fn edits_share_untouched_tracks() {
        let mut a = Song::default();
        for _ in 0..3 {
            let id = a.alloc_id();
            a.tracks.push(Arc::new(Track {
                id,
                name: "t".into(),
                color: [0; 3],
                kind: TrackKind::Instrument,
                instrument: None,
                effects: vec![],
                clips: vec![],
                volume_db: 0.0,
                pan: 0.0,
                mute: false,
                solo: false,
                output: None,
                sends: vec![],
                automation: vec![],
            }));
        }
        let mut b = a.clone();
        let id = b.tracks[1].id;
        b.track_mut(id).unwrap().volume_db = -3.0;
        assert!(Arc::ptr_eq(&a.tracks[0], &b.tracks[0]));
        assert!(!Arc::ptr_eq(&a.tracks[1], &b.tracks[1]));
        assert_eq!(a.tracks[1].volume_db, 0.0, "snapshot untouched");
    }

    #[test]
    fn migrates_format_1_notes() {
        let v1 = r#"{"format":1,"title":"x","bpm":120,"time_sig":{"num":4,"den":4},"key":{"root":0,"scale":"major"},
          "tracks":[{"id":1,"name":"a","color":[0,0,0],"kind":"instrument","instrument":null,"volume_db":0,"pan":0,
            "clips":[{"id":2,"name":"c","start":0,"length":960,"content":{"type":"midi","notes":[
              {"pitch":60,"start":0,"length":240,"velocity":100},{"pitch":62,"start":240,"length":240,"velocity":100}]}}]}],
          "master":{"volume_db":0},"next_id":3}"#;
        let s = Song::from_json(v1).unwrap();
        assert_eq!(s.format, FORMAT_VERSION);
        let ids: Vec<Id> = s.tracks[0].clips[0].notes().iter().map(|n| n.id).collect();
        assert_eq!(ids, vec![3, 4]);
        assert_eq!(s.next_id, 5);
    }

    fn track(id: Id, kind: TrackKind) -> Arc<Track> {
        Arc::new(Track {
            id,
            name: format!("t{id}"),
            color: [0; 3],
            kind,
            instrument: None,
            effects: vec![],
            clips: vec![],
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            output: None,
            sends: vec![],
            automation: vec![],
        })
    }

    #[test]
    fn routing_order_and_loops() {
        let mut s = Song {
            next_id: 10,
            tracks: vec![track(1, TrackKind::Bus), track(2, TrackKind::Instrument), track(3, TrackKind::Bus)],
            ..Default::default()
        };
        Arc::make_mut(&mut s.tracks[1]).output = Some(1);
        Arc::make_mut(&mut s.tracks[1]).sends.push(Send { to: 3, gain_db: -6.0, pre_fader: false });
        let order = s.routing_order().unwrap();
        let pos = |i: usize| order.iter().position(|x| *x == i).unwrap();
        assert!(pos(1) < pos(0) && pos(1) < pos(2), "sources before buses: {order:?}");
        // Bus 1 → bus 3 → bus 1 is a loop.
        Arc::make_mut(&mut s.tracks[0]).output = Some(3);
        Arc::make_mut(&mut s.tracks[2]).output = Some(1);
        assert!(s.routing_order().unwrap_err().to_string().contains("loop"));
        // Only buses can receive.
        Arc::make_mut(&mut s.tracks[2]).output = Some(2);
        assert!(s.routing_order().is_err());
    }

    #[test]
    fn automation_interpolates() {
        let lane = AutomationLane {
            id: 1,
            target: AutoTarget::Volume,
            points: vec![AutoPoint { tick: 0, value: -12.0 }, AutoPoint { tick: 960, value: 0.0 }],
        };
        assert_eq!(lane.value_at(-5.0), Some(-12.0));
        assert_eq!(lane.value_at(480.0), Some(-6.0));
        assert_eq!(lane.value_at(5000.0), Some(0.0));
    }
}

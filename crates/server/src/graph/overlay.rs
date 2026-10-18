//! Monitoring mute and solo (MON-1 to MON-4): what the user mutes and solos while listening, how
//! each overlay site's value follows from it (the path rule), where it is kept, and how it reaches
//! the engine: cell writes on the control plane, never through the session (RENDER-29).
//!
//! - **The path rule (MON-3):** a site `m → c` (track `m`'s audio read in module `c`) is heard when
//!   no track is soloed, or when it lies on a path through a soloed track: it leaves one or
//!   something one feeds, or it feeds one or something that feeds one. Sidechain inputs aren't
//!   sites, so they are never edges here and never silenced (MON-2). The checker's routes (track
//!   `m` reaching bus `b` through the song, RENDER-34) are edges too, and a site `m → song` feeds
//!   each bus `m` routes to.
//! - **Mute wins:** a track muted while listening or in the song is silent, soloed or not.
//! - **The file (MON-4):** `.music2/overlays.json`, `{"mute": [...], "solo": [...]}`, sorted. Read at
//!   open; written by the server's `music2-tell` thread after a change, and before the project
//!   closes or opens again (a temp file, then a rename), never under the control lock. A read-only window keeps them in memory only. Entries
//!   of a track that is gone stay, so it comes back as it was; meanwhile they are ignored, so a
//!   solo of a track that is gone never silences the rest.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_runtime::{CellId, CellTarget, PlanInfo};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::lease::PlaysView;
use super::{Device, Graph, Inner};

/// The root module: never muted or soloed (MON-1).
const SONG: &str = "song";

/// What the user mutes and solos while listening, by track key (MON-4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overlays {
    #[serde(default)]
    pub mute: BTreeSet<String>,
    #[serde(default)]
    pub solo: BTreeSet<String>,
}

impl Overlays {
    pub fn is_empty(&self) -> bool {
        self.mute.is_empty() && self.solo.is_empty()
    }
}

/// A change, as `POST /api/graph/overlay` sends it: `clear` first, then `exclusive` (solo only this
/// track), then each mute and solo set or cleared.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Change {
    #[serde(default)]
    pub mute: BTreeMap<String, bool>,
    #[serde(default)]
    pub solo: BTreeMap<String, bool>,
    #[serde(default)]
    pub exclusive: Option<String>,
    #[serde(default)]
    pub clear: bool,
}

/// A site as the server reads it: the track whose audio it carries, and the module that reads it.
pub type Site = (String, String);

/// Each site's value (MON-3): 0 when its track is muted while listening or in the song, or when a
/// solo leaves it off every path through a soloed track; else 1. Only solos of `tracks`, the
/// revision's tracks, count: one of a track that is gone stays in the file and is ignored (MON-4).
pub fn values(sites: &[Site], routes: &[Site], o: &Overlays, muted: &[String], tracks: &[String]) -> Vec<f32> {
    let solo: Vec<&String> = o.solo.iter().filter(|s| tracks.contains(s)).collect();
    // Who feeds whom, over the sites (sidechains aren't sites, so aren't edges) and the routes:
    // track `m` reaching bus `b` through the song (RENDER-34).
    let mut feeds: HashMap<&str, Vec<&str>> = HashMap::new();
    for (m, c) in sites.iter().chain(routes) {
        feeds.entry(m.as_str()).or_default().push(c.as_str());
    }
    let reaches = |from: &str, to: &str| {
        let (mut seen, mut stack) = (BTreeSet::new(), vec![from]);
        while let Some(x) = stack.pop() {
            if x == to {
                return true;
            }
            if seen.insert(x) {
                stack.extend(feeds.get(x).into_iter().flatten().copied());
            }
        }
        false
    };
    // A site into the song feeds the song, and through it each bus its track routes to.
    let on_path = |m: &str, c: &str| {
        solo.is_empty()
            || solo.iter().any(|s| reaches(s, m) || reaches(c, s) || (c == SONG && routes.iter().any(|(r, b)| r == m && reaches(b, s))))
    };
    sites
        .iter()
        .map(|(m, c)| {
            let silent = muted.iter().any(|x| x == m) || o.mute.contains(m) || !on_path(m, c);
            if silent { 0.0 } else { 1.0 }
        })
        .collect()
}

/// The sites of a plan, as its info lists them.
fn sites_of(info: &PlanInfo) -> Vec<Site> {
    info.overlays.iter().map(|(m, c, _)| (m.clone(), c.clone())).collect()
}

/// The values for every site of a plan of `rev`: what a renderer is given before it is sent
/// ([`m2_runtime::Renderer::set_overlays`]).
pub fn plan_values(info: &PlanInfo, o: &Overlays, rev: &m2_session::Revision) -> Vec<f32> {
    values(&sites_of(info), &rev.compiled.flat.routes, o, &rev.compiled.flat.muted, &tracks(rev))
}

/// The track keys of `rev`.
pub fn tracks(rev: &m2_session::Revision) -> Vec<String> {
    rev.compiled.ir.modules.iter().filter(|m| m.track).map(|m| m.key.clone()).collect()
}

/// Write the cells of the plan `info` describes, of a revision with `tracks` and `routes`, in the
/// player of `epoch`, whose value differs between `before` and `after` (each a set and the tracks
/// muted in the song): two cell writes at most, which glide there in the engine. Never waits.
pub(super) fn write_cells(
    info: &PlanInfo,
    epoch: u64,
    device: &Device,
    (tracks, routes): (&[String], &[Site]),
    before: (&Overlays, &[String]),
    after: (&Overlays, &[String]),
) {
    if epoch != device.epoch {
        return;
    }
    let sites = sites_of(info);
    let (was, now) = (values(&sites, routes, before.0, before.1, tracks), values(&sites, routes, after.0, after.1, tracks));
    for target in [0.0, 1.0] {
        let cells: Vec<CellId> =
            info.overlays.iter().zip(was.iter().zip(&now)).filter(|(_, (a, b))| a != b && **b == target).map(|(o, _)| o.2).collect();
        if !cells.is_empty() {
            device.player.set_cells(&CellTarget { slots: info.cell_slots.clone(), cells: cells.into() }, target as f64);
        }
    }
}

/// [`write_cells`] into what plays.
fn write_plays(plays: &PlaysView, device: &Device, before: &Overlays, after: &Overlays) {
    write_cells(&plays.info, plays.epoch, device, (&plays.tracks, &plays.routes), (before, &plays.muted), (after, &plays.muted));
}

/// The accepted revision as the overlay state reads it: its tracks, its sites and routes, its
/// tracks muted in the song, and what each track drives besides its sound (MON-2).
#[derive(Clone, Debug, Default)]
pub struct Song {
    pub tracks: Vec<String>,
    pub sites: Vec<Site>,
    /// Which bus each track reaches through the song (RENDER-34, MON-3).
    pub routes: Vec<Site>,
    pub muted: Vec<String>,
    pub feeds: BTreeMap<String, Vec<m2_editor::mute::Feed>>,
}

impl Song {
    pub fn of(rev: &m2_session::Revision) -> Song {
        let c = &rev.compiled;
        Song {
            tracks: tracks(rev),
            sites: c.flat.overlays.iter().map(|s| (s.module.clone(), s.into.clone())).collect(),
            routes: c.flat.routes.clone(),
            muted: c.flat.muted.clone(),
            feeds: m2_editor::mute::feeds(rev),
        }
    }
}

/// The open project's overlays, on the control plane: made new at each open (M6).
pub(super) struct Monitor {
    pub(super) set: Overlays,
    /// Changes made, and the last of them written to the file.
    version: u64,
    written: u64,
    /// The project's folder, and whether this window may write it: a read-only one keeps its
    /// overlays in memory only (MON-4).
    dir: PathBuf,
    writer: bool,
    pub(super) song: Arc<Song>,
}

impl Monitor {
    /// The overlays saved in `dir`, for a window that may write it or not, playing `rev`.
    pub(super) fn open(dir: &Path, writer: bool, rev: &m2_session::Revision) -> Monitor {
        Monitor { set: read(dir), version: 0, written: 0, dir: dir.to_path_buf(), writer, song: Arc::new(Song::of(rev)) }
    }

    /// The accepted revision is `song` now (None: its code didn't change), and this window may
    /// write or not.
    pub(super) fn follow(&mut self, song: Option<Song>, writer: bool) {
        if let Some(song) = song {
            self.song = Arc::new(song);
        }
        self.writer = writer;
    }

    /// The state's `overlays`: the sets, the tracks muted in the song, how each track is heard,
    /// and whether they are saved.
    pub(super) fn state(&self) -> Value {
        let s = &self.song;
        let now = values(&s.sites, &s.routes, &self.set, &s.muted, &s.tracks);
        let heard: serde_json::Map<String, Value> = s
            .tracks
            .iter()
            .map(|k| {
                let mine: Vec<f32> = s.sites.iter().zip(&now).filter(|((m, _), _)| m == k).map(|(_, v)| *v).collect();
                let how = if s.muted.contains(k) {
                    "muted-in-song"
                } else if self.set.mute.contains(k) {
                    "muted"
                } else if !mine.is_empty() && mine.iter().all(|v| *v == 0.0) {
                    "silenced"
                } else {
                    "heard"
                };
                (k.clone(), json!(how))
            })
            .collect();
        json!({
            "mute": self.set.mute,
            "solo": self.set.solo,
            "muted_in_song": s.muted,
            "heard": heard,
            "feeds": s.feeds,
            "saved": self.writer,
        })
    }

    /// Whether any monitoring mute or solo changes what is heard (the export's `overlays.active`).
    pub(super) fn active(&self) -> bool {
        let s = &self.song;
        values(&s.sites, &s.routes, &self.set, &s.muted, &s.tracks) != values(&s.sites, &s.routes, &Overlays::default(), &s.muted, &s.tracks)
    }

    /// Set to `set`: marked to be written.
    pub(super) fn replace(&mut self, set: Overlays) {
        if set != self.set {
            self.set = set;
            self.version += 1;
        }
    }

    /// A track renamed (`Rekeyed`): its entries follow it.
    pub(super) fn rekey(&mut self, from: &str, to: &str) {
        let mut set = self.set.clone();
        for s in [&mut set.mute, &mut set.solo] {
            if s.remove(from) {
                s.insert(to.to_string());
            }
        }
        self.replace(set);
    }
}

/// `.music2/overlays.json` in `dir`.
fn file(dir: &Path) -> PathBuf {
    dir.join(".music2").join("overlays.json")
}

/// The overlays saved in `dir`: none when there is no file, or it doesn't read.
pub fn read(dir: &Path) -> Overlays {
    std::fs::read_to_string(file(dir)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Save `o` in `dir`: a temp file in `.music2/`, then a rename, so a reader never sees half.
fn write(dir: &Path, o: &Overlays) -> std::io::Result<()> {
    let to = file(dir);
    let folder = to.parent().expect("in .music2");
    std::fs::create_dir_all(folder)?;
    let tmp = folder.join("overlays.json.tmp");
    let mut text = serde_json::to_string_pretty(o).expect("overlays serialise");
    text.push('\n');
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &to)
}

/// A write `.music2/overlays.json` still owes: the overlays changed since they were last written,
/// and the window may write.
pub(super) struct Owed {
    token: u64,
    dir: PathBuf,
    set: Overlays,
    version: u64,
}

impl super::Published {
    /// What its overlays still owe the file, if anything.
    pub(super) fn owed(&self) -> Option<Owed> {
        let m = &self.overlays;
        (m.writer && m.version != m.written).then(|| Owed { token: self.engine.token, dir: m.dir.clone(), set: m.set.clone(), version: m.version })
    }
}

impl Inner {
    /// Write what `owed` says, off the control lock; then, if its project is still open, mark it
    /// written.
    pub(super) fn save_owed(&self, owed: Option<Owed>) {
        let Some(Owed { token, dir, set, version }) = owed else { return };
        if let Err(e) = write(&dir, &set) {
            eprintln!("music2: couldn't save the mutes and solos in .music2/overlays.json: {e}");
            return;
        }
        let mut cp = self.control.lock();
        if let Some(p) = cp.open.as_mut().filter(|p| p.engine.token == token) {
            p.overlays.written = p.overlays.written.max(version);
        }
    }

    /// The overlays of the open project, if `token` names it.
    fn monitor(cp: &mut super::ControlPlane, token: u64) -> Result<&mut super::Published> {
        cp.open.as_mut().filter(|p| p.engine.token == token).context("the open project changed; refresh before muting")
    }
}

impl Graph {
    /// Mute or solo tracks while listening (MON-1 to MON-4): the cells of what plays glide to their
    /// new values at once, on the control plane; the file is written soon after. Works in a
    /// read-only window, in memory. Answers with the state's `overlays`.
    pub fn overlay(&self, token: u64, change: Change) -> Result<Value> {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let p = Inner::monitor(&mut cp, token)?;
        let named = change.mute.iter().chain(&change.solo).map(|(k, on)| (k, *on)).chain(change.exclusive.iter().map(|k| (k, true)));
        for (k, on) in named {
            if k == SONG {
                bail!("The song can't be muted or soloed: mute or solo its tracks.");
            }
            if on && !p.overlays.song.tracks.contains(k) {
                bail!("There's no track \"{k}\" to mute or solo.");
            }
        }
        let mut set = if change.clear { Overlays::default() } else { p.overlays.set.clone() };
        if let Some(k) = &change.exclusive {
            set.solo = [k.clone()].into();
        }
        for (map, into) in [(&change.mute, &mut set.mute), (&change.solo, &mut set.solo)] {
            for (k, on) in map {
                if *on { into.insert(k.clone()) } else { into.remove(k) };
            }
        }
        let before = p.overlays.set.clone();
        p.overlays.replace(set);
        if let (Some(plays), Some(d)) = (&p.engine.plays, &device) {
            write_plays(plays, d, &before, &p.overlays.set);
        }
        Ok(p.overlays.state())
    }

    /// Mute `modules` in the song, or unmute them (MON-5): one `Confined` Tx on their `track()`
    /// calls' options, one undo step. What plays takes it as overlay cell writes.
    pub fn mute_in_song(&self, token: u64, modules: Vec<String>, muted: bool) -> Result<Value> {
        self.edit(token, move |s| m2_editor::mute::mute_in_song(s, &modules, muted))
    }

    /// Drop every mute and solo of `keys` (a track deleted with a builder, or added with a key one
    /// left behind): Q7. What plays follows.
    pub fn forget_overlays(&self, keys: &[String]) {
        let device = self.inner.current_device();
        let mut cp = self.inner.control.lock();
        let Some(p) = cp.open.as_mut() else { return };
        let before = p.overlays.set.clone();
        let mut set = before.clone();
        for k in keys {
            set.mute.remove(k);
            set.solo.remove(k);
        }
        p.overlays.replace(set);
        if let (Some(plays), Some(d)) = (&p.engine.plays, &device) {
            write_plays(plays, d, &before, &p.overlays.set);
        }
    }

    /// Write the overlays to `.music2/overlays.json` if they changed since they were last written
    /// and this window may write: off the control lock, from the server's `music2-tell` thread.
    pub fn save_overlays(&self) {
        let owed = self.inner.control.lock().open.as_ref().and_then(|p| p.owed());
        self.inner.save_owed(owed);
    }

    /// The export's Warning when monitoring mutes or solos change what is heard (MON-4): the file
    /// has every track, as the song says.
    pub(super) fn overlays_warning(&self) -> Option<m2_song::Diagnostic> {
        let cp = self.inner.control.lock();
        let m = &cp.open.as_ref()?.overlays;
        m.active().then(|| {
            let muted: Vec<&str> = m.set.mute.iter().filter(|k| m.song.tracks.contains(k)).map(String::as_str).collect();
            let solo: Vec<&str> = m.set.solo.iter().filter(|k| m.song.tracks.contains(k)).map(String::as_str).collect();
            let what = match (muted.is_empty(), solo.is_empty()) {
                (false, true) => format!("{} muted only while you listen", muted.join(", ")),
                (true, false) => format!("{} soloed", solo.join(", ")),
                _ => format!("{} muted and {} soloed only while you listen", muted.join(", "), solo.join(", ")),
            };
            m2_song::Diagnostic::warning("overlays.active", format!("{what}: the file has every track, as the song says."))
        })
    }
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;

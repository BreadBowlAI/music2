//! The library on the server (LIB-1 to LIB-3): its items for the open song, and previews, kept in
//! the cache folder.
//!
//! The items come from the open song's build (its packages and its own components), or std's with
//! no song open; std's presets and yours; and the plugins the app has listed, never a scan
//! (AGENT-11), nor waiting for one. [`preview`] makes an item's preview with
//! `m2_editor::library::preview`, which renders on a thread of its own at background priority
//! (never the audio thread or the session's), with std and the item's own package loaded: kept in
//! the cache as `<item>-<what it renders>.wav`, each a hash, so a second ask renders nothing and a
//! changed preset, package or music2 renders anew, and the item's older preview goes. One render of
//! an item at a time: a second ask waits for the first, then finds its file; an answer is read
//! before the next render of the item can replace it. Your presets and the cache are the folders
//! the server keeps ([`Folders`]).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use m2_editor::library::{self, Item, Plugin, Sources, preview as made};
use m2_editor::presets;
use parking_lot::Mutex;

use crate::AppState;

/// The previews being made: one lock per item, so a second ask waits for the first's render.
#[derive(Default)]
pub struct Previews {
    making: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// Where the library reads your presets and keeps its previews (LIB-1, LIB-3), as the server keeps
/// them ([`AppState::keep_library`]). None of either: no presets of yours, and no previews.
#[derive(Clone, Debug, Default)]
pub struct Folders {
    /// Your Presets folder.
    pub presets: Option<PathBuf>,
    /// Where previews are kept: `previews` in music2's cache folder.
    pub previews: Option<PathBuf>,
}

impl Folders {
    /// This user's: the Presets folder (`m2_os::dirs::presets`) and `previews` in music2's cache
    /// folder (`m2_os::dirs::cache`).
    pub fn of_this_user() -> Folders {
        Folders { presets: m2_os::dirs::presets(), previews: m2_os::dirs::cache().map(|c| c.join("previews")) }
    }
}

/// A preview, as [`preview`] answers it: its WAV, read while the item's lock was held, its file,
/// and whether this ask rendered it.
#[derive(Debug)]
pub struct Preview {
    pub wav: Vec<u8>,
    pub file: PathBuf,
    pub rendered: bool,
}

/// std, found as the app finds it: what a build loads with no song open.
fn std_only() -> Vec<(String, PathBuf)> {
    vec![("std".to_string(), crate::resources::std_dir())]
}

/// How old a part file of another render must be to be taken for one that failed: a render writes
/// its part file in well under a second.
const STALE_PART: Duration = Duration::from_secs(60);

/// The plugins in the app's kept list (`GET /api/plugins`), as the library takes them.
fn listed(kept: Option<&serde_json::Value>) -> Vec<Plugin> {
    let list = kept.and_then(|k| k["plugins"].as_array()).cloned().unwrap_or_default();
    list.iter()
        .filter_map(|p| {
            Some(Plugin {
                id: p["id"].as_str()?.to_string(),
                name: p["name"].as_str()?.to_string(),
                vendor: p["vendor"].as_str().unwrap_or_default().to_string(),
                instrument: p["instrument"].as_bool().unwrap_or(false),
                effect: p["effect"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}

impl AppState {
    /// Every item of the library (LIB-1): the open song's build's components, or std's with no
    /// song open; std's presets and yours; the plugins the app has listed. Reads files: call it
    /// off the async threads.
    pub fn library(&self) -> Result<Vec<Item>> {
        let catalog = match self.graph.library()? {
            Some((c, _)) => c,
            None => m2_script::describe(&std_only()),
        };
        // The list kept from before any scan that runs now: the scan holds another lock.
        let plugins = listed(self.plugins.lock().as_ref());
        let (std, yours) = (crate::resources::std_dir(), self.library_folders().presets);
        Ok(library::items(&Sources { catalog: &catalog, std: Some(&std), yours: yours.as_deref(), plugins: &plugins }))
    }

    /// The packages a preview may load (LIB-3): the open song's build's, or std alone.
    pub fn preview_packages(&self) -> Result<Vec<(String, PathBuf)>> {
        Ok(self.graph.library()?.map_or_else(std_only, |(_, packages)| packages))
    }
}

/// A short hash of `text`, for a file name.
fn short(text: &[u8]) -> String {
    m2_song::Sha::of(text).hex()[..16].to_string()
}

/// The preview of item `id` (LIB-3), in the library `packages` (the open song's, or std alone; std
/// among them) and `yours` (your Presets folder) make: its WAV in `cache`, rendered now with std and
/// the item's own package loaded, unless kept there. Why there is none, in words.
pub fn preview(previews: &Previews, id: &str, packages: &[(String, PathBuf)], yours: Option<&Path>, cache: &Path) -> Result<Preview> {
    let catalog = m2_script::describe(packages);
    let std = packages.iter().find(|(n, _)| n == "std").map(|(_, d)| d.clone()).context("no std package is loaded")?;
    let items = library::items(&Sources { catalog: &catalog, std: Some(&std), yours, plugins: &[] });
    let item = items.iter().find(|i| i.id == id).ok_or_else(|| anyhow!("There is no {id} in the library."))?;
    // std (the song's mixer, an effect's loop) and the item's own package.
    let own = item.component.split_once(':').map_or("", |(p, _)| p);
    let packages: Vec<(String, PathBuf)> = packages.iter().filter(|(n, _)| n == "std" || n == own).cloned().collect();
    let values = match &item.preset {
        Some(p) => {
            let dir = match p.from.as_str() {
                "std" => presets::factory(&std, &item.component),
                _ => yours.map(|y| y.join(presets::folder(&item.component))),
            };
            let dir = dir.ok_or_else(|| anyhow!("{} can't be found.", p.name))?;
            presets::read(&dir, &item.component, &p.name).map_err(|e| anyhow!(e))?.values
        }
        None => BTreeMap::new(),
    };
    let song = made::song(item, &catalog, &values).map_err(|e| anyhow!(e))?;
    let named = short(id.as_bytes());
    let file = cache.join(format!("{named}-{}.wav", &made::fingerprint(&song, &packages).hex()[..16]));
    let lock = previews.making.lock().entry(named.clone()).or_default().clone();
    let _making = lock.lock();
    if let Ok(wav) = std::fs::read(&file) {
        return Ok(Preview { wav, file, rendered: false });
    }
    made::render(&song, &packages, &cache.join(format!("making-{named}")), &file)?;
    // The item's older previews, of what it rendered before, and the part file of a render that
    // stopped half way (a crash): not one that may be another app's at work.
    for old in std::fs::read_dir(cache).into_iter().flatten().flatten().map(|e| e.path()) {
        let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stale = || old.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age > STALE_PART));
        if old != file && name.starts_with(&format!("{named}-")) && (name.ends_with(".wav") || name.ends_with(".part") && stale()) {
            let _ = std::fs::remove_file(&old);
        }
    }
    Ok(Preview { wav: std::fs::read(&file)?, file, rendered: true })
}

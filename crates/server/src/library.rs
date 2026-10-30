//! The library on the server (LIB-1 to LIB-3): its items for the open song, and previews, kept in
//! the cache folder.
//!
//! The items come from the open song's build (its packages and its own components), or std's with
//! no song open; std's presets and yours; and the plugins the app has listed, never a scan
//! (AGENT-11). [`preview`] makes an item's preview with `m2_editor::library::preview`, on the
//! caller's thread (a blocking one, never the audio thread or the session's): kept in the cache as
//! `<item>-<what it renders>.wav`, each a hash, so a second ask renders nothing and a changed preset
//! or package renders anew, and the item's older preview goes. One render of an item at a time: a
//! second ask waits for the first, then finds its file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use m2_editor::library::{self, Item, Plugin, Sources, preview as made};
use m2_editor::presets;
use parking_lot::Mutex;

use crate::AppState;

/// The previews being made: one lock per item, so a second ask waits for the first's render.
#[derive(Default)]
pub struct Previews {
    making: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// A preview, as [`preview`] answers it: its file, and whether this ask rendered it.
#[derive(Debug)]
pub struct Preview {
    pub file: PathBuf,
    pub rendered: bool,
}

/// The packages a preview loads: std, found as the app finds it. Packs join it later.
fn packages() -> Vec<(String, PathBuf)> {
    vec![("std".to_string(), crate::resources::std_dir())]
}

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
        let catalog = match self.graph.catalog()? {
            Some(c) => c,
            None => m2_script::describe(&packages()),
        };
        let plugins = listed(self.plugins.lock().as_ref());
        let (std, yours) = (crate::resources::std_dir(), m2_os::dirs::presets());
        Ok(library::items(&Sources { catalog: &catalog, std: Some(&std), yours: yours.as_deref(), plugins: &plugins }))
    }
}

/// A short hash of `text`, for a file name.
fn short(text: &[u8]) -> String {
    m2_song::Sha::of(text).hex()[..16].to_string()
}

/// The preview of item `id` (LIB-3), in the library std and `yours` (your Presets folder) make: its
/// WAV in `cache`, rendered now unless kept there. Why there is none, in words.
pub fn preview(previews: &Previews, id: &str, yours: Option<&Path>, cache: &Path) -> Result<Preview> {
    let packages = packages();
    let catalog = m2_script::describe(&packages);
    let std = crate::resources::std_dir();
    let items = library::items(&Sources { catalog: &catalog, std: Some(&std), yours, plugins: &[] });
    let item = items.iter().find(|i| i.id == id).ok_or_else(|| anyhow!("There is no {id} in the library."))?;
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
    if file.is_file() {
        return Ok(Preview { file, rendered: false });
    }
    made::render(&song, &packages, &cache.join(format!("making-{named}")), &file)?;
    // The item's older previews, of what it rendered before.
    for old in std::fs::read_dir(cache).into_iter().flatten().flatten().map(|e| e.path()) {
        let name = old.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if old != file && name.starts_with(&format!("{named}-")) && name.ends_with(".wav") {
            let _ = std::fs::remove_file(&old);
        }
    }
    Ok(Preview { file, rendered: true })
}

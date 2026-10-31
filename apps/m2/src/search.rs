//! `m2 search <words…> [--kind <kind>] [--json]` (LIB-2): the library's one search, as ⌘F asks it.
//! With the app open it asks the app (`GET /api/library`), whose list holds the open song's own
//! components and the plugins it has listed; with the app closed it gathers the same list itself,
//! from std, the store's packs, your presets and your sample folders, and searches it with the same
//! function. After them come From the library's items (REG-13), each marked `library`: the
//! registry's catalog, kept on this computer, searched here, so nothing typed is sent.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, bail};
use m2_editor::library::samples::{self, Indexed, Root};
use m2_editor::library::{self, Kind, Library, Query};
use serde_json::Value;

/// The packages with the app closed: std, found as the app finds it, and the store's packs the
/// lists offer (PACK-11).
pub fn packages() -> Vec<(String, PathBuf)> {
    let std = m2_server::resources::std_dir();
    let mut packages = vec![("std".to_string(), std.clone())];
    if let Some(store) = crate::packs_store() {
        packages.extend(library::store_packs(&store.dir, &std));
    }
    packages
}

/// Your sample folders, as this computer's settings keep them (`~/.music2/settings.json`, LIB-5).
pub fn sample_folders() -> Vec<PathBuf> {
    let file = m2_os::dirs::home().map(|h| h.join(".music2").join("settings.json"));
    let kept: Option<Value> = file.and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    kept.and_then(|k| serde_json::from_value(k["samples"].clone()).ok()).unwrap_or_default()
}

/// Where what is measured of samples is kept (LIB-5): `samples` in the cache folder.
fn samples_cache() -> Option<PathBuf> {
    m2_os::dirs::cache().map(|c| c.join("samples"))
}

/// The samples of `packages`, indexed now through the cache folder's measures (LIB-5).
pub fn packages_samples(packages: &[(String, PathBuf)]) -> Vec<Arc<Indexed>> {
    let cache = samples_cache();
    packages.iter().map(|(name, dir)| Arc::new(samples::index(Root::Package { name, dir }, cache.as_deref()))).collect()
}

/// The samples of your folders, indexed now through the cache folder's measures (LIB-5); what
/// each skipped is said. A folder inside another of yours is that one's.
pub fn folders_samples() -> Vec<Arc<Indexed>> {
    let cache = samples_cache();
    let yours = sample_folders();
    let mut out = vec![];
    for dir in yours.iter().filter(|d| !yours.iter().any(|o| o != *d && d.starts_with(o))) {
        let found = samples::index(Root::Folder(dir), cache.as_deref());
        if found.missing {
            eprintln!("{} isn't there: its samples are left out.", dir.display());
        }
        if found.not_wav > 0 {
            eprintln!("{}: {} audio files that aren't WAV or FLAC were skipped: WAV and FLAC only for now.", dir.display(), found.not_wav);
        }
        out.push(Arc::new(found));
    }
    out
}

/// The library with the app closed (LIB-2): std's and the store's packs' components, their presets
/// and yours, and the samples of both and of your folders.
pub fn closed() -> Result<Library> {
    let packages = packages();
    let yours = m2_os::dirs::presets();
    let samples = [packages_samples(&packages), folders_samples()].concat();
    Ok(library::without_song(&packages, yours.as_deref(), &[], &samples))
}

/// The sample with id `id` in the library of the song `s` (LIB-7), as its own build lists it with
/// the app closed: a package's from std, the song's packs at their locked versions, or the store's
/// other packs (only the one the id names is indexed); one of yours only for a `sample:folder:` id.
pub fn song_sample(s: &m2_session::Session, id: &str) -> Result<library::Sample> {
    let indexes = if id.starts_with("sample:folder:") {
        folders_samples()
    } else {
        let named = id.strip_prefix("sample:").and_then(|r| r.split_once(':')).map(|(n, _)| n);
        let std = s.packages.get(m2_song::pack::STD).map(|p| (p.name.clone(), p.dir.clone()));
        let packs = s.offered().iter().map(|(n, _, dir)| (n.clone(), dir.clone()));
        let packages: Vec<(String, PathBuf)> = std.into_iter().chain(packs).filter(|(n, _)| Some(n.as_str()) == named).collect();
        packages_samples(&packages)
    };
    indexes
        .iter()
        .flat_map(|i| i.items.iter())
        .find(|i| i.id == id)
        .and_then(|i| i.sample.clone())
        .ok_or_else(|| anyhow::anyhow!("There is no sample {id} in the library: `m2 search --kind sample` lists them."))
}

/// Run `m2 search`.
pub fn search(args: &[String]) -> Result<()> {
    let (mut words, mut json) = (vec![], false);
    let mut rest = args.iter().skip(1);
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--json" => json = true,
            "--kind" => match rest.next() {
                Some(k) if Kind::named(k).is_some() => words.push(k.clone()),
                other => bail!("--kind is instrument, sound, effect, sample or plugin, not {}", other.map_or("nothing", String::as_str)),
            },
            w => words.push(w.to_string()),
        }
    }
    let q = words.join(" ");
    let items: Vec<Value> = match crate::app::library(&q)? {
        Some(items) => items,
        None => {
            let all = closed()?;
            let query = Query::parse(&q);
            let mut found: Vec<Value> = all.search(&query).into_iter().map(serde_json::to_value).collect::<Result<_, _>>()?;
            let offers = crate::get::offers();
            found.extend(m2_editor::get::find(&offers, &query).into_iter().map(|o| o.json()));
            found
        }
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if items.is_empty() {
        eprintln!("Nothing in the library matches {q}.");
        return Ok(());
    }
    // A word from the registry's catalog is shown with any control character written out.
    let text = |v: &Value, k: &str| crate::get::shown(v[k].as_str().unwrap_or_default());
    // Each item's columns: its label, its kind, what it is and where it is from, and its id.
    let rows: Vec<[String; 4]> = items
        .iter()
        .map(|i| {
            let what =
                if text(i, "kind") == "sound" { crate::get::shown(i["tags"][0].as_str().unwrap_or_default()) } else { text(i, "category") };
            let library = if i.get("library").is_some() { " · library" } else { "" };
            [text(i, "label"), text(i, "kind"), format!("{what} · {}{library}", text(i, "package")), text(i, "id")]
        })
        .collect();
    let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0);
    let (w0, w1, w2) = (width(0), width(1), width(2));
    let mut out = String::new();
    for [label, kind, what, id] in &rows {
        out += &format!("{label:w0$}  {kind:w1$}  {what:w2$}  {id}\n");
    }
    print!("{out}");
    Ok(())
}

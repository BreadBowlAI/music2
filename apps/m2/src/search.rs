//! `m2 search <words…> [--kind <kind>] [--json]` (LIB-2): the library's one search, as ⌘F asks it.
//! With the app open it asks the app (`GET /api/library`), whose list holds the open song's own
//! components and the plugins it has listed; with the app closed it gathers the same list itself,
//! from std, the store's packs, your presets and your sample folders, and searches it with the same
//! function.

use std::path::PathBuf;

use anyhow::{Result, bail};
use m2_editor::library::samples::{self, Root};
use m2_editor::library::{self, Kind, Query};
use serde_json::Value;

/// The packages with the app closed: std, found as the app finds it, and the store's packs the
/// lists offer (PACK-11).
pub fn packages() -> Vec<(String, PathBuf)> {
    let std = m2_server::resources::std_dir();
    let mut packages = vec![("std".to_string(), std.clone())];
    if let Some(store) = m2_os::dirs::packs() {
        packages.extend(library::store_packs(&store, &std));
    }
    packages
}

/// Your sample folders, as this computer's settings keep them (`~/.music2/settings.json`, LIB-5).
pub fn sample_folders() -> Vec<PathBuf> {
    let file = m2_os::dirs::home().map(|h| h.join(".music2").join("settings.json"));
    let kept: Option<Value> = file.and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok());
    kept.and_then(|k| serde_json::from_value(k["samples"].clone()).ok()).unwrap_or_default()
}

/// The samples of `packages` and of your folders, indexed now through the cache folder's
/// measures (LIB-5).
pub fn samples(packages: &[(String, PathBuf)]) -> Vec<library::Item> {
    let cache = m2_os::dirs::cache().map(|c| c.join("samples"));
    let mut out = vec![];
    for (name, dir) in packages {
        out.extend(samples::index(Root::Package { name, dir }, cache.as_deref()).items);
    }
    for dir in sample_folders() {
        let found = samples::index(Root::Folder(&dir), cache.as_deref());
        if found.not_wav > 0 {
            eprintln!("{}: {} audio files that aren't WAV were skipped: WAV only for now.", dir.display(), found.not_wav);
        }
        out.extend(found.items);
    }
    out
}

/// The library with the app closed (LIB-2): std's and the store's packs' components, their presets
/// and yours, and the samples of both and of your folders.
pub fn closed() -> Result<Vec<library::Item>> {
    let packages = packages();
    let yours = m2_os::dirs::presets();
    Ok(library::without_song(&packages, yours.as_deref(), &[], &samples(&packages)))
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
            library::search(&all, &Query::parse(&q)).into_iter().map(serde_json::to_value).collect::<Result<_, _>>()?
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
    let text = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    // Each item's columns: its label, its kind, what it is and where it is from, and its id.
    let rows: Vec<[String; 4]> = items
        .iter()
        .map(|i| {
            let what = if text(i, "kind") == "sound" { i["tags"][0].as_str().unwrap_or_default().to_string() } else { text(i, "category") };
            [text(i, "label"), text(i, "kind"), format!("{what} · {}", text(i, "package")), text(i, "id")]
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

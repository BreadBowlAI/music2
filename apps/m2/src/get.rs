//! The registry with `m2` (REG-11 to REG-17): `m2 pack get`, and the library's items in
//! `m2 search` with the app closed. `m2` reads the registry the app reads (`M2_REGISTRY`, else
//! the settings' `registry`), and keeps its catalog in the same cache folder, fetching it only
//! when the one kept is a day old.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use m2_editor::get::{self, Remote};
use m2_editor::packs::Install;
use m2_song::Version;
use m2_song::pack::{Lock, PackName, Store};
use m2_song::registry::Catalog;

/// `text` as `m2` prints a word from the registry: its control characters written out (`\u{1b}`,
/// `\n`), so none acts on the terminal (PACK-12).
pub fn shown(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { c.escape_default().to_string() } else { c.to_string() }).collect()
}

/// The registry `m2` reads (REG-11), kept in `registry` in the cache folder. Why there is none,
/// in words saying how to set one.
pub fn remote() -> Result<Remote> {
    let Some(at) = m2_server::library::registry_named() else {
        bail!(
            "No registry is set, so there is no library to get packs from yet. Set M2_REGISTRY to its address \
             (M2_REGISTRY=https://… m2 pack get …), or `registry` in ~/.music2/settings.json"
        );
    };
    let cache = m2_os::dirs::cache().ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set), where the library is kept"))?;
    Remote::new(&at, &cache.join("registry")).map_err(|e| anyhow!(e))
}

/// The catalog (REG-12): the one kept when it is less than a day old, else fetched; offline, the
/// one kept, saying why it couldn't be fetched.
pub fn catalog(remote: &Remote) -> Result<Arc<Catalog>> {
    let (kept, why) = remote.current(get::FRESH);
    match (kept, why) {
        (Some(k), None) => Ok(k.catalog),
        (Some(k), Some(why)) => {
            eprintln!("{}: using the catalog kept from {}", shown(&why), k.catalog.made);
            Ok(k.catalog)
        }
        (None, Some(why)) => Err(anyhow!(shown(&why))),
        (None, None) => Err(anyhow!("the library's catalog couldn't be read")),
    }
}

/// The library's items with the app closed (REG-13): of the packs the store hasn't got, or has
/// an older version of. None, and nothing said, with no registry; offline with none kept, said.
pub fn offers() -> Vec<get::Offer> {
    let Ok(remote) = remote() else { return vec![] };
    let catalog = match catalog(&remote) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: the library's items are left out", shown(&e.to_string()));
            return vec![];
        }
    };
    let store = crate::packs_store();
    get::offers(&catalog, &|name| store.as_ref().and_then(|s| s.versions(name).last().copied()), &|_| false)
}

/// Say how far a Get is, on stderr: a line at each quarter.
fn progress(what: &str) -> impl FnMut(get::Progress) + '_ {
    let mut said = 0;
    move |p: get::Progress| {
        let quarter = (u128::from(p.done) * 4).checked_div(u128::from(p.total)).unwrap_or(4);
        if quarter > said && quarter < 4 {
            said = quarter;
            eprintln!("getting {what}: {} of {}", mb(p.done), mb(p.total));
        }
    }
}

/// Bytes in MB, as a person reads them: `1.2 MB`.
fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

/// Get version `version` of `name` from `remote` into `store` (REG-15), saying what was got.
fn get_one(remote: &Remote, store: &Store, name: &PackName, version: &m2_song::registry::CatalogVersion) -> Result<()> {
    let what = format!("{name} {}", version.version);
    if version.yanked.is_some() {
        eprintln!(
            "{what} was yanked ({}): got all the same, as a song that locks it needs it",
            shown(version.yanked.as_deref().unwrap_or(""))
        );
    }
    let std = m2_server::resources::std_dir();
    let done =
        get::pack(remote, name, version, store, &std, &mut progress(&what), &get::Cancel::default()).map_err(|e| anyhow!(shown(&e)))?;
    match done {
        Install::Installed { dir, .. } => println!("got {what} ({}), installed in {}", mb(version.size.unwrap_or(0)), dir.display()),
        Install::Already { .. } => println!("{what} is already installed, with these very files"),
        Install::Repaired { why, .. } => println!("got {what} again: its installed copy was changed ({}), and is put back", shown(&why)),
    }
    Ok(())
}

/// `m2 pack get [<@owner/name>[@<version>] | <song>]` (REG-17): a pack, the latest version when
/// none is named; else every version the song's lock names that isn't installed (the song in the
/// folder given, or the one `m2` runs in).
pub fn pack_get(arg: Option<&str>) -> Result<()> {
    let remote = remote()?;
    let store = crate::packs_store().ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set), where your packs are"))?;
    let catalog = catalog(&remote)?;
    if let Some(named) = arg.filter(|a| a.starts_with('@')) {
        let (name, version) = match named[1..].split_once('@') {
            Some((n, v)) => (format!("@{n}"), Some(Version::parse(v).ok_or_else(|| anyhow!("`{v}` isn't a version such as 1.2.0"))?)),
            None => (named.to_string(), None),
        };
        let name = PackName::new(&name).map_err(|e| anyhow!(e))?;
        let version = get::chosen(&catalog, &name, version).map_err(|e| anyhow!(shown(&e)))?;
        get_one(&remote, &store, &name, version)?;
        return tell_app();
    }
    let song = arg.map_or_else(|| PathBuf::from("."), PathBuf::from);
    let lock = read_lock(&song)?;
    let missing = get::missing(&lock, &store, Some(&catalog));
    if missing.is_empty() {
        println!("nothing is missing: every pack {} names is installed", m2_song::path::LOCK);
        return Ok(());
    }
    let mut failed = 0;
    for m in &missing {
        match &m.got {
            Ok(v) => {
                if let Err(e) = get_one(&remote, &store, &m.name, v) {
                    eprintln!("problem: {}", shown(&format!("{e:#}")));
                    failed += 1;
                }
            }
            Err(why) => {
                eprintln!("problem: {}", shown(why));
                failed += 1;
            }
        }
    }
    if failed < missing.len() {
        tell_app()?;
    }
    if failed > 0 {
        bail!("{failed} of the song's {} missing pack{} couldn't be got", missing.len(), if missing.len() == 1 { "" } else { "s" });
    }
    Ok(())
}

/// Tell the open app, if one is, that the store changed (REG-17): its songs then use what came.
fn tell_app() -> Result<()> {
    if crate::app::tell_store_changed()? {
        println!("the app was told: its open song uses what came, with no need to open it again");
    }
    Ok(())
}

/// The song in `dir`'s `music2.lock`: none locked when it has none.
fn read_lock(dir: &Path) -> Result<Lock> {
    if !dir.join(m2_song::path::MANIFEST).is_file() {
        bail!("{} isn't a song's folder: name a pack (m2 pack get @owner/name), or run it in a song's folder", dir.display());
    }
    match std::fs::read_to_string(dir.join(m2_song::path::LOCK)) {
        Ok(text) => Lock::read(&text).map_err(|d| anyhow!("{}", d.message)),
        Err(_) => Ok(Lock::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PACK-12: a word from the registry that holds a control character (an escape sequence
    // that would clear the terminal, a line break that would pass for another line) is printed
    // with it written out, never as it is.
    #[test]
    fn a_control_character_is_printed_written_out() {
        assert_eq!(shown("Soft keys"), "Soft keys");
        assert_eq!(shown("Soft\u{1b}[2J keys\n"), "Soft\\u{1b}[2J keys\\n");
        assert!(!shown("a\u{7}\u{1b}]0;title\u{7}").chars().any(char::is_control));
        assert_eq!(shown("Café ♪"), "Café ♪", "other characters are as they are");
    }
}

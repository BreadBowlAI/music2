//! Writing the store (PACK-6): a pack's folder copied in as a version, read-only, and a version
//! taken out. `m2 pack install` checks a pack first (`m2_editor::packs::install`); music2's own
//! packs, which ship with it, are copied in as they are when it starts (PACK-16).

use std::path::{Path, PathBuf};

use super::{LISTING, Listing, PackName, PkgManifest, Store};
use crate::hash::Sha;
use crate::manifest::Version;

/// What an install did.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// Copied into the store, at its folder, with its hash.
    Installed { name: PackName, version: Version, dir: PathBuf, hash: Sha },
    /// That version was there with this very hash and its files whole: nothing to do.
    Already { name: PackName, version: Version, hash: Sha },
    /// That version was there with this very hash, but its copy no longer matched its listing
    /// (`why`): the copy was replaced with these files (PACK-6).
    Repaired { name: PackName, version: Version, why: String },
}

impl Store {
    /// Copy the pack in `dir` into the store, its files and folders read-only (PACK-6), listing
    /// its files as they are now; nothing is written in `dir`. Refused when its version is there
    /// with another hash (the packs design's P5). With the same hash it does nothing when the
    /// installed copy still matches its listing, and replaces the copy when it doesn't. A link is
    /// never followed: listing a pack that holds one fails. Another program installing the same
    /// version at the same moment is no failure: the one that puts it in place first wins.
    pub fn install(&self, dir: &Path) -> Result<Install, String> {
        let m = PkgManifest::at(dir)?;
        let (name, version) = (m.package.name, m.package.version);
        let io = |e: std::io::Error| e.to_string();
        let listing = Listing::of(dir).map_err(io)?;
        let hash = listing.hash();
        let target = self.version_dir(&name, &version);
        let parent = target.parent().expect("a version's folder is in its pack's").to_path_buf();
        // The changed copy an install of this version that stopped left aside goes, so no build takes
        // the version for one being installed again (PACK-6).
        for (_, old) in self.aside(&name).into_iter().filter(|(v, _)| *v == version) {
            let _ = remove_tree(&old);
        }
        let there = |store: &Store| -> Result<Option<Result<Sha, String>>, String> {
            if std::fs::symlink_metadata(&target).is_err() {
                return Ok(None);
            }
            match store.hash(&name, &version) {
                Ok(h) if h == hash => Ok(Some(store.check(&name, &version))),
                _ => Err(format!(
                    "{name} {version} is installed already with other files. A version is never changed (P5): give this one a \
                     new version in m2pkg.toml, or remove the installed one first with `m2 pack remove {name} {version}`"
                )),
            }
        };
        let repair = match there(self)? {
            Some(Ok(_)) => return Ok(Install::Already { name, version, hash }),
            Some(Err(why)) => Some(why),
            None => None,
        };
        std::fs::create_dir_all(&parent).map_err(io)?;
        let temp = parent.join(format!(".{version}.{}.tmp", std::process::id()));
        let _ = remove_tree(&temp);
        let copied = (|| -> std::io::Result<()> {
            for f in &listing.files {
                let (from, to) = (dir.join(&f.path), temp.join(&f.path));
                // Listed with no link, and copied only as a plain file, never through one.
                if !std::fs::symlink_metadata(&from)?.file_type().is_file() {
                    return Err(std::io::Error::other(format!("{} is no longer a plain file: install it again", f.path)));
                }
                std::fs::create_dir_all(to.parent().expect("inside the pack"))?;
                std::fs::copy(&from, &to)?;
            }
            std::fs::write(temp.join(LISTING), listing.text())?;
            // What is copied must be what was listed.
            if Listing::of(&temp)? != listing {
                return Err(std::io::Error::other("the pack's files changed while it was copied: install it again"));
            }
            read_only_inside(&temp)?;
            match repair {
                Some(_) => {
                    swap(&temp, &target, &parent.join(format!(".{version}.{}.old", std::process::id())), &mut |a, b| std::fs::rename(a, b))?
                }
                None => std::fs::rename(&temp, &target)?,
            }
            set_read_only(&target, true)
        })();
        if let Err(e) = copied {
            let _ = remove_tree(&temp);
            // Another program put this very version in place first (the app and `m2` starting
            // together): it is installed, whole, and that is what was asked.
            if repair.is_none()
                && let Ok(Some(Ok(_))) = there(self)
            {
                return Ok(Install::Already { name, version, hash });
            }
            return Err(io(e));
        }
        Ok(match repair {
            Some(why) => Install::Repaired { name, version, why },
            None => Install::Installed { name, version, dir: target, hash },
        })
    }

    /// Take version `version` of `name` out of the store (`m2 pack remove`). A song that locks it
    /// then says `pkg.missing`: the store can't know every song.
    pub fn remove(&self, name: &PackName, version: &Version) -> std::io::Result<()> {
        let dir = self.version_dir(name, version);
        if !dir.join(LISTING).is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{name} {version} isn't installed")));
        }
        remove_tree(&dir)?;
        // The pack's folder, and its owner's, go when nothing is left in them.
        let pack = dir.parent().expect("a version is in its pack's folder");
        if std::fs::remove_dir(pack).is_ok() {
            let _ = std::fs::remove_dir(pack.parent().expect("a pack is in its owner's folder"));
        }
        Ok(())
    }
}

/// Put the copy in `temp` in place of the installed copy at `target` (a repair, PACK-6), through
/// `rename` (`std::fs::rename` but in tests). The changed copy is moved aside to `old` first, so
/// the version is never half one and half the other, and removed once the new one is in. Should
/// the new one not go in, the changed one is put back as it was, read-only: a failed repair
/// leaves the version installed, never missing.
pub(crate) fn swap(
    temp: &Path,
    target: &Path,
    old: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // A folder is renamed while it can be written, on every OS, and made read-only after.
    set_read_only(target, false)?;
    if let Err(e) = rename(target, old) {
        let _ = set_read_only(target, true);
        return Err(e);
    }
    if let Err(e) = rename(temp, target) {
        return match rename(old, target).and_then(|()| set_read_only(target, true)) {
            Ok(()) => Err(e),
            Err(back) => {
                Err(std::io::Error::other(format!("{e}; and the copy before couldn't be put back ({back}): it is at {}", old.display())))
            }
        };
    }
    remove_tree(old)
}

/// Make every file and folder in `dir` read-only (PACK-6): its folders last, deepest first, so
/// nothing can be added to, removed from or renamed in an installed version.
pub fn read_only_inside(dir: &Path) -> std::io::Result<()> {
    let mut folders = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let kind = e.file_type()?;
            if kind.is_dir() {
                folders.push(e.path());
                stack.push(e.path());
            } else if kind.is_file() {
                set_read_only(&e.path(), true)?;
            }
        }
    }
    for d in folders.iter().rev() {
        set_read_only(d, true)?;
    }
    Ok(())
}

/// Make `path` read-only, or writable again.
pub fn set_read_only(path: &Path, on: bool) -> std::io::Result<()> {
    let mut perm = std::fs::symlink_metadata(path)?.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(on);
    std::fs::set_permissions(path, perm)
}

/// Remove `dir` and everything in it, read-only files and folders included (an installed
/// version's, PACK-6). A link is removed, never followed.
pub fn remove_tree(dir: &Path) -> std::io::Result<()> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if !std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let _ = set_read_only(&d, false);
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => drop(set_read_only(&e.path(), false)),
                _ => {}
            }
        }
    }
    std::fs::remove_dir_all(dir)
}

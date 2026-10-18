//! The folders music2 uses outside a project. What it writes there is under `$HOME` when it is set
//! (PLAT-7), so a test isolates itself by setting `HOME`; the system's CLAP folders it only reads.

use std::path::PathBuf;

use crate::os;

/// `$HOME`, when set and not empty.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Where New Song puts songs: `~/Music/music2`, on Linux too (`XDG_MUSIC_DIR` waits for Linux
/// packaging).
pub fn songs() -> Option<PathBuf> {
    home().map(|h| h.join("Music").join("music2"))
}

/// Where your presets go (FMT-22): `~/Music/music2/Presets`, beside your songs, on every OS. It
/// doesn't follow the "new songs go" setting. One folder per sound inside it.
pub fn presets() -> Option<PathBuf> {
    songs().map(|s| s.join("Presets"))
}

/// Where music2 keeps what it can always rebuild, outside every project: the hash cache (FMT-16),
/// later decoded and warped renders (RENDER-30). `~/Library/Caches/music2` on macOS,
/// `~/.cache/music2` on Linux: from `HOME` alone, so a test's scratch `HOME` holds it too.
pub fn cache() -> Option<PathBuf> {
    home().map(|h| os::cache(&h))
}

/// The OS's CLAP folders, the user's first (the CLAP standard's): `~/Library/Audio/Plug-Ins/CLAP`
/// and `/Library/Audio/Plug-Ins/CLAP` on macOS, `~/.clap` and `/usr/lib/clap` on Linux.
pub fn clap() -> Vec<PathBuf> {
    os::clap(home().as_deref())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    // PLAT-7: every folder music2 writes outside a project is under `$HOME`.
    #[test]
    fn what_music2_writes_is_under_home() {
        let cache = super::os::cache(Path::new("/h"));
        assert!(cache.starts_with("/h") && cache.ends_with("music2") && cache != Path::new("/h/music2"), "{}", cache.display());
        if let Some(home) = super::home() {
            assert_eq!(super::songs().unwrap(), home.join("Music/music2"));
            assert_eq!(super::cache().unwrap(), super::os::cache(&home));
            assert_eq!(super::presets().unwrap(), home.join("Music/music2/Presets"));
        }
    }

    #[test]
    fn the_users_clap_folder_comes_first_and_the_systems_is_outside_home() {
        let v = super::os::clap(Some(Path::new("/h")));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].starts_with("/h") && v[0] != Path::new("/h"), "{v:?}");
        assert!(v[1].is_absolute() && !v[1].starts_with("/h"), "{v:?}");
        assert_eq!(super::os::clap(None), v[1..], "without a home, only the system's");
    }
}

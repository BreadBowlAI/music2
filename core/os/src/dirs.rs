//! The folders music2 uses outside a project. What it writes there is under `$HOME` when it is set
//! (PLAT-7), so a test isolates itself by setting `HOME`; the system's CLAP folders it only reads.

use std::path::PathBuf;

use crate::os;

/// `$HOME`, when set and not empty.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// The OS's CLAP folders, the user's first (the CLAP standard's): `~/Library/Audio/Plug-Ins/CLAP`
/// and `/Library/Audio/Plug-Ins/CLAP` on macOS, `~/.clap` and `/usr/lib/clap` on Linux.
pub fn clap() -> Vec<PathBuf> {
    os::clap(home().as_deref())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_users_clap_folder_comes_first_and_the_systems_is_outside_home() {
        let v = super::os::clap(Some(Path::new("/h")));
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v[0].starts_with("/h") && v[0] != Path::new("/h"), "{v:?}");
        assert!(v[1].is_absolute() && !v[1].starts_with("/h"), "{v:?}");
        assert_eq!(super::os::clap(None), v[1..], "without a home, only the system's");
    }
}

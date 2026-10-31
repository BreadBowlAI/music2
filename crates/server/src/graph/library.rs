//! What the library (LIB-1, LIB-3, LIB-4) needs of the open song: the components its build offers,
//! and the packages they come from.

use std::path::PathBuf;

use anyhow::Result;
use m2_script::Catalog;

use super::Graph;
use m2_editor::library::{Fits, Key};

/// Packages, by name and folder.
pub type Packages = Vec<(String, PathBuf)>;

impl Graph {
    /// The components the open song's build offers (its packages', the store's other packs' and its
    /// own, PKG-11, PACK-11), and the packages they come from: std, the packs it loaded, and the
    /// newest runnable version of every other pack in the store. None with no song open.
    pub fn library(&self) -> Result<Option<(Catalog, Packages)>> {
        self.call(|a| {
            Ok(a.open.as_ref().map(|o| {
                let p = &o.session.packages;
                let mut packages: Packages =
                    p.packages().filter(|p| p.name == m2_song::pack::STD).map(|p| (p.name.clone(), p.dir.clone())).collect();
                packages.extend(o.session.offered().iter().map(|(name, _, dir)| (name.clone(), dir.clone())));
                (o.session.catalog().clone(), packages)
            }))
        })
    }

    /// Fits my song (LIB-1) at `beat`: the open song's tempo there, and its key (`layout.json`'s)
    /// if it has one. None with no song open.
    pub fn fits(&self, beat: f64) -> Result<Option<Fits>> {
        self.call(move |a| {
            Ok(a.open.as_ref().map(|o| {
                let rev = o.session.accepted();
                let bpm = rev.compiled.data.tempo().bpm_at(beat.max(0.0));
                // A key whose scale has no one third (chromatic) is no key a sample can fit.
                let key = m2_editor::songkey::of(&rev.project.dir).and_then(|k| Some(Key { root: k.pc(), minor: k.scale().minor()? }));
                Fits { bpm, key }
            }))
        })
    }
}

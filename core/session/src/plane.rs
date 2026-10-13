//! Which way a change reaches the live engine (PLANE-1): chosen by the class of the files it
//! touches, never by who wrote them, so a lane file written by an agent or by git takes the same
//! fast path as the piano roll.
//!
//! Step 3a has two ways. When the graph and the rate are the same and only lane files changed (and
//! code that left the graph as it was), the lanes whose data changed are swapped into the running
//! renderer (`SwapLane`, RENDER-26). Anything else (the manifest, the arrangement, an asset, a
//! plugin state, the graph) is a new renderer, activated where the engine is (RENDER-25). From step 6
//! the Tx delta decides.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use m2_ir::Flat;
use m2_song::{DataPath, DataSet, LaneAddr, Snapshot};

/// One side of a change: what a renderer plays, and the files it was read from.
#[derive(Clone, Copy)]
pub struct Plays<'a> {
    pub flat: &'a Flat,
    pub snapshot: &'a Snapshot,
    pub data: &'a DataSet,
    pub rate: u32,
}

/// Whether the code differs from `a` to `b`: any Code or Manifest file (WRITE-13's `code_rev`
/// change, until step 6 counts `(code_rev, data_rev)` itself). Control ids and origins come from
/// the code alone (RENDER-15), so between two snapshots with the same code they are the same, and
/// a gesture lease goes on untouched (WRITE-22).
pub fn code_changed(a: &Snapshot, b: &Snapshot) -> bool {
    let code = |p: &&std::path::PathBuf| matches!(DataPath::parse(p), DataPath::Code(_) | DataPath::Manifest);
    let differ = |p: &std::path::PathBuf| a.files.get(p) != b.files.get(p) || a.unreadable.get(p) != b.unreadable.get(p);
    let paths: BTreeSet<&std::path::PathBuf> =
        a.files.keys().chain(b.files.keys()).chain(a.unreadable.keys()).chain(b.unreadable.keys()).collect();
    paths.into_iter().filter(code).any(differ)
}

/// The lanes whose data differs from `old` to `new`, when that is all that differs (RENDER-26):
/// the same graph and rate, the same binaries, and every file that changed is a lane file or code
/// that left the graph as it was. `None` when anything else changed, so a new renderer must play
/// `new`. A lane file that doesn't read keeps the data it had (FMT-9), so it changes nothing here.
pub fn lane_swaps(old: Plays, new: Plays) -> Option<Vec<LaneAddr>> {
    if old.rate != new.rate || old.snapshot.binaries != new.snapshot.binaries || old.flat != new.flat {
        return None;
    }
    let (a, b) = (old.snapshot, new.snapshot);
    let paths: BTreeSet<&Path> =
        a.files.keys().chain(b.files.keys()).chain(a.unreadable.keys()).chain(b.unreadable.keys()).map(|p| p.as_path()).collect();
    for p in paths {
        if a.files.get(p) == b.files.get(p) && a.unreadable.get(p) == b.unreadable.get(p) {
            continue;
        }
        // PLANE-1: by the class of the file, whoever wrote it.
        if !matches!(DataPath::parse(p), DataPath::Code(_) | DataPath::Lane { .. }) {
            return None;
        }
    }
    let (x, y): (BTreeMap<_, _>, BTreeMap<_, _>) = (old.data.lanes().collect(), new.data.lanes().collect());
    let addrs: BTreeSet<&LaneAddr> = x.keys().chain(y.keys()).copied().collect();
    let changed = addrs.into_iter().filter(|addr| match (x.get(addr), y.get(addr)) {
        (Some(p), Some(q)) => !Arc::ptr_eq(p, q) && p != q,
        (None, None) => false,
        _ => true,
    });
    Some(changed.cloned().collect())
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use m2_runtime::PackageSet;
    use m2_song::Stored;

    use super::*;
    use crate::Session;
    use crate::tests::MANIFEST;

    fn std_packages() -> PackageSet {
        let mut p = PackageSet::default();
        p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        p
    }

    const SONG: &str = "import { secs, song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                        const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n";
    const HITS: &str = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;

    struct Dir(PathBuf);

    impl Dir {
        fn new() -> Dir {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let d = Dir(std::env::temp_dir().join(format!("m2-plane-test-{}-{n}", std::process::id())));
            d.write("music2.toml", MANIFEST);
            d.write("song.ts", SONG);
            d.write("data/kick/hits.json", HITS);
            d
        }

        fn write(&self, name: &str, text: &str) {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// What the accepted revision plays, owned.
    struct Owned(Flat, Snapshot, DataSet, u32);

    impl Owned {
        fn of(s: &Session) -> Owned {
            let r = s.accepted();
            Owned(r.compiled.flat.clone(), r.snapshot.clone(), r.compiled.data.clone(), r.project.sample_rate())
        }

        fn plays(&self) -> Plays<'_> {
            Plays { flat: &self.0, snapshot: &self.1, data: &self.2, rate: self.3 }
        }
    }

    /// The swaps from the revision accepted now to the one accepted after `change`.
    fn after(dir: &Dir, s: &mut Session, change: impl FnOnce(&Dir)) -> Option<Vec<String>> {
        let old = Owned::of(s);
        change(dir);
        assert!(s.refresh(), "a new revision");
        let new = Owned::of(s);
        lane_swaps(old.plays(), new.plays()).map(|v| v.iter().map(|a| format!("{}/{}", a.module, a.lane)).collect())
    }

    // PLANE-1, RENDER-26: a lane file, whoever wrote it, is swapped; nothing else is.
    #[test]
    fn only_lane_data_is_swapped() {
        let dir = Dir::new();
        let mut s = Session::open(&dir.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"));
        let more = HITS.replace(r#""length": 1}]"#, r#""length": 1}, {"pitch": "D1", "start": 2, "length": 1}]"#);
        assert_eq!(after(&dir, &mut s, |d| d.write("data/kick/hits.json", &more)), Some(vec!["kick/hits".to_string()]));
        // Code that leaves the graph as it was changes no lane.
        assert_eq!(after(&dir, &mut s, |d| d.write("song.ts", &format!("{SONG}// a comment\n"))), Some(vec![]));
        // A lane file that doesn't read keeps playing its data (FMT-9): nothing to swap.
        assert_eq!(after(&dir, &mut s, |d| d.write("data/kick/hits.json", "{")), Some(vec![]));
        assert_eq!(after(&dir, &mut s, |d| d.write("data/kick/hits.json", HITS)), Some(vec!["kick/hits".to_string()]));
        // The graph, the arrangement and the rate each need a new renderer.
        let longer = SONG.replace("kick(lane.notes('hits'))", "kick(lane.notes('hits'), { decay: secs(0.5) })");
        assert_eq!(after(&dir, &mut s, |d| d.write("song.ts", &longer)), None);
        assert_eq!(after(&dir, &mut s, |d| d.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 90}]}"#)), None);
        assert_eq!(after(&dir, &mut s, |d| d.write("music2.toml", &MANIFEST.replace("48000", "44100"))), None);
    }

    // WRITE-13, WRITE-22: only code and the manifest change what a control id names, so only they
    // carry a gesture lease; lanes, state blobs and layout.json leave it as it is.
    #[test]
    fn only_code_and_the_manifest_change_the_code() {
        let dir = Dir::new();
        let mut last = Snapshot::read(&dir.0).unwrap();
        let mut changed = |write: &dyn Fn(&Dir)| {
            write(&dir);
            let now = Snapshot::read(&dir.0).unwrap();
            let c = code_changed(&last, &now);
            last = now;
            c
        };
        assert!(!changed(&|d| d.write("data/kick/hits.json", &HITS.replace("\"length\": 1}", "\"length\": 2}"))));
        assert!(!changed(&|d| d.write("data/kick/kick.state", "a blob")));
        assert!(!changed(&|d| d.write("layout.json", "{}")));
        assert!(!changed(&|d| d.write("data/arrangement.json", r#"{"tempo": [{"beat": 0, "bpm": 90}]}"#)));
        assert!(changed(&|d| d.write("song.ts", &format!("{SONG}// a comment\n"))));
        assert!(changed(&|d| d.write("tracks/new.ts", "export const x = 1;\n")));
        assert!(changed(&|d| d.write("music2.toml", &MANIFEST.replace("48000", "44100"))));
        assert!(!changed(&|_| {}));
    }

    #[test]
    fn a_changed_binary_needs_a_new_renderer() {
        let dir = Dir::new();
        let s = Session::open(&dir.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"));
        let old = Owned::of(&s);
        let mut new = Owned::of(&s);
        assert_eq!(lane_swaps(old.plays(), new.plays()), Some(vec![]));
        new.1.binaries.insert("assets/take.wav".into(), Stored::Listed);
        assert_eq!(lane_swaps(old.plays(), new.plays()), None);
    }
}

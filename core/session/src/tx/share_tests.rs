//! What a one-file change shares with the snapshot it was made from (the G11 fix's A.4): the
//! diff of two snapshots, the asset settle and the edits that copy a shared map only when they
//! change it, each against the whole-map form it replaces.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_song::{Sha, Snapshot, Stored};

use super::tests::Dir;
use super::{changed_paths, move_under, put_file, remove_under, settle_assets};

/// A small deterministic generator.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

/// The diff as it was before the maps were shared: every key of every map, looked up in all four.
fn changed_paths_whole(a: &Snapshot, b: &Snapshot) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let keys = a.files.keys().chain(b.files.keys()).chain(a.binaries.keys()).chain(b.binaries.keys());
    let keys = keys.chain(a.unreadable.keys()).chain(b.unreadable.keys()).chain(a.not_utf8.keys()).chain(b.not_utf8.keys());
    for k in keys {
        if a.files.get(k).map(|t| &**t) != b.files.get(k).map(|t| &**t)
            || a.binaries.get(k) != b.binaries.get(k)
            || a.unreadable.get(k) != b.unreadable.get(k)
            || a.not_utf8.get(k) != b.not_utf8.get(k)
        {
            out.insert(k.clone());
        }
    }
    out
}

/// The asset settle as it was: every asset's entry recomputed in place.
fn settle_assets_whole(snap: &mut Snapshot) {
    let named: BTreeSet<PathBuf> = snap
        .files
        .iter()
        .filter(|(p, _)| matches!(m2_song::DataPath::parse(p), m2_song::DataPath::Lane { .. }))
        .filter_map(|(_, t)| serde_json::from_str::<serde_json::Value>(t).ok())
        .flat_map(|v| {
            let clips = v.get("clips").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            clips.into_iter().filter_map(|c| Some(m2_song::DataPath::Asset(m2_song::AssetName::new(c.get("asset")?.as_str()?).ok()?).path()))
        })
        .collect();
    let dir = snap.dir.clone();
    for (p, s) in Arc::make_mut(&mut snap.binaries).iter_mut() {
        if !matches!(m2_song::DataPath::parse(p), m2_song::DataPath::Asset(_)) {
            continue;
        }
        match (named.contains(p), &s) {
            (false, _) => *s = Stored::Listed,
            (true, Stored::Listed) => {
                *s = match m2_song::HashCache::global().hash(&dir.join(p)) {
                    Ok(sha) => Stored::Hash(sha),
                    Err(e) => Stored::Unreadable(e.to_string()),
                }
            }
            (true, _) => {}
        }
    }
}

const PATHS: [&str; 12] = [
    "song.ts",
    "tracks/a.ts",
    "data/arrangement.json",
    "data/kick/hits.json",
    "data/kick/x.state",
    "data/kick/x.state.json",
    "data/bass/notes.json",
    "data/bass/y.state",
    "assets/a.wav",
    "assets/b.wav",
    "assets/vox/c.wav",
    "layout.json",
];

/// A lane naming `asset`, or none.
fn lane(asset: Option<&str>) -> String {
    match asset {
        Some(a) => format!(r#"{{"clips": [{{"id": "a", "start": 0, "length": 1, "asset": "{a}"}}]}}"#),
        None => r#"{"clips": []}"#.into(),
    }
}

/// One random edit of `snap`, of the kinds commits and take-ins make.
fn edit(rng: &mut Rng, snap: &mut Snapshot) {
    let p = Path::new(PATHS[rng.below(PATHS.len())]);
    match rng.below(7) {
        0 | 1 => {
            let text = match m2_song::DataPath::parse(p) {
                m2_song::DataPath::Lane { .. } => lane([None, Some("a.wav"), Some("vox/c.wav")][rng.below(3)]),
                _ => format!("// {}\n", rng.below(4)),
            };
            put_file(snap, p, Some(text.as_bytes()));
        }
        2 => put_file(snap, p, Some(&[0xff, rng.below(3) as u8])),
        3 => put_file(snap, p, None),
        4 => remove_under(snap, Path::new(["data/kick", "assets", "data"][rng.below(3)])),
        5 => move_under(snap, Path::new("data/kick"), Path::new("data/drum")),
        _ => {
            if rng.below(2) == 0 {
                snap.unreadable.insert(p.to_path_buf(), "denied".into());
            } else {
                snap.not_utf8.insert(p.to_path_buf(), Sha::of(&[rng.below(3) as u8]));
            }
        }
    }
}

// A.4 (PLANE-1): with texts and the binaries map shared between snapshots, the diff is the full
// diff, over random chains of the edits commits make: a shared map is skipped only when it is the
// same map, and a text compared by pointer first.
#[test]
fn changed_paths_with_shared_maps_is_the_full_diff() {
    let d = Dir::new();
    for f in ["assets/a.wav", "assets/b.wav", "assets/vox/c.wav"] {
        d.write(f, f);
    }
    for case in 0..300u64 {
        let mut rng = Rng(0xa4_0000 + case);
        let mut base = Snapshot { dir: d.0.clone(), ..Snapshot::default() };
        for _ in 0..rng.below(12) {
            edit(&mut rng, &mut base);
        }
        let mut chain = vec![base];
        for _ in 0..4 {
            let mut next = chain.last().unwrap().clone();
            for _ in 0..rng.below(3) {
                edit(&mut rng, &mut next);
            }
            if rng.below(2) == 0 {
                settle_assets(&mut next);
            }
            chain.push(next);
        }
        for a in &chain {
            for b in &chain {
                assert_eq!(changed_paths(a, b), changed_paths_whole(a, b), "case {case}");
            }
        }
    }
}

// A.4 (FMT-15): the settle that writes only the entries whose state changes gives what settling
// every asset does, and leaves a map it doesn't change shared.
#[test]
fn settling_assets_changes_only_what_a_whole_settle_changes() {
    let d = Dir::new();
    for f in ["assets/a.wav", "assets/b.wav", "assets/vox/c.wav"] {
        d.write(f, f);
    }
    for case in 0..300u64 {
        let mut rng = Rng(0x5e77_0000 + case);
        let mut snap = Snapshot { dir: d.0.clone(), ..Snapshot::default() };
        for _ in 0..rng.below(10) {
            edit(&mut rng, &mut snap);
        }
        let (mut fast, mut whole) = (snap.clone(), snap.clone());
        settle_assets(&mut fast);
        settle_assets_whole(&mut whole);
        assert_eq!(*fast.binaries, *whole.binaries, "case {case}");
        // Settled already: nothing to write, so nothing is copied.
        let settled = fast.clone();
        settle_assets(&mut fast);
        assert!(Arc::ptr_eq(&fast.binaries, &settled.binaries), "case {case}: a settled map is left shared");
    }
}

// A.4: an edit of texts never copies the binaries map, and one that removes nothing there leaves
// it shared; one that does change it leaves the map it came from as it was.
#[test]
fn edits_copy_the_binaries_only_when_they_change_them() {
    let mut a = Snapshot::default();
    put_file(&mut a, Path::new("assets/a.wav"), Some(b"a"));
    put_file(&mut a, Path::new("data/kick/hits.json"), Some(lane(None).as_bytes()));
    let mut b = a.clone();
    put_file(&mut b, Path::new("data/kick/hits.json"), Some(lane(Some("a.wav")).as_bytes()));
    remove_under(&mut b, Path::new("data/bass"));
    put_file(&mut b, Path::new("data/bass/notes.json"), None);
    assert!(Arc::ptr_eq(&a.binaries, &b.binaries));
    assert_eq!(changed_paths(&a, &b), BTreeSet::from([PathBuf::from("data/kick/hits.json")]));
    put_file(&mut b, Path::new("assets/a.wav"), None);
    assert!(!Arc::ptr_eq(&a.binaries, &b.binaries));
    assert_eq!(a.binaries.keys().collect::<Vec<_>>(), [Path::new("assets/a.wav")], "the old snapshot is as it was");
    let mut c = BTreeMap::new();
    c.insert(PathBuf::from("assets/a.wav"), ());
    assert_eq!(m2_song::under(&c, Path::new("assets")).count(), 1);
    assert_eq!(m2_song::under(&c, Path::new("asset")).count(), 0, "a prefix of a name isn't a folder");
}

/// Whether undo step `step`'s Clips ops would go ahead, judged by parsing both texts as undo did
/// before it shared the accepted revision's read (WRITE-21).
fn clips_by_parse(s: &crate::Session, step: &super::TxStep) -> bool {
    use m2_song::tx::{Base, Op, TxPath};
    let tx = &step.inverse;
    tx.ops.iter().all(|op| {
        let Op::Clips { lane, put, delete } = op else { panic!("only Clips here") };
        let file = TxPath::lane(lane);
        let kind = s.accepted.compiled.decls.lanes.iter().find(|(a, _)| a == lane).map(|(_, k)| k.clone()).unwrap();
        let (Some(Base::WithText { text, .. }), Some(now)) = (tx.base.get(&file), s.accepted.snapshot.files.get(&file.path())) else {
            return false;
        };
        let (Ok(was), Ok(is)) = (m2_song::LaneData::read(text, &kind), m2_song::LaneData::read(now, &kind)) else { return false };
        let lines = |d: &m2_song::LaneData, id: &str| {
            d.clip_lines().into_iter().filter(|(i, _)| *i == Some(id)).map(|(_, l)| l).collect::<Vec<_>>()
        };
        let ids = put.iter().filter_map(|c| c.get("id").and_then(|i| i.as_str())).chain(delete.iter().map(String::as_str));
        ids.into_iter().all(|id| {
            let (a, b) = (lines(&was, id), lines(&is, id));
            a.len() <= 1 && a == b
        })
    })
}

// WRITE-21 (the G11 fix's A.8): whether undo and redo would go ahead, judged without parsing when
// the step's base is the accepted file, is what parsing both texts says, over random lane edits,
// outside edits of the same clip or another taken in, an id made twice, undo and redo.
#[test]
fn can_undo_fast_path_matches_the_parse() {
    use m2_song::tx::{Confined, Expect, Op};
    use serde_json::json;
    let d = Dir::first_song();
    let lane = || m2_song::LaneAddr::new("kick", "hits").unwrap();
    let two = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": []}, {"id": "b", "start": 4, "length": 4, "notes": []}]}"#;
    d.write("data/kick/hits.json", two);
    let mut s = d.open();
    let confined = || Expect::Confined(Confined { lanes: vec![lane()], ..Confined::default() });
    let (mut checked, mut fast) = (0, 0);
    for case in 0..200u64 {
        let mut rng = Rng(0x0adb_0000 + case);
        match rng.below(6) {
            0 | 1 => {
                let id = ["a", "b", "c"][rng.below(3)];
                let put = json!({"id": id, "start": rng.below(16), "length": 4, "notes": [{"pitch": 36 + rng.below(12), "start": 0, "length": 1}]});
                let tx = super::tests::based(&d, vec![Op::Clips { lane: lane(), put: vec![put], delete: vec![] }], confined());
                let _ = s.commit(tx);
            }
            2 => {
                // An outside edit of one clip, taken in.
                let mut v: serde_json::Value = serde_json::from_str(&d.read("data/kick/hits.json")).unwrap();
                if let Some(c) = v["clips"].as_array_mut().and_then(|c| c.first_mut()) {
                    c["length"] = json!(1 + rng.below(8));
                }
                d.write("data/kick/hits.json", &v.to_string());
                s.refresh();
            }
            3 => {
                // An id made twice outside.
                let text = d.read("data/kick/hits.json");
                d.write("data/kick/hits.json", &text.replacen("\"id\": \"b\"", "\"id\": \"a\"", 1));
                s.refresh();
            }
            4 => {
                let _ = s.undo();
            }
            _ => {
                let _ = s.redo();
            }
        }
        for step in s.history.last().into_iter().chain(s.redo.last()) {
            if step.inverse.ops.iter().all(|op| matches!(op, Op::Clips { .. })) && !step.inverse.ops.is_empty() {
                assert_eq!(s.can_tx(step), clips_by_parse(&s, step), "case {case}: {}", step.inverse.to_json());
                checked += 1;
                let base = step.inverse.base.values().next();
                let now = s.accepted.snapshot.files.get(std::path::Path::new("data/kick/hits.json"));
                fast += usize::from(matches!((base, now), (Some(m2_song::tx::Base::WithText { text, .. }), Some(n)) if **text == **n));
            }
        }
    }
    assert!(checked > 100 && fast > 30 && checked - fast > 30, "{checked} judged, {fast} by the shared read");
}

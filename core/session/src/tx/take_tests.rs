//! Taking in a batch the watcher settled, read alone (WRITE-19, C.7 of the G11 design).

use std::collections::BTreeSet;
use std::path::PathBuf;

use m2_song::Snapshot;

use super::tests::Dir;
use crate::watch::{Listing, listing_stamped};

/// A small generator.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// The outside changes a round may make, each to both copies alike.
const CHANGES: usize = 10;

fn change(d: &Dir, which: usize) {
    match which {
        // A lane file edited, as an editor saves it.
        0 => d.write("data/kick/hits.json", &d.read("data/kick/hits.json").replacen("\"start\": 0", "\"start\": 0.5", 1)),
        // An audio lane names another asset, which is there.
        1 => {
            std::fs::copy(d.0.join("assets/keys-bounce.wav"), d.0.join("assets/chop-2.wav")).unwrap();
            d.write("data/chops/clips.json", &d.read("data/chops/clips.json").replacen("\"keys-bounce.wav\"", "\"chop-2.wav\"", 1));
        }
        // The asset a lane names, replaced with other bytes.
        2 => d.write_bytes("assets/keys-bounce.wav", &std::fs::read(d.0.join("assets/keys-bounce.wav")).unwrap()[..4096]),
        // An asset no lane names, added; and one removed.
        3 => d.write_bytes("assets/drums/new-take.wav", b"RIFF"),
        4 => std::fs::remove_file(d.0.join("assets/keys-bounce.wav.json")).unwrap(),
        // Files a snapshot lists as Ignored: one under data/ that is no data path, and an asset's
        // name in another Unicode form.
        5 => d.write("data/kick/notes.txt", "a note"),
        6 => d.write_bytes("assets/cafe\u{301}.wav", b"RIFF"),
        // Code, and a lane file gone.
        7 => d.write("tracks/kick.ts", &format!("{}\n// edited\n", d.read("tracks/kick.ts"))),
        8 => std::fs::remove_file(d.0.join("data/keys/chords.json")).unwrap(),
        // A text file that isn't UTF-8.
        _ => d.write_bytes("data/kick/hits.json", b"{\"clips\": [\xff]}"),
    }
}

/// The tracked paths whose entries differ between two walks: what the watcher's batch names.
fn batch(a: &Listing, b: &Listing) -> BTreeSet<PathBuf> {
    let keys: BTreeSet<&String> = a.entries.keys().chain(b.entries.keys()).collect();
    let differs = |k: &String| match (a.entries.get(k), b.entries.get(k)) {
        // A same-size rewrite within the clock's resolution shows only in the content: the
        // rewritten files are named as changed by the caller anyway.
        (Some(x), Some(y)) => x != y,
        _ => true,
    };
    keys.into_iter().filter(|k| differs(k)).map(PathBuf::from).collect()
}

/// A snapshot as it compares, but for the folder it was read from.
fn shape(s: &Snapshot) -> impl PartialEq + std::fmt::Debug {
    (s.files.clone(), (*s.binaries).clone(), (*s.unknown).clone(), s.unreadable.clone(), s.not_utf8.clone())
}

// WRITE-19: a batch the watcher settled, read alone (`Snapshot::read_paths`, with the Ignored files
// the walk that made it ready listed, and the assets the watcher tracked by hash hashed), is taken
// in as a whole read of the folder takes it in: the same draft, revision and problems. Over
// generated outside changes to a project with an audio lane: lane files, code, assets a lane names
// and others, Ignored files, files gone, text that isn't UTF-8.
#[test]
fn a_settled_batch_reads_only_its_paths_and_takes_in_the_same() {
    let mut rng = Lcg(7);
    for round in 0..12 {
        let (whole, alone) = (Dir::copy_of("examples/resample"), Dir::copy_of("examples/resample"));
        let (mut a, mut b) = (whole.open(), alone.open());
        let before = listing_stamped(&alone.0, &BTreeSet::new());
        let picked: Vec<usize> = (0..CHANGES).filter(|_| rng.next().is_multiple_of(3)).collect();
        for &c in &picked {
            change(&whole, c);
            change(&alone, c);
        }
        let named: BTreeSet<String> = ["assets/keys-bounce.wav".to_string()].into();
        let after = crate::watch::listing(&alone.0, &named);
        let paths = batch(&before, &listing_stamped(&alone.0, &BTreeSet::new()));
        let hashed = |p: &std::path::Path| named.contains(&m2_song::path::slashed(p));
        let read = Snapshot::read_paths(&alone.0, &paths, &hashed, after.unknown.clone()).unwrap();
        // Each path reads as a whole read has it; an asset the watcher didn't hash is listed (a
        // lane that names it now has it hashed where it is taken in).
        let full = Snapshot::read(&alone.0).unwrap();
        for p in &paths {
            let listed = p.starts_with("assets") && !hashed(p);
            let want = if listed { full.binaries.get(p).map(|_| m2_song::Stored::Listed) } else { full.binaries.get(p).cloned() };
            assert_eq!(read.binaries.get(p), want.as_ref(), "round {round} ({picked:?}): {p:?}");
            assert_eq!((read.files.get(p), read.unreadable.get(p)), (full.files.get(p), full.unreadable.get(p)), "round {round}: {p:?}");
        }
        let (ta, tb) = (a.take_in_only(Snapshot::read(&whole.0).unwrap(), &paths), b.take_in_only(read, &paths));
        let what = format!("round {round}, changes {picked:?}, paths {paths:?}");
        assert_eq!(ta.draft_changed, tb.draft_changed, "{what}");
        assert_eq!(
            ta.accepted.map(|x| (x.number, x.code_rev, x.data_rev, x.plane)),
            tb.accepted.map(|x| (x.number, x.code_rev, x.data_rev, x.plane)),
            "{what}"
        );
        assert_eq!(shape(&a.draft), shape(&b.draft), "{what}");
        let problems = |s: &crate::Session| {
            s.accepted()
                .compiled
                .problems
                .iter()
                .map(|p| (p.code, p.src.clone(), p.message.replace(&*s.accepted().project.dir.to_string_lossy(), "")))
                .collect::<Vec<_>>()
        };
        assert_eq!(problems(&a), problems(&b), "{what}");
        let played = |s: &crate::Session| {
            let d = &s.accepted().compiled.data;
            format!("{:?} {:?}", d.notes("kick", "hits"), d.audio("chops", "clips"))
        };
        assert_eq!(played(&a), played(&b), "{what}");
    }
}

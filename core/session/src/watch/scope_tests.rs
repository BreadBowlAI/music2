//! The settle rule polled as the server's watcher polls it with file system events: only the
//! paths the events name and the open batch's, and the whole project at the first tick and
//! whenever a batch is due ([`Scope`], [`Settle::due`]).

use std::cell::RefCell;

use super::tests::{OLD, WALL, list, stamp};
use super::*;

/// One tick of a scenario, 50 ms or more after the one before.
#[derive(Clone)]
struct Step {
    ms: u64,
    disk: Listing,
    git: Git,
    /// What files hold from this tick on, where it isn't their path: for hashing.
    content: Vec<(&'static str, &'static str)>,
    /// The paths events name at this tick. None: every path whose entry or content changed since
    /// the tick before, named on time.
    events: Option<Vec<&'static str>>,
    /// After a `Ready`, what the snapshot read for it finds (the step 6 design's 8.2).
    read: Option<fn(&Path) -> Option<Sha>>,
    /// After the tick, the session wrote these itself, with their new stamps (R12).
    wrote: Vec<(&'static str, Option<Stamp>)>,
}

fn step(ms: u64, disk: &Listing) -> Step {
    Step { ms, disk: disk.clone(), git: Git::default(), content: vec![], events: None, read: None, wrote: vec![] }
}

/// The paths whose entries differ in `a` and `b`.
fn changed(a: &Listing, b: &Listing) -> Vec<String> {
    let keys: BTreeSet<&String> = a.entries.keys().chain(b.entries.keys()).collect();
    keys.into_iter().filter(|k| a.entries.get(*k) != b.entries.get(*k)).cloned().collect()
}

/// The entries of `l` in `scope` (each path with what is under it), as `listing_of` stats them.
fn seen(l: &Listing, scope: &BTreeSet<String>) -> Listing {
    let inside = |k: &str| scope.iter().any(|q| k == q || k.starts_with(&format!("{q}/")));
    Listing { entries: l.entries.iter().filter(|(k, _)| inside(k)).map(|(k, v)| (k.clone(), *v)).collect(), ..Listing::default() }
}

/// How a run polls.
#[derive(Clone, Copy, PartialEq)]
enum By {
    /// Every tick sees the whole project.
    Walks,
    /// As the watcher polls with events: the whole project at the first tick and when the batch is
    /// due, else only what is named and the batch (no poll at all with neither).
    Events,
    /// Only by scope after the first tick, never seeing the whole project again.
    ScopeOnly,
}

/// What each tick of `steps` polled, from `base`.
fn run(base: &Listing, steps: &[Step], scoped: bool) -> (Vec<Poll>, Settle) {
    run_by(base, steps, if scoped { By::Events } else { By::Walks })
}

fn run_by(base: &Listing, steps: &[Step], by: By) -> (Vec<Poll>, Settle) {
    let scoped = by != By::Walks;
    let t0 = Instant::now();
    let mut s = Settle::new(base, WALL);
    let content: RefCell<BTreeMap<String, &str>> = RefCell::default();
    let (mut prev, mut named, mut out) = (base.clone(), BTreeSet::<String>::new(), vec![]);
    for (i, st) in steps.iter().enumerate() {
        content.borrow_mut().extend(st.content.iter().map(|(p, c)| (p.to_string(), *c)));
        let mut hash = |p: &Path| {
            let key = m2_song::path::slashed(p);
            Some(Sha::of(content.borrow().get(&key).copied().unwrap_or(&key).as_bytes()))
        };
        let at = t0 + Duration::from_millis(st.ms);
        let polled = if !scoped {
            s.poll(at, WALL, &st.disk, st.git, &mut hash)
        } else {
            match &st.events {
                Some(e) => named.extend(e.iter().map(|p| p.to_string())),
                None => {
                    named.extend(changed(&prev, &st.disk));
                    named.extend(st.content.iter().map(|(p, _)| p.to_string()));
                }
            }
            if i == 0 || (by == By::Events && s.due(at) && !st.git.busy()) {
                named.clear();
                s.poll(at, WALL, &st.disk, st.git, &mut hash)
            } else if named.is_empty() && !s.settling() {
                Poll::Idle
            } else {
                let scope: BTreeSet<String> = std::mem::take(&mut named).into_iter().chain(s.batch_paths().map(String::from)).collect();
                s.poll_in(at, WALL, &seen(&st.disk, &scope), Scope::paths(&scope), st.git, &mut hash)
            }
        };
        if let (Poll::Ready(paths), Some(read)) = (&polled, st.read) {
            // What the read found written again goes on: the next tick looks at it, named or not.
            let kept = s.confirm(paths, &read);
            named.extend(paths.difference(&kept).map(|p| m2_song::path::slashed(p)));
        }
        out.push(polled);
        prev = st.disk.clone();
        if !st.wrote.is_empty() {
            s.wrote(&st.wrote.iter().map(|(p, st)| (PathBuf::from(p), *st)).collect::<Vec<_>>(), WALL);
        }
    }
    (out, s)
}

/// What a baseline says of each path: its track and stamp (not when it was last hashed, which a
/// scoped poll records only for what it saw).
fn tracked(s: &Settle) -> Vec<(String, Option<(Track, Stamp)>)> {
    let of = |v: &Seen| match v {
        Seen::Input { track, stamp, .. } => Some((*track, *stamp)),
        Seen::Present => None,
    };
    s.baseline.iter().map(|(k, v)| (k.clone(), of(v))).collect()
}

/// Every scenario of the full-poll tests, as ticks.
fn scenarios() -> Vec<(&'static str, Listing, Vec<Step>)> {
    let mut all = vec![];
    // Two halves, 40 ms apart, then left.
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    let hits = |size: u64| {
        list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(size, OLD + size as i128))])
    };
    let mut steps = vec![step(0, &base), step(50, &hits(5)), step(100, &hits(22))];
    steps.extend([150, 200, 250, 300, 350, 400].map(|ms| step(ms, &hits(22))));
    all.push(("two halves", base, steps));
    // Git holds a batch while it writes; a stale lock doesn't, and is said once.
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD))]);
    let changed = list(&[("song.ts", Track::Text, stamp(11, OLD))]);
    let again = list(&[("song.ts", Track::Text, stamp(12, OLD))]);
    let (busy, stale) = (Git { lock_age: Some(Duration::from_secs(1)) }, Git { lock_age: Some(Duration::from_secs(11)) });
    let steps = vec![
        Step { git: busy, ..step(0, &changed) },
        Step { git: busy, ..step(1000, &changed) },
        Step { git: stale, ..step(2000, &changed) },
        Step { git: stale, ..step(3000, &again) },
        Step { git: stale, ..step(4000, &again) },
    ];
    all.push(("git", base, steps));
    // A path written to again after it settled goes on.
    let base = list(&[("a.ts", Track::Text, stamp(10, OLD)), ("b.ts", Track::Text, stamp(10, OLD))]);
    let l = list(&[("a.ts", Track::Text, stamp(11, OLD)), ("b.ts", Track::Text, stamp(11, OLD))]);
    let read: fn(&Path) -> Option<Sha> = |p| Some(if p == Path::new("a.ts") { Sha::of(b"a.ts") } else { Sha::of(b"more") });
    let mut steps =
        vec![step(0, &base), step(50, &l), step(100, &l), step(150, &l), step(200, &l), Step { read: Some(read), ..step(250, &l) }];
    steps.extend([300, 350, 400, 450, 500, 550].map(|ms| step(ms, &l)));
    all.push(("written after it settled", base, steps));
    // The session's own writes make no batch; an outside write elsewhere meanwhile is taken in.
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    let l = list(&[("song.ts", Track::Text, stamp(11, OLD + 5)), ("data/kick/hits.json", Track::Text, stamp(21, OLD))]);
    let replaced =
        list(&[("song.ts", Track::Text, Stamp { inode: 9, ..stamp(11, OLD + 5) }), ("data/kick/hits.json", Track::Text, stamp(21, OLD))]);
    let mut steps = vec![step(0, &base), Step { wrote: vec![("song.ts", Some(stamp(11, OLD + 5)))], ..step(50, &l) }];
    steps.extend([100, 150, 200, 250].map(|ms| step(ms, &l)));
    steps.extend([300, 350, 400, 450, 500, 550].map(|ms| step(ms, &replaced)));
    all.push(("own writes", base, steps));
    // A render growing in assets/ holds nothing back; a growing state blob is hashed once stable.
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("assets/render.wav", Track::Present, stamp(1, OLD))]);
    let render =
        |i: u64| list(&[("song.ts", Track::Text, stamp(10, OLD)), ("assets/render.wav", Track::Present, stamp(100 * i, OLD + i as i128))]);
    all.push(("growing render", base, (0..6).map(|i| step(50 * i, &render(i))).collect()));
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD))]);
    let blob = |size: u64| list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/lead/lead.state", Track::Binary, stamp(size, OLD))]);
    let steps =
        vec![step(0, &base), step(50, &blob(1)), step(100, &blob(2)), step(150, &blob(2)), step(200, &blob(2)), step(300, &blob(2))];
    all.push(("growing blob", base, steps));
    // A path there for one poll only settles as gone.
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD))]);
    let phantom = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("tracks/scratch.ts", Track::Text, stamp(5, OLD))]);
    let edited = list(&[("song.ts", Track::Text, stamp(11, OLD))]);
    let mut steps = vec![step(0, &base), step(50, &phantom)];
    steps.extend([100, 150, 200, 250, 300].map(|ms| step(ms, &base)));
    steps.extend([350, 400, 450, 500, 550].map(|ms| step(ms, &edited)));
    all.push(("there for one poll", base, steps));
    // A same-size rewrite within the timestamp's resolution shows in its hash.
    let base = list(&[("song.ts", Track::Text, stamp(10, WALL - 1_000_000_000))]);
    let steps = vec![
        Step { content: vec![("song.ts", "one")], ..step(0, &base) },
        Step { content: vec![("song.ts", "two")], ..step(50, &base) },
        step(100, &base),
        step(150, &base),
        step(200, &base),
        step(250, &base),
    ];
    all.push(("racy rewrite", base, steps));
    all
}

// WRITE-19, R12, R14, FMT-16: polled as the watcher polls with events whose names cover each
// change on time, every scenario polls as it does when every poll sees the whole project, and
// leaves the same baseline.
#[test]
fn a_scoped_poll_sees_what_a_full_poll_sees() {
    for (name, base, steps) in scenarios() {
        let (full, a) = run(&base, &steps, false);
        let (scoped, b) = run(&base, &steps, true);
        assert!(full.iter().any(|p| matches!(p, Poll::Ready(_))) || name.starts_with("growing render"), "{name}: {full:?}");
        assert_eq!(scoped, full, "{name}");
        assert_eq!(tracked(&b), tracked(&a), "{name}");
        assert_eq!(b.batch_paths().collect::<Vec<_>>(), a.batch_paths().collect::<Vec<_>>(), "{name}");
    }
}

// WRITE-19: a change whose second file's event comes 300 ms after it was written (fseventsd
// behind, under a checkout's burst) still settles as one batch: the poll that would make the
// first file's batch ready sees the whole project, and finds the second.
#[test]
fn a_late_event_never_readies_half_a_change() {
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/t01/notes.json", Track::Text, stamp(20, OLD))]);
    let code = list(&[("song.ts", Track::Text, stamp(11, OLD)), ("data/t01/notes.json", Track::Text, stamp(20, OLD))]);
    let both = list(&[("song.ts", Track::Text, stamp(11, OLD)), ("data/t01/notes.json", Track::Text, stamp(21, OLD))]);
    let mut steps = vec![step(0, &base), Step { events: Some(vec!["song.ts"]), ..step(50, &code) }];
    steps.extend((2..4).map(|i| Step { events: Some(vec![]), ..step(50 * i, &code) }));
    // The lane is written by 200 ms; its event comes 300 ms later.
    steps.extend((4..20).map(|i| Step { events: Some(if i == 10 { vec!["data/t01/notes.json"] } else { vec![] }), ..step(50 * i, &both) }));
    let (polls, _) = run(&base, &steps, true);
    let ready: Vec<&BTreeSet<PathBuf>> = polls.iter().filter_map(|p| if let Poll::Ready(p) = p { Some(p) } else { None }).collect();
    assert_eq!(ready, [&[PathBuf::from("data/t01/notes.json"), PathBuf::from("song.ts")].into()], "{polls:?}");
    // Polled only by scope, the code's batch is quiet and stable long before the lane's event
    // comes, and still never ready: a scoped poll can't tell that nothing else changed.
    let (polls, s) = run_by(&base, &steps, By::ScopeOnly);
    assert!(polls.iter().all(|p| !matches!(p, Poll::Ready(_))) && s.settling(), "{polls:?}");
}

// WRITE-19: a path out of a poll's scope is as it was, whatever its listing says; the next poll of
// the whole project finds it.
#[test]
fn a_path_outside_the_scope_is_left_as_it_was() {
    let t0 = Instant::now();
    let base = list(&[("a.ts", Track::Text, stamp(10, OLD)), ("b.ts", Track::Text, stamp(10, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    let l = list(&[("a.ts", Track::Text, stamp(10, OLD)), ("b.ts", Track::Text, stamp(11, OLD))]);
    let a: BTreeSet<String> = ["a.ts".to_string()].into();
    assert_eq!(s.poll_in(t0, WALL, &l, Scope::paths(&a), Git::default(), &mut hash), Poll::Idle);
    assert_eq!(s.poll(t0 + Duration::from_millis(50), WALL, &l, Git::default(), &mut hash), Poll::Settling, "found by a full poll");
}

// WRITE-19: a folder in scope with nothing under it in the listing (removed, or moved away) takes
// every path the baseline has under it as gone; the batch is ready at a full poll.
#[test]
fn a_folder_gone_settles_every_path_under_it_as_gone() {
    let t0 = Instant::now();
    let base = list(&[
        ("song.ts", Track::Text, stamp(10, OLD)),
        ("data/kick/hits.json", Track::Text, stamp(20, OLD)),
        ("data/kick/lead.state", Track::Binary, stamp(30, OLD)),
        ("data/kick-2/hits.json", Track::Text, stamp(20, OLD)),
    ]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    let gone = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick-2/hits.json", Track::Text, stamp(20, OLD))]);
    let kick: BTreeSet<String> = ["data/kick".to_string()].into();
    assert_eq!(s.poll_in(t0, WALL, &seen(&gone, &kick), Scope::paths(&kick), Git::default(), &mut hash), Poll::Settling);
    assert_eq!(s.batch_paths().collect::<Vec<_>>(), ["data/kick/hits.json", "data/kick/lead.state"], "kick-2 is another folder");
    let at = |ms| t0 + Duration::from_millis(ms);
    assert_eq!(s.poll_in(at(50), WALL, &seen(&gone, &kick), Scope::paths(&kick), Git::default(), &mut hash), Poll::Settling);
    assert_eq!(s.poll_in(at(200), WALL, &seen(&gone, &kick), Scope::paths(&kick), Git::default(), &mut hash), Poll::Settling, "scoped");
    assert!(s.due(at(200)));
    let ready = [PathBuf::from("data/kick/hits.json"), PathBuf::from("data/kick/lead.state")].into();
    assert_eq!(s.poll(at(200), WALL, &gone, Git::default(), &mut hash), Poll::Ready(ready));
}

// R12: a path the session wrote while a listing was taken is left as it was by that poll (the
// listing may hold either version); its own stamps are told by `wrote`.
#[test]
fn a_path_the_session_wrote_meanwhile_is_left_as_it_was() {
    let t0 = Instant::now();
    let base = list(&[("song.ts", Track::Text, stamp(10, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    let mut s = Settle::new(&base, WALL);
    let mut hash = |_: &Path| Some(Sha::of(b"x"));
    // The session commits the lane while a walk is taken: the walk saw the old file.
    s.wrote(&[(PathBuf::from("data/kick/hits.json"), Some(stamp(21, OLD + 1)))], WALL);
    let mine: BTreeSet<String> = ["data/kick/hits.json".to_string()].into();
    assert_eq!(s.poll_in(t0, WALL, &base, Scope::all().except(&mine), Git::default(), &mut hash), Poll::Idle);
    // An outside write to another file in the same walk is still a change.
    let other = list(&[("song.ts", Track::Text, stamp(11, OLD)), ("data/kick/hits.json", Track::Text, stamp(20, OLD))]);
    assert_eq!(s.poll_in(t0, WALL, &other, Scope::all().except(&mine), Git::default(), &mut hash), Poll::Settling);
    assert_eq!(s.batch_paths().collect::<Vec<_>>(), ["song.ts"]);
}

/// A small generator, for the property below.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// The poll as step 6 visited paths: every path of the listing, the baseline and the batch, made
/// one set and looked up one by one. The oracle for the merge walk.
fn visit_union(s: &mut Settle, wall_ns: i128, listing: &Listing, hash: &mut dyn FnMut(&Path) -> Option<Sha>) -> Vec<String> {
    let batch = s.batch.iter().flat_map(|b| b.paths.keys());
    let paths: BTreeSet<String> = listing.entries.keys().chain(s.baseline.keys()).chain(batch).cloned().collect();
    let mut changed = vec![];
    for p in &paths {
        let in_batch = s.batch.as_ref().is_some_and(|b| b.paths.contains_key(p));
        if differs(s.baseline.get_mut(p), listing.entries.get(p).copied(), p, wall_ns, hash) || in_batch {
            changed.push(p.clone());
        }
    }
    changed
}

// WRITE-19: the merge walk of a full poll visits exactly what the union of every path did, in the
// same order, with the same hashes asked for, over generated listings (paths whose bytes and
// parts sort differently among them: `data/a-b.json` and `data/a/x.json`).
#[test]
fn the_merge_poll_equals_the_union_poll() {
    let pool =
        ["song.ts", "data/a-b.json", "data/a/x.json", "data/a/y.json", "data/a.json", "assets/x.wav", "assets/x/y.wav", "tracks/t.ts"];
    let mut rng = Lcg(42);
    for round in 0..200 {
        let gen_listing = |rng: &mut Lcg| {
            let mut l = Listing::default();
            for p in pool {
                if !rng.next().is_multiple_of(3) {
                    let track = [Track::Text, Track::Binary, Track::Present][(rng.next() % 3) as usize];
                    // A third racy, so hashing is asked for.
                    let mtime = if rng.next().is_multiple_of(3) { WALL - 1 } else { OLD };
                    l.entries.insert(p.to_string(), (track, stamp(rng.next() % 3, mtime)));
                }
            }
            l
        };
        let base = gen_listing(&mut rng);
        let (mut a, mut b) = (Settle::new(&base, WALL), Settle::new(&base, WALL));
        let t0 = Instant::now();
        for i in 0..12i128 {
            let l = gen_listing(&mut rng);
            let salt = rng.next() % 2;
            let (mut asked_a, mut asked_b) = (vec![], vec![]);
            let mut ha = |p: &Path| {
                asked_a.push(p.to_path_buf());
                Some(Sha::of(format!("{}{salt}", p.display()).as_bytes()))
            };
            let changed_a = a.visit(WALL + i, &l, Scope::all(), &mut ha);
            let mut hb = |p: &Path| {
                asked_b.push(p.to_path_buf());
                Some(Sha::of(format!("{}{salt}", p.display()).as_bytes()))
            };
            let changed_b = visit_union(&mut b, WALL + i, &l, &mut hb);
            assert_eq!(changed_a, changed_b, "round {round}, poll {i}");
            assert_eq!(asked_a, asked_b, "round {round}, poll {i}");
            let at = t0 + Duration::from_millis(50 * i as u64);
            let (pa, pb) = (
                a.conclude(changed_a, at, WALL + i, &l, Scope::all(), Git::default(), &mut |_| None),
                b.conclude(changed_b, at, WALL + i, &l, Scope::all(), Git::default(), &mut |_| None),
            );
            assert_eq!(pa, pb, "round {round}, poll {i}");
            assert_eq!((&a.baseline, &a.batch), (&b.baseline, &b.batch), "round {round}, poll {i}");
        }
    }
}

// R14, WRITE-19: a walk that stats no asset only tracked by being there lists the same paths, as
// the same tracks, as one that stats every file; scoped walks of a folder, a file and a path with
// nothing there list what the full walk has there.
#[test]
fn listing_without_stats_for_present_assets_lists_the_same_paths() {
    let root = std::env::temp_dir().join(format!("m2-watch-listing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in [
        "music2.toml",
        "song.ts",
        "tracks/a.ts",
        "data/kick/hits.json",
        "data/kick/lead.state",
        "data/kick/notes.txt",
        "assets/one.wav",
        "assets/drums/two.wav",
        ".music2/tx/j.json",
        "node_modules/x/y.ts",
        "data/.hidden/x.json",
    ] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, f).unwrap();
    }
    let named: BTreeSet<String> = ["assets/one.wav".to_string()].into();
    let (fast, stamped) = (listing(&root, &named), listing_stamped(&root, &named));
    let shape = |l: &Listing| l.entries.iter().map(|(k, (t, _))| (k.clone(), *t)).collect::<Vec<_>>();
    assert_eq!(shape(&fast), shape(&stamped));
    // And the Ignored files a snapshot lists, as it lists them.
    assert_eq!(fast.unknown, *m2_song::Snapshot::read(&root).unwrap().unknown);
    assert_eq!(fast.unknown, [PathBuf::from("data/kick/notes.txt")].into());
    let paths: Vec<&str> = fast.entries.keys().map(String::as_str).collect();
    assert_eq!(
        paths,
        ["assets/drums/two.wav", "assets/one.wav", "data/kick/hits.json", "data/kick/lead.state", "music2.toml", "song.ts", "tracks/a.ts"]
    );
    assert_eq!(fast.entries["assets/drums/two.wav"], (Track::Present, Stamp::default()), "listed, not stat'd");
    assert_eq!(fast.entries["assets/one.wav"], stamped.entries["assets/one.wav"], "a named asset is stat'd");
    assert_eq!(fast.entries["song.ts"], stamped.entries["song.ts"]);
    let some: BTreeSet<String> =
        ["data/kick".into(), "song.ts".into(), "gone.ts".into(), "data/kick/hits.json".into(), ".music2".into()].into();
    let of = listing_of(&root, &some, &named);
    let want: Vec<&String> = fast.entries.keys().filter(|k| k.starts_with("data/kick/") || *k == "song.ts").collect();
    assert_eq!(of.entries.keys().collect::<Vec<_>>(), want);
    assert_eq!(of.entries["song.ts"], fast.entries["song.ts"]);
    let assets = listing_of(&root, &["assets".to_string(), "assets/drums/two.wav".to_string()].into(), &named);
    let want: BTreeMap<String, (Track, Stamp)> =
        fast.entries.iter().filter(|(k, _)| k.starts_with("assets/")).map(|(k, v)| (k.clone(), *v)).collect();
    assert_eq!(assets.entries, want, "stamped as the full walk stamps them");
    std::fs::remove_dir_all(&root).ok();
}

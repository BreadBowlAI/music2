use std::path::{Path, PathBuf};

use m2_runtime::Renderer;

use super::*;

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// A fresh copy of `examples/first-song`.
fn first_song() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-edit-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).unwrap(); }
        }
    }
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &dir);
    dir
}

fn control(s: &Session, file: &str, arg: &str) -> Control {
    s.controls().into_iter().find(|c| c.file == file && c.id.ends_with(&format!("/{arg}"))).unwrap_or_else(|| {
        panic!("no {file} …/{arg} in {:?}", s.controls().iter().map(|c| &c.id).collect::<Vec<_>>())
    })
}

fn render(s: &mut Session, preview: Option<(&Origin, f64)>) -> Vec<[f32; 2]> {
    let mut r = Renderer::for_project(&s.accepted().flat, &s.project, &s.packages, 48_000).unwrap();
    if let Some((o, v)) = preview {
        assert!(r.set_const(o, v) > 0, "the preview reached the graph");
    }
    r.render(1.0).unwrap()
}

#[test]
fn finds_the_numbers_written_in_the_project() {
    let dir = first_song();
    let s = Session::open(&dir, std_packages()).unwrap();
    let pan = control(&s, "song.ts", "0.hats.pan");
    assert_eq!((pan.value, pan.literal.as_str(), pan.scale), (0.3, "0.3", 1.0));
    let wobble_max = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "2.5").unwrap();
    assert_eq!((wobble_max.value, wobble_max.scale, wobble_max.unit), (2500.0, 1000.0, Some(Unit::Hz)));
    assert!(s.controls().iter().all(|c| !c.file.starts_with("std/")), "package code is not editable");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_edit_changes_only_its_bytes_and_preview_sounds_like_the_commit() {
    let dir = first_song();
    let song = dir.join("song.ts");
    let original = std::fs::read_to_string(&song).unwrap();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let pan = control(&s, "song.ts", "0.hats.pan");

    let previewed = render(&mut s, Some((&pan.origin, 0.45)));
    let c = s.set(&pan.id, 0.45, Some(0.3)).unwrap();
    assert_eq!((c.before.as_str(), c.after.as_str(), c.revision), ("0.3", "0.45", 2));
    let edited = std::fs::read_to_string(&song).unwrap();
    assert_eq!(edited, original.replacen("pan: 0.3", "pan: 0.45", 1), "only the number changed");
    assert_eq!(control(&s, "song.ts", "0.hats.pan").value, 0.45);
    let committed = render(&mut s, None);
    assert!(previewed == committed, "the live preview sounds exactly like the committed edit");

    // Undo puts every byte back.
    s.undo().unwrap();
    assert_eq!(std::fs::read_to_string(&song).unwrap(), original);
    assert!(s.undo().is_err(), "nothing left to undo");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn units_keep_their_spelling() {
    let dir = first_song();
    let bass = dir.join("tracks/bass.ts");
    let original = std::fs::read_to_string(&bass).unwrap();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let id = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "2.5").unwrap().id;
    s.set(&id, 900.0, None).unwrap();
    assert_eq!(std::fs::read_to_string(&bass).unwrap(), original.replacen("khz(2.5)", "khz(0.9)", 1));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn unrelated_saves_survive_and_overlapping_ones_conflict() {
    let dir = first_song();
    let song = dir.join("song.ts");
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let pan = control(&s, "song.ts", "0.hats.pan");

    // Someone else adds a comment while the knob is being dragged.
    let external = std::fs::read_to_string(&song).unwrap().replacen("// The root module", "// Edited elsewhere.\n// The root module", 1);
    std::fs::write(&song, &external).unwrap();
    s.set(&pan.id, 0.45, Some(0.3)).unwrap();
    let now = std::fs::read_to_string(&song).unwrap();
    assert!(now.starts_with("// Edited elsewhere.\n") && now.contains("pan: 0.45"), "both edits are kept");

    // Someone else changes the same number: conflict, nothing written.
    std::fs::write(&song, now.replacen("pan: 0.45", "pan: 0.2", 1)).unwrap();
    assert_eq!(s.set(&pan.id, 0.6, Some(0.45)), Err(EditError::Conflict { current: 0.2 }));
    assert!(std::fs::read_to_string(&song).unwrap().contains("pan: 0.2"));
    // And undo refuses to overwrite it.
    assert!(matches!(s.undo(), Err(EditError::Rejected(_))));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_broken_draft_keeps_playing_the_accepted_revision() {
    let dir = first_song();
    let song = dir.join("song.ts");
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let pan = control(&s, "song.ts", "0.hats.pan");
    let good = std::fs::read_to_string(&song).unwrap();
    std::fs::write(&song, good.replacen("master(", "master((", 1)).unwrap();
    assert!(s.refresh());
    assert!(!s.draft_errors.is_empty());
    assert!(matches!(s.set(&pan.id, 0.5, None), Err(EditError::DraftInvalid(_))));
    assert!(render(&mut s, None).iter().any(|f| f[0] != 0.0), "the accepted revision still plays");
    // Fixing the file makes it editable again.
    std::fs::write(&song, &good).unwrap();
    s.set(&pan.id, 0.5, None).unwrap();
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn verification_rejects_an_edit_that_changes_something_else() {
    // Rewrite the pan's bytes while claiming to change the hats' gain: the candidate
    // graph differs from the expected one, so nothing is written.
    let dir = first_song();
    let song = dir.join("song.ts");
    let original = std::fs::read_to_string(&song).unwrap();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let pan = control(&s, "song.ts", "0.hats.pan");
    let gain = s.controls().into_iter().find(|c| c.file == "song.ts" && c.literal == "-6").unwrap();
    let path = song.canonicalize().unwrap();
    let r = s.apply(&path, "song.ts", pan.start, pan.end, "0.9", &gain.origin, gain.value);
    assert!(matches!(&r, Err(EditError::Rejected(why)) if why.contains("more than this value")), "{r:?}");
    assert_eq!(std::fs::read_to_string(&song).unwrap(), original);
    assert_eq!(s.revision, 1);
    std::fs::remove_dir_all(dir).ok();
}

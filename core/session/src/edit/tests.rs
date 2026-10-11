use std::path::{Path, PathBuf};

use m2_runtime::Renderer;

use super::*;

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// A fresh copy of the stable editing fixture, independent of demo knob changes.
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
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-song"), &dir);
    dir
}

fn control(s: &Session, file: &str, arg: &str) -> Control {
    s.controls().into_iter().find(|c| c.file == file && c.id.ends_with(&format!("/{arg}"))).unwrap_or_else(|| {
        panic!("no {file} …/{arg} in {:?}", s.controls().iter().map(|c| &c.id).collect::<Vec<_>>())
    })
}

fn render(s: &mut Session, preview: Option<(&Origin, f64)>) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    let mut r = Renderer::for_project(&rev.compiled.flat, &rev.project, &s.packages, 48_000).unwrap();
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
    let c = s.set(pan.revision, &pan.id, 0.45, Some(0.3)).unwrap();
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
    s.set(1, &id, 900.0, None).unwrap();
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
    s.set(pan.revision, &pan.id, 0.45, Some(0.3)).unwrap();
    let now = std::fs::read_to_string(&song).unwrap();
    assert!(now.starts_with("// Edited elsewhere.\n") && now.contains("pan: 0.45"), "both edits are kept");

    // Someone else changes the same number: the old id is stale, nothing is written.
    let pan = control(&s, "song.ts", "0.hats.pan");
    std::fs::write(&song, now.replacen("pan: 0.45", "pan: 0.2", 1)).unwrap();
    assert_eq!(s.set(pan.revision, &pan.id, 0.6, Some(0.45)), Err(EditError::Stale { current: 4 }));
    // Listed again, it shows 0.2; a caller still expecting 0.45 gets a conflict.
    let pan = control(&s, "song.ts", "0.hats.pan");
    assert_eq!(pan.value, 0.2);
    assert_eq!(s.set(pan.revision, &pan.id, 0.6, Some(0.45)), Err(EditError::Conflict { current: 0.2 }));
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
    assert!(matches!(s.set(pan.revision, &pan.id, 0.5, None), Err(EditError::DraftInvalid(_))));
    // Inspecting the last working graph stays safe while the draft is broken.
    assert_eq!(control(&s, "song.ts", "0.hats.pan").literal, "0.3");
    assert!(render(&mut s, None).iter().any(|f| f[0] != 0.0), "the accepted revision still plays");
    // Fixing the file makes it editable again.
    std::fs::write(&song, &good).unwrap();
    s.set(pan.revision, &pan.id, 0.5, None).unwrap();
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
    let r = s.apply(Path::new("song.ts"), pan.lens.start, pan.lens.end, "0.9", &gain.origin, gain.value + 1.0, gain.value);
    assert!(matches!(&r, Err(EditError::Rejected(why)) if why.contains("more than this value")), "{r:?}");
    assert_eq!(std::fs::read_to_string(&song).unwrap(), original);
    assert_eq!(s.accepted().number, 1);
    std::fs::remove_dir_all(dir).ok();
}

/// A project from the given files, in a fresh temp dir.
fn project(files: &[(&str, &str)]) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-edit-proj-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    for (name, text) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

const TWO_OSCS: &str = r#"import { hz, song } from "@m2/core";
import { osc } from "std/dsp";
import { add } from "std/math";

export default song(() => {
  const s = osc.sine(hz(110));
  const w = osc.saw(hz(220));
  return add(s, w);
});
"#;

#[test]
fn a_stale_id_still_edits_the_knob_it_named() {
    let dir = project(&[("song.ts", TWO_OSCS)]);
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let sine = s.controls().into_iter().find(|c| c.literal == "110").unwrap();
    // Two calls are inserted above it elsewhere, which shifts call positions.
    let edited = TWO_OSCS.replace("  const s =", "  const t = osc.tri(hz(55));\n  const u = osc.tri(hz(66));\n  const s =");
    std::fs::write(dir.join("song.ts"), &edited).unwrap();
    s.set(sine.revision, &sine.id, 330.0, Some(110.0)).unwrap();
    let now = std::fs::read_to_string(dir.join("song.ts")).unwrap();
    assert_eq!(now, edited.replace("osc.sine(hz(110))", "osc.sine(hz(330))"), "the sine changed, nothing else");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_stale_id_whose_call_changed_is_refused() {
    let dir = project(&[("song.ts", TWO_OSCS)]);
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let sine = s.controls().into_iter().find(|c| c.literal == "110").unwrap();
    let edited = TWO_OSCS.replace("osc.sine(hz(110))", "osc.sine(hz(115))");
    std::fs::write(dir.join("song.ts"), &edited).unwrap();
    assert_eq!(s.set(sine.revision, &sine.id, 330.0, None), Err(EditError::Stale { current: 2 }));
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), edited);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_draft_with_bad_timeline_data_doesnt_replace_the_accepted_timeline() {
    let dir = first_song();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let notes = dir.join("timeline/bass.notes.json");
    let text = std::fs::read_to_string(&notes).unwrap();
    std::fs::write(&notes, text.replacen("\"A1\"", "\"H9\"", 1).replacen("\"length\": 32", "\"length\": 40", 1)).unwrap();
    assert!(s.refresh());
    assert!(!s.draft_errors.is_empty(), "H9 is not a pitch");
    assert!(render(&mut s, None).iter().any(|f| f[0] != 0.0), "the accepted revision, with its own timeline, still renders");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn timeline_changes_are_picked_up_even_when_the_length_stays() {
    let dir = first_song();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let song = dir.join("timeline/song.json");
    std::fs::write(&song, std::fs::read_to_string(&song).unwrap().replace("124", "62")).unwrap();
    assert!(s.refresh());
    assert_eq!(s.accepted().number, 2);
    assert_eq!(s.accepted().project.timeline.tempo().unwrap().bpm_at(0.0), 62.0);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn undo_restores_any_javascript_number_spelling() {
    for spelling in ["0x6e", "1_10", "(110)", "110.0", "1.1e2"] {
        let src = TWO_OSCS.replace("hz(110)", &format!("hz({spelling})"));
        let dir = project(&[("song.ts", &src)]);
        let mut s = Session::open(&dir, std_packages()).unwrap();
        let c = s.controls().into_iter().find(|c| c.value == 110.0).unwrap_or_else(|| panic!("{spelling} is a control"));
        s.set(c.revision, &c.id, 330.0, Some(110.0)).unwrap();
        s.undo().unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), src, "{spelling} restored byte for byte");
        std::fs::remove_dir_all(dir).ok();
    }
}

#[test]
fn the_view_shows_the_calls_written_in_the_project() {
    let dir = first_song();
    let s = Session::open(&dir, std_packages()).unwrap();
    let v = s.view();
    let module = |name: &str| v.modules.iter().find(|m| m.name == name).unwrap();
    let node = |m: &crate::ModuleView, label: &str| m.nodes.iter().find(|n| n.label == label).cloned().unwrap_or_else(|| panic!("no {label} in {}: {:?}", m.name, m.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()));

    // The bass track is a handful of calls, not the kernels they expand into.
    let bass = module("Bass");
    let labels: Vec<&str> = bass.nodes.iter().map(|n| n.label.as_str()).collect();
    for want in ["cutoff", "drop", "notes", "time.beats", "lfo", "xrange", "blend", "bass", "out"] {
        assert!(labels.contains(&want), "{want} missing from {labels:?}");
    }
    assert!(bass.nodes.len() < 14, "{labels:?}");
    let knob = |n: &crate::ViewNode, l: &str| n.knobs.iter().find(|k| k.label == l).cloned().unwrap_or_else(|| panic!("no knob {l} on {}", n.label));
    assert_eq!(knob(&node(bass, "lfo"), "period").value, 0.5);
    let hi = knob(&node(bass, "xrange"), "hi");
    assert_eq!((hi.value, hi.literal.as_str(), hi.unit, hi.log), (2500.0, "2.5", Some("hz"), true));
    let edge = |m: &crate::ModuleView, a: &str, b: &str| {
        let id = |l: &str| node(m, l).id;
        m.edges.contains(&(id(a), id(b)))
    };
    assert!(edge(bass, "lfo", "xrange") && edge(bass, "xrange", "blend") && edge(bass, "blend", "bass") && edge(bass, "bass", "out"));
    assert!(edge(bass, "notes", "bass"));
    assert_eq!(node(bass, "cutoff").kind, "lane");

    // The mixer carries the channel knobs, named by where they are written.
    let song = module("song");
    let mixer = node(song, "mixer");
    let pan = knob(&mixer, "hats.pan");
    assert_eq!((pan.value, pan.min, pan.max), (0.3, -1.0, 1.0), "pan feeds a bipolar port");
    assert_eq!(knob(&mixer, "hats.db").value, -6.0);
    assert_eq!(knob(&node(song, "duck"), "depth").value, -9.0);
    assert!(edge(song, "Kick.env", "duck") && edge(song, "duck", "mixer") && edge(song, "mixer", "master"));
    std::fs::remove_dir_all(dir).ok();
}

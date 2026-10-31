use std::path::{Path, PathBuf};

use m2_runtime::Renderer;

use m2_ir::{Origin, Unit};
use m2_runtime::PackageSet;
use m2_session::{Session, StepError};
use m2_song::tx::{Anchor, Expect, Range};

use super::*;

/// Where `start..end` of `old` is in `new`, when no change touched it: how ids and leases carry.
fn carry(old: &str, new: &str, start: usize, end: usize) -> Option<usize> {
    m2_session::text::Edits::between(old, new).carry(start..end).map(|r| r.start)
}

fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// A fresh copy of the stable editing fixture, independent of demo knob changes.
fn first_song() -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("m2-edit-test-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (f, t) = (e.path(), to.join(e.file_name()));
            if f.is_dir() {
                copy(&f, &t)
            } else {
                std::fs::copy(&f, &t).unwrap();
            }
        }
    }
    copy(&crate::testing::fixtures().join("first-song"), &dir);
    dir
}

fn control(s: &Session, file: &str, arg: &str) -> Control {
    s.controls()
        .into_iter()
        .find(|c| c.file == file && c.id.ends_with(&format!("/{arg}")))
        .unwrap_or_else(|| panic!("no {file} …/{arg} in {:?}", s.controls().iter().map(|c| &c.id).collect::<Vec<_>>()))
}

fn render(s: &mut Session, preview: Option<(&Origin, f64)>) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    let mut r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap();
    if let Some((o, v)) = preview {
        // As a lease's value reaches a renderer before it is sent: its cells, by CellId (RENDER-23).
        let cells = r.cell_map().cells(o).to_vec();
        assert!(!cells.is_empty(), "the preview reached the graph");
        r.set_cells(&cells, v);
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

    // Someone else changes the same number: the old id is stale, nothing is written. (A gesture
    // never reads the folder itself, R2: the change is taken in first. The outside comment rode
    // along with the knob's commit, so this is revision 3.)
    let pan = control(&s, "song.ts", "0.hats.pan");
    std::fs::write(&song, now.replacen("pan: 0.45", "pan: 0.2", 1)).unwrap();
    assert!(s.refresh());
    assert_eq!(s.set(pan.revision, &pan.id, 0.6, Some(0.45)), Err(EditError::Stale { current: 3 }));
    // Listed again, it shows 0.2; a caller still expecting 0.45 gets a conflict.
    let pan = control(&s, "song.ts", "0.hats.pan");
    assert_eq!(pan.value, 0.2);
    assert_eq!(s.set(pan.revision, &pan.id, 0.6, Some(0.45)), Err(EditError::Conflict { current: 0.2 }));
    assert!(std::fs::read_to_string(&song).unwrap().contains("pan: 0.2"));
    // And undo refuses to overwrite it.
    assert!(matches!(s.undo(), Err(StepError::Refused(_))));
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
    // Fixing the file makes it editable again, once taken in.
    std::fs::write(&song, &good).unwrap();
    assert!(s.refresh());
    let pan = control(&s, "song.ts", "0.hats.pan");
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
    let lens = pan.lens().unwrap();
    let g = gain.lens().unwrap();
    let file = plan::path("song.ts").unwrap();
    let op = plan::text_edit(&file, &original, &[(lens.start, lens.end, "0.9".into())]);
    let anchor = Anchor::Written { file, range: Range::bytes(g.start, g.end) };
    let tx = plan::planned(&s, ("0.3".into(), "0.9".into()), vec![op], Expect::Values(vec![(anchor, gain.value + 1.0)]));
    let said = |why: &str| EditError::Rejected(format!("the edit would change more than this value ({why})"));
    let r = plan::run_as(&mut s, tx, said, |_, _| Ok(()));
    assert!(matches!(&r, Err(EditError::Rejected(why)) if why.contains("more than this value")), "{r:?}");
    assert_eq!(std::fs::read_to_string(&song).unwrap(), original);
    assert_eq!(s.accepted().number, 1);
    std::fs::remove_dir_all(dir).ok();
}

/// A project from the given files, in a fresh temp dir.
fn project(files: &[(&str, &str)]) -> PathBuf {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir =
        std::env::temp_dir().join(format!("m2-edit-proj-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    for (name, text) in [("music2.toml", crate::testing::MANIFEST)].iter().chain(files) {
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
    assert!(s.refresh());
    assert_eq!(s.set(sine.revision, &sine.id, 330.0, None), Err(EditError::Stale { current: 2 }));
    assert_eq!(std::fs::read_to_string(dir.join("song.ts")).unwrap(), edited);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
// FMT-9: a lane file that fails in a running session is an Error naming the file, and the lane
// keeps playing the data last accepted for it; nothing else stops.
fn a_bad_lane_file_keeps_playing_its_last_accepted_data() {
    let dir = first_song();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let before = render(&mut s, None);
    let notes = dir.join("data/bass/notes.json");
    let text = std::fs::read_to_string(&notes).unwrap();
    std::fs::write(&notes, text.replacen("\"A1\"", "\"H9\"", 1)).unwrap();
    assert!(s.refresh());
    assert!(s.draft_errors.is_empty(), "a lane never blocks: {:?}", s.draft_errors);
    let problems = &s.accepted().compiled.problems;
    assert_eq!(problems.iter().map(|d| (d.code, d.src.as_deref())).collect::<Vec<_>>(), [("lane.invalid", Some("data/bass/notes.json"))]);
    assert!(render(&mut s, None) == before, "the bass plays the notes it had");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn timeline_changes_are_picked_up_even_when_the_length_stays() {
    let dir = first_song();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let song = dir.join("data/arrangement.json");
    std::fs::write(&song, std::fs::read_to_string(&song).unwrap().replace("124", "62")).unwrap();
    assert!(s.refresh());
    assert_eq!(s.accepted().number, 2);
    assert_eq!(s.accepted().tempo().bpm_at(0.0), 62.0);
    std::fs::remove_dir_all(dir).ok();
}

// WRITE-21, the step 6 design's 10.3: a value written in any JavaScript spelling is put back byte
// for byte, and undo is offered for it (can_undo agrees with what undo does, the step 1 gate).
#[test]
fn undo_restores_any_javascript_number_spelling() {
    for spelling in ["0x6e", "1_10", "0o156", "0b1101110", "(110)", "110.0", "1.1e2", ".11e3", "110."] {
        let src = TWO_OSCS.replace("hz(110)", &format!("hz({spelling})"));
        let dir = project(&[("song.ts", &src)]);
        let mut s = Session::open(&dir, std_packages()).unwrap();
        let c = s.controls().into_iter().find(|c| c.value == 110.0).unwrap_or_else(|| panic!("{spelling} is a control"));
        s.set(c.revision, &c.id, 330.0, Some(110.0)).unwrap();
        assert!(s.can_undo(), "{spelling}: undo is offered");
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
    let module = |name: &str| v.modules.iter().find(|m| m.key == name).unwrap();
    let node = |m: &crate::ModuleView, label: &str| {
        m.nodes
            .iter()
            .find(|n| n.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("no {label} in {}: {:?}", m.key, m.nodes.iter().map(|n| &n.label).collect::<Vec<_>>()))
    };

    // The bass track is a handful of calls, not the kernels they expand into.
    let bass = module("bass");
    let labels: Vec<&str> = bass.nodes.iter().map(|n| n.label.as_str()).collect();
    for want in ["cutoff", "drop", "notes", "time.beats", "lfo", "xrange", "blend", "bass", "out"] {
        assert!(labels.contains(&want), "{want} missing from {labels:?}");
    }
    assert!(bass.nodes.len() < 14, "{labels:?}");
    let knob = |n: &crate::ViewNode, l: &str| {
        n.knobs.iter().find(|k| k.label == l).cloned().unwrap_or_else(|| panic!("no knob {l} on {}", n.label))
    };
    assert_eq!(knob(&node(bass, "lfo"), "period").value, 0.5);
    let hi = knob(&node(bass, "xrange"), "hi");
    // `xrange`'s ends take any unit, so no range is declared for them (D4): the UI picks a span
    // for a frequency, and core guesses none (P10).
    assert_eq!(
        (hi.value.clone(), hi.literal.as_str(), hi.unit, hi.range, hi.log),
        (serde_json::json!(2500), "2.5", Some("hz"), None, false)
    );
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
    assert_eq!(
        (pan.value.clone(), pan.range, pan.slot.as_deref()),
        (serde_json::json!(0.3), Some([-1.0, 1.0]), Some("channels.hats.pan")),
        "the mixer declares pan"
    );
    assert_eq!(knob(&mixer, "hats.db").value, serde_json::json!(-6));
    assert_eq!(knob(&node(song, "duck"), "depth").value, serde_json::json!(-9));
    // An import is shown by the imported track's label, and addressed by its key (ID-10).
    assert!(edge(song, "Kick.env", "duck") && edge(song, "duck", "mixer") && edge(song, "mixer", "master"));
    std::fs::remove_dir_all(dir).ok();
}

/// S1: a knob set in tracks/bass.ts, then a call that makes no sound inserted in song.ts from
/// outside the app. song.ts is loaded first, so every call after it is numbered one higher.
#[test]
fn undo_finds_its_knob_after_a_call_is_inserted_in_a_file_loaded_before_it() {
    let dir = first_song();
    let (song, bass) = (dir.join("song.ts"), dir.join("tracks/bass.ts"));
    let original = std::fs::read_to_string(&bass).unwrap();
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let hi = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "2.5").unwrap();
    s.set(hi.revision, &hi.id, 900.0, Some(2500.0)).unwrap();
    let set = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "0.9").unwrap();

    let text = std::fs::read_to_string(&song).unwrap();
    std::fs::write(&song, text.replacen("export default", "Math.max(1, 2);\n\nexport default", 1)).unwrap();
    assert!(s.refresh() && s.draft_errors.is_empty() && s.accepted().number == 3);
    let moved = s.controls().into_iter().find(|c| c.file == "tracks/bass.ts" && c.literal == "0.9").unwrap();
    assert_ne!(moved.origin, set.origin, "the knob's call is numbered one higher");

    assert!(s.can_undo());
    let undone = s.undo().unwrap();
    assert_eq!((undone.file.as_str(), undone.before.as_str(), undone.after.as_str()), ("tracks/bass.ts", "0.9", "2.5"));
    assert_eq!(std::fs::read_to_string(&bass).unwrap(), original);
    assert!(std::fs::read_to_string(&song).unwrap().contains("Math.max(1, 2);"), "the outside edit stays");
    assert!(s.can_redo());
    s.redo().unwrap();
    assert_eq!(std::fs::read_to_string(&bass).unwrap(), original.replacen("khz(2.5)", "khz(0.9)", 1));
    std::fs::remove_dir_all(dir).ok();
}

/// A small deterministic generator for the carry property test.
struct Xorshift(u64);

impl Xorshift {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n.max(1) as u64) as usize
    }
}

// WRITE-12: a range carries exactly when no change touched it. Random line edits (insert, delete,
// replace) around a run of lines: with every line unique, the run is found where it went whenever
// no edit touched it, and never when one did.
#[test]
fn carrying_finds_a_range_wherever_no_edit_touched_it() {
    let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
    let mut fresh = 0;
    let mut line = |rng: &mut Xorshift| {
        fresh += 1;
        format!("  const v{fresh} = osc.sine(hz({}));\n", rng.below(1000))
    };
    for case in 0..400 {
        let n = 2 + rng.below(30);
        let old: Vec<String> = (0..n).map(|_| line(&mut rng)).collect();
        // The range: part of line `r0` to part of line `r1`.
        let r0 = rng.below(n);
        let r1 = (r0 + rng.below(3)).min(n - 1);
        let start_of = |lines: &[String], i: usize| lines[..i].iter().map(String::len).sum::<usize>();
        let start = start_of(&old, r0) + 2;
        let end = start_of(&old, r1) + old[r1].len() - 2;
        // Edits as (line, kind), applied back to front so earlier indexes stay valid.
        let mut edits: Vec<(usize, usize)> = (0..1 + rng.below(4)).map(|_| (rng.below(n + 1), rng.below(3))).collect();
        edits.sort();
        edits.dedup_by_key(|e| e.0);
        let mut new = old.clone();
        let mut touched = false;
        for &(at, kind) in edits.iter().rev() {
            match kind {
                // Insert before line `at`: inside the range only between its lines.
                0 => {
                    touched |= at > r0 && at <= r1;
                    new.insert(at, line(&mut rng));
                }
                1 if at < n => {
                    touched |= (r0..=r1).contains(&at);
                    new.remove(at);
                }
                _ if at < n => {
                    touched |= (r0..=r1).contains(&at);
                    new[at] = line(&mut rng);
                }
                _ => {}
            }
        }
        let (o, w) = (old.concat(), new.concat());
        let carried = carry(&o, &w, start, end);
        if touched {
            assert_eq!(carried, None, "case {case}: an edit touched the range");
        } else {
            let moved = new.iter().position(|l| *l == old[r0]).expect("kept");
            assert_eq!(carried, Some(start_of(&new, moved) + 2), "case {case}: the range is where its lines went");
        }
    }
}

// WRITE-12, WRITE-22: an agent's save that adds an import at the top of the file and a line at
// its end keeps a dragged control: its call is carried, by `Edits::between`, to the same control.
#[test]
fn a_control_is_carried_past_edits_above_and_below_its_call() {
    let dir = project(&[("song.ts", TWO_OSCS)]);
    let mut s = Session::open(&dir, std_packages()).unwrap();
    let sine = s.controls().into_iter().find(|c| c.literal == "110").unwrap();
    let edited = format!("import {{ lfo }} from \"std/control\";\n{TWO_OSCS}// the end\n");
    std::fs::write(dir.join("song.ts"), &edited).unwrap();
    assert!(s.refresh() && s.draft_errors.is_empty());
    let current = s.controls_by_id();
    let carried = s.carry(sine.revision, &sine.id, &current).unwrap();
    assert_eq!((carried.literal.as_str(), carried.value, carried.revision), ("110", 110.0, s.accepted().number));
    // Its own line changed too, past the call, while the lines above and below change back: no
    // change touched the call, so it carries (WRITE-12: a run of changed lines is refined by
    // tokens, ID-13); a change inside the call stops it.
    let commented = TWO_OSCS.replace("const s = osc.sine(hz(110));", "const s = osc.sine(hz(110)); // s");
    std::fs::write(dir.join("song.ts"), &commented).unwrap();
    assert!(s.refresh());
    let again = s.carry(carried.revision, &carried.id, &s.controls_by_id()).unwrap();
    assert_eq!((again.literal.as_str(), again.revision), ("110", s.accepted().number));
    std::fs::write(dir.join("song.ts"), commented.replace("hz(110)", "hz(110 /* Hz */)")).unwrap();
    assert!(s.refresh());
    assert_eq!(s.carry(again.revision, &again.id, &s.controls_by_id()), Err(EditError::Stale { current: s.accepted().number }));
    assert_eq!(carried.audition(330.0), Ok(330.0));
    assert!(carried.audition(f64::NAN).is_err());
    std::fs::remove_dir_all(dir).ok();
}

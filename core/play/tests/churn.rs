//! RENDER-25, RENDER-28 (§7.6, §12.7): activations leak nothing. Alone in its binary, so the
//! process-wide counts it compares see no other test.

mod common;

use std::sync::{Arc, Weak};

use common::*;
use m2_plugins::Owner;
use m2_runtime::audio::{Cache, RenderedClip};
use m2_runtime::{Basis, Keep, PlanInfo, Renderer, live_kernels};
use m2_session::{Revision, Session};

/// An edit: a file, and its new text from its first.
type Edit = (&'static str, Box<dyn Fn(&str) -> String>);

/// A plan of the session's accepted revision, prepared against `plays` (its plan's `info`) through
/// the session's names, as the server prepares one.
fn prepare(s: &Session, plays: &Revision, info: &PlanInfo, cache: &mut Cache) -> Renderer {
    let rev = s.accepted();
    let cont = m2_session::tx::continues::continues(plays, rev);
    let restarts = std::collections::HashSet::new();
    let keep = Keep { old: &plays.compiled.flat, old_data: &plays.compiled.data, info, cont: &cont, restarts: &restarts };
    let (flat, data) = (&rev.compiled.flat, &rev.compiled.data);
    let mut r = Renderer::prepare(flat, data, &s.packages, rev.project.sample_rate(), cache, &Owner::discard(), Basis::Keep(keep)).unwrap();
    cache.prune();
    r.set_strict(false);
    r
}

/// Play the session's project, then make each of `edits` (a file and its new text, or None to put
/// it back) while it plays, each an activation; then put everything back. Returns the kernel
/// instances alive while its first plan played and once it is over, the engine's leaks, and the
/// clip renders the run made that are still alive with nothing playing them.
fn churn(dir: &std::path::Path, edits: &[Edit]) -> (usize, usize, u64, usize) {
    let mut s = Session::open(dir, std_packages()).unwrap_or_else(|d| panic!("{d:?}"));
    let mut cache = Cache::default();
    let mut plays = s.accepted_arc();
    let rev = s.accepted();
    let r = Renderer::prepare(&rev.compiled.flat, &rev.compiled.data, &s.packages, SR, &mut cache, &Owner::discard(), Basis::Fresh).unwrap();
    let mut info = r.info();
    let (player, mut d) = playing(r, SR);
    player.play();
    hear(&mut d, SR as usize / 2, &mut |_, _| {});
    let start = live_kernels();
    let mut renders: Vec<Weak<RenderedClip>> = cache.renders();
    let originals: Vec<(String, String)> = edits.iter().map(|(f, _)| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap())).collect();
    let mut last = vec![];
    for (k, (file, edit)) in edits.iter().enumerate() {
        let original = &originals.iter().find(|(f, _)| f == file).unwrap().1;
        let text = edit(original);
        std::fs::write(dir.join(file), &text).unwrap();
        assert!(s.refresh(), "edit {k} of {file} changes the project");
        let r = prepare(&s, &plays, &info, &mut cache);
        renders.extend(cache.renders());
        (plays, info) = (s.accepted_arc(), r.info());
        player.activate(r).unwrap();
        hear(&mut d, SR as usize / 10, &mut |_, _| {});
        last.push(file.to_string());
    }
    // Everything back as it was: the plan that plays has the first one's structure.
    for (file, text) in &originals {
        std::fs::write(dir.join(file), text).unwrap();
    }
    if s.refresh() {
        let r = prepare(&s, &plays, &info, &mut cache);
        info = r.info();
        player.activate(r).unwrap();
    }
    hear(&mut d, SR as usize / 2, &mut |_, _| {});
    drop(d.drain_retired());
    let engine = d.engine().unwrap();
    assert!(engine.refused_activations() == 0 && engine.activations() as usize >= edits.len(), "every edit an activation");
    let leaked = engine.leaked();
    // Only what the cache holds for the playing plan may be alive.
    let held: Vec<Arc<RenderedClip>> = cache.renders().iter().filter_map(Weak::upgrade).collect();
    let stray = renders.iter().filter_map(Weak::upgrade).filter(|r| !held.iter().any(|h| Arc::ptr_eq(h, r))).count();
    drop(info);
    (start, live_kernels(), leaked, stray)
}

// RENDER-25, RENDER-28 (§12.7): 50 structural edits on first-song while it plays (effects wrapped
// around each track's instrument, unwrapped, replaced) and 5 tempo changes, then everything put
// back: the kernel instances alive are as many as before, nothing leaked; and on resample, 10 audio
// clip edits leave no clip render alive that no plan plays.
#[test]
fn random_structural_edits_leak_nothing() {
    let first = temp("churn-first-song");
    copy(&repo().join("examples/first-song"), &first);
    let tracks = ["bass", "hats", "keys", "kick"];
    let mut edits: Vec<Edit> = vec![];
    let file = |t: &str| -> &'static str { Box::leak(format!("tracks/{t}.ts").into_boxed_str()) };
    for k in 0..50 {
        let t = tracks[k % 4];
        let (fx, import) = [("delay", "delay"), ("reverb", "reverb"), ("", "")][(k / 4) % 3];
        let instrument = match t {
            "bass" => "bass(lane.notes(\"notes\")",
            "hats" => "hat(lane.notes(\"hits\"))",
            "keys" => "pluck(lane.notes(\"chords\"))",
            _ => "kick(lane.notes(\"hits\"))",
        };
        edits.push((
            file(t),
            Box::new(move |text: &str| {
                if fx.is_empty() {
                    // A replacement: the instrument's call written on a line of its own.
                    return text.replacen(instrument, &format!("\n    {instrument}"), 1);
                }
                let wrapped = if instrument.ends_with(')') && !instrument.starts_with("bass") {
                    text.replacen(instrument, &format!("{fx}({instrument})"), 1)
                } else {
                    text.replacen("return bass(", &format!("return {fx}(bass("), 1).replacen("drop) });", "drop) }));", 1)
                };
                format!("import {{ {import} }} from \"std/fx\";\n{wrapped}")
            }),
        ));
        if k % 10 == 9 {
            let bpm = 100 + 7 * k;
            edits.push(("data/arrangement.json", Box::new(move |_: &str| format!("{{\"tempo\": [{{\"beat\": 0, \"bpm\": {bpm}}}], \"meter\": []}}\n"))));
        }
    }
    let (before, after, leaked, _) = churn(&first, &edits);
    assert_eq!((after, leaked), (before, 0), "first-song: kernel instances alive before and after 55 activations");

    let resample = temp("churn-resample");
    copy(&repo().join("examples/resample"), &resample);
    let edits: Vec<Edit> = (0..10)
        .map(|k| {
            let edit: Box<dyn Fn(&str) -> String> = Box::new(move |t: &str| match k % 3 {
                0 => t.replacen("\"gain_db\": 0", &format!("\"gain_db\": -{}", k + 1), 1),
                1 => t.replacen("\"fade_out\": 0,", &format!("\"fade_out\": 0.0{},", k), 1),
                _ => t.replacen("\"start\": 4,", &format!("\"start\": 4.{k},"), 1),
            });
            ("data/chops/clips.json", edit)
        })
        .collect();
    let (before, after, leaked, stray) = churn(&resample, &edits);
    assert_eq!((after, leaked, stray), (before, 0, 0), "resample: no instance and no clip render left behind");
}

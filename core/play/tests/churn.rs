//! RENDER-25, RENDER-28 (§7.6, §12.7): activations leak nothing. Alone in its binary, so the
//! process-wide counts it compares see no other test; its two tests take turns (`ALONE`).

mod common;

use std::sync::{Arc, Mutex, Weak};

use common::*;
use m2_plugins::Owner;
use m2_runtime::audio::{AudioLane, Cache};
use m2_runtime::{Basis, Keep, PlanInfo, Renderer, live_kernels};
use m2_session::{Revision, Session};

/// An edit: a file, and its new text from its first.
type Edit = (&'static str, Box<dyn Fn(&str) -> String>);

/// Held by each test while it runs, so each counts what it made alone.
static ALONE: Mutex<()> = Mutex::new(());

/// What a churn left: the kernel instances alive while its first plan played and once it is over,
/// the engine's leaks, the clip renders the run made that are still alive with nothing playing
/// them, and the process's footprint, in bytes, after its warm-up edits and once it is over.
struct Left {
    start: usize,
    end: usize,
    leaked: u64,
    stray: usize,
    footprint: (u64, u64),
}

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
/// it back) while it plays, each an activation; then put everything back. The footprint is first
/// read after the first `warm` edits (0: while the first plan plays).
fn churn(dir: &std::path::Path, edits: &[Edit], warm: usize) -> Left {
    let mut s = Session::open(dir, std_packages()).unwrap_or_else(|d| panic!("{d:?}"));
    let mut cache = Cache::default();
    let mut plays = s.accepted_arc();
    let rev = s.accepted();
    let r =
        Renderer::prepare(&rev.compiled.flat, &rev.compiled.data, &s.packages, SR, &mut cache, &Owner::discard(), Basis::Fresh).unwrap();
    let mut info = r.info();
    let (player, mut d) = playing(r, SR);
    player.play();
    hear(&mut d, SR as usize / 2, &mut |_, _| {});
    let start = live_kernels();
    let footprint = || m2_os::process::memory().map_or(0, |m| m.footprint);
    let mut first = footprint();
    let mut renders: Vec<Weak<AudioLane>> = cache.lanes();
    let originals: Vec<(String, String)> =
        edits.iter().map(|(f, _)| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap())).collect();
    let mut last = vec![];
    for (k, (file, edit)) in edits.iter().enumerate() {
        let original = &originals.iter().find(|(f, _)| f == file).unwrap().1;
        let text = edit(original);
        std::fs::write(dir.join(file), &text).unwrap();
        assert!(s.refresh(), "edit {k} of {file} changes the project");
        let r = prepare(&s, &plays, &info, &mut cache);
        renders.extend(cache.lanes());
        (plays, info) = (s.accepted_arc(), r.info());
        player.activate(r).unwrap();
        hear(&mut d, SR as usize / 10, &mut |_, _| {});
        last.push(file.to_string());
        if k + 1 == warm {
            drop(d.drain_retired());
            first = footprint();
        }
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
    let held: Vec<Arc<AudioLane>> = cache.lanes().iter().filter_map(Weak::upgrade).collect();
    let stray = renders.iter().filter_map(Weak::upgrade).filter(|r| !held.iter().any(|h| Arc::ptr_eq(h, r))).count();
    drop(info);
    Left { start, end: live_kernels(), leaked, stray, footprint: (first, footprint()) }
}

// RENDER-25, RENDER-28 (§12.7): 50 structural edits on first-song while it plays (effects wrapped
// around each track's instrument, unwrapped, replaced) and 5 tempo changes, then everything put
// back: the kernel instances alive are as many as before, nothing leaked; and on resample, 10 audio
// clip edits leave no clip render alive that no plan plays.
#[test]
fn random_structural_edits_leak_nothing() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let first = temp("churn-first-song");
    copy(&repo().join("examples/first-song"), &first);
    // The edits wrap @m2/effects' delay and reverb: the song lists it, as adding one would (PACK-10).
    let manifest = std::fs::read_to_string(first.join("music2.toml")).unwrap() + "\"@m2/effects\" = \"1\"\n";
    std::fs::write(first.join("music2.toml"), manifest).unwrap();
    let tracks = ["bass", "hats", "keys", "kick"];
    let mut edits: Vec<Edit> = vec![];
    let file = |t: &str| -> &'static str { Box::leak(format!("tracks/{t}.ts").into_boxed_str()) };
    for k in 0..50 {
        let t = tracks[k % 4];
        let (fx, import) = [("delay", "delay"), ("reverb", "reverb"), ("", "")][(k / 4) % 3];
        let instrument = match t {
            "bass" => "bass(lane.notes(\"notes\")",
            "hats" => "drums.drum(lane.notes(\"hits\"), { drum: \"hats\" })",
            "keys" => "pluck(lane.notes(\"chords\"))",
            _ => "drums.drum(lane.notes(\"hits\"))",
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
                format!("import {{ {import} }} from \"@m2/effects\";\n{wrapped}")
            }),
        ));
        if k % 10 == 9 {
            let bpm = 100 + 7 * k;
            edits.push((
                "data/arrangement.json",
                Box::new(move |_: &str| format!("{{\"tempo\": [{{\"beat\": 0, \"bpm\": {bpm}}}], \"meter\": []}}\n")),
            ));
        }
    }
    let left = churn(&first, &edits, 0);
    assert_eq!((left.end, left.leaked), (left.start, 0), "first-song: kernel instances alive before and after 55 activations");

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
    let left = churn(&resample, &edits, 0);
    assert_eq!((left.end, left.leaked, left.stray), (left.start, 0, 0), "resample: no instance and no clip render left behind");
}

// RENDER-25, RENDER-28 (§12.7), recorded: 200 structural edits on p16 while it plays (each track's
// instrument wrapped in a delay or a reverb, or its call moved to a line of its own) and 10 tempo
// changes, then everything put back: the kernel instances alive are as many as before, nothing
// leaked, and the process's footprint grows by at most 20 MB. The footprint is first read after
// 70 edits more, once the session's ring of 64 past revisions is full: over the first 60 the
// process grows by about 100 MB, then stays flat (measured every 20 edits; bounded, not a leak).
// `cargo test -p m2-play --release --test churn -- --ignored`.
#[test]
#[ignore = "about half a minute in release"]
fn p16_churns_200_edits_within_20_mb() {
    let _alone = ALONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = temp("churn-p16");
    std::fs::remove_dir(&dir).unwrap();
    m2_session::reference::write(&dir, &m2_session::reference::Reference::P16).unwrap();
    let file = |k: usize| -> &'static str { Box::leak(format!("tracks/t{:02}.ts", k % 16 + 1).into_boxed_str()) };
    let mut edits: Vec<Edit> = vec![];
    let (warm, measured) = (70, 200);
    for k in 0..warm + measured {
        // p16's tracks cycle bass, pluck and pad; each file imports delay and reverb already.
        let instrument = ["bass(lane.notes(\"notes\"))", "pluck(lane.notes(\"notes\"))", "pad(lane.notes(\"notes\"))"][k % 16 % 3];
        let how = (k / 16) % 3;
        edits.push((
            file(k),
            Box::new(move |text: &str| match how {
                0 => text.replacen(instrument, &format!("delay({instrument})"), 1),
                1 => text.replacen(instrument, &format!("reverb({instrument})"), 1),
                _ => text.replacen(instrument, &format!("\n    {instrument}"), 1),
            }),
        ));
        if k % 20 == 19 {
            let bpm = 100 + k / 4;
            edits.push((
                "data/arrangement.json",
                Box::new(move |_: &str| format!("{{\"tempo\": [{{\"beat\": 0, \"bpm\": {bpm}}}], \"meter\": []}}\n")),
            ));
        }
    }
    let left = churn(&dir, &edits, warm + warm / 20);
    let (first, last) = left.footprint;
    let mb = |b: u64| b as f64 / 1e6;
    let grew = mb(last) - mb(first);
    let n = edits.len();
    eprintln!("p16, {n} activations: footprint {:.1} MB after {warm} edits, then {:.1} MB ({grew:+.1} MB)", mb(first), mb(last));
    assert_eq!((left.end, left.leaked), (left.start, 0), "p16: kernel instances alive before and after {n} activations");
    assert!(first > 0 && grew <= 20.0, "p16: the footprint grew by {grew:.1} MB over {measured} edits");
}

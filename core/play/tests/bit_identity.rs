//! RENDER-33: live playback from transport 0, on a fresh engine, with no edits, no jumps and no
//! overlays, driven with arbitrary device buffer sizes, gives the same `f32` bits as the offline
//! render. Step 3's gate, run on every pure fixture from step 3 on.
//!
//! "Fresh" matters: an engine that sat stopped has run its free-running sources on (RENDER-6a),
//! so it plays them from another phase than the export.

mod common;

use std::sync::Arc;

use common::*;
use m2_runtime::{Continues, LaneSwap, PlanInfo, Renderer, notes_data};
use m2_song::{DataSet, Project};
use serde_json::json;

/// `song` live through the player, from 0 for `frames`, with a no-op swap of every lane at
/// frame `swap_at` when given.
fn live(song: &Song, frames: usize, swap_at: Option<usize>) -> Vec<[f32; 2]> {
    let r = song.renderer();
    let generation = r.generation();
    let slots = r.lane_slots().to_vec();
    let (player, mut d) = playing(r, song.rate);
    let mut swapped = false;
    hear(&mut d, frames, &mut |_, done| {
        if swap_at.is_some_and(|at| done >= at) && !swapped {
            swapped = true;
            for (i, s) in slots.iter().enumerate() {
                let data = match s.kind {
                    m2_ir::Kind::Events(_) => LaneSwap::Notes(Arc::new(notes_data(&song.data, &s.module, &s.lane, song.rate as f64))),
                    m2_ir::Kind::Control(_) => LaneSwap::Control(Arc::new(song.data.control(&s.module, &s.lane))),
                    m2_ir::Kind::Audio(_) => continue,
                };
                player.swap_lane(generation, i as u32, data).unwrap();
            }
        }
    })
}

fn offline(song: &Song, frames: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    song.renderer().process(&mut out).unwrap();
    out
}

fn check(name: &str, song: &Song, frames: usize, swap_at: Option<usize>) {
    let export = offline(song, frames);
    let heard = live(song, frames, swap_at);
    assert!(peak(&export) > 0.0 || name.contains("empty"), "{name} makes sound");
    assert_eq!(first_diff(&heard, &export), None, "{name}: live differs from the export");
}

// RENDER-33 on every pure fixture: their whole song and a second past it.
#[test]
fn live_is_the_export_on_every_pure_fixture() {
    for name in ["examples/first-song", "examples/resample", "templates/starter", "templates/empty"] {
        let song = fixture(name);
        let frames = (song.seconds() * song.rate as f64) as usize;
        check(name, &song, frames, None);
    }
}

// RENDER-26, RENDER-26a: a swap of the same data, while notes sound, changes nothing at all.
#[test]
fn a_no_op_swap_changes_no_bit() {
    let song = fixture("examples/first-song");
    check("first-song with a swap at 5 s", &song, 8 * SR as usize, Some(5 * SR as usize));
    // Equal keys: duplicate notes in a clip, layered clips with one id, anonymous clips at one start.
    let dup = |len: f64| json!({"pitch": 60, "start": 0, "length": len});
    let lane = json!({"clips": [
        {"id": "a", "start": 0, "length": 16, "notes": [dup(8.0), dup(12.0), {"pitch": 64, "start": 2, "length": 10}]},
        {"id": "l", "start": 4, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 8}]},
        {"id": "l", "start": 4, "length": 8, "notes": [{"pitch": 67, "start": 0, "length": 8}]},
        {"start": 6, "length": 8, "notes": [{"pitch": 72, "start": 0, "length": 6}]},
        {"start": 6, "length": 8, "notes": [{"pitch": 72, "start": 0, "length": 6}]}]});
    let song = synth(0.1, json!([]), "v", lane, at_bpm(120.0));
    check("duplicates with a swap at 3.5 s", &song, 9 * SR as usize, Some(7 * SR as usize / 2));
}

/// The 16-track reference project (p16), written into a temp folder.
fn p16() -> Song {
    let dir = temp("p16");
    std::fs::remove_dir(&dir).unwrap();
    m2_session::reference::write(&dir, &m2_session::reference::Reference::P16).unwrap();
    compile(dir)
}

// RENDER-33 on p16: its first 30 seconds in CI.
#[test]
fn live_is_the_export_on_p16() {
    check("p16, 30 s", &p16(), 30 * SR as usize, None);
}

// RENDER-33 on all of p16 (4:02), recorded: `cargo test -p m2-play --release -- --ignored`.
#[test]
#[ignore = "about a minute in release"]
fn live_is_the_export_on_all_of_p16() {
    let song = p16();
    let frames = (song.seconds() * SR as f64) as usize;
    check("p16", &song, frames, None);
}

/// `song` live from 0 for `frames`, with a plan activated at each of `at`, each made by `next`
/// against the plan sent before it; every activation must keep every instance and fade nothing.
fn activated(song: &Song, frames: usize, at: &[usize], next: &dyn Fn(&PlanInfo) -> Renderer) -> Vec<[f32; 2]> {
    let r = song.renderer();
    let mut info = r.info();
    let (player, mut d) = playing(r, song.rate);
    let mut sent = 0;
    let mut stats = vec![];
    let out = hear(&mut d, frames, &mut |d, done| {
        // Each activation's stats, once it plays.
        let cur = d.engine().and_then(|e| e.current()).filter(|c| sent > stats.len() && c.generation() == info.id);
        if let Some(c) = cur {
            stats.push(c.stats().clone());
        }
        if at.get(sent).is_some_and(|&a| done >= a) && stats.len() == sent {
            let r = next(&info);
            info = r.info();
            player.activate(r).unwrap();
            sent += 1;
        }
    });
    assert_eq!((sent, stats.len()), (at.len(), at.len()));
    for s in &stats {
        assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.resets), (0, 0, 0, 0), "{s:?}");
        assert!(s.kept.total() > 0);
    }
    out
}

/// Three points to activate at, from 5 s on, while notes sound.
fn points(song: &Song) -> Vec<usize> {
    let end = (song.seconds() * song.rate as f64) as usize;
    [5 * SR as usize, 5 * SR as usize + 77_777, 5 * SR as usize + 151_515].into_iter().filter(|&a| a + SR as usize <= end).collect()
}

/// The fixtures G7c plays: every pure one, and p16.
fn g7c() -> Vec<(String, Song)> {
    let mut out: Vec<(String, Song)> =
        ["examples/first-song", "examples/resample", "templates/starter"].into_iter().map(|n| (n.to_string(), fixture(n))).collect();
    out.push(("p16".into(), p16()));
    out
}

// RENDER-33, RENDER-25 (G7c): a plan of the same revision activated while playing, three times,
// mid-note, keeps every instance (schedulers, voice pools, rings, audio players, cells) and
// changes no bit of the export.
#[test]
fn a_no_op_activation_changes_no_bit() {
    for (name, song) in g7c() {
        let frames = 10 * SR as usize;
        let export = offline(&song, frames);
        let heard = activated(&song, frames, &points(&song), &|info| song.against(&song, info, &Continues::identity(&song.flat)));
        assert_eq!(first_diff(&heard, &export), None, "{name}: a no-op activation changed the output");
    }
}

// RENDER-27, RENDER-33 (G7c): an activation whose every lane comes with its data rebuilt (new data
// and renders, the same contents) swaps every lane and crossfades every audio lane, and changes no
// bit: equal sides stay equal through the crossfade.
#[test]
fn an_activation_with_rebuilt_lane_data_changes_no_bit() {
    for (name, song) in g7c() {
        let frames = 10 * SR as usize;
        let export = offline(&song, frames);
        let as_new = song.with_data(DataSet::empty());
        // Against no data: every lane compares changed, so each moves in with its data again.
        let heard = activated(&song, frames, &points(&song), &|info| song.against(&as_new, info, &Continues::identity(&song.flat)));
        assert_eq!(first_diff(&heard, &export), None, "{name}: rebuilt lane data changed the output");
    }
}

// RENDER-27, RENDER-33 (G7c): an arrangement read again (an activation of the same structure with
// equal tables and renders, from a new read of the project) changes no bit.
#[test]
fn an_identity_arrangement_activation_changes_no_bit() {
    let song = fixture("examples/resample");
    let frames = 10 * SR as usize;
    let export = offline(&song, frames);
    let decls = m2_session::compile(&Project::open(&song.dir).unwrap(), &mut std_packages()).unwrap().decls.clone();
    let reread = song.with_data(Project::open(&song.dir).unwrap().data(&decls, None).0);
    let heard = activated(&song, frames, &points(&song), &|info| reread.against(&song, info, &Continues::identity(&song.flat)));
    assert_eq!(first_diff(&heard, &export), None, "an arrangement read again changed the output");
}

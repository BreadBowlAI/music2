//! Plugin instances carried across rebuilds (HOST-5, RENDER-25, step 3b §5), with the probe plugin
//! (`m2_plugins::probe`): what carries and what doesn't, the notes sounding into a carried
//! instance, lending during the crossfade, retiring, and idling with plugins that sleep.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use m2_plugins::probe::{self, Note, Stats};
use m2_plugins::{Handle, Owner};

use super::*;
use crate::{Carry, Fresh, InstanceSource};

/// 120 bpm: a beat is 24 000 frames.
const BEAT: u64 = 24_000;

fn probe_packages() -> PackageSet {
    m2_plugins::test_main_thread();
    probe::install();
    std_packages()
}

/// A probe on the notes lane, its gain a constant, keyed `key` when given; `more` adds nodes after
/// it (the output is the last plugin, or `out`).
fn probe_song(pk: &mut PackageSet, gain: f64, key: Option<&str>, more: Value, out: &str) -> Flat {
    let mut p = json!({"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n", "gain_1": "g"}});
    if let Some(k) = key {
        p["key"] = json!(k);
    }
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"}), json!({"id": "g", "op": "const", "value": gain}), p];
    nodes.extend(more.as_array().unwrap().iter().cloned());
    let mut ir = song(Value::Array(nodes), out, json!([{"key": "notes", "kind": "events<note>"}]));
    let diags = pk.bind_plugins(&mut ir);
    assert!(diags.is_empty(), "{diags:?}");
    check(&ir, &*pk).unwrap_or_else(|e| panic!("{e:?}"))
}

/// A chord of `pitches` over beats 0–3.5, then one note at beat 4.
fn chord(pitches: &[u32]) -> DataSet {
    let mut ns: Vec<Value> = pitches.iter().map(|p| json!({"pitch": p, "start": 0, "length": 3.5})).collect();
    ns.push(json!({"pitch": 72, "start": 4, "length": 1}));
    with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
}

/// A renderer as the live engine gets one: built from `source`, not strict, stopped at 0.
fn build(flat: &Flat, data: &DataSet, pk: &PackageSet, owner: &Owner, source: &mut dyn InstanceSource) -> Renderer {
    let mut r = Renderer::build_live(flat, data, pk, SR, &mut audio::Cache::default(), owner, source).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    r
}

fn n(a: &std::sync::atomic::AtomicU64) -> u64 {
    a.load(Ordering::Relaxed)
}

/// The probe's note events from steady time `from` on.
fn notes_from(s: &Stats, from: i64) -> Vec<(i64, Note, i32)> {
    s.notes().into_iter().filter(|e| e.0 >= from).collect()
}

fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(2) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

/// A chord playing into a probe, then a rebuild with the probe's gain edited, activated at beat 1:
/// the rig, the probe's handle and record, and the engine clock of the activation's block.
fn carried(new_data: &DataSet) -> (Rig, Handle, Arc<Stats>, u64, Owner) {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[57, 60, 64]);
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &data, &pk, &owner, &mut Fresh);
    let h = r1.plugin_handles()[0];
    let mut carry = Carry::of(&r1, 1);
    let mut rig = Rig::new(r1);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let edited = probe_song(&mut pk, 0.5, None, json!([]), "p");
    let r2 = build(&edited, new_data, &pk, &owner, carry.source(1));
    assert_eq!(r2.plugin_handles(), [h], "the edit's renderer took the instance");
    assert_eq!(owner.live(), 1, "and created none");
    let at = rig.e.clock();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    (rig, h, probe::stats(h).unwrap(), at, owner)
}

// HOST-5, RENDER-25, C7: an edit's renderer takes the instance its code still names, with its
// state and the notes sounding into it: the chord goes on with the same ids, no Off reaches the
// probe at the activation, and its Offs come at the chord's own end (beat 3.5). The new code's
// value is sent at once. The instance is processed once per engine block through the crossfade,
// and the fading renderer copies what it made, bit for bit.
#[test]
fn a_carried_instance_keeps_its_notes() {
    let (mut rig, h, stats, at, _owner) = carried(&chord(&[57, 60, 64]));
    let before = n(&stats.process);
    let ons: Vec<i32> = stats.notes().iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [1, 2, 3]);
    rig.run(BLOCK);
    // The first block of the fade: the old renderer lent its node and copies the new one's output.
    let cur = rig.e.current().unwrap();
    let old = rig.e.fading().expect("fading");
    let node = |r: &Renderer| r.graph.exec.iter().position(|e| matches!(e, Exec::Plugin(_))).unwrap();
    let (ni, oi) = (node(cur), node(old));
    assert!(matches!(&old.graph.exec[oi], Exec::Plugin(p) if p.lent == Some(ni)));
    assert_eq!(old.graph.bufs[oi][0], cur.graph.bufs[ni][0], "the lent node holds the instance's own output");
    let views = cur.plugins();
    assert_eq!((views[0].handle, views[0].notes.clone(), views[0].offs), (h, vec![(1, 57), (2, 60), (3, 64)], 0), "the chord moved");
    assert!(old.plugins()[0].lent && old.plugins()[0].notes.is_empty());
    let gain = m2_plugins::host().main_call(h, m2_plugins::MainCall::Value(probe::GAIN));
    assert!(matches!(gain, Ok(m2_plugins::Reply::Value(Some(v))) if v == 0.5), "the new code's gain is sent at once: {gain:?}");
    rig.run(4 * BLOCK);
    assert!(rig.e.fading().is_none(), "the fade is over");
    assert_eq!(n(&stats.process) - before, 5, "once per engine block, through the fade");
    assert_eq!(n(&stats.doubled), 0);
    // Nothing ends at the activation; the chord ends where it was written.
    rig.run(3 * BEAT as usize);
    let after = notes_from(&stats, at as i64);
    let offs: Vec<(i64, i32)> = after.iter().filter(|e| e.1 != Note::On).map(|e| (e.0, e.2)).collect();
    let end = (3 * BEAT + BEAT / 2) as i64;
    assert_eq!(offs.iter().map(|o| o.1).collect::<Vec<_>>(), [1, 2, 3], "{after:?}");
    assert!(offs.iter().all(|o| o.0 == end), "the chord's Offs at beat 3.5: {offs:?}");
    assert_eq!(n(&stats.unknown_offs) + n(&stats.repeated_ids), 0);
    let ons: Vec<i32> = after.iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [4], "the note at beat 4 takes the next id");
    rig.run(BEAT as usize);
    assert_eq!(rig.e.current().unwrap().held_notes(), 0, "nothing stuck");
}

// RENDER-26a, C7: a note the edit removed is let go at the activation's first frame; the others go
// on. Ids never repeat into the carried instance.
#[test]
fn notes_the_edit_removed_are_let_go() {
    let (mut rig, _h, stats, at, _owner) = carried(&chord(&[57, 60]));
    rig.run(BEAT as usize * 4);
    let after = notes_from(&stats, at as i64);
    let (first, rest) = after.split_first().expect("events after the activation");
    assert_eq!(*first, (at as i64, Note::Off, 3), "the dropped top note, at the activation");
    let end = (3 * BEAT + BEAT / 2) as i64;
    assert_eq!(rest[..2].iter().map(|e| (e.0, e.1, e.2)).collect::<Vec<_>>(), [(end, Note::Off, 1), (end, Note::Off, 2)]);
    assert_eq!(rest[2].1, Note::On);
    assert!(rest[2].2 > 3, "a new note never takes an id the instance had: {after:?}");
    assert_eq!(n(&stats.repeated_ids) + n(&stats.unknown_offs), 0);
}

// RENDER-26a, C7: ids never repeat into a carried instance, not even after a renderer whose node
// read no notes (the code took `notes:` off the plugin, then put it back), which has no scheduler
// to pass them on: the instance keeps them.
#[test]
fn ids_never_repeat_after_a_renderer_without_notes() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let without = {
        let p = json!([{"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {}}]);
        let mut ir = song(p, "p", json!([{"key": "notes", "kind": "events<note>"}]));
        assert!(pk.bind_plugins(&mut ir).is_empty());
        check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"))
    };
    let with = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r0 = build(&with, &chord(&[57, 60, 64]), &pk, &owner, &mut Fresh);
    let h = r0.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut carry = Carry::of(&r0, 1);
    let mut rig = Rig::new(r0);
    rig.cmd(Cmd::Play);
    // The chord (ids 1 to 3), and the note at beat 4 (id 4).
    rig.run((4 * BEAT + BEAT / 4) as usize);
    let r1 = build(&without, &chord(&[57, 60, 64]), &pk, &owner, carry.source(1));
    assert_eq!(r1.plugin_handles(), [h], "carried without its notes");
    let mut carry = Carry::of(&r1, 1);
    rig.send(Msg::Activate { renderer: Box::new(r1) });
    rig.run(BEAT as usize);
    let note = json!({"pitch": 62, "start": 7, "length": 1});
    let later = with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 16, "notes": [note]}]}), at_bpm(120.0));
    let r2 = build(&with, &later, &pk, &owner, carry.source(1));
    assert_eq!(r2.plugin_handles(), [h], "and with them again");
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(3 * BEAT as usize);
    let ons: Vec<i32> = stats.notes().iter().filter(|e| e.1 == Note::On).map(|e| e.2).collect();
    assert_eq!(ons, [1, 2, 3, 4, 5], "the note at beat 7 takes an id the instance never had");
    assert_eq!(n(&stats.repeated_ids) + n(&stats.unknown_offs), 0);
}

// RENDER-25, RENDER-26a: without carrying (an instance built fresh) the chord isn't chased: the new
// probe hears nothing until the next note, and the old one is faded out. The reference the
// with-plugins gate's level check must fail on.
#[test]
fn a_fresh_instance_chases_nothing() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[57, 60, 64]);
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let mut rig = Rig::new(build(&flat, &data, &pk, &owner, &mut Fresh));
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let r2 = build(&probe_song(&mut pk, 0.5, None, json!([]), "p"), &data, &pk, &owner, &mut Fresh);
    let h2 = r2.plugin_handles()[0];
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    let out = rig.run(BEAT as usize / 2);
    assert_eq!(owner.live(), 2);
    assert!(peak(&out[out.len() / 2..]) == 0.0, "the new instance plays nothing of the chord");
    assert_eq!(probe::stats(h2).unwrap().notes(), [], "nothing chased");
}

// §5.3, RENDER-28: when the lane shrank, the new scheduler adopts what its room holds and the node
// lets go of the rest at the activation, with no event dropped (debug builds assert it) and none
// sent for a note the probe doesn't hold.
#[test]
fn a_lane_that_shrank_lets_go_of_what_doesnt_fit() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let lane = |n: usize| {
        let ns: Vec<Value> = (0..n).map(|k| json!({"pitch": 20 + k % 100, "start": (k / 100) as f64 * 0.001, "length": 4})).collect();
        with_notes(json!({"clips": [{"id": "c", "start": 0, "length": 8, "notes": ns}]}), at_bpm(120.0))
    };
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &lane(300), &pk, &owner, &mut Fresh);
    let h = r1.plugin_handles()[0];
    let mut carry = Carry::of(&r1, 1);
    let mut rig = Rig::new(r1);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    let stats = probe::stats(h).unwrap();
    assert_eq!(stats.sounding.load(Ordering::Relaxed), 300);
    let r2 = build(&probe_song(&mut pk, 0.5, None, json!([]), "p"), &lane(250), &pk, &owner, carry.source(1));
    assert_eq!(r2.caps(0).unwrap().sounding, Caps::FLOOR.sounding, "room for 256 notes");
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(2 * BLOCK);
    let held = rig.e.current().unwrap().plugins()[0].notes.len();
    assert!(held <= 250 && held > 0, "{held}");
    assert_eq!(stats.sounding.load(Ordering::Relaxed) as usize, held, "the probe holds what the node holds");
    assert_eq!((n(&stats.unknown_offs), n(&stats.full)), (0, 0));
    rig.run(4 * BEAT as usize);
    assert_eq!((stats.sounding.load(Ordering::Relaxed), rig.e.current().unwrap().held_notes()), (0, 0), "nothing stuck");
}

// HOST-5: a keyed instance follows its key when two of one plugin trade places in the code; an
// unkeyed one goes by its ordinal.
#[test]
fn keyed_instances_follow_their_keys() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let data = chord(&[60]);
    let second = |key: &str| json!({"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "key": key, "inputs": {"in": "p"}});
    let a = probe_song(&mut pk, 1.0, Some("a"), json!([second("b")]), "q");
    let r1 = build(&a, &data, &pk, &owner, &mut Fresh);
    let h1 = r1.plugin_handles();
    let mut carry = Carry::of(&r1, 7);
    let b = probe_song(&mut pk, 1.0, Some("b"), json!([second("a")]), "q");
    let r2 = build(&b, &data, &pk, &owner, carry.source(7));
    assert_eq!(r2.plugin_handles(), [h1[1], h1[0]], "each key kept its instance");
    assert_eq!(owner.live(), 2);
    // Another engine (a new player) never shares an instance.
    let r3 = build(&b, &data, &pk, &owner, Carry::of(&r2, 7).source(8));
    assert!(r3.plugin_handles().iter().all(|h| !h1.contains(h)));
    // Nor does a renderer at another rate.
    let mut other = Carry::of(&r2, 7);
    let key = r2.graph.exec.iter().find_map(|e| if let Exec::Plugin(p) = e { Some(p.inst.key().clone()) } else { None }).unwrap();
    assert!(other.source(7).take(&key, 44_100).is_none());
    assert!(other.source(7).take(&key, SR).is_some());
}

// HOST-5: a build that fails after it took instances from a carry (a later plugin failed to load)
// leaves the carry whole: the next build from it, the retry, still carries them. Within one build an
// instance is taken once.
#[test]
fn a_failed_build_leaves_its_carry_whole() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r1 = build(&flat, &chord(&[60]), &pk, &owner, &mut Fresh);
    let h1 = r1.plugin_handles();
    let key = r1.graph.exec.iter().find_map(|e| if let Exec::Plugin(p) = e { Some(p.inst.key().clone()) } else { None }).unwrap();
    let mut carry = Carry::of(&r1, 1);
    let source = carry.source(1);
    let taken = source.take(&key, SR);
    assert!(taken.is_some(), "a build takes it");
    assert!(source.take(&key, SR).is_none(), "once per build");
    // That build failed: what it took is dropped with it.
    drop(taken);
    let r2 = build(&flat, &chord(&[60]), &pk, &owner, carry.source(1));
    assert_eq!(r2.plugin_handles(), h1, "the retry carries it");
    assert_eq!(owner.live(), 1);
}

// HOST-5, RENDER-25: a changed state blob is a new instance. The old one is destroyed on the plugin
// main thread only once the renderer that played it is retired and dropped, never by the engine.
#[test]
fn a_changed_state_is_a_new_instance_and_the_old_one_goes_with_its_renderer() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let state = |tag: &str| {
        let dir = project(&[(
            "data/song/notes.json",
            json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 4}]}]}),
        )]);
        std::fs::write(dir.join("data/song/lead.state"), probe::state(tag, 1.0)).unwrap();
        let side = json!({"plugin": "clap:m2.probe", "plugin_version": "1.0.0"});
        std::fs::write(dir.join("data/song/lead.state.json"), side.to_string()).unwrap();
        dir
    };
    let flat = probe_song(&mut pk, 1.0, Some("lead"), json!([]), "p");
    // What the data is read against: the lane, and the state the keyed probe files (ID-8).
    let ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": "lead", "inputs": {"notes": "n"}}]),
        "p",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let (d1, d2) = (state("one"), state("two"));
    let (data1, data2) = (data(&ir, &d1), data(&ir, &d2));
    assert!(data1.state_hash("song", &["lead"]).is_some());
    let r1 = build(&flat, &data1, &pk, &owner, &mut Fresh);
    let h1 = r1.plugin_handles()[0];
    assert_eq!(*probe::stats(h1).unwrap().tag.lock().unwrap(), "one");
    // The same blob carries; another doesn't.
    let same = build(&flat, &data1, &pk, &owner, Carry::of(&r1, 1).source(1));
    assert_eq!(same.plugin_handles(), [h1]);
    drop(same);
    let mut carry = Carry::of(&r1, 1);
    let r2 = build(&flat, &data2, &pk, &owner, carry.source(1));
    let h2 = r2.plugin_handles()[0];
    assert_ne!(h1, h2);
    assert_eq!(*probe::stats(h2).unwrap().tag.lock().unwrap(), "two");
    drop(carry);
    let mut rig = Rig::new(r1);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(10 * BLOCK);
    assert_eq!(rig.retired_renderers().len(), 1, "the old renderer was retired");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(owner.live(), 2, "retired, but not dropped: its instance lives");
    rig.retired.clear();
    assert!(eventually(|| owner.live() == 1), "dropped: its instance is destroyed");
    assert!(probe::stats(h1).is_none());
    let calls = probe::tagged("one").pop().unwrap().main_calls();
    assert!(calls.iter().any(|c| c.what == "destroy" && c.says_main && !c.says_audio));
    std::fs::remove_dir_all(d1).ok();
    std::fs::remove_dir_all(d2).ok();
}

// RENDER-6a, §5.6: with every plugin asleep, a stopped engine idles; a plugin that asks to be
// processed from a thread of its own wakes it within one block.
#[test]
fn sleeping_plugins_let_the_engine_idle_and_wake_it() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let r = build(&flat, &chord(&[60]), &pk, &owner, &mut Fresh);
    let h = r.plugin_handles()[0];
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::new(r);
    rig.cmd(Cmd::Play);
    rig.run(BEAT as usize);
    rig.cmd(Cmd::Stop);
    rig.run(20 * BLOCK);
    assert!(!rig.e.is_idle(), "a plugin that doesn't sleep keeps the engine running");
    stats.sleep.store(true, Ordering::Relaxed);
    rig.run(20 * BLOCK);
    assert!(rig.e.is_idle(), "every plugin asleep and the output silent: idle");
    let calls = n(&stats.process);
    rig.run(4 * BLOCK);
    assert_eq!(n(&stats.process), calls, "idle: not processed");
    stats.sleep.store(false, Ordering::Relaxed);
    std::thread::spawn(move || assert!(probe::request_process(h))).join().unwrap();
    rig.run(BLOCK);
    assert!(!rig.e.is_idle() && n(&stats.process) == calls + 1, "woken at the next block");
}

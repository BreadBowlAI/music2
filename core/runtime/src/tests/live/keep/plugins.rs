//! Plugins across edits and exports (step 7's review fixes): what a kept instance holds when the
//! code stops setting a parameter, which instances an export may borrow, a plugin that fails live,
//! the render mode each render runs its plugins in, and the transport a plugin is told.

use super::*;

/// The keyed probe `lead` on the notes lane, its gain 1; with `level`, the code sets its level
/// (`level_2`) to that.
fn leveled(pk: &mut PackageSet, level: Option<f64>) -> Flat {
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"}), json!({"id": "g", "op": "const", "value": 1.0})];
    let mut inputs = json!({"notes": "n", "gain_1": "g"});
    if let Some(v) = level {
        nodes.push(json!({"id": "l", "op": "const", "value": v}));
        inputs["level_2"] = json!("l");
    }
    nodes.push(json!({"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": "lead", "inputs": inputs}));
    plugged(pk, song(Value::Array(nodes), "p", json!([{"key": "notes", "kind": "events<note>"}])))
}

/// How many of the plan last prepared's instances an export of `flat` with `data` may borrow.
fn lendable(rig: &Rig, flat: &Flat, data: &DataSet, pk: &PackageSet) -> usize {
    let (old, _, info) = rig.last.as_ref().unwrap();
    info.lend(flat, data, pk, &Continues::named(old, flat)).len()
}

// HOME-5a, RENDER-1: a parameter the code stops setting (its literal deleted) keeps, in the kept
// instance, the value it was last sent, which no file records: an export doesn't borrow that
// instance, and plays its node anew from the committed blob, as `m2 render` does. A literal whose
// value changed releases nothing. A state load that lands records the instance's state again.
#[test]
fn a_released_parameter_keeps_its_value_and_its_instance_isnt_lent() {
    let mut pk = probe_packages();
    let owner = Owner::discard();
    // The blob's level is 0.4; the code sets 0.05.
    let (data1, d1) = with_blob(Some(probe::state_with("one", 1.0, 0.4)));
    let set = leveled(&mut pk, Some(0.05));
    let r1 = build(&set, &data1, &pk, &owner);
    let h = r1.plugin_handles()[0];
    let mut rig = Rig::of(&set, &data1, r1);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    assert_eq!(param_of(h, probe::LEVEL), 0.05, "the code's level");
    assert_eq!(lendable(&rig, &set, &data1, &pk), 1);
    // Another value: still the code's.
    let other = leveled(&mut pk, Some(0.06));
    let r2 = rig.prepare_with(&other, &data1, &pk, &owner, &Continues::named(&set, &other)).unwrap();
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    rig.run(4 * BLOCK);
    assert_eq!((param_of(h, probe::LEVEL), lendable(&rig, &other, &data1, &pk)), (0.06, 1), "a new value releases nothing");
    // The literal deleted.
    let unset = leveled(&mut pk, None);
    let r3 = rig.prepare_with(&unset, &data1, &pk, &owner, &Continues::named(&other, &unset)).unwrap();
    rig.send(Msg::Activate { renderer: Box::new(r3) });
    rig.run(4 * BLOCK);
    assert_eq!(rig.e.current().unwrap().plugin_handles(), [h], "kept");
    assert_eq!(param_of(h, probe::LEVEL), 0.06, "it keeps the value it was last sent");
    assert_eq!(lendable(&rig, &unset, &data1, &pk), 0, "an export plays it anew, from the blob");
    // A new committed state lands: what it holds is recorded again.
    let (data2, d2) = with_blob(Some(probe::state_with("two", 1.0, 0.3)));
    let r4 = rig.prepare_with(&unset, &data2, &pk, &owner, &Continues::identity(&unset)).unwrap();
    assert_eq!(r4.stats().loads, 1);
    assert_eq!((param_of(h, probe::LEVEL), lendable(&rig, &unset, &data2, &pk)), (0.3, 1));
    for d in [d1, d2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-1: an export borrows only an instance that holds the export's committed blob. One whose
// blob changed since its plan was prepared (an edit whose build failed since) or is gone plays anew
// in the export, so an export never changes what a live instance holds; one that failed isn't
// lent either.
#[test]
fn an_export_borrows_only_an_instance_that_holds_its_blob() {
    let mut pk = probe_packages();
    let flat = probe_song(&mut pk, 1.0, Some("lead"), json!([]), "p");
    let (one, d1) = with_blob(Some(probe::state("one", 1.0)));
    let (two, d2) = with_blob(Some(probe::state("two", 1.0)));
    let (none, d3) = with_blob(None);
    let r = build(&flat, &one, &pk, &Owner::discard());
    let info = r.info();
    let lend = |data: &DataSet| info.lend(&flat, data, &pk, &Continues::identity(&flat)).len();
    assert_eq!((lend(&one), lend(&two), lend(&none)), (1, 0, 0));
    info.plugins[0].inst.upgrade().unwrap().kill();
    assert_eq!(lend(&one), 0, "a failed one isn't lent");
    for d in [d1, d2, d3] {
        std::fs::remove_dir_all(d).ok();
    }
}

// HOST-5, RENDER-25: live, a plugin that fails (here it reports a processing error: it was made
// inactive under the plan) goes silent alone, and is never called again: the song plays on, the
// renderer isn't broken, no crash is counted, and the instance is marked for the session to replace.
// A panic's reset doesn't call it either. Offline, the same failure fails the render.
#[test]
fn a_plugin_that_fails_live_goes_silent_alone() {
    let mut pk = probe_packages();
    let mut ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "n"}},
            {"id": "f", "op": "const", "value": 440, "unit": "hz"}, {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "pl", "op": "kernel", "kernel": "std:pan", "inputs": {"in": "o"}},
            {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "q", "b": "pl"}}]),
        "s",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    assert!(pk.bind_plugins(&mut ir).is_empty());
    let flat = check(&ir, &pk).unwrap();
    let data = chord(&[60]);
    let r = build(&flat, &data, &pk, &Owner::discard());
    let (h, inst) = (r.plugin_handles()[0], r.info().plugins[0].inst.upgrade().unwrap());
    let stats = probe::stats(h).unwrap();
    let mut rig = Rig::new(r);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    let q = node(&flat, "q");
    assert!(rig.e.current().unwrap().output_of(q).iter().any(|x| *x != 0.0), "the probe sounds");
    // Made inactive under the plan, as one that doesn't come back after an export is.
    m2_plugins::host().deactivate(h);
    let out = rig.run(4 * BLOCK);
    let cur = rig.e.current().unwrap();
    assert!(inst.dead() && !cur.broken() && rig.e.crashes() == 0 && cur.transport().playing(), "only the plugin failed");
    assert!(cur.output_of(q).iter().all(|x| *x == 0.0), "it is silent");
    assert!(out[BLOCK..].iter().any(|f| f[0] != 0.0), "the song plays on");
    let (calls, resets) = (n(&stats.process), n(&stats.resets));
    rig.send(Msg::Panic);
    rig.run(4 * BLOCK);
    assert_eq!((n(&stats.process), n(&stats.resets)), (calls, resets), "never called again");
    // Offline, a failing plugin fails the render.
    let mut offline = Renderer::new(&flat, &data, &pk, SR).unwrap();
    m2_plugins::host().deactivate(offline.plugin_handles()[0]);
    assert!(offline.render(0.1).is_err_and(|e| format!("{e:#}").contains("processing error")));
}

// RENDER-1, K4 of the step 7 review: every render that isn't live runs its plugins in CLAP's
// offline mode, set on the plugin main thread before each is activated: an offline render (`m2
// render`), a range render, an export's own instances, and the new instance an export makes for
// one it can't borrow. The live engine's run in realtime mode, CLAP's default, with no call. (A
// borrowed instance goes offline for the export and back after it:
// `an_export_borrows_and_gives_back_the_live_instances` in crates/server.)
#[test]
fn renders_that_arent_live_run_their_plugins_offline() {
    let mut pk = probe_packages();
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    let data = chord(&[60]);
    let modes = |h: Handle| -> Vec<&'static str> {
        let calls = probe::stats(h).unwrap().main_calls();
        calls.iter().map(|c| c.what).filter(|w| ["render_offline", "render_realtime", "activate"].contains(w)).collect()
    };
    let offline = Renderer::new(&flat, &data, &pk, SR).unwrap();
    assert_eq!(modes(offline.plugin_handles()[0]), ["render_offline", "activate"], "m2 render");
    let range = Renderer::starting_at(&flat, &data, &pk, SR, 24_000).unwrap();
    assert_eq!(modes(range.plugin_handles()[0]), ["render_offline", "activate"], "a range render");
    let own = Renderer::lending(&flat, &data, &pk, SR, &HashMap::new()).unwrap();
    assert_eq!(modes(own.plugin_handles()[0]), ["render_offline", "activate"], "an export's own");
    let live = build(&flat, &data, &pk, &Owner::discard());
    assert_eq!(modes(live.plugin_handles()[0]), ["activate"], "live: realtime, with no call");
    let found = pk.plugin_info("clap:m2.probe").unwrap().found.clone();
    let anew = live.info().plugins[0].inst.upgrade().unwrap().another(&found, None, SR).unwrap();
    assert_eq!(modes(anew.handle()), ["render_offline", "activate"], "one an export makes for one it can't borrow");
}

// HOST-6: every call tells a plugin the transport: playing or not, the loop, the tempo, the meter
// and its bar from the arrangement, and its song position; `steady_time` is the engine clock.
// Across a loop's wrap inside a block it is called twice, each call told its own run's transport.
#[test]
fn plugins_are_told_the_transport() {
    let mut pk = probe_packages();
    let flat = probe_song(&mut pk, 1.0, None, json!([]), "p");
    // 90 bpm in 7/8: a beat is 32,000 frames, and a bar three and a half beats.
    let arr: Arrangement =
        serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": 90}], "meter": [{"beat": 0, "beats": 7, "unit": 8}]})).unwrap();
    let data = with_notes(json!({"clips": [{"start": 0, "length": 16, "notes": [{"pitch": 60, "start": 0, "length": 16}]}]}), arr);
    let r = build(&flat, &data, &pk, &Owner::discard());
    let stats = probe::stats(r.plugin_handles()[0]).unwrap();
    let mut rig = Rig::new(r);
    let near = |a: f64, b: f64| (a - b).abs() < 1e-5;
    // Stopped at 0: still called (RENDER-6a), and told it doesn't play.
    rig.run(BLOCK);
    let t = *stats.told().last().unwrap();
    assert!(!t.playing && !t.looping && t.steady == 0 && t.frames == BLOCK as u32, "{t:?}");
    assert_eq!((t.tempo, t.tsig, t.bar, t.beats, t.bar_start), (90.0, (7, 8), 0, 0.0, 0.0), "{t:?}");
    // Playing, looping beats 1 to 2.01 (frames 32,000 to 64,320): the wrap falls 64 frames into the
    // block that plays from frame 64,256, which the engine clock (one block ahead) puts at 64,384.
    rig.cmd(Cmd::Loop(Some((1.0, 2.01))));
    rig.cmd(Cmd::Play);
    rig.run(64_256);
    let calls = n(&stats.process);
    rig.run(BLOCK);
    assert_eq!(n(&stats.process), calls + 2, "two calls in the block");
    let told = stats.told();
    let [a, b] = told[told.len() - 2..] else { unreachable!() };
    let loop_seconds = (60.0 / 90.0, 2.01 * 60.0 / 90.0);
    for t in [a, b] {
        assert!(t.playing && t.looping && t.frames == 64 && t.tempo == 90.0 && t.tsig == (7, 8), "{t:?}");
        assert!(near(t.loop_beats.0, 1.0) && near(t.loop_beats.1, 2.01), "{t:?}");
        assert!(near(t.loop_seconds.0, loop_seconds.0) && near(t.loop_seconds.1, loop_seconds.1), "{t:?}");
        assert!(t.bar == 0 && t.bar_start == 0.0, "{t:?}");
    }
    assert!(a.steady == 64_384 && near(a.beats, 64_256.0 / 32_000.0) && near(a.seconds, 64_256.0 / 48_000.0), "before the wrap: {a:?}");
    assert!(b.steady == 64_448 && near(b.beats, 1.0) && near(b.seconds, 60.0 / 90.0), "from the loop's start: {b:?}");
    // Beat 8, the loop off: the third bar (0 is the first), which starts at beat 7.
    rig.cmd(Cmd::Loop(None));
    rig.cmd(Cmd::Seek(8.0));
    rig.run(BLOCK);
    let t = *stats.told().last().unwrap();
    assert!(t.playing && !t.looping && t.steady == 64_512 && t.frames == BLOCK as u32, "{t:?}");
    assert!(t.bar == 2 && near(t.bar_start, 7.0) && near(t.beats, 8.0) && near(t.seconds, 8.0 * 60.0 / 90.0), "{t:?}");
    // Stopped: told so, the position held.
    rig.cmd(Cmd::Stop);
    rig.run(2 * BLOCK);
    let t = *stats.told().last().unwrap();
    assert!(!t.playing && t.steady == 64_512 + 2 * BLOCK as i64 && near(t.beats, 8.0 + BLOCK as f64 / 32_000.0), "{t:?}");
}

//! Latency compensation with a latent plugin (RENDER-17 to RENDER-19, HOST-6): the probe's
//! `latency-<n>` reports `n` frames and plays its output that late.

use super::*;

/// A sine through the keyed probe `lead` (its state says how late it plays), and the same sine
/// through an unkeyed probe at gain -1; `sum` adds them. `out` is the song's output.
fn paths(pk: &mut PackageSet, out: &str) -> Flat {
    plugged(
        pk,
        song(
            json!([
                {"id": "f", "op": "const", "value": 440.0},
                {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
                {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": "lead", "inputs": {"in": "x"}},
                {"id": "neg", "op": "const", "value": -1.0},
                {"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"in": "x", "gain_1": "neg"}},
                {"id": "sum", "op": "kernel", "kernel": "std:add", "inputs": {"a": "p", "b": "q"}},
            ]),
            out,
            json!([{"key": "notes", "kind": "events<note>"}]),
        ),
    )
}

fn silent(x: &[[f32; 2]]) -> bool {
    x.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
}

// RENDER-18, RENDER-19: the probe 100 frames late, against the same sine inverted through a probe
// with none: the planner delays the faster path, so the two cancel exactly. Offline the first `L`
// frames are dropped and `L` more rendered: the late path alone is the sine from its first sample
// to its last.
#[test]
fn a_latent_plugin_stays_aligned_offline() {
    let mut pk = probe_packages();
    let (data, dir) = with_blob(Some(probe::state("latency-100", 1.0)));
    let flat = paths(&mut pk, "sum");
    let mut r = Renderer::new(&flat, &data, &pk, SR).unwrap();
    assert_eq!((r.latency(), r.stats().delays), (100, 1));
    assert!(silent(&r.render(0.5).unwrap()), "the two paths cancel");
    let late = Renderer::new(&paths(&mut pk, "p"), &data, &pk, SR).unwrap().render(0.5).unwrap();
    let sine = Renderer::new(&paths(&mut pk, "x"), &data, &pk, SR).unwrap().render(0.5).unwrap();
    assert!(late == sine, "sample 0 is song time 0, and the last 100 frames are there");
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-18, RENDER-19 live: the engine plays the two paths cancelled, and its status says the
// position heard, `L` frames before the transport's.
#[test]
fn a_latent_plugin_stays_aligned_live() {
    let mut pk = probe_packages();
    let (data, dir) = with_blob(Some(probe::state("latency-100", 1.0)));
    let flat = paths(&mut pk, "sum");
    let mut rig = Rig::of(&flat, &data, build(&flat, &data, &pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    assert!(silent(&rig.run(48_000)), "the two paths cancel");
    let s = rig.e.status();
    let pos = rig.e.current().unwrap().transport().pos();
    assert_eq!(s.pos + 100, pos, "the position heard is L before the transport's");
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-18, HOST-6: after the probe 100 frames late, a probe on the notes lane gets its notes 100
// frames later and is told the transport 100 frames earlier, and a lane scaling it is read 100
// frames late: the song renders as it does with no latency.
#[test]
fn lanes_notes_and_time_arrive_late_by_the_offset() {
    let mut pk = probe_packages();
    let dir = notes_project();
    let (late, d) = notes_song(&mut pk, "lead", &dir);
    let (plain, d0) = notes_song(&mut pk, "other", &dir);
    let node = |f: &Flat, id: &str| f.nodes.iter().position(|n| n.name == format!("song/{id}")).unwrap();
    let render = |f: &Flat, d: &DataSet| {
        let mut r = Renderer::new(f, d, &pk, SR).unwrap();
        let q = r.info().plugin(node(f, "q")).unwrap().handle;
        (r.render(0.5).unwrap(), probe::stats(q).unwrap(), r.latency(), r.stats().delays)
    };
    let (a, sa, l, delays) = render(&late, &d);
    let (b, sb, l0, _) = render(&plain, &d0);
    assert_eq!((l, delays, l0), (100, 2, 0), "the notes and the lane are delayed");
    assert!(a == b, "the song as it renders with no latency");
    // The note at beat 0.5 (frame 12,000) reaches `q` 100 frames late on the engine clock.
    let on = |s: &Stats| s.notes().into_iter().find(|n| n.1 == Note::On).unwrap().0;
    assert_eq!((on(&sa), on(&sb)), (12_100, 12_000));
    // Told the transport 100 frames earlier: a block's call starts 100 frames back in the song.
    // CLAP's seconds are fixed point: within a thousandth of a frame.
    let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
    let t = *sa.told().last().unwrap();
    assert!(t.playing && near(t.seconds * 48_000.0, (t.steady - 100) as f64), "{t:?}");
    let t0 = *sb.told().last().unwrap();
    assert!(near(t0.seconds * 48_000.0, t0.steady as f64), "{t0:?}");
    std::fs::remove_dir_all(dir).ok();
}

/// A project with a note at beat 0.5, a level lane rising from 0 to 1 over two beats, and the keyed
/// probe `lead` 100 frames late.
fn notes_project() -> PathBuf {
    let dir = project(&[
        ("data/song/notes.json", json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0.5, "length": 1}]}]})),
        ("data/song/level.json", json!({"unit": "gain", "clips": [{"start": 0, "length": 2, "points": [[0, 0], [2, 1]]}]})),
    ]);
    std::fs::write(dir.join("data/song/lead.state"), probe::state("latency-100", 1.0)).unwrap();
    std::fs::write(dir.join("data/song/lead.state.json"), json!({"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}).to_string())
        .unwrap();
    dir
}

/// A sine through a probe keyed `key` (`lead` is 100 frames late, anything else has no state), then
/// a probe on the notes lane, scaled by the level lane; with its data from `dir`.
fn notes_song(pk: &mut PackageSet, key: &str, dir: &Path) -> (Flat, DataSet) {
    let ir = song(
        json!([
            {"id": "f", "op": "const", "value": 440.0},
            {"id": "x", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "key": key, "inputs": {"in": "x"}},
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "l", "op": "lane", "lane": "level"},
            {"id": "q", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"in": "p", "notes": "n"}},
            {"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "q", "by": "l"}},
        ]),
        "out",
        json!([{"key": "notes", "kind": "events<note>"}, {"key": "level", "kind": "control<gain>", "default": 0}]),
    );
    let flat = plugged(pk, ir.clone());
    (flat, super::super::super::data(&ir, dir))
}

// RENDER-25, RENDER-18: a plan of the same song activated while it plays keeps every delay, with
// the notes and samples in flight in it: no bit changes.
#[test]
fn a_no_op_activation_keeps_every_delay() {
    let mut pk = probe_packages();
    let dir = notes_project();
    let (flat, data) = notes_song(&mut pk, "lead", &dir);
    let play = |at: &[usize]| {
        let mut rig = Rig::of(&flat, &data, build(&flat, &data, &pk, &Owner::discard()));
        rig.cmd(Cmd::Play);
        let (mut out, mut done) = (vec![], 0);
        for &a in at {
            out.extend(rig.run(a - done));
            let r = rig.prepare_with(&flat, &data, &pk, &Owner::discard(), &Continues::identity(&flat)).unwrap();
            let s = r.stats();
            assert_eq!((s.fresh.total(), s.ghosts, s.fades, s.delays, s.kept.rings), (0, 0, 0, 2, 2), "{s:?}");
            rig.send(Msg::Activate { renderer: Box::new(r) });
            done = a;
        }
        out.extend(rig.run(48_000 - done));
        out
    };
    // Mid-note (12,000 to 36,000), a block line apart from the delay's own.
    assert!(play(&[]) == play(&[12_050, 20_000, 30_011]), "no bit changes");
    std::fs::remove_dir_all(dir).ok();
}

// RENDER-17: a state load that changes a kept plugin's latency is compensated from the next
// activation: the plan it lands in keeps the old delays and says so; the one after aligns again.
#[test]
fn a_latency_change_applies_at_the_next_activation() {
    let mut pk = probe_packages();
    let (d1, dir1) = with_blob(Some(probe::state("latency-100", 1.0)));
    let (d2, dir2) = with_blob(Some(probe::state("latency-200", 1.0)));
    let flat = paths(&mut pk, "sum");
    let p = flat.nodes.iter().position(|n| n.name == "song/p").unwrap();
    let mut rig = Rig::of(&flat, &d1, build(&flat, &d1, &pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    assert!(silent(&rig.run(4 * BLOCK)));
    // The new state lands in the kept instance: 200 frames now, compensated as 100.
    let r2 = rig.prepare_with(&flat, &d2, &pk, &Owner::discard(), &Continues::identity(&flat)).unwrap();
    assert_eq!((r2.stats().loads, r2.stats().relatent.clone(), r2.latency()), (1, vec![(p, 200)], 100));
    rig.send(Msg::Activate { renderer: Box::new(r2) });
    assert!(!silent(&rig.run(4 * BLOCK)), "misaligned until the next activation");
    // The next activation compensates 200: a new delay, faded in.
    let r3 = rig.prepare_with(&flat, &d2, &pk, &Owner::discard(), &Continues::identity(&flat)).unwrap();
    assert_eq!((r3.stats().relatent.clone(), r3.latency(), r3.stats().loads), (vec![], 200, 0));
    rig.send(Msg::Activate { renderer: Box::new(r3) });
    rig.run(16 * BLOCK);
    assert!(silent(&rig.run(8 * BLOCK)), "aligned again");
    for d in [dir1, dir2] {
        std::fs::remove_dir_all(d).ok();
    }
}

// RENDER-28: delays and note delays allocate nothing on the audio thread: playing, a seek, a loop,
// an activation that takes the latency away (the old delays play out the window as ghosts, faded
// from), a stop, a panic and play again.
#[test]
fn delays_allocate_nothing_on_the_audio_thread() {
    let mut pk = probe_packages();
    let dir = notes_project();
    let (late, data) = notes_song(&mut pk, "lead", &dir);
    let (plain, data0) = notes_song(&mut pk, "other", &dir);
    let mut rig = Rig::of(&late, &data, build(&late, &data, &pk, &Owner::discard()));
    rig.cmd(Cmd::Play);
    rig.run(16 * BLOCK);
    let next = rig.prepare(&plain, &data0, &pk);
    assert_eq!((next.latency(), next.stats().ghosts > 0, next.stats().fades > 0), (0, true, true), "{:?}", next.stats());
    for m in [
        Msg::Transport(Cmd::Seek(0.4)),
        Msg::Transport(Cmd::Loop(Some((0.0, 1.0)))),
        Msg::Activate { renderer: Box::new(next) },
        Msg::Transport(Cmd::Stop),
        Msg::Panic,
        Msg::Transport(Cmd::Play),
    ] {
        rig.send(m);
    }
    let mut out = vec![[0.0; 2]; 300];
    let mut kept: Vec<Retired> = Vec::with_capacity(64);
    let Rig { e, inbox, .. } = &mut rig;
    let ((), allocs) = during(|| {
        for _ in 0..200 {
            let mut retire = |r: Retired| {
                if kept.len() == kept.capacity() {
                    return Err(r);
                }
                kept.push(r);
                Ok(())
            };
            e.process(&mut out, &mut || inbox.pop_front(), &mut retire, CallbackStamp::default());
        }
    });
    assert_eq!(allocs, 0);
    assert!(inbox.is_empty() && kept.iter().any(|r| matches!(r, Retired::Nodes(_))), "the window ended");
    std::fs::remove_dir_all(dir).ok();
}

//! The time limit on every kernel call (ABI-6, ABI-7): a kernel that never returns is stopped,
//! live and offline, at its birth and as its module starts; one merely slow plays on bit for bit.

use std::time::{Duration, Instant};

use super::alloc_count::during;
use super::cell::step_const;
use super::live::{Rig, at_bpm, with_notes};
use super::*;
use crate::engine::{CallbackStamp, Msg, Retired};
use crate::fault::{Code, Faults, Subject};
use crate::limit::{Limits, TICK, ticks};

/// Calls of `@test/l:slow` spin this many turns of a loop before they write: tens of milliseconds,
/// so each call outlasts a few ticks.
const SPINS: u32 = 50_000_000;

/// `@test/l:wipe`'s reset spins this many turns: hundreds of milliseconds, past the live limit and
/// far inside the offline one, as clearing large buffers on a slow machine may take.
const WIPES: u32 = 600_000_000;

/// A package `@test/l` of hand-written kernels, each with one control input `in` and one output:
/// `l:spin` (outputs 1, and never returns from a call whose input is over 0.5), `l:slow` (spins
/// [`SPINS`] turns at each call, then outputs its input plus 1), `l:fast` (outputs its input plus
/// 1 at once), `l:stuck` (its `k_new` never returns), `l:wipe` (outputs 1; its reset spins
/// [`WIPES`] turns) and `l:tally` (never returns from a call whose input is over 0.5; else outputs
/// the frames it played since its last reset: 1, 2, 3 …).
fn l_kernels() -> PathBuf {
    use crate::tests::{kernel_v1, module_v1, out_v1 as out, process_v1};
    let dir = project(&[]).join("l");
    std::fs::create_dir_all(&dir).unwrap();
    let spin = kernel_v1(
        "spin",
        &format!("(if (f32.gt (local.get $x) (f32.const 0.5)) (then (loop $forever (br $forever)))) {}", out("(f32.const 1)")),
    );
    let fast = kernel_v1("fast", &out("(f32.add (local.get $x) (f32.const 1))"));
    let tally = kernel_v1(
        "tally",
        &format!(
            "(if (f32.gt (local.get $x) (f32.const 0.5)) (then (loop $forever (br $forever)))) \
             (f32.store (i32.const 4096) (f32.add (f32.load (i32.const 4096)) (f32.const 1))) {}",
            out("(f32.load (i32.const 4096))")
        ),
    );
    // `slow_process` spins, then does what `fast` does: `slow_frames`.
    let slow = format!(
        r#"(func (export "slow_new") (param i32) (result i32) (i32.const 4096))
           (func (export "slow_ports") (result i32) (i32.const 257))
           (func (export "slow_reset") (param i32))
           (func (export "slow_active") (param i32) (result i32) (i32.const 0))
           (func (export "slow_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32) (local $k i32)
             (local.set $k (i32.const {SPINS}))
             (loop $spin
               (local.set $k (i32.sub (local.get $k) (i32.const 1)))
               (br_if $spin (i32.gt_s (local.get $k) (i32.const 0))))
             (call $slow_frames (local.get $s) (local.get $io) (local.get $from) (local.get $to)))
           {}"#,
        process_v1("slow_frames", &out("(f32.add (local.get $x) (f32.const 1))"))
            .replace(r#"(export "slow_frames_process")"#, "$slow_frames")
    );
    let stuck = r#"(func (export "stuck_new") (param i32) (result i32) (loop $forever (br $forever)) (i32.const 4096))
           (func (export "stuck_ports") (result i32) (i32.const 257))
           (func (export "stuck_reset") (param i32))
           (func (export "stuck_active") (param i32) (result i32) (i32.const 0))"#
        .to_string()
        + &process_v1("stuck", &out("(f32.const 1)"));
    let wipe = format!(
        r#"(func (export "wipe_new") (param i32) (result i32) (i32.const 4096))
           (func (export "wipe_ports") (result i32) (i32.const 257))
           (func (export "wipe_reset") (param i32) (local $k i32)
             (local.set $k (i32.const {WIPES}))
             (loop $spin
               (local.set $k (i32.sub (local.get $k) (i32.const 1)))
               (br_if $spin (i32.gt_s (local.get $k) (i32.const 0)))))
           (func (export "wipe_active") (param i32) (result i32) (i32.const 0))
           {}"#,
        process_v1("wipe", &out("(f32.const 1)"))
    );
    std::fs::write(dir.join("k.wasm"), crate::tests::wasm(&module_v1(&[spin, slow, fast, stuck, wipe, tally].concat()))).unwrap();
    let kernel = |k: &str| format!("[[kernel]]\nname = \"{k}\"\ninputs = [\"in: control<*>\"]\noutputs = [\"out: audio<1>\"]\n");
    let manifest = format!(
        "[package]\nname = \"@test/l\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n{}{}{}{}{}{}",
        kernel("spin"),
        kernel("slow"),
        kernel("fast"),
        kernel("stuck"),
        kernel("wipe"),
        kernel("tally")
    );
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

/// std and `@test/l`, with `limits`.
fn l_packages(limits: Limits) -> PackageSet {
    let mut pk = PackageSet::default().with_limits(limits);
    pk.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).expect("std loads");
    pk.load(&l_kernels()).unwrap();
    pk
}

/// Limits short enough for a test to wait for, the live one as it is.
fn short() -> Limits {
    Limits { live: Limits::default().live, offline: Duration::from_millis(300) }
}

/// Two tracks: `@test/l:<kind>` on `a`, on a cue cell (site 5) starting at `cue`, and a sine
/// through a delay on `b`, into the song's sum.
fn two_tracks(kind: &str, cue: f64) -> Ir {
    let a = json!([step_const("cue", cue, 5), {"id": "k", "op": "kernel", "kernel": format!("@test/l:{kind}"), "inputs": {"in": "cue"}}]);
    let b = json!([
        {"id": "fr", "op": "const", "value": 330, "unit": "hz"},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "fr"}},
        {"id": "d", "op": "kernel", "kernel": "std:delay", "inputs": {"in": "o"}},
    ]);
    let track = |key: &str, nodes: Value, out: &str| json!({"key": key, "track": true, "nodes": nodes, "exports": {"out": out}});
    let song = json!([
        {"id": "a", "op": "import", "module": "a", "export": "out"},
        {"id": "b", "op": "import", "module": "b", "export": "out"},
        {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "a", "b": "b"}},
    ]);
    let modules = json!([track("a", a, "k"), track("b", b, "d"), {"key": "song", "nodes": song, "exports": {"out": "x"}}]);
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": modules})).unwrap()
}

/// The top-level node of `flat` named `name` (`<module>/<id>`).
fn named(flat: &Flat, name: &str) -> usize {
    flat.nodes.iter().position(|n| n.name == name).unwrap_or_else(|| panic!("no node {name}"))
}

/// A plan as the live engine gets one (realtime, so its sandboxes have the live limit), playing
/// from 0.
fn playing(flat: &Flat, data: &DataSet, pk: &PackageSet) -> Rig {
    let mut r = Renderer::prepare(flat, data, pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = Rig::of(flat, data, r);
    rig.cmd(Cmd::Play);
    rig
}

/// Write `v` into the cue cell of what `rig` plays.
fn cue(rig: &Rig, v: f32, seq: u64) {
    let r = rig.e.current().unwrap();
    r.cell_slots().set(r.cell_map().cells(&super::cell::origin(5)), v, seq);
}

/// Play `out.len()` frames of `rig`, its messages taken and nothing retired: what the device's
/// thread does, with nothing of the test's own on the heap.
fn fill(rig: &mut Rig, out: &mut [[f32; 2]]) {
    let Rig { e, inbox, .. } = rig;
    e.process(out, &mut || inbox.pop_front(), &mut |r: Retired| Err(r), CallbackStamp::default());
}

/// What `faults` was told since the last look, by node, oldest first.
fn told(faults: &Faults) -> Vec<(usize, Code)> {
    faults
        .drain()
        .into_iter()
        .map(|f| match f.subject {
            Subject::Node(n) => (n as usize, f.code),
            s => panic!("a fault about {s:?}, not a node"),
        })
        .collect()
}

// ABI-6: a call is never stopped before its limit, nor more than a tick after it: the epoch may
// move on just after the call starts, so a limit of n ticks is given n + 1.
#[test]
fn a_deadline_is_never_short_of_its_limit() {
    assert_eq!(TICK, Duration::from_millis(10));
    assert_eq!(ticks(Duration::from_millis(100)), 11);
    assert_eq!(ticks(Duration::from_millis(95)), 11);
    assert_eq!(ticks(Duration::from_secs(5)), 501);
    assert_eq!(Limits::default(), Limits { live: Duration::from_millis(100), offline: Duration::from_secs(5) });
}

/// A package named `name` (std, or a pack) whose one kernel `k` (one control input, one output)
/// spins `turns` turns of a loop at each call, then outputs 1.
fn spinning(name: &str, turns: u32) -> PathBuf {
    use crate::tests::{module_v1, out_v1 as out, process_v1};
    let dir = project(&[]).join("spinning");
    std::fs::create_dir_all(&dir).unwrap();
    let k = format!(
        r#"(func (export "k_new") (param i32) (result i32) (i32.const 4096))
           (func (export "k_ports") (result i32) (i32.const 257))
           (func (export "k_reset") (param i32))
           (func (export "k_active") (param i32) (result i32) (i32.const 0))
           (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32) (local $k i32)
             (local.set $k (i32.const {turns}))
             (loop $spin
               (local.set $k (i32.sub (local.get $k) (i32.const 1)))
               (br_if $spin (i32.gt_s (local.get $k) (i32.const 0))))
             (call $frames (local.get $s) (local.get $io) (local.get $from) (local.get $to)))
           {}"#,
        process_v1("frames", &out("(f32.const 1)")).replace(r#"(export "frames_process")"#, "$frames")
    );
    std::fs::write(dir.join("k.wasm"), crate::tests::wasm(&module_v1(&k))).unwrap();
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n[[kernel]]\nname = \"k\"\ninputs = [\"in: control<*>\"]\noutputs = [\"out: audio<1>\"]\n"
    );
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    dir
}

// ABI-6 (the owner's decision on the review's T1): the limit is a pack's own kernels' only. std's
// kernels are music2's own code and run with no time checks, as before ABI-6: the same module, every
// call of it far past the limit, renders as std and is stopped as a pack. The package decides, by
// its name alone (`packages::checked`), which engine compiles its module.
#[test]
fn only_a_packs_own_kernels_are_checked() {
    assert!(!crate::packages::checked("std") && crate::packages::checked("@test/l"));
    let limits = Limits { live: Duration::from_millis(50), offline: Duration::from_millis(50) };
    for (name, checked) in [("std", false), ("@test/s", true)] {
        let mut pk = PackageSet::default().with_limits(limits);
        pk.load(&spinning(name, 300_000_000)).unwrap();
        let nodes = json!([{"id": "c", "op": "const", "value": 0}, {"id": "k", "op": "kernel", "kernel": format!("{name}:k"), "inputs": {"in": "c"}}]);
        let ir = song(nodes, "k", json!([]));
        let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
        let t = Instant::now();
        let made = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(BLOCK as f64 / SR as f64);
        match checked {
            false => {
                let out = made.unwrap_or_else(|e| panic!("std is never stopped: {e:#}"));
                assert!(t.elapsed() > Duration::from_millis(50), "the call ran past the limit: {:?}", t.elapsed());
                assert!(out.iter().all(|f| f[0] == 1.0));
            }
            true => {
                let e = made.unwrap_err();
                let failed = e.downcast_ref::<crate::fault::Failed>().expect("a fault failed it");
                assert_eq!(failed.fault.code, Code::TooSlow, "a pack's is stopped");
            }
        }
    }
}

// ABI-6, ABI-7, RENDER-28: live, a kernel whose call never returns is stopped at its limit. The
// engine plays on, the other track bit for bit, the kernel's node is silent from that block on,
// and its fault, `sound.too-slow`, is reported once through the plan's ring. The audio thread
// allocates nothing before it, and nothing after (the trap's own error is RENDER-28's one
// exception). On engine threads too.
#[test]
fn a_kernel_that_never_returns_live_is_stopped_alone() {
    let pk = l_packages(Limits::default());
    let f = check(&two_tracks("spin", 0.0), &pk).unwrap();
    let (k, d) = (named(&f, "a/k"), named(&f, "b/d"));
    // Plays 2 blocks, the second cued to spin when `stuck`, then 6 more; the song, `b/d`'s last
    // block, `a/k`'s, what was told, how long the stuck block took, and the heap operations of
    // the blocks around it.
    let run = |stuck: bool, threads: Option<Spread>| {
        let mut rig = playing(&f, &DataSet::empty(), &pk);
        rig.threads = threads;
        let mut out = vec![[0.0f32; 2]; 8 * BLOCK];
        let (first, rest) = out.split_at_mut(BLOCK);
        let (second, rest) = rest.split_at_mut(BLOCK);
        let threaded = rig.threads.is_some();
        let play = |rig: &mut Rig, out: &mut [[f32; 2]]| match threaded {
            true => {
                let Rig { e, inbox, threads, .. } = rig;
                let runner = &mut threads.as_mut().unwrap().runner;
                e.process_on(out, &mut || inbox.pop_front(), &mut |r: Retired| Err(r), CallbackStamp::default(), runner);
            }
            false => fill(rig, out),
        };
        // The first block warms the thread; the second counts.
        play(&mut rig, &mut first[..BLOCK / 2]);
        let ((), before) = during(|| play(&mut rig, &mut first[BLOCK / 2..]));
        cue(&rig, if stuck { 1.0 } else { 0.0 }, 1);
        let t = Instant::now();
        play(&mut rig, second);
        let took = t.elapsed();
        cue(&rig, 0.0, 2);
        let ((), after) = during(|| play(&mut rig, rest));
        let cur = rig.e.current().unwrap();
        assert!(cur.transport().playing() && rig.e.crashes() == 0, "the song plays on");
        let faults = cur.info().faults.clone();
        (out, cur.output_of(d).to_vec(), cur.output_of(k).to_vec(), told(&faults), took, (before, after))
    };
    let (clean, clean_d, clean_k, clean_told, _, _) = run(false, None);
    assert!(clean_k.iter().all(|&x| x == 1.0) && clean_told.is_empty(), "uncued, it plays");
    for threads in [None, Some(spread(3))] {
        let on = threads.as_ref().map_or(1, |_| 3);
        let (out, b, a, said, took, allocs) = run(true, threads);
        assert!(took >= Duration::from_millis(100) && took < Duration::from_secs(3), "stopped at its limit, on {on}: {took:?}");
        assert_eq!(said, [(k, Code::TooSlow)], "told once, on {on}");
        assert!(a.iter().all(|&x| x == 0.0), "it is silent, on {on}");
        assert_eq!(b, clean_d, "the other track plays on bit for bit, on {on}");
        let last: Vec<f32> = out[out.len() - BLOCK..].iter().map(|f| f[0]).collect();
        assert_eq!(last, b, "what plays is the other track, exactly, on {on}");
        assert!(out[..BLOCK] == clean[..BLOCK], "the same before, on {on}");
        if on == 1 {
            assert_eq!(allocs, (0, 0), "the audio thread allocates nothing before it, or after");
        }
    }
}

// ABI-6, RENDER-28 (review T4): each sound stopped holds its thread at least the live limit, so ten
// stuck in one callback on one thread hold it a second or more: past the second after which the
// server lets a player that doesn't call back go. The engine itself plays on: each is told once,
// silent from then on, and what else plays is as it was; the next callback is a plain one.
#[test]
fn ten_sounds_stuck_in_one_callback_hold_it_a_second() {
    let pk = l_packages(Limits::default());
    let mut nodes = vec![
        json!({"id": "c", "op": "const", "value": 1}),
        json!({"id": "fr", "op": "const", "value": 330, "unit": "hz"}),
        json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "fr"}}),
    ];
    let mut sum = "o".to_string();
    for i in 0..10 {
        nodes.push(json!({"id": format!("s{i}"), "op": "kernel", "kernel": "@test/l:spin", "inputs": {"in": "c"}}));
        nodes.push(json!({"id": format!("x{i}"), "op": "kernel", "kernel": "std:add", "inputs": {"a": sum, "b": format!("s{i}")}}));
        sum = format!("x{i}");
    }
    let f = check(&song(Value::Array(nodes), &sum, json!([])), &pk).unwrap();
    let mut rig = playing(&f, &DataSet::empty(), &pk);
    let mut out = vec![[0.0f32; 2]; BLOCK];
    let t = Instant::now();
    fill(&mut rig, &mut out);
    let took = t.elapsed();
    assert!(took >= 10 * Limits::default().live, "ten stops on one thread: {took:?}");
    let cur = rig.e.current().unwrap();
    let mut said = told(&cur.info().faults);
    said.sort_by_key(|&(n, _)| n);
    let spins: Vec<(usize, Code)> = (0..10).map(|i| (named(&f, &format!("song/s{i}")), Code::TooSlow)).collect();
    assert_eq!(said, spins, "each told once");
    assert!(cur.transport().playing() && rig.e.crashes() == 0, "the engine plays on");
    let sine = cur.output_of(named(&f, "song/o")).to_vec();
    let mixed: Vec<f32> = out.iter().map(|f| f[0]).collect();
    assert_eq!(mixed, sine, "what plays is what else plays, exactly");
    let t = Instant::now();
    fill(&mut rig, &mut out);
    assert!(t.elapsed() < Limits::default().live, "the next callback is a plain one: {:?}", t.elapsed());
    assert!(told(&rig.e.current().unwrap().info().faults).is_empty());
}

// ABI-7, RENDER-10: Stop All Sound never calls a kernel that ran out of time: it stays silent,
// is told no more, and the panic allocates nothing. The next plan resets it as it moves it, so
// it plays again (RENDER-25), and there it is no longer stuck: Stop All Sound resets it again
// (review T6). An instrument whose voice got stuck stops whole, alone, the same.
#[test]
fn stop_all_sound_never_calls_a_kernel_that_ran_out_of_time() {
    let pk = l_packages(Limits::default());
    let f = check(&two_tracks("tally", 1.0), &pk).unwrap();
    let k = named(&f, "a/k");
    let mut rig = playing(&f, &DataSet::empty(), &pk);
    let mut out = vec![[0.0f32; 2]; BLOCK];
    fill(&mut rig, &mut out);
    let faults = rig.e.current().unwrap().info().faults.clone();
    assert_eq!(told(&faults), [(k, Code::TooSlow)]);
    // Uncued, a reset and a call would play 1, 2, 3 ….
    cue(&rig, 0.0, 1);
    rig.send(Msg::Panic);
    let t = Instant::now();
    let ((), allocs) = during(|| fill(&mut rig, &mut out));
    fill(&mut rig, &mut out);
    assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
    let cur = rig.e.current().unwrap();
    assert_eq!(rig.e.resets(), 1, "the panic came");
    assert!(cur.output_of(k).iter().all(|&x| x == 0.0), "still silent: not called again");
    assert_eq!((told(&faults), allocs), (vec![], 0));
    // The next plan, of the song as it is, resets it as it moves it: it plays again.
    let g = check(&two_tracks("tally", 0.0), &pk).unwrap();
    let r = rig.prepare(&g, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    let gk = named(&g, "a/k");
    let tally = |rig: &Rig| rig.e.current().unwrap().output_of(gk).iter().fold(0.0f32, |m, &x| m.max(x));
    for _ in 0..4 {
        fill(&mut rig, &mut out);
    }
    assert!(tally(&rig) > 3.0 * BLOCK as f32, "it plays again in the next plan: {}", tally(&rig));
    assert!(told(&rig.e.current().unwrap().info().faults).is_empty());
    // There it is no longer stuck: Stop All Sound resets it, so it counts from 1 again.
    rig.send(Msg::Panic);
    fill(&mut rig, &mut out);
    assert!(tally(&rig) <= BLOCK as f32, "reset by the panic: {}", tally(&rig));

    // In a voice: the instrument stops whole at its first note, alone, and a panic leaves it so.
    let body = json!([
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "s", "op": "kernel", "kernel": "@test/l:spin", "inputs": {"in": "g"}},
    ]);
    let ir = song(
        json!([
            {"id": "n", "op": "lane", "lane": "notes"},
            {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "s", "nodes": body}},
            {"id": "fr", "op": "const", "value": 330, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "fr"}},
            {"id": "x", "op": "kernel", "kernel": "std:add", "inputs": {"a": "v", "b": "o"}},
        ]),
        "x",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let data = with_notes(json!({"clips": [{"start": 0, "length": 4, "notes": [{"pitch": 60, "start": 0, "length": 1}]}]}), at_bpm(120.0));
    let f = check(&ir, &pk).unwrap();
    let (v, o) = (named(&f, "song/v"), named(&f, "song/o"));
    let mut rig = playing(&f, &data, &pk);
    fill(&mut rig, &mut out);
    let faults = rig.e.current().unwrap().info().faults.clone();
    assert_eq!(told(&faults), [(v, Code::TooSlow)], "the instrument stopped");
    rig.send(Msg::Panic);
    let ((), allocs) = during(|| fill(&mut rig, &mut out));
    let cur = rig.e.current().unwrap();
    assert!(cur.output_of(v).iter().all(|&x| x == 0.0) && allocs == 0, "{allocs} heap operations");
    assert!(cur.output_of(o).iter().any(|&x| x != 0.0) && told(&faults).is_empty(), "the sine plays on");
}

// ABI-6, ABI-7, RENDER-1: offline, a call past its limit fails the render within it, naming the
// node and the code: never a file with the sound silent.
#[test]
fn an_offline_render_past_its_limit_fails_in_time() {
    let pk = l_packages(short());
    let f = check(&two_tracks("spin", 1.0), &pk).unwrap();
    let k = named(&f, "a/k");
    let t = Instant::now();
    let e = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap().render(1.0).unwrap_err();
    assert!(t.elapsed() >= Duration::from_millis(300) && t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    let failed = e.downcast_ref::<crate::fault::Failed>().expect("a fault failed it");
    assert_eq!((failed.fault.code, failed.fault.subject), (Code::TooSlow, Subject::Node(k as u32)));
    assert_eq!(failed.fault.code.diag(), "sound.too-slow");
    assert_eq!(format!("{e}"), "a/k took too long and was stopped: one call into it ran past 300 ms");
}

// ABI-6, ABI-7: a kernel whose `k_new` never returns is refused as the plan is made, live (its
// birth has the offline limit) and offline, naming its node, within the limit.
#[test]
fn a_kernel_stuck_at_its_birth_is_refused() {
    let owner = m2_plugins::Owner::discard();
    for live in [true, false] {
        let pk = l_packages(short());
        let f = check(&two_tracks("stuck", 0.0), &pk).unwrap();
        let t = Instant::now();
        let made = match live {
            true => Renderer::prepare(&f, &DataSet::empty(), &pk, SR, &mut audio::Cache::default(), &owner, Basis::Fresh),
            false => Renderer::new(&f, &DataSet::empty(), &pk, SR),
        };
        let e = made.map(drop).unwrap_err();
        assert!(t.elapsed() >= Duration::from_millis(300) && t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
        let slow = e.downcast_ref::<crate::TooSlow>().unwrap_or_else(|| panic!("{e:#}"));
        assert_eq!((slow.node.as_str(), slow.limit), ("a/k", Duration::from_millis(300)), "live: {live}");
    }
}

// ABI-7 (review T3): a box whose birth ran past its limit is remembered by its shape (its kernel,
// its module's hash and its settings, RENDER-25): a kernel is deterministic, so it would get stuck
// again. The next plan that would make it again, live or offline, is refused at once in the same
// words, without calling it; a plan without it builds. An instrument whose voice got stuck being
// born is remembered by the instrument's shape.
#[test]
fn a_box_stuck_at_its_birth_is_refused_at_once_after() {
    let pk = l_packages(short());
    let owner = m2_plugins::Owner::discard();
    let body =
        json!([{"id": "g", "op": "lane", "lane": "gate"}, {"id": "s", "op": "kernel", "kernel": "@test/l:stuck", "inputs": {"in": "g"}}]);
    let poly = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "v", "op": "poly", "events": "n", "voices": 2, "body": {"out": "s", "nodes": body}}]),
        "v",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    for (ir, node) in [(two_tracks("stuck", 0.0), "a/k"), (poly, "song/s")] {
        let f = check(&ir, &pk).unwrap();
        let build = |live: bool| {
            let t = Instant::now();
            let made = match live {
                true => Renderer::prepare(&f, &DataSet::empty(), &pk, SR, &mut audio::Cache::default(), &owner, Basis::Fresh),
                false => Renderer::new(&f, &DataSet::empty(), &pk, SR),
            };
            let e = made.map(drop).unwrap_err();
            let slow = e.downcast_ref::<crate::TooSlow>().unwrap_or_else(|| panic!("{e:#}"));
            (slow.node.clone(), slow.limit, t.elapsed())
        };
        let (first, limit, took) = build(true);
        assert!(took >= Duration::from_millis(300), "it waited the limit: {took:?}");
        assert_eq!((first.as_str(), limit), (node, Duration::from_millis(300)));
        for live in [true, false, true] {
            let (again, limit, took) = build(live);
            assert!(took < Duration::from_millis(250), "refused at once, live {live}: {took:?}");
            assert_eq!((again.as_str(), limit), (node, Duration::from_millis(300)), "in the same words");
        }
    }
    let f = check(&two_tracks("fast", 0.0), &pk).unwrap();
    assert!(Renderer::new(&f, &DataSet::empty(), &pk, SR).is_ok(), "a song without it builds");
}

// ABI-6 (review T2): a sandbox's prewarm, its first reset at the build, has the birth's limit, live
// too: a kernel whose first reset takes hundreds of milliseconds, past the live limit, builds into a
// live plan and plays, as it does into an export.
#[test]
fn a_slow_first_reset_builds_live_too() {
    let pk = l_packages(Limits::default());
    let f = check(&two_tracks("wipe", 0.0), &pk).unwrap();
    let t = Instant::now();
    let mut rig = playing(&f, &DataSet::empty(), &pk);
    assert!(t.elapsed() > Limits::default().live, "the reset ran past the live limit: {:?}", t.elapsed());
    let mut out = vec![[0.0f32; 2]; BLOCK];
    fill(&mut rig, &mut out);
    let cur = rig.e.current().unwrap();
    assert!(cur.output_of(named(&f, "a/k")).iter().all(|&x| x == 1.0), "it plays");
    assert!(told(&cur.info().faults).is_empty());
}

// ABI-6: a module whose start function never ends is refused as its package loads, in words.
#[test]
fn a_module_that_never_starts_is_refused() {
    let wat = module_v1(&format!("(func $forever (loop $l (br $l))) (start $forever) {}", crate::tests::kernel_v1("k", "")));
    let dir = package(&wat);
    let t = Instant::now();
    let e = PackageSet::default().with_limits(short()).load(&dir).map(drop).unwrap_err();
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    assert!(format!("{e:#}").contains("its module took too long to start and was stopped: its start ran past 300 ms"), "{e:#}");
    std::fs::remove_dir_all(dir).ok();
}

// ABI-6, RENDER-33: a kernel slow across many ticks, but within its limit, is never stopped, and
// the checks change no bit: it plays live what it renders offline, which is what its fast twin
// renders.
#[test]
fn a_slow_kernel_within_its_limit_changes_no_bit() {
    let pk = l_packages(Limits { live: Duration::from_secs(10), offline: Duration::from_secs(10) });
    let blocks = 6;
    let offline = |kind: &str| {
        let f = check(&two_tracks(kind, 0.25), &pk).unwrap();
        let mut r = Renderer::new(&f, &DataSet::empty(), &pk, SR).unwrap();
        let mut out = vec![[0.0f32; 2]; blocks * BLOCK];
        r.process(&mut out).unwrap();
        out
    };
    let t = Instant::now();
    let slow = offline("slow");
    assert!(t.elapsed() > blocks as u32 * TICK, "each call outlasts a tick: {:?}", t.elapsed());
    assert!(slow == offline("fast"), "the slow kernel renders its fast twin's bits");
    let f = check(&two_tracks("slow", 0.25), &pk).unwrap();
    let mut rig = playing(&f, &DataSet::empty(), &pk);
    let mut live = vec![[0.0f32; 2]; blocks * BLOCK];
    fill(&mut rig, &mut live);
    assert!(live == slow, "live, the same bits");
    assert!(told(&rig.e.current().unwrap().info().faults).is_empty(), "never stopped");
    assert!(slow.iter().all(|f| f[0].is_finite()) && peak(&slow) > 1.0);
}

// ABI-6: the time the host spends reading a file for a pack's offline kernel isn't the kernel's:
// one whose first read waits twice its limit (a drive waking up) still makes its render, the file's
// own samples, as with a quick disk. (std's `std:stretch` has no limit: only a pack's kernels do.)
#[test]
fn a_slow_file_read_is_not_the_kernels_time() {
    // `@test/t:k`, offline: at each call it reads the next frames of its asset 0 through
    // `m2.asset_read` into its output, then goes over them in a loop, whose every turn checks the
    // deadline. Its state at 4096 holds the frame it is at.
    let k = r#"
  (func (export "k_new") (param i32) (result i32) (i32.store (i32.const 4096) (i32.const 0)) (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 1))
  (func (export "k_reset") (param i32) (i32.store (i32.const 4096) (i32.const 0)))
  (func (export "k_active") (param i32) (result i32) (i32.const 0))
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32) (local $at i32)
    (drop (call $read (i32.const 0) (i32.const 0) (i32.load (i32.const 4096))
      (i32.add (i32.load (local.get $io)) (i32.shl (local.get $from) (i32.const 2))) (i32.sub (local.get $to) (local.get $from))))
    (i32.store (i32.const 4096) (i32.add (i32.load (i32.const 4096)) (i32.sub (local.get $to) (local.get $from))))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $from) (local.get $to)))
      (local.set $at (i32.add (i32.load (local.get $io)) (i32.shl (local.get $from) (i32.const 2))))
      (f32.store (local.get $at) (f32.mul (f32.load (local.get $at)) (f32.const 1)))
      (local.set $from (i32.add (local.get $from) (i32.const 1)))
      (br $l))))"#;
    let import = r#"(module (import "m2" "asset_read" (func $read (param i32 i32 i32 i32 i32) (result i32)))"#;
    let pkg = crate::tests::package_with(
        &crate::tests::module_v1(k).replacen("(module", import, 1),
        "outputs = [\"out: audio<1>\"]\noffline = true",
    );
    let mut pk = PackageSet::default().with_limits(short());
    pk.load(&pkg).unwrap();
    let k = offline::Kernel::of(&pk, "@test/t:k").expect("an offline kernel");
    let dir = project(&[]);
    let tone = sine(440.0, 0.25, SR);
    audio::write_wav(&dir.join("a.wav"), std::slice::from_ref(&tone), SR).unwrap();
    let wav = Arc::new(crate::stream::Wav::open(&dir.join("a.wav")).unwrap());
    let play = |slow: Duration| {
        let run = offline::Run {
            source: sandbox::Held::Disk(wav.clone()),
            values: &[],
            map: &[],
            rate: SR as f64,
            channels: 1,
            frames: tone.len(),
        };
        let mut made = vec![];
        let t = Instant::now();
        let mut sink = |b: &[&[f32]]| {
            made.extend_from_slice(b[0]);
            Ok(())
        };
        let done = sandbox::slow_disk(slow, || offline::run(&k, &run, &mut sink));
        (done.map(|()| made), t.elapsed())
    };
    let (quick, _) = play(Duration::ZERO);
    assert!(quick.unwrap() == tone, "it plays the file");
    let (slow, took) = play(Duration::from_millis(600));
    assert!(took > Duration::from_millis(600), "it read slowly: {took:?}");
    let slow = slow.unwrap_or_else(|e| panic!("a slow disk stopped it: {e:#}"));
    assert!(slow == tone, "the same render");
    for d in [dir, pkg] {
        std::fs::remove_dir_all(d).ok();
    }
}

// ABI-6: what the time checks cost a kernel's call. Each std kernel's `k_process` (a block, stereo,
// its inputs at their defaults) is timed as std's, which has none, and as a pack's: std's own
// module and kernels under a pack's name, compiled with them. The best of 7 rounds of 3000 calls,
// in ns. By hand: `cargo test -p m2-runtime --lib kernel_call_cost -- --ignored --nocapture`.
#[test]
#[ignore]
fn kernel_call_cost() {
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std");
    let pack = project(&[]).join("pack-std");
    std::fs::create_dir_all(pack.join("kernels")).unwrap();
    std::fs::copy(std.join("kernels/std.wasm"), pack.join("kernels/std.wasm")).unwrap();
    let manifest = std::fs::read_to_string(std.join("m2pkg.toml")).unwrap().replacen("name = \"std\"", "name = \"@test/std\"", 1);
    std::fs::write(pack.join("m2pkg.toml"), manifest).unwrap();
    let mut pk = std_packages();
    pk.load(&pack).unwrap();
    let mut ids: Vec<String> = pk.kernel_sigs().map(|s| s.id.clone()).filter(|id| id.starts_with("std:")).collect();
    ids.sort();
    let time = |id: &str| {
        let mut k = crate::tests::std_kernel(&pk, id, SR as f64);
        let KernelInst { own, state, .. } = &mut k;
        let sb = own.as_mut().unwrap();
        for _ in 0..500 {
            state.process(sb, BLOCK).unwrap();
        }
        (0..7)
            .map(|_| {
                let t = Instant::now();
                for _ in 0..3000 {
                    state.process(sb, BLOCK).unwrap();
                }
                t.elapsed().as_nanos() as f64 / 3000.0
            })
            .fold(f64::MAX, f64::min)
    };
    let (mut plain, mut checked) = (0.0, 0.0);
    for id in ids {
        let (p, c) = (time(&id), time(&id.replacen("std:", "@test/std:", 1)));
        (plain, checked) = (plain + p, checked + c);
        println!("BENCH {id} {p:.0} {c:.0}");
    }
    println!("BENCH total {plain:.0} {checked:.0} ({:+.1}%)", 100.0 * (checked / plain - 1.0));
}

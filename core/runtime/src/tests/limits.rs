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

/// A package `@test/l` of hand-written kernels, each with one control input `in` and one output:
/// `l:spin` (outputs 1, and never returns from a call whose input is over 0.5), `l:slow` (spins
/// [`SPINS`] turns at each call, then outputs its input plus 1), `l:fast` (outputs its input plus
/// 1 at once) and `l:stuck` (its `k_new` never returns).
fn l_kernels() -> PathBuf {
    use crate::tests::{kernel_v1, module_v1, out_v1 as out, process_v1};
    let dir = project(&[]).join("l");
    std::fs::create_dir_all(&dir).unwrap();
    let spin = kernel_v1(
        "spin",
        &format!("(if (f32.gt (local.get $x) (f32.const 0.5)) (then (loop $forever (br $forever)))) {}", out("(f32.const 1)")),
    );
    let fast = kernel_v1("fast", &out("(f32.add (local.get $x) (f32.const 1))"));
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
    std::fs::write(dir.join("k.wasm"), crate::tests::wasm(&module_v1(&[spin, slow, fast, stuck].concat()))).unwrap();
    let kernel = |k: &str| format!("[[kernel]]\nname = \"{k}\"\ninputs = [\"in: control<*>\"]\noutputs = [\"out: audio<1>\"]\n");
    let manifest = format!(
        "[package]\nname = \"@test/l\"\nversion = \"0.1.0\"\nwasm = \"k.wasm\"\n\n{}{}{}{}",
        kernel("spin"),
        kernel("slow"),
        kernel("fast"),
        kernel("stuck")
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

// ABI-7, RENDER-10: Stop All Sound never calls a kernel that ran out of time: it stays silent,
// is told no more, and the panic allocates nothing. The next plan resets it as it moves it, so
// it plays again (RENDER-25). An instrument whose voice got stuck stops whole, alone, the same.
#[test]
fn stop_all_sound_never_calls_a_kernel_that_ran_out_of_time() {
    let pk = l_packages(Limits::default());
    let f = check(&two_tracks("spin", 1.0), &pk).unwrap();
    let k = named(&f, "a/k");
    let mut rig = playing(&f, &DataSet::empty(), &pk);
    let mut out = vec![[0.0f32; 2]; BLOCK];
    fill(&mut rig, &mut out);
    let faults = rig.e.current().unwrap().info().faults.clone();
    assert_eq!(told(&faults), [(k, Code::TooSlow)]);
    // Uncued, a reset and a call would play 1.
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
    let g = check(&two_tracks("spin", 0.0), &pk).unwrap();
    let r = rig.prepare(&g, &DataSet::empty(), &pk);
    rig.send(Msg::Activate { renderer: Box::new(r) });
    fill(&mut rig, &mut out);
    fill(&mut rig, &mut out);
    let cur = rig.e.current().unwrap();
    assert!(cur.output_of(named(&g, "a/k")).iter().all(|&x| x == 1.0), "it plays again in the next plan");
    assert!(told(&cur.info().faults).is_empty());

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
    assert_eq!(format!("{e}"), "a/k took too long and was stopped: one call into it ran past 300 ms (ABI-6)");
}

// ABI-6, ABI-7: a kernel whose `k_new` never returns is refused as the plan is made, live (its
// birth has the offline limit) and offline, naming its node, within the limit.
#[test]
fn a_kernel_stuck_at_its_birth_is_refused() {
    let pk = l_packages(short());
    let f = check(&two_tracks("stuck", 0.0), &pk).unwrap();
    let owner = m2_plugins::Owner::discard();
    for live in [true, false] {
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

// ABI-6: a module whose start function never ends is refused as its package loads, in words.
#[test]
fn a_module_that_never_starts_is_refused() {
    let wat = module_v1(&format!("(func $forever (loop $l (br $l))) (start $forever) {}", crate::tests::kernel_v1("k", "")));
    let dir = package(&wat);
    let t = Instant::now();
    let e = PackageSet::default().with_limits(short()).load(&dir).map(drop).unwrap_err();
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    assert!(format!("{e:#}").contains("its module took too long to start and was stopped (ABI-6)"), "{e:#}");
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

// ABI-6: the time the host spends reading a file for an offline kernel isn't the kernel's: a
// stretch whose first read waits twice its limit (a drive waking up) still makes its render, the
// same bits as with a quick disk.
#[test]
fn a_slow_file_read_is_not_the_kernels_time() {
    let pk = l_packages(short());
    let k = offline::Kernel::of(&pk, offline::STRETCH).unwrap();
    let dir = project(&[]);
    audio::write_wav(&dir.join("a.wav"), &[sine(440.0, 0.25, SR)], SR).unwrap();
    let wav = Arc::new(crate::stream::Wav::open(&dir.join("a.wav")).unwrap());
    let stretch = |slow: Duration| {
        let run = offline::Run {
            source: sandbox::Held::Disk(wav.clone()),
            values: &[0.0],
            map: &[[0.0, 0.0], [0.25, 0.5]],
            rate: SR as f64,
            channels: 1,
            frames: SR as usize / 2,
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
    let (quick, _) = stretch(Duration::ZERO);
    let (slow, took) = stretch(Duration::from_millis(600));
    assert!(took > Duration::from_millis(600), "it read slowly: {took:?}");
    let slow = slow.unwrap_or_else(|e| panic!("a slow disk stopped it: {e:#}"));
    assert!(slow == quick.unwrap(), "the same render");
    std::fs::remove_dir_all(dir).ok();
}

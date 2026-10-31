//! Event ports for kernels (ABI-8): a kernel with an `events<note>` input gets its lane's events
//! each block, in kernel ABI v1's reserved shape, read-only, with room for all its lane can bring;
//! and its lane is part of its shape.

use m2_song::{LaneData, LaneKind};

use super::goldens::addr;
use super::*;
use crate::sched::LiveEv;

/// A kernel `@test/t:k` with one input, `notes: events<note>`, and one mono output, which writes
/// what it read: for each of its first 20 events, six numbers (kind, frame, id, and its three
/// values: a NaN as -1, a MIDI message's bytes and a note expression's kind as the whole number they
/// make), then at 123 and 124 how many Ons and Offs it holds in all, at 125 where its events start
/// from the header, at 126 its `cap` and at 127 its `count`. `extra` runs after.
fn probe(extra: &str) -> PathBuf {
    let k = format!(
        r#"
  (func (export "k_new") (param i32) (result i32) (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 257))
  (func (export "k_reset") (param i32))
  (func (export "k_active") (param i32) (result i32) (i32.const 0))
  (func $nan (param $v f32) (result f32) (select (f32.const -1) (local.get $v) (f32.ne (local.get $v) (local.get $v))))
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $h i32) (local $out i32) (local $n i32) (local $i i32) (local $r i32) (local $o i32) (local $kind i32)
    (local $ons i32) (local $offs i32)
    (local.set $h (i32.load (local.get $io)))
    (local.set $out (i32.load offset=4 (local.get $io)))
    (memory.fill (local.get $out) (i32.const 0) (i32.const 512))
    (f32.store offset=508 (local.get $out) (f32.convert_i32_u (i32.load (local.get $h))))
    (f32.store offset=504 (local.get $out) (f32.convert_i32_u (i32.load offset=4 (local.get $h))))
    (f32.store offset=500 (local.get $out) (f32.convert_i32_s (i32.sub (i32.load offset=8 (local.get $h)) (local.get $h))))
    (local.set $n (i32.load (local.get $h)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $r (i32.add (i32.load offset=8 (local.get $h)) (i32.mul (local.get $i) (i32.const 24))))
      (local.set $kind (i32.load (local.get $r)))
      (local.set $ons (i32.add (local.get $ons) (i32.eqz (local.get $kind))))
      (local.set $offs (i32.add (local.get $offs) (i32.eq (local.get $kind) (i32.const 1))))
      (if (i32.lt_u (local.get $i) (i32.const 20)) (then
        (local.set $o (i32.add (local.get $out) (i32.mul (local.get $i) (i32.const 24))))
        (f32.store (local.get $o) (f32.convert_i32_u (local.get $kind)))
        (f32.store offset=4 (local.get $o) (f32.convert_i32_u (i32.load offset=4 (local.get $r))))
        (f32.store offset=8 (local.get $o) (f32.convert_i32_u (i32.load offset=8 (local.get $r))))
        (f32.store offset=12 (local.get $o)
          (select (f32.convert_i32_u (i32.load offset=12 (local.get $r))) (call $nan (f32.load offset=12 (local.get $r)))
            (i32.or (i32.eq (local.get $kind) (i32.const 4)) (i32.eq (local.get $kind) (i32.const 2)))))
        (f32.store offset=16 (local.get $o) (call $nan (f32.load offset=16 (local.get $r))))
        (f32.store offset=20 (local.get $o) (call $nan (f32.load offset=20 (local.get $r))))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (f32.store offset=492 (local.get $out) (f32.convert_i32_u (local.get $ons)))
    (f32.store offset=496 (local.get $out) (f32.convert_i32_u (local.get $offs)))
    {extra})"#
    );
    package_with(&module_v1(&k), "inputs = [\"notes: events<note>\"]\noutputs = [\"out: audio<1>\"]")
}

/// A song whose output is the probe reading its notes lane.
fn probed() -> Ir {
    song(
        json!([{"id": "n", "op": "lane", "lane": "notes"}, {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": "n"}}]),
        "k",
        json!([{"key": "notes", "kind": "events<note>"}]),
    )
}

/// The lane's notes, at 120 bpm: a beat is 24 000 frames.
fn lane(v: Value) -> DataSet {
    let notes = LaneData::read(&v.to_string(), &LaneKind::Notes).unwrap();
    DataSet::with_lanes(live::at_bpm(120.0), [(addr("notes"), notes)])
}

/// What the probe read in one block: its count, cap, where its events start, how many Ons and Offs,
/// and its first 20 events as six numbers each.
#[derive(Debug, PartialEq)]
struct Read {
    count: f32,
    cap: f32,
    at: f32,
    ons: f32,
    offs: f32,
    events: Vec<[f32; 6]>,
}

fn read(block: &[[f32; 2]]) -> Read {
    let x: Vec<f32> = block.iter().map(|f| f[0]).collect();
    let n = (x[127] as usize).min(20);
    Read {
        count: x[127],
        cap: x[126],
        at: x[125],
        ons: x[123],
        offs: x[124],
        events: (0..n).map(|i| std::array::from_fn(|k| x[6 * i + k])).collect(),
    }
}

fn load(pkg: &Path) -> PackageSet {
    let mut pk = PackageSet::default();
    pk.load(pkg).unwrap();
    pk
}

// ABI-8: each block the kernel gets its lane's events in the reserved shape: the header's count,
// its cap (its lane's room: 812 at its smallest) and where its events start (just after it, at 16), then 24 bytes an event: the
// kind, the frame in the block (never counted across blocks), the note's id, and its values (an
// On's pitch, velocity and no `from_pitch`; an Off's release velocity). In RENDER-14's order: at one
// frame the Off that ends a note before the Ons that start there.
#[test]
fn a_kernel_reads_its_lanes_events() {
    let pkg = probe("");
    let pk = load(&pkg);
    let ir = probed();
    // A at 0 to 60; B and C at 60, C to 84, B to 180 (the next block's frame 52).
    let data = lane(json!({"clips": [{"start": 0, "length": 1, "notes": [
        {"pitch": 60, "velocity": 0.5, "start": 0, "length": 0.0025},
        {"pitch": 64, "velocity": 1.0, "start": 0.0025, "length": 0.005},
        {"pitch": 67, "velocity": 0.25, "start": 0.0025, "length": 0.001}]}]}));
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &data, &pk, SR).unwrap();
    let blocks: Vec<Read> = (0..3).map(|b| read(r.render_block(b * BLOCK as u64).unwrap())).collect();
    let (on, off) = (0.0, 1.0);
    assert_eq!(
        blocks[0],
        Read {
            count: 5.0,
            cap: 812.0,
            at: 16.0,
            ons: 3.0,
            offs: 2.0,
            events: vec![
                [on, 0.0, 1.0, 60.0, 0.5, -1.0],
                [off, 60.0, 1.0, 0.0, 0.0, 0.0],
                [on, 60.0, 2.0, 64.0, 1.0, -1.0],
                [on, 60.0, 3.0, 67.0, 0.25, -1.0],
                [off, 84.0, 3.0, 0.0, 0.0, 0.0],
            ],
        }
    );
    assert_eq!(blocks[1].events, [[off, 52.0, 2.0, 0.0, 0.0, 0.0]], "the next block's frames start at 0 again");
    assert_eq!((blocks[2].count, blocks[2].cap), (0.0, 812.0), "a block with none");
    std::fs::remove_dir_all(pkg).ok();
}

// ABI-8, RENDER-14: live, the lane's controllers reach the kernel too, at the block's first frame
// before the notes there: a MIDI message's three bytes in the low bytes of its first value.
#[test]
fn a_kernel_hears_the_lanes_wheels() {
    let pkg = probe("");
    let pk = load(&pkg);
    let ir = probed();
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &lane(json!({"clips": []})), &pk, SR).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    r.live = true;
    assert!(r.live(0, LiveEv::Midi { source: 1, bytes: [0xB0, 1, 100] }));
    assert!(r.live(0, LiveEv::On { source: 1, handle: 7, pitch: 62.0, velocity: 0.75 }));
    let got = read(r.render_block(0).unwrap());
    let mod_wheel = f32::from(0xB0u16 | 1 << 8) + 100.0 * 65_536.0;
    assert_eq!(got.events, [[4.0, 0.0, 0.0, mod_wheel, 0.0, 0.0], [0.0, 0.0, 1.0, 62.0, 0.75, -1.0]]);
    std::fs::remove_dir_all(pkg).ok();
}

// ABI-1, ABI-8: a kernel's events are its input memory, read-only: debug builds find a kernel
// that writes them and fail its render, naming it.
#[cfg(debug_assertions)]
#[test]
fn a_kernel_that_writes_its_events_fails() {
    let pkg = probe("(i32.store (local.get $h) (i32.const 0))");
    let pk = load(&pkg);
    let data = lane(json!({"clips": [{"start": 0, "length": 1, "notes": [{"pitch": 60, "start": 0, "length": 0.5}]}]}));
    let e = Renderer::new(&check(&probed(), &pk).unwrap(), &data, &pk, SR).unwrap().render(0.01).unwrap_err();
    assert!(format!("{e:#}").contains("`@test/t:k` wrote its input `notes` (ABI-1: kernel input memory is read-only)"), "{e:#}");
    std::fs::remove_dir_all(pkg).ok();
}

/// `n` notes that start at frame 0 and at frame 128 (the first `first` at 0), all ending at frame 10
/// of the fourth block, on a lane at 120 bpm.
fn crowded(n: usize, first: usize) -> DataSet {
    let beats = |frames: f64| frames / 24_000.0;
    let notes: Vec<Value> = (0..n)
        .map(|k| {
            let start = if k < first { 0.0 } else { 128.0 };
            json!({"pitch": k % 128, "start": beats(start), "length": beats(3.0 * 128.0 + 10.0 - start)})
        })
        .collect();
    lane(json!({"clips": [{"start": 0, "length": 1, "notes": notes}]}))
}

// ABI-8 (review E3): a kernel's events input holds what its lane's buffer can bring in a block, so
// none is ever left out: 1100 notes heard starting over two blocks, all ending at one frame, are
// 1100 Ons and 1100 Offs. Its `cap` is its lane's room; a quiet lane's is the smallest, 812.
#[test]
fn every_event_of_a_crowded_lane_reaches_the_kernel() {
    let smallest = sched::Caps::FLOOR.events() + sched::LIVE_ROOM + sched::WHEEL_EVENTS;
    assert_eq!(smallest, 812);
    let pkg = probe("");
    let pk = load(&pkg);
    let data = crowded(1100, 600);
    let mut r = Renderer::new(&check(&probed(), &pk).unwrap(), &data, &pk, SR).unwrap();
    let reads: Vec<Read> = (0..4).map(|k| read(r.render_block(k * BLOCK as u64).unwrap())).collect();
    let (ons, offs): (f32, f32) = (reads.iter().map(|x| x.ons).sum(), reads.iter().map(|x| x.offs).sum());
    assert_eq!((ons, offs), (1100.0, 1100.0), "every On and every Off");
    assert_eq!(reads.iter().map(|x| x.count).collect::<Vec<_>>(), [600.0, 500.0, 0.0, 1100.0]);
    let needs = crate::notes_data(&data, "song", "notes", SR as f64).needs().max(sched::Caps::FLOOR);
    assert_eq!(reads[0].cap, (needs.events() + sched::LIVE_ROOM + sched::WHEEL_EVENTS) as f32, "its lane's room");
    std::fs::remove_dir_all(pkg).ok();
}

/// A kernel `@test/t:k` whose output is how many notes it heard start and not end, with inputs
/// `notes: events<note>` and `in: audio<1>` (which it doesn't read); and `@test/t:slow`, which
/// passes its input on 64 frames late, and says so (`latency = 64`).
fn counter() -> PathBuf {
    let k = r#"
  (func (export "k_new") (param i32) (result i32) (i32.store (i32.const 4096) (i32.const 0)) (i32.const 4096))
  (func (export "k_ports") (result i32) (i32.const 513))
  (func (export "k_reset") (param $s i32) (i32.store (local.get $s) (i32.const 0)))
  (func (export "k_active") (param i32) (result i32) (i32.const 0))
  (func (export "k_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $h i32) (local $out i32) (local $n i32) (local $i i32) (local $kind i32) (local $c i32)
    (local.set $h (i32.load (local.get $io)))
    (local.set $out (i32.load offset=8 (local.get $io)))
    (local.set $n (i32.load (local.get $h)))
    (local.set $c (i32.load (local.get $s)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $kind (i32.load (i32.add (i32.load offset=8 (local.get $h)) (i32.mul (local.get $i) (i32.const 24)))))
      (if (i32.eqz (local.get $kind)) (then (local.set $c (i32.add (local.get $c) (i32.const 1)))))
      (if (i32.eq (local.get $kind) (i32.const 1)) (then (local.set $c (i32.sub (local.get $c) (i32.const 1)))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (i32.store (local.get $s) (local.get $c))
    (local.set $i (local.get $from))
    (block $d2 (loop $l2
      (br_if $d2 (i32.ge_u (local.get $i) (local.get $to)))
      (f32.store (i32.add (local.get $out) (i32.shl (local.get $i) (i32.const 2))) (f32.convert_i32_s (local.get $c)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l2))))
  (func (export "slow_new") (param i32) (result i32) (i32.const 6144))
  (func (export "slow_ports") (result i32) (i32.const 257))
  (func (export "slow_reset") (param i32))
  (func (export "slow_active") (param i32) (result i32) (i32.const 0))
  (func (export "slow_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (memory.copy (i32.load offset=4 (local.get $io)) (i32.load (local.get $io)) (i32.shl (local.get $to) (i32.const 2))))"#;
    let ports = "inputs = [\"notes: events<note>\", \"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]\n\n\
                 [[kernel]]\nname = \"slow\"\ninputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]\nlatency = 64";
    package_with(&module_v1(k), ports)
}

/// The counter reading lane `lane`, its unread audio input straight from a constant or, `late`,
/// through `@test/t:slow`, which makes its notes come 64 frames late (RENDER-18).
fn counting(lane: &str, late: bool) -> Ir {
    let src = if late { "s" } else { "x" };
    song(
        json!([{"id": "n", "op": "lane", "lane": lane}, {"id": "x", "op": "kernel", "kernel": "std:sine"},
            {"id": "s", "op": "kernel", "kernel": "@test/t:slow", "inputs": {"in": "x"}},
            {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": "n", "in": src}}]),
        "k",
        json!([{"key": "a", "kind": "events<note>"}, {"key": "b", "kind": "events<note>"}]),
    )
}

/// What the counter plays at the end of 64 blocks, before and after `to` is activated over `from`.
fn rewired(from: &Ir, to: &Ir, data: &DataSet) -> (f32, f32, usize) {
    let pkg = counter();
    let mut pk = std_packages();
    pk.load(&pkg).unwrap();
    let (fa, fb) = (check(from, &pk).unwrap(), check(to, &pk).unwrap());
    let mut r = Renderer::prepare(&fa, data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = live::Rig::of(&fa, data, r);
    rig.cmd(crate::transport::Cmd::Play);
    let before = rig.run(64 * BLOCK);
    let r = rig.prepare(&fb, data, &pk);
    rig.send(crate::engine::Msg::Activate { renderer: Box::new(r) });
    let after = rig.run(64 * BLOCK);
    let kept = rig.e.current().unwrap().stats().kept.kernels;
    std::fs::remove_dir_all(pkg).ok();
    (before[63 * BLOCK][0], after[63 * BLOCK][0], kept)
}

/// Lane `a`'s note from 0 to 8 beats, lane `b`'s from 12 to 13.
fn two_lanes() -> DataSet {
    let notes =
        |n: Value| LaneData::read(&json!({"clips": [{"start": 0, "length": 16, "notes": [n]}]}).to_string(), &LaneKind::Notes).unwrap();
    DataSet::with_lanes(
        live::at_bpm(120.0),
        [
            (addr("a"), notes(json!({"pitch": 60, "start": 0, "length": 8}))),
            (addr("b"), notes(json!({"pitch": 72, "start": 12, "length": 1}))),
        ],
    )
}

// ABI-8, RENDER-26a (review E1): a kernel's lane is part of its shape, as its settings are. Rewired to
// another lane, it is new: the one that heard lane `a`'s note start plays out the activation's window
// and goes, and the new one has heard nothing. Nothing crossfades its notes.
#[test]
fn a_kernel_rewired_to_another_lane_is_new() {
    let (before, after, kept) = rewired(&counting("a", false), &counting("b", false), &two_lanes());
    assert_eq!(before, 1.0, "lane a's note sounds into it");
    assert_eq!((after, kept), (0.0, 1), "a new counter (only std:sine is kept): the note it heard start is gone with it");
}

// ABI-8, RENDER-18 (review E1): so is one whose notes start or stop coming through a note delay, as
// latency compensation adds or takes one away: the notes in flight in the delay would be lost.
#[test]
fn a_kernel_whose_notes_come_late_or_on_time_is_new() {
    let (before, after, _) = rewired(&counting("a", false), &counting("a", true), &two_lanes());
    assert_eq!((before, after), (1.0, 0.0), "now late: new");
    let (before, after, _) = rewired(&counting("a", true), &counting("a", false), &two_lanes());
    assert_eq!((before, after), (1.0, 0.0), "now on time: new");
    let (before, after, _) = rewired(&counting("a", true), &counting("a", true), &two_lanes());
    assert_eq!((before, after), (1.0, 1.0), "late as before: kept, still counting the note");
}

// ABI-8 (review E3): an events input's room is its lane's, so a lane that outgrows its room at an
// activation (its data grew) makes the kernel that reads it new, with room enough.
#[test]
fn a_kernel_whose_lane_outgrows_its_room_is_new() {
    let pkg = counter();
    let mut pk = std_packages();
    pk.load(&pkg).unwrap();
    let f = check(&counting("a", false), &pk).unwrap();
    let one = |n: usize| {
        let notes: Vec<Value> = (0..n).map(|k| json!({"pitch": k % 128, "start": 0, "length": 8})).collect();
        DataSet::with_lanes(
            live::at_bpm(120.0),
            [(
                addr("a"),
                LaneData::read(&json!({"clips": [{"start": 0, "length": 16, "notes": notes}]}).to_string(), &LaneKind::Notes).unwrap(),
            )],
        )
    };
    let mut r = Renderer::prepare(&f, &one(1), &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = live::Rig::of(&f, &one(1), r);
    rig.cmd(crate::transport::Cmd::Play);
    rig.run(4 * BLOCK);
    let r = rig.prepare(&f, &one(1), &pk);
    rig.send(crate::engine::Msg::Activate { renderer: Box::new(r) });
    rig.run(4 * BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().kept.kernels, 2, "the same lane, the same room: kept");
    let r = rig.prepare(&f, &one(1200), &pk);
    rig.send(crate::engine::Msg::Activate { renderer: Box::new(r) });
    rig.run(4 * BLOCK);
    assert_eq!(rig.e.current().unwrap().stats().kept.kernels, 1, "its lane grew: the counter is new, std:sine kept");
    std::fs::remove_dir_all(pkg).ok();
}

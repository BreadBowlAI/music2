//! Event ports for kernels (ABI-8): a kernel with an `events<note>` input gets its lane's events
//! each block, in kernel ABI v1's reserved shape, read-only, with room for all its lane can bring;
//! and its lane is part of its shape.

use m2_song::{Arrangement, LaneData, LaneKind};

use super::goldens::addr;
use super::*;
use crate::sched::LiveEv;

/// A kernel `@test/t:k` with one input, `notes: events<note>`, and one mono output, which writes
/// what it read: for each of its first 20 events, six numbers (kind, frame, id, and its three
/// values: a NaN as -1, a MIDI message's bytes and a note expression's kind as the whole number they
/// make), then at 123 and 124 how many Ons and Offs it holds in all, at 125 where its events start
/// from the header, at 126 its `cap` and at 127 its `count`. `extra` runs after.
fn probe(extra: &str) -> PathBuf {
    let ports = "inputs = [\"notes: events<note>\"]\noutputs = [\"out: audio<1>\"]";
    package_with(&module_v1(&probe_wat(extra)), ports)
}

/// [`probe`]'s exports.
fn probe_wat(extra: &str) -> String {
    format!(
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
    )
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

// ---- Note effects: kernels that give notes, run in the lane pass behind a guard (RENDER-41,
// RENDER-42, ABI-15) ----

/// Calls of `@test/t:script` that aren't its events alone: one that traps; one that gives its two
/// events where the host put them and then writes nonsense into its header, a `cap` of 2 billion
/// and its events at [`DECOY`]; and one that gives its two events with a `count` of a million.
const TRAP: u32 = u32::MAX;
const LIAR: u32 = u32::MAX - 1;
const OVER: u32 = u32::MAX - 2;

/// Where the script's memory holds four events no one gave: Ons of pitch 99.
const DECOY: usize = 7312;

/// One raw event as a kernel writes it (24 bytes): its kind, frame, id and three values.
fn raw(kind: u32, frame: u32, id: u32, values: [f32; 3]) -> [u32; 6] {
    [kind, frame, id, values[0].to_bits(), values[1].to_bits(), values[2].to_bits()]
}

fn on(frame: u32, id: u32, pitch: f32) -> [u32; 6] {
    raw(0, frame, id, [pitch, 1.0, f32::NAN])
}

fn off(frame: u32, id: u32) -> [u32; 6] {
    raw(1, frame, id, [0.0, 0.0, 0.0])
}

fn cc(frame: u32, bytes: [u8; 3]) -> [u32; 6] {
    [4, frame, 0, u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]), 0, 0]
}

/// The package of the note effect tests, `@test/t`: the probe `k` ([`probe`]), `slow`, which passes
/// its audio on 64 frames late (`latency = 64`), and these note effects, each with
/// `notes: events<note>` in and `out: events<note>`:
/// - `up`: each event as it comes, an On `by` semitones up (its input, 12 unless wired); `beat`
///   the same, `by` in bpm, and `shift`, `by` of any unit;
/// - `script`: at its `c`th call, the raw events `script[c]` names, whatever they are, then none
///   ([`TRAP`], [`LIAR`] and [`OVER`] name calls of other kinds); a reset leaves its place alone;
/// - `flood`: `n` Ons a call (its input), at frame 0, each with an id of its own, never an Off;
/// - `burst`: `n` Ons at every frame, each ended at the next;
/// - `alt`: as `flood`, its Ons at frame 0 on even calls and at frame 127 on odd ones;
/// - `hold`: once it hears an On, a note of its own every block (the Off of the last, then an On),
///   until a reset: as an arpeggiator's Hold holds;
/// - `bog`, whose `fan` is 8: its room of Ons at its first call, then its room of Offs of a note it
///   never started at every call.
fn effects(script: &[Vec<[u32; 6]>]) -> PathBuf {
    const STRIDE: usize = 4 + 6 * 24;
    assert!(script.len() <= 20 && script.iter().all(|c| c.len() <= 6), "the script fits below the heap");
    let mut data = (script.len() as u32).to_le_bytes().to_vec();
    data.resize(4352 - 4100, 0);
    let word = |data: &mut Vec<u8>, at: usize, v: u32| data[at..at + 4].copy_from_slice(&v.to_le_bytes());
    for call in script {
        let mut entry = vec![0u8; STRIDE];
        let n = match call.first() {
            Some(e) if e[0] >= OVER => e[0],
            _ => call.len() as u32,
        };
        word(&mut entry, 0, n);
        for (k, e) in call.iter().enumerate().skip((n >= OVER) as usize) {
            for (w, v) in e.iter().enumerate() {
                word(&mut entry, 4 + 24 * k + 4 * w, *v);
            }
        }
        data.extend(entry);
    }
    data.resize(DECOY - 4100, 0);
    for k in 0..4 {
        data.extend(on(k, 90 + k, 99.0).iter().flat_map(|w| w.to_le_bytes()));
    }
    let bytes: String = data.iter().map(|b| format!("\\{b:02x}")).collect();
    let k = format!(
        r#"{probe}
  (data (i32.const 4100) "{bytes}")
  {up}
  {beat}
  {shift}
  (func (export "script_new") (param i32) (result i32) (i32.store (i32.const 4096) (i32.const 0)) (i32.const 4096))
  (func (export "script_ports") (result i32) (i32.const 257))
  (func (export "script_reset") (param i32))
  (func (export "script_active") (param i32) (result i32) (i32.const 0))
  (func (export "script_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $c i32) (local $o i32) (local $at i32) (local $n i32)
    (local.set $c (i32.load (i32.const 4096)))
    (local.set $o (i32.load offset=4 (local.get $io)))
    (if (i32.lt_u (local.get $c) (i32.load (i32.const 4100))) (then
      (local.set $at (i32.add (i32.const 4352) (i32.mul (local.get $c) (i32.const {STRIDE}))))
      (local.set $n (i32.load (local.get $at)))
      (if (i32.eq (local.get $n) (i32.const -1)) (then unreachable))
      (if (i32.ge_u (local.get $n) (i32.const -3))
        (then
          (memory.copy (i32.load offset=8 (local.get $o)) (i32.add (local.get $at) (i32.const 28)) (i32.const 48))
          (if (i32.eq (local.get $n) (i32.const -2))
            (then
              (i32.store offset=4 (local.get $o) (i32.const 0x7fffffff))
              (i32.store offset=8 (local.get $o) (i32.const {DECOY}))
              (local.set $n (i32.const 2)))
            (else (local.set $n (i32.const 1000000)))))
        (else
          (memory.copy (i32.load offset=8 (local.get $o)) (i32.add (local.get $at) (i32.const 4)) (i32.mul (local.get $n) (i32.const 24)))))
      (i32.store (local.get $o) (local.get $n))))
    (i32.store (i32.const 4096) (i32.add (local.get $c) (i32.const 1))))
  (func (export "flood_new") (param i32) (result i32) (i32.store (i32.const 4104) (i32.const 1)) (i32.const 4096))
  (func (export "flood_ports") (result i32) (i32.const 513))
  (func (export "flood_reset") (param i32) (i32.store (i32.const 4104) (i32.const 1)))
  (func (export "flood_active") (param i32) (result i32) (i32.const 0))
  (func (export "flood_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $o i32) (local $n i32) (local $i i32) (local $e i32) (local $id i32)
    (local.set $n (i32.trunc_f32_u (f32.load (i32.load offset=4 (local.get $io)))))
    (local.set $o (i32.load offset=8 (local.get $io)))
    (if (i32.gt_u (local.get $n) (i32.load offset=4 (local.get $o))) (then (local.set $n (i32.load offset=4 (local.get $o)))))
    (local.set $id (i32.load (i32.const 4104)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $e (i32.add (i32.load offset=8 (local.get $o)) (i32.mul (local.get $i) (i32.const 24))))
      (i32.store (local.get $e) (i32.const 0))
      (i32.store offset=4 (local.get $e) (i32.const 0))
      (i32.store offset=8 (local.get $e) (i32.add (local.get $id) (local.get $i)))
      (f32.store offset=12 (local.get $e) (f32.const 60))
      (f32.store offset=16 (local.get $e) (f32.const 1))
      (f32.store offset=20 (local.get $e) (f32.const nan))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (i32.store (i32.const 4104) (i32.add (local.get $id) (local.get $n)))
    (i32.store (local.get $o) (local.get $n)))
  (func (export "burst_new") (param i32) (result i32) (i32.store (i32.const 4116) (i32.const 1)) (i32.const 4096))
  (func (export "burst_ports") (result i32) (i32.const 513))
  (func (export "burst_reset") (param i32) (i32.store (i32.const 4116) (i32.const 1)))
  (func (export "burst_active") (param i32) (result i32) (i32.const 0))
  (func (export "burst_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $o i32) (local $k i32) (local $f i32) (local $j i32) (local $e i32) (local $c i32) (local $id i32)
    (local.set $k (i32.trunc_f32_u (f32.load (i32.load offset=4 (local.get $io)))))
    (local.set $o (i32.load offset=8 (local.get $io)))
    (local.set $e (i32.load offset=8 (local.get $o)))
    (local.set $id (i32.load (i32.const 4116)))
    (block $df (loop $lf
      (br_if $df (i32.ge_u (local.get $f) (i32.const 128)))
      (local.set $j (i32.const 0))
      (block $dj (loop $lj
        (br_if $dj (i32.ge_u (local.get $j) (local.get $k)))
        (if (i32.gt_u (local.get $id) (local.get $k)) (then
          (i32.store (local.get $e) (i32.const 1))
          (i32.store offset=4 (local.get $e) (local.get $f))
          (i32.store offset=8 (local.get $e) (i32.sub (local.get $id) (local.get $k)))
          (f32.store offset=12 (local.get $e) (f32.const 0))
          (local.set $e (i32.add (local.get $e) (i32.const 24)))
          (local.set $c (i32.add (local.get $c) (i32.const 1)))))
        (i32.store (local.get $e) (i32.const 0))
        (i32.store offset=4 (local.get $e) (local.get $f))
        (i32.store offset=8 (local.get $e) (local.get $id))
        (f32.store offset=12 (local.get $e) (f32.const 60))
        (f32.store offset=16 (local.get $e) (f32.const 1))
        (f32.store offset=20 (local.get $e) (f32.const nan))
        (local.set $e (i32.add (local.get $e) (i32.const 24)))
        (local.set $c (i32.add (local.get $c) (i32.const 1)))
        (local.set $id (i32.add (local.get $id) (i32.const 1)))
        (local.set $j (i32.add (local.get $j) (i32.const 1)))
        (br $lj)))
      (local.set $f (i32.add (local.get $f) (i32.const 1)))
      (br $lf)))
    (i32.store (i32.const 4116) (local.get $id))
    (i32.store (local.get $o) (local.get $c)))
  (func (export "slow_new") (param i32) (result i32) (i32.const 4096))
  (func (export "slow_ports") (result i32) (i32.const 257))
  (func (export "slow_reset") (param i32))
  (func (export "slow_active") (param i32) (result i32) (i32.const 0))
  (func (export "slow_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (memory.copy (i32.load offset=4 (local.get $io)) (i32.load (local.get $io)) (i32.shl (local.get $to) (i32.const 2))))
  (func (export "alt_new") (param i32) (result i32) (i32.store (i32.const 4096) (i32.const 1)) (i32.store (i32.const 4100) (i32.const 0)) (i32.const 4096))
  (func (export "alt_ports") (result i32) (i32.const 513))
  (func (export "alt_reset") (param i32) (i32.store (i32.const 4096) (i32.const 1)) (i32.store (i32.const 4100) (i32.const 0)))
  (func (export "alt_active") (param i32) (result i32) (i32.const 0))
  (func (export "alt_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $o i32) (local $n i32) (local $i i32) (local $e i32) (local $id i32) (local $f i32) (local $c i32)
    (local.set $n (i32.trunc_f32_u (f32.load (i32.load offset=4 (local.get $io)))))
    (local.set $o (i32.load offset=8 (local.get $io)))
    (if (i32.gt_u (local.get $n) (i32.load offset=4 (local.get $o))) (then (local.set $n (i32.load offset=4 (local.get $o)))))
    (local.set $id (i32.load (i32.const 4096)))
    (local.set $c (i32.load (i32.const 4100)))
    (local.set $f (i32.mul (i32.and (local.get $c) (i32.const 1)) (i32.const 127)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $e (i32.add (i32.load offset=8 (local.get $o)) (i32.mul (local.get $i) (i32.const 24))))
      (i32.store (local.get $e) (i32.const 0))
      (i32.store offset=4 (local.get $e) (local.get $f))
      (i32.store offset=8 (local.get $e) (i32.add (local.get $id) (local.get $i)))
      (f32.store offset=12 (local.get $e) (f32.const 60))
      (f32.store offset=16 (local.get $e) (f32.const 1))
      (f32.store offset=20 (local.get $e) (f32.const nan))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (i32.store (i32.const 4096) (i32.add (local.get $id) (local.get $n)))
    (i32.store (i32.const 4100) (i32.add (local.get $c) (i32.const 1)))
    (i32.store (local.get $o) (local.get $n)))
  (func (export "hold_new") (param i32) (result i32) (i32.store (i32.const 4104) (i32.const 0)) (i32.store (i32.const 4108) (i32.const 1)) (i32.const 4096))
  (func (export "hold_ports") (result i32) (i32.const 257))
  (func (export "hold_reset") (param i32) (i32.store (i32.const 4104) (i32.const 0)))
  (func (export "hold_active") (param i32) (result i32) (i32.load (i32.const 4104)))
  (func (export "hold_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $o i32) (local $e i32) (local $id i32) (local $h i32)
    (local.set $h (i32.load (local.get $io)))
    (if (i32.gt_u (i32.load (local.get $h)) (i32.const 0)) (then
      (if (i32.eqz (i32.load (i32.load offset=8 (local.get $h)))) (then (i32.store (i32.const 4104) (i32.const 1))))))
    (local.set $o (i32.load offset=4 (local.get $io)))
    (if (i32.load (i32.const 4104)) (then
      (local.set $id (i32.load (i32.const 4108)))
      (local.set $e (i32.load offset=8 (local.get $o)))
      (i32.store (local.get $e) (i32.const 1))
      (i32.store offset=4 (local.get $e) (i32.const 0))
      (i32.store offset=8 (local.get $e) (local.get $id))
      (f32.store offset=12 (local.get $e) (f32.const 0))
      (i32.store offset=24 (local.get $e) (i32.const 0))
      (i32.store offset=28 (local.get $e) (i32.const 0))
      (i32.store offset=32 (local.get $e) (i32.add (local.get $id) (i32.const 1)))
      (f32.store offset=36 (local.get $e) (f32.const 60))
      (f32.store offset=40 (local.get $e) (f32.const 1))
      (f32.store offset=44 (local.get $e) (f32.const nan))
      (i32.store (i32.const 4108) (i32.add (local.get $id) (i32.const 1)))
      (i32.store (local.get $o) (i32.const 2)))))
  (func (export "bog_new") (param i32) (result i32) (i32.store (i32.const 4112) (i32.const 0)) (i32.const 4096))
  (func (export "bog_ports") (result i32) (i32.const 257))
  (func (export "bog_reset") (param i32) (i32.store (i32.const 4112) (i32.const 0)))
  (func (export "bog_active") (param i32) (result i32) (i32.const 0))
  (func (export "bog_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $o i32) (local $n i32) (local $i i32) (local $e i32) (local $c i32) (local $off i32)
    (local.set $o (i32.load offset=4 (local.get $io)))
    (local.set $c (i32.load (i32.const 4112)))
    (local.set $n (i32.load offset=4 (local.get $o)))
    (local.set $off (i32.ne (local.get $c) (i32.const 0)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $e (i32.add (i32.load offset=8 (local.get $o)) (i32.mul (local.get $i) (i32.const 24))))
      (i32.store (local.get $e) (local.get $off))
      (i32.store offset=4 (local.get $e) (i32.const 0))
      (i32.store offset=8 (local.get $e) (if (result i32) (local.get $off) (then (i32.const 0)) (else (i32.add (local.get $i) (i32.const 1)))))
      (f32.store offset=12 (local.get $e) (if (result f32) (local.get $off) (then (f32.const 0)) (else (f32.const 60))))
      (f32.store offset=16 (local.get $e) (f32.const 1))
      (f32.store offset=20 (local.get $e) (f32.const nan))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (i32.store (i32.const 4112) (i32.add (local.get $c) (i32.const 1)))
    (i32.store (local.get $o) (local.get $n)))"#,
        probe = probe_wat(""),
        up = up_wat("up"),
        beat = up_wat("beat"),
        shift = up_wat("shift"),
    );
    let notes = "inputs = [\"notes: events<note>\"]";
    let ports = format!(
        "{notes}\noutputs = [\"out: audio<1>\"]\n\n\
         [[kernel]]\nname = \"up\"\ninputs = [\"notes: events<note>\", \"by: control<semitones> = 12\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"beat\"\ninputs = [\"notes: events<note>\", \"by: control<bpm> = 120\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"shift\"\ninputs = [\"notes: events<note>\", \"by: control<*> = 0\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"script\"\n{notes}\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"flood\"\ninputs = [\"notes: events<note>\", \"n: control<scalar> = 0\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"burst\"\ninputs = [\"notes: events<note>\", \"n: control<scalar> = 0\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"slow\"\ninputs = [\"in: audio<1>\"]\noutputs = [\"out: audio<1>\"]\nlatency = 64\n\n\
         [[kernel]]\nname = \"alt\"\ninputs = [\"notes: events<note>\", \"n: control<scalar> = 0\"]\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"hold\"\n{notes}\noutputs = [\"out: events<note>\"]\n\n\
         [[kernel]]\nname = \"bog\"\n{notes}\noutputs = [\"out: events<note>\"]\nfan = 8"
    );
    package_with(&module_v1(&k), &ports)
}

/// The exports of a kernel `k` that gives each event it takes, an On `by` (its second input) up.
fn up_wat(k: &str) -> String {
    format!(
        r#"  (func (export "{k}_new") (param i32) (result i32) (i32.const 4096))
  (func (export "{k}_ports") (result i32) (i32.const 513))
  (func (export "{k}_reset") (param i32))
  (func (export "{k}_active") (param i32) (result i32) (i32.const 0))
  (func (export "{k}_process") (param $s i32) (param $io i32) (param $from i32) (param $to i32)
    (local $h i32) (local $o i32) (local $n i32) (local $i i32) (local $src i32) (local $dst i32) (local $by f32)
    (local.set $h (i32.load (local.get $io)))
    (local.set $by (f32.load (i32.load offset=4 (local.get $io))))
    (local.set $o (i32.load offset=8 (local.get $io)))
    (local.set $n (i32.load (local.get $h)))
    (block $done (loop $l
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      (local.set $src (i32.add (i32.load offset=8 (local.get $h)) (i32.mul (local.get $i) (i32.const 24))))
      (local.set $dst (i32.add (i32.load offset=8 (local.get $o)) (i32.mul (local.get $i) (i32.const 24))))
      (memory.copy (local.get $dst) (local.get $src) (i32.const 24))
      (if (i32.eqz (i32.load (local.get $src)))
        (then (f32.store offset=12 (local.get $dst) (f32.add (f32.load offset=12 (local.get $src)) (local.get $by)))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l)))
    (i32.store (local.get $o) (local.get $n)))"#
    )
}

/// std, music2's own packs, and [`effects`]' package.
pub(super) fn with_effects(script: &[Vec<[u32; 6]>]) -> (PackageSet, PathBuf) {
    let pkg = effects(script);
    let mut pk = own_packages();
    pk.load(&pkg).unwrap();
    (pk, pkg)
}

/// A song whose output is the probe reading note effect `e`, `@test/t:<kernel>` with `inputs`, over
/// the notes lane `notes`; and `more` nodes.
fn through(kernel: &str, inputs: Value, more: Value) -> Ir {
    let mut ins = inputs;
    ins["notes"] = json!("n");
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "e", "op": "kernel", "kernel": format!("@test/t:{kernel}"), "inputs": ins}),
        json!({"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": "e"}}),
    ];
    nodes.extend(more.as_array().unwrap().iter().cloned());
    song(
        Value::Array(nodes),
        "k",
        json!([{"key": "notes", "kind": "events<note>"}, {"key": "by", "kind": "control<semitones>", "default": 3}]),
    )
}

/// The slot of the first note effect of `r`'s plan: after every lane's.
fn fx_slot(r: &Renderer) -> usize {
    r.info().effects[0].1
}

/// A renderer as the live engine plays it (a fault stops one sound), stopped at 0.
fn live_one(ir: &Ir, data: &DataSet, pk: &PackageSet) -> Renderer {
    let mut r = Renderer::new(&check(ir, pk).unwrap_or_else(|e| panic!("{e:?}")), data, pk, SR).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    r.live = true;
    r
}

/// The note effect's events in each of `blocks` blocks.
fn given(r: &mut Renderer, blocks: usize) -> Vec<Vec<Event>> {
    (0..blocks)
        .map(|_| {
            r.render_block(r.clock()).unwrap();
            r.lane_events(fx_slot(r)).to_vec()
        })
        .collect()
}

fn ev_on(frame: u32, id: u32, pitch: f32) -> Event {
    Event::On { frame, id, pitch, velocity: 1.0, from_pitch: None }
}

fn ev_off(frame: u32, id: u32) -> Event {
    Event::Off { frame, id, velocity: 0.0 }
}

/// The faults its plan's ring holds, by code and node.
fn faults(r: &Renderer) -> Vec<(fault::Code, fault::Subject)> {
    r.info().faults.drain().into_iter().map(|f| (f.code, f.subject)).collect()
}

/// A poly of sine voices at their notes' pitches, playing `events`, before the nodes `before`.
fn voices(events: &str, before: Value) -> Ir {
    let body = json!({"out": "amp", "nodes": [
        {"id": "p", "op": "lane", "lane": "pitch"},
        {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]});
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"})];
    nodes.extend(before.as_array().unwrap().iter().cloned());
    nodes.push(json!({"id": "v", "op": "poly", "events": events, "voices": 4, "body": body}));
    song(Value::Array(nodes), "v", json!([{"key": "notes", "kind": "events<note>"}]))
}

/// Notes at 120 bpm (a beat is 24 000 frames), starting at `starts` (in frames), each 300 frames
/// long, at `pitch` and up.
fn notes_at(starts: &[u32], pitch: u32) -> DataSet {
    let beats = |f: u32| f as f64 / 24_000.0;
    let ns: Vec<Value> =
        starts.iter().enumerate().map(|(k, &s)| json!({"pitch": pitch + k as u32, "start": beats(s), "length": beats(300)})).collect();
    lane(json!({"clips": [{"start": 0, "length": 8, "notes": ns}]}))
}

// RENDER-41: a note effect's notes reach an instrument to the frame: a poly over `up` (an octave up)
// plays the very bits of a poly over the same notes written an octave up.
#[test]
fn a_note_effects_notes_reach_a_poly_to_the_frame() {
    let (pk, pkg) = with_effects(&[]);
    let starts = [0, 77, 128, 300, 1000];
    let up = voices("e", json!([{"id": "e", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "n"}}]));
    let heard = Renderer::new(&check(&up, &pk).unwrap(), &notes_at(&starts, 60), &pk, SR).unwrap().render(0.05).unwrap();
    let written =
        Renderer::new(&check(&voices("n", json!([])), &pk).unwrap(), &notes_at(&starts, 72), &pk, SR).unwrap().render(0.05).unwrap();
    assert!(heard.iter().any(|f| f[0].abs() > 0.1), "it sounds");
    assert!(heard == written, "the same bits");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-41: and a plugin, to the frame: its notes are the note effect's, an octave up.
#[test]
fn a_note_effects_notes_reach_a_plugin_to_the_frame() {
    m2_plugins::test_main_thread();
    m2_plugins::probe::install();
    let (mut pk, pkg) = with_effects(&[]);
    let mut ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"},
            {"id": "e", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "n"}},
            {"id": "p", "op": "plugin", "plugin": "clap:m2.probe", "inputs": {"notes": "e"}}]),
        "p",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    assert!(pk.bind_plugins(&mut ir).is_empty());
    let flat = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let data = notes_at(&[5, 200], 60);
    let mut r =
        Renderer::prepare(&flat, &data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    let h = r.plugin_handles()[0];
    for k in 0..6 {
        r.render_block(k * BLOCK as u64).unwrap();
    }
    let notes = m2_plugins::probe::stats(h).unwrap().notes();
    let seen: Vec<(i64, m2_plugins::probe::Note)> = notes.iter().map(|n| (n.0, n.1)).collect();
    use m2_plugins::probe::Note::{Off, On};
    assert_eq!(seen, [(5, On), (200, On), (305, Off), (500, Off)], "{notes:?}");
    assert_eq!((notes[0].2, notes[1].2), (2, 4), "the note effect's own ids, after the lane's 1 and 3");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-41: a note effect reads another's notes: a chain, run in order in the lane pass, each
// numbering its own notes.
#[test]
fn two_note_effects_make_a_chain() {
    let (pk, pkg) = with_effects(&[]);
    let ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"},
            {"id": "e", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "n"}},
            {"id": "f", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "e", "by": "m"}},
            {"id": "m", "op": "const", "value": -5},
            {"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": "f"}}]),
        "k",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &notes_at(&[10, 40], 60), &pk, SR).unwrap();
    let got = read(r.render_block(0).unwrap());
    let (on, off) = (0.0, 1.0);
    // The lane's ids are 1 and 2, the first effect's 3 and 4, the second's 5 and 6.
    assert_eq!(got.events, [[on, 10.0, 5.0, 67.0, 0.8, -1.0], [on, 40.0, 6.0, 68.0, 0.8, -1.0]]);
    r.render_block(BLOCK as u64).unwrap();
    assert_eq!(read(r.render_block(2 * BLOCK as u64).unwrap()).events, [[off, 54.0, 5.0, 0.0, 0.0, 0.0], [off, 84.0, 6.0, 0.0, 0.0, 0.0]]);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-41: a note effect's other inputs are read in the lane pass as their own nodes read them: a
// knob's cell as it holds it this block, a control lane from its data at each beat (and its default
// where it has none), `time` from the block's times.
#[test]
fn a_note_effect_reads_its_knob_time_and_a_control_lane() {
    let (pk, pkg) = with_effects(&[]);
    let knob = json!([{"id": "b", "op": "const", "value": 7, "origin": {"site": 1}}]);
    let flat = check(&through("up", json!({"by": "b"}), knob), &pk).unwrap();
    let data = notes_at(&[0, 128 * 3], 60);
    let mut r = Renderer::new(&flat, &data, &pk, SR).unwrap();
    let pitch = |r: &mut Renderer, at: u64| {
        r.render_block(at).unwrap();
        r.lane_events(fx_slot(r)).iter().find_map(|e| if let Event::On { pitch, .. } = e { Some(*pitch) } else { None })
    };
    assert_eq!(pitch(&mut r, 0), Some(67.0), "the knob's 7");
    let origin: m2_ir::Origin = serde_json::from_value(json!({"site": 1})).unwrap();
    let cell = r.cell_map().cells(&origin).first().copied().expect("the knob's cell");
    r.set_cells(&[cell], 9.0);
    pitch(&mut r, BLOCK as u64);
    pitch(&mut r, 2 * BLOCK as u64);
    assert_eq!(pitch(&mut r, 3 * BLOCK as u64), Some(70.0), "turned to 9: the next note");
    let lane_by = check(&through("up", json!({"by": "c"}), json!([{"id": "c", "op": "lane", "lane": "by"}])), &pk).unwrap();
    let mut r = Renderer::new(&lane_by, &data, &pk, SR).unwrap();
    assert_eq!(pitch(&mut r, 0), Some(63.0), "the lane's default where it has no data");
    let by = LaneData::read(
        &json!({"unit": "semitones", "clips": [{"start": 0, "length": 8, "points": [[0, 5], [8, 5]]}]}).to_string(),
        &LaneKind::Control { unit: Some("semitones".into()) },
    )
    .unwrap();
    let notes = LaneData::read(
        &json!({"clips": [{"start": 0, "length": 8, "notes": [{"pitch": 60, "start": 0, "length": 0.01}]}]}).to_string(),
        &LaneKind::Notes,
    )
    .unwrap();
    let with_by = DataSet::with_lanes(live::at_bpm(120.0), [(addr("notes"), notes.clone()), (addr("by"), by)]);
    let mut r = Renderer::new(&lane_by, &with_by, &pk, SR).unwrap();
    assert_eq!(pitch(&mut r, 0), Some(65.0), "the lane's 5, from its data");
    // `time.playing` is 1 where the song plays, 0 where it is stopped; `time.beats` where it is.
    let playing = through("shift", json!({"by": "t"}), json!([{"id": "t", "op": "time", "of": "playing"}]));
    let data = DataSet::with_lanes(live::at_bpm(120.0), [(addr("notes"), notes)]);
    let mut r = Renderer::new(&check(&playing, &pk).unwrap(), &data, &pk, SR).unwrap();
    assert_eq!(pitch(&mut r, 0), Some(61.0), "playing");
    let mut r = live_one(&playing, &data, &pk);
    assert!(r.live(0, LiveEv::On { source: 1, handle: 1, pitch: 60.0, velocity: 1.0 }));
    assert_eq!(pitch(&mut r, 0), Some(60.0), "stopped");
    let beats = check(&through("shift", json!({"by": "t"}), json!([{"id": "t", "op": "time", "of": "beats"}])), &pk).unwrap();
    let mut r = Renderer::new(&beats, &notes_at(&[256 + 5], 60), &pk, SR).unwrap();
    pitch(&mut r, 0);
    pitch(&mut r, BLOCK as u64);
    assert_eq!(pitch(&mut r, 2 * BLOCK as u64), Some(60.0 + 256.0 / 24_000.0), "the beat at the block's first frame");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-7: `time.tempo` is the tempo where the song is, in bpm, as `time.beats` is the beat; a note
// effect reads it too.
#[test]
fn time_tempo_is_the_tempo_where_the_song_is() {
    let (pk, pkg) = with_effects(&[]);
    let arr: Arrangement =
        serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": 30}, {"beat": 0.01, "bpm": 45}], "meter": []})).unwrap();
    let notes = LaneData::read(
        &json!({"clips": [{"start": 0, "length": 1, "notes": [{"pitch": 0, "start": 0, "length": 0.001}, {"pitch": 0, "start": 0.05, "length": 0.001}]}]})
            .to_string(),
        &LaneKind::Notes,
    )
    .unwrap();
    let data = DataSet::with_lanes(arr, [(addr("notes"), notes)]);
    let flat = check(&through("beat", json!({"by": "t"}), json!([{"id": "t", "op": "time", "of": "tempo"}])), &pk).unwrap();
    let mut r = Renderer::new(&flat, &data, &pk, SR).unwrap();
    let mut ons = vec![];
    for k in 0..60 {
        r.render_block(k * BLOCK as u64).unwrap();
        ons.extend(r.lane_events(fx_slot(&r)).iter().filter_map(|e| if let Event::On { pitch, .. } = e { Some(*pitch) } else { None }));
    }
    assert_eq!(ons, [30.0, 45.0], "30 bpm at the start, 45 after beat 0.01");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: what a kernel gives is put in RENDER-14's order, a frame's controllers, then Offs,
// then Ons, and each On numbered by the engine.
#[test]
fn the_guard_puts_what_a_kernel_gives_in_order() {
    let script = [vec![on(10, 1, 60.0), cc(10, [0xB0, 1, 5]), on(3, 2, 62.0), off(10, 2)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 1).remove(0);
    assert_eq!(got, [ev_on(3, 1, 62.0), Event::Midi { frame: 10, bytes: [0xB0, 1, 5] }, ev_off(10, 1), ev_on(10, 2, 60.0)]);
    assert!(faults(&r).is_empty(), "nothing left out");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: what no one can play is left out, and said once live: a frame past the block, a kind it
// doesn't send, a pitch outside 0 to 127 or not a number, a velocity outside 0 to 1, a controller
// that isn't a wheel or the pedal. An On's third value may be NaN: no slide.
#[test]
fn the_guard_leaves_out_what_no_one_can_play() {
    let bad = vec![
        on(128, 1, 60.0),
        raw(2, 0, 1, [1.0, 0.5, 0.0]),
        on(0, 2, 130.0),
        on(0, 3, f32::NAN),
        raw(0, 0, 4, [60.0, 1.5, f32::NAN]),
        cc(0, [0xB0, 7, 100]),
    ];
    let script = [bad.clone(), vec![raw(1, 0, 9, [-1.0, 0.0, 0.0]), on(5, 6, 61.0)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 2);
    assert_eq!(got, [vec![], vec![ev_on(5, 1, 61.0)]]);
    let fx = r.info().effects[0].0;
    assert_eq!(faults(&r), [(fault::Code::NotesLeftOut, fault::Subject::Node(fx as u32))], "said once a plan");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42, RENDER-1: offline, a drop fails the render, naming the note effect.
#[test]
fn a_drop_fails_an_export() {
    let (pk, pkg) = with_effects(&[vec![on(0, 1, 200.0)]]);
    let flat = check(&through("script", json!({}), json!([])), &pk).unwrap();
    let mut r = Renderer::new(&flat, &lane(json!({"clips": []})), &pk, SR).unwrap();
    r.set_strict(false);
    let e = r.render(0.01).unwrap_err();
    let failed = e.downcast_ref::<fault::Failed>().expect("a fault");
    let fx = flat.nodes.iter().position(|n| n.name == "song/e").unwrap();
    assert_eq!((failed.fault.code, failed.fault.subject), (fault::Code::NotesLeftOut, fault::Subject::Node(fx as u32)));
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: live, a drop is said once a plan, however many come: the song plays on.
#[test]
fn a_drop_is_said_once_live() {
    let script: Vec<Vec<[u32; 6]>> = (0..4).map(|_| vec![on(0, 1, -1.0)]).collect();
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    given(&mut r, 4);
    assert_eq!(faults(&r).len(), 1);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: an Off goes out only for a note this kernel started, under the id the guard gave it;
// any other is left out, with no fault. An On whose id is sounding ends that note first.
#[test]
fn the_guard_sends_offs_only_for_its_own_notes() {
    let script = [vec![on(0, 5, 60.0)], vec![off(3, 6), off(4, 5)], vec![on(0, 7, 61.0)], vec![on(9, 7, 62.0)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 4);
    assert_eq!(got, [vec![ev_on(0, 1, 60.0)], vec![ev_off(4, 1)], vec![ev_on(0, 2, 61.0)], vec![ev_off(9, 2), ev_on(9, 3, 62.0)]]);
    assert!(faults(&r).is_empty(), "an Off of no note of its own is no drop");
    assert_eq!(r.sounding_notes(), 1);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: a kernel that never ends its notes keeps sounding: with its room full of notes, each new
// one ends the oldest.
#[test]
fn the_guard_ends_the_oldest_note_when_its_room_is_full() {
    let (pk, pkg) = with_effects(&[]);
    let one = json!([{"id": "c", "op": "const", "value": 1}]);
    let mut r = live_one(&through("flood", json!({"n": "c"}), one), &lane(json!({"clips": []})), &pk);
    let room = sched::Caps::FLOOR.sounding + sched::LIVE_ROOM;
    let got = given(&mut r, room + 2);
    assert!(got[..room].iter().all(|b| matches!(b[..], [Event::On { .. }])), "one On a block while there is room");
    let first = |b: &[Event]| b.iter().find_map(|e| if let Event::On { id, .. } = e { Some(*id) } else { None }).unwrap();
    assert_eq!(got[room][0], ev_off(0, first(&got[0])), "the oldest ends");
    assert_eq!(got[room + 1][0], ev_off(0, first(&got[1])), "then the next oldest");
    assert_eq!(r.sounding_notes(), room);
    assert!(faults(&r).is_empty(), "nothing left out");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: past its room in one block, it keeps the first: as many Ons as its consumers can start
// in a block (here three a frame, each ending a frame later), the rest left out, and said; and at
// one frame, as many notes as it has room to sound, as none of them sounds yet to make room.
#[test]
fn the_guard_keeps_the_first_events_of_a_full_block() {
    let (pk, pkg) = with_effects(&[]);
    let ons =
        |got: &[Event]| -> Vec<u32> { got.iter().filter_map(|e| if let Event::On { id, .. } = e { Some(*id) } else { None }).collect() };
    let three = json!([{"id": "c", "op": "const", "value": 3}]);
    let mut r = live_one(&through("burst", json!({"n": "c"}), three), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 1).remove(0);
    let most = 2 * (sched::Caps::FLOOR.ons + sched::LIVE_ROOM) + sched::LIVE_ROOM;
    assert_eq!(ons(&got), (1..=most as u32).collect::<Vec<_>>(), "the first {most}");
    assert!(
        got.len() <= r.info().lanes[fx_slot(&r)].caps.map_or(0, |c| c.events() + sched::LIVE_ROOM + sched::WHEEL_EVENTS),
        "within its room"
    );
    assert_eq!(faults(&r).len(), 1, "said");
    let many = json!([{"id": "c", "op": "const", "value": 400}]);
    let mut r = live_one(&through("flood", json!({"n": "c"}), many), &lane(json!({"clips": []})), &pk);
    let room = sched::Caps::FLOOR.sounding + sched::LIVE_ROOM;
    assert_eq!(ons(&given(&mut r, 1)[0]), (1..=room as u32).collect::<Vec<_>>(), "at one frame, the first {room}");
    assert_eq!(faults(&r).len(), 1, "said");
    std::fs::remove_dir_all(pkg).ok();
}

// ABI-15: the host reads the count a kernel says from where it put the events, never from the
// header's own `cap` and address, which a kernel may overwrite: here pointed at four events no one
// gave, none of which comes out.
#[test]
fn the_host_reads_no_more_than_its_own_room() {
    let (pk, pkg) = with_effects(&[vec![[LIAR; 6], on(1, 1, 60.0), on(2, 2, 61.0)]]);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    assert_eq!(given(&mut r, 1)[0], [ev_on(1, 1, 60.0), ev_on(2, 2, 61.0)], "the two it gave, where the host put them");
    assert!(faults(&r).is_empty());
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42, ABI-15: a `count` past the room is a drop: the guard reads its room's worth, and says so.
#[test]
fn a_count_past_the_room_is_a_drop() {
    let (pk, pkg) = with_effects(&[vec![[OVER; 6], on(1, 1, 60.0), on(2, 2, 61.0)]]);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 1).remove(0);
    let room = r.info().lanes[fx_slot(&r)].caps.map_or(0, |c| c.events() + sched::LIVE_ROOM + sched::WHEEL_EVENTS);
    assert!(got.len() <= room, "{} events", got.len());
    assert_eq!(faults(&r).into_iter().map(|f| f.0).collect::<Vec<_>>(), [fault::Code::NotesLeftOut]);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: an On and its Off at one frame, in that order, is a note of no length, left out as a
// lane leaves one out, with no fault. An Off then an On of one id at one frame end a note and
// start another.
#[test]
fn the_guard_leaves_out_a_note_of_no_length() {
    let script = [vec![on(0, 5, 60.0)], vec![on(10, 1, 62.0), off(10, 1), off(20, 5), on(20, 5, 63.0)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    // The note of no length took an id, 2, that no note ever gets.
    assert_eq!(given(&mut r, 2), [vec![ev_on(0, 1, 60.0)], vec![ev_off(20, 1), ev_on(20, 3, 63.0)]]);
    assert_eq!(r.sounding_notes(), 1, "nothing hangs");
    assert!(faults(&r).is_empty(), "no drop");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: a kernel that stops live (here, a trap) gives no more notes: its guard ends every note
// it has sounding and puts its wheels at rest, the pedal and the bend before the Offs.
#[test]
fn a_stopped_note_effect_ends_its_notes_and_lets_the_pedal_up() {
    let script = [vec![cc(0, [0xB0, 64, 127]), cc(0, [0xE0, 0, 80]), on(1, 1, 60.0)], vec![[TRAP; 6]], vec![on(0, 2, 60.0)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    let got = given(&mut r, 3);
    let midi = |bytes| Event::Midi { frame: 0, bytes };
    assert_eq!(got[0], [midi([0xB0, 64, 127]), midi([0xE0, 0, 80]), ev_on(1, 1, 60.0)]);
    assert_eq!(got[1], [midi([0xE0, 0, 64]), midi([0xB0, 64, 0]), ev_off(0, 1)], "stopped: at rest, then the Offs");
    assert_eq!(got[2], [], "silent from then on");
    assert_eq!(faults(&r).into_iter().map(|f| f.0).collect::<Vec<_>>(), [fault::Code::Stopped]);
    assert_eq!(r.sounding_notes(), 0);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: Stop pressed while the song is already stopped ends a note effect's notes, and lets its
// pedal up, at the next block's first frame.
#[test]
fn stop_pressed_while_stopped_ends_a_note_effects_notes() {
    let script = [vec![cc(0, [0xB0, 64, 127]), on(2, 1, 60.0)]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    given(&mut r, 2);
    assert_eq!(r.sounding_notes(), 1, "stopped, it plays on");
    r.transport_cmd(Cmd::Stop);
    assert_eq!(given(&mut r, 1)[0], [Event::Midi { frame: 0, bytes: [0xB0, 64, 0] }, ev_off(0, 1)]);
    assert_eq!(r.sounding_notes(), 0);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: Stop pressed while stopped resets the kernel too, as a panic does: one that holds a note
// itself (an arpeggiator's Hold) goes silent.
#[test]
fn stop_pressed_while_stopped_silences_a_kernel_that_holds() {
    let (pk, pkg) = with_effects(&[]);
    let ir = voices("e", json!([{"id": "e", "op": "kernel", "kernel": "@test/t:hold", "inputs": {"notes": "n"}}]));
    let mut r = live_one(&ir, &lane(json!({"clips": []})), &pk);
    assert!(r.live(0, LiveEv::On { source: 1, handle: 1, pitch: 50.0, velocity: 0.9 }));
    r.render_block(r.clock()).unwrap();
    assert!(r.live(0, LiveEv::Off { source: 1, handle: 1 }));
    for _ in 0..3 {
        r.render_block(r.clock()).unwrap();
    }
    assert_eq!(r.sounding_notes(), 1, "held by its Hold, the key let go");
    r.transport_cmd(Cmd::Stop);
    let after: Vec<usize> = (0..3)
        .map(|_| {
            r.render_block(r.clock()).unwrap();
            r.sounding_notes()
        })
        .collect();
    assert_eq!(after, [0, 0, 0], "Stop while stopped: silence");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42, RENDER-14: Stop pressed while stopped ends the notes after the kernel's own controllers
// at frame 0, in RENDER-14's order.
#[test]
fn stop_pressed_while_stopped_keeps_renders_14s_order() {
    let script = [vec![cc(0, [0xB0, 64, 127]), on(2, 1, 60.0)], vec![], vec![cc(0, [0xB0, 1, 5])]];
    let (pk, pkg) = with_effects(&script);
    let mut r = live_one(&through("script", json!({}), json!([])), &lane(json!({"clips": []})), &pk);
    given(&mut r, 2);
    r.transport_cmd(Cmd::Stop);
    let midi = |bytes| Event::Midi { frame: 0, bytes };
    assert_eq!(given(&mut r, 1)[0], [midi([0xB0, 1, 5]), midi([0xB0, 64, 0]), ev_off(0, 1)]);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42, RENDER-9, RENDER-36: a seek ends a note effect's notes at its frame and lets the pedal
// up, as a lane does; a loop's wrap ends them and leaves the pedal.
#[test]
fn a_seek_or_a_wrap_ends_a_note_effects_notes() {
    let script = [vec![cc(0, [0xB0, 64, 127]), on(2, 1, 60.0)], vec![], vec![on(0, 3, 61.0)]];
    let (pk, pkg) = with_effects(&script);
    let ir = through("script", json!({}), json!([]));
    let mut r = live_one(&ir, &lane(json!({"clips": []})), &pk);
    r.transport_cmd(Cmd::Play);
    given(&mut r, 1);
    r.transport_cmd(Cmd::Seek(1.0));
    assert_eq!(given(&mut r, 1)[0], [Event::Midi { frame: 0, bytes: [0xB0, 64, 0] }, ev_off(0, 1)], "a seek");
    let mut r = live_one(&ir, &lane(json!({"clips": []})), &pk);
    r.transport_cmd(Cmd::Play);
    // 300 frames at 120 bpm: the wrap falls at frame 44 of the third block.
    r.transport_cmd(Cmd::Loop(Some((0.0, 300.0 / 24_000.0))));
    let got = given(&mut r, 3);
    assert_eq!(got[2], [ev_on(0, 2, 61.0), ev_off(44, 1), ev_off(44, 2)], "a wrap: every note ends there, the pedal stays down");
    std::fs::remove_dir_all(pkg).ok();
}

/// A song of `bog` (fan 8) over the lane, or `chain` of them, into the probe.
fn bogged(chain: usize) -> Ir {
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"})];
    for k in 0..chain {
        let from = if k == 0 { "n".to_string() } else { format!("e{}", k - 1) };
        nodes.push(json!({"id": format!("e{k}"), "op": "kernel", "kernel": "@test/t:bog", "inputs": {"notes": from}}));
    }
    nodes.push(json!({"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": format!("e{}", chain - 1)}}));
    song(Value::Array(nodes), "k", json!([{"key": "notes", "kind": "events<note>"}]))
}

// RENDER-42: the guard's own time is a few steps an event, however many notes sound: a kernel that
// fills its room with notes and then gives its room of Offs of a note it never started, every block,
// costs at most 16 steps of the guard's table an event.
#[test]
fn the_guards_steps_per_event_stay_few() {
    let (pk, pkg) = with_effects(&[]);
    let mut r = live_one(&bogged(1), &lane(json!({"clips": []})), &pk);
    let slot = fx_slot(&r);
    for k in 0..6 {
        r.render_block(k * BLOCK as u64).unwrap();
    }
    let (steps, events) = r.effect_steps(slot);
    assert!(r.sounding_notes() >= 2000, "its room of notes sounds: {}", r.sounding_notes());
    assert!(events > 5 * 6000, "a full room of events each block: {events}");
    assert!(steps <= 16 * events, "{steps} steps for {events} events");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-42: and in time: the flood above takes a small part of a block (nightly: it judges speed;
// the review measured 7.1 ms a block before the guard's table).
#[test]
fn a_flooded_guard_takes_a_small_part_of_a_block() {
    let (pk, pkg) = with_effects(&[]);
    let mut r = live_one(&bogged(1), &lane(json!({"clips": []})), &pk);
    let mut runner = Runner::one();
    let quickest =
        (0..40).map(|k| r.gauge_block(k * BLOCK as u64, &mut runner, crate::gauge::How::Parts).unwrap().pre).skip(1).min().unwrap();
    let budget = BLOCK as f64 / SR as f64 * 1e9;
    assert!((quickest as f64) < budget / 10.0, "the lane pass took {quickest} ns of the block's {budget:.0}");
    std::fs::remove_dir_all(pkg).ok();
}

/// A song of `up` over the lane, an instrument after it, and the data of a long note.
fn kept(kernel: &str) -> (Ir, DataSet) {
    let ir = voices("e", json!([{"id": "e", "op": "kernel", "kernel": format!("@test/t:{kernel}"), "inputs": {"notes": "n"}}]));
    (ir, lane(json!({"clips": [{"start": 0, "length": 16, "notes": [{"pitch": 60, "start": 0, "length": 8}]}]})))
}

/// The instrument's notes held, and whether each is one a lane or note effect has sounding, after
/// 8 blocks of `from`, then 8 of `to` activated over it.
fn activated(from: &Ir, to: &Ir, data: &DataSet, pk: &PackageSet) -> [(usize, bool); 2] {
    let (fa, fb) = (check(from, pk).unwrap(), check(to, pk).unwrap());
    let mut r = Renderer::prepare(&fa, data, pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = live::Rig::of(&fa, data, r);
    rig.cmd(crate::transport::Cmd::Play);
    rig.run(8 * BLOCK);
    let cur = |rig: &live::Rig| {
        let r = rig.e.current().unwrap();
        (r.held_notes(), r.gates_are_owned())
    };
    let before = cur(&rig);
    let r = rig.prepare(&fb, data, pk);
    rig.send(crate::engine::Msg::Activate { renderer: Box::new(r) });
    rig.run(8 * BLOCK);
    [before, cur(&rig)]
}

// RENDER-26a: a note effect kept by its address keeps its kernel and the notes it has sounding: the
// instrument after it, kept too, holds its note, which the guard still owns.
#[test]
fn a_kept_note_effect_keeps_its_notes_sounding() {
    let (pk, pkg) = with_effects(&[]);
    let (ir, data) = kept("up");
    assert_eq!(activated(&ir, &ir, &data, &pk), [(1, true), (1, true)]);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-26a: a replaced note effect (another kernel, or the instrument reading the lane itself, as
// Bypass does) changes its instrument's address: the instrument, kept, lets go of its notes.
#[test]
fn a_replaced_note_effect_releases_its_kept_instrument() {
    let (pk, pkg) = with_effects(&[]);
    let (ir, data) = kept("up");
    let (other, _) = kept("script");
    assert_eq!(activated(&ir, &other, &data, &pk), [(1, true), (0, true)], "another note effect");
    let bypassed = voices("n", json!([]));
    assert_eq!(activated(&ir, &bypassed, &data, &pk), [(1, true), (0, true)], "bypassed");
    std::fs::remove_dir_all(pkg).ok();
}

/// A song of two tracks' worth of notes through note effects into instruments: `up` into a poly, and
/// `up` then `up` again into another, summed.
fn busy() -> (Ir, DataSet) {
    let body = |id: &str| {
        json!({"out": "amp", "nodes": [
            {"id": "p", "op": "lane", "lane": "pitch"},
            {"id": "g", "op": "lane", "lane": "gate"},
            {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
            {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}], "id": id})
    };
    let ir = song(
        json!([{"id": "n", "op": "lane", "lane": "notes"},
            {"id": "e", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "n"}},
            {"id": "f", "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": "e", "by": "m"}},
            {"id": "m", "op": "const", "value": -19},
            {"id": "v", "op": "poly", "events": "e", "voices": 4, "body": body("v")},
            {"id": "w", "op": "poly", "events": "f", "voices": 4, "body": body("w")},
            {"id": "s", "op": "kernel", "kernel": "std:add", "inputs": {"a": "v", "b": "w"}}]),
        "s",
        json!([{"key": "notes", "kind": "events<note>"}]),
    );
    let ns: Vec<Value> = (0..24).map(|k| json!({"pitch": 48 + (k * 5) % 24, "start": k as f64 * 0.125, "length": 0.3})).collect();
    (ir, lane(json!({"clips": [{"start": 0, "length": 8, "notes": ns}]})))
}

// RENDER-41, RENDER-2: note effects run on the lane pass's one thread, their ids from the one
// counter, so a song through them gives the same bits on 1 to 7 threads.
#[test]
fn a_note_effect_plays_the_same_bits_on_1_to_7_threads() {
    let (pk, pkg) = with_effects(&[]);
    let (ir, data) = busy();
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let one = Renderer::new(&f, &data, &pk, SR).unwrap().render(1.5).unwrap();
    assert!(one.iter().any(|x| x[0].abs() > 0.1), "it sounds");
    for threads in 2..=7 {
        let mut t = spread(threads);
        let out = Renderer::new(&f, &data, &pk, SR).unwrap().render_on(1.5, &mut t.runner).unwrap();
        assert!(out == one, "{threads} threads: the same bits");
    }
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-41, RENDER-6: played live from 0, a song through note effects is its export, bit for bit.
#[test]
fn a_note_effect_plays_live_from_0_as_offline() {
    let (pk, pkg) = with_effects(&[]);
    let (ir, data) = busy();
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let offline = Renderer::new(&f, &data, &pk, SR).unwrap().render(1.5).unwrap();
    let mut r = Renderer::prepare(&f, &data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = live::Rig::of(&f, &data, r);
    rig.cmd(crate::transport::Cmd::Play);
    let heard = rig.run(offline.len());
    assert!(heard == offline, "live from 0 is the export");
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-28, RENDER-41: once a thread has played a block, the lane pass with note effects allocates
// and frees nothing: notes in and out of the guards, live keys, a seek, a loop's wraps, Stop, Stop
// again while stopped, an activation that keeps the note effects and one that replaces one, a panic,
// and Play again.
#[test]
fn note_effects_allocate_nothing_live() {
    let (pk, pkg) = with_effects(&[]);
    let (ir, data) = busy();
    let f = check(&ir, &pk).unwrap_or_else(|e| panic!("{e:?}"));
    let mut r = Renderer::prepare(&f, &data, &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh).unwrap();
    r.set_strict(false);
    r.set_transport(Transport::new(0, false));
    let mut rig = live::Rig::of(&f, &data, r);
    rig.cmd(Cmd::Play);
    rig.run(BLOCK);
    let same = rig.prepare(&f, &data, &pk);
    let mut other = ir.clone();
    other.modules[0].nodes[2] =
        serde_json::from_value(json!({"id": "f", "op": "kernel", "kernel": "@test/t:beat", "inputs": {"notes": "e", "by": "m"}})).unwrap();
    let replaced = rig.prepare(&check(&other, &pk).unwrap(), &data, &pk);
    let g = rig.e.current().unwrap().generation();
    let key = |on: bool| match on {
        true => LiveEv::On { source: 1, handle: 1, pitch: 50.0, velocity: 0.9 },
        false => LiveEv::Off { source: 1, handle: 1 },
    };
    let msgs = [
        Msg::Live { generation: g, slot: 0, ev: key(true) },
        Msg::Transport(Cmd::Seek(0.5)),
        Msg::Transport(Cmd::Loop(Some((0.0, 1.0)))),
        Msg::Activate { renderer: Box::new(same) },
        Msg::Live { generation: g, slot: 0, ev: key(false) },
        Msg::Transport(Cmd::Stop),
        Msg::Transport(Cmd::Stop),
        Msg::Activate { renderer: Box::new(replaced) },
        Msg::Panic,
        Msg::Transport(Cmd::Play),
    ];
    let live::Rig { e, inbox, .. } = &mut rig;
    let mut kept: Vec<Retired> = Vec::with_capacity(64);
    let mut out = vec![[0.0; 2]; 300];
    let ((), allocs) = super::alloc_count::during(|| {
        for m in msgs {
            inbox.push_back(Envelope { seq: 0, msg: m });
            for _ in 0..20 {
                let mut retire = |r: Retired| {
                    if kept.len() == kept.capacity() {
                        return Err(r);
                    }
                    kept.push(r);
                    Ok(())
                };
                e.process(&mut out, &mut || inbox.pop_front(), &mut retire, CallbackStamp::default());
            }
        }
    });
    assert_eq!(allocs, 0);
    assert_eq!(e.activations(), 2);
    std::fs::remove_dir_all(pkg).ok();
}

// RENDER-41: the lane pass runs on one thread before the others start, so its note effects must
// take a small part of a block: eight in a chain over a busy lane, the whole pass at most a tenth of
// the block's 2.7 ms (judged on its quickest of 200 blocks; nightly: it judges speed).
#[test]
fn eight_note_effects_take_a_small_part_of_a_block() {
    let (pk, pkg) = with_effects(&[]);
    let mut nodes = vec![json!({"id": "n", "op": "lane", "lane": "notes"})];
    for k in 0..8 {
        let from = if k == 0 { "n".to_string() } else { format!("e{}", k - 1) };
        nodes.push(json!({"id": format!("e{k}"), "op": "kernel", "kernel": "@test/t:up", "inputs": {"notes": from, "by": "z"}}));
    }
    nodes.extend([
        json!({"id": "z", "op": "const", "value": 0}),
        json!({"id": "k", "op": "kernel", "kernel": "@test/t:k", "inputs": {"notes": "e7"}}),
    ]);
    let ir = song(Value::Array(nodes), "k", json!([{"key": "notes", "kind": "events<note>"}]));
    let ns: Vec<Value> = (0..400).map(|k| json!({"pitch": 40 + k % 40, "start": k as f64 * 0.01, "length": 0.05})).collect();
    let data = lane(json!({"clips": [{"start": 0, "length": 8, "notes": ns}]}));
    let mut r = Renderer::new(&check(&ir, &pk).unwrap(), &data, &pk, SR).unwrap();
    let mut runner = Runner::one();
    let quickest = (0..200).map(|k| r.gauge_block(k * BLOCK as u64, &mut runner, crate::gauge::How::Parts).unwrap().pre).min().unwrap();
    let budget = BLOCK as f64 / SR as f64 * 1e9;
    assert!((quickest as f64) < budget / 10.0, "the lane pass took {quickest} ns of the block's {budget:.0}");
    std::fs::remove_dir_all(pkg).ok();
}

// ABI-15: a note effect's manifest may give its `fan`, 1 to 8 (1 when left out), which sizes its
// room; no other kernel may, and a kernel that gives notes adds no latency, in seconds either.
#[test]
fn a_fan_is_a_note_effects_alone() {
    let module = module_v1(&format!("{}\n{}", probe_wat(""), up_wat("up")));
    let load = |k: &str, up: &str| {
        let ports = format!(
            "inputs = [\"notes: events<note>\"]\noutputs = [\"out: audio<1>\"]\n{k}\n\n[[kernel]]\nname = \"up\"\n\
             inputs = [\"notes: events<note>\", \"by: control<semitones> = 12\"]\noutputs = [\"out: events<note>\"]\n{up}"
        );
        let dir = package_with(&module, &ports);
        let mut pk = PackageSet::default();
        let loaded = pk.load(&dir).map(|_| ());
        let got = loaded.map(|()| m2_ir::Kernels::kernel(&pk, "@test/t:up").map(|s| s.fan));
        std::fs::remove_dir_all(dir).ok();
        got.map_err(|e| format!("{e:#}"))
    };
    assert_eq!(load("", ""), Ok(Some(1)));
    assert_eq!(load("", "fan = 4"), Ok(Some(4)));
    for (k, up, says) in [
        ("", "fan = 0", "`fan` is how many notes it may give for each it takes, 1 to 8, not 0"),
        ("", "fan = 9", "1 to 8, not 9"),
        ("fan = 2", "", "kernel `@test/t:k` gives a `fan`, which only a kernel that gives notes has"),
        ("", "latency_s = 0.01", "a kernel that gives notes adds no latency"),
    ] {
        let e = load(k, up).unwrap_err();
        assert!(e.contains(says), "{k} {up}: {e}");
    }
}

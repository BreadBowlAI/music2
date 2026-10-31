//! Event ports for kernels (ABI-8): a kernel with an `events<note>` input gets its lane's events
//! each block, in kernel ABI v1's reserved shape, read-only, and never more than its room without
//! saying so.

use m2_song::{LaneData, LaneKind};

use super::goldens::addr;
use super::*;
use crate::fault::{Code, Subject};
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
// its cap (1024) and where its events start (just after it, at 16), then 24 bytes an event: the
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
            cap: 1024.0,
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
    assert_eq!((blocks[2].count, blocks[2].cap), (0.0, 1024.0), "a block with none");
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

/// 600 notes that start at frame 0 and end at the next block's frame 10, and 600 that start at its
/// frame 22: 1200 events in that block, past the kernel's 1024.
fn crowded() -> DataSet {
    let notes: Vec<Value> = (0..1200)
        .map(|k| {
            let (start, length) = if k < 600 { (0.0, 0.00575) } else { (0.00625, 0.5) };
            json!({"pitch": k % 128, "start": start, "length": length})
        })
        .collect();
    lane(json!({"clips": [{"start": 0, "length": 1, "notes": notes}]}))
}

// ABI-8: past its 1024 a kernel keeps every event that isn't an On, and the latest Ons are left
// out. Never silently: an offline render fails, naming the sound and how many came.
#[test]
fn past_its_room_an_export_fails_in_words() {
    // A lane's buffer at its smallest fits: only a lane whose data needs more brings more.
    let smallest = sched::Caps::FLOOR.events() + sched::LIVE_ROOM + sched::WHEEL_EVENTS;
    assert!(smallest == 812 && smallest < NOTES_CAP);
    let pkg = probe("");
    let pk = load(&pkg);
    let mut r = Renderer::new(&check(&probed(), &pk).unwrap(), &crowded(), &pk, SR).unwrap();
    assert_eq!(read(r.render_block(0).unwrap()).count, 600.0);
    let e = r.render_block(BLOCK as u64).unwrap_err();
    assert_eq!(e.downcast_ref::<crate::fault::Failed>().map(|f| f.fault.code), Some(Code::NotesDropped));
    assert!(
        format!("{e:#}")
            .contains("song/k got 1200 note events in one block, more than the 1024 its input `notes` holds: 176 notes were left out"),
        "{e:#}"
    );
    std::fs::remove_dir_all(pkg).ok();
}

// ABI-8: live, the sound plays on with what fits, and says so once a plan through the fault ring.
#[test]
fn past_its_room_live_it_plays_on_and_says_so_once() {
    let pkg = probe("");
    let pk = load(&pkg);
    let flat = check(&probed(), &pk).unwrap();
    let node = flat.nodes.iter().position(|n| n.name == "song/k").unwrap() as u32;
    let mut r = Renderer::new(&flat, &crowded(), &pk, SR).unwrap();
    r.set_strict(false);
    r.live = true;
    r.render_block(0).unwrap();
    let got = read(r.render_block(BLOCK as u64).unwrap());
    assert_eq!((got.count, got.offs, got.ons), (1024.0, 600.0, 424.0), "every Off, and the earliest Ons");
    assert!(got.events.iter().all(|e| e[0] == 1.0 && e[1] == 10.0), "the Offs first, at their frame: {:?}", got.events);
    let faults = r.info().faults.drain();
    assert_eq!(faults.iter().map(|f| (f.code, f.subject)).collect::<Vec<_>>(), [(Code::NotesDropped, Subject::Node(node))]);
    r.render_block(2 * BLOCK as u64).unwrap();
    assert!(r.info().faults.drain().is_empty(), "said once a plan");
    std::fs::remove_dir_all(pkg).ok();
}

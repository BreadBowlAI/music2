//! Taps (TAP-1 to TAP-5, TAP-8): a live plan's meters and frame rings. A meter reads its points
//! after the graph ran, only while watched; a ring copies its point's frames while it points
//! somewhere. Taps read buffers only, so no bit changes; offline renders have none.

use super::cell::origin;
use super::live::flat;
use super::*;
use crate::taps::{RING, RINGS};

/// `kick` (a 110 Hz sine) and `bass` (a 55 Hz sine at a third), added in `song`; the kick muted in
/// the song or not. With `alone`, the song's output is that track, read straight.
fn two(muted: bool, alone: Option<&str>) -> Ir {
    let song = match alone {
        Some(k) => json!({"key": "song", "nodes": [{"id": "x", "op": "import", "module": k, "export": "out"}], "exports": {"out": "x"}}),
        None => json!({"key": "song", "nodes": [
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "b", "op": "import", "module": "bass", "export": "out"},
            {"id": "m", "op": "kernel", "kernel": "std:add", "inputs": {"a": "k", "b": "b"}}], "exports": {"out": "m"}}),
    };
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "muted": muted, "nodes": [
            {"id": "f", "op": "const", "value": 110, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}], "exports": {"out": "o"}},
        {"key": "bass", "track": true, "nodes": [
            {"id": "f", "op": "const", "value": 55, "unit": "hz"},
            {"id": "s", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "t", "op": "const", "value": 0.3},
            {"id": "o", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "s", "by": "t"}}], "exports": {"out": "o"}},
        song]}))
    .unwrap()
}

/// A live plan of `ir`, as the server prepares one: fresh, playing from 0.
fn prepared(ir: &Ir) -> Renderer {
    let pk = std_packages();
    Renderer::prepare(&flat(ir), &DataSet::empty(), &pk, SR, &mut audio::Cache::default(), &m2_plugins::Owner::discard(), Basis::Fresh)
        .unwrap()
}

/// [`prepared`], with a meter for each track at its overlay sites, watched.
fn metered(ir: &Ir) -> (Renderer, Arc<TapSlots>) {
    let mut r = prepared(ir);
    let f = flat(ir);
    let meters: Vec<Vec<TapAt>> =
        ["kick", "bass"].iter().map(|k| (0..f.overlays.len()).filter(|&s| f.overlays[s].module == *k).map(TapAt::Site).collect()).collect();
    r.tap(&meters).unwrap();
    let taps = r.info().taps.clone().expect("a live plan has taps");
    taps.watch(true);
    (r, taps)
}

fn offline(ir: &Ir) -> Renderer {
    Renderer::new(&flat(ir), &DataSet::empty(), &std_packages(), SR).unwrap()
}

/// The next `blocks` blocks of `r`.
fn blocks(r: &mut Renderer, blocks: usize) -> Vec<[f32; 2]> {
    let mut out = vec![];
    for _ in 0..blocks {
        let clock = r.clock();
        out.extend_from_slice(r.render_block(clock).unwrap());
    }
    out
}

/// The largest `|x|` of channel `c`.
fn top(x: &[[f32; 2]], c: usize) -> f32 {
    x.iter().fold(0.0f32, |m, f| m.max(f[c].abs()))
}

fn bits(x: &[[f32; 2]]) -> Vec<[u32; 2]> {
    x.iter().map(|f| [f[0].to_bits(), f[1].to_bits()]).collect()
}

/// Every node of `ir`'s graph with sound.
fn sounding(ir: &Ir) -> Vec<usize> {
    let f = flat(ir);
    (0..f.nodes.len()).filter(|&i| matches!(f.nodes[i].outputs.first(), Some(m2_ir::Kind::Audio(_)))).collect()
}

// TAP-2: a live plan with every tap on (a meter at every site and every node with sound, watched,
// and every ring pointed) plays the same bits as an offline render, which has no taps.
#[test]
fn taps_change_no_bit() {
    let ir = two(false, None);
    let (mut live, mut off) = (prepared(&ir), offline(&ir));
    assert!(off.info().taps.is_none(), "offline renders have none");
    assert!(off.tap(&[vec![TapAt::Out]]).is_err(), "and take none");
    let f = flat(&ir);
    let mut meters: Vec<Vec<TapAt>> = (0..f.overlays.len()).map(|s| vec![TapAt::Site(s)]).collect();
    meters.extend(sounding(&ir).into_iter().map(|i| vec![TapAt::Node(i)]));
    live.tap(&meters).unwrap();
    let taps = live.info().taps.clone().expect("a live plan has taps");
    taps.watch(true);
    let mut points = vec![TapAt::Out];
    points.extend(sounding(&ir).into_iter().map(TapAt::Node));
    for (ring, &at) in points.iter().cycle().take(RINGS).enumerate() {
        taps.point(ring, Some(at)).unwrap();
    }
    let (a, b) = (live.render(0.5).unwrap(), off.render(0.5).unwrap());
    assert!(top(&a, 0) > 0.5);
    assert!(bits(&a) == bits(&b), "the same bits");
    assert!(taps.take().meters.iter().all(|m| m[0] > 0.0), "every meter read");
}

// TAP-1: each meter holds its channels' largest |x| since it was last taken: the output's two
// channels, and each track's signal where it enters the song.
#[test]
fn a_tap_reports_its_blocks_peak() {
    let (mut r, taps) = metered(&two(false, None));
    let (mut kick, mut bass) = (offline(&two(false, Some("kick"))), offline(&two(false, Some("bass"))));
    for n in [1, 3, 1, 7] {
        let out = blocks(&mut r, n);
        let (k, b) = (blocks(&mut kick, n), blocks(&mut bass, n));
        let p = taps.take();
        assert_eq!(p.out, [top(&out, 0), top(&out, 1)], "the output, over {n} blocks");
        assert_eq!(p.meters, [vec![top(&k, 0)], vec![top(&b, 0)]], "the kick and the bass, over {n} blocks");
        assert!(p.out[0] > 0.5 && p.meters[1][0] > 0.0);
    }
    let p = taps.take();
    assert_eq!((p.out, p.meters), ([0.0; 2], vec![vec![0.0], vec![0.0]]), "taken: 0 until the next block");
}

// MON-1, MON-5, TAP-5: a track's meter reads after its mute, in the song or while listening:
// silence, while the other track's moves.
#[test]
fn a_muted_tracks_tap_reads_silence() {
    let mut listening = metered(&two(false, None));
    let kick = listening.0.info().overlays.iter().position(|o| o.0 == "kick").unwrap();
    let mut values = vec![1.0; listening.0.info().overlays.len()];
    values[kick] = 0.0;
    listening.0.set_overlays(&values);
    for (mut r, taps) in [metered(&two(true, None)), listening] {
        let out = blocks(&mut r, 4);
        let p = taps.take();
        assert_eq!(p.meters[0], [0.0], "muted: silence");
        let bass = p.meters[1][0];
        assert!(bass > 0.0, "the bass moves");
        assert_eq!(p.out, [top(&out, 0), top(&out, 1)]);
        assert_eq!(p.out[0], bass, "the output is the bass alone");
    }
}

// TAP-4: unwatched, a plan raises no peak and its rings copy nothing; watched again, its meters
// read from the next block.
#[test]
fn unwatched_taps_read_nothing() {
    let (mut r, taps) = metered(&two(false, None));
    taps.watch(false);
    blocks(&mut r, 4);
    let p = taps.take();
    assert_eq!((p.out, p.meters), ([0.0; 2], vec![vec![0.0], vec![0.0]]), "no peak while unwatched");
    let mut frames = vec![];
    assert!((0..RINGS).all(|ring| taps.frames(ring, 0, RING, &mut frames).is_none()), "no ring points anywhere");
    taps.watch(true);
    blocks(&mut r, 1);
    assert!(taps.take().meters.iter().all(|m| m[0] > 0.0), "watched again, from the next block");
}

/// `kick` (a 110 Hz sine) into a fader, `std:gain` reading a cell written at site 2 at `db`.
fn faded(db: f64) -> Ir {
    serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"}, "modules": [
        {"key": "kick", "track": true, "nodes": [
            {"id": "f", "op": "const", "value": 110, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}], "exports": {"out": "o"}},
        {"key": "song", "nodes": [
            {"id": "k", "op": "import", "module": "kick", "export": "out"},
            {"id": "d", "op": "const", "value": db, "unit": "db", "origin": {"site": 2, "arg": ["0"]}},
            {"id": "g", "op": "kernel", "kernel": "std:gain", "inputs": {"in": "k", "db": "d"}}], "exports": {"out": "g"}}]}))
    .unwrap()
}

// TAP-5: a meter at a fader reads after it. The fader moved to −12 dB while the song plays (a cell
// write, no new plan) moves the meter down by 12 dB, once its glide is over; the meter at the
// track's site, before the fader, stays where it was.
#[test]
fn a_fader_at_minus_12_db_moves_its_meter_by_12_db() {
    let ir = faded(0.0);
    let f = flat(&ir);
    let fader = f.nodes.iter().position(|n| n.name == "song/g").unwrap();
    let mut r = prepared(&ir);
    r.tap(&[vec![TapAt::Node(fader)], vec![TapAt::Site(0)]]).unwrap();
    let taps = r.info().taps.clone().unwrap();
    taps.watch(true);
    let db = |x: f32| 20.0 * (x as f64).log10();
    blocks(&mut r, 16);
    taps.take();
    blocks(&mut r, 8);
    let before = taps.take();
    r.cell_slots().set(r.cell_map().cells(&origin(2)), -12.0, 1);
    // The fader's glide.
    blocks(&mut r, 64);
    taps.take();
    blocks(&mut r, 8);
    let after = taps.take();
    let moved = db(after.meters[0][0]) - db(before.meters[0][0]);
    assert!((moved + 12.0).abs() < 0.01, "the fader's meter moved {moved:.3} dB");
    assert!((db(after.meters[1][0]) - db(before.meters[1][0])).abs() < 0.01, "the track's own sound didn't move: {before:?} {after:?}");
    assert!((db(after.out[0]) - db(after.meters[0][0])).abs() < 1e-6, "the output is the fader's");
}

// TAP-8, TAP-3: a ring pointed at a node copies that node's output, frame for frame; one at the
// output, the song's; pointed at another node, it copies only that one's frames from its next block
// on; pointed nowhere, nothing.
#[test]
fn a_rings_frames_equal_the_nodes_output() {
    let ir = two(false, None);
    let f = flat(&ir);
    let node = |name: &str| f.nodes.iter().position(|n| n.name == name).unwrap();
    let (mut r, taps) = metered(&ir);
    assert_eq!(taps.point(1, Some(TapAt::Node(node("bass/o")))).unwrap(), 1, "the bass is mono");
    assert_eq!(taps.point(0, Some(TapAt::Out)).unwrap(), 2);
    assert!(taps.point(2, Some(TapAt::Node(node("bass/t")))).is_err(), "a constant makes no sound");
    assert!(taps.point(RINGS, Some(TapAt::Out)).is_err(), "no such ring");
    let mut frames = vec![];
    assert_eq!(taps.frames(1, 0, 1024, &mut frames), None, "not taken until the next block");
    let (mut kick, mut bass) = (offline(&two(false, Some("kick"))), offline(&two(false, Some("bass"))));
    let out = blocks(&mut r, 10);
    let (b, k) = (blocks(&mut bass, 10), blocks(&mut kick, 10));
    let both = |x: &[[f32; 2]]| bits(&x.iter().map(|f| [f[0], f[0]]).collect::<Vec<_>>());
    assert_eq!(taps.frames(1, 0, 1 << 20, &mut frames), Some(0), "every frame since its point was taken");
    assert_eq!(bits(&frames), both(&b), "the bass, on both sides");
    assert_eq!(taps.frames(1, 0, 256, &mut frames), Some(1024), "the last 256");
    assert_eq!(bits(&frames), both(&b[1024..]));
    assert_eq!(taps.frames(0, 0, 1 << 20, &mut frames), Some(0));
    assert_eq!(bits(&frames), bits(&out), "the output's ring is what played");
    assert_eq!(taps.frames(0, 1000, 1 << 20, &mut frames), Some(1000), "from a frame on");
    assert_eq!(frames.len(), 10 * BLOCK - 1000);
    // Pointed at the kick: only its frames, from the block after.
    taps.point(1, Some(TapAt::Node(node("kick/o")))).unwrap();
    let k2 = blocks(&mut kick, 3);
    blocks(&mut r, 3);
    assert_eq!(taps.frames(1, 0, 1 << 20, &mut frames), Some(10 * BLOCK as u64), "from its point on");
    assert_eq!(bits(&frames), both(&k2), "the kick's, none of the bass's");
    assert_eq!(k.len(), 10 * BLOCK);
    taps.point(1, None).unwrap();
    blocks(&mut r, 1);
    assert_eq!(taps.frames(1, 0, 1 << 20, &mut frames), None, "pointed nowhere");
}

// TAP-3: a ring keeps its last frames, all but the room of the block being written: a reader
// asking for older ones gets what is still there.
#[test]
fn a_ring_keeps_its_last_frames() {
    let (mut r, taps) = metered(&two(false, None));
    taps.point(0, Some(TapAt::Out)).unwrap();
    let blocks_in = RING / BLOCK + 5;
    let out = blocks(&mut r, blocks_in);
    let mut frames = vec![];
    let first = taps.frames(0, 0, 1 << 20, &mut frames).unwrap();
    let end = (blocks_in * BLOCK) as u64;
    assert_eq!(first, end + BLOCK as u64 - RING as u64, "the oldest a block's write can't reach");
    assert_eq!(bits(&frames), bits(&out[first as usize..]));
}

// AGENT-7: a copy tap at a site copies the track's own sound where it enters the song, one at a
// node copies that node, and the output's the output: each block from the one that holds the frame
// asked, none before it. Nothing in this song is late.
#[test]
fn copy_taps_copy_each_block_from_the_frame_asked() {
    let ir = two(false, None);
    let f = flat(&ir);
    let kick = f.overlays.iter().position(|s| s.module == "kick").unwrap();
    let bass = f.nodes.iter().position(|n| n.name == "bass/o").unwrap();
    let mut r = Copying::new(offline(&ir), &[TapAt::Site(kick), TapAt::Node(bass), TapAt::Out], 3 * BLOCK as u64 + 5).unwrap();
    let mut out = vec![[0.0f32; 2]; 6 * BLOCK];
    r.process_on(&mut out, &mut Runner::one()).unwrap();
    let (k, b) = (blocks(&mut offline(&two(false, Some("kick"))), 6), blocks(&mut offline(&two(false, Some("bass"))), 6));
    let c = r.copies();
    let starts: Vec<u64> = (3..6).map(|n| (n * BLOCK) as u64).collect();
    assert_eq!(c.blocks(), &starts[..], "from the block that holds the frame asked");
    assert_eq!((c.taps(), c.channels(0), c.channels(1), c.channels(2)), (3, 1, 1, 2));
    assert_eq!((c.late(0), c.late(1), c.late(2)), (0, 0, 0));
    for (n, &at) in c.blocks().iter().enumerate() {
        let at = at as usize;
        let mono = |x: &[[f32; 2]]| x[at..at + BLOCK].iter().map(|f| f[0].to_bits()).collect::<Vec<u32>>();
        let copied = |t: usize| c.block(t, n).iter().map(|x| x.to_bits()).collect::<Vec<u32>>();
        assert_eq!(copied(0), mono(&k), "the kick's own sound, block {n}");
        assert_eq!(copied(1), mono(&b), "the bass node, block {n}");
        let both: Vec<u32> = (0..2).flat_map(|ch| out[at..at + BLOCK].iter().map(move |f| f[ch].to_bits())).collect();
        assert_eq!(copied(2), both, "the output, one channel after the other, block {n}");
    }
    c.clear();
    assert!(c.blocks().is_empty(), "cleared");
}

// AGENT-7: only an offline plan gets copy taps. A live plan refuses them, and so does a tap at a
// node with no audio output or a site the song hasn't got.
#[test]
fn copy_taps_are_offline_only() {
    let ir = two(false, None);
    assert!(Copying::new(prepared(&ir), &[TapAt::Out], 0).is_err(), "a live plan");
    let f = flat(&ir);
    let constant = f.nodes.iter().position(|n| n.name == "bass/t").unwrap();
    assert!(Copying::new(offline(&ir), &[TapAt::Node(constant)], 0).is_err(), "a constant is no sound");
    assert!(Copying::new(offline(&ir), &[TapAt::Site(f.overlays.len())], 0).is_err(), "no such site");
    assert!(Copying::new(offline(&ir), &[TapAt::Node(f.nodes.len())], 0).is_err(), "no such node");
}

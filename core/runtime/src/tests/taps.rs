//! Render taps (8b.8): a live plan taps the song's output and each track where its audio enters
//! another module, after its mute. Taps read buffers only, so no bit changes; offline renders have
//! none.

use super::live::flat;
use super::*;

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

// A live plan with taps plays the same bits as an offline render, which has none.
#[test]
fn taps_change_no_bit() {
    let ir = two(false, None);
    let (mut live, mut off) = (prepared(&ir), offline(&ir));
    assert!(off.info().taps.is_none(), "offline renders have none");
    let taps = live.info().taps.clone().expect("a live plan has taps");
    let mut tracks: Vec<&str> = taps.tracks().collect();
    tracks.sort_unstable();
    assert_eq!(tracks, ["bass", "kick"]);
    let (a, b) = (live.render(0.5).unwrap(), off.render(0.5).unwrap());
    assert!(top(&a, 0) > 0.5);
    assert!(bits(&a) == bits(&b), "the same bits");
}

// Each tap holds its channels' largest |x| since it was last taken: the output's two channels, and
// each track's signal where it enters the song.
#[test]
fn a_tap_reports_its_blocks_peak() {
    let mut r = prepared(&two(false, None));
    let taps = r.info().taps.clone().unwrap();
    let (mut kick, mut bass) = (offline(&two(false, Some("kick"))), offline(&two(false, Some("bass"))));
    for n in [1, 3, 1, 7] {
        let out = blocks(&mut r, n);
        let (k, b) = (blocks(&mut kick, n), blocks(&mut bass, n));
        let p = taps.take();
        assert_eq!(p.out, [top(&out, 0), top(&out, 1)], "the output, over {n} blocks");
        assert_eq!(p.track("kick"), Some(&[top(&k, 0)][..]), "the kick, over {n} blocks");
        assert_eq!(p.track("bass"), Some(&[top(&b, 0)][..]));
        assert!(p.out[0] > 0.5 && p.track("bass").unwrap()[0] > 0.0);
    }
    let p = taps.take();
    assert_eq!((p.out, p.track("kick"), p.track("bass")), ([0.0; 2], Some(&[0.0][..]), Some(&[0.0][..])), "taken: 0 until the next block");
}

// MON-1, MON-5: a track's tap reads after its mute, in the song or while listening: silence, while
// the other track's moves.
#[test]
fn a_muted_tracks_tap_reads_silence() {
    let in_song = prepared(&two(true, None));
    let mut listening = prepared(&two(false, None));
    let kick = listening.info().overlays.iter().position(|o| o.0 == "kick").unwrap();
    let mut values = vec![1.0; listening.info().overlays.len()];
    values[kick] = 0.0;
    listening.set_overlays(&values);
    for mut r in [in_song, listening] {
        let taps = r.info().taps.clone().unwrap();
        let out = blocks(&mut r, 4);
        let p = taps.take();
        assert_eq!(p.track("kick"), Some(&[0.0][..]), "muted: silence");
        let bass = p.track("bass").unwrap()[0];
        assert!(bass > 0.0, "the bass moves");
        assert_eq!(p.out, [top(&out, 0), top(&out, 1)]);
        assert_eq!(p.out[0], bass, "the output is the bass alone");
    }
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
    let mut r = offline(&ir);
    r.copy_taps(&[TapAt::Site(kick), TapAt::Node(bass), TapAt::Out], 3 * BLOCK as u64 + 5).unwrap();
    let out = blocks(&mut r, 6);
    let (k, b) = (blocks(&mut offline(&two(false, Some("kick"))), 6), blocks(&mut offline(&two(false, Some("bass"))), 6));
    let c = r.copies().expect("copies");
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
    assert!(prepared(&ir).copy_taps(&[TapAt::Out], 0).is_err(), "a live plan");
    let f = flat(&ir);
    let constant = f.nodes.iter().position(|n| n.name == "bass/t").unwrap();
    assert!(offline(&ir).copy_taps(&[TapAt::Node(constant)], 0).is_err(), "a constant is no sound");
    assert!(offline(&ir).copy_taps(&[TapAt::Site(f.overlays.len())], 0).is_err(), "no such site");
    assert!(offline(&ir).copy_taps(&[TapAt::Node(f.nodes.len())], 0).is_err(), "no such node");
}

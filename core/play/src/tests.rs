use m2_ir::Origin;
use m2_runtime::BLOCK;

use super::*;

fn build(nodes: serde_json::Value, out: &str, rate: u32) -> Renderer {
    let ir: m2_ir::Ir = serde_json::from_value(serde_json::json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "nodes": nodes, "exports": {"out": out},
            "frames": [{"site": 1, "component": "test:level"}]}]}))
    .unwrap();
    let mut packages = m2_runtime::PackageSet::default();
    packages.load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let flat = m2_ir::check(&ir, &packages).unwrap();
    Renderer::new(&flat, &m2_song::DataSet::empty(), &packages, rate).unwrap()
}

/// A renderer of a 1 kHz sine at `rate`.
pub(crate) fn sine(rate: u32) -> Renderer {
    let nodes = serde_json::json!([
        {"id": "f", "op": "const", "value": 1000, "unit": "hz"},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}}]);
    build(nodes, "o", rate)
}

/// The value written at site 2: a level declared smooth.
fn level() -> Origin {
    Origin { site: 2, arg: vec!["0".into()], ..Origin::default() }
}

/// Where a gesture on `level()` writes in `r`: the cells of that value, through `r`'s slots (RENDER-23).
fn level_of(r: &Renderer) -> CellTarget {
    CellTarget { slots: r.cell_slots(), cells: r.cell_map().cells(&level()).into() }
}

/// A 1 kHz sine at 48 kHz times `level()`, 0.5 as built.
fn leveled() -> Renderer {
    let nodes = serde_json::json!([
        {"id": "f", "op": "const", "value": 1000, "unit": "hz"},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
        {"id": "g", "op": "const", "value": 0.5, "origin": {"site": 2, "arg": ["0"], "control": {"frame": 0, "slot": "level", "smooth": true}}},
        {"id": "out", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "g"}}]);
    build(nodes, "out", 48_000)
}

/// Play `frames` through the callback in device buffers of uneven sizes, stereo.
fn hear(d: &mut Driver, frames: usize) -> Vec<[f32; 2]> {
    let mut out = vec![];
    for n in [1, 7, 128, 300, 64, 1000].iter().cycle() {
        let n = (*n).min(frames - out.len());
        if n == 0 {
            break;
        }
        let mut data = vec![0.0; n * 2];
        d.fill(&mut data, 2);
        out.extend(data.chunks(2).map(|f| [f[0], f[1]]));
    }
    out
}

/// Upward zero crossings per second, near enough the frequency.
fn freq(x: &[[f32; 2]], rate: u32) -> f64 {
    let ups = x.windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
    ups as f64 * rate as f64 / x.len() as f64
}

// RENDER-28: a cell value never uses the queue, so a full queue neither drops nor delays it: it
// is in the renderer's slot at once, and the renderer plays it from its next block.
#[test]
fn a_cell_write_never_waits_for_the_queue() {
    let (tx, rx) = bounded(1);
    let player =
        Player { tx, shared: Arc::default(), next_seq: Arc::default(), cells: Arc::default(), device_rate: 48_000, device: "test".into() };
    let mut r = leveled();
    let target = level_of(&r);
    assert_eq!(target.cells.len(), 1);
    player.cells.sending(&mut r);
    player.play();
    let t = Instant::now();
    let seq = player.set_cells(&target, 0.25);
    assert!(t.elapsed() < Duration::from_millis(5), "never waits");
    assert!(seq > CELL_SEQ, "a cell write's seq");
    assert_eq!(rx.len(), 1, "only the play is queued");
    r.render_block(0, false).unwrap();
    r.render_block(BLOCK as u64, false).unwrap();
    assert_eq!(r.cell_value(target.cells[0]), Some(0.25));
    assert_eq!(r.take_heard(), Some((0, seq)));
    let next = player.set_cells(&target, 0.75);
    assert!(next > seq && rx.len() == 1);
    r.render_block(2 * BLOCK as u64, false).unwrap();
    assert_eq!((r.cell_value(target.cells[0]), r.take_heard()), (Some(0.75), Some((seq, next))));
}

// RENDER-22, RENDER-22a, RENDER-33: a cell write while playing is heard at the next block line,
// ramped, and live playback stays the offline render with the same write at the same block, for
// any device buffer sizes; the player reports when the engine heard it.
#[test]
fn a_cell_write_plays_live_as_offline() {
    let (player, mut d) = Player::driven(48_000);
    let r = leveled();
    let (generation, target) = (r.generation(), level_of(&r));
    let cell = target.cells[0];
    player.open(r).unwrap();
    player.play();
    let mut live = hear(&mut d, 10 * BLOCK);
    let seq = player.set_cells(&target, 0.9);
    live.extend(hear(&mut d, 10 * BLOCK));
    let mut offline = leveled();
    let mut expected = vec![[0.0; 2]; 20 * BLOCK];
    offline.process(&mut expected[..10 * BLOCK]).unwrap();
    offline.cell_slots().set(&[cell], 0.9, 1);
    offline.process(&mut expected[10 * BLOCK..]).unwrap();
    assert!(live == expected, "differs from frame {:?}", live.iter().zip(&expected).position(|(a, b)| a != b));
    assert!(peak(&live[10 * BLOCK..11 * BLOCK]) < peak(&live[12 * BLOCK..]), "it ramped up");
    let heard = player.applied(seq).expect("heard");
    assert_eq!((heard.seq, heard.clock), (seq, 10 * BLOCK as u64));
    let e = d.engine().unwrap();
    assert_eq!((e.activations(), e.current().unwrap().generation()), (0, generation), "nothing rebuilt");
}

fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()))
}

// RENDER-28: a sender never waits forever. With the audio thread gone quiet, it gives up after
// half a second, says so, and the player reports itself stalled.
#[test]
fn a_full_queue_stalls_instead_of_hanging() {
    let (tx, _rx) = bounded(1);
    let player =
        Player { tx, shared: Arc::default(), next_seq: Arc::default(), cells: Arc::default(), device_rate: 48_000, device: "test".into() };
    player.play();
    let t = Instant::now();
    let e = player.seek(4.0).unwrap_err();
    assert!(e.downcast_ref::<Stalled>().is_some(), "{e}");
    assert!((STALL..STALL + Duration::from_millis(400)).contains(&t.elapsed()), "{:?}", t.elapsed());
    assert!(player.stalled());
    let t = Instant::now();
    let _ = player.status();
    assert!(t.elapsed() < Duration::from_millis(5), "status never waits");
}

// RENDER-31, RENDER-33: at the device's own rate, live playback is the offline render bit
// for bit, whatever the device's buffer sizes.
#[test]
fn at_the_devices_rate_live_is_the_offline_render() {
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(48_000)).unwrap();
    player.play();
    let live = hear(&mut d, 9_600);
    assert_eq!(live, sine(48_000).render(0.2).unwrap());
    assert_eq!((player.rate(), player.position()), (48_000, 9_600));
}

// RENDER-31: the engine runs at the project's rate, and the device hears it converted to its
// own: a 1 kHz sine is 1 kHz either way, and positions count engine frames.
#[test]
fn the_engine_runs_at_the_projects_rate_on_any_device() {
    for (project, dev) in [(44_100, 48_000), (96_000, 48_000), (48_000, 44_100)] {
        let (player, mut d) = Player::driven(dev);
        player.open(sine(project)).unwrap();
        player.play();
        let heard = hear(&mut d, dev as usize / 2);
        assert_eq!(player.rate(), project);
        let f = freq(&heard[dev as usize / 10..], dev);
        assert!((f - 1000.0).abs() < 5.0, "{project} on {dev}: {f} Hz");
        // Half a second of the device is half a second of the engine, give or take the
        // converter's lookahead (its filter's half, and a chunk pulled at a time) and the
        // engine's block.
        let ahead = project as i64 / 2 - player.position() as i64;
        assert!((-96 - 128..=0).contains(&ahead), "{project} on {dev}: {} frames ahead", -ahead);
        // As loud as it was: no gain from converting.
        let peak = |x: &[[f32; 2]]| x.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
        let engine = peak(&sine(project).render(0.5).unwrap());
        assert!((peak(&heard[dev as usize / 10..]) - engine).abs() < 0.02 * engine, "{project} on {dev}");
    }
}

// A project loaded is where playback is as soon as it is sent: one opened at its start, stopped;
// one replaced (a rate change) at the same beat, playing as before.
#[test]
fn a_loaded_engine_is_where_playback_is_at_once() {
    let (player, mut d) = Player::driven(48_000);
    let mut data = vec![0.0; 256];
    let playing_from_beat_2 = |d: &mut Driver, data: &mut [f32]| {
        player.seek(2.0).unwrap();
        player.play();
        d.fill(data, 2);
        d.fill(data, 2);
        let s = player.status();
        assert!(s.playing && s.beats > 2.0 && s.pending.is_none(), "{s:?}");
        s
    };
    player.open(sine(48_000)).unwrap();
    playing_from_beat_2(&mut d, &mut data);
    player.open(sine(48_000)).unwrap();
    let s = player.status();
    assert_eq!((s.pos, s.beats, s.seconds, s.playing, s.pending), (0, 0.0, 0.0, false, Some(0.0)), "before the audio thread takes it");
    // The callback meets it in the inbox, then takes it at its next start.
    d.fill(&mut data, 2);
    d.fill(&mut data, 2);
    let s = player.status();
    assert_eq!((s.pos, s.playing, s.pending), (0, false, None), "and once it has");
    let before = playing_from_beat_2(&mut d, &mut data);
    player.replace(sine(96_000)).unwrap();
    let s = player.status();
    assert_eq!((s.pos, s.beats, s.playing, s.pending), (2 * before.pos, before.beats, true, Some(before.beats)));
    d.fill(&mut data, 2);
    d.fill(&mut data, 2);
    let s = player.status();
    assert!(s.playing && s.beats > before.beats && s.pending.is_none(), "{s:?}");
    drop(d.drain_retired());
}

// RENDER-9: a seek is the target at once in the status, and the engine's position once taken.
#[test]
fn a_seek_reports_its_target_until_taken() {
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(48_000)).unwrap();
    let mut data = vec![0.0; 256];
    d.fill(&mut data, 2);
    player.seek(2.0).unwrap();
    assert_eq!(player.status().pending, Some(2.0));
    d.fill(&mut data, 2);
    let s = player.status();
    assert_eq!((s.pending, s.pos, s.beats, s.playing), (None, 48_000, 2.0, false), "stopped, it holds the target");
    assert!(player.applied(1).is_some_and(|a| a.clock == 128 && a.frame == 0), "{:?}", player.applied(1));
}

// RENDER-31: a rate change replaces the engine and keeps the beat, the play state and the loop;
// the new engine's frames count at its rate.
#[test]
fn a_replaced_engine_keeps_the_beat_at_its_rate() {
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(48_000)).unwrap();
    player.set_loop(Some((0.0, 64.0))).unwrap();
    player.play();
    let mut data = vec![0.0; 2 * 4800];
    d.fill(&mut data, 2);
    let before = player.status();
    player.replace(sine(96_000)).unwrap();
    assert_eq!(player.position(), 2 * before.pos, "the same beat at once, in 96 kHz frames");
    d.fill(&mut data, 2);
    d.fill(&mut data, 2);
    let after = player.status();
    assert!(after.playing && after.beats > before.beats && player.rate() == 96_000, "{after:?}");
    let e = d.engine().unwrap();
    assert_eq!(e.rate(), 96_000);
    assert!(e.current().unwrap().transport().looping().is_some_and(|l| l.end_beat == 64.0), "the loop is kept");
    assert!(d.fading() || d.drain_retired().len() == 1, "the old engine fades out, then is retired");
}

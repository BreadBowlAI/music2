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
    let player = Player {
        tx,
        shared: Arc::default(),
        next_seq: Arc::default(),
        cells: Arc::default(),
        alive: Arc::default(),
        device_rate: 48_000,
        device: "test".into(),
    };
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
    r.render_block(0).unwrap();
    r.render_block(BLOCK as u64).unwrap();
    assert_eq!(r.cell_value(target.cells[0]), Some(0.25));
    assert_eq!(r.take_heard(), Some((0, seq)));
    let next = player.set_cells(&target, 0.75);
    assert!(next > seq && rx.len() == 1);
    r.render_block(2 * BLOCK as u64).unwrap();
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
    let player = Player {
        tx,
        shared: Arc::default(),
        next_seq: Arc::default(),
        cells: Arc::default(),
        alive: Arc::default(),
        device_rate: 48_000,
        device: "test".into(),
    };
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

/// Every kind of error cpal 0.18.2 reports, and what it means for the player.
const KINDS: [(DeviceErrorKind, DeviceEvent); 14] = {
    use DeviceErrorKind as K;
    use DeviceEvent::{Changed, Gone, Other, Xrun};
    [
        (K::Xrun, Xrun),
        (K::DeviceChanged, Changed),
        (K::StreamInvalidated, Gone),
        (K::DeviceNotAvailable, Gone),
        (K::DeviceBusy, Other),
        (K::HostUnavailable, Other),
        (K::InvalidInput, Other),
        (K::PermissionDenied, Other),
        (K::RealtimeDenied, Other),
        (K::ResourceExhausted, Other),
        (K::UnsupportedConfig, Other),
        (K::UnsupportedOperation, Other),
        (K::BackendError, Other),
        (K::Other, Other),
    ]
};

// RENDER-28 (platform survey-later's bug 1): the device's errors are sorted by kind. An xrun is a
// dropout; a change of output device asks for a new player; only a stream invalidated or a device
// unavailable is gone; every other kind is just counted.
#[test]
fn the_devices_errors_are_sorted_by_kind() {
    for (kind, event) in KINDS {
        assert_eq!(DeviceEvent::of(kind), event, "{kind:?}");
    }
}

// RENDER-28, bug 1: xruns and every other error that isn't a change or a loss of the device are
// counted, and play goes on: the player isn't stalled, it plays the same bits, and it takes its
// messages. Before, one xrun marked it stalled, and the server let it go at the next action,
// which cut every tail.
#[test]
fn an_xrun_is_counted_and_play_goes_on() {
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(48_000)).unwrap();
    player.play();
    let mut live = hear(&mut d, 4_800);
    for _ in 0..3 {
        d.error(DeviceErrorKind::Xrun.into());
    }
    let others: Vec<DeviceErrorKind> = KINDS.iter().filter(|(_, e)| *e == DeviceEvent::Other).map(|(k, _)| *k).collect();
    for kind in &others {
        d.error((*kind).into());
    }
    // With a text cpal formats, as ALSA's unsorted errors carry.
    d.error(DeviceError::with_message(DeviceErrorKind::BackendError, format!("poll failed: {}", 5)));
    assert_eq!(player.device_errors(), DeviceErrors { xruns: 3, other: others.len() as u64 + 1, ..DeviceErrors::default() });
    assert!(!player.stalled() && !player.changed());
    live.extend(hear(&mut d, 4_800));
    assert_eq!(live, sine(48_000).render(0.2).unwrap(), "it played on, bit for bit");
    player.stop();
    hear(&mut d, 128);
    assert!(!player.is_playing(), "and takes its messages");
}

// RENDER-28, bug 1: a change of output device asks for a new player (`changed`), which the server
// opens on the new device. Meanwhile this one isn't stalled and plays on, as cpal reroutes it.
#[test]
fn a_device_change_asks_for_a_new_player_and_plays_on_meanwhile() {
    let (player, mut d) = Player::driven(48_000);
    player.open(sine(48_000)).unwrap();
    player.play();
    let mut live = hear(&mut d, 4_800);
    assert!(!player.changed());
    d.error(DeviceErrorKind::DeviceChanged.into());
    assert!(player.changed() && !player.stalled());
    assert_eq!(player.device_errors(), DeviceErrors { changes: 1, ..DeviceErrors::default() });
    live.extend(hear(&mut d, 4_800));
    assert_eq!(live, sine(48_000).render(0.2).unwrap());
}

// RENDER-28, bug 1: a stream invalidated or a device unavailable means the device is gone: the
// player is stalled at once, as one whose sender timed out is, and the server lets it go.
#[test]
fn a_device_that_is_gone_stalls_the_player() {
    for kind in [DeviceErrorKind::StreamInvalidated, DeviceErrorKind::DeviceNotAvailable] {
        let (player, mut d) = Player::driven(48_000);
        player.open(sine(48_000)).unwrap();
        hear(&mut d, 128);
        assert!(!player.stalled());
        d.error(kind.into());
        assert!(player.stalled() && !player.changed(), "{kind:?}");
        assert_eq!(player.device_errors(), DeviceErrors { gone: 1, ..DeviceErrors::default() });
    }
}

// The player's thread logs what the device reported, once a second while it reports anything:
// the error callback itself never prints (RENDER-28).
#[test]
fn what_the_device_reported_is_said_in_words() {
    let none = DeviceErrors::default();
    assert_eq!(none.since(none), None);
    let one = DeviceErrors { xruns: 1, ..none };
    assert_eq!(one.since(none).as_deref(), Some("the audio device reported 1 dropout"));
    assert_eq!(one.since(one), None, "nothing new");
    let all = DeviceErrors { xruns: 4, changes: 1, gone: 2, other: 3 };
    assert_eq!(
        all.since(one).as_deref(),
        Some("the audio device reported 3 dropouts, the output moved to another device, the device is gone, 3 other errors")
    );
}

/// Wait up to 2 s for `f`.
fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(2) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

// RENDER-28 (K1): a silent player let go stops. Its clock's thread ends once every clone is let
// go, and the engine it played leaves for the dropper, which frees it and ends too. Before, the
// thread rendered on for good, holding the engine (264 kernel instances alive a second after the
// server let go of a player, measured).
#[test]
fn a_silent_player_let_go_stops_and_frees_its_engine() {
    let player = Player::silent().unwrap();
    let r = sine(48_000);
    let plan = Arc::downgrade(&r.info());
    player.open(r).unwrap();
    assert!(eventually(|| player.status().pending.is_none()), "the clock took the project");
    let (shared, also) = (Arc::downgrade(&player.shared), player.clone());
    drop(player);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(plan.strong_count(), 1, "a clone left keeps it playing");
    drop(also);
    assert!(eventually(|| plan.strong_count() == 0), "the engine is freed");
    assert!(eventually(|| shared.strong_count() == 0), "the clock's thread ended");
}

/// A stream as cpal's ALSA one runs: the callback on a thread of its own (calling back, or not, as
/// a stalled device), until the stream drops, which stops that thread and waits for it. The
/// callback drops there, on the audio thread.
struct Stream {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Stream {
    fn new(mut cb: Callback, calls_back: bool) -> Stream {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut buf = vec![0.0f32; 2 * 128];
            while !s.load(Ordering::Relaxed) {
                if calls_back {
                    cb.fill(&mut buf, 2);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        Stream { stop, thread: Some(thread) }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

/// A stream whose callback runs once for each `()` sent, with the peak of each buffer it filled
/// kept, until the stream drops.
struct Stepped {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Stepped {
    fn new(mut cb: Callback) -> (Stepped, crossbeam_channel::Sender<()>, crossbeam_channel::Receiver<f32>) {
        let (stop, (go, steps), (peaks, peak)) =
            (Arc::new(AtomicBool::new(false)), crossbeam_channel::unbounded(), crossbeam_channel::unbounded());
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut buf = vec![0.0f32; 2 * 512];
            while !s.load(Ordering::Relaxed) {
                if steps.recv_timeout(Duration::from_millis(1)).is_ok() {
                    cb.fill(&mut buf, 2);
                    let _ = peaks.send(buf.iter().fold(0.0f32, |m, x| m.max(x.abs())));
                }
            }
        });
        (Stepped { stop, thread: Some(thread) }, go, peak)
    }
}

impl Drop for Stepped {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

// RENDER-28 (K1): a player let go stops its stream only once the device has played the unload's
// fade out. The server unloads a player before it lets go of it (its output moved, say, where it
// may still sound), and a stream stopped before that fade cuts the sound off with a click. Before,
// the stream stopped up to 50 ms after the last clone was dropped, whatever the device had played.
#[test]
fn a_player_let_go_stops_its_stream_after_its_fade_out() {
    let (player, cb, _retired) = Player::parts(48_000, "test");
    let (shared, alive) = (player.shared.clone(), Arc::downgrade(&player.alive));
    let (stream, go, peak) = Stepped::new(cb);
    let device = std::thread::spawn(move || serve(stream, shared, alive));
    player.open(sine(48_000)).unwrap();
    let step = || go.send(()).ok().and_then(|()| peak.recv_timeout(Duration::from_secs(5)).ok());
    let playing: Vec<Option<f32>> = (0..4).map(|_| step()).collect();
    assert!(playing.iter().all(|p| p.is_some_and(|p| p > 0.5)), "it plays: {playing:?}");
    player.unload().unwrap();
    drop(player);
    std::thread::sleep(Duration::from_millis(150));
    assert!(!device.is_finished(), "the stream plays on until the device has played the fade out");
    // The callback that reads the unload plays on, the next fades out, then silence.
    let after: Vec<Option<f32>> = (0..4).map(|_| step()).collect();
    assert!(after[1].is_some_and(|p| 0.0 < p && p < 1.0) && after[2..].iter().all(|p| *p == Some(0.0)), "it faded out: {after:?}");
    device.join().unwrap();
}

// RENDER-28 (K1): a device's player let go stops its stream, and its thread ends; the engine the
// callback played leaves the audio thread for the dropper, even when the stream drops the callback
// there. With a device that calls back and one that stopped (a stall, when the server lets a player
// go, with the project it sent never taken). Before, the thread parked for good, holding the stream.
#[test]
fn a_device_player_let_go_stops_its_stream_and_frees_its_engine_elsewhere() {
    for calls_back in [true, false] {
        let (player, cb, retired) = Player::parts(48_000, "test");
        let (shared, alive) = (player.shared.clone(), Arc::downgrade(&player.alive));
        let device = std::thread::spawn(move || serve(Stream::new(cb, calls_back), shared, alive));
        let r = sine(48_000);
        let plan = Arc::downgrade(&r.info());
        player.open(r).unwrap();
        if calls_back {
            assert!(eventually(|| player.status().pending.is_none()), "the callback took the project");
        }
        let shared = Arc::downgrade(&player.shared);
        drop(player);
        device.join().unwrap();
        assert_eq!(shared.strong_count(), 0, "the stream stopped, its callback gone (calls back: {calls_back})");
        let gone: Vec<Gone> = retired.try_iter().collect();
        assert!(gone.iter().any(|g| matches!(g, Gone::Live(_))), "the engine left for the dropper (calls back: {calls_back})");
        assert_eq!(plan.strong_count(), 1);
        drop(gone);
        assert_eq!(plan.strong_count(), 0, "freed there");
    }
}

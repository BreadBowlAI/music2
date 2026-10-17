//! 8a's live half (RENDER-2a, RENDER-28, RENDER-28a): the player's own mixing in the one
//! floating-point mode, and its engine threads when something goes wrong: a task that panics on a
//! worker, a worker that never comes, a task that is stuck while the device's thread waits for it,
//! and a player let go. Every test holds the one-pool lock: workers spin, and
//! `threads::live_workers` is one count for the process. No test has a time bound: each waits on
//! counters, under a 60 s watchdog that only catches a hang.

mod common;

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::*;
use m2_os::rt::Group;
use m2_play::{Between, LiveThreads, Player};
use m2_runtime::{Pool, Runner, Setup, Trip, fpenv, threads};
use m2_song::DataSet;
use serde_json::json;

/// One pool at a time in this binary.
fn one_pool() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Wait (a 60 s watchdog) until `ok` holds.
fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(60), "hung: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A live player's engine threads, set up as a player's (plain threads: no device here).
fn live_setup() -> Setup {
    threads::live(Arc::new(Group::none(Some(Duration::from_micros(2667)))), false, None).0
}

/// `n` engine threads in spread mode: every worker woken each block, the device's thread kept off
/// the roots, so tasks surely run on workers.
fn engine_threads(n: usize, setup: Setup) -> (Pool, Runner) {
    let (pool, mut runner) = Pool::new(n, setup).unwrap();
    runner.spread(true);
    (pool, runner)
}

/// A sine scaled by 1e-20 twice: every sample it plays is subnormal (about 1e-40).
fn subnormal() -> Song {
    let a = 1e-20;
    song(
        json!([
            {"id": "f", "op": "const", "value": 440.0, "unit": "hz"},
            {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "f"}},
            {"id": "a", "op": "const", "value": a},
            {"id": "x", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "a"}},
            {"id": "y", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "x", "by": "a"}}]),
        "y",
        json!([]),
        DataSet::empty(),
    )
}

/// `song` on a 44.1 kHz device (the resampler mixes every sample), replaced by itself half way (the
/// 5 ms fade mixes two engines), its callbacks run on a thread that flushes subnormals when
/// `flush`; and whether the thread's own mode is back after them.
fn mixed(song: &Song, flush: bool) -> (Vec<[f32; 2]>, bool) {
    let (player, mut d) = playing(song.renderer(), 44_100);
    fpenv::flush_for_tests(flush);
    let mut out = hear(&mut d, 4410, &mut |_, _| {});
    player.replace(song.renderer()).unwrap();
    out.extend(hear(&mut d, 4410, &mut |_, _| {}));
    // Read from the register itself: the compiler may move arithmetic across a mode change.
    let own = fpenv::is_canon() != flush;
    fpenv::flush_for_tests(false);
    (out, own)
}

// RENDER-2a (8a's live half): the callback mixes in the one mode, whatever its thread's: the
// resampler and the 5 ms fade keep a subnormal song's samples on a thread that flushes them to
// zero, bit for bit as on one that doesn't; and the thread's own mode is back after each callback.
#[test]
fn the_callback_mixes_in_the_one_mode() {
    let song = subnormal();
    let (clear, _) = std::thread::spawn({
        let song = subnormal();
        move || mixed(&song, false)
    })
    .join()
    .unwrap();
    let (flushed, own) = mixed(&song, true);
    assert!(clear.iter().skip(500).any(|f| f[0] != 0.0 && f[0].is_subnormal()), "the song plays subnormal samples");
    assert_eq!(first_diff(&flushed, &clear), None, "a thread that flushes mixes the same bits");
    assert!(own, "the thread's own mode is back after the callbacks");
}

// RENDER-28 (8a's live half): a task that panics on a worker crashes the player, as a panic in the
// engine always did: the block finishes, the callback returns, the player says it crashed and
// plays silence; the server then makes a new one.
#[test]
fn a_panic_on_a_worker_crashes_the_player() {
    let _one = one_pool();
    let song = fixture("examples/first-song");
    let (_pool, mut runner) = engine_threads(4, live_setup());
    runner.trip(runner.runs() + 50, Trip::Panic);
    let (player, mut d) = Player::driven_on(song.rate, runner);
    player.open(song.renderer()).unwrap();
    player.play();
    let out = hear(&mut d, 100 * 128, &mut |_, _| {});
    assert!(player.crashed(), "the player crashed");
    let on = d.runner().tripped_on().expect("a task tripped");
    assert_ne!(on, 0, "on a worker");
    assert!(peak(&out[..40 * 128]) > 0.0, "it played before");
    assert_eq!(peak(&out[out.len() - 20 * 128..]), 0.0, "and plays silence after");
}

// RENDER-28 (8a's live half): a worker that never comes to a block (not scheduled; here its start
// never ends) costs only speed: the device's thread and the other workers take every task, and
// the song plays the export's bits.
#[test]
fn a_worker_that_never_comes_costs_only_speed() {
    let _one = one_pool();
    let song = fixture("examples/first-song");
    let frames = 2 * song.rate as usize;
    let export = {
        let mut out = vec![[0.0; 2]; frames];
        song.renderer().process(&mut out).unwrap();
        out
    };
    let latch = Arc::new(AtomicBool::new(false));
    let mut setup = live_setup();
    let start = setup.start.take().expect("a live start");
    let hold = latch.clone();
    setup.start = Some(Arc::new(move |k| {
        while k == 3 && !hold.load(SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        start(k)
    }));
    let (pool, runner) = Pool::new(4, setup).unwrap();
    let (player, mut d) = Player::driven_on(song.rate, runner);
    player.open(song.renderer()).unwrap();
    player.play();
    let out = hear(&mut d, frames, &mut |_, _| {});
    assert_eq!(first_diff(&out, &export), None, "the export's bits on the threads that came");
    let c = pool.counters();
    assert!(c[1].tasks + c[2].tasks > 0 && c[3].tasks == 0, "workers 1 and 2 worked, 3 never came: {c:?}");
    latch.store(true, SeqCst);
    drop((player, d));
    drop(pool);
}

// RENDER-28 (8a's live half): while a task is stuck on a worker, the device's thread (here one
// std didn't start, as CoreAudio's isn't) sleeps on its wake, using no CPU, and the task's end
// wakes it: the block finishes with one thread's bits.
#[test]
fn a_stuck_task_puts_the_device_thread_to_sleep_on_its_wake() {
    let _one = one_pool();
    let song = Arc::new(fixture("examples/first-song"));
    let frames = 60 * 128;
    let export = {
        let mut out = vec![[0.0; 2]; frames];
        song.renderer().process(&mut out).unwrap();
        out
    };
    let setup = Setup { stuck: Duration::from_millis(1), doze: Duration::from_secs(3600), ..live_setup() };
    let (pool, mut runner) = engine_threads(4, setup);
    let open = Arc::new(AtomicBool::new(false));
    runner.trip(runner.runs() + 40, Trip::Hold(open.clone()));
    let device = {
        let song = song.clone();
        std::thread::spawn(move || {
            let (player, mut d) = Player::driven_on(song.rate, runner);
            player.open(song.renderer()).unwrap();
            player.play();
            let mut out = vec![];
            m2_os::thread::foreign(&mut || out = hear(&mut d, frames, &mut |_, _| {})).unwrap();
            out
        })
    };
    // Let the task go however this test ends, so the pool can join its workers.
    struct Opens(Arc<AtomicBool>);
    impl Drop for Opens {
        fn drop(&mut self) {
            self.0.store(true, SeqCst);
        }
    }
    let opens = Opens(open);
    until("the device's thread dozes", || pool.counters()[0].dozes >= 1);
    drop(opens);
    // Its doze lasts an hour: only a ring (the task's end, or a worker leaving the run) ends it.
    until("the callbacks end", || device.is_finished());
    let out = device.join().unwrap();
    let c = pool.counters()[0];
    assert!(c.rung >= 1 && c.timeouts == 0 && c.rung == c.dozes, "every doze was rung: {c:?}");
    assert_eq!(first_diff(&out, &export), None, "one thread's bits");
}

// RENDER-28a (8a's live half): a player let go joins its engine threads: each worker leaves (and
// on a Mac leaves the workgroup) before the player's thread ends.
#[test]
fn a_player_let_go_joins_its_workers() {
    let _one = one_pool();
    let before = threads::live_workers();
    let player = Player::silent_with(480, LiveThreads { count: Some(4), between: Between::Park }).unwrap();
    until("its workers start", || threads::live_workers() == before + 3);
    let song = fixture("examples/first-song");
    player.open(song.renderer()).unwrap();
    player.play();
    until("it plays", || player.position() > 48_000);
    let e = player.engine_threads();
    assert_eq!((e.count, e.real_time, e.joined, e.between), (4, 0, 0, Between::Park), "plain threads: {e:?}");
    player.unload().unwrap();
    drop(player);
    until("its workers are joined", || threads::live_workers() == before);
}

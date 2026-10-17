//! RENDER-28 (8a's live half): engine threads never allocate or free while they render, from a
//! fresh worker's first block on. Each worker is under the allocation trap from its start to its
//! stop (`threads::live`'s `mark`), and the device's thread during each fill, as in debug builds.
//! The trap's count is one for the process, so this is a test binary of its own: no other test
//! allocates under it here. Debug builds count; a release callback marks nothing.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::*;
use m2_os::rt::Group;
use m2_play::{Player, alloc};
use m2_runtime::{Pool, threads};

#[global_allocator]
static TRAP: alloc::Counting = alloc::Counting;

// RENDER-28 (8a's live half): first-song on 4 engine threads in spread mode, from the workers'
// first block: play, a seek, an activation with a ghost and fades through its window,
// `Player::panic()` (every note off), stop, idle, and play again. Nothing the device's thread or a
// worker did allocated or freed.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "only debug builds' callbacks mark their thread")]
fn engine_threads_never_allocate() {
    let (song, next) = (fixture("examples/first-song"), first_song_edited());
    let cont = continues(&song, &next);
    let r = song.renderer();
    let after = next.against(&song, &r.info(), &cont);
    let s = after.stats().clone();
    assert!(s.ghosts >= 1 && s.fades >= 2, "a ghost and fades: {s:?}");
    let group = Arc::new(Group::none(Some(Duration::from_micros(2667))));
    let (setup, joins) = threads::live(group, false, Some(alloc::set_in_callback));
    let (pool, mut runner) = Pool::new(4, setup).unwrap();
    runner.spread(true);
    let start = Instant::now();
    while joins.started.load(Ordering::SeqCst) < 3 {
        assert!(start.elapsed() < Duration::from_secs(60), "hung: the workers never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    let before = alloc::all();
    // A fresh device thread, under the trap only while it fills.
    let (on_workers, crossed) = std::thread::spawn(move || {
        let (player, mut d) = Player::driven_on(SR, runner);
        player.open(r).unwrap();
        let mut buf = vec![0.0f32; 2 * 256];
        let mut fill = |d: &mut m2_play::Driver, seconds: f64| {
            for _ in 0..(seconds * SR as f64 / 256.0) as usize {
                d.fill(&mut buf, 2);
                drop(d.drain_retired());
            }
        };
        fill(&mut d, 0.1);
        player.play();
        fill(&mut d, 1.0);
        player.seek(8.0).unwrap();
        fill(&mut d, 0.5);
        player.activate(after).unwrap();
        fill(&mut d, 2.0);
        player.panic().unwrap();
        fill(&mut d, 0.5);
        player.stop();
        fill(&mut d, 1.0);
        player.play();
        fill(&mut d, 0.5);
        let s = d.runner().spread_report();
        (s.tasks_on_workers, s.edges_across)
    })
    .join()
    .unwrap();
    let allocs = alloc::all() - before;
    assert!(on_workers > 0 && crossed > 0, "tasks ran on workers ({on_workers}), reads crossed threads ({crossed})");
    assert_eq!(allocs, 0, "the engine threads allocated or freed {allocs} times (M2_TRAP_TRACE=1 says where)");
    drop(pool);
}

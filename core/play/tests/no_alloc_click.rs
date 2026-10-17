//! RENDER-28 for the metronome (gaps #19): with the click on, the audio callback never allocates or
//! frees, from its first fill on a fresh thread, through a tempo change, a meter change and a loop's
//! wraps. Its own file, so `no_alloc.rs` stays the other tracks' (the next track's X5).
//!
//! As in `no_alloc.rs`, debug builds' callbacks mark their thread for the counter, which is how
//! ci/check.sh runs it; a release build counts nothing.

mod common;

use common::*;
use m2_play::{Player, alloc};
use serde_json::json;

#[global_allocator]
static TRAP: alloc::Counting = alloc::Counting;

/// A device callback's fill under the trap: what it allocated or freed.
fn trapped(f: impl FnOnce()) -> u64 {
    alloc::trace(std::env::var_os("M2_TRAP_TRACE").is_some());
    let before = alloc::here();
    f();
    alloc::here() - before
}

#[test]
fn the_click_never_allocates() {
    // A silent song: voices with no notes. 120 bpm in 3/4, 90 bpm from beat 4, 6/8 from beat 6,
    // looping beats 2 to 10.
    let arr = serde_json::from_value(json!({
        "tempo": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 90}],
        "meter": [{"beat": 0, "beats": 3, "unit": 4}, {"beat": 6, "beats": 6, "unit": 8}]}))
    .unwrap();
    let song = synth(0.1, json!([]), "v", json!({"clips": []}), arr);
    let (player, d) = Player::driven(SR);
    player.open(song.renderer()).unwrap();
    player.set_click(true);
    player.set_loop(Some((2.0, 10.0))).unwrap();
    player.play();
    let (allocs, heard) = std::thread::spawn(move || {
        let mut d = d;
        // The trap counts on this thread too, or the zero below proves nothing.
        let before = alloc::here();
        alloc::set_in_callback(true);
        drop(std::hint::black_box(Box::new(7u64)));
        alloc::set_in_callback(false);
        assert_eq!(alloc::here() - before, 2);
        let mut buf = vec![0.0f32; 2 * 4096];
        let (mut allocs, mut heard) = (0, 0.0f32);
        let mut done = 0;
        // 8 s: past the tempo and meter changes, and two wraps.
        for n in SIZES.iter().cycle() {
            if done >= 8 * SR as usize {
                break;
            }
            allocs += trapped(|| d.fill(&mut buf[..2 * n], 2));
            heard = buf[..2 * n].iter().fold(heard, |m, x| m.max(x.abs()));
            done += n;
            drop(d.drain_retired());
        }
        (allocs, heard)
    })
    .join()
    .unwrap();
    assert!(heard > 0.1, "the click sounded over a silent song: peak {heard}");
    assert_eq!(allocs, 0, "the callback allocated or freed with the click on");
    assert!(player.click());
}

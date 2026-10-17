//! RENDER-28 for the count-in (lane B): from the count's first fill on a fresh thread, through its
//! clicks, the song starting a bar later, and a second count cancelled by a stop, the audio
//! callback never allocates or frees. Its own file, as `no_alloc_click.rs` is.
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
fn the_count_in_never_allocates() {
    // A silent song (voices with no notes), 120 bpm in 3/4: a bar of 72,000 frames, which ends
    // on a block line 64 frames after the bar's length.
    let arr = serde_json::from_value(json!({
        "tempo": [{"beat": 0, "bpm": 120}],
        "meter": [{"beat": 0, "beats": 3, "unit": 4}]}))
    .unwrap();
    let song = synth(0.1, json!([]), "v", json!({"clips": []}), arr);
    let (player, d) = Player::driven(SR);
    player.open(song.renderer()).unwrap();
    player.count_in(3, 24_000.0).unwrap();
    let p = player.clone();
    let (allocs, heard, played) = std::thread::spawn(move || {
        let mut d = d;
        // The trap counts on this thread too, or the zero below proves nothing.
        let before = alloc::here();
        alloc::set_in_callback(true);
        drop(std::hint::black_box(Box::new(7u64)));
        alloc::set_in_callback(false);
        assert_eq!(alloc::here() - before, 2);
        let mut buf = vec![0.0f32; 2 * 4096];
        let (mut allocs, mut heard, mut played) = (0, 0.0f32, false);
        let mut done = 0;
        let (mut second, mut stopped) = (false, false);
        // 4 s: the count, 2.5 s of the song, then a count stopped half way.
        for n in SIZES.iter().cycle() {
            if done >= 4 * SR as usize {
                break;
            }
            if !second && done >= 3 * SR as usize {
                played = p.is_playing();
                p.stop();
                p.count_in(3, 24_000.0).unwrap();
                second = true;
            }
            if second && !stopped && done >= 3 * SR as usize + 30_000 {
                p.stop();
                stopped = true;
            }
            allocs += trapped(|| d.fill(&mut buf[..2 * n], 2));
            heard = buf[..2 * n].iter().fold(heard, |m, x| m.max(x.abs()));
            done += n;
            drop(d.drain_retired());
        }
        (allocs, heard, played)
    })
    .join()
    .unwrap();
    assert!(heard > 0.1, "the count clicked over a silent song: peak {heard}");
    assert!(played, "the song played after the bar");
    assert_eq!(allocs, 0, "the callback allocated or freed while counting in");
}

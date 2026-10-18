//! RENDER-30: long audio plays from the disk. A seek, a loop's wraps and a tempo change find their
//! audio ready; a slow disk makes the lane miss, and the sound thread never waits for it. The
//! tests take turns (`ALONE`): the disk's seam is one per process.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::*;
use m2_play::{Driver, Player};
use m2_runtime::Renderer;

static ALONE: Mutex<()> = Mutex::new(());
/// Every read of the disk thread takes 200 ms more while this is set.
static SLOW: AtomicBool = AtomicBool::new(false);

fn watch(_: usize) {
    if SLOW.load(Ordering::Relaxed) && std::thread::current().name() == Some("m2-disk") {
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn alone() -> MutexGuard<'static, ()> {
    m2_os::fs::watch_reads(watch);
    ALONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Callbacks of `frames` frames for `seconds`, each at its time as a device calls them: what they
/// played, and the longest one took.
fn real_time(d: &mut Driver, frames: usize, seconds: f64) -> (Vec<[f32; 2]>, Duration) {
    let period = Duration::from_secs_f64(frames as f64 / SR as f64);
    let (mut out, mut buf, mut longest) = (vec![], vec![0.0f32; 2 * frames], Duration::ZERO);
    let mut next = Instant::now();
    for _ in 0..((seconds * SR as f64 / frames as f64).ceil() as usize).max(1) {
        let t = Instant::now();
        d.fill(&mut buf, 2);
        longest = longest.max(t.elapsed());
        out.extend(buf.chunks(2).map(|f| [f[0], f[1]]));
        drop(d.drain_retired());
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    (out, longest)
}

fn misses(p: &Player) -> u64 {
    p.disk_misses().iter().map(|m| m.1).sum()
}

/// A player driven as a device is, in real time: its callbacks never wait for the disk.
fn device(r: Renderer) -> (Player, Driver) {
    let (player, mut d) = Player::driven(SR);
    d.set_paced(false);
    player.open(r).unwrap();
    (player, d)
}

// RENDER-30, D8: a seek to 1:30 of a 2-minute clip while it plays misses nothing; one to 1:00
// while stopped plays, from the first block after Play, the offline render there bit for bit.
#[test]
fn a_seek_lands_on_audio() {
    let _alone = alone();
    let song = long_audio(120.0, 120.0);
    let (player, mut d) = device(song.renderer());
    player.play();
    real_time(&mut d, 128, 0.3);
    player.seek(180.0).unwrap();
    let (after, _) = real_time(&mut d, 128, 0.2);
    assert_eq!(misses(&player), 0, "the seek found its audio");
    assert!(peak(&after[after.len() / 2..]) > 0.0, "and plays it");
    player.stop();
    real_time(&mut d, 128, 0.05);
    player.seek(120.0).unwrap();
    player.play();
    let (heard, _) = real_time(&mut d, 128, 0.0);
    let mut there = Renderer::starting_at(&song.flat, &song.data, &song.packages, SR, 60 * SR as u64).unwrap();
    there.set_strict(false);
    let want = there.render(128.0 / SR as f64).unwrap();
    assert!(peak(&want) > 0.0);
    assert_eq!(first_diff(&heard, &want), None, "the offline render at 1:00");
    assert_eq!(misses(&player), 0);
}

// RENDER-30, D6: a 4-bar loop over a long clip wraps 100 times and never misses: the loop start
// is ready at every wrap.
#[test]
fn a_loop_over_audio_never_misses() {
    let _alone = alone();
    let song = long_audio(120.0, 120.0);
    // Paced: each callback as soon as the disk thread holds what it keeps, far faster than real time.
    let (player, mut d) = Player::driven(SR);
    player.open(song.renderer()).unwrap();
    player.set_loop(Some((100.0, 116.0))).unwrap();
    player.seek(100.0).unwrap();
    player.play();
    let (mut wraps, mut last, mut buf) = (0, 0, vec![0.0f32; 2 * 4096]);
    while wraps < 100 {
        d.fill(&mut buf, 2);
        drop(d.drain_retired());
        let pos = player.status().pos;
        wraps += usize::from(pos < last);
        last = pos;
    }
    assert_eq!(misses(&player), 0, "every wrap found the loop start");
    assert!(peak(&buf.chunks(2).map(|f| [f[0], f[1]]).collect::<Vec<_>>()) > 0.0);
}

// RENDER-30, D9, RENDER-27: a tempo change while a long clip plays rebuilds its lane in the new
// map; the activation waits until the new lane holds the playhead's next half second, so the
// switch misses nothing.
#[test]
fn a_tempo_change_finds_its_audio_ready() {
    let _alone = alone();
    let (a, b) = (long_audio(120.0, 120.0), long_audio(120.0, 100.0));
    let r = a.renderer();
    let info = r.info();
    let (player, mut d) = device(r);
    player.seek(40.0).unwrap();
    player.play();
    real_time(&mut d, 128, 0.3);
    player.activate(b.after(&a, &info)).unwrap();
    let (heard, _) = real_time(&mut d, 128, 0.3);
    let engine = d.engine().unwrap();
    assert!(engine.activations() >= 1 && engine.refused_activations() == 0, "the new plan plays");
    assert_eq!(misses(&player), 0, "the new lane was ready");
    assert!(peak(&heard[heard.len() / 2..]) > 0.0);
}

// RENDER-30, D7, RENDER-28: with every read 200 ms slow, a seek far away can't find its audio in its
// 30 ms: the lane misses, the misses are counted for its module, and no callback waits: each ends
// within its period.
#[test]
fn a_slow_disk_misses_but_never_waits() {
    let _alone = alone();
    let song = long_audio(60.0, 120.0);
    let (player, mut d) = device(song.renderer());
    SLOW.store(true, Ordering::Relaxed);
    player.play();
    let (_, before) = real_time(&mut d, 480, 0.3);
    player.seek(80.0).unwrap();
    let (_, after) = real_time(&mut d, 480, 0.6);
    SLOW.store(false, Ordering::Relaxed);
    let period = Duration::from_secs_f64(480.0 / SR as f64);
    assert!(before.max(after) < period, "a callback took {:?} of its {period:?}", before.max(after));
    let missed = player.disk_misses();
    assert!(matches!(missed.as_slice(), [(m, n)] if m == "song" && *n > 0), "{missed:?}");
}

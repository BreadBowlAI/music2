//! RENDER-30b live: an edit of a voice's notes while the song plays is heard without a miss. Until
//! the edited phrase's render is made, the lane plays what it played there; once made, the disk
//! thread works out again the chunks it played so, and they play the edit.

mod common;

use std::collections::HashSet;
use std::time::{Duration, Instant};

use common::*;
use m2_play::{Driver, Player};
use m2_plugins::Owner;
use m2_runtime::audio::Cache;
use m2_runtime::{Basis, Continues, Keep, Renderer};
use m2_song::{DataSet, LaneData, LaneKind};
use serde_json::{Value, json};

/// Callbacks of `frames` frames for `seconds`, each at its time as a device calls them.
fn real_time(d: &mut Driver, frames: usize, seconds: f64) -> Vec<[f32; 2]> {
    let period = Duration::from_secs_f64(frames as f64 / SR as f64);
    let (mut out, mut buf) = (vec![], vec![0.0f32; 2 * frames]);
    let mut next = Instant::now();
    for _ in 0..((seconds * SR as f64 / frames as f64).ceil() as usize).max(1) {
        d.fill(&mut buf, 2);
        out.extend(buf.chunks(2).map(|f| [f[0], f[1]]));
        drop(d.drain_retired());
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    out
}

fn misses(p: &Player) -> u64 {
    p.disk_misses().iter().map(|m| m.1).sum()
}

/// The frequency of the left channel of `x`'s second half, from its upward zero crossings.
fn hz(x: &[[f32; 2]]) -> f64 {
    let x: Vec<f32> = x[x.len() / 2..].iter().map(|f| f[0]).collect();
    let ups: Vec<f64> =
        x.windows(2).enumerate().filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0).map(|(i, w)| i as f64 + (w[0] / (w[0] - w[1])) as f64).collect();
    (ups.len() - 1) as f64 / ((ups[ups.len() - 1] - ups[0]) / SR as f64)
}

/// A voice at 220 Hz for `seconds`: a pulse each period through a gentle low-pass, so each period
/// crosses zero upward once.
fn voice(seconds: f64) -> Vec<f32> {
    let mut y = 0.0f32;
    (0..(seconds * SR as f64) as usize)
        .map(|i| {
            let t = i as f64 / SR as f64;
            let x = (std::f64::consts::TAU * 220.0 * t).sin() as f32 + 0.3 * (std::f64::consts::TAU * 440.0 * t).sin() as f32;
            y = 0.8 * y + 0.2 * x;
            0.3 * y
        })
        .collect()
}

/// The clip over the file, its voice's one note from 0.5 to 3.5 s moved by `by` semitones.
fn clip(by: f64) -> Value {
    json!({"start": 0, "length": 16, "asset": "long.wav", "render": "@m2/vocals:voice",
           "sung": [{"at": 0.5, "length": 3.0, "pitch": 57, "move": by, "straighten": 0}]})
}

/// The song's data with its clip moved by `by`.
fn data_of(song: &Song, by: f64) -> DataSet {
    let text = json!({"clips": [clip(by)]}).to_string();
    let lane = LaneData::read(&text, &LaneKind::Audio).unwrap();
    song.data.seeded(&addr("clips"), &LaneKind::Audio, text.into(), std::sync::Arc::new(lane))
}

// RENDER-30b: a pitch edit while the song plays over the note counts no miss: held back, its phrase
// plays the clip as it was; made, it plays the edit, and still no miss; then another edit, the
// same.
#[test]
fn an_edit_while_playing_counts_no_miss() {
    let song = long_audio_clip(8.0, 120.0, clip(0.0));
    let v = voice(8.0);
    m2_runtime::audio::write_wav(&song.dir.join("assets/long.wav"), &[v.clone(), v], SR).unwrap();
    let mut cache = Cache::live_in(song.dir.join("renders"));
    let owner = Owner::discard();
    let mut r = Renderer::prepare(&song.flat, &song.data, &song.packages, SR, &mut cache, &owner, Basis::Fresh).unwrap();
    r.set_strict(false);
    let mut info = r.info();
    let (player, mut d) = Player::driven(SR);
    d.set_paced(false);
    player.open(r).unwrap();
    player.play();
    real_time(&mut d, 128, 0.4);
    let mut old = song.data.clone();
    let mut was = 220.0;
    for (n, by) in [2.0, -3.0].into_iter().enumerate() {
        let data = data_of(&song, by);
        cache.hold_renders(Some(0));
        let (restarts, cont) = (HashSet::new(), Continues::named(&song.flat, &song.flat));
        let keep = Keep { old: &song.flat, old_data: &old, info: &info, cont: &cont, restarts: &restarts };
        let mut r = Renderer::prepare(&song.flat, &data, &song.packages, SR, &mut cache, &owner, Basis::Keep(keep)).unwrap();
        r.set_strict(false);
        let next = r.info();
        player.activate(r).unwrap();
        info = next;
        let held = real_time(&mut d, 128, 0.4);
        assert_eq!(misses(&player), 0, "edit {n}: held back, the phrase plays what it played before");
        assert!(peak(&held) > 0.01, "edit {n}: and it plays");
        let heard = hz(&held);
        assert!((heard / was - 1.0).abs() < 0.01, "edit {n}: held back, it plays at {was:.1} Hz as before, not {heard:.1}");
        cache.hold_renders(None);
        assert!(cache.settle_renders(Duration::from_secs(30)));
        let made = real_time(&mut d, 128, 0.6);
        assert_eq!(misses(&player), 0, "edit {n}: made, it switches with no miss");
        was = 220.0 * 2f64.powf(by / 12.0);
        let heard = hz(&made);
        assert!((heard / was - 1.0).abs() < 0.01, "edit {n}: made, it plays the edit, {was:.1} Hz, not {heard:.1}");
        old = data;
    }
}

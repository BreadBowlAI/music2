// The outputs a player can open, through the silent seam (no test opens a real device).

use std::time::{Duration, Instant};

use super::*;
use crate::Player;

fn silent(names: &[&str]) -> Audio {
    Audio::Silent(names.iter().map(|n| n.to_string()).collect())
}

fn output(name: &str, default: bool) -> Output {
    Output { name: name.into(), default, rate: 48_000, frames: Some((64, 2048)) }
}

/// The player's buffer once its clock called back: None if it never did within a second.
fn frames_of(p: &Player) -> Option<u32> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(1) {
        if let Some(n) = p.frames() {
            return Some(n);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

#[test]
fn outputs_put_the_default_first() {
    let listed = silent(&["Speakers", "Headphones"]).outputs();
    assert_eq!(listed.iter().map(|o| (o.name.as_str(), o.default)).collect::<Vec<_>>(), [("Speakers", true), ("Headphones", false)]);
    assert_eq!(silent(&["Speakers", "Headphones"]).default_output().as_deref(), Some("Speakers"));
    // The OS lists its devices in its own order: the default goes first, the rest keep theirs.
    let os = vec![output("HDMI", false), output("MacBook Pro Speakers", true), output("Headphones", false)];
    let names: Vec<String> = default_first(os).into_iter().map(|o| o.name).collect();
    assert_eq!(names, ["MacBook Pro Speakers", "HDMI", "Headphones"]);
    // No output works: none listed, none ready.
    assert!(Audio::Fail.outputs().is_empty() && Audio::Fail.default_output().is_none());
}

#[test]
fn a_chosen_device_that_is_gone_falls_back_to_the_default() {
    let audio = silent(&["Speakers", "Headphones"]);
    let chosen = Player::start_in(audio.clone(), &Choice { device: Some("Headphones".into()), frames: None }).unwrap();
    assert_eq!((chosen.device.as_str(), chosen.fell_back()), ("Headphones", false));
    let gone = Player::start_in(audio.clone(), &Choice { device: Some("USB Interface".into()), frames: None }).unwrap();
    assert_eq!((gone.device.as_str(), gone.fell_back()), ("Speakers", true), "it plays on the default, and says so");
    let default = Player::start_in(audio, &Choice::default()).unwrap();
    assert_eq!((default.device.as_str(), default.fell_back()), ("Speakers", false));
    // With no output at all, the start fails in today's words.
    let none = Player::start_in(Audio::Fail, &Choice::default()).err().expect("no output starts");
    assert_eq!(none.to_string(), "no audio output device");
}

#[test]
fn a_fixed_buffer_is_used_when_in_range() {
    let audio = silent(&["Speakers"]);
    let p = Player::start_in(audio.clone(), &Choice { device: None, frames: Some(256) }).unwrap();
    assert_eq!(frames_of(&p), Some(256));
    // Out of the device's range: its own size.
    let p = Player::start_in(audio, &Choice { device: None, frames: Some(8) }).unwrap();
    assert!(frames_of(&p).is_some_and(|n| n != 8), "8 frames is under the range: {:?}", p.frames());
    // A device opens at a size it takes, else its default; one that doesn't say gets its default.
    let range = SupportedBufferSize::Range { min: 64, max: 2048 };
    assert_eq!(buffer(Some(128), &range), BufferSize::Fixed(128));
    assert_eq!(buffer(Some(4096), &range), BufferSize::Default);
    assert_eq!(buffer(None, &range), BufferSize::Default);
    assert_eq!(buffer(Some(128), &SupportedBufferSize::Unknown), BufferSize::Default);
}

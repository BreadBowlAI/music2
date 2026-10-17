// The outputs a player can open, through the silent seam (no test opens a real device).

use std::time::{Duration, Instant};

use super::*;
use crate::Player;

fn silent(names: &[&str]) -> Audio {
    Audio::Silent(names.iter().map(|n| n.to_string()).collect())
}

fn output(name: &str, default: bool) -> Output {
    Output { id: format!("coreaudio:{name}"), name: name.into(), default, rate: 48_000, frames: Some((64, 2048)) }
}

/// One card as ALSA lists it, in its order: the raw `hw:` and `plughw:` first, then the card's
/// shared PCM (`sysdefault:`), its other plugins, an HDMI device, and cpal's numbered `hw:` again.
fn alsa_card() -> Vec<(String, String)> {
    let analog = "HDA Intel PCH, ALC897 Analog";
    let hdmi = "HDA Intel PCH, HDMI 0";
    [
        ("alsa:pipewire", "PipeWire Sound Server"),
        ("alsa:hw:CARD=PCH,DEV=0", analog),
        ("alsa:plughw:CARD=PCH,DEV=0", analog),
        ("alsa:sysdefault:CARD=PCH", analog),
        ("alsa:front:CARD=PCH,DEV=0", analog),
        ("alsa:dmix:CARD=PCH,DEV=0", analog),
        ("alsa:hw:CARD=PCH,DEV=3", hdmi),
        ("alsa:hdmi:CARD=PCH,DEV=0", hdmi),
        ("alsa:hw:CARD=0,DEV=0", analog),
    ]
    .into_iter()
    .map(|(id, name)| (id.to_string(), name.to_string()))
    .collect()
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

#[test]
fn a_player_let_go_frees_its_device() {
    let p = Player::start_in(silent(&["Speakers"]), &Choice::default()).unwrap();
    let freed = p.freed();
    assert!(!freed.wait(Duration::from_millis(50)), "held while the player lives");
    drop(p);
    assert!(freed.wait(Duration::from_secs(1)), "free once it is let go");
    let (driven, _driver) = Player::driven(48_000);
    assert!(driven.freed().is_free(), "a driven player has no device");
}

// NX-S1: a name is listed once, from the device a person means by it: on ALSA the card's shared
// PCM, never its raw `hw:` (which one stream holds alone), whether that is busy or idle. Only the
// best way in is opened to read its config, and the next only if it won't open. The default is
// known by its id.
#[test]
fn an_output_is_listed_from_the_device_a_person_means() {
    let seen = alsa_card();
    for busy in [true, false] {
        let mut opened = vec![];
        let rows = listed(&seen, Some("alsa:pipewire"), |i| {
            opened.push(seen[i].0.clone());
            let raw = seen[i].0.starts_with("alsa:hw:") || seen[i].0.starts_with("alsa:plughw:");
            (!(busy && raw)).then_some((48_000, Some((64, 4096))))
        });
        let ids: Vec<(&str, &str, bool)> = rows.iter().map(|o| (o.id.as_str(), o.name.as_str(), o.default)).collect();
        assert_eq!(
            ids,
            [
                ("alsa:pipewire", "PipeWire Sound Server", true),
                ("alsa:sysdefault:CARD=PCH", "HDA Intel PCH, ALC897 Analog", false),
                ("alsa:hdmi:CARD=PCH,DEV=0", "HDA Intel PCH, HDMI 0", false),
            ],
            "the card's raw hw: {}",
            if busy { "busy" } else { "idle" }
        );
        assert_eq!(opened, ["alsa:pipewire", "alsa:sysdefault:CARD=PCH", "alsa:hdmi:CARD=PCH,DEV=0"], "only the best ways in are opened");
    }
    // A shared PCM that won't open: the next way in, by rank, never the raw one while another opens.
    let rows = listed(&seen, None, |i| (!seen[i].0.contains("sysdefault")).then_some((44_100, None)));
    assert_eq!(rows[1].id, "alsa:front:CARD=PCH,DEV=0");
    // Elsewhere (CoreAudio, WASAPI) each device is its own name: every one listed, as it is.
    let mac = vec![("coreaudio:BuiltIn".to_string(), "MacBook Pro Speakers".to_string()), ("coreaudio:USB".to_string(), "Scarlett 2i2".to_string())];
    let rows = listed(&mac, Some("coreaudio:USB"), |_| Some((48_000, None)));
    let ids: Vec<(&str, bool)> = rows.iter().map(|o| (o.id.as_str(), o.default)).collect();
    assert_eq!(ids, [("coreaudio:USB", true), ("coreaudio:BuiltIn", false)]);
}

// NX-S1: a start opens the output chosen, by its id; the default when it is gone or won't open,
// saying so (fell back); only the default when none is chosen.
#[test]
fn a_start_tries_the_output_chosen_then_the_default() {
    assert_eq!(in_order(true, Some("USB"), Some("Speakers")), [("USB", false), ("Speakers", true)], "chosen, then the default if it won't open");
    assert_eq!(in_order(true, None, Some("Speakers")), [("Speakers", true)], "chosen and gone");
    assert_eq!(in_order(false, None, Some("Speakers")), [("Speakers", false)], "none chosen");
    assert_eq!(in_order::<&str>(true, None, None), [], "no output at all");
}

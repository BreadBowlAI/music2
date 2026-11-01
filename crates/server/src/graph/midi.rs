//! MIDI keyboards (8b.10): a keyboard plugged in plays the selected track live, as audition's
//! notes do (MON-7). A take records what it plays (lane B, `graph/record.rs`); nothing else is
//! written. One turned off in Settings plays and records nothing.
//!
//! - **The way in:** [`Graph::midi`] hears what `m2_os::midi` hears (or what a suite posts, with
//!   `M2_MIDI=inject`): a keyboard plugged in or out, and each message. Every input, every channel.
//! - **What plays:** the lane a window last said (`{"t": "midi", "module", "lane"}` on its
//!   WebSocket, [`Graph::midi_target`]): its selected track's notes lane. It stays until another
//!   window says otherwise or another song opens; with none, or a lane that has gone, a key plays
//!   nothing. A target changed while a key is held leaves that note where it sounds until its Off.
//! - **Which messages:** Note On (velocity 0 is an Off) and Note Off; All Notes Off and All Sound
//!   Off (CC 123, 120), which let go of every note; the pitch wheel (pitch bend), the mod wheel
//!   (CC 1), the sustain pedal (CC 64) and reset all controllers (CC 121), which reach the target
//!   lane live (RENDER-14's `Midi`, D8 of track pm, RENDER-35). The rest are ignored (aftertouch,
//!   other controllers, clock…).
//! - **The pedal** reaches the lane, and each key's Off reaches it when the key is let go: the
//!   lane's voices hold the sound while the pedal is down (RENDER-38), and a plugin holds its own
//!   (RENDER-37). A take keeps a key let go under the pedal until the pedal lifts or the key is
//!   struck again, so it records the note as it sounded (RENDER-36).
//! - **The wheels go back to rest** on the lane they moved when the keyboard lets go: unplugged, All
//!   Notes Off, turned off in Settings, another target, or ⌘. (a panic, in the engine). A song
//!   closed takes its lanes with it.
//! - **A keyboard's notes** are an audition source of its own, `midi:<name>`, whose handles are
//!   `note + 128 × channel`, so two channels never share a key. The watchdog never lets it go
//!   (Q6): unplugging it, All Notes Off, ⌘. (a panic) or closing the song does.
//! - **A kit** (KIT-25): while what keys play is a drum kit's bus or one of its Drum tracks, a key
//!   goes to the kit's Drum track whose sound answers to its note, the first of two, at its own
//!   pitch, by the kit the song shows ([`Pads`]); a note none answers to plays nothing, said once.
//! - **Never through the session** (RENDER-29): a compile never holds a key up.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
pub use m2_os::midi::Event;
use serde_json::{Value, json};

use super::audition::{Audition, Strike};
use super::{ControlPlane, Graph, Inner};

/// What the control plane keeps of MIDI.
#[derive(Default)]
pub(super) struct Midi {
    /// The keyboards plugged in, in the order they came.
    keyboards: Vec<Keyboard>,
    target: Option<Target>,
    /// Keyboards turned off in Settings, by name (lane B, Q28): they neither play nor record.
    off: BTreeSet<String>,
    /// The notes no Drum of the kit keys play answers to, said already (KIT-25): by the kit's bus
    /// and the key, until keys play something else.
    unanswered: BTreeSet<(String, u8)>,
}

/// A drum kit as keys play it (KIT-25): its bus, its name, and its Drum tracks in the kit's order,
/// each its key, its notes lane and the sound it plays (none when the code works it out), as the
/// song shown now has them (KIT-1).
pub(super) struct Pads {
    pub bus: String,
    pub label: String,
    pub rows: Vec<(String, String, Option<String>)>,
}

impl Pads {
    /// The kit track `module` is in, its bus or one of its Drum tracks, in the song shown now.
    pub(super) fn of(cp: &ControlPlane, module: &str) -> Option<Pads> {
        let kits = cp.open.as_ref()?.shown.state["view"]["drum_kits"].as_array()?;
        let in_it = |k: &&Value| k["bus"] == module || k["rows"].as_array().is_some_and(|r| r.iter().any(|r| r["track"] == module));
        let k = kits.iter().find(in_it)?;
        let rows = k["rows"]
            .as_array()?
            .iter()
            .filter_map(|r| {
                Some((r["track"].as_str()?.to_string(), r["lane"].as_str()?.to_string(), r["sound"].as_str().map(String::from)))
            })
            .collect();
        Some(Pads { bus: k["bus"].as_str()?.to_string(), label: k["label"].as_str().unwrap_or("Drums").to_string(), rows })
    }

    /// The Drum track and lane that answers to `key` (KIT-6): the first whose sound has the note.
    pub(super) fn answers(&self, key: u8) -> Option<(String, String)> {
        self.rows
            .iter()
            .find(|(_, _, sound)| sound.as_deref().is_some_and(|s| m2_editor::drums::notes_of(s).contains(&key)))
            .map(|(m, l, _)| (m.clone(), l.clone()))
    }
}

struct Keyboard {
    name: String,
    /// The sustain pedal is down.
    pedal: bool,
    /// Keys let go while the pedal was down: a take keeps their notes until it lifts, or until the
    /// key is struck again (RENDER-36).
    held_back: BTreeSet<u32>,
}

/// What keyboards play: a lane of the song open when a window said so (its token).
#[derive(Clone, Debug, PartialEq)]
struct Target {
    token: u64,
    module: String,
    lane: String,
}

/// The audition source of keyboard `name`: never a window's client id.
pub(super) fn client(name: &str) -> String {
    format!("midi:{name}")
}

/// Whether an audition source is a keyboard's (Q6: the watchdog leaves it alone).
pub(super) fn is_keyboard(client: &str) -> bool {
    client.starts_with("midi:")
}

/// What a message says to a keyboard's notes.
#[derive(Debug, PartialEq)]
enum Said {
    On {
        h: u32,
        pitch: f64,
        velocity: f64,
    },
    Off {
        h: u32,
    },
    /// The sustain pedal (CC 64), as it goes to the lane: down at 64 or more.
    Pedal([u8; 3]),
    AllOff,
    /// A wheel: pitch bend, CC 1 or CC 121.
    Wheel([u8; 3]),
    Nothing,
}

fn read(bytes: &[u8]) -> Said {
    let Some(&status) = bytes.first() else { return Said::Nothing };
    let (d1, d2) = (bytes.get(1).copied(), bytes.get(2).copied());
    let key = |n: u8| u32::from(n & 0x7F) + 128 * u32::from(status & 0x0F);
    match (status & 0xF0, d1, d2) {
        (0x90, Some(n), Some(v)) if v & 0x7F > 0 => {
            Said::On { h: key(n), pitch: f64::from(n & 0x7F), velocity: f64::from(v & 0x7F) / 127.0 }
        }
        (0x90 | 0x80, Some(n), Some(_)) => Said::Off { h: key(n) },
        (0xB0, Some(64), Some(v)) => Said::Pedal([status, 64, v & 0x7F]),
        (0xE0, Some(l), Some(m)) => Said::Wheel([status, l & 0x7F, m & 0x7F]),
        (0xB0, Some(c @ (1 | 121)), Some(v)) => Said::Wheel([status, c, v & 0x7F]),
        (0xB0, Some(120 | 123), _) => Said::AllOff,
        _ => Said::Nothing,
    }
}

impl Graph {
    /// What a MIDI listener heard: a keyboard plugged in or out, or one of its messages. Answers the
    /// words to say when a keyboard came or went. Never waits for the session.
    pub fn midi(&self, ev: Event<'_>) -> Option<String> {
        match ev {
            Event::Plugged(name) => {
                let mut cp = self.inner.control.lock();
                if cp.midi.keyboards.iter().any(|k| k.name == name) {
                    return None;
                }
                cp.midi.keyboards.push(Keyboard { name: name.into(), pedal: false, held_back: BTreeSet::new() });
                if cp.midi.off.contains(name) {
                    return Some(format!("{name} is connected. It is turned off in Settings."));
                }
                Some(format!("{name} is connected. It plays the selected track."))
            }
            Event::Unplugged(name) => {
                let known = {
                    let mut cp = self.inner.control.lock();
                    let before = cp.midi.keyboards.len();
                    cp.midi.keyboards.retain(|k| k.name != name);
                    cp.midi.keyboards.len() != before
                };
                // One release: every note it held, on every lane.
                self.audition_gone(&client(name));
                known.then(|| format!("{name} was unplugged. Its notes stopped."))
            }
            Event::Bytes(name, bytes) => self.midi_message(name, bytes),
        }
    }

    /// One message from keyboard `name`, as audition: an On on the target (on a kit, the Drum track
    /// that answers to it), an Off (kept in a take while the pedal is down), the pedal, or every
    /// note let go. Answers the words for a note no Drum of a kit answers to, the first time.
    fn midi_message(&self, name: &str, bytes: &[u8]) -> Option<String> {
        let said = read(bytes);
        if said == Said::Nothing {
            return None;
        }
        let a = {
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token);
            // Turned off in Settings: it plays and records nothing (lane B).
            if cp.midi.off.contains(name) {
                return None;
            }
            let target = cp.midi.target.clone().filter(|t| Some(t.token) == token);
            // While a take records, keys play its lane, whatever a window selects (lane B, D8).
            let target = match cp.record.lane(token) {
                Some((module, lane)) => Some(Target { token: token.unwrap_or_default(), module, lane }),
                None => target,
            };
            // A kit's bus or Drum track: each key goes to the Drum track that answers to it (KIT-25).
            let pads = target.as_ref().and_then(|t| Pads::of(&cp, &t.module));
            let k = cp.midi.keyboards.iter_mut().find(|k| k.name == name)?;
            match said {
                Said::On { h, pitch, velocity } => {
                    // Struck again under the pedal: the take's note it held ends here (RENDER-36).
                    let again = k.held_back.remove(&h);
                    let key = pitch as u8;
                    let lane = match (target, &pads) {
                        (None, _) => None,
                        (Some(t), None) => Some((t.module, t.lane)),
                        (Some(_), Some(p)) => p.answers(key),
                    };
                    // A note no Drum of the kit answers to plays nothing: said once (KIT-25).
                    let unanswered = match (&lane, &pads) {
                        (None, Some(p)) if cp.midi.unanswered.insert((p.bus.clone(), key)) => Some(format!(
                            "No drum of the {} kit answers to {} ({key}). + Drum in Beat adds one.",
                            p.label,
                            m2_song::lane::pitch_name(f64::from(key)).unwrap_or_default()
                        )),
                        _ => None,
                    };
                    drop(cp);
                    if again {
                        self.take_lets_go(&client(name), &[h]);
                    }
                    let Some((module, lane)) = lane else { return unanswered };
                    Audition { module: Some(module), lane: Some(lane), on: vec![Strike { h, pitch, velocity }], ..Audition::default() }
                }
                // The lane hears it let go: its voices hold the sound while the pedal is down
                // (RENDER-38). A take keeps the note until the pedal lifts (RENDER-36).
                Said::Off { h } => {
                    if k.pedal {
                        k.held_back.insert(h);
                    }
                    Audition { off: vec![h], pedalled: k.pedal, ..Audition::default() }
                }
                Said::Pedal(bytes) => {
                    let down = bytes[2] >= 64;
                    let lifted = if k.pedal && !down { std::mem::take(&mut k.held_back) } else { BTreeSet::new() };
                    k.pedal = down;
                    drop(cp);
                    self.wheel(&client(name), target.map(|t| (t.module, t.lane)), bytes);
                    self.take_lets_go(&client(name), &lifted.into_iter().collect::<Vec<_>>());
                    return None;
                }
                Said::AllOff => {
                    // Its release puts the lane's pedal up, and ends what a take kept.
                    (k.pedal, k.held_back) = (false, BTreeSet::new());
                    drop(cp);
                    self.audition_gone(&client(name));
                    return None;
                }
                Said::Wheel(bytes) => {
                    drop(cp);
                    self.wheel(&client(name), target.map(|t| (t.module, t.lane)), bytes);
                    return None;
                }
                Said::Nothing => return None,
            }
        };
        // No song open, or a lane nothing plays: nothing sounds, and nobody is told.
        let _ = self.audition(&client(name), a);
        None
    }

    /// A seek or a Stop let the lanes' pedals up (RENDER-36): every keyboard's pedal is up for its
    /// take too, until the foot lifts and presses it again. A take that ran ended there, with the
    /// notes it kept.
    pub(super) fn midi_pedals_up(&self) {
        for k in &mut self.inner.control.lock().midi.keyboards {
            (k.pedal, k.held_back) = (false, BTreeSet::new());
        }
    }

    /// What MIDI keyboards play from now on: lane `(module, lane)` of the open song, or nothing.
    /// The last window to say wins (Q4). The wheels a keyboard moved on another lane go back to
    /// rest there; its held keys sound on where they are.
    pub fn midi_target(&self, target: Option<(String, String)>) -> Result<()> {
        let (keyboards, plays) = {
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token).context("no project is open")?;
            cp.midi.target = target.map(|(module, lane)| Target { token, module, lane });
            // Keys play something else: a note no Drum answers to is said again (KIT-25).
            cp.midi.unanswered.clear();
            // While a take records, keys play its lane (lane B, D8).
            let plays = cp.record.lane(Some(token)).or_else(|| cp.midi.target.as_ref().map(|t| (t.module.clone(), t.lane.clone())));
            (cp.midi.keyboards.iter().map(|k| client(&k.name)).collect::<Vec<_>>(), plays)
        };
        for k in keyboards {
            self.wheels_rest(&k, plays.as_ref());
        }
        Ok(())
    }

    /// The keyboards plugged in and what they play: the state's `midi`, and the `midi` event's.
    pub fn midi_state(&self) -> Value {
        self.inner.midi_state()
    }

    /// The keyboards turned off in Settings, by name (lane B, Q28): from now on they neither play
    /// nor record, and what one holds is let go.
    pub fn set_midi_off(&self, names: &[String]) {
        let now: BTreeSet<String> = names.iter().cloned().collect();
        let newly: Vec<String> = {
            let mut cp = self.inner.control.lock();
            let newly = now.difference(&cp.midi.off).cloned().collect();
            cp.midi.off = now;
            newly
        };
        for name in newly {
            self.audition_gone(&client(&name));
        }
    }
}

impl Inner {
    /// `{devices: [name], target: {module, lane} | null}`: the target only while its song is open.
    pub(super) fn midi_state(&self) -> Value {
        let cp = self.control.lock();
        let token = cp.open.as_ref().map(|p| p.engine.token);
        let target = cp.midi.target.as_ref().filter(|t| Some(t.token) == token).map(|t| match Pads::of(&cp, &t.module) {
            // A kit's: keys play it by sound (KIT-25).
            Some(p) => json!({"module": t.module, "lane": t.lane, "kit": p.bus}),
            None => json!({"module": t.module, "lane": t.lane}),
        });
        let mut v = json!({"devices": cp.midi.keyboards.iter().map(|k| k.name.as_str()).collect::<Vec<_>>(), "target": target});
        // Turned off in Settings (lane B): named only when some are.
        if !cp.midi.off.is_empty() {
            v["off"] = json!(cp.midi.off);
        }
        v
    }
}

#[cfg(test)]
#[path = "midi_tests.rs"]
mod tests;

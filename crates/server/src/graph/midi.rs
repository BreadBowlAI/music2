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
//! - **Which messages:** Note On (velocity 0 is an Off) and Note Off; the sustain pedal (CC 64),
//!   which holds back Offs until it lifts; All Notes Off and All Sound Off (CC 123, 120), which let
//!   go of every note. The rest are ignored (pitch bend, the mod wheel, clock…).
//! - **A keyboard's notes** are an audition source of its own, `midi:<name>`, whose handles are
//!   `note + 128 × channel`, so two channels never share a key. The watchdog never lets it go
//!   (Q6): unplugging it, All Notes Off, ⌘. (a panic) or closing the song does.
//! - **Never through the session** (RENDER-29): a compile never holds a key up.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
pub use m2_os::midi::Event;
use serde_json::{Value, json};

use super::audition::{Audition, Strike};
use super::{Graph, Inner};

/// What the control plane keeps of MIDI.
#[derive(Default)]
pub(super) struct Midi {
    /// The keyboards plugged in, in the order they came.
    keyboards: Vec<Keyboard>,
    target: Option<Target>,
    /// Keyboards turned off in Settings, by name (lane B, Q28): they neither play nor record.
    off: BTreeSet<String>,
}

struct Keyboard {
    name: String,
    /// The sustain pedal is down.
    pedal: bool,
    /// Keys let go while the pedal was down: their Offs, sent when it lifts.
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
    On { h: u32, pitch: f64, velocity: f64 },
    Off { h: u32 },
    Pedal(bool),
    AllOff,
    Nothing,
}

fn read(bytes: &[u8]) -> Said {
    let Some(&status) = bytes.first() else { return Said::Nothing };
    let (d1, d2) = (bytes.get(1).copied(), bytes.get(2).copied());
    let key = |n: u8| u32::from(n & 0x7F) + 128 * u32::from(status & 0x0F);
    match (status & 0xF0, d1, d2) {
        (0x90, Some(n), Some(v)) if v & 0x7F > 0 => Said::On { h: key(n), pitch: f64::from(n & 0x7F), velocity: f64::from(v & 0x7F) / 127.0 },
        (0x90 | 0x80, Some(n), Some(_)) => Said::Off { h: key(n) },
        (0xB0, Some(64), Some(v)) => Said::Pedal(v >= 64),
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
            Event::Bytes(name, bytes) => {
                self.midi_message(name, bytes);
                None
            }
        }
    }

    /// One message from keyboard `name`, as audition: an On on the target, an Off (or held back by
    /// the pedal), the pedal, or every note let go.
    fn midi_message(&self, name: &str, bytes: &[u8]) {
        let said = read(bytes);
        if said == Said::Nothing {
            return;
        }
        let a = {
            let mut cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token);
            // Turned off in Settings: it plays and records nothing (lane B).
            if cp.midi.off.contains(name) {
                return;
            }
            let target = cp.midi.target.clone().filter(|t| Some(t.token) == token);
            // While a take records, keys play its lane, whatever a window selects (lane B, D8).
            let target = match cp.record.lane(token) {
                Some((module, lane)) => Some(Target { token: token.unwrap_or_default(), module, lane }),
                None => target,
            };
            let Some(k) = cp.midi.keyboards.iter_mut().find(|k| k.name == name) else { return };
            match said {
                Said::On { h, pitch, velocity } => {
                    // Struck again under the pedal: audition lets the old one go first.
                    k.held_back.remove(&h);
                    let Some(t) = target else { return };
                    Audition { module: Some(t.module), lane: Some(t.lane), on: vec![Strike { h, pitch, velocity }], ..Audition::default() }
                }
                Said::Off { h } if k.pedal => {
                    k.held_back.insert(h);
                    return;
                }
                Said::Off { h } => Audition { off: vec![h], ..Audition::default() },
                Said::Pedal(down) => {
                    k.pedal = down;
                    if down {
                        return;
                    }
                    Audition { off: std::mem::take(&mut k.held_back).into_iter().collect(), ..Audition::default() }
                }
                Said::AllOff => {
                    k.held_back.clear();
                    drop(cp);
                    self.audition_gone(&client(name));
                    return;
                }
                Said::Nothing => return,
            }
        };
        // No song open, or a lane nothing plays: nothing sounds, and nobody is told.
        let _ = self.audition(&client(name), a);
    }

    /// What MIDI keyboards play from now on: lane `(module, lane)` of the open song, or nothing.
    /// The last window to say wins (Q4).
    pub fn midi_target(&self, target: Option<(String, String)>) -> Result<()> {
        let mut cp = self.inner.control.lock();
        let token = cp.open.as_ref().map(|p| p.engine.token).context("no project is open")?;
        cp.midi.target = target.map(|(module, lane)| Target { token, module, lane });
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
        let target = cp.midi.target.as_ref().filter(|t| Some(t.token) == token).map(|t| json!({"module": t.module, "lane": t.lane}));
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

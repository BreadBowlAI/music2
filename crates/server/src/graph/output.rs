//! The sound output (lane C of the next track).
//!
//! A song opens with no sound device (gaps #13): when no output starts, the silent player stands
//! in, so the playhead moves and nothing sounds, and the status says "No sound output". While it
//! stands in, the server looks for an output every 2 s ([`Graph::look_for_output`]); once one
//! starts, the next action plays the song there, where it was, stopped, and the user is told.
//!
//! A start that failed isn't tried again until the default output changes, or for 30 s: a device
//! the OS lists that won't open would otherwise be tried, and fail, at every action (plan X23).
//!
//! Settings chooses the output and its buffer size (gaps #31, [`Graph::set_output`]): the player
//! is let go, and once its stream has stopped a new one starts there, where it was, stopped.
//!
//! Only [`Inner::device`](super::Inner) starts players: it asks [`Outputs::start`] for one, and
//! [`Outputs::came_back`] whether a real one may take over from the stand-in.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use m2_play::{Choice, Player};
use parking_lot::Mutex;
use serde::Serialize;

use super::{Graph, retire};

/// What plays now, as Settings' Now row says it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Now {
    /// The device's name ("none" for the silent player).
    pub device: String,
    /// The device's own rate, in Hz.
    pub rate: u32,
    /// Its buffer, in samples, once it has called back.
    pub frames: Option<u32>,
    /// The output chosen is gone, so this is the default.
    pub fell_back: bool,
    /// No output started: the silent player stands in.
    pub no_output: bool,
}

/// Starts a player on an output: the real one opens the device ([`Player::start_on`]).
pub(super) type Start = Box<dyn Fn(&Choice) -> Result<Player> + Send + Sync>;
/// The default output's name, if there is one now ([`m2_play::default_output`]).
pub(super) type Ready = Box<dyn Fn() -> Option<String> + Send + Sync>;

/// How long a start that failed waits to be tried again, the default output unchanged.
pub(super) const RETRY: Duration = Duration::from_secs(30);
/// How often the server looks for an output while the silent player stands in.
pub const LOOK_EVERY: Duration = Duration::from_secs(2);
/// How long a change of output waits for the old stream to stop before it opens the new one.
const FREE: Duration = Duration::from_secs(1);
/// What the user is told once an output plays again.
pub(super) const BACK: &str = "Sound output is back. Press Play to go on.";

/// How players start, and what the server knows of the output.
pub(super) struct Outputs {
    start: Start,
    ready: Ready,
    state: Mutex<OutputState>,
    /// Tests: what runs once a change of output has taken the old player out of its slot.
    #[cfg(test)]
    pub(super) changing: Mutex<Option<Box<dyn FnMut() + Send>>>,
}

#[derive(Default)]
struct OutputState {
    /// What Settings chose (C2): the default output at its own buffer size until then.
    choice: Choice,
    /// The silent player standing in for an output that didn't start, by its epoch.
    stand_in: Option<u64>,
    /// When a start last failed, and the default output then.
    failed: Option<(Instant, Option<String>)>,
    /// An output to try: the next action does.
    back: bool,
    /// The log said once that none started.
    told: bool,
}

impl Outputs {
    /// The device's: players open the output chosen. Unit tests never open an audio device.
    pub(super) fn real() -> Outputs {
        if cfg!(test) {
            Outputs::with(Box::new(|_| Player::silent()), Box::new(|| Some("none".into())))
        } else {
            Outputs::with(Box::new(Player::start_on), Box::new(m2_play::default_output))
        }
    }

    pub(super) fn with(start: Start, ready: Ready) -> Outputs {
        Outputs {
            start,
            ready,
            state: Mutex::default(),
            #[cfg(test)]
            changing: Mutex::default(),
        }
    }

    /// A player for the epoch `epoch`, on the output chosen; when none starts, the silent player
    /// stands in, and the log says why, once.
    pub(super) fn start(&self, epoch: u64) -> Result<Player> {
        let choice = self.state.lock().choice.clone();
        match (self.start)(&choice) {
            Ok(player) => {
                let mut s = self.state.lock();
                (s.stand_in, s.failed, s.back) = (None, None, false);
                Ok(player)
            }
            Err(e) => {
                let ready = (self.ready)();
                let mut s = self.state.lock();
                if !std::mem::replace(&mut s.told, true) {
                    eprintln!("music2: no sound output started ({e:#}), so the song plays silently");
                }
                (s.stand_in, s.failed, s.back) = (Some(epoch), Some((Instant::now(), ready)), false);
                drop(s);
                Player::silent()
            }
        }
    }

    /// Settings' choice, from now on: whether it changed.
    fn choose(&self, choice: Choice) -> bool {
        let mut s = self.state.lock();
        let changed = s.choice != choice;
        s.choice = choice;
        changed
    }

    /// Whether the player `epoch` is the silent one standing in for an output.
    pub(super) fn standing_in(&self, epoch: u64) -> bool {
        self.state.lock().stand_in == Some(epoch)
    }

    /// A real player to take over from the stand-in `epoch`, if an output was found since and
    /// starts now; None while the stand-in plays on. Not while it is held for an export.
    pub(super) fn came_back(&self, epoch: u64, held: bool) -> Option<Player> {
        let choice = {
            let mut s = self.state.lock();
            if held || s.stand_in != Some(epoch) || !std::mem::take(&mut s.back) {
                return None;
            }
            s.choice.clone()
        };
        match (self.start)(&choice) {
            Ok(player) => {
                let mut s = self.state.lock();
                (s.stand_in, s.failed) = (None, None);
                eprintln!("music2: the sound output is back: {}", player.device);
                Some(player)
            }
            Err(_) => {
                let ready = (self.ready)();
                self.state.lock().failed = Some((Instant::now(), ready));
                None
            }
        }
    }

    /// While the stand-in plays: whether a default output exists now that wasn't tried, or was
    /// tried long enough ago. If so, the next action tries it.
    fn look(&self) {
        if self.state.lock().stand_in.is_none() {
            return;
        }
        let now = (self.ready)();
        let mut s = self.state.lock();
        let again = match &s.failed {
            Some((at, then)) => *then != now || at.elapsed() >= RETRY,
            None => true,
        };
        if now.is_some() && again {
            s.back = true;
        }
    }
}

impl Graph {
    /// While no output plays (the silent player stands in), look for one: the next action plays
    /// the song there. Called every [`LOOK_EVERY`] by the server's `music2-tell` thread.
    pub fn look_for_output(&self) {
        self.inner.output.look();
    }

    /// Whether no output plays: the silent player stands in for one that didn't start.
    pub fn no_output(&self) -> bool {
        self.inner.current_device().is_some_and(|d| self.inner.output.standing_in(d.epoch))
    }

    /// The output and buffer size Settings chose.
    pub fn output_choice(&self) -> Choice {
        self.inner.output.state.lock().choice.clone()
    }

    /// What plays now; None while no song has started a player.
    pub fn output_now(&self) -> Option<Now> {
        let d = self.inner.current_device()?;
        let p = &d.player;
        Some(Now {
            device: p.device.clone(),
            rate: p.device_rate,
            frames: p.frames(),
            fell_back: p.fell_back(),
            no_output: self.inner.output.standing_in(d.epoch),
        })
    }

    /// Play on the output `choice` names, at its buffer size (Settings, gaps #31). A player that
    /// plays is let go, as after a device change, and once its stream stopped (at most a second)
    /// a new one starts there, with the song
    /// loaded into it where it was, stopped (unless the session is busy: then the next catch-up). Returns whether it was playing (the
    /// window says "Playback stopped. Press Play to go on."). With no player yet, the next song's
    /// opens there. Refused while an export holds the player (plan X22).
    pub fn set_output(&self, choice: Choice) -> Result<bool> {
        let st = {
            let mut d = self.inner.player.lock();
            if d.as_ref().is_some_and(|d| d.player.held()) {
                bail!("Wait for the export to finish.");
            }
            if !self.inner.output.choose(choice) {
                return Ok(false);
            }
            let Some(old) = d.take() else { return Ok(false) };
            // Where it was, kept before the slot is let go: another action may start a player
            // the moment it is, and loads the song where it was (NX-S6).
            let st = old.player.status();
            *self.inner.resume.lock() = st.pending.unwrap_or(st.beats);
            // Its stream stops before the new one opens, the slot locked meanwhile so no other
            // action opens one: an output only one stream may hold at once (an ALSA `hw:` device)
            // would refuse it (NX-S5). It first plays the unload's fade out; one stuck isn't
            // waited for past `FREE`.
            let freed = old.player.freed();
            retire(old.player);
            freed.wait(FREE);
            st
        };
        #[cfg(test)]
        if let Some(f) = self.inner.output.changing.lock().as_mut() {
            f();
        }
        self.inner.device()?;
        // The song goes into it now, not at the server's next catch-up: Play pressed at once plays.
        self.catch_up();
        Ok(st.playing)
    }
}

#[cfg(test)]
impl Graph {
    /// One whose players start with `start`, and which finds the default output with `ready`.
    pub(crate) fn starting(start: Start, ready: Ready) -> Graph {
        Graph::with_outputs(None, Outputs::with(start, ready))
    }

    /// The last start that failed: make it look `secs` seconds older than it is.
    pub(crate) fn age_failed_start(&self, secs: u64) {
        if let Some((at, _)) = &mut self.inner.output.state.lock().failed {
            *at -= Duration::from_secs(secs);
        }
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

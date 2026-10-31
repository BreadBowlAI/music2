//! The drum kit's map, shared by the drum voice (`drum`) and what marks its hits (std's `drumhit`):
//! which drum a note plays, by its General MIDI key or a fixed model, and each drum's choke group.

use crate::voice::{clampf, pitch_of};

/// The drums. The hats share one choke group; a crash chokes the crash, a ride doesn't.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Silent,
    Kick,
    Snare,
    Clap,
    ClosedHat,
    PedalHat,
    OpenHat,
    Crash,
    Ride,
    Tom,
    Rim,
    Claves,
    Cowbell,
    Shaker,
    Tambourine,
}

impl Kind {
    /// The choke group: a hit of the same group fades this drum out.
    pub fn group(self) -> Option<u32> {
        match self {
            Kind::Kick => Some(0),
            Kind::Snare => Some(1),
            Kind::Clap => Some(2),
            Kind::ClosedHat | Kind::PedalHat | Kind::OpenHat => Some(3),
            Kind::Crash => Some(4),
            _ => None,
        }
    }
}

/// What a note plays: the drum, its tuning in semitones from the drum's own, and whether
/// it is the drum's variant (a softer kick, a snappier snare).
pub fn kind_of(model: f32, pitch: f32) -> (Kind, f32, bool) {
    let key = pitch_of(pitch).round() as i32;
    let model = if model.is_nan() { -1 } else { clampf(model.round(), -1.0, 99.0) as i32 };
    match model {
        0 => (Kind::Kick, 0.0, false),
        1 => (Kind::Snare, 0.0, false),
        2 => (Kind::Clap, 0.0, false),
        3 => (
            match key {
                44 => Kind::PedalHat,
                46 => Kind::OpenHat,
                _ => Kind::ClosedHat,
            },
            0.0,
            false,
        ),
        // Chromatic, A2 (45) at the tune.
        4 => (Kind::Tom, (pitch_of(pitch) - 45.0).clamp(-36.0, 36.0), false),
        5 => (Kind::Rim, 0.0, false),
        6 => (Kind::Cowbell, 0.0, false),
        7 => (Kind::Crash, 0.0, false),
        8 => (Kind::Shaker, 0.0, false),
        m if m >= 0 => (Kind::Silent, 0.0, false),
        // The General MIDI kit.
        _ => match key {
            35 => (Kind::Kick, -2.0, true),
            36 => (Kind::Kick, 0.0, false),
            37 => (Kind::Rim, 0.0, false),
            38 => (Kind::Snare, 0.0, false),
            39 => (Kind::Clap, 0.0, false),
            40 => (Kind::Snare, 2.0, true),
            41 | 43 | 45 | 47 | 48 | 50 => (Kind::Tom, (key - 45) as f32 * 1.5, false),
            42 => (Kind::ClosedHat, 0.0, false),
            44 => (Kind::PedalHat, 0.0, false),
            46 => (Kind::OpenHat, 0.0, false),
            49 => (Kind::Crash, 0.0, false),
            57 => (Kind::Crash, 2.0, false),
            51 => (Kind::Ride, 0.0, false),
            59 => (Kind::Ride, 2.0, false),
            54 => (Kind::Tambourine, 0.0, false),
            56 => (Kind::Cowbell, 0.0, false),
            69 => (Kind::Shaker, 0.0, false),
            70 => (Kind::Shaker, 3.0, false),
            75 => (Kind::Claves, 0.0, false),
            _ => (Kind::Silent, 0.0, false),
        },
    }
}

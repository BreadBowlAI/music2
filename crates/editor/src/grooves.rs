//! Grooves (NOTE-7): what gives notes a feel. One mechanism: a step (an eighth or a sixteenth of a
//! beat), and for each step of a cycle (a pair for swing, a beat for the others) a timing and a
//! velocity, either of which may be none. Steps count from each bar's start, in the song's time
//! signature. A note belongs to the step whose straight place it starts from a quarter step before
//! to 45% of a step after; it moves to the step's place where the groove times that step, keeping
//! its end, and takes the step's velocity where the groove sets one. A beat that holds a picked
//! note on no step (a triplet, a sixteenth under an eighths' swing, a 32nd) is another subdivision,
//! and is left whole. So a groove applied twice is applied once, and one that times the same steps
//! (one swing after another, Pushed after Laid back) puts the notes at its own places.
//!
//! Pure: the `groove` clip gesture (`clips.rs`) applies it to a clip's notes as one Tx.

use std::collections::BTreeSet;

use m2_song::{Arrangement, Note};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A swing's step: what it pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Eighths,
    Sixteenths,
}

impl Step {
    /// How long a step is, in beats.
    pub fn beats(self) -> f64 {
        match self {
            Step::Eighths => 0.5,
            Step::Sixteenths => 0.25,
        }
    }
}

/// A feel (NOTE-7), named in a request by `name`: `{"name": "swing", "amount": 0.6, "step":
/// "sixteenths"}`, `{"name": "laid-back"}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Groove {
    /// The second step of each pair at `amount` of the pair: 0.5 is straight, 0.71 the most. A
    /// triplet feel (a shuffle) is about two thirds.
    Swing { amount: f64, step: Step },
    /// The three sixteenths after each beat 1/32 of a beat late.
    LaidBack,
    /// The same, 1/32 of a beat early.
    Pushed,
    /// Velocities only: the beat 0.9, the "and" 0.75, the other two sixteenths 0.6.
    Accents,
}

/// The swings the roll offers, as shares of a pair (NOTE-7).
pub const SWINGS: [f64; 5] = [0.5, 0.55, 0.6, 0.66, 0.71];
/// The most swing: past it, a swung note would be nearer the next step than its own.
const MOST: f64 = 0.71;

/// Each groove's name, label, what it does in plain words, and what a request writes for it: what
/// `GET /api/theory` lists.
pub fn named() -> Vec<(&'static str, &'static str, &'static str, Value)> {
    vec![
        (
            "swing",
            "Swing",
            "Every second eighth or sixteenth a little later, for a bouncy feel; about 66% is a shuffle",
            json!({"name": "swing", "amount": 0.6, "step": "sixteenths"}),
        ),
        ("laid-back", "Laid back", "The notes between the beats a little late (1/32 of a beat): relaxed", json!({"name": "laid-back"})),
        ("pushed", "Pushed", "The notes between the beats a little early (1/32 of a beat): urgent", json!({"name": "pushed"})),
        (
            "accents",
            "Accents",
            "Loud on the beat, softer between: the beat 90%, the half beat 75%, the rest 60%",
            json!({"name": "accents"}),
        ),
    ]
}

/// One step of a groove's cycle: where a note on it goes, in beats after its straight place (None:
/// it stays), and the velocity it takes (None: its own).
#[derive(Clone, Copy)]
struct Slot {
    shift: Option<f64>,
    velocity: Option<f64>,
}

const STAY: Slot = Slot { shift: None, velocity: None };

/// Where a note in a step's reach may start, in steps from the step's straight place.
const EARLY: f64 = 0.25;
const LATE: f64 = 0.45;
/// The shortest a moved note gets, in beats.
const SHORTEST: f64 = 1.0 / 64.0;
/// Rounding room, in beats.
const EPS: f64 = 1e-9;

impl Groove {
    /// The groove's step in beats and its cycle of slots, or why it can't be.
    fn template(&self) -> Result<(f64, Vec<Slot>), String> {
        let late = |by: f64| Slot { shift: Some(by), velocity: None };
        Ok(match *self {
            Groove::Swing { amount, step } => {
                if !(amount.is_finite() && (0.5 - EPS..=MOST + EPS).contains(&amount)) {
                    return Err(format!("Swing goes from 50% to 71%, not {}%", m2_song::lane::number(amount * 100.0)));
                }
                let s = step.beats();
                (s, vec![STAY, late((2.0 * amount - 1.0) * s)])
            }
            Groove::LaidBack => (0.25, vec![STAY, late(1.0 / 32.0), late(1.0 / 32.0), late(1.0 / 32.0)]),
            Groove::Pushed => (0.25, vec![STAY, late(-1.0 / 32.0), late(-1.0 / 32.0), late(-1.0 / 32.0)]),
            Groove::Accents => {
                let hit = |v: f64| Slot { shift: None, velocity: Some(v) };
                (0.25, vec![hit(0.9), hit(0.6), hit(0.75), hit(0.6)])
            }
        })
    }
}

/// Beats as the roll writes them: to a millionth of a beat, never -0.
fn clean(x: f64) -> f64 {
    (x * 1e6).round() / 1e6 + 0.0
}

/// Where a note falls on a groove's grid: on a step (its straight place in song beats, and its
/// index from its bar's start), or on none; and the beat it is in (bar, beat of the bar).
enum Place {
    On { at: f64, step: usize, beat: (u64, u64) },
    Off { beat: (u64, u64) },
}

/// Where song beat `t` falls on a grid of `step` beats counted from each bar's start in `arr`.
fn place(t: f64, step: f64, arr: &Arrangement) -> Place {
    let (bar, into) = arr.bar_at(t + EPS);
    let into = (into - EPS).max(0.0);
    let q = into / step;
    let k = (q + EARLY).floor();
    if q > k + LATE + EPS {
        return Place::Off { beat: (bar, (into + EPS).floor() as u64) };
    }
    // A step at the bar's end is the next bar's first.
    let at = t - into + k * step;
    let (bar, into) = arr.bar_at(at + EPS);
    let k = ((into - EPS).max(0.0) / step).round() as usize;
    Place::On { at, step: k, beat: (bar, (k as f64 * step + EPS).floor() as u64) }
}

/// Groove `g` on the notes at `picked` of a clip that starts at song beat `start`, in a song of
/// arrangement `arr`, in place (NOTE-7). Refused in words, changing nothing, when the groove can't
/// be.
pub fn apply(notes: &mut [Note], picked: &[usize], start: f64, arr: &Arrangement, g: &Groove) -> Result<(), String> {
    let (step, slots) = g.template()?;
    let places: Vec<Place> = picked.iter().map(|&i| place(start + notes[i].start, step, arr)).collect();
    // The beats another subdivision plays in: left whole.
    let other: BTreeSet<(u64, u64)> = places.iter().filter_map(|p| if let Place::Off { beat } = p { Some(*beat) } else { None }).collect();
    for (&i, p) in picked.iter().zip(&places) {
        let Place::On { at, step: k, beat } = *p else { continue };
        if other.contains(&beat) {
            continue;
        }
        let slot = slots[k % slots.len()];
        let n = &mut notes[i];
        if let Some(by) = slot.shift {
            let end = n.start + n.length;
            let to = clean((at + by - start).max(0.0));
            n.length = clean((end - to).max(SHORTEST));
            n.start = to;
        }
        if let Some(v) = slot.velocity {
            n.velocity = v;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "grooves_tests.rs"]
mod tests;

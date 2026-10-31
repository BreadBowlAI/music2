//! Grooves (NOTE-7): what gives notes a feel. One mechanism: a step (an eighth or a sixteenth of a
//! beat), and for each step of a beat (or of a pair, for swing) a timing and a velocity, either of
//! which may be none. A note belongs to the step whose straight place it starts from a quarter step
//! before to 45% of a step after; it moves to the step's place where the groove times that step,
//! keeping its end, and takes the step's velocity where the groove sets one. Notes near no step
//! stay. So a groove applied twice is applied once, and one that times the same steps (one swing
//! after another, Pushed after Laid back) puts the notes at its own places.
//!
//! Pure: the `groove` clip gesture (`clips.rs`) applies it to a clip's notes as one Tx.

use m2_song::Note;
use serde::{Deserialize, Serialize};

/// A feel (NOTE-7), named in a request by `name`: `{"name": "swing", "amount": 0.6, "step":
/// 0.25}`, `{"name": "laid-back"}`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case")]
pub enum Groove {
    /// The second step of each pair at `amount` of the pair: 0.5 is straight, 0.71 the most.
    /// `step` is an eighth (0.5) or a sixteenth (0.25) of a beat.
    Swing { amount: f64, step: f64 },
    /// The second eighth of each beat at 2/3 of the beat: a triplet feel.
    Shuffle,
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

/// Each groove's name, label and what it does, in plain words: what `GET /api/theory` lists.
pub const NAMED: [(&str, &str, &str); 5] = [
    ("swing", "Swing", "Every second eighth or sixteenth a little later, for a bouncy feel"),
    ("shuffle", "Shuffle", "Every second eighth at the third triplet of the beat: blues, boogie"),
    ("laid-back", "Laid back", "The notes after each beat a little late (1/32 of a beat): relaxed"),
    ("pushed", "Pushed", "The notes after each beat a little early (1/32 of a beat): urgent"),
    ("accents", "Accents", "Loud on the beat, softer between: the beat 90%, the \"and\" 75%, the rest 60%"),
];

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

impl Groove {
    /// The groove's step in beats and its cycle of slots, or why it can't be.
    fn template(&self) -> Result<(f64, Vec<Slot>), String> {
        let late = |by: f64| Slot { shift: Some(by), velocity: None };
        Ok(match *self {
            Groove::Swing { amount, step } => {
                if !(amount.is_finite() && (0.5 - 1e-9..=MOST + 1e-9).contains(&amount)) {
                    return Err(format!("Swing goes from 50% to 71%, not {}%", m2_song::lane::number(amount * 100.0)));
                }
                if step != 0.5 && step != 0.25 {
                    return Err(format!("Swing pairs eighths (0.5) or sixteenths (0.25), not {} beat", m2_song::lane::number(step)));
                }
                (step, vec![STAY, late((2.0 * amount - 1.0) * step)])
            }
            Groove::Shuffle => (0.5, vec![STAY, late(1.0 / 6.0)]),
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

/// Groove `g` on the notes at `picked` of a clip that starts at song beat `start`, in place
/// (NOTE-7). Steps are counted from the song's start. Refused in words, changing nothing, when the
/// groove can't be.
pub fn apply(notes: &mut [Note], picked: &[usize], start: f64, g: &Groove) -> Result<(), String> {
    let (step, slots) = g.template()?;
    for &i in picked {
        let n = &mut notes[i];
        let at = (start + n.start) / step;
        let k = (at + EARLY).floor();
        if at > k + LATE + 1e-9 {
            continue;
        }
        let slot = slots[(k as i64).rem_euclid(slots.len() as i64) as usize];
        if let Some(by) = slot.shift {
            let end = n.start + n.length;
            let to = clean((k * step + by - start).max(0.0));
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

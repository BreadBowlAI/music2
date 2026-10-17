//! The song bar's builders (design-8b §4.1): the tempo and the time signature, written in
//! `data/arrangement.json` (FMT-13), and the song's title, written in `music2.toml`.
//!
//! - **Tempo** sets the entry in effect at a beat (the playhead's), adds a change at a beat, or
//!   removes one; the first entry is the song's own and stays. One `Confined {}` Tx: check 5 admits
//!   the arrangement it writes, and nothing else may change.
//! - **Time signature** sets the meter in effect at a beat, keeping its beat, adds a change on the
//!   bar line at or before a beat, or removes one. A change elsewhere moves the bar lines after it,
//!   so every later change keeps its bar: it goes to where that bar now starts, on a bar line
//!   (FMT-13's rule). So an edit and its inverse give back the same file, and nothing is refused
//!   for landing on the change before it.
//! - **Title** is a `TextEdit` of `[project] name`, written as a TOML basic string: `Identical`,
//!   since no graph names the song. The folder keeps its name (gaps #28).
//!
//! The arrangement is written whole, in its canonical form, based on the accepted text; undo puts
//! the old bytes back.

use std::path::Path;

use m2_session::{Checked, Session};
use m2_song::tx::{Confined, Expect, Op, Payload};
use m2_song::{Arrangement, DataPath, Meter, Tempo};

use crate::plan::{self, path};
use crate::{EditError, Edited, ready};

/// The file the tempo and the meter are written in.
pub const FILE: &str = "data/arrangement.json";

/// `124 BPM`: up to 2 decimals, trailing zeros trimmed (taste.md, Words and numbers).
pub fn bpm_words(bpm: f64) -> String {
    let s = format!("{bpm:.2}");
    format!("{} BPM", s.trim_end_matches('0').trim_end_matches('.'))
}

/// Where an entry starts, in bars counted from 1: `bar 9`, or `9.3` off a bar line (bar and beat).
pub fn at_words(a: &Arrangement, beat: f64) -> String {
    let (bar, into) = a.bar_at(beat);
    match into == 0.0 {
        true => format!("bar {}", bar + 1),
        false => format!("{}.{}", bar + 1, m2_song::lane::number(into + 1.0)),
    }
}

/// A tempo entry as history names it: `124 BPM`, or `128 BPM from bar 9` for a later one.
fn tempo_words(a: &Arrangement, t: &Tempo) -> String {
    match t.beat == 0.0 {
        true => bpm_words(t.bpm),
        false => format!("{} from {}", bpm_words(t.bpm), at_words(a, t.beat)),
    }
}

/// A meter entry as history names it: `4/4`, or `7/8 from bar 9`.
fn meter_words(a: &Arrangement, m: &Meter) -> String {
    match m.beat == 0.0 {
        true => format!("{}/{}", m.beats, m.unit),
        false => format!("{}/{} from {}", m.beats, m.unit, at_words(a, m.beat)),
    }
}

/// The accepted revision's arrangement.
fn accepted(s: &Session) -> Arrangement {
    s.accepted().project.arrangement.clone()
}

/// The index of the entry in effect at `beat`: the last one at or before it (the first is at 0).
fn in_effect<T>(entries: &[T], beat: f64, at: impl Fn(&T) -> f64) -> usize {
    entries.partition_point(|e| at(e) <= beat).saturating_sub(1)
}

fn finite(beat: f64) -> Result<f64, EditError> {
    match beat.is_finite() {
        true => Ok(beat.max(0.0)),
        false => Err(EditError::Rejected(format!("{beat} isn't a beat"))),
    }
}

/// Each meter's bar, counted from the song's first (0): what a change keeps when one before it
/// changes. Every change of an accepted arrangement is on a bar line (FMT-13).
fn bars(meter: &[Meter]) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::with_capacity(meter.len());
    for (i, m) in meter.iter().enumerate() {
        let bar = match i.checked_sub(1) {
            None => 0,
            Some(p) => out[p] + ((m.beat - meter[p].beat) / meter[p].bar()).round() as u64,
        };
        out.push(bar);
    }
    out
}

/// Put every meter change after the one at `from` at its bar, `bars[j]`, in the meters before it
/// now: a change at or before `from` moved the bar lines (FMT-13). A change keeps its bar, not its
/// beat, and stays at least a bar after the one before it.
fn keep_bars(a: &mut Arrangement, bars: &[u64], from: usize) {
    for j in from + 1..a.meter.len() {
        let prev = a.meter[j - 1];
        // Bars of `n/2^k` are exact binary fractions, so this is exact.
        a.meter[j].beat = prev.beat + bars[j].saturating_sub(bars[j - 1]).max(1) as f64 * prev.bar();
    }
}

/// Whether two arrangements count bars and beats the same: no meter written is 4/4 from the start.
fn plays_same(a: &Arrangement, b: &Arrangement) -> bool {
    let meters = |x: &Arrangement| if x.meter.is_empty() { vec![Meter::COMMON] } else { x.meter.clone() };
    a.tempo == b.tempo && meters(a) == meters(b)
}

/// Write `after` over the accepted arrangement, as one `Confined {}` Tx labelled `label`, and assert
/// on the checked candidate that it plays exactly `after`. Writing what is there already writes
/// nothing.
fn write(s: &mut Session, after: Arrangement, label: (String, String)) -> Result<Edited, EditError> {
    let text = after.canonical();
    // The reader's own refusal (FMT-13), in its words: 20 to 999 BPM, and the rest.
    let read = Arrangement::read(&text).map_err(|d| EditError::Rejected(d.message.replace(&format!("{FILE}: "), "")))?;
    let rev = s.accepted();
    if plays_same(&read, &rev.project.arrangement) {
        return Ok(Edited { revision: rev.number, file: FILE.into(), before: label.0.clone(), after: label.0 });
    }
    let file = path(FILE)?;
    let op = Op::Write { file, bytes: Payload::Text(text.into()) };
    let tx = plan::planned(s, label.clone(), vec![op], Expect::Confined(Confined::default()));
    let done = plan::run(s, tx, move |_, checked: &Checked| match checked.candidate().project.arrangement == read {
        true => Ok(()),
        false => Err(EditError::Rejected("the arrangement wouldn't be what was asked".into())),
    })?;
    Ok(Edited { revision: done.number, file: FILE.into(), before: label.0, after: label.1 })
}

/// A number to write as a tempo: the reader then says whether it is one (20 to 999).
fn bpm_of(bpm: f64) -> Result<f64, EditError> {
    match bpm.is_finite() {
        true => Ok(bpm),
        false => Err(EditError::Rejected(format!("{bpm} isn't a tempo"))),
    }
}

/// Set the tempo in effect at `at` (a beat: the playhead's) to `bpm`.
pub fn tempo(s: &mut Session, at: f64, bpm: f64) -> Result<Edited, EditError> {
    let (at, bpm) = (finite(at)?, bpm_of(bpm)?);
    let mut a = accepted(s);
    let i = in_effect(&a.tempo, at, |t| t.beat);
    let before = tempo_words(&a, &a.tempo[i]);
    a.tempo[i].bpm = bpm;
    let after = tempo_words(&a, &a.tempo[i]);
    write(s, a, (before, after))
}

/// Add a tempo change at `beat` (one there already is set instead).
pub fn tempo_change(s: &mut Session, beat: f64, bpm: f64) -> Result<Edited, EditError> {
    let (beat, bpm) = (finite(beat)?, bpm_of(bpm)?);
    let mut a = accepted(s);
    let i = in_effect(&a.tempo, beat, |t| t.beat);
    let before = tempo_words(&a, &a.tempo[i]);
    match a.tempo[i].beat == beat {
        true => a.tempo[i].bpm = bpm,
        false => a.tempo.insert(i + 1, Tempo { beat, bpm }),
    }
    let after = tempo_words(&a, &Tempo { beat, bpm });
    write(s, a, (before, after))
}

/// Remove the tempo change at `beat`. The first entry is the song's tempo, and stays.
pub fn remove_tempo(s: &mut Session, beat: f64) -> Result<Edited, EditError> {
    let mut a = accepted(s);
    let Some(i) = a.tempo.iter().position(|t| t.beat == beat) else {
        return Err(EditError::NotFound(format!("a tempo change at beat {}", m2_song::lane::number(beat))));
    };
    if i == 0 {
        return Err(EditError::Rejected("the song's own tempo can't be removed: change it instead".into()));
    }
    let before = tempo_words(&a, &a.tempo[i]);
    a.tempo.remove(i);
    let after = tempo_words(&a, &a.tempo[i - 1]);
    write(s, a, (before, after))
}

/// Remove the tempo change and the time signature change at `beat`, whichever are there, in one
/// step: a ruler mark's Remove these changes. The song's own stay.
pub fn remove_changes(s: &mut Session, beat: f64) -> Result<Edited, EditError> {
    let mut a = accepted(s);
    let t = a.tempo.iter().position(|t| t.beat == beat).filter(|&i| i > 0);
    let m = a.meter.iter().position(|m| m.beat == beat).filter(|&i| i > 0);
    let words = |a: &Arrangement, t: Option<&Tempo>, m: Option<&Meter>| match (t, m) {
        (Some(t), Some(m)) if m.beat > 0.0 && t.beat == m.beat => format!("{} and {}/{} from {}", bpm_words(t.bpm), m.beats, m.unit, at_words(a, m.beat)),
        (Some(t), Some(m)) => format!("{} and {}", tempo_words(a, t), meter_words(a, m)),
        (Some(t), None) => tempo_words(a, t),
        (None, Some(m)) => meter_words(a, m),
        (None, None) => String::new(),
    };
    let before = words(&a, t.map(|i| &a.tempo[i]), m.map(|i| &a.meter[i]));
    if before.is_empty() {
        return Err(EditError::NotFound(format!("a tempo or time signature change at beat {}", m2_song::lane::number(beat))));
    }
    if let Some(i) = t {
        a.tempo.remove(i);
    }
    if let Some(i) = m {
        let mut bars = bars(&a.meter);
        a.meter.remove(i);
        bars.remove(i);
        keep_bars(&mut a, &bars, i - 1);
    }
    let after = words(&a, t.map(|i| &a.tempo[i - 1]), m.map(|i| &a.meter[i - 1]));
    write(s, a, (before, after))
}

/// A time signature FMT-13 allows: 1 to 64 beats of a whole, half, quarter, … or 32nd note.
fn meter_of(beat: f64, beats: u32, unit: u32) -> Result<Meter, EditError> {
    if !(1..=64).contains(&beats) {
        return Err(EditError::Rejected(format!("a bar has 1 to 64 beats, not {beats}")));
    }
    if ![1, 2, 4, 8, 16, 32].contains(&unit) {
        return Err(EditError::Rejected(format!("a beat is a 1, 2, 4, 8, 16 or 32 note, not {unit}")));
    }
    Ok(Meter { beat, beats, unit })
}

/// The meters written, or 4/4 from the start when none is.
fn meters(a: &mut Arrangement) {
    if a.meter.is_empty() {
        a.meter.push(Meter::COMMON);
    }
}

/// Set the time signature in effect at `at` to `beats`/`unit`, keeping where it starts.
pub fn meter(s: &mut Session, at: f64, beats: u32, unit: u32) -> Result<Edited, EditError> {
    let at = finite(at)?;
    let mut a = accepted(s);
    meters(&mut a);
    let bars = bars(&a.meter);
    let i = in_effect(&a.meter, at, |m| m.beat);
    let before = meter_words(&a, &a.meter[i]);
    a.meter[i] = meter_of(a.meter[i].beat, beats, unit)?;
    let after = meter_words(&a, &a.meter[i]);
    keep_bars(&mut a, &bars, i);
    write(s, a, (before, after))
}

/// Add a time signature change on the bar line at or before `at`, in the meter before it (one
/// there already is set instead; at the first bar, the song's own is).
pub fn meter_change(s: &mut Session, at: f64, beats: u32, unit: u32) -> Result<Edited, EditError> {
    let at = finite(at)?;
    let mut a = accepted(s);
    meters(&mut a);
    let mut bars = bars(&a.meter);
    let i = in_effect(&a.meter, at, |m| m.beat);
    let prev = a.meter[i];
    let n = ((at - prev.beat) / prev.bar()).floor();
    let beat = prev.beat + n * prev.bar();
    let before = meter_words(&a, &prev);
    let m = meter_of(beat, beats, unit)?;
    let from = match prev.beat == beat {
        true => {
            a.meter[i] = m;
            i
        }
        false => {
            a.meter.insert(i + 1, m);
            bars.insert(i + 1, bars[i] + n as u64);
            i + 1
        }
    };
    let after = meter_words(&a, &m);
    keep_bars(&mut a, &bars, from);
    write(s, a, (before, after))
}

/// Remove the time signature change at `beat`. The first is the song's own, and stays.
pub fn remove_meter(s: &mut Session, beat: f64) -> Result<Edited, EditError> {
    let mut a = accepted(s);
    let Some(i) = a.meter.iter().position(|m| m.beat == beat) else {
        return Err(EditError::NotFound(format!("a time signature change at beat {}", m2_song::lane::number(beat))));
    };
    if i == 0 {
        return Err(EditError::Rejected("the song's own time signature can't be removed: change it instead".into()));
    }
    let mut bars = bars(&a.meter);
    let before = meter_words(&a, &a.meter[i]);
    a.meter.remove(i);
    bars.remove(i);
    let after = meter_words(&a, &a.meter[i - 1]);
    keep_bars(&mut a, &bars, i - 1);
    write(s, a, (before, after))
}

/// The byte range of `[project] name`'s value in a manifest, quotes and all, however it is written.
fn name_span(text: &str) -> Option<std::ops::Range<usize>> {
    #[derive(serde::Deserialize)]
    struct M {
        project: P,
    }
    #[derive(serde::Deserialize)]
    struct P {
        name: toml::Spanned<String>,
    }
    Some(toml::from_str::<M>(text).ok()?.project.name.span())
}

/// Call the song `name`: its `[project] name` in `music2.toml`, verified to change nothing that
/// plays. Undo puts the old name back.
pub fn title(s: &mut Session, name: &str) -> Result<Edited, EditError> {
    ready(s)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(EditError::Rejected("A song needs a name".into()));
    }
    if name.chars().any(char::is_control) {
        return Err(EditError::Rejected("a song's name is one line of text".into()));
    }
    let rev = s.accepted();
    let manifest = DataPath::Manifest.path();
    let file = m2_song::path::MANIFEST;
    let text = rev.snapshot.files.get(Path::new(&manifest)).cloned().ok_or_else(|| EditError::NotFound(file.into()))?;
    let before = rev.project.manifest.project.name.clone();
    if name == before {
        return Ok(Edited { revision: rev.number, file: file.into(), before: before.clone(), after: before });
    }
    let span = name_span(&text).ok_or_else(|| EditError::Rejected(format!("{file} doesn't say the song's name where it can be changed")))?;
    // A JSON string of one line is a TOML basic string: `"`, `\` and controls escaped.
    let written = serde_json::to_string(name).expect("a string serialises");
    let op = plan::text_edit(&path(file)?, &text, &[(span.start as u32, span.end as u32, written)]);
    let tx = plan::planned(s, (before.clone(), name.to_string()), vec![op], Expect::Identical);
    let want = name.to_string();
    let done = plan::run(s, tx, move |_, checked: &Checked| match checked.candidate().project.manifest.project.name == want {
        true => Ok(()),
        false => Err(EditError::Rejected("the manifest wouldn't name the song that".into())),
    })?;
    Ok(Edited { revision: done.number, file: file.into(), before, after: name.to_string() })
}

#[cfg(test)]
#[path = "song_tests.rs"]
mod tests;

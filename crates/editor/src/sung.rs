//! A voice's notes in the editor (VOX-40 to VOX-44): Find notes, which turns a measure's runs into
//! a clip's `sung` notes, and the rule that a clip keeps only the notes the part of its file it
//! plays reaches. The musical rules are the measure's, in its pack (`@m2/vocals:notes`); here is only
//! the turning of its numbers into notes.

use std::path::PathBuf;

use m2_runtime::PackageSet;
use m2_runtime::measure::Measured;
use m2_session::Session;
use m2_song::lane::kernel_id;
use m2_song::{AudioClip, Sung, TempoMap};

use crate::EditError;

/// Round `x` to `places` decimals: what Find notes writes (0.1 ms, a cent).
fn rounded(x: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (x * k).round() / k
}

/// Whether `n` reaches the part of the file from `a` to `b` seconds.
fn reaches(n: &Sung, (a, b): (f64, f64)) -> bool {
    n.end() > a && n.at < b
}

/// Keep only the sung notes of `c` that reach the part of its file it plays (VOX-44).
pub fn trim(c: &mut AudioClip, tempo: &TempoMap) {
    if c.sung.is_empty() {
        return;
    }
    let span = m2_runtime::audio::file_span(c, tempo);
    c.sung.retain(|n| reaches(n, span));
}

/// Why Find notes can't hear clip `c`, if it can't: it plays backwards, or at another pitch.
pub fn refusal(c: &AudioClip) -> Option<String> {
    if c.reverse {
        return Some("Find notes hears a voice played forwards: turn Reverse off first.".into());
    }
    if c.pitch != 0.0 {
        return Some("Find notes hears a voice at its own pitch: set the clip's pitch back to 0 first.".into());
    }
    None
}

/// The notes measure `m` found that reach the part of its file clip `c` plays: each run of one
/// `note` number, its `centre` its pitch (VOX-43).
pub fn found(c: &AudioClip, measure: &str, m: &Measured, tempo: &TempoMap) -> Result<Vec<Sung>, EditError> {
    let (Some(note), Some(centre)) = (m.output("note"), m.output("centre")) else {
        return Err(EditError::Rejected(format!("`{measure}` doesn't find notes: Find notes needs a measure that writes `note` and `centre`")));
    };
    let span = m2_runtime::audio::file_span(c, tempo);
    let mut out = vec![];
    let mut k = 0;
    while k < note.len() {
        let n = note[k];
        let from = k;
        while k < note.len() && note[k] == n {
            k += 1;
        }
        if n == 0.0 || !n.is_finite() {
            continue;
        }
        let at = rounded(m.seconds(from), 4);
        let length = rounded(rounded(m.seconds(k), 4) - at, 4);
        let pitch = rounded(centre[from] as f64, 2).clamp(0.0, 127.0);
        let s = Sung { at, length, pitch, move_by: 0.0, straighten: 0.0 };
        if length > 0.0 && reaches(&s, span) {
            out.push(s);
        }
    }
    if out.is_empty() {
        return Err(EditError::Rejected("No sung notes were found in this clip: Find notes hears one voice singing.".into()));
    }
    Ok(out)
}

/// Each edited note of `old` gives its `move` and `straighten` to the note of `new` that overlaps
/// it by more than half its length (D8): Find notes run again keeps the edits.
pub fn keep_edits(old: &[Sung], new: &mut [Sung]) {
    for o in old.iter().filter(|o| o.edited()) {
        let over = |n: &Sung| (n.end().min(o.end()) - n.at.max(o.at)).max(0.0);
        if let Some(n) = new.iter_mut().filter(|n| over(n) > 0.5 * o.length).max_by(|a, b| over(a).total_cmp(&over(b))) {
            (n.move_by, n.straighten) = (o.move_by, o.straighten);
        }
    }
}

/// Find notes on clip `c` (VOX-43): its notes from what measure `measure` found, the edits it had
/// kept, and `render` to play them.
pub fn find(c: &mut AudioClip, measure: &str, render: &str, m: &Measured, tempo: &TempoMap) -> Result<(), EditError> {
    if let Some(why) = refusal(c) {
        return Err(EditError::Rejected(why));
    }
    if !kernel_id(render) {
        return Err(EditError::Rejected(format!("`{render}` isn't a kernel: name the one that plays edited notes, as `@m2/vocals:voice`")));
    }
    let mut notes = found(c, measure, m, tempo)?;
    keep_edits(&c.sung, &mut notes);
    c.sung = notes;
    c.render = Some(render.to_string());
    Ok(())
}

/// The packages a measure and a renderer are run from: the song's, and their packs from the store
/// when the song doesn't list them yet (PACK-10: Find notes adds them). Refused in words when a
/// pack isn't installed, or `measure` isn't a measure, or `render` no offline kernel that makes
/// sound.
pub fn packages_for(s: &Session, measure: &str, render: &str) -> Result<PackageSet, EditError> {
    let set = packages_with(s, &[measure, render])?;
    if set.offline_decl(render).is_none_or(|d| d.hop > 0) {
        return Err(EditError::Rejected(format!("`{render}` doesn't play edited notes: name an offline kernel, as `@m2/vocals:voice`")));
    }
    Ok(set)
}

/// The packages that hold the measure `measure`, as [`packages_for`] makes them: for the measure
/// route (VOX-41).
pub fn packages_measuring(s: &Session, measure: &str) -> Result<PackageSet, EditError> {
    packages_with(s, &[measure])
}

/// The song's packages, and from the store each pack of `ids` the song doesn't list; the first
/// of `ids` a measure.
fn packages_with(s: &Session, ids: &[&str]) -> Result<PackageSet, EditError> {
    let measure = ids[0];
    let rev = s.accepted();
    let mut set = s.packages.viewing(&rev.compiled.view).into_owned();
    let mut packs = set.packs();
    for &id in ids {
        let Some((package, _)) = m2_song::pack::split_id(id) else {
            return Err(EditError::Rejected(format!("`{id}` isn't a kernel's id: `<pack>:<name>`, as `@m2/vocals:notes`")));
        };
        if package == m2_song::pack::STD || packs.iter().any(|(n, _)| n == package) {
            continue;
        }
        let (_, dir) = s
            .offered_one(package)
            .ok_or_else(|| EditError::Rejected(format!("{package} isn't installed: install it with `m2 pack install <folder>`")))?;
        packs.push((package.to_string(), dir.to_path_buf()));
    }
    let view = set.view_of(&packs).map_err(|e| EditError::Rejected(format!("{e:#}")))?;
    set.show(&view);
    if set.offline_decl(measure).is_none_or(|d| d.hop == 0) {
        return Err(EditError::Rejected(format!("`{measure}` isn't a measure: a measure is an offline kernel with a hop, as `@m2/vocals:notes`")));
    }
    Ok(set)
}

/// The file clip `clip` of the accepted revision plays.
pub fn file_of(s: &Session, clip: &crate::clips::ClipRef) -> Result<PathBuf, EditError> {
    let rev = s.accepted();
    let c = crate::clips::audio_clip(rev, clip)?;
    rev.compiled.data.asset_path(&c.asset).ok_or_else(|| EditError::Rejected(format!("{} isn't there to be heard.", c.asset)))
}

#[cfg(test)]
#[path = "sung_tests.rs"]
mod tests;

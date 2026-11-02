//! The song's key (the polish plan's D2, Q22): `layout.json`'s `key`, as
//! `{ "root": "A", "scale": "minor" }`. Shared with the song in git, like the rest of the layout,
//! and nothing that plays reads it (PLANE-3): Notes shades the rows out of the key, can lock notes to
//! it and stamps its chords. A scale is one of the table's (`scales.rs`, NOTE-1), or none.
//!
//! Written by a Tx of `layout.json` alone: no revision, one undo step, as a reorder is. Like
//! reorder and recolour it never writes over a `layout.json` that doesn't read (R15), and it keeps
//! every other key of the file as read.

use std::path::Path;

use m2_session::Session;
use m2_song::tx::Expect;
use serde::{Deserialize, Serialize};

use crate::layout::{self, J, Layout, Read};
use crate::scales::Scale;
use crate::{EditError, Edited, plan};

/// The roots as `layout.json` writes them, by pitch class from C. The app spells them with ♯ and ♭
/// (Q32); the file keeps to ASCII, so a hand or a script can write it.
pub const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];
/// A key: its root (one of [`ROOTS`]) and its scale, one of the table's (NOTE-1). Only
/// [`SongKey::new`] makes one, and a request's is read through it, so every key is one that reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Written")]
pub struct SongKey {
    root: String,
    scale: Scale,
}

/// A key as a file or a request writes it, before it is read: what `POST /api/graph/song-key`
/// takes, so a key that doesn't read is refused as an edit, in words ([`set_written`]).
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Written {
    pub root: String,
    pub scale: String,
}

impl TryFrom<Written> for SongKey {
    type Error = String;

    fn try_from(w: Written) -> Result<SongKey, String> {
        SongKey::new(&w.root, &w.scale)
    }
}

impl SongKey {
    /// A key from what a file or a request says: the root by any of its names (`C#`, `C♯`, `Db`,
    /// `D♭`), the scale by its name in any case (`Harmonic Minor`). The root is kept in [`ROOTS`]'s
    /// spelling.
    pub fn new(root: &str, scale: &str) -> Result<SongKey, String> {
        let pc = pitch_class(root).ok_or_else(|| format!("{root} isn't a key's root: one of {}", ROOTS.join(", ")))?;
        let scale = Scale::try_from(scale.to_string())?;
        Ok(SongKey { root: ROOTS[pc].to_string(), scale })
    }

    /// Its root, as [`ROOTS`] spells it: `C#`, `Bb`.
    pub fn root(&self) -> &str {
        &self.root
    }

    /// Its scale.
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// Its name, as the app says it: `E♭ major`, `F♯ minor`, `D Dorian`, `A harmonic minor`. A
    /// mode's name is a name, so it keeps its capital.
    pub fn name(&self) -> String {
        let mode = matches!(self.scale, Scale::Dorian | Scale::Phrygian | Scale::Lydian | Scale::Mixolydian | Scale::Locrian);
        let scale = if mode { self.scale.label().to_string() } else { self.scale.label().to_lowercase() };
        format!("{} {scale}", crate::scales::pc_name(self.pc()))
    }

    fn json(&self) -> J {
        J::Obj(vec![("root".into(), J::Str(self.root.clone())), ("scale".into(), J::Str(self.scale.name().into()))])
    }
}

/// The pitch class of a root's name, from C: a letter, then `#`, `♯`, `b` or `♭`, or nothing.
fn pitch_class(name: &str) -> Option<usize> {
    let mut chars = name.trim().chars();
    let letter = match chars.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let shift = match (chars.next(), chars.next()) {
        (None, _) => 0,
        (Some('#' | '♯'), None) => 1,
        (Some('b' | '♭'), None) => 11,
        _ => return None,
    };
    Some((letter + shift) % 12)
}

/// The key a layout says, if it says one that reads.
pub fn read(l: &Layout) -> Option<SongKey> {
    let (_, J::Obj(fields)) = l.other.iter().find(|(k, _)| k == "key")? else { return None };
    let field = |name: &str| match fields.iter().find(|(k, _)| k == name) {
        Some((_, J::Str(s))) => Some(s.as_str()),
        _ => None,
    };
    SongKey::new(field("root")?, field("scale")?).ok()
}

/// The key `layout.json` in the project at `dir` says now, if any.
pub fn of(dir: &Path) -> Option<SongKey> {
    match layout::read_disk(dir) {
        Read::Valid(l) => read(&l),
        _ => None,
    }
}

/// [`set`] from a key as a request writes it: one that doesn't read is refused in
/// [`SongKey::new`]'s words, writing nothing.
pub fn set_written(s: &mut Session, key: Option<Written>) -> Result<Edited, EditError> {
    let key = key.map(|w| SongKey::new(&w.root, &w.scale)).transpose().map_err(EditError::Rejected)?;
    set(s, key)
}

/// Set the song's key, or with None take it out: one undo step that writes only `layout.json`.
/// Labelled by the keys' names, `("C major", "A minor")`, an empty side for none. Setting the key
/// it has already writes nothing.
pub fn set(s: &mut Session, key: Option<SongKey>) -> Result<Edited, EditError> {
    let mut l = match layout::read_disk(&s.accepted().project.dir) {
        Read::Valid(l) => l,
        Read::Absent => Layout::default(),
        Read::Invalid(_) => return Err(EditError::Rejected("layout.json doesn't read: fix it, or reset the layout".into())),
    };
    let was = read(&l);
    if was == key {
        let same = "no change".to_string();
        return Ok(Edited { revision: s.accepted().number, file: layout::FILE.into(), before: same.clone(), after: same });
    }
    let at = l.other.iter().position(|(k, _)| k == "key");
    match (&key, at) {
        (Some(k), Some(i)) => l.other[i].1 = k.json(),
        // A new key goes right after the markers, before keys this version doesn't know.
        (Some(k), None) => l.other.insert(0, ("key".into(), k.json())),
        (None, Some(i)) => {
            l.other.remove(i);
        }
        (None, None) => {}
    }
    let name = |k: &Option<SongKey>| k.as_ref().map_or(String::new(), SongKey::name);
    let label = (name(&was), name(&key));
    let (op, file, base) = layout::write_op(s, l.canonical());
    let mut tx = plan::planned(s, label.clone(), vec![op], Expect::Identical);
    tx.base.insert(file, base);
    let done = plan::run(s, tx, |_, _| Ok(()))?;
    Ok(Edited { revision: done.number, file: layout::FILE.into(), before: label.0, after: label.1 })
}

#[cfg(test)]
#[path = "songkey_tests.rs"]
mod tests;

//! Setting the song's key (VOX-10): `data/arrangement.json`'s `key`, written with the tempo and the
//! meter, as the song bar writes them (`crate::song`): one undo step in a revision. The key is music,
//! so sound reads it as `time.key` (VOX-12); Notes shades the rows out of the key, can lock notes to
//! it and stamps its chords. What a key is lives with the arrangement (`m2_song::songkey`).

use m2_session::Session;

pub use m2_song::songkey::{ROOTS, SongKey, Written};

use crate::{EditError, Edited};

/// [`set`] from a key as a request writes it: one that doesn't read is refused in
/// [`SongKey::new`]'s words, writing nothing.
pub fn set_written(s: &mut Session, key: Option<Written>) -> Result<Edited, EditError> {
    let key = key.map(|w| SongKey::new(&w.root, &w.scale)).transpose().map_err(EditError::Rejected)?;
    set(s, key)
}

/// Set the song's key, or with None take it out: one undo step that writes only
/// `data/arrangement.json`. Labelled by the keys' names, `("C major", "A minor")`, an empty side
/// for none. Setting the key it has already writes nothing.
pub fn set(s: &mut Session, key: Option<SongKey>) -> Result<Edited, EditError> {
    let rev = s.accepted();
    let mut a = rev.project.arrangement.clone();
    let name = |k: &Option<SongKey>| k.as_ref().map_or(String::new(), SongKey::name);
    let label = (name(&a.key()), name(&key));
    let key = key.map(|k| k.written());
    // The page records no step for it (`setSongKey`).
    if a.key == key {
        let same = "no change".to_string();
        return Ok(Edited { revision: rev.number, file: crate::song::FILE.into(), before: same.clone(), after: same });
    }
    a.key = key;
    crate::song::write(s, a, label, false)
}

#[cfg(test)]
#[path = "songkey_tests.rs"]
mod tests;

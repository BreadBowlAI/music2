//! Setting the song's key (VOX-10): `data/arrangement.json`'s `key`, written with the tempo and the
//! meter, as the song bar writes them (`crate::song`): one undo step in a revision. The key is music,
//! so sound reads it as `time.key` (VOX-12); Notes shades the rows out of the key, can lock notes to
//! it and stamps its chords. What a key is lives with the arrangement (`m2_song::songkey`).

use std::collections::BTreeSet;

use m2_ir::{Node, Op, TimeOf};
use m2_session::{Compiled, Session};

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

/// The calls of `compiled` that read the song's key: each whose body made a `time.key`, as Tune's
/// does, so its card says the key (VOX-26). Worked out from the graph, in one walk for a whole view
/// (review U6): no descriptor says so.
pub fn readers(compiled: &Compiled) -> BTreeSet<u32> {
    let mut sites = BTreeSet::new();
    for m in &compiled.ir.modules {
        crate::controls::walk(&m.nodes, &mut |n: &Node| {
            if matches!(n.op, Op::Time { of: TimeOf::Key }) {
                sites.extend(crate::effect::calls_of(m, n));
            }
        });
    }
    sites
}

#[cfg(test)]
#[path = "songkey_tests.rs"]
mod tests;

//! The shape every builder shares (the step 6 design's 13.4): plan a Tx on the accepted revision,
//! check it, assert what the gesture means on the checked candidate, commit.

use std::collections::BTreeMap;

use m2_session::{Checked, Refusal, Revision, Session};
use m2_song::tx::{Base, Edit, Expect, Op, Pos, Tx, TxPath};
use m2_song::{DataPath, Stored};

use crate::EditError;

/// What the accepted revision has at `p`, as a Tx's base: the text it was planned on (carried,
/// so the commit's outcome doesn't depend on a store), a binary's hash, a folder's directory
/// hash, or Absent.
pub fn base_of(rev: &Revision, p: &TxPath) -> Base {
    let snap = &rev.snapshot;
    let path = p.path();
    if let DataPath::Module { .. } = p.data() {
        let files: Vec<(String, m2_song::Sha)> = snap
            .under(&path)
            .into_iter()
            .filter(|(k, _)| matches!(DataPath::parse(k), DataPath::Lane { .. } | DataPath::PluginState { .. }))
            .filter_map(|(k, _)| Some((m2_song::path::slashed(k.strip_prefix(&path).ok()?), snap.sha(&k)?)))
            .collect();
        return if files.is_empty() { Base::Absent } else { Base::Sha(m2_song::dir_hash(&files)) };
    }
    match (snap.files.get(&path), snap.binaries.get(&path)) {
        (Some(text), _) if !snap.not_utf8.contains_key(&path) => Base::text(text),
        (Some(_), _) => snap.sha(&path).map_or(Base::Absent, Base::Sha),
        (None, Some(Stored::Hash(sha))) => Base::Sha(*sha),
        _ => Base::Absent,
    }
}

/// A Tx of `ops`, labelled `label` for history, judged by `expect`, with a fresh id (FMT-11) and
/// every file it touches based on what the accepted revision has: every position in it is in
/// the text the builder planned on.
pub fn planned(s: &Session, label: (String, String), ops: Vec<Op>, expect: Expect) -> Tx {
    let mut tx = Tx::new(&m2_song::tx::random_id(), label, ops, expect);
    let rev = s.accepted();
    tx.base = tx.touched().into_iter().map(|p| (p.clone(), base_of(rev, &p))).collect::<BTreeMap<_, _>>();
    tx
}

/// The path of a project file as a Tx names it.
pub fn path(file: &str) -> Result<TxPath, EditError> {
    TxPath::new(file).map_err(EditError::Rejected)
}

/// `TextEdit`s replacing each `(start, end, text)` of `file`, whose base text is `old`: each guarded
/// by what it replaces.
pub fn text_edit(file: &TxPath, old: &str, edits: &[(u32, u32, String)]) -> Op {
    let edits = edits
        .iter()
        .map(|(start, end, text)| Edit {
            start: Pos::Byte(*start),
            end: Pos::Byte(*end),
            expected: old[*start as usize..*end as usize].to_string(),
            text: text.clone(),
        })
        .collect();
    Op::TextEdit { file: file.clone(), edits }
}

/// Check `tx`, assert what the gesture means on the checked candidate, and commit it.
pub fn run(
    s: &mut Session,
    tx: Tx,
    assert: impl FnOnce(&Session, &Checked) -> Result<(), EditError>,
) -> Result<m2_session::Committed, EditError> {
    run_as(s, tx, |why| EditError::Rejected(why.to_string()), assert)
}

/// [`run`], saying a broken expectation as the gesture says it: `said` gets core's reason (a
/// `Confined` that a node outside the edit breaks is the planner's "would change more").
pub fn run_as(
    s: &mut Session,
    tx: Tx,
    said: impl Fn(&str) -> EditError,
    assert: impl FnOnce(&Session, &Checked) -> Result<(), EditError>,
) -> Result<m2_session::Committed, EditError> {
    let unexpected = |s: &Session, r: Refusal| match &r {
        Refusal::Rejected { diagnostics, .. } if diagnostics.iter().any(|d| d.code == "tx.unexpected") => {
            said(&diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; "))
        }
        _ => crate::refused(s, r),
    };
    let checked = s.check(&tx).map_err(|r| unexpected(s, r))?;
    assert(s, &checked)?;
    s.commit(tx).map_err(|r| unexpected(s, r))
}

/// A structural planner asserts on positions it planned in the accepted revision: so the file
/// it edits must be on disk as accepted, or it is stale until the change is taken in (a knob's
/// `Values` Tx needs no positions, and rides along with such a change, R2).
pub fn unmoved(s: &Session, checked: &Checked) -> Result<(), EditError> {
    match std::ptr::eq(checked.baseline(), s.accepted()) {
        true => Ok(()),
        false => Err(EditError::Stale { current: s.accepted().number }),
    }
}

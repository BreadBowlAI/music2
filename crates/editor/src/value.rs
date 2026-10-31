//! Set a value (WRITE-10's first gesture, and WRITE-22's commit): one `Values` Tx.
//!
//! A written number is rewritten in place, through the same lens a preview uses, so what plays
//! during a drag is exactly what the commit writes. A declared control a call leaves out is
//! written into the call on its first edit (the step 5 design's D5), as a plain number in base
//! units, anchored by its call and slot (PKG-3). Core verifies that only those constants change,
//! to those values; nothing else may (WRITE-6).

use std::path::Path;

use m2_ir::{Op, Origin};
use m2_session::Session;
use m2_song::tx::{Anchor, Expect, Range, Spot};

use crate::controls::{Audition, Control, Edit, resolve};
use crate::lens::{self, Insertion};
use crate::plan::{self, path};
use crate::{Committed, EditError, ready};

/// Set a written number, or write a declared control into its call. `at` is the revision the
/// caller's control came from. `value` is in the control's units (so `khz(2.5)` set to 900 is
/// written as `khz(0.9)`). With `expect`, the edit applies only if the value is still that.
pub fn set(s: &mut Session, at: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError> {
    ready(s)?;
    if !value.is_finite() {
        return Err(EditError::Rejected(format!("{value} is not a number that can be written")));
    }
    let c = resolve(s, at, id)?;
    c.numbered()?;
    if let Some(e) = expect
        && e != c.value
    {
        return Err(EditError::Conflict { current: c.value });
    }
    let lens = match &c.edit {
        Edit::Literal { lens, .. } => lens.clone(),
        Edit::Insert(insertion) => return insert(s, &c, &insertion.clone(), value),
    };
    let edit = lens.propose(value)?;
    let unchanged = Committed {
        revision: s.accepted().number,
        file: c.file.clone(),
        before: c.literal.clone(),
        after: c.literal.clone(),
        origin: c.origin.clone(),
        value: c.value,
    };
    // Closing a preview at its starting value is a real gesture, but not a source edit: spellings
    // such as `0x320`, `8e2` and `-0` stay.
    if edit.value == c.value || c.literal == edit.text {
        return Ok(unchanged);
    }
    let file = path(&c.file)?;
    let old = s.accepted().snapshot.files[Path::new(&c.file)].clone();
    let op = plan::text_edit(&file, &old, &[(lens.start, lens.end, edit.text.clone())]);
    let anchor = Anchor::Written { file, range: Range::bytes(lens.start, lens.end) };
    let tx = plan::planned(s, (c.literal.clone(), edit.text.clone()), vec![op], Expect::Values(vec![(anchor, edit.value)]));
    let said = |why: &str| EditError::Rejected(format!("the edit would change more than this value ({why})"));
    let done = plan::run_as(s, tx, said, |_, _| Ok(()))?;
    Ok(Committed { revision: done.number, file: c.file, before: c.literal, after: edit.text, origin: c.origin, value: edit.value })
}

/// Write inherited control `c` into its call as `value` (its first edit, D5): a plain number in
/// base units where `insertion` says (or its dictionary's unit call, `db(-3)`; a bare dictionary
/// entry wrapped as `{ in: kick.out, db: … }`, design-8b §5.2), anchored by the call and the slot:
/// the constants of every evaluation of the call change to the value, are written at the
/// insertion's path, and nothing else changes. Releasing at the default writes nothing. Undo puts
/// the old text back wherever later edits carry it (WRITE-21).
fn insert(s: &mut Session, c: &Control, insertion: &Insertion, value: f64) -> Result<Committed, EditError> {
    let edit = lens::propose(value, 1.0)?;
    if edit.value == c.value {
        return Ok(Committed {
            revision: s.accepted().number,
            file: c.file.clone(),
            before: String::new(),
            after: String::new(),
            origin: c.origin.clone(),
            value: c.value,
        });
    }
    let edits = insertion.edits(&insertion.number(&edit.text));
    let file = path(&c.file)?;
    let site = s.accepted().compiled.sites.iter().find(|x| x.id == c.origin.site).cloned();
    let site = site.ok_or_else(|| EditError::Rejected("the control is no longer in the graph".into()))?;
    let old = s.accepted().snapshot.files[Path::new(&c.file)].clone();
    let op = plan::text_edit(&file, &old, &edits);
    let slot = c.origin.slot().unwrap_or_default().to_string();
    let anchor = Anchor::Slot { site: Spot { file, range: Range::bytes(site.start, site.end) }, slot };
    let (before, after) = crate::label::inserted(s, c, &edits);
    let tx = plan::planned(s, (before.clone(), after.clone()), vec![op], Expect::Values(vec![(anchor, edit.value)]));
    let said = |why: &str| EditError::Rejected(format!("writing this value into the call would change more than this value ({why})"));
    let done = plan::run_as(s, tx, said, |_, _| Ok(()))?;
    let origin = written(s, &c.origin).unwrap_or_else(|| {
        let mut origin = c.origin.clone();
        origin.arg.clone_from(&insertion.path);
        if let Some(slot) = &mut origin.control {
            slot.inherited = false;
        }
        origin
    });
    Ok(Committed { revision: done.number, file: c.file.clone(), before, after, origin, value: edit.value })
}

/// The origin of the constant now written in the accepted revision for `inherited`, a control
/// its call left out: the same call's slot, written (a `db(-3)` written for it is its own call).
fn written(s: &Session, inherited: &Origin) -> Option<Origin> {
    let slot = inherited.control.as_ref()?;
    let mut found = None;
    for m in &s.accepted().compiled.ir.modules {
        crate::controls::walk(&m.nodes, &mut |n| {
            if let (None, Op::Const { .. }, Some(o)) = (&found, &n.op, &n.origin)
                && let Some(c) = o.control.as_ref().filter(|c| !c.inherited && c.slot == slot.slot)
                && m.frames.get(c.frame as usize).is_some_and(|f| f.site == inherited.site)
            {
                found = Some(o.clone());
            }
        });
    }
    found
}

/// Resolve a live edit using exactly the same literal lens as a saved edit. Preview never
/// carries an old origin into a different accepted graph, and no build runs on this path.
///
/// Like [`set`], the control is carried forward from revision `at` when the files changed
/// without touching it, so an unrelated save during a drag doesn't end the drag; a change that
/// touched it makes the preview stale.
pub fn preview(s: &Session, at: u64, id: &str, value: f64) -> Result<Audition, EditError> {
    ready(s)?;
    let c = resolve(s, at, id)?;
    let value = c.audition(value)?;
    Ok(Audition { id: c.id, origin: c.origin, value, current: c.value })
}

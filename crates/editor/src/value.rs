//! Set a value (WRITE-10's first gesture, and WRITE-22's commit): one `Values` Tx.
//!
//! A written number is rewritten in place, through the same lens a preview uses, so what plays
//! during a drag is exactly what the commit writes. A declared control a call leaves out is
//! written into the call on its first edit (the step 5 design's D5), as a plain number in base
//! units, anchored by its call and slot (PKG-3). Core verifies that only those constants change,
//! to those values; nothing else may (WRITE-6).

use std::collections::BTreeMap;
use std::path::Path;

use m2_ir::{Op, Origin, Unit};
use m2_session::Session;
use m2_song::tx::{Anchor, Expect, Range, Spot};
use serde_json::Value as Json;

use crate::controls::{Audition, Control, Edit, resolve};
use crate::lens::{self, Insertion};
use crate::plan::{self, path};
use crate::{Committed, EditError, kind, ready};

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

/// Write `value`, as the code writes it, into control `id` named at revision `at` (CARD-21): a
/// number as [`set`] writes it; a choice's name or a switch's `true` or `false` in place of the
/// literal written there (keeping its quotes), or into its call when it is left out. One `Values`
/// Tx, which core checks against the place (or 1 or 0) the name makes; one undo step. A value it
/// doesn't take is refused in words; the value it has already writes nothing. With `expect`, only
/// while it is that.
pub fn write(s: &mut Session, at: u64, id: &str, value: &Json, expect: Option<&Json>) -> Result<Committed, EditError> {
    ready(s)?;
    let c = resolve(s, at, id)?;
    let name = c.declared.first().map_or("this value", |d| d.label.as_str()).to_string();
    let number = kind::number(c.unit, &name, value).map_err(EditError::Rejected)?;
    let expect = expect.map(|e| kind::number(c.unit, &name, e).map_err(EditError::Rejected)).transpose()?;
    if c.unit.is_none_or(Unit::is_number) {
        return set(s, at, id, number, expect);
    }
    if let Some(e) = expect
        && e != c.value
    {
        return Err(EditError::Conflict { current: c.value });
    }
    let text = spelled(&c, value);
    let unchanged = Committed {
        revision: s.accepted().number,
        file: c.file.clone(),
        before: c.literal.clone(),
        after: c.literal.clone(),
        origin: c.origin.clone(),
        value: c.value,
    };
    let Some(part) = part(s, &c, number, text)? else { return Ok(unchanged) };
    let edits = part.edits();
    let (before, after) = match &part.insertion {
        Some(_) => crate::label::inserted(s, &c, &edits),
        None => (c.literal.clone(), edits[0].2.clone()),
    };
    let old = s.accepted().snapshot.files[Path::new(&c.file)].clone();
    let op = plan::text_edit(&path(&c.file)?, &old, &edits);
    let tx = plan::planned(s, (before.clone(), after.clone()), vec![op], Expect::Values(vec![(part.anchor, number)]));
    let said = |why: &str| EditError::Rejected(format!("the edit would change more than this value ({why})"));
    let done = plan::run_as(s, tx, said, |_, _| Ok(()))?;
    let origin = if part.insertion.is_some() { written(s, &c.origin).unwrap_or_else(|| c.origin.clone()) } else { c.origin.clone() };
    Ok(Committed { revision: done.number, file: c.file, before, after, origin, value: number })
}

/// Write several values one gesture moved (CARD-22), each as [`write`] would, named at revision
/// `at`: one `Values` Tx over every file they are written in, so one undo step. Controls one call
/// leaves out are written into it together, as a preset's load writes them (FMT-22). A value
/// refused refuses them all, and nothing is written. Each one's commit, in the order given (a
/// value it has already is its control as it is).
pub fn write_all(s: &mut Session, at: u64, values: &[(String, Json)]) -> Result<Vec<Committed>, EditError> {
    if let [(id, value)] = values {
        return Ok(vec![write(s, at, id, value, None)?]);
    }
    ready(s)?;
    let mut controls = vec![];
    let mut parts = vec![];
    for (id, value) in values {
        let c = resolve(s, at, id)?;
        if controls.iter().any(|(x, _): &(Control, f64)| x.origin.same_value(&c.origin)) {
            return Err(EditError::Rejected(format!("{} is given twice", c.declared.first().map_or(id.as_str(), |d| d.label.as_str()))));
        }
        let name = c.declared.first().map_or("this value", |d| d.label.as_str()).to_string();
        let number = kind::number(c.unit, &name, value).map_err(EditError::Rejected)?;
        let (number, text) = match c.unit.is_none_or(Unit::is_number) {
            true => {
                c.audition(number)?;
                match &c.edit {
                    Edit::Literal { lens, .. } => {
                        let e = lens.propose(number)?;
                        (e.value, e.text)
                    }
                    Edit::Insert(insertion) => {
                        let e = lens::propose(number, 1.0)?;
                        (e.value, insertion.number(&e.text))
                    }
                }
            }
            false => (number, spelled(&c, value)),
        };
        if let Some(p) = part(s, &c, number, text)? {
            parts.push(p);
        }
        controls.push((c, number));
    }
    let unchanged = |c: &Control| Committed {
        revision: s.accepted().number,
        file: c.file.clone(),
        before: c.literal.clone(),
        after: c.literal.clone(),
        origin: c.origin.clone(),
        value: c.value,
    };
    if parts.is_empty() {
        return Ok(controls.iter().map(|(c, _)| unchanged(c)).collect());
    }
    // Each file's rewrites; each call's insertions, joined (a preset's load does the same).
    let mut edits: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    let mut inserts: BTreeMap<(String, u32), Vec<(Insertion, String)>> = BTreeMap::new();
    let mut anchors = vec![];
    for p in parts {
        match &p.insertion {
            Some((i, text)) => inserts.entry((p.file.clone(), p.site)).or_default().push((i.clone(), text.clone())),
            None => edits.entry(p.file.clone()).or_default().extend(p.edits()),
        }
        anchors.push((p.anchor, p.value));
    }
    for ((file, _), items) in inserts {
        let joined =
            lens::together(&items).ok_or_else(|| EditError::Rejected("these values can't be written into the call at once".into()))?;
        edits.entry(file).or_default().extend(joined);
    }
    let mut ops = vec![];
    let mut words = vec![];
    for (file, mut e) in edits {
        e.sort_by_key(|x| (x.0, x.1));
        if e.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err(EditError::Rejected("two values are written in the same place".into()));
        }
        let old = s.accepted().snapshot.files[Path::new(&file)].clone();
        words.extend(e.iter().map(|x| (old[x.0 as usize..x.1 as usize].to_string(), x.2.clone())));
        ops.push(plan::text_edit(&path(&file)?, &old, &e));
    }
    let joined = |pick: fn(&(String, String)) -> &str| words.iter().map(pick).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(", ");
    let label = (joined(|w| &w.0), joined(|w| &w.1));
    let tx = plan::planned(s, label.clone(), ops, Expect::Values(anchors));
    let said = |why: &str| EditError::Rejected(format!("the edit would change more than these values ({why})"));
    let done = plan::run_as(s, tx, said, |_, _| Ok(()))?;
    Ok(controls
        .into_iter()
        .map(|(c, number)| {
            let origin = match &c.edit {
                Edit::Insert(_) if number != c.value => written(s, &c.origin).unwrap_or_else(|| c.origin.clone()),
                _ => c.origin.clone(),
            };
            Committed {
                revision: done.number,
                file: c.file.clone(),
                before: label.0.clone(),
                after: label.1.clone(),
                origin,
                value: number,
            }
        })
        .collect())
}

/// A choice's name or a switch's `true` or `false` as control `c` writes it (CARD-21): a name in
/// the quotes the literal written there has (`"` for one left out, or one the quote can't hold).
fn spelled(c: &Control, value: &Json) -> String {
    match value {
        Json::String(name) => {
            let quote = c.literal.chars().next().filter(|q| matches!(q, '\'' | '`'));
            match quote {
                Some(q) if !name.contains([q, '\\', '\n']) && !name.contains("${") => format!("{q}{name}{q}"),
                _ => serde_json::to_string(name).expect("a string"),
            }
        }
        other => other.to_string(),
    }
}

/// One control's part of a value edit: where it is written, what core checks, and the value it
/// plays.
struct Part {
    file: String,
    /// Its call, for an insertion (joined with others into the same call).
    site: u32,
    /// The literal's rewrite, `(start, end, text)`, or the insertion and what it writes.
    rewrite: Option<(u32, u32, String)>,
    insertion: Option<(Insertion, String)>,
    anchor: Anchor,
    value: f64,
}

impl Part {
    fn edits(&self) -> Vec<(u32, u32, String)> {
        match (&self.rewrite, &self.insertion) {
            (Some(r), _) => vec![r.clone()],
            (None, Some((i, text))) => i.edits(text),
            (None, None) => vec![],
        }
    }
}

/// Control `c` written as `text`, which plays `value`: its literal rewritten, or written into its
/// call. None when it plays that already, or says it so (a spelling such as `0x320` stays). A
/// number is anchored where it is written; a name, or `true` or `false`, which core reads as no
/// number, by its instance's call and slot, as a control written into its call is (PKG-3).
fn part(s: &Session, c: &Control, value: f64, text: String) -> Result<Option<Part>, EditError> {
    let file = path(&c.file)?;
    let by_slot = |file: m2_song::tx::TxPath| -> Result<(Anchor, u32), EditError> {
        let call = *c.calls.first().ok_or_else(|| EditError::Rejected("the control's call is no longer in the graph".into()))?;
        let site = s.accepted().compiled.sites.iter().find(|x| x.id == call);
        let site = site.ok_or_else(|| EditError::Rejected("the control's call is no longer in the graph".into()))?;
        let slot = c.origin.slot().unwrap_or_default().to_string();
        Ok((Anchor::Slot { site: Spot { file, range: Range::bytes(site.start, site.end) }, slot }, site.id))
    };
    match &c.edit {
        Edit::Literal { lens, .. } => {
            if value == c.value || c.literal == text {
                return Ok(None);
            }
            let anchor = match c.unit.is_none_or(Unit::is_number) {
                true => Anchor::Written { file, range: Range::bytes(lens.start, lens.end) },
                false => by_slot(file)?.0,
            };
            Ok(Some(Part {
                file: c.file.clone(),
                site: c.origin.site,
                rewrite: Some((lens.start, lens.end, text)),
                insertion: None,
                anchor,
                value,
            }))
        }
        Edit::Insert(insertion) => {
            if value == c.value {
                return Ok(None);
            }
            let (anchor, site) = by_slot(file)?;
            let insertion = Some((insertion.clone(), text));
            Ok(Some(Part { file: c.file.clone(), site, rewrite: None, insertion, anchor, value }))
        }
    }
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

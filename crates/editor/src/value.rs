//! Set a value (WRITE-10's first gesture, and WRITE-22's commit): one `Values` Tx.
//!
//! A written number is rewritten in place, through the same lens a preview uses, so what plays
//! during a drag is exactly what the commit writes. A declared control a call leaves out is
//! written into the call on its first edit (the step 5 design's D5), as a plain number in base
//! units, anchored by its call and slot (PKG-3). A choice is written by its name and a switch as
//! `true` or `false` (CARD-21). Core verifies that only those constants change, to those values;
//! nothing else may (WRITE-6).
//!
//! One mechanism writes them all: each control's [`Part`] (its rewrite or its insertion, and what
//! core checks), then [`commit`], one `Values` Tx over every part. A knob's set, a menu's pick, a
//! view's drag of several knobs (CARD-22) and a preset's load (FMT-22) are each that.

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
    one(s, &c, &Json::from(value), expect.map(Json::from).as_ref())
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
    one(s, &c, value, expect)
}

/// What a control is called in words: its declared label.
fn name_of(c: &Control) -> &str {
    c.declared.first().map_or("this value", |d| d.label.as_str())
}

/// Control `c` written as `value`, one Tx (see [`write`]). A number changed elsewhere is a
/// `Conflict` with its number; a choice or a switch says its name, or true or false (review V10).
fn one(s: &mut Session, c: &Control, value: &Json, expect: Option<&Json>) -> Result<Committed, EditError> {
    let (number, text) = spell(c, value)?;
    if let Some(e) = expect {
        let e = kind::number(c.unit, name_of(c), e).map_err(EditError::Rejected)?;
        if e != c.value {
            return Err(match kind::said(c.unit, c.value).filter(|_| c.unit.is_some_and(|u| !u.is_number())) {
                Some(now) => EditError::Rejected(format!(
                    "{} changed elsewhere: it is {now} now, not {}",
                    name_of(c),
                    kind::said(c.unit, e).unwrap_or(Json::Null)
                )),
                None => EditError::Conflict { current: c.value },
            });
        }
    }
    let insert = matches!(c.edit, Edit::Insert(_));
    let Some(part) = part(s, c, number, text)? else {
        // Closing a preview at its starting value is a real gesture, but not a source edit:
        // spellings such as `0x320`, `8e2` and `-0` stay.
        let shown = if insert { String::new() } else { c.literal.clone() };
        let (revision, file) = (s.accepted().number, c.file.clone());
        return Ok(Committed { revision, file, before: shown.clone(), after: shown, origin: c.origin.clone(), value: c.value });
    };
    let edits = part.edits();
    let label = match insert {
        true => crate::label::inserted(s, c, &edits),
        false => (c.literal.clone(), edits[0].2.clone()),
    };
    let said = |why: &str| match insert {
        true => EditError::Rejected(format!("writing this value into the call would change more than this value ({why})")),
        false => EditError::Rejected(format!("the edit would change more than this value ({why})")),
    };
    let value = part.value;
    let done = commit(s, vec![part], label.clone(), said)?;
    Ok(Committed { revision: done.number, file: c.file.clone(), before: label.0, after: label.1, origin: origin_now(s, c, value), value })
}

/// Write several values one gesture moved (CARD-22), each as [`write`] would, named at revision
/// `at`: one `Values` Tx over every file they are written in, so one undo step. Controls one call
/// leaves out are written into it together, as a preset's load writes them (FMT-22). A value
/// refused refuses them all, and nothing is written; so does a control named twice. Each one's
/// commit, in the order given (a value it has already is its control as it is). Its words name
/// each control: `decay 0.4, sustain (default)` → `decay 0.25, sustain 0.5`.
pub fn write_all(s: &mut Session, at: u64, values: &[(String, Json)]) -> Result<Vec<Committed>, EditError> {
    if let [(id, value)] = values {
        return Ok(vec![write(s, at, id, value, None)?]);
    }
    ready(s)?;
    let mut controls: Vec<(Control, f64)> = vec![];
    let mut parts = vec![];
    let (mut before, mut after) = (vec![], vec![]);
    for (id, value) in values {
        let c = resolve(s, at, id)?;
        if controls.iter().any(|(x, _)| x.origin.same_value(&c.origin)) {
            return Err(EditError::Rejected(format!("{} is given twice", name_of(&c))));
        }
        let (number, text) = spell(&c, value)?;
        let was = if c.written { c.literal.clone() } else { "(default)".to_string() };
        before.push(format!("{} {was}", name_of(&c)));
        after.push(format!("{} {}", name_of(&c), if number == c.value { was } else { text.clone() }));
        parts.extend(part(s, &c, number, text)?);
        controls.push((c, number));
    }
    let revision = s.accepted().number;
    let label = (before.join(", "), after.join(", "));
    if parts.is_empty() {
        let unchanged = |c: &Control| Committed {
            revision,
            file: c.file.clone(),
            before: label.0.clone(),
            after: label.0.clone(),
            origin: c.origin.clone(),
            value: c.value,
        };
        return Ok(controls.iter().map(|(c, _)| unchanged(c)).collect());
    }
    let said = |why: &str| EditError::Rejected(format!("the edit would change more than these values ({why})"));
    let done = commit(s, parts, label.clone(), said)?;
    Ok(controls
        .into_iter()
        .map(|(c, number)| Committed {
            revision: done.number,
            file: c.file.clone(),
            before: label.0.clone(),
            after: label.1.clone(),
            origin: origin_now(s, &c, number),
            value: number,
        })
        .collect())
}

/// How control `c` is written as `value` (a number in its units, a choice's name, a switch's true
/// or false): the number it then plays, and the text its code holds. A number is spelled through
/// the lens a preview uses (`khz(0.9)`), or as its dictionary writes it (`db(-3)`); a value of
/// another kind is refused in words that say what it takes. Pure.
fn spell(c: &Control, value: &Json) -> Result<(f64, String), EditError> {
    let number = kind::number(c.unit, name_of(c), value).map_err(EditError::Rejected)?;
    if !c.unit.is_none_or(Unit::is_number) {
        return Ok((number, spelled(c, value)));
    }
    c.audition(number)?;
    Ok(match &c.edit {
        Edit::Literal { lens, .. } => {
            let e = lens.propose(number)?;
            (e.value, e.text)
        }
        Edit::Insert(insertion) => {
            let e = lens::propose(number, 1.0)?;
            (e.value, insertion.number(&e.text))
        }
    })
}

/// The origin of control `c` in the accepted revision after a commit gave it `value`: one its
/// call left out is written now.
fn origin_now(s: &Session, c: &Control, value: f64) -> Origin {
    match &c.edit {
        Edit::Insert(insertion) if value != c.value => written(s, &c.origin).unwrap_or_else(|| {
            let mut origin = c.origin.clone();
            origin.arg.clone_from(&insertion.path);
            if let Some(slot) = &mut origin.control {
                slot.inherited = false;
            }
            origin
        }),
        _ => c.origin.clone(),
    }
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
pub(crate) struct Part {
    file: String,
    /// Its call, for an insertion (joined with others into the same call).
    site: u32,
    /// The literal's rewrite, `(start, end, text)`, or the insertion and what it writes.
    rewrite: Option<(u32, u32, String)>,
    insertion: Option<(Insertion, String)>,
    anchor: Anchor,
    pub(crate) value: f64,
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
/// number, and a control written into its call, by the call and the slot (PKG-3).
pub(crate) fn part(s: &Session, c: &Control, value: f64, text: String) -> Result<Option<Part>, EditError> {
    let file = path(&c.file)?;
    let slot = c.origin.slot().unwrap_or_default().to_string();
    // The call a name or an insertion is written in: its origin's.
    let call = s.accepted().compiled.sites.iter().find(|x| x.id == c.origin.site);
    let call = call.ok_or_else(|| EditError::Rejected("the control is no longer in the graph".into()))?;
    let (site, start, end) = (call.id, call.start, call.end);
    let by_slot = |file| Anchor::Slot { site: Spot { file, range: Range::bytes(start, end) }, slot: slot.clone() };
    match &c.edit {
        Edit::Literal { lens, .. } => {
            if value == c.value || c.literal == text {
                return Ok(None);
            }
            let anchor = match c.unit.is_none_or(Unit::is_number) {
                true => Anchor::Written { file, range: Range::bytes(lens.start, lens.end) },
                false => by_slot(file),
            };
            Ok(Some(Part { file: c.file.clone(), site, rewrite: Some((lens.start, lens.end, text)), insertion: None, anchor, value }))
        }
        Edit::Insert(insertion) => {
            if value == c.value {
                return Ok(None);
            }
            Ok(Some(Part {
                file: c.file.clone(),
                site,
                rewrite: None,
                insertion: Some((insertion.clone(), text)),
                anchor: by_slot(file),
                value,
            }))
        }
    }
}

/// One `Values` Tx writing `parts`, named `label` (one undo step): each file's rewrites, and each
/// call's insertions joined into it (`lens::together`); two parts written in one place are
/// refused. `said` words a refusal by core's check.
pub(crate) fn commit(
    s: &mut Session,
    parts: Vec<Part>,
    label: (String, String),
    said: impl Fn(&str) -> EditError,
) -> Result<m2_session::Committed, EditError> {
    let mut edits: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    let mut inserts: BTreeMap<(String, u32), Vec<(Insertion, String)>> = BTreeMap::new();
    let mut anchors = vec![];
    // A number written in a unit call of its own (`db(-3)`) adds a call: that moves ids (AGENT-2).
    let adds_call = parts.iter().any(|p| p.insertion.as_ref().is_some_and(|(i, _)| i.spelling.is_some()));
    for p in parts {
        match (&p.rewrite, &p.insertion) {
            (Some(r), _) => edits.entry(p.file.clone()).or_default().push(r.clone()),
            (None, Some(i)) => inserts.entry((p.file.clone(), p.site)).or_default().push(i.clone()),
            (None, None) => {}
        }
        anchors.push((p.anchor, p.value));
    }
    for ((file, _), items) in inserts {
        let joined =
            lens::together(&items).ok_or_else(|| EditError::Rejected("these values can't be written into the call at once".into()))?;
        edits.entry(file).or_default().extend(joined);
    }
    let mut ops = vec![];
    for (file, mut e) in edits {
        e.sort_by_key(|x| (x.0, x.1));
        if e.windows(2).any(|w| w[1].0 < w[0].1) {
            return Err(EditError::Rejected("two values are written in the same place".into()));
        }
        let old = s.accepted().snapshot.files[Path::new(&file)].clone();
        ops.push(plan::text_edit(&path(&file)?, &old, &e));
    }
    let mut tx = plan::planned(s, label, ops, Expect::Values(anchors));
    tx.in_place = !adds_call;
    plan::run_as(s, tx, said, |_, _| Ok(()))
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

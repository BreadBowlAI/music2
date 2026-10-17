//! The values of the project's code the graph can show as knobs, read from the accepted
//! revision's raw tables (sites, origins, descriptors): a number written in the code, or a
//! declared control a call leaves out (PKG-3).
//!
//! A control is named by a `file#call/arg` id, a debug label meaningful within one revision
//! (WRITE-7): the web's handle, never core's. An id from an older revision is carried to the
//! accepted one when no change touched its call (WRITE-12, WRITE-22): what a client a revision or
//! two behind, or a gesture lease across a save, names.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use m2_ir::{Module, Node, Op, Origin, Unit};
use m2_script::Descriptor;
use m2_script::sites::Site;
use m2_session::text::Edits;
use m2_session::{Compiled, Home, Revision, Session};

use crate::EditError;
use crate::lens::{self, Insertion, LiteralLens};

/// A value of the project's code that the graph can show as a knob: a number written in the
/// project's own files, or a declared control one of its calls leaves out, which plays the
/// descriptor's default (inherited, PKG-3) and whose first edit writes it into the call.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    /// `file#call/arg` for a written number: the file, the call's position among the file's
    /// calls, and the argument path; `file#call/@slot` for a control the call leaves out.
    /// Meaningful together with `revision`.
    pub id: String,
    /// The accepted revision this control was read from.
    pub revision: u64,
    pub file: String,
    pub line: u32,
    /// The current value, in the constant's units (written number × scale).
    pub value: f64,
    /// The text written in the code, e.g. `0.3`, or `2.5` in `khz(2.5)`; empty when inherited.
    pub literal: String,
    pub unit: Option<Unit>,
    /// The constant is the written number times this.
    pub scale: f64,
    /// How many graph nodes use it (a helper called twice gives two).
    pub uses: usize,
    pub origin: Origin,
    /// Written in the code, or left out of its call (the declared default).
    pub written: bool,
    /// Where the value lives (HOME-1): the code, for every control until lanes, latches and
    /// plugin state have knobs.
    pub home: Home,
    /// Every declared control this value feeds, each `(component, slot)` once: several when one
    /// literal reaches several slots (`const g = frac(0.3)` given to `feedback` and `mix`).
    pub declared: Vec<Declared>,
    pub(crate) edit: Edit,
}

/// A declared control (PKG-3) a value feeds, as its component's descriptor declares it, for a
/// value in the constant's unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    /// The ComponentId (`std:delay`).
    pub component: String,
    /// The slot (`time`, `channels.hats.db`).
    pub slot: String,
    /// What a knob is called: the control's label, else its name (`hats.db` in a dictionary).
    pub label: String,
    /// The range a knob shows (a hint, PKG-8) and whether it moves logarithmically: the
    /// declared ones when the value has the declared unit, an `accepts` entry's for its unit,
    /// else none.
    pub range: Option<(f64, f64)>,
    pub log: bool,
    /// The descriptor's default, when the value has the declared unit.
    pub default: Option<f64>,
}

/// How a control is written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Edit {
    /// Its number is rewritten in place. `expression` is the whole value written at the slot: the
    /// number, or a quantity's call (`hz(500)`), which a cable replaces.
    Literal { lens: LiteralLens, expression: (u32, u32) },
    /// It is written into its call (the first edit of an inherited control).
    Insert(Insertion),
}

impl Control {
    /// The value `value` plays as, exactly as a commit would write it (the literal's spelling,
    /// rounded through its units): what a gesture's update sends the engine (WRITE-22). Pure.
    pub fn audition(&self, value: f64) -> Result<f64, EditError> {
        Ok(match &self.edit {
            Edit::Literal { lens, .. } => lens.propose(value)?.value,
            Edit::Insert(_) => lens::propose(value, 1.0)?.value,
        })
    }

    /// For a control its call leaves out: the edits writing `written` (a signal's name, or a
    /// number as [`Control::number`] spells it) into the call, each `(start, end, text)` in the
    /// file's order, as `set` and `connect` write it. One edit, or two when a bare dictionary
    /// entry is wrapped (`{ in: ` before it).
    pub fn planned(&self, written: &str) -> Option<Vec<(u32, u32, String)>> {
        match &self.edit {
            Edit::Insert(i) => Some(i.edits(written)),
            Edit::Literal { .. } => None,
        }
    }

    /// How a number in base units is written into the call by a value edit: as it is, or in the
    /// unit call its dictionary's other entries use (`db(-3)`, design-8b §5.2).
    pub fn number(&self, number: &str) -> String {
        match &self.edit {
            Edit::Insert(i) => i.number(number),
            Edit::Literal { .. } => number.to_string(),
        }
    }

    /// The dictionary entry an inherited control is written into, if it is one's.
    pub(crate) fn entry(&self) -> Option<&lens::Entry> {
        match &self.edit {
            Edit::Insert(i) => i.entry.as_ref(),
            Edit::Literal { .. } => None,
        }
    }

    /// The whole value written at the slot (the number, or a quantity's call), for a written one.
    pub(crate) fn expression(&self) -> Option<(u32, u32)> {
        match &self.edit {
            Edit::Literal { expression, .. } => Some(*expression),
            Edit::Insert(_) => None,
        }
    }

    /// The literal's bytes, for a written number.
    #[cfg(test)]
    pub(crate) fn lens(&self) -> Option<&LiteralLens> {
        match &self.edit {
            Edit::Literal { lens, .. } => Some(lens),
            Edit::Insert(_) => None,
        }
    }
}

/// Where a previewed value lands in the accepted revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Audition {
    /// The control's id in the accepted revision (it may differ from the id it was
    /// named by, if the files changed without touching it).
    pub id: String,
    pub origin: Origin,
    /// The value the preview plays, exactly as a commit would write it.
    pub value: f64,
    /// The value in the accepted revision, to return to if the preview is cancelled.
    pub current: f64,
}

/// The descriptor of the component whose instance frame `frame` of `m` is, if it was declared
/// with one.
pub(crate) fn component_of<'a>(compiled: &'a Compiled, m: &Module, frame: u32) -> Option<&'a Arc<Descriptor>> {
    compiled.components.get(m.frames.get(frame as usize)?.component.as_deref()?)
}

/// The declared control a constant of `m` with `origin` is, for a value in `unit` (PKG-3).
pub(crate) fn declared(compiled: &Compiled, m: &Module, origin: &Origin, unit: Option<Unit>) -> Option<Declared> {
    let slot = origin.control.as_ref()?;
    let desc = component_of(compiled, m, slot.frame)?;
    let c = desc.control(&slot.slot)?;
    // A dictionary entry's control is named by its key: `hats.db`.
    let name = slot.slot.split_once('.').map_or(slot.slot.as_str(), |(_, rest)| rest);
    let range = desc.range_for(&slot.slot, unit);
    Some(Declared {
        component: desc.id.clone(),
        slot: slot.slot.clone(),
        label: c.label.clone().unwrap_or_else(|| name.to_string()),
        range: range.map(|r| r.0),
        log: range.is_some_and(|r| r.1),
        default: c.default.as_ref().filter(|_| unit == c.unit).map(|d| d.value),
    })
}

/// Every node of `ns`, voices included.
pub(crate) fn walk(ns: &[Node], f: &mut impl FnMut(&Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

/// The control a constant of module `m` with `origin` and `value` shows in `rev`: a number written
/// in the project's own code, or a declared control a call in it leaves out, when the call can
/// take it (see [`Insertion::plan`]). `sites` is `rev`'s by id.
fn control(rev: &Revision, sites: &HashMap<u32, &Site>, m: &Module, origin: &Origin, value: f64, unit: Option<Unit>) -> Option<Control> {
    let site = sites.get(&origin.site).filter(|s| s.in_project)?;
    let text = rev.snapshot.files.get(Path::new(&site.file))?;
    let (id, literal, scale, edit) = if origin.arg.is_empty() {
        let slot = origin.control.as_ref().filter(|c| c.inherited)?;
        let desc = component_of(&rev.compiled, m, slot.frame)?;
        let insertion = Insertion::plan(site, desc, &slot.slot, text, options_of(m, site.id).ok()?, &rev.compiled.units).ok()?;
        (format!("{}#{}/@{}", site.file, site.ordinal, slot.slot), String::new(), 1.0, Edit::Insert(insertion))
    } else {
        let lens = LiteralLens::read(site, origin, text, value)?;
        // A quantity (`hz(500)`) is its own call; a plain number is written in its instance's
        // call, or, for an old-form component, is a constant with no unit.
        let quantity = match &origin.control {
            Some(slot) => m.frames.get(slot.frame as usize).is_some_and(|f| f.site != origin.site),
            None => unit.is_some(),
        };
        let expression = if quantity { (site.start, site.end) } else { (lens.start, lens.end) };
        let id = format!("{}#{}/{}", site.file, site.ordinal, origin.arg.join("."));
        (id, lens.literal.clone(), lens.scale, Edit::Literal { lens, expression })
    };
    Some(Control {
        id,
        revision: rev.number,
        file: site.file.clone(),
        line: site.line,
        value,
        literal,
        unit,
        scale,
        uses: 1,
        origin: origin.clone(),
        written: !origin.arg.is_empty(),
        home: Home::Code,
        declared: declared(&rev.compiled, m, origin, unit).into_iter().collect(),
        edit,
    })
}

/// Why a declared control that a call leaves out can't be written into it, or None when it can
/// or isn't one: for the view, which shows such a value as read-only.
pub(crate) fn not_insertable(rev: &Revision, site: &Site, m: &Module, origin: &Origin) -> Option<&'static str> {
    let slot = origin.control.as_ref().filter(|c| c.inherited && origin.arg.is_empty())?;
    let desc = component_of(&rev.compiled, m, slot.frame)?;
    let text = rev.snapshot.files.get(Path::new(&site.file))?;
    match options_of(m, site.id) {
        Ok(options) => Insertion::plan(site, desc, &slot.slot, text, options, &rev.compiled.units).err(),
        Err(why) => Some(why),
    }
}

/// Which argument of call `site` held its options when `m` was built (`Frame.options`), if one
/// did; why not known, when its evaluations disagree.
pub(crate) fn options_of(m: &Module, site: u32) -> Result<Option<usize>, &'static str> {
    let mut read = m.frames.iter().filter(|f| f.site == site && f.component.is_some()).map(|f| f.options);
    let first = read.next().flatten();
    match read.all(|o| o == first) {
        true => Ok(first.map(|i| i as usize)),
        false => Err("its evaluations read its arguments differently, so where it would go isn't known"),
    }
}

/// Every value of the project's own code that feeds the graph: each number written in its
/// files, and each declared control its calls leave out that can be written into them.
/// Values are keyed as their ids are: a written number by where it is written (so one
/// literal reaching two slots is one control, with both in `declared`), an inherited control
/// by its call and slot (every evaluation of that call).
pub fn controls(s: &Session) -> Vec<Control> {
    let rev = s.accepted();
    let sites: HashMap<u32, &Site> = rev.compiled.sites.iter().map(|s| (s.id, s)).collect();
    let mut found: BTreeMap<String, Control> = BTreeMap::new();
    for m in &rev.compiled.ir.modules {
        walk(&m.nodes, &mut |n: &Node| {
            let (Op::Const { value, unit }, Some(o)) = (&n.op, &n.origin) else { return };
            let Some(c) = control(rev, &sites, m, o, *value, *unit) else { return };
            match found.get_mut(&c.id) {
                Some(k) => {
                    k.uses += 1;
                    for d in c.declared {
                        if !k.declared.iter().any(|x| x.component == d.component && x.slot == d.slot) {
                            k.declared.push(d);
                        }
                    }
                }
                None => {
                    found.insert(c.id.clone(), c);
                }
            }
        });
    }
    found.into_values().collect()
}

/// [`controls`] by id.
pub fn controls_by_id(s: &Session) -> HashMap<String, Control> {
    controls(s).into_iter().map(|c| (c.id.clone(), c)).collect()
}

/// The control `id` named at revision `at`, as it is in the accepted revision: carried forward
/// when the files changed without touching its call, by the correspondence every carried range
/// uses (`Edits::between`: WRITE-12, WRITE-22). `current` is the accepted revision's
/// [`controls_by_id`], so many can be carried for one walk of the graph. Never builds anything.
pub fn carry(s: &Session, at: u64, id: &str, current: &HashMap<String, Control>) -> Result<Control, EditError> {
    resolve_with(s, at, id, |id| current.get(id).cloned())
}

/// The control `id` named at revision `at`, as it is in the accepted revision.
pub(crate) fn resolve(s: &Session, at: u64, id: &str) -> Result<Control, EditError> {
    let current = controls(s);
    resolve_with(s, at, id, |id| current.iter().find(|c| c.id == id).cloned())
}

fn resolve_with(s: &Session, at: u64, id: &str, find: impl Fn(&str) -> Option<Control>) -> Result<Control, EditError> {
    let accepted = s.accepted();
    if at == accepted.number {
        return find(id).ok_or_else(|| EditError::NotFound(id.to_string()));
    }
    // Carry the target from revision `at`: its call must be untouched and in place.
    let stale = EditError::Stale { current: accepted.number };
    let carried = || -> Option<Control> {
        let past = s.past(at)?;
        let (old, arg) = site_by_id(past.sites(), id)?;
        let file = Path::new(&old.file);
        let (was, is) = (past.snapshot.files.get(file)?, accepted.snapshot.files.get(file)?);
        let to = Edits::between(was, is).carry(old.start as usize..old.end as usize)?.start as u32;
        let site = accepted.compiled.sites.iter().find(|x| x.file == old.file && x.start == to && x.end == to + (old.end - old.start))?;
        find(&format!("{}#{}/{}", site.file, site.ordinal, arg.join(".")))
    };
    carried().ok_or(stale)
}

/// The call `id` names (`tracks/bass.ts#8/1`), and the argument path after it.
pub(crate) fn site_by_id<'a>(sites: &'a [Site], id: &str) -> Option<(&'a Site, Vec<String>)> {
    // The file may hold `/`, the argument path never does.
    let (call, arg) = id.rsplit_once('/')?;
    let (file, ordinal) = call.rsplit_once('#')?;
    let ordinal: u32 = ordinal.parse().ok()?;
    let site = sites.iter().find(|s| s.file == file && s.ordinal == ordinal)?;
    Some((site, arg.split('.').map(str::to_string).collect()))
}

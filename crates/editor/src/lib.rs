//! The editor: what a gesture means, and what the app and the CLI show (WRITE-9, WRITE-10, P10).
//!
//! Core keeps `save`, Tx, the delta and its checkers, undo, the journal, and the raw tables a
//! build makes (sites, origins, descriptors). Everything gesture-shaped is here: each gesture is a
//! builder from the accepted revision and a request to a Tx, plus the planner's own assertion on
//! the checked candidate (a cable rewires exactly one input, an effect wraps exactly one call, a
//! lane reads exactly the clips written), then a commit. Core never depends on this crate, so
//! adding a gesture never changes core.
//!
//! Here too: the controls and their `file#call/arg` ids (debug labels, WRITE-7), the canvas view,
//! the timeline, the knob lens, the labels history shows, the source helpers the planners use,
//! and offline renders at host level.

pub mod automate;
pub mod bus;
pub mod cable;
pub mod changed;
pub mod clips;
pub mod controls;
pub mod data;
pub mod drums;
pub mod effect;
pub mod explain;
pub mod faults;
pub mod fetch;
pub mod get;
pub mod grooves;
pub mod import;
pub mod json;
pub mod keys;
pub mod kind;
pub mod label;
pub mod layout;
pub mod lens;
pub mod library;
pub mod listen;
pub mod markers;
pub mod midifile;
pub mod mix;
pub mod mute;
pub mod note_names;
pub mod notes;
pub mod packs;
pub mod plan;
pub mod plugins;
pub mod presets;
pub mod registry;
pub mod rekey;
pub mod rename;
pub mod render;
pub mod sampler;
pub mod scales;
pub mod scope;
pub mod sections;
pub mod settings;
pub mod sfz;
pub mod sites;
pub mod smf;
pub mod song;
pub mod songkey;
pub mod swap;
pub mod take;
pub mod timeline;
pub mod tracks;
pub mod upgrade;
pub mod value;
pub mod view;
pub mod views;
pub mod voices;

use std::collections::HashMap;
use std::path::Path;

use m2_ir::{Diagnostic, Origin};
use m2_session::{ConflictFile, Refusal, Session, StepError};
use m2_song::{Fix, NoteClip};

pub use automate::Automated;
pub use cable::{Inlet, Outlet};
pub use controls::{Audition, Control, Declared};
pub use effect::EffectInfo;
pub use mix::{Inserted, Loop, MasterStrip, MixView, Strip};
pub use timeline::{TimelineLane, TimelineTrack, TimelineView};
pub use tracks::{Deleted, Dependent, Instrument, Made};
pub use view::{GraphView, Knob, ModuleView, ReadOnly, ViewNode};

/// An accepted edit: the revision it made, the file it is shown as, and what the edit said
/// before and says now (its label).
#[derive(Clone, Debug, PartialEq)]
pub struct Edited {
    pub revision: u64,
    pub file: String,
    pub before: String,
    pub after: String,
}

/// An accepted value change.
#[derive(Clone, Debug, PartialEq)]
pub struct Committed {
    pub revision: u64,
    pub file: String,
    /// What the edited range said before and says now.
    pub before: String,
    pub after: String,
    /// The value that changed, and what it is now (in its units).
    pub origin: Origin,
    pub value: f64,
}

#[derive(Debug, PartialEq)]
pub enum EditError {
    /// No such control in that revision.
    NotFound(String),
    /// Something the edit names isn't there: a sentence that says what, shown as it is.
    Missing(String),
    /// The files changed since the edit's revision in a way that touches its target.
    /// List the controls again and retry.
    Stale {
        current: u64,
    },
    /// The value is not what the caller expected: someone else changed it.
    Conflict {
        current: f64,
    },
    /// The files on disk don't compile, so there is no base to edit.
    DraftInvalid(Vec<Diagnostic>),
    /// The edit didn't compile, or didn't change exactly what it should have.
    Rejected(String),
    Io(String),
    /// The project is open read-only here (WRITE-17): why, as WRITE-18 says it.
    ReadOnly(String),
    /// A Tx's bases no longer apply: each file (and clip) that changed (WRITE-12, WRITE-21).
    Changed(Vec<ConflictFile>),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            EditError::NotFound(id) => write!(f, "no control `{id}` in that revision"),
            EditError::Missing(what) => write!(f, "{what}"),
            EditError::Stale { current } => {
                write!(f, "the code changed around this control; it is revision {current} now, list the controls again")
            }
            EditError::Conflict { current } => write!(f, "the value changed elsewhere (it is now {current})"),
            EditError::DraftInvalid(d) => write!(f, "the project doesn't compile right now ({} problems), so it can't be edited", d.len()),
            EditError::Rejected(why) => write!(f, "edit rejected: {why}"),
            EditError::Io(e) => write!(f, "{e}"),
            EditError::ReadOnly(why) => write!(f, "{why}"),
            EditError::Changed(files) => write!(f, "{}", Refusal::Conflict { files: files.clone() }),
        }
    }
}

impl From<Refusal> for EditError {
    fn from(r: Refusal) -> EditError {
        match r {
            Refusal::Conflict { files } => EditError::Changed(files),
            Refusal::Rejected { diagnostics, .. } if diagnostics.iter().any(|d| d.code.starts_with("lock.")) => {
                EditError::ReadOnly(diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))
            }
            Refusal::Rejected { diagnostics, .. } => {
                EditError::Rejected(diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))
            }
            Refusal::Io { message } => EditError::Io(message),
        }
    }
}

/// A refusal as a gesture says it: a conflict only because a touched file changed on disk and
/// doesn't compile yet is `DraftInvalid`, which a gesture lease's commit waits out (C5 of step 3b,
/// the step 6 design's 16); the rest as [`From<Refusal>`] maps them. (A draft that doesn't compile
/// is refused before planning, as [`ready`] says.)
pub(crate) fn refused(s: &Session, r: Refusal) -> EditError {
    match &r {
        Refusal::Conflict { files } if files.iter().all(|f| f.why.as_deref().is_some_and(|w| w.contains("doesn't compile"))) => {
            EditError::DraftInvalid(s.draft_errors.clone())
        }
        _ => r.into(),
    }
}

/// Names as words: "Kick", "Kick and Bass", "Kick, Bass and Hats".
pub fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// A gesture that writes code plans on the accepted revision, which needs the draft to compile
/// (the draft rule, C7): otherwise there is no base to edit.
pub(crate) fn ready(s: &Session) -> Result<(), EditError> {
    match s.draft_errors.is_empty() {
        true => Ok(()),
        false => Err(EditError::DraftInvalid(s.draft_errors.clone())),
    }
}

impl From<StepError> for EditError {
    fn from(e: StepError) -> EditError {
        match e {
            StepError::Nothing(what) => EditError::NotFound(what.into()),
            StepError::DraftInvalid(d) => EditError::DraftInvalid(d),
            StepError::ReadOnly(why) => EditError::ReadOnly(why),
            StepError::Refused(r) => r.into(),
        }
    }
}

/// Undo the newest change made through the session: its stored inverse (WRITE-21).
pub fn undo(s: &mut Session) -> Result<Edited, EditError> {
    let done = s.undo()?;
    Ok(Edited { revision: done.number, file: done.file, before: done.before, after: done.after })
}

/// Redo the newest change undone.
pub fn redo(s: &mut Session) -> Result<Edited, EditError> {
    let done = s.redo()?;
    Ok(Edited { revision: done.number, file: done.file, before: done.before, after: done.after })
}

/// The gestures, as Tx builders on a session: what the server, the CLI and the tests call
/// (the step 6 design's 13.3). Each plans on the accepted revision and never takes in the
/// folder first (R2): an outside change in a file it touches enters through the commit's
/// baseline.
pub trait Gestures {
    /// Every value of the project's own code that feeds the graph (see [`controls::controls`]).
    fn controls(&self) -> Vec<Control>;
    fn controls_by_id(&self) -> HashMap<String, Control>;
    /// The control `id` named at revision `at`, as it is in the accepted revision (WRITE-22).
    fn carry(&self, at: u64, id: &str, current: &HashMap<String, Control>) -> Result<Control, EditError>;
    /// Where a previewed value lands, with no build and no write.
    fn preview(&self, at: u64, id: &str, value: f64) -> Result<Audition, EditError>;
    /// Set a value: one `Values` Tx (WRITE-10, WRITE-22's commit).
    fn set(&mut self, at: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError>;
    /// Write a value as its code writes it: a number, a choice's name, a switch's true or false
    /// (CARD-21). One `Values` Tx.
    fn write(&mut self, at: u64, id: &str, value: &serde_json::Value, expect: Option<&serde_json::Value>) -> Result<Committed, EditError>;
    /// Write the values one gesture moved: one `Values` Tx, one undo step (CARD-22).
    fn write_all(&mut self, at: u64, values: &[(String, serde_json::Value)]) -> Result<Vec<Committed>, EditError>;
    fn outlets(&self) -> Vec<Outlet>;
    fn inlets(&self) -> Vec<Inlet>;
    fn connect(&mut self, at: u64, from: &str, to: &str) -> Result<Edited, EditError>;
    fn disconnect(&mut self, at: u64, id: &str) -> Result<Edited, EditError>;
    fn effects(&self) -> Vec<EffectInfo>;
    fn add_effect(&mut self, at: u64, call: &str, component: &str) -> Result<Edited, EditError>;
    fn remove_effect(&mut self, at: u64, call: &str) -> Result<Edited, EditError>;
    /// Write a notes lane's clips, as planned at revision `at` (the lane's base is its file
    /// then).
    fn write_notes(&mut self, at: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Edited, EditError>;
    fn relink(&mut self, at: u64, from: &Path, to: &Path) -> Result<Edited, EditError>;
    fn delete_data(&mut self, at: u64, path: &Path) -> Result<Edited, EditError>;
    /// Commit a fix a problem offered (DIAG-1): its Tx, as it came.
    fn apply_fix(&mut self, fix: &Fix) -> Result<Edited, EditError>;
    /// Show a track as `label` (ID-10): one `Identical` Tx on its `track()` call's name.
    fn rename_label(&mut self, module: &str, label: &str) -> Result<Edited, EditError>;
    /// Automate control `id`, named at revision `at` (HOME-2): its value becomes a new control
    /// lane's fallback.
    fn automate(&mut self, at: u64, id: &str) -> Result<Automated, EditError>;
    /// The instruments a new track can play (the + Track list).
    fn instruments(&self) -> Vec<Instrument>;
    /// Add a track playing `component` (WRITE-10): see [`tracks::add`].
    fn add_track(&mut self, component: &str, label: Option<&str>, key: Option<&str>, after: Option<&str>) -> Result<Made, EditError>;
    fn duplicate_track(&mut self, module: &str) -> Result<Made, EditError>;
    /// Delete a track; with `cascade`, the effects that read it lose that input (see [`tracks::delete`]).
    fn delete_track(&mut self, module: &str, cascade: bool) -> Result<Deleted, EditError>;
    /// Put the tracks in `order` (every track, once): a layout write, no revision (PLANE-3).
    fn reorder(&mut self, order: &[String]) -> Result<Edited, EditError>;
    /// Give a track a palette colour, by name.
    fn recolour(&mut self, module: &str, colour: &str) -> Result<Edited, EditError>;
    fn view(&self) -> GraphView;
    fn timeline(&self) -> TimelineView;
    /// The Mix view: a strip for each track in the layout's order, and the master (design-8b §5.4).
    fn mix(&self) -> MixView;
    /// A channel's first pan: write optional control `control` of mixer channel `channel` as
    /// `value` (see [`mix::insert`]).
    fn mix_insert(&mut self, channel: &str, control: &str, value: f64) -> Result<Inserted, EditError>;
}

impl Gestures for Session {
    fn controls(&self) -> Vec<Control> {
        controls::controls(self)
    }

    fn controls_by_id(&self) -> HashMap<String, Control> {
        controls::controls_by_id(self)
    }

    fn carry(&self, at: u64, id: &str, current: &HashMap<String, Control>) -> Result<Control, EditError> {
        controls::carry(self, at, id, current)
    }

    fn preview(&self, at: u64, id: &str, value: f64) -> Result<Audition, EditError> {
        value::preview(self, at, id, value)
    }

    fn set(&mut self, at: u64, id: &str, value: f64, expect: Option<f64>) -> Result<Committed, EditError> {
        value::set(self, at, id, value, expect)
    }

    fn write(&mut self, at: u64, id: &str, value: &serde_json::Value, expect: Option<&serde_json::Value>) -> Result<Committed, EditError> {
        value::write(self, at, id, value, expect)
    }

    fn write_all(&mut self, at: u64, values: &[(String, serde_json::Value)]) -> Result<Vec<Committed>, EditError> {
        value::write_all(self, at, values)
    }

    fn outlets(&self) -> Vec<Outlet> {
        cable::outlets(self)
    }

    fn inlets(&self) -> Vec<Inlet> {
        cable::inlets(self)
    }

    fn connect(&mut self, at: u64, from: &str, to: &str) -> Result<Edited, EditError> {
        cable::connect(self, at, from, to)
    }

    fn disconnect(&mut self, at: u64, id: &str) -> Result<Edited, EditError> {
        cable::disconnect(self, at, id)
    }

    fn effects(&self) -> Vec<EffectInfo> {
        effect::effects(self)
    }

    fn add_effect(&mut self, at: u64, call: &str, component: &str) -> Result<Edited, EditError> {
        effect::add_effect(self, at, call, component)
    }

    fn remove_effect(&mut self, at: u64, call: &str) -> Result<Edited, EditError> {
        effect::remove_effect(self, at, call)
    }

    fn write_notes(&mut self, at: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Edited, EditError> {
        notes::write_notes(self, at, module, lane, clips)
    }

    fn relink(&mut self, at: u64, from: &Path, to: &Path) -> Result<Edited, EditError> {
        data::relink(self, at, from, to)
    }

    fn delete_data(&mut self, at: u64, path: &Path) -> Result<Edited, EditError> {
        data::delete_data(self, at, path)
    }

    fn apply_fix(&mut self, fix: &Fix) -> Result<Edited, EditError> {
        data::apply_fix(self, fix)
    }

    fn rename_label(&mut self, module: &str, label: &str) -> Result<Edited, EditError> {
        rename::rename_label(self, module, label)
    }

    fn automate(&mut self, at: u64, id: &str) -> Result<Automated, EditError> {
        automate::automate(self, at, id)
    }

    fn instruments(&self) -> Vec<Instrument> {
        tracks::instruments(self)
    }

    fn add_track(&mut self, component: &str, label: Option<&str>, key: Option<&str>, after: Option<&str>) -> Result<Made, EditError> {
        tracks::add(self, component, label, key, after)
    }

    fn duplicate_track(&mut self, module: &str) -> Result<Made, EditError> {
        tracks::duplicate(self, module)
    }

    fn delete_track(&mut self, module: &str, cascade: bool) -> Result<Deleted, EditError> {
        tracks::delete(self, module, cascade)
    }

    fn reorder(&mut self, order: &[String]) -> Result<Edited, EditError> {
        layout::reorder(self, order)
    }

    fn recolour(&mut self, module: &str, colour: &str) -> Result<Edited, EditError> {
        layout::recolour(self, module, colour)
    }

    fn view(&self) -> GraphView {
        view::view(self)
    }

    fn timeline(&self) -> TimelineView {
        timeline::timeline(self)
    }

    fn mix(&self) -> MixView {
        mix::view(self, &layout::view(self))
    }

    fn mix_insert(&mut self, channel: &str, control: &str, value: f64) -> Result<Inserted, EditError> {
        mix::insert(self, channel, control, value)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod restricted_tests;

#[cfg(test)]
mod insert_tests;

#[cfg(test)]
mod gate5_tests;

#[cfg(test)]
pub(crate) mod testing;

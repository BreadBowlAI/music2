//! `m2 upgrade <song>` and the app's Upgrade it (FMT-25, AGENT-14): a song made for an older music2
//! moved to this one in one step, one undo step. Its `[dependencies]` ask for this app's
//! `@m2/core`, std and music2's own packs, and its `music2.lock` is written as `m2 pack lock`
//! writes it; in its code, what music2's own changes moved or renamed is rewritten by the tables
//! below, read with the build's parser (oxc, as `m2_script::sites` and `crate::sites` read it),
//! never by patterns over text; and a lane that drives a knob whose unit changed is converted
//! with it. Nothing else is touched.
//!
//! An upgrade keeps the song's sound, or says exactly where it can't, and never writes half a
//! conversion: what it can't convert exactly (a value worked out by code, a name used some other
//! way, a lane two knobs read in different units, hats whose lengths the Drum can't keep) is named
//! with its file and line and the line to write, and then nothing is written at all. What changes
//! though it is converted as well as it can be (the simple kick and hat, other sounds than One
//! drum's) is named too, and written. It is written only when the song then builds, as one save of
//! every file (`Session::save_all_built`). The musician's presets, outside the song, are never
//! changed: those that name a sound that moved or was renamed are named.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_script::sites::{Arg, Site};
use m2_session::{Refusal, Session};
use m2_song::pack::{self, Lock, PackName};
use m2_song::tx::Base;
use m2_song::{DataPath, LaneData, LaneKind, Manifest, Requirement};
use oxc::allocator::Allocator;
use oxc::ast::ast::{ImportDeclarationSpecifier, Statement};
use oxc::parser::Parser;
use oxc::span::SourceType;
use serde::{Deserialize, Serialize};

use crate::EditError;
use crate::sites::{Property, bound_names, properties, references};

/// What an upgrade's undo step is called.
pub const NAME: &str = "Upgrade";

/// A file the upgrade changes, as it says it: the lines before and after, or what changed in it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub file: String,
    pub lines: Vec<String>,
}

/// A place the upgrade names: its file and line (0: the whole file), what is there, and what to
/// write, when it can say.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Left {
    pub file: String,
    pub line: u32,
    pub what: String,
    pub write: String,
}

/// What an upgrade did, or would do (`--check`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Upgraded {
    /// Each file it changes, in the order it says them: the manifest, the lock, then by path.
    pub changes: Vec<Change>,
    /// What it can't convert exactly: while any is left, nothing is written.
    pub left: Vec<Left>,
    /// What to check by hand, written all the same: a sound it converts as well as it can, and the
    /// musician's presets (`Presets/…`) that name a sound that moved, which stay as they are.
    pub notes: Vec<Left>,
    /// The revision it made, as one undo step; None with `--check`, with nothing to change, with
    /// something left, or when the song wouldn't build after it.
    pub written: Option<u64>,
    /// Why nothing was written though nothing was left: what doesn't build after it.
    pub refused: Vec<String>,
}

impl Upgraded {
    /// Made for this music2 already: nothing to change, nothing left.
    pub fn current(&self) -> bool {
        self.changes.is_empty() && self.left.is_empty()
    }
}

/// Whether the song at `s` was made for an older music2 and doesn't build here (FMT-3, PACK-19,
/// PACK-24): what the app's status says, with Upgrade it (AGENT-14). By what its files hold: a line
/// of `music2.toml` (or of the lock) asking for an older `@m2/core`, std or one of music2's own
/// packs, or code that plays what moved or was removed. A song that builds needs nothing, and costs
/// nothing to ask.
pub fn needed(s: &Session) -> bool {
    if s.draft_errors.is_empty() {
        return false;
    }
    let files = &s.draft().files;
    let Some(text) = files.get(Path::new(m2_song::path::MANIFEST)) else { return false };
    let Ok(m) = Manifest::read(text) else { return false };
    let app = m2_session::app(&s.packages);
    let lock = files.get(Path::new(m2_song::path::LOCK)).and_then(|t| Lock::read(t).ok()).unwrap_or_default();
    let old = old_modules(&m, &app);
    m.dependencies.iter().any(|(name, dep)| dep.requirement().is_some_and(|req| older(name, req, &app).is_some()))
        || lock
            .packs
            .iter()
            .any(|l| pack::comes_with(&l.name).is_some_and(|v| l.version < v || (l.version == v && !pack::own(&l.name, &v, &l.hash))))
        || code(files, &old).any(|f| !f.edits.is_empty() || !f.left.is_empty())
}

/// What this app has of `name` when `req` asks for an older version of it (older only: a song
/// made for a newer music2 can't be moved back): `@m2/core`, std, or one of music2's own packs.
fn older(name: &str, req: &Requirement, app: &pack::App) -> Option<m2_song::Version> {
    have(name, app).filter(|v| *v >= req.upper())
}

/// The version of `name` this app has: `@m2/core`, std, or one of music2's own packs.
fn have(name: &str, app: &pack::App) -> Option<m2_song::Version> {
    match name {
        m2_song::SDK => app.sdk,
        pack::STD => app.std,
        _ => PackName::new(name).ok().and_then(|n| pack::comes_with(&n)),
    }
}

/// Upgrade the song (FMT-25): what it changes, and, unless `check`, those changes written as one
/// undo step when nothing is left and the song then builds. `presets`: the musician's presets
/// folder, whose presets of sounds that moved are named (read, never written).
pub fn upgrade(s: &mut Session, check: bool, presets: Option<&Path>) -> Result<Upgraded, EditError> {
    let plan = plan(s, presets)?;
    let mut out = Upgraded {
        changes: plan.files.iter().map(|f| f.change.clone()).collect(),
        left: plan.left,
        notes: plan.notes,
        ..Upgraded::default()
    };
    // All or nothing: while anything is left, nothing is written.
    if plan.files.is_empty() || !out.left.is_empty() {
        return Ok(out);
    }
    let files: Vec<(PathBuf, Base, String)> = plan
        .files
        .iter()
        .map(|f| (PathBuf::from(&f.change.file), f.before.as_deref().map_or(Base::Absent, Base::text), f.after.clone()))
        .collect();
    match s.save_all_built(&files, NAME, check) {
        Ok(saved) => out.written = (!check && saved.recorded).then(|| s.accepted().number),
        Err(Refusal::Rejected { diagnostics, .. }) => {
            out.refused =
                diagnostics.iter().map(|d| d.src.as_ref().map_or_else(|| d.message.clone(), |f| format!("{f}: {}", d.message))).collect();
        }
        Err(r) => return Err(crate::refused(s, r)),
    }
    Ok(out)
}

/// The files the upgrade would change, by their paths: what the app checks against what the
/// musician holds before it runs it (AGENT-3).
pub fn files(s: &Session) -> Result<Vec<String>, EditError> {
    Ok(plan(s, None)?.files.into_iter().map(|f| f.change.file).collect())
}

/// A file the upgrade writes: what it holds now (None: it isn't there), and after.
struct Planned {
    change: Change,
    before: Option<String>,
    after: String,
}

/// Everything the upgrade would write, what it leaves, and what it names.
struct Plan {
    files: Vec<Planned>,
    left: Vec<Left>,
    notes: Vec<Left>,
}

/// The modules whose names still mean what they meant before wave 3 renamed them: std's that
/// moved, and music2's own packs the song asks for an older major of. Only the bass's `decay`
/// needs it: a name no pack has now (the simple kick, `drums.snare`) means the old one whatever the
/// manifest says, but a song made for this music2 keeps what `decay` means now.
fn old_modules(m: &Manifest, app: &pack::App) -> BTreeSet<String> {
    let mut old: BTreeSet<String> = MOVED_FROM.iter().map(|m| m.to_string()).collect();
    for (name, dep) in &m.dependencies {
        if PackName::new(name).is_ok_and(|n| pack::comes_with(&n).is_some())
            && dep.requirement().is_some_and(|req| older(name, req, app).is_some())
        {
            old.insert(name.clone());
        }
    }
    old
}

/// Each code file of `files` as the upgrade reads it, upgraded.
fn code<'a>(files: &'a BTreeMap<PathBuf, Arc<str>>, old: &'a BTreeSet<String>) -> impl Iterator<Item = Code<'a>> + 'a {
    files.iter().filter(|(p, _)| matches!(DataPath::parse(p), DataPath::Code(_))).map(move |(path, text)| {
        let mut f = Code::read(&m2_song::path::slashed(path), text, old, files);
        f.upgrade();
        f
    })
}

/// What the upgrade writes, read from the files on disk (the draft: an old song doesn't build).
fn plan(s: &Session, presets: Option<&Path>) -> Result<Plan, EditError> {
    let files = &s.draft().files;
    let text_of = |p: &str| files.get(Path::new(p)).map(|t| t.to_string());
    let manifest_text = text_of(m2_song::path::MANIFEST).ok_or_else(|| EditError::NotFound(m2_song::path::MANIFEST.into()))?;
    let manifest = Manifest::read(&manifest_text).map_err(|d| EditError::Rejected(d.message))?;
    let app = m2_session::app(&s.packages);
    newer(&manifest, &app)?;
    let old = old_modules(&manifest, &app);
    let (mut left, mut notes, mut code_files, mut packs) = (vec![], vec![], vec![], BTreeSet::new());
    let mut lanes: BTreeMap<String, (Knob, Left)> = BTreeMap::new();
    for mut f in code(files, &old) {
        left.extend(f.unparsed.take());
        left.append(&mut f.left);
        notes.append(&mut f.notes);
        packs.extend(f.packs.iter().cloned());
        for (lane, knob, at) in f.lanes.drain(..) {
            match lanes.get(&lane) {
                // One lane can't be read in two units.
                Some((k, _)) if *k != knob => left.push(Left {
                    what: format!("{lane} is a lane two knobs read, which the Drum reads in different units now"),
                    write: "give each knob a lane of its own, then upgrade".into(),
                    ..at
                }),
                _ => drop(lanes.insert(lane, (knob, at))),
            }
        }
        let after = apply(f.text, &f.edits).map_err(|at| clash(&f.file, f.text, at))?;
        // The packs the code imports, as it will be.
        packs.extend(imported_packs(&after));
        if !f.edits.is_empty() {
            let change = Change { file: f.file.clone(), lines: shown(f.text, &f.edits) };
            code_files.push(Planned { change, before: Some(f.text.to_string()), after });
        }
    }
    // The lanes of the knobs converted: their unit and every point.
    let mut data = vec![];
    for (lane, (knob, _)) in &lanes {
        let Some(text) = text_of(lane) else { continue };
        match convert_lane(&text, *knob) {
            Ok(Some((after, n))) => {
                let (from, to) = knob.unit();
                let lines = vec![format!("unit {from} → {to}, written as {n} point{}", plural(n))];
                data.push(Planned { change: Change { file: lane.clone(), lines }, before: Some(text), after });
            }
            Ok(None) => {}
            Err(why) => left.push(Left { file: lane.clone(), line: 1, what: why, write: String::new() }),
        }
    }
    let mut out = vec![];
    let (manifest_after, edits) = dependencies(&manifest_text, &manifest, &packs, &app)?;
    if !edits.is_empty() {
        let change = Change { file: m2_song::path::MANIFEST.into(), lines: shown(&manifest_text, &edits) };
        out.push(Planned { change, before: Some(manifest_text.clone()), after: manifest_after.clone() });
    }
    // The lock, as `m2 pack lock` writes it from the manifest as it will be (PACK-8).
    let lock_text = text_of(m2_song::path::LOCK);
    let mut locked = lock_text.as_deref().and_then(|t| Lock::read(t).ok()).unwrap_or_default();
    // music2's own packs move to the versions it comes with, its very files: an older one the store
    // keeps may be made for an older @m2/core, which this app can't run (PACK-22), and a lock of the
    // same version with other files names a build from before its release.
    let kept = locked.packs.len();
    locked.packs.retain(|l| pack::comes_with(&l.name).is_none_or(|v| v == l.version && pack::own(&l.name, &l.version, &l.hash)));
    // Only a song that moves is locked again: one made for this music2 keeps its lock, or none.
    let moves = !edits.is_empty() || locked.packs.len() != kept;
    let new_manifest = Manifest::read(&manifest_after).map_err(|d| EditError::Rejected(d.message))?;
    let lock = pack::relock(&new_manifest, &locked, s.packages.store(), &app, pack::Move::Needed)
        .map_err(|d| EditError::Rejected(d.into_iter().map(|d| d.message).collect::<Vec<_>>().join("\n")))?;
    let canonical = lock.canonical();
    if moves && lock_text.as_deref() != Some(canonical.as_str()) && !(lock_text.is_none() && lock.packs.is_empty()) {
        let said: Vec<String> = lock.packs.iter().map(|l| format!("{} {}", l.name, l.version)).collect();
        let what = if said.is_empty() { "locks no packs".to_string() } else { format!("locks {}", said.join(", ")) };
        let line = if lock_text.is_none() { format!("new: {what}") } else { what };
        out.push(Planned { change: Change { file: m2_song::path::LOCK.into(), lines: vec![line] }, before: lock_text, after: canonical });
    }
    out.extend(code_files);
    out.extend(data);
    if let Some(dir) = presets {
        notes.extend(preset_notes(dir, s.catalog()));
    }
    Ok(Plan { files: out, left, notes })
}

/// Refused, saying so, for a song made for a newer music2 than this: it can't be moved back.
fn newer(m: &Manifest, app: &pack::App) -> Result<(), EditError> {
    for (name, dep) in &m.dependencies {
        if let (Some(req), Some(v)) = (dep.requirement(), have(name, app))
            && v < req.least()
        {
            return Err(EditError::Rejected(format!(
                "this song was made for a newer music2: it asks for {name} \"{req}\", and this music2 has {v}. Open it with a newer music2"
            )));
        }
    }
    Ok(())
}

/// The words for edits of one file that clash, at byte `at`: none is written.
fn clash(file: &str, text: &str, at: u32) -> EditError {
    let line = text[..(at as usize).min(text.len())].matches('\n').count() + 1;
    EditError::Rejected(format!("the upgrade's own changes clash in {file} at line {line}, so nothing was written"))
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

// ---------------------------------------------------------------- the tables

/// Where an import from a module std moved in std 0.7 goes now: the pack (`m2_script::MOVED`,
/// the one table), or, for what std kept of `std/synth`, `std/dsp`. None: it stays.
fn moved_import(module: &str, name: &str) -> Option<&'static str> {
    if module == "std/synth" && matches!(name, "mono" | "Voice") {
        return Some("std/dsp");
    }
    let sub = module.strip_prefix("std/")?;
    m2_script::MOVED.iter().find(|(m, names, ..)| *m == sub && (names.is_empty() || names.contains(&name))).map(|(_, _, pack, _)| *pack)
}

/// Whether `module` is one of std's that moved into the packs (`m2_script::MOVED`).
fn moved_module(module: &str) -> bool {
    module.strip_prefix("std/").is_some_and(|sub| m2_script::MOVED.iter().any(|(m, ..)| *m == sub))
}

/// How a knob of a drum the Drum plays now reads there (wave 3, CARD-33). Each is exact for the
/// values the old drum played: the old drum clamped its value to `lo..hi` first, and the Drum plays
/// a length 0.05 to 20 times its own.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Knob {
    /// A tune or tone in Hz: semitones from the drum's own pitch, 12 × log2(Hz ÷ own).
    Tune { own: f64, lo: f64, hi: f64 },
    /// A decay in seconds: times the drum's own length, seconds ÷ own.
    Decay { own: f64, lo: f64, hi: f64 },
    /// A kick's punch, a snare's snap, 0 to 1: the Drum's punch, less the drum's own.
    Punch(f64),
}

const fn tune(own: f64, lo: f64, hi: f64) -> Option<Knob> {
    Some(Knob::Tune { own, lo, hi })
}

const fn decay(own: f64, lo: f64, hi: f64) -> Option<Knob> {
    Some(Knob::Decay { own, lo, hi })
}

/// The hats single's closed and open hats' decays, as the Drum's `decay` reads them.
const CLOSED: Knob = Knob::Decay { own: 0.045, lo: 0.005, hi: 2.0 };
const OPEN: Knob = Knob::Decay { own: 0.45, lo: 0.02, hi: 5.0 };

impl Knob {
    /// `x`, in the old knob's base unit, as the Drum's knob reads it; None where the Drum can't
    /// play it.
    fn convert(self, x: f64) -> Option<f64> {
        if !x.is_finite() {
            return None;
        }
        match self {
            Knob::Tune { own, lo, hi } => Some(12.0 * (x.clamp(lo, hi) / own).log2()),
            Knob::Decay { own, lo, hi } => Some(x.clamp(lo, hi) / own).filter(|k| (0.05..=20.0).contains(k)),
            Knob::Punch(own) => Some(x.clamp(0.0, 1.0) - own),
        }
    }

    /// The unit a lane of it is in, before and after.
    fn unit(self) -> (&'static str, &'static str) {
        match self {
            Knob::Tune { .. } => ("hz", "semitones"),
            Knob::Decay { .. } => ("seconds", "scalar"),
            Knob::Punch(_) => ("unit01", "bipolar"),
        }
    }

    /// The unit constructors of @m2/core a value of it may be written with, and their scale.
    fn spellings(self) -> &'static [(&'static str, f64)] {
        match self {
            Knob::Tune { .. } => &[("hz", 1.0), ("khz", 1000.0)],
            Knob::Decay { .. } => &[("secs", 1.0), ("ms", 0.001)],
            Knob::Punch(_) => &[("frac", 1.0)],
        }
    }

    /// How to write it by hand.
    fn how(self, old: &str, new: &str) -> String {
        match self {
            Knob::Tune { own, .. } => format!("{new}: semitones from its own {own} Hz, 12 × log2(Hz ÷ {own})"),
            Knob::Decay { own, .. } => format!("{new}: times its own {} s, seconds ÷ {}", exact(own), exact(own)),
            Knob::Punch(own) => format!("{new}: the {old} less {own}"),
        }
    }
}

/// A drum the Drum plays now: the single (or simple) one's name, the Drum's name for it, how each
/// of its knobs reads (old key, the Drum's key, how; None: the same knob), and the voices it played
/// at when the Drum's own 6 aren't them.
struct Drum {
    old: &'static str,
    drum: &'static str,
    knobs: &'static [(&'static str, &'static str, Option<Knob>)],
    voices: Option<u32>,
}

/// `drums.kick` … `drums.shaker` of `@m2/drums` 1 (and `std/drums` before it), as the Drum plays
/// them: at their own pitch and length and their own voices, so they are the single's sound
/// (CARD-33). The ranges are those the old drum clamped each value to.
const SINGLES: &[Drum] = &[
    Drum {
        old: "kick",
        drum: "kick",
        knobs: &[
            ("tune", "tune", tune(48.0, 20.0, 200.0)),
            ("decay", "decay", decay(0.45, 0.05, 5.0)),
            ("punch", "punch", Some(Knob::Punch(0.5))),
            ("drive", "drive", None),
        ],
        voices: Some(4),
    },
    Drum {
        old: "snare",
        drum: "snare",
        knobs: &[
            ("tune", "tune", tune(190.0, 60.0, 1000.0)),
            ("snap", "punch", Some(Knob::Punch(0.6))),
            ("decay", "decay", decay(0.18, 0.02, 2.0)),
        ],
        voices: Some(4),
    },
    Drum {
        old: "clap",
        drum: "clap",
        knobs: &[("tone", "tune", tune(1200.0, 300.0, 8000.0)), ("decay", "decay", decay(0.25, 0.03, 3.0))],
        voices: Some(4),
    },
    Drum {
        old: "hats",
        drum: "hats",
        knobs: &[("decay", "decay", Some(CLOSED)), ("openDecay", "decay", Some(OPEN)), ("tone", "tune", tune(8000.0, 2000.0, 16000.0))],
        voices: None,
    },
    Drum {
        old: "tom",
        drum: "tom",
        knobs: &[("tune", "tune", tune(130.0, 30.0, 600.0)), ("decay", "decay", decay(0.35, 0.03, 5.0))],
        voices: None,
    },
    Drum { old: "rim", drum: "rim", knobs: &[("tone", "tune", tune(1700.0, 300.0, 8000.0))], voices: Some(4) },
    Drum { old: "cowbell", drum: "cowbell", knobs: &[("tune", "tune", tune(540.0, 100.0, 3000.0))], voices: Some(4) },
    Drum { old: "cymbal", drum: "cymbal", knobs: &[("decay", "decay", decay(1.6, 0.1, 10.0))], voices: Some(4) },
    Drum { old: "shaker", drum: "shaker", knobs: &[("decay", "decay", decay(0.06, 0.01, 2.0))], voices: Some(4) },
];

/// The simple `kick` and `hat` of `std/instruments` and `@m2/synths` 1: other sounds than One
/// drum's kick and hats (it is said), their tune and decay kept as pitch and length.
const SIMPLE: &[Drum] = &[
    Drum {
        old: "kick",
        drum: "kick",
        knobs: &[("tune", "tune", tune(48.0, 20.0, 200.0)), ("decay", "decay", decay(0.45, 0.01, 2.0))],
        voices: None,
    },
    Drum { old: "hat", drum: "hats", knobs: &[("decay", "decay", decay(0.045, 0.005, 1.0))], voices: None },
];

/// std's modules that moved into the packs, whose names meant what they did before wave 3.
const MOVED_FROM: &[&str] = &["std/instruments", "std/synth", "std/drums"];
/// The modules the simple kick and hat came from.
const SIMPLE_FROM: &[&str] = &["std/instruments", "@m2/synths"];
/// The modules `drums` came from, and is from.
const DRUMS_FROM: &[&str] = &["std/drums", "@m2/drums"];
/// The modules `synth` came from, and is from.
const SYNTH_FROM: &[&str] = &["std/synth", "@m2/synths"];
/// The Drum's pack.
const DRUMS_PACK: &str = "@m2/drums";

/// Whether `name`, imported from `module`, is one no module has now: the simple kick and hat.
fn removed(module: &str, name: &str) -> bool {
    SIMPLE_FROM.contains(&module) && SIMPLE.iter().any(|d| d.old == name)
}

// ---------------------------------------------------------------- code

/// A named import or re-export: the name in its module, the name it binds here (an import's), and
/// its bytes.
#[derive(Clone, Debug)]
struct Spec {
    name: String,
    local: Option<String>,
    span: (u32, u32),
}

/// A top-level `import … from` or `export … from` statement.
#[derive(Clone, Debug)]
struct Stmt {
    span: (u32, u32),
    source: String,
    /// The module's string, its quotes included.
    source_span: (u32, u32),
    specs: Vec<Spec>,
    /// `import`, `import type`, `export` or `export type`.
    keyword: &'static str,
    /// What it takes whole: `import * as fx`'s `fx` (Some), or `export *` (None).
    whole: Option<Option<String>>,
}

fn statements(source: &str) -> Vec<Stmt> {
    use oxc::ast::ast::ModuleExportName;
    let name_of = |n: &ModuleExportName| n.name().as_str().to_string();
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut out = vec![];
    for statement in &ret.program.body {
        match statement {
            Statement::ImportDeclaration(i) => {
                let (mut specs, mut whole) = (vec![], None);
                for s in i.specifiers.iter().flatten() {
                    match s {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => specs.push(Spec {
                            name: name_of(&s.imported),
                            local: Some(s.local.name.to_string()),
                            span: (s.span.start, s.span.end),
                        }),
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => whole = Some(Some(s.local.name.to_string())),
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                    }
                }
                out.push(Stmt {
                    span: (i.span.start, i.span.end),
                    source: i.source.value.as_str().unwrap_or_default().to_string(),
                    source_span: (i.source.span.start, i.source.span.end),
                    specs,
                    keyword: if i.import_kind.is_type() { "import type" } else { "import" },
                    whole,
                });
            }
            Statement::ExportFromDeclaration(e) => {
                let specs =
                    e.specifiers.iter().map(|s| Spec { name: name_of(&s.local), local: None, span: (s.span.start, s.span.end) }).collect();
                out.push(Stmt {
                    span: (e.span.start, e.span.end),
                    source: e.source.value.as_str().unwrap_or_default().to_string(),
                    source_span: (e.source.span.start, e.source.span.end),
                    specs,
                    keyword: if e.export_kind.is_type() { "export type" } else { "export" },
                    whole: None,
                });
            }
            Statement::ExportAllDeclaration(e) => out.push(Stmt {
                span: (e.span.start, e.span.end),
                source: e.source.value.as_str().unwrap_or_default().to_string(),
                source_span: (e.source.span.start, e.source.span.end),
                specs: vec![],
                keyword: "export",
                whole: Some(None),
            }),
            _ => {}
        }
    }
    out
}

/// The packs `source` imports or re-exports from: `@m2/drums`, and `@alice/x` for `@alice/x/keys`.
fn imported_packs(source: &str) -> Vec<PackName> {
    statements(source)
        .iter()
        .filter_map(|i| {
            let mut parts = i.source.splitn(3, '/');
            let (owner, name) = (parts.next()?, parts.next()?);
            owner.starts_with('@').then(|| PackName::new(&format!("{owner}/{name}")).ok()).flatten()
        })
        .collect()
}

/// A knob's value as written.
#[derive(Clone)]
enum Value {
    /// A number, in the knob's base unit.
    Number(f64),
    /// A lane drives it: `lane.control("<key>", "<unit>", <fallback>)`.
    Lane { site: Site, key: String, unit: (String, (u32, u32)), fallback: Option<(u32, u32)> },
}

/// An edit: bytes `start..end` of the file become the text.
type Edit = (u32, u32, String);

/// A hat the hats single played by a note's pitch, each with its own length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Hat {
    Closed,
    Pedal,
    Open,
}

/// One code file as the upgrade reads it, and what it makes of it.
struct Code<'t> {
    file: String,
    text: &'t str,
    /// How its lines end: `\r\n` or `\n`.
    eol: &'static str,
    statements: Vec<Stmt>,
    sites: Vec<Site>,
    /// What the file's names stand for: local name to (module, the name it imports).
    locals: BTreeMap<String, (String, String)>,
    edits: Vec<Edit>,
    /// Where code is replaced: a name read only inside these is read no more.
    replaced: Vec<(u32, u32)>,
    /// The starts of the calls handled (converted or left), so no other check repeats them.
    handled: BTreeSet<u32>,
    left: Vec<Left>,
    /// Why it can't be read at all: an upgrade can't go on, but a song made for this music2 with a
    /// typo in it wasn't made for an older one (`needed`).
    unparsed: Option<Left>,
    notes: Vec<Left>,
    /// The packs its rewritten code plays.
    packs: BTreeSet<PackName>,
    /// Each lane file a converted knob reads, how, and where it is read.
    lanes: Vec<(String, Knob, Left)>,
    /// What the Drum's `drums` is called here, and whether its import must be added; or why it
    /// can't be.
    drums: Result<(String, bool), String>,
    /// The modules whose names mean what they did before wave 3 (`old_modules`).
    old: &'t BTreeSet<String>,
    /// The song's files, for the notes a hats call plays.
    files: &'t BTreeMap<PathBuf, Arc<str>>,
}

impl<'t> Code<'t> {
    fn read(file: &str, text: &'t str, old: &'t BTreeSet<String>, files: &'t BTreeMap<PathBuf, Arc<str>>) -> Code<'t> {
        let statements = statements(text);
        let mut locals = BTreeMap::new();
        for i in statements.iter().filter(|i| i.keyword.starts_with("import")) {
            for s in &i.specs {
                if let Some(l) = &s.local {
                    locals.insert(l.clone(), (i.source.clone(), s.name.clone()));
                }
            }
        }
        let (sites, unparsed) = match m2_script::sites::instrument(file, text, 0) {
            Ok(i) => (i.sites, None),
            Err(e) => (
                vec![],
                Some(Left { file: file.into(), line: 1, what: format!("it doesn't read as TypeScript: {e}"), write: String::new() }),
            ),
        };
        let drums = match locals.iter().find(|(_, (m, n))| DRUMS_FROM.contains(&m.as_str()) && n == "drums") {
            Some((local, _)) => Ok((local.clone(), false)),
            None if bound_names(text).contains("drums") => Err("this file has a `drums` of its own".to_string()),
            None => Ok(("drums".to_string(), true)),
        };
        Code {
            file: file.into(),
            text,
            eol: if text.contains("\r\n") { "\r\n" } else { "\n" },
            statements,
            sites,
            locals,
            edits: vec![],
            replaced: vec![],
            handled: BTreeSet::new(),
            left: vec![],
            unparsed,
            notes: vec![],
            packs: BTreeSet::new(),
            lanes: vec![],
            drums,
            old,
            files,
        }
    }

    /// Whether local name `name` is `what`, imported from one of `from`: a removed name whatever
    /// the manifest says (`gated`: false), else only from a module as it was before wave 3.
    fn is(&self, name: &str, from: &[&str], what: &str, gated: bool) -> bool {
        self.locals.get(name).is_some_and(|(m, n)| from.contains(&m.as_str()) && n == what && (!gated || self.old.contains(m)))
    }

    /// What local name `local` is of `@m2/core`, if it is one of its: `hz` for `hz`.
    fn core(&self, local: &str) -> Option<&str> {
        self.locals.get(local).filter(|(m, _)| m == m2_song::SDK).map(|(_, n)| n.as_str())
    }

    fn slice(&self, (a, b): (u32, u32)) -> &'t str {
        &self.text[a as usize..b as usize]
    }

    fn line_at(&self, at: u32) -> u32 {
        self.text[..at as usize].matches('\n').count() as u32 + 1
    }

    fn leave(&mut self, at: u32, what: String, write: String) {
        let line = self.line_at(at);
        self.left.push(Left { file: self.file.clone(), line, what, write });
    }

    fn upgrade(&mut self) {
        let sites = std::mem::take(&mut self.sites);
        for site in &sites {
            let callee = site.callee.trim();
            let (object, member) = match callee.rsplit_once('.') {
                Some((o, m)) => (o.trim(), Some(m.trim())),
                None => (callee, None),
            };
            match member {
                None if self.is(object, SIMPLE_FROM, "kick", false) || self.is(object, SIMPLE_FROM, "hat", false) => {
                    let name = self.locals[object].1.clone();
                    let d = SIMPLE.iter().find(|d| d.old == name).expect("kick or hat");
                    self.drum(site, &sites, d, false);
                }
                Some(m) if self.is(object, DRUMS_FROM, "drums", false) && SINGLES.iter().any(|d| d.old == m) => {
                    let d = SINGLES.iter().find(|d| d.old == m).expect("a single");
                    self.drum(site, &sites, d, true);
                }
                Some("bass") if self.is(object, SYNTH_FROM, "synth", true) => self.bass(site),
                None if self.core(object) == Some("kernel") => self.kernel(site),
                _ => {}
            }
        }
        self.sites = sites;
        self.others();
        self.wholes();
        self.rewrite_statements();
    }

    /// The hats a call's notes play, by the lane its notes come from: none when its file isn't there;
    /// None when its notes aren't a lane of the song's (then every hat may play).
    fn hats_played(&self, site: &Site, sites: &[Site]) -> Option<BTreeSet<Hat>> {
        let span = site.args.first()?.span();
        let call = sites.iter().find(|s| (s.start, s.end) == span)?;
        if !call.callee.trim().ends_with(".notes") {
            return None;
        }
        let key = string(self.slice(call.args.first()?.span()))?;
        let module = self.module_of(call, sites)?;
        let Some(text) = self.files.get(Path::new(&format!("data/{module}/{key}.json"))) else { return Some(BTreeSet::new()) };
        let LaneData::Notes(clips) = LaneData::read(text, &LaneKind::Notes).ok()? else { return None };
        Some(
            clips
                .iter()
                .flat_map(|c| &c.notes)
                .map(|n| match n.pitch.round() as i64 {
                    44 => Hat::Pedal,
                    46 => Hat::Open,
                    _ => Hat::Closed,
                })
                .collect(),
        )
    }

    /// `kick(x)`, `drums.snare(x, { snap: 0.75 })` as the Drum: `drums.drum(x, { drum: "snare",
    /// punch: 0.15, voices: 4 })`. All or nothing: a call with a value it can't convert exactly is
    /// left as it is, and named.
    fn drum(&mut self, site: &Site, sites: &[Site], d: &Drum, single: bool) {
        self.handled.insert(site.start);
        let notes = site.args.first().map(|a| self.slice(a.span()).to_string()).unwrap_or_default();
        let (mut edits, mut replaced, mut lanes, mut why) = (vec![], vec![], vec![], vec![]);
        let drums = match &self.drums {
            Ok((local, _)) => local.clone(),
            Err(e) => {
                why.push(e.clone());
                "drums".into()
            }
        };
        let drum = (d.drum != "kick").then(|| format!("drum: \"{}\"", d.drum));
        let opts = site.args.get(1);
        if site.args.len() > 2 {
            why.push("it is given more than its notes and options".into());
        }
        let props = match opts {
            None => Some(vec![]),
            Some(Arg::Object { start, end, .. }) => properties(self.text, (*start, *end)),
            Some(_) => None,
        };
        let Some(props) = props else {
            let opts = opts.map(|a| self.slice(a.span())).unwrap_or_default();
            why.push(format!("its options are worked out by code (`{opts}`)"));
            let hows: Vec<String> = d.knobs.iter().filter_map(|(o, n, k)| k.map(|k| k.how(o, n))).collect();
            let first = drum.as_ref().map_or_else(String::new, |d| format!("{d}, "));
            let write = format!("{drums}.drum({notes}, {{ {first}… }}), its knobs as the Drum's: {}", hows.join("; "));
            return self.leave(site.start, format!("`{}`: {}", site.callee.trim(), why.join("; ")), write);
        };
        // Each knob of the old drum this call writes: its property and its value.
        let mut given: BTreeMap<&str, (&Property, Value)> = BTreeMap::new();
        let mut suggested: Vec<String> = vec![];
        for p in &props {
            let value = self.slice(p.value);
            match d.knobs.iter().find(|(old, ..)| *old == p.key.as_str()) {
                None if p.key == "voices" => suggested.push(format!("voices: {value}")),
                None => why.push(format!("the Drum has no `{}`", p.key)),
                Some((_, new, None)) => suggested.push(format!("{new}: {value}")),
                Some((old, _, Some(knob))) => match self.value(p.value, sites, *knob) {
                    Ok(v) => drop(given.insert(old, (p, v))),
                    Err(()) => why.push(format!("its `{}` is worked out by code (`{value}`)", p.key)),
                },
            }
        }
        // How each old knob reads now; for the tom, its decay by its tune.
        let mut knobs: BTreeMap<&str, Knob> = d.knobs.iter().filter_map(|(o, _, k)| Some((*o, (*k)?))).collect();
        // Knobs to add, which the call leaves out: (key, value).
        let mut extra: Vec<(&str, f64)> = vec![];
        if d.drum == "tom" {
            // The Drum rings a tom longer the lower it is tuned from its own 130 Hz, where the tom
            // single rang its decay at every tune: its decay is times sqrt(tune ÷ 130).
            let tuned = match given.get("tune") {
                Some((_, Value::Lane { .. })) => {
                    why.push(
                        "its tune is on a lane: the Drum rings a tom longer the lower it is tuned, where this tom rang the same at every tune"
                            .into(),
                    );
                    None
                }
                Some((_, Value::Number(x))) => Some(x.clamp(30.0, 600.0)),
                None => Some(130.0),
            };
            if let Some(t) = tuned {
                knobs.insert("decay", Knob::Decay { own: 0.35 * (130.0 / t).sqrt(), lo: 0.03, hi: 5.0 });
                if !given.contains_key("decay") && t != 130.0 {
                    extra.push(("decay", (t / 130.0).sqrt()));
                }
            }
        }
        // What each property becomes: a value (None: it goes, its knob said by another).
        let mut values: BTreeMap<&str, Option<Value>> = given.iter().map(|(k, (_, v))| (*k, Some(v.clone()))).collect();
        if d.drum == "hats" {
            self.hats(site, sites, &given, &mut values, &mut knobs, &mut why);
        }
        // Each property as the Drum reads it.
        for p in &props {
            let Some(v) = values.get(p.key.as_str()) else { continue };
            let new = d.knobs.iter().find(|(o, ..)| *o == p.key).map(|(_, n, _)| *n).expect("a knob");
            let knob = knobs[p.key.as_str()];
            let Some(v) = v else {
                edits.push(prop_removal(&props, p));
                continue;
            };
            let said = match v {
                Value::Number(x) => match knob.convert(*x) {
                    Some(c) => {
                        edits.push((p.value.0, p.value.1, exact(c)));
                        replaced.push(p.value);
                        Some(format!("{new}: {}", exact(c)))
                    }
                    None => {
                        why.push(format!("its `{}` of {} is past what the Drum plays", p.key, self.slice(p.value)));
                        None
                    }
                },
                Value::Lane { site: call, key, unit, fallback } => match self.lane(call, sites, knob, key, unit.clone(), *fallback) {
                    Ok((e, r, l)) => {
                        edits.extend(e);
                        replaced.extend(r);
                        lanes.push(l);
                        Some(format!("{new}: <its lane, in {}>", knob.unit().1))
                    }
                    Err(e) => {
                        why.push(e);
                        None
                    }
                },
            };
            suggested.push(said.unwrap_or_else(|| format!("<{}>", knob.how(&p.key, new))));
            if new != p.key {
                match key_span(self.text, p) {
                    Some((a, b, quote)) => edits.push((a, b, format!("{quote}{new}{quote}"))),
                    None => why.push(format!("its `{}` is written so its name can't be changed", p.key)),
                }
            }
        }
        let mut extras: Vec<String> = vec![];
        for (k, v) in extra {
            match Some(v).filter(|v| (0.05..=20.0).contains(v)) {
                Some(v) => extras.push(format!("{k}: {}", exact(v))),
                None => why.push(format!("its {k} would be past what the Drum plays")),
            }
        }
        if let Some(n) = d.voices
            && !props.iter().any(|p| p.key == "voices")
        {
            extras.push(format!("voices: {n}"));
        }
        if !why.is_empty() {
            let all: Vec<String> = drum.iter().cloned().chain(suggested).chain(extras).collect();
            let opts = if all.is_empty() { String::new() } else { format!(", {{ {} }}", all.join(", ")) };
            let write = format!("{drums}.drum({notes}{opts})");
            return self.leave(site.start, format!("`{}`: {}", site.callee.trim(), why.join("; ")), write);
        }
        // The callee: `drums.drum`, keeping how `drums` is written.
        let callee_end = site.start + site.callee.trim_end().len() as u32;
        if single {
            edits.push((callee_end - d.old.len() as u32, callee_end, "drum".into()));
        } else {
            edits.push((site.start, callee_end, format!("{drums}.drum")));
            replaced.push((site.start, callee_end));
            let what = if d.drum == "kick" { "kick" } else { "hats" };
            self.notes.push(Left {
                file: self.file.clone(),
                line: self.line_at(site.start),
                what: format!("the simple {} is the Drum's {what} now: another sound, a drum machine's", d.old),
                write: String::new(),
            });
        }
        let new_props: Vec<String> = drum.iter().cloned().chain(extras.iter().cloned()).collect();
        match (opts, props.first(), props.last()) {
            (Some(_), Some(first), Some(last)) => {
                if let Some(drum) = &drum {
                    edits.push((first.start, first.start, format!("{drum}, ")));
                }
                if !extras.is_empty() {
                    edits.push((last.end, last.end, format!(", {}", extras.join(", "))));
                }
            }
            (Some(o), ..) if !new_props.is_empty() => {
                let (a, b) = o.span();
                edits.push((a, b, format!("{{ {} }}", new_props.join(", "))));
            }
            (None, ..) if !new_props.is_empty() => {
                let end = site.args.first().map_or(site.open + 1, |a| a.span().1);
                edits.push((end, end, format!(", {{ {} }}", new_props.join(", "))));
            }
            _ => {}
        }
        self.edits.extend(edits);
        self.replaced.extend(replaced);
        self.lanes.extend(lanes);
        self.packs.insert(PackName::new(DRUMS_PACK).expect("a pack's name"));
    }

    /// The hats single's `decay` and `openDecay` as the Drum's one `decay`, which sets every hat's
    /// length, where the single had its closed hats', its open hats' and its pedal hats' (25 ms).
    /// Exact when every hat the call's lane plays keeps its length: then the first of the two the
    /// call writes says it, as a factor of the Drum's, and the other goes. Otherwise named.
    fn hats(
        &self,
        site: &Site,
        sites: &[Site],
        given: &BTreeMap<&str, (&Property, Value)>,
        values: &mut BTreeMap<&str, Option<Value>>,
        knobs: &mut BTreeMap<&str, Knob>,
        why: &mut Vec<String>,
    ) {
        if !given.contains_key("decay") && !given.contains_key("openDecay") {
            return;
        }
        let played = self.hats_played(site, sites).unwrap_or_else(|| [Hat::Closed, Hat::Pedal, Hat::Open].into());
        // What each hat played needs of the Drum's decay: a factor, or the lane of `decay` or
        // `openDecay`.
        let need = |k: &'static str, knob: Knob| match given.get(k) {
            Some((_, Value::Number(x))) => knob.convert(*x).ok_or(k),
            Some((_, Value::Lane { .. })) => Err(k),
            None => Ok(1.0),
        };
        let needs: Vec<Result<f64, &str>> = played
            .iter()
            .map(|h| match h {
                Hat::Pedal => Ok(1.0),
                Hat::Closed => need("decay", CLOSED),
                Hat::Open => need("openDecay", OPEN),
            })
            .collect();
        let lanes: BTreeSet<&str> = needs.iter().filter_map(|n| n.err()).collect();
        let factors: Vec<f64> = needs.iter().filter_map(|n| n.ok()).collect();
        let same = factors.windows(2).all(|w| (w[0] - w[1]).abs() <= 1e-9);
        let first = |k: &str| given.get(k).map(|(p, _)| p.start);
        // Which of the two says the Drum's decay, and with which knob.
        let carrier = match (lanes.len(), factors.is_empty()) {
            (1, true) => lanes.iter().next().copied(),
            (0, _) if same => ["decay", "openDecay"].into_iter().filter(|k| given.contains_key(k)).min_by_key(|k| first(k)),
            _ => None,
        };
        let Some(carrier) = carrier else {
            let ms = |k: &str, knob: Knob, own: f64| match (given.get(k), knob) {
                (Some((_, Value::Number(x))), Knob::Decay { lo, hi, .. }) => format!("{} ms", exact(x.clamp(lo, hi) * 1000.0)),
                (Some((p, _)), _) => format!("as `{}` says", self.slice(p.value)),
                (None, _) => format!("{} ms", exact(own * 1000.0)),
            };
            let pedal = if played.contains(&Hat::Pedal) { ", its pedal hats 25 ms" } else { "" };
            why.push(format!(
                "its closed hats ring {} and its open hats {}{pedal}, and the Drum's `decay` sets every hat's length: put the hats that \
                 ring longer on a lane of their own",
                ms("decay", CLOSED, 0.045),
                ms("openDecay", OPEN, 0.45)
            ));
            return;
        };
        for k in ["decay", "openDecay"] {
            if given.contains_key(k) {
                values.insert(k, None);
            }
        }
        match (&given[carrier].1, factors.first()) {
            (Value::Lane { .. }, _) => drop(values.insert(carrier, Some(given[carrier].1.clone()))),
            // The factor, written through a knob that gives it back as it is.
            (_, Some(f)) => {
                values.insert(carrier, Some(Value::Number(*f)));
                knobs.insert(carrier, Knob::Decay { own: 1.0, lo: f64::MIN, hi: f64::MAX });
            }
            // No hat plays: the carrier's own factor.
            (v, None) => {
                values.insert(carrier, Some(v.clone()));
            }
        }
    }

    /// What a knob's value is: a number (in base units), or a lane; Err: worked out by code.
    fn value(&self, span: (u32, u32), sites: &[Site], knob: Knob) -> Result<Value, ()> {
        if let Some(x) = literal(self.slice(span)) {
            return Ok(Value::Number(x));
        }
        let call = sites.iter().find(|s| (s.start, s.end) == span).ok_or(())?;
        let callee = call.callee.trim();
        // A unit constructor of @m2/core: `hz(50)`, `ms(30)`.
        if let Some(name) = self.core(callee)
            && let Some((_, scale)) = knob.spellings().iter().find(|(n, _)| *n == name)
        {
            return match &call.args[..] {
                [Arg::Number { value, .. }] => Ok(Value::Number(value * scale)),
                _ => Err(()),
            };
        }
        // `lane.control("hats-decay", "seconds", ms(45))`: a lane drives it.
        if callee.ends_with(".control")
            && let [key, unit, rest @ ..] = &call.args[..]
            && rest.len() <= 2
            && let (Some(k), Some(u)) = (string(self.slice(key.span())), string(self.slice(unit.span())))
        {
            return Ok(Value::Lane { site: call.clone(), key: k, unit: (u, unit.span()), fallback: rest.first().map(Arg::span) });
        }
        Err(())
    }

    /// A knob's lane converted: its unit and fallback in the code, and its file (by the module
    /// whose function reads it). The edits, what they replace, and the lane.
    #[allow(clippy::type_complexity)]
    fn lane(
        &self,
        call: &Site,
        sites: &[Site],
        knob: Knob,
        key: &str,
        (unit, unit_at): (String, (u32, u32)),
        fallback: Option<(u32, u32)>,
    ) -> Result<(Vec<Edit>, Vec<(u32, u32)>, (String, Knob, Left)), String> {
        let (from, to) = knob.unit();
        if unit != from {
            return Err(format!("its lane `{key}` is in {unit}, not {from}"));
        }
        let module = self.module_of(call, sites).ok_or_else(|| format!("which track its lane `{key}` is on can't be told"))?;
        let quote = &self.slice(unit_at)[..1];
        let mut edits = vec![(unit_at.0, unit_at.1, format!("{quote}{to}{quote}"))];
        let mut replaced = vec![];
        let was = match fallback {
            Some(span) => match self.value(span, sites, knob) {
                Ok(Value::Number(x)) => Some((span, x)),
                _ => return Err(format!("its lane `{key}`'s fallback is worked out by code")),
            },
            None => None,
        };
        let now =
            knob.convert(was.map_or(0.0, |(_, x)| x)).ok_or_else(|| format!("its lane `{key}`'s fallback is past what the Drum plays"))?;
        match was {
            Some((span, _)) => {
                edits.push((span.0, span.1, exact(now)));
                replaced.push(span);
            }
            None => edits.push((unit_at.1, unit_at.1, format!(", {}", exact(now)))),
        }
        let at = Left { file: self.file.clone(), line: self.line_at(call.start), what: String::new(), write: String::new() };
        Ok((edits, replaced, (format!("data/{module}/{key}.json"), knob, at)))
    }

    /// The module whose function `call` is in: the innermost `track("<key>", …)` or `bus(…)` around
    /// it, or the song's own.
    fn module_of(&self, call: &Site, sites: &[Site]) -> Option<String> {
        let around = sites
            .iter()
            .filter(|s| s.start <= call.start && call.end <= s.end && (s.start, s.end) != (call.start, call.end))
            .filter(|s| matches!(self.core(s.callee.trim()), Some("track" | "bus" | "song")))
            .max_by_key(|s| s.start)?;
        match self.core(around.callee.trim())? {
            "song" => Some(m2_song::ROOT.to_string()),
            _ => string(self.slice(around.args.first()?.span())),
        }
    }

    /// `synth.bass(x, { decay })`: its filter's decay is `biteDecay` now, `decay` the envelope's.
    fn bass(&mut self, site: &Site) {
        self.handled.insert(site.start);
        let say = "write its `decay` as `biteDecay`: a bass's `decay` is how its notes fade now".to_string();
        match site.args.get(1) {
            None => {}
            Some(Arg::Object { start, end, .. }) => {
                let Some(props) = properties(self.text, (*start, *end)) else {
                    return self.leave(site.start, "`synth.bass`'s options are worked out by code".into(), say);
                };
                if let Some(p) = props.iter().find(|p| p.key == "decay") {
                    match key_span(self.text, p) {
                        Some((a, b, quote)) => self.edits.push((a, b, format!("{quote}biteDecay{quote}"))),
                        None => self.leave(site.start, "`synth.bass`'s `decay` is written so its name can't be changed".into(), say),
                    }
                }
            }
            Some(a) => {
                let opts = self.slice(a.span()).to_string();
                self.leave(site.start, format!("`synth.bass`'s options are worked out by code (`{opts}`)"), say);
            }
        }
    }

    /// `kernel("std:fm", …)`: the kernel's id in the pack it moved to (`m2_ir::MOVED_KERNELS`).
    fn kernel(&mut self, site: &Site) {
        let Some(first) = site.args.first() else { return };
        let span = first.span();
        let Some(id) = string(self.slice(span)) else { return };
        let Some(m) = m2_ir::MOVED_KERNELS.iter().find(|m| m.from == id) else { return };
        let quote = &self.slice(span)[..1];
        self.edits.push((span.0, span.1, format!("{quote}{}{quote}", m.to)));
        if let Some((p, _)) = m.to.split_once(':')
            && let Ok(name) = PackName::new(p)
        {
            self.packs.insert(name);
        }
    }

    /// Every other use of a name whose meaning changed: the simple kick or hat, a single drum,
    /// `synth.bass`, where it isn't called. Left as it is, and named.
    fn others(&mut self) {
        let mut found = vec![];
        for (local, (_, name)) in &self.locals {
            let simple = self.is(local, SIMPLE_FROM, name, false) && SIMPLE.iter().any(|d| d.old == name);
            let drums = self.is(local, DRUMS_FROM, "drums", false);
            let synth = self.is(local, SYNTH_FROM, "synth", true);
            if !(simple || drums || synth) {
                continue;
            }
            for at in references(self.text, local) {
                if self.handled.contains(&at) {
                    continue;
                }
                let after = self.text[at as usize + local.len()..].trim_start();
                let member = after.strip_prefix('.').map(str::trim_start).map(|m| {
                    let end = m.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).unwrap_or(m.len());
                    &m[..end]
                });
                let drum_call = |drum: &str| {
                    let opts = if drum == "kick" { String::new() } else { format!(", {{ drum: \"{drum}\" }}") };
                    format!("drums.drum(notes{opts}), from \"@m2/drums\"")
                };
                let found_here = match member {
                    _ if simple => {
                        let d = SIMPLE.iter().find(|d| d.old == name).expect("kick or hat");
                        Some((format!("`{local}` is used here, not called: the Drum plays the {name} now"), drum_call(d.drum)))
                    }
                    Some(m) if drums && SINGLES.iter().any(|d| d.old == m) => {
                        Some((format!("`{local}.{m}` is used here, not called: the Drum plays the {m} now"), drum_call(m)))
                    }
                    Some("bass") if synth => Some((
                        format!("`{local}.bass` is used here, not called: its `decay` is `biteDecay` now"),
                        "write its `decay` as `biteDecay`".to_string(),
                    )),
                    _ => None,
                };
                if let Some(f) = found_here {
                    found.push((at, f));
                }
            }
        }
        for (at, (what, write)) in found {
            self.leave(at, what, write);
        }
    }

    /// Modules taken whole whose names moved or were removed: `import * as fx from "std/fx"`, a
    /// pack's `import * as pk` whose removed names it plays, `export * from "std/instruments"`.
    /// Named, with the imports to write.
    fn wholes(&mut self) {
        let mut found = vec![];
        for st in self.statements.iter().filter(|s| s.whole.is_some()) {
            let Some(ns) = st.whole.clone().flatten() else {
                if moved_module(&st.source) {
                    let what = format!("it re-exports all of `{}`, whose names moved into the packs", st.source);
                    found.push((st.span.0, what, format!("export each name from where it is now, as an import of `{}` would", st.source)));
                }
                continue;
            };
            // The names it reads through `ns`, and any removed one it plays.
            let mut names: Vec<String> = vec![];
            for at in references(self.text, &ns) {
                let after = self.text[at as usize + ns.len()..].trim_start();
                let Some(m) = after.strip_prefix('.').map(str::trim_start) else { continue };
                let end = m.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).unwrap_or(m.len());
                let n = &m[..end];
                if !names.iter().any(|x| x == n) {
                    names.push(n.to_string());
                }
                let rest = m[end..].trim_start().strip_prefix('.').map(str::trim_start).unwrap_or("");
                let single =
                    SINGLES.iter().find(|d| rest.starts_with(d.old) && !rest[d.old.len()..].starts_with(|c: char| c.is_alphanumeric()));
                let what = match (n, single) {
                    (n, _) if removed(&st.source, n) => Some(format!("`{ns}.{n}` plays the simple {n}, which no pack has now")),
                    ("drums", Some(d)) if DRUMS_FROM.contains(&st.source.as_str()) => {
                        Some(format!("`{ns}.drums.{}` is the Drum's now", d.old))
                    }
                    _ => None,
                };
                if let Some(what) = what {
                    found.push((at, what, "import { drums } from \"@m2/drums\", and play drums.drum(notes, { drum: \"…\" })".into()));
                }
            }
            if moved_module(&st.source) {
                let mut groups: Vec<(String, Vec<String>)> = vec![];
                for n in &names {
                    let (to, n) = match removed(&st.source, n) {
                        true => (DRUMS_PACK.to_string(), "drums".to_string()),
                        false => (moved_import(&st.source, n).unwrap_or(&st.source).to_string(), n.clone()),
                    };
                    match groups.iter_mut().find(|(m, _)| *m == to) {
                        Some((_, g)) if !g.contains(&n) => g.push(n),
                        Some(_) => {}
                        None => groups.push((to, vec![n])),
                    }
                }
                if groups.iter().any(|(m, _)| *m != st.source) {
                    let lines: Vec<String> = groups.iter().map(|(m, g)| format!("import {{ {} }} from \"{m}\";", g.join(", "))).collect();
                    let what = format!("`{}` is imported whole as `{ns}`, and what it held moved", st.source);
                    found.push((st.span.0, what, format!("{} Then write `{ns}.name` as `name`", lines.join(" "))));
                }
            }
        }
        for (at, what, write) in found {
            self.leave(at, what, write);
        }
    }

    /// The imports and re-exports, as the code now reads them: a moved name from where it is now (a
    /// statement of its own after the one it was in, which keeps the rest, with their comments), a
    /// name nothing reads any more gone, and the Drum's `drums` where the simple kick or hat was.
    fn rewrite_statements(&mut self) {
        let simple_gone = self.locals.iter().any(|(l, (m, n))| removed(m, n) && self.unread(l));
        let mut add = match &self.drums {
            Ok((local, true)) if simple_gone => Some(local.clone()),
            _ => None,
        };
        let statements = std::mem::take(&mut self.statements);
        for st in statements.iter().filter(|s| s.whole.is_none()) {
            let export = st.keyword.starts_with("export");
            // Each name's fate: kept here, gone, or to another module (with the comments it takes).
            let mut kept: Vec<usize> = vec![];
            let mut groups: Vec<(String, Vec<String>, Vec<String>)> = vec![];
            let mut dropped_simple = false;
            let push = |groups: &mut Vec<(String, Vec<String>, Vec<String>)>, to: &str, name: Option<String>, comment: Option<String>| {
                match groups.iter_mut().find(|(m, ..)| m == to) {
                    Some((_, names, comments)) => {
                        names.extend(name);
                        comments.extend(comment);
                    }
                    None => groups.push((to.to_string(), name.into_iter().collect(), comment.into_iter().collect())),
                }
            };
            for (i, s) in st.specs.iter().enumerate() {
                let comment = trailing_comment(self.text, s.span.1);
                if export && removed(&st.source, &s.name) {
                    let what = format!("it re-exports the simple {}, which no pack has now", s.name);
                    self.leave(s.span.0, what, "re-export `drums` from \"@m2/drums\", and play drums.drum(notes, { drum: \"…\" })".into());
                    kept.push(i);
                    continue;
                }
                let local = s.local.as_deref().unwrap_or("");
                // Read no more, or a removed name never read: it goes.
                let unread = !export && self.unread(local);
                if unread || (!export && removed(&st.source, &s.name) && references(self.text, local).is_empty()) {
                    if unread && removed(&st.source, &s.name) {
                        dropped_simple = true;
                        if add.is_some() || self.drums.is_ok() {
                            // Its comment goes with the import that takes its place.
                            push(&mut groups, DRUMS_PACK, None, comment);
                        }
                    }
                    continue;
                }
                match moved_import(&st.source, &s.name) {
                    Some(to) => push(&mut groups, to, Some(self.slice(s.span).to_string()), comment),
                    None => kept.push(i),
                }
            }
            if dropped_simple && let Some(local) = add.take() {
                push(&mut groups, DRUMS_PACK, Some(local), None);
            }
            groups.retain(|(_, names, _)| !names.is_empty());
            if kept.len() == st.specs.len() && groups.is_empty() {
                continue;
            }
            let quote = &self.slice(st.source_span)[..1];
            let line = |(m, names, comments): &(String, Vec<String>, Vec<String>)| {
                let comment = if comments.is_empty() { String::new() } else { format!(" {}", comments.join(" ")) };
                format!("{} {{ {} }} from {quote}{m}{quote};{comment}", st.keyword, names.join(", "))
            };
            let end = st.span.1 + trailing_len(self.text, st.span.1);
            if kept.is_empty() && !groups.is_empty() {
                // None stays: the statement is the new ones'.
                let lines: Vec<String> = groups.iter().map(line).collect();
                self.edits.push((st.span.0, st.span.1, lines.join(self.eol)));
                continue;
            }
            if kept.is_empty() {
                // Nothing stays: the statement goes, its line with it unless a comment is on it.
                self.edits.push(statement_removal(self.text, st.span));
                continue;
            }
            let gone: Vec<usize> = (0..st.specs.len()).filter(|i| !kept.contains(i)).collect();
            self.edits.extend(spec_removals(self.text, st, &gone));
            if !groups.is_empty() {
                let after: String = groups.iter().map(|g| format!("{}{}", self.eol, line(g))).collect();
                self.edits.push((end, end, after));
            }
        }
        self.statements = statements;
    }

    /// Whether nothing reads local `name` any more: every place it was read is replaced.
    fn unread(&self, name: &str) -> bool {
        let refs = references(self.text, name);
        !refs.is_empty() && refs.iter().all(|r| self.replaced.iter().any(|(a, b)| a <= r && r < b))
    }
}

/// The comment on the rest of the line after byte `at` (past a comma): `// the closed hat`.
fn trailing_comment(text: &str, at: u32) -> Option<String> {
    let rest = &text[at as usize..];
    let line = &rest[..rest.find('\n').unwrap_or(rest.len())];
    let line = line.trim_start().trim_start_matches(',').trim_start().trim_end_matches('\r');
    (line.starts_with("//") || (line.starts_with("/*") && line.ends_with("*/"))).then(|| line.to_string())
}

/// How many bytes after a statement's end are a comment on its line, which goes with it.
fn trailing_len(text: &str, at: u32) -> u32 {
    let rest = &text[at as usize..];
    let line = rest[..rest.find('\n').unwrap_or(rest.len())].trim_end_matches('\r');
    if line.trim_start().starts_with("//") { line.len() as u32 } else { 0 }
}

/// The bytes that take a statement out: its whole lines when nothing else is on them, else the
/// statement alone (a comment after it stays).
fn statement_removal(text: &str, (start, end): (u32, u32)) -> Edit {
    let (s, e) = (start as usize, end as usize);
    let line_start = text[..s].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[e..].find('\n').map_or(text.len(), |i| e + i);
    let before = &text[line_start..s];
    let after = text[e..line_end].trim_end_matches('\r');
    if before.trim().is_empty() && after.trim().is_empty() {
        let end = if line_end < text.len() { line_end + 1 } else { line_end };
        return (line_start as u32, end as u32, String::new());
    }
    (start, end, String::new())
}

/// The bytes that take the names `gone` (by place) out of a statement's list: on a list of a name
/// a line, each one's line (the others, with their comments, stay as they are); on one line, the
/// list written again.
fn spec_removals(text: &str, st: &Stmt, gone: &[usize]) -> Vec<Edit> {
    let (Some(first), Some(last)) = (st.specs.first(), st.specs.last()) else { return vec![] };
    let open = text[st.span.0 as usize..first.span.0 as usize].rfind('{').map(|i| st.span.0 as usize + i);
    let close = text[last.span.1 as usize..st.source_span.0 as usize].find('}').map(|i| last.span.1 as usize + i);
    let (Some(open), Some(close)) = (open, close) else { return vec![] };
    let own_line = |s: &Spec| {
        let (a, b) = (s.span.0 as usize, s.span.1 as usize);
        let line_start = text[..a].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[b..].find('\n').map_or(text.len(), |i| b + i);
        let rest = text[b..line_end].trim_start().trim_start_matches(',').trim();
        let alone = text[line_start..a].trim().is_empty() && (rest.is_empty() || rest.starts_with("//") || rest.starts_with("/*"));
        alone.then_some((line_start as u32, (line_end + 1).min(text.len()) as u32))
    };
    if text[open..close].contains('\n') && gone.iter().all(|i| own_line(&st.specs[*i]).is_some()) {
        return gone.iter().filter_map(|i| own_line(&st.specs[*i]).map(|(a, b)| (a, b, String::new()))).collect();
    }
    let keep: Vec<&str> = (0..st.specs.len())
        .filter(|i| !gone.contains(i))
        .map(|i| &text[st.specs[i].span.0 as usize..st.specs[i].span.1 as usize])
        .collect();
    vec![(open as u32 + 1, close as u32, format!(" {} ", keep.join(", ")))]
}

/// Where a property's key is written, and its quote: `snap`, `"snap"`.
fn key_span(text: &str, p: &Property) -> Option<(u32, u32, &'static str)> {
    let rest = &text[p.start as usize..];
    let ends = |k: &str| rest.starts_with(k) && !rest[k.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$');
    if ends(&p.key) {
        return Some((p.start, p.start + p.key.len() as u32, ""));
    }
    for q in ["\"", "'"] {
        if ends(&format!("{q}{}{q}", p.key)) {
            return Some((p.start, p.start + p.key.len() as u32 + 2, q));
        }
    }
    None
}

/// The bytes that take property `p` out of its object, with the comma that joins it.
fn prop_removal(props: &[Property], p: &Property) -> Edit {
    let i = props.iter().position(|q| q.start == p.start).expect("its property");
    match (props.get(i + 1), i.checked_sub(1).map(|j| &props[j])) {
        (Some(next), _) => (p.start, next.start, String::new()),
        (None, Some(prev)) => (prev.end, p.end, String::new()),
        (None, None) => (p.start, p.end, String::new()),
    }
}

/// A number literal, maybe negative: `0.6`, `-3`, `1_000`; none in another base or with an
/// exponent.
fn literal(text: &str) -> Option<f64> {
    let t = text.trim();
    let digits = t.strip_prefix('-').map_or(t, str::trim_start);
    if !digits.starts_with(|c: char| c.is_ascii_digit() || c == '.') || !digits.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
    {
        return None;
    }
    let x: f64 = digits.replace('_', "").parse().ok()?;
    Some(if t.starts_with('-') { -x } else { x })
}

/// A plain string literal's text: `"hats"`, `'hz'`; none with an escape or a `${`.
fn string(text: &str) -> Option<String> {
    let q = text.chars().next().filter(|q| matches!(q, '"' | '\'' | '`'))?;
    let inner = text.strip_prefix(q)?.strip_suffix(q)?;
    (!inner.contains(['\\', q]) && !inner.contains("${")).then(|| inner.to_string())
}

/// A number as data and code write it: its shortest form (`0.1`, `2`, `-0.2`), never `-0`.
fn number(x: f64) -> String {
    m2_song::lane::number(x)
}

/// A converted value as the upgrade writes it: in the fewest decimals, from three, that give it
/// back to half a millionth (`0.706724` semitones, `0.666667` times, `0.15`).
fn exact(v: f64) -> String {
    for d in 3..=9 {
        let scale = 10f64.powi(d);
        let r = (v * scale).round() / scale;
        if (r - v).abs() <= 5e-7 {
            return number(r);
        }
    }
    number(v)
}

/// `text` with `edits` made, each `(start, end, new)` in its bytes. Edits that overlap are refused,
/// at the byte where they meet: none is made.
fn apply(text: &str, edits: &[Edit]) -> Result<String, u32> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|(a, b, _)| (*a, *b));
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    for (a, b, new) in sorted {
        let (a, b) = (*a as usize, *b as usize);
        if a < at || b < a || b > text.len() {
            return Err(a as u32);
        }
        out.push_str(&text[at..a]);
        out.push_str(new);
        at = b;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

/// What `edits` change in `text`, as the upgrade says it: each run of changed lines, its first
/// line's number, and the lines before (`-`) and after (`+`).
fn shown(text: &str, edits: &[Edit]) -> Vec<String> {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|(a, b, _)| (*a, *b));
    let line_start = |at: usize| text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = |at: usize| text[at..].find('\n').map_or(text.len(), |i| at + i);
    // Runs of edits whose lines touch, each as whole lines' bytes.
    let mut runs: Vec<(usize, usize, Vec<&Edit>)> = vec![];
    for e in sorted {
        let (a, b) = (line_start(e.0 as usize), line_end(e.1 as usize));
        match runs.last_mut() {
            Some(run) if a <= run.1 => {
                run.1 = run.1.max(b);
                run.2.push(e);
            }
            _ => runs.push((a, b, vec![e])),
        }
    }
    let mut out = vec![];
    for (a, b, run) in runs {
        let line = (text[..a].matches('\n').count() + 1).to_string();
        let local: Vec<Edit> = run.iter().map(|(s, e, t)| (*s - a as u32, *e - a as u32, t.clone())).collect();
        let before = &text[a..b];
        let Ok(after) = apply(before, &local) else { continue };
        let mut first = Some(line.as_str());
        let width = line.len();
        for (sign, l) in before.lines().map(|l| ('-', l)).chain(after.lines().map(|l| ('+', l))) {
            out.push(format!("{:>width$} {sign} {}", first.take().unwrap_or(""), l.trim_end_matches('\r')));
        }
    }
    out
}

// ---------------------------------------------------------------- data

/// A control lane's file with every point converted by `knob`, in the Drum's unit: the new text
/// and how many points; None when it is in that unit already. A lane moves in straight lines in
/// its own unit: Hz in a straight line bend in semitones, so a tune lane gets points between, as
/// many as keep it within a cent of the old line.
fn convert_lane(text: &str, knob: Knob) -> Result<Option<(String, usize)>, String> {
    let (from, to) = knob.unit();
    let data = LaneData::read(text, &LaneKind::Control { unit: None }).map_err(|e| format!("it doesn't read as a lane: {e:?}"))?;
    let LaneData::Control { unit, mut clips } = data else { return Err("it isn't a control lane".into()) };
    match unit.as_deref() {
        Some(u) if u == to => return Ok(None),
        Some(u) if u != from => return Err(format!("it is in {u}, not {from}")),
        _ => {}
    }
    let value = |x: f64| -> Result<f64, String> {
        knob.convert(x).and_then(|v| exact(v).parse().ok()).ok_or_else(|| format!("its point of {} is past what the Drum plays", number(x)))
    };
    let mut n = 0;
    for c in &mut clips {
        let mut points = vec![];
        for (i, &(beat, x)) in c.points.iter().enumerate() {
            if i > 0 && matches!(knob, Knob::Tune { .. }) {
                bend(&mut points, c.points[i - 1], (beat, x), knob, 0);
            }
            points.push((beat, value(x)?));
        }
        n += points.len();
        c.points = points;
    }
    Ok(Some((m2_song::lane::canonical(&LaneData::Control { unit: Some(to.into()), clips }), n)))
}

/// The points between `(b0, x0)` and `(b1, x1)`, a straight line in Hz, that keep its line in
/// semitones within half a cent of it, pushed in order.
fn bend(points: &mut Vec<(f64, f64)>, (b0, x0): (f64, f64), (b1, x1): (f64, f64), knob: Knob, depth: u32) {
    if b1 <= b0 || x0 == x1 || depth > 12 {
        return;
    }
    let st = |x: f64| knob.convert(x).unwrap_or(0.0);
    // The worst of a few places along it: a chord of log2 bends most near its middle.
    let off = (1..8).map(|k| k as f64 / 8.0).map(|t| (st(x0 + t * (x1 - x0)) - (st(x0) + t * (st(x1) - st(x0)))).abs()).fold(0.0, f64::max);
    if off <= 0.005 {
        return;
    }
    let (bm, xm) = ((b0 + b1) / 2.0, (x0 + x1) / 2.0);
    bend(points, (b0, x0), (bm, xm), knob, depth + 1);
    points.push((bm, exact(st(xm)).parse().unwrap_or(st(xm))));
    bend(points, (bm, xm), (b1, x1), knob, depth + 1);
}

// ---------------------------------------------------------------- presets

/// The musician's presets in `dir` (`~/Music/music2/Presets`) that name a sound that moved or was
/// renamed: each named, with what to do. They are theirs, outside the song: never changed.
fn preset_notes(dir: &Path, catalog: &m2_script::Catalog) -> Vec<Left> {
    // A folder per sound, `<package>/<name>` (FMT-22): every preset file under `dir`.
    let (mut files, mut folders) = (vec![], vec![dir.to_path_buf()]);
    while let Some(d) = folders.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => folders.push(e.path()),
                Ok(t) if t.is_file() && e.path().extension().is_some_and(|x| x == "json") => files.push(e.path()),
                _ => {}
            }
        }
    }
    files.sort();
    let mut out = vec![];
    for file in files {
        let Some(p) = std::fs::read_to_string(&file).ok().and_then(|t| crate::presets::Preset::parse(&t).ok()) else { continue };
        let Some((package, id)) = pack::split_id(&p.sound) else { continue };
        let single =
            id.strip_prefix("drum-").and_then(|d| SINGLES.iter().find(|s| s.old == d)).filter(|_| matches!(package, "std" | "@m2/drums"));
        let simple = SIMPLE.iter().find(|s| s.old == id).filter(|_| matches!(package, "std" | "@m2/synths"));
        let moved = || {
            ["@m2/synths", "@m2/drums", "@m2/effects"]
                .iter()
                .map(|pk| format!("{pk}:{id}"))
                .find(|now| package == "std" && !catalog.components.contains_key(&p.sound) && catalog.components.contains_key(now))
        };
        let what = if let Some(d) = single {
            format!(
                "\"{}\" is for the single {} (`{}`), which the Drum plays now: no list offers it. Set the Drum's knobs as it says, by hand",
                p.name, d.old, p.sound
            )
        } else if let Some(d) = simple {
            format!("\"{}\" is for the simple {} (`{}`), which no pack has now: no list offers it", p.name, d.old, p.sound)
        } else if id == "synth-bass" && p.values.contains_key("decay") {
            format!("\"{}\" sets the bass's `decay`, which is `biteDecay` now: change `decay` to `biteDecay` in it", p.name)
        } else if let Some(now) = moved() {
            let folder = m2_song::path::slashed(&crate::presets::folder(&now));
            format!(
                "\"{}\" is for `{}`, which is `{now}` now: change its `sound` to \"{now}\", and move it to Presets/{folder}",
                p.name, p.sound
            )
        } else {
            continue;
        };
        let rel = file.strip_prefix(dir).map_or_else(|_| file.display().to_string(), m2_song::path::slashed);
        out.push(Left {
            file: format!("Presets/{rel}"),
            line: 0,
            what: format!("{what} (your presets aren't changed)"),
            write: String::new(),
        });
    }
    out
}

// ---------------------------------------------------------------- the manifest

/// `[dependencies]` as this app's: `@m2/core`, std and each of music2's own packs whose line asks
/// for an older version than this app has get the line for it, and each of music2's own packs the
/// code imports now that isn't listed gets its line, ended as the file's lines are. The text after,
/// and the edits.
fn dependencies(text: &str, m: &Manifest, imported: &BTreeSet<PackName>, app: &pack::App) -> Result<(String, Vec<Edit>), EditError> {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut edits = vec![];
    for (name, dep) in &m.dependencies {
        let Some(now) = dep.requirement().and_then(|req| older(name, req, app)) else { continue };
        let line = Requirement::admitting(now).to_string();
        let span = requirement_at(text, name)
            .ok_or_else(|| EditError::Rejected(format!("{}: write {name}'s line as \"{name}\" = \"{line}\"", m2_song::path::MANIFEST)))?;
        edits.push((span.0, span.1, line));
    }
    let (mut lines, mut at) = (String::new(), None);
    // Only music2's own: a pack of anyone else's the code imports and doesn't list was never this to add.
    for (name, now) in imported.iter().filter(|n| !m.dependencies.contains_key(n.as_str())).filter_map(|n| Some((n, pack::comes_with(n)?)))
    {
        let (place, line) = crate::packs::dependency_line(text, name, &Requirement::admitting(now))
            .ok_or_else(|| EditError::Rejected(format!("{} has no [dependencies] to add {name} to", m2_song::path::MANIFEST)))?;
        let line = if at.is_some() { line.trim_start_matches('\n') } else { &line };
        lines.push_str(&line.replace('\n', eol));
        at = Some(place);
    }
    if let Some(at) = at {
        edits.push((at, at, lines));
    }
    let after = apply(text, &edits).map_err(|at| clash(m2_song::path::MANIFEST, text, at))?;
    Ok((after, edits))
}

/// Where the requirement of `name` is written in `[dependencies]`: the bytes inside its quotes, on
/// a line `name = "…"` (the name plain or quoted). None for any other shape.
fn requirement_at(text: &str, name: &str) -> Option<(u32, u32)> {
    let mut offset = 0;
    let mut in_table = false;
    for line in text.split_inclusive('\n') {
        let bare = line.split('#').next().unwrap_or("").trim();
        if bare.starts_with('[') {
            in_table = bare == "[dependencies]";
        } else if in_table && let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            if key == name || key.strip_prefix('"').and_then(|k| k.strip_suffix('"')) == Some(name) {
                let v = value.trim_start();
                let start = offset + line.find('=')? + 1 + (value.len() - v.len());
                let end = v.strip_prefix('"')?.find('"')?;
                return Some(((start + 1) as u32, (start + 1 + end) as u32));
            }
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;

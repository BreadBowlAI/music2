//! `m2 upgrade <song>` and the app's Upgrade (FMT-25, AGENT-14): a song made for an older music2
//! moved to this one in one step, one undo step. Its `[dependencies]` ask for this app's
//! `@m2/core`, std and music2's own packs, and its `music2.lock` is written as `m2 pack lock`
//! writes it; in its code, what music2's own changes moved or renamed is rewritten by the tables
//! below, read with the build's parser (oxc, as `m2_script::sites` and `crate::sites` read it),
//! never by patterns over text; and a lane that drives a knob whose unit changed is converted
//! with it. Nothing else is touched. What it can't follow (a value worked out by code, a name used
//! some other way) is left as it is and named, with the line to write. It is written only when the
//! song then builds, as one save of every file (`Session::save_all_built`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
use crate::sites::{bound_names, properties, references, whole_lines};

/// What an upgrade's undo step is called.
pub const NAME: &str = "Upgrade";

/// A file the upgrade changes, as it says it: the lines before and after, or what changed in it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub file: String,
    pub lines: Vec<String>,
}

/// What the upgrade left as it is, to change by hand: where, why, and the line to write.
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
    /// What it left as it is.
    pub left: Vec<Left>,
    /// The revision it made, as one undo step; None with `--check`, with nothing to change, or
    /// when the song wouldn't build after it.
    pub written: Option<u64>,
    /// Why nothing was written: what doesn't build after it, in the build's words.
    pub refused: Vec<String>,
}

impl Upgraded {
    /// Made for this music2 already: nothing to change, nothing left.
    pub fn current(&self) -> bool {
        self.changes.is_empty() && self.left.is_empty()
    }
}

/// Whether the song at `s` asks for (or locks) an older `@m2/core`, std or one of music2's own
/// packs than this app has (FMT-3, PACK-19, PACK-24): what the app's status says, with Upgrade
/// (AGENT-14). Read from the manifest and the lock on disk alone, so it costs nothing while the
/// song doesn't build.
pub fn needed(s: &Session) -> bool {
    let files = &s.draft().files;
    let Some(text) = files.get(Path::new(m2_song::path::MANIFEST)) else { return false };
    let Ok(m) = Manifest::read(text) else { return false };
    let app = m2_session::app(&s.packages);
    let lock = files.get(Path::new(m2_song::path::LOCK)).and_then(|t| Lock::read(t).ok()).unwrap_or_default();
    m.dependencies.iter().any(|(name, dep)| dep.requirement().is_some_and(|req| older(name, req, &app).is_some()))
        || lock.packs.iter().any(|l| pack::comes_with(&l.name).is_some_and(|v| l.version < v))
}

/// What this app has of `name` when `req` asks for an older version of it (older only: a song
/// made for a newer music2 can't be moved back): `@m2/core`, std, or one of music2's own packs.
fn older(name: &str, req: &Requirement, app: &pack::App) -> Option<m2_song::Version> {
    let have = match name {
        m2_song::SDK => app.sdk,
        pack::STD => app.std,
        _ => PackName::new(name).ok().and_then(|n| pack::comes_with(&n)),
    };
    have.filter(|v| *v >= req.upper())
}

/// Upgrade the song (FMT-25): what it changes, and, unless `check`, those changes written as one
/// undo step when the song then builds. Nothing is written when it wouldn't build: `refused` says
/// why, and `left` what to change by hand.
pub fn upgrade(s: &mut Session, check: bool) -> Result<Upgraded, EditError> {
    let plan = plan(s)?;
    let mut out = Upgraded { changes: plan.files.iter().map(|f| f.change.clone()).collect(), left: plan.left, ..Upgraded::default() };
    if plan.files.is_empty() {
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
    Ok(plan(s)?.files.into_iter().map(|f| f.change.file).collect())
}

/// A file the upgrade writes: what it holds now (None: it isn't there), and after.
struct Planned {
    change: Change,
    before: Option<String>,
    after: String,
}

/// Everything the upgrade would write, and what it leaves.
struct Plan {
    files: Vec<Planned>,
    left: Vec<Left>,
}

/// What the upgrade writes, read from the files on disk (the draft: an old song doesn't build).
fn plan(s: &Session) -> Result<Plan, EditError> {
    let files = &s.draft().files;
    let text_of = |p: &str| files.get(Path::new(p)).map(|t| t.to_string());
    let manifest_text = text_of(m2_song::path::MANIFEST).ok_or_else(|| EditError::NotFound(m2_song::path::MANIFEST.into()))?;
    let manifest = Manifest::read(&manifest_text).map_err(|d| EditError::Rejected(d.message))?;
    let app = m2_session::app(&s.packages);
    let (mut left, mut code, mut packs) = (vec![], vec![], BTreeSet::new());
    let mut lanes: BTreeMap<String, Knob> = BTreeMap::new();
    // The modules whose names still mean what they meant before wave 3 renamed them: std's that
    // moved, and music2's own packs the song asks for an older major of. A song made for this
    // music2 keeps what its names mean now (its bass's `decay` is the envelope's).
    let mut old: BTreeSet<String> = MOVED_FROM.iter().map(|m| m.to_string()).collect();
    for (name, dep) in &manifest.dependencies {
        if PackName::new(name).is_ok_and(|n| pack::comes_with(&n).is_some())
            && dep.requirement().is_some_and(|req| older(name, req, &app).is_some())
        {
            old.insert(name.clone());
        }
    }
    for (path, text) in files.iter().filter(|(p, _)| matches!(DataPath::parse(p), DataPath::Code(_))) {
        let file = m2_song::path::slashed(path);
        let mut f = Code::read(&file, text, &old);
        f.upgrade();
        left.append(&mut f.left);
        packs.extend(f.packs.iter().cloned());
        for (lane, knob, at) in f.lanes.drain(..) {
            match lanes.get(&lane) {
                // One lane can't be converted two ways.
                Some(k) if *k != knob => left.push(Left { what: format!("{lane} drives two knobs whose units changed differently"), ..at }),
                _ => drop(lanes.insert(lane, knob)),
            }
        }
        let after = if f.edits.is_empty() { text.to_string() } else { apply(text, &f.edits) };
        // The packs the code imports, as it will be.
        packs.extend(imported_packs(&after));
        if !f.edits.is_empty() {
            code.push(Planned { change: Change { file, lines: shown(text, &f.edits) }, before: Some(text.to_string()), after });
        }
    }
    // The lanes of the knobs converted: their unit and every point.
    let mut data = vec![];
    for (lane, knob) in &lanes {
        let Some(text) = text_of(lane) else { continue };
        match convert_lane(&text, *knob) {
            Ok(Some((after, n))) => {
                let (from, to) = knob.unit();
                let lines = vec![format!("unit {from} → {to}, {n} point{} converted", plural(n))];
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
    // music2's own packs move to the versions it comes with: an older one the store keeps may be
    // made for an older @m2/core, which this app can't run (PACK-22).
    let kept = locked.packs.len();
    locked.packs.retain(|l| pack::comes_with(&l.name).is_none_or(|v| v == l.version));
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
    out.extend(code);
    out.extend(data);
    Ok(Plan { files: out, left })
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

/// How a knob of a drum One drum plays now reads there (wave 3, CARD-33).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Knob {
    /// A tune or tone in Hz: semitones from the drum's own pitch, 12 × log2(Hz ÷ own).
    Tune(f64),
    /// A decay in seconds: times the drum's own length, seconds ÷ own.
    Decay(f64),
    /// A kick's punch, a snare's snap, 0 to 1: One drum's punch, less the drum's own.
    Punch(f64),
}

impl Knob {
    /// `x`, in the old knob's base unit, as One drum's knob reads it: to three decimals (0.06
    /// cents of tune, a thousandth of a length or of a punch).
    fn convert(self, x: f64) -> f64 {
        let v = match self {
            Knob::Tune(own) => 12.0 * (x / own).log2(),
            Knob::Decay(own) => x / own,
            Knob::Punch(own) => x - own,
        };
        (v * 1000.0).round() / 1000.0
    }

    /// The unit a lane of it is in, before and after.
    fn unit(self) -> (&'static str, &'static str) {
        match self {
            Knob::Tune(_) => ("hz", "semitones"),
            Knob::Decay(_) => ("seconds", "scalar"),
            Knob::Punch(_) => ("unit01", "bipolar"),
        }
    }

    /// The unit constructors of @m2/core a value of it may be written with, and their scale.
    fn spellings(self) -> &'static [(&'static str, f64)] {
        match self {
            Knob::Tune(_) => &[("hz", 1.0), ("khz", 1000.0)],
            Knob::Decay(_) => &[("secs", 1.0), ("ms", 0.001)],
            Knob::Punch(_) => &[("frac", 1.0)],
        }
    }

    /// How to write it by hand.
    fn how(self, old: &str, new: &str) -> String {
        match self {
            Knob::Tune(own) => format!("{new}: semitones from its own {own} Hz, 12 × log2(Hz ÷ {own})"),
            Knob::Decay(own) => format!("{new}: times its own {own} s, seconds ÷ {own}"),
            Knob::Punch(own) => format!("{new}: the {old} less {own}"),
        }
    }
}

/// A drum One drum plays now: the single (or simple) one's name, One drum's name for it, and
/// how each of its knobs reads: (old key, One drum's key, how; None: the same knob).
struct Drum {
    old: &'static str,
    drum: &'static str,
    knobs: &'static [(&'static str, &'static str, Option<Knob>)],
}

/// `drums.kick` … `drums.shaker` of `@m2/drums` 1 (and `std/drums` before it), as One drum plays
/// them: at their own pitch and length, so 0, 1 and 0 are the single's sound (CARD-33).
const SINGLES: &[Drum] = &[
    Drum {
        old: "kick",
        drum: "kick",
        knobs: &[
            ("tune", "tune", Some(Knob::Tune(48.0))),
            ("decay", "decay", Some(Knob::Decay(0.45))),
            ("punch", "punch", Some(Knob::Punch(0.5))),
            ("drive", "drive", None),
        ],
    },
    Drum {
        old: "snare",
        drum: "snare",
        knobs: &[
            ("tune", "tune", Some(Knob::Tune(190.0))),
            ("snap", "punch", Some(Knob::Punch(0.6))),
            ("decay", "decay", Some(Knob::Decay(0.18))),
        ],
    },
    Drum { old: "clap", drum: "clap", knobs: &[("tone", "tune", Some(Knob::Tune(1200.0))), ("decay", "decay", Some(Knob::Decay(0.25)))] },
    Drum {
        old: "hats",
        drum: "hats",
        knobs: &[
            ("decay", "decay", Some(Knob::Decay(0.045))),
            ("openDecay", "decay", Some(Knob::Decay(0.45))),
            ("tone", "tune", Some(Knob::Tune(8000.0))),
        ],
    },
    Drum { old: "tom", drum: "tom", knobs: &[("tune", "tune", Some(Knob::Tune(130.0))), ("decay", "decay", Some(Knob::Decay(0.35)))] },
    Drum { old: "rim", drum: "rim", knobs: &[("tone", "tune", Some(Knob::Tune(1700.0)))] },
    Drum { old: "cowbell", drum: "cowbell", knobs: &[("tune", "tune", Some(Knob::Tune(540.0)))] },
    Drum { old: "cymbal", drum: "cymbal", knobs: &[("decay", "decay", Some(Knob::Decay(1.6)))] },
    Drum { old: "shaker", drum: "shaker", knobs: &[("decay", "decay", Some(Knob::Decay(0.06)))] },
];

/// The simple `kick` and `hat` of `std/instruments` and `@m2/synths` 1: One drum's kick and hats,
/// their tune and decay kept as pitch and length.
const SIMPLE: &[Drum] = &[
    Drum { old: "kick", drum: "kick", knobs: &[("tune", "tune", Some(Knob::Tune(48.0))), ("decay", "decay", Some(Knob::Decay(0.45)))] },
    Drum { old: "hat", drum: "hats", knobs: &[("decay", "decay", Some(Knob::Decay(0.045)))] },
];

/// std's modules that moved into the packs, whose names meant what they did before wave 3.
const MOVED_FROM: &[&str] = &["std/instruments", "std/synth", "std/drums"];
/// The modules the simple kick and hat came from.
const SIMPLE_FROM: &[&str] = &["std/instruments", "@m2/synths"];
/// The modules `drums` came from, and is from.
const DRUMS_FROM: &[&str] = &["std/drums", "@m2/drums"];
/// The modules `synth` came from, and is from.
const SYNTH_FROM: &[&str] = &["std/synth", "@m2/synths"];
/// One drum's pack.
const DRUMS_PACK: &str = "@m2/drums";

// ---------------------------------------------------------------- code

/// A named import: what it imports, under which name, and its bytes.
#[derive(Clone, Debug)]
struct Spec {
    imported: String,
    local: String,
    span: (u32, u32),
}

/// A top-level import statement.
#[derive(Clone, Debug)]
struct Import {
    span: (u32, u32),
    source: String,
    /// The module's string, its quotes included.
    source_span: (u32, u32),
    specs: Vec<Spec>,
    /// `import type { … }`.
    typed: bool,
    /// A default or namespace import: `import * as fx from "std/fx"`.
    whole: bool,
}

fn imports(source: &str) -> Vec<Import> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let mut out = vec![];
    for statement in &ret.program.body {
        let Statement::ImportDeclaration(i) = statement else { continue };
        let mut specs = vec![];
        let mut whole = false;
        for s in i.specifiers.iter().flatten() {
            match s {
                ImportDeclarationSpecifier::ImportSpecifier(s) => specs.push(Spec {
                    imported: s.imported.name().to_string(),
                    local: s.local.name.to_string(),
                    span: (s.span.start, s.span.end),
                }),
                _ => whole = true,
            }
        }
        out.push(Import {
            span: (i.span.start, i.span.end),
            source: i.source.value.as_str().unwrap_or_default().to_string(),
            source_span: (i.source.span.start, i.source.span.end),
            specs,
            typed: i.import_kind.is_type(),
            whole,
        });
    }
    out
}

/// The packs `source` imports from: `@m2/drums`, and `@alice/x` for `@alice/x/keys`.
fn imported_packs(source: &str) -> Vec<PackName> {
    imports(source)
        .iter()
        .filter_map(|i| {
            let mut parts = i.source.splitn(3, '/');
            let (owner, name) = (parts.next()?, parts.next()?);
            owner.starts_with('@').then(|| PackName::new(&format!("{owner}/{name}")).ok()).flatten()
        })
        .collect()
}

/// A knob's value as written.
enum Value {
    /// A number, in the knob's base unit.
    Number(f64),
    /// A lane drives it: `lane.control("<key>", "<unit>", <fallback>)`.
    Lane { site: Site, key: String, unit: (String, (u32, u32)), fallback: Option<(u32, u32)> },
}

/// An edit: bytes `start..end` of the file become the text.
type Edit = (u32, u32, String);

/// One code file as the upgrade reads it, and what it makes of it.
struct Code<'t> {
    file: String,
    text: &'t str,
    imports: Vec<Import>,
    sites: Vec<Site>,
    /// What the file's names stand for: local name to (module, the name it imports).
    locals: BTreeMap<String, (String, String)>,
    edits: Vec<Edit>,
    /// Where code is replaced: a name read only inside these is read no more.
    replaced: Vec<(u32, u32)>,
    /// The starts of the calls handled (converted or left), so no other check repeats them.
    handled: BTreeSet<u32>,
    left: Vec<Left>,
    /// The packs its rewritten code plays.
    packs: BTreeSet<PackName>,
    /// Each lane file a converted knob reads, how, and where it is read.
    lanes: Vec<(String, Knob, Left)>,
    /// What One drum's `drums` is called here, and whether its import must be added; or why it
    /// can't be.
    drums: Result<(String, bool), String>,
    /// The modules whose names mean what they did before wave 3 (`plan`'s `old`).
    old: &'t BTreeSet<String>,
}

impl<'t> Code<'t> {
    fn read(file: &str, text: &'t str, old: &'t BTreeSet<String>) -> Code<'t> {
        let imports = imports(text);
        let mut locals = BTreeMap::new();
        for i in &imports {
            for s in &i.specs {
                locals.insert(s.local.clone(), (i.source.clone(), s.imported.clone()));
            }
        }
        let (sites, left) = match m2_script::sites::instrument(file, text, 0) {
            Ok(i) => (i.sites, vec![]),
            Err(e) => (
                vec![],
                vec![Left { file: file.into(), line: 1, what: format!("it doesn't read as TypeScript: {e}"), write: String::new() }],
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
            imports,
            sites,
            locals,
            edits: vec![],
            replaced: vec![],
            handled: BTreeSet::new(),
            left,
            packs: BTreeSet::new(),
            lanes: vec![],
            drums,
            old,
        }
    }

    /// Whether local name `name` is `what`, imported from one of `from` as it was before wave 3.
    fn was(&self, name: &str, from: &[&str], what: &str) -> bool {
        self.locals.get(name).is_some_and(|(m, n)| from.contains(&m.as_str()) && self.old.contains(m) && n == what)
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
                None if self.was(object, SIMPLE_FROM, "kick") || self.was(object, SIMPLE_FROM, "hat") => {
                    let name = self.locals[object].1.clone();
                    let d = SIMPLE.iter().find(|d| d.old == name).expect("kick or hat");
                    self.drum(site, &sites, d, false);
                }
                Some(m) if self.was(object, DRUMS_FROM, "drums") && SINGLES.iter().any(|d| d.old == m) => {
                    let d = SINGLES.iter().find(|d| d.old == m).expect("a single");
                    self.drum(site, &sites, d, true);
                }
                Some("bass") if self.was(object, SYNTH_FROM, "synth") => self.bass(site),
                None if self.core(object) == Some("kernel") => self.kernel(site),
                _ => {}
            }
        }
        self.sites = sites;
        self.others();
        self.rewrite_imports();
    }

    /// `kick(x)`, `drums.snare(x, { snap: 0.75 })` as One drum: `drums.drum(x, { drum: "snare",
    /// punch: 0.15 })`. All or nothing: a call with a value it can't convert is left as it is.
    fn drum(&mut self, site: &Site, sites: &[Site], d: &Drum, single: bool) {
        self.handled.insert(site.start);
        let notes = site.args.first().map(|a| self.slice(a.span()).to_string()).unwrap_or_default();
        let (mut edits, mut replaced, mut lanes, mut why) = (vec![], vec![], vec![], vec![]);
        // Each of One drum's knobs, as written, and the old knob it came from.
        let mut written: BTreeMap<&str, (String, String)> = BTreeMap::new();
        // The options to write, for a call left as it is.
        let mut suggested: Vec<String> = vec![];
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
            let write = format!("{drums}.drum({notes}, {{ {first}… }}), its knobs as One drum's: {}", hows.join("; "));
            return self.leave(site.start, format!("`{}`: {}", site.callee.trim(), why.join("; ")), write);
        };
        for p in &props {
            let value = self.slice(p.value);
            let Some((_, new, knob)) = d.knobs.iter().find(|(old, ..)| *old == p.key.as_str()) else {
                if p.key == "voices" {
                    suggested.push(format!("voices: {value}"));
                } else {
                    why.push(format!("One drum has no `{}`", p.key));
                }
                continue;
            };
            let Some(knob) = knob else {
                suggested.push(format!("{new}: {value}"));
                continue;
            };
            let converted = match self.value(p.value, sites, *knob) {
                Ok(Value::Number(x)) if knob.convert(x).is_finite() => {
                    let text = number(knob.convert(x));
                    edits.push((p.value.0, p.value.1, text.clone()));
                    replaced.push(p.value);
                    Some(text)
                }
                Ok(Value::Number(_)) => {
                    why.push(format!("its `{}` of {value} has no value on One drum", p.key));
                    None
                }
                Ok(Value::Lane { site: call, key, unit, fallback }) => match self.lane(&call, sites, *knob, &key, unit, fallback) {
                    Ok((e, r, l)) => {
                        edits.extend(e);
                        replaced.extend(r);
                        lanes.push(l);
                        Some(format!("lane `{key}`"))
                    }
                    Err(e) => {
                        why.push(e);
                        None
                    }
                },
                Err(()) => {
                    why.push(format!("its `{}` is worked out by code (`{value}`)", p.key));
                    None
                }
            };
            if let (Some(c), Some((other, by))) = (&converted, written.get(new))
                && c != other
            {
                why.push(format!("its `{by}` and `{}` are both One drum's `{new}` now, and differ", p.key));
            }
            suggested.push(match &converted {
                Some(c) if c.starts_with("lane ") => format!("{new}: {}", self.slice(p.value)),
                Some(c) => format!("{new}: {c}"),
                None => format!("<{}>", knob.how(&p.key, new)),
            });
            if let Some(c) = converted {
                written.insert(new, (c, p.key.clone()));
            }
            if *new != p.key {
                // The key, as written: plain or quoted.
                let at = p.start as usize;
                let rest = &self.text[at..];
                let key = [p.key.clone(), format!("\"{}\"", p.key), format!("'{}'", p.key)].into_iter().find(|k| {
                    rest.starts_with(k.as_str()) && !rest[k.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$')
                });
                match key {
                    Some(k) if k.starts_with(['"', '\'']) => {
                        edits.push((p.start, p.start + k.len() as u32, format!("{}{new}{}", &k[..1], &k[..1])))
                    }
                    Some(k) => edits.push((p.start, p.start + k.len() as u32, new.to_string())),
                    None => why.push(format!("its `{}` is written so its name can't be changed", p.key)),
                }
            }
        }
        if !why.is_empty() {
            let all: Vec<String> = drum.iter().cloned().chain(suggested).collect();
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
        }
        if let Some(drum) = drum {
            match (opts, props.first()) {
                (Some(_), Some(first)) => edits.push((first.start, first.start, format!("{drum}, "))),
                (Some(o), None) => {
                    let (a, b) = o.span();
                    edits.push((a, b, format!("{{ {drum} }}")));
                }
                (None, _) => {
                    let end = site.args.first().map_or(site.open + 1, |a| a.span().1);
                    edits.push((end, end, format!(", {{ {drum} }}")));
                }
            }
        }
        self.edits.extend(edits);
        self.replaced.extend(replaced);
        self.lanes.extend(lanes);
        self.packs.insert(PackName::new(DRUMS_PACK).expect("a pack's name"));
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
        let now = knob.convert(was.map_or(0.0, |(_, x)| x));
        if !now.is_finite() {
            return Err(format!("its lane `{key}` has no fallback One drum can read"));
        }
        match was {
            Some((span, _)) => {
                edits.push((span.0, span.1, number(now)));
                replaced.push(span);
            }
            None => edits.push((unit_at.1, unit_at.1, format!(", {}", number(now)))),
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
        let say = "its `decay` is `biteDecay` now: `decay` is the envelope's".to_string();
        match site.args.get(1) {
            None => {}
            Some(Arg::Object { start, end, .. }) => {
                let Some(props) = properties(self.text, (*start, *end)) else {
                    return self.leave(site.start, "`synth.bass`'s options are worked out by code".into(), say);
                };
                if let Some(p) = props.iter().find(|p| p.key == "decay") {
                    let rest = &self.text[p.start as usize..];
                    if rest.starts_with("decay") && !rest[5..].starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '$') {
                        self.edits.push((p.start, p.start + 5, "biteDecay".into()));
                    } else {
                        self.leave(site.start, "`synth.bass`'s `decay` is written so its name can't be changed".into(), say);
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
            let simple = self.was(local, SIMPLE_FROM, name) && SIMPLE.iter().any(|d| d.old == name);
            let drums = self.was(local, DRUMS_FROM, "drums");
            let synth = self.was(local, SYNTH_FROM, "synth");
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
                let what = match member {
                    _ if simple => Some(format!("`{local}` is used here, not called: One drum plays the {name} now")),
                    Some(m) if drums && SINGLES.iter().any(|d| d.old == m) => {
                        Some(format!("`{local}.{m}` is used here, not called: One drum plays the {m} now"))
                    }
                    Some("bass") if synth => Some(format!("`{local}.bass` is used here, not called: its `decay` is `biteDecay` now")),
                    _ => None,
                };
                if let Some(what) = what {
                    found.push((at, what));
                }
            }
        }
        for (at, what) in found {
            self.leave(at, what, "drums.drum(notes, { drum: \"…\" }), with its knobs as One drum's".into());
        }
    }

    /// The imports, as the code now reads them: a moved name from where it is now, a name nothing
    /// reads any more gone, and One drum's `drums` where the simple kick or hat was.
    fn rewrite_imports(&mut self) {
        let simple_gone =
            self.locals.iter().any(|(l, (m, n))| SIMPLE_FROM.contains(&m.as_str()) && (n == "kick" || n == "hat") && self.unread(l));
        let mut add = match &self.drums {
            Ok((local, true)) if simple_gone => Some(local.clone()),
            _ => None,
        };
        let imports = std::mem::take(&mut self.imports);
        for i in &imports {
            let from_moved = m2_script::MOVED.iter().any(|(m, ..)| i.source == format!("std/{m}"));
            if i.whole {
                if from_moved {
                    self.leave(
                        i.span.0,
                        format!("`{}` is imported whole, and what it held moved", i.source),
                        "import each name from where it is now".into(),
                    );
                }
                continue;
            }
            // Each module the statement's names come from now, its own first, then in order, with
            // their names.
            let mut groups: Vec<(String, Vec<String>)> = vec![(i.source.clone(), vec![])];
            let (mut dropped, mut dropped_simple) = (false, false);
            for s in &i.specs {
                if self.unread(&s.local) {
                    dropped = true;
                    dropped_simple |= SIMPLE_FROM.contains(&i.source.as_str()) && (s.imported == "kick" || s.imported == "hat");
                    continue;
                }
                let to = moved_import(&i.source, &s.imported).map_or_else(|| i.source.clone(), str::to_string);
                let text = self.slice(s.span).to_string();
                match groups.iter_mut().find(|(m, _)| *m == to) {
                    Some((_, names)) => names.push(text),
                    None => groups.push((to, vec![text])),
                }
            }
            let added = dropped_simple && add.as_ref().is_some();
            if let Some(local) = add.take_if(|_| dropped_simple) {
                match groups.iter_mut().find(|(m, _)| m == DRUMS_PACK) {
                    Some((_, names)) => names.push(local),
                    None => groups.push((DRUMS_PACK.into(), vec![local])),
                }
            }
            groups.retain(|(_, names)| !names.is_empty());
            if !dropped && !added && groups.iter().all(|(m, _)| *m == i.source) {
                continue;
            }
            let quote = &self.slice(i.source_span)[..1];
            if groups.is_empty() {
                let (a, b) = whole_lines(self.text, i.span.0, i.span.1);
                self.edits.push((a, b, String::new()));
            } else if !dropped && !added && groups.len() == 1 {
                // Every name kept, from one module: only the module changes.
                self.edits.push((i.source_span.0, i.source_span.1, format!("{quote}{}{quote}", groups[0].0)));
            } else {
                let kind = if i.typed { "import type" } else { "import" };
                let lines: Vec<String> =
                    groups.iter().map(|(m, names)| format!("{kind} {{ {} }} from {quote}{m}{quote};", names.join(", "))).collect();
                self.edits.push((i.span.0, i.span.1, lines.join("\n")));
            }
        }
        self.imports = imports;
    }

    /// Whether nothing reads local `name` any more: every place it was read is replaced.
    fn unread(&self, name: &str) -> bool {
        let refs = references(self.text, name);
        !refs.is_empty() && refs.iter().all(|r| self.replaced.iter().any(|(a, b)| a <= r && r < b))
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

/// A number as the upgrade writes it: its shortest form (`0.707`, `2`, `-0.2`), never `-0`.
fn number(x: f64) -> String {
    m2_song::lane::number(x)
}

/// `text` with `edits` made, each `(start, end, new)` in its bytes; they never overlap.
fn apply(text: &str, edits: &[Edit]) -> String {
    let mut sorted: Vec<&Edit> = edits.iter().collect();
    sorted.sort_by_key(|(a, b, _)| (*a, *b));
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    for (a, b, new) in sorted {
        let (a, b) = (*a as usize, *b as usize);
        debug_assert!(a >= at, "the upgrade's edits overlap at byte {a}");
        out.push_str(&text[at..a.max(at)]);
        out.push_str(new);
        at = b.max(at);
    }
    out.push_str(&text[at..]);
    out
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
        let after = apply(before, &local);
        let mut first = Some(line.as_str());
        let width = line.len();
        for (sign, l) in before.lines().map(|l| ('-', l)).chain(after.lines().map(|l| ('+', l))) {
            out.push(format!("{:>width$} {sign} {l}", first.take().unwrap_or("")));
        }
    }
    out
}

// ---------------------------------------------------------------- data

/// A control lane's file with every point converted by `knob`, in One drum's unit: the new text
/// and how many points; None when it is in that unit already.
fn convert_lane(text: &str, knob: Knob) -> Result<Option<(String, usize)>, String> {
    let (from, to) = knob.unit();
    let data = LaneData::read(text, &LaneKind::Control { unit: None }).map_err(|e| format!("it doesn't read as a lane: {e:?}"))?;
    let LaneData::Control { unit, mut clips } = data else { return Err("it isn't a control lane".into()) };
    match unit.as_deref() {
        Some(u) if u == to => return Ok(None),
        Some(u) if u != from => return Err(format!("it is in {u}, not {from}")),
        _ => {}
    }
    let mut n = 0;
    for c in &mut clips {
        for (_, v) in &mut c.points {
            *v = knob.convert(*v);
            if !v.is_finite() {
                return Err(format!("a point of it has no value in {to}"));
            }
            n += 1;
        }
    }
    Ok(Some((m2_song::lane::canonical(&LaneData::Control { unit: Some(to.into()), clips }), n)))
}

// ---------------------------------------------------------------- the manifest

/// `[dependencies]` as this app's: `@m2/core`, std and each of music2's own packs whose line asks
/// for an older version than this app has get the line for it, and each of music2's own packs the
/// code imports now that isn't listed gets its line. The text after, and the edits.
fn dependencies(text: &str, m: &Manifest, imported: &BTreeSet<PackName>, app: &pack::App) -> Result<(String, Vec<Edit>), EditError> {
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
        lines.push_str(if at.is_some() { line.trim_start_matches('\n') } else { &line });
        at = Some(place);
    }
    if let Some(at) = at {
        edits.push((at, at, lines));
    }
    Ok((apply(text, &edits), edits))
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
                let start = offset + key_len(line) + (value.len() - v.len());
                let end = v.strip_prefix('"')?.find('"')?;
                return Some(((start + 1) as u32, (start + 1 + end) as u32));
            }
        }
        offset += line.len();
    }
    None
}

/// The bytes of a line up to and including its first `=`.
fn key_len(line: &str) -> usize {
    line.find('=').map_or(0, |i| i + 1)
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;

//! The Tx (WRITE-3): a verified edit of several files, as one JSON shape for the server, `m2 tx`,
//! agents and later TypeScript tools.
//!
//! A Tx names every file it reads or writes with the content it was planned on (its base: a
//! content hash, the base text itself, or absent), the ops, and what it may change (its
//! expectation, WRITE-6). Every position in it is in its file's base text. The session checks
//! and commits it; nothing here reads the disk.
//!
//! Every path is validated as WRITE-3a says before anything else: relative, `/`-separated, no
//! empty, `.` or `..` segment, and parsed by [`DataPath`] to a class a Tx may write. Parsing is
//! strict, as FMT-8 asks of data files: an unknown field is refused, never dropped.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

use crate::diag::Diagnostic;
use crate::hash::Sha;
use crate::key::{ClipId, Key, LaneAddr};
use crate::path::{Class, DataPath, LOCAL, slashed};

/// What a delta's hash is: SHA-256 of its JSON in JCS form (WRITE-8).
pub type DeltaHash = Sha;

/// A verified edit (WRITE-3). One Tx is one journal entry and one undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Tx {
    /// Metadata, `^[0-9a-z-]{1,32}$` (WRITE-3a); core names the journal itself.
    pub id: String,
    /// Exactly one base per file an op touches.
    pub base: BTreeMap<TxPath, Base>,
    /// What the change said before and says after, for history: made by the builder, never by core (P10).
    pub label: (String, String),
    pub ops: Vec<Op>,
    pub expect: Expect,
    /// A code revision, only a hint (WRITE-12).
    pub hint: Option<u64>,
    /// The app's planner says it writes values where they are written and adds no call (a knob, a
    /// menu, a switch): it moves no ids (AGENT-2). Never read from JSON: a Tx from outside the app
    /// moves the ids of every file it touches, but a lane's data.
    pub in_place: bool,
}

/// A file's content as a Tx was planned on (WRITE-12).
#[derive(Clone, Debug, PartialEq)]
pub enum Base {
    /// The SHA-256 of its bytes; for a `data/<module>/` folder, its directory hash.
    Sha(Sha),
    /// It doesn't exist: a file the Tx creates, or a `Rename`'s `to`.
    Absent,
    /// The base text itself, whose hash core checks: the same outcome in every process.
    WithText { sha: Sha, text: Arc<str> },
}

impl Base {
    pub fn sha(&self) -> Option<Sha> {
        match self {
            Base::Sha(s) | Base::WithText { sha: s, .. } => Some(*s),
            Base::Absent => None,
        }
    }

    /// The base of a text file with `text`, carrying the text.
    pub fn text(text: &str) -> Base {
        Base::WithText { sha: Sha::of(text.as_bytes()), text: text.into() }
    }

    /// [`Base::text`] of a text a snapshot shares: neither copied nor hashed again.
    pub fn shared(text: &Arc<str>) -> Base {
        Base::WithText { sha: Sha::of_shared(text), text: text.clone() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Replace byte ranges of the base text; `expected` guards each.
    TextEdit {
        file: TxPath,
        edits: Vec<Edit>,
    },
    /// The whole file.
    Write {
        file: TxPath,
        bytes: Payload,
    },
    Remove {
        file: TxPath,
    },
    /// A file, or a folder `data/<module>/`.
    Rename {
        from: TxPath,
        to: TxPath,
    },
    /// Whole clips put (each replacing the clip with its id, or added) and clips deleted by id:
    /// the only data op (WRITE-3). At most one per lane.
    Clips {
        lane: LaneAddr,
        put: Vec<Value>,
        delete: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub start: Pos,
    pub end: Pos,
    /// What the base text says at `start..end`: a mismatch rejects the Tx.
    pub expected: String,
    pub text: String,
}

/// A position in a base text: a UTF-8 byte offset, or a line and a UTF-16 column, both counted
/// from 0 as LSP counts them (the step 6 design's C5), which core resolves against the base text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pos {
    Byte(u32),
    LineCol { line: u32, utf16_col: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

impl Range {
    pub fn bytes(start: u32, end: u32) -> Range {
        Range { start: Pos::Byte(start), end: Pos::Byte(end) }
    }
}

/// A `Write`'s bytes.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Text(Arc<str>),
    Bytes(Arc<[u8]>),
    /// A file under `.music2/tx/` with this content: a staged write or a stored before-image,
    /// which only recovery fixes and the undo of a binary write carry (the step 6 design's C3).
    /// Core checks the hash before it uses one.
    Local {
        path: String,
        sha: Sha,
    },
}

/// What a Tx may change (WRITE-6): a closed set.
#[derive(Clone, Debug, PartialEq)]
pub enum Expect {
    /// An empty delta.
    Identical,
    /// Only these constants change, to these values.
    Values(Vec<(Anchor, f64)>),
    Confined(Confined),
    /// Commit only if the delta's hash is the one `check` returned (WRITE-11).
    Seen(DeltaHash),
    /// The Tx restores exactly what journal `id` recorded, checked against the journal and never
    /// the graph: only the FinishTx and DiscardTx fixes carry it (C3).
    Journal(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Confined {
    pub ranges: BTreeMap<TxPath, Declared>,
    pub lanes: Vec<LaneAddr>,
    pub modules: Vec<ModuleChange>,
}

/// What a file may change: the whole of it, or these ranges of its base text (none: nothing).
#[derive(Clone, Debug, PartialEq)]
pub enum Declared {
    Whole,
    Ranges(Vec<Range>),
}

/// A value in the code (WRITE-6, WRITE-7): structured, never a `file#ordinal` string.
#[derive(Clone, Debug, PartialEq)]
pub enum Anchor {
    /// A number written in the code, by its bytes.
    Written { file: TxPath, range: Range },
    /// A declared control a call leaves out, named by its call and its slot (PKG-3).
    Slot { site: Spot, slot: String },
}

/// A call site, by its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub file: TxPath,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleChange {
    Added(Key),
    Removed(Key),
    Rekeyed {
        from: Key,
        to: Key,
    },
    /// "Mute in song" (MON-5): in the type now, inert until `Module.muted` exists.
    Muted {
        module: Key,
        muted: bool,
    },
}

/// A path WRITE-3a admits, with its parse: one that [`DataPath`] spells back exactly and
/// classes as something a Tx may write.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TxPath {
    text: String,
    path: DataPath,
}

/// Characters no path may hold: controls, and the invisible ones that make a name read as
/// another (bidi overrides, zero-width marks). Nor may an agent's words the app shows (AGENT-10).
pub fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}')
        || matches!(c, '\u{2066}'..='\u{2069}' | '\u{feff}')
}

impl TxPath {
    /// Why `s` can't be a Tx's path (`tx.path`), or the path.
    pub fn new(s: &str) -> Result<TxPath, String> {
        let why = if s.is_empty() {
            Some("it is empty")
        } else if s.starts_with('/') {
            Some("it isn't relative to the project")
        } else if s.contains('\\') {
            Some("paths are `/`-separated")
        } else if s.chars().any(hidden) {
            Some("it holds a control or an invisible character")
        } else if s.split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..") {
            Some("a segment of it is empty, `.` or `..`")
        } else {
            None
        };
        if let Some(why) = why {
            return Err(format!("`{}` can't be named by a Tx: {why}", s.escape_debug()));
        }
        let path = DataPath::parse(Path::new(s));
        if slashed(&path.path()) != s {
            return Err(format!("`{s}` can't be named by a Tx: it isn't spelled as the project spells it"));
        }
        match path {
            DataPath::Local(_) => Err(format!("`{s}` can't be named by a Tx: {LOCAL}/ is this machine's, never the song's")),
            DataPath::Unknown(_) => Err(format!(
                "`{s}` can't be named by a Tx: it isn't a file music2 reads (code, the manifest, the arrangement, a lane, a plugin \
                 state, an asset or layout.json)"
            )),
            path => Ok(TxPath { text: s.to_string(), path }),
        }
    }

    /// The path of `d`, which must be one a Tx may name.
    pub fn of(d: &DataPath) -> Result<TxPath, String> {
        TxPath::new(&d.to_string())
    }

    /// The path of lane `addr`'s file.
    pub fn lane(addr: &LaneAddr) -> TxPath {
        let path = DataPath::lane(addr);
        TxPath { text: path.to_string(), path }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn data(&self) -> &DataPath {
        &self.path
    }

    /// Relative to the project.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.text)
    }

    pub fn class(&self) -> Class {
        self.path.class()
    }

    /// Whether its content is text a snapshot holds (the manifest, code, the arrangement, a lane,
    /// a state sidecar) or `layout.json`; otherwise it is a binary (a state blob, an asset) or a
    /// folder.
    pub fn is_text(&self) -> bool {
        matches!(
            self.path,
            DataPath::Manifest
                | DataPath::Lock
                | DataPath::Code(_)
                | DataPath::Arrangement
                | DataPath::Lane { .. }
                | DataPath::PluginState { sidecar: true, .. }
                | DataPath::Presentation
        )
    }

    /// Whether it is `p` or a file or folder inside it.
    pub fn within(&self, p: &TxPath) -> bool {
        Path::new(&self.text).starts_with(&p.text)
    }
}

impl fmt::Display for TxPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl Op {
    /// The paths the op touches: what its bases are for, and what may not overlap (1.3).
    pub fn paths(&self) -> Vec<TxPath> {
        match self {
            Op::TextEdit { file, .. } | Op::Write { file, .. } | Op::Remove { file } => vec![file.clone()],
            Op::Rename { from, to } => vec![from.clone(), to.clone()],
            Op::Clips { lane, .. } => vec![TxPath::lane(lane)],
        }
    }
}

/// Whether `id` is a Tx id: `^[0-9a-z-]{1,32}$` (WRITE-3a).
pub fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `id` names a journal as core names them: `<unix ms>-<pid>-<n>`.
pub fn valid_journal(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.len() == 3 && parts.iter().all(|p| (1..=20).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `p` is a file `Payload::Local` may name: inside `.music2/tx/`, plainly spelled.
pub fn valid_local(p: &str) -> bool {
    let rest = p.strip_prefix(".music2/tx/");
    rest.is_some_and(|r| {
        !r.is_empty() && !p.chars().any(hidden) && !p.contains('\\') && r.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
    }) && Path::new(p).components().all(|c| matches!(c, Component::Normal(_)))
}

/// A Tx id derived from `parts`: `<prefix>-` and the first 12 Crockford characters of the
/// SHA-256 of the parts joined by NUL bytes, so the same inputs always name the same Tx (a fix's,
/// R19).
pub fn derived_id(prefix: &str, parts: &[String]) -> String {
    let digest = crate::Sha::of(parts.join("\0").as_bytes());
    format!("{prefix}-{}", crate::key::crockford(&hex_bytes(&digest.hex()), 12))
}

/// A fresh Tx id for a builder outside a sandbox: `tx-` and 12 random Crockford characters
/// (FMT-11's only random input).
pub fn random_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    format!("tx-{}", crate::key::crockford(&h.finish().to_be_bytes(), 12))
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect()
}

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("tx.invalid", message)
}

impl Tx {
    /// A Tx with no bases yet.
    pub fn new(id: &str, label: (String, String), ops: Vec<Op>, expect: Expect) -> Tx {
        Tx { id: id.to_string(), base: BTreeMap::new(), label, ops, expect, hint: None, in_place: false }
    }

    /// The paths the ops touch, in op order, each once.
    pub fn touched(&self) -> Vec<TxPath> {
        let mut seen = BTreeSet::new();
        self.ops.iter().flat_map(Op::paths).filter(|p| seen.insert(p.clone())).collect()
    }

    /// The shape rules of WRITE-3a and WRITE-3 that need no disk (the step 6 design's 1.3): the id,
    /// one base per touched file and no stray base, renames between paths of one kind into an
    /// absent name, assets only created, clip ids, finite values, and no two op paths that
    /// overlap. Positions are checked against the base texts later.
    pub fn validate(&self) -> Result<(), Vec<Diagnostic>> {
        let mut out = vec![];
        if !valid_id(&self.id) {
            out.push(invalid(format!("`{}` isn't a Tx id: a Tx id is 1 to 32 of `a-z`, `0-9` and `-`", self.id.escape_debug())));
        }
        let paths: Vec<TxPath> = self.ops.iter().flat_map(Op::paths).collect();
        for p in &paths {
            if !self.base.contains_key(p) {
                out.push(invalid(format!("{p} has no base: every file an op reads or writes names the content it was planned on")));
            }
        }
        for p in self.base.keys() {
            if !paths.contains(p) {
                out.push(invalid(format!("{p} has a base but no op touches it")));
            }
        }
        // No two op paths overlap, so ops are independent and their order changes nothing (R15).
        for (i, p) in paths.iter().enumerate() {
            if let Some(q) = paths[..i].iter().find(|q| p.within(q) || q.within(p)) {
                out.push(invalid(format!("{q} and {p} overlap: a Tx touches each file or folder once")));
            }
        }
        let absent = |p: &TxPath| matches!(self.base.get(p), Some(Base::Absent));
        for op in &self.ops {
            for p in op.paths() {
                if matches!(p.data(), DataPath::Module { .. }) && !matches!(op, Op::Rename { .. }) {
                    out.push(invalid(format!("{p} is a folder: only a `rename` moves one")));
                }
            }
            match op {
                Op::TextEdit { file, edits } => {
                    if !file.is_text() {
                        out.push(invalid(format!("{file} isn't a text file, so a `text_edit` can't change it")));
                    }
                    if edits.is_empty() {
                        out.push(invalid(format!("the `text_edit` of {file} has no edits")));
                    }
                }
                Op::Write { file, bytes } => {
                    if matches!(file.data(), DataPath::Asset(_)) && !absent(file) {
                        out.push(invalid(format!("{file} is an asset, and assets are never overwritten: write a new name (FMT-14)")));
                    }
                    if let Payload::Local { path, .. } = bytes
                        && !valid_local(path)
                    {
                        out.push(invalid(format!("`{}` isn't a file under .music2/tx/", path.escape_debug())));
                    }
                }
                Op::Remove { .. } => {}
                Op::Rename { from, to } => {
                    let kind = |d: &DataPath| match d {
                        DataPath::Module { .. } => 1,
                        DataPath::Lane { .. } => 2,
                        DataPath::PluginState { sidecar: false, .. } => 3,
                        DataPath::PluginState { sidecar: true, .. } => 4,
                        DataPath::Code(_) => 5,
                        DataPath::Asset(_) => 6,
                        _ => 0,
                    };
                    if kind(from.data()) == 0 || kind(from.data()) != kind(to.data()) {
                        out.push(invalid(format!("{from} can't be renamed to {to}: a rename keeps a file or folder what it is")));
                    }
                    if !absent(to) {
                        out.push(invalid(format!("{to} must be absent: a rename never replaces anything")));
                    }
                }
                Op::Clips { lane, put, delete } => {
                    let mut ids = BTreeSet::new();
                    for (i, c) in put.iter().enumerate() {
                        match c.get("id").and_then(Value::as_str) {
                            Some(id) if ClipId::valid(id) => {
                                if !ids.insert(id.to_string()) {
                                    out.push(invalid(format!("the `clips` of {lane} puts clip `{id}` twice")));
                                }
                            }
                            _ => out.push(invalid(format!(
                                "clip {} that the `clips` of {lane} puts has no valid id: a builder chooses it (FMT-11)",
                                i + 1
                            ))),
                        }
                    }
                    for id in delete {
                        if !ClipId::valid(id) {
                            out.push(invalid(format!("`{}` isn't a clip id, so the `clips` of {lane} can't delete it", id.escape_debug())));
                        } else if ids.contains(id) {
                            out.push(invalid(format!("the `clips` of {lane} both puts and deletes clip `{id}`")));
                        }
                    }
                }
            }
        }
        match &self.expect {
            Expect::Values(values) => {
                for (anchor, v) in values {
                    if !v.is_finite() {
                        out.push(invalid(format!("{v} isn't a number a value can have")));
                    }
                    let file = match anchor {
                        Anchor::Written { file, .. } | Anchor::Slot { site: Spot { file, .. }, .. } => file,
                    };
                    if !self.base.contains_key(file) {
                        out.push(invalid(format!("a value is anchored in {file}, which the Tx doesn't touch")));
                    }
                }
            }
            Expect::Confined(c) => {
                for (file, declared) in &c.ranges {
                    if !self.base.contains_key(file) {
                        out.push(invalid(format!("ranges are declared in {file}, which the Tx doesn't touch")));
                    }
                    let whole_op = self.ops.iter().any(|op| match op {
                        Op::Write { file: f, .. } | Op::Remove { file: f } => f == file,
                        Op::Rename { from, to } => from == file || to == file,
                        _ => false,
                    });
                    if whole_op && matches!(declared, Declared::Ranges(r) if r.is_empty()) {
                        out.push(invalid(format!("{file} is written whole, but declares no bytes it may change: declare it \"whole\"")));
                    }
                }
                // A whole-file op on code or the manifest says so (R23).
                for op in &self.ops {
                    let files = match op {
                        Op::Write { file, .. } | Op::Remove { file } => vec![file],
                        Op::Rename { from, to } => vec![from, to],
                        _ => vec![],
                    };
                    for f in files {
                        if matches!(f.class(), Class::Code | Class::Manifest) && !matches!(c.ranges.get(f), Some(Declared::Whole)) {
                            out.push(invalid(format!("{f} is written whole, so a confined Tx declares it \"whole\"")));
                        }
                    }
                }
            }
            Expect::Journal(id) if !valid_journal(id) => out.push(invalid(format!("`{}` isn't a journal's name", id.escape_debug()))),
            _ => {}
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }

    /// The SHA-256 of the Tx's JSON in JCS form: what the check cache keys it by.
    pub fn hash(&self) -> Sha {
        crate::jcs::hash(&self.to_json())
    }

    /// The Tx's JSON (1.2 of the step 6 design).
    pub fn to_json(&self) -> Value {
        let mut base = Map::new();
        for (p, b) in &self.base {
            base.insert(
                p.to_string(),
                match b {
                    Base::Sha(s) => json!(s.prefixed()),
                    Base::Absent => Value::Null,
                    Base::WithText { sha, text } => json!({"sha": sha.prefixed(), "text": &**text}),
                },
            );
        }
        let mut out = json!({
            "id": self.id,
            "base": base,
            "label": [self.label.0, self.label.1],
            "ops": self.ops.iter().map(op_json).collect::<Vec<_>>(),
            "expect": expect_json(&self.expect),
        });
        if let Some(h) = self.hint {
            out["hint"] = json!(h);
        }
        out
    }

    /// A Tx from its JSON: strict, every path checked (`tx.path`), anything else malformed
    /// `tx.invalid`. The shape rules are [`Tx::validate`]'s.
    pub fn from_json(v: &Value) -> Result<Tx, Vec<Diagnostic>> {
        parse(v).map_err(|(code, message)| vec![Diagnostic::error(code, message)])
    }
}

impl Serialize for Tx {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Tx {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Tx, D::Error> {
        let v = Value::deserialize(d)?;
        Tx::from_json(&v).map_err(|e| serde::de::Error::custom(e.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; ")))
    }
}

pub fn pos_json(p: &Pos) -> Value {
    match p {
        Pos::Byte(b) => json!(b),
        Pos::LineCol { line, utf16_col } => json!({"line": line, "utf16_col": utf16_col}),
    }
}

fn range_json(r: &Range) -> Value {
    json!([pos_json(&r.start), pos_json(&r.end)])
}

fn op_json(op: &Op) -> Value {
    match op {
        Op::TextEdit { file, edits } => json!({
            "op": "text_edit", "file": file.as_str(),
            "edits": edits
                .iter()
                .map(|e| json!({"start": pos_json(&e.start), "end": pos_json(&e.end), "expected": e.expected, "text": e.text}))
                .collect::<Vec<_>>(),
        }),
        Op::Write { file, bytes } => match bytes {
            Payload::Text(t) => json!({"op": "write", "file": file.as_str(), "text": &**t}),
            Payload::Bytes(b) => json!({"op": "write", "file": file.as_str(), "base64": base64(b)}),
            Payload::Local { path, sha } => json!({"op": "write", "file": file.as_str(), "local": {"path": path, "sha": sha.prefixed()}}),
        },
        Op::Remove { file } => json!({"op": "remove", "file": file.as_str()}),
        Op::Rename { from, to } => json!({"op": "rename", "from": from.as_str(), "to": to.as_str()}),
        Op::Clips { lane, put, delete } => {
            json!({"op": "clips", "lane": [lane.module.as_str(), lane.lane.as_str()], "put": put, "delete": delete})
        }
    }
}

fn anchor_json(a: &Anchor) -> Value {
    match a {
        Anchor::Written { file, range } => json!({"written": {"file": file.as_str(), "range": range_json(range)}}),
        Anchor::Slot { site, slot } => {
            json!({"slot": {"site": {"file": site.file.as_str(), "range": range_json(&site.range)}, "slot": slot}})
        }
    }
}

pub fn module_json(m: &ModuleChange) -> Value {
    match m {
        ModuleChange::Added(k) => json!({"added": k.as_str()}),
        ModuleChange::Removed(k) => json!({"removed": k.as_str()}),
        ModuleChange::Rekeyed { from, to } => json!({"rekeyed": [from.as_str(), to.as_str()]}),
        ModuleChange::Muted { module, muted } => json!({"muted": [module.as_str(), muted]}),
    }
}

fn expect_json(e: &Expect) -> Value {
    match e {
        Expect::Identical => json!("identical"),
        Expect::Values(v) => json!({"values": v.iter().map(|(a, x)| json!([anchor_json(a), x])).collect::<Vec<_>>()}),
        Expect::Confined(c) => {
            let mut ranges = Map::new();
            for (f, d) in &c.ranges {
                ranges.insert(
                    f.to_string(),
                    match d {
                        Declared::Whole => json!("whole"),
                        Declared::Ranges(r) => Value::Array(r.iter().map(range_json).collect()),
                    },
                );
            }
            json!({"confined": {
                "ranges": ranges,
                "lanes": c.lanes.iter().map(|l| json!([l.module.as_str(), l.lane.as_str()])).collect::<Vec<_>>(),
                "modules": c.modules.iter().map(module_json).collect::<Vec<_>>(),
            }})
        }
        Expect::Seen(h) => json!({"seen": h.prefixed()}),
        Expect::Journal(id) => json!({"journal": id}),
    }
}

// ---------------------------------------------------------------- base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64, padded.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64 back to bytes: padded, nothing else in it.
pub fn unbase64(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for (k, chunk) in b.chunks(4).enumerate() {
        let last = k == b.len() / 4 - 1;
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = match c {
                b'=' if i >= 4 - pad => 0,
                _ => B64.iter().position(|x| x == c)? as u32,
            };
            n |= v << (18 - 6 * i);
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    Some(out)
}

// ---------------------------------------------------------------- parsing

type Parse<T> = Result<T, (&'static str, String)>;

fn bad<T>(message: impl Into<String>) -> Parse<T> {
    Err(("tx.invalid", message.into()))
}

/// The fields of object `v` (`what`), refusing any but `allowed`.
fn object<'a>(v: &'a Value, what: &str, allowed: &[&str]) -> Parse<&'a Map<String, Value>> {
    let Value::Object(o) = v else { return bad(format!("{what} must be an object")) };
    if let Some(k) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
        return bad(format!("unknown field `{k}` in {what}, expected one of {}", allowed.join(", ")));
    }
    Ok(o)
}

fn string<'a>(v: Option<&'a Value>, what: &str) -> Parse<&'a str> {
    match v {
        Some(Value::String(s)) => Ok(s),
        _ => bad(format!("{what} must be a string")),
    }
}

fn path(v: Option<&Value>, what: &str) -> Parse<TxPath> {
    TxPath::new(string(v, what)?).map_err(|e| ("tx.path", e))
}

fn sha(v: Option<&Value>, what: &str) -> Parse<Sha> {
    let s = string(v, what)?;
    Sha::parse_prefixed(s).map_or_else(|| bad(format!("{what} must be `sha256:` and 64 lowercase hex digits, not `{s}`")), Ok)
}

fn pos(v: &Value, what: &str) -> Parse<Pos> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Pos::Byte)
            .map_or_else(|| bad(format!("{what} must be a byte offset (a whole number from 0) or {{\"line\", \"utf16_col\"}}")), Ok),
        Value::Object(_) => {
            let o = object(v, what, &["line", "utf16_col"])?;
            let n = |k: &str| o.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
            match (n("line"), n("utf16_col")) {
                (Some(line), Some(utf16_col)) => Ok(Pos::LineCol { line, utf16_col }),
                _ => bad(format!("{what} needs a whole `line` and `utf16_col`, from 0")),
            }
        }
        _ => bad(format!("{what} must be a byte offset or {{\"line\", \"utf16_col\"}}")),
    }
}

fn range(v: &Value, what: &str) -> Parse<Range> {
    match v.as_array().map(Vec::as_slice) {
        Some([s, e]) => Ok(Range { start: pos(s, what)?, end: pos(e, what)? }),
        _ => bad(format!("{what} must be a range, [start, end]")),
    }
}

fn key(v: Option<&Value>, what: &str) -> Parse<Key> {
    Key::new(string(v, what)?).map_or_else(|e| bad(format!("{what}: {e}")), Ok)
}

fn lane(v: Option<&Value>, what: &str) -> Parse<LaneAddr> {
    match v.and_then(Value::as_array).map(Vec::as_slice) {
        Some([m, l]) => Ok(LaneAddr { module: key(Some(m), what)?, lane: key(Some(l), what)? }),
        _ => bad(format!("{what} must be a lane, [module key, lane key]")),
    }
}

fn parse(v: &Value) -> Parse<Tx> {
    let o = object(v, "a Tx", &["id", "base", "label", "ops", "expect", "hint"])?;
    let id = string(o.get("id"), "`id`")?.to_string();
    let mut base = BTreeMap::new();
    let Some(Value::Object(bases)) = o.get("base") else { return bad("`base` must be an object of paths") };
    for (p, b) in bases {
        let p = TxPath::new(p).map_err(|e| ("tx.path", e))?;
        let what = format!("the base of {p}");
        let b = match b {
            Value::Null => Base::Absent,
            Value::String(_) => Base::Sha(sha(Some(b), &what)?),
            Value::Object(_) => {
                let w = object(b, &what, &["sha", "text"])?;
                Base::WithText { sha: sha(w.get("sha"), &what)?, text: string(w.get("text"), &format!("{what}'s text"))?.into() }
            }
            _ => return bad(format!("{what} must be `sha256:…`, null, or {{\"sha\", \"text\"}}")),
        };
        base.insert(p, b);
    }
    let label = match o.get("label") {
        None => (String::new(), String::new()),
        Some(l) => match l.as_array().map(Vec::as_slice) {
            Some([Value::String(a), Value::String(b)]) => (a.clone(), b.clone()),
            _ => return bad("`label` must be two strings, [before, after]"),
        },
    };
    let hint = match o.get("hint") {
        None | Some(Value::Null) => None,
        Some(h) => Some(h.as_u64().map_or_else(|| bad("`hint` must be a revision number"), Ok)?),
    };
    let Some(Value::Array(ops)) = o.get("ops") else { return bad("`ops` must be a list") };
    let ops = ops.iter().enumerate().map(|(i, op)| parse_op(op, i + 1)).collect::<Parse<Vec<_>>>()?;
    let expect = parse_expect(o.get("expect").map_or_else(|| bad("a Tx needs `expect`"), Ok)?)?;
    Ok(Tx { id, base, label, ops, expect, hint, in_place: false })
}

fn parse_op(v: &Value, n: usize) -> Parse<Op> {
    let what = format!("op {n}");
    let kind = v.get("op").and_then(Value::as_str).unwrap_or_default();
    Ok(match kind {
        "text_edit" => {
            let o = object(v, &what, &["op", "file", "edits"])?;
            let Some(Value::Array(edits)) = o.get("edits") else { return bad(format!("{what}'s `edits` must be a list")) };
            let edits = edits
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let w = format!("edit {} of {what}", i + 1);
                    let e = object(e, &w, &["start", "end", "expected", "text"])?;
                    let p = |k: &str| e.get(k).map_or_else(|| bad(format!("{w} needs `{k}`")), |x| pos(x, &format!("{w}'s `{k}`")));
                    Ok(Edit {
                        start: p("start")?,
                        end: p("end")?,
                        expected: string(e.get("expected"), &format!("{w}'s `expected`"))?.to_string(),
                        text: string(e.get("text"), &format!("{w}'s `text`"))?.to_string(),
                    })
                })
                .collect::<Parse<Vec<_>>>()?;
            Op::TextEdit { file: path(o.get("file"), &format!("{what}'s `file`"))?, edits }
        }
        "write" => {
            let o = object(v, &what, &["op", "file", "text", "base64", "local"])?;
            let file = path(o.get("file"), &format!("{what}'s `file`"))?;
            let bytes = match (o.get("text"), o.get("base64"), o.get("local")) {
                (Some(t), None, None) => Payload::Text(string(Some(t), &format!("{what}'s `text`"))?.into()),
                (None, Some(b), None) => {
                    let s = string(Some(b), &format!("{what}'s `base64`"))?;
                    Payload::Bytes(unbase64(s).map_or_else(|| bad(format!("{what}'s `base64` isn't standard, padded base64")), Ok)?.into())
                }
                (None, None, Some(l)) => {
                    let w = format!("{what}'s `local`");
                    let l = object(l, &w, &["path", "sha"])?;
                    Payload::Local { path: string(l.get("path"), &w)?.to_string(), sha: sha(l.get("sha"), &w)? }
                }
                _ => return bad(format!("{what} needs exactly one of `text`, `base64` and `local`")),
            };
            Op::Write { file, bytes }
        }
        "remove" => {
            let o = object(v, &what, &["op", "file"])?;
            Op::Remove { file: path(o.get("file"), &format!("{what}'s `file`"))? }
        }
        "rename" => {
            let o = object(v, &what, &["op", "from", "to"])?;
            Op::Rename { from: path(o.get("from"), &format!("{what}'s `from`"))?, to: path(o.get("to"), &format!("{what}'s `to`"))? }
        }
        "clips" => {
            let o = object(v, &what, &["op", "lane", "put", "delete"])?;
            let put = match o.get("put") {
                None => vec![],
                Some(Value::Array(p)) if p.iter().all(Value::is_object) => p.clone(),
                _ => return bad(format!("{what}'s `put` must be a list of clips")),
            };
            let delete = match o.get("delete") {
                None => vec![],
                Some(Value::Array(d)) => {
                    d.iter().map(|x| string(Some(x), &format!("{what}'s `delete`")).map(str::to_string)).collect::<Parse<Vec<_>>>()?
                }
                _ => return bad(format!("{what}'s `delete` must be a list of clip ids")),
            };
            Op::Clips { lane: lane(o.get("lane"), &format!("{what}'s `lane`"))?, put, delete }
        }
        other => return bad(format!("{what}: `{other}` isn't an op: text_edit, write, remove, rename or clips")),
    })
}

fn parse_anchor(v: &Value) -> Parse<Anchor> {
    let o = object(v, "an anchor", &["written", "slot"])?;
    match (o.get("written"), o.get("slot")) {
        (Some(w), None) => {
            let w = object(w, "a written anchor", &["file", "range"])?;
            let r = w.get("range").map_or_else(|| bad("a written anchor needs `range`"), |r| range(r, "a written anchor's range"))?;
            Ok(Anchor::Written { file: path(w.get("file"), "a written anchor's `file`")?, range: r })
        }
        (None, Some(s)) => {
            let s = object(s, "a slot anchor", &["site", "slot"])?;
            let site = s.get("site").map_or_else(|| bad("a slot anchor needs `site`"), Ok)?;
            let site = object(site, "a slot anchor's site", &["file", "range"])?;
            let r = site.get("range").map_or_else(|| bad("a site needs `range`"), |r| range(r, "a site's range"))?;
            Ok(Anchor::Slot {
                site: Spot { file: path(site.get("file"), "a site's `file`")?, range: r },
                slot: string(s.get("slot"), "a slot anchor's `slot`")?.to_string(),
            })
        }
        _ => bad("an anchor is {\"written\": …} or {\"slot\": …}"),
    }
}

fn parse_expect(v: &Value) -> Parse<Expect> {
    if v.as_str() == Some("identical") {
        return Ok(Expect::Identical);
    }
    let o = object(v, "`expect`", &["values", "confined", "seen", "journal"])?;
    if o.len() != 1 {
        return bad("`expect` is \"identical\" or one of values, confined, seen and journal");
    }
    let (k, x) = o.iter().next().expect("one");
    Ok(match k.as_str() {
        "values" => {
            let Value::Array(vs) = x else { return bad("`values` must be a list of [anchor, number]") };
            let vs = vs
                .iter()
                .map(|p| match p.as_array().map(Vec::as_slice) {
                    Some([a, n]) => Ok((parse_anchor(a)?, n.as_f64().map_or_else(|| bad("a value must be a number"), Ok)?)),
                    _ => bad("`values` must be a list of [anchor, number]"),
                })
                .collect::<Parse<Vec<_>>>()?;
            Expect::Values(vs)
        }
        "confined" => {
            let c = object(x, "`confined`", &["ranges", "lanes", "modules"])?;
            let mut ranges = BTreeMap::new();
            match c.get("ranges") {
                None => {}
                Some(Value::Object(r)) => {
                    for (f, d) in r {
                        let f = TxPath::new(f).map_err(|e| ("tx.path", e))?;
                        let d = match d {
                            Value::String(s) if s == "whole" => Declared::Whole,
                            Value::Array(rs) => {
                                Declared::Ranges(rs.iter().map(|r| range(r, &format!("a range of {f}"))).collect::<Parse<_>>()?)
                            }
                            _ => return bad(format!("the ranges of {f} are a list of [start, end], or \"whole\"")),
                        };
                        ranges.insert(f, d);
                    }
                }
                _ => return bad("`ranges` must be an object of paths"),
            }
            let lanes = match c.get("lanes") {
                None => vec![],
                Some(Value::Array(l)) => l.iter().map(|l| lane(Some(l), "a confined lane")).collect::<Parse<_>>()?,
                _ => return bad("`lanes` must be a list of [module key, lane key]"),
            };
            let modules = match c.get("modules") {
                None => vec![],
                Some(Value::Array(m)) => m.iter().map(parse_module).collect::<Parse<_>>()?,
                _ => return bad("`modules` must be a list"),
            };
            Expect::Confined(Confined { ranges, lanes, modules })
        }
        "seen" => Expect::Seen(sha(Some(x), "`seen`")?),
        "journal" => Expect::Journal(string(Some(x), "`journal`")?.to_string()),
        _ => unreachable!("only known fields"),
    })
}

fn parse_module(v: &Value) -> Parse<ModuleChange> {
    let o = object(v, "a module change", &["added", "removed", "rekeyed", "muted"])?;
    let (k, x) = match (o.len(), o.iter().next()) {
        (1, Some(kv)) => kv,
        _ => return bad("a module change is one of added, removed, rekeyed and muted"),
    };
    Ok(match (k.as_str(), x.as_array().map(Vec::as_slice)) {
        ("added", _) => ModuleChange::Added(key(Some(x), "an added module")?),
        ("removed", _) => ModuleChange::Removed(key(Some(x), "a removed module")?),
        ("rekeyed", Some([a, b])) => {
            ModuleChange::Rekeyed { from: key(Some(a), "a rekeyed module")?, to: key(Some(b), "a rekeyed module")? }
        }
        ("muted", Some([m, Value::Bool(b)])) => ModuleChange::Muted { module: key(Some(m), "a muted module")?, muted: *b },
        _ => return bad(format!("`{k}` needs [from, to] for rekeyed, or [module, true|false] for muted")),
    })
}

#[cfg(test)]
mod tests;

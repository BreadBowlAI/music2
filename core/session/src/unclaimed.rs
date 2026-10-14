//! Data no declared lane or plugin reads (ID-17, DIAG-6). Data is filed under keys
//! (`data/kick/hits.json`, `data/lead/lead.state`), so renaming a track's, a lane's or a
//! plugin's key in code leaves its data behind, and it plays nothing. Each such file, or the
//! whole folder of a track whose key changed, is the Warning `data.unclaimed`: what happened in
//! plain words, and the fixes, likeliest first. A fix is a ready-to-commit Tx (DIAG-1), committed
//! only when asked; nothing applies one by itself (DIAG-5).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_ir::{Ir, Kind, LaneDecl, Module, Unit};
use m2_song::data::{delete_fix, relink_fix};
use m2_song::path::slashed;
use m2_song::{
    Confined, DataPath, Decls, Diagnostic, Expect, Fix, Holds, Key, LaneAddr, LaneData, LaneKind, Project, ROOT, Sha, Sidecar, Stored,
    Unused,
};

/// What earlier revisions declared, by module key, for messages about data a rename left
/// behind: each module's label, so the data is named as its track was, and each lane as last
/// declared, so it is offered only to lanes that can be it.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// Module labels by key: `lead-vox` → `Lead Vox`.
    pub names: BTreeMap<String, String>,
    /// Each lane as last declared, by module key and lane key: `(bass, cutoff)`.
    pub lanes: BTreeMap<(String, String), SeenLane>,
}

/// A lane as it was last declared.
#[derive(Clone, Debug, PartialEq)]
pub struct SeenLane {
    /// `control<hz>`: the unit its automation is in.
    pub kind: Kind,
    /// The keys of the modules declared with it. None of them is its track renamed, so its
    /// data isn't offered to them in one click.
    pub beside: Arc<BTreeSet<String>>,
}

impl Seen {
    /// Take in what `ir` declares, over what earlier revisions did.
    pub fn add(&mut self, ir: &Ir) {
        let beside: Arc<BTreeSet<String>> = Arc::new(ir.modules.iter().map(|m| m.key.clone()).collect());
        for m in &ir.modules {
            let lanes = m.lanes.iter().map(|l| ((m.key.clone(), l.key.clone()), SeenLane { kind: l.kind, beside: beside.clone() }));
            self.lanes.extend(lanes);
            self.names.insert(m.key.clone(), m.label().to_string());
        }
    }
}

/// `data.unclaimed` for `project` compiled to `ir`, which declares `decls`, given what earlier
/// revisions declared. When exactly one `data/<k>/` folder is claimed by no module and exactly
/// one module that reads data has no folder, a track's key plainly changed: the folder is one
/// Warning, offered whole to that module first, then file by file (DIAG-6). Otherwise each
/// file is a Warning of its own.
pub fn unclaimed(project: &Project, ir: &Ir, decls: &Decls, seen: &Seen) -> Vec<Diagnostic> {
    let unused = project.unused(&used(ir));
    let offers = lane_offers(project, ir, seen, &unused);
    let states = states(project, decls);
    let homes = project.data_modules();
    let declared: BTreeSet<&str> = ir.modules.iter().map(|m| m.key.as_str()).collect();
    let folders: Vec<&Key> = homes.iter().filter(|k| !declared.contains(k.as_str())).collect();
    // Modules that read data and have none: where a renamed track's folder can go. The root
    // module is never a track renamed.
    let homeless: Vec<&Module> = ir
        .modules
        .iter()
        .filter(|m| m.key != ROOT && !homes.iter().any(|h| h.as_str() == m.key))
        .filter(|m| !m.lanes.is_empty() || decls.states.iter().any(|s| s.module.as_str() == m.key))
        .collect();
    let folder = match (&folders[..], &homeless[..]) {
        ([k], [m]) => Some((*k, *m)),
        _ => None,
    };
    let mut out = vec![];
    if let Some((from, to)) = folder {
        out.push(folder_warning(project, ir, decls, seen, (from, to), &unused, &offers, &states));
    }
    let elsewhere = |k: &Key| folder.is_none_or(|(f, _)| f != k);
    for (n, u) in unused.iter().enumerate().filter(|(_, u)| elsewhere(&u.addr.module)) {
        out.push(lane_warning(ir, seen, u, &offers, n));
    }
    for s in states.iter().filter(|s| elsewhere(&s.module)) {
        out.push(state_warning(project, ir, decls, seen, s));
    }
    out
}

/// Every declared lane, with its module.
pub fn declared(ir: &Ir) -> impl Iterator<Item = (&Module, &LaneDecl)> {
    ir.modules.iter().flat_map(|m| m.lanes.iter().map(move |l| (m, l)))
}

/// A track as messages name it: its label, with its key when they differ, since the data is
/// filed under the key and a key renamed by hand keeps its label (ID-17).
pub fn named(label: &str, key: &str) -> String {
    if label == key { key.to_string() } else { format!("{label} ({key})") }
}

/// The module keyed `key` as messages name it: as declared now, else as last seen.
pub fn name_of(ir: &Ir, seen: &Seen, key: &str) -> String {
    let label = ir.modules.iter().find(|m| m.key == key).map(|m| m.label()).or(seen.names.get(key).map(String::as_str));
    named(label.unwrap_or(key), key)
}

/// Every declared lane's address.
pub fn used(ir: &Ir) -> BTreeSet<LaneAddr> {
    declared(ir).filter_map(|(m, l)| LaneAddr::new(&m.key, &l.key).ok()).collect()
}

/// Whether `u` is filed under `module`'s key.
fn ours(u: &Unused, module: &Module) -> bool {
    u.addr.module.as_str() == module.key
}

/// The SHA-256 of a text file's bytes, as a fix's base names it.
fn sha(text: &str) -> Option<Sha> {
    Some(Sha::of(text.as_bytes()))
}

/// A binary's base: its hash, if it could be read.
fn stored(s: &Stored) -> Option<Sha> {
    match s {
        Stored::Hash(sha) => Some(*sha),
        Stored::Listed | Stored::Unreadable(_) => None,
    }
}

/// A Warning that data isn't used: `title`, then why and what to do. Each fix's Tx gets its id
/// from the problem and its bases (R19).
fn warning(title: String, message: String, file: &str, module: &Key, fixes: Vec<Fix>) -> Diagnostic {
    let d = Diagnostic::warning("data.unclaimed", format!("{title}. {message}")).file(file).module(module);
    fixes.into_iter().fold(d, Diagnostic::fix)
}

/// The fix that deletes `files`, each while it has its hash: it changes nothing that plays, since
/// nothing reads them (`Identical`). Labelled `what` → `deleted`.
fn delete(files: impl IntoIterator<Item = (String, Option<Sha>)>, what: &str, title: &str) -> Option<Fix> {
    let files: Vec<(PathBuf, Option<Sha>)> = files.into_iter().map(|(f, s)| (PathBuf::from(f), s)).collect();
    delete_fix(&files, (what.to_string(), "deleted".to_string()), title)
}

/// The fix that renames each of `moves` (from, its hash, to), none of whose targets may exist,
/// confined to `lanes`, the lanes it makes play (WRITE-10), and labelled `label`.
fn relink(
    moves: impl IntoIterator<Item = (String, Option<Sha>, String)>,
    label: (String, String),
    title: String,
    likely: bool,
    lanes: Vec<LaneAddr>,
) -> Option<Fix> {
    let moves: Vec<(PathBuf, Option<Sha>, PathBuf)> = moves.into_iter().map(|(f, s, t)| (PathBuf::from(f), s, PathBuf::from(t))).collect();
    relink_fix(&moves, label, title, likely, Expect::Confined(Confined { lanes, ..Confined::default() }))
}

// ---------------------------------------------------------------- lanes

/// A declared lane with no data, where data no lane reads can go.
struct Free<'a> {
    module: &'a Module,
    lane: &'a LaneDecl,
    file: PathBuf,
}

impl Free<'_> {
    /// `Kick (kicks) › hits`.
    fn place(&self) -> String {
        format!("{} › {}", named(self.module.label(), &self.module.key), self.lane.key)
    }
}

/// Where each lane file no lane reads can go: indexes into `free`, plainest first, and whether
/// each is the one it plainly belongs to.
struct Offers<'a> {
    free: Vec<Free<'a>>,
    to: Vec<Vec<(usize, bool)>>,
}

/// The lanes each of `unused` can go to: declared lanes with no data that can read it (DIAG-6).
/// A lane is `likely` only when it is a plain claim, alone in first place, on a lane no other
/// file has as plain a claim on: two files left by one change can't both be the renamed one's.
fn lane_offers<'a>(project: &Project, ir: &'a Ir, seen: &Seen, unused: &[Unused]) -> Offers<'a> {
    let free: Vec<Free> = declared(ir)
        .filter_map(|(m, l)| Some((m, l, LaneAddr::new(&m.key, &l.key).ok()?)))
        .filter(|(_, _, a)| project.lane_file(a).is_none())
        .map(|(module, lane, a)| Free { module, lane, file: DataPath::lane(&a).path() })
        .collect();
    let claims: Vec<Vec<(usize, Claim)>> = unused
        .iter()
        .map(|u| {
            let held = Held::of(u, seen);
            let here = ir.modules.iter().any(|m| ours(u, m));
            let beside = seen.lanes.get(&(u.addr.module.to_string(), u.addr.lane.to_string())).map(|l| &*l.beside);
            let mut to: Vec<(usize, Claim)> = free
                .iter()
                .enumerate()
                .filter(|(_, f)| held.unreadable(f.lane.kind).is_none())
                .map(|(i, f)| (i, Claim::new(u, &held, here || beside.is_some_and(|b| b.contains(&f.module.key)), f.module, f.lane)))
                .collect();
            to.sort_by_key(|(_, c)| *c);
            to
        })
        .collect();
    let to = claims
        .iter()
        .enumerate()
        .map(|(n, mine)| {
            let likely = match &mine[..] {
                [(lane, first), rest @ ..] => {
                    !first.loose
                        && rest.first().is_none_or(|(_, next)| next != first)
                        && claims.iter().enumerate().all(|(k, theirs)| k == n || theirs.iter().all(|(l, c)| l != lane || c > first))
                }
                [] => false,
            };
            mine.iter().enumerate().map(|(k, &(i, _))| (i, likely && k == 0)).collect()
        })
        .collect();
    Offers { free, to }
}

/// The fix that moves lane file `u` to `to`, confined to that lane.
fn relink_lane(u: &Unused, to: &Free, likely: bool) -> Option<Fix> {
    let addr = LaneAddr::new(&to.module.key, &to.lane.key).ok()?;
    let label = (format!("unused {}", noun(Holds::of(u.text))), format!("{} › {}", addr.module, addr.lane));
    relink([(slashed(u.path), sha(u.text), slashed(&to.file))], label, format!("Move to {}", to.place()), likely, vec![addr])
}

/// `data.unclaimed` on lane file `u`, the `n`th of the unused.
fn lane_warning(ir: &Ir, seen: &Seen, u: &Unused, offers: &Offers, n: usize) -> Diagnostic {
    let file = slashed(u.path);
    let key = u.addr.module.as_str();
    let here = ir.modules.iter().any(|m| ours(u, m));
    let name = name_of(ir, seen, key);
    let mine = &offers.to[n];
    let likely = mine.first().is_some_and(|&(_, l)| l);
    let mut fixes: Vec<Fix> = mine.iter().filter_map(|&(i, likely)| relink_lane(u, &offers.free[i], likely)).collect();
    fixes.extend(delete([(file.clone(), sha(u.text))], &format!("unused {}", noun(Holds::of(u.text))), "Delete it"));

    let lane = &u.addr.lane;
    let place = format!("{name} › {lane}");
    let places: Vec<String> = mine.iter().map(|&(i, _)| offers.free[i].place()).collect();
    let why = match here {
        true => format!("{name} has no lane called {lane} any more."),
        false => format!("There's no track called {name} any more."),
    };
    let to = match &places[..] {
        [one, ..] if likely => To::Likely(one),
        [] if here => To::Nowhere(format!("a lane called {lane} to {name}")),
        [] => To::Nowhere(format!("a track called {name} with a lane called {lane}")),
        many => To::Choice(many),
    };
    let (title, message) = words(Holds::of(u.text), &place, &why, to, &file);
    warning(title, message, &file, &u.addr.module, fixes)
}

// ---------------------------------------------------------------- a track's folder

/// The hash a `data/<module>/` folder has as a base (WRITE-12): the SHA-256 of the `\n`-joined
/// lines `<path>\0<sha256 hex>`, one for each lane and state file under it, by its path in the
/// folder, sorted bytewise. None when a file in it can't be read.
pub fn folder_hash(project: &Project, module: &Key) -> Option<Sha> {
    let dir = DataPath::Module { module: module.clone() }.path();
    let lanes = project.lane_files().filter(|(a, ..)| &a.module == module).map(|(_, p, t)| Some((p, Sha::of(t.as_bytes()))));
    let states = project.state_files().filter(|p| DataPath::parse(p).module() == Some(module)).map(|p| {
        let hash = match project.sidecar(p) {
            Some(text) => sha(text),
            None => project.binary(p).and_then(stored),
        };
        Some((p, hash?))
    });
    let files: Vec<(String, Sha)> = lanes
        .chain(states)
        .map(|f| f.map(|(p, sha)| (slashed(p.strip_prefix(&dir).expect("under its folder")), sha)))
        .collect::<Option<_>>()?;
    Some(m2_song::dir_hash(&files))
}

/// `data.unclaimed` on the folder of a track whose key changed, `from` to `to.key` (DIAG-6):
/// the whole folder offered to `to` first, likely when its files are what `to` reads; then each
/// file where it can go; then delete it all.
#[allow(clippy::too_many_arguments)]
fn folder_warning(
    project: &Project,
    ir: &Ir,
    decls: &Decls,
    seen: &Seen,
    (from, to): (&Key, &Module),
    unused: &[Unused],
    offers: &Offers,
    states: &[State],
) -> Diagnostic {
    let dir = slashed(&DataPath::Module { module: from.clone() }.path());
    let name = name_of(ir, seen, from.as_str());
    let new = named(to.label(), &to.key);
    let lanes: Vec<(usize, &Unused)> = unused.iter().enumerate().filter(|(_, u)| &u.addr.module == from).collect();
    let mine: Vec<&State> = states.iter().filter(|s| &s.module == from).collect();
    let lanes_fit = lanes.iter().all(|(_, u)| {
        let held = Held::of(u, seen);
        to.lanes.iter().any(|l| l.key == u.addr.lane.as_str() && held.unreadable(l.kind).is_none())
    });
    let states_fit = mine.iter().all(|s| decls.states.iter().any(|d| d.module.as_str() == to.key && d.path == s.path));
    // A module declared beside it in an earlier revision isn't it renamed.
    let beside = seen.lanes.iter().any(|((m, _), l)| m == from.as_str() && l.beside.contains(&to.key));
    // A file in it that is neither a lane nor a state (a dot file aside), or one that can't be
    // read, keeps the folder from moving whole (WRITE-12's directory hash refuses it): then only
    // its files are offered, one at a time, never a fix that can't apply (DIAG-1).
    let folder = DataPath::Module { module: from.clone() }.path();
    let stray = project.unknown_files().chain(project.unreadable().map(|(p, _)| p)).find(|p| p.starts_with(&folder));
    let likely = lanes_fit && states_fit && !beside && stray.is_none();

    let mut fixes = vec![];
    if let (None, Some(base), Ok(key)) = (stray, folder_hash(project, from), Key::new(&to.key)) {
        let dest = slashed(&DataPath::Module { module: key.clone() }.path());
        // Confined to every lane of the new module the folder holds data for (ID-17).
        let fed: Vec<LaneAddr> = lanes
            .iter()
            .filter(|(_, u)| to.lanes.iter().any(|l| l.key == u.addr.lane.as_str()))
            .map(|(_, u)| LaneAddr { module: key.clone(), lane: u.addr.lane.clone() })
            .collect();
        let label = ("unused data".to_string(), new.clone());
        // "all": the folder whole, beside the offers that move one file (ID-17).
        fixes.extend(relink([(dir.clone(), Some(base), dest)], label, format!("Move all to {new}"), likely, fed));
    }
    for (n, u) in &lanes {
        fixes.extend(offers.to[*n].iter().filter_map(|&(i, _)| relink_lane(u, &offers.free[i], false)));
    }
    for s in mine.iter().filter(|s| s.blob.is_some()) {
        fixes.extend(state_targets(project, decls, s).iter().filter_map(|t| s.relink(t, false)));
    }
    let files = lanes.iter().map(|(_, u)| (slashed(u.path), sha(u.text))).chain(mine.iter().flat_map(|s| s.files()));
    fixes.extend(delete(files, "unused data", "Delete it all"));

    let mut held: Vec<String> = lanes.iter().map(|(_, u)| held_in(Holds::of(u.text), u.addr.lane.as_str())).collect();
    held.extend(mine.iter().map(|s| format!("the state of {} at {}", s.plugin().unwrap_or_else(|| "a plugin".into()), s.at())));
    let fix = match (likely, stray) {
        (true, _) => format!("Did you rename it? Move its data to {new} to hear it again, or delete it."),
        (false, Some(f)) => format!(
            "Did you rename it? Move its files one at a time where they belong now, or delete it. {} isn't data, so the folder \
             moves whole only once that file is moved out or deleted.",
            slashed(f)
        ),
        (false, None) => {
            format!("Did you rename it? Move its data to {new}, or its files one at a time where they belong now, or delete it.")
        }
    };
    let title = format!("Data filed under {name} isn't playing");
    let message = format!("There's no track called {name} any more, so nothing plays what {dir} holds: {}. {fix}", and(&held));
    warning(title, message, &dir, from, fixes)
}

// ---------------------------------------------------------------- plugin state

/// A plugin state no stateful plugin reads: its blob, its sidecar, or both.
pub(crate) struct State<'a> {
    pub(crate) module: Key,
    pub(crate) path: Vec<Key>,
    blob: Option<(&'a Path, &'a Stored)>,
    sidecar: Option<(&'a Path, &'a str)>,
}

impl State<'_> {
    /// The file a message is about: the blob, else its sidecar.
    pub(crate) fn file(&self) -> &Path {
        self.blob.map(|b| b.0).or(self.sidecar.map(|s| s.0)).expect("one of them")
    }

    /// The plugin its sidecar says saved it.
    fn plugin(&self) -> Option<String> {
        Sidecar::read(self.sidecar?.1).ok().map(|s| s.plugin)
    }

    /// `chain-l/tune`: its frames, then its key.
    fn at(&self) -> String {
        self.path.iter().map(Key::as_str).collect::<Vec<_>>().join("/")
    }

    /// Its files, each with its base.
    fn files(&self) -> Vec<(String, Option<Sha>)> {
        let blob = self.blob.map(|(p, s)| (slashed(p), stored(s)));
        blob.into_iter().chain(self.sidecar.map(|(p, t)| (slashed(p), sha(t)))).collect()
    }

    /// The fix that moves it, blob and sidecar together, to `to`. The state it then loads is the
    /// Tx's own write (C1), so it is confined to no lane.
    fn relink(&self, to: &m2_song::StateDecl, likely: bool) -> Option<Fix> {
        let at = |sidecar| slashed(&DataPath::PluginState { module: to.module.clone(), path: to.path.clone(), sidecar }.path());
        let blob = self.blob.map(|(p, s)| (slashed(p), stored(s), at(false)));
        let side = self.sidecar.map(|(p, t)| (slashed(p), sha(t), at(true)));
        let place = format!("{} › {}", to.module, to.path.iter().map(Key::as_str).collect::<Vec<_>>().join("/"));
        let label = ("unused plugin state".to_string(), place.clone());
        relink(blob.into_iter().chain(side), label, format!("Move it to {place}"), likely, vec![])
    }
}

/// Every plugin state in the project that no declared stateful plugin reads.
pub(crate) fn states<'a>(project: &'a Project, decls: &Decls) -> Vec<State<'a>> {
    let mut found: BTreeMap<(Key, Vec<Key>), State> = BTreeMap::new();
    for p in project.state_files() {
        let DataPath::PluginState { module, path, sidecar } = DataPath::parse(p) else { continue };
        if decls.states.iter().any(|s| s.module == module && s.path == path) {
            continue;
        }
        let s = found.entry((module.clone(), path.clone())).or_insert(State { module, path, blob: None, sidecar: None });
        match sidecar {
            true => s.sidecar = project.sidecar(p).map(|t| (p, t)),
            false => s.blob = project.binary(p).map(|b| (p, b)),
        }
    }
    found.into_values().collect()
}

/// The declared stateful plugins `s` can go to: of the plugin its sidecar names, with no state
/// of their own; those in its own module first.
fn state_targets<'d>(project: &Project, decls: &'d Decls, s: &State) -> Vec<&'d m2_song::StateDecl> {
    let Some(plugin) = s.plugin() else { return vec![] };
    let free = |d: &&m2_song::StateDecl| {
        let blob = DataPath::PluginState { module: d.module.clone(), path: d.path.clone(), sidecar: false }.path();
        d.plugin == plugin && project.binary(&blob).is_none()
    };
    let mut to: Vec<&m2_song::StateDecl> = decls.states.iter().filter(free).collect();
    to.sort_by_key(|d| d.module != s.module);
    to
}

/// `data.unclaimed` on a plugin state no stateful plugin reads.
fn state_warning(project: &Project, ir: &Ir, decls: &Decls, seen: &Seen, s: &State) -> Diagnostic {
    let file = slashed(s.file());
    let name = name_of(ir, seen, s.module.as_str());
    let here = ir.modules.iter().any(|m| m.key == s.module.as_str());
    // A sidecar alone has nothing to move: it is only deleted.
    let targets = if s.blob.is_some() { state_targets(project, decls, s) } else { vec![] };
    let likely = matches!(&targets[..], [one] if one.module == s.module);
    let mut fixes: Vec<Fix> = targets.iter().enumerate().filter_map(|(k, t)| s.relink(t, likely && k == 0)).collect();
    fixes.extend(delete(s.files(), "unused plugin state", "Delete it"));

    let at = s.at();
    if s.blob.is_none() {
        let title = format!("{file} isn't used");
        return warning(title, format!("There is no saved state beside it at {name} › {at}: delete it."), &file, &s.module, fixes);
    }
    let why = match here {
        true => format!("No plugin in {name} keeps its state at {at} any more."),
        false => format!("There's no track called {name} any more."),
    };
    let place = |t: &m2_song::StateDecl| {
        format!("{} › {}", name_of(ir, seen, t.module.as_str()), t.path.iter().map(Key::as_str).collect::<Vec<_>>().join("/"))
    };
    let fix = match (&targets[..], s.plugin()) {
        ([one], _) if likely => format!("Did you rename its key? Move it to {} to load it again, or delete it.", place(one)),
        ([], Some(plugin)) => format!("To load it again, give a {plugin} plugin in {name} the key path {at}, or delete it."),
        ([], None) => "It has no sidecar saying which plugin saved it: delete it, or move it where it belongs by hand.".into(),
        (many, _) => format!(
            "Did you rename its key? Move it to the plugin it belongs to now ({}), or delete it.",
            or(&many.iter().map(|t| place(t)).collect::<Vec<_>>())
        ),
    };
    warning(format!("The plugin state saved at {name} › {at} isn't loaded"), format!("{why} {fix}"), &file, &s.module, fixes)
}

/// What a lane file holds, in a few words: `notes in hits`.
fn held_in(data: Holds, lane: &str) -> String {
    match data {
        Holds::Notes => format!("notes in {lane}"),
        Holds::Control => format!("automation in {lane}"),
        Holds::Audio => format!("audio clips in {lane}"),
        Holds::Empty => format!("{lane}, which is empty"),
        Holds::Unknown => format!("{lane}, which no lane can read"),
    }
}

/// `a`, `a and b`, `a, b and c`.
fn and(items: &[String]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

// ---------------------------------------------------------------- words

/// How plainly a free lane is where a file's data belongs; the plainest sorts first: the lanes
/// left on its own track, then the same lane on another track, then the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Claim {
    /// Only ever a choice, never offered in one click.
    loose: bool,
    /// On another track than the one the file is named after.
    elsewhere: bool,
    /// Under another name than the file's lane.
    renamed: bool,
}

impl Claim {
    /// The claim of `held`, the data in `u`, on `module`'s lane `lane`. `kept` is whether
    /// `module` can't be the track `u` is named after, renamed: that track is still there, or
    /// `module` was there beside it. Then another track's lane is only a choice. A lane under
    /// another name is only a choice for automation of a unit not known, since it may be a lane
    /// of another unit.
    fn new(u: &Unused, held: &Held, kept: bool, module: &Module, lane: &LaneDecl) -> Claim {
        let (elsewhere, renamed) = (!ours(u, module), lane.key != u.addr.lane.as_str());
        let loose = match (elsewhere, renamed) {
            (false, false) => false,
            (false, true) => held.data == Holds::Control && held.unit.is_none(),
            (true, false) => kept,
            (true, true) => true,
        };
        Claim { loose, elsewhere, renamed }
    }
}

/// What a lane file no lane reads holds, as far as can be told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub data: Holds,
    /// For automation, the unit the file records (FMT-10), or else that of the lane that last
    /// read it in this session.
    unit: Option<Unit>,
    /// For automation, its lowest and highest values.
    span: Option<(f64, f64)>,
}

impl Held {
    pub fn of(u: &Unused, seen: &Seen) -> Held {
        let data = Holds::of(u.text);
        let control = data == Holds::Control;
        let read = LaneData::read(u.text, &LaneKind::Control { unit: None }).ok().filter(|_| control);
        let recorded = match &read {
            Some(LaneData::Control { unit: Some(u), .. }) => u.parse().ok(),
            _ => None,
        };
        let unit = recorded.or(match seen.lanes.get(&(u.addr.module.to_string(), u.addr.lane.to_string())).map(|l| l.kind) {
            Some(Kind::Control(unit)) if control => unit,
            _ => None,
        });
        let points = match &read {
            Some(LaneData::Control { clips, .. }) => clips.iter().flat_map(|c| &c.points).map(|p| p.1).collect(),
            _ => vec![],
        };
        let span = points.into_iter().fold(None, |s: Option<(f64, f64)>, v| Some(s.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v)))));
        Held { data, unit, span }
    }

    /// What it holds, in a few words, if a lane of `kind` can't read it: `notes`,
    /// `automation in hz`.
    pub fn unreadable(&self, kind: Kind) -> Option<String> {
        let reads = self.data.fits(&m2_runtime::lane_kind(kind));
        if !reads {
            return Some(noun(self.data).to_string());
        }
        let Kind::Control(Some(into)) = kind else { return None };
        if let Some(unit) = self.unit
            && !unit.fits(into)
        {
            return Some(format!("automation in {}", unit.name()));
        }
        // Units whose values are bounded by what they are, whatever unit the data had.
        let (lo, hi) = match into {
            Unit::Unit01 => (0.0, 1.0),
            Unit::Bipolar => (-1.0, 1.0),
            _ => return None,
        };
        match self.span {
            Some((low, high)) if low < lo || high > hi => Some(format!("automation that reaches {}", if high > hi { high } else { low })),
            _ => None,
        }
    }
}

/// What a lane file holds, in a few words: `notes`.
pub fn noun(data: Holds) -> &'static str {
    match data {
        Holds::Notes => "notes",
        Holds::Control => "automation",
        Holds::Audio => "audio clips",
        Holds::Empty => "empty lane",
        Holds::Unknown => "lane data",
    }
}

/// Where data no lane reads can go, as `Track › lane`.
enum To<'a> {
    /// The one lane it plainly belongs to.
    Likely(&'a str),
    /// Lanes that could take it, likeliest first.
    Choice(&'a [String]),
    /// Nowhere: what to add so it plays again.
    Nowhere(String),
}

/// A warning's title and message about data no lane reads, from where it was (`place`, as
/// `Track › lane`), `why` no lane reads it, and where it can go.
fn words(data: Holds, place: &str, why: &str, to: To<'_>, file: &str) -> (String, String) {
    // What the data is, the words that agree with it, and what being unread means for it.
    let (what, it, they, belong, unread, hear) = match data {
        Holds::Notes => ("Notes", "them", "they", "belong", "aren't playing", "hear"),
        Holds::Control => ("Automation", "it", "it", "belongs", "isn't used", "hear"),
        Holds::Audio => ("Audio clips", "them", "they", "belong", "aren't playing", "hear"),
        Holds::Empty => ("An empty lane file", "it", "it", "belongs", "isn't used", "use"),
        Holds::Unknown => {
            let fix = "It doesn't hold notes, automation or audio clips that a lane can read: fix it by hand, or delete it.";
            return (format!("{file} isn't used"), format!("{why} {fix}"));
        }
    };
    let fix = match to {
        To::Likely(one) => format!("Did you rename it? Move {it} to {one} to {hear} {it} again, or delete {it}."),
        To::Choice(places) => format!("Did you rename it? Move {it} to the lane {they} {belong} to now ({}), or delete {it}.", or(places)),
        To::Nowhere(add) => format!("To {hear} {it} again, add {add}, or delete {it}."),
    };
    (format!("{what} on {place} {unread}"), format!("{why} {fix}"))
}

/// `a`, `a or b`, `a, b or c`.
fn or(items: &[String]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use m2_runtime::PackageSet;
    use m2_song::{Base, TxOp as Op, TxPath};

    use super::*;

    fn tp(s: &str) -> TxPath {
        TxPath::new(s).unwrap()
    }

    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-warnings-test-{}-{n}", std::process::id()));
        for (name, text) in [("music2.toml", crate::tests::MANIFEST)].iter().chain(files) {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    fn compiled(dir: &Path) -> Vec<Diagnostic> {
        seeing(dir, &Seen::default())
    }

    /// `data.unclaimed` for the project in `dir`, as a session that has seen `seen` gives it.
    fn seeing(dir: &Path, seen: &Seen) -> Vec<Diagnostic> {
        let mut packages = PackageSet::default();
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        let project = Project::open(dir).unwrap();
        let ir = crate::compile(&project, &mut packages).unwrap_or_else(|d| panic!("{d:?}")).ir.clone();
        let found = unclaimed(&project, &ir, &m2_runtime::decls(&ir), seen);
        assert!(found.iter().all(|d| d.code == "data.unclaimed" && d.severity == m2_song::Severity::Warning));
        found
    }

    /// What a session knows once `lanes`, as `(module key, lane key, kind)`, were declared together.
    fn seen(lanes: &[(&str, &str, &str)]) -> Seen {
        let beside: Arc<BTreeSet<String>> = Arc::new(lanes.iter().map(|(m, ..)| m.to_string()).collect());
        let lane = |k: &str| SeenLane { kind: k.parse().unwrap(), beside: beside.clone() };
        Seen { lanes: lanes.iter().map(|(m, l, k)| ((m.to_string(), l.to_string()), lane(k))).collect(), ..Seen::default() }
    }

    fn song(kick: &str, lanes: &str) -> String {
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ kick }} from \"std/instruments\";\n\
             const k = track(\"{kick}\", ({{ lane }}) => {{ {lanes} return kick(lane.notes(\"hits\")); }});\n\
             export default song(() => k.out);\n"
        )
    }

    const NOTES: &str = r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;
    const CONTROL: &str = r#"{"clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#;

    /// A song of tracks, each a kick with a notes lane (`hits` unless named as `track.lane`).
    fn tracks(names: &[&str]) -> String {
        let made: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let (n, lane) = n.split_once('.').unwrap_or((n, "hits"));
                format!("const t{i} = track(\"{n}\", ({{ lane }}) => kick(lane.notes(\"{lane}\")));\n")
            })
            .collect();
        let outs: Vec<String> = (0..names.len()).map(|i| format!("t{i}.out")).collect();
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ kick }} from \"std/instruments\";\n\
             import {{ sum }} from \"std/math\";\n{}export default song(() => sum({}));\n",
            made.concat(),
            outs.join(", ")
        )
    }

    /// Where a warning's relinks move its data, as `(track key, lane key, likely)`: a lane is
    /// `data/<track>/<lane>.json`, a whole folder `data/<track>` (lane key empty).
    fn relinks(w: &Diagnostic) -> Vec<(&str, &str, bool)> {
        w.fixes
            .iter()
            .filter(|f| f.code == "relink")
            .map(|f| {
                let Some(Op::Rename { to, .. }) = f.tx.ops.first() else { panic!("a relink renames: {f:?}") };
                let mut segs = to.as_str().split('/').skip(1);
                let module = segs.next().unwrap();
                (module, segs.next().map_or("", |l| l.trim_end_matches(".json")), f.likely)
            })
            .collect()
    }

    fn about<'a>(w: &'a [Diagnostic], file: &str) -> &'a Diagnostic {
        w.iter().find(|w| w.src.as_deref() == Some(file)).unwrap_or_else(|| panic!("no warning about {file}: {w:?}"))
    }

    // DIAG-6, ID-17: a track key renamed by hand leaves its whole folder unclaimed. With one such
    // folder and one module that reads data and has none, the folder is offered whole to it
    // first, likely when its files are what that module reads; then file by file.
    #[test]
    fn a_renamed_tracks_folder_is_offered_whole_to_the_track_it_plainly_is_now() {
        let dir = project(&[("song.ts", &song("kicks", "")), ("data/kick/hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(w.len(), 1);
        assert_eq!((w[0].src.as_deref(), w[0].module.as_deref()), (Some("data/kick"), Some("kick")));
        assert_eq!(
            w[0].message,
            "Data filed under kick isn't playing. There's no track called kick any more, so nothing plays what data/kick holds: \
             notes in hits. Did you rename it? Move its data to kicks to hear it again, or delete it."
        );
        assert_eq!(relinks(&w[0]), [("kicks", "", true), ("kicks", "hits", false)]);
        let folder = &w[0].fixes[0];
        assert_eq!(folder.title, "Move all to kicks");
        assert_eq!(folder.tx.ops, [Op::Rename { from: tp("data/kick"), to: tp("data/kicks") }]);
        // WRITE-12: a folder's base is the hash of its files' paths and hashes.
        let hits = Sha::of(NOTES.as_bytes()).hex();
        let base = Sha::of(format!("hits.json\0{hits}").as_bytes());
        assert_eq!(folder.tx.base, BTreeMap::from([(tp("data/kick"), Base::Sha(base)), (tp("data/kicks"), Base::Absent)]));
        // DIAG-1, ID-17: a ready Tx, confined to the lanes the folder feeds in the new module.
        assert_eq!(
            folder.tx.expect,
            Expect::Confined(Confined { lanes: vec![LaneAddr::new("kicks", "hits").unwrap()], ..Confined::default() })
        );
        assert!(folder.tx.validate().is_ok() && folder.tx.id.starts_with("fix-"));
        let delete = w[0].fixes.last().unwrap();
        assert_eq!((delete.code, delete.title.as_str()), ("delete-data", "Delete it all"));
        assert_eq!(delete.tx.ops, [Op::Remove { file: tp("data/kick/hits.json") }]);
        assert_eq!(delete.tx.expect, Expect::Identical);
        // A name seen before is the one shown.
        let named = seeing(&dir, &Seen { names: BTreeMap::from([("kick".to_string(), "Kick".to_string())]), ..Seen::default() });
        assert!(named[0].message.starts_with("Data filed under Kick (kick) isn't playing. There's no track called Kick (kick) any more"));
        std::fs::remove_dir_all(dir).ok();

        // Files the new track doesn't read: the folder is only a choice, before each file's own offers.
        let dir = project(&[
            ("song.ts", &song("kicks", "lane.control(\"env\", \"unit01\", 0);")),
            ("data/kick/hits.json", NOTES),
            ("data/kick/sweep.json", CONTROL),
        ]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("kicks", "", false), ("kicks", "hits", false), ("kicks", "env", false)]);
        let ends = "notes in hits and automation in sweep. Did you rename it? Move its data to kicks, or its files one at a time \
                    where they belong now, or delete it.";
        assert!(w[0].message.ends_with(ends), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    // DIAG-6, FMT-17: a plugin's state moves with its track's folder, and a state no stateful
    // plugin reads is offered to a plugin of the kind that saved it.
    #[test]
    fn plugin_state_left_behind_is_offered_to_a_plugin_that_can_load_it() {
        let lead = |track: &str, key: &str| {
            format!(
                "import {{ plugin, song, track }} from \"@m2/core\";\n\
                 const l = track(\"{track}\", ({{ lane }}) =>\n\
                   plugin(\"clap:org.example.synth\", {{ key: \"{key}\", notes: lane.notes(\"notes\") }}));\n\
                 export default song(() => l.out);\n"
            )
        };
        let side = r#"{"plugin": "clap:org.example.synth", "plugin_version": "1.0.0"}"#;
        let files = |track: &str, key: &str| {
            vec![
                ("song.ts".to_string(), lead(track, key)),
                ("data/lead/notes.json".to_string(), NOTES.to_string()),
                ("data/lead/lead.state".to_string(), "a blob".to_string()),
                ("data/lead/lead.state.json".to_string(), side.to_string()),
            ]
        };
        let made = |f: Vec<(String, String)>| project(&f.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect::<Vec<_>>());
        // The track renamed: its notes and its state go together, plainly.
        let dir = made(files("leads", "lead"));
        let w = compiled(&dir);
        assert_eq!(w.len(), 1);
        assert_eq!(relinks(&w[0])[0], ("leads", "", true));
        assert!(w[0].message.contains("notes in notes and the state of clap:org.example.synth at lead."), "{}", w[0].message);
        let delete = w[0].fixes.last().unwrap();
        let removed: Vec<&Op> = delete.tx.ops.iter().collect();
        assert_eq!(removed.len(), 3, "{removed:?}");
        std::fs::remove_dir_all(dir).ok();

        // The plugin's key renamed: the state alone, to the plugin of its kind that has none.
        let dir = made(files("lead", "pad"));
        let w = compiled(&dir);
        let state = about(&w, "data/lead/lead.state");
        assert_eq!(relinks(state), [("lead", "pad.state", true)]);
        assert_eq!(
            state.message,
            "The plugin state saved at lead › lead isn't loaded. No plugin in lead keeps its state at lead any more. Did you rename \
             its key? Move it to lead › pad to load it again, or delete it."
        );
        let mv = &state.fixes[0].tx.ops;
        assert_eq!(
            mv,
            &[
                Op::Rename { from: tp("data/lead/lead.state"), to: tp("data/lead/pad.state") },
                Op::Rename { from: tp("data/lead/lead.state.json"), to: tp("data/lead/pad.state.json") },
            ]
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn data_left_by_a_renamed_lane_offers_every_lane_that_reads_it_likeliest_first() {
        let lanes = "lane.control(\"env\", \"unit01\", 0); lane.notes(\"accents\"); lane.notes(\"ghosts\");";
        let dir = project(&[
            ("song.ts", &song("kick", lanes).replace("lane.notes(\"hits\")", "lane.notes(\"beats\")")),
            ("data/kick/hits.json", NOTES),
            ("data/kick/ghosts.json", NOTES),
            ("data/kick/sweep.json", CONTROL),
        ]);
        let w = compiled(&dir);
        let hits = about(&w, "data/kick/hits.json");
        assert_eq!(
            relinks(hits),
            [("kick", "accents", false), ("kick", "beats", false)],
            "notes lanes with no data, in the order written (not ghosts, which has some); two as likely, so the person picks"
        );
        assert_eq!(
            hits.message,
            "Notes on kick › hits aren't playing. kick has no lane called hits any more. Did you rename it? Move them to the lane they \
             belong to now (kick › accents or kick › beats), or delete them."
        );
        // The only automation lane Kick has left, but a project just opened doesn't know the
        // unit sweep was in: env may be a lane of another unit, so it is only a choice.
        let sweep = about(&w, "data/kick/sweep.json");
        assert_eq!(relinks(sweep), [("kick", "env", false)]);
        assert_eq!(
            sweep.message,
            "Automation on kick › sweep isn't used. kick has no lane called sweep any more. Did you rename it? Move it to the lane it \
             belongs to now (kick › env), or delete it."
        );
        // Seen as a 0..1 lane: plainly where its automation belongs.
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<unit01>")]));
        let sweep = about(&w, "data/kick/sweep.json");
        assert_eq!(relinks(sweep), [("kick", "env", true)]);
        assert_eq!(
            sweep.message,
            "Automation on kick › sweep isn't used. kick has no lane called sweep any more. Did you rename it? Move it to kick › env \
             to hear it again, or delete it."
        );
        // Seen in hertz: a 0..1 lane can't take it.
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<hz>")]));
        let sweep = about(&w, "data/kick/sweep.json");
        assert_eq!(relinks(sweep), []);
        assert!(sweep.message.ends_with("To hear it again, add a lane called sweep to kick, or delete it."), "{}", sweep.message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_lane_is_offered_in_one_click_only_where_the_data_plainly_belongs() {
        // Two renamed tracks with the same lane: either could be Kick's.
        let dir = project(&[("song.ts", &tracks(&["kicks", "snares"])), ("data/kick/hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("kicks", "hits", false), ("snares", "hits", false)]);
        assert!(w[0].message.contains("(kicks › hits or snares › hits)"), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();

        // A lane under another name on another track is only ever a choice, even alone; so is
        // the whole folder, which Pad wouldn't read (DIAG-6).
        let dir = project(&[("song.ts", &song("pad", "").replace("\"hits\"", "\"chords\"")), ("data/kick/hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("pad", "", false), ("pad", "chords", false)]);
        assert!(
            w[0].message.ends_with("Move its data to pad, or its files one at a time where they belong now, or delete it."),
            "{}",
            w[0].message
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_lane_two_files_could_have_come_from_is_only_a_choice() {
        // One change renamed Kick to Kicks and deleted Snare: either file could be Kicks'.
        let dir = project(&[("song.ts", &tracks(&["kicks"])), ("data/kick/hits.json", NOTES), ("data/snare/hits.json", NOTES)]);
        let w = compiled(&dir);
        for file in ["data/kick/hits.json", "data/snare/hits.json"] {
            let w = about(&w, file);
            assert_eq!(relinks(w), [("kicks", "hits", false)], "{file}");
            assert!(w.message.ends_with("Move them to the lane they belong to now (kicks › hits), or delete them."), "{}", w.message);
        }
        std::fs::remove_dir_all(dir).ok();

        // A file with a weaker claim doesn't stand in the way: chords on Pad are another lane.
        let dir = project(&[("song.ts", &tracks(&["kicks"])), ("data/kick/hits.json", NOTES), ("data/pad/chords.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(about(&w, "data/kick/hits.json")), [("kicks", "hits", true)]);
        assert_eq!(relinks(about(&w, "data/pad/chords.json")), [("kicks", "hits", false)]);
        std::fs::remove_dir_all(dir).ok();

        // Two of Kick's automation lanes renamed, and one left: either could be it.
        let dir = project(&[
            ("song.ts", &song("kick", "lane.control(\"env\", \"unit01\", 0);")),
            ("data/kick/hits.json", NOTES),
            ("data/kick/sweep.json", CONTROL),
            ("data/kick/wobble.json", CONTROL),
        ]);
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<unit01>"), ("kick", "wobble", "control<unit01>")]));
        assert_eq!(relinks(about(&w, "data/kick/sweep.json")), [("kick", "env", false)]);
        assert_eq!(relinks(about(&w, "data/kick/wobble.json")), [("kick", "env", false)]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn while_its_track_is_there_another_tracks_lane_is_only_a_choice() {
        // Kick's lane renamed from hits to beats; Clap has a hits lane with no data yet.
        let dir = project(&[("song.ts", &tracks(&["kick.beats", "clap"])), ("data/kick/hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("kick", "beats", true), ("clap", "hits", false)]);
        assert_eq!(
            w[0].message,
            "Notes on kick › hits aren't playing. kick has no lane called hits any more. Did you rename it? Move them to kick › beats \
             to hear them again, or delete them."
        );
        // Kick's lanes all have data: Clap's lane is still only a choice.
        std::fs::write(dir.join("data/kick/beats.json"), NOTES).unwrap();
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("clap", "hits", false)]);
        assert!(w[0].message.ends_with("Move them to the lane they belong to now (clap › hits), or delete them."), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_track_that_was_there_beside_the_data_is_not_its_track_renamed() {
        // Kick is gone; Clap has a hits lane with no data yet.
        let dir = project(&[("song.ts", &tracks(&["clap"])), ("data/kick/hits.json", NOTES)]);
        let just_opened = compiled(&dir);
        assert_eq!(
            relinks(&just_opened[0]),
            [("clap", "", true), ("clap", "hits", false)],
            "just opened, Kick may have been renamed to Clap"
        );
        // Seen without Clap: Kick may have been renamed to it.
        assert_eq!(relinks(&seeing(&dir, &seen(&[("kick", "hits", "events<note>")]))[0]), [("clap", "", true), ("clap", "hits", false)]);
        // Seen beside Clap: Kick was deleted, and Clap is another track.
        let w = seeing(&dir, &seen(&[("kick", "hits", "events<note>"), ("clap", "hits", "events<note>")]));
        assert_eq!(relinks(&w[0]), [("clap", "", false), ("clap", "hits", false)]);
        assert!(
            w[0].message.ends_with("Move its data to clap, or its files one at a time where they belong now, or delete it."),
            "{}",
            w[0].message
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn automation_goes_only_to_lanes_whose_unit_can_hold_it() {
        let cutoff = r#"{"clips": [{"start": 0, "length": 16, "points": [[0, 300], [16, 2200]]}]}"#;
        let dir = project(&[
            ("song.ts", &song("kick", "lane.mask(\"drop\"); lane.control(\"pitch\", \"semitones\", 0);")),
            ("data/kick/cutoff.json", cutoff),
        ]);
        // Not known as hertz in a project just opened, but no 0..1 lane holds 2200.
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("kick", "pitch", false)]);
        // Known as hertz: not semitones either.
        let w = seeing(&dir, &seen(&[("kick", "cutoff", "control<hz>")]));
        assert_eq!(relinks(&w[0]), []);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn data_no_lane_can_take_says_so() {
        let odd = r#"{"clips": [{"start": 0}]}"#;
        let dir = project(&[("song.ts", &song("kick", "")), ("data/kick/hits.json", NOTES), ("data/snare/hits.json", NOTES)]);
        std::fs::write(dir.join("data/kick/odd.json"), odd).unwrap();
        std::fs::write(dir.join("data/kick/old.json"), r#"{"clips": []}"#).unwrap();
        let w = compiled(&dir);
        let snare = about(&w, "data/snare/hits.json");
        assert_eq!(
            snare.fixes.iter().map(|f| (f.code, &f.tx.ops[..])).collect::<Vec<_>>(),
            [("delete-data", &[Op::Remove { file: tp("data/snare/hits.json") }][..])],
            "kick › hits has its own notes"
        );
        assert_eq!(
            snare.message,
            "Notes on snare › hits aren't playing. There's no track called snare any more. To hear them again, add a track called \
             snare with a lane called hits, or delete them."
        );
        let odd = about(&w, "data/kick/odd.json");
        assert_eq!(
            odd.message,
            "data/kick/odd.json isn't used. kick has no lane called odd any more. It doesn't hold notes, automation or audio clips \
             that a lane can read: fix it by hand, or delete it."
        );
        let old = about(&w, "data/kick/old.json");
        assert_eq!(
            old.message,
            "An empty lane file on kick › old isn't used. kick has no lane called old any more. To use it again, add a lane called old \
             to kick, or delete it."
        );
        std::fs::remove_dir_all(dir).ok();
    }
}

//! Tracks: add one, duplicate one, delete one (WRITE-10, ID-2, ID-18). Each is one Tx, so one undo
//! step, built on the accepted revision and asserted on the checked candidate (WRITE-9).
//!
//! - **Add** writes `tracks/<key>.ts` playing an instrument from a new lane, imports it in the
//!   song's file, gives it the mixer's last channel, writes the lane's empty file, and puts it in
//!   `layout.json` with the first colour no track uses.
//! - **Duplicate** copies a track's file (or its statement, when it is written in the song's
//!   file), its lanes with fresh clip ids (FMT-12), its plugin states byte for byte, and its channel
//!   with whatever the channel does to it (a duck stays a duck).
//! - **Delete** takes out its file, every import of it, its channel, its data and its layout
//!   entry; a read of its exports elsewhere is taken out with it only when that read is an effect's
//!   sidechain the effect can lose (`duck(bass.out, kick.env)` → `bass.out`), and only when asked.
//!
//! The channel goes last, so no other channel's place changes. The Tx declares the whole mixer
//! call: the new strip's nodes are std's, made inside the call, and have no other place in the
//! project (design-8b §3.1). The planner's assertion takes up what that admits beyond the gesture.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use m2_ir::Module;
use m2_script::sites::Site;
use m2_session::{Checked, Revision, Session};
use m2_song::tx::{Confined, Declared, Expect, ModuleChange, Op, Payload, Range, Tx, TxPath};
use m2_song::{Content, DataPath, Key, LaneAddr, LaneData, ROOT};
use serde::Serialize;

use crate::layout::{self, Change, Read};
use crate::mix::{self, Mixer, Mixing};
use crate::plan::{self, path, unmoved};
use crate::rename::{name_edit, track_call};
use crate::sites::{self, camel, free_binding, identifier, import_place, imports_of, specifier, statements, whole_lines};
use crate::{EditError, Edited, keys, ready};

/// An instrument a new track can play (PKG-7): what the + Track list shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Instrument {
    /// The ComponentId (`std:synth-pluck`): what adding names.
    pub component: String,
    pub label: String,
    /// Where it is imported from and as what: `pluck` from `std/instruments`, or a member of an
    /// exported object, `synth.pad` from `std/synth`.
    pub name: String,
    pub module: String,
    pub description: String,
    /// A free string the list groups by: std's are Drums and Instruments.
    pub category: String,
    /// The lane a new track plays it from: `hits` for drums, as the fixtures name them, else `notes`.
    pub lane: String,
}

/// Every instrument the packages and the project offer: each that a track can play and that has a
/// category (PKG-7). Drums first, then Instruments, then any other category, each by where it is
/// declared. One with no category is left out: code can still play it.
pub fn instruments(s: &Session) -> Vec<Instrument> {
    let catalog = s.catalog();
    let mut found: Vec<(u8, String, u32, Instrument)> = catalog
        .instruments()
        .filter_map(|d| {
            let category = d.editor.category.clone()?;
            // A plain export when there is one, else a member of an exported object (`synth.pad`).
            let exports = catalog.exports.get(&d.id)?;
            let export = exports.iter().find(|e| !e.name.contains('.')).or_else(|| exports.first())?;
            let lane = if category == "Drums" { "hits" } else { "notes" };
            let line = d.at.rsplit(':').nth(1).and_then(|l| l.parse::<u32>().ok()).unwrap_or(0);
            let group = match category.as_str() {
                "Drums" => 0,
                "Instruments" => 1,
                _ => 2,
            };
            let i = Instrument {
                component: d.id.clone(),
                label: d.label.clone(),
                name: export.name.clone(),
                module: export.module.clone(),
                description: d.editor.description.clone(),
                category,
                lane: lane.into(),
            };
            Some((group, d.file.clone(), line, i))
        })
        .collect();
    found.sort_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)));
    found.into_iter().map(|f| f.3).collect()
}

/// A track a builder made: its key and what it is called.
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    pub module: String,
    pub label: String,
    pub edited: Edited,
    /// A copy whose original reaches no channel got a plain one of its own.
    pub new_channel: bool,
}

/// What a track's code reads of another, outside its channel: where, and whether deleting it can
/// take the read out with it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Dependent {
    /// The track the read is in: the reading module, or for a read in the song's own code, the
    /// track of the channel it sits in.
    pub module: String,
    pub label: String,
    /// The call that reads it, by its component's label (`Duck`), or its callee.
    pub component: String,
    pub file: String,
    pub line: u32,
    /// An effect's other input, which the effect can lose (`duck(bass.out, kick.env)`).
    pub removable: bool,
    #[serde(skip)]
    site: Option<(u32, u32, u32, u32)>,
}

/// What a delete did, or why it waits.
#[derive(Clone, Debug, PartialEq)]
pub enum Deleted {
    /// Gone; with the calls taken out with it.
    Done { edited: Edited, excised: Vec<Dependent> },
    /// Other code reads it in calls that can lose it: delete again with `cascade` to take them out
    /// too.
    Asks(Vec<Dependent>),
    /// Other code reads it in a way that can't be taken out from here.
    Blocked(Vec<Dependent>),
}

/// The modules' keys, the mixer's channel keys and the folders and track files taken, in any
/// capitals: what a new key must not be (ID-2). A new track's key is its new channel's (ID-18), and
/// a channel's key may differ from its track's. An unclaimed `data/<k>/` folder counts, so a new
/// track never takes over a deleted one's data.
fn taken(rev: &Revision) -> impl Fn(&str) -> bool + '_ {
    let channels = channel_keys(rev);
    move |k: &str| {
        let Ok(key) = Key::new(k) else { return true };
        k == ROOT
            || rev.compiled.ir.modules.iter().any(|m| m.key == k)
            || channels.iter().any(|c| c.eq_ignore_ascii_case(k))
            || rev.snapshot.taken(&DataPath::Module { module: key }.path()).is_some()
            || rev.snapshot.taken(Path::new(&format!("tracks/{k}.ts"))).is_some()
    }
}

/// The keys of the song's mixer's channels.
fn channel_keys(rev: &Revision) -> Vec<String> {
    match mix::mixer(rev) {
        Mixing::One(m) => m.channels.into_iter().map(|c| c.key).collect(),
        _ => vec![],
    }
}

/// `label`, or `label 2`, `label 3`… so no track shows the same name.
fn unique_label(rev: &Revision, label: &str) -> String {
    let used: Vec<&str> = rev.compiled.ir.modules.iter().filter(|m| m.track).map(|m| m.label()).collect();
    if !used.contains(&label) {
        return label.to_string();
    }
    (2..).map(|n| format!("{label} {n}")).find(|l| !used.contains(&l.as_str())).expect("a free name")
}

/// A track file's path for `key`: `tracks/<key>.ts`, or `tracks/<key>-2.ts`… when that name is taken
/// in any capitals.
fn track_file(rev: &Revision, key: &str) -> String {
    let free = |p: &str| rev.snapshot.taken(Path::new(p)).is_none();
    let first = format!("tracks/{key}.ts");
    if free(&first) {
        return first;
    }
    (2..).map(|n| format!("tracks/{key}-{n}.ts")).find(|p| free(p)).expect("a free file")
}

/// `s` as a TypeScript string.
fn quoted(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

/// A channel key as a property name: quoted when it isn't an identifier (`"lead-vox"`).
fn prop_key(key: &str) -> String {
    if identifier(key) { key.to_string() } else { quoted(key) }
}

/// The edits that make `entry` (`pluck: pluck.out`) the last property of the mixer's object,
/// in the object's style: on a line of its own at the properties' indentation when they are on
/// lines, with a trailing comma when they have one; inline otherwise. In an object that holds only
/// comments (the Empty template), before the closing brace at the comments' indentation.
fn channel_edits(text: &str, mixer: &Mixer, entry: &str) -> Vec<(u32, u32, String)> {
    let (open, close) = (mixer.object.0 as usize, mixer.object.1 as usize - 1);
    let inner = &text[open + 1..close];
    let Some(last) = mixer.channels.last() else {
        if !inner.contains('\n') {
            return vec![(open as u32, close as u32 + 1, format!("{{ {entry} }}"))];
        }
        // The closing brace's line, and the first line inside with something on it.
        let brace_line = text[..close].rfind('\n').map_or(0, |i| i + 1);
        let brace_indent: String = text[brace_line..close].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let indent = inner
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>())
            .unwrap_or_else(|| format!("{brace_indent}  "));
        if text[brace_line..close].trim().is_empty() {
            return vec![(brace_line as u32, brace_line as u32, format!("{indent}{entry},\n"))];
        }
        return vec![(close as u32, close as u32, format!("\n{indent}{entry},\n{brace_indent}"))];
    };
    let end = last.property.end as usize;
    let rest = &text[end..close];
    let comma = rest.trim_start().starts_with(',');
    let lined = text[open + 1..mixer.channels[0].property.start as usize].contains('\n');
    if !lined {
        return match comma {
            true => {
                let at = end + rest.find(',').expect("a comma") + 1;
                vec![(at as u32, at as u32, format!(" {entry},"))]
            }
            false => vec![(end as u32, end as u32, format!(", {entry}"))],
        };
    }
    let line_start = text[..last.property.start as usize].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    // After the last property's line, its own comment included.
    let line_end = (end + text[end..].find('\n').unwrap_or(text.len() - end)).min(close);
    let line_end = if line_end == close { end + rest.find(|c: char| !c.is_whitespace() && c != ',').unwrap_or(0) } else { line_end };
    match comma {
        true => vec![(line_end as u32, line_end as u32, format!("\n{indent}{entry},"))],
        false => vec![(end as u32, end as u32, ",".into()), (line_end as u32, line_end as u32, format!("\n{indent}{entry}"))],
    }
}

/// Where a default import of `target` goes in `file`'s `text`, and its text.
fn import_edit(text: &str, file: &str, target: &str, binding: &str) -> (u32, String) {
    let line = format!("import {binding} from {};", quoted(&specifier(file, target)));
    match import_place(text, file, target) {
        (at, false) => (at, format!("\n{line}")),
        (at, true) => (at, format!("{line}\n")),
    }
}

/// Merge `edits` of one file, sorted; overlapping ones are a planner error.
fn sorted(mut edits: Vec<(u32, u32, String)>) -> Result<Vec<(u32, u32, String)>, EditError> {
    edits.sort_by_key(|e| (e.0, e.1));
    for w in edits.windows(2) {
        if w[0].1 > w[1].0 {
            return Err(EditError::Rejected("the edits this needs overlap; change it in code".into()));
        }
    }
    Ok(edits)
}

/// The `track(…)` call a new track file makes, playing `inst` from its lane: also checked as a
/// call form (PKG-11, `planned_call_forms_are_current`).
pub(crate) fn track_call_text(inst: &Instrument, key: &str, label: &str) -> String {
    let options = if label == key { String::new() } else { format!(", {{ name: {} }}", quoted(label)) };
    format!("track({}{options}, ({{ lane }}) => {}(lane.notes({})))", quoted(key), inst.name, quoted(&inst.lane))
}

/// What a track file imports to play `inst`: `pluck`, or for `synth.pad` the object, `synth`.
pub(crate) fn instrument_binding(inst: &Instrument) -> &str {
    inst.name.split('.').next().unwrap_or(&inst.name)
}

/// The text of a new track file playing `inst` from lane `lane`.
fn track_text(inst: &Instrument, key: &str, label: &str, file: &str) -> String {
    // A project's own instrument is imported by its path from the track's folder.
    let from = match inst.module.ends_with(".ts") {
        true => specifier(file, &inst.module),
        false => inst.module.clone(),
    };
    format!(
        "import {{ track }} from \"@m2/core\";\nimport {{ {} }} from {};\n\nexport default {};\n",
        instrument_binding(inst),
        quoted(&from),
        track_call_text(inst, key, label)
    )
}

/// The mixer, or why a track can't be added to it.
fn the_mixer(rev: &Revision) -> Result<Mixer, EditError> {
    match mix::mixer(rev) {
        Mixing::One(m) => Ok(m),
        other => Err(EditError::Rejected(other.refusal().expect("not one mixer"))),
    }
}

/// The layout after a builder's change, as the op that writes it; none while `layout.json` doesn't
/// read, which the track builders leave as it is (R15).
fn layout_write(
    s: &Session,
    change: impl FnOnce(&layout::LayoutView) -> (Vec<String>, BTreeMap<String, &'static str>, Option<String>),
) -> Option<(Op, TxPath, m2_song::tx::Base)> {
    let read = layout::read_disk(&s.accepted().project.dir);
    if matches!(read, Read::Invalid(_)) {
        return None;
    }
    let view = layout::effective(&layout::default_order(s), &read);
    let (order, colours, gone) = change(&view);
    let text = layout::text_after(&read, &Change { order, colours: &colours, gone: gone.as_deref() })?;
    Some(layout::write_op(s, text))
}

/// `order` with `key` right after `after` (when it is listed), else at the end.
fn placed(order: &[String], key: &str, after: Option<&str>) -> Vec<String> {
    let mut out = order.to_vec();
    match after.and_then(|a| out.iter().position(|k| k == a)) {
        Some(i) => out.insert(i + 1, key.to_string()),
        None => out.push(key.to_string()),
    }
    out
}

/// A planned Tx with `layout` written beside its ops, based on the disk.
fn with_layout(
    s: &Session,
    label: (String, String),
    mut ops: Vec<Op>,
    expect: Expect,
    layout: Option<(Op, TxPath, m2_song::tx::Base)>,
) -> Tx {
    let base = layout.map(|(op, file, base)| {
        ops.push(op);
        (file, base)
    });
    let mut tx = plan::planned(s, label, ops, expect);
    if let Some((file, base)) = base {
        tx.base.insert(file, base);
    }
    tx
}

/// The planner's assertion shared by add and duplicate: exactly one module added, nothing of the
/// song removed but an empty mixer's stand-in, no value changed, and the new track in a channel
/// of its own (ID-18).
fn adds_one(checked: &Checked, key: &str, channel: &str) -> Result<(), EditError> {
    let d = &checked.delta;
    let refused = |why: &str| EditError::Rejected(format!("adding the track would {why}"));
    let k = Key::new(key).map_err(EditError::Rejected)?;
    if d.modules != [ModuleChange::Added(k)] {
        return Err(refused("change other tracks too"));
    }
    if !d.values.is_empty() {
        return Err(refused("change a value"));
    }
    // An empty mixer's silent stand-in (`scale(sine(0), 0)` in std's mix.ts) goes with its first channel.
    let stand_in = |op: &serde_json::Value| {
        let op = op.to_string();
        op.contains("std:sine") || op.contains("std:scale") || op.contains("const")
    };
    if d.removed.iter().any(|n| n.name.first().map(String::as_str) != Some(ROOT) || !stand_in(&n.op)) {
        return Err(refused("take something out of the song"));
    }
    match mix::mixer(checked.candidate()) {
        Mixing::One(m) if m.channel_of(key).is_some_and(|c| c.key == channel) => Ok(()),
        _ => Err(refused("not reach the mixer through its own channel")),
    }
}

/// Add a track playing instrument `component` (a ComponentId), shown as `label` (else the
/// instrument's), keyed `key` (else one proposed from the label, ID-2), placed after track
/// `after` in the layout (else last).
pub fn add(s: &mut Session, component: &str, label: Option<&str>, key: Option<&str>, after: Option<&str>) -> Result<Made, EditError> {
    ready(s)?;
    let inst = instruments(s).into_iter().find(|i| i.component == component).ok_or_else(|| EditError::NotFound(component.to_string()))?;
    let rev = s.accepted();
    let mixer = the_mixer(rev)?;
    let label = match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if l.chars().any(char::is_control) => return Err(EditError::Rejected("a name is one line of text".into())),
        Some(l) => unique_label(rev, l),
        None => unique_label(rev, &inst.label),
    };
    let taken = taken(rev);
    let key = match key {
        Some(k) if !Key::valid(k) => {
            return Err(EditError::Rejected(format!(
                "{k} isn't an ID: IDs are lowercase letters, digits and hyphens, such as {}",
                keys::propose(k, &taken).chars().take(32).collect::<String>().trim_matches('-')
            )));
        }
        Some(k) if taken(k) => {
            let track = rev.compiled.ir.modules.iter().any(|m| m.key == k);
            return Err(EditError::Rejected(match channel_keys(rev).iter().any(|c| c.eq_ignore_ascii_case(k)) && !track {
                true => format!("The mixer already has a channel called {k}: pick another ID"),
                false => format!("{k} is taken: another track or its data has that ID"),
            }));
        }
        Some(k) => k.to_string(),
        None => match keys::propose(&label, &taken) {
            k if k.is_empty() => keys::propose("track", &taken),
            k => k,
        },
    };
    let file = track_file(rev, &key);
    let song_text = rev.snapshot.files[Path::new(&mixer.file)].clone();
    let binding = free_binding(&song_text, &camel(&key));
    let entry = format!("{}: {binding}.out", prop_key(&key));
    let (import_at, import) = import_edit(&song_text, &mixer.file, &file, &binding);
    let mut edits = channel_edits(&song_text, &mixer, &entry);
    edits.push((import_at, import_at, import));
    let edits = sorted(edits)?;

    let k = Key::new(&key).map_err(EditError::Rejected)?;
    let lane = LaneAddr { module: k.clone(), lane: Key::new(&inst.lane).map_err(EditError::Rejected)? };
    let empty = m2_song::canonical(&LaneData::Notes(vec![]));
    let song_file = path(&mixer.file)?;
    let new_file = path(&file)?;
    let ops = vec![
        Op::Write { file: new_file.clone(), bytes: Payload::Text(track_text(&inst, &key, &label, &file).into()) },
        plan::text_edit(&song_file, &song_text, &edits),
        Op::Write { file: TxPath::lane(&lane), bytes: Payload::Text(empty.into()) },
    ];
    let ranges = vec![Range::bytes(import_at, import_at), Range::bytes(mixer.call.0, mixer.call.1)];
    let expect = Expect::Confined(Confined {
        ranges: [(new_file, Declared::Whole), (song_file, Declared::Ranges(ranges))].into(),
        lanes: vec![lane],
        modules: vec![ModuleChange::Added(k)],
    });
    let layout = layout_write(s, |v| {
        let mut colours = v.colours.clone();
        colours.insert(key.clone(), layout::unused(v.colours.values().copied()));
        (placed(&v.order, &key, after), colours, None)
    });
    drop(taken);
    let tx = with_layout(s, (String::new(), label.clone()), ops, expect, layout);
    let said = |why: &str| EditError::Rejected(format!("adding {label} would change more than adding a track ({why})"));
    let channel = key.clone();
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        adds_one(checked, &key, &channel)
    })?;
    let edited = Edited { revision: done.number, file, before: String::new(), after: label.clone() };
    Ok(Made { module: key, label, edited, new_channel: false })
}

/// The top-level statement of `text` that holds bytes `at..end`.
fn statement_at(text: &str, at: u32, end: u32) -> Option<(u32, u32)> {
    statements(text).into_iter().find(|(s, e, _)| *s <= at && end <= *e).map(|(s, e, _)| (s, e))
}

/// Whether `text` holds nothing but imports and the statement at `stmt` (and `export default`
/// of its binding).
fn only(text: &str, stmt: (u32, u32), binding: Option<&str>) -> bool {
    statements(text).into_iter().all(|(s, e, import)| {
        let t = text[s as usize..e as usize].trim_end_matches(';').trim();
        import || (s, e) == stmt || binding.is_some_and(|b| t == format!("export default {b}"))
    })
}

/// The track call of module `m` and its file's text, or why a builder can't edit it.
fn call_of<'a>(rev: &'a Revision, m: &Module, doing: &str) -> Result<(&'a Site, Arc<str>), EditError> {
    let site = track_call(&rev.compiled.sites, &rev.snapshot.files, m)
        .ok_or_else(|| EditError::Rejected(format!("{} is made by code that makes several tracks, so {doing} it in code.", m.label())))?;
    let text = rev.snapshot.files.get(Path::new(&site.file)).cloned().ok_or_else(|| EditError::NotFound(site.file.clone()))?;
    Ok((site, text))
}

/// The innermost call of the project at byte `at` of `file`.
fn call_around<'a>(rev: &'a Revision, file: &str, at: u32) -> Option<&'a Site> {
    rev.compiled.sites.iter().filter(|x| x.in_project && x.file == file && x.start <= at && at < x.end).min_by_key(|x| x.end - x.start)
}

/// The bytes taking channel `c` out of `mixer`'s object removes: on a line of its own, the line,
/// its trailing comma and comment, and the comment lines right above it with no blank line
/// between (`// Sidechain: …`); inline, the property and the comma that joins it to the others.
fn channel_removal(t: &str, mixer: &Mixer, c: &mix::Channel) -> (u32, u32) {
    let p = &c.property;
    let after = &t[p.end as usize..];
    let spaces = after.len() - after.trim_start_matches([' ', '\t']).len();
    let comma = after[spaces..].starts_with(',');
    let end = p.end + if comma { spaces as u32 + 1 } else { 0 };
    let (mut a, b) = whole_lines(t, p.start, end);
    if (a, b) != (p.start, end) {
        loop {
            let prev_end = (a as usize).saturating_sub(1);
            let prev_start = t[..prev_end].rfind('\n').map_or(0, |i| i + 1);
            if a == 0 || !t[prev_start..prev_end].trim_start().starts_with("//") {
                break;
            }
            a = prev_start as u32;
        }
        return (a, b);
    }
    let i = mixer.channels.iter().position(|x| x.key == c.key).expect("its channel");
    match (comma, i.checked_sub(1).map(|j| &mixer.channels[j])) {
        (true, _) => {
            let rest = &t[end as usize..];
            (p.start, end + (rest.len() - rest.trim_start_matches([' ', '\t']).len()) as u32)
        }
        (false, Some(prev)) => (prev.property.end, p.end),
        (false, None) => (p.start, p.end),
    }
}

/// The component call `site` of the project is, when it takes audio first and makes audio: an
/// effect (`duck`, categorized or not), which can lose its other inputs and leave its audio, as
/// removing an effect does.
fn passes_audio(s: &Session, site: &Site) -> Option<Arc<m2_script::Descriptor>> {
    let compiled = &s.accepted().compiled;
    let mut ids = compiled.ir.modules.iter().flat_map(|m| &m.frames).filter(|f| f.site == site.id).map(|f| f.component.as_deref());
    let id = ids.next()??;
    if ids.any(|x| x != Some(id)) {
        return None;
    }
    let d = compiled.components.get(id)?;
    let audio = |k: Option<m2_script::component::PortKind>| k == Some(m2_script::component::PortKind::Audio);
    (site.in_project && audio(d.inputs.first().map(|i| i.kind)) && audio(d.output.as_ref().map(|o| o.kind))).then(|| d.clone())
}

/// Take out track `module`: its code, every import of it, its channel, its data and its layout
/// entry. Reads of it elsewhere that an effect can lose are taken out too with `cascade`;
/// without, the answer lists them.
pub fn delete(s: &mut Session, module: &str, cascade: bool) -> Result<Deleted, EditError> {
    ready(s)?;
    if module == ROOT {
        return Err(EditError::Rejected("the song itself can't be deleted".into()));
    }
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    if !m.track {
        return Err(EditError::Rejected(format!("{module} isn't a track")));
    }
    let label = m.label().to_string();
    let (site, text) = call_of(rev, m, "delete")?;
    let stmt =
        statement_at(&text, site.start, site.end).ok_or_else(|| EditError::Rejected(format!("{label}'s code isn't one statement")))?;
    let song_file = rev
        .compiled
        .ir
        .modules
        .iter()
        .find(|x| x.key == ROOT)
        .and_then(|r| r.src.as_deref())
        .and_then(|s| s.rsplitn(3, ':').nth(2))
        .unwrap_or("song.ts")
        .to_string();
    // A file of its own goes; one written in the song's file is its statement.
    let whole = site.file != song_file && only(&text, stmt, None);
    if site.file != song_file && !whole {
        return Err(EditError::Rejected(format!("{}'s file has more in it than {label}, so delete it in code.", site.file)));
    }
    let mixer = match mix::mixer(rev) {
        Mixing::One(m) => Some(m),
        _ => None,
    };
    let channel = mixer.as_ref().and_then(|mx| mx.channel_of(module)).cloned();
    if let Some(c) = channel.as_ref().filter(|c| c.tracks.len() > 1) {
        return Err(EditError::Rejected(format!("{label} shares its mixer channel {} with other tracks, so delete it in code.", c.key)));
    }
    let in_channel = |file: &str, at: u32| {
        channel
            .as_ref()
            .is_some_and(|c| Some(file) == mixer.as_ref().map(|m| m.file.as_str()) && c.property.start <= at && at < c.property.end)
    };
    let in_own = |file: &str, at: u32| file == site.file && stmt.0 <= at && at < stmt.1;

    // The names the project reads it by: each default import of its file, or in the song's own
    // file, the binding its statement declares. The build shares one import node between reads,
    // so the reads are found by name in the code, each one.
    let mut readers: Vec<(String, String)> = vec![];
    if whole {
        for (p, t) in rev.snapshot.files.iter() {
            let Some(f) = p.to_str().filter(|_| matches!(DataPath::parse(p), DataPath::Code(_))) else { continue };
            readers.extend(imports_of(t, f, &site.file).into_iter().filter_map(|i| i.default).map(|b| (f.to_string(), b)));
        }
    } else {
        let b = site.binding.clone().ok_or_else(|| EditError::Rejected(format!("{label} isn't bound to a name, so delete it in code.")))?;
        readers.push((site.file.clone(), b));
    }
    // Every other read of it, each judged.
    let mut dependents: Vec<Dependent> = vec![];
    let reads = readers.iter().flat_map(|(file, name)| {
        let t = rev.snapshot.files.get(Path::new(file)).map_or("", |t| &**t);
        sites::references(t, name).into_iter().map(move |at| (file.clone(), at))
    });
    for (file, at) in reads.collect::<Vec<_>>() {
        if in_channel(&file, at) || (!whole && in_own(&file, at)) {
            continue;
        }
        // The module whose code it is in.
        let reader = rev
            .compiled
            .ir
            .modules
            .iter()
            .find(|x| x.src.as_deref().and_then(|s| s.rsplitn(3, ':').nth(2)) == Some(file.as_str()))
            .map_or(ROOT, |x| x.key.as_str());
        let call = call_around(rev, &file, at);
        let line = rev.snapshot.files.get(Path::new(&file)).map_or(0, |t| t[..at as usize].matches('\n').count() as u32 + 1);
        let effect = call.and_then(|c| passes_audio(s, c).map(|d| (c, d)));
        // The track a read in the song's own code feeds: the channel it sits in.
        let fed = match (reader == ROOT, &mixer) {
            (true, Some(mx)) => mx
                .channels
                .iter()
                .find(|c| mx.file == file && c.property.start <= at && at < c.property.end)
                .and_then(|c| c.tracks.first().cloned()),
            _ => None,
        };
        let fed = fed.unwrap_or_else(|| reader.to_string());
        let fed_label = rev.compiled.ir.modules.iter().find(|x| x.key == fed).map_or(fed.clone(), |x| x.label().to_string());
        let dep = match effect {
            Some((c, d)) if c.args.first().is_some_and(|a| !(a.span().0 <= at && at < a.span().1)) => {
                let input = c.args[0].span();
                Dependent {
                    module: fed,
                    label: fed_label,
                    component: d.label.clone(),
                    file,
                    line,
                    removable: true,
                    site: Some((c.start, input.0, input.1, c.end)),
                }
            }
            _ => Dependent {
                module: fed,
                label: fed_label,
                component: call.map_or_else(|| "its code".to_string(), |c| c.callee.clone()),
                file,
                line,
                removable: false,
                site: None,
            },
        };
        if !dependents.iter().any(|d| d.site.is_some() && d.site == dep.site && d.file == dep.file) {
            dependents.push(dep);
        }
    }
    if dependents.iter().any(|d| !d.removable) {
        return Ok(Deleted::Blocked(dependents.into_iter().filter(|d| !d.removable).collect()));
    }
    if !dependents.is_empty() && !cascade {
        return Ok(Deleted::Asks(dependents));
    }

    // The edits, file by file, and what each may change.
    let mut edits: BTreeMap<String, Vec<(u32, u32, String)>> = BTreeMap::new();
    let mut ranges: BTreeMap<String, Vec<Range>> = BTreeMap::new();
    fn add(
        edits: &mut BTreeMap<String, Vec<(u32, u32, String)>>,
        ranges: &mut BTreeMap<String, Vec<Range>>,
        file: &str,
        (a, b): (u32, u32),
        declared: (u32, u32),
    ) {
        edits.entry(file.to_string()).or_default().push((a, b, String::new()));
        ranges.entry(file.to_string()).or_default().push(Range::bytes(declared.0, declared.1));
    }
    if !whole {
        let (a, b) = whole_lines(&text, stmt.0, stmt.1);
        add(&mut edits, &mut ranges, &site.file, (a, b), (a, b));
    }
    // Every import of its file, in every project file.
    if whole {
        for (p, t) in rev.snapshot.files.iter() {
            let Some(f) = p.to_str().filter(|_| matches!(DataPath::parse(p), DataPath::Code(_))) else { continue };
            for i in imports_of(t, f, &site.file) {
                if i.named {
                    return Err(EditError::Rejected(format!("{f} imports more than {label} from {}, so delete it in code.", site.file)));
                }
                let (a, b) = whole_lines(t, i.start, i.end);
                add(&mut edits, &mut ranges, f, (a, b), (a, b));
            }
        }
    }
    if let (Some(mx), Some(c)) = (&mixer, &channel) {
        let t = &rev.snapshot.files[Path::new(&mx.file)];
        let removed = channel_removal(t, mx, c);
        add(&mut edits, &mut ranges, &mx.file, removed, mx.call);
    }
    for d in &dependents {
        let Some((start, i0, i1, end)) = d.site else { continue };
        let entry = edits.get(&d.file);
        // Inside what goes anyway: nothing more to take out.
        if entry.is_some_and(|e| e.iter().any(|(a, b, _)| *a <= start && end <= *b)) {
            continue;
        }
        add(&mut edits, &mut ranges, &d.file, (start, i0), (start, end));
        add(&mut edits, &mut ranges, &d.file, (i1, end), (start, end));
    }

    // What the edits leave unread goes from the imports too (`duck`, once the last duck is out).
    for (file, e) in edits.clone() {
        let t = rev.snapshot.files[Path::new(&file)].clone();
        let gone: Vec<(u32, u32)> = e.iter().map(|(a, b, _)| (*a, *b)).collect();
        for r in sites::unused_imports(&t, &gone) {
            add(&mut edits, &mut ranges, &file, r, r);
        }
    }
    let mut ops = vec![];
    let mut declared: BTreeMap<TxPath, Declared> = BTreeMap::new();
    if whole {
        let f = path(&site.file)?;
        ops.push(Op::Remove { file: f.clone() });
        declared.insert(f, Declared::Whole);
    }
    for (file, e) in edits {
        let t = rev.snapshot.files[Path::new(&file)].clone();
        let mut e = sorted(e)?;
        e.dedup_by(|b, a| a.0 == b.0 && a.1 == b.1);
        let f = path(&file)?;
        ops.push(plan::text_edit(&f, &t, &e));
        declared.insert(f, Declared::Ranges(ranges.remove(&file).unwrap_or_default()));
    }
    // Its data: every lane and plugin state under its folder. The folder goes once it is empty;
    // another file in it keeps it (stage.rs).
    let k = Key::new(module).map_err(EditError::Rejected)?;
    let folder = DataPath::Module { module: k.clone() }.path();
    for (p, _) in rev.snapshot.under(&folder) {
        if let DataPath::Lane { module: mk, .. } | DataPath::PluginState { module: mk, .. } = DataPath::parse(&p)
            && mk == k
        {
            let file = TxPath::new(&m2_song::path::slashed(&p)).map_err(EditError::Rejected)?;
            ops.push(Op::Remove { file });
        }
    }
    let lanes = m.lanes.iter().filter_map(|l| Some(LaneAddr { module: k.clone(), lane: Key::new(&l.key).ok()? })).collect();
    let expect = Expect::Confined(Confined { ranges: declared, lanes, modules: vec![ModuleChange::Removed(k.clone())] });
    let gone = module.to_string();
    let layout = layout_write(s, |v| {
        let order = v.order.iter().filter(|x| **x != gone).cloned().collect();
        let mut colours = v.colours.clone();
        colours.remove(&gone);
        (order, colours, Some(gone.clone()))
    });
    let tx = with_layout(s, (label.clone(), String::new()), ops, expect, layout);
    let before: Vec<(String, Vec<String>)> = mixer.as_ref().map_or(vec![], |mx| {
        mx.channels.iter().filter(|c| Some(&c.key) != channel.as_ref().map(|c| &c.key)).map(|c| (c.key.clone(), c.tracks.clone())).collect()
    });
    let said = |why: &str| EditError::Rejected(format!("deleting {label} would change more than taking it out ({why})"));
    let file = site.file.clone();
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        // The planner's assertion: one module removed, and every other channel as it was, compared
        // by key, since a middle channel's going renumbers the strips after it.
        if checked.delta.modules != [ModuleChange::Removed(k.clone())] {
            return Err(EditError::Rejected(format!("deleting {label} would change other tracks too")));
        }
        let after: Vec<(String, Vec<String>)> = match mix::mixer(checked.candidate()) {
            Mixing::One(mx) => mx.channels.iter().map(|c| (c.key.clone(), c.tracks.clone())).collect(),
            _ => vec![],
        };
        match after == before {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("deleting {label} would change the other mixer channels"))),
        }
    })?;
    let edited = Edited { revision: done.number, file, before: label, after: String::new() };
    Ok(Deleted::Done { edited, excised: dependents })
}

/// A fresh clip id (FMT-11's random input): 8 Crockford characters.
fn fresh_id(taken: &mut Vec<String>) -> String {
    loop {
        let id: String = m2_song::tx::random_id().trim_start_matches("tx-").chars().take(8).collect();
        if !taken.contains(&id) {
            taken.push(id.clone());
            return id;
        }
    }
}

/// `data` with a fresh id on every clip (FMT-12).
fn fresh(data: &LaneData, taken: &mut Vec<String>) -> LaneData {
    let mut d = data.clone();
    match &mut d {
        LaneData::Notes(clips) => clips.iter_mut().for_each(|c| (c.id, c.other_id) = (Some(fresh_id(taken)), None)),
        LaneData::Control { clips, .. } => clips.iter_mut().for_each(|c| (c.id, c.other_id) = (Some(fresh_id(taken)), None)),
        LaneData::Audio(clips) => clips.iter_mut().for_each(|c| (c.id, c.other_id) = (Some(fresh_id(taken)), None)),
    }
    d
}

/// The identifier written at byte `at` of `text`.
fn ident_at(text: &str, at: u32) -> &str {
    let rest = &text[at as usize..];
    &rest[..rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$')).unwrap_or(rest.len())]
}

/// `text` with `edits` (positions in it) applied.
fn applied(text: &str, edits: &[(u32, u32, String)]) -> String {
    let mut out = String::new();
    let mut at = 0;
    for (a, b, t) in edits {
        out.push_str(&text[at..*a as usize]);
        out.push_str(t);
        at = *b as usize;
    }
    out.push_str(&text[at..]);
    out
}

/// Duplicate track `module`: its code, its data with new clip ids, its plugin states, and its
/// channel, right below it in the same colour.
pub fn duplicate(s: &mut Session, module: &str) -> Result<Made, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let m = rev.compiled.ir.modules.iter().find(|m| m.key == module && m.track).ok_or_else(|| EditError::NotFound(module.to_string()))?;
    let label0 = m.label().to_string();
    let mixer = the_mixer(rev)?;
    let (site, text) = call_of(rev, m, "duplicate")?;
    let stmt =
        statement_at(&text, site.start, site.end).ok_or_else(|| EditError::Rejected(format!("{label0}'s code isn't one statement")))?;
    let taken = taken(rev);
    let key = keys::propose(module, &taken);
    let label = unique_label(rev, &label0);
    // The call with its key and name changed, as edits of `text`.
    let mut call_edits = vec![(site.args[0].span().0, site.args[0].span().1, quoted(&key))];
    let named = name_edit(&text, site, &key, &label).ok_or_else(|| {
        EditError::Rejected(format!("{label0}'s track() call isn't written with its key and options in the code, so duplicate it in code."))
    })?;
    // The copy's name: written even when it equals its new key's default, so it shows as asked.
    match named {
        Some(e) => call_edits.push(e),
        None if label != key => {
            call_edits.push((site.args[0].span().1, site.args[0].span().1, format!(", {{ name: {} }}", quoted(&label))))
        }
        None => {}
    }
    let mut call_edits = sorted(call_edits)?;
    let song_text = rev.snapshot.files[Path::new(&mixer.file)].clone();
    let in_song = site.file == mixer.file;
    let mut song_edits: Vec<(u32, u32, String)> = vec![];
    let mut song_ranges: Vec<Range> = vec![Range::bytes(mixer.call.0, mixer.call.1)];
    let mut ops: Vec<Op> = vec![];
    let mut declared: BTreeMap<TxPath, Declared> = BTreeMap::new();
    let (binding, file) = if !in_song {
        if !only(&text, stmt, None) {
            return Err(EditError::Rejected(format!("{}'s file has more in it than {label0}, so duplicate it in code.", site.file)));
        }
        let file = track_file(rev, &key);
        let binding = free_binding(&song_text, &camel(&key));
        let f = path(&file)?;
        ops.push(Op::Write { file: f.clone(), bytes: Payload::Text(applied(&text, &call_edits).into()) });
        declared.insert(f, Declared::Whole);
        let (at, line) = import_edit(&song_text, &mixer.file, &file, &binding);
        song_edits.push((at, at, line));
        song_ranges.push(Range::bytes(at, at));
        (binding, file)
    } else {
        // Written in the song's file: its statement again, right after itself, with a free binding.
        let old =
            site.binding.clone().ok_or_else(|| EditError::Rejected(format!("{label0} isn't bound to a name, so duplicate it in code.")))?;
        let binding = free_binding(&song_text, &camel(&key));
        let decl = &text[stmt.0 as usize..site.start as usize];
        let at = decl.rfind(&old).ok_or_else(|| EditError::Rejected(format!("{label0}'s statement isn't `const {old} = track(…)`")))?
            as u32
            + stmt.0;
        call_edits.push((at, at + old.len() as u32, binding.clone()));
        call_edits = sorted(call_edits)?;
        let local: Vec<(u32, u32, String)> = call_edits.iter().map(|(a, b, t)| (a - stmt.0, b - stmt.0, t.clone())).collect();
        let copy = applied(&text[stmt.0 as usize..stmt.1 as usize], &local);
        song_edits.push((stmt.1, stmt.1, format!("\n{copy}")));
        song_ranges.push(Range::bytes(stmt.1, stmt.1));
        (binding, mixer.file.clone())
    };
    // The channel, copied with the copy's reads in place of the track's; a plain one when the
    // track reaches none.
    let (entry, plain) = match mixer.channel_of(module) {
        // Found by the graph, through a name bound elsewhere (`kick: k`): a copy of its text
        // would play the track, not the copy.
        Some(c) if !c.reads.iter().any(|r| r.module == module) => {
            return Err(EditError::Rejected(format!("{label0}'s mixer channel doesn't name it, so duplicate it in code.")));
        }
        Some(c) if c.tracks.len() == 1 => {
            let p = &c.property;
            let mut e: Vec<(u32, u32, String)> = c
                .reads
                .iter()
                .filter(|r| r.module == module)
                .map(|r| (r.at - p.start, r.at - p.start + ident_at(&song_text, r.at).len() as u32, binding.clone()))
                .collect();
            let key_end = song_text[p.start as usize..p.value.0 as usize].find(':').unwrap_or(0) as u32;
            e.push((0, key_end, prop_key(&key)));
            (applied(&song_text[p.start as usize..p.end as usize], &sorted(e)?), false)
        }
        Some(_) => {
            return Err(EditError::Rejected(format!("{label0} shares its mixer channel with other tracks, so duplicate it in code.")));
        }
        None => (format!("{}: {binding}.out", prop_key(&key)), true),
    };
    song_edits.extend(channel_edits(&song_text, &mixer, &entry));
    let song_edits = sorted(song_edits)?;
    let song_file = path(&mixer.file)?;
    ops.push(plan::text_edit(&song_file, &song_text, &song_edits));
    declared.insert(song_file, Declared::Ranges(song_ranges));

    // The data: lanes with new clip ids, plugin states byte for byte.
    let (k0, k) = (Key::new(module).map_err(EditError::Rejected)?, Key::new(&key).map_err(EditError::Rejected)?);
    let mut ids = vec![];
    let mut lanes = vec![];
    for l in &m.lanes {
        let Some(data) = rev.compiled.data.lane(module, &l.key) else { continue };
        let addr = LaneAddr { module: k.clone(), lane: Key::new(&l.key).map_err(EditError::Rejected)? };
        ops.push(Op::Write { file: TxPath::lane(&addr), bytes: Payload::Text(m2_song::canonical(&fresh(data, &mut ids)).into()) });
        lanes.push(addr);
    }
    for (p, content) in rev.snapshot.under(&DataPath::Module { module: k0.clone() }.path()) {
        let DataPath::PluginState { module: mk, path: frames, sidecar } = DataPath::parse(&p) else { continue };
        if mk != k0 {
            continue;
        }
        let to = TxPath::of(&DataPath::PluginState { module: k.clone(), path: frames, sidecar }).map_err(EditError::Rejected)?;
        let bytes = match content {
            Content::Text(t) => Payload::Text(t),
            Content::Binary(_) => {
                let b = std::fs::read(rev.project.dir.join(&p)).map_err(|e| EditError::Io(format!("reading {}: {e}", p.display())))?;
                Payload::Bytes(b.into())
            }
        };
        ops.push(Op::Write { file: to, bytes });
    }
    let expect = Expect::Confined(Confined { ranges: declared, lanes: lanes.clone(), modules: vec![ModuleChange::Added(k.clone())] });
    let layout = layout_write(s, |v| {
        let mut colours = v.colours.clone();
        // The copy keeps the original's colour, as in Live and Logic.
        let c = v.colours.get(module).copied().unwrap_or_else(|| layout::unused(v.colours.values().copied()));
        colours.insert(key.clone(), c);
        (placed(&v.order, &key, Some(module)), colours, None)
    });
    drop(taken);
    let tx = with_layout(s, (String::new(), label.clone()), ops, expect, layout);
    let said = |why: &str| EditError::Rejected(format!("duplicating {label0} would change more than adding its copy ({why})"));
    let channel = key.clone();
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        adds_one(checked, &key, &channel)?;
        // The copy's graph is the original's but for its key.
        let shape = |rev: &Revision, k: &str| {
            rev.compiled
                .ir
                .modules
                .iter()
                .find(|m| m.key == k)
                .map(|m| (m.nodes.len(), m.lanes.iter().map(|l| l.key.clone()).collect::<Vec<_>>()))
        };
        match shape(checked.candidate(), &key) == shape(checked.baseline(), module) {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("the copy of {label0} wouldn't be the same as it"))),
        }
    })?;
    let edited = Edited { revision: done.number, file, before: String::new(), after: label.clone() };
    Ok(Made { module: key, label, edited, new_channel: plain })
}

#[cfg(test)]
#[path = "tracks_tests.rs"]
mod tests;

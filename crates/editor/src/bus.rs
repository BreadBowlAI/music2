//! Buses (RENDER-34): the edits the Mix view makes. Add a bus (a Reverb, a Delay or a Group), add
//! a send to one or take it out, send before the fader or after it, and send a channel to a group
//! or back to the master. Each is one Tx, so one undo step, built on the accepted revision and
//! asserted on the checked candidate (WRITE-9).
//!
//! All routing is written in the song's one mixer, on its channels (std's `sends` and `to`); a
//! bus's own file holds only its effects. A send names its bus by the bus channel's key, which the
//! app never changes (ID-18), so a rename never breaks one. A send's level is a knob like a fader's:
//! turning it is a value edit, a cell write with no rebuild (`value::set`).
//!
//! A send or an output that would make a loop is refused before anything is written, in words that
//! name the tracks. Who feeds whom is the checker's routes (which track's sound reaches which bus
//! through the song) and each module's reads of another's exports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use m2_script::sites::Arg;
use m2_session::{Checked, Revision, Session};
use m2_song::tx::{Confined, Declared, Expect, ModuleChange, Op, Payload, Range};
use m2_song::{Key, ROOT};

use crate::mix::{self, Channel, Mixer, Mixing};
use crate::plan::{self, path, unmoved};
use crate::sites::{Property, camel, free_binding, imported_names, properties};
use crate::tracks::{self, Made, prop_key, quoted};
use crate::{EditError, Edited, keys, layout, ready};

/// What a new bus does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusKind {
    /// std's reverb, all wet: a room the tracks share.
    Reverb,
    /// std's delay, all wet: echoes the tracks share.
    Delay,
    /// Its input as it is, through a gain at 0 dB (no bit changes): tracks go to it for one fader
    /// and shared effects. The gain is the bus's own node, where mute and solo act (MON-3).
    Group,
}

impl BusKind {
    /// Every kind, as the Add bus menu lists them.
    pub const ALL: [BusKind; 3] = [BusKind::Reverb, BusKind::Delay, BusKind::Group];

    /// The kind a request names: `reverb`, `delay` or `group`.
    pub fn named(word: &str) -> Option<BusKind> {
        match word {
            "reverb" => Some(BusKind::Reverb),
            "delay" => Some(BusKind::Delay),
            "group" => Some(BusKind::Group),
            _ => None,
        }
    }

    /// What a new one is called.
    fn label(self) -> &'static str {
        match self {
            BusKind::Reverb => "Reverb",
            BusKind::Delay => "Delay",
            BusKind::Group => "Group",
        }
    }

    /// What it plays: a component (its ComponentId), by the name its package exports it as, and
    /// what its function returns. A Reverb or a Delay is `@m2/effects`', a pack music2 ships
    /// (PACK-15), added as any pack's item is (PACK-10); a Group is std's gain.
    fn body(self) -> (&'static str, &'static str, &'static str) {
        match self {
            BusKind::Reverb => ("@m2/effects:reverb", "reverb", "reverb(input, { mix: 1 })"),
            BusKind::Delay => ("@m2/effects:delay", "delay", "delay(input, { mix: 1 })"),
            BusKind::Group => ("std:gain", "gain", "gain(input)"),
        }
    }
}

/// The `bus(…)` call a new bus's file makes: also checked as a call form (PKG-11,
/// `planned_call_forms_are_current`).
pub(crate) fn bus_call_text(kind: BusKind, key: &str, label: &str) -> String {
    let options = if label == key { String::new() } else { format!(", {{ name: {} }}", quoted(label)) };
    format!("bus({}{options}, ({{ input }}) => {})", quoted(key), kind.body().2)
}

/// The name a bus file imports to play `kind`.
pub(crate) fn bus_import(kind: BusKind) -> &'static str {
    kind.body().1
}

/// The text of a new bus file, importing what plays `kind` from `module`.
fn bus_text(kind: BusKind, module: &str, key: &str, label: &str) -> String {
    format!(
        "import {{ bus }} from \"@m2/core\";\nimport {{ {} }} from {};\n\nexport default {};\n",
        bus_import(kind),
        quoted(module),
        bus_call_text(kind, key, label)
    )
}

/// The module a bus of `kind` imports what plays it from: where the packages the lists offer export
/// it (PACK-11), as an added effect is imported. None when no package offers it (a pack removed).
fn bus_module(s: &Session, kind: BusKind) -> Option<String> {
    let (component, name, _) = kind.body();
    let catalog = s.catalog();
    catalog.exports.get(component)?.iter().find(|e| e.name == name).map(|e| e.module.clone())
}

/// Add a bus of `kind`, shown as `label` (else the kind's name), placed after track `after` in the
/// layout (else last): `tracks/<key>.ts`, its import in the song, the mixer's last channel
/// (`verb: verb.out`, which std's mixer feeds what is sent to it) and its layout place. One Tx; the
/// planner asserts that exactly that module is added, a bus, in a channel of its own.
pub fn add_bus(s: &mut Session, kind: BusKind, label: Option<&str>, after: Option<&str>) -> Result<Made, EditError> {
    ready(s)?;
    let (component, _, _) = kind.body();
    let module =
        bus_module(s, kind).ok_or_else(|| EditError::Rejected(format!("no package offers {component}: install the pack it comes from")))?;
    // Its pack's [dependencies] line and lock, when the song doesn't have them (PACK-10).
    let needs = crate::packs::needs(s, component)?;
    let rev = s.accepted();
    let mixer = tracks::the_mixer(rev)?;
    let song_text = rev.snapshot.files[Path::new(&mixer.file)].clone();
    let label = match label.map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) if l.chars().any(char::is_control) => return Err(EditError::Rejected("a name is one line of text".into())),
        Some(l) => tracks::unique_label_among(rev, l, &[]),
        None => tracks::unique_label_among(rev, kind.label(), &[]),
    };
    let taken = tracks::taken(rev);
    let key = match keys::propose(&label, &taken) {
        k if k.is_empty() => keys::propose("bus", &taken),
        k => k,
    };
    drop(taken);
    let file = tracks::track_file(rev, &key);
    let binding = free_binding(&song_text, &camel(&key));
    let mut edits = tracks::channel_edits(&song_text, &mixer, &format!("{}: {binding}.out", prop_key(&key)));
    let (at, line) = tracks::import_edit(&song_text, &mixer.file, &file, &binding);
    edits.push((at, at, line));
    let edits = tracks::sorted(edits)?;
    let (song_file, new_file) = (path(&mixer.file)?, path(&file)?);
    let mut ops = vec![
        plan::text_edit(&song_file, &song_text, &edits),
        Op::Write { file: new_file.clone(), bytes: Payload::Text(bus_text(kind, &module, &key, &label).into()) },
    ];
    let mut ranges = vec![
        (song_file, Declared::Ranges(vec![Range::bytes(mixer.call.0, mixer.call.1), Range::bytes(at, at)])),
        (new_file, Declared::Whole),
    ];
    ops.extend(needs.ops);
    ranges.extend(needs.ranges);
    let k = Key::new(&key).map_err(EditError::Rejected)?;
    let expect = Expect::Confined(Confined { ranges: ranges.into_iter().collect(), lanes: vec![], modules: vec![ModuleChange::Added(k)] });
    let layout = tracks::layout_write(s, |v| {
        let mut colours = v.colours.clone();
        colours.insert(key.clone(), layout::unused(colours.values().copied()));
        (tracks::placed(&v.order, &key, after), colours, None)
    });
    let tx = tracks::with_layout(s, (String::new(), label.clone()), ops, expect, layout);
    let said = |why: &str| EditError::Rejected(format!("adding {label} would change more than adding a bus ({why})"));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        tracks::adds_one(checked, &key, &key)?;
        match checked.candidate().compiled.ir.modules.iter().any(|m| m.key == key && m.bus) {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("{label} wouldn't be a bus"))),
        }
    })?;
    let edited = Edited { revision: done.number, file, before: String::new(), after: label.clone() };
    Ok(Made { module: key, label, edited, new_channel: false })
}

/// A channel's routing, as the mixer's call writes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Route {
    /// The bus channel it goes to instead of the master.
    pub to: Option<String>,
    /// Each send, in order: the bus channel's key, and whether it is before the fader.
    pub sends: Vec<(String, bool)>,
}

/// A string literal's text, unquoted.
fn unquoted(text: &str) -> Option<String> {
    serde_json::from_str::<String>(text).ok().or_else(|| Some(text.strip_prefix('\'')?.strip_suffix('\'')?.to_string()))
}

/// Each channel's routing as mixer `m`'s call writes it, by key.
pub(crate) fn routing(rev: &Revision, m: &Mixer) -> BTreeMap<String, Route> {
    let text = rev.snapshot.files.get(Path::new(&m.file));
    let Some(Arg::Object { props, .. }) = m.site(rev).and_then(|x| x.args.first()) else { return BTreeMap::new() };
    let said = |a: &Arg| {
        let (s, e) = a.span();
        text.and_then(|t| t.get(s as usize..e as usize)).unwrap_or("")
    };
    let pre = ["pre".to_string()];
    props
        .iter()
        .map(|(key, a)| {
            let mut r = Route::default();
            if let Arg::Object { props: fields, .. } = a {
                for (name, v) in fields {
                    match (name.as_str(), v) {
                        ("to", _) => r.to = unquoted(said(v)),
                        ("sends", Arg::Object { props: sends, .. }) => {
                            r.sends = sends.iter().map(|(b, x)| (b.clone(), x.at(&pre).is_some_and(|p| said(p) == "true"))).collect();
                        }
                        _ => {}
                    }
                }
            }
            (key.clone(), r)
        })
        .collect()
}

/// Mixer `m`'s bus channels: each whose one track is a bus, by key, with that bus.
pub(crate) fn bus_channels(rev: &Revision, m: &Mixer) -> BTreeMap<String, String> {
    m.channels
        .iter()
        .filter_map(|c| match c.tracks.as_slice() {
            [b] if rev.compiled.ir.modules.iter().any(|x| x.key == *b && x.bus) => Some((c.key.clone(), b.clone())),
            _ => None,
        })
        .collect()
}

/// Who feeds whom: for each module, the modules its sound reaches directly, by the checker's routes
/// (a track into a bus, through the song) and by reads of another module's exports.
pub(crate) fn feeds(rev: &Revision) -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (m, b) in &rev.compiled.flat.routes {
        out.entry(m.clone()).or_default().insert(b.clone());
    }
    for m in rev.compiled.ir.modules.iter().filter(|m| m.key != ROOT) {
        crate::controls::walk(&m.nodes, &mut |n| {
            if let m2_ir::Op::Import { module, .. } = &n.op
                && module != ROOT
                && *module != m.key
            {
                out.entry(module.clone()).or_default().insert(m.key.clone());
            }
        });
    }
    out
}

/// Whether `from`'s sound reaches `to` through `feeds`.
fn reaches(feeds: &BTreeMap<String, BTreeSet<String>>, from: &str, to: &str) -> bool {
    let (mut seen, mut stack) = (BTreeSet::new(), vec![from]);
    while let Some(m) = stack.pop() {
        for n in feeds.get(m).into_iter().flatten() {
            if n == to {
                return true;
            }
            if seen.insert(n.as_str()) {
                stack.push(n);
            }
        }
    }
    false
}

/// What module `key` is called.
fn label_of(rev: &Revision, key: &str) -> String {
    rev.compiled.ir.modules.iter().find(|m| m.key == key).map_or(key, |m| m.label()).to_string()
}

/// What channel `c` is called: its tracks' names (`Kick + Snare`), else its key.
pub(crate) fn named(rev: &Revision, c: &Channel) -> String {
    match c.tracks.is_empty() {
        true => c.key.clone(),
        false => c.tracks.iter().map(|t| label_of(rev, t)).collect::<Vec<_>>().join(" + "),
    }
}

/// Why channel `c` can't send to bus `bus` (a module), or go to it, without a loop, in words that
/// name the tracks; none when it can.
pub(crate) fn looped(rev: &Revision, feeds: &BTreeMap<String, BTreeSet<String>>, c: &Channel, bus: &str, going: bool) -> Option<String> {
    let verb = if going { "go to" } else { "send to" };
    let b = label_of(rev, bus);
    if c.tracks.iter().any(|t| t == bus) {
        return Some(format!("{b} can't {verb} itself."));
    }
    let t = c.tracks.iter().find(|t| reaches(feeds, bus, t))?;
    Some(format!("{b} already feeds {}, so {} can't {verb} {b}.", label_of(rev, t), named(rev, c)))
}

/// The song's mixer, or why its routing is set in code.
fn mixer_of(rev: &Revision) -> Result<Mixer, EditError> {
    match mix::mixer(rev) {
        Mixing::One(m) => Ok(m),
        other => Err(EditError::Rejected(other.levels().unwrap_or_default())),
    }
}

/// Channel `key` of mixer `m`.
fn channel_of(m: &Mixer, key: &str) -> Result<Channel, EditError> {
    m.channels.iter().find(|c| c.key == key).cloned().ok_or_else(|| EditError::Missing(format!("The mixer has no channel {key}.")))
}

/// The bus that bus channel `key` of `m` plays, or why it isn't one (`refused`: what can't be done).
fn bus_of(rev: &Revision, m: &Mixer, key: &str, refused: &str) -> Result<String, EditError> {
    match bus_channels(rev, m).remove(key) {
        Some(b) => Ok(b),
        None => {
            let c = channel_of(m, key)?;
            Err(EditError::Rejected(format!("{} isn't a bus, so {refused}.", named(rev, &c))))
        }
    }
}

/// What bus channel `key` is called: its bus's name, else its key.
fn bus_named(rev: &Revision, m: &Mixer, key: &str) -> String {
    bus_channels(rev, m).get(key).map_or_else(|| key.to_string(), |b| label_of(rev, b))
}

/// A channel's entry as written, for an edit.
pub(crate) struct Written {
    /// Its properties, when it is an object (`{ in: kick.out, db: … }`); none for a bare signal.
    pub fields: Option<Vec<Property>>,
    /// Its `sends` object's properties, when it writes one.
    pub sends: Option<Vec<Property>>,
}

/// Channel `c`'s entry in `text`, named `name` in a refusal: an object this can't read (a spread,
/// `sends` not written as an object) is changed in code.
pub(crate) fn written(text: &str, c: &Channel, name: &str) -> Result<Written, EditError> {
    let in_code =
        || EditError::Rejected(format!("{name}'s mixer entry isn't written in a way this can change: change it in the Code view."));
    let p = &c.property;
    let object = text.get(p.value.0 as usize..p.value.1 as usize).is_some_and(|v| v.trim_start().starts_with('{'));
    let fields = match object {
        true => Some(properties(text, p.value).ok_or_else(in_code)?),
        false => None,
    };
    let sends = match fields.as_ref().and_then(|f| f.iter().find(|x| x.key == "sends")) {
        Some(f) => Some(properties(text, f.value).ok_or_else(in_code)?),
        None => None,
    };
    Ok(Written { fields, sends })
}

type Edits = Vec<(u32, u32, String)>;

/// The edits that give entry `p` (a channel's, or a send's) the property `prop` (`to: "drums"`):
/// one more property of an object (`{}` filled), or a bare value wrapped as `{ <first>: <value>,
/// <prop> }` (`first` is `in` for a channel, `db` for a send), its text kept where it is (ID-13).
fn with_prop(text: &str, p: &Property, first: &str, prop: &str) -> Result<Edits, EditError> {
    let (a, b) = p.value;
    let value = &text[a as usize..b as usize];
    if value.trim_start().starts_with('{') {
        let fields = properties(text, p.value).ok_or_else(|| {
            EditError::Rejected(format!("{}'s entry isn't written in a way this can change: change it in the Code view.", p.key))
        })?;
        return Ok(match fields.last() {
            Some(last) => vec![(last.end, last.end, format!(", {prop}"))],
            None => vec![(a, b, format!("{{ {prop} }}"))],
        });
    }
    // A shorthand (`kick` for `kick: kick`) is named in full.
    if (p.start, p.end) == p.value {
        return Ok(vec![(p.start, p.end, format!("{}: {{ {first}: {value}, {prop} }}", prop_key(&p.key)))]);
    }
    Ok(vec![(a, a, format!("{{ {first}: ")), (b, b, format!(", {prop} }}"))])
}

/// The deletions taking properties `gone` (indices, in order) out of an object whose properties
/// are `props`: each run of them with the comma that joins it to the property before it, or at the
/// start, to the one after it.
fn cut(props: &[Property], gone: &[usize]) -> Edits {
    let mut out = vec![];
    let mut i = 0;
    while i < gone.len() {
        let (start, mut end) = (gone[i], gone[i]);
        while i + 1 < gone.len() && gone[i + 1] == end + 1 {
            i += 1;
            end += 1;
        }
        i += 1;
        let (a, b) = match (start.checked_sub(1), props.get(end + 1)) {
            (Some(prev), _) => (props[prev].end, props[end].end),
            (None, Some(next)) => (props[start].start, next.start),
            (None, None) => (props[start].start, props[end].end),
        };
        out.push((a, b, String::new()));
    }
    out
}

/// The deletions taking the properties named `names` out of entry `p`, whose properties are
/// `fields`: an entry left with only `bare` (`in` for a channel, `db` for a send) is that value
/// alone again (`kick: kick.out`), its text kept where it is.
fn without(p: &Property, fields: &[Property], names: &[&str], bare: &str) -> Edits {
    let gone: Vec<usize> = (0..fields.len()).filter(|&i| names.contains(&fields[i].key.as_str())).collect();
    let kept: Vec<&Property> = (0..fields.len()).filter(|i| !gone.contains(i)).map(|i| &fields[i]).collect();
    match kept[..] {
        [only] if only.key == bare && (only.start, only.end) != only.value => {
            vec![(p.value.0, only.value.0, String::new()), (only.value.1, p.value.1, String::new())]
        }
        _ => cut(fields, &gone),
    }
}

/// The deletions taking channel `c`'s `to` (when `to`) and its send to `send` (when given) out of
/// its entry `w`: the `sends` object goes with its last send, and an entry left with only its `in`
/// is bare again. Deletions only, so what still reads a name keeps it.
pub(crate) fn taken_out(c: &Channel, w: &Written, to: bool, send: Option<&str>) -> Edits {
    let Some(fields) = &w.fields else { return vec![] };
    let mut names: Vec<&str> = vec![];
    if to && fields.iter().any(|f| f.key == "to") {
        names.push("to");
    }
    let mut inner = vec![];
    if let (Some(b), Some(sends)) = (send, &w.sends)
        && let Some(i) = sends.iter().position(|x| x.key == b)
    {
        match sends.len() {
            1 => names.push("sends"),
            _ => inner = cut(sends, &[i]),
        }
    }
    if names.is_empty() {
        return inner;
    }
    let mut out = without(&c.property, fields, &names, "in");
    out.extend(inner);
    out
}

/// What taking bus channel `bus` away takes out of the other channels: for each that sends to it or
/// goes to it, its key, what it does (`Send`, `Output`, or both), its line, and the deletions.
pub(crate) fn unrouted(rev: &Revision, m: &Mixer, bus: &str) -> Result<Vec<(String, &'static str, u32, Edits)>, EditError> {
    let text = rev.snapshot.files.get(Path::new(&m.file)).cloned().unwrap_or_default();
    let routes = routing(rev, m);
    let mut out = vec![];
    for c in m.channels.iter().filter(|c| c.key != bus) {
        let Some(r) = routes.get(&c.key) else { continue };
        let (to, send) = (r.to.as_deref() == Some(bus), r.sends.iter().any(|(b, _)| b == bus));
        let what = match (to, send) {
            (false, false) => continue,
            (true, false) => "Output",
            (false, true) => "Send",
            (true, true) => "Output and send",
        };
        let w = written(&text, c, &named(rev, c))?;
        let line = text[..c.property.start as usize].matches('\n').count() as u32 + 1;
        out.push((c.key.clone(), what, line, taken_out(c, &w, to, send.then_some(bus))));
    }
    Ok(out)
}

/// The edit that changes nothing: at the accepted revision, saying `label`.
fn unchanged(rev: &Revision, m: &Mixer, label: String) -> Edited {
    Edited { revision: rev.number, file: m.file.clone(), before: label.clone(), after: label }
}

/// Write `edits` into the mixer's file as one Tx confined to the mixer's call, labelled `label`,
/// asserting on the candidate that every channel keeps its tracks, the routing is `want`, and no
/// module, value or data changes.
fn commit(s: &mut Session, m: &Mixer, edits: Edits, label: (String, String), want: BTreeMap<String, Route>) -> Result<Edited, EditError> {
    let text = s.accepted().snapshot.files[Path::new(&m.file)].clone();
    let edits = tracks::sorted(edits)?;
    let file = path(&m.file)?;
    let op = plan::text_edit(&file, &text, &edits);
    let ranges = [(file, Declared::Ranges(vec![Range::bytes(m.call.0, m.call.1)]))].into();
    let tx = plan::planned(s, label.clone(), vec![op], Expect::Confined(Confined { ranges, ..Confined::default() }));
    let before: Vec<(String, Vec<String>)> = m.channels.iter().map(|c| (c.key.clone(), c.tracks.clone())).collect();
    let said = |why: &str| EditError::Rejected(format!("“{}” would change more than the mixer's routing ({why})", label.1));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        routed(checked, &before, &want).map_err(|why| EditError::Rejected(format!("“{}” would {why}", label.1)))
    })?;
    Ok(Edited { revision: done.number, file: m.file.clone(), before: label.0, after: label.1 })
}

/// The routing edits' assertion on the checked candidate: no module, value or data changes, every
/// channel keeps its tracks, and the routing is `want`.
fn routed(checked: &Checked, before: &[(String, Vec<String>)], want: &BTreeMap<String, Route>) -> Result<(), String> {
    let d = &checked.delta;
    if !(d.modules.is_empty() && d.values.is_empty() && d.lanes.is_empty() && d.states.is_empty() && d.assets.is_empty() && !d.arrangement)
    {
        return Err("change a track, a value or the song's data too".into());
    }
    let Mixing::One(m) = mix::mixer(checked.candidate()) else { return Err("lose the mixer".into()) };
    if m.channels.iter().map(|c| (c.key.clone(), c.tracks.clone())).collect::<Vec<_>>() != before {
        return Err("move a track to another channel".into());
    }
    match routing(checked.candidate(), &m) == *want {
        true => Ok(()),
        false => Err("route more than it says".into()),
    }
}

/// What the routing edits share: the mixer, channel `channel`, its name, its file's text and the
/// routing now.
struct Base {
    m: Mixer,
    c: Channel,
    name: String,
    text: std::sync::Arc<str>,
    routes: BTreeMap<String, Route>,
}

fn base(s: &Session, channel: &str) -> Result<Base, EditError> {
    ready(s)?;
    let rev = s.accepted();
    let m = mixer_of(rev)?;
    let c = channel_of(&m, channel)?;
    let name = named(rev, &c);
    let text = rev.snapshot.files[Path::new(&m.file)].clone();
    let routes = routing(rev, &m);
    Ok(Base { m, c, name, text, routes })
}

/// Why a send isn't written in a way this can change.
fn send_in_code(name: &str, bus: &str) -> EditError {
    EditError::Rejected(format!("{name}'s send to {bus} isn't written in a way this can change: change it in the Code view."))
}

/// Send channel `channel`'s sound to bus channel `bus`: `sends: { verb: db(-12) }` in its entry (a
/// bare entry wrapped, one more send in its `sends`), after the fader, or before it with `pre`
/// (`{ db: db(-12), pre: true }`); a plain `-12` where the file doesn't import `db`. Refused when it
/// sends there already, or when that would make a loop.
pub fn add_send(s: &mut Session, channel: &str, bus: &str, pre: bool) -> Result<Edited, EditError> {
    let Base { m, c, name, text, mut routes } = base(s, channel)?;
    let rev = s.accepted();
    let b = bus_of(rev, &m, bus, "nothing can be sent to it")?;
    let bus_name = label_of(rev, &b);
    if routes.get(channel).is_some_and(|r| r.sends.iter().any(|(x, _)| x == bus)) {
        return Err(EditError::Rejected(format!("{name} already sends to {bus_name}.")));
    }
    if let Some(why) = looped(rev, &feeds(rev), &c, &b, false) {
        return Err(EditError::Rejected(why));
    }
    let w = written(&text, &c, &name)?;
    let level = if imported_names(&text, "@m2/core").iter().any(|n| n == "db") { "db(-12)" } else { "-12" };
    let send = match pre {
        true => format!("{}: {{ db: {level}, pre: true }}", prop_key(bus)),
        false => format!("{}: {level}", prop_key(bus)),
    };
    let sends = w.fields.as_ref().and_then(|f| f.iter().find(|x| x.key == "sends"));
    let edits = match (&w.sends, sends) {
        (Some(all), Some(p)) => match all.last() {
            Some(last) => vec![(last.end, last.end, format!(", {send}"))],
            None => vec![(p.value.0, p.value.1, format!("{{ {send} }}"))],
        },
        _ => with_prop(&text, &c.property, "in", &format!("sends: {{ {send} }}"))?,
    };
    routes.entry(channel.to_string()).or_default().sends.push((bus.to_string(), pre));
    commit(s, &m, edits, (name.clone(), format!("{name} sends to {bus_name}")), routes)
}

/// Take channel `channel`'s send to bus channel `bus` out: the send, and its `sends` object with
/// the last one; an entry left with only its `in` is bare again.
pub fn remove_send(s: &mut Session, channel: &str, bus: &str) -> Result<Edited, EditError> {
    let Base { m, c, name, text, mut routes } = base(s, channel)?;
    let bus_name = bus_named(s.accepted(), &m, bus);
    let r = routes.entry(channel.to_string()).or_default();
    let Some(i) = r.sends.iter().position(|(x, _)| x == bus) else {
        return Err(EditError::Rejected(format!("{name} doesn't send to {bus_name}.")));
    };
    r.sends.remove(i);
    let w = written(&text, &c, &name)?;
    let edits = taken_out(&c, &w, false, Some(bus));
    commit(s, &m, edits, (format!("{name} sends to {bus_name}"), name.clone()), routes)
}

/// Send channel `channel`'s sound to bus channel `bus` before its fader (`pre`) or after it: a bare
/// level wrapped as `{ db: …, pre: true }`, and back to the bare level after the fader. Setting
/// what is so already writes nothing.
pub fn set_pre(s: &mut Session, channel: &str, bus: &str, pre: bool) -> Result<Edited, EditError> {
    let Base { m, c, name, text, mut routes } = base(s, channel)?;
    let bus_name = bus_named(s.accepted(), &m, bus);
    let said = |pre: bool| format!("{name} sends to {bus_name} {} the fader", if pre { "before" } else { "after" });
    let r = routes.entry(channel.to_string()).or_default();
    let Some(send) = r.sends.iter_mut().find(|(x, _)| x == bus) else {
        return Err(EditError::Rejected(format!("{name} doesn't send to {bus_name}.")));
    };
    if send.1 == pre {
        return Ok(unchanged(s.accepted(), &m, said(pre)));
    }
    send.1 = pre;
    let w = written(&text, &c, &name)?;
    let p = w.sends.as_ref().and_then(|all| all.iter().find(|x| x.key == bus)).ok_or_else(|| send_in_code(&name, &bus_name))?;
    let object = text[p.value.0 as usize..p.value.1 as usize].trim_start().starts_with('{');
    let edits = match (object, object.then(|| properties(&text, p.value)).flatten()) {
        (true, Some(fields)) => match fields.iter().position(|f| f.key == "pre") {
            Some(i) if pre || fields.len() == 1 => vec![(fields[i].value.0, fields[i].value.1, pre.to_string())],
            Some(_) => without(p, &fields, &["pre"], "db"),
            None => with_prop(&text, p, "db", "pre: true")?,
        },
        (false, _) if pre => with_prop(&text, p, "db", "pre: true")?,
        _ => return Err(send_in_code(&name, &bus_name)),
    };
    commit(s, &m, edits, (said(!pre), said(pre)), routes)
}

/// Send channel `channel` to bus channel `to` instead of the master (`to: "drums"` in its entry, a
/// bare entry wrapped), or with none, back to the master (its `to` taken out; an entry left with
/// only its `in` is bare again). Refused when that would make a loop.
pub fn route(s: &mut Session, channel: &str, to: Option<&str>) -> Result<Edited, EditError> {
    let Base { m, c, name, text, mut routes } = base(s, channel)?;
    let rev = s.accepted();
    if let Some(b) = to {
        let bus = bus_of(rev, &m, b, "no track can go to it")?;
        if let Some(why) = looped(rev, &feeds(rev), &c, &bus, true) {
            return Err(EditError::Rejected(why));
        }
    }
    let shown = |b: Option<&str>| b.map_or_else(|| "Master".to_string(), |b| bus_named(rev, &m, b));
    let r = routes.entry(channel.to_string()).or_default();
    let now = r.to.clone();
    if now.as_deref() == to {
        return Ok(unchanged(rev, &m, format!("{name} → {}", shown(to))));
    }
    r.to = to.map(str::to_string);
    let label = (format!("{name} → {}", shown(now.as_deref())), format!("{name} → {}", shown(to)));
    let w = written(&text, &c, &name)?;
    let edits = match to {
        Some(b) => match w.fields.as_ref().and_then(|f| f.iter().find(|x| x.key == "to")) {
            Some(p) => vec![(p.value.0, p.value.1, quoted(b))],
            None => with_prop(&text, &c.property, "in", &format!("to: {}", quoted(b)))?,
        },
        None => taken_out(&c, &w, true, None),
    };
    commit(s, &m, edits, label, routes)
}

#[cfg(test)]
#[path = "bus_tests.rs"]
mod tests;

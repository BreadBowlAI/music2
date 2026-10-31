//! A call's settings on its card (CARD-1, CARD-3 to CARD-5): every setting its component declares,
//! each with its kind (CARD-2), its value as JSON, whether it is written or plays its default, its
//! home (HOME-1), and whether the card can change it; and the one write that changes any of them.
//!
//! A setting is read from the call as written: a string, a boolean or a number literal there is its
//! value; one the call leaves out plays the descriptor's default. Anything else (a name, an
//! expression, options the call doesn't write in itself) is worked out by the code, and shows
//! read-only. The write is one `TextEdit` Tx confined to the call: over the literal written there,
//! or into the call by PKG-3's insertion rules, as a knob's first edit is.

use std::path::Path;

use m2_ir::{Module, Op};
use m2_script::Descriptor;
use m2_script::component::{EachDesc, SettingDesc, SettingKind};
use m2_script::sites::{Arg, Site};
use m2_session::{Checked, Home, Revision, Session};
use m2_song::tx::{Confined, Declared, Expect, Range};
use serde::Serialize;
use serde_json::Value as Json;

use crate::cable::current_site;
use crate::controls::{options_of, walk};
use crate::effect::calls_of;
use crate::import::{self, Imported};
use crate::kind::{self, Choice, Kind};
use crate::lens::{self, Insertion};
use crate::plan::{self, path, unmoved};
use crate::sampler::{self, SamplerCard};
use crate::{EditError, Edited, ready};

/// A setting as a card shows it (CARD-1).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Setting {
    /// What the write names: the call and the setting, `tracks/bass.ts#3/@shape`, or a dictionary
    /// entry's by its path, `song.ts#2/@channels.vox.to`. Meaningful within its revision.
    pub id: String,
    /// The setting: `shape`, or a dictionary entry's by its path (`channels.vox.to`).
    pub slot: String,
    /// What it is called: its name, or a dictionary entry's by its key (`vox.to`).
    pub label: String,
    pub kind: Kind,
    /// A whole number (`setting.int`): a fraction is refused.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub whole: bool,
    /// A choice's options, or a switch's two (off, then on); the song's tracks for a track.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// What the call plays, as JSON; null where the code works it out, or a call leaves out one
    /// with no default.
    pub value: Json,
    /// Written in the call; false while it plays its declared default.
    pub written: bool,
    /// Where the value lives (HOME-1): the code.
    pub home: Home,
    /// Why it can be changed only in its code, when the card can't change it: the code works it
    /// out, the call can't take it, or (regions, a keymap) the card shows it only in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<String>,
    /// The code that works it out, when it isn't a value written in the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// What regions or a keymap play, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
    pub file: String,
    pub line: u32,
}

/// Where a setting is in a call as written.
enum At<'a> {
    /// Written there: the argument.
    Written(&'a Arg),
    /// Left out: the call gives none, so it plays its default.
    Left,
    /// What the call gives can't be told from it: why.
    Unknown(&'static str),
}

/// One call of a component, as its card reads it.
struct Call<'a> {
    rev: &'a Revision,
    site: &'a Site,
    desc: &'a Descriptor,
    text: &'a str,
    /// Which argument held its options when it ran (`Frame.options`), or why that isn't known.
    options: Result<Option<usize>, &'static str>,
    /// The song's tracks, for a setting that names one.
    tracks: Vec<Choice>,
    /// What a Sampler plays, for its keymap's and its regions' words.
    sampler: Option<SamplerCard>,
}

/// The song's tracks as a menu offers them: each by its key, called by its label.
fn tracks(rev: &Revision) -> Vec<Choice> {
    rev.compiled.ir.modules.iter().filter(|m| m.track).map(|m| Choice::new(m.key.clone(), m.label())).collect()
}

/// What a setting declared as `d` is on a card (CARD-2), whether it is whole, and its options.
fn kind_of(d: &SettingDesc, tracks: &[Choice]) -> (Kind, bool, Vec<Choice>) {
    match d.kind {
        SettingKind::Number => (Kind::Number, false, vec![]),
        SettingKind::Int => (Kind::Number, true, vec![]),
        SettingKind::Bool => (Kind::Switch, false, kind::switch(false, true, None)),
        SettingKind::Choice => (Kind::Choice, false, d.options.iter().flatten().map(|o| Choice::new(o.clone(), o.clone())).collect()),
        SettingKind::Asset => (Kind::Recording, false, vec![]),
        SettingKind::Keymap => (Kind::Keymap, false, vec![]),
        SettingKind::Regions => (Kind::Regions, false, vec![]),
        SettingKind::Key => (Kind::Track, false, tracks.to_vec()),
    }
}

/// A number as JSON: whole numbers as integers, so `8` written reads as the `8` a descriptor declares.
fn number(v: f64) -> Json {
    match v.fract() == 0.0 && v.abs() < 9.0e15 {
        true => Json::from(v as i64),
        false => serde_json::Number::from_f64(v).map_or(Json::Null, Json::Number),
    }
}

/// Whether two values are the same: numbers by what they are, whatever their JSON form.
fn same(a: &Json, b: &Json) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// A string literal's text, when it is one written plainly: quoted with `"` or `'`, with no escape.
fn string_literal(t: &str) -> Option<&str> {
    let q = t.chars().next().filter(|q| matches!(q, '"' | '\''))?;
    let inner = t.strip_prefix(q)?.strip_suffix(q)?;
    (!inner.contains([q, '\\', '\n'])).then_some(inner)
}

/// The value an argument written in the call is, when it is a literal: a number, true or false,
/// or a plain string.
fn read(a: &Arg, text: &str) -> Option<Json> {
    match a {
        Arg::Number { value, .. } => Some(number(*value)),
        Arg::Other { start, end } => match text.get(*start as usize..*end as usize)?.trim() {
            "true" => Some(Json::Bool(true)),
            "false" => Some(Json::Bool(false)),
            t => string_literal(t).map(|s| Json::String(s.to_string())),
        },
        _ => None,
    }
}

impl<'a> Call<'a> {
    fn new(rev: &'a Revision, m: &Module, site: &'a Site, desc: &'a Descriptor, text: &'a str) -> Call<'a> {
        let sampler = (desc.id == sampler::SAMPLER).then(|| sampler::card(&rev.compiled, site.id)).flatten();
        Call { rev, site, desc, text, options: options_of(m, site.id), tracks: tracks(rev), sampler }
    }

    /// Where setting `slot` (or a dictionary, by its name) is written in the call: at its place
    /// among the arguments, in the options object, or in a dictionary entry written in the call.
    fn find(&self, slot: &str) -> At<'a> {
        let options = match self.options {
            Ok(o) => o,
            Err(why) => return At::Unknown(why),
        };
        let args = &self.site.args;
        let given = options.unwrap_or(args.len());
        let mut parts = slot.split('.');
        let name = parts.next().unwrap_or_default();
        let top = match lens::params(self.desc).iter().position(|p| *p == name) {
            Some(p) if p < given => args.get(p),
            _ => match options.and_then(|i| args.get(i)) {
                Some(Arg::Object { props, .. }) => props.iter().find(|(k, _)| k == name).map(|(_, a)| a),
                Some(_) => return At::Unknown("its options aren't an object written in the call"),
                None => None,
            },
        };
        let Some(mut at) = top else { return At::Left };
        // Into a dictionary: an entry by its key, then a setting of the entry or a dictionary of its own.
        for (i, part) in parts.enumerate() {
            at = match at {
                Arg::Object { props, .. } => match props.iter().find(|(k, _)| k == part) {
                    Some((_, a)) => a,
                    None => return At::Left,
                },
                // A bare entry (`kick: kick.out`) gives none of its settings.
                _ if i % 2 == 1 => return At::Left,
                _ => return At::Unknown("its dictionary isn't an object written in the call"),
            };
        }
        At::Written(at)
    }

    /// The setting declared as `d` at `slot`, as the card shows it.
    fn setting(&self, slot: String, d: &SettingDesc) -> Setting {
        let (kind, whole, options) = kind_of(d, &self.tracks);
        let words = matches!(kind, Kind::Regions | Kind::Keymap);
        let (mut value, mut written, mut readonly, mut expression) = (Json::Null, true, None, None);
        match self.find(&slot) {
            At::Written(a) => match read(a, self.text).filter(|_| !words) {
                Some(v) => value = v,
                None => {
                    readonly = Some("it is worked out by its code".to_string());
                    let (start, end) = a.span();
                    expression = self.text.get(start as usize..end as usize).filter(|_| !words).map(str::to_string);
                }
            },
            At::Left => {
                (value, written) = (d.default.clone().unwrap_or(Json::Null), false);
                readonly = match words {
                    true => None,
                    false => self.insertion(&slot).err().map(|why| format!("it can't be written into the call: {why}")),
                };
            }
            At::Unknown(why) => readonly = Some(why.to_string()),
        }
        if words {
            readonly = Some("the card says what it plays in words; it is set in its code".to_string());
        }
        // The Sampler plays one of a file, zones and regions (PKG-16): with zones or regions written,
        // its file is theirs to change, in its code.
        if self.sampler.is_some() && slot == "sample" && readonly.is_none() {
            readonly = sampler::plays_others(self.options.ok().flatten().and_then(|i| self.site.args.get(i))).map(str::to_string);
        }
        let said = match (&self.sampler, kind) {
            (Some(c), Kind::Regions) => c.regions.clone(),
            (Some(c), Kind::Keymap) if c.regions.is_none() && c.files.len() > 1 => {
                Some(format!("{} files: {}", c.files.len(), c.files.join(", ")))
            }
            _ => None,
        };
        let label = slot.split_once('.').map_or(slot.as_str(), |(_, rest)| rest).to_string();
        Setting {
            id: format!("{}#{}/@{slot}", self.site.file, self.site.ordinal),
            slot,
            label,
            kind,
            whole,
            options,
            value,
            written,
            home: Home::Code,
            readonly,
            expression,
            said,
            file: self.site.file.clone(),
            line: self.site.line,
        }
    }

    /// How setting `slot`, left out of the call, is written into it (PKG-3's insertion).
    fn insertion(&self, slot: &str) -> Result<Insertion, &'static str> {
        Insertion::plan(self.site, self.desc, slot, self.text, self.options?, &self.rev.compiled.units)
    }

    /// Every setting of the call, in its descriptor's order: its own, then each dictionary entry's
    /// written in the call, entry by entry (PKG-6).
    fn all(&self) -> Vec<Setting> {
        let mut out: Vec<Setting> = self.desc.settings.iter().map(|d| self.setting(d.name.clone(), d)).collect();
        for c in &self.desc.controls {
            if let (Some(each), At::Written(Arg::Object { props, .. })) = (c.each.as_deref(), self.find(&c.name)) {
                self.entries(&c.name, each, props, &mut out);
            }
        }
        out
    }

    /// The settings of each entry `props` of the dictionary at `path`, whose entries are `each`.
    fn entries(&self, path: &str, each: &EachDesc, props: &[(String, Arg)], out: &mut Vec<Setting>) {
        for (key, entry) in props {
            out.extend(each.settings.iter().map(|d| self.setting(format!("{path}.{key}.{}", d.name), d)));
            let Arg::Object { props: fields, .. } = entry else { continue };
            for c in &each.controls {
                if let (Some(inner), Some((_, Arg::Object { props, .. }))) = (c.each.as_deref(), fields.iter().find(|(k, _)| *k == c.name))
                {
                    self.entries(&format!("{path}.{key}.{}", c.name), inner, props, out);
                }
            }
        }
    }
}

/// The component call `site` of `rev` is an instance of: its module and its descriptor.
fn instance(rev: &Revision, site: u32) -> Option<(&Module, &Descriptor)> {
    rev.compiled.ir.modules.iter().find_map(|m| {
        let f = m.frames.iter().find(|f| f.site == site && f.component.is_some())?;
        Some((m, &**rev.compiled.components.get(f.component.as_deref()?)?))
    })
}

/// The setting `slot` of `desc` declares: its own, or a dictionary entry's by its path
/// (`channels.vox.to`, `channels.vox.sends.verb.pre`).
fn declared<'a>(desc: &'a Descriptor, slot: &str) -> Option<&'a SettingDesc> {
    let parts: Vec<&str> = slot.split('.').collect();
    let (first, mut rest) = parts.split_first()?;
    if rest.is_empty() {
        return desc.settings.iter().find(|s| s.name == *first);
    }
    let mut each = desc.controls.iter().find(|c| c.name == *first)?.each.as_deref()?;
    loop {
        match rest {
            [_, name] => return each.settings.iter().find(|s| s.name == *name),
            [_, sub, more @ ..] => {
                each = each.controls.iter().find(|c| c.name == *sub)?.each.as_deref()?;
                rest = more;
            }
            _ => return None,
        }
    }
}

/// The settings of the card of call `site` of `rev`, when it is a call of a component (CARD-1).
pub(crate) fn of_card(rev: &Revision, site: &Site) -> Vec<Setting> {
    let Some((m, desc)) = instance(rev, site.id) else { return vec![] };
    let Some(text) = rev.snapshot.files.get(Path::new(&site.file)) else { return vec![] };
    Call::new(rev, m, site, desc, text).all()
}

/// Setting `id` (`file#call/@slot`) named at revision `at`, as it is in the accepted revision: its
/// call, its declaration, and the card's view of it.
fn resolve(s: &Session, at: u64, id: &str) -> Result<(Site, Setting, String), EditError> {
    let missing = || EditError::NotFound(id.to_string());
    let (call, slot) = id.rsplit_once("/@").ok_or_else(missing)?;
    let site = current_site(s, at, call)?.clone();
    let rev = s.accepted();
    let (m, desc) = instance(rev, site.id).ok_or_else(missing)?;
    let d = declared(desc, slot).ok_or_else(missing)?;
    let text = rev.snapshot.files.get(Path::new(&site.file)).ok_or_else(missing)?;
    let now = Call::new(rev, m, &site, desc, text).setting(slot.to_string(), d);
    Ok((site, now, desc.label.clone()))
}

/// What the call of setting `id` is called: its component's label ("Sampler").
pub fn title(s: &Session, at: u64, id: &str) -> Option<String> {
    resolve(s, at, id).ok().map(|(_, _, title)| title)
}

/// Whether `id`, named at revision `at`, is a setting of a call: what `POST /api/graph/set` writes
/// with [`set`] rather than as a knob.
pub fn names(s: &Session, at: u64, id: &str) -> bool {
    resolve(s, at, id).is_ok()
}

/// `value` written as setting `now`'s literal, quoted as `old` (the literal written there) is, or
/// refused in words when it isn't of the setting's kind.
fn literal(now: &Setting, value: &Json, old: Option<&str>) -> Result<String, EditError> {
    let no = |what: String| Err(EditError::Rejected(format!("{} is {what}, not {value}", now.label)));
    let quoted = |v: &str| match old.and_then(|o| o.chars().next()) {
        Some('\'') if !v.contains(['\'', '\\', '\n']) => format!("'{v}'"),
        _ => serde_json::to_string(v).expect("a string"),
    };
    let one_of = |options: &[Choice]| options.iter().map(|o| o.value.to_string()).collect::<Vec<_>>().join(", ");
    match now.kind {
        Kind::Number => match value.as_f64().filter(|v| v.is_finite()) {
            Some(v) if !now.whole || v.fract() == 0.0 => Ok(lens::propose(v, 1.0)?.text),
            _ if now.whole => no("a whole number".into()),
            _ => no("a number".into()),
        },
        Kind::Switch => match value {
            Json::Bool(b) => Ok(b.to_string()),
            _ => no("on or off (true or false)".into()),
        },
        Kind::Choice | Kind::Track => match value.as_str() {
            Some(v) if now.options.iter().any(|o| o.value == *value) => Ok(quoted(v)),
            _ if now.options.is_empty() => Err(EditError::Rejected(format!("{} names a track, and the song has none", now.label))),
            _ => no(format!("one of {}", one_of(&now.options))),
        },
        Kind::Recording => match value.as_str().filter(|v| !v.is_empty()) {
            Some(v) => Ok(quoted(v)),
            None => no("the name of a file in assets/".into()),
        },
        Kind::Regions | Kind::Keymap => Err(EditError::Rejected(format!("{} is set in its code", now.label))),
    }
}

/// Write `value` into setting `id` (CARD-4), named at revision `at`: one Tx confined to its call,
/// over the literal written there or into the call by PKG-3's insertion rules, checked against the
/// graph; one undo step. A value of the wrong kind, or a setting the card can't change, is refused
/// in words. The value it has already writes nothing. With `expect`, only while it is that.
pub fn set(s: &mut Session, at: u64, id: &str, value: &Json, expect: Option<&Json>) -> Result<Edited, EditError> {
    write(s, at, id, value, expect, |_, _, _| Ok(()))
}

/// [`set`], with the planner's own assertion on the checked candidate, given the call's site there.
fn write(
    s: &mut Session,
    at: u64,
    id: &str,
    value: &Json,
    expect: Option<&Json>,
    assert: impl FnOnce(&Session, &Checked, &Site) -> Result<(), EditError>,
) -> Result<Edited, EditError> {
    ready(s)?;
    let (site, now, _) = resolve(s, at, id)?;
    if let Some(why) = &now.readonly {
        return Err(EditError::Rejected(format!("{} can be changed only in its code: {why}", now.label)));
    }
    if let Some(e) = expect.filter(|e| !same(e, &now.value)) {
        return Err(EditError::Rejected(format!("{} changed elsewhere: it is {} now, not {e}", now.label, now.value)));
    }
    let rev = s.accepted();
    let text = rev.snapshot.files.get(Path::new(&site.file)).cloned().ok_or_else(|| EditError::NotFound(site.file.clone()))?;
    let (m, desc) = instance(rev, site.id).ok_or_else(|| EditError::NotFound(id.to_string()))?;
    let call = Call::new(rev, m, &site, desc, &text);
    let written = match call.find(&now.slot) {
        At::Written(a) => Some(a.span()),
        _ => None,
    };
    let old = written.and_then(|(a, b)| text.get(a as usize..b as usize));
    let literal = literal(&now, value, old)?;
    if same(value, &now.value) {
        let shown = old.unwrap_or_default().to_string();
        return Ok(Edited { revision: rev.number, file: site.file, before: shown.clone(), after: shown });
    }
    let (edits, entry) = match written {
        Some((a, b)) => (vec![(a, b, literal)], None),
        None => {
            let insertion = call.insertion(&now.slot).map_err(|why| EditError::Rejected(format!("{}: {why}", now.label)))?;
            (insertion.edits(&literal), insertion.entry)
        }
    };
    let label = match (&entry, edits.as_slice()) {
        (Some(e), _) => crate::label::shown_entry(&text, e, &edits),
        (None, [(a, b, t)]) => crate::label::shown(s, site.id, *a, *b, t),
        _ => (String::new(), edits.iter().map(|e| e.2.as_str()).collect()),
    };
    let file = path(&site.file)?;
    let op = plan::text_edit(&file, &text, &edits);
    let expect = Expect::Confined(Confined {
        ranges: [(file, Declared::Ranges(vec![Range::bytes(site.start, site.end)]))].into(),
        lanes: vec![],
        modules: vec![],
    });
    let tx = plan::planned(s, label.clone(), vec![op], expect);
    let said = |why: &str| EditError::Rejected(format!("setting {} would change more than its call ({why})", now.label));
    let done = plan::run_as(s, tx, said, |s, checked| {
        unmoved(s, checked)?;
        assert(s, checked, &site)
    })?;
    Ok(Edited { revision: done.number, file: site.file, before: label.0, after: label.1 })
}

/// Play `file` (just brought in, as Import Audio brings one in, FMT-14) in recording `id` (CARD-5),
/// named at revision `at`: the file is taken in, then [`set`] writes its name. The planner checks
/// that a kernel the call makes reads it. One undo step; the file stays in `assets/`.
pub fn set_recording(s: &mut Session, at: u64, id: &str, file: &Imported) -> Result<Edited, EditError> {
    ready(s)?;
    let (_, now, title) = resolve(s, at, id)?;
    let name = file.asset.as_str().to_string();
    if now.kind != Kind::Recording {
        return Err(EditError::Rejected(format!("{title}'s {} isn't a recording: drop a WAV or FLAC file on one", now.label)));
    }
    if let Some(why) = &now.readonly {
        return Err(EditError::Rejected(format!("{title}'s {} can be changed only in its code: {why}", now.label)));
    }
    if now.value.as_str() == Some(name.as_str()) {
        return Err(EditError::Rejected(format!("{title} plays {name} already.")));
    }
    // Taken in before the Tx names it, so the revision plays it from the start.
    import::take_in(s, std::slice::from_ref(file))?;
    write(s, at, id, &Json::String(name.clone()), None, |_, checked, site| {
        let c = &checked.candidate().compiled;
        let new = c.sites.iter().find(|x| x.file == site.file && x.start == site.start && x.callee == site.callee).map(|x| x.id);
        let mut reads = false;
        for m in &c.ir.modules {
            walk(&m.nodes, &mut |n| {
                if let Op::Kernel { assets, .. } = &n.op {
                    reads |= assets.contains(&name) && calls_of(m, n).any(|f| Some(f) == new);
                }
            });
        }
        match reads {
            true => Ok(()),
            false => Err(EditError::Rejected(format!("{title} wouldn't play {name}: no sound it makes reads the file"))),
        }
    })
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

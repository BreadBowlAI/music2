//! A card's groups and views (CARD-25, CARD-26), resolved for its call: what the window draws over
//! the card's rows, and what an agent reads of it.
//!
//! - **Groups** put a card's values and views in its author's order (CARD-17). A row is an input of
//!   the call that is one of its declared controls (a knob, a cable, a lane, a value the code works
//!   out), a setting, or a view; what no group names goes in a last group, More. An input that is a
//!   connection (notes, audio) is in no group: the card draws it above them.
//! - **Views** are pictures over values with homes (HOME-1). An envelope's and a filter's parts each
//!   name their control's slot and its knob, when the card can move it; a keys view what std's
//!   Sampler plays; a scope its own call, which the window asks the server for (TAP-8).

use m2_ir::Module;
use m2_script::Descriptor;
use m2_script::component::ViewOf;
use serde::Serialize;

use crate::lens;
pub use crate::sampler::KeyRegion;
use crate::view::{InputPort, LaneRef, ViewNode};

/// A group of a card (CARD-25): its name, and its rows in the order the group names them.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CardGroup {
    pub name: String,
    pub rows: Vec<Row>,
}

/// A row of a card: an input of its call (by `InputPort.id`), a setting (by its id) or a view (by
/// its name).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Row {
    Input(String),
    Setting(String),
    View(String),
}

/// A view of a card (CARD-26), resolved for its call.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CardView {
    /// Its name in the descriptor (`amp`).
    pub name: String,
    #[serde(flatten)]
    pub of: Drawn,
}

/// What a view draws.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Drawn {
    /// Which recording plays on which keys and velocities, from what std's Sampler plays.
    Keys {
        /// The `regions` or `keymap` setting it draws, by its id.
        setting: String,
        regions: Vec<KeyRegion>,
        /// The notes lane that plays the call, which a clicked key plays on (CARD-28).
        #[serde(skip_serializing_if = "Option::is_none")]
        lane: Option<LaneRef>,
        /// Why no keys are drawn, when they can't be.
        #[serde(skip_serializing_if = "Option::is_none")]
        why: Option<String>,
    },
    /// The call's sound as it plays (TAP-8).
    Scope {},
    /// A picture its pack or song draws (CARD-63): its axes, its live layers, and every value its
    /// draw reads. Its shapes and words are asked for (CARD-67), never carried here.
    Picture {
        x: m2_script::component::Axis,
        y: m2_script::component::Axis,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        live: Vec<serde_json::Value>,
        values: Vec<PictureValue>,
        /// Its words at these values, or why it can't be drawn: worked out by the picture runner
        /// after the view is made, never by the settle that makes it, so null here (CARD-63).
        words: Option<String>,
        /// The notes lane that plays the call, which a `p.play` place plays on (CARD-72), as the
        /// keys view's keys do.
        #[serde(skip_serializing_if = "Option::is_none")]
        lane: Option<LaneRef>,
    },
}

/// A value a picture reads, by its name in the descriptor (CARD-63): what plays, the knob that
/// moves it when the card can, and why not when it can't.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct PictureValue {
    pub name: String,
    /// A number in its declared unit, a choice's name, a switch's true or false, a setting's value;
    /// null where a cable or the code works it out.
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// The declared control at argument `path` of a call of `desc` whose options are argument
/// `options` (`Frame.options`): a positional one by its place, one in the options object by its
/// name, a dictionary entry's by its whole path (`channels.hats.db`). None for an input port, or
/// what the descriptor doesn't declare.
pub(crate) fn slot_at(desc: &Descriptor, options: Option<usize>, path: &[String]) -> Option<String> {
    let (first, rest) = path.split_first()?;
    let i: usize = first.parse().ok()?;
    let slot = match options == Some(i) {
        true => rest.join("."),
        false => {
            let name = *lens::params(desc).get(i)?;
            std::iter::once(name).chain(rest.iter().map(String::as_str)).collect::<Vec<_>>().join(".")
        }
    };
    desc.control(&slot).map(|_| slot)
}

/// The rows of card `node` that the name `name` of `desc` stands for, as a group or the face names
/// it: an input of the call, a setting, a view; a dictionary (`parts`) all its entries' values
/// (`parts.a.db`, review V7).
fn rows_named(desc: &Descriptor, node: &ViewNode, name: &str) -> Vec<Row> {
    let named = |slot: &str| slot == name || slot.strip_prefix(name).is_some_and(|rest| rest.starts_with('.'));
    let mut rows: Vec<Row> =
        node.inputs.iter().filter(|p| p.slot.as_deref().is_some_and(named)).map(|p| Row::Input(p.id.clone())).collect();
    rows.extend(node.settings.iter().filter(|x| named(&x.slot)).map(|x| Row::Setting(x.id.clone())));
    if desc.editor.views.iter().any(|v| v.name == name) {
        rows.push(Row::View(name.to_string()));
    }
    rows
}

/// Card `node`'s groups (CARD-25), of a call of `desc`: empty when it declares none.
pub(crate) fn groups(desc: &Descriptor, node: &ViewNode) -> Vec<CardGroup> {
    if desc.editor.groups.is_empty() {
        return vec![];
    }
    let mut out: Vec<CardGroup> = desc
        .editor
        .groups
        .iter()
        .map(|g| CardGroup { name: g.name.clone(), rows: g.items.iter().flat_map(|i| rows_named(desc, node, i)).collect() })
        .collect();
    // More: the values and views no group names, in the card's order.
    let more: Vec<Row> = node
        .inputs
        .iter()
        .filter(|p| p.slot.is_some())
        .map(|p| Row::Input(p.id.clone()))
        .chain(node.settings.iter().map(|x| Row::Setting(x.id.clone())))
        .chain(desc.editor.views.iter().map(|v| Row::View(v.name.clone())))
        .filter(|r| !out.iter().any(|g| g.rows.contains(r)))
        .collect();
    if !more.is_empty() {
        out.push(CardGroup { name: "More".into(), rows: more });
    }
    out
}

/// Card `node`'s face (CARD-41), of a call of `desc`: its rows in the order the face names them,
/// empty when it names none.
pub(crate) fn face(desc: &Descriptor, node: &ViewNode) -> Vec<Row> {
    desc.editor.face.iter().flat_map(|name| rows_named(desc, node, name)).collect()
}

/// A call's code (CARD-41): `site`'s text in `text`, its file, on one line (`one_line`). An
/// effect's first argument, the sound it works on (its descriptor's first slot an audio input), is
/// written `…` when it is a call.
pub(crate) fn code(site: &m2_script::sites::Site, text: &str, desc: Option<&Descriptor>) -> Option<String> {
    let whole = text.get(site.start as usize..site.end as usize)?;
    let effect = desc.is_some_and(|d| {
        let first = lens::params(d).first().copied();
        d.inputs.iter().find(|i| Some(i.name.as_str()) == first).is_some_and(|i| i.kind == m2_script::component::PortKind::Audio)
    });
    // Only a call is `…`: the card before it shows it. A name (`w`, `bass.out`) is said as written,
    // since it says which sound, another track's in a song's mix (review F5).
    let shown = match site.args.first().filter(|_| effect) {
        Some(a) => {
            let (s, e) = a.span();
            let (s, e) = ((s - site.start) as usize, (e - site.start) as usize);
            match whole.get(s..e)?.contains('(') {
                true => format!("{}…{}", whole.get(..s)?, whole.get(e..)?),
                false => whole.to_string(),
            }
        }
        None => whole.to_string(),
    };
    Some(one_line(&shown))
}

/// Code on one line: its comments left out, and each line break, with the indents around it, made
/// one space, or nothing after an opening bracket and before a closing one (whose trailing comma
/// goes too). Code it can't read safely, a regex, a division or a template string (review F7), is
/// left as written: its first line, then `…` when it has more.
pub(crate) fn one_line(code: &str) -> String {
    let Some(clean) = uncommented(code) else {
        let mut lines = code.lines();
        let first = lines.next().unwrap_or_default().trim_end();
        return if lines.next().is_some() { format!("{first} …") } else { first.to_string() };
    };
    let mut out = String::new();
    for line in clean.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if out.is_empty() {
            out.push_str(line);
            continue;
        }
        let closes = line.starts_with([')', ']', '}']);
        if closes && out.ends_with(',') {
            out.pop();
        }
        let tight = out.ends_with(['(', '[']) || line.starts_with([')', ']']);
        if !tight {
            out.push(' ');
        }
        out.push_str(line);
    }
    out
}

/// `code` with its comments (`// …` to the line's end, `/* … */`) taken out, never inside a
/// string; None when it has a `/` that starts no comment (a regex, a division) or a template
/// string, which this reading can't tell apart safely.
fn uncommented(code: &str) -> Option<String> {
    let (mut out, mut chars) = (String::new(), code.chars().peekable());
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == '\\' {
                    out.extend(chars.next());
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '/' && chars.peek() == Some(&'/') => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            None if c == '/' && chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for n in chars.by_ref() {
                    if last == '*' && n == '/' {
                        break;
                    }
                    last = n;
                }
                // The comment and the spaces around it are one space, or none before a line's end or a comma.
                while out.ends_with([' ', '\t']) {
                    out.pop();
                }
                while chars.peek().is_some_and(|&n| n == ' ' || n == '\t') {
                    chars.next();
                }
                if !out.is_empty() && !out.ends_with('\n') && chars.peek().is_some_and(|&n| n != '\n' && n != ',') {
                    out.push(' ');
                }
            }
            None if c == '/' || c == '`' => return None,
            None => {
                if matches!(c, '"' | '\'') {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    Some(out)
}

/// Card `node`'s views (CARD-26), of call `site` of `desc`, made in module `m`; `notes` is the
/// notes lane that plays the call, when one does.
pub(crate) fn views(m: &Module, site: u32, desc: &Descriptor, node: &ViewNode, notes: Option<&LaneRef>) -> Vec<CardView> {
    desc.editor
        .views
        .iter()
        .map(|v| {
            let of = match &v.of {
                ViewOf::Keys { regions } => {
                    let setting = node.settings.iter().find(|x| &x.slot == regions).map(|x| x.id.clone()).unwrap_or_default();
                    let (regions, why) = match crate::sampler::key_regions(m, site) {
                        Some(r) => (r, None),
                        None => (vec![], Some("its recordings don't play through std's Sampler, so their keys can't be drawn".to_string())),
                    };
                    Drawn::Keys { setting, regions, lane: notes.cloned(), why }
                }
                ViewOf::Scope => Drawn::Scope {},
                ViewOf::Picture { x, y, live } => Drawn::Picture {
                    x: x.clone(),
                    y: y.clone(),
                    live: live.clone(),
                    values: picture_values(m, desc, node),
                    words: None,
                    lane: notes.cloned(),
                },
            };
            CardView { name: v.name.clone(), of }
        })
        .collect()
}

/// What card `node`'s picture reads (CARD-63), of a call of `desc` in module `m`: each of its
/// controls but a dictionary, then each setting, by name.
fn picture_values(m: &Module, desc: &Descriptor, node: &ViewNode) -> Vec<PictureValue> {
    let mut out = vec![];
    for c in desc.controls.iter().filter(|c| c.each.is_none()) {
        let none = PictureValue { name: c.name.clone(), value: serde_json::Value::Null, knob: None, why: None };
        let declared = c.unit.map(m2_ir::Unit::name);
        let v = match node.knobs.iter().find(|k| k.slot.as_deref() == Some(c.name.as_str())) {
            // A number written in another unit than its declared one isn't drawn in it (review V2).
            Some(k) if k.kind == crate::kind::Kind::Number && k.unit != declared => {
                PictureValue { why: Some(format!("it is written in {}, which this picture doesn't draw", unit_words(k.unit))), ..none }
            }
            Some(k) => PictureValue { value: k.value.clone(), knob: Some(k.id.clone()), ..none },
            None => {
                let input = node.inputs.iter().find(|p| p.slot.as_deref() == Some(c.name.as_str()));
                let lane = input.and_then(|p| p.lane.as_ref()).and_then(|l| m.lanes.iter().find(|x| x.key == l.lane));
                let held = lane.and_then(|l| l.default).map(|d| serde_json::json!(d));
                PictureValue { value: held.unwrap_or_default(), why: Some(why_not(m, input)), ..none }
            }
        };
        out.push(v);
    }
    for x in &node.settings {
        // A setting's value as a picture reads it: a number, a name, true or false. Regions and the
        // like are no picture's to read, and stay null.
        let plain = x.value.is_number() || x.value.is_string() || x.value.is_boolean();
        if plain && desc.settings.iter().any(|d| d.name == x.slot) {
            out.push(PictureValue { name: x.slot.clone(), value: x.value.clone(), knob: None, why: None });
        }
    }
    out
}

/// A picture to draw (CARD-64): what the runner is asked, with the view's values as they play,
/// each with its knob, for the server to put a lease's value on (CARD-68), and the card's label,
/// which names a picture that can't be drawn (CARD-65).
pub struct PictureCall {
    pub ask: m2_script::pictures::Ask,
    pub values: Vec<PictureValue>,
    pub label: String,
}

impl PictureCall {
    /// The ask at the view's values, with `playing` (a knob's id to the value a lease plays) on top.
    pub fn at(&self, playing: impl Fn(&str) -> Option<f64>) -> m2_script::pictures::Ask {
        let values: serde_json::Map<String, serde_json::Value> = self
            .values
            .iter()
            .map(|v| (v.name.clone(), v.knob.as_deref().and_then(&playing).map_or_else(|| v.value.clone(), |x| serde_json::json!(x))))
            .collect();
        m2_script::pictures::Ask { values: serde_json::Value::Object(values).to_string(), ..self.ask.clone() }
    }
}

/// What picture `view` of card `node` is drawn from as revision `rev` plays it: None when the card
/// has no such picture. Its runtime's key is what the code is (CARD-64): the hashes of every package
/// file the build read, and for a sound of the song's own, the song's code revision too; so an
/// edited folder dependency, or `m2 upgrade`, makes a new runtime (review G3).
pub fn picture_call(rev: &m2_session::Revision, node: &ViewNode, view: &str) -> Option<PictureCall> {
    let Drawn::Picture { values, .. } = &node.views.iter().find(|v| v.name == view)?.of else { return None };
    let cid = node.component.clone()?;
    let code = rev.compiled.code();
    let desc = code.components.get(&cid)?.clone();
    let own = !cid.contains(':');
    let song = own.then(|| {
        std::sync::Arc::new(m2_script::pictures::Song {
            key: rev.code_rev,
            code: rev.project.code.iter().map(|(p, t)| (p.clone(), t.to_string())).collect(),
            dependencies: rev.project.manifest.dependencies.keys().filter(|d| *d != m2_song::SDK).cloned().collect(),
        })
    });
    let packages: Vec<(String, std::path::PathBuf)> = code.decls.packages.iter().map(|(n, d)| (n.clone(), d.clone())).collect();
    let mut key = <sha2::Sha256 as sha2::Digest>::new();
    for (n, d) in &packages {
        sha2::Digest::update(&mut key, format!("{n}\0{}\0", d.display()));
    }
    for f in &code.package_files {
        sha2::Digest::update(&mut key, format!("{}\0{}\0", f.path, f.sha256));
    }
    if own {
        sha2::Digest::update(&mut key, format!("song {}", rev.code_rev));
    }
    let key: String = sha2::Digest::finalize(key).iter().map(|b| format!("{b:02x}")).collect();
    let ask = m2_script::pictures::Ask {
        key,
        packages,
        song,
        module: desc.module.clone(),
        descriptor: desc,
        view: view.to_string(),
        values: String::new(),
        memo: format!("{}\u{0}{view}", node.id),
    };
    let label = node.title.clone().unwrap_or_else(|| node.label.clone());
    Some(PictureCall { ask, values: values.clone(), label })
}

/// A unit in words, as a beginner reads it.
fn unit_words(u: Option<&str>) -> &'static str {
    match u {
        Some("hz") => "Hz",
        Some("db") => "dB",
        Some("gain") => "a gain",
        Some("semitones") => "semitones",
        Some("seconds") => "seconds",
        Some("beats") => "beats",
        Some("unit01") => "a level from 0 to 1",
        Some("bipolar") => "a value from −1 to 1",
        Some("scalar") => "a plain number",
        _ => "a number with no unit",
    }
}

/// Why the card can't move the value at input `p`, in words.
fn why_not(m: &Module, p: Option<&InputPort>) -> String {
    match p {
        Some(InputPort { lane: Some(l), .. }) => {
            let label = m.lanes.iter().find(|x| x.key == l.lane).map_or(l.lane.as_str(), |x| x.label());
            format!("it follows the {label} lane")
        }
        Some(InputPort { inlet: Some(_), .. }) => "a cable drives it".to_string(),
        Some(InputPort { readonly: Some(_), .. }) => "it is worked out by its code".to_string(),
        _ => "it can be changed only in its code".to_string(),
    }
}

#[cfg(test)]
#[path = "views_tests.rs"]
mod tests;

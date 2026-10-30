//! Instruments written as data (PACK-13): a pack's `instruments/<id>.toml`, read strictly, and the
//! TypeScript music2 makes of it, the component a musician would otherwise write.
//!
//! ```toml
//! label = "Felt Piano"
//! category = "Keys"
//! engine = "std:sampler"
//! mode = "gate"
//!
//! [[region]]
//! file = "samples/C4.wav"   # a path in the pack
//! root = 60
//! keys = [58, 62]
//! ```
//!
//! The code is laid out on the file's own lines (each region's object on its `[[region]]` line), so
//! a place in the code is a place in the file. Ranges, pairs and turns are checked where every
//! region list is, by `@m2/core` (PKG-15); everything else here, each with its line.

use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// The engines a data instrument may name.
pub const ENGINES: &[&str] = &["std:sampler"];

/// What is wrong with a data instrument, and where: its line and column, from 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

/// An instrument written as data, read.
#[derive(Clone, Debug, PartialEq)]
pub struct DataInstrument {
    pub label: String,
    pub category: String,
    pub description: String,
    pub tags: Vec<String>,
    pub engine: String,
    /// The controls' defaults it gives, in [`CONTROLS`]' order.
    pub controls: Vec<(&'static str, f64)>,
    pub mode: Option<String>,
    pub voices: Option<f64>,
    pub regions: Vec<Region>,
}

/// One `[[region]]`: its line, and its fields as written, in PKG-15's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub line: u32,
    pub fields: Vec<(&'static str, Field)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Text(String),
    Number(f64),
    Numbers(Vec<f64>),
    Texts(Vec<String>),
}

/// The Sampler's controls (`std/src/sampler.ts`): name, constructor, its default and its range.
/// `sampler_controls_are_stds` checks them against std's descriptor.
pub const CONTROLS: &[(&str, &str, f64, (f64, f64))] = &[
    ("tune", "semis", 0.0, (-24.0, 24.0)),
    ("level", "db", 0.0, (-24.0, 12.0)),
    ("attack", "secs", 0.0, (0.0, 2.0)),
    ("release", "secs", 0.05, (0.0, 5.0)),
    ("velocity", "frac", 1.0, (0.0, 1.0)),
];
/// The Sampler's `mode` choices and default, and its `voices` default.
pub const MODES: &[&str] = &["oneshot", "gate"];
pub const MODE: &str = "oneshot";
pub const VOICES: f64 = 8.0;
/// The most voices a call may have (`poly`'s, `core/script/sdk/core.ts`).
pub const MAX_VOICES: f64 = 128.0;

/// A data instrument's id (PACK-13): its file's name, `instruments/<id>.toml`, checked before any
/// code is made from it. A key (ID-1), whose name in code, [`Id::binding`], is a name a module may
/// export: never a word JavaScript keeps for itself. The only way to that name, so code is made
/// only from a name that passed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

/// Words JavaScript keeps for itself in a module (always strict), and the globals a song would
/// lose if an instrument took their name.
const RESERVED: &str = "arguments await break case catch class const continue debugger default delete do else enum eval \
                        export extends false finally for function if implements import in instanceof interface let new null \
                        package private protected public return static super switch this throw true try typeof var void \
                        while with yield undefined NaN Infinity";

impl Id {
    /// `id` as an instrument's id, or why it can't be one, in words that follow "it can't be one: ".
    /// The words never repeat `id`, which may be anything a file can be named.
    pub fn new(id: &str) -> Result<Id, String> {
        if !crate::Key::valid(id) {
            return Err("an id is 1 to 32 lowercase letters, digits and inner hyphens, such as `felt-piano`".into());
        }
        let b = camel(id);
        if RESERVED.split_whitespace().any(|w| w == b) {
            return Err(format!("its name in code, `{b}`, is a word JavaScript keeps for itself"));
        }
        Ok(Id(id.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The name it is exported by: its id in camelCase, `felt-piano` → `feltPiano`; one that starts
    /// with a digit gets `_` first, `808-kit` → `_808Kit`. Letters, digits and that `_` only, so
    /// never a name the module made from it uses (`__m2_…`).
    pub fn binding(&self) -> String {
        camel(&self.0)
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a value must be, said in words.
#[derive(Clone, Copy)]
enum Kind {
    Text,
    Texts,
    Number,
    Numbers,
}

const TOP: &[(&str, Kind)] = &[
    ("label", Kind::Text),
    ("category", Kind::Text),
    ("description", Kind::Text),
    ("tags", Kind::Texts),
    ("engine", Kind::Text),
    ("tune", Kind::Number),
    ("level", Kind::Number),
    ("attack", Kind::Number),
    ("release", Kind::Number),
    ("velocity", Kind::Number),
    ("mode", Kind::Text),
    ("voices", Kind::Number),
];

/// PKG-15's fields, in its order.
const REGION: &[(&str, Kind)] = &[
    ("file", Kind::Text),
    ("root", Kind::Number),
    ("keys", Kind::Numbers),
    ("velocity", Kind::Numbers),
    ("round_robin", Kind::Numbers),
    ("trigger", Kind::Text),
    ("loop", Kind::Numbers),
    ("tune", Kind::Number),
    ("volume", Kind::Number),
    ("pan", Kind::Number),
    ("offset", Kind::Number),
];

/// Line and column (from 1, the column in characters) of byte `at` in `text`.
pub fn place(text: &str, at: usize) -> (u32, u32) {
    let at = at.min(text.len());
    let before = text.get(..at).unwrap_or(text);
    let line = before.matches('\n').count() as u32 + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() as u32 + 1)
}

/// The known word nearest to `word`, when it is near enough to be a slip: at most a third of its
/// letters apart, and two at most.
pub fn nearest<'a>(word: &str, known: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (word.chars().count() / 3).clamp(1, 2);
    known.into_iter().map(|k| (distance(word, k), k)).filter(|(d, _)| *d <= limit).min_by_key(|(d, _)| *d).map(|(_, k)| k)
}

/// Edits (a letter added, removed, changed, or two side by side swapped) from `a` to `b`.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

impl DataInstrument {
    /// Read `text`, an `instruments/<id>.toml`, strictly (PACK-13): the first problem, at its place.
    pub fn read(text: &str) -> Result<DataInstrument, Problem> {
        let at = |span: std::ops::Range<usize>, message: String| {
            let (line, col) = place(text, span.start);
            Problem { line, col, message }
        };
        let table = DeTable::parse(text).map_err(|e| at(e.span().unwrap_or(0..0), e.message().trim_end().to_string()))?;
        let whole = table.span();
        let table = table.into_inner();
        let mut entries: Vec<(&Spanned<_>, &Spanned<DeValue>)> = table.iter().collect();
        entries.sort_by_key(|(k, _)| k.span().start);
        let mut out = DataInstrument {
            label: String::new(),
            category: String::new(),
            description: String::new(),
            tags: vec![],
            engine: String::new(),
            controls: vec![],
            mode: None,
            voices: None,
            regions: vec![],
        };
        let mut seen_regions = None;
        for (key, value) in entries {
            let name: &str = key.get_ref();
            if name == "region" {
                seen_regions = Some(key.span());
                out.regions = regions(text, value)?;
                continue;
            }
            let Some((name, kind)) = TOP.iter().find(|(k, _)| *k == name).copied() else {
                return Err(at(key.span(), unknown(name, TOP.iter().map(|(k, _)| *k).chain(["region"]), "")));
            };
            let field = value_of(value, kind).map_err(|m| at(value.span(), format!("`{name}` {m}")))?;
            let wrong = |message: String| Err(at(value.span(), message));
            let written = text.get(value.span()).unwrap_or("").trim();
            match (name, field) {
                ("label" | "category", Field::Text(s)) if s.trim().is_empty() => return wrong(format!("`{name}` must not be empty")),
                ("label", Field::Text(s)) => out.label = s,
                ("category", Field::Text(s)) => out.category = s,
                ("description", Field::Text(s)) => out.description = s,
                ("engine", Field::Text(s)) if !ENGINES.contains(&s.as_str()) => {
                    return wrong(format!(
                        "`engine = {s:?}`: music2 plays data instruments only with std's Sampler for now: write `engine = \"std:sampler\"`"
                    ));
                }
                ("engine", Field::Text(s)) => out.engine = s,
                ("mode", Field::Text(s)) if !MODES.contains(&s.as_str()) => {
                    return wrong(format!("`mode = {s:?}` must be \"oneshot\" or \"gate\""));
                }
                ("mode", Field::Text(s)) => out.mode = Some(s),
                ("tags", Field::Texts(t)) => out.tags = t,
                ("voices", Field::Number(n)) if n.fract() != 0.0 || !(1.0..=MAX_VOICES).contains(&n) => {
                    return wrong(format!("`voices` must be a whole number from 1 to {}, not {written}", number(MAX_VOICES)));
                }
                ("voices", Field::Number(n)) => out.voices = Some(n),
                (_, Field::Number(n)) => {
                    let (lo, hi) = CONTROLS.iter().find(|c| c.0 == name).map(|c| c.3).expect("a number in TOP is a control");
                    if !(lo..=hi).contains(&n) {
                        return wrong(format!("`{name}` must be from {} to {}, not {written}", number(lo), number(hi)));
                    }
                    out.controls.push((name, n))
                }
                _ => unreachable!("value_of gives the kind asked for"),
            }
        }
        out.controls.sort_by_key(|(k, _)| CONTROLS.iter().position(|c| c.0 == *k));
        let end = |what: &str| at(whole.end.saturating_sub(1)..whole.end, what.to_string());
        let start = at(0..0, String::new());
        for (key, value) in [("label", &out.label), ("category", &out.category), ("engine", &out.engine)] {
            if value.is_empty() {
                let hint = match key {
                    "label" => "label = \"Felt Piano\"",
                    "category" => "category = \"Keys\"",
                    _ => "engine = \"std:sampler\"",
                };
                return Err(Problem { message: format!("`{key}` is missing: add `{hint}` at the top"), ..start });
            }
        }
        if out.regions.is_empty() {
            let message = "no `[[region]]`: an instrument plays at least one recording, as `[[region]]` then `file = \"samples/C4.wav\"`";
            return Err(match seen_regions {
                Some(span) => at(span, message.into()),
                None => end(message),
            });
        }
        Ok(out)
    }

    /// The TypeScript of this instrument as `<package>:<id>`, exported as [`Id::binding`], laid
    /// out on the file's lines: the component on line 1, each region on its own line, and the
    /// imports last (they are hoisted). A function of the instrument alone. Its own names all
    /// start `__m2_`, which no instrument's can (PACK-13).
    pub fn module(&self, package: &str, id: &Id) -> String {
        let q = |s: &str| serde_json::to_string(s).expect("a string");
        let head = format!("{} const __m2_regions: readonly __m2_Region[] = [", self.component(id, "__m2_regions"));
        let last = self.regions.iter().map(|r| r.line).max().unwrap_or(1).max(1) as usize;
        let mut lines = vec![String::new(); last];
        lines[0] = head;
        for r in &self.regions {
            let fields: Vec<String> = r
                .fields
                .iter()
                .map(|(k, f)| {
                    let v = match f {
                        Field::Text(s) if *k == "file" => q(&format!("{package}:{s}")),
                        Field::Text(s) => q(s),
                        Field::Number(n) => number(*n),
                        Field::Numbers(ns) => format!("[{}]", ns.iter().map(|n| number(*n)).collect::<Vec<_>>().join(", ")),
                        Field::Texts(ts) => format!("[{}]", ts.iter().map(|t| q(t)).collect::<Vec<_>>().join(", ")),
                    };
                    format!("{k}: {v}")
                })
                .collect();
            let line = &mut lines[r.line as usize - 1];
            line.push_str(&format!(" {{ {} }},", fields.join(", ")));
        }
        lines.push(format!("]; {IMPORTS}"));
        lines.join("\n") + "\n"
    }

    /// `export const <binding> = component(…);` on one line: the descriptor, and a body that plays
    /// `regions` (an expression) on std's Sampler with the instrument's controls and settings.
    fn component(&self, id: &Id, regions: &str) -> String {
        let q = |s: &str| serde_json::to_string(s).expect("a string");
        let controls: Vec<String> = CONTROLS
            .iter()
            .map(|(name, ctor, default, (lo, hi))| {
                let v = self.controls.iter().find(|(k, _)| k == name).map_or(*default, |(_, v)| *v);
                format!("{name}: __m2_control.{ctor}({}).range({}, {})", number(v), number(*lo), number(*hi))
            })
            .collect();
        let mode = self.mode.as_deref().unwrap_or(MODE);
        let voices = self.voices.unwrap_or(VOICES);
        let modes: Vec<String> = MODES.iter().map(|m| q(m)).collect();
        let pass: Vec<String> = CONTROLS
            .iter()
            .map(|c| format!("{0}: __m2_p.{0}", c.0))
            .chain(["mode: __m2_p.mode".into(), "voices: __m2_p.voices".into()])
            .collect();
        format!(
            "export const {} = __m2_component({{ id: {}, label: {}, signature: __m2_signature<(notes: __m2_Events) => __m2_Audio>(), \
             inputs: {{ notes: __m2_port.notes() }}, controls: {{ {} }}, settings: {{ mode: __m2_setting.choice([{}], {}), voices: __m2_setting.int({}) }}, \
             output: __m2_port.audio(), editor: {{ category: {}, description: {} }} }}, \
             (__m2_notes: __m2_Events, __m2_p: __m2_Params): __m2_Audio => __m2_sampler(__m2_notes, {{ regions: {regions}, {} }}));",
            id.binding(),
            q(id.as_str()),
            q(&self.label),
            controls.join(", "),
            modes.join(", "),
            q(mode),
            number(voices),
            q(&self.category),
            q(&self.description),
            pass.join(", "),
        )
    }
}

/// What the module made from a data instrument imports, each under a name no instrument's can be.
const IMPORTS: &str = "import { component as __m2_component, control as __m2_control, port as __m2_port, setting as __m2_setting, \
                       signature as __m2_signature, type Audio as __m2_Audio, type Events as __m2_Events, type Params as __m2_Params, \
                       type Region as __m2_Region } from \"@m2/core\"; import { sampler as __m2_sampler } from \"std/sampler\";";

/// What a code editor checks `import { feltPiano } from "<package>/instruments"` against: one
/// module declaring each instrument (its file's name, its text) that reads, as its component, with
/// no regions (types need none). One whose name isn't an id, whose name in code an earlier one has,
/// or that doesn't read is left out: the build says why.
pub fn types(instruments: &[(String, String)]) -> String {
    let mut out = format!("// The pack's instruments written as data (instruments/*.toml), as code editors see them.\n{IMPORTS}\n");
    let mut taken = std::collections::BTreeSet::new();
    for (id, text) in instruments {
        let Ok(id) = Id::new(id) else { continue };
        if let Ok(i) = DataInstrument::read(text)
            && taken.insert(id.binding())
        {
            out.push_str(&i.component(&id, "[]"));
            out.push('\n');
        }
    }
    out
}

/// `id` in camelCase, `_` first when it starts with a digit: see [`Id::binding`].
fn camel(id: &str) -> String {
    let mut out = String::new();
    for (i, part) in id.split('-').filter(|p| !p.is_empty()).enumerate() {
        let mut c = part.chars();
        match (i, c.next()) {
            (0, Some(f)) => out.extend(std::iter::once(f).chain(c)),
            (_, Some(f)) => out.extend(f.to_uppercase().chain(c)),
            _ => {}
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// A number as JavaScript reads it back, the same on every OS: whole ones without a point.
fn number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

fn unknown<'a>(name: &str, known: impl IntoIterator<Item = &'a str> + Clone, place: &str) -> String {
    match nearest(name, known.clone()) {
        Some(k) => format!("`{name}`{place}: no such key; did you mean `{k}`?"),
        None => format!("`{name}`{place}: no such key (keys: {})", known.into_iter().collect::<Vec<_>>().join(", ")),
    }
}

/// A value of `kind`, or what is wrong with it, after the key's name.
fn value_of(v: &Spanned<DeValue>, kind: Kind) -> Result<Field, String> {
    let num = |v: &DeValue| -> Option<f64> {
        match v {
            DeValue::Integer(i) => i64::from_str_radix(&i.as_str().replace('_', ""), i.radix()).ok().map(|i| i as f64),
            DeValue::Float(f) => f.as_str().replace('_', "").parse::<f64>().ok().filter(|f| f.is_finite()),
            _ => None,
        }
    };
    let shown = |v: &DeValue| match v {
        DeValue::String(s) => format!("the text {s:?}"),
        other => format!("{} {}", article(other.type_str()), other.type_str()),
    };
    match kind {
        Kind::Text => v
            .get_ref()
            .as_str()
            .map(|s| Field::Text(s.to_string()))
            .ok_or_else(|| format!("must be text in quotes, not {}", shown(v.get_ref()))),
        Kind::Number => num(v.get_ref()).map(Field::Number).ok_or_else(|| format!("must be a number, not {}", shown(v.get_ref()))),
        Kind::Texts => match v.get_ref().as_array() {
            Some(a) => a
                .iter()
                .map(|x| x.get_ref().as_str().map(String::from))
                .collect::<Option<Vec<String>>>()
                .map(Field::Texts)
                .ok_or_else(|| "must be a list of words in quotes, as [\"piano\", \"soft\"]".into()),
            None => Err("must be a list of words in quotes, as [\"piano\", \"soft\"]".into()),
        },
        Kind::Numbers => match v.get_ref().as_array() {
            Some(a) => a
                .iter()
                .map(|x| num(x.get_ref()))
                .collect::<Option<Vec<f64>>>()
                .map(Field::Numbers)
                .ok_or_else(|| "must be numbers, as [60, 64]".into()),
            None => Err(format!("must be two numbers, as [60, 64], not {}", shown(v.get_ref()))),
        },
    }
}

fn article(word: &str) -> &'static str {
    if word.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" }
}

/// The `[[region]]` tables, each read strictly, at its line: the line of its `[[region]]` header,
/// or of its `{` in a list written on one line each.
fn regions(text: &str, value: &Spanned<DeValue>) -> Result<Vec<Region>, Problem> {
    let at = |span: std::ops::Range<usize>, message: String| {
        let (line, col) = place(text, span.start);
        Problem { line, col, message }
    };
    let Some(list) = value.get_ref().as_array() else {
        return Err(at(value.span(), "`region` must be `[[region]]` tables, each a recording".into()));
    };
    let mut out = vec![];
    for (i, item) in list.iter().enumerate() {
        let Some(table) = item.get_ref().as_table() else {
            return Err(at(
                item.span(),
                format!("region {}: must be a table of fields, as `[[region]]` then `file = \"samples/C4.wav\"`", i + 1),
            ));
        };
        let start = header(text, item.span().start);
        let (line, _) = place(text, start);
        let mut entries: Vec<_> = table.iter().collect();
        entries.sort_by_key(|(k, _)| k.span().start);
        let mut fields = vec![];
        for (key, v) in entries {
            let name: &str = key.get_ref();
            let Some((name, kind)) = REGION.iter().find(|(k, _)| *k == name).copied() else {
                return Err(at(key.span(), unknown(name, REGION.iter().map(|(k, _)| *k), &format!(" in region {}", i + 1))));
            };
            let field = value_of(v, kind).map_err(|m| at(v.span(), format!("region {}: `{name}` {m}", i + 1)))?;
            if let (Kind::Text, Field::Text(s)) = (kind, &field) {
                let wrong = match name {
                    "file" => file_problem(s),
                    _ if s != "attack" && s != "release" => Some(format!("`trigger = {s:?}` must be \"attack\" or \"release\"")),
                    _ => None,
                };
                if let Some(m) = wrong {
                    return Err(at(v.span(), format!("region {}: {m}", i + 1)));
                }
            }
            fields.push((name, field));
        }
        if !fields.iter().any(|(k, _)| *k == "file") {
            return Err(at(start..start, format!("region {}: no `file`: name its recording, as `file = \"samples/C4.wav\"`", i + 1)));
        }
        fields.sort_by_key(|(k, _)| REGION.iter().position(|(r, _)| r == k));
        out.push(Region { line, fields });
    }
    Ok(out)
}

/// Where the table at byte `at` starts: its `[[region]]` header, when it has one before it on
/// the lines above (a table's span starts at its first key), else `at`.
fn header(text: &str, at: usize) -> usize {
    let before = &text[..at.min(text.len())];
    match before.rfind("[[") {
        Some(h) if before[h..].trim_start_matches("[[").trim_start().starts_with("region") && !before[..h].ends_with('"') => {
            // Only when nothing but the header, blank lines and comments come between.
            let between = &before[h..];
            let after_header = between.find(']').unwrap_or(between.len());
            let rest = &between[after_header..];
            let plain = rest.lines().skip(1).all(|l| l.trim().is_empty() || l.trim_start().starts_with('#'));
            if plain { h } else { at }
        }
        _ => at,
    }
}

/// Why `path` isn't a plain path in the pack, if it isn't (PACK-5): relative, `/`-separated, no
/// empty, `.`-led or `..` part, no `\`, no `:`.
fn file_problem(path: &str) -> Option<String> {
    let plain = !path.is_empty()
        && !path.contains(':')
        && path.split('/').all(|seg| !seg.is_empty() && !seg.starts_with('.'))
        && !path.chars().any(|c| c == '\\' || c.is_control());
    (!plain)
        .then(|| format!("`file = {path:?}` must be a path in this pack, such as \"samples/C4.wav\": no `:`, no `..`, no `/` at the start"))
}

#[cfg(test)]
#[path = "instrument_tests.rs"]
mod tests;

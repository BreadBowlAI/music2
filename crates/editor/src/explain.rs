//! Errors in plain words (the studio plan's D2, Q45): for the common problems, a sentence that
//! says what went wrong and a fix, made here from the problem's code and its message. Core keeps
//! its own, exact words (P10): the app shows them under Details. A problem no rule knows keeps its
//! own words (`None`).

use std::collections::{BTreeMap, BTreeSet};

use m2_session::Session;

/// A problem in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plain {
    /// What went wrong, as a sentence: "Cutoff wants a frequency (Hz), but gets a level (dB)."
    pub words: String,
    /// What to do about it: "Write it in Hz, such as hz(800)."
    pub fix: Option<String>,
}

/// What explaining needs to know of the song.
#[derive(Clone, Debug, Default)]
pub struct Song {
    /// Its track files' names, as `./tracks/…` imports name them: kick, bass, keys.
    pub tracks: Vec<String>,
    /// Each name a package exports, with the module it is imported from: `pluck` → `std/instruments`.
    pub exports: BTreeMap<String, String>,
    /// What to call each track, by its ID: bass → Bass.
    pub names: BTreeMap<String, String>,
}

impl Song {
    /// The song `s` has open: its files, what its packages export, and its tracks' names.
    pub fn of(s: &Session) -> Song {
        let mut song = Song::named(s);
        song.exports = exports(s);
        song
    }

    /// The song `s` has open without what its packages export (which reads their sources): enough
    /// for the revision's problems, which name tracks and files, never an import.
    pub fn named(s: &Session) -> Song {
        let rev = s.accepted();
        // From the folder: a song whose code never compiled has no code in its revision.
        let mut tracks: Vec<String> = std::fs::read_dir(rev.project.dir.join("tracks"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| Some(e.file_name().to_str()?.strip_suffix(".ts")?.to_string()))
            .collect();
        tracks.sort();
        let names = rev.compiled.code().ir.modules.iter().map(|m| (m.key.clone(), m.label().to_string())).collect();
        Song { tracks, exports: BTreeMap::new(), names }
    }

    fn name(&self, key: &str) -> String {
        self.names.get(key).cloned().unwrap_or_else(|| title(key))
    }
}

/// Each name the packages of `s` export, with the module it is imported from: every top-level
/// `export const`, `export let` and `export function` of each package's own modules, then the
/// catalog's components where each is exported first (PKG-11).
fn exports(s: &Session) -> BTreeMap<String, String> {
    let mut exports = BTreeMap::new();
    for p in s.packages.packages() {
        let Ok(entries) = std::fs::read_dir(p.dir.join("src")) else { continue };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let (Some(stem), Some("ts")) = (path.file_stem().and_then(|s| s.to_str()), path.extension().and_then(|s| s.to_str())) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let module = if stem == "index" { p.name.clone() } else { format!("{}/{stem}", p.name) };
            for name in exported(&text) {
                exports.entry(name).or_insert_with(|| module.clone());
            }
        }
    }
    for list in s.catalog().exports.values() {
        if let Some(e) = list.iter().find(|e| !e.name.contains('.')) {
            exports.entry(e.name.clone()).or_insert_with(|| e.module.clone());
        }
    }
    exports
}

/// The names `export const`, `export let` and `export function` declare at the start of a line.
fn exported(text: &str) -> Vec<String> {
    let mut out = vec![];
    for line in text.lines() {
        let rest = ["export const ", "export function ", "export let "].iter().find_map(|p| line.strip_prefix(p));
        if let Some(rest) = rest {
            let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

/// Where a problem is: its file, line and column, the text of that line, and the track it is about.
#[derive(Clone, Copy, Debug, Default)]
pub struct At<'a> {
    pub file: Option<&'a str>,
    pub line: Option<u32>,
    /// 1-based, in characters.
    pub col: Option<u32>,
    /// The text of `line` in `file`.
    pub text: Option<&'a str>,
    /// The ID of the track it is about.
    pub module: Option<&'a str>,
}

/// The problem with `code` and `message`, at `at` in `song`, in plain words, or `None` for one no
/// rule knows, which keeps its own words.
pub fn explain(code: &str, message: &str, song: &Song, at: &At) -> Option<Plain> {
    let plain = |words: String, fix: String| Some(Plain { words, fix: Some(fix) });
    let file = at.file.unwrap_or("the code");
    match code {
        // Several mistakes at one node are joined with "; ": the first one known speaks.
        "code.check" => message.split("; ").find_map(|part| check(part, at)),
        "code.build" => {
            if let Some(name) =
                message.strip_suffix(" is not defined").map(|n| n.trim_start_matches("ReferenceError: ")).filter(|n| ident(n))
            {
                let import = |n: &str, m: &str| format!("Import it: `import {{ {n} }} from \"{m}\"`.");
                let fix = match song.exports.get(name) {
                    Some(module) => import(name, module),
                    None => match near(name, song.exports.keys()) {
                        Some(n) => format!("Did you mean {n}? {}", import(n, &song.exports[n])),
                        None => "Check its spelling.".into(),
                    },
                };
                return plain(format!("`{name}` isn't known in {file}."), fix);
            }
            // A component called without its sound: `reverb(): the input in is missing`.
            if let Some((id, port)) =
                message.split_once("(): the input ").and_then(|(id, rest)| Some((id, rest.strip_suffix(" is missing")?)))
            {
                return Some(empty(&title(id), port));
            }
            if PARSE.iter().any(|p| message.starts_with(p)) {
                let words = match at.line {
                    Some(l) => format!("There is a typo on line {l} of {file}."),
                    None => format!("There is a typo in {file}."),
                };
                return plain(words, "Check the brackets, commas and quotes on that line and the one before.".into());
            }
            if message.ends_with("not a function") {
                let what = callee(at).map_or("Something".to_string(), |c| format!("`{c}`"));
                let line = at.line.map_or(String::new(), |l| format!(" on line {l}"));
                return plain(
                    format!("{what}{line} of {file} is used like an instrument or effect, but it isn't one."),
                    "Check the name before the brackets: is it spelled right, and imported?".into(),
                );
            }
            // An import of a package's module that isn't there: `` `std/nope`: file not found ``.
            if let Some(spec) = message.strip_prefix('`').and_then(|m| m.strip_suffix("`: file not found")) {
                return missing_import(spec, file, song);
            }
            let (spec, from) = unresolved(message)?;
            missing_import(&spec, &from, song)
        }
        "import.outside-code" => {
            let spec = message.strip_prefix('`')?.split_once("` names no code file")?.0;
            missing_import(spec, file, song)
        }
        "pkg.undeclared" => {
            let pkg = message.split_once("from the package `")?.1.split_once('`')?.0;
            plain(
                format!("{file} uses {pkg}, but music2.toml doesn't list it."),
                format!("Add `{pkg} = \"…\"` under `[dependencies]` in music2.toml."),
            )
        }
        "key.duplicate" => {
            let id = quoted(message, '"').or_else(|| quoted(message, '`'))?;
            plain(
                format!("Two tracks have the ID {id}."),
                "Give one another ID: in Code, right-click its file, then Change track ID….".into(),
            )
        }
        "plugin.missing" => {
            let track = at.module.map(|m| song.name(m));
            // An instrument plays silence; an effect passes its input through (FMT-18a).
            let silent = !message.contains("passes its audio through");
            match (track, silent) {
                (Some(t), true) => plain(
                    format!("{t} plays a plugin that isn't installed on this computer, so it is silent."),
                    format!("Install it, or pick another instrument: right-click {t}, then Change instrument…."),
                ),
                (Some(t), false) => plain(
                    format!("An effect on {t} is a plugin that isn't installed on this computer, so it lets the sound through unchanged."),
                    "Install it, or take the effect out in Device.".into(),
                ),
                (None, _) => plain(
                    "A plugin isn't installed on this computer, so it plays nothing.".into(),
                    "Install it, or pick another sound.".into(),
                ),
            }
        }
        "asset.missing" => {
            let name = quoted(message, '`')?;
            if message.contains("` isn't in assets/") {
                plain(
                    format!("{name} isn't in the song's assets folder, so its clip plays nothing."),
                    "Put the file back, or delete the clip.".into(),
                )
            } else if message.contains("` can't be read") {
                plain(
                    format!("{name} can't be read, so its clip plays nothing."),
                    "Put a good copy of the file back, or delete the clip.".into(),
                )
            } else {
                None
            }
        }
        "lane.invalid" => {
            let data = at.file.filter(|f| f.starts_with("data/") && f.ends_with(".json"))?;
            let track = at.module.map(|m| song.name(m)).unwrap_or_else(|| "its track".into());
            let then = if message.contains("plays the data it had before") {
                format!("{track} plays what it had")
            } else if message.contains("No lane reads it now") {
                "nothing else changes".to_string()
            } else {
                format!("{track} plays nothing from it")
            };
            plain(format!("{data} can't be read, so {then}."), "Undo the last change to it, or fix the file.".into())
        }
        _ => None,
    }
}

/// How the parser's messages start (oxc's): a typo, not a mistake the song makes as it runs.
const PARSE: [&str; 9] = [
    "Expected ",
    "Unexpected ",
    "Unterminated ",
    "Parenthesized expressions may not have a trailing comma",
    "Empty parenthesized expression",
    "Missing ",
    "Identifier expected",
    "Invalid Character",
    "Lexical declaration cannot appear",
];

/// One mistake the graph check found at a node, in plain words.
fn check(part: &str, at: &At) -> Option<Plain> {
    let (port, rest) = part.strip_prefix("input `")?.split_once('`')?;
    if let Some(kinds) = rest.strip_prefix(" expects control<") {
        let (want, got) = kinds.split_once(">, got control<")?;
        let got = got.strip_suffix('>')?;
        let (want, fix) = unit(want)?;
        let got = unit(got).map_or(got, |(words, _)| words);
        return Some(Plain { words: format!("{} wants {want}, but gets {got}.", title(port)), fix: Some(fix.into()) });
    }
    let thing = callee(at).map(|c| title(c.rsplit('.').next().unwrap_or(c))).unwrap_or_else(|| "It".into());
    if rest.ends_with(") is not connected") {
        return Some(empty(&thing, port));
    }
    if rest.starts_with(" is mono") && rest.ends_with("but a stereo signal is connected") {
        return Some(Plain {
            words: format!("{thing} takes one channel, but gets two."),
            fix: Some("Make it mono first, or use the stereo version.".into()),
        });
    }
    None
}

/// `thing`'s input `port` has nothing in it.
fn empty(thing: &str, port: &str) -> Plain {
    let words = if port == "in" {
        format!("{thing} needs a sound to work on, but it gets none.")
    } else {
        format!("{thing} needs something to work on: its {port} input is empty.")
    };
    Plain { words, fix: Some("Give it a sound first, such as `reverb(bass.out)`.".into()) }
}

/// A unit's words, and how to write one.
fn unit(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "hz" => ("a frequency (Hz)", "Write it in Hz, such as hz(800)."),
        "db" => ("a level (dB)", "Write it in dB, such as db(-6)."),
        "seconds" => ("a time", "Write it as a time, such as ms(250) or secs(1)."),
        "semitones" => ("semitones", "Write it in semitones, such as semis(7)."),
        "beats" => ("beats", "Write it in beats, such as beats(1)."),
        "unit01" => ("a number from 0 to 1", "Write a number from 0 to 1, such as 0.5."),
        "bipolar" => ("a number from −1 to 1", "Write a number from −1 to 1, such as 0.5."),
        "gain" => ("a volume as a number (1 keeps it as loud)", "Write a number, such as 0.5 for half as loud."),
        "scalar" => ("a plain number", "Write a plain number, such as 2."),
        _ => return None,
    })
}

/// "{from} brings in `{spec}`, which isn't there", with the song's track files, or the package's modules.
fn missing_import(spec: &str, from: &str, song: &Song) -> Option<Plain> {
    let words = format!("{from} brings in `{spec}`, which isn't there.");
    let fix = if spec.starts_with('.') {
        if song.tracks.is_empty() { "Check its name.".to_string() } else { format!("The track files are: {}.", song.tracks.join(", ")) }
    } else {
        let pkg = spec.split('/').next().unwrap_or(spec);
        let modules: BTreeSet<&str> = song.exports.values().map(String::as_str).filter(|m| m.split('/').next() == Some(pkg)).collect();
        if modules.is_empty() {
            "Check its name.".to_string()
        } else {
            format!("Check its name: {pkg} has {}.", modules.into_iter().collect::<Vec<_>>().join(", "))
        }
    };
    Some(Plain { words, fix: Some(fix) })
}

/// `Error resolving module 'std/nope' from 'tracks/bass.ts': …` → the specifier and the importing file.
fn unresolved(message: &str) -> Option<(String, String)> {
    let rest = message.split_once("Error resolving module '")?.1;
    let (spec, rest) = rest.split_once("' from '")?;
    let (from, _) = rest.split_once('\'')?;
    Some((spec.to_string(), from.to_string()))
}

/// The first text between two `q`s.
fn quoted(s: &str, q: char) -> Option<&str> {
    let (_, rest) = s.split_once(q)?;
    Some(rest.split_once(q)?.0).filter(|x| !x.is_empty())
}

/// Whether `s` is a JavaScript name.
fn ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// The callee written where `at` points, if a call starts there: `lowpass` in `lowpass(x, db(-3))`, `osc.saw`.
fn callee<'a>(at: &At<'a>) -> Option<&'a str> {
    let text = at.text?;
    let start = text.char_indices().nth(at.col?.checked_sub(1)? as usize)?.0;
    let rest = &text[start..];
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$' || c == '.')).unwrap_or(rest.len());
    let name = rest[..end].trim_end_matches('.');
    (!name.is_empty() && rest[end..].starts_with('(')).then_some(name)
}

/// The name of `names` nearest `name`, if one is a slip away (one or two letters changed, added
/// or dropped), for names of four letters or more.
fn near<'a>(name: &str, names: impl Iterator<Item = &'a String>) -> Option<&'a str> {
    if name.chars().count() < 4 {
        return None;
    }
    let lower = name.to_lowercase();
    names
        .map(|n| (distance(&lower, &n.to_lowercase()), n))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, n)| (*d, n.len()))
        .map(|(_, n)| n.as_str())
}

/// The edit distance between two names (Levenshtein), in characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != b[j])).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// `cutoff` → `Cutoff`, `send_a` → `Send a`.
fn title(s: &str) -> String {
    let s = s.replace('_', " ");
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;

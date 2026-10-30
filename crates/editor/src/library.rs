//! The library (LIB-1): everything that can be added to a song, as one list of items, and the one
//! search over it that ⌘F, the side browser and `m2 search` all call.
//!
//! [`items`] gathers the list from its [`Sources`]: the components a build offers with a category
//! (PKG-7), each preset as a sound (FMT-22, std's and yours), and the installed plugins once the app
//! has listed them. It is the one place a new kind of item plugs in: samples, from packs, are one
//! more source there, and [`Item::sample`] and the filters [`Query`] reads are ready for them.
//!
//! [`search`] keeps the items whose every word matches (a word of the label, inside the label, a
//! tag, a word of the description) and meets every filter typed (a kind, `loop`, `one-shot`, a
//! tempo, a key), ranked: the label itself, then a label that starts with what was typed, then by
//! each item's weakest word; then std's and ours before yours, before plugins; then by kind, label
//! and id. The same list and words give the same order every time.
//!
//! Previews (LIB-3) are in [`preview`].

use std::collections::BTreeSet;
use std::path::Path;

use m2_script::Catalog;
use m2_script::component::Descriptor;
use serde::Serialize;

use crate::presets;

pub mod preview;

/// What an item is. The order is the order of the panel's groups, and of kinds in a ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Instrument,
    Sound,
    Effect,
    Sample,
    Plugin,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Instrument, Kind::Sound, Kind::Effect, Kind::Sample, Kind::Plugin];

    /// Its name, as a filter and the API write it: `instrument`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Instrument => "instrument",
            Kind::Sound => "sound",
            Kind::Effect => "effect",
            Kind::Sample => "sample",
            Kind::Plugin => "plugin",
        }
    }

    /// The kind a word typed names: `instrument` or `instruments`, `effect`, `effects` or `fx`,
    /// `sound`, `sounds`, `preset` or `presets`, `sample` or `samples`, `plugin` or `plugins`.
    pub fn named(word: &str) -> Option<Kind> {
        let w = word.to_lowercase();
        let one = w.strip_suffix('s').filter(|_| w != "fx").unwrap_or(&w);
        match one {
            "instrument" => Some(Kind::Instrument),
            "effect" | "fx" => Some(Kind::Effect),
            "sound" | "preset" => Some(Kind::Sound),
            "sample" => Some(Kind::Sample),
            "plugin" => Some(Kind::Plugin),
            _ => None,
        }
    }
}

/// A sample's type: a loop plays in time with the song; a one-shot plays once per note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SampleType {
    #[serde(rename = "loop")]
    Loop,
    #[serde(rename = "one-shot")]
    OneShot,
}

/// A musical key: its root as a pitch class (0 is C, 1 is C♯ or D♭…) and whether it is minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub root: u8,
    pub minor: bool,
}

const ROOTS: [&str; 12] = ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

impl Key {
    /// A key as typed: a capital letter A to G, then `#`, `♯`, `b` or `♭` if any, then `m` or `min`
    /// for minor, or `maj`. `Am`, `F#`, `Bbm`, `Cmaj`. A bare letter isn't one (it is the start of a
    /// word, `B` of `Bass`): with no sign it needs `major` or `minor` after it ([`Query::parse`]).
    pub fn parse(text: &str) -> Option<Key> {
        let mut chars = text.chars();
        let letter = chars.next()?;
        let base = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let rest: String = chars.collect();
        let (shift, rest) = match rest.chars().next() {
            Some('#' | '♯') => (1i8, &rest[rest.chars().next()?.len_utf8()..]),
            Some('b' | '♭') => (-1, &rest[rest.chars().next()?.len_utf8()..]),
            _ => (0, rest.as_str()),
        };
        let minor = match rest {
            "m" | "min" => true,
            "" | "maj" => false,
            _ => return None,
        };
        if shift == 0 && rest.is_empty() {
            return None;
        }
        Some(Key { root: (base + 12 + shift) as u8 % 12, minor })
    }

    /// `A minor`, `F# major`.
    pub fn words(self) -> String {
        format!("{} {}", ROOTS[self.root as usize], if self.minor { "minor" } else { "major" })
    }
}

impl Serialize for Key {
    /// `Am`, `F#`: as a sample's tags write it.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}{}", ROOTS[self.root as usize], if self.minor { "m" } else { "" }))
    }
}

/// What a sample is, beyond its words: what the filters read.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Sample {
    #[serde(rename = "type")]
    pub kind: SampleType,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
    pub seconds: Option<f64>,
}

/// A sound's preset: where it is from (`std` or `yours`) and its name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PresetOf {
    pub from: String,
    pub name: String,
}

/// One thing that can be added to a song.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Item {
    pub kind: Kind,
    /// Its id in the library: a component's ComponentId (`std:synth-pluck`), a plugin's
    /// `clap:<id>`, a sound's `sound:<from>/<sound>/<name>`.
    pub id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    /// More words it is found by: what code calls it (`synth.pluck`), its category, what its notes
    /// play (a drum kit's `Snare`), a plugin's vendor.
    pub tags: Vec<String>,
    /// Where it comes from: a package (`std`), `song` (the song's own code), `yours` (your
    /// presets) or `plugin`.
    pub package: String,
    /// What adding it names: a ComponentId or a plugin's `clap:<id>`; for a sound, its sound's.
    pub component: String,
    /// Adding it makes a new track: it plays notes (an instrument, an instrument plugin, a sound of
    /// an instrument).
    pub instrument: bool,
    /// It has a preview (LIB-3): not a plugin, a plugin's preset or the song's own code.
    pub preview: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<PresetOf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<Sample>,
}

/// An installed plugin, as the app lists it (`GET /api/plugins`).
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin {
    /// `clap:<id>`.
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub instrument: bool,
    pub effect: bool,
}

/// Where the library's items come from (LIB-1). A new source is one more field here and one more
/// line in [`items`].
pub struct Sources<'a> {
    /// The components a build offers: the open song's packages and its own code, or std alone.
    pub catalog: &'a Catalog,
    /// std's folder, for its presets (`std/presets`).
    pub std: Option<&'a Path>,
    /// Your Presets folder (`m2_os::dirs::presets`).
    pub yours: Option<&'a Path>,
    /// The installed plugins, once the app has listed them; none otherwise.
    pub plugins: &'a [Plugin],
}

/// Every item the sources give, each once (the first source's), in the order [`search`] gives with
/// no words.
pub fn items(src: &Sources) -> Vec<Item> {
    let mut out = components(src.catalog);
    out.extend(sounds(src, &out));
    out.extend(plugins(src.plugins));
    let mut seen = BTreeSet::new();
    out.retain(|i| seen.insert(i.id.clone()));
    let all = Query::default();
    search(&out, &all).into_iter().cloned().collect()
}

/// The package a ComponentId is from: `std` of `std:delay`; `song` for the song's own.
fn package_of(component: &str) -> String {
    component.split_once(':').map_or("song", |(p, _)| p).to_string()
}

/// The words code calls `d` by: its exports (`synth.pluck` gives `synth` and `pluck`).
fn export_words(catalog: &Catalog, d: &Descriptor) -> Vec<String> {
    let mut words: Vec<String> = vec![];
    for e in catalog.exports.get(&d.id).into_iter().flatten() {
        for w in e.name.split('.') {
            if !words.iter().any(|x| x == w) {
                words.push(w.to_string());
            }
        }
    }
    words
}

/// The instruments and effects the catalog offers: each with a category (PKG-7).
fn components(catalog: &Catalog) -> Vec<Item> {
    let mut out = vec![];
    for d in catalog.components.values() {
        let Some(category) = d.editor.category.clone() else { continue };
        let kind = match (d.is_instrument(), d.is_effect()) {
            (true, _) => Kind::Instrument,
            (false, true) => Kind::Effect,
            _ => continue,
        };
        let package = package_of(&d.id);
        let mut tags = export_words(catalog, d);
        tags.push(category.clone());
        tags.extend(d.editor.notes.values().cloned());
        out.push(Item {
            kind,
            id: d.id.clone(),
            label: d.label.clone(),
            category,
            description: d.editor.description.clone(),
            tags,
            preview: package != "song",
            package,
            component: d.id.clone(),
            instrument: kind == Kind::Instrument,
            preset: None,
            sample: None,
        });
    }
    out
}

/// Each preset, std's and yours, as a sound: for a component the library has, or a plugin.
fn sounds(src: &Sources, components: &[Item]) -> Vec<Item> {
    let mut out = vec![];
    let mut add = |from: &str, of: &Item, names: Vec<presets::Listed>| {
        for l in names {
            let mut tags = vec![of.label.clone()];
            tags.extend(of.tags.iter().cloned());
            out.push(Item {
                kind: Kind::Sound,
                id: format!("sound:{from}/{}/{}", of.component, l.name),
                label: l.name.clone(),
                category: of.category.clone(),
                description: of.description.clone(),
                tags,
                package: from.to_string(),
                component: of.component.clone(),
                instrument: of.instrument,
                preview: of.preview,
                preset: Some(PresetOf { from: from.to_string(), name: l.name }),
                sample: None,
            });
        }
    };
    if let Some(std) = src.std {
        for c in components {
            if let Some(dir) = presets::factory(std, &c.component) {
                add("std", c, presets::list(&dir, &c.component));
            }
        }
    }
    if let Some(yours) = src.yours {
        let plugins: Vec<Item> = plugins(src.plugins);
        for sound in your_sounds(yours) {
            // A preset for a sound this computer hasn't got (a package not installed) is left out.
            let of = components.iter().chain(&plugins).find(|c| c.component == sound).cloned().or_else(|| {
                let id = sound.strip_prefix("clap:")?;
                Some(Item {
                    kind: Kind::Plugin,
                    id: sound.clone(),
                    label: id.to_string(),
                    category: "Plugins".into(),
                    description: String::new(),
                    tags: vec![],
                    package: "plugin".into(),
                    component: sound.clone(),
                    instrument: false,
                    preview: false,
                    preset: None,
                    sample: None,
                })
            });
            if let Some(of) = of {
                add("yours", &of, presets::list(&yours.join(presets::folder(&sound)), &sound));
            }
        }
    }
    out
}

/// The sounds your Presets folder has presets for, by the `sound` each file names: two folders
/// down (`std/synth-pluck/Warm.json`), as [`presets::folder`] files them.
fn your_sounds(yours: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let dirs = |d: &Path| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        v.sort();
        v
    };
    for package in dirs(yours) {
        for sound in dirs(&package) {
            for e in std::fs::read_dir(&sound).into_iter().flatten().flatten() {
                let file = e.path();
                if file.extension().is_none_or(|x| x != "json") {
                    continue;
                }
                if let Some(p) = std::fs::read_to_string(&file).ok().and_then(|t| presets::Preset::parse(&t).ok())
                    && yours.join(presets::folder(&p.sound)) == sound
                {
                    found.insert(p.sound);
                }
            }
        }
    }
    found
}

/// The installed plugins: an instrument or an effect each, by its CLAP features.
fn plugins(list: &[Plugin]) -> Vec<Item> {
    list.iter()
        .map(|p| {
            let mut tags = vec![p.vendor.clone()];
            if p.instrument {
                tags.push("instrument".into());
            }
            if p.effect {
                tags.push("effect".into());
            }
            Item {
                kind: Kind::Plugin,
                id: p.id.clone(),
                label: p.name.clone(),
                category: "Plugins".into(),
                description: format!("{} plugin by {}", if p.instrument { "An instrument" } else { "An effect" }, p.vendor),
                tags,
                package: "plugin".into(),
                component: p.id.clone(),
                instrument: p.instrument,
                preview: false,
                preset: None,
                sample: None,
            }
        })
        .collect()
}

/// What was typed: the words, and the filters among them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// Lowercase, in the order typed.
    pub words: Vec<String>,
    /// The kinds named (`effects`): any of them. None named: every kind.
    pub kinds: Vec<Kind>,
    pub sample: Option<SampleType>,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
}

impl Query {
    /// Read what was typed (LIB-1): a kind (`effects`), `loop` or `one-shot`, a tempo (`90 bpm`,
    /// `90bpm`) and a key (`Am`, `F#`, `C major`) are filters; every other word is a word.
    pub fn parse(text: &str) -> Query {
        let mut q = Query::default();
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut i = 0;
        while i < tokens.len() {
            let (t, next) = (tokens[i], tokens.get(i + 1).map(|n| n.to_lowercase()));
            let lower = t.to_lowercase();
            let number = |s: &str| s.parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0);
            i += 1;
            if let Some(k) = Kind::named(t) {
                if !q.kinds.contains(&k) {
                    q.kinds.push(k);
                }
            } else if matches!(lower.as_str(), "loop" | "loops") {
                q.sample = Some(SampleType::Loop);
            } else if matches!(lower.as_str(), "one-shot" | "one-shots" | "oneshot" | "oneshots") {
                q.sample = Some(SampleType::OneShot);
            } else if let Some(n) = lower.strip_suffix("bpm").and_then(number) {
                q.bpm = Some(n);
            } else if let (Some(n), Some("bpm")) = (number(&lower), next.as_deref()) {
                q.bpm = Some(n);
                i += 1;
            } else if let (Some(mode @ ("major" | "minor")), Some(k)) = (next.as_deref(), Key::parse(&format!("{t}maj"))) {
                q.key = Some(Key { root: k.root, minor: mode == "minor" });
                i += 1;
            } else if let Some(k) = Key::parse(t) {
                q.key = Some(k);
            } else {
                q.words.push(lower);
            }
        }
        q
    }

    /// Whether it has neither words nor filters.
    pub fn is_empty(&self) -> bool {
        *self == Query::default()
    }

    /// Whether `item` meets every filter.
    fn admits(&self, item: &Item) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&item.kind) {
            return false;
        }
        if self.sample.is_none() && self.bpm.is_none() && self.key.is_none() {
            return true;
        }
        let Some(s) = &item.sample else { return false };
        self.sample.is_none_or(|t| t == s.kind)
            && self.bpm.is_none_or(|b| s.bpm.is_some_and(|x| (x - b).abs() < 0.5))
            && self.key.is_none_or(|k| s.key == Some(k))
    }
}

/// The words of `text`, lowercase: its runs of letters and digits.
fn words_of(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase)
}

/// Where `item` matches `q`'s words, as a rank, lower first: 0 the whole query is its label, 1 its
/// label starts with it; else its weakest word's: 2 a word of the label, 3 inside the label, 4 a
/// tag, 5 a word of the description. None when a word matches nothing.
fn place(item: &Item, words: &[String]) -> Option<u8> {
    if words.is_empty() {
        return Some(0);
    }
    let label = item.label.to_lowercase();
    let typed = words.join(" ");
    if label == typed {
        return Some(0);
    }
    if label.starts_with(&typed) {
        return Some(1);
    }
    let label_words: Vec<String> = words_of(&item.label).collect();
    let tag_words: Vec<String> = item.tags.iter().flat_map(|t| words_of(t)).collect();
    let described: Vec<String> = words_of(&item.description).collect();
    let starts = |list: &[String], w: &str| list.iter().any(|x| x.starts_with(w));
    let mut worst = 2;
    for w in words {
        let at = if starts(&label_words, w) {
            2
        } else if label.contains(w.as_str()) {
            3
        } else if starts(&tag_words, w) {
            4
        } else if starts(&described, w) {
            5
        } else {
            return None;
        };
        worst = worst.max(at);
    }
    Some(worst)
}

/// Where an item's package puts it: std's and music2's own (`@m2/`) first, then yours and the
/// song's, then plugins and everyone else's.
fn origin(package: &str) -> u8 {
    match package {
        "std" => 0,
        p if p.starts_with("@m2/") => 0,
        "yours" | "song" => 1,
        _ => 2,
    }
}

/// The items `q` finds, best first (LIB-1): each meets every filter and has every word somewhere;
/// by [`place`], then [`origin`], kind, label and id.
pub fn search<'a>(items: &'a [Item], q: &Query) -> Vec<&'a Item> {
    let mut found: Vec<(u8, &Item)> = items.iter().filter(|i| q.admits(i)).filter_map(|i| Some((place(i, &q.words)?, i))).collect();
    found.sort_by(|(a, x), (b, y)| {
        (a, origin(&x.package), x.kind, x.label.to_lowercase(), &x.label, &x.id).cmp(&(b, origin(&y.package), y.kind, y.label.to_lowercase(), &y.label, &y.id))
    });
    found.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;

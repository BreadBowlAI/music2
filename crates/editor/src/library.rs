//! The library (LIB-1): everything that can be added to a song, as one list of items, and the one
//! search over it that ⌘F, the side browser and `m2 search` all call.
//!
//! [`items`] gathers the list from its [`Sources`]: the components a build offers with a category
//! (PKG-7), each preset as a sound (FMT-22: each package's own, and yours), the samples of std, the
//! packs and your folders ([`samples`], LIB-4), and the installed plugins once the app has listed
//! them. It is the one place a new kind of item plugs in.
//!
//! [`search`] keeps the items whose every word matches (a word of the label, inside the label, a
//! tag, a word of the description) and meets every filter typed (a kind, `loop`, `one-shot`, a
//! tempo within 2 bpm, a key or its relative) and Fits my song when asked ([`Fits`]), ranked: the label itself, then a label that starts with what was typed, then by
//! each item's weakest word; then std's and ours before yours, before plugins; then by kind, label
//! and id. What is typed is split into words as an item's text is ([`words_of`]), and a kind or a
//! sample's type typed also finds a label that has that word. The same list and words give the
//! same order every time.
//!
//! Previews (LIB-3) are in [`preview`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_script::Catalog;
use m2_script::component::Descriptor;
use serde::Serialize;

use crate::presets;

pub mod kit;
pub mod preview;
pub mod samples;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum SampleType {
    #[serde(rename = "loop")]
    Loop,
    #[default]
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

    /// Its relative: the minor key of a major key's sixth (`Am` of `C`), the major key of a minor
    /// key's third (`C` of `Am`). The same notes.
    pub fn relative(self) -> Key {
        match self.minor {
            true => Key { root: (self.root + 3) % 12, minor: false },
            false => Key { root: (self.root + 9) % 12, minor: true },
        }
    }
}

impl Serialize for Key {
    /// `Am`, `F#`: as a sample's tags write it.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{}{}", ROOTS[self.root as usize], if self.minor { "m" } else { "" }))
    }
}

/// What a sample is, beyond its words (LIB-4): what the filters read, what was measured from its
/// file, and where it came from.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Sample {
    #[serde(rename = "type")]
    pub kind: SampleType,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
    pub seconds: Option<f64>,
    /// Its instrument: a word of [`samples::INSTRUMENTS`].
    pub instrument: Option<String>,
    pub channels: u16,
    pub rate: u32,
    /// Its loudest sample: 1 is full scale.
    pub peak: f64,
    /// The SHA-256 of its bytes, in hex.
    pub hash: String,
    /// Where it came from, as its note says it when it is added (LIB-7): `{pack, version, file,
    /// author, license}`, or `{folder: true}`.
    pub from: serde_json::Value,
    /// Its file on this computer, which only the server reads: never sent (LIB-6).
    #[serde(skip)]
    pub file: PathBuf,
    /// Its file's size and modified time (nanoseconds since 1970) when it was indexed.
    #[serde(skip)]
    pub stamp: (u64, u64),
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
    /// It goes on a track's sound: an effect, an effect plugin, a sound of an effect.
    pub effect: bool,
    /// It has a preview (LIB-3): not a plugin, a plugin's preset, the song's own code or an
    /// instrument that plays a file you give it ([`preview::none_for`]).
    pub preview: bool,
    /// Why it has none, in words, when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_preview: Option<String>,
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
    /// The components a build offers: std's, the packs the lists offer, and the open song's own.
    pub catalog: &'a Catalog,
    /// The folders of the packages the catalog's components are from, by name (std's among them):
    /// their own presets (`presets/<component>/`).
    pub packages: &'a [(String, PathBuf)],
    /// Your Presets folder (`m2_os::dirs::presets`).
    pub yours: Option<&'a Path>,
    /// The installed plugins, once the app has listed them; none otherwise.
    pub plugins: &'a [Plugin],
    /// The samples (LIB-4): std's, the packs' and your folders', as [`samples::index`] made them,
    /// shared with the index that keeps them.
    pub samples: &'a [Arc<samples::Indexed>],
}

/// Every item the sources give but the samples, each once (the first source's), in the order
/// [`search`] gives with no words.
pub fn items(src: &Sources) -> Vec<Item> {
    let mut out = components(src.catalog);
    out.extend(sounds(src, &out));
    out.extend(plugins(src.plugins));
    let mut seen = BTreeSet::new();
    out.retain(|i| seen.insert(i.id.clone()));
    let all = Query::default();
    search(&out, &all).into_iter().cloned().collect()
}

/// The library (LIB-1): the items its sources give ([`items`]), and the samples where their
/// indexes keep them. A search reads the samples there, by reference: none is copied, however many
/// your folders hold (only what it answers is).
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub items: Vec<Item>,
    pub samples: Vec<Arc<samples::Indexed>>,
}

impl Library {
    /// The library of `src`.
    pub fn of(src: &Sources) -> Library {
        Library { items: items(src), samples: src.samples.to_vec() }
    }

    /// Every item, the samples last; an id twice (a sample in two of your folders) is there twice.
    pub fn all(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().chain(self.samples.iter().flat_map(|i| i.items.iter()))
    }

    /// The items `q` finds, best first ([`search`]), each id once: the first source's.
    pub fn search(&self, q: &Query) -> Vec<&Item> {
        search(self.all(), q)
    }

    /// The item with id `id`, if the library has it.
    pub fn get(&self, id: &str) -> Option<&Item> {
        self.all().find(|i| i.id == id)
    }
}

/// The library with no song open (LIB-2): the components of `packages` (std, found as the app finds
/// it, and the packs the lists offer), their presets and `yours`, `samples`, and `plugins`.
pub fn without_song(
    packages: &[(String, PathBuf)],
    yours: Option<&Path>,
    plugins: &[Plugin],
    samples: &[Arc<samples::Indexed>],
) -> Library {
    let catalog = m2_script::describe(packages);
    Library::of(&Sources { catalog: &catalog, packages, yours, plugins, samples })
}

/// The packs of the store in `store` the lists offer with no song open (PACK-11): of each, its
/// newest installed version this app can run with std in `std` (PACK-2), by name and folder.
pub fn store_packs(store: &Path, std: &Path) -> Vec<(String, PathBuf)> {
    use m2_song::pack::{PkgManifest, Store, meets};
    let store = Store::at(store.to_path_buf());
    let sdk = m2_song::Version::parse(m2_script::SDK_VERSION);
    let std = PkgManifest::at(std).ok().map(|m| m.package.version);
    let runs = |dir: &Path| PkgManifest::at(dir).is_ok_and(|m| meets(&m.dependencies, sdk, std).is_ok());
    store
        .list()
        .into_iter()
        .filter_map(|(name, versions)| {
            let dir = versions.iter().rev().map(|v| store.version_dir(&name, v)).find(|d| runs(d))?;
            Some((name.as_str().to_string(), dir))
        })
        .collect()
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
            preview: preview::none_for(d).is_none(),
            no_preview: preview::none_for(d),
            package,
            component: d.id.clone(),
            instrument: kind == Kind::Instrument,
            effect: kind == Kind::Effect,
            preset: None,
            sample: None,
        });
    }
    out
}

/// Each preset, each package's own and yours, as a sound: for a component the library has, or a
/// plugin. A package's own are `from` `std` (as the presets' routes name them, FMT-22) and of its
/// package; yours are `yours`.
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
                package: if from == "yours" { from.to_string() } else { package_of(&of.component) },
                component: of.component.clone(),
                instrument: of.instrument,
                effect: of.effect,
                preview: of.preview,
                no_preview: if of.kind == Kind::Plugin {
                    Some("A plugin's sounds have no preview yet.".into())
                } else {
                    of.no_preview.clone()
                },
                preset: Some(PresetOf { from: from.to_string(), name: l.name }),
                sample: None,
            });
        }
    };
    for c in components {
        let package = package_of(&c.component);
        let Some((_, dir)) = src.packages.iter().find(|(n, _)| *n == package) else { continue };
        if let Some(dir) = presets::factory(dir, &c.component) {
            add("std", c, presets::list(&dir, &c.component));
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
                    effect: false,
                    preview: false,
                    no_preview: Some("Plugins have no preview yet.".into()),
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
                effect: p.effect,
                preview: false,
                no_preview: Some("Plugins have no preview yet.".into()),
                preset: None,
                sample: None,
            }
        })
        .collect()
}

impl Item {
    /// Whether it is of `kind`: a plugin is an instrument or an effect too, by what it plays.
    fn is(&self, kind: Kind) -> bool {
        self.kind == kind
            || self.kind == Kind::Plugin
                && match kind {
                    Kind::Instrument => self.instrument,
                    Kind::Effect => self.effect,
                    _ => false,
                }
    }
}

/// Fits my song (LIB-1): the song's tempo where the playhead is, and its key if it has one. It keeps
/// the loops within [`FITS_TEMPO`] of that tempo, in that key or its relative (any key for a loop
/// with none).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fits {
    pub bpm: f64,
    pub key: Option<Key>,
}

/// How far a loop's tempo may be from the song's and fit it: 8%.
pub const FITS_TEMPO: f64 = 0.08;
/// How far a sample's tempo may be from a tempo typed and meet it, in bpm.
pub const NEAR_BPM: f64 = 2.0;

/// What was typed: the words, and the filters among them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    /// The words to find: lowercase, split as an item's text is ([`words_of`]), in the order typed.
    pub words: Vec<String>,
    /// The kinds named (`effects`): any of them. None named: every kind.
    pub kinds: Vec<Kind>,
    /// The words that named them, as typed: a label with one of them meets the filter too.
    pub kind_words: Vec<String>,
    pub sample: Option<SampleType>,
    /// The word that named the sample's type (`loop`): a label with it meets the filter too.
    pub sample_word: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<Key>,
    /// Fits my song: never typed, given by the caller ([`Fits`]).
    pub fits: Option<Fits>,
    /// Every word typed, filters too, split as `words` is: the whole query a label may be.
    pub all: Vec<String>,
    /// What was typed, lowercase, its spaces made one: an item's whole id finds it.
    pub text: String,
}

impl Query {
    /// Read what was typed (LIB-1): a kind (`effects`), `loop` or `one-shot`, a tempo (`90 bpm`,
    /// `90bpm`) and a key (`Am`, `F#`, `C major`) are filters, each a word between spaces; the rest
    /// is split into words as an item's text is, so `hi-hat` is `hi` and `hat`.
    pub fn parse(text: &str) -> Query {
        let tokens: Vec<&str> = text.split_whitespace().collect();
        let mut q = Query { all: words_of(text).collect(), text: tokens.join(" ").to_lowercase(), ..Query::default() };
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
                q.kind_words.push(lower);
            } else if matches!(lower.as_str(), "loop" | "loops") {
                q.sample = Some(SampleType::Loop);
                q.sample_word = Some(lower);
            } else if matches!(lower.as_str(), "one-shot" | "one-shots" | "oneshot" | "oneshots") {
                q.sample = Some(SampleType::OneShot);
                q.sample_word = Some(lower);
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
                q.words.extend(words_of(t));
            }
        }
        q
    }

    /// Whether it has neither words nor filters.
    pub fn is_empty(&self) -> bool {
        *self == Query::default()
    }

    /// Whether `item`, whose label's words are `label`, meets every filter. A kind or a sample's
    /// type is met by the item's own, or by a label that has the word that named it.
    fn admits(&self, item: &Item, label: &[String]) -> bool {
        let named = |typed: &str| words_of(typed).all(|w| label.iter().any(|x| x.starts_with(&w)));
        if !self.kinds.is_empty() && !self.kinds.iter().any(|&k| item.is(k)) && !self.kind_words.iter().any(|w| named(w)) {
            return false;
        }
        if let Some(t) = self.sample
            && item.sample.as_ref().is_none_or(|s| s.kind != t)
            && !self.sample_word.as_deref().is_some_and(named)
        {
            return false;
        }
        if self.bpm.is_none() && self.key.is_none() && self.fits.is_none() {
            return true;
        }
        let Some(s) = &item.sample else { return false };
        let in_key = |k: Key| s.key == Some(k) || s.key == Some(k.relative());
        let fits = |f: &Fits| {
            s.kind == SampleType::Loop
                && s.bpm.is_some_and(|b| (b - f.bpm).abs() <= FITS_TEMPO * f.bpm)
                && (f.key.is_none() || s.key.is_none() || f.key.is_some_and(in_key))
        };
        self.bpm.is_none_or(|b| s.bpm.is_some_and(|x| (x - b).abs() <= NEAR_BPM))
            && self.key.is_none_or(in_key)
            && self.fits.as_ref().is_none_or(fits)
    }
}

/// The words of `text`, lowercase: its runs of letters and digits. What is typed and an item's
/// text are split alike.
fn words_of(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase)
}

/// Where `item`, whose label's words are `label`, matches `q`, as a rank, lower first: 0 its whole
/// id was typed, or the whole query is its label, 1 its label starts with it; else its weakest
/// word's: 2 a word of the label, 3 inside the label, 4 a tag, 5 a word of the description (2 with
/// no words, only filters). None when a word matches nothing. The query and the label are compared
/// word for word, so their punctuation doesn't count.
fn place(item: &Item, q: &Query, label: &[String]) -> Option<u8> {
    if !q.text.is_empty() && item.id.to_lowercase() == q.text {
        return Some(0);
    }
    let label = label.join(" ");
    let (typed, all) = (q.words.join(" "), q.all.join(" "));
    if !typed.is_empty() && label == typed || !all.is_empty() && label == all {
        return Some(0);
    }
    if !typed.is_empty() && label.starts_with(&typed) || !all.is_empty() && label.starts_with(&all) {
        return Some(1);
    }
    let label_words: Vec<&str> = label.split(' ').collect();
    let tag_words: Vec<String> = item.tags.iter().flat_map(|t| words_of(t)).collect();
    let described: Vec<String> = words_of(&item.description).collect();
    let mut worst = 2;
    for w in &q.words {
        let at = if label_words.iter().any(|x| x.starts_with(w.as_str())) {
            2
        } else if label.contains(w.as_str()) {
            3
        } else if tag_words.iter().any(|x| x.starts_with(w.as_str())) {
            4
        } else if described.iter().any(|x| x.starts_with(w.as_str())) {
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
/// by [`place`], then [`origin`], kind, label and id. An id found twice is the first one's.
pub fn search<'a>(items: impl IntoIterator<Item = &'a Item>, q: &Query) -> Vec<&'a Item> {
    let mut seen = std::collections::HashSet::new();
    let mut found: Vec<(u8, &Item)> = items
        .into_iter()
        .filter_map(|i| {
            let label: Vec<String> = words_of(&i.label).collect();
            q.admits(i, &label).then_some(())?;
            let at = place(i, q, &label)?;
            seen.insert(i.id.as_str()).then_some((at, i))
        })
        .collect();
    found.sort_by_cached_key(|(at, x)| (*at, origin(&x.package), x.kind, x.label.to_lowercase(), x.label.as_str(), x.id.as_str()));
    found.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;

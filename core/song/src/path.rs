//! Where everything lives in a project folder (FMT-5): the one place a project path is
//! spelled or classified. Every other crate goes through [`DataPath`].
//!
//! ```text
//! music2.toml                                  Manifest
//! *.ts (outside data/, assets/, .music2/, node_modules/ and dot folders)   Code
//! data/arrangement.json                        Arrangement
//! data/<module>/                               Module (the folder itself)
//! data/<module>/<lane>.json                    Lane
//! data/<module>/<frame>/…/<plugin>.state       PluginState, and its .state.json sidecar
//! assets/<name>                                Asset
//! layout.json                                  Presentation
//! .music2/**                                   Local
//! anything else                                Unknown
//! ```

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::key::{Key, LaneAddr};

pub const MANIFEST: &str = "music2.toml";
pub const DATA: &str = "data";
pub const ASSETS: &str = "assets";
pub const LOCAL: &str = ".music2";
pub const LAYOUT: &str = "layout.json";
const ARRANGEMENT: &str = "arrangement.json";

/// A project path, by what it is.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataPath {
    Manifest,
    /// A TypeScript file, relative to the project.
    Code(PathBuf),
    Arrangement,
    /// The folder `data/<module>/` itself.
    Module {
        module: Key,
    },
    Lane {
        module: Key,
        lane: Key,
    },
    /// A plugin's state blob, or with `sidecar` its `.state.json`. `path` is the `key()`
    /// frames, then the plugin key (ID-8).
    PluginState {
        module: Key,
        path: Vec<Key>,
        sidecar: bool,
    },
    Asset(AssetName),
    Presentation,
    /// Under `.music2/`: this machine's, never the song's.
    Local(PathBuf),
    /// Anything else, as given.
    Unknown(PathBuf),
}

/// What a path's changes mean (PLANE-1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Manifest,
    Code,
    Arrangement,
    Data,
    Presentation,
    Ignored,
}

impl DataPath {
    pub fn lane(addr: &LaneAddr) -> DataPath {
        DataPath::Lane { module: addr.module.clone(), lane: addr.lane.clone() }
    }

    /// The state file of the plugin keyed `path` (its `key()` frames, then its own key) in module
    /// `module`, as a plugin node names them (ID-8), spelled as the song spells it: a key that is a
    /// Windows device name (`aux`) ends with a hyphen there. None when one of them isn't a key.
    pub fn plugin_state(module: &str, path: &[impl AsRef<str>]) -> Option<DataPath> {
        let module = Key::new(module).ok()?;
        let path = path.iter().map(|k| Key::new(k.as_ref())).collect::<Result<Vec<_>, _>>().ok()?;
        Some(DataPath::PluginState { module, path, sidecar: false })
    }

    /// Classify a path relative to the project folder. A path that isn't relative, has an
    /// empty, `.` or `..` segment, or isn't UTF-8 is `Unknown`.
    pub fn parse(p: &Path) -> DataPath {
        let unknown = || DataPath::Unknown(p.to_path_buf());
        let mut segs = vec![];
        for c in p.components() {
            match c {
                Component::Normal(s) => match s.to_str() {
                    Some(s) => segs.push(s),
                    None => return unknown(),
                },
                _ => return unknown(),
            }
        }
        match segs.as_slice() {
            [] => unknown(),
            [LOCAL, ..] => DataPath::Local(p.to_path_buf()),
            [MANIFEST] => DataPath::Manifest,
            [LAYOUT] => DataPath::Presentation,
            [DATA, rest @ ..] => data(rest).unwrap_or_else(unknown),
            [ASSETS, rest @ ..] if !rest.is_empty() => AssetName::new(&rest.join("/")).map(DataPath::Asset).unwrap_or_else(|_| unknown()),
            [.., file] if file.ends_with(".ts") && !segs.iter().any(|s| s.starts_with('.') || *s == "node_modules") => {
                DataPath::Code(PathBuf::from(segs.join("/")))
            }
            _ => unknown(),
        }
    }

    /// The path relative to the project folder, `/`-separated. `parse(p.path()) == p`.
    pub fn path(&self) -> PathBuf {
        let data = |rest: &[String]| PathBuf::from(format!("{DATA}/{}", rest.join("/")));
        match self {
            DataPath::Manifest => PathBuf::from(MANIFEST),
            DataPath::Code(p) | DataPath::Local(p) | DataPath::Unknown(p) => p.clone(),
            DataPath::Arrangement => data(&[ARRANGEMENT.to_string()]),
            DataPath::Module { module } => data(&[module.segment()]),
            DataPath::Lane { module, lane } => data(&[module.segment(), format!("{}.json", lane.segment())]),
            DataPath::PluginState { module, path, sidecar } => {
                let mut segs: Vec<String> = std::iter::once(module).chain(path).map(Key::segment).collect();
                if let Some(last) = segs.last_mut() {
                    last.push_str(if *sidecar { ".state.json" } else { ".state" });
                }
                data(&segs)
            }
            DataPath::Asset(a) => PathBuf::from(format!("{ASSETS}/{a}")),
            DataPath::Presentation => PathBuf::from(LAYOUT),
        }
    }

    pub fn class(&self) -> Class {
        match self {
            DataPath::Manifest => Class::Manifest,
            DataPath::Code(_) => Class::Code,
            DataPath::Arrangement => Class::Arrangement,
            DataPath::Module { .. } | DataPath::Lane { .. } | DataPath::PluginState { .. } | DataPath::Asset(_) => Class::Data,
            DataPath::Presentation => Class::Presentation,
            DataPath::Local(_) | DataPath::Unknown(_) => Class::Ignored,
        }
    }

    /// An `Unknown` path under `data/`, which raises the Notice `file.unknown` (FMT-7).
    pub fn unknown_data(&self) -> bool {
        let under_data = |p: &Path| p.components().next() == Some(Component::Normal(DATA.as_ref())) && p.components().count() > 1;
        matches!(self, DataPath::Unknown(p) if under_data(p))
    }

    /// The module whose data this is.
    pub fn module(&self) -> Option<&Key> {
        match self {
            DataPath::Module { module } | DataPath::Lane { module, .. } | DataPath::PluginState { module, .. } => Some(module),
            _ => None,
        }
    }
}

/// `DataPath::path`, `/`-separated, as diagnostics name files.
impl fmt::Display for DataPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", slashed(&self.path()))
    }
}

/// A relative path written with `/` on every platform.
pub fn slashed(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// A `data/<module>/` folder's hash as a Tx's base names it (WRITE-12): the SHA-256 of the
/// `\n`-joined lines `<path>\0<sha256 hex>`, one for each lane and state file under it, by its
/// path in the folder, `/`-separated, sorted bytewise. The TypeScript tools compute the same.
pub fn dir_hash(files: &[(String, crate::Sha)]) -> crate::Sha {
    let mut lines: Vec<String> = files.iter().map(|(p, sha)| format!("{p}\0{}", sha.hex())).collect();
    lines.sort();
    crate::Sha::of(lines.join("\n").as_bytes())
}

/// `segs` after `data/`.
fn data(segs: &[&str]) -> Option<DataPath> {
    match segs {
        [ARRANGEMENT] => Some(DataPath::Arrangement),
        [m] => Some(DataPath::Module { module: Key::from_segment(m)? }),
        [m, file] if !file.ends_with(".state.json") && file.ends_with(".json") => {
            let lane = Key::from_segment(file.strip_suffix(".json")?)?;
            Some(DataPath::Lane { module: Key::from_segment(m)?, lane })
        }
        [m, frames @ .., file] => {
            let (stem, sidecar) = match file.strip_suffix(".state.json") {
                Some(stem) => (stem, true),
                None => (file.strip_suffix(".state")?, false),
            };
            let path = frames.iter().copied().chain([stem]).map(Key::from_segment).collect::<Option<Vec<Key>>>()?;
            Some(DataPath::PluginState { module: Key::from_segment(m)?, path, sidecar })
        }
        _ => None,
    }
}

/// Whether a file at project path `rel`, classed as `d`, is one a snapshot lists as Ignored
/// (`Snapshot::unknown`): a file under `data/` that isn't a data path (FMT-7), but for dot files
/// (`.DS_Store`, the system's, not a typo), or an asset's name in another Unicode form (FMT-15).
pub fn listed_unknown(d: &DataPath, rel: &Path) -> bool {
    let dot = rel.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'));
    (d.unknown_data() && !dot) || (matches!(d, DataPath::Unknown(_)) && denormal_asset(rel).is_some())
}

/// The asset name a file in `assets/` would have, if only its Unicode form keeps it from being
/// one (`cafe\u{301}.wav`, as some disks and copies write `café.wav`). Such a file is no asset
/// (FMT-14), but a reference to its name can say where it is (FMT-15).
pub fn denormal_asset(p: &Path) -> Option<AssetName> {
    let name = slashed(p.strip_prefix(ASSETS).ok()?);
    let nfc = icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(&name).into_owned();
    (nfc != name && AssetName::new(&name).is_err()).then(|| AssetName::new(&nfc).ok()).flatten()
}

/// An asset's name: a path under `assets/`, `/`-separated (FMT-14). It is NFC and has no
/// empty, `.` or `..` segment, no segment that starts with `.`, and no `\` or control character.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetName(String);

impl AssetName {
    pub fn new(s: &str) -> Result<AssetName, String> {
        let why = if s.is_empty() {
            Some("it is empty")
        } else if s.split('/').any(|seg| seg.is_empty() || seg.starts_with('.')) {
            Some("a folder or file name in it is empty or starts with `.`")
        } else if s.chars().any(|c| c == '\\' || c.is_control()) {
            Some("it holds a `\\` or a control character")
        } else if !icu_normalizer::ComposingNormalizerBorrowed::new_nfc().is_normalized(s) {
            Some("it isn't in Unicode normal form C")
        } else {
            None
        };
        match why {
            None => Ok(AssetName(s.to_string())),
            Some(why) => Err(format!("`{s}` can't name an asset in assets/: {why}")),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// This name, or the first with a numeric suffix that `taken` says is free: `kick.wav`,
    /// `kick 2.wav`, `kick 3.wav`… An import never replaces an asset (FMT-14).
    pub fn untaken(&self, taken: impl Fn(&str) -> bool) -> AssetName {
        let (folder, file) = self.0.rsplit_once('/').unwrap_or(("", &self.0));
        let (stem, ext) = match file.rfind('.') {
            Some(i) if i > 0 => (&file[..i], &file[i..]),
            _ => (file, ""),
        };
        let at = |n: u32| {
            let file = if n == 1 { format!("{stem}{ext}") } else { format!("{stem} {n}{ext}") };
            if folder.is_empty() { file } else { format!("{folder}/{file}") }
        };
        let free = (1..).map(at).find(|n| !taken(n)).expect("some number is free");
        AssetName(free)
    }

    /// A form that ignores case and Unicode normalisation, to find a reference that names a
    /// file only nearly (FMT-15).
    pub(crate) fn folded(s: &str) -> String {
        icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(s).to_lowercase()
    }

    /// The name an imported file gets in `assets/` from its own file name (FMT-14, lane A's Q2):
    /// in NFC, control characters dropped, `/`, `\` and `:` made `-`, leading dots and spaces
    /// dropped, the extension lowercased, at most [`IMPORT_BYTES`] bytes (the stem cut, never the
    /// extension). One with nothing left is `audio.wav`.
    pub fn for_import(file_name: &str) -> AssetName {
        let nfc = icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(file_name);
        let clean: String = nfc.chars().filter(|c| !c.is_control()).map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
        let clean = clean.trim().trim_start_matches(['.', ' ']).trim_end();
        let (stem, ext) = match clean.rfind('.') {
            Some(i) if i > 0 => (&clean[..i], clean[i..].to_lowercase()),
            _ => (clean, String::new()),
        };
        let room = IMPORT_BYTES.saturating_sub(ext.len());
        let cut = (0..=stem.len().min(room)).rev().find(|&i| stem.is_char_boundary(i)).unwrap_or(0);
        let stem = stem[..cut].trim_end();
        match AssetName::new(&format!("{stem}{ext}")) {
            Ok(name) if !stem.is_empty() => name,
            _ => AssetName("audio.wav".into()),
        }
    }

    /// [`AssetName::untaken`] where a name is taken by any of `names` in any capitals or Unicode
    /// form: a song copied to a disk that ignores them never holds two that differ only so (D2).
    pub fn untaken_folded<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> AssetName {
        let taken: std::collections::BTreeSet<String> = names.into_iter().map(AssetName::folded).collect();
        self.untaken(|n| taken.contains(&AssetName::folded(n)))
    }
}

/// The longest name an import gives a file, in bytes.
pub const IMPORT_BYTES: usize = 120;

impl fmt::Display for AssetName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for AssetName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AssetName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<AssetName, D::Error> {
        AssetName::new(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> Key {
        Key::new(s).unwrap()
    }

    fn parse(p: &str) -> DataPath {
        DataPath::parse(Path::new(p))
    }

    // FMT-5: every project path is spelled and classified here, and the table's rows hold.
    #[test]
    fn the_path_table() {
        let rows = [
            ("music2.toml", DataPath::Manifest, Class::Manifest),
            ("song.ts", DataPath::Code("song.ts".into()), Class::Code),
            ("tracks/bass.ts", DataPath::Code("tracks/bass.ts".into()), Class::Code),
            ("lib/a/b.ts", DataPath::Code("lib/a/b.ts".into()), Class::Code),
            ("data/arrangement.json", DataPath::Arrangement, Class::Arrangement),
            ("data/kick", DataPath::Module { module: key("kick") }, Class::Data),
            ("data/kick/hits.json", DataPath::Lane { module: key("kick"), lane: key("hits") }, Class::Data),
            ("data/song/hats-pan.json", DataPath::Lane { module: key("song"), lane: key("hats-pan") }, Class::Data),
            ("data/lead/lead.state", DataPath::PluginState { module: key("lead"), path: vec![key("lead")], sidecar: false }, Class::Data),
            (
                "data/vox/chain-l/tune.state.json",
                DataPath::PluginState { module: key("vox"), path: vec![key("chain-l"), key("tune")], sidecar: true },
                Class::Data,
            ),
            ("assets/vox/lead.wav", DataPath::Asset(AssetName::new("vox/lead.wav").unwrap()), Class::Data),
            ("assets/kick 2.wav.json", DataPath::Asset(AssetName::new("kick 2.wav.json").unwrap()), Class::Data),
            ("layout.json", DataPath::Presentation, Class::Presentation),
            (".music2/lock", DataPath::Local(".music2/lock".into()), Class::Ignored),
            (".music2/tx/k3/data/kick/hits.json", DataPath::Local(".music2/tx/k3/data/kick/hits.json".into()), Class::Ignored),
        ];
        for (p, want, class) in rows {
            let got = parse(p);
            assert_eq!(got, want, "{p}");
            assert_eq!(got.class(), class, "{p}");
            assert_eq!(got.path(), PathBuf::from(p), "{p} spells back");
        }
    }

    // FMT-5, FMT-7: everything else is Unknown, and only an unknown file under data/ is noticed.
    #[test]
    fn unknown_paths() {
        for p in [
            "README.md",
            // FMT-23: the song's files for agents.
            "AGENTS.md",
            "CLAUDE.md",
            "song.js",
            "lib/x.mjs",
            "tsconfig.json",
            "example.toml",
            "Music2.toml",
            "node_modules/x/index.ts",
            ".git/hooks/x.ts",
            "lib/.hidden/x.ts",
            "data/kick/hits.ts",
            "data/Kick/hits.json",
            "data/kick/Hits.json",
            "data/kick/hits.JSON",
            "data/kick/a/b.json",
            "data/kick.hits.json",
            "data/kick/.DS_Store",
            "data/song.json",
            "data/kick/lead.state.bak",
            "assets/.DS_Store",
            "assets/a/../b.wav",
            "../song.ts",
            "/abs/song.ts",
            "./song.ts",
            "",
        ] {
            let d = parse(p);
            assert!(matches!(d, DataPath::Unknown(_)), "{p}: {d:?}");
            assert_eq!(d.class(), Class::Ignored, "{p}");
        }
        assert!(parse("data/kick/a/b.json").unknown_data());
        assert!(parse("data/Kick").unknown_data());
        assert!(!parse("README.md").unknown_data());
        assert!(!parse("data").unknown_data(), "the folder itself");
        assert!(!parse("assets/.DS_Store").unknown_data());
    }

    // ID-9
    #[test]
    fn device_name_keys_get_a_hyphen_in_every_segment() {
        let state = DataPath::PluginState { module: key("aux"), path: vec![key("con"), key("nul")], sidecar: true };
        assert_eq!(state.path(), PathBuf::from("data/aux-/con-/nul-.state.json"));
        assert_eq!(parse("data/aux-/con-/nul-.state.json"), state);
        let lane = DataPath::Lane { module: key("kick"), lane: key("con") };
        assert_eq!(lane.path(), PathBuf::from("data/kick/con-.json"));
        assert_eq!(parse("data/kick/con-.json"), lane);
        assert!(matches!(parse("data/aux/hits.json"), DataPath::Unknown(_)), "never a bare device name");
        assert!(matches!(parse("data/kick-/hits.json"), DataPath::Unknown(_)), "no key ends in `-`");
    }

    // WRITE-12: the directory hash, computed by hand.
    #[test]
    fn a_folders_hash() {
        let (a, b) = (crate::Sha::of(b"a"), crate::Sha::of(b"b"));
        let lines = format!("hits.json\0{}\nlead.state\0{}", a.hex(), b.hex());
        let want = crate::Sha::of(lines.as_bytes());
        assert_eq!(dir_hash(&[("lead.state".into(), b), ("hits.json".into(), a)]), want, "sorted, whatever the order given");
        assert_eq!(dir_hash(&[]), crate::Sha::of(b""));
    }

    // FMT-14: a taken name gets a numeric suffix.
    #[test]
    fn an_import_never_takes_a_name_in_use() {
        let taken = ["kick.wav", "kick 2.wav", "vox/take", "vox/take 2"];
        let name = |s: &str| AssetName::new(s).unwrap().untaken(|n| taken.contains(&n)).as_str().to_string();
        assert_eq!(name("kick.wav"), "kick 3.wav");
        assert_eq!(name("snare.wav"), "snare.wav");
        assert_eq!(name("vox/take"), "vox/take 3");
        assert_eq!(name("vox/kick.wav"), "vox/kick.wav");
    }

    // FMT-14
    #[test]
    fn asset_names() {
        for ok in ["kick.wav", "kick 2.wav", "vox/lead.wav", "Drums/Kick.WAV", "café.wav", "ベース.wav", "a/b/c/d.wav"] {
            assert!(AssetName::new(ok).is_ok(), "{ok}");
        }
        let nfd = "cafe\u{301}.wav";
        for bad in [
            "",
            "/kick.wav",
            "kick.wav/",
            "a//b.wav",
            "./a.wav",
            "../a.wav",
            "a/../b.wav",
            ".hidden.wav",
            "a/.b/c.wav",
            "a\\b.wav",
            "a\nb.wav",
            nfd,
        ] {
            assert!(AssetName::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(AssetName::folded("Drums/KICK.wav"), AssetName::folded("drums/kick.wav"));
        assert_eq!(AssetName::folded(nfd), AssetName::folded("café.wav"));
    }

    // FMT-14, lane A's Q2: an imported file's own name made an asset's name.
    #[test]
    fn for_import_names_are_asset_names() {
        let rows = [
            ("vox.wav", "vox.wav"),
            ("Lead Vox.WAV", "Lead Vox.wav"),
            ("cafe\u{301}.wav", "café.wav"),
            ("a/b\\c:d.wav", "a-b-c-d.wav"),
            (".hidden.wav", "hidden.wav"),
            ("...wav", "wav"),
            ("  spaced .wav", "spaced.wav"),
            ("tab\there\n.wav", "tabhere.wav"),
            ("", "audio.wav"),
            (".", "audio.wav"),
            ("noext", "noext"),
            ("ベース.Wave", "ベース.wave"),
        ];
        for (from, want) in rows {
            let got = AssetName::for_import(from);
            assert_eq!(got.as_str(), want, "{from:?}");
            assert!(AssetName::new(got.as_str()).is_ok(), "{from:?}");
        }
        // Cut to whole characters: 58 two-byte é and `.wav`.
        let cut = AssetName::for_import(&format!("{}.WAV", "é".repeat(100)));
        assert_eq!(cut.as_str(), format!("{}.wav", "é".repeat(58)));
        assert!(cut.as_str().len() <= IMPORT_BYTES);
    }

    // D2: a taken name is compared in any capitals and Unicode form.
    #[test]
    fn an_import_name_is_free_in_any_capitals() {
        let name = AssetName::for_import("Vox.wav");
        assert_eq!(name.untaken_folded(["vox.WAV", "VOX 2.wav"]).as_str(), "Vox 3.wav");
        assert_eq!(name.untaken_folded(["kick.wav"]).as_str(), "Vox.wav");
        assert_eq!(AssetName::for_import("café.wav").untaken_folded(["CAFE\u{301}.WAV"]).as_str(), "café 2.wav");
    }
}

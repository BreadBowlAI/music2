//! `music2.toml`: the project's name, entry, format and sample rate, and the versions of the
//! SDK and packages its code is written against (FMT-2, FMT-3, FMT-4).
//!
//! ```toml
//! [project]
//! name = "Starter beat"   # required: the project's label
//! entry = "song.ts"       # optional, default "song.ts"
//! format = 1              # required: the version of the file layout
//! sample_rate = 48000     # required: the rate the engine runs and exports at
//!
//! [dependencies]
//! "@m2/core" = "1"        # required: the SDK version the code is written against
//! std = "0.2"             # every package the code imports from must be listed
//! "@alice/felt-piano" = "1.2"                  # a pack from the store (PACK-7)
//! "@bob/strings" = { path = "../strings" }     # a pack in a folder
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::diag::Diagnostic;
use crate::path::{DataPath, MANIFEST};

/// The file layout this app reads and writes.
pub const FORMAT: u32 = 1;
/// The dependency every project lists: the SDK.
pub const SDK: &str = "@m2/core";
/// The package the app bundles, one version of it (FMT-3).
const STD: &str = crate::pack::STD;

/// The rates a project may run at. Outside this, a typo (`4800`) would silently run the
/// whole engine at a rate no device or file uses.
const RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub project: ProjectInfo,
    /// Package name → what it must be. `@m2/core` is required.
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

/// A `[dependencies]` entry (PACK-7): a requirement (FMT-2), or a pack in a folder, relative to
/// the project, for its author trying it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    Version(Requirement),
    Path(PathDependency),
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathDependency {
    pub path: String,
}

impl Dependency {
    /// The requirement, when it is one.
    pub fn requirement(&self) -> Option<&Requirement> {
        match self {
            Dependency::Version(r) => Some(r),
            Dependency::Path(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    /// The project's label.
    pub name: String,
    /// The root module, relative to the project folder.
    #[serde(default = "default_entry")]
    pub entry: String,
    pub format: u32,
    pub sample_rate: u32,
}

fn default_entry() -> String {
    "song.ts".into()
}

impl Manifest {
    /// Read a manifest: its `format` first, on its own and leniently (FMT-3), then the rest
    /// strictly (FMT-4). An error's code says which: `format.missing`, `format.older` and
    /// `format.newer` mean the project doesn't open; `manifest.invalid` blocks acceptance.
    pub fn read(text: &str) -> Result<Manifest, Diagnostic> {
        check_format(text)?;
        let invalid = |message: String| Diagnostic::error("manifest.invalid", message).file(MANIFEST);
        let m: Manifest = toml::from_str(text).map_err(|e| invalid(format!("{MANIFEST} can't be read: {}", e.message())))?;
        let p = &m.project;
        if !matches!(DataPath::parse(Path::new(&p.entry)), DataPath::Code(_)) {
            return Err(invalid(format!("{MANIFEST}: `entry = \"{}\"` must name a .ts file in the project, such as \"song.ts\"", p.entry)));
        }
        if !RATES.contains(&p.sample_rate) {
            return Err(invalid(format!(
                "{MANIFEST}: `sample_rate = {}` is not a rate music2 runs at; use one from {} to {}, such as 48000",
                p.sample_rate,
                RATES.start(),
                RATES.end()
            )));
        }
        for (name, dep) in &m.dependencies {
            let named = match name.as_str() {
                SDK | STD => Ok(()),
                other => crate::pack::PackName::new(other).map(drop),
            };
            if let Err(why) = named {
                return Err(invalid(format!("{MANIFEST}: [dependencies] {why}")));
            }
            if matches!(name.as_str(), SDK | STD) && dep.requirement().is_none() {
                return Err(invalid(format!("{MANIFEST}: [dependencies] {name} is a version this app has, such as \"1\", never a folder")));
            }
        }
        if !m.dependencies.contains_key(SDK) {
            return Err(invalid(format!(
                "{MANIFEST} must list the SDK the code is written against: add `\"{SDK}\" = \"1\"` under [dependencies]"
            )));
        }
        Ok(m)
    }

    /// Check every `[dependencies]` requirement against what is installed (FMT-3): an unmet
    /// `@m2/core` is `sdk.unsatisfied`, any other package `pkg.unsatisfied`. `installed` gives
    /// the version of a package, or `None` when it isn't installed. A song that asks for an older
    /// std is also told how to move to the one the app has; never for a newer std, or another
    /// package. A pack in a folder has no requirement: resolving it says what it is (PACK-9).
    pub fn check_dependencies(&self, installed: impl Fn(&str) -> Option<Version>) -> Vec<Diagnostic> {
        let mut out = vec![];
        for (name, req) in self.dependencies.iter().filter_map(|(n, d)| Some((n, d.requirement()?))) {
            let code = if name == SDK { "sdk.unsatisfied" } else { "pkg.unsatisfied" };
            let have = installed(name);
            if have.is_some_and(|v| req.matches(v)) {
                continue;
            }
            let message = match have {
                // What to do comes first: the app shows a message on one line, cut at the window's edge.
                Some(v) if name == STD && v >= req.upper() => {
                    let now = Requirement::admitting(v);
                    format!(
                        "This app has std {now}. To use it, change the std line in {MANIFEST} to std = \"{now}\". The song may sound a \
                         little different. {MANIFEST} asks for {name} \"{req}\" (at least {}, below {}), but {v} is installed",
                        req.0,
                        req.upper()
                    )
                }
                Some(v) => {
                    format!("{MANIFEST} asks for {name} \"{req}\" (at least {}, below {}), but {v} is installed", req.0, req.upper())
                }
                None => format!("{MANIFEST} asks for {name} \"{req}\", which isn't installed"),
            };
            out.push(Diagnostic::error(code, message).file(MANIFEST));
        }
        out
    }
}

/// `[project] format`, read before anything else and without the rest of the schema: a
/// strict read would hide a newer format behind `manifest.invalid`, and a manifest whose
/// TOML is broken elsewhere still says its format on its own line.
fn check_format(text: &str) -> Result<(), Diagnostic> {
    let found = match toml::from_str::<toml::Table>(text) {
        Ok(t) => t.get("project").and_then(|p| p.get("format")).map(|f| f.as_integer().ok_or(())),
        Err(_) => format_line(text),
    };
    let fail = |code, message: String| Err(Diagnostic::error(code, message).file(MANIFEST));
    match found {
        None => fail(
            "format.missing",
            format!(
                "{MANIFEST} has no `format` under [project], so it is a project from before format {FORMAT}. Convert it with the \
                 conversion script, then open it again"
            ),
        ),
        Some(Err(())) => {
            fail("format.missing", format!("{MANIFEST}: [project] `format` must be a whole number, such as `format = {FORMAT}`"))
        }
        Some(Ok(f)) if f < FORMAT as i64 => fail(
            "format.older",
            format!("this project is in format {f}, older than the format {FORMAT} this app reads. Convert it first, then open it again"),
        ),
        Some(Ok(f)) if f > FORMAT as i64 => fail(
            "format.newer",
            format!("this project is in format {f}, newer than the format {FORMAT} this app reads. Open it with a newer music2"),
        ),
        Some(Ok(_)) => Ok(()),
    }
}

/// `format = N` on its own line in the `[project]` table of a manifest that doesn't parse.
fn format_line(text: &str) -> Option<Result<i64, ()>> {
    let mut in_project = false;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_project = line == "[project]";
        } else if in_project
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == "format"
        {
            return Some(v.trim().parse().map_err(drop));
        }
    }
    None
}

/// A released version: `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `1.2.3`.
    pub fn parse(s: &str) -> Option<Version> {
        let p: Vec<u64> = s.split('.').map(number).collect::<Option<_>>()?;
        match p[..] {
            [a, b, c] => Some(Version(a, b, c)),
            _ => None,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A part of a version: digits only (`u64::from_str` would take `+1`).
fn number(s: &str) -> Option<u64> {
    if s.bytes().all(|c| c.is_ascii_digit()) { s.parse().ok() } else { None }
}

/// A version requirement with Cargo's caret rule (FMT-2): `"0.1"` is at least 0.1.0 and below
/// 0.2.0, `"1"` at least 1.0.0 and below 2.0.0. Only bare versions are written, never `>=` or `~`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement(Version, usize);

impl Requirement {
    pub fn parse(s: &str) -> Result<Requirement, String> {
        let bad = || format!("`{s}` is not a version requirement; write a version such as \"1\", \"0.1\" or \"1.2.3\"");
        let parts: Vec<u64> = s.split('.').map(number).collect::<Option<_>>().ok_or_else(bad)?;
        match parts[..] {
            [a] => Ok(Requirement(Version(a, 0, 0), 1)),
            [a, b] => Ok(Requirement(Version(a, b, 0), 2)),
            [a, b, c] => Ok(Requirement(Version(a, b, c), 3)),
            _ => Err(bad()),
        }
    }

    /// The first version the requirement no longer admits: the next change of the leftmost
    /// non-zero part written, or of the last part written when all are zero.
    pub fn upper(&self) -> Version {
        let Version(a, b, c) = self.0;
        match (a, b, self.1) {
            (0, 0, 3) => Version(0, 0, c + 1),
            (0, 0, 2) => Version(0, b + 1, 0),
            (0, _, n) if n >= 2 => Version(0, b + 1, 0),
            _ => Version(a + 1, 0, 0),
        }
    }

    pub fn matches(&self, v: Version) -> bool {
        v >= self.0 && v < self.upper()
    }

    /// `v`'s major and minor: `"1.2"` for 1.2.5, what adding a pack's item writes (PACK-10).
    pub fn minor(v: Version) -> Requirement {
        Requirement(Version(v.0, v.1, 0), 2)
    }

    /// The shortest requirement that admits `v`: `"0.2"` for 0.2.0, `"1"` for 1.4.2.
    pub fn admitting(v: Version) -> Requirement {
        match v {
            Version(0, b, _) => Requirement(Version(0, b, 0), 2),
            Version(a, _, _) => Requirement(Version(a, 0, 0), 1),
        }
    }
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Version(a, b, c) = self.0;
        match self.1 {
            1 => write!(f, "{a}"),
            2 => write!(f, "{a}.{b}"),
            _ => write!(f, "{a}.{b}.{c}"),
        }
    }
}

impl Serialize for Requirement {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Requirement {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Requirement, D::Error> {
        Requirement::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str =
        "[project]\nname = \"Starter beat\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";

    fn code(text: &str) -> &'static str {
        Manifest::read(text).unwrap_err().code
    }

    #[test]
    fn a_v1_manifest() {
        let m = Manifest::read(GOOD).unwrap();
        assert_eq!(
            (m.project.name.as_str(), m.project.entry.as_str(), m.project.format, m.project.sample_rate),
            ("Starter beat", "song.ts", 1, 48000)
        );
        assert_eq!(m.dependencies.keys().collect::<Vec<_>>(), ["@m2/core", "std"]);
    }

    // FMT-3: format is read first, leniently, and refused when missing, older or newer.
    #[test]
    fn format_first() {
        assert_eq!(code("[project]\nname = \"x\"\n"), "format.missing");
        assert_eq!(code("[project]\nname = \"x\"\nformat = \"1\"\n"), "format.missing");
        assert_eq!(code("[project]\nname = \"x\"\nformat = 0\n"), "format.older");
        assert_eq!(code(&GOOD.replace("format = 1", "format = 2")), "format.newer");
        // The acceptance gate: a newer format with an unknown key is format.newer, never manifest.invalid.
        assert_eq!(code(&GOOD.replace("format = 1", "format = 2\nfuture = true")), "format.newer");
        // Leniently: a manifest that doesn't parse still says its format on its own line.
        assert_eq!(code("[project]\nname = \"x\nformat = 2 # later\n"), "format.newer");
        assert_eq!(code("[project]\nname = \"x\nformat = 1\n"), "manifest.invalid");
        assert_eq!(code("[other]\nformat = 1\n[project]\nname = \"x\n"), "format.missing");
        assert!(Manifest::read("[project]\nname = \"x\"\nformat = 3\n").unwrap_err().message.contains("newer music2"));
    }

    // FMT-4: after format, strictly.
    #[test]
    fn strict_after_format() {
        for bad in [
            GOOD.replace("sample_rate = 48000", "sample_rate = 48000\ncolour = \"red\""),
            GOOD.replace("[dependencies]", "[dependencies]\n\"other\" = \"x\""),
            GOOD.replace("[dependencies]", "[locks]\nx = 1\n[dependencies]"),
            GOOD.replace("name = \"Starter beat\"\n", ""),
            GOOD.replace("sample_rate = 48000\n", ""),
            GOOD.replace("sample_rate = 48000", "sample_rate = 4800"),
            GOOD.replace("\"@m2/core\" = \"1\"\n", ""),
            GOOD.replace("name = \"Starter beat\"", "name = \"x\"\nentry = \"song.js\""),
            GOOD.replace("name = \"Starter beat\"", "name = \"x\"\nentry = \"../song.ts\""),
            GOOD.replace("std = \"0.2\"", "std = \">=0.2\""),
        ] {
            let e = Manifest::read(&bad).unwrap_err();
            assert_eq!((e.code, e.src.as_deref()), ("manifest.invalid", Some(MANIFEST)), "{bad}");
        }
        assert!(
            Manifest::read(&GOOD.replace("sample_rate = 48000", "sample_rate = 48000\ncolour = 1")).unwrap_err().message.contains("colour")
        );
    }

    // PACK-7: a pack's entry is named by PACK-1, and is a requirement or a folder; the SDK and std
    // are always versions.
    #[test]
    fn pack_dependencies_are_named_and_may_be_folders() {
        let m =
            Manifest::read(&GOOD.replace("[dependencies]", "[dependencies]\n\"@a/b\" = \"1.2\"\n\"@c/d\" = { path = \"../d\" }")).unwrap();
        assert_eq!(m.dependencies["@a/b"], Dependency::Version(Requirement::parse("1.2").unwrap()));
        assert_eq!(m.dependencies["@c/d"], Dependency::Path(PathDependency { path: "../d".into() }));
        for bad in [
            "\"drums\" = \"1\"",
            "\"@A/b\" = \"1\"",
            "\"@a/b\" = { path = \"../d\", version = \"1\" }",
            "\"@a/b\" = { git = \"x\" }",
            "std = { path = \"../std\" }",
        ] {
            let text = GOOD.replace("std = \"0.2\"", bad).replace("[dependencies]", "[dependencies]\nstd = \"0.2\"");
            let text = if bad.starts_with("std") { GOOD.replace("std = \"0.2\"", bad) } else { text };
            assert_eq!(Manifest::read(&text).map_err(|e| e.code), Err("manifest.invalid"), "{text}");
        }
        assert_eq!(Requirement::minor(Version(1, 2, 5)).to_string(), "1.2");
        assert_eq!(Requirement::minor(Version(0, 4, 1)).to_string(), "0.4");
        // A folder has no version to check: resolving it says what it is (PACK-9).
        assert!(
            m.check_dependencies(|n| Version::parse(if n == SDK {
                "1.0.0"
            } else if n == "@a/b" {
                "1.2.0"
            } else {
                "0.2.0"
            }))
            .is_empty()
        );
    }

    // FMT-2: Cargo's caret rule.
    #[test]
    fn caret_requirements() {
        let req = |s| Requirement::parse(s).unwrap();
        let v = |s| Version::parse(s).unwrap();
        let cases = [
            ("1", "1.0.0", true),
            ("1", "1.9.3", true),
            ("1", "2.0.0", false),
            ("1", "0.9.0", false),
            ("0.1", "0.1.0", true),
            ("0.1", "0.1.7", true),
            ("0.1", "0.2.0", false),
            ("0.1", "0.0.9", false),
            ("1.2", "1.2.0", true),
            ("1.2", "1.1.9", false),
            ("1.2.3", "1.2.3", true),
            ("1.2.3", "1.9.0", true),
            ("1.2.3", "1.2.2", false),
            ("0.1.2", "0.1.9", true),
            ("0.1.2", "0.2.0", false),
            ("0.0.3", "0.0.3", true),
            ("0.0.3", "0.0.4", false),
            ("0.0", "0.0.9", true),
            ("0.0", "0.1.0", false),
            ("0", "0.9.9", true),
            ("0", "1.0.0", false),
        ];
        for (r, version, ok) in cases {
            assert_eq!(req(r).matches(v(version)), ok, "{r} admits {version}");
        }
        for bad in ["", "x", "1.", ">=1", "~1.2", "^1", "1.2.3.4", "1.2.3-beta", "*"] {
            assert!(Requirement::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(req("0.1").to_string(), "0.1");
        for (version, shortest) in [("0.2.0", "0.2"), ("0.2.7", "0.2"), ("0.0.3", "0.0"), ("1.4.2", "1"), ("12.0.0", "12")] {
            let r = Requirement::admitting(v(version));
            assert_eq!((r.to_string(), r.matches(v(version))), (shortest.to_string(), true), "{version}");
        }
        assert_eq!(Version::parse("0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("1.2"), None);
    }

    // FMT-3: each dependency is checked, the SDK with its own code.
    #[test]
    fn unmet_dependencies() {
        let m = Manifest::read(GOOD).unwrap();
        let installed = |sdk: &'static str, std: Option<&'static str>| {
            move |name: &str| match name {
                SDK => Version::parse(sdk),
                "std" => std.and_then(Version::parse),
                _ => None,
            }
        };
        assert!(m.check_dependencies(installed("1.0.0", Some("0.2.4"))).is_empty());
        let d = m.check_dependencies(installed("2.0.0", Some("0.3.0")));
        assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), ["sdk.unsatisfied", "pkg.unsatisfied"]);
        assert!(d[0].message.contains("\"1\"") && d[0].message.contains("2.0.0"), "names both versions: {}", d[0].message);
        assert!(d[1].message.contains("\"0.2\"") && d[1].message.contains("0.3.0"), "{}", d[1].message);
        // A song for an older std is told how to move (L7), first, so a line cut short at the
        // window's edge still shows it (review finding D3); a newer SDK says nothing of the kind.
        assert!(
            d[1].message.starts_with(
                "This app has std 0.3. To use it, change the std line in music2.toml to std = \"0.3\". The song may sound a little different. "
            ),
            "{}",
            d[1].message
        );
        assert!(!d[0].message.contains("This app has"), "{}", d[0].message);
        // A std older than the song asks for needs a newer music2, so no such hint.
        let d = m.check_dependencies(installed("1.0.0", Some("0.1.9")));
        assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        assert!(!d[0].message.contains("This app has"), "{}", d[0].message);
        // Only std: another package asked for at an older version gets none.
        let drums = Manifest::read(&GOOD.replace("[dependencies]", "[dependencies]\n\"@x/drums\" = \"1\"")).unwrap();
        let d = drums.check_dependencies(|name: &str| {
            Version::parse(if name == SDK {
                "1.0.0"
            } else if name == STD {
                "0.2.0"
            } else {
                "2.0.0"
            })
        });
        assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        assert!(d[0].message.contains("drums") && !d[0].message.contains("This app has"), "{}", d[0].message);
        let d = m.check_dependencies(installed("1.0.0", None));
        assert_eq!(d.iter().map(|d| d.code).collect::<Vec<_>>(), ["pkg.unsatisfied"]);
        assert!(d[0].message.contains("isn't installed"));
    }
}

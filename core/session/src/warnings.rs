//! What needs a person's attention in a project that plays. Today that is lane data no
//! declared lane reads: a lane's file is named after its track and lane
//! (`timeline/kick.hits.json`), so renaming either in code leaves the data behind and the
//! lane plays nothing. Each warning says what happened in plain words and offers fixes;
//! nothing applies a fix by itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use m2_ir::{Ir, Kind, LaneDecl, Module, Unit};
use m2_song::{LaneData, Project, Timeline, Unused, slug};
use serde::Serialize;

/// A problem that doesn't stop the song from playing.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Warning {
    /// What kind of problem it is: `data.unclaimed`.
    pub code: &'static str,
    /// What is wrong, in a few words: "Notes on Kick › hits aren't playing".
    pub title: String,
    /// Why, and how to fix it, in plain words.
    pub message: String,
    /// The file it is about, relative to the project.
    pub file: String,
    /// What can be done about it, the likeliest first.
    pub fixes: Vec<Fix>,
}

/// The title, then the message: "Notes on Kick › hits aren't playing. There's no track…".
impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}. {}", self.title, self.message)
    }
}

/// One way to fix a warning, applied only when someone asks for it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// Move the file to a declared lane that can read it and has no data yet: what its lane,
    /// or its lane's track, is likely called now. Paths are relative to the project. `likely`
    /// marks the one lane it plainly belongs to (its own lane when only capitals differ, the
    /// only lane of its kind left on its own track, or the same lane on a track that is gone,
    /// so likely renamed), and only when no other file has as plain a claim on that lane; it
    /// can be offered in one click. Otherwise the person picks.
    Relink { from: String, to: String, module: String, lane: String, likely: bool },
    /// Delete the file.
    DeleteData { file: String },
}

/// What earlier revisions declared, by how lane files spell module names, for warnings about
/// data a rename left behind: each module's name, so the data is named as its track was, and
/// each lane as last declared, so it is offered only to lanes that can be it.
#[derive(Clone, Debug, Default)]
pub struct Seen {
    /// Module names: `lead-vox` → `Lead Vox`.
    pub names: BTreeMap<String, String>,
    /// Each lane as last declared, by module and lane: `(bass, cutoff)`.
    pub lanes: BTreeMap<(String, String), SeenLane>,
}

/// A lane as it was last declared.
#[derive(Clone, Debug, PartialEq)]
pub struct SeenLane {
    /// `control<hz>`: the unit its automation is in.
    pub kind: Kind,
    /// The modules declared with it, as lane files spell them. None of them is its track
    /// renamed, so its data isn't offered to them in one click.
    pub beside: Arc<BTreeSet<String>>,
}

impl Seen {
    /// Take in what `ir` declares, over what earlier revisions did.
    pub fn add(&mut self, ir: &Ir) {
        let beside: Arc<BTreeSet<String>> = Arc::new(ir.modules.iter().map(|m| slug(&m.name)).collect());
        for m in &ir.modules {
            let stem = slug(&m.name);
            let lanes = m.lanes.iter().map(|l| ((stem.clone(), l.id.clone()), SeenLane { kind: l.kind, beside: beside.clone() }));
            self.lanes.extend(lanes);
            self.names.insert(stem, m.name.clone());
        }
    }
}

/// The warnings for `project` compiled to `ir`, given what earlier revisions declared.
pub fn warnings(project: &Project, ir: &Ir, seen: &Seen) -> Vec<Warning> {
    let tl = &project.timeline;
    let unused = tl.unused(&used(ir));
    // Every lane file, read or not. On a disk that ignores capitals (macOS by default) one takes
    // the name of a lane whose file differs from it only in capitals, so that lane can't get data.
    let files: Vec<&Path> = unused.iter().map(|u| u.path).chain(declared(ir).filter_map(|(m, l)| tl.lane_file(&m.name, &l.id))).collect();
    // Declared lanes with no data: where data left behind can go.
    let free: Vec<(&Module, &LaneDecl, PathBuf)> = declared(ir)
        .filter(|(m, l)| tl.lane_file(&m.name, &l.id).is_none())
        .filter_map(|(m, l)| Some((m, l, crate::lanes::file(&m.name, &l.id).ok()?)))
        .collect();
    // Where each file can go, as indexes into `free`, plainest first.
    let to: Vec<Vec<(usize, Claim)>> = unused
        .iter()
        .map(|u| {
            let held = Held::of(tl, u, seen);
            let here = ir.modules.iter().any(|m| ours(u, m));
            let beside = seen.lanes.get(&(u.stem.to_string(), u.lane.to_string())).map(|l| &*l.beside);
            let mut to: Vec<(usize, Claim)> = free
                .iter()
                .enumerate()
                .filter(|(_, (_, l, p))| held.unreadable(l.kind).is_none() && !files.iter().any(|f| *f != u.path && same_name(f, p)))
                .map(|(i, (m, l, _))| (i, Claim::new(u, &held, here || beside.is_some_and(|b| b.contains(&slug(&m.name))), m, l)))
                .collect();
            to.sort_by_key(|(_, c)| *c);
            to
        })
        .collect();
    let mut out = vec![];
    for (n, u) in unused.iter().enumerate() {
        let file = crate::relative(&project.dir, u.path).replace('\\', "/");
        let here = ir.modules.iter().find(|m| ours(u, m)).map(|m| m.name.as_str());
        let name = here.or(seen.names.get(u.stem).map(String::as_str)).unwrap_or(u.stem);
        let mine = &to[n];
        // Plainly where it belongs: a plain claim, alone in first place, on a lane no other file
        // has as plain a claim on (two files left by one change can't both be the renamed one's).
        let likely = match &mine[..] {
            [(lane, first), rest @ ..] => {
                !first.loose
                    && rest.first().is_none_or(|(_, next)| next != first)
                    && to.iter().enumerate().all(|(k, theirs)| k == n || theirs.iter().all(|(l, c)| l != lane || c > first))
            }
            [] => false,
        };
        let path = |i: usize| free[i].2.display().to_string().replace('\\', "/");
        let mut fixes: Vec<Fix> = mine
            .iter()
            .enumerate()
            .map(|(k, &(i, _))| Fix::Relink {
                from: file.clone(),
                to: path(i),
                module: free[i].0.name.clone(),
                lane: free[i].1.id.clone(),
                likely: likely && k == 0,
            })
            .collect();
        fixes.push(Fix::DeleteData { file: file.clone() });

        let place = format!("{name} › {}", u.lane);
        let places: Vec<String> = mine.iter().map(|&(i, _)| format!("{} › {}", free[i].0.name, free[i].1.id)).collect();
        // Its own lane, which reads its name in other capitals.
        let own = mine.first().filter(|(_, c)| !c.elsewhere && !c.renamed).map(|&(i, _)| path(i));
        let why = match (&own, here) {
            (Some(own), _) => format!("The file is named {file}, but {place} reads {own}, and capitals count."),
            (None, Some(_)) => format!("{name} has no lane called {} any more.", u.lane),
            (None, None) => format!("There's no track called {name} any more."),
        };
        let to = match (&places[..], own) {
            ([one, ..], Some(_)) => To::Own(one),
            ([one, ..], None) if likely => To::Likely(one),
            ([], _) if here.is_some() => To::Nowhere(format!("a lane called {} to {name}", u.lane)),
            ([], _) => To::Nowhere(format!("a track called {name} with a lane called {}", u.lane)),
            (many, _) => To::Choice(many),
        };
        let (title, message) = words(u.data, &place, &why, to, &file);
        out.push(Warning { code: "data.unclaimed", title, message, file, fixes });
    }
    out
}

/// Every declared lane, with its module.
pub(crate) fn declared(ir: &Ir) -> impl Iterator<Item = (&Module, &LaneDecl)> {
    ir.modules.iter().flat_map(|m| m.lanes.iter().map(move |l| (m, l)))
}

/// Every declared lane, as `(module, lane)`.
pub(crate) fn used(ir: &Ir) -> Vec<(String, String)> {
    declared(ir).map(|(m, l)| (m.name.clone(), l.id.clone())).collect()
}

/// Whether `u` is named after `module`, capitals aside: a file named `Kick.hits.json` is still
/// Kick's, which reads `kick.hits.json`.
fn ours(u: &Unused, module: &Module) -> bool {
    slug(&module.name).eq_ignore_ascii_case(u.stem)
}

/// Whether two lane files have one name on a disk that ignores capitals.
fn same_name(a: &Path, b: &Path) -> bool {
    a.file_name().zip(b.file_name()).is_some_and(|(a, b)| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()))
}

/// How plainly a free lane is where a file's data belongs; the plainest sorts first: its own
/// lane in other capitals, then the lanes left on its own track, then the same lane on another
/// track, then the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Claim {
    /// Only ever a choice, never offered in one click.
    loose: bool,
    /// On another track than the one the file is named after.
    elsewhere: bool,
    /// Under another name than the file's lane.
    renamed: bool,
}

impl Claim {
    /// The claim of `held`, the data in `u`, on `module`'s lane `lane`. `kept` is whether
    /// `module` can't be the track `u` is named after, renamed: that track is still there, or
    /// `module` was there beside it. Then another track's lane is only a choice. A lane under
    /// another name is only a choice for automation of a unit not known, since it may be a lane
    /// of another unit.
    fn new(u: &Unused, held: &Held, kept: bool, module: &Module, lane: &LaneDecl) -> Claim {
        let (elsewhere, renamed) = (!ours(u, module), !lane.id.eq_ignore_ascii_case(u.lane));
        let loose = match (elsewhere, renamed) {
            (false, false) => false,
            (false, true) => held.data == LaneData::Control && held.unit.is_none(),
            (true, false) => kept,
            (true, true) => true,
        };
        Claim { loose, elsewhere, renamed }
    }
}

/// What a lane file no lane reads holds, as far as can be told.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Held {
    pub(crate) data: LaneData,
    /// For automation, the unit of the lane that last read it in this session. Lane files don't
    /// record it, so in a project just opened it isn't known.
    unit: Option<Unit>,
    /// For automation, its lowest and highest values.
    span: Option<(f64, f64)>,
}

impl Held {
    pub(crate) fn of(timeline: &Timeline, u: &Unused, seen: &Seen) -> Held {
        let control = u.data == LaneData::Control;
        let unit = match seen.lanes.get(&(u.stem.to_string(), u.lane.to_string())).map(|l| l.kind) {
            Some(Kind::Control(unit)) if control => unit,
            _ => None,
        };
        // Read as a lane of the track it is named after reads it; no track's lane reads a name
        // like `Kick.hits.json`, so its values aren't known.
        let lane = (control && slug(u.stem) == u.stem).then(|| timeline.control(u.stem, u.lane).ok()).flatten();
        let values = lane.iter().flat_map(|l| l.clips()).flat_map(|c| &c.points).map(|p| p.1);
        let span = values.fold(None, |s: Option<(f64, f64)>, v| Some(s.map_or((v, v), |(lo, hi)| (lo.min(v), hi.max(v)))));
        Held { data: u.data, unit, span }
    }

    /// What it holds, in a few words, if a lane of `kind` can't read it: `notes`,
    /// `automation in hz`.
    pub(crate) fn unreadable(&self, kind: Kind) -> Option<String> {
        let reads = match self.data {
            LaneData::Empty => true,
            LaneData::Notes => matches!(kind, Kind::Events(_)),
            LaneData::Control => matches!(kind, Kind::Control(_)),
            LaneData::Audio => matches!(kind, Kind::Audio(_)),
            LaneData::Unknown => false,
        };
        if !reads {
            return Some(noun(self.data).to_string());
        }
        let Kind::Control(Some(into)) = kind else { return None };
        if let Some(unit) = self.unit
            && !unit.fits(into)
        {
            return Some(format!("automation in {}", unit.name()));
        }
        // Units whose values are bounded by what they are, whatever unit the data had.
        let (lo, hi) = match into {
            Unit::Unit01 => (0.0, 1.0),
            Unit::Bipolar => (-1.0, 1.0),
            _ => return None,
        };
        match self.span {
            Some((low, high)) if low < lo || high > hi => Some(format!("automation that reaches {}", if high > hi { high } else { low })),
            _ => None,
        }
    }
}

/// What a lane file holds, in a few words: `notes`.
pub(crate) fn noun(data: LaneData) -> &'static str {
    match data {
        LaneData::Notes => "notes",
        LaneData::Control => "automation",
        LaneData::Audio => "audio clips",
        LaneData::Empty => "empty lane",
        LaneData::Unknown => "lane data",
    }
}

/// Where data no lane reads can go, as `Track › lane`.
enum To<'a> {
    /// Its own lane, which reads a file whose name differs only in capitals.
    Own(&'a str),
    /// The one lane it plainly belongs to.
    Likely(&'a str),
    /// Lanes that could take it, likeliest first.
    Choice(&'a [String]),
    /// Nowhere: what to add so it plays again.
    Nowhere(String),
}

/// A warning's title and message about data no lane reads, from where it was (`place`, as
/// `Track › lane`), `why` no lane reads it, and where it can go.
fn words(data: LaneData, place: &str, why: &str, to: To<'_>, file: &str) -> (String, String) {
    // What the data is, the words that agree with it, and what being unread means for it.
    let (what, it, they, belong, unread, hear) = match data {
        LaneData::Notes => ("Notes", "them", "they", "belong", "aren't playing", "hear"),
        LaneData::Control => ("Automation", "it", "it", "belongs", "isn't used", "hear"),
        LaneData::Audio => ("Audio clips", "them", "they", "belong", "aren't playing", "hear"),
        LaneData::Empty => ("An empty lane file", "it", "it", "belongs", "isn't used", "use"),
        LaneData::Unknown => {
            let fix = "It doesn't hold notes, automation or audio clips that a lane can read: fix it by hand, or delete it.";
            return (format!("{file} isn't used"), format!("{why} {fix}"));
        }
    };
    let fix = match to {
        To::Own(own) => format!("Move {it} to {own} to {hear} {it} again, or delete {it}."),
        To::Likely(one) => format!("Did you rename it? Move {it} to {one} to {hear} {it} again, or delete {it}."),
        To::Choice(places) => format!("Did you rename it? Move {it} to the lane {they} {belong} to now ({}), or delete {it}.", or(places)),
        To::Nowhere(add) => format!("To {hear} {it} again, add {add}, or delete {it}."),
    };
    (format!("{what} on {place} {unread}"), format!("{why} {fix}"))
}

/// `a`, `a or b`, `a, b or c`.
fn or(items: &[String]) -> String {
    match items {
        [rest @ .., last] if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => items.join(""),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use m2_runtime::PackageSet;

    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-warnings-test-{}-{n}", std::process::id()));
        for (name, text) in files {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        dir
    }

    fn compiled(dir: &Path) -> Vec<Warning> {
        seeing(dir, &Seen::default())
    }

    /// The warnings for the project in `dir`, as a session that has seen `seen` gives them.
    fn seeing(dir: &Path, seen: &Seen) -> Vec<Warning> {
        let mut packages = PackageSet::default();
        packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        let project = Project::open(dir).unwrap();
        let ir = crate::compile(&project, &mut packages).unwrap_or_else(|d| panic!("{d:?}")).ir;
        warnings(&project, &ir, seen)
    }

    /// What a session knows once `lanes`, as `(stem, lane, kind)`, were declared together.
    fn seen(lanes: &[(&str, &str, &str)]) -> Seen {
        let beside: Arc<BTreeSet<String>> = Arc::new(lanes.iter().map(|(m, ..)| m.to_string()).collect());
        let lane = |k: &str| SeenLane { kind: k.parse().unwrap(), beside: beside.clone() };
        Seen { lanes: lanes.iter().map(|(m, l, k)| ((m.to_string(), l.to_string()), lane(k))).collect(), ..Seen::default() }
    }

    fn song(kick: &str, lanes: &str) -> String {
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ kick }} from \"std/instruments\";\n\
             const k = track(\"{kick}\", ({{ lane }}) => {{ {lanes} return kick(lane.notes(\"hits\")); }});\n\
             export default song(() => k.out);\n"
        )
    }

    const NOTES: &str = r#"{"clips": [{"start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]}]}"#;
    const CONTROL: &str = r#"{"clips": [{"start": 0, "length": 4, "points": [[0, 1]]}]}"#;

    /// A song of tracks, each a kick with a notes lane (`hits` unless named as `Track.lane`).
    fn tracks(names: &[&str]) -> String {
        let made: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let (n, lane) = n.split_once('.').unwrap_or((n, "hits"));
                format!("const t{i} = track(\"{n}\", ({{ lane }}) => kick(lane.notes(\"{lane}\")));\n")
            })
            .collect();
        let outs: Vec<String> = (0..names.len()).map(|i| format!("t{i}.out")).collect();
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ kick }} from \"std/instruments\";\n\
             import {{ sum }} from \"std/math\";\n{}export default song(() => sum({}));\n",
            made.concat(),
            outs.join(", ")
        )
    }

    /// The lanes a warning offers, as `(track, lane, likely)`.
    fn relinks(w: &Warning) -> Vec<(&str, &str, bool)> {
        w.fixes
            .iter()
            .filter_map(|f| {
                if let Fix::Relink { module, lane, likely, .. } = f { Some((module.as_str(), lane.as_str(), *likely)) } else { None }
            })
            .collect()
    }

    fn about<'a>(w: &'a [Warning], file: &str) -> &'a Warning {
        w.iter().find(|w| w.file == file).unwrap_or_else(|| panic!("no warning about {file}: {w:?}"))
    }

    #[test]
    fn data_left_by_a_renamed_track_says_where_it_can_go() {
        let dir = project(&[("song.ts", &song("Kicks", "")), ("timeline/kick.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(w.len(), 1);
        assert_eq!((w[0].code, w[0].file.as_str()), ("data.unclaimed", "timeline/kick.hits.json"));
        assert_eq!(
            w[0].to_string(),
            "Notes on kick › hits aren't playing. There's no track called kick any more. Did you rename it? Move them to Kicks › hits \
             to hear them again, or delete them."
        );
        let relink = Fix::Relink {
            from: "timeline/kick.hits.json".into(),
            to: "timeline/kicks.hits.json".into(),
            module: "Kicks".into(),
            lane: "hits".into(),
            likely: true,
        };
        assert_eq!(w[0].fixes, [relink, Fix::DeleteData { file: "timeline/kick.hits.json".into() }]);
        // A name seen before is the one shown.
        let named = seeing(&dir, &Seen { names: BTreeMap::from([("kick".to_string(), "Kick".to_string())]), ..Seen::default() });
        assert_eq!(
            (named[0].title.as_str(), named[0].message.as_str()),
            (
                "Notes on Kick › hits aren't playing",
                "There's no track called Kick any more. Did you rename it? Move them to Kicks › hits to hear them again, or delete them."
            )
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn data_left_by_a_renamed_lane_offers_every_lane_that_reads_it_likeliest_first() {
        let lanes = "lane.control(\"env\", \"unit01\", 0); lane.notes(\"accents\"); lane.notes(\"ghosts\");";
        let dir = project(&[
            ("song.ts", &song("Kick", lanes).replace("lane.notes(\"hits\")", "lane.notes(\"beats\")")),
            ("timeline/kick.hits.json", NOTES),
            ("timeline/kick.ghosts.json", NOTES),
            ("timeline/kick.sweep.json", CONTROL),
        ]);
        let w = compiled(&dir);
        let hits = about(&w, "timeline/kick.hits.json");
        assert_eq!(
            relinks(hits),
            [("Kick", "accents", false), ("Kick", "beats", false)],
            "notes lanes with no data, in the order written (not ghosts, which has some); two as likely, so the person picks"
        );
        assert_eq!(
            hits.to_string(),
            "Notes on Kick › hits aren't playing. Kick has no lane called hits any more. Did you rename it? Move them to the lane they \
             belong to now (Kick › accents or Kick › beats), or delete them."
        );
        // The only automation lane Kick has left, but a project just opened doesn't know the
        // unit sweep was in: env may be a lane of another unit, so it is only a choice.
        let sweep = about(&w, "timeline/kick.sweep.json");
        assert_eq!(relinks(sweep), [("Kick", "env", false)]);
        assert_eq!(
            sweep.to_string(),
            "Automation on Kick › sweep isn't used. Kick has no lane called sweep any more. Did you rename it? Move it to the lane it \
             belongs to now (Kick › env), or delete it."
        );
        // Seen as a 0..1 lane: plainly where its automation belongs.
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<unit01>")]));
        let sweep = about(&w, "timeline/kick.sweep.json");
        assert_eq!(relinks(sweep), [("Kick", "env", true)]);
        assert_eq!(
            sweep.to_string(),
            "Automation on Kick › sweep isn't used. Kick has no lane called sweep any more. Did you rename it? Move it to Kick › env to \
             hear it again, or delete it."
        );
        // Seen in hertz: a 0..1 lane can't take it.
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<hz>")]));
        let sweep = about(&w, "timeline/kick.sweep.json");
        assert_eq!(relinks(sweep), []);
        assert!(sweep.message.ends_with("To hear it again, add a lane called sweep to Kick, or delete it."), "{}", sweep.message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_lane_is_offered_in_one_click_only_where_the_data_plainly_belongs() {
        // Two renamed tracks with the same lane: either could be Kick's.
        let dir = project(&[("song.ts", &tracks(&["Kicks", "Snares"])), ("timeline/kick.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("Kicks", "hits", false), ("Snares", "hits", false)]);
        assert!(w[0].message.contains("(Kicks › hits or Snares › hits)"), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();

        // A lane under another name on another track is only ever a choice, even alone.
        let dir = project(&[("song.ts", &song("Pad", "").replace("\"hits\"", "\"chords\"")), ("timeline/kick.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("Pad", "chords", false)]);
        assert!(w[0].message.ends_with("Move them to the lane they belong to now (Pad › chords), or delete them."), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_lane_two_files_could_have_come_from_is_only_a_choice() {
        // One change renamed Kick to Kicks and deleted Snare: either file could be Kicks'.
        let dir = project(&[("song.ts", &tracks(&["Kicks"])), ("timeline/kick.hits.json", NOTES), ("timeline/snare.hits.json", NOTES)]);
        let w = compiled(&dir);
        for file in ["timeline/kick.hits.json", "timeline/snare.hits.json"] {
            let w = about(&w, file);
            assert_eq!(relinks(w), [("Kicks", "hits", false)], "{file}");
            assert!(w.message.ends_with("Move them to the lane they belong to now (Kicks › hits), or delete them."), "{}", w.message);
        }
        std::fs::remove_dir_all(dir).ok();

        // A file with a weaker claim doesn't stand in the way: chords on Pad are another lane.
        let dir = project(&[("song.ts", &tracks(&["Kicks"])), ("timeline/kick.hits.json", NOTES), ("timeline/pad.chords.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(about(&w, "timeline/kick.hits.json")), [("Kicks", "hits", true)]);
        assert_eq!(relinks(about(&w, "timeline/pad.chords.json")), [("Kicks", "hits", false)]);
        std::fs::remove_dir_all(dir).ok();

        // Two of Kick's automation lanes renamed, and one left: either could be it.
        let dir = project(&[
            ("song.ts", &song("Kick", "lane.control(\"env\", \"unit01\", 0);")),
            ("timeline/kick.hits.json", NOTES),
            ("timeline/kick.sweep.json", CONTROL),
            ("timeline/kick.wobble.json", CONTROL),
        ]);
        let w = seeing(&dir, &seen(&[("kick", "sweep", "control<unit01>"), ("kick", "wobble", "control<unit01>")]));
        assert_eq!(relinks(about(&w, "timeline/kick.sweep.json")), [("Kick", "env", false)]);
        assert_eq!(relinks(about(&w, "timeline/kick.wobble.json")), [("Kick", "env", false)]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn while_its_track_is_there_another_tracks_lane_is_only_a_choice() {
        // Kick's lane renamed from hits to beats; Clap has a hits lane with no data yet.
        let dir = project(&[("song.ts", &tracks(&["Kick.beats", "Clap"])), ("timeline/kick.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("Kick", "beats", true), ("Clap", "hits", false)]);
        assert_eq!(
            w[0].message,
            "Kick has no lane called hits any more. Did you rename it? Move them to Kick › beats to hear them again, or delete them."
        );
        // Kick's lanes all have data: Clap's lane is still only a choice.
        std::fs::write(dir.join("timeline/kick.beats.json"), NOTES).unwrap();
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("Clap", "hits", false)]);
        assert!(w[0].message.ends_with("Move them to the lane they belong to now (Clap › hits), or delete them."), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_track_that_was_there_beside_the_data_is_not_its_track_renamed() {
        // Kick is gone; Clap has a hits lane with no data yet.
        let dir = project(&[("song.ts", &tracks(&["Clap"])), ("timeline/kick.hits.json", NOTES)]);
        assert_eq!(relinks(&compiled(&dir)[0]), [("Clap", "hits", true)], "just opened, Kick may have been renamed to Clap");
        // Seen without Clap: Kick may have been renamed to it.
        assert_eq!(relinks(&seeing(&dir, &seen(&[("kick", "hits", "events<note>")]))[0]), [("Clap", "hits", true)]);
        // Seen beside Clap: Kick was deleted, and Clap is another track.
        let w = seeing(&dir, &seen(&[("kick", "hits", "events<note>"), ("clap", "hits", "events<note>")]));
        assert_eq!(relinks(&w[0]), [("Clap", "hits", false)]);
        assert!(w[0].message.ends_with("Move them to the lane they belong to now (Clap › hits), or delete them."), "{}", w[0].message);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn automation_goes_only_to_lanes_whose_unit_can_hold_it() {
        let cutoff = r#"{"clips": [{"start": 0, "length": 16, "points": [[0, 300], [16, 2200]]}]}"#;
        let dir = project(&[
            ("song.ts", &song("Kick", "lane.mask(\"drop\"); lane.control(\"pitch\", \"semitones\", 0);")),
            ("timeline/kick.cutoff.json", cutoff),
        ]);
        // Not known as hertz in a project just opened, but no 0..1 lane holds 2200.
        let w = compiled(&dir);
        assert_eq!(relinks(&w[0]), [("Kick", "pitch", false)]);
        // Known as hertz: not semitones either.
        let w = seeing(&dir, &seen(&[("kick", "cutoff", "control<hz>")]));
        assert_eq!(relinks(&w[0]), []);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_file_named_with_other_capitals_is_offered_to_its_own_lane() {
        let dir = project(&[("song.ts", &song("Kick", "")), ("timeline/Kick.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(w.len(), 1);
        assert!(matches!(&w[0].fixes[0], Fix::Relink { to, likely: true, .. } if to == "timeline/kick.hits.json"), "{:?}", w[0].fixes);
        assert_eq!(
            w[0].to_string(),
            "Notes on Kick › hits aren't playing. The file is named timeline/Kick.hits.json, but Kick › hits reads \
             timeline/kick.hits.json, and capitals count. Move them to Kick › hits to hear them again, or delete them."
        );
        std::fs::remove_dir_all(dir).ok();
    }

    /// On a disk that ignores capitals, `Kicks.hits.json` is `kicks.hits.json`: no other data
    /// can be moved there.
    #[test]
    fn a_lane_whose_name_another_file_has_in_other_capitals_takes_only_that_file() {
        let dir = project(&[("song.ts", &song("Kicks", "")), ("timeline/kick.hits.json", NOTES), ("timeline/Kicks.hits.json", NOTES)]);
        let w = compiled(&dir);
        assert_eq!(relinks(about(&w, "timeline/Kicks.hits.json")), [("Kicks", "hits", true)]);
        let kick = about(&w, "timeline/kick.hits.json");
        assert_eq!(relinks(kick), []);
        assert!(kick.message.ends_with("To hear them again, add a track called kick with a lane called hits, or delete them."), "{kick}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn data_no_lane_can_take_says_so() {
        let odd = r#"{"clips": [{"start": 0}]}"#;
        let dir = project(&[("song.ts", &song("Kick", "")), ("timeline/kick.hits.json", NOTES), ("timeline/snare.hits.json", NOTES)]);
        std::fs::write(dir.join("timeline/kick.odd.json"), odd).unwrap();
        std::fs::write(dir.join("timeline/kick.old.json"), r#"{"clips": []}"#).unwrap();
        let w = compiled(&dir);
        let snare = about(&w, "timeline/snare.hits.json");
        assert_eq!(snare.fixes, [Fix::DeleteData { file: "timeline/snare.hits.json".into() }], "Kick › hits has its own notes");
        assert_eq!(
            snare.to_string(),
            "Notes on snare › hits aren't playing. There's no track called snare any more. To hear them again, add a track called \
             snare with a lane called hits, or delete them."
        );
        let odd = about(&w, "timeline/kick.odd.json");
        assert_eq!(
            odd.to_string(),
            "timeline/kick.odd.json isn't used. Kick has no lane called odd any more. It doesn't hold notes, automation or audio clips \
             that a lane can read: fix it by hand, or delete it."
        );
        let old = about(&w, "timeline/kick.old.json");
        assert_eq!(
            old.to_string(),
            "An empty lane file on Kick › old isn't used. Kick has no lane called old any more. To use it again, add a lane called old \
             to Kick, or delete it."
        );
        std::fs::remove_dir_all(dir).ok();
    }
}

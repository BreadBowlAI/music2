//! The step 4 gates, as a running session sees them: keys and labels, data that doesn't read,
//! the manifest's format, imports, presentation files and assets.

use std::path::{Path, PathBuf};

use m2_ir::{FNode, FOp, Flat};
use m2_runtime::{PackageSet, Renderer};
use m2_song::tx::{Anchor, Base, Edit, Expect, Op, Pos, Range, Tx, TxPath};
use m2_song::{LaneData, Project, Snapshot};

use crate::{Session, compile_sources};

pub(crate) fn std_packages() -> PackageSet {
    let mut p = PackageSet::default().with_store(m2_song::pack::test_store());
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

pub(crate) struct Fixture(pub(crate) PathBuf);

impl Fixture {
    pub(crate) fn new() -> Fixture {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Fixture(std::env::temp_dir().join(format!("m2-gate-test-{}-{n}", std::process::id())))
    }

    /// A copy of the stable first-song fixture.
    fn first_song() -> Fixture {
        fn copy(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                let (f, t) = (e.path(), to.join(e.file_name()));
                if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
            }
        }
        let f = Fixture::new();
        copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-song"), &f.0);
        f
    }

    pub(crate) fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap()
    }

    pub(crate) fn write(&self, name: &str, text: impl AsRef<[u8]>) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub(crate) fn open(&self) -> Session {
        Session::open(&self.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A copy of `every-component`: a song that calls each std component once, with some controls
/// written, some connected and some left out.
pub(crate) fn every_component() -> Fixture {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/every-component");
    let f = Fixture::new();
    for file in files_under(&from) {
        f.write(file.strip_prefix(&from).unwrap().to_str().unwrap(), std::fs::read(&file).unwrap());
    }
    f
}

/// A `Values` Tx writing `new` over the first `old` after `after` in `file`, as the knob of that
/// literal commits it (the editor's builder, WRITE-10).
pub(crate) fn literal(s: &Session, file: &str, after: &str, old: &str, new: f64) -> Tx {
    let text = s.accepted().snapshot.files[Path::new(file)].clone();
    let at = text.find(after).unwrap() + text[text.find(after).unwrap()..].find(old).unwrap();
    let path = TxPath::new(file).unwrap();
    let (start, end) = (at as u32, (at + old.len()) as u32);
    let edit = Edit { start: Pos::Byte(start), end: Pos::Byte(end), expected: old.into(), text: new.to_string() };
    let anchor = Anchor::Written { file: path.clone(), range: Range::bytes(start, end) };
    let mut tx = Tx::new(
        "tx-knob",
        (old.into(), new.to_string()),
        vec![Op::TextEdit { file: path.clone(), edits: vec![edit] }],
        Expect::Values(vec![(anchor, new)]),
    );
    tx.base.insert(path, Base::text(&text));
    tx
}

/// Two seconds of the accepted revision, offline.
fn render(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap().render(2.0).unwrap()
}

/// `flat` without source positions, which a longer label on a line shifts.
fn placeless(flat: &Flat) -> Flat {
    fn strip(nodes: &mut [FNode]) {
        for n in nodes {
            n.src = None;
            if let FOp::Poly { voice, .. } = &mut n.op {
                strip(&mut voice.nodes);
            }
        }
    }
    let mut flat = flat.clone();
    strip(&mut flat.nodes);
    flat
}

/// Every data file of the project, by path.
fn data(f: &Fixture) -> Vec<(PathBuf, String)> {
    let snap = Snapshot::read(&f.0).unwrap();
    snap.files.into_iter().filter(|(p, _)| p.starts_with("data")).map(|(p, t)| (p, t.to_string())).collect()
}

// ID-10, ID-17: a label rename changes nothing but the label: no data file, no graph, no audio.
// The label reaches only what shows it.
#[test]
fn changing_a_tracks_label_changes_nothing_but_the_label() {
    let f = Fixture::first_song();
    let mut s = f.open();
    let (sound, graph, files) = (render(&s), placeless(&s.accepted().compiled.flat), data(&f));
    let kick = f.read("tracks/kick.ts");
    f.write("tracks/kick.ts", kick.replacen("{ name: \"Kick\" }", "{ name: \"Big Kick\" }", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert_eq!(placeless(&s.accepted().compiled.flat), graph, "labels are ignored by the graph");
    assert!(render(&s) == sound, "it sounds the same");
    assert_eq!(data(&f), files);
    assert!(s.problems().is_empty(), "{:?}", s.problems());
    // What the timeline and the canvas show it by (`crates/editor` reads it there).
    let module = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "kick").unwrap();
    assert_eq!(module.label(), "Big Kick");
}

// The step 4 gate, ID-17, DIAG-6: a track key renamed by hand silences the track until its
// folder's Relink, which restores exactly the sound it had.
#[test]
fn relinking_a_renamed_tracks_folder_restores_its_sound() {
    let f = Fixture::first_song();
    let mut s = f.open();
    let sound = render(&s);
    f.write("tracks/kick.ts", f.read("tracks/kick.ts").replacen("track(\"kick\"", "track(\"kicks\"", 1));
    assert!(s.refresh() && s.draft_errors.is_empty());
    assert!(render(&s) != sound, "the kick is silent");
    let fix = s.problems()[0].fixes[0].clone();
    assert_eq!((s.problems()[0].src.as_deref(), fix.code), (Some("data/kick"), "relink"));
    // DIAG-1: applying a fix is committing its Tx.
    s.commit(fix.tx).unwrap();
    assert!(render(&s) == sound, "the same bits as before the rename");
}

// The step 4 gate, ID-17, DIAG-6, for a lane key: renamed by hand, it leaves the lane's file
// unclaimed and the lane silent, until the file's Relink, which moves it byte for byte and
// restores exactly the sound it had.
#[test]
fn relinking_a_renamed_lanes_file_restores_its_sound() {
    let f = Fixture::first_song();
    let mut s = f.open();
    let (sound, hits) = (render(&s), f.read("data/kick/hits.json"));
    f.write("tracks/kick.ts", f.read("tracks/kick.ts").replacen("lane.notes(\"hits\")", "lane.notes(\"beats\")", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    assert!(render(&s) != sound, "the kick is silent");
    let p = &s.problems()[0];
    assert_eq!((p.code, p.src.as_deref()), ("data.unclaimed", Some("data/kick/hits.json")));
    let fix = p.fixes[0].clone();
    assert!(fix.code == "relink" && fix.likely, "{fix:?}");
    s.commit(fix.tx).unwrap();
    assert!(render(&s) == sound, "the same bits as before the rename");
    assert_eq!(f.read("data/kick/beats.json"), hits, "moved as it was");
    assert!(!f.0.join("data/kick/hits.json").exists() && s.problems().is_empty(), "{:?}", s.problems());
}

// FMT-9: a data file that can't be read (mode 000, or a dangling link) never stops anything
// else. In a running session code and data changes are still taken in and edits still made;
// the lane that reads it plays the data it had, and on open, nothing.
#[cfg(unix)]
#[test]
fn a_data_file_that_cant_be_read_stops_nothing_else() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::first_song();
    let mode = |p: &str, m: u32| std::fs::set_permissions(f.0.join(p), std::fs::Permissions::from_mode(m)).unwrap();
    let mut s = f.open();
    let sound = render(&s);
    f.write("data/kick/other.json", "{\"clips\": []}\n");
    mode("data/kick/other.json", 0o000);
    std::fs::rename(f.0.join("data/kick/hits.json"), f.0.join("hits.json")).unwrap();
    std::os::unix::fs::symlink(f.0.join("nowhere.json"), f.0.join("data/kick/hits.json")).unwrap();
    assert!(s.refresh() && s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let found: Vec<_> = s.problems().iter().map(|d| (d.code, d.src.as_deref().unwrap())).collect();
    assert_eq!(found, [("lane.invalid", "data/kick/hits.json"), ("lane.invalid", "data/kick/other.json")]);
    assert!(render(&s) == sound, "the kick plays the notes it had");
    // A code change is taken in, and an edit made, while the files still can't be read.
    let song = f.read("song.ts");
    f.write("song.ts", format!("// Edited elsewhere.\n{song}"));
    assert!(s.refresh() && s.draft_errors.is_empty() && s.accepted().number == 3, "{:?}", s.draft_errors);
    s.commit(literal(&s, "tracks/bass.ts", "period: ", "0.5", 0.25)).unwrap();
    // On open, only that lane is silent. The lock is one session's at a time (WRITE-17).
    drop(s);
    let opened = f.open();
    assert!(opened.problems()[0].message.contains("plays nothing"), "{}", opened.problems()[0].message);
    mode("data/kick/other.json", 0o644);
}

// FMT-8, FMT-9: a lane file with an unknown field plays its last accepted data in a running
// session, and silences only that lane on open, with an Error naming the file and the field.
#[test]
fn a_lane_file_with_an_unknown_field_keeps_its_data_or_silences_only_its_lane() {
    let f = Fixture::first_song();
    let mut s = f.open();
    let sound = render(&s);
    let notes = f.read("data/bass/notes.json");
    f.write("data/bass/notes.json", notes.replacen("{\"clips\"", "{\"colour\": \"red\", \"clips\"", 1));
    assert!(s.refresh() && s.draft_errors.is_empty(), "a lane never blocks: {:?}", s.draft_errors);
    let p = &s.problems()[0];
    assert_eq!((p.severity, p.code, p.src.as_deref()), (m2_song::Severity::Error, "lane.invalid", Some("data/bass/notes.json")));
    assert!(p.message.contains("unknown field `colour`") && p.message.contains("plays the data it had"), "{}", p.message);
    assert!(render(&s) == sound, "the bass plays the notes it had");

    drop(s);
    let s = f.open();
    assert!(s.problems()[0].message.contains("plays nothing"), "{}", s.problems()[0].message);
    // What each lane plays, as the timeline shows it.
    let clips = |module: &str, lane: &str| match s.accepted().compiled.data.lane(module, lane).map(|d| &**d) {
        Some(LaneData::Notes(clips)) => clips.len(),
        None => 0,
        _ => panic!("no notes lane {module} › {lane}"),
    };
    assert_eq!((clips("bass", "notes"), clips("kick", "hits") > 0, clips("keys", "chords") > 0), (0, true, true));
}

// FMT-3: `format` is read first and leniently: a newer format with an unknown key is
// `format.newer`, never `manifest.invalid`, and the project doesn't open.
#[test]
fn a_newer_format_does_not_open_whatever_else_the_manifest_says() {
    let f = Fixture::first_song();
    let manifest = f.read("music2.toml").replace("format = 1", "format = 2\ncolour = \"red\"");
    f.write("music2.toml", manifest);
    let e = Session::open(&f.0, std_packages()).err().unwrap();
    assert_eq!(e.iter().map(|d| d.code).collect::<Vec<_>>(), ["format.newer"]);
    assert!(e[0].message.contains("newer music2"), "{}", e[0].message);
}

// FMT-6, FMT-6a, PLANE-3: the build imports only the snapshot's code, byte for byte: an import
// of a `.js` file or one spelled with other capitals fails on every disk, and changing a `.js`
// file never makes a revision.
#[test]
fn imports_resolve_only_to_the_snapshots_code() {
    let f = Fixture::first_song();
    let mut s = f.open();
    let song = f.read("song.ts");
    f.write("lib/helper.js", "export const x = 1;\n");
    assert!(!s.refresh(), "a .js file is no part of a revision");
    f.write("lib/helper.js", "export const x = 2;\n");
    assert!(!s.refresh());
    for import in ["import { x } from \"./lib/helper.js\";\n", "import { x } from \"./lib/helper\";\n"] {
        f.write("song.ts", format!("{import}const y = x;\n{song}"));
        assert!(s.refresh());
        assert_eq!(s.draft_errors.iter().map(|d| d.code).collect::<Vec<_>>(), ["import.outside-code"], "{import}");
    }
    f.write("song.ts", song.replacen("./tracks/kick", "./Tracks/kick", 1));
    assert!(s.refresh());
    assert_eq!(s.draft_errors.iter().map(|d| d.code).collect::<Vec<_>>(), ["import.outside-code"]);
}

// PLANE-3, FMT-19, FMT-20, FMT-23: presentation, local files and the files for agents never make a
// revision.
#[test]
fn editing_layout_json_creates_no_revision() {
    let f = Fixture::first_song();
    let mut s = f.open();
    for (file, text) in [
        ("layout.json", "{\"tracks\": {\"kick\": {\"colour\": \"#e4572e\"}}}"),
        ("layout.json", "{\"tracks\": {}}"),
        (".music2/overlays.json", "{}"),
        ("README.md", "notes"),
        // FMT-23: the song's files for agents.
        ("AGENTS.md", "# Working on a music2 song\n"),
        ("CLAUDE.md", "@AGENTS.md\n"),
    ] {
        f.write(file, text);
        assert!(!s.refresh(), "{file}");
        assert_eq!(s.accepted().number, 1, "{file}");
    }
}

/// Every file under `dir`, recursively.
pub(crate) fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) { out.extend(files_under(&e.path())) } else { out.push(e.path()) }
    }
    out
}

/// A pure fixture's whole song and a second past it, as `s` plays it.
fn render_song(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    let data = &rev.compiled.data;
    let seconds = data.tempo().seconds_at(data.end()) + 1.0;
    Renderer::new(&rev.compiled.flat, data, &s.packages, rev.project.sample_rate()).unwrap().render(seconds).unwrap()
}

/// The fixtures made only of kernels, which render the same bits every time (RENDER-2). The
/// other one, examples/with-plugins, hosts plugins, which need not. `sampler-regions` is std's
/// Sampler playing regions (PKG-16): two velocity layers, two takes in turn, a release and a loop.
const PURE: [&str; 7] = [
    "examples/first-song",
    "examples/resample",
    "examples/sounds",
    "examples/sampler",
    "templates/starter",
    "templates/empty",
    "core/session/tests/fixtures/sampler-regions",
];

/// A copy of the repo's fixture `rel`, such as `examples/first-song`.
pub(crate) fn fixture(rel: &str) -> Fixture {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    let f = Fixture::new();
    for file in files_under(&from) {
        let to = f.0.join(file.strip_prefix(&from).unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(&file, to).unwrap();
    }
    f
}

/// The SHA-256 of a render's samples, as little-endian `f32` bits.
fn bits(frames: &[[f32; 2]]) -> String {
    let bytes: Vec<u8> = frames.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    m2_song::Sha::of(&bytes).hex()
}

// The step 4 gate, FMT-8a: permuting any array in a fixture renders the same bits, since readers
// put clips, notes and points in one order before use. Points are reversed only because no
// fixture has two at one beat: those are a step, whose order is the one thing a file keeps.
#[test]
fn permuting_any_array_in_a_fixture_renders_the_same_bits() {
    // Reverses the lists of things, never a point's `[beat, value]` pair.
    fn permute(v: &mut serde_json::Value) {
        const LISTS: [&str; 6] = ["clips", "notes", "points", "warp", "tempo", "meter"];
        match v {
            serde_json::Value::Object(o) => {
                for (k, v) in o.iter_mut() {
                    if let serde_json::Value::Array(a) = v
                        && LISTS.contains(&k.as_str())
                    {
                        a.reverse();
                    }
                    permute(v);
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(permute),
            _ => {}
        }
    }
    for name in PURE {
        let f = fixture(name);
        let sound = render_song(&f.open());
        let mut changed = 0;
        for file in files_under(&f.0.join("data")).into_iter().filter(|p| p.extension().is_some_and(|e| e == "json")) {
            let text = std::fs::read_to_string(&file).unwrap();
            let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
            permute(&mut v);
            let permuted = serde_json::to_string_pretty(&v).unwrap();
            changed += usize::from(v != serde_json::from_str::<serde_json::Value>(&text).unwrap());
            std::fs::write(&file, permuted).unwrap();
        }
        let s = f.open();
        assert!(s.problems().is_empty(), "{name}: {:?}", s.problems());
        assert!(name == "templates/empty" || changed > 0, "{name}: nothing was permuted");
        assert!(render_song(&s) == sound, "{name}: permuting its arrays changed the sound");
    }
}

// FMT-3, RENDER-2: a std release that changes the bits of an existing render must bump std's
// version, so an old project gets `pkg.unsatisfied` rather than another sound. `renders.json`
// records each pure fixture's render under the std version that made it. After a version bump,
// record the new renders: the failure prints them.
#[test]
fn fixture_renders_change_only_with_stds_version() {
    let recorded: serde_json::Value = serde_json::from_str(include_str!("../tests/renders.json")).unwrap();
    let version = std_packages().get("std").unwrap().version.clone();
    let render = |n: &str| {
        // A fixture that doesn't build (a manifest left behind at a bump) would render nothing. The
        // copy stays until the render is done: the render reads its audio files.
        let f = fixture(n);
        let s = f.open();
        assert!(s.draft_errors.is_empty(), "{n} doesn't build with std {version}: {:?}", s.draft_errors);
        bits(&render_song(&s))
    };
    let renders: serde_json::Map<String, serde_json::Value> = PURE.iter().map(|n| (n.to_string(), render(n).into())).collect();
    let now = serde_json::json!({"std": version, "renders": renders});
    let record = format!("core/session/tests/renders.json:\n{}", serde_json::to_string_pretty(&now).unwrap());
    assert!(
        recorded["std"] == now["std"],
        "the renders were recorded for std {}, and std is {version}: record them in {record}",
        recorded["std"]
    );
    assert!(
        recorded == now,
        "a pure fixture renders other bits while std is still {version}. A std change must bump std's version (FMT-3), \
         and nothing else may change an existing render (RENDER-2). Once it is meant, record the renders in {record}"
    );
}

/// `tests/fixtures/mono-k127` as std's `mono()` played it, from two `std:notecv` polys into
/// `std:mono` (std 0.2.1, `1bc5ac8`): recorded before std's one-note sounds moved onto the `poly`
/// op's options (RENDER-16).
const MONO_K127: &str = "c48c79a632d6833ce006de5f3b50abcff0102d11aaa25e776448f0ae75133cfd";

// RENDER-16, FMT-3: std's one-note sounds (its lead, 808 sub and bass, FM keys and the whole synth
// given `voices: 1`) play on one `poly` in `"legato"` mode, not through `std:mono`. Where every
// velocity is one a keyboard sends (k/127, which `std:mono` rounded every velocity to), they
// render the same bits as before: overlaps slide, touches start again, a trill glides over a held
// note, and a note let go brings back the one still held.
#[test]
fn mono_sounds_keep_their_bits_on_keyboard_velocities() {
    let f = Fixture::new();
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mono-k127");
    for file in files_under(&from) {
        f.write(file.strip_prefix(&from).unwrap().to_str().unwrap(), std::fs::read(&file).unwrap());
    }
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    let sound = render_song(&s);
    assert!(peak(&sound) > 0.05, "the song plays");
    assert_eq!(bits(&sound), MONO_K127, "std's one-note sounds changed their bits on keyboard velocities");
}

/// A stereo saw and a mono square through `@m2/effects`' bitcrusher, as std's `std:bitcrush` played
/// them (std 0.7.0, `@m2/effects` 1.0.0, `3be7bc3`): recorded before the kernel moved into its pack
/// (PACK-21).
const BITCRUSH: &str = "817bb3d128ec6d90df492758bea12b0c813701b71ec42e4cd8cc7e1b396fe126";

// PACK-21, ABI-10: the bitcrusher plays from `@m2/effects`' own module, by the pack's id, and std
// has it no more: the same bits as when it was std's, mono and stereo, held at two rates.
#[test]
fn bitcrush_plays_from_its_pack_with_the_bits_it_had_in_std() {
    let f = Fixture::new();
    let std = m2_song::Version::parse(&std_packages().get("std").unwrap().version).unwrap();
    f.write(
        "music2.toml",
        format!(
            "[project]\nname = \"crush\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"{}.{}\"\n\
             \"@m2/effects\" = \"1\"\n",
            std.0, std.1
        ),
    );
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { osc, pan } from \"std/dsp\";\nimport { sum } from \"std/math\";\n\
         import { bitcrush } from \"@m2/effects\";\n\n\
         export default song(() =>\n  sum(\n    bitcrush(pan(osc.saw(110), -0.4), { bits: 5, rate: 7000, mix: 0.8 }),\n    \
         pan(bitcrush(osc.square(165), { bits: 3.5, rate: 2500 }), 0.5),\n  ),\n);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    let sound = render_song(&s);
    assert!(peak(&sound) > 0.3, "the song plays");
    assert_eq!(bits(&sound), BITCRUSH, "the bitcrusher plays other bits from its pack than it did from std");
    let kernels: Vec<&str> = s
        .accepted()
        .compiled
        .flat
        .nodes
        .iter()
        .filter_map(|n| match &n.op {
            FOp::Kernel { kernel, .. } if kernel.ends_with(":bitcrush") => Some(kernel.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(kernels, ["@m2/effects:bitcrush"; 2], "the bitcrusher is the pack's own");
}

/// The engines that left std for their packs in wave 3, as std played them (std 0.8.0, `@m2/synths`
/// 1.0.0, `@m2/drums` 1.0.0, `@m2/effects` 1.1.0, `8a914b1`): the polysynth and the FM synth on one
/// lane, the drum machine on another, and delay, reverb, phaser, tremolo and widen over them.
/// Recorded before the kernels moved (PACK-21).
const ENGINES: &str = "d09c156022b2db81ce7707cb2ff4268637cc2ad5d8a29c2298706f77c0cb9956";

/// Every kernel id the graph `nodes` names, its voices' included.
fn kernel_ids(nodes: &[FNode], into: &mut Vec<String>) {
    for n in nodes {
        match &n.op {
            FOp::Kernel { kernel, .. } => into.push(kernel.clone()),
            FOp::Poly { voice, .. } => kernel_ids(&voice.nodes, into),
            _ => {}
        }
    }
}

// PACK-21, ABI-10: the polysynth's and the FM synth's voices play from `@m2/synths`' own module,
// the drum machine's from `@m2/drums`', and delay, reverb, phaser, tremolo and widen from `@m2/effects`',
// each by its pack's id, and std has none of them: the bits they had as std's.
#[test]
fn engines_play_from_their_packs_with_the_bits_they_had_in_std() {
    let f = Fixture::new();
    let std = m2_song::Version::parse(&std_packages().get("std").unwrap().version).unwrap();
    f.write(
        "music2.toml",
        format!(
            "[project]\nname = \"engines\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"{}.{}\"\n\
             \"@m2/drums\" = \"2\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n",
            std.0, std.1
        ),
    );
    let note =
        |pitch: &str, start: f64, length: f64| serde_json::json!({"pitch": pitch, "start": start, "length": length, "velocity": 0.8});
    let lane =
        |notes: Vec<serde_json::Value>| serde_json::json!({"clips": [{"id": "c1", "start": 0, "length": 4, "notes": notes}]}).to_string();
    f.write("data/keys/notes.json", lane(vec![note("C4", 0.0, 1.5), note("E4", 0.0, 1.5), note("G3", 2.0, 1.0), note("A4", 2.5, 1.0)]));
    let beat = ["C2", "F#2", "D2", "F#2", "C2", "A#2", "D#2", "F2"];
    f.write("data/beat/notes.json", lane(beat.iter().enumerate().map(|(i, p)| note(p, i as f64 * 0.5, 0.25)).collect()));
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\nimport { osc, pan } from \"std/dsp\";\nimport { sum } from \"std/math\";\n\
         import { synth, fm } from \"@m2/synths\";\nimport { drums } from \"@m2/drums\";\n\
         import { delay, phaser, reverb, tremolo, widen } from \"@m2/effects\";\n\n\
         const keys = track(\"keys\", ({ lane }) =>\n  sum(synth.custom(lane.notes(\"notes\"), { square: 0.3, unison: 3, filterEnv: 2 }), fm.custom(lane.notes(\"notes\"))),\n);\n\
         const beat = track(\"beat\", ({ lane }) => drums.kit(lane.notes(\"notes\")));\n\n\
         export default song(() =>\n  sum(\n    reverb(delay(keys.out, { time: 0.3, feedback: 0.4 }), { mix: 0.4 }),\n    widen(phaser(beat.out)),\n    \
         tremolo(pan(osc.saw(110), -0.3), { depth: 0.8 }),\n  ),\n);\n",
    );
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{:?} {:?}", s.draft_errors, s.problems());
    let sound = render_song(&s);
    assert!(peak(&sound) > 0.3, "the song plays");
    assert_eq!(bits(&sound), ENGINES, "the engines play other bits from their packs than they did from std");
    let mut kernels = vec![];
    kernel_ids(&s.accepted().compiled.flat.nodes, &mut kernels);
    kernels.retain(|k| ["synth", "fm", "drum", "delay", "reverb", "phaser", "tremolo", "widen"].contains(&k.split_once(':').unwrap().1));
    kernels.sort();
    kernels.dedup();
    assert_eq!(
        kernels,
        [
            "@m2/drums:drum",
            "@m2/effects:delay",
            "@m2/effects:phaser",
            "@m2/effects:reverb",
            "@m2/effects:tremolo",
            "@m2/effects:widen",
            "@m2/synths:fm",
            "@m2/synths:synth"
        ],
        "each engine is its pack's own"
    );
}

/// What each of `@m2/drums`' nine single drums played (`drums.kick(lane.notes("notes"))` …) of
/// [`one_drum_song`]'s beat, as std's `std:drum` played them (std 0.8.0, `@m2/drums` 1.0.0,
/// `8a914b1`; the move into the pack changed no bit): recorded before the Drum took their place.
const SINGLES: [(&str, &str); 9] = [
    ("kick", "7767ad38a756c3203d0814167a2d32f95d493741baf83936a7c197ab83ec8b31"),
    ("snare", "67d0f6b40f595767b3fcbcb4edbe3bf09b7f458c43ac39ebf1e5a92445f4a69b"),
    ("clap", "6436617e03b0b01a1908ee68f61e6a1b5b1e12c9e4d7f92f72e2c2854b3acfce"),
    ("hats", "4f5a0555ef5887cace130d9bb3f45e6e30f86e8e7f66dd4f53e96b943a88f474"),
    ("tom", "4d11e75353f1aa2e3a6508e5d24126d6dfc96463125a657132000655f9f35a0e"),
    ("rim", "67627b850030b3d510db5ea84f9ac2f58e87648391926b47f2cb1ddc3c25e2fe"),
    ("cowbell", "4040bd8781d3f22787142d8e03ef5dc8d0fb3a5644615c11ef4e73c368f46df9"),
    ("cymbal", "a523877b8f5157e958b97efbe1f26b93cf23339f5cf8895b158f84fe0b500a96"),
    ("shaker", "a3738e16376c19dc610801a90c2e28ce3b9f23258ceaddeba64739af2b790582"),
];

/// A song whose one track, `beat`, plays `call` on a beat of five hits of several notes and
/// strengths, asking for the `@m2/drums` and `@m2/synths` this tree bundles.
fn one_drum_song(f: &Fixture, call: &str) {
    let note =
        |pitch: &str, start: f64, velocity: f64| serde_json::json!({"pitch": pitch, "start": start, "length": 0.25, "velocity": velocity});
    let notes = vec![note("C2", 0.0, 0.8), note("D2", 0.5, 0.5), note("F#2", 1.0, 1.0), note("A#2", 1.5, 0.7), note("A2", 2.0, 0.9)];
    notes_song(f, call, notes);
}

/// A song of one track whose notes lane `notes` plays `call` (which may use `drums` and `synth`).
fn notes_song(f: &Fixture, call: &str, notes: Vec<serde_json::Value>) {
    let std = m2_song::Version::parse(&std_packages().get("std").unwrap().version).unwrap();
    let major = |pack: &str| m2_song::pack::PkgManifest::at(&m2_song::pack::source_packs().join(pack)).unwrap().package.version.0;
    f.write(
        "music2.toml",
        format!(
            "[project]\nname = \"drum\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"{}.{}\"\n\
             \"@m2/drums\" = \"{}\"\n\"@m2/synths\" = \"{}\"\n",
            std.0,
            std.1,
            major("@m2/drums"),
            major("@m2/synths")
        ),
    );
    f.write("data/beat/notes.json", serde_json::json!({"clips": [{"id": "c1", "start": 0, "length": 4, "notes": notes}]}).to_string());
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\nimport {{ synth }} from \"@m2/synths\";\n\n\
             const beat = track(\"beat\", ({{ lane }}) => {call});\n\nexport default song(() => beat.out);\n"
        ),
    );
}

// CARD-33: the Drum plays each drum its menu offers as that drum's single instrument played it,
// bit for bit, its knobs at their defaults; and its knobs move the drum it plays.
#[test]
fn the_drum_plays_each_drum_as_its_single_did() {
    for (drum, single) in SINGLES {
        let f = Fixture::new();
        one_drum_song(&f, &format!("drums.drum(lane.notes(\"notes\"), {{ drum: \"{drum}\" }})"));
        let s = f.open();
        assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{drum}: {:?} {:?}", s.draft_errors, s.problems());
        assert_eq!(bits(&render_song(&s)), single, "the Drum's {drum} plays other bits than drums.{drum} did");
    }
    // Tuned up, shorter and harder, a kick is another sound.
    let f = Fixture::new();
    one_drum_song(&f, "drums.drum(lane.notes(\"notes\"), { tune: 5, decay: 0.5, punch: 0.4 })");
    let s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let moved = render_song(&s);
    assert!(peak(&moved) > 0.05 && bits(&moved) != SINGLES[0].1, "the knobs move the kick");
}

// CARD-32: the polysynth's wave shape is a menu, written by its name, which reaches the synth's
// kernel as its place; another shape plays another sound, and the saw, its default, is the synth
// as it always was (`engines_play_from_their_packs_with_the_bits_they_had_in_std` holds its bits).
#[test]
fn a_synths_wave_shape_reaches_its_kernel_as_its_place() {
    let mut heard = vec![];
    for (shape, place) in [("saw", 0.0), ("square", 1.0), ("triangle", 2.0), ("sine", 3.0)] {
        let f = Fixture::new();
        one_drum_song(&f, &format!("synth.custom(lane.notes(\"notes\"), {{ shape: \"{shape}\" }})"));
        let s = f.open();
        assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{shape}: {:?} {:?}", s.draft_errors, s.problems());
        let rev = s.accepted();
        let flat = &rev.compiled.flat;
        // The synth's kernel in its voice, and what its `shape` (input 31) reads: a constant, in the
        // voice or captured from outside it.
        let (voice, synth) = flat
            .nodes
            .iter()
            .find_map(|n| match &n.op {
                FOp::Poly { voice, .. } => voice
                    .nodes
                    .iter()
                    .find(|v| matches!(&v.op, FOp::Kernel { kernel, .. } if kernel == "@m2/synths:synth"))
                    .map(|k| (voice, k)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{shape}: no synth voice"));
        let FOp::Kernel { inputs, .. } = &synth.op else { unreachable!() };
        let value = match &inputs[31] {
            m2_ir::Input::Const(v) => *v,
            m2_ir::Input::Port { src, .. } => match &voice.nodes[src.node].op {
                FOp::Const(v) => *v,
                FOp::Capture(from) => match &flat.nodes[from.node].op {
                    FOp::Const(v) => *v,
                    other => panic!("{shape}: the shape comes from {other:?}"),
                },
                other => panic!("{shape}: the shape comes from {other:?}"),
            },
            other => panic!("{shape}: {other:?}"),
        };
        assert_eq!(value, place, "{shape} reaches the kernel as its place");
        let sound = render_song(&s);
        assert!(peak(&sound) > 0.05, "{shape} plays");
        heard.push(bits(&sound));
    }
    heard.sort();
    heard.dedup();
    assert_eq!(heard.len(), 4, "each shape plays a sound of its own");
}

/// The RMS of both channels of `x`, in dB.
fn rms_db(x: &[[f32; 2]]) -> f64 {
    let p = x.iter().map(|f| (f[0] as f64).powi(2) + (f[1] as f64).powi(2)).sum::<f64>() / (2 * x.len().max(1)) as f64;
    10.0 * p.max(1e-30).log10()
}

// CARD-32 (review Y2): a pick of shape never jumps the loudness. Every sound of the polysynth's
// with a shape, held on one note, is within 1.5 dB of its saw at every shape; and a four-note
// chord of it stays under full scale at every shape, so an export never clips.
#[test]
fn every_synth_sound_keeps_its_loudness_at_every_shape() {
    let note = |pitch: &str, velocity: f64| serde_json::json!({"pitch": pitch, "start": 0.0, "length": 4.0, "velocity": velocity});
    let render = |sound: &str, shape: &str, notes: Vec<serde_json::Value>| {
        let f = Fixture::new();
        notes_song(&f, &format!("synth.{sound}(lane.notes(\"notes\"), {{ shape: \"{shape}\" }})"), notes);
        let s = f.open();
        assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{sound} {shape}: {:?} {:?}", s.draft_errors, s.problems());
        render_song(&s)
    };
    let mut said = vec![];
    for sound in ["custom", "supersaw", "lead", "pad", "pluck", "bass", "strings"] {
        let held = |shape: &str| {
            let x = render(sound, shape, vec![note(if sound == "bass" { "A1" } else { "A3" }, 0.8)]);
            rms_db(&x[..(2.0 * 48_000.0) as usize])
        };
        let saw = held("saw");
        for shape in ["square", "triangle", "sine"] {
            let d = held(shape) - saw;
            if d.abs() > 1.5 {
                said.push(format!("{sound} at {shape}: {d:+.1} dB from its saw"));
            }
        }
        for shape in ["saw", "square", "triangle", "sine"] {
            let chord = ["C4", "E4", "G4", "B4"].map(|p| note(p, 0.8)).to_vec();
            let top = peak(&render(sound, shape, chord));
            if top >= 1.0 {
                said.push(format!("{sound}'s chord at {shape} peaks at {:+.1} dBFS", 20.0 * top.log10()));
            }
        }
    }
    assert!(said.is_empty(), "{}", said.join("\n"));
}

fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

/// std's mixer and master before every channel and the master had a gain (8b.3), as a project's
/// own file: a channel's `db` optional, no master `db`, nothing to make the master stereo. Buses
/// came later (RENDER-34): a channel that goes to one (`to`) feeds it as today's mixer does, so a
/// song with a kit's bus (KIT-22: the sounds example) compares too; it has no sends.
const OLD_MIX: &str = r#"import { busOf, component, control, each, feed, kernel, port, setting, signature, type Audio, type Params, type Signal, type Value } from "@m2/core";
import { gain, pan } from "std/dsp";
import { limit } from "std/fx";
import { sum } from "std/math";

export type Channel = Audio | { in: Audio; db?: Value; pan?: Value; to?: string };

export const mixer = component(
  {
    id: "mixer",
    label: "Mixer",
    signature: signature<(channels: Record<string, Channel>) => Audio>(),
    controls: {
      channels: each({
        in: port.audio(),
        db: control.db(0).range(-60, 12).optional(),
        pan: control.bipolar(0).range(-1, 1).optional().smooth(),
        to: setting.key().optional(),
      }),
    },
    output: port.audio(),
    positional: ["channels"],
  },
  function mixer({ channels }: Params): Audio {
    const chans = Object.values(channels as Record<string, { in: Audio; db?: Signal; pan?: Signal; to?: string }>);
    const keys = Object.keys(channels as object);
    const strips = chans.map((c) => {
      let x = c.in;
      if (c.pan !== undefined) x = pan(x, c.pan);
      if (c.db !== undefined) x = gain(x, c.db);
      return x;
    });
    if (strips.length === 0) return kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
    for (const [i, k] of keys.entries()) {
      const b = busOf(chans[i].in);
      if (b === undefined) continue;
      const into = strips.filter((_, j) => chans[j].to === k);
      feed(b, into.length === 0 ? kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 }) : sum(...into));
    }
    return sum(...strips.filter((_, j) => chans[j].to === undefined));
  },
);

export const master = component(
  {
    id: "master",
    label: "Master",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: { ceiling: control.db(-1).range(-24, 0) },
    output: port.audio(),
  },
  (x: Audio, { ceiling }: Params) => limit(x, ceiling),
);
"#;

// PKG-5, design-8b §5.1, RENDER-2: every mixer channel and the master have a gain, and the master
// is stereo from its input on, and no pure fixture renders one bit otherwise: each, played through
// a copy of std's mixer and master from before, against the same song through today's. Both are
// rendered here, so this holds whatever `renders.json` records. A gain at 0 dB is `x × 1.0`; a
// mono mix copied into both channels limits as it did in one.
#[test]
fn the_mixer_and_master_gains_change_no_bit() {
    let kernels = |s: &Session, id: &str| {
        s.accepted().compiled.flat.nodes.iter().filter(|n| matches!(&n.op, FOp::Kernel { kernel, .. } if kernel == id)).count()
    };
    // Its gains and stereos, and its sound.
    let heard = |s: Session| ((kernels(&s, "std:gain"), kernels(&s, "std:stereo")), render_song(&s));
    for name in PURE {
        let f = fixture(name);
        let ((gains, stereo), now) = heard(f.open());
        let song = f.read("song.ts");
        assert!(song.contains("from \"std/mix\""), "{name}: its song uses std's mixer");
        f.write("old-mix.ts", OLD_MIX);
        f.write("song.ts", song.replace("from \"std/mix\"", "from \"./old-mix\""));
        let before = f.open();
        assert!(before.problems().is_empty(), "{name}: {:?}", before.problems());
        let ((gains_before, stereo_before), then) = heard(before);
        // Today's graph has more: a gain for each channel that didn't write one, the master's, and its stereo.
        assert!(gains > gains_before, "{name}: {gains} gains today against {gains_before} before");
        assert_eq!((stereo, stereo_before), (1, 0), "{name}");
        assert_eq!(now.len(), then.len(), "{name}");
        let moved = now.iter().zip(&then).position(|(a, b)| a.map(f32::to_bits) != b.map(f32::to_bits));
        assert!(moved.is_none(), "{name}: frame {moved:?} differs: {:?} today, {:?} before", moved.map(|i| now[i]), moved.map(|i| then[i]));
        assert!(name == "templates/empty" || now.iter().any(|f| f[0] != 0.0), "{name}: it plays something");
    }
}

// FMT-6, FMT-16: compile never opens an asset (it reads the hashes the snapshot recorded), so
// its time doesn't depend on how big the assets are.
#[test]
fn compile_never_opens_an_asset() {
    let f = Fixture::new();
    f.write("music2.toml", "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\n");
    f.write(
        "song.ts",
        "import { song, track } from \"@m2/core\";\n\
         const v = track(\"vox\", ({ lane }) => lane.audio(\"takes\"));\n\
         export default song(() => v.out);\n",
    );
    f.write("data/vox/takes.json", r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "take.wav"}]}"#);
    f.write("assets/take.wav", vec![0u8; 8 << 20]);
    let snap = Snapshot::read(&f.0).unwrap();
    // Gone from the disk: a compile that opened it would find it missing.
    std::fs::remove_file(f.0.join("assets/take.wav")).unwrap();
    let project = Project::from_snapshot(&snap).unwrap();
    let c = compile_sources(&project, &mut std_packages(), &Default::default(), None).unwrap_or_else(|d| panic!("{d:?}"));
    assert_eq!(c.problems.iter().map(|d| d.code).collect::<Vec<_>>(), ["asset.unpinned"]);
}

// RENDER-32: one sandbox a box (report §2.2). p50, the 50-track reference song, builds 202: one
// for each kernel outside an instrument (152: the report counted 150, before std's master gained its
// `stereo` and its gain), and one for each poly's voices (50), which share it. One a kernel state
// was 6,060.
#[test]
fn p50_builds_202_sandboxes() {
    let f = Fixture::new();
    crate::reference::write(&f.0, &crate::reference::Reference::P50).unwrap();
    let s = f.open();
    let rev = s.accepted();
    let boxes = rev.compiled.flat.nodes.iter().filter(|n| matches!(n.op, FOp::Kernel { .. } | FOp::Poly { .. })).count();
    let r = Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, rev.project.sample_rate()).unwrap();
    assert_eq!((r.sandboxes(), boxes), (202, 202));
}

// ---- review Y6/Y8 songs (shared by the recorder at main and the tests at wave3) ----

/// Thirty-two hits a sixteenth apart, never two at once, over several notes and strengths: more
/// ring at once than four voices hold, for the drums that ring long.
fn dense_beat() -> Vec<serde_json::Value> {
    (0..32)
        .map(|i| {
            let pitch = ["C2", "D2", "F#2", "A#2", "G#2", "A2", "C3", "D#3"][i % 8];
            serde_json::json!({"pitch": pitch, "start": i as f64 * 0.25, "length": 0.2, "velocity": 0.4 + 0.6 * ((i * 7) % 11) as f64 / 10.0})
        })
        .collect()
}

/// Two hits on one sample, four times.
fn doubled_beat() -> Vec<serde_json::Value> {
    (0..8)
        .map(|i| {
            let pitch = ["C2", "F#2"][i % 2];
            serde_json::json!({"pitch": pitch, "start": (i / 2) as f64, "length": 0.5, "velocity": 0.5 + 0.1 * i as f64})
        })
        .collect()
}

/// Five notes of mixed lengths and strengths, two of them overlapping.
fn five_notes() -> Vec<serde_json::Value> {
    [("A3", 0.0, 1.5, 0.8), ("C4", 0.5, 0.25, 0.5), ("E4", 1.0, 3.0, 1.0), ("G3", 2.0, 0.5, 0.3), ("A4", 2.5, 1.0, 0.9)]
        .iter()
        .map(|(p, s, l, v)| serde_json::json!({"pitch": p, "start": s, "length": l, "velocity": v}))
        .collect()
}

/// A song of one track playing `call` on `notes`, with `deps` as its `[dependencies]`.
fn y_song(f: &Fixture, deps: &str, call: &str, notes: Vec<serde_json::Value>) {
    f.write("music2.toml", format!("[project]\nname = \"y\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n{deps}"));
    f.write("data/beat/notes.json", serde_json::json!({"clips": [{"id": "c1", "start": 0, "length": 12, "notes": notes}]}).to_string());
    f.write(
        "song.ts",
        format!(
            "import {{ song, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\nimport {{ bass, fm, pad, pluck, synth }} from \"@m2/synths\";\n\n\
             const beat = track(\"beat\", ({{ lane }}) => {call});\n\nexport default song(() => beat.out);\n"
        ),
    );
}

/// The named sounds of `@m2/synths` (y8), as `(name, call)`.
const NAMED: [(&str, &str); 13] = [
    ("synth.supersaw", "synth.supersaw(lane.notes(\"notes\"))"),
    ("synth.lead", "synth.lead(lane.notes(\"notes\"))"),
    ("synth.pad", "synth.pad(lane.notes(\"notes\"))"),
    ("synth.pluck", "synth.pluck(lane.notes(\"notes\"))"),
    ("synth.sub", "synth.sub(lane.notes(\"notes\"))"),
    ("synth.bass", "synth.bass(lane.notes(\"notes\"))"),
    ("synth.strings", "synth.strings(lane.notes(\"notes\"))"),
    ("fm.keys", "fm.keys(lane.notes(\"notes\"))"),
    ("fm.bell", "fm.bell(lane.notes(\"notes\"))"),
    ("fm.metal", "fm.metal(lane.notes(\"notes\"))"),
    ("bass", "bass(lane.notes(\"notes\"))"),
    ("pluck", "pluck(lane.notes(\"notes\"))"),
    ("pad", "pad(lane.notes(\"notes\"))"),
];

/// What each of `@m2/drums`' nine single drums played of [`dense_beat`] (at its own voices, 4, or
/// 6 for the hats and the tom) and of [`doubled_beat`], and what each named sound of `@m2/synths`
/// played of [`five_notes`] at its defaults: recorded at `1cd9b79` (main before wave 3: std 0.8.0,
/// `@m2/drums` and `@m2/synths` 1.0.0), with the songs these tests play (review Y6, Y8).
const DENSE: [(&str, &str); 9] = [
    ("kick", "b0b62bd32f989601e3a7cd2276769d46018b21509033aeab12251115ee9565d6"),
    ("snare", "ac01f945eda49570f9043cbabfe9fe0742a807110c057219c26ad82d33cdab1e"),
    ("clap", "391affbe61d5b8be721b782135a3e1b1224eccb3a9681719d1a640303f44187f"),
    ("hats", "75e2a08e8c48a929cb083cf79afd645177ef62884cc875792d061cd5a110870e"),
    ("tom", "52b25824070c69d3a9f4c03b1721c7afdf70c664b6a073672ae3f49883087ce9"),
    ("rim", "2d16290baf1aa685c54e60f1a6a6011c0a1828b689cbd6e3817d98f9bdd5fb41"),
    ("cowbell", "5452c25cb14d3b7f40ecaba812c808591501484f48a97d195d4a9304b10d2bbf"),
    ("cymbal", "3e2d7e8d459a40fa865d875b569e1074a290022b9871dcd840331bfec88ed0bc"),
    ("shaker", "fc2abcf187191e93cf4af68691d14bd9a2a4b32595e7133858a86281c61cbd0f"),
];
const DOUBLED: [(&str, &str); 9] = [
    ("kick", "c0975c22c2a7f7ef6cc944356ede57da1e766ed8ae6c4545e06d47553f35c75a"),
    ("snare", "de680c8638efb26767a954e0b883eaf13ca15e2cbee3fdf5e9529307f487a72c"),
    ("clap", "ebfb53e024f80697c922df9b40d402a818794bf37c916d21b4a94ac55392a051"),
    ("hats", "d00584031ba39252af3834df4aa729b2ec5349976ec93149130654ceebb7f16c"),
    ("tom", "025218510141a480d515147df8514ef79287216e3cd2bc3beedd7b7d594f3596"),
    ("rim", "32766cf5cd0105fd744c5409ad743a602b00373be2eeda0209d491281802d359"),
    ("cowbell", "c550cb50bd964fa0a326fbee6fa1d2e733c732d8f2cbf119b95af3678d90ab8d"),
    ("cymbal", "6c10f9c00ceb1d5f29d28dc2329c798a0b73067d57e540db3ebb3652d94954ac"),
    ("shaker", "f6ae5e2f1173b2cf88717357e80ed71d439553489cd110f12bc5362dab75cff6"),
];
const NAMED_BITS: [(&str, &str); 13] = [
    ("synth.supersaw", "d0beb5c2ec3afede7ee479d38831e0f7d3dda6e37fbdc49ed209f477d5ea3624"),
    ("synth.lead", "311ac8abdee69686ce0bda17c60cbef257f21ad4ccb295d732fea23d113bca2a"),
    ("synth.pad", "4f68f5a7b687f89fcd3cb320d8accacb1e7ce2f35c4758a4d748bbe53566a279"),
    ("synth.pluck", "0193dfbf3445e63ff7d6ccdf0c8ddd782e7f38f64666c1aa550f4f32f97beac4"),
    ("synth.sub", "c558be1599658595ffaccd336ed1e21eec0ea8e975ca3ea230f5f9f1f12ed7ad"),
    ("synth.bass", "ab7ef1855345faaedc9a42324fb23980b7cc063048a19162c338a1af3d36cbba"),
    ("synth.strings", "1f7f4aa442a315299c973cf8c8f798335426b4bd7188eecb18b633aac8168d96"),
    ("fm.keys", "dfbff17bcb17f7fcbbe9cd9db4ffa848c875fb2c519a0f26a1326543b863d40b"),
    ("fm.bell", "d58e695da0b95957ab258932df1e84f80713dd7898b4f0e07db15a598fb71300"),
    ("fm.metal", "d90ad214b5fcfa8d571d0c8d3eeee1b66390dedaa96d5c7ba1a307b2e73bba0b"),
    ("bass", "e40ed13e9d3bbdccb4b6e887ceccab47b04dba295c362c7dcb2027cb4b1974cf"),
    ("pluck", "464b7411dff513df7cecf992d9dfdb211e4c033d06f449c206d966096d8b591f"),
    ("pad", "002e5b6c2b2f492ac6b7549a5d0c45d5421c370771e8caf8db1881820ff9eb39"),
];

/// `call` played on `notes` in a song asking for what this tree bundles.
fn render_y(call: &str, notes: Vec<serde_json::Value>) -> String {
    let f = Fixture::new();
    let std = m2_song::Version::parse(&std_packages().get("std").unwrap().version).unwrap();
    let major = |pack: &str| m2_song::pack::PkgManifest::at(&m2_song::pack::source_packs().join(pack)).unwrap().package.version.0;
    let deps = format!(
        "\"@m2/core\" = \"2\"\nstd = \"{}.{}\"\n\"@m2/drums\" = \"{}\"\n\"@m2/synths\" = \"{}\"\n",
        std.0,
        std.1,
        major("@m2/drums"),
        major("@m2/synths")
    );
    y_song(&f, &deps, call, notes);
    let s = f.open();
    assert!(s.draft_errors.is_empty() && s.problems().is_empty(), "{call}: {:?} {:?}", s.draft_errors, s.problems());
    bits(&render_song(&s))
}

// CARD-33 (review Y6): One drum at its single's voices (4, or 6 for the hats and the tom) plays the
// bits its single did on a beat where more ring at once than its voices hold; and two hits on one
// sample of a drum that chokes as its single did. (Two hits on one sample of a drum that rings on,
// a tom, rim, cowbell or shaker, count as two where its single counted one: CARD-33 says so.)
#[test]
fn one_drum_plays_its_singles_bits_at_their_voices() {
    for ((drum, dense), (_, doubled)) in DENSE.iter().zip(DOUBLED) {
        let voices = if matches!(*drum, "hats" | "tom") { 6 } else { 4 };
        let call = format!("drums.drum(lane.notes(\"notes\"), {{ drum: \"{drum}\", voices: {voices} }})");
        assert_eq!(&render_y(&call, dense_beat()), dense, "One drum's {drum} at {voices} voices, on a dense beat");
        if matches!(*drum, "kick" | "snare" | "clap" | "hats" | "cymbal") {
            assert_eq!(render_y(&call, doubled_beat()), doubled, "One drum's {drum}, two hits on one sample");
        }
    }
}

// CARD-31 (review Y8): every named sound of `@m2/synths`, given its new knobs at its patch's own
// numbers, plays at its defaults the bits it played before wave 3.
#[test]
fn the_named_sounds_play_the_bits_they_did() {
    for (name, call) in NAMED {
        let want = NAMED_BITS.iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(render_y(call, five_notes()), want, "{name} plays other bits than it did");
    }
}

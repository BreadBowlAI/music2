//! The step 4 gates, as a running session sees them: keys and labels, data that doesn't read,
//! the manifest's format, imports, presentation files and assets.

use std::path::{Path, PathBuf};

use m2_ir::{FNode, FOp, Flat};
use m2_runtime::{PackageSet, Renderer};
use m2_song::tx::{Anchor, Base, Edit, Expect, Op, Pos, Range, Tx, TxPath};
use m2_song::{LaneData, Project, Snapshot};

use crate::{Session, compile_sources};

pub(crate) fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
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

// PLANE-3, FMT-19, FMT-20: presentation and local files never make a revision.
#[test]
fn editing_layout_json_creates_no_revision() {
    let f = Fixture::first_song();
    let mut s = f.open();
    for (file, text) in [
        ("layout.json", "{\"tracks\": {\"kick\": {\"colour\": \"#e4572e\"}}}"),
        ("layout.json", "{\"tracks\": {}}"),
        (".music2/overlays.json", "{}"),
        ("README.md", "notes"),
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
/// other one, examples/with-plugins, hosts plugins, which need not.
const PURE: [&str; 5] = ["examples/first-song", "examples/resample", "examples/sounds", "templates/starter", "templates/empty"];

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

/// std's mixer and master before every channel and the master had a gain (8b.3), as a project's
/// own file: a channel's `db` optional, no master `db`, nothing to make the master stereo.
const OLD_MIX: &str = r#"import { component, control, each, kernel, port, signature, type Audio, type Params, type Signal, type Value } from "@m2/core";
import { gain, pan } from "std/dsp";
import { limit } from "std/fx";
import { sum } from "std/math";

export type Channel = Audio | { in: Audio; db?: Value; pan?: Value };

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
      }),
    },
    output: port.audio(),
    positional: ["channels"],
  },
  function mixer({ channels }: Params): Audio {
    const strips = Object.values(channels as Record<string, { in: Audio; db?: Signal; pan?: Signal }>).map((c) => {
      let x = c.in;
      if (c.pan !== undefined) x = pan(x, c.pan);
      if (c.db !== undefined) x = gain(x, c.db);
      return x;
    });
    if (strips.length === 0) return kernel("std:scale", { in: kernel("std:sine", { freq: 0 }), by: 0 });
    return sum(...strips);
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
    f.write("music2.toml", "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\n");
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

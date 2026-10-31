use std::path::Path;

use super::*;
use crate::Snapshot;
use crate::diag::Severity;

const MANIFEST: &str = "[project]\nname = \"Test\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.6\"\n\"@m2/drums\" = \"1\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"1\"\n";

/// A project folder with `files`, removed when dropped.
struct Dir(PathBuf);

impl Dir {
    fn new(files: &[(&str, &[u8])]) -> Dir {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("m2-song-data-{}-{n}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        let d = Dir(d);
        d.write("music2.toml", MANIFEST.as_bytes());
        d.write("song.ts", b"export default 0;\n");
        for (p, bytes) in files {
            d.write(p, bytes);
        }
        d
    }

    fn write(&self, p: &str, bytes: &[u8]) {
        let p = self.0.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    fn project(&self) -> Project {
        Project::open(&self.0).unwrap_or_else(|e| panic!("{e:?}"))
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn addr(m: &str, l: &str) -> LaneAddr {
    LaneAddr::new(m, l).unwrap()
}

fn key(k: &str) -> Key {
    Key::new(k).unwrap()
}

fn lanes(decls: &[(&str, &str, LaneKind)]) -> Decls {
    Decls { lanes: decls.iter().map(|(m, l, k)| (addr(m, l), k.clone())).collect(), ..Decls::default() }
}

fn hz() -> LaneKind {
    LaneKind::Control { unit: Some("hz".into()) }
}

fn codes(d: &[Diagnostic]) -> Vec<&'static str> {
    d.iter().map(|d| d.code).collect()
}

const HITS: &str =
    r#"{"clips": [{"id": "a1", "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1, "velocity": 1}]}]}"#;

// FMT-6: a snapshot reads Manifest, Code, Arrangement, Lane and sidecar files only; assets and
// blobs are hashed or listed, never read into it.
#[test]
fn a_snapshot_reads_the_explicit_set() {
    let d = Dir::new(&[
        ("tracks/kick.ts", b"export default 1;\n"),
        ("data/arrangement.json", b"{\"tempo\": [{\"beat\": 0, \"bpm\": 120}]}"),
        ("data/kick/hits.json", HITS.as_bytes()),
        ("data/lead/lead.state", b"\x00blob"),
        ("data/lead/lead.state.json", b"{\"plugin\": \"clap:x\", \"plugin_version\": \"1\"}"),
        ("assets/kick.wav", b"RIFF"),
        ("layout.json", b"{}"),
        ("README.md", b"hi"),
        ("lib/helper.js", b"export {}"),
        ("tsconfig.json", b"{}"),
        (".music2/overlays.json", b"{}"),
        ("node_modules/x/index.ts", b""),
        ("data/kick/notes.txt", b"typo"),
        ("data/kick/.DS_Store", b""),
    ]);
    let s = Snapshot::read(&d.0).unwrap();
    let files: Vec<String> = s.files.keys().map(|p| slashed(p)).collect();
    assert_eq!(
        files,
        ["data/arrangement.json", "data/kick/hits.json", "data/lead/lead.state.json", "music2.toml", "song.ts", "tracks/kick.ts"]
    );
    assert_eq!(s.binaries.get(Path::new("data/lead/lead.state")), Some(&Stored::Hash(Sha::of(b"\x00blob"))));
    assert_eq!(s.binaries.get(Path::new("assets/kick.wav")), Some(&Stored::Listed), "no lane names it");
    assert_eq!(s.unknown.iter().map(|p| slashed(p)).collect::<Vec<_>>(), ["data/kick/notes.txt"]);
}

// PLANE-3: Presentation and Ignored files never make a snapshot differ.
#[test]
fn presentation_and_ignored_files_change_no_snapshot() {
    let d = Dir::new(&[("data/kick/hits.json", HITS.as_bytes())]);
    let before = Snapshot::read(&d.0).unwrap();
    d.write("layout.json", b"{\"tracks\": {\"kick\": {\"colour\": \"#e4572e\"}}}");
    d.write(".music2/zoom.json", b"{}");
    d.write("data/kick/old.bak", b"x");
    d.write("notes.md", b"x");
    assert_eq!(Snapshot::read(&d.0).unwrap(), before);
    d.write("data/kick/hits.json", HITS.replace("\"velocity\": 1", "\"velocity\": 0.5").as_bytes());
    assert_ne!(Snapshot::read(&d.0).unwrap(), before);
}

// FMT-9: a lane file that fails raises an Error naming the file and the field, never stops
// anything else, and plays the last accepted data, or nothing.
#[test]
fn a_bad_lane_keeps_its_last_accepted_data() {
    let d = Dir::new(&[("data/kick/hits.json", HITS.as_bytes()), ("data/bass/notes.json", HITS.replace("C1", "A1").as_bytes())]);
    let decls = lanes(&[("kick", "hits", LaneKind::Notes), ("bass", "notes", LaneKind::Notes)]);
    let (accepted, diags) = d.project().data(&decls, None);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(accepted.notes("kick", "hits").len(), 1);

    // A hand edit with an unknown field, in a running session.
    d.write("data/kick/hits.json", HITS.replace("\"velocity\": 1", "\"velocity\": 1, \"vel\": 2").as_bytes());
    let (data, diags) = d.project().data(&decls, Some(&accepted));
    assert_eq!(codes(&diags), ["lane.invalid"]);
    let e = &diags[0];
    assert_eq!((e.severity, e.src.as_deref(), e.module.as_deref()), (Severity::Error, Some("data/kick/hits.json"), Some("kick")));
    assert!(e.message.contains("unknown field `vel`") && e.message.contains("plays the data it had"), "{}", e.message);
    assert_eq!(data.notes("kick", "hits"), accepted.notes("kick", "hits"), "the last accepted data keeps playing");
    assert_eq!(data.notes("bass", "notes"), accepted.notes("bass", "notes"), "other lanes are untouched");

    // On open there is no accepted data: only that lane is silent.
    let (data, diags) = d.project().data(&decls, None);
    assert_eq!(codes(&diags), ["lane.invalid"]);
    assert!(diags[0].message.contains("plays nothing"), "{}", diags[0].message);
    assert!(data.notes("kick", "hits").is_empty());
    assert_eq!(data.notes("bass", "notes").len(), 1);

    // Data accepted for another kind is never kept.
    let other = lanes(&[("kick", "hits", hz())]);
    let (data, _) = d.project().data(&other, Some(&accepted));
    assert!(data.control("kick", "hits").is_empty());
}

// FMT-10: a control lane in another unit is lane.unit-mismatch, with a Retag fix.
#[test]
fn unit_mismatch_offers_retag() {
    let text = r#"{"unit": "db", "clips": [{"id": "c1", "start": 0, "length": 4, "points": [[0, -6]]}]}"#;
    let d = Dir::new(&[("data/bass/cutoff.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("bass", "cutoff", hz())]), None);
    assert_eq!(codes(&diags), ["lane.unit-mismatch"]);
    assert!(diags[0].message.contains("`db`") && diags[0].message.contains("`hz`"), "{}", diags[0].message);
    assert!(data.control("bass", "cutoff").is_empty());
    let fix = &diags[0].fixes[0];
    assert_eq!(fix.code, "retag");
    let Op::Write { file, bytes: Payload::Text(new) } = &fix.tx.ops[0] else { panic!() };
    assert_eq!(file.as_str(), "data/bass/cutoff.json");
    assert!(new.starts_with("{\"unit\": \"hz\", "), "{new}");
    assert_eq!(fix.tx.base[file], Base::Sha(Sha::of(text.as_bytes())));
    // DIAG-1: the fix is a Tx confined to its lane, with an id derived from what it was raised on.
    assert_eq!(fix.tx.expect, Expect::Confined(Confined { lanes: vec![LaneAddr::new("bass", "cutoff").unwrap()], ..Default::default() }));
    assert!(fix.tx.id.starts_with("fix-") && fix.tx.validate().is_ok(), "{}", fix.tx.id);
}

// FMT-12: anonymous clips, invalid ids and duplicates are diagnostics with fixes, never parse
// failures, and ids are never assigned on read.
#[test]
fn clip_ids() {
    let text = r#"{"clips": [{"start": 0, "length": 4, "notes": []}, {"id": "BAD", "start": 4, "length": 4, "notes": []},
                             {"id": "x1", "start": 8, "length": 4, "notes": []}, {"id": "x1", "start": 12, "length": 4, "notes": []}]}"#;
    let d = Dir::new(&[("data/kick/hits.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("kick", "hits", LaneKind::Notes)]), None);
    assert_eq!(codes(&diags), ["clip.anonymous", "clip.invalid-id", "clip.duplicate-id"]);
    assert_eq!(diags.iter().map(|d| d.severity).collect::<Vec<_>>(), [Severity::Notice, Severity::Notice, Severity::Warning]);
    assert_eq!(data.lane("kick", "hits").unwrap().ids(), [None, Some("BAD"), Some("x1"), Some("x1")], "kept as written");

    let fixed = |fix: &Fix| {
        let Op::Write { bytes: Payload::Text(text), .. } = &fix.tx.ops[0] else { panic!() };
        LaneData::read(text, &LaneKind::Notes).unwrap()
    };
    let assigned = fixed(&diags[0].fixes[0]);
    assert_eq!(diags[0].fixes[0].code, "assign-clip-ids");
    assert_eq!(diags[0].fixes[0], diags[1].fixes[0], "one fix for both");
    let ids: Vec<&str> = assigned.ids().into_iter().flatten().collect();
    assert_eq!(ids.len(), 4);
    assert!(ids.iter().all(|i| ClipId::valid(i)));
    let renumbered = fixed(&diags[2].fixes[0]);
    let ids: BTreeSet<&str> = renumbered.ids().into_iter().flatten().filter(|i| ClipId::valid(i)).collect();
    assert_eq!(ids.len(), 2, "each its own id: {ids:?}");
    assert!(ids.contains("x1"), "the first keeps its id");
    assert!(renumbered.ids().contains(&None) && renumbered.ids().contains(&Some("BAD")), "only shared ids change");
    // The same file always gets the same fix (FMT-11: the id is derived, not random).
    let (_, again) = d.project().data(&lanes(&[("kick", "hits", LaneKind::Notes)]), None);
    assert_eq!(again, diags);
}

// FMT-15: references are found by listing, pins are checked, and a missing asset silences
// only its clips.
#[test]
fn asset_pins_and_names() {
    let wav = b"RIFF....WAVE";
    let pin = Sha::of(wav).pin();
    let clip = |id: &str, start: u32, asset: &str, hash: Option<&str>| {
        let hash = hash.map_or_else(String::new, |h| format!(", \"hash\": \"{h}\""));
        format!(r#"{{"id": "{id}", "start": {start}, "length": 1, "asset": "{asset}"{hash}}}"#)
    };
    let text = format!(
        r#"{{"clips": [{}, {}, {}, {}, {}]}}"#,
        clip("a", 0, "Drums/Kick.wav", Some(&pin)),
        clip("b", 1, "Drums/Kick.wav", None),
        clip("c", 2, "drums/kick.wav", Some(&pin)),
        clip("d", 3, "snare.wav", Some("0000000000000000")),
        clip("e", 4, "gone.wav", None),
    );
    let d = Dir::new(&[("assets/Drums/Kick.wav", wav), ("assets/snare.wav", wav), ("data/drums/takes.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("drums", "takes", LaneKind::Audio)]), None);
    assert_eq!(codes(&diags), ["asset.missing", "asset.missing", "asset.changed", "asset.unpinned"]);
    let missing: Vec<&str> = diags[..2].iter().map(|d| d.message.as_str()).collect();
    assert!(missing[0].contains("`drums/kick.wav`") && missing[0].contains("use that name"), "{}", missing[0]);
    assert_eq!(diags[0].fixes[0].code, "fix-asset-name");
    assert!(missing[1].contains("`gone.wav`") && diags[1].fixes.is_empty(), "{}", missing[1]);
    assert_eq!((diags[2].severity, diags[2].fixes[0].code), (Severity::Warning, "repin-asset"));
    assert_eq!((diags[3].severity, diags[3].fixes[0].code), (Severity::Notice, "repin-asset"));
    assert!(data.asset_path("Drums/Kick.wav").is_some() && data.asset_path("snare.wav").is_some());
    assert!(data.asset_path("drums/kick.wav").is_none(), "a case-only match never plays");
    assert!(data.asset_path("gone.wav").is_none());
    // The repin fixes write the pins.
    let Op::Write { bytes: Payload::Text(repinned), .. } = &diags[3].fixes[0].tx.ops[0] else { panic!() };
    assert!(
        repinned.contains(&format!("\"id\": \"b\", \"start\": 1, \"length\": 1, \"asset\": \"Drums/Kick.wav\", \"hash\": \"{pin}\"")),
        "{repinned}"
    );
}

// FMT-16: compile never decodes audio. Reading the data costs the same whatever an asset holds:
// a file that isn't even audio is just hashed.
#[test]
fn reading_data_never_decodes_assets() {
    let text = r#"{"clips": [{"id": "a", "start": 0, "length": 1, "asset": "big.wav"}]}"#;
    let d = Dir::new(&[("assets/big.wav", &vec![7u8; 1 << 20]), ("data/v/takes.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("v", "takes", LaneKind::Audio)]), None);
    assert_eq!(codes(&diags), ["asset.unpinned"]);
    assert!(data.asset_path("big.wav").is_some());
}

// FMT-17: a blob loads only with a sidecar naming the code's plugin.
#[test]
fn plugin_states_need_their_sidecar() {
    let surge = "clap:org.surge-synth-team.surge-xt";
    let sidecar = |p: &str| Sidecar { plugin: p.into(), plugin_version: "1.3.1".into() }.canonical();
    let d = Dir::new(&[
        ("data/lead/lead.state", b"surge"),
        ("data/lead/lead.state.json", sidecar(surge).as_bytes()),
        ("data/pad/pad.state", b"other"),
        ("data/pad/pad.state.json", sidecar("clap:com.airwindows.consolidated").as_bytes()),
        ("data/vox/chain-l/tune.state", b"bare"),
        ("data/fx/space.state", b"x"),
        ("data/fx/space.state.json", b"{\"plugin\": \"clap:x\", \"plugin_version\": \"1\", \"params\": {}}"),
    ]);
    let state = |m: &str, path: &[&str], plugin: &str| StateDecl {
        module: key(m),
        path: path.iter().map(|k| key(k)).collect(),
        plugin: plugin.into(),
    };
    let decls = Decls {
        lanes: vec![],
        assets: vec![],
        packages: BTreeMap::new(),
        states: vec![
            state("lead", &["lead"], surge),
            state("pad", &["pad"], surge),
            state("vox", &["chain-l", "tune"], surge),
            state("fx", &["space"], "clap:x"),
            state("keys", &["airwin"], "clap:com.airwindows.consolidated"),
            state("bass", &["synth"], surge),
        ],
    };
    let (data, diags) = d.project().data(&decls, None);
    assert_eq!(codes(&diags), ["state.plugin-mismatch", "state.unverified", "state.unverified"]);
    assert_eq!(data.state("lead", &["lead"]), Some(d.0.canonicalize().unwrap().join("data/lead/lead.state")));
    assert_eq!(data.state("pad", &["pad"]), None);
    assert_eq!(data.state("vox", &["chain-l", "tune"]), None);
    assert_eq!(data.state("fx", &["space"]), None, "a sidecar with more than its two fields is unverified");
    assert_eq!(data.state("bass", &["synth"]), None, "no file: the plugin's defaults, and nothing to say");
    assert!(diags[2].message.contains("params"), "names the field: {}", diags[2].message);
    // A mismatch offers the plugin that wrote it, where it has no state, and Delete.
    let fixes: Vec<(&str, &str)> = diags[0].fixes.iter().map(|f| (f.code, f.title.as_str())).collect();
    assert_eq!(fixes, [("relink", "Move it to airwin"), ("delete-data", "Delete the saved state")]);
    assert_eq!(diags[1].fixes.iter().map(|f| f.code).collect::<Vec<_>>(), ["delete-data"]);
    assert_eq!(diags[1].src.as_deref(), Some("data/vox/chain-l/tune.state"));
}

// FMT-7: an unparseable file under data/ is a Notice; elsewhere, silence.
#[test]
fn unknown_data_files_are_noticed() {
    let d = Dir::new(&[("data/Kick/hits.json", b"{}"), ("data/kick.hits.json", b"{}"), ("notes.txt", b"x")]);
    let (_, diags) = d.project().data(&Decls::default(), None);
    assert_eq!(codes(&diags), ["file.unknown", "file.unknown"]);
    assert_eq!(diags.iter().map(|d| d.src.as_deref().unwrap()).collect::<Vec<_>>(), ["data/Kick/hits.json", "data/kick.hits.json"]);
    assert!(diags.iter().all(|d| d.severity == Severity::Notice));
}

// FMT-1, FMT-3, FMT-9: there is no reader for format 0, and format errors stop the open; manifest
// and arrangement errors block; lanes never.
#[test]
fn what_blocks() {
    let d = Dir::new(&[("data/kick/hits.json", b"not json")]);
    assert!(Project::open(&d.0).is_ok(), "a lane file never blocks");
    d.write("data/arrangement.json", b"{\"tempo\": [{\"beat\": 0, \"bpm\": 5}]}");
    assert_eq!(codes(&Project::open(&d.0).unwrap_err()), ["arrangement.invalid"]);
    d.write("music2.toml", MANIFEST.replace("format = 1", "format = 2\nwhat = 1").as_bytes());
    assert_eq!(codes(&Project::open(&d.0).unwrap_err()), ["format.newer"]);
    d.write("music2.toml", b"[project]\nname = \"old\"\n");
    assert_eq!(codes(&Project::open(&d.0).unwrap_err()), ["format.missing"]);
    std::fs::remove_file(d.0.join("music2.toml")).unwrap();
    assert_eq!(codes(&Project::open(&d.0).unwrap_err()), ["manifest.invalid"]);
}

// DIAG-6's inputs: lane files no declared lane reads.
#[test]
fn unused_lane_files() {
    let d = Dir::new(&[("data/kick/hits.json", HITS.as_bytes()), ("data/kick/old.json", b"{\"clips\": []}"), ("data/lead/x.state", b"s")]);
    let p = d.project();
    let declared: BTreeSet<LaneAddr> = [addr("kick", "hits")].into();
    let unused: Vec<String> = p.unused(&declared).iter().map(|u| u.addr.to_string()).collect();
    assert_eq!(unused, ["kick/old"]);
    assert_eq!(p.data_modules(), [key("kick"), key("lead")].into());
}

// FMT-8a gate: permuting any array of a lane file plays the same data.
#[test]
fn permuted_files_play_the_same() {
    let a = r#"{"clips": [{"id": "b", "start": 4, "length": 4, "notes": [{"pitch": 40, "start": 1, "length": 1}, {"pitch": 40, "start": 0, "length": 1}]},
                          {"id": "a", "start": 0, "length": 4, "notes": [{"pitch": 45, "start": 0, "length": 2}, {"pitch": 43, "start": 0, "length": 2}]}]}"#;
    let b = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": 43, "start": 0, "length": 2}, {"pitch": 45, "start": 0, "length": 2}]},
                          {"id": "b", "start": 4, "length": 4, "notes": [{"pitch": 40, "start": 0, "length": 1}, {"pitch": 40, "start": 1, "length": 1}]}]}"#;
    let decls = lanes(&[("k", "n", LaneKind::Notes)]);
    let x = Dir::new(&[("data/k/n.json", a.as_bytes())]).project().data(&decls, None).0;
    let y = Dir::new(&[("data/k/n.json", b.as_bytes())]).project().data(&decls, None).0;
    assert_eq!(x.notes("k", "n"), y.notes("k", "n"));
    assert_eq!(x.lane("k", "n"), y.lane("k", "n"));
}

#[test]
fn sidecars_are_canonical_and_strict() {
    let s = Sidecar { plugin: "clap:x".into(), plugin_version: "1.2".into() };
    assert_eq!(s.canonical(), "{\"plugin\": \"clap:x\", \"plugin_version\": \"1.2\"}\n");
    assert_eq!(Sidecar::read(&s.canonical()), Ok(s));
    assert!(Sidecar::read("{\"plugin\": \"clap:x\"}").is_err());
}

// The song ends where its last clip does, in any lane file: data a rename left behind still
// counts until it is moved or deleted.
#[test]
fn the_end_counts_every_lane_file() {
    let later = r#"{"clips": [{"id": "z", "start": 8, "length": 4, "notes": []}]}"#;
    let d = Dir::new(&[("data/kick/hits.json", HITS.as_bytes()), ("data/kick/old.json", later.as_bytes())]);
    let (data, _) = d.project().data(&lanes(&[("kick", "hits", LaneKind::Notes)]), None);
    assert_eq!(data.end(), 12.0);
    assert_eq!(DataSet::empty().end(), 0.0);
}

// ID-16 (a CI gate from step 4): no format v1 schema has a field for a derived id. Strict
// parsing (FMT-8) refuses one wherever it is written.
#[test]
fn no_schema_admits_a_derived_id() {
    let derived = [r#""site": 3"#, r#""node": "n3""#, r#""origin": {"site": 3}"#, r#""runtime_id": 7"#, r#""control": "song.ts#2/0""#];
    for field in derived {
        let notes = format!(
            r#"{{"clips": [{{"id": "a", "start": 0, "length": 1, "notes": [{{"pitch": 60, "start": 0, "length": 1, {field}}}]}}]}}"#
        );
        let clip = format!(r#"{{"clips": [{{"id": "a", "start": 0, "length": 1, {field}, "notes": []}}]}}"#);
        let lane = format!(r#"{{"clips": [], {field}}}"#);
        let points = format!(r#"{{"unit": "hz", "clips": [{{"id": "a", "start": 0, "length": 1, "points": [], {field}}}]}}"#);
        let audio = format!(r#"{{"clips": [{{"id": "a", "start": 0, "length": 1, "asset": "a.wav", {field}}}]}}"#);
        assert!(LaneData::read(&notes, &LaneKind::Notes).is_err(), "{notes}");
        assert!(LaneData::read(&clip, &LaneKind::Notes).is_err(), "{clip}");
        assert!(LaneData::read(&lane, &LaneKind::Notes).is_err(), "{lane}");
        assert!(LaneData::read(&points, &hz()).is_err(), "{points}");
        assert!(LaneData::read(&audio, &LaneKind::Audio).is_err(), "{audio}");
        assert!(Arrangement::read(&format!(r#"{{"tempo": [{{"beat": 0, "bpm": 120, {field}}}]}}"#)).is_err());
        assert!(Arrangement::read(&format!(r#"{{"tempo": [{{"beat": 0, "bpm": 120}}], {field}}}"#)).is_err());
        assert!(Sidecar::read(&format!(r#"{{"plugin": "clap:x", "plugin_version": "1", {field}}}"#)).is_err());
    }
    let toml_field = "site = 3";
    assert!(crate::Manifest::read(&format!("{MANIFEST}\n[project.extra]\n{toml_field}\n")).is_err());
}

/// Make `p` in `d` unreadable (mode 000), as a permission problem or an offline file is.
#[cfg(unix)]
fn lock(d: &Dir, p: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.0.join(p), std::fs::Permissions::from_mode(0o000)).unwrap();
}

#[cfg(unix)]
fn unlock(d: &Dir, p: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(d.0.join(p), std::fs::Permissions::from_mode(0o755)).unwrap();
}

// FMT-9: a lane file that can't be read is a lane file that fails: it stops nothing else, its
// lane plays the last accepted data or nothing, and an unread file no lane reads is still seen.
#[cfg(unix)]
#[test]
fn an_unreadable_lane_file_stops_nothing_else() {
    let d = Dir::new(&[("data/kick/hits.json", HITS.as_bytes()), ("data/bass/notes.json", HITS.replace("C1", "A1").as_bytes())]);
    let decls = lanes(&[("kick", "hits", LaneKind::Notes), ("bass", "notes", LaneKind::Notes)]);
    let (accepted, _) = d.project().data(&decls, None);

    // A file no lane reads, at mode 000, and a dangling link where a lane reads.
    d.write("data/kick/other.json", HITS.as_bytes());
    lock(&d, "data/kick/other.json");
    std::fs::remove_file(d.0.join("data/bass/notes.json")).unwrap();
    std::os::unix::fs::symlink(d.0.join("nowhere.json"), d.0.join("data/bass/notes.json")).unwrap();
    let snap = Snapshot::read(&d.0).expect("a data file that can't be read never stops the read");
    assert_eq!(snap.unreadable.keys().map(|p| slashed(p)).collect::<Vec<_>>(), ["data/bass/notes.json", "data/kick/other.json"]);
    let (data, diags) = d.project().data(&decls, Some(&accepted));
    assert_eq!(codes(&diags), ["lane.invalid", "lane.invalid"]);
    assert_eq!(diags.iter().map(|d| d.src.as_deref().unwrap()).collect::<Vec<_>>(), ["data/bass/notes.json", "data/kick/other.json"]);
    assert!(diags[0].message.contains("can't be read") && diags[0].message.contains("plays the data it had"), "{}", diags[0].message);
    assert!(diags[1].message.contains("No lane reads it"), "{}", diags[1].message);
    assert_eq!(data.notes("bass", "notes"), accepted.notes("bass", "notes"), "the last accepted data keeps playing");
    assert_eq!(data.notes("kick", "hits").len(), 1, "the rest plays");
    // On open, that lane plays nothing; the lane file is still where the lane's data is.
    let p = d.project();
    let (data, _) = p.data(&decls, None);
    assert!(data.notes("bass", "notes").is_empty());
    assert_eq!(p.lane_file(&addr("bass", "notes")), Some(Path::new("data/bass/notes.json")));
    unlock(&d, "data/kick/other.json");
}

// FMT-9, FMT-17: a folder of data that can't be listed silences only what is in it, and a
// sidecar that can't be read leaves its state unverified.
#[cfg(unix)]
#[test]
fn an_unreadable_data_folder_or_sidecar_stops_nothing_else() {
    let surge = "clap:org.surge-synth-team.surge-xt";
    let sidecar = Sidecar { plugin: surge.into(), plugin_version: "1".into() }.canonical();
    let d = Dir::new(&[
        ("data/kick/hits.json", HITS.as_bytes()),
        ("data/bass/notes.json", HITS.as_bytes()),
        ("data/lead/lead.state", b"surge"),
        ("data/lead/lead.state.json", sidecar.as_bytes()),
    ]);
    lock(&d, "data/bass");
    lock(&d, "data/lead/lead.state.json");
    let mut decls = lanes(&[("kick", "hits", LaneKind::Notes), ("bass", "notes", LaneKind::Notes)]);
    decls.states.push(StateDecl { module: key("lead"), path: vec![key("lead")], plugin: surge.into() });
    let p = d.project();
    let (data, diags) = p.data(&decls, None);
    unlock(&d, "data/bass");
    unlock(&d, "data/lead/lead.state.json");
    assert_eq!(codes(&diags), ["lane.invalid", "state.unverified"]);
    assert!(diags[0].message.contains("data/bass can't be read, nor data/bass/notes.json in it"), "{}", diags[0].message);
    assert!(diags[1].message.contains("data/lead/lead.state.json can't be read"), "{}", diags[1].message);
    assert_eq!(data.notes("kick", "hits").len(), 1);
    assert!(data.notes("bass", "notes").is_empty() && data.state("lead", &["lead"]).is_none());
    assert!(p.data_modules().contains(&key("bass")), "its folder is there, read or not");
}

// FMT-9: what can't be read blocks only when it is the manifest, code or the arrangement.
#[cfg(unix)]
#[test]
fn unreadable_code_and_arrangement_block() {
    let d =
        Dir::new(&[("tracks/kick.ts", b"export default 1;\n"), ("data/arrangement.json", b"{\"tempo\": [{\"beat\": 0, \"bpm\": 120}]}")]);
    lock(&d, "tracks/kick.ts");
    let e = Project::open(&d.0).unwrap_err();
    unlock(&d, "tracks/kick.ts");
    assert_eq!((codes(&e), e[0].src.as_deref()), (vec!["code.build"], Some("tracks/kick.ts")));
    lock(&d, "data/arrangement.json");
    let e = Project::open(&d.0).unwrap_err();
    unlock(&d, "data/arrangement.json");
    assert_eq!(codes(&e), ["arrangement.invalid"]);
    lock(&d, "tracks");
    let e = Project::open(&d.0).unwrap_err();
    unlock(&d, "tracks");
    assert!(e[0].code == "code.build" && e[0].message.contains("the folder tracks can't be read"), "{e:?}");
    lock(&d, "music2.toml");
    let e = Project::open(&d.0).unwrap_err();
    unlock(&d, "music2.toml");
    assert!(e[0].code == "manifest.invalid" && e[0].message.contains("music2.toml can't be read"), "{e:?}");
    assert!(Project::open(&d.0).is_ok());
}

// FMT-12: an id written as a number, a boolean or an object is read as anonymous and kept as
// written, with clip.invalid-id and its fix: one hand edit never makes a lane unreadable.
#[test]
fn an_id_that_is_not_a_string_is_kept_and_read_as_anonymous() {
    let text = r#"{"clips": [{"id": 5, "start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 1}]},
                             {"id": {"a": 1}, "start": 4, "length": 4, "notes": []}, {"id": null, "start": 8, "length": 4, "notes": []}]}"#;
    let d = Dir::new(&[("data/kick/hits.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("kick", "hits", LaneKind::Notes)]), None);
    assert_eq!(codes(&diags), ["clip.anonymous", "clip.invalid-id"]);
    assert!(diags[1].message.contains("`5`, `{\"a\":1}`"), "{}", diags[1].message);
    let lane = data.lane("kick", "hits").unwrap();
    assert_eq!(data.notes("kick", "hits").len(), 1, "the lane plays");
    assert_eq!(lane.ids(), [None, None, None]);
    // Kept as written: the canonical form writes it back, and reads back the same.
    let out = canonical(lane);
    assert!(out.contains("{\"id\": 5, \"start\": 0") && out.contains("{\"id\": {\"a\":1}, \"start\": 4"), "{out}");
    assert_eq!(canonical(&LaneData::read(&out, &LaneKind::Notes).unwrap()), out);
    // The fix gives every clip a valid id, in place of what was written.
    let Op::Write { bytes: Payload::Text(fixed), .. } = &diags[1].fixes[0].tx.ops[0] else { panic!() };
    let fixed = LaneData::read(fixed, &LaneKind::Notes).unwrap();
    assert!(fixed.ids().iter().all(|i| i.is_some_and(ClipId::valid)) && fixed.other_ids().iter().all(Option::is_none), "{fixed:?}");
}

// FMT-15, FMT-14: a file whose name is the reference's in another Unicode form is no asset, so
// the clip is silent, and the message says where the file is and what to do.
#[test]
fn an_asset_named_in_another_unicode_form_is_found_and_named() {
    let text = r#"{"clips": [{"id": "a", "start": 0, "length": 1, "asset": "café.wav"}]}"#;
    let d = Dir::new(&[("assets/cafe\u{301}.wav", b"RIFF"), ("data/v/takes.json", text.as_bytes())]);
    let (data, diags) = d.project().data(&lanes(&[("v", "takes", LaneKind::Audio)]), None);
    assert_eq!(codes(&diags), ["asset.missing"]);
    let m = &diags[0].message;
    assert!(m.contains("another Unicode form") && m.contains("Rename the file") && !m.contains("isn't in assets/"), "{m}");
    assert!(data.asset_path("café.wav").is_none());
}

// ID-9, FMT-7: data filed under a bare Windows device name is noticed, and the notice says
// where it belongs.
#[test]
fn data_under_a_bare_device_name_is_told_where_it_belongs() {
    let d = Dir::new(&[("data/aux/hits.json", HITS.as_bytes()), ("data/kick/con.json", HITS.as_bytes())]);
    let (_, diags) = d.project().data(&lanes(&[("aux", "hits", LaneKind::Notes)]), None);
    assert_eq!(codes(&diags), ["file.unknown", "file.unknown"]);
    assert!(diags[0].message.contains("This one belongs at data/aux-/hits.json"), "{}", diags[0].message);
    assert!(diags[1].message.contains("This one belongs at data/kick/con-.json"), "{}", diags[1].message);
}

// WRITE-12, R24 of the step 6 design: a text file that isn't UTF-8 is read with its bad bytes
// replaced, for the build, and its base is the hash of its own bytes.
#[test]
fn a_file_that_isnt_utf8_is_based_on_its_bytes() {
    let bytes = b"{\"clips\": []}\xff\n";
    let d = Dir::new(&[("data/kick/hits.json", bytes)]);
    let snap = Snapshot::read(&d.0).unwrap();
    let p = Path::new("data/kick/hits.json");
    assert_eq!(snap.sha(p), Some(Sha::of(bytes)));
    assert_ne!(snap.sha(p), Some(Sha::of(snap.files[p].as_bytes())));
    assert_eq!(snap.sha(Path::new("song.ts")), Some(Sha::of(b"export default 0;\n")));
    assert_eq!(snap.sha(Path::new("nothing.ts")), None);
}

// DIAG-1, R19: a fix is a ready Tx whose id comes from what it was raised on: the same file gives
// the same fix, and another content of the same lane file another id.
#[test]
fn a_fixs_id_follows_the_content_it_was_raised_on() {
    let ids = |text: &str| {
        let d = Dir::new(&[("data/kick/hits.json", text.as_bytes())]);
        let (_, diags) = d.project().data(&lanes(&[("kick", "hits", LaneKind::Notes)]), None);
        diags[0].fixes[0].tx.id.clone()
    };
    let one = r#"{"clips": [{"start": 0, "length": 4, "notes": []}]}"#;
    let two = r#"{"clips": [{"start": 4, "length": 4, "notes": []}]}"#;
    assert_eq!(ids(one), ids(one));
    assert_ne!(ids(one), ids(two));
    assert!(crate::tx::valid_id(&ids(one)), "{}", ids(one));
}

// PLANE-1 (the step 6 design's 3.2): reading a project again with the data accepted before keeps
// each lane whose file is the text it was read from, as the same kind, instead of parsing it again;
// that gives exactly what reading every file anew gives: the same data, problems in the same
// order, and the same end. Random edits to random lane files, some that don't read, some that
// change a unit, some that move a file away and back.
#[test]
fn reading_again_keeps_unchanged_lanes_as_a_fresh_read_does() {
    let note = |n: u64| format!(r#"{{"pitch": "C{}", "start": {}, "length": 0.5}}"#, 1 + n % 4, n % 8);
    let notes = |seed: u64, clips: u64| {
        let clips: Vec<String> = (0..clips)
            .map(|c| format!(r#"{{"id": "c{c}", "start": {}, "length": 8, "notes": [{}, {}]}}"#, c * 8, note(seed + c), note(seed * 3 + c)))
            .collect();
        format!(r#"{{"clips": [{}]}}"#, clips.join(", "))
    };
    let control = |unit: &str, v: u64| {
        format!(r#"{{"unit": "{unit}", "clips": [{{"id": "p1", "start": 0, "length": 4, "points": [[0, {v}], [4, 1]]}}]}}"#)
    };
    let files =
        [("data/kick/hits.json", notes(1, 2)), ("data/bass/notes.json", notes(2, 3)), ("data/bass/cutoff.json", control("hz", 400))];
    let bytes: Vec<(&str, Vec<u8>)> = files.iter().map(|(p, t)| (*p, t.clone().into_bytes())).collect();
    let refs: Vec<(&str, &[u8])> = bytes.iter().map(|(p, b)| (*p, b.as_slice())).collect();
    let d = Dir::new(&refs);
    let decls = lanes(&[("kick", "hits", LaneKind::Notes), ("bass", "notes", LaneKind::Notes), ("bass", "cutoff", hz())]);
    let (mut accepted, _) = d.project().data(&decls, None);
    let mut rng: u64 = 0x5eed;
    let mut next = |n: u64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng % n
    };
    let mut reused = 0;
    for case in 0..300 {
        let (path, _) = &files[next(files.len() as u64) as usize];
        let text = match next(6) {
            0 => "{\"clips\": [".to_string(),
            1 if path.contains("cutoff") => control(["hz", "db", "gain"][next(3) as usize], next(900)),
            2 => {
                std::fs::remove_file(d.0.join(path)).ok();
                String::new()
            }
            _ if path.contains("cutoff") => control("hz", next(900)),
            _ => notes(next(50), 1 + next(4)),
        };
        if !text.is_empty() {
            d.write(path, text.as_bytes());
        }
        let p = d.project();
        let (kept, kept_diags) = p.data(&decls, Some(&accepted));
        // The same accepted data, with nothing remembered of what it was read from.
        let fresh_previous = DataSet { read: BTreeMap::new(), ..accepted.clone() };
        let (fresh, fresh_diags) = p.data(&decls, Some(&fresh_previous));
        let lanes = |x: &DataSet| x.lanes().map(|(a, d)| (a.clone(), (**d).clone())).collect::<Vec<_>>();
        assert_eq!(lanes(&kept), lanes(&fresh), "case {case}: the data");
        assert_eq!(kept_diags, fresh_diags, "case {case}: the problems, in order");
        assert_eq!(kept.end(), fresh.end(), "case {case}: the end");
        reused += kept.lanes().filter(|(a, d)| accepted.lanes.get(*a).is_some_and(|x| Arc::ptr_eq(x, d))).count();
        accepted = kept;
    }
    assert!(reused > 300, "unchanged lanes were kept, not read again: {reused}");
}

// PLANE-1, FMT-15 (the G11 fix's A.2, A.5): what a read reuses from the data read before is the
// same as reading anew. A lane whose file is the text read before keeps that data, shared; one
// whose file changed is read from it. The assets clips may play are listed again exactly when
// the binaries changed, so an asset added outside plays once taken in.
#[test]
fn a_read_reuses_only_what_its_files_are() {
    let audio = |asset: &str| format!(r#"{{"clips": [{{"id": "a", "start": 0, "length": 4, "asset": "{asset}"}}]}}"#);
    let notes = r#"{"clips": [{"id": "n", "start": 0, "length": 4, "notes": []}]}"#;
    let d =
        Dir::new(&[("data/vox/takes.json", audio("a.wav").as_bytes()), ("data/bass/notes.json", notes.as_bytes()), ("assets/a.wav", b"a")]);
    let decls = lanes(&[("vox", "takes", LaneKind::Audio), ("bass", "notes", LaneKind::Notes)]);
    let snap = Snapshot::read(&d.0).unwrap();
    let (first, _) = Project::from_snapshot(&snap).unwrap().data(&decls, None);
    assert!(first.parsed(&addr("bass", "notes"), &LaneKind::Notes, notes).is_some());
    assert!(first.parsed(&addr("bass", "notes"), &LaneKind::Audio, notes).is_none(), "another kind is another read");
    assert!(first.parsed(&addr("bass", "notes"), &LaneKind::Notes, r#"{"clips": []}"#).is_none(), "another text is another read");

    // A lane edited: the other lane and the assets are shared; the edited one is read from its file.
    let mut edited = snap.clone();
    let moved = notes.replace("\"start\": 0", "\"start\": 8");
    edited.files.insert(PathBuf::from("data/bass/notes.json"), moved.as_str().into());
    let (second, _) = Project::from_snapshot(&edited).unwrap().data(&decls, Some(&first));
    assert!(Arc::ptr_eq(second.lane("vox", "takes").unwrap(), first.lane("vox", "takes").unwrap()));
    assert_eq!(**second.lane("bass", "notes").unwrap(), LaneData::read(&moved, &LaneKind::Notes).unwrap());
    assert!(Arc::ptr_eq(&second.assets, &first.assets), "the binaries are the same map");

    // An asset added outside, and named: listed again, so it plays.
    d.write("assets/b.wav", b"b");
    d.write("data/vox/takes.json", audio("b.wav").as_bytes());
    let outside = Snapshot::read(&d.0).unwrap();
    let (third, _) = Project::from_snapshot(&outside).unwrap().data(&decls, Some(&second));
    assert!(third.asset_path("b.wav").is_some() && second.asset_path("b.wav").is_none());
    assert!(!Arc::ptr_eq(&third.assets, &second.assets));

    // A seed is taken as the read of its text, and only of that text.
    let seed = Arc::new(LaneData::read(notes, &LaneKind::Notes).unwrap());
    let seeded = second.seeded(&addr("bass", "notes"), &LaneKind::Notes, Arc::from(notes), seed.clone());
    let (fourth, _) = Project::from_snapshot(&snap).unwrap().data(&decls, Some(&seeded));
    assert!(Arc::ptr_eq(fourth.lane("bass", "notes").unwrap(), &seed));
}

// FMT-5 (the G11 fix's A.4): the run of paths at or inside one path is exactly the paths that start
// with it by component, whatever the names around it: one that only starts with the same letters,
// a dot, a space, a digit, or `assets` itself.
#[test]
fn the_paths_under_a_path_are_one_run() {
    let names = [
        "assets",
        "assets0",
        "assets.wav",
        "asset",
        "assets/a.wav",
        "assets/vox/b.wav",
        "assets/vox",
        "assetsz/x",
        "data",
        "data/kick",
        "data/kick/hits.json",
        "data/kick-2/hits.json",
        "data/kicks/x.json",
        "data/kick/sub/a.state",
        "data/kick.json",
        "data/ kick",
        "song.ts",
        "a b/c",
        "Assets/x",
        "assets/z",
        "assets/vox-2/c.wav",
    ];
    let map: BTreeMap<PathBuf, usize> = names.iter().enumerate().map(|(i, n)| (PathBuf::from(n), i)).collect();
    let set: BTreeSet<PathBuf> = map.keys().cloned().collect();
    for p in names.iter().copied().chain(["", "assets/", "data/kick/", "nothing", "data/kick/hits.json/x"]) {
        let p = Path::new(p);
        let want: Vec<&PathBuf> = map.keys().filter(|k| k.starts_with(p)).collect();
        assert_eq!(crate::under(&map, p).map(|(k, _)| k).collect::<Vec<_>>(), want, "{}", p.display());
        assert_eq!(crate::under_set(&set, p).collect::<Vec<_>>(), want, "{}", p.display());
    }
}

// RENDER-27a: a tempo drag's data is the same lanes, each shared, in the new arrangement's tempo
// map; the data it came from is as it was.
#[test]
fn with_arrangement_keeps_every_lane() {
    let notes = LaneData::Notes(vec![]);
    let control = LaneData::Control { unit: None, clips: vec![] };
    let data = DataSet::with_lanes(Arrangement::default(), [(addr("bass", "notes"), notes), (addr("bass", "cutoff"), control)]);
    let mut faster = Arrangement::default();
    faster.tempo[0].bpm = 150.0;
    let dragged = data.with_arrangement(Arc::new(faster.clone()));
    assert_eq!(*dragged.arrangement, faster);
    assert_eq!(dragged.tempo().bpm_at(0.0), 150.0);
    assert_eq!(data.tempo().bpm_at(0.0), 120.0, "the data it came from keeps its tempo");
    let both: Vec<_> = data.lanes().zip(dragged.lanes()).collect();
    assert_eq!(both.len(), 2);
    assert!(both.iter().all(|((a, x), (b, y))| a == b && Arc::ptr_eq(x, y)), "every lane shared");
}

// PACK-5: a package's file resolves inside that package's folder only, byte for byte, and only for a
// package the code was built with; the revision records its hash beside the song's assets.
#[test]
fn package_assets_resolve_inside_their_package() {
    let d = Dir::new(&[("assets/kick.wav", b"kick"), ("pack/samples/C4.wav", b"c4"), ("pack/m2pkg.toml", b"x"), ("secret.wav", b"no")]);
    let pack = d.0.join("pack");
    let read = |names: &[&str]| {
        let assets = names.iter().map(|n| AssetDecl { module: key("keys"), name: n.to_string() }).collect();
        let packages = BTreeMap::from([("@alice/felt-piano".to_string(), pack.clone())]);
        d.project().data(&Decls { assets, packages, ..Decls::default() }, None)
    };
    let (data, diags) = read(&["kick.wav", "@alice/felt-piano:samples/C4.wav"]);
    assert!(diags.is_empty(), "{diags:?}");
    assert_eq!(data.kernel_assets().get("@alice/felt-piano:samples/C4.wav"), Some(&Sha::of(b"c4")), "the revision records its hash");
    assert_eq!(data.kernel_assets().get("kick.wav"), Some(&Sha::of(b"kick")));
    assert_eq!(data.asset_path("@alice/felt-piano:samples/C4.wav"), Some(pack.join("samples/C4.wav")));
    for bad in [
        "@alice/felt-piano:samples/c4.wav",
        "@alice/felt-piano:samples/../../secret.wav",
        "@alice/felt-piano:samples/none.wav",
        "@alice/felt-piano:samples",
        "@bob/other:samples/C4.wav",
        "std:samples/C4.wav",
    ] {
        let (data, diags) = read(&[bad]);
        assert_eq!(diags.iter().map(|d| d.code).collect::<Vec<_>>(), ["asset.missing"], "{bad}");
        assert!(diags[0].message.contains(bad) && diags[0].module.as_deref() == Some("keys"), "{}", diags[0].message);
        assert!(data.asset_path(bad).is_none() && data.kernel_assets().is_empty(), "{bad} never plays");
    }
}

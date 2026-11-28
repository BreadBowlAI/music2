//! `m2 export <song> --web <folder>` (WEB-1 to WEB-6): the web bundle a page plays a song from,
//! the refusals at export, and the samples' credits. Each in a scratch folder, with a HOME of its
//! own (PLAT-7).

mod app;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Output;

use app::{Temp, copy, said};
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `m2 args…` with the tests' home, which holds the packs this tree bundles.
fn m2(args: &[&str]) -> Output {
    app::m2_in(&m2_song::pack::test_home(), args, &[])
}

/// `m2 export <song> --web <out>`, which must succeed: what it said.
fn export(song: &Path, out: &Path) -> String {
    let o = m2(&["export", song.to_str().unwrap(), "--web", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", said(&o));
    said(&o)
}

/// `m2 export`, which must be refused with exit 1: what it said.
fn refused(o: Output) -> String {
    assert_eq!(o.status.code(), Some(1), "{}", said(&o));
    said(&o)
}

/// Every file under `dir`, by its path there, `/`-separated, with its bytes.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            } else {
                let path = e.path();
                let rel: Vec<String> =
                    path.strip_prefix(dir).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                out.insert(rel.join("/"), std::fs::read(&path).unwrap());
            }
        }
    }
    out
}

/// The version of the pack in `packs/<name>` (or std) this tree bundles.
fn version(dir: &str) -> String {
    let toml = std::fs::read_to_string(repo().join(dir).join("m2pkg.toml")).unwrap();
    let line = toml.lines().find_map(|l| l.strip_prefix("version = \"")).unwrap();
    line.trim_end_matches('"').to_string()
}

/// A copy of the starter template, as `m2 new` makes it, at `dir`.
fn starter(dir: &Path) {
    let o = m2(&["new", dir.to_str().unwrap()]);
    assert!(o.status.success(), "{}", said(&o));
}

// WEB-1, WEB-2, WEB-3: the starter's bundle is the folder the note shows: the IR, the controls, the
// song's manifest, lock and data files as they are, and std's, the drums' and the effects' kernels
// (not the synths', whose sounds are std's kernels), with each package's listing; no code. The
// manifest names every other file with its size and hash, each right, and its own hash is printed.
#[test]
fn the_starters_bundle_is_its_layout_with_each_hash_right() {
    let t = Temp::new("export-starter");
    let song = t.0.join("starter");
    starter(&song);
    let out = t.0.join("starter-web");
    let printed = export(&song, &out);

    let got = files(&out);
    let (std, drums, effects) = (version("std"), version("packs/@m2/drums"), version("packs/@m2/effects"));
    let mut want: Vec<String> = [
        "m2web.json",
        "ir.json",
        "controls.json",
        "music2.toml",
        "music2.lock",
        "data/arrangement.json",
        "data/bass/notes.json",
        "data/chords/notes.json",
        "data/hook/notes.json",
        "data/clap/hits.json",
        "data/hats/hits.json",
        "data/kick/hits.json",
    ]
    .map(String::from)
    .into();
    for (at, wasm) in
        [(format!("std/{std}"), "std"), (format!("@m2/drums/{drums}"), "drums"), (format!("@m2/effects/{effects}"), "effects")]
    {
        want.extend([format!("packs/{at}/m2pkg.toml"), format!("packs/{at}/m2pack.json"), format!("packs/{at}/kernels/{wasm}.wasm")]);
    }
    want.sort();
    assert_eq!(got.keys().cloned().collect::<Vec<_>>(), want);

    // As they are: the song's files, and the packages' as music2 ships them.
    for f in ["music2.toml", "music2.lock", "data/arrangement.json", "data/bass/notes.json", "data/kick/hits.json"] {
        assert!(got[f] == std::fs::read(song.join(f)).unwrap(), "{f} isn't the song's");
    }
    assert!(got[&format!("packs/std/{std}/kernels/std.wasm")] == std::fs::read(repo().join("std/kernels/std.wasm")).unwrap());
    assert!(got[&format!("packs/std/{std}/m2pkg.toml")] == std::fs::read(repo().join("std/m2pkg.toml")).unwrap());
    assert!(
        got[&format!("packs/@m2/drums/{drums}/kernels/drums.wasm")]
            == std::fs::read(repo().join("packs/@m2/drums/kernels/drums.wasm")).unwrap()
    );
    // std's listing is the one its record holds (PACK-17a); a pack's, the one its lock names.
    let std_listing = &got[&format!("packs/std/{std}/m2pack.json")];
    let std_listing = m2_song::Sha::of(std_listing);
    let v = m2_song::Version::parse(&std).unwrap();
    assert!(m2_song::pack::own(&m2_song::pack::PackName::new("std").unwrap(), &v, &std_listing));
    let lock = String::from_utf8(got["music2.lock"].clone()).unwrap();
    assert!(lock.contains(&m2_song::Sha::of(&got[&format!("packs/@m2/drums/{drums}/m2pack.json")]).prefixed()), "{lock}");

    // The IR is `m2 check --json`'s.
    let check = m2(&["check", song.to_str().unwrap(), "--json"]);
    assert!(check.status.success(), "{}", said(&check));
    let ir: Value = serde_json::from_slice(&got["ir.json"]).unwrap();
    assert_eq!(ir, serde_json::from_slice::<Value>(&check.stdout).unwrap());

    // The manifest: every other file, its size and hash right; what the song is.
    let m: Value = serde_json::from_slice(&got["m2web.json"]).unwrap();
    assert_eq!((m["format"].as_u64(), m["ir"].as_u64(), m["abi"].as_u64()), (Some(1), Some(2), Some(1)), "{m:#}");
    assert_eq!(m["music2"], env!("CARGO_PKG_VERSION"));
    assert_eq!((m["song"]["name"].as_str(), m["song"]["sample_rate"].as_u64()), (Some("starter"), Some(48000)), "{m:#}");
    assert!(m["song"]["seconds"].as_f64().is_some_and(|s| s > 10.0), "{m:#}");
    let listed: Vec<&str> = m["files"].as_array().unwrap().iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_eq!(listed, want.iter().filter(|p| *p != "m2web.json").map(String::as_str).collect::<Vec<_>>());
    for f in m["files"].as_array().unwrap() {
        let bytes = &got[f["path"].as_str().unwrap()];
        assert_eq!(f["size"].as_u64(), Some(bytes.len() as u64), "{f}");
        assert_eq!(f["sha256"].as_str(), Some(m2_song::Sha::of(bytes).prefixed().as_str()), "{f}");
    }
    let packs: Vec<(&str, &str)> =
        m["packs"].as_array().unwrap().iter().map(|p| (p["name"].as_str().unwrap(), p["version"].as_str().unwrap())).collect();
    assert_eq!(packs, [("@m2/drums", drums.as_str()), ("@m2/effects", effects.as_str()), ("std", std.as_str())]);
    assert_eq!(m["packs"][2]["hash"].as_str(), Some(std_listing.prefixed().as_str()));
    assert_eq!(m["credits"], serde_json::json!([]), "the starter plays no samples");
    let tracks: Vec<&str> = m["tracks"].as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert_eq!(tracks, ["bass", "chords", "clap", "hats", "hook", "kick"], "{m:#}");
    let bass = &m["tracks"][0];
    assert_eq!((&bass["label"], &bass["feed"], &bass["lanes"][0]["kind"]), (&"Bass".into(), &"notes".into(), &"notes".into()), "{bass}");

    // The controls are `m2 controls`'s, each with where it is in the IR.
    let controls: Value = serde_json::from_slice(&got["controls.json"]).unwrap();
    let listed = m2(&["controls", song.to_str().unwrap()]);
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    let ids: Vec<&str> = text.lines().skip(1).map(|l| l.split_whitespace().next().unwrap()).collect();
    let bundled: Vec<&str> = controls["controls"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert!(!bundled.is_empty() && bundled == ids, "{bundled:?} against {ids:?}");
    assert!(controls["controls"].as_array().unwrap().iter().all(|c| c["origin"]["site"].is_u64() && c["value"].is_f64()), "{controls:#}");

    // The manifest's own hash, for a page's `integrity`.
    let integrity =
        printed.lines().find_map(|l| l.split_whitespace().find(|w| w.starts_with("sha256-"))).unwrap_or_else(|| panic!("{printed}"));
    assert_eq!(integrity, m2_editor::web::integrity(&got["m2web.json"]), "{printed}");
}

// WEB-2: the same song exports to the same bytes, from another folder and again: no date, no path.
#[test]
fn exporting_twice_gives_the_same_bytes() {
    let t = Temp::new("export-twice");
    let (a, b) = (t.0.join("one/starter"), t.0.join("two/elsewhere/starter"));
    starter(&a);
    copy(&a, &b);
    export(&a, &t.0.join("a-web"));
    export(&b, &t.0.join("b-web"));
    export(&a, &t.0.join("again-web"));
    let first = files(&t.0.join("a-web"));
    assert!(first == files(&t.0.join("b-web")), "another folder exports other bytes");
    assert!(first == files(&t.0.join("again-web")), "the same song exports other bytes");
    let home = m2_song::pack::test_home();
    for (path, bytes) in &first {
        let text = String::from_utf8_lossy(bytes);
        for machine in [t.0.to_string_lossy(), home.to_string_lossy()] {
            assert!(!text.contains(&*machine), "{path} names {machine}");
        }
    }
}

// WEB-5: a song with a plugin is refused, naming it and its track, before anything is written.
#[test]
fn a_plugin_song_is_refused_in_words() {
    let t = Temp::new("export-plugin");
    let song = t.0.join("with-plugins");
    copy(&app::example("with-plugins"), &song);
    let out = t.0.join("web");
    let said = refused(m2(&["export", song.to_str().unwrap(), "--web", out.to_str().unwrap()]));
    assert!(said.contains("This song plays plugins ("), "{said}");
    assert!(
        said.contains(", on the track Lead). A web page can't load plugins. In a copy of the song (Save As), bounce the track to audio (Bounce to audio) and delete the track with the plugins, then export the copy."),
        "{said}"
    );
    assert!(!out.exists(), "nothing is written");
}

/// A copy of the fixtures' pack `tiny` at 0.2.0 with a kernel of its own, `@test/tiny:sine`
/// (std's module under its name, with a section of its own), played by `tinyTone`.
fn tiny_with_kernel(t: &Temp) -> PathBuf {
    let dir = t.0.join("tiny-0.2.0");
    copy(&repo().join("core/session/tests/fixtures/packs/tiny"), &dir);
    let _ = std::fs::remove_file(dir.join("m2pack.json"));
    let toml =
        std::fs::read_to_string(dir.join("m2pkg.toml")).unwrap().replace("version = \"0.1.0\"", "version = \"0.2.0\"\nwasm = \"k.wasm\"");
    let toml = toml
        + "\n[[kernel]]\nname = \"sine\"\ndescription = \"A sine\"\ninputs = [\"freq: control<hz> = 440\"]\noutputs = [\"out: audio<1>\"]\n";
    std::fs::write(dir.join("m2pkg.toml"), toml).unwrap();
    let mut wasm = std::fs::read(repo().join("std/kernels/std.wasm")).unwrap();
    wasm.extend([0u8, 3, 2, b'm', b'2']);
    std::fs::write(dir.join("k.wasm"), &wasm).unwrap();
    let index = std::fs::read_to_string(dir.join("src/index.ts")).unwrap();
    let tone = "\nimport { hz } from \"@m2/core\";\n\n/** A sine of the pack's own kernel. */\nexport const tinyTone = component(\n  {\n    \
                id: \"tiny-tone\",\n    label: \"Tiny tone\",\n    signature: signature<(notes: Events) => Audio>(),\n    \
                inputs: { notes: port.notes() },\n    output: port.audio(),\n    editor: { category: \"Keys\", description: \"A sine\" },\n  },\n  \
                (_notes: Events): Audio => kernel(\"@test/tiny:sine\", { freq: hz(440) }),\n);\n";
    std::fs::write(dir.join("src/index.ts"), index + tone).unwrap();
    dir
}

// WEB-4: a song that plays a kernel of a pack that isn't music2's own is refused in the note's
// words, before anything is written.
#[test]
fn a_strangers_pack_is_refused_in_words() {
    let t = Temp::new("export-stranger");
    let home = t.0.join("home");
    let run = |args: &[&str]| app::m2_in(&home, args, &[]);
    let o = run(&["pack", "install", tiny_with_kernel(&t).to_str().unwrap()]);
    assert!(o.status.success(), "{}", said(&o));
    let song = t.0.join("song");
    copy(&repo().join("core/session/tests/fixtures/packs/song-store"), &song);
    let manifest =
        std::fs::read_to_string(song.join("music2.toml")).unwrap().replace("\"@test/tiny\" = \"0.1\"", "\"@test/tiny\" = \"0.2\"");
    std::fs::write(song.join("music2.toml"), manifest).unwrap();
    std::fs::remove_file(song.join("music2.lock")).unwrap();
    let keys = "import { track } from \"@m2/core\";\nimport { tinyTone } from \"@test/tiny\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => tinyTone(lane.notes(\"notes\")));\n";
    std::fs::write(song.join("tracks/keys.ts"), keys).unwrap();
    std::fs::write(song.join("tracks/pluck.ts"), keys.replace("\"keys\", { name: \"Keys\" }", "\"pluck\", { name: \"Pluck\" }")).unwrap();
    let o = run(&["pack", "lock", song.to_str().unwrap()]);
    assert!(o.status.success(), "{}", said(&o));
    let out = t.0.join("web");
    let said = refused(run(&["export", song.to_str().unwrap(), "--web", out.to_str().unwrap()]));
    assert!(
        said.contains(
            "This song plays @test/tiny, which isn't one of music2's own packs. A web bundle can hold only std and music2's own packs \
             until the web has a time limit for other packs' code."
        ),
        "{said}"
    );
    assert!(!out.exists(), "nothing is written");
}

// WEB-4, PACK-17a: a std whose module isn't the one its record holds is refused: a page couldn't
// tell it from a stranger's module.
#[test]
fn a_changed_std_is_refused() {
    let t = Temp::new("export-std");
    let song = t.0.join("starter");
    starter(&song);
    let std = t.0.join("std");
    std::fs::create_dir_all(&std).unwrap();
    for part in ["m2pkg.toml", "kernels", "src", "samples", "samples.toml"] {
        let from = repo().join("std").join(part);
        if from.is_dir() { copy(&from, &std.join(part)) } else { std::fs::copy(&from, std.join(part)).map(drop).unwrap() }
    }
    let mut wasm = std::fs::read(std.join("kernels/std.wasm")).unwrap();
    wasm.extend([0u8, 2, 1, b'x']);
    std::fs::write(std.join("kernels/std.wasm"), wasm).unwrap();
    let out = t.0.join("web");
    let env = [("M2_STD", std.to_str().unwrap())];
    // It builds and plays here: only a page can't trust it.
    let o = app::m2_in(&m2_song::pack::test_home(), &["check", song.to_str().unwrap()], &env);
    assert!(o.status.success(), "{}", said(&o));
    let said = refused(app::m2_in(&m2_song::pack::test_home(), &["export", song.to_str().unwrap(), "--web", out.to_str().unwrap()], &env));
    assert!(said.contains("isn't std as music2 ships it"), "{said}");
    assert!(!out.exists(), "nothing is written");
}

// WEB-5: the folder must be new, or empty, and outside the song: else refused, and nothing written.
#[test]
fn the_folder_must_be_new_empty_and_outside_the_song() {
    let t = Temp::new("export-folder");
    let song = t.0.join("starter");
    starter(&song);
    let s = song.to_str().unwrap();
    let inside = song.join("web");
    let said = refused(m2(&["export", s, "--web", inside.to_str().unwrap()]));
    assert!(said.contains("Export to a new, empty folder outside the song."), "{said}");
    assert!(!inside.exists());
    let said = refused(m2(&["export", s, "--web", s]));
    assert!(said.contains("Export to a new, empty folder outside the song."), "{said}");
    let full = t.0.join("full");
    std::fs::create_dir_all(&full).unwrap();
    std::fs::write(full.join("index.html"), "<p>mine</p>").unwrap();
    let said = refused(m2(&["export", s, "--web", full.to_str().unwrap()]));
    assert!(said.contains("Export to a new, empty folder outside the song."), "{said}");
    assert_eq!(files(&full).len(), 1, "what was there stays, alone");
    // An empty folder that is there takes it.
    let empty = t.0.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    export(&song, &empty);
    assert!(empty.join("m2web.json").is_file());
}

// WEB-6: samples go in with their notes, and export says that anyone can download them, with each
// one's credit: a pack's sample by its pack, version, author and licence; a bounce of yours with no
// licence on record. The manifest carries the credits.
#[test]
fn samples_are_credited_and_said() {
    let t = Temp::new("export-samples");
    let song = t.0.join("resample");
    copy(&app::example("resample"), &song);
    let o = m2(&["add-sample", song.to_str().unwrap(), "sample:std:samples/kit/kick.wav"]);
    assert!(o.status.success(), "{}", said(&o));
    let out = t.0.join("web");
    let printed = export(&song, &out);
    assert!(
        printed.contains("2 samples go into the bundle, and anyone who opens the page can download them. Their licences:"),
        "{printed}"
    );
    let std = version("std");
    assert!(printed.contains(&format!("assets/kick.wav: std {std}, by music2, licence MIT OR Apache-2.0")), "{printed}");
    assert!(printed.contains("assets/keys-bounce.wav: no licence on record (your own)"), "{printed}");
    let got = files(&out);
    for f in ["assets/kick.wav", "assets/kick.wav.json", "assets/keys-bounce.wav", "assets/keys-bounce.wav.json"] {
        assert!(got[f] == std::fs::read(song.join(f)).unwrap(), "{f} isn't the song's");
    }
    let m: Value = serde_json::from_slice(&got["m2web.json"]).unwrap();
    let credits = m["credits"].as_array().unwrap();
    assert_eq!(credits.len(), 2, "{m:#}");
    assert_eq!((&credits[0]["path"], &credits[0]["from"]), (&"assets/keys-bounce.wav".into(), &Value::Null), "{m:#}");
    assert_eq!(credits[1]["path"], "assets/kick.wav");
    assert_eq!((&credits[1]["from"]["pack"], &credits[1]["from"]["license"]), (&"std".into(), &"MIT OR Apache-2.0".into()), "{m:#}");
}

use super::*;
use crate::testing::{Fixture, files_under, fixture};

/// A manifest asking for `deps`, as a song made then wrote it.
fn manifest(deps: &str) -> String {
    format!("[project]\nname = \"old\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n{deps}")
}

/// Every file of the song at `f`, by its path, as bytes.
fn bytes(f: &Fixture) -> Vec<(PathBuf, Vec<u8>)> {
    let mut all: Vec<(PathBuf, Vec<u8>)> = files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().to_path_buf(), std::fs::read(&p).unwrap()))
        .collect();
    all.sort();
    all
}

/// Folder `from`, copied to `to`.
fn copy_dir(from: &Path, to: &Path) {
    for file in files_under(from) {
        let at = to.join(file.strip_prefix(from).unwrap());
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::copy(&file, at).unwrap();
    }
}

/// The song's root module, with tracks `names` mixed: the song file of every song here.
fn song_of(names: &[&str]) -> String {
    let imports: String = names.iter().map(|n| format!("import {n} from \"./tracks/{n}\";\n")).collect();
    let mix: Vec<String> = names.iter().map(|n| format!("{n}: {n}.out")).collect();
    format!(
        "import {{ song }} from \"@m2/core\";\nimport {{ master, mixer }} from \"std/mix\";\n{imports}\nexport default song(() => master(mixer({{ {} }})));\n",
        mix.join(", ")
    )
}

// FMT-25: an old song's std line, @m2/core line and imports of what left std move to today's,
// the packs it now plays are listed and locked, and the song builds: in one step.
#[test]
fn an_old_songs_lines_and_imports_move_to_the_packs() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.5\" # the std it was made with\n"));
    f.write("song.ts", song_of(&["keys", "lead"]));
    f.write(
        "tracks/keys.ts",
        "import { track } from \"@m2/core\";\nimport { chorus, duck } from \"std/fx\";\nimport { pluck } from \"std/instruments\";\n\n\
         export default track(\"keys\", ({ lane }) => duck(chorus(pluck(lane.notes(\"n\")), { mix: 0.3 }), lane.mask(\"m\")));\n",
    );
    f.write(
        "tracks/lead.ts",
        "import { ms, track } from \"@m2/core\";\nimport { mono, synth } from \"std/synth\";\nimport { fm } from \"std/fm\";\n\n\
         // The hook.\nexport default track(\"lead\", ({ lane }) => synth.lead(lane.notes(\"n\"), { glide: ms(70) }));\n\
         export const bell = fm.bell;\nexport const one = mono;\n",
    );
    let mut s = f.open();
    assert!(!s.draft_errors.is_empty(), "made for std 0.5, it doesn't build as it is");
    assert!(needed(&s), "its manifest asks for an older music2");
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.left.is_empty(), "{:?}", up.left);
    assert!(up.refused.is_empty(), "{:?}", up.refused);
    assert!(up.written.is_some(), "written");
    assert_eq!(
        f.read("music2.toml"),
        manifest("\"@m2/core\" = \"2\"\nstd = \"0.9\" # the std it was made with\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n")
    );
    assert_eq!(
        f.read("tracks/keys.ts"),
        "import { track } from \"@m2/core\";\nimport { duck } from \"std/fx\";\nimport { chorus } from \"@m2/effects\";\nimport { pluck } from \"@m2/synths\";\n\n\
         export default track(\"keys\", ({ lane }) => duck(chorus(pluck(lane.notes(\"n\")), { mix: 0.3 }), lane.mask(\"m\")));\n"
    );
    assert_eq!(
        f.read("tracks/lead.ts"),
        "import { ms, track } from \"@m2/core\";\nimport { mono } from \"std/dsp\";\nimport { synth } from \"@m2/synths\";\nimport { fm } from \"@m2/synths\";\n\n\
         // The hook.\nexport default track(\"lead\", ({ lane }) => synth.lead(lane.notes(\"n\"), { glide: ms(70) }));\n\
         export const bell = fm.bell;\nexport const one = mono;\n"
    );
    let lock = m2_song::pack::Lock::read(&f.read("music2.lock")).unwrap();
    let locked: Vec<(String, String)> = lock.packs.iter().map(|p| (p.name.to_string(), p.version.to_string())).collect();
    let now = |n: &str| m2_song::pack::comes_with(&PackName::new(n).unwrap()).unwrap().to_string();
    assert_eq!(locked, [("@m2/effects".into(), now("@m2/effects")), ("@m2/synths".into(), now("@m2/synths"))]);
    assert!(s.draft_errors.is_empty() && s.accepted().has_code(), "it builds: {:?}", s.draft_errors);
    assert!(!needed(&s), "made for this music2 now");
    // What it says it changed is each file it wrote, the lines before and after.
    let files: Vec<&str> = up.changes.iter().map(|c| c.file.as_str()).collect();
    assert_eq!(files, ["music2.toml", "music2.lock", "tracks/keys.ts", "tracks/lead.ts"]);
    let keys = &up.changes[2].lines;
    assert!(keys.iter().any(|l| l.contains("- import { chorus, duck } from \"std/fx\";")), "{keys:#?}");
    assert!(keys.iter().any(|l| l.contains("+ import { chorus } from \"@m2/effects\";")), "{keys:#?}");
}

// FMT-25: the nine single drums, and the simple kick and hat, are One drum, each knob converted as
// One drum reads it; a unit no call reads any more leaves its import.
#[test]
fn the_single_drums_are_one_drum_with_their_knobs_converted() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n\"@m2/synths\" = \"1\"\n"));
    f.write("song.ts", song_of(&["perc", "simple"]));
    f.write(
        "tracks/perc.ts",
        "import { hz, ms, secs, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"perc\", ({ lane }) =>\n  mixer({\n\
         \x20   kick: drums.kick(lane.notes(\"kick\"), { tune: hz(50), punch: 0.6 }),\n\
         \x20   boom: drums.kick(lane.notes(\"boom\"), { decay: secs(1.4), punch: 0.3 }),\n\
         \x20   roll: drums.snare(lane.notes(\"roll\"), { snap: 0.75 }),\n\
         \x20   clap: drums.clap(lane.notes(\"clap\")),\n\
         \x20   hats: drums.hats(lane.notes(\"hats\"), { decay: ms(30) /* tight */ }),\n\
         \x20   rim: drums.rim(lane.notes(\"rim\"), { tone: 1700 }),\n\
         \x20   kit: drums.kit(lane.notes(\"beat\")),\n  }),\n);\n",
    );
    f.write(
        "tracks/simple.ts",
        "import { ms, track } from \"@m2/core\";\nimport { hat, kick } from \"@m2/synths\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"simple\", ({ lane }) => mixer({ k: kick(lane.notes(\"k\")), h: hat(lane.notes(\"h\"), { decay: ms(90) }) }));\n",
    );
    let mut s = f.open();
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.left.is_empty() && up.refused.is_empty(), "{:?} {:?}", up.left, up.refused);
    assert_eq!(
        f.read("tracks/perc.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"perc\", ({ lane }) =>\n  mixer({\n\
         \x20   kick: drums.drum(lane.notes(\"kick\"), { tune: 0.707, punch: 0.1 }),\n\
         \x20   boom: drums.drum(lane.notes(\"boom\"), { decay: 3.111, punch: -0.2 }),\n\
         \x20   roll: drums.drum(lane.notes(\"roll\"), { drum: \"snare\", punch: 0.15 }),\n\
         \x20   clap: drums.drum(lane.notes(\"clap\"), { drum: \"clap\" }),\n\
         \x20   hats: drums.drum(lane.notes(\"hats\"), { drum: \"hats\", decay: 0.667 /* tight */ }),\n\
         \x20   rim: drums.drum(lane.notes(\"rim\"), { drum: \"rim\", tune: 0 }),\n\
         \x20   kit: drums.kit(lane.notes(\"beat\")),\n  }),\n);\n"
    );
    assert_eq!(
        f.read("tracks/simple.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"simple\", ({ lane }) => mixer({ k: drums.drum(lane.notes(\"k\")), h: drums.drum(lane.notes(\"h\"), { drum: \"hats\", decay: 2 }) }));\n"
    );
    assert!(f.read("music2.toml").ends_with("\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n\"@m2/synths\" = \"2\"\n"));
    assert!(s.draft_errors.is_empty() && s.accepted().has_code(), "it builds: {:?}", s.draft_errors);
}

// FMT-25: a drum knob a lane drives keeps its lane: the lane's unit and every point are converted
// as the knob is, in the same step.
#[test]
fn a_lane_that_drives_a_drum_knob_is_converted_with_it() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n"));
    f.write("song.ts", song_of(&["hats"]));
    f.write(
        "tracks/hats.ts",
        "import { ms, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"hats\", ({ lane }) => drums.hats(lane.notes(\"n\"), { decay: lane.control(\"hats-decay\", \"seconds\", ms(45)) }));\n",
    );
    f.write(
        "data/hats/hats-decay.json",
        "{\"unit\": \"seconds\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 4, \"points\": [[0, 0.09], [4, 0.0225]]}\n]}\n",
    );
    let mut s = f.open();
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.left.is_empty() && up.refused.is_empty(), "{:?} {:?}", up.left, up.refused);
    assert_eq!(
        f.read("tracks/hats.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"hats\", ({ lane }) => drums.drum(lane.notes(\"n\"), { drum: \"hats\", decay: lane.control(\"hats-decay\", \"scalar\", 1) }));\n"
    );
    assert_eq!(
        f.read("data/hats/hats-decay.json"),
        "{\"unit\": \"scalar\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 4, \"points\": [[0, 2], [4, 0.5]]}\n]}\n"
    );
    assert!(up.changes.iter().any(|c| c.file == "data/hats/hats-decay.json"), "{:?}", up.changes);
    assert!(s.draft_errors.is_empty(), "it builds: {:?}", s.draft_errors);
}

// FMT-25: synth.bass's filter decay is `biteDecay` (its `decay` is the envelope's now); a moved
// kernel's id is its pack's, and the pack is listed.
#[test]
fn bass_decay_is_bite_decay_and_a_moved_kernel_is_its_packs() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/synths\" = \"1\"\n"));
    f.write("song.ts", song_of(&["bass", "raw"]));
    f.write(
        "tracks/bass.ts",
        "import { track } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\n\n\
         export default track(\"bass\", ({ lane }) => synth.bass(lane.notes(\"n\"), { decay: 0.3, bite: 2 }));\n",
    );
    f.write(
        "tracks/raw.ts",
        "import { kernel, poly, track } from \"@m2/core\";\n\n\
         export default track(\"raw\", ({ lane }) => poly(lane.notes(\"n\"), (n) => kernel(\"std:fm\", { pitch: n.pitch, gate: n.gate })));\n",
    );
    let mut s = f.open();
    let up = upgrade(&mut s, true).unwrap();
    assert!(up.left.is_empty(), "{:?}", up.left);
    assert!(f.read("tracks/bass.ts").contains("{ decay: 0.3, bite: 2 }"), "--check wrote nothing");
    let changed: Vec<&str> = up.changes.iter().map(|c| c.file.as_str()).collect();
    assert_eq!(changed, ["music2.toml", "music2.lock", "tracks/bass.ts", "tracks/raw.ts"]);
    assert!(up.changes[2].lines.iter().any(|l| l.contains("{ biteDecay: 0.3, bite: 2 }")), "{:?}", up.changes[2]);
    assert!(up.changes[3].lines.iter().any(|l| l.contains("kernel(\"@m2/synths:fm\"")), "{:?}", up.changes[3]);
}

// FMT-25: a call worked out by code is left as it is and named by its file and line, with the line
// to write; the song wouldn't build, so nothing is written.
#[test]
fn a_call_it_cant_convert_is_named_and_nothing_is_written() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n"));
    f.write("song.ts", song_of(&["perc"]));
    f.write(
        "tracks/perc.ts",
        "import { hz, track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         const tight = { tune: hz(200) };\n\
         export default track(\"perc\", ({ lane }) =>\n  mixer({ a: drums.snare(lane.notes(\"a\"), tight), b: drums.tom(lane.notes(\"b\")) }),\n);\n",
    );
    let before = bytes(&f);
    let mut s = f.open();
    let up = upgrade(&mut s, false).unwrap();
    assert_eq!(up.written, None);
    assert!(!up.refused.is_empty(), "it doesn't build with the call left");
    assert_eq!(up.left.len(), 1, "{:?}", up.left);
    let left = &up.left[0];
    assert_eq!((left.file.as_str(), left.line), ("tracks/perc.ts", 7));
    assert!(left.write.starts_with("drums.drum(lane.notes(\"a\"), { drum: \"snare\""), "{left:?}");
    assert_eq!(bytes(&f), before, "nothing written");
}

// FMT-25: a song made for this music2 is left as it is, and says so.
#[test]
fn a_current_song_is_left_alone() {
    let f = fixture("examples/first-song");
    let before = bytes(&f);
    let mut s = f.open();
    assert!(!needed(&s));
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.current(), "{:?} {:?}", up.changes, up.left);
    assert_eq!(up.written, None);
    assert_eq!(bytes(&f), before);
}

// FMT-25: one undo puts back every file the upgrade changed, byte for byte, and takes away the
// lock it made.
#[test]
fn one_undo_puts_every_file_back() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.4\"\n"));
    f.write("song.ts", song_of(&["kick"]));
    f.write(
        "tracks/kick.ts",
        "import { ms, track } from \"@m2/core\";\nimport { follow } from \"std/dsp\";\nimport { kick } from \"std/instruments\";\n\n\
         export default track(\"kick\", ({ lane }) => {\n  const out = kick(lane.notes(\"hits\"));\n  return { out, env: follow(out, { release: ms(150) }) };\n});\n",
    );
    let before = bytes(&f);
    let mut s = f.open();
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.written.is_some(), "{:?} {:?}", up.left, up.refused);
    assert!(f.read("tracks/kick.ts").contains("import { drums } from \"@m2/drums\";\n"), "{}", f.read("tracks/kick.ts"));
    assert_ne!(bytes(&f), before);
    s.undo().unwrap();
    assert_eq!(bytes(&f), before, "every file as it was, the lock gone");
}

/// An old song (std 0.4, the simple kick) in `f`: its files as bytes.
fn old_kick_song(f: &Fixture) -> Vec<(PathBuf, Vec<u8>)> {
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.4\"\n"));
    f.write("song.ts", song_of(&["kick"]));
    f.write(
        "tracks/kick.ts",
        "import { ms, track } from \"@m2/core\";\nimport { follow } from \"std/dsp\";\nimport { kick } from \"std/instruments\";\n\n\
         export default track(\"kick\", ({ lane }) => {\n  const out = kick(lane.notes(\"hits\"));\n  return { out, env: follow(out, { release: ms(150) }) };\n});\n",
    );
    bytes(f)
}

// FMT-25 (the review's G1): once an upgrade is undone, the old files don't build, so the song plays
// the upgraded revision still. Redo puts the upgrade back, and upgrading again writes it again:
// each lands on the revision that plays, so it builds, whatever the old files' errors.
#[test]
fn after_undo_redo_and_upgrading_again_each_write_the_upgrade() {
    let f = Fixture::new();
    let before = old_kick_song(&f);
    let mut s = f.open();
    assert!(upgrade(&mut s, false).unwrap().written.is_some());
    let upgraded = bytes(&f);
    s.undo().unwrap();
    assert_eq!(bytes(&f), before);
    assert!(!s.draft_errors.is_empty(), "the old files don't build");
    assert!(s.can_redo(), "redo lands on the revision that plays");
    s.redo().unwrap_or_else(|e| panic!("redo: {e:?}"));
    assert_eq!(bytes(&f), upgraded, "redo writes the upgrade back, byte for byte");
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    s.undo().unwrap();
    assert_eq!(bytes(&f), before);
    let again = upgrade(&mut s, false).unwrap();
    assert!(again.refused.is_empty() && again.left.is_empty(), "{:?} {:?}", again.refused, again.left);
    assert!(again.written.is_some(), "upgraded again");
    assert_eq!(bytes(&f), upgraded, "the same files as the first time");
    assert!(s.can_undo(), "one undo step again");
    s.undo().unwrap();
    assert_eq!(bytes(&f), before);
}

// FMT-25: a song locked to an older version of one of music2's own packs, which its store still
// holds, made for an older @m2/core, is locked to the version this music2 comes with: never kept
// on one this app can't run.
#[test]
fn an_older_own_pack_in_the_store_is_locked_to_the_one_music2_comes_with() {
    use m2_song::pack::{LISTING, Listing, Store};
    let f = Fixture::new();
    let store = f.0.join("Packs");
    for (dir, done) in m2_song::pack::install_bundled(&m2_song::pack::source_packs(), &Store::at(store.clone())) {
        done.unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    }
    let effects = PackName::new("@m2/effects").unwrap();
    let now = m2_song::pack::comes_with(&effects).unwrap();
    let old = Store::at(store.clone()).version_dir(&effects, &m2_song::Version(1, 1, 0));
    copy_dir(&m2_song::pack::source_packs().join("@m2/effects"), &old);
    let toml = std::fs::read_to_string(old.join("m2pkg.toml")).unwrap();
    let toml = toml.replace(&format!("version = \"{now}\""), "version = \"1.1.0\"").replace("\"@m2/core\" = \"2\"", "\"@m2/core\" = \"1\"");
    std::fs::write(old.join("m2pkg.toml"), toml).unwrap();
    let listing = Listing::of(&old).unwrap();
    std::fs::write(old.join(LISTING), listing.text()).unwrap();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/effects\" = \"1\"\n"));
    f.write("music2.lock", format!("[[pack]]\nname = \"@m2/effects\"\nversion = \"1.1.0\"\nhash = \"{}\"\n", listing.hash().prefixed()));
    f.write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { reverb } from \"@m2/effects\";\nimport { noise } from \"std/dsp\";\n\n\
         export default song(() => reverb(noise()));\n",
    );
    let mut packages = m2_runtime::PackageSet::default().with_store(store);
    packages.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    let mut s = m2_session::Session::open(&f.0, packages).unwrap();
    assert!(needed(&s));
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.refused.is_empty() && up.left.is_empty(), "{:?} {:?}", up.refused, up.left);
    let lock = m2_song::pack::Lock::read(&f.read("music2.lock")).unwrap();
    assert_eq!(lock.packs.iter().map(|p| (p.name.to_string(), p.version)).collect::<Vec<_>>(), [("@m2/effects".to_string(), now)]);
    assert!(s.draft_errors.is_empty(), "it builds: {:?}", s.draft_errors);
    assert!(!needed(&s));
}

// FMT-25: the renames are of what a song made before them meant. A song made for this music2 whose
// bass sets its `decay` (the envelope's, since wave 3) keeps it: the upgrade leaves it as it is.
#[test]
fn a_current_songs_bass_decay_is_its_envelopes_and_stays() {
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("song.ts", song_of(&["bass"]));
    f.write(
        "tracks/bass.ts",
        "import { track } from \"@m2/core\";\nimport { synth } from \"@m2/synths\";\n\n\
         export default track(\"bass\", ({ lane }) => synth.bass(lane.notes(\"n\"), { decay: 0.3 }));\n",
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let before = bytes(&f);
    let up = upgrade(&mut s, false).unwrap();
    assert!(up.changes.iter().all(|c| c.file != "tracks/bass.ts"), "{:?}", up.changes);
    assert!(up.left.is_empty(), "{:?}", up.left);
    assert_eq!(
        bytes(&f).into_iter().find(|(p, _)| p == Path::new("tracks/bass.ts")),
        before.into_iter().find(|(p, _)| p == Path::new("tracks/bass.ts"))
    );
}

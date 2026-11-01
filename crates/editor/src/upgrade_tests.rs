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
    let up = upgrade(&mut s, false, None).unwrap();
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
    let up = upgrade(&mut s, false, None).unwrap();
    assert!(up.left.is_empty() && up.refused.is_empty(), "{:?} {:?}", up.left, up.refused);
    assert_eq!(
        f.read("tracks/perc.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"perc\", ({ lane }) =>\n  mixer({\n\
         \x20   kick: drums.drum(lane.notes(\"kick\"), { tune: 0.706724, punch: 0.1, voices: 4 }),\n\
         \x20   boom: drums.drum(lane.notes(\"boom\"), { decay: 3.111111, punch: -0.2, voices: 4 }),\n\
         \x20   roll: drums.drum(lane.notes(\"roll\"), { drum: \"snare\", punch: 0.15, voices: 4 }),\n\
         \x20   clap: drums.drum(lane.notes(\"clap\"), { drum: \"clap\", voices: 4 }),\n\
         \x20   hats: drums.drum(lane.notes(\"hats\"), { drum: \"hats\", decay: 0.666667 /* tight */ }),\n\
         \x20   rim: drums.drum(lane.notes(\"rim\"), { drum: \"rim\", tune: 0, voices: 4 }),\n\
         \x20   kit: drums.kit(lane.notes(\"beat\")),\n  }),\n);\n"
    );
    assert_eq!(
        f.read("tracks/simple.ts"),
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"simple\", ({ lane }) => mixer({ k: drums.drum(lane.notes(\"k\")), h: drums.drum(lane.notes(\"h\"), { drum: \"hats\", decay: 2 }) }));\n"
    );
    assert!(f.read("music2.toml").ends_with("\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n\"@m2/synths\" = \"2\"\n"));
    assert!(s.draft_errors.is_empty() && s.accepted().has_code(), "it builds: {:?}", s.draft_errors);
    // The simple kick and hat were other sounds than One drum's: said, by file and line (G3).
    let said: Vec<(&str, u32)> = up.notes.iter().map(|n| (n.file.as_str(), n.line)).collect();
    assert_eq!(said, [("tracks/simple.ts", 5), ("tracks/simple.ts", 5)], "{:?}", up.notes);
    assert!(up.notes.iter().all(|n| n.what.contains("another sound")), "{:?}", up.notes);
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
    let up = upgrade(&mut s, false, None).unwrap();
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
    let up = upgrade(&mut s, true, None).unwrap();
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
    let up = upgrade(&mut s, false, None).unwrap();
    assert_eq!(up.written, None);
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
    let up = upgrade(&mut s, false, None).unwrap();
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
    let up = upgrade(&mut s, false, None).unwrap();
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
    assert!(upgrade(&mut s, false, None).unwrap().written.is_some());
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
    let again = upgrade(&mut s, false, None).unwrap();
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
    let up = upgrade(&mut s, false, None).unwrap();
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
    let up = upgrade(&mut s, false, None).unwrap();
    assert!(up.changes.iter().all(|c| c.file != "tracks/bass.ts"), "{:?}", up.changes);
    assert!(up.left.is_empty(), "{:?}", up.left);
    assert_eq!(
        bytes(&f).into_iter().find(|(p, _)| p == Path::new("tracks/bass.ts")),
        before.into_iter().find(|(p, _)| p == Path::new("tracks/bass.ts"))
    );
}

// The song at `f` after an upgrade that writes it: the upgrade, with nothing left.
fn upgraded(f: &Fixture) -> (Upgraded, m2_session::Session) {
    let mut s = f.open();
    let up = upgrade(&mut s, false, None).unwrap();
    assert!(up.left.is_empty() && up.refused.is_empty(), "{:?} {:?}", up.left, up.refused);
    assert!(up.written.is_some());
    assert!(s.draft_errors.is_empty(), "it builds: {:?}", s.draft_errors);
    (up, s)
}

/// An old song of one track, `perc`, whose code is `code` (after the imports of `@m2/core`'s
/// `track`, `hz`, `ms`, `secs` and `@m2/drums`' `drums`).
fn old_perc(f: &Fixture, code: &str) {
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/drums\" = \"1\"\n"));
    f.write("song.ts", song_of(&["perc"]));
    f.write("tracks/perc.ts", format!("import {{ hz, ms, secs, track }} from \"@m2/core\";\nimport {{ drums }} from \"@m2/drums\";\nimport {{ mixer }} from \"std/mix\";\n\n{code}"));
}

/// A notes lane's file of one clip, holding `pitches` one after another.
fn notes_of(pitches: &[&str]) -> String {
    let notes: Vec<String> = pitches
        .iter()
        .enumerate()
        .map(|(i, p)| format!("{{\"pitch\": \"{p}\", \"start\": {i}, \"length\": 0.25, \"velocity\": 0.8}}"))
        .collect();
    format!("{{\"clips\": [{{\"id\": \"c1\", \"start\": 0, \"length\": 16, \"notes\": [{}]}}]}}\n", notes.join(", "))
}

// FMT-25 (the review's G2): all or nothing. A lane it can't convert exactly (one lane that two
// knobs would read in different units, a length past what One drum plays) leaves the song as it
// is: nothing is written, and each is named with its file and line.
#[test]
fn a_lane_it_cant_convert_writes_nothing() {
    let f = Fixture::new();
    old_perc(
        &f,
        "export default track(\"perc\", ({ lane }) => mixer({\n\
         \x20 a: drums.hats(lane.notes(\"a\"), { decay: lane.control(\"len\", \"seconds\", ms(45)) }),\n\
         \x20 b: drums.hats(lane.notes(\"b\"), { openDecay: lane.control(\"len\", \"seconds\", ms(450)) }),\n\
         \x20 c: drums.shaker(lane.notes(\"c\"), { decay: lane.control(\"shake\", \"seconds\", ms(60)) }),\n\
         }));\n",
    );
    f.write(
        "data/perc/len.json",
        "{\"unit\": \"seconds\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 4, \"points\": [[0, 0.09]]}\n]}\n",
    );
    f.write(
        "data/perc/shake.json",
        "{\"unit\": \"seconds\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 4, \"points\": [[0, 0.06], [4, 1.5]]}\n]}\n",
    );
    let before = bytes(&f);
    let mut s = f.open();
    let up = upgrade(&mut s, false, None).unwrap();
    assert_eq!(up.written, None);
    let left: Vec<(&str, u32)> = up.left.iter().map(|l| (l.file.as_str(), l.line)).collect();
    assert!(left.contains(&("tracks/perc.ts", 7)) || left.contains(&("tracks/perc.ts", 6)), "the lane two knobs read: {:?}", up.left);
    assert!(left.contains(&("data/perc/shake.json", 1)), "the shaker's lane: {:?}", up.left);
    assert_eq!(bytes(&f), before, "nothing written");
}

// FMT-25 (G3): One drum's `decay` sets every hat's length, where the hats single had its closed
// and open hats' own (and its pedal hat's 25 ms). A call is converted when the hats its lane plays
// keep their lengths; otherwise it is named, and nothing is written. Two knobs that now are one are
// written once.
#[test]
fn each_hat_keeps_its_own_length() {
    let f = Fixture::new();
    old_perc(
        &f,
        "export default track(\"perc\", ({ lane }) => mixer({\n\
         \x20 closed: drums.hats(lane.notes(\"closed\"), { decay: ms(30) }),\n\
         \x20 both: drums.hats(lane.notes(\"both\"), { decay: ms(45), openDecay: ms(450) }),\n\
         \x20 open: drums.hats(lane.notes(\"open\"), { openDecay: ms(900), decay: ms(30) }),\n\
         }));\n",
    );
    f.write("data/perc/closed.json", notes_of(&["F#2", "F#2"]));
    f.write("data/perc/both.json", notes_of(&["F#2", "G#2", "A#2"]));
    f.write("data/perc/open.json", notes_of(&["A#2"]));
    let (_, _) = upgraded(&f);
    let perc = f.read("tracks/perc.ts");
    assert!(perc.contains("closed: drums.drum(lane.notes(\"closed\"), { drum: \"hats\", decay: 0.666667 }),"), "{perc}");
    assert!(perc.contains("both: drums.drum(lane.notes(\"both\"), { drum: \"hats\", decay: 1 }),"), "{perc}");
    assert!(perc.contains("open: drums.drum(lane.notes(\"open\"), { drum: \"hats\", decay: 2 }),"), "{perc}");

    // Closed and open hats on one lane, at other lengths than each other's: named.
    let g = Fixture::new();
    old_perc(&g, "export default track(\"perc\", ({ lane }) => drums.hats(lane.notes(\"n\"), { decay: ms(30) }));\n");
    g.write("data/perc/n.json", notes_of(&["F#2", "A#2"]));
    let before = bytes(&g);
    let mut s = g.open();
    let up = upgrade(&mut s, false, None).unwrap();
    assert_eq!(up.written, None);
    assert_eq!(up.left.len(), 1, "{:?}", up.left);
    assert!(up.left[0].what.contains("closed hats ring 30 ms") && up.left[0].what.contains("open hats 450 ms"), "{:?}", up.left);
    assert_eq!(bytes(&g), before);
}

// FMT-25 (G3): One drum rings a tom longer the lower it is tuned from its own 130 Hz, where the
// tom single rang its `decay` at every tune: a converted tune writes the decay that keeps it.
#[test]
fn a_toms_tune_keeps_its_ring_length() {
    let f = Fixture::new();
    old_perc(
        &f,
        "export default track(\"perc\", ({ lane }) => mixer({\n\
         \x20 low: drums.tom(lane.notes(\"low\"), { tune: hz(65) }),\n\
         \x20 high: drums.tom(lane.notes(\"high\"), { decay: secs(0.7), tune: hz(260) }),\n\
         }));\n",
    );
    upgraded(&f);
    let perc = f.read("tracks/perc.ts");
    assert!(perc.contains("low: drums.drum(lane.notes(\"low\"), { drum: \"tom\", tune: -12, decay: 0.707107 }),"), "{perc}");
    assert!(perc.contains("high: drums.drum(lane.notes(\"high\"), { drum: \"tom\", decay: 2.828427, tune: 12 }),"), "{perc}");
    // On a lane, its tune can't keep its length: named.
    let g = Fixture::new();
    old_perc(
        &g,
        "export default track(\"perc\", ({ lane }) => drums.tom(lane.notes(\"n\"), { tune: lane.control(\"t\", \"hz\", hz(130)) }));\n",
    );
    let mut s = g.open();
    let up = upgrade(&mut s, false, None).unwrap();
    assert_eq!((up.written, up.left.len()), (None, 1), "{:?}", up.left);
    assert!(up.left[0].what.contains("longer the lower it is tuned"), "{:?}", up.left);
}

// FMT-25 (G4): each single drum plays at its own voices, as One drum does at those voices
// (CARD-33): 4, written where One drum's own 6 isn't the single's.
#[test]
fn each_single_keeps_its_voices() {
    let f = Fixture::new();
    old_perc(
        &f,
        "export default track(\"perc\", ({ lane }) => mixer({ a: drums.kick(lane.notes(\"a\")), b: drums.cowbell(lane.notes(\"b\"), { voices: 8 }), c: drums.tom(lane.notes(\"c\")) }));\n",
    );
    upgraded(&f);
    let perc = f.read("tracks/perc.ts");
    assert!(perc.contains("a: drums.drum(lane.notes(\"a\"), { voices: 4 })"), "{perc}");
    assert!(perc.contains("b: drums.drum(lane.notes(\"b\"), { drum: \"cowbell\", voices: 8 })"), "{perc}");
    assert!(perc.contains("c: drums.drum(lane.notes(\"c\"), { drum: \"tom\" })"), "{perc}");
}

// FMT-25 (G5): a lane of Hz moves in straight lines in Hz; in semitones the same line bends. A
// converted tune lane gets points between, so it stays within a cent of the old line.
#[test]
fn a_tune_lanes_ramp_stays_within_a_cent() {
    let f = Fixture::new();
    old_perc(
        &f,
        "export default track(\"perc\", ({ lane }) => drums.cowbell(lane.notes(\"n\"), { tune: lane.control(\"t\", \"hz\", hz(540)) }));\n",
    );
    f.write("data/perc/t.json", "{\"unit\": \"hz\", \"clips\": [\n  {\"id\": \"a1\", \"start\": 0, \"length\": 8, \"points\": [[0, 270], [4, 270], [8, 1080]]}\n]}\n");
    upgraded(&f);
    let lane = m2_song::LaneData::read(&f.read("data/perc/t.json"), &m2_song::LaneKind::Control { unit: None }).unwrap();
    let m2_song::LaneData::Control { unit, clips } = lane else { panic!() };
    assert_eq!(unit.as_deref(), Some("semitones"));
    let points = &clips[0].points;
    assert!(points.len() > 3, "points between: {points:?}");
    assert!(points.iter().any(|(b, v)| *b == 4.0 && (*v + 12.0).abs() < 1e-6), "the flat part stays one line: {points:?}");
    let old = |b: f64| if b <= 4.0 { 270.0 } else { 270.0 + (b - 4.0) / 4.0 * 810.0 };
    for k in 0..=800 {
        let b = k as f64 / 100.0;
        let i = points.iter().rposition(|(pb, _)| *pb <= b).unwrap();
        let v = match points.get(i + 1) {
            Some((nb, nv)) if *nb > points[i].0 => points[i].1 + (b - points[i].0) / (nb - points[i].0) * (nv - points[i].1),
            _ => points[i].1,
        };
        let want = 12.0 * (old(b) / 540.0).log2();
        assert!((v - want).abs() <= 0.01, "at beat {b}: {v} for {want}");
    }
}

// FMT-25 (G6): a song whose lines were moved by hand, as the old messages said, while its code
// plays what no pack has now: it needs upgrading, and the upgrade converts it.
#[test]
fn lines_moved_by_hand_still_upgrade() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/synths\" = \"2\"\n"));
    f.write("song.ts", song_of(&["hats"]));
    f.write(
        "tracks/hats.ts",
        "import { track } from \"@m2/core\";\nimport { hat } from \"@m2/synths\";\n\nexport default track(\"hats\", ({ lane }) => hat(lane.notes(\"n\")));\n",
    );
    let s = f.open();
    assert!(needed(&s), "it needs upgrading: {:?}", s.draft_errors);
    drop(s);
    upgraded(&f);
    assert!(f.read("tracks/hats.ts").contains("drums.drum(lane.notes(\"n\"), { drum: \"hats\" })"), "{}", f.read("tracks/hats.ts"));
    assert!(f.read("music2.toml").contains("\"@m2/drums\" = \"2\"\n"));
}

// FMT-25 (G7): a re-export of a moved module is from where it is now; a module imported whole
// whose names moved is named, with the imports to write.
#[test]
fn reexports_move_and_whole_imports_are_named() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.6\"\n"));
    f.write("song.ts", song_of(&["keys"]));
    f.write("lib/sounds.ts", "export { pad } from \"std/instruments\";\nexport { duck, reverb } from \"std/fx\";\n");
    f.write(
        "tracks/keys.ts",
        "import { track } from \"@m2/core\";\nimport { duck, pad, reverb } from \"../lib/sounds\";\n\n\
         export default track(\"keys\", ({ lane }) => duck(reverb(pad(lane.notes(\"n\"))), lane.mask(\"m\")));\n",
    );
    upgraded(&f);
    assert_eq!(
        f.read("lib/sounds.ts"),
        "export { pad } from \"@m2/synths\";\nexport { duck } from \"std/fx\";\nexport { reverb } from \"@m2/effects\";\n"
    );
    let g = Fixture::new();
    g.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.6\"\n"));
    g.write("song.ts", "import { song } from \"@m2/core\";\nimport * as fx from \"std/fx\";\nimport { noise } from \"std/dsp\";\n\nexport default song(() => fx.duck(fx.reverb(noise()), noise()));\n");
    let mut s = g.open();
    let up = upgrade(&mut s, false, None).unwrap();
    assert_eq!((up.written, up.left.len()), (None, 1), "{:?}", up.left);
    assert_eq!((up.left[0].file.as_str(), up.left[0].line), ("song.ts", 2));
    assert!(
        up.left[0].write.contains("import { reverb } from \"@m2/effects\";")
            && up.left[0].write.contains("import { duck } from \"std/fx\";"),
        "{:?}",
        up.left
    );
}

// FMT-25 (G9): the musician's presets are theirs, outside the song: never changed. Those that name a
// sound that moved or was renamed are named, with what to do.
#[test]
fn presets_of_moved_sounds_are_named_and_never_changed() {
    let f = Fixture::new();
    old_perc(&f, "export default track(\"perc\", ({ lane }) => drums.snare(lane.notes(\"n\")));\n");
    // The musician's presets folder: outside the song, as in `~/Music/music2/Presets`.
    let home = Fixture::new();
    let presets = home.0.join("Presets");
    let preset = |folder: &str, name: &str, sound: &str, values: &str| {
        let p = presets.join(folder).join(format!("{name}.json"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(
            &p,
            format!("{{\n  \"m2preset\": 1,\n  \"name\": \"{name}\",\n  \"sound\": \"{sound}\",\n  \"values\": {values}\n}}\n"),
        )
        .unwrap();
    };
    preset("_m2_drums/drum-snare", "Tight", "@m2/drums:drum-snare", "{\"tune\": 200}");
    preset("_m2_synths/synth-bass", "Acid", "@m2/synths:synth-bass", "{\"decay\": 0.1}");
    preset("std/synth-pluck", "Glass", "std:synth-pluck", "{\"brightness\": 3000}");
    preset("_m2_synths/synth-pad", "Warm", "@m2/synths:synth-pad", "{\"cutoff\": 2000}");
    let before: Vec<_> = files_under(&presets).into_iter().map(|p| (p.clone(), std::fs::read(&p).unwrap())).collect();
    let mut s = f.open();
    let up = upgrade(&mut s, false, Some(&presets)).unwrap();
    assert!(up.written.is_some(), "{:?}", up.left);
    let named: Vec<&str> = up.notes.iter().map(|n| n.file.as_str()).collect();
    assert!(named.iter().any(|n| n.ends_with("drum-snare/Tight.json")), "{:?}", up.notes);
    assert!(named.iter().any(|n| n.ends_with("synth-bass/Acid.json")), "{:?}", up.notes);
    assert!(named.iter().any(|n| n.ends_with("synth-pluck/Glass.json")), "{:?}", up.notes);
    assert!(!named.iter().any(|n| n.ends_with("Warm.json")), "{:?}", up.notes);
    let after: Vec<_> = files_under(&presets).into_iter().map(|p| (p.clone(), std::fs::read(&p).unwrap())).collect();
    assert_eq!(before, after, "the presets are left as they are");
}

// FMT-25 (G10, G11, G12): an import it splits keeps its comments; the lines it adds end as the
// file's lines do; an import of a name no pack has now that nothing reads goes.
#[test]
fn comments_line_ends_and_unused_removed_names() {
    let f = Fixture::new();
    f.write("music2.toml", manifest("\"@m2/core\" = \"1\"\nstd = \"0.8\"\n\"@m2/synths\" = \"1\"\n").replace('\n', "\r\n"));
    f.write("song.ts", song_of(&["keys", "more"]));
    f.write(
        "tracks/keys.ts",
        "import { track } from \"@m2/core\";\nimport {\n  hat, // the closed hat, short and bright\n  pluck, // kept for the fill later\n} from \"@m2/synths\";\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"keys\", ({ lane }) => mixer({ h: hat(lane.notes(\"h\")), p: pluck(lane.notes(\"p\")) }));\n",
    );
    f.write(
        "tracks/more.ts",
        "import { track } from \"@m2/core\";\nimport { hat, kick, pluck } from \"std/instruments\";\n\nexport default track(\"more\", ({ lane }) => pluck(lane.notes(\"p\")));\n",
    );
    upgraded(&f);
    assert_eq!(
        f.read("tracks/keys.ts"),
        "import { track } from \"@m2/core\";\nimport {\n  pluck, // kept for the fill later\n} from \"@m2/synths\";\nimport { drums } from \"@m2/drums\"; // the closed hat, short and bright\nimport { mixer } from \"std/mix\";\n\n\
         export default track(\"keys\", ({ lane }) => mixer({ h: drums.drum(lane.notes(\"h\"), { drum: \"hats\" }), p: pluck(lane.notes(\"p\")) }));\n"
    );
    assert_eq!(
        f.read("tracks/more.ts"),
        "import { track } from \"@m2/core\";\nimport { pluck } from \"@m2/synths\";\n\nexport default track(\"more\", ({ lane }) => pluck(lane.notes(\"p\")));\n"
    );
    let manifest = f.read("music2.toml");
    assert!(manifest.ends_with("\"@m2/synths\" = \"2\"\r\n\"@m2/drums\" = \"2\"\r\n"), "{manifest:?}");
    assert!(!manifest.replace("\r\n", "").contains('\n'), "every line ends as the file's do: {manifest:?}");
}

// FMT-25 (G14): edits that would overlap are refused, never written over each other.
#[test]
fn edits_that_clash_are_refused() {
    assert_eq!(apply("abcdef", &[(1, 3, "X".into()), (4, 5, "Y".into())]).unwrap(), "aXdYf");
    assert!(apply("abcdef", &[(1, 4, "X".into()), (3, 5, "Y".into())]).is_err());
}

//! The track builders from the terminal (WRITE-10, WRITE-18): `m2 add-track`, `add-kit`,
//! `duplicate-track`, `delete-track` and `layout` on a copy of first-song.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (f, t) = (e.path(), to.join(e.file_name()));
        if f.is_dir() { copy(&f, &t) } else { std::fs::copy(&f, &t).map(drop).unwrap() }
    }
}

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        // Read-only files too: a pack m2 installed in a HOME here (PACK-16).
        let _ = m2_song::pack::remove_tree(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).env("HOME", m2_song::pack::test_home()).args(args).output().unwrap()
}

fn ok(args: &[&str]) -> String {
    let out = m2(args);
    assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// WRITE-10: each command is its builder, verified, and says what it did in words.
#[test]
fn tracks_are_added_duplicated_deleted_and_laid_out_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-tracks-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let dir = song.to_str().unwrap();
    let read = |p: &str| std::fs::read_to_string(song.join(p)).unwrap();

    let said = ok(&["add-track", dir, "synth.pluck", "--name", "Lead Vox", "--after", "kick"]);
    assert!(said.contains("added Lead Vox (Synth pluck), ID lead-vox, in tracks/lead-vox.ts"), "{said}");
    assert!(
        read("tracks/lead-vox.ts").contains("import { synth } from \"@m2/synths\";")
            && read("tracks/lead-vox.ts").contains("synth.pluck(lane.notes(\"notes\"))")
    );
    assert!(read("song.ts").contains("\"lead-vox\": leadVox.out"));
    assert!(read("layout.json").contains("[\"kick\", \"lead-vox\", \"hats\""), "{}", read("layout.json"));
    // An instrument that isn't one says which there are.
    let out = m2(&["add-track", dir, "@m2/effects:delay"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("@m2/synths:synth-pluck (Synth pluck)") && !err.contains("@m2/synths:pluck"), "{err}");

    let said = ok(&["duplicate-track", dir, "bass"]);
    assert!(said.contains("as Bass 2, ID bass-2"), "{said}");
    assert!(song.join("data/bass-2/notes.json").exists());

    // The kick feeds two ducks now: listed, nothing changed, until --cascade.
    let before = read("song.ts");
    let out = m2(&["delete-track", dir, "kick"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("Duck in Bass (song.ts") && err.contains("--cascade"), "{err}");
    assert_eq!(read("song.ts"), before);
    let said = ok(&["delete-track", dir, "kick", "--cascade"]);
    assert!(said.contains("deleted Kick") && said.contains("Duck"), "{said}");
    assert!(!song.join("tracks/kick.ts").exists() && !read("song.ts").contains("kick.") && !read("song.ts").contains("./tracks/kick"));

    let said = ok(&["layout", dir, "colour", "hats", "violet"]);
    assert!(said.contains("teal → violet") && said.contains("no revision"), "{said}");
    let out = m2(&["layout", dir, "colour", "hats", "red"]);
    assert!(!out.status.success());
    ok(&["layout", dir, "order", "keys", "bass-2", "bass", "hats", "lead-vox"]);
    assert!(read("layout.json").contains("[\"keys\", \"bass-2\", \"bass\", \"hats\", \"lead-vox\"]"));
    // KIT-5: a kit by its name, after a track: a Drums bus and its tracks, each going to it, named
    // apart from the song's own (the kick is gone, the hats are there).
    let said = ok(&["add-kit", dir, "Basic kit", "--after", "lead-vox"]);
    assert!(said.contains("added Basic kit: Drums (ID drums), and Kick, Snare and Hats 2 going to it"), "{said}");
    assert!(read("song.ts").contains("\"hats-2\": { in: hats2.out, to: \"drums\" }"), "{}", read("song.ts"));
    assert!(
        read("layout.json")
            .contains("[\"keys\", \"bass-2\", \"bass\", \"hats\", \"lead-vox\", \"drums\", \"kick\", \"snare\", \"hats-2\"]"),
        "{}",
        read("layout.json")
    );
    // A kit no instrument names says which there are.
    let out = m2(&["add-kit", dir, "Jazz kit"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("no kit `Jazz kit`: the kits are 808 kit, 909 kit and Basic kit"), "{err}");

    ok(&["check", dir]);
}

// KIT-1: `m2 graph` says each drum kit on a line of its own, after the revision: its name and bus,
// then each row's track, lane and sound.
#[test]
fn m2_graph_says_each_drum_kit() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-kits-cli-{}", std::process::id())));
    let song = temp.0.join("buses");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/buses"), &song);
    let said = ok(&["graph", song.to_str().unwrap()]);
    let kits: Vec<&str> = said.lines().filter(|l| l.starts_with("kit ")).collect();
    assert_eq!(kits, ["kit Drums (drums): Kick (kick/hits, kick) · Hats (hats/hits, hats)"], "{said}");
}

// KIT-24: `m2 make-kit` makes a drum machine track a kit, as its track menu's Make it a kit does: the
// track the kit's bus, a Drum track for each sound its notes use going to it, in one undo step; it
// says what it keeps, and what went. A track that plays no drum machine is refused in words.
#[test]
fn a_drum_machine_track_is_made_a_kit_from_the_terminal() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-make-kit-cli-{}", std::process::id())));
    let song = temp.0.join("beat");
    let write = |name: &str, text: &str| {
        let p = song.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    write(
        "music2.toml",
        "[project]\nname = \"beat\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.9\"\n\"@m2/drums\" = \"2\"\n",
    );
    write("data/arrangement.json", "{\"tempo\": [{\"beat\": 0, \"bpm\": 120}], \"meter\": []}\n");
    write(
        "song.ts",
        "import { song } from \"@m2/core\";\nimport { master, mixer } from \"std/mix\";\nimport beat from \"./tracks/beat\";\n\n\
         export default song(() => master(mixer({ beat: beat.out })));\n",
    );
    write(
        "tracks/beat.ts",
        "import { track } from \"@m2/core\";\nimport { drums } from \"@m2/drums\";\n\n\
         export default track(\"beat\", { name: \"Beat\" }, ({ lane }) => drums.kit(lane.notes(\"hits\")));\n",
    );
    write(
        "data/beat/hits.json",
        "{\"clips\": [{\"id\": \"c1\", \"start\": 0, \"length\": 4, \"notes\": [{\"pitch\": 36, \"start\": 0, \"length\": 0.25}, \
         {\"pitch\": 38, \"start\": 1, \"length\": 0.25}, {\"pitch\": 42, \"start\": 1.5, \"length\": 0.25}, {\"pitch\": 55, \"start\": 2, \"length\": 0.25}]}]}\n",
    );
    let dir = song.to_str().unwrap();
    let said = ok(&["make-kit", dir, "beat"]);
    assert!(said.contains("made Beat a kit: its bus Beat (ID beat), and Kick, Snare and Hats going to it (revision"), "{said}");
    assert!(said.contains("Same drums, pitches, levels and lengths; each hit's noise comes out a little different."), "{said}");
    assert!(said.contains("1 note the drum machine is silent at went"), "{said}");
    assert!(
        std::fs::read_to_string(song.join("tracks/beat.ts"))
            .unwrap()
            .contains("bus(\"beat\", { name: \"Beat\" }, ({ input }) => gain(input))")
    );
    ok(&["check", dir]);
    let out = m2(&["make-kit", dir, "kick"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("Kick plays no drum machine."), "{err}");
}

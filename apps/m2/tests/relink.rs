//! Lane data a rename left behind: every command that reads the project says so, with the
//! commands that fix it, and `m2 relink` / `m2 delete-data` fix it.

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
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn m2(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_m2")).args(args).output().unwrap()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn every_command_says_what_a_rename_left_behind_and_relink_puts_it_back() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-relink-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let kick = std::fs::read_to_string(song.join("tracks/kick.ts")).unwrap();
    std::fs::write(song.join("tracks/kick.ts"), kick.replacen("track(\"Kick\"", "track(\"Big Kick\"", 1)).unwrap();
    let notes = std::fs::read_to_string(song.join("timeline/kick.hits.json")).unwrap();
    let dir = song.to_str().unwrap();

    let warning = "warning: timeline/kick.hits.json: Notes on kick › hits aren't playing. There's no track called kick any more. Did \
                   you rename it? Move them to Big Kick › hits to hear them again, or delete them.";
    let relink = format!("  move it:   m2 relink {dir} timeline/kick.hits.json 'Big Kick' hits");
    let delete = format!("  delete it: m2 delete-data {dir} timeline/kick.hits.json");
    let wav = temp.0.join("out.wav");
    let wav = wav.to_str().unwrap();
    let commands: [&[&str]; 11] = [
        &["check", dir],
        &["render", dir, wav, "--seconds", "0.1"],
        &["bounce", dir, "kick-bounce", "--seconds", "0.1"],
        &["graph", dir],
        &["controls", dir],
        &["set", dir, "song.ts#0/0", "1"],
        &["cables", dir],
        &["connect", dir, "nothing", "here"],
        &["disconnect", dir, "nothing/here"],
        &["effects", dir],
        &["remove-effect", dir, "nothing"],
    ];
    for args in commands {
        let err = stderr(&m2(args));
        assert!(err.contains(warning) && err.contains(&relink) && err.contains(&delete), "m2 {}:\n{err}", args[0]);
    }

    // Moved from a path given from here, as well as one in the project.
    let from = song.join("timeline/kick.hits.json");
    let out = m2(&["relink", dir, from.to_str().unwrap(), "Big Kick", "hits"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("timeline/kick.hits.json: `unused notes` → `Big Kick › hits` (revision 2"));
    assert!(!stderr(&out).contains("warning"), "{}", stderr(&out));
    assert_eq!(std::fs::read_to_string(song.join("timeline/big-kick.hits.json")).unwrap(), notes);
    assert!(!song.join("timeline/kick.hits.json").exists());
    let out = m2(&["check", dir]);
    assert!(out.status.success() && !stderr(&out).contains("warning"), "{}", stderr(&out));

    // Again: nothing to move, and nothing is touched.
    let out = m2(&["relink", dir, "timeline/kick.hits.json", "Big Kick", "hits"]);
    assert!(
        !out.status.success() && stderr(&out).contains("edit rejected: the project has no timeline/kick.hits.json"),
        "{}",
        stderr(&out)
    );

    // Data no lane can take is deleted on request only, and claimed data never.
    std::fs::write(song.join("timeline/snare.hits.json"), &notes).unwrap();
    let err = stderr(&m2(&["check", dir]));
    assert!(
        err.contains("warning: timeline/snare.hits.json: Notes on snare › hits aren't playing.") && !err.contains("m2 relink"),
        "{err}"
    );
    let out = m2(&["delete-data", dir, "timeline/big-kick.hits.json"]);
    assert!(!out.status.success() && song.join("timeline/big-kick.hits.json").exists(), "{}", stderr(&out));
    let out = m2(&["delete-data", dir, "timeline/snare.hits.json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("timeline/snare.hits.json: `unused notes` → `deleted`"));
    assert!(!song.join("timeline/snare.hits.json").exists() && !stderr(&out).contains("warning"));
}

//! `m2 mute-in-song` (MON-5, WRITE-18): the option written and taken out as one edit each, and a
//! refusal that changes nothing while another process holds the project.

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

fn said(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn mute_in_song_writes_the_option_and_takes_the_lock() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-mute-cli-{}", std::process::id())));
    let song = temp.0.join("first-song");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-song"), &song);
    let (dir, kick) = (song.to_str().unwrap(), song.join("tracks/kick.ts"));
    let before = std::fs::read_to_string(&kick).unwrap();

    let on = m2(&["mute-in-song", dir, "kick", "on"]);
    assert!(on.status.success(), "{}", said(&on));
    assert!(said(&on).contains("Kick → Kick, muted in song"), "{}", said(&on));
    assert_eq!(std::fs::read_to_string(&kick).unwrap(), before.replace("{ name: \"Kick\" }", "{ name: \"Kick\", muted: true }"));
    let off = m2(&["mute-in-song", dir, "kick", "off"]);
    assert!(off.status.success(), "{}", said(&off));
    assert_eq!(std::fs::read_to_string(&kick).unwrap(), before);
    let missing = m2(&["mute-in-song", dir, "kik", "on"]);
    assert!(!missing.status.success() && said(&missing).ends_with("error: There's no track `kik`.\n"), "said whole: {}", said(&missing));
    let bad = m2(&["mute-in-song", dir, "kick", "maybe"]);
    assert!(!bad.status.success() && said(&bad).contains("on or off"), "{}", said(&bad));

    // WRITE-18: another process holds the project: refused, naming it, and nothing changes.
    let held = m2_session::lock::acquire(&song, "music2", m2_session::lock::RETRY).unwrap();
    let refused = m2(&["mute-in-song", dir, "kick", "on"]);
    assert!(!refused.status.success(), "{}", said(&refused));
    assert!(said(&refused).contains("music2") && said(&refused).contains(&std::process::id().to_string()), "{}", said(&refused));
    assert_eq!(std::fs::read_to_string(&kick).unwrap(), before);
    drop(held);
}

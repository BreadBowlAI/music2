//! gaps #31, plan X20: new songs go where Settings says. In a process of its own, as the songs
//! folder Settings chooses is the whole process's: unit tests that read the default folder in
//! parallel never see it.

use std::path::PathBuf;

use m2_play::Player;
use m2_server::graph::Graph;
use m2_server::{AppState, export, project};
use serde_json::json;

/// A folder of its own under the system's temporary folder, removed after.
struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn new_songs_go_where_settings_say() {
    let t = Temp(std::env::temp_dir().join(format!("m2-songs-folder-{}", std::process::id())));
    // The home folder and the songs folder are reached through a link, as on a Mac, whose
    // temporary folder is one (/var is /private/var). A song opens by its real path, so every
    // folder is named by its real path too: one folder, named alike everywhere.
    std::fs::create_dir_all(t.0.join("real/home/.music2")).unwrap();
    std::os::unix::fs::symlink(t.0.join("real"), t.0.join("link")).unwrap();
    // SAFETY: this test is its process's only one, and sets HOME before anything reads it.
    unsafe { std::env::set_var("HOME", t.0.join("link/home")) };
    let real = t.0.join("real").canonicalize().unwrap();
    let chosen = t.0.join("link/My Songs");
    let file = t.0.join("link/home/.music2/settings.json");
    let kept = json!({"output": {"device": null, "frames": null}, "songs": chosen});
    std::fs::write(&file, kept.to_string()).unwrap();
    // The player is one the test drives (and never fills): no device opens.
    let (player, _driver) = Player::driven(48_000);
    let state = AppState::new(String::new(), Graph::with_player(player));
    let default = project::songs_dir().unwrap();
    assert_eq!(default, real.join("home/Music/music2"), "before the settings are read: the default, by its real path");
    assert_eq!(default, project::default_songs_dir().unwrap());
    assert_eq!(project::catalog()["home"], json!(real.join("home")), "the window writes the songs folder as ~/Music/music2");
    state.keep_settings(file.clone());
    let there = real.join("My Songs");
    assert_eq!(project::songs_dir().unwrap(), there, "the folder chosen, by its real path");
    assert_eq!(project::catalog()["songs"], json!(there), "the New Song dialog's folder");
    assert_eq!(export::exports_dir().unwrap(), there.join("Exports"), "exports go with them");
    let made = project::new_song(&state.graph, &project::NewSong { name: "Night".into(), ..Default::default() }).unwrap();
    assert_eq!(made["dir"], json!(there.join("Night")), "a new song is made there, named as the dialog names its folder");
    assert!(chosen.join("Night/song.ts").is_file());
    // A folder inside a song isn't one: the default again, and it's said in words.
    let inside = project::songs_folder(&chosen.join("Night/tracks")).unwrap_err().to_string();
    assert_eq!(inside, "That folder is inside the song Night. Pick a folder outside any song.");
    let relative = project::songs_folder(std::path::Path::new("Songs")).unwrap_err().to_string();
    assert_eq!(relative, "Type a full path, starting with /.");
    project::set_songs_dir(None);
    assert_eq!(project::songs_dir().unwrap(), default);
    // A kept folder that is now inside a song is the default at the next launch.
    std::fs::write(&file, json!({"songs": chosen.join("Night")}).to_string()).unwrap();
    let (player, _driver) = Player::driven(48_000);
    AppState::new(String::new(), Graph::with_player(player)).keep_settings(file);
    assert_eq!(project::songs_dir().unwrap(), default);
}

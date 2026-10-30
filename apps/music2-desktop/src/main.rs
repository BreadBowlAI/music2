//! music2 as a native app.
//!
//! The React UI in a native window: the app's server runs in-process on a private port with
//! two per-launch keys (AGENT-10): the page's, handed only to the window, and the tools key that
//! `~/.music2/session.json` publishes for agents and scripts. The window loads the UI from it. This shell adds what a browser
//! can't: the menu bar, file dialogs and the prompt about unsaved code.
//!
//!   music2-desktop [project-folder/]
//!   MUSIC2_DEV_URL=http://localhost:5173 music2-desktop   # load the UI from Vite (server on :4100)
//!
//! With no folder given, it opens the song that was open when it last quit (polish Q18), from the
//! recent songs list in `~/.music2/recent.json`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod menu;
mod plugin_host;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use m2_server::recent::Reopen;
use m2_server::{AppState, ServeOptions};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub struct Native {
    pub server: Arc<AppState>,
}

// RENDER-28: debug builds count what the audio callback allocates or frees, and log it once a
// second while there is any; release builds use the system allocator as it is.
#[cfg(debug_assertions)]
#[global_allocator]
static ALLOC: m2_play::alloc::Counting = m2_play::alloc::Counting;

fn main() {
    #[cfg(debug_assertions)]
    m2_play::alloc::watch();
    // PLAT-7: a program keeps the cache folder (hashes, peaks, renders, previews); a test doesn't.
    m2_server::resources::keep_cache();
    // HOST-2: plugins make their main-thread calls on this thread, AppKit's, through the main
    // dispatch queue; it must be installed before anything touches a plugin. From here on the main
    // thread must never wait on a thread that may be creating plugins (the server's project work):
    // that thread waits on the main thread.
    if let Err(e) = plugin_host::install() {
        eprintln!("music2: plugins can't be hosted: {e:#}");
    }
    // A folder with a song.ts is a song project. Anything else is reported rather than ignored.
    let project = std::env::args().skip(1).map(PathBuf::from).find(|p| p.join("song.ts").is_file());
    let unknown: Vec<String> =
        std::env::args().skip(1).filter(|a| !a.starts_with('-') && !PathBuf::from(a).join("song.ts").is_file()).collect();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![actions::action])
        .setup(move |app| {
            let dev_url = std::env::var("MUSIC2_DEV_URL").ok();
            let running = m2_server::start(ServeOptions {
                // Vite proxies to a fixed port in dev; otherwise any free one.
                port: if dev_url.is_some() { 4100 } else { 0 },
                web_dir: web_dir(app.handle()),
                token: token(),
                page_token: token(),
            })?;
            let server = running.state.clone();
            server.keep_recent(home().join(".music2").join("recent.json"));
            // The sound output and where new songs go (gaps #31), before anything plays.
            server.keep_settings(home().join(".music2").join("settings.json"));
            // Your presets, and previews kept in the cache folder (LIB-1, LIB-3).
            server.keep_library(m2_server::library::Folders::of_this_user());
            publish_session(&running.url, &server.token);
            // No window is connected yet: the server keeps these toasts for the first one.
            // They name the folder, not its whole path, so the reason fits. Opened off the main
            // thread: opening creates the song's plugins, which run on the main thread (HOST-2).
            if let Some(dir) = project.clone() {
                plugin_host::open_off_main(server.clone(), dir, |server, dir, e| {
                    eprintln!("music2: couldn't open {}: {e:#}", dir.display());
                    server.toast(format!("Couldn't open {}: {e:#}", name(dir)));
                });
            } else {
                reopen(&server);
            }
            for a in &unknown {
                eprintln!("music2: `{a}` is not a project folder (a folder with song.ts)");
                server.toast(format!("“{}” is not a song folder (a folder with song.ts in it)", name(Path::new(a))));
            }
            app.manage(Native { server: server.clone() });
            app.set_menu(menu::build(app.handle())?)?;

            let url = dev_url.unwrap_or(running.url);
            let init = format!(
                "window.__M2__ = {};",
                serde_json::json!({"native": true, "token": server.page_token, "platform": std::env::consts::OS})
            );
            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("music2")
                .inner_size(1440.0, 900.0)
                // taste.md §3.2: the smallest window the layout is drawn for.
                .min_inner_size(1100.0, 700.0)
                .theme(Some(tauri::Theme::Light))
                .initialization_script(&init);
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                // Vertically centred in the UI's 44px top bar, inside the 80px it keeps for them.
                .traffic_light_position(tauri::LogicalPosition::new(18.0, 16.0));
            let window = builder.build()?;
            keep_title_in_sync(window, server);
            Ok(())
        })
        .on_menu_event(|app, event| menu::handle(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            // Closing the window quits; ask about unsaved code first.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                actions::run(window.app_handle(), "quit");
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start music2");
}

/// At a launch with no folder given: the song that was open when the app quit (Q18, X22), off the
/// main thread. Unless the last launch closed while opening it: then the start screen says why it
/// didn't open, and it isn't tried again.
fn reopen(server: &Arc<AppState>) {
    let Some(recent) = server.recent() else { return };
    match recent.to_reopen() {
        Reopen::Song(dir) => {
            if let Err(e) = recent.reopening(&dir) {
                eprintln!("music2: couldn't keep the recent songs list: {e:#}");
            }
            let server = server.clone();
            std::thread::spawn(move || {
                let opened = plugin_host::open_project(&server, &dir);
                if let Some(Err(e)) = server.recent().map(|r| r.reopened()) {
                    eprintln!("music2: couldn't keep the recent songs list: {e:#}");
                }
                if let Err(e) = opened {
                    eprintln!("music2: couldn't open {}: {e:#}", dir.display());
                    server.toast(format!("Couldn't open {}: {e:#}", name(&dir)));
                }
            });
        }
        Reopen::Crashed(dir) => {
            let song = recent.list().into_iter().find(|s| s.path == dir).map_or_else(|| name(&dir), |s| s.name);
            server.toast(format!("music2 closed while opening {song} last time, so it didn't open it again."));
        }
        Reopen::Nothing => {}
    }
}

/// The user's home folder.
fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// A path's last part (`my-song` for `~/Music/my-song`), or the whole path when it has none.
fn name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The built React UI: bundled next to the app, or `web/dist` in a dev checkout.
fn web_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(dir) = std::env::var("MUSIC2_WEB_DIR") {
        return dir.into();
    }
    if let Ok(res) = app.path().resource_dir() {
        let bundled = res.join("web");
        if bundled.join("index.html").exists() {
            return bundled;
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist"))
}

/// Where local tools (Claude Code, scripts, `m2`) find the running app: its server URL and the
/// tools key, in `~/.music2/session.json`, readable only by this user. Web pages can't read it,
/// which is what the key protects against; a request with it is an agent's (AGENT-10).
fn publish_session(url: &str, token: &str) {
    let path = home().join(".music2").join("session.json");
    let body = serde_json::json!({"url": url, "token": token, "pid": std::process::id()});
    let write = || -> std::io::Result<()> { write_private(&path, serde_json::to_string_pretty(&body)?.as_bytes()) };
    if let Err(e) = write() {
        eprintln!("music2: couldn't write {}: {e}", path.display());
    }
}

/// Writes `bytes` to `path` whole, so that only this user can read it (SECURITY.md, AGENT-10): to
/// a new file beside it with mode 0600, set before the bytes go in, then renamed over the old one,
/// so a reader never reads half of it and the mode holds even where an older file had another.
/// 0700 for a folder it makes. (On Unix; elsewhere the OS's default rights.)
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut dirs = std::fs::DirBuilder::new();
    dirs.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut dirs, 0o700);
    dirs.create(path.parent().unwrap_or(path))?;
    let mut next = path.as_os_str().to_owned();
    next.push(".new");
    let next = PathBuf::from(next);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut file = opts.open(&next)?;
    // The mode above is only for a file it creates: one left by a launch that stopped half way
    // may have another.
    #[cfg(unix)]
    file.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::io::Write::write_all(&mut file, bytes)?;
    drop(file);
    std::fs::rename(&next, path)
}

fn token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("system randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Window title follows the open project, File ▸ Open Recent ▸ the recent songs list (the menu is
/// built again when its file changes), and Edit ▸ Undo and Redo the names of what they take back.
fn keep_title_in_sync(window: tauri::WebviewWindow, server: Arc<AppState>) {
    let app = window.app_handle().clone();
    std::thread::spawn(move || {
        let mut last = String::new();
        let stamp = || server.recent().and_then(|r| std::fs::metadata(r.file()).and_then(|m| m.modified()).ok());
        let mut listed = stamp();
        loop {
            let now = stamp();
            if now != listed {
                listed = now;
                if let Err(e) = menu::build(&app).and_then(|m| app.set_menu(m)) {
                    eprintln!("music2: couldn't show the recent songs in the menu: {e}");
                }
            }
            // Edit ▸ Undo and Redo say what they would take back (gaps #29, plan X8).
            let (undo, redo) = server.graph.undo_names();
            menu::set_undo_names(undo, redo);
            // No "— Edited": edits are written to the project's files at once (the code editor
            // asks before unsaved code is lost).
            let title = server.graph.name().unwrap_or_else(|| "music2".into());
            if title != last {
                if window.set_title(&title).is_err() {
                    return; // window gone
                }
                last = title;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    });
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    /// SECURITY.md: session.json holds the local API's token, so only you may read it: mode 0600
    /// even when an older one was there with another mode, in a ~/.music2 made 0700.
    #[test]
    fn the_session_file_is_private_even_when_one_was_there() {
        let dir = std::env::temp_dir().join(format!("m2-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(".music2").join("session.json");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;

        super::write_private(&path, b"first").unwrap();
        assert_eq!((mode(&path), mode(path.parent().unwrap())), (0o600, 0o700), "a new file, in a new folder");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        // AGENT-10: written whole: one who opened the old file reads it whole, never half of the new.
        let mut old = std::fs::File::open(&path).unwrap();
        super::write_private(&path, b"second").unwrap();
        assert_eq!(mode(&path), 0o600, "a file that was there, readable by others");
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let mut was = vec![];
        std::io::Read::read_to_end(&mut old, &mut was).unwrap();
        assert_eq!(was, b"first", "the old file was replaced, not written over");
        assert!(!dir.join(".music2/session.json.new").exists(), "nothing left beside it");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

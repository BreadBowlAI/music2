//! music2 as a native app.
//!
//! The React UI in a native window: the app's server runs in-process on a private port with
//! a per-launch token, and the window loads the UI from it. This shell adds what a browser
//! can't: the menu bar, file dialogs and the prompt about unsaved code.
//!
//!   music2-desktop [project-folder/]
//!   MUSIC2_DEV_URL=http://localhost:5173 music2-desktop   # load the UI from Vite (server on :4100)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod menu;
mod plugin_host;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use m2_server::{AppState, ServeOptions};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub struct Native {
    pub server: Arc<AppState>,
}

fn main() {
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
            })?;
            let server = running.state.clone();
            publish_session(&running.url, &server.token);
            // No window is connected yet: the server keeps these toasts for the first one.
            // They name the folder, not its whole path, so the reason fits. Opened off the main
            // thread: opening creates the song's plugins, which run on the main thread (HOST-2).
            if let Some(dir) = project.clone() {
                plugin_host::open_off_main(server.clone(), dir, |server, dir, e| {
                    eprintln!("music2: couldn't open {}: {e:#}", dir.display());
                    server.toast(format!("Couldn't open {}: {e:#}", name(dir)));
                });
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
                serde_json::json!({"native": true, "token": server.token, "platform": std::env::consts::OS})
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

/// Where local tools (Claude Code, scripts) find the running app: its server URL and
/// token, in `~/.music2/session.json`, readable only by this user. Web pages can't read
/// it, which is what the token protects against.
fn publish_session(url: &str, token: &str) {
    let path = dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".music2").join("session.json");
    let body = serde_json::json!({"url": url, "token": token, "pid": std::process::id()});
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(path.parent().unwrap_or(&path))?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        std::io::Write::write_all(&mut opts.open(&path)?, serde_json::to_string_pretty(&body)?.as_bytes())
    };
    if let Err(e) = write() {
        eprintln!("music2: couldn't write {}: {e}", path.display());
    }
}

fn token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("system randomness");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Window title follows the open project.
fn keep_title_in_sync(window: tauri::WebviewWindow, server: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
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

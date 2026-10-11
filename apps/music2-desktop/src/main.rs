//! L6 — music2 as a native app.
//!
//! The same engine and React UI as `music2`, but in a native window instead of a browser
//! tab: the engine's HTTP server runs in-process on a private port with a per-launch
//! token, the window loads the UI from it, and this shell adds what a browser can't —
//! the menu bar, file dialogs, Finder integration, "unsaved changes" prompts, and native
//! right-click menus (the UI builds those through Tauri's menu API).
//!
//!   music2-desktop [song.m2song | project-folder/]
//!   MUSIC2_DEV_URL=http://localhost:5173 music2-desktop   # load the UI from Vite (engine on :4100)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod menu;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use m2_server::{AppState, ServeOptions};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub struct Native {
    pub engine: Arc<AppState>,
}

fn main() {
    let song = std::env::args().skip(1).find(|a| a.ends_with(".m2song")).map(PathBuf::from);
    // A folder with a song.ts is a song-as-program project, shown as a graph. Anything
    // else that isn't a song is reported rather than ignored.
    let project = std::env::args().skip(1).map(PathBuf::from).find(|p| p.join("song.ts").is_file());
    let unknown: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-') && !a.ends_with(".m2song") && !PathBuf::from(a).join("song.ts").is_file())
        .collect();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![actions::action])
        .setup(move |app| {
            let dev_url = std::env::var("MUSIC2_DEV_URL").ok();
            let running = m2_server::start(ServeOptions {
                // Vite proxies to a fixed port in dev; otherwise any free one.
                port: if dev_url.is_some() { 4100 } else { 0 },
                web_dir: web_dir(app.handle()),
                song: song.clone(),
                token: token(),
            })?;
            let engine = running.state.clone();
            publish_session(&running.url, &engine.token);
            if let Some(dir) = &project
                && let Err(e) = actions::open_project(&engine, dir)
            {
                eprintln!("music2: couldn't open {}: {e:#}", dir.display());
                engine.toast(format!("Couldn't open {}: {e:#}", dir.display()));
            }
            for a in &unknown {
                eprintln!("music2: `{a}` is not a song or a project folder (a folder with song.ts)");
                engine.toast(format!("`{a}` is not a song or a project folder (a folder with song.ts)"));
            }
            app.manage(Native { engine: engine.clone() });
            app.set_menu(menu::build(app.handle())?)?;

            let url = dev_url.unwrap_or(running.url);
            let init = format!(
                "window.__M2__ = {};",
                serde_json::json!({"native": true, "token": engine.token, "platform": std::env::consts::OS})
            );
            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("music2")
                .inner_size(1440.0, 900.0)
                .min_inner_size(960.0, 620.0)
                .theme(Some(tauri::Theme::Light))
                .initialization_script(&init);
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                // Vertically centred in the UI's 64px top bar.
                .traffic_light_position(tauri::LogicalPosition::new(20.0, 25.0));
            let window = builder.build()?;
            keep_title_in_sync(window, engine);
            Ok(())
        })
        .on_menu_event(|app, event| menu::handle(app, event.id().as_ref()))
        .on_window_event(|window, event| {
            // Closing the window quits; ask about unsaved changes first.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                actions::run(window.app_handle(), "quit", None);
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to start music2");

    app.run(|app, event| {
        // Double-clicked a .m2song in Finder (or dropped it on the Dock icon).
        #[cfg(target_os = "macos")]
        if let RunEvent::Opened { urls } = &event {
            for url in urls {
                if let Ok(path) = url.to_file_path() {
                    actions::run(app, "open_path", Some(serde_json::json!({"path": path})));
                }
            }
        }
        let _ = (app, event);
    });
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

/// Where local tools (Claude Code, scripts) find the running app: its engine URL and
/// token, in `~/.music2/session.json`, readable only by this user. Web pages can't read
/// it, which is what the token protects against.
fn publish_session(url: &str, token: &str) {
    let path = m2_engine::music2_home().join("session.json");
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

/// Window title follows the song: "Song Name — Edited", like any document app.
fn keep_title_in_sync(window: tauri::WebviewWindow, engine: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut last = String::new();
        loop {
            let title = {
                let s = engine.session.lock();
                format!("{}{}", s.song().title, if s.is_dirty() { " — Edited" } else { "" })
            };
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

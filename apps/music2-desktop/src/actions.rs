//! Native actions: the Open Project Folder dialog, Show in Finder, the UI's "ready"
//! handshake, and the prompts about unsaved code and a running export. They block on dialogs, so each runs on its
//! own thread; the UI updates itself through the server's WebSocket like after any other edit.
//! A few answer the UI instead: the Export Audio save dialog returns the file picked.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

use crate::{Native, plugin_host};

/// One dialog flow at a time (a second ⌘O while the first panel is open is ignored).
static BUSY: AtomicBool = AtomicBool::new(false);

/// Called from the UI: "ready" once it has loaded, and "open_project" from the start
/// screen's button (the same as the menu item). Those that answer wait for it: "export_path"
/// returns the file picked in the Export Audio save dialog (None if cancelled), and
/// "reveal_file" shows `arg`, an exported file, in Finder. "song_folder" returns the folder
/// picked for a new song (the New Song dialog's Choose Folder…), starting at `arg`.
#[tauri::command]
pub async fn action(app: AppHandle, id: String, arg: Option<String>) -> Result<Option<String>, String> {
    let answer = match id.as_str() {
        "export_path" => tauri::async_runtime::spawn_blocking(move || {
            // One dialog at a time: while another is open, it's as if this one was cancelled.
            if BUSY.swap(true, Ordering::SeqCst) {
                return Ok(None);
            }
            let picked = export_path(&app);
            BUSY.store(false, Ordering::SeqCst);
            picked
        }),
        "reveal_file" => tauri::async_runtime::spawn_blocking(move || reveal_file(Path::new(&arg.unwrap_or_default())).map(|()| None)),
        "song_folder" => tauri::async_runtime::spawn_blocking(move || {
            if BUSY.swap(true, Ordering::SeqCst) {
                return Ok(None);
            }
            let picked = song_folder(&app, arg);
            BUSY.store(false, Ordering::SeqCst);
            picked
        }),
        _ => {
            run(&app, &id);
            return Ok(None);
        }
    };
    answer.await.map_err(|e| e.to_string())?.map_err(|e| format!("{e:#}"))
}

/// The Export Audio save dialog, starting at the song's new file in the songs folder's Exports.
fn export_path(app: &AppHandle) -> anyhow::Result<Option<String>> {
    let server = app.state::<Native>().server.clone();
    let suggested = m2_server::export::default_path(&server)?;
    let mut dialog = app.dialog().file().set_title("Export Audio").add_filter("WAV audio", &["wav"]).set_can_create_directories(true);
    if let (Some(dir), Some(name)) = (suggested.parent(), suggested.file_name()) {
        dialog = dialog.set_directory(dir).set_file_name(name.to_string_lossy());
    }
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    Ok(dialog.blocking_save_file().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// Where to make a new song: a folder picked starting at `start` (the one chosen before) or
/// the songs folder, which is made if it isn't there yet.
fn song_folder(app: &AppHandle, start: Option<String>) -> anyhow::Result<Option<String>> {
    let dir = match start.map(PathBuf::from).filter(|d| d.is_dir()) {
        Some(d) => d,
        None => m2_server::project::make_songs_dir()?,
    };
    let mut dialog = app.dialog().file().set_title("Choose Where to Save the Song").set_directory(dir).set_can_create_directories(true);
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    Ok(dialog.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// Show a file in Finder, selected (elsewhere, open its folder).
fn reveal_file(path: &Path) -> anyhow::Result<()> {
    if !path.is_file() {
        anyhow::bail!("{} isn't there any more", path.display());
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open").arg("-R").arg(path).status()?;
    #[cfg(not(target_os = "macos"))]
    open::that(path.parent().unwrap_or(path))?;
    Ok(())
}

pub fn run(app: &AppHandle, id: &str) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, id) = (app.clone(), id.to_string());
    std::thread::spawn(move || {
        if let Err(e) = perform(&app, &id) {
            error(&app, &format!("{e:#}"));
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

fn perform(app: &AppHandle, id: &str) -> anyhow::Result<()> {
    let server = app.state::<Native>().server.clone();
    match id {
        "open_project" => {
            // A song project: a folder with song.ts.
            let mut dialog = app.dialog().file().set_title("Open Project Folder");
            // Where new songs go, once there is one.
            if let Some(songs) = m2_server::project::songs_dir().ok().filter(|d| d.is_dir()) {
                dialog = dialog.set_directory(songs);
            }
            let picked = dialog.blocking_pick_folder();
            if let Some(dir) = picked.and_then(|p| p.into_path().ok())
                && keep_code(app)?
            {
                plugin_host::open_project(&server, &dir)?;
            }
        }
        "reveal" => {
            // With no project open there is nothing to show.
            if let Some(dir) = server.graph.dir() {
                open::that(dir)?;
            }
        }
        "ready" => println!("music2: UI connected to the native shell"),
        "quit" => {
            if keep_export(app)? && keep_code(app)? {
                // A stopped export removes its unfinished file; give it a moment to.
                server.exports.stop(Duration::from_secs(5));
                // HOST-2: the project's plugin instances are destroyed on the main thread, which
                // keeps serving them while this thread waits (2 s at most). Exit handlers would
                // unload plugin code under any left alive (a hung device thread holds its
                // renderer), so then the app leaves without running them.
                let t = plugin_host::close_for_quit(&server);
                if t.left > 0 {
                    eprintln!("music2: {} plugin instance(s) still alive at quit; leaving without exit handlers", t.left);
                    plugin_host::leave_now();
                }
                app.exit(0);
            }
        }
        other => anyhow::bail!("unknown action {other}"),
    }
    Ok(())
}

/// Before quitting while a song is being exported: the export would stop, and no file be
/// saved. False = the user would rather let it finish.
fn keep_export(app: &AppHandle) -> anyhow::Result<bool> {
    let server = app.state::<Native>().server.clone();
    let Some(song) = server.exports.song() else { return Ok(true) };
    const QUIT: &str = "Quit Anyway";
    let mut dialog = app
        .dialog()
        .message(format!("“{song}” is still being exported. If you quit now, the export stops and no audio file is saved."))
        .title("Quit while exporting?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(QUIT.into(), "Keep Exporting".into()));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    Ok(match dialog.blocking_show_with_result() {
        MessageDialogResult::Ok => true,
        MessageDialogResult::Custom(s) => s == QUIT,
        _ => false,
    })
}

/// Before replacing or closing the open project: offer to save the code editor's files
/// with unsaved edits. False = the user cancelled.
fn keep_code(app: &AppHandle) -> anyhow::Result<bool> {
    let server = app.state::<Native>().server.clone();
    let files = server.graph.unsaved();
    let Some(first) = files.first() else { return Ok(true) };
    let what = match files.len() {
        1 => format!("{first} has edits"),
        n => format!("{first} and {} more have edits", n - 1),
    };
    let mut dialog = app
        .dialog()
        .message(format!("{what} in the code editor that you haven't saved. They'll be lost if you don't save them."))
        .title("Do you want to save your code?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom("Save".into(), "Don't Save".into(), "Cancel".into()));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    match dialog.blocking_show_with_result() {
        MessageDialogResult::Yes => save_code(app),
        MessageDialogResult::Custom(s) if s == "Save" => save_code(app),
        MessageDialogResult::No => Ok(true),
        MessageDialogResult::Custom(s) if s == "Don't Save" => Ok(true),
        _ => Ok(false),
    }
}

/// Have the code editor save every file, and wait until it says none is left unsaved.
fn save_code(app: &AppHandle) -> anyhow::Result<bool> {
    let server = app.state::<Native>().server.clone();
    app.emit("menu", "save_all")?;
    let start = Instant::now();
    while !server.graph.unsaved().is_empty() {
        if start.elapsed() > Duration::from_secs(5) {
            anyhow::bail!("The code editor couldn't save every file (see its banner), so nothing was closed.");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(true)
}

fn error(app: &AppHandle, msg: &str) {
    let mut dialog = app.dialog().message(msg).title("music2").kind(MessageDialogKind::Error);
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    dialog.blocking_show();
}

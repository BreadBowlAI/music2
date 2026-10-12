//! Native actions: the Open Project Folder dialog, Show in Finder, the UI's "ready"
//! handshake, and the prompt about unsaved code. They block on dialogs, so each runs on its
//! own thread; the UI updates itself through the server's WebSocket like after any other edit.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

use crate::Native;

/// One dialog flow at a time (a second ⌘O while the first panel is open is ignored).
static BUSY: AtomicBool = AtomicBool::new(false);

/// Called from the UI: "ready" once it has loaded, and "open_project" from the start
/// screen's button (the same as the menu item).
#[tauri::command]
pub fn action(app: AppHandle, id: String) {
    run(&app, &id);
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
            let picked = app.dialog().file().set_title("Open Project Folder").blocking_pick_folder();
            if let Some(dir) = picked.and_then(|p| p.into_path().ok())
                && keep_code(app)?
            {
                open_project(&server, &dir)?;
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
            if keep_code(app)? {
                app.exit(0);
            }
        }
        other => anyhow::bail!("unknown action {other}"),
    }
    Ok(())
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

/// Open a song project folder and tell the UI to show it.
pub fn open_project(server: &m2_server::AppState, dir: &Path) -> anyhow::Result<()> {
    server.graph.open(dir)?;
    server.graph_changed();
    Ok(())
}

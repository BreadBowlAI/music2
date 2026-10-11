//! Native file actions. They block on dialogs, so each runs on its own thread; the UI
//! updates itself through the engine's WebSocket like after any other edit.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use m2_server::library_dir;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult};

use crate::Native;

/// One dialog flow at a time (a second ⌘O while the first panel is open is ignored).
static BUSY: AtomicBool = AtomicBool::new(false);

/// Called from the UI: right-click menus, drag-and-drop, and buttons that want native
/// behaviour (e.g. "Import audio here…").
#[tauri::command]
pub fn action(app: AppHandle, id: String, args: Option<Value>) {
    run(&app, &id, args);
}

pub fn run(app: &AppHandle, id: &str, args: Option<Value>) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, id) = (app.clone(), id.to_string());
    std::thread::spawn(move || {
        if let Err(e) = perform(&app, &id, args.unwrap_or_default()) {
            error(&app, &format!("{e:#}"));
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

fn perform(app: &AppHandle, id: &str, args: Value) -> anyhow::Result<()> {
    let engine = app.state::<Native>().engine.clone();
    let bar = || args.get("bar").and_then(Value::as_f64).unwrap_or_else(|| engine.playhead_bar());
    match id {
        "new" => {
            if keep_going(app)? {
                engine.new_song()?;
            }
        }
        "open" => {
            let picked = app
                .dialog()
                .file()
                .set_title("Open Song")
                .add_filter("music2 song", &["m2song"])
                .set_directory(library_dir())
                .blocking_pick_file();
            if let Some(path) = picked.and_then(|p| p.into_path().ok())
                && keep_going(app)?
            {
                engine.open_song(&path)?;
            }
        }
        "open_path" => {
            let path = PathBuf::from(args["path"].as_str().unwrap_or_default());
            if keep_going(app)? {
                engine.open_song(&path)?;
            }
        }
        "save" => {
            save(app, false)?;
        }
        "save_as" => {
            save(app, true)?;
        }
        "import" | "import_instrument" => {
            let picked = app
                .dialog()
                .file()
                .set_title(if id == "import" { "Import Audio" } else { "Choose a Sound for the Sampler" })
                .add_filter("WAV audio", &["wav", "wave"])
                .blocking_pick_file();
            if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
                engine.import_audio_file(&path, id == "import_instrument", bar())?;
                engine.toast(format!("Imported {}", file_name(&path)));
            }
        }
        "import_paths" => {
            // Files dropped from Finder. Songs open; sounds become clips at the drop point.
            let paths: Vec<PathBuf> = args["paths"].as_array().into_iter().flatten().filter_map(Value::as_str).map(PathBuf::from).collect();
            if let Some(song) = paths.iter().find(|p| ext(p) == "m2song") {
                if keep_going(app)? {
                    engine.open_song(song)?;
                }
                return Ok(());
            }
            for p in paths.iter().filter(|p| matches!(ext(p).as_str(), "wav" | "wave")) {
                engine.import_audio_file(p, false, bar())?;
                engine.toast(format!("Imported {}", file_name(p)));
            }
            if paths.iter().any(|p| !matches!(ext(p).as_str(), "wav" | "wave")) {
                engine.toast("music2 can import WAV files for now");
            }
        }
        "export" => {
            let title = engine.session.lock().song().title.clone();
            let dir = library_dir().join("exports");
            let _ = std::fs::create_dir_all(&dir);
            let picked = app
                .dialog()
                .file()
                .set_title("Export as WAV")
                .add_filter("WAV audio", &["wav"])
                .set_directory(dir)
                .set_file_name(format!("{title}.wav"))
                .blocking_save_file();
            if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
                engine.toast(format!("Exporting {}…", file_name(&path)));
                let path = engine.export_wav(Some(path))?;
                engine.toast(format!("Exported {}", file_name(&path)));
                m2_server::reveal(&path)?;
            }
        }
        "reveal" => {
            let path = engine.session.lock().path().map(Path::to_path_buf);
            m2_server::reveal(&path.unwrap_or_else(library_dir))?;
        }
        "ready" => println!("music2: UI connected to the native shell"),
        "quit" => {
            if keep_going(app)? {
                app.exit(0);
            }
        }
        other => anyhow::bail!("unknown action {other}"),
    }
    Ok(())
}

/// Save; asks where when the song has no file yet (or for Save As). Returns false if
/// the user cancelled.
fn save(app: &AppHandle, choose: bool) -> anyhow::Result<bool> {
    let engine = app.state::<Native>().engine.clone();
    let (title, current) = {
        let s = engine.session.lock();
        (s.song().title.clone(), s.path().map(Path::to_path_buf))
    };
    let path = match current {
        Some(p) if !choose => p,
        _ => {
            let dir = library_dir();
            let _ = std::fs::create_dir_all(&dir);
            let picked = app
                .dialog()
                .file()
                .set_title(if choose { "Save As" } else { "Save Song" })
                .add_filter("music2 song", &["m2song"])
                .set_directory(dir)
                .set_file_name(format!("{title}.m2song"))
                .blocking_save_file();
            match picked.and_then(|p| p.into_path().ok()) {
                Some(p) => p,
                None => return Ok(false),
            }
        }
    };
    let path = engine.save(Some(&path), None)?;
    engine.toast(format!("Saved {}", file_name(&path)));
    Ok(true)
}

/// Before replacing the song: offer to save unsaved changes. False = the user cancelled.
fn keep_going(app: &AppHandle) -> anyhow::Result<bool> {
    let engine = app.state::<Native>().engine.clone();
    if !engine.is_dirty() {
        return Ok(true);
    }
    let title = engine.session.lock().song().title.clone();
    let mut dialog = app
        .dialog()
        .message("Your changes will be lost if you don't save them.")
        .title(format!("Do you want to save the changes you made to “{title}”?"))
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom("Save".into(), "Don't Save".into(), "Cancel".into()));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    match dialog.blocking_show_with_result() {
        MessageDialogResult::Yes => save(app, false),
        MessageDialogResult::Custom(s) if s == "Save" => save(app, false),
        MessageDialogResult::No => Ok(true),
        MessageDialogResult::Custom(s) if s == "Don't Save" => Ok(true),
        _ => Ok(false),
    }
}

fn error(app: &AppHandle, msg: &str) {
    let mut dialog = app.dialog().message(msg).title("music2").kind(MessageDialogKind::Error);
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.parent(&w);
    }
    dialog.blocking_show();
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

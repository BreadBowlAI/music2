//! Native actions: the Open Project Folder dialog, Open Recent, Show in Finder, the UI's "ready"
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
/// picked for a new song (the New Song dialog's Choose Folder…), starting at `arg`: a folder, or
/// `{"start": folder, "title": words}` (Settings' "Choose Where New Songs Go", plan X38).
/// "open_file" returns the files picked in an open dialog, as a JSON list (File ▸ Import Audio…).
/// "save_file" returns the file picked in a save dialog (Export ▸ MIDI, a clip's Export as MIDI…).
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
        "open_file" => tauri::async_runtime::spawn_blocking(move || {
            if BUSY.swap(true, Ordering::SeqCst) {
                return Ok(None);
            }
            let picked = open_file(&app, arg);
            BUSY.store(false, Ordering::SeqCst);
            picked
        }),
        "save_file" => tauri::async_runtime::spawn_blocking(move || {
            if BUSY.swap(true, Ordering::SeqCst) {
                return Ok(None);
            }
            let picked = save_file(&app, arg);
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

/// What the page asks of the open dialog (P0.5, built in studio lane A): `{"title", "filters":
/// [{"name", "extensions"}], "multiple"}`. Read by hand, as `folder_ask` reads its ask.
#[derive(Debug, Default, PartialEq)]
struct OpenAsk {
    title: String,
    /// Each filter's name and extensions.
    filters: Vec<(String, Vec<String>)>,
    multiple: bool,
}

/// A string of the page's ask, or "".
fn text(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

/// The ask's filters: each one's name and extensions.
fn filters(v: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    v["filters"]
        .as_array()
        .map(|fs| {
            fs.iter()
                .map(|f| (text(&f["name"]), f["extensions"].as_array().map(|e| e.iter().map(text).collect()).unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default()
}

fn open_ask(arg: Option<&str>) -> OpenAsk {
    let v: serde_json::Value = arg.and_then(|a| serde_json::from_str(a).ok()).unwrap_or_default();
    OpenAsk { title: text(&v["title"]), filters: filters(&v), multiple: v["multiple"].as_bool().unwrap_or(false) }
}

/// What the page asks of the save dialog (the review's F1): `{"title", "name", "start", "filters":
/// [{"name", "extensions"}]}`: the file's name, the folder it starts in.
#[derive(Debug, Default, PartialEq)]
struct SaveAsk {
    title: String,
    name: String,
    start: String,
    filters: Vec<(String, Vec<String>)>,
}

fn save_ask(arg: Option<&str>) -> SaveAsk {
    let v: serde_json::Value = arg.and_then(|a| serde_json::from_str(a).ok()).unwrap_or_default();
    SaveAsk { title: text(&v["title"]), name: text(&v["name"]), start: text(&v["start"]), filters: filters(&v) }
}

/// The file picked in the save dialog (Export ▸ MIDI, a clip's Export as MIDI…), or None when
/// cancelled.
fn save_file(app: &AppHandle, arg: Option<String>) -> anyhow::Result<Option<String>> {
    let ask = save_ask(arg.as_deref());
    let mut dialog = app.dialog().file().set_title(if ask.title.is_empty() { "Save" } else { &ask.title }).set_can_create_directories(true);
    for (name, extensions) in &ask.filters {
        let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &exts);
    }
    if Path::new(&ask.start).is_dir() {
        dialog = dialog.set_directory(&ask.start);
    }
    if !ask.name.is_empty() {
        dialog = dialog.set_file_name(&ask.name);
    }
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    Ok(dialog.blocking_save_file().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// Files picked in the open dialog (File ▸ Import Audio…): a JSON list of their paths, or None
/// when cancelled.
fn open_file(app: &AppHandle, arg: Option<String>) -> anyhow::Result<Option<String>> {
    let ask = open_ask(arg.as_deref());
    let mut dialog = app.dialog().file().set_title(if ask.title.is_empty() { "Open" } else { &ask.title });
    for (name, extensions) in &ask.filters {
        let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(name, &exts);
    }
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    let paths: Option<Vec<String>> = match ask.multiple {
        true => dialog
            .blocking_pick_files()
            .map(|ps| ps.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.display().to_string()).collect()),
        false => dialog.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| vec![p.display().to_string()]),
    };
    Ok(paths.map(|p| serde_json::to_string(&p).expect("paths serialise")))
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
/// the songs folder, which is made if it isn't there yet. `arg` is the folder, or JSON with the
/// folder and the dialog's title ([`folder_ask`]).
fn song_folder(app: &AppHandle, arg: Option<String>) -> anyhow::Result<Option<String>> {
    let (start, title) = folder_ask(arg);
    let dir = match start.map(PathBuf::from).filter(|d| d.is_dir()) {
        Some(d) => d,
        None => m2_server::project::make_songs_dir()?,
    };
    let mut dialog = app.dialog().file().set_title(title).set_directory(dir).set_can_create_directories(true);
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    Ok(dialog.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string()))
}

/// What the page asks of the folder dialog: where it starts, and its title. A plain folder (New
/// Song, Save As) is titled "Choose Where to Save the Song"; Settings sends
/// `{"start": …, "title": "Choose Where New Songs Go"}`.
fn folder_ask(arg: Option<String>) -> (Option<String>, String) {
    const SONG: &str = "Choose Where to Save the Song";
    match arg.as_deref().map(serde_json::from_str::<serde_json::Value>) {
        Some(Ok(serde_json::Value::Object(o))) => {
            (o.get("start").and_then(|v| v.as_str()).map(String::from), o.get("title").and_then(|v| v.as_str()).unwrap_or(SONG).to_string())
        }
        _ => (arg, SONG.to_string()),
    }
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
        // File ▸ Open Recent ▸ a song: unsaved code is asked about first, as Open does.
        id if id.starts_with(crate::menu::OPEN_RECENT) => {
            let dir = PathBuf::from(&id[crate::menu::OPEN_RECENT.len()..]);
            if !dir.join("song.ts").is_file() {
                anyhow::bail!("{} isn't there any more: it was moved or deleted.", dir.display());
            }
            if keep_code(app)? {
                plugin_host::open_project(&server, &dir)?;
            }
        }
        "clear_recent" => {
            if let Some(recent) = server.recent() {
                recent.clear()?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Plan X38: the folder dialog is titled for what it picks: a song's folder (a plain path, as
    /// New Song and Save As send it), or where new songs go (Settings sends its title).
    #[test]
    fn the_folder_dialog_takes_its_title_from_the_page() {
        let song = "Choose Where to Save the Song".to_string();
        assert_eq!(folder_ask(Some("/Users/me/Music".into())), (Some("/Users/me/Music".into()), song.clone()));
        assert_eq!(folder_ask(None), (None, song));
        let settings = r#"{"start": "/Users/me/Songs", "title": "Choose Where New Songs Go"}"#;
        assert_eq!(folder_ask(Some(settings.into())), (Some("/Users/me/Songs".into()), "Choose Where New Songs Go".into()));
    }

    /// P0.5 (studio lane A): the open dialog takes its title, filters and whether it picks several
    /// from the page.
    #[test]
    fn the_open_dialog_takes_its_ask_from_the_page() {
        let arg = r#"{"title": "Import Audio", "filters": [{"name": "WAV files", "extensions": ["wav", "wave"]}], "multiple": true}"#;
        let ask = open_ask(Some(arg));
        assert_eq!(ask.title, "Import Audio");
        assert!(ask.multiple);
        assert_eq!(ask.filters, [("WAV files".to_string(), vec!["wav".to_string(), "wave".to_string()])]);
        assert_eq!(open_ask(Some("{}")), OpenAsk::default());
        assert_eq!(open_ask(None), OpenAsk::default());
    }

    /// Whether the shell has the action `id`: one of `action`'s arms that answer the page, or one
    /// `perform` runs.
    fn known(id: &str) -> bool {
        matches!(id, "export_path" | "reveal_file" | "song_folder" | "open_file" | "save_file")
            || matches!(id, "open_project" | "clear_recent" | "reveal" | "ready" | "quit")
            || id.starts_with(crate::menu::OPEN_RECENT)
    }

    /// The review's F1: every action the page asks for (`nativeAction("…")` in web/src) has an arm
    /// here. One without reaches `perform`, whose "unknown action" is an error box, and the page
    /// hears nothing.
    #[test]
    fn every_action_the_page_asks_for_is_known() {
        fn files(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x == "ts" || x == "tsx") {
                    out.push(p);
                }
            }
        }
        let mut all = vec![];
        files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src"), &mut all);
        let mut asked = std::collections::BTreeSet::new();
        for f in all {
            let text = std::fs::read_to_string(&f).unwrap();
            for (i, call) in text.match_indices("nativeAction(\"") {
                let rest = &text[i + call.len()..];
                asked.insert(rest[..rest.find('"').unwrap()].to_string());
            }
        }
        assert!(asked.contains("open_file") && asked.contains("save_file"), "{asked:?}");
        let unknown: Vec<&String> = asked.iter().filter(|id| !known(id)).collect();
        assert!(unknown.is_empty(), "the page asks for actions the app doesn't have: {unknown:?}");
    }

    /// The save dialog takes its title, file name, folder and filters from the page (Export ▸ MIDI,
    /// a clip's Export as MIDI…).
    #[test]
    fn the_save_dialog_takes_its_ask_from_the_page() {
        let arg = r#"{"title": "Export MIDI", "name": "First.mid", "start": "/Users/me/Music/music2/Exports", "filters": [{"name": "MIDI file", "extensions": ["mid", "midi"]}]}"#;
        let ask = save_ask(Some(arg));
        assert_eq!(
            (ask.title.as_str(), ask.name.as_str(), ask.start.as_str()),
            ("Export MIDI", "First.mid", "/Users/me/Music/music2/Exports")
        );
        assert_eq!(ask.filters, [("MIDI file".to_string(), vec!["mid".to_string(), "midi".to_string()])]);
        assert_eq!(save_ask(None), SaveAsk::default());
    }
}

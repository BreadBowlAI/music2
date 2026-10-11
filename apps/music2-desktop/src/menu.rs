//! The menu bar. File actions run here in Rust (dialogs + engine); everything that is
//! about the UI (undo in a text field vs. the song, panels, transport) is forwarded to
//! the web view as a `menu` event with the item's id.

use tauri::menu::{AboutMetadata, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::actions;

/// Item ids handled natively (see `actions::run`); every other id goes to the UI.
const NATIVE: &[&str] = &["new", "open", "open_project", "save", "save_as", "import", "export", "reveal", "quit"];

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, text);
        match accel {
            Some(a) => b.accelerator(a).build(app),
            None => b.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, "music2")
        .item(&PredefinedMenuItem::about(app, Some("About music2"), Some(AboutMetadata { name: Some("music2".into()), ..Default::default() }))?)
        .separator()
        .item(&item("settings", "Settings…", Some("CmdOrCtrl+,"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .item(&item("quit", "Quit music2", Some("CmdOrCtrl+Q"))?)
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new", "New Song", Some("CmdOrCtrl+N"))?)
        .item(&item("open", "Open…", Some("CmdOrCtrl+O"))?)
        .item(&item("open_project", "Open Project Folder…", Some("CmdOrCtrl+Alt+O"))?)
        .item(&item("library", "Song Library…", Some("CmdOrCtrl+Shift+O"))?)
        .separator()
        .item(&item("save", "Save", Some("CmdOrCtrl+S"))?)
        .item(&item("save_as", "Save As…", Some("CmdOrCtrl+Shift+S"))?)
        .separator()
        .item(&item("import", "Import Audio…", Some("CmdOrCtrl+I"))?)
        .item(&item("export", "Export as WAV…", Some("CmdOrCtrl+E"))?)
        .separator()
        .item(&item("reveal", "Show in Finder", None)?)
        .separator()
        .close_window()
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&item("undo", "Undo", Some("CmdOrCtrl+Z"))?)
        .item(&item("redo", "Redo", Some("CmdOrCtrl+Shift+Z"))?)
        .separator()
        // Native items so text fields get real cut/copy/paste.
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item("duplicate", "Duplicate Clip", Some("CmdOrCtrl+D"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("panel:track", "Track Inspector", Some("CmdOrCtrl+1"))?)
        .item(&item("panel:sounds", "Sounds", Some("CmdOrCtrl+2"))?)
        .separator()
        .item(&item("zoom_in", "Zoom In", Some("CmdOrCtrl+="))?)
        .item(&item("zoom_out", "Zoom Out", Some("CmdOrCtrl+-"))?)
        .separator()
        .fullscreen()
        .build()?;

    // No Space accelerator: the menu would steal it from text fields. The UI handles Space.
    let transport = SubmenuBuilder::new(app, "Transport")
        .item(&item("play", "Play / Pause", None)?)
        .item(&item("home", "Go to Start", None)?)
        .item(&item("loop", "Loop", Some("CmdOrCtrl+L"))?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().separator().close_window().build()?;

    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &view, &transport, &window]).build()
}

pub fn handle(app: &AppHandle, id: &str) {
    if NATIVE.contains(&id) {
        actions::run(app, id, None);
    } else {
        let _ = app.emit("menu", id);
    }
}

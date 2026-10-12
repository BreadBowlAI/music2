//! The menu bar. Opening a project folder, Show in Finder and Quit run here in Rust (dialogs);
//! everything about the UI (save in the code editor, undo in a text field vs. the project,
//! transport) is forwarded to the web view as a `menu` event with the item's id.

use tauri::menu::{AboutMetadata, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::actions;

/// Item ids handled natively (see `actions::run`); every other id goes to the UI, Save
/// included: the code editor saves its files.
const NATIVE: &[&str] = &["open_project", "reveal", "quit"];

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, text);
        match accel {
            Some(a) => b.accelerator(a).build(app),
            None => b.build(app),
        }
    };

    let app_menu = SubmenuBuilder::new(app, "music2")
        .item(&PredefinedMenuItem::about(
            app,
            Some("About music2"),
            Some(AboutMetadata { name: Some("music2".into()), ..Default::default() }),
        )?)
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
        // The UI shows the New Song dialog, even with a song open.
        .item(&item("new_song", "New Song…", Some("CmdOrCtrl+N"))?)
        .item(&item("open_project", "Open Project Folder…", Some("CmdOrCtrl+O"))?)
        .separator()
        .item(&item("save", "Save", Some("CmdOrCtrl+S"))?)
        .separator()
        // The UI asks for the file (actions.rs `export_path`), then shows the progress.
        .item(&item("export", "Export Audio…", Some("CmdOrCtrl+E"))?)
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
        .item(&item("duplicate", "Duplicate", Some("CmdOrCtrl+D"))?)
        .build()?;

    let view = SubmenuBuilder::new(app, "View").fullscreen().build()?;

    // No Space accelerator: the menu would steal it from text fields. The UI handles Space.
    let transport = SubmenuBuilder::new(app, "Transport")
        .item(&item("play", "Play / Pause", None)?)
        .item(&item("home", "Go to Start", None)?)
        .build()?;

    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().separator().close_window().build()?;

    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &view, &transport, &window]).build()
}

pub fn handle(app: &AppHandle, id: &str) {
    if NATIVE.contains(&id) {
        actions::run(app, id);
    } else {
        let _ = app.emit("menu", id);
    }
}

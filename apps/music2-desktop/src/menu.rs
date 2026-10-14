//! The menu bar. Opening a project folder, Show in Finder and Quit run here in Rust (dialogs);
//! everything about the UI (save in the code editor, undo in a text field vs. the project,
//! transport) is forwarded to the web view as a `menu` event with the item's id.
//!
//! Only ⌘ and ⌃ chords are accelerators (taste.md §5.3): a native accelerator fires even while
//! the producer types in the code editor or a rename field, so single keys, Space, Return and ⌥
//! chords are the page's, which ignores them while typing. Their items are here without one; the
//! ⌥K sheet lists every key. Where focus is decides what a chord does (the code editor's own ⌘D),
//! in the page's `appCommand`.

use tauri::menu::{AboutMetadata, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::actions;

/// Item ids handled natively (see `actions::run`); every other id goes to the UI, Save
/// included: the code editor saves its files.
const NATIVE: &[&str] = &["open_project", "reveal", "quit"];

/// The menu bar's items, as (menu, id, title, accelerator): the ids are the page's command ids.
/// `None` for a key the page handles, or an item with no key.
pub const ITEMS: &[(&str, &str, &str, Option<&str>)] = &[
    ("music2", "quit", "Quit music2", Some("CmdOrCtrl+Q")),
    // The UI shows the New Song dialog, even with a song open.
    ("File", "new_song", "New Song…", Some("CmdOrCtrl+N")),
    ("File", "open_project", "Open Project Folder…", Some("CmdOrCtrl+O")),
    // The UI asks about unsaved code first, then the start screen shows.
    ("File", "close_song", "Close Song", Some("CmdOrCtrl+W")),
    ("File", "save", "Save", Some("CmdOrCtrl+S")),
    // The UI asks for the file (actions.rs `export_path`), then shows the progress. ⌘E is kept for
    // Split, which comes with clip editing.
    ("File", "export", "Export Audio…", Some("CmdOrCtrl+Shift+E")),
    ("File", "reveal", "Show in Finder", None),
    // The song's name in the top bar turns into a field (a double-click there does the same).
    ("File", "rename_song", "Rename Song…", None),
    ("Edit", "undo", "Undo", Some("CmdOrCtrl+Z")),
    ("Edit", "redo", "Redo", Some("CmdOrCtrl+Shift+Z")),
    // One item for whatever is selected (the roll's notes today), or the code editor's own ⌘D.
    ("Edit", "duplicate", "Duplicate", Some("CmdOrCtrl+D")),
    // The instrument list at + Track; the selected track goes, asking first when others read it.
    ("Track", "new_track", "New Track…", Some("CmdOrCtrl+T")),
    ("Track", "delete_track", "Delete Track", Some("CmdOrCtrl+Backspace")),
    ("Track", "rename", "Rename", Some("CmdOrCtrl+R")),
    // ⌥1 to ⌥4, X, A, E: the page's keys.
    ("View", "view_device", "Device", None),
    ("View", "view_notes", "Notes", None),
    ("View", "view_mix", "Mix", None),
    ("View", "view_code", "Code", None),
    ("View", "lanes", "Show Automation", None),
    ("View", "dock", "Show Dock", None),
    ("View", "browser", "Show Browser", Some("Alt+CmdOrCtrl+B")),
    // No Space accelerator: the menu would steal it from text fields. The UI handles Space, Return and C.
    ("Transport", "play", "Play / Stop", None),
    // ⇧Space, the page's: from where it stopped, not where play started.
    ("Transport", "continue", "Continue", None),
    ("Transport", "home", "Go to Start", None),
    ("Transport", "loop", "Loop", None),
    // The selected clip's span, else the bar at the playhead.
    ("Transport", "loop_selection", "Loop Selection", Some("CmdOrCtrl+L")),
    // Panic (RENDER-10): every note off, every instance reset.
    ("Transport", "panic", "Stop All Sound", Some("CmdOrCtrl+Period")),
    ("Help", "shortcuts", "Keyboard Shortcuts", None),
];

/// Window → Close Window, which quits (as the window's close button does): ⌘W is Close Song.
const CLOSE_WINDOW: &str = "Shift+CmdOrCtrl+W";

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str| {
        let &(_, id, text, accel) = ITEMS.iter().find(|i| i.1 == id).expect("a menu item");
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
        .item(&item("quit")?)
        .build()?;

    let file = SubmenuBuilder::new(app, "File")
        .item(&item("new_song")?)
        .item(&item("open_project")?)
        .separator()
        .item(&item("close_song")?)
        .item(&item("save")?)
        .separator()
        .item(&item("export")?)
        .separator()
        .item(&item("reveal")?)
        .item(&item("rename_song")?)
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&item("undo")?)
        .item(&item("redo")?)
        .separator()
        // Native items so text fields get real cut/copy/paste.
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&item("duplicate")?)
        .build()?;

    let track = SubmenuBuilder::new(app, "Track").item(&item("new_track")?).item(&item("delete_track")?).item(&item("rename")?).build()?;

    let view = SubmenuBuilder::new(app, "View")
        .item(&item("view_device")?)
        .item(&item("view_notes")?)
        .item(&item("view_mix")?)
        .item(&item("view_code")?)
        .separator()
        .item(&item("lanes")?)
        .item(&item("dock")?)
        .item(&item("browser")?)
        .separator()
        .fullscreen()
        .build()?;

    let transport = SubmenuBuilder::new(app, "Transport")
        .item(&item("play")?)
        .item(&item("continue")?)
        .item(&item("home")?)
        .separator()
        .item(&item("loop")?)
        .item(&item("loop_selection")?)
        .separator()
        .item(&item("panic")?)
        .build()?;

    // ⌘W closes the song (File); closing the window, which quits, is ⇧⌘W.
    let close_window = MenuItemBuilder::with_id("close_window", "Close Window").accelerator(CLOSE_WINDOW).build(app)?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().separator().item(&close_window).build()?;

    let help = SubmenuBuilder::new(app, "Help").item(&item("shortcuts")?).build()?;

    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &track, &view, &transport, &window, &help]).build()
}

pub fn handle(app: &AppHandle, id: &str) {
    if id == "close_window" {
        // Closing the window quits: ask about unsaved code first, as its close button does.
        actions::run(app, "quit");
    } else if NATIVE.contains(&id) {
        actions::run(app, id);
    } else {
        let _ = app.emit("menu", id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The accelerators that are native.
    fn accelerators() -> impl Iterator<Item = &'static str> {
        ITEMS.iter().filter_map(|i| i.3).chain([CLOSE_WINDOW])
    }

    /// taste.md §5.3: a native accelerator fires while typing, so every one is a ⌘ or ⌃ chord.
    #[test]
    fn every_accelerator_is_a_command_or_control_chord() {
        for a in accelerators() {
            let parts: Vec<&str> = a.split('+').collect();
            assert!(parts.iter().any(|p| matches!(*p, "CmdOrCtrl" | "Cmd" | "Ctrl" | "Control")), "{a} is not a ⌘ or ⌃ chord");
            assert_eq!(parts.len(), parts.iter().collect::<std::collections::BTreeSet<_>>().len(), "{a}");
        }
    }

    #[test]
    fn no_two_items_share_an_id_or_an_accelerator() {
        let ids: std::collections::BTreeSet<_> = ITEMS.iter().map(|i| i.1).collect();
        assert_eq!(ids.len(), ITEMS.len());
        let accels: Vec<_> = accelerators().collect();
        let unique: std::collections::BTreeSet<_> = accels.iter().collect();
        assert_eq!(unique.len(), accels.len(), "{accels:?}");
    }

    /// Export moved off ⌘E (kept for Split, taste.md §5.1), and ⌘W is Close Song.
    #[test]
    fn export_is_shift_command_e_and_command_w_closes_the_song() {
        let accel = |id: &str| ITEMS.iter().find(|i| i.1 == id).and_then(|i| i.3);
        assert_eq!(accel("export"), Some("CmdOrCtrl+Shift+E"));
        assert_eq!(accel("close_song"), Some("CmdOrCtrl+W"));
        assert!(!accelerators().any(|a| a == "CmdOrCtrl+E"));
    }
}

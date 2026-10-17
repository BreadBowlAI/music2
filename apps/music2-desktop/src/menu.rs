//! The menu bar. Opening a project folder, Show in Finder and Quit run here in Rust (dialogs);
//! everything about the UI (save in the code editor, undo in a text field vs. the project,
//! transport) is forwarded to the web view as a `menu` event with the item's id.
//!
//! Only ⌘ and ⌃ chords are accelerators (taste.md §5.3): a native accelerator fires even while
//! the producer types in the code editor or a rename field, so single keys, Space, Return and ⌥
//! chords are the page's, which ignores them while typing. Their items are here without one; the
//! ⌥K sheet lists every key. Where focus is decides what a chord does (the code editor's own ⌘D,
//! a field's own ⌘⌫), in the page's `appCommand`.
//!
//! One menu bar per OS (OS-54). The Mac's has the app menu (About, Services, Hide, Quit) and a
//! Window menu. Elsewhere (GTK) those items are greyed out and the app menu isn't a convention:
//! Quit (Ctrl+Q) ends File, About ends Help, and no Window menu. `CmdOrCtrl` is ⌘ on a Mac and
//! Ctrl elsewhere.

use tauri::menu::{AboutMetadata, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::actions;

/// Item ids handled natively (see `actions::run`); every other id goes to the UI, Save
/// included: the code editor saves its files.
const NATIVE: &[&str] = &["open_project", "reveal", "quit"];

/// Whether this build's menu bar is the Mac's.
const MAC: bool = cfg!(target_os = "macos");

/// The reveal item: Finder on a Mac; elsewhere the system's file manager (OS-55), as the export
/// toast says it.
const REVEAL: &str = if MAC { "Show in Finder" } else { "Show in Folder" };

/// Mute's and Solo's keys: ⌃M and ⌃S on a Mac only (taste.md §5.3). Elsewhere Ctrl is ⌘'s, and
/// Ctrl+S saves; M and S on the selected track are the page's keys everywhere.
const MUTE_KEY: Option<&str> = if MAC { Some("Ctrl+M") } else { None };
const SOLO_KEY: Option<&str> = if MAC { Some("Ctrl+S") } else { None };

/// The menu bar's items, as (id, title, accelerator): the ids are the page's command ids.
/// `None` for a key the page handles, or an item with no key. Where each goes: `layout`.
const ITEMS: &[(&str, &str, Option<&str>)] = &[
    ("quit", "Quit music2", Some("CmdOrCtrl+Q")),
    // The UI shows the New Song dialog, even with a song open.
    ("new_song", "New Song…", Some("CmdOrCtrl+N")),
    ("open_project", "Open Project Folder…", Some("CmdOrCtrl+O")),
    // The UI asks about unsaved code first, then the start screen shows.
    ("close_song", "Close Song", Some("CmdOrCtrl+W")),
    ("save", "Save", Some("CmdOrCtrl+S")),
    // The UI asks for the file (actions.rs `export_path`), then shows the progress. ⌘E is Split.
    ("export", "Export Audio…", Some("CmdOrCtrl+Shift+E")),
    ("reveal", REVEAL, None),
    // The song's name in the top bar turns into a field (a double-click there does the same).
    ("rename_song", "Rename Song…", None),
    ("undo", "Undo", Some("CmdOrCtrl+Z")),
    ("redo", "Redo", Some("CmdOrCtrl+Shift+Z")),
    // One item for whatever is selected (the roll's notes, the timeline's clips, the track), or the
    // code editor's own ⌘D.
    ("duplicate", "Duplicate", Some("CmdOrCtrl+D")),
    // The timeline's selected clips (8b.5): at the playhead; notes clips of one lane as one; their
    // notes onto the roll's grid (Q, the page's key). In the code editor ⌘E and ⌘J are its own.
    ("split", "Split at Playhead", Some("CmdOrCtrl+E")),
    ("join", "Join Clips", Some("CmdOrCtrl+J")),
    ("quantize", "Quantize", None),
    // The instrument list at + Track; the selected track goes, asking first when others read it.
    ("new_track", "New Track…", Some("CmdOrCtrl+T")),
    // In a field, the field's own: to the line's start on a Mac, the word before elsewhere.
    ("delete_track", "Delete Track", Some("CmdOrCtrl+Backspace")),
    ("rename", "Rename", Some("CmdOrCtrl+R")),
    // While listening, on the selected track (not undo steps); M and S are the page's keys.
    ("mute", "Mute", MUTE_KEY),
    ("solo", "Solo", SOLO_KEY),
    // In the track's code: one undo step.
    ("mute_in_song", "Mute in Song", None),
    // ⌥1 to ⌥4, X, A, E: the page's keys.
    ("view_device", "Device", None),
    ("view_notes", "Notes", None),
    ("view_mix", "Mix", None),
    ("view_code", "Code", None),
    ("lanes", "Show Automation", None),
    ("dock", "Show Dock", None),
    ("browser", "Show Browser", Some("Alt+CmdOrCtrl+B")),
    // The grid of the view the pointer or the keyboard was last in: the timeline's or the roll's.
    ("grid_narrower", "Narrower Grid", Some("CmdOrCtrl+1")),
    ("grid_wider", "Wider Grid", Some("CmdOrCtrl+2")),
    ("grid_triplet", "Triplet Grid", Some("CmdOrCtrl+3")),
    ("grid_snap", "Snap to Grid", Some("CmdOrCtrl+4")),
    // The timeline's zoom, around the playhead or the view's middle (the roll's own while it has
    // the keyboard). ⇧⌘= is the page's too. Z, the page's key, fits the selected clips or the song.
    ("zoom_in", "Zoom In", Some("CmdOrCtrl+=")),
    ("zoom_out", "Zoom Out", Some("CmdOrCtrl+-")),
    ("zoom_fit", "Zoom to Fit", None),
    // The selected track's height, or every track's (in the code editor, its own ⌘↓ and ⌘↑).
    ("taller", "Taller Tracks", Some("CmdOrCtrl+Down")),
    ("shorter", "Shorter Tracks", Some("CmdOrCtrl+Up")),
    // No Space accelerator: the menu would steal it from text fields. The UI handles Space, Return and C.
    ("play", "Play / Stop", None),
    // ⇧Space, the page's: from where it stopped, not where play started.
    ("continue", "Continue", None),
    ("home", "Go to Start", None),
    ("loop", "Loop", None),
    // The selected clip's span, else the bar at the playhead.
    ("loop_selection", "Loop Selection", Some("CmdOrCtrl+L")),
    // The timeline pages to keep the playhead in view while it plays.
    ("follow", "Follow Playhead", Some("CmdOrCtrl+Shift+F")),
    // At the playhead's bar; ⌥← and ⌥→, the page's, move play's start to the marker before or after.
    ("add_marker", "Add Marker", None),
    ("prev_marker", "Previous Marker", None),
    ("next_marker", "Next Marker", None),
    // Panic (RENDER-10): every note off, every instance reset.
    ("panic", "Stop All Sound", Some("CmdOrCtrl+Period")),
    ("shortcuts", "Keyboard Shortcuts", None),
];

/// Window → Close Window, which quits (as the window's close button does): ⌘W is Close Song.
const CLOSE_WINDOW: &str = "Shift+CmdOrCtrl+W";

/// One row of a menu: an item of `ITEMS` by id, a line, or one the system provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Item(&'static str),
    Line,
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Cut,
    Copy,
    Paste,
    SelectAll,
    FullScreen,
    Minimize,
    Maximize,
    CloseWindow,
}

/// The menu bar, as (menu, rows): the Mac's when `mac`, else the one for GTK (and Windows later).
fn layout(mac: bool) -> Vec<(&'static str, Vec<Row>)> {
    use Row::*;
    let file = [
        Item("new_song"),
        Item("open_project"),
        Line,
        Item("close_song"),
        Item("save"),
        Line,
        Item("export"),
        Line,
        Item("reveal"),
        Item("rename_song"),
    ];
    // What is selected: the roll's notes, the timeline's clips, the track.
    let selected = [Item("duplicate"), Item("split"), Item("join"), Item("quantize")];
    let track = vec![Item("new_track"), Item("delete_track"), Item("rename"), Line, Item("mute"), Item("solo"), Item("mute_in_song")];
    let views = [
        Item("zoom_in"),
        Item("zoom_out"),
        Item("zoom_fit"),
        Item("taller"),
        Item("shorter"),
        Line,
        Item("view_device"),
        Item("view_notes"),
        Item("view_mix"),
        Item("view_code"),
        Line,
        Item("lanes"),
        Item("dock"),
        Item("browser"),
        Line,
        Item("grid_narrower"),
        Item("grid_wider"),
        Item("grid_triplet"),
        Item("grid_snap"),
    ];
    let transport = vec![
        Item("play"),
        Item("continue"),
        Item("home"),
        Line,
        Item("loop"),
        Item("loop_selection"),
        Item("follow"),
        Line,
        Item("add_marker"),
        Item("prev_marker"),
        Item("next_marker"),
        Line,
        Item("panic"),
    ];
    if mac {
        vec![
            ("music2", vec![About, Line, Services, Line, Hide, HideOthers, ShowAll, Line, Item("quit")]),
            ("File", file.to_vec()),
            // The system's cut, copy and paste, so text fields get them.
            ("Edit", [&[Item("undo"), Item("redo"), Line, Cut, Copy, Paste, SelectAll, Line][..], &selected].concat()),
            ("Track", track),
            ("View", [&views[..], &[Line, FullScreen]].concat()),
            ("Transport", transport),
            // ⌘W closes the song (File); closing the window, which quits, is ⇧⌘W.
            ("Window", vec![Minimize, Maximize, Line, CloseWindow]),
            ("Help", vec![Item("shortcuts")]),
        ]
    } else {
        // GTK greys out Services, Hide, Full Screen and the Window menu's items, and its Cut,
        // Copy and Paste rows do nothing here (muda needs libxdo for them): the web view has
        // those keys itself, so they aren't rows.
        vec![
            ("File", [&file[..], &[Line, Item("quit")]].concat()),
            ("Edit", [&[Item("undo"), Item("redo"), Line][..], &selected].concat()),
            ("Track", track),
            ("View", views.to_vec()),
            ("Transport", transport),
            ("Help", vec![Item("shortcuts"), Line, About]),
        ]
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let mut bar = MenuBuilder::new(app);
    for (name, rows) in layout(MAC) {
        let mut menu = SubmenuBuilder::new(app, name);
        for row in rows {
            menu = match row {
                Row::Item(id) => {
                    let &(id, text, accel) = ITEMS.iter().find(|i| i.0 == id).expect("a menu item");
                    let b = MenuItemBuilder::with_id(id, text);
                    menu.item(&match accel {
                        Some(a) => b.accelerator(a).build(app)?,
                        None => b.build(app)?,
                    })
                }
                Row::Line => menu.separator(),
                Row::About => menu.item(&PredefinedMenuItem::about(app, Some("About music2"), Some(about(app)))?),
                Row::Services => menu.services(),
                Row::Hide => menu.hide(),
                Row::HideOthers => menu.hide_others(),
                Row::ShowAll => menu.show_all(),
                Row::Cut => menu.cut(),
                Row::Copy => menu.copy(),
                Row::Paste => menu.paste(),
                Row::SelectAll => menu.select_all(),
                Row::FullScreen => menu.fullscreen(),
                Row::Minimize => menu.minimize(),
                Row::Maximize => menu.maximize(),
                Row::CloseWindow => {
                    menu.item(&MenuItemBuilder::with_id("close_window", "Close Window").accelerator(CLOSE_WINDOW).build(app)?)
                }
            };
        }
        bar = bar.item(&menu.build()?);
    }
    bar.build()
}

/// What About says: the Mac's panel finds the version and icon in the app's bundle; GTK's dialog
/// is told them.
fn about(app: &AppHandle) -> AboutMetadata<'static> {
    let mut about = AboutMetadata { name: Some("music2".into()), ..Default::default() };
    if !MAC {
        about.version = Some(env!("CARGO_PKG_VERSION").into());
        about.icon = app.default_window_icon().map(|i| i.clone().to_owned());
    }
    about
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
        ITEMS.iter().filter_map(|i| i.2).chain([CLOSE_WINDOW])
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
        let ids: std::collections::BTreeSet<_> = ITEMS.iter().map(|i| i.0).collect();
        assert_eq!(ids.len(), ITEMS.len());
        let accels: Vec<_> = accelerators().collect();
        let unique: std::collections::BTreeSet<_> = accels.iter().collect();
        assert_eq!(unique.len(), accels.len(), "{accels:?}");
    }

    /// Export moved off ⌘E, which is Split (taste.md §5.1, design-8b §7.4); ⌘J joins; ⌘W is Close Song.
    #[test]
    fn export_is_shift_command_e_split_is_command_e_and_command_w_closes_the_song() {
        let accel = |id: &str| ITEMS.iter().find(|i| i.0 == id).and_then(|i| i.2);
        assert_eq!(accel("export"), Some("CmdOrCtrl+Shift+E"));
        assert_eq!(accel("split"), Some("CmdOrCtrl+E"));
        assert_eq!(accel("join"), Some("CmdOrCtrl+J"));
        assert_eq!(accel("quantize"), None, "Q is the page's key");
        assert_eq!(accel("close_song"), Some("CmdOrCtrl+W"));
    }

    /// The clips' commands sit in Edit on both menu bars, after Duplicate.
    #[test]
    fn split_join_and_quantize_are_in_edit() {
        for mac in [true, false] {
            let bar = layout(mac);
            let edit = &bar.iter().find(|m| m.0 == "Edit").expect("an Edit menu").1;
            let at = edit.iter().position(|r| *r == Row::Item("duplicate")).expect("Duplicate");
            assert_eq!(edit[at + 1..at + 4], [Row::Item("split"), Row::Item("join"), Row::Item("quantize")], "mac: {mac}");
        }
    }

    /// The items each menu bar holds, by id.
    fn items(mac: bool) -> Vec<&'static str> {
        layout(mac).into_iter().flat_map(|(_, rows)| rows).filter_map(|r| if let Row::Item(id) = r { Some(id) } else { None }).collect()
    }

    /// OS-54: every item is in both menu bars, once.
    #[test]
    fn every_item_is_on_each_menu_bar_once() {
        for mac in [true, false] {
            let mut on = items(mac);
            on.sort_unstable();
            let mut all: Vec<_> = ITEMS.iter().map(|i| i.0).collect();
            all.sort_unstable();
            assert_eq!(on, all, "mac: {mac}");
        }
    }

    /// The Mac's menu bar as it was: the app menu first, a Window menu, Full Screen.
    #[test]
    fn the_mac_keeps_its_app_menu_and_window_menu() {
        let bar = layout(true);
        let names: Vec<_> = bar.iter().map(|m| m.0).collect();
        assert_eq!(names, ["music2", "File", "Edit", "Track", "View", "Transport", "Window", "Help"]);
        assert_eq!(bar[0].1.first(), Some(&Row::About));
        assert_eq!(bar[0].1.last(), Some(&Row::Item("quit")));
        assert!(bar[0].1.contains(&Row::Services) && bar[0].1.contains(&Row::Hide));
        assert!(bar[2].1.contains(&Row::Copy) && bar[4].1.contains(&Row::FullScreen) && bar[6].1.contains(&Row::CloseWindow));
    }

    /// OS-54: off the Mac, no app menu; Quit ends File and About ends Help; and no row the system
    /// greys out or that does nothing there (GTK supports only About and lines of its own).
    #[test]
    fn off_the_mac_quit_is_in_file_and_about_in_help() {
        let bar = layout(false);
        let names: Vec<_> = bar.iter().map(|m| m.0).collect();
        assert_eq!(names, ["File", "Edit", "Track", "View", "Transport", "Help"]);
        assert_eq!(bar[0].1.last(), Some(&Row::Item("quit")));
        assert_eq!(bar[5].1.last(), Some(&Row::About));
        for (menu, rows) in &bar {
            for r in rows {
                assert!(matches!(r, Row::Item(_) | Row::Line | Row::About), "{menu}: {r:?}");
            }
        }
    }

    /// The timeline's rows (polish B): zoom and heights lead View, follow and the markers are in
    /// Transport, on both menu bars, with ⌘ chords only where the page has no key of its own.
    #[test]
    fn zoom_heights_follow_and_markers_have_their_rows() {
        let accel = |id: &str| ITEMS.iter().find(|i| i.0 == id).and_then(|i| i.2);
        assert_eq!(accel("zoom_in"), Some("CmdOrCtrl+="));
        assert_eq!(accel("zoom_out"), Some("CmdOrCtrl+-"));
        assert_eq!(accel("zoom_fit"), None, "Z is the page's key");
        assert_eq!(accel("taller"), Some("CmdOrCtrl+Down"));
        assert_eq!(accel("shorter"), Some("CmdOrCtrl+Up"));
        assert_eq!(accel("follow"), Some("CmdOrCtrl+Shift+F"));
        for id in ["add_marker", "prev_marker", "next_marker"] {
            assert_eq!(accel(id), None, "{id}: ⌥← and ⌥→ are the page's keys");
        }
        for mac in [true, false] {
            let bar = layout(mac);
            let menu = |name: &str| bar.iter().find(|m| m.0 == name).expect(name).1.clone();
            let view = menu("View");
            assert_eq!(view[..6], [Row::Item("zoom_in"), Row::Item("zoom_out"), Row::Item("zoom_fit"), Row::Item("taller"), Row::Item("shorter"), Row::Line]);
            let transport = menu("Transport");
            let at = transport.iter().position(|r| *r == Row::Item("loop_selection")).expect("Loop Selection");
            assert_eq!(transport[at + 1], Row::Item("follow"), "mac: {mac}");
            let at = transport.iter().position(|r| *r == Row::Item("add_marker")).expect("Add Marker");
            assert_eq!(transport[at - 1..at + 4], [Row::Line, Row::Item("add_marker"), Row::Item("prev_marker"), Row::Item("next_marker"), Row::Line]);
        }
    }

    /// OS-55: the reveal item names the Mac's Finder only on a Mac.
    #[test]
    fn the_reveal_item_says_finder_only_on_a_mac() {
        let title = ITEMS.iter().find(|i| i.0 == "reveal").map(|i| i.1);
        assert_eq!(title, Some(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in Folder" }));
    }
}

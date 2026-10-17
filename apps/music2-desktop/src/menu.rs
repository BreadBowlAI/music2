//! The menu bar. Opening a project folder, Open Recent, Show in Finder and Quit run here in Rust
//! (dialogs);
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

use std::sync::Mutex;

use tauri::menu::{AboutMetadata, Menu, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::{Native, actions};

/// Item ids handled natively (see `actions::run`); every other id goes to the UI, Save
/// included: the code editor saves its files. Open Recent's songs (`open_recent:<folder>`) too.
const NATIVE: &[&str] = &["open_project", "reveal", "quit", CLEAR_RECENT];

/// File ▸ Open Recent ▸ a song: the id is this and the song's folder.
pub const OPEN_RECENT: &str = "open_recent:";
/// File ▸ Open Recent ▸ Clear Menu.
const CLEAR_RECENT: &str = "clear_recent";
/// How many songs Open Recent shows (the start screen shows all the list keeps).
const RECENT_SHOWN: usize = 10;

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
    // A copy under a new name, which opens (polish C3); the song stays as it was.
    ("save_as", "Save As…", Some("CmdOrCtrl+Shift+S")),
    // WAV files onto the selected audio track at the playhead's bar, else a new audio track (lane
    // A's Q7). Logic's key.
    ("import_audio", "Import Audio…", Some("CmdOrCtrl+Shift+I")),
    // The UI asks for the file (actions.rs `export_path`), then shows the progress. ⌘E is Split.
    ("export", "Export Audio…", Some("CmdOrCtrl+Shift+E")),
    ("reveal", REVEAL, None),
    // The name, and its folder too (polish C3); the top bar's double-click renames the name only.
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
    // The selected track's instrument, from + Track's list (polish A3). No chord.
    ("change_instrument", "Change Instrument…", None),
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
    // The metronome (gaps #19): a click on each beat while the song plays. K is the page's key.
    ("metronome", "Metronome", None),
    // The keyboard piano (Q8): the letter keys play the selected track. ⌘K from anywhere, typing
    // included: the code editor's own ⌘K chords give way to it (X15).
    ("piano", "Keyboard Piano", Some("CmdOrCtrl+K")),
    ("shortcuts", "Keyboard Shortcuts", None),
    // This machine's settings (gaps #31): the sound output, the buffer size, where new songs go.
    // In the app menu on a Mac, after About; in File elsewhere, above Quit.
    ("settings", "Settings…", Some("CmdOrCtrl+Comma")),
];

/// Undo's and Redo's names as the Edit menu says them (gaps #29), and the two items of the menu bar
/// built last: a menu built again (the recent songs changed) is titled from the names.
struct Undo {
    names: (Option<String>, Option<String>),
    items: Option<(MenuItem<Wry>, MenuItem<Wry>)>,
}

static UNDO: Mutex<Undo> = Mutex::new(Undo { names: (None, None), items: None });

/// An item's title: Undo's and Redo's with the name of what they take back ("Undo Hats pan"), or
/// their word alone; any other item's as `ITEMS` has it.
fn title(id: &str, text: &str) -> String {
    let names = UNDO.lock().map(|u| u.names.clone()).unwrap_or_default();
    let name = match id {
        "undo" => names.0,
        "redo" => names.1,
        _ => None,
    };
    match name {
        Some(name) => format!("{text} {name}"),
        None => text.to_string(),
    }
}

/// Edit ▸ Undo and Redo say what they would take back (gaps #29): the app's title loop gives the
/// names the server publishes, and the items change only when they do.
pub fn set_undo_names(undo: Option<String>, redo: Option<String>) {
    let items = {
        let Ok(mut u) = UNDO.lock() else { return };
        if u.names == (undo.clone(), redo.clone()) {
            return;
        }
        u.names = (undo, redo);
        u.items.clone()
    };
    // Off the lock: a title is set on the main thread, which may be building the menu.
    if let Some((undo, redo)) = items {
        let _ = undo.set_text(title("undo", "Undo"));
        let _ = redo.set_text(title("redo", "Redo"));
    }
}

/// Window → Close Window, which quits (as the window's close button does): ⌘W is Close Song.
const CLOSE_WINDOW: &str = "Shift+CmdOrCtrl+W";

/// One row of a menu: an item of `ITEMS` by id, a line, or one the system provides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Item(&'static str),
    Line,
    /// File ▸ Open Recent ▸, built from the recent songs list ([`recent_rows`]).
    Recent,
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
        Recent,
        Line,
        Item("close_song"),
        Item("save"),
        Item("save_as"),
        Line,
        Item("import_audio"),
        Line,
        Item("export"),
        Line,
        Item("reveal"),
        Item("rename_song"),
    ];
    // What is selected: the roll's notes, the timeline's clips, the track.
    let selected = [Item("duplicate"), Item("split"), Item("join"), Item("quantize")];
    let track = vec![
        Item("new_track"),
        Item("delete_track"),
        Item("rename"),
        Item("change_instrument"),
        Line,
        Item("mute"),
        Item("solo"),
        Item("mute_in_song"),
    ];
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
        Item("piano"),
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
        Item("metronome"),
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
            ("music2", vec![About, Line, Item("settings"), Line, Services, Line, Hide, HideOthers, ShowAll, Line, Item("quit")]),
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
            ("File", [&file[..], &[Line, Item("settings"), Line, Item("quit")]].concat()),
            ("Edit", [&[Item("undo"), Item("redo"), Line][..], &selected].concat()),
            ("Track", track),
            ("View", views.to_vec()),
            ("Transport", transport),
            ("Help", vec![Item("shortcuts"), Line, About]),
        ]
    }
}

/// A row of File ▸ Open Recent ▸.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RecentRow {
    /// A song: its item id (`open_recent:<folder>`) and its name.
    Song(String, String),
    Line,
    /// Clear Menu, which does something only while there is a song to clear.
    Clear { enabled: bool },
}

/// File ▸ Open Recent ▸'s rows for `songs` (folder, name), newest first: up to ten songs, a line,
/// and Clear Menu. With none, only Clear Menu, greyed out.
fn recent_rows(songs: &[(std::path::PathBuf, String)]) -> Vec<RecentRow> {
    let mut rows: Vec<RecentRow> =
        songs.iter().take(RECENT_SHOWN).map(|(dir, name)| RecentRow::Song(format!("{OPEN_RECENT}{}", dir.display()), name.clone())).collect();
    if !rows.is_empty() {
        rows.push(RecentRow::Line);
    }
    rows.push(RecentRow::Clear { enabled: !songs.is_empty() });
    rows
}

/// The recent songs as Open Recent lists them: folder and name, newest first.
fn recent_songs(app: &AppHandle) -> Vec<(std::path::PathBuf, String)> {
    let Some(native) = app.try_state::<Native>() else { return vec![] };
    native.server.recent().map(|r| r.list().into_iter().map(|s| (s.path, s.name)).collect()).unwrap_or_default()
}

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let mut bar = MenuBuilder::new(app);
    let (mut undo, mut redo) = (None, None);
    for (name, rows) in layout(MAC) {
        let mut menu = SubmenuBuilder::new(app, name);
        for row in rows {
            menu = match row {
                Row::Item(id) => {
                    let &(id, text, accel) = ITEMS.iter().find(|i| i.0 == id).expect("a menu item");
                    let b = MenuItemBuilder::with_id(id, title(id, text));
                    let item = match accel {
                        Some(a) => b.accelerator(a).build(app)?,
                        None => b.build(app)?,
                    };
                    match id {
                        "undo" => undo = Some(item.clone()),
                        "redo" => redo = Some(item.clone()),
                        _ => {}
                    }
                    menu.item(&item)
                }
                Row::Line => menu.separator(),
                Row::Recent => {
                    let mut sub = SubmenuBuilder::new(app, "Open Recent");
                    for row in recent_rows(&recent_songs(app)) {
                        sub = match row {
                            RecentRow::Song(id, name) => sub.item(&MenuItemBuilder::with_id(id, name).build(app)?),
                            RecentRow::Line => sub.separator(),
                            RecentRow::Clear { enabled } => {
                                sub.item(&MenuItemBuilder::with_id(CLEAR_RECENT, "Clear Menu").enabled(enabled).build(app)?)
                            }
                        };
                    }
                    menu.item(&sub.build()?)
                }
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
    if let Ok(mut u) = UNDO.lock() {
        u.items = undo.zip(redo);
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
    } else if NATIVE.contains(&id) || id.starts_with(OPEN_RECENT) {
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
                assert!(matches!(r, Row::Item(_) | Row::Line | Row::About | Row::Recent), "{menu}: {r:?}");
            }
        }
    }

    /// Polish A3: Track ▸ Change Instrument… comes right after Rename, on both menu bars, with no chord.
    #[test]
    fn change_instrument_follows_rename_in_track() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "change_instrument").map(|i| (i.1, i.2)), Some(("Change Instrument…", None)));
        for mac in [true, false] {
            let bar = layout(mac);
            let track = &bar.iter().find(|m| m.0 == "Track").expect("Track").1;
            let at = track.iter().position(|r| *r == Row::Item("rename")).expect("Rename");
            assert_eq!(track[at + 1], Row::Item("change_instrument"), "mac: {mac}");
        }
    }

    /// Polish C3: File ▸ Save As… (⇧⌘S, Ctrl+Shift+S) sits after Save on both menu bars.
    #[test]
    fn save_as_is_shift_command_s_after_save() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "save_as").map(|i| (i.1, i.2)), Some(("Save As…", Some("CmdOrCtrl+Shift+S"))));
        for mac in [true, false] {
            let bar = layout(mac);
            let file = &bar.iter().find(|m| m.0 == "File").expect("a File menu").1;
            let at = file.iter().position(|r| *r == Row::Item("save")).expect("Save");
            assert_eq!(file[at + 1], Row::Item("save_as"), "mac: {mac}");
        }
    }

    /// Polish C2: File ▸ Open Recent ▸ lists the ten newest songs by name, a line, and Clear Menu;
    /// with none, only Clear Menu, greyed out. It sits after Open Project Folder… on both menu bars.
    #[test]
    fn open_recent_lists_ten_and_clear_menu() {
        let songs: Vec<_> = (0..12).map(|n| (std::path::PathBuf::from(format!("/songs/Song {n}")), format!("Song {n}"))).collect();
        let rows = recent_rows(&songs);
        assert_eq!(rows.len(), 12, "{rows:?}");
        assert_eq!(rows[0], RecentRow::Song("open_recent:/songs/Song 0".into(), "Song 0".into()));
        assert_eq!(rows[9], RecentRow::Song("open_recent:/songs/Song 9".into(), "Song 9".into()));
        assert_eq!(rows[10..], [RecentRow::Line, RecentRow::Clear { enabled: true }]);
        assert_eq!(recent_rows(&[]), [RecentRow::Clear { enabled: false }]);
        for mac in [true, false] {
            let bar = layout(mac);
            let file = &bar.iter().find(|m| m.0 == "File").expect("a File menu").1;
            let at = file.iter().position(|r| *r == Row::Item("open_project")).expect("Open Project Folder…");
            assert_eq!(file[at + 1], Row::Recent, "mac: {mac}");
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
            assert_eq!(transport[at + 1..at + 3], [Row::Item("metronome"), Row::Item("follow")], "mac: {mac}");
            let at = transport.iter().position(|r| *r == Row::Item("add_marker")).expect("Add Marker");
            assert_eq!(transport[at - 1..at + 4], [Row::Line, Row::Item("add_marker"), Row::Item("prev_marker"), Row::Item("next_marker"), Row::Line]);
        }
    }

    /// Next lane C2 (plan Q18): Settings… is ⌘, (Ctrl+, off the Mac): in the app menu after About on
    /// a Mac, and in File above Quit's line elsewhere.
    #[test]
    fn settings_is_command_comma_in_the_app_menu_on_a_mac_and_in_file_elsewhere() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "settings").map(|i| (i.1, i.2)), Some(("Settings…", Some("CmdOrCtrl+Comma"))));
        let mac = layout(true);
        assert_eq!(mac[0].1[..4], [Row::About, Row::Line, Row::Item("settings"), Row::Line]);
        let gtk = layout(false);
        let file = &gtk.iter().find(|m| m.0 == "File").expect("a File menu").1;
        assert_eq!(file[file.len() - 4..], [Row::Line, Row::Item("settings"), Row::Line, Row::Item("quit")]);
        assert!(!mac.iter().find(|m| m.0 == "File").expect("File").1.contains(&Row::Item("settings")), "once on a Mac");
    }

    /// Gaps #29 (plan X8): Edit ▸ Undo and Redo take the names the server gives, and say their word
    /// alone with none; a menu built again is titled from them.
    #[test]
    fn the_undo_items_take_a_name() {
        assert_eq!((title("undo", "Undo"), title("redo", "Redo")), ("Undo".to_string(), "Redo".to_string()));
        set_undo_names(Some("Hats pan".into()), Some("adding Reverb".into()));
        assert_eq!(title("undo", "Undo"), "Undo Hats pan");
        assert_eq!(title("redo", "Redo"), "Redo adding Reverb");
        assert_eq!(title("save", "Save"), "Save", "no other item");
        set_undo_names(None, Some("Bass notes".into()));
        assert_eq!((title("undo", "Undo"), title("redo", "Redo")), ("Undo".to_string(), "Redo Bass notes".to_string()));
        set_undo_names(None, None);
    }

    /// The next track's lane B: Transport ▸ Metronome, after Loop Selection and before Follow
    /// Playhead (taste.md's order), on both menu bars, with no chord: K is the page's key.
    #[test]
    fn the_metronome_has_its_row_in_transport() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "metronome").map(|i| (i.1, i.2)), Some(("Metronome", None)));
        for mac in [true, false] {
            let bar = layout(mac);
            let transport = &bar.iter().find(|m| m.0 == "Transport").expect("Transport").1;
            let at = transport.iter().position(|r| *r == Row::Item("loop")).expect("Loop");
            assert_eq!(transport[at..at + 4], [Row::Item("loop"), Row::Item("loop_selection"), Row::Item("metronome"), Row::Item("follow")], "mac: {mac}");
        }
    }

    /// The next track's lane B: View ▸ Keyboard Piano, ⌘K (Ctrl+K), right after Show Browser on both
    /// menu bars.
    #[test]
    fn the_keyboard_piano_is_command_k_in_view() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "piano").map(|i| (i.1, i.2)), Some(("Keyboard Piano", Some("CmdOrCtrl+K"))));
        for mac in [true, false] {
            let bar = layout(mac);
            let view = &bar.iter().find(|m| m.0 == "View").expect("View").1;
            let at = view.iter().position(|r| *r == Row::Item("browser")).expect("Show Browser");
            assert_eq!(view[at + 1], Row::Item("piano"), "mac: {mac}");
        }
    }

    /// Studio lane A (Q7): File ▸ Import Audio… is ⇧⌘I (Ctrl+Shift+I), after Save As and a line, on
    /// both menu bars.
    #[test]
    fn import_audio_is_shift_command_i_in_file() {
        assert_eq!(ITEMS.iter().find(|i| i.0 == "import_audio").map(|i| (i.1, i.2)), Some(("Import Audio…", Some("CmdOrCtrl+Shift+I"))));
        for mac in [true, false] {
            let bar = layout(mac);
            let file = &bar.iter().find(|m| m.0 == "File").expect("a File menu").1;
            let at = file.iter().position(|r| *r == Row::Item("save_as")).expect("Save As");
            assert_eq!(file[at + 1..at + 3], [Row::Line, Row::Item("import_audio")], "mac: {mac}");
        }
    }

    /// OS-55: the reveal item names the Mac's Finder only on a Mac.
    #[test]
    fn the_reveal_item_says_finder_only_on_a_mac() {
        let title = ITEMS.iter().find(|i| i.0 == "reveal").map(|i| i.1);
        assert_eq!(title, Some(if cfg!(target_os = "macos") { "Show in Finder" } else { "Show in Folder" }));
    }
}

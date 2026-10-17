import { LucideProvider, WifiOff } from "lucide-react";
import { useEffect } from "react";
import { connect, on } from "./lib/events";
import { useStore } from "./lib/store";
import Toasts from "./components/Toasts";
import GraphCanvas, { closeSong, graphUndo } from "./components/GraphCanvas";
import Shortcuts from "./components/Shortcuts";
import StartScreen from "./components/StartScreen";
import NewSongDialog, { openNewSong } from "./components/NewSongDialog";
import RenameSongDialog, { openRenameSong } from "./components/RenameSongDialog";
import SaveAsDialog, { openSaveAs } from "./components/SaveAsDialog";
import SettingsDialog from "./components/SettingsDialog";
import { exportSong, onExportEvent } from "./lib/exporting";
import { openSettings } from "./lib/settings";
import { CutOff, editorHooks, graphApi } from "./lib/graph";
import { isMac, native, nativeAction, onMenu } from "./lib/native";
import { followGridView } from "./lib/grid";
import { canvas, useShell, VIEWS, type GridOp } from "./lib/shell";
import { deleteTrack, duplicateTrack, newTrack } from "./components/Tracks";
import { toggleMetronome } from "./lib/metronome";
import { togglePiano } from "./lib/piano";
// MIDI keyboards: the window tells the server what they play.
import "./lib/midi";
import KeyboardPiano from "./components/KeyboardPiano";
import { openGuide } from "./lib/guide";

/** Fetch the open project. An error means none is open: the start screen shows. */
const refreshGraph = () =>
  graphApi
    .get()
    .then((graph) => useStore.getState().set({ graph, ready: true }))
    .catch((error) => {
      // An answer cut off on its way (the window reloading while it waited): the window stays as
      // it was, and the next event asks again.
      if (error instanceof CutOff) return;
      const { set, toast } = useStore.getState();
      set({ graph: null, ready: true });
      // "No project is open" is what the start screen says; anything else is worth saying.
      const message = error instanceof Error ? error.message : String(error);
      if (message !== "no project is open") toast(message);
    });

/**
 * The page's own keys (taste.md §5.3): single keys and ⌥ chords, never while typing. ⌘ and ⌃
 * chords are the menu bar's in the desktop app; in a browser, `chord` plays its part.
 */
function key(e: KeyboardEvent): string | null {
  if (e.metaKey || e.ctrlKey) return null;
  if (e.altKey) {
    // By the key's place: ⌥1 types ¡ on a Mac.
    const n = /^Digit([1-4])$/.exec(e.code);
    if (n) return `view_${VIEWS[Number(n[1]) - 1]}`;
    // ⌥← and ⌥→: Transport ▸ Previous and Next Marker.
    if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && !e.shiftKey) return e.key === "ArrowLeft" ? "prev_marker" : "next_marker";
    return e.code === "KeyK" ? "shortcuts" : null;
  }
  if (e.code === "Space") return e.shiftKey ? "continue" : "play";
  // ⇧Return renames the selected track, as in Logic; Return alone goes to the start.
  if (e.key === "Enter" && e.shiftKey) return "rename";
  if (e.key === "Enter" || e.key === "Home") return "home";
  if (e.key === "Escape") return "deselect";
  if (e.shiftKey) return null;
  return ({ c: "loop", a: "lanes", e: "dock", x: "view_mix", b: "draw", m: "mute", s: "solo", z: "zoom_fit", k: "metronome" } as Record<string, string>)[e.key.toLowerCase()] ?? null;
}

/** ⌘1 to ⌘4, View's grid rows: the command ids, in order. */
const GRID_IDS = ["grid_narrower", "grid_wider", "grid_triplet", "grid_snap"];

/**
 * A ⌘ or ⌃ chord as the menu bar would take it, in a browser; `always`: even while typing (⌘S, ⌘N);
 * `page`: the page's even in the app, where no menu row has it (⇧⌘=).
 */
function chord(e: KeyboardEvent): { id: string; always?: boolean; page?: boolean } | null {
  if (!(e.metaKey || e.ctrlKey)) return null;
  const k = e.key.toLowerCase();
  // ⌃M and ⌃S on a Mac, where ⌘S saves: Track ▸ Mute and Solo. Elsewhere Ctrl is ⌘.
  if (isMac && e.ctrlKey && !e.metaKey && !e.shiftKey && !e.altKey && (k === "m" || k === "s")) return { id: k === "m" ? "mute" : "solo" };
  // By the key's place, as ⌥1 to ⌥4 are.
  const n = /^Digit([1-4])$/.exec(e.code);
  if (n && !e.shiftKey && !e.altKey) return { id: GRID_IDS[Number(n[1]) - 1] };
  // Cut, copy and paste are the clips' while the timeline takes them; else the page's own, for text.
  if ((k === "c" || k === "x" || k === "v") && !e.shiftKey && !e.altKey && !editing() && !field(e.target) && canvas.takesClipboard?.()) {
    return { id: ({ c: "copy", x: "cut", v: "paste" } as const)[k] };
  }
  if (k === "s" && !e.shiftKey) return { id: "save", always: true };
  if (k === "s" && e.shiftKey && !e.altKey) return { id: "save_as", always: true };
  if (k === "n" && !e.shiftKey) return { id: "new_song", always: true };
  if (k === "z") return { id: e.shiftKey ? "redo" : "undo" };
  if (k === "d" && !e.shiftKey) return { id: "duplicate" };
  if (k === "e" && e.shiftKey) return { id: "export" };
  if (k === "e" && !e.altKey) return { id: "split" };
  if (k === "j" && !e.shiftKey && !e.altKey) return { id: "join" };
  if (k === "w" && !e.shiftKey) return { id: "close_song" };
  if (k === "r" && !e.shiftKey) return { id: "rename" };
  if (k === "t" && !e.shiftKey) return { id: "new_track" };
  if (k === "l" && !e.shiftKey) return { id: "loop_selection" };
  // Stop all sound works from a text field too; in the code editor, ⌘. is its quick fix.
  if (e.key === "." && !e.shiftKey) return { id: "panic", always: true };
  if (e.key === "Backspace" && !e.shiftKey) return { id: "delete_track" };
  if (e.code === "KeyB" && e.altKey) return { id: "browser" };
  // View ▸ Zoom In and Zoom Out (⌘= and ⌘−, and ⇧⌘= that types +), Taller and Shorter Tracks
  // (⌘↓ and ⌘↑); Transport ▸ Follow Playhead (⇧⌘F).
  if (e.key === "+" && !e.altKey) return { id: "zoom_in", page: true };
  if (e.key === "=" && !e.altKey && !e.shiftKey) return { id: "zoom_in" };
  if (e.key === "-" && !e.altKey && !e.shiftKey) return { id: "zoom_out" };
  if ((e.key === "ArrowUp" || e.key === "ArrowDown") && !e.altKey && !e.shiftKey) return { id: e.key === "ArrowUp" ? "shorter" : "taller" };
  if (k === "f" && e.shiftKey && !e.altKey) return { id: "follow" };
  // Settings (⌘, and Ctrl+,), from anywhere: the start screen, a field, the code editor.
  if (e.key === "," && !e.shiftKey && !e.altKey) return { id: "settings", always: true };
  return null;
}

export default function App() {
  const ready = useStore((s) => s.ready);
  const connected = useStore((s) => s.connected);
  const graph = useStore((s) => s.graph);

  useEffect(() => {
    const { set, toast } = useStore.getState();
    const heard = [
      on("toast", (msg) => toast(msg.text, undefined, true)),
      on("graph", () => void refreshGraph()),
      on("export", (msg) => void onExportEvent(msg)),
    ];
    const stop = connect((c) => {
      set({ connected: c });
      if (c) void refreshGraph();
    });
    return () => {
      stop();
      heard.forEach((off) => off());
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const c = chord(e);
      if (c) {
        // In the desktop app the menu bar owns ⌘ chords (they arrive via onMenu).
        if (native && !c.page) return;
        if (!c.always && (editing() || field(e.target) || e.defaultPrevented)) return;
        e.preventDefault();
        return appCommand(c.id);
      }
      // A key a field or a panel already took (Enter that adds an effect) isn't a shortcut
      // too, even if that field is gone or blurred by now.
      if (editing() || field(e.target) || e.defaultPrevented) return;
      // Return on a button Tab reached presses that button, and only it: it doesn't go to the
      // start too. Space stays the transport everywhere (taste.md §2.11).
      if (e.key === "Enter" && e.target instanceof HTMLButtonElement) return;
      const id = key(e);
      if (!id) return;
      // ⌥← and ⌥→ never go back a page in WebKitGTK.
      if (id === "play" || id === "continue" || id.startsWith("view_") || id === "shortcuts" || id.endsWith("_marker")) e.preventDefault();
      appCommand(id);
    };
    // Off the Mac, Ctrl+Backspace in a field deletes the word before, by the code the app's menu
    // runs (`deleteBack`), whatever the browser's own editing keys are. Before the field sees it:
    // fields keep their keys from the window. The code editor has its own.
    const onFieldKey = (e: KeyboardEvent) => {
      if (isMac || native || e.key !== "Backspace" || !e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;
      const t = e.target;
      if (!textField(t) || t.closest(".monaco-editor")) return;
      e.preventDefault();
      deleteBack(t);
    };
    // Right-click: nothing (rather than the browser's Reload/Inspect menu), except in text fields.
    const onContext = (e: MouseEvent) => {
      if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("keydown", onFieldKey, true);
    window.addEventListener("contextmenu", onContext);
    const offMenu = onMenu(appCommand);
    const offGrid = followGridView();
    if (native) nativeAction("ready");
    document.documentElement.classList.toggle("native", native);
    document.documentElement.classList.toggle("mac", isMac);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keydown", onFieldKey, true);
      window.removeEventListener("contextmenu", onContext);
      offMenu();
      offGrid();
    };
  }, []);

  if (!ready) {
    return <div className="h-full grid place-items-center text-sm text-fg-muted">{connected ? "Loading…" : "Connecting…"}</div>;
  }

  return (
    // One icon style everywhere (taste.md §2.12): Lucide at 1.5px, whatever the icon's size.
    <LucideProvider strokeWidth={1.5} absoluteStrokeWidth>
      <div className="h-full">
        {graph ? <GraphCanvas key={graph.session} graph={graph} /> : <StartScreen />}
        <NewSongDialog />
        <SaveAsDialog />
        <RenameSongDialog />
        <Shortcuts />
        <SettingsDialog />
        <Toasts />
        <KeyboardPiano />
        {/* With a song open, the top bar's status pill says it; on the start screen, this one does. */}
        {!connected && !graph && (
          <div className="fixed top-3 right-3 z-50 h-ctl-sm px-2 inline-flex items-center gap-1.5 rounded-sm bg-danger-soft text-danger-strong text-sm font-medium">
            <WifiOff size={14} className="text-danger" />
            Reconnecting…
          </div>
        )}
      </div>
    </LucideProvider>
  );
}

function editing() {
  return field(document.activeElement);
}

function field(a: EventTarget | null) {
  return a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement || (a instanceof HTMLElement && a.isContentEditable);
}

/** A field whose text can be selected (not a number, a checkbox or a slider). */
function textField(a: EventTarget | null): a is HTMLInputElement | HTMLTextAreaElement {
  return (a instanceof HTMLInputElement && a.selectionStart !== null) || a instanceof HTMLTextAreaElement;
}

/**
 * ⌘⌫ in a text field, as each OS's fields do it: the selection, or else up to the line's start on
 * a Mac and the word before elsewhere (Ctrl+Backspace). As one typing step, so the field's own undo
 * puts it back.
 */
function deleteBack(el: HTMLInputElement | HTMLTextAreaElement) {
  const end = el.selectionEnd ?? 0;
  const at = el.selectionStart ?? end;
  const from = at !== end ? at : isMac ? el.value.lastIndexOf("\n", at - 1) + 1 : wordStart(el.value, at);
  if (from === end) return;
  el.setSelectionRange(from, end);
  if (document.execCommand("delete")) return;
  el.setRangeText("", from, end, "start");
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Where the word before `at` starts: back over spaces, then over letters and digits, or else over other marks. */
function wordStart(text: string, at: number) {
  const word = (c: string) => /[\p{L}\p{N}_]/u.test(c);
  const space = (c: string) => /\s/.test(c);
  let i = at;
  while (i > 0 && space(text[i - 1])) i--;
  const kind = i > 0 && word(text[i - 1]);
  while (i > 0 && !space(text[i - 1]) && word(text[i - 1]) === kind) i--;
  return i;
}

/** Every app-level command, from the keyboard or the menu bar (desktop): the command ids are the menu items'. */
export function appCommand(id: string) {
  const st = useStore.getState();
  const shell = useShell.getState();
  // The desktop app asks for this before it closes or replaces a project with unsaved code.
  if (id === "save_all") return void editorHooks.saveAll?.();
  // With a song open or not: making one opens it in place of the open one.
  if (id === "new_song") return openNewSong();
  if (id === "shortcuts") return shell.set({ sheet: !shell.sheet });
  if (id === "settings") return openSettings();
  const graph = st.graph;
  if (!graph) return;
  if (id.startsWith("view_")) {
    const view = VIEWS.find((v) => `view_${v}` === id);
    return void (view && shell.setView(view));
  }
  switch (id) {
    case "save":
      // An open code editor saves its files. Every other edit is written at once.
      if (!editorHooks.save?.()) st.toast("Changes are saved as you make them.");
      return;
    case "undo":
    case "redo":
      // In the code editor, its own history; in a text field, undo the typing, like any native app.
      if (editorHooks.undo?.(id === "redo")) return;
      if (editing()) return void document.execCommand(id);
      return void graphUndo(graph.session, id);
    case "duplicate": {
      // Focus decides (taste.md §5.3): in the code editor, its own ⌘D; else the roll's selected
      // notes; else the timeline's selected clips; else the selected track, when no clip of it is
      // selected (design-8b §2.6).
      if (editorHooks.chord?.("duplicate")) return;
      if (editorHooks.duplicate?.()) return;
      if (editing()) return;
      if (canvas.duplicate?.()) return;
      const m = shell.selected;
      if (m && m !== "song" && !shell.clips[m]) void duplicateTrack(m);
      return;
    }
    case "new_track":
      return newTrack();
    case "delete_track": {
      // In the code editor, its own ⌘⌫; in a field, the field's (`deleteBack`). On a Mac, both delete
      // to the line's start; elsewhere, the word before.
      if (editorHooks.chord?.("delete_track")) return;
      const el = document.activeElement;
      if (textField(el)) return deleteBack(el);
      // In the dock (the roll's notes, a knob), the chord is the dock's, never the track's.
      if (editing() || document.activeElement?.closest("#dock")) return;
      if (shell.selected && shell.selected !== "song") void deleteTrack(shell.selected);
      return;
    }
    case "play":
      return canvas.play?.();
    case "continue":
      return canvas.continue?.();
    case "home":
      return canvas.home?.();
    case "loop":
      return canvas.loop?.();
    case "loop_selection":
      // In the code editor, its own ⌘L (select the line).
      if (editorHooks.chord?.("loop_selection") || editing()) return;
      return canvas.loopSelection?.();
    case "panic":
      if (editorHooks.chord?.("panic")) return;
      return void graphApi.panic().catch((error) => st.toast(error instanceof Error ? error.message : String(error)));
    case "save_as":
      return void openSaveAs();
    case "rename_song":
      return openRenameSong();
    case "export":
      return void exportSong();
    case "close_song":
      return closeSong();
    case "rename":
      // The selected track's name turns into a field; typing in a field already, the chord is the field's.
      if (!editing() && shell.selected) shell.set({ renaming: { module: shell.selected, at: "header" } });
      return;
    case "lanes": {
      // The selected track's automation, or every track's with none selected.
      const all = graph.timeline.tracks.filter((t) => t.lanes.some((l) => l.kind === "control")).map((t) => t.module);
      if (shell.selected && !all.includes(shell.selected)) return;
      return shell.toggleLanes(shell.selected, all);
    }
    case "dock":
      return shell.setDock(!shell.dock);
    case "browser":
      return shell.setBrowser(!shell.browser);
    case "draw":
      return shell.set({ draw: !shell.draw });
    case "deselect":
      return shell.deselect();
    case "mute":
    case "solo":
    case "mute_in_song": {
      // The selected track's; the Song row has no mute or solo (MON-1).
      const m = shell.selected;
      if (!m || m === "song") return;
      return id === "mute" ? canvas.mute?.(m) : id === "solo" ? canvas.solo?.(m) : canvas.muteInSong?.(m);
    }
    case "change_instrument": {
      // The selected track's (polish A3); not the Song row's.
      const m = shell.selected;
      if (!m || m === "song") return;
      return canvas.changeInstrument?.(m);
    }
    case "grid_narrower":
    case "grid_wider":
    case "grid_triplet":
    case "grid_snap":
      return canvas.grid?.(id.slice("grid_".length) as GridOp);
    case "split":
    case "join":
      // In the code editor, its own ⌘E and ⌘J.
      if (editorHooks.chord?.(id) || editing()) return;
      return id === "split" ? canvas.split?.() : canvas.join?.();
    case "quantize":
      // The roll's notes while it has the keyboard (X2); else the timeline's selected clips.
      if (editing() || editorHooks.quantize?.()) return;
      return canvas.quantize?.();
    case "copy":
    case "cut":
    case "paste":
      return canvas.clipboard?.(id);
    case "zoom_in":
    case "zoom_out": {
      // Focus decides (taste.md §5.3): the roll's own zoom while it has the keyboard, else the timeline's.
      const dir = id === "zoom_in" ? 1 : -1;
      if (editorHooks.zoom?.(dir) || editing()) return;
      return canvas.zoom?.(dir);
    }
    case "zoom_fit":
      return canvas.zoomFit?.();
    case "follow":
      return canvas.follow?.();
    case "taller":
    case "shorter": {
      // In the code editor, its own ⌘↑ and ⌘↓; in a text field, to its start or end (the app's
      // menu takes the chord before the field sees it).
      if (editorHooks.chord?.(id)) return;
      const el = document.activeElement;
      if (textField(el)) {
        const at = id === "shorter" ? 0 : el.value.length;
        return el.setSelectionRange(at, at);
      }
      if (editing()) return;
      return canvas.height?.(id === "taller" ? 1 : -1);
    }
    case "add_marker":
    case "prev_marker":
    case "next_marker":
      return canvas.marker?.(id === "add_marker" ? "add" : id === "prev_marker" ? "prev" : "next");
    case "metronome":
      return toggleMetronome();
    case "piano":
      // View ▸ Keyboard Piano, the desktop app's ⌘K. In a browser lib/piano.ts takes ⌘K itself, before
      // a field or the code editor can.
      return togglePiano();
    case "guide":
      // Help ▸ Make Your First Song (the studio plan's Q47).
      return openGuide();
  }
}

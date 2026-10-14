import { LucideProvider, WifiOff } from "lucide-react";
import { useEffect } from "react";
import { connect } from "./lib/events";
import { useStore } from "./lib/store";
import Toasts from "./components/Toasts";
import GraphCanvas, { closeSong, graphUndo } from "./components/GraphCanvas";
import Shortcuts from "./components/Shortcuts";
import StartScreen from "./components/StartScreen";
import NewSongDialog, { openNewSong } from "./components/NewSongDialog";
import { exportSong, onExportEvent } from "./lib/exporting";
import { editorHooks, graphApi } from "./lib/graph";
import { isMac, native, nativeAction, onMenu } from "./lib/native";
import { canvas, useShell, VIEWS } from "./lib/shell";

/** Fetch the open project. An error means none is open: the start screen shows. */
const refreshGraph = () =>
  graphApi
    .get()
    .then((graph) => useStore.getState().set({ graph, ready: true }))
    .catch((error) => {
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
    return e.code === "KeyK" ? "shortcuts" : null;
  }
  if (e.code === "Space" && !e.shiftKey) return "play";
  // ⇧Return renames the selected track, as in Logic; Return alone goes to the start.
  if (e.key === "Enter" && e.shiftKey) return "rename";
  if (e.key === "Enter" || e.key === "Home") return "home";
  if (e.key === "Escape") return "deselect";
  if (e.shiftKey) return null;
  return ({ c: "loop", a: "lanes", e: "dock", x: "view_mix", b: "draw" } as Record<string, string>)[e.key.toLowerCase()] ?? null;
}

/** A ⌘ or ⌃ chord as the menu bar would take it, in a browser; `always`: even while typing (⌘S, ⌘N). */
function chord(e: KeyboardEvent): { id: string; always?: boolean } | null {
  if (!(e.metaKey || e.ctrlKey)) return null;
  const k = e.key.toLowerCase();
  if (k === "s" && !e.shiftKey) return { id: "save", always: true };
  if (k === "n" && !e.shiftKey) return { id: "new_song", always: true };
  if (k === "z") return { id: e.shiftKey ? "redo" : "undo" };
  if (k === "d" && !e.shiftKey) return { id: "duplicate" };
  if (k === "e" && e.shiftKey) return { id: "export" };
  if (k === "w" && !e.shiftKey) return { id: "close_song" };
  if (k === "r" && !e.shiftKey) return { id: "rename" };
  if (e.code === "KeyB" && e.altKey) return { id: "browser" };
  return null;
}

export default function App() {
  const ready = useStore((s) => s.ready);
  const connected = useStore((s) => s.connected);
  const graph = useStore((s) => s.graph);

  useEffect(() => {
    const { set, toast } = useStore.getState();
    return connect(
      (msg) => {
        if (msg.t === "toast") toast(msg.text);
        else if (msg.t === "graph") void refreshGraph();
        else if (msg.t === "export") void onExportEvent(msg);
      },
      (c) => {
        set({ connected: c });
        if (c) void refreshGraph();
      },
    );
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const c = chord(e);
      if (c) {
        // In the desktop app the menu bar owns ⌘ chords (they arrive via onMenu).
        if (native) return;
        if (!c.always && (editing() || field(e.target) || e.defaultPrevented)) return;
        e.preventDefault();
        return appCommand(c.id);
      }
      // A key a field or a panel already took (Enter that adds an effect) isn't a shortcut
      // too, even if that field is gone or blurred by now.
      if (editing() || field(e.target) || e.defaultPrevented) return;
      const id = key(e);
      if (!id) return;
      if (id === "play" || id.startsWith("view_") || id === "shortcuts") e.preventDefault();
      appCommand(id);
    };
    // Right-click: nothing (rather than the browser's Reload/Inspect menu), except in text fields.
    const onContext = (e: MouseEvent) => {
      if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContext);
    const offMenu = onMenu(appCommand);
    if (native) nativeAction("ready");
    document.documentElement.classList.toggle("native", native);
    document.documentElement.classList.toggle("mac", isMac);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContext);
      offMenu();
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
        <Shortcuts />
        <Toasts />
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

/** Every app-level command, from the keyboard or the menu bar (desktop): the command ids are the menu items'. */
export function appCommand(id: string) {
  const st = useStore.getState();
  const shell = useShell.getState();
  // The desktop app asks for this before it closes or replaces a project with unsaved code.
  if (id === "save_all") return void editorHooks.saveAll?.();
  // With a song open or not: making one opens it in place of the open one.
  if (id === "new_song") return openNewSong();
  if (id === "shortcuts") return shell.set({ sheet: !shell.sheet });
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
    case "duplicate":
      // Focus decides (taste.md §5.3): in the code editor, its own ⌘D; else the roll's selected notes.
      if (editorHooks.chord?.("duplicate")) return;
      return void editorHooks.duplicate?.();
    case "play":
      return void graphApi.transport("toggle");
    case "home":
      return void graphApi.seek(0);
    case "loop":
      return canvas.loop?.();
    case "export":
      return void exportSong();
    case "close_song":
      return closeSong();
    case "rename":
      // The selected track's name turns into a field; typing in a field already, the chord is the field's.
      if (!editing() && shell.selected) shell.set({ renaming: shell.selected });
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
  }
}

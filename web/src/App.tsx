import { useEffect } from "react";
import { connect } from "./lib/events";
import { useStore } from "./lib/store";
import Toasts from "./components/Toasts";
import GraphCanvas, { graphUndo } from "./components/GraphCanvas";
import StartScreen from "./components/StartScreen";
import NewSongDialog, { openNewSong } from "./components/NewSongDialog";
import { exportSong, onExportEvent } from "./lib/exporting";
import { editorHooks, graphApi } from "./lib/graph";
import { isMac, native, nativeAction, onMenu } from "./lib/native";

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
      const cmd = e.metaKey || e.ctrlKey;
      // In the desktop app the menu bar owns ⌘-shortcuts (they arrive via onMenu).
      if (cmd && native) return;
      if (cmd && e.key.toLowerCase() === "s") {
        e.preventDefault();
        return appCommand("save");
      }
      if (cmd && e.key.toLowerCase() === "n") {
        e.preventDefault();
        return appCommand("new_song");
      }
      // A key a field or a panel already took (Enter that adds an effect) isn't a shortcut
      // too, even if that field is gone or blurred by now.
      if (editing() || field(e.target) || e.defaultPrevented) return;
      if (e.code === "Space") {
        e.preventDefault();
        appCommand("play");
      } else if (cmd && e.key.toLowerCase() === "z") {
        e.preventDefault();
        appCommand(e.shiftKey ? "redo" : "undo");
      } else if (cmd && e.key.toLowerCase() === "d") {
        e.preventDefault();
        appCommand("duplicate");
      } else if (cmd && e.key.toLowerCase() === "e") {
        e.preventDefault();
        appCommand("export");
      } else if (e.key === "Enter" || e.key === "Home") {
        appCommand("home");
      }
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
    return (
      <div className="h-full grid place-items-center">
        <div className="text-center space-y-3">
          <div className="text-3xl font-semibold pop-text">music2</div>
          <div className="text-ink-400 text-sm">{connected ? "Loading…" : "Connecting to music2…"}</div>
        </div>
      </div>
    );
  }

  return (
    <div className="h-full">
      {graph ? <GraphCanvas key={graph.session} graph={graph} /> : <StartScreen />}
      <NewSongDialog />
      <Toasts />
      {/* Below the 64px top bar, so it doesn't cover the project's name. */}
      {!connected && (
        <div className="fixed top-[76px] left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-full bg-red-50 text-red-700 text-sm ring-1 ring-red-500/40 shadow-lg shadow-red-900/10">
          Lost connection to music2 — reconnecting…
        </div>
      )}
    </div>
  );
}

function editing() {
  return field(document.activeElement);
}

function field(a: EventTarget | null) {
  return a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement || (a instanceof HTMLElement && a.isContentEditable);
}

/** Every app-level command, from the keyboard or the menu bar (desktop). */
export function appCommand(id: string) {
  const st = useStore.getState();
  // The desktop app asks for this before it closes or replaces a project with unsaved code.
  if (id === "save_all") return void editorHooks.saveAll?.();
  // With a song open or not: making one opens it in place of the open one.
  if (id === "new_song") return openNewSong();
  const graph = st.graph;
  if (!graph) return;
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
      // The piano roll duplicates its selected notes (the desktop menu bar takes ⌘D first).
      return void editorHooks.duplicate?.();
    case "play":
      return void graphApi.transport("toggle");
    case "home":
      return void graphApi.seek(0);
    case "export":
      return void exportSong();
  }
}

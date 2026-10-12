import { useEffect } from "react";
import { api, connect } from "./lib/api";
import { findClip } from "./lib/music";
import { applyPatch } from "./lib/patch";
import { useStore } from "./lib/store";
import type { ServerState } from "./lib/types";
import TopBar from "./components/TopBar";
import Timeline from "./components/Timeline";
import PianoRoll from "./components/PianoRoll";
import SidePanel from "./components/SidePanel";
import Settings from "./components/Settings";
import Library from "./components/Library";
import Toasts from "./components/Toasts";
import GraphCanvas, { graphUndo } from "./components/GraphCanvas";
import { graphApi } from "./lib/graph";
import { isMac, native, nativeAction, onFileDrop, onMenu } from "./lib/native";

let first = true;

/** Fetch the open graph project, if any. */
const refreshGraph = () =>
  graphApi
    .get()
    .then((graph) => useStore.getState().set({ graph }))
    .catch(() => useStore.getState().set({ graph: null }));

export default function App() {
  const server = useStore((s) => s.server);
  const modal = useStore((s) => s.modal);
  const selectedClip = useStore((s) => s.selectedClip);
  const connected = useStore((s) => s.connected);
  const graph = useStore((s) => s.graph);

  useEffect(() => {
    const { set, toast } = useStore.getState();
    return connect(
      (msg) => {
        const st = useStore.getState();
        if (msg.t === "state" || msg.t === "patch") {
          // Full state on connect; afterwards only what changed.
          const server: ServerState = msg.t === "state" ? msg.state : applyPatch(st.server!, msg.ops);
          set({ server });
          // Drop selections that no longer exist (undo, edits from elsewhere).
          if (st.selectedClip != null && !findClip(server.song, st.selectedClip)) set({ selectedClip: null });
          if (first) {
            first = false;
            devSetup(server);
          }
        } else if (msg.t === "tick") {
          set({ tick: msg });
        } else if (msg.t === "toast") {
          toast(msg.text);
        } else if (msg.t === "graph") {
          void refreshGraph();
        }
      },
      (c) => {
        set({ connected: c });
        if (c) void refreshGraph();
      },
    );
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const st = useStore.getState();
      const cmd = e.metaKey || e.ctrlKey;
      // In the desktop app the menu bar owns ⌘-shortcuts (they arrive via onMenu).
      if (cmd && native) return;
      if (cmd && e.key.toLowerCase() === "s") {
        e.preventDefault();
        return appCommand("save");
      }
      if (editing() || st.modal) return;
      if (e.code === "Space") {
        e.preventDefault();
        appCommand("play");
      } else if (cmd && e.key.toLowerCase() === "z") {
        e.preventDefault();
        appCommand(e.shiftKey ? "redo" : "undo");
      } else if (cmd && e.key.toLowerCase() === "d") {
        e.preventDefault();
        appCommand("duplicate");
      } else if ((e.key === "Delete" || e.key === "Backspace") && st.selectedClip != null) {
        appCommand("delete");
      } else if (e.key === "Enter" || e.key === "Home") {
        appCommand("home");
      } else if (e.key === "Escape") {
        st.set({ selectedClip: null });
      }
    };
    // Right-click: our own menus where we have them; nothing (rather than the
    // browser's Reload/Inspect menu) elsewhere — except in text fields.
    const onContext = (e: MouseEvent) => {
      if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("contextmenu", onContext);
    const offMenu = onMenu(appCommand);
    const offDrop = onFileDrop(
      (paths, x, y) => {
        const lane = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-bar-px]");
        const bar = lane ? Math.max(1, Math.floor((x - lane.getBoundingClientRect().left) / Number(lane.dataset.barPx)) + 1) : undefined;
        nativeAction("import_paths", { paths, bar });
      },
      (over) => useStore.getState().dropHover !== over && useStore.getState().set({ dropHover: over }),
    );
    if (native) nativeAction("ready");
    document.documentElement.classList.toggle("native", native);
    document.documentElement.classList.toggle("mac", isMac);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("contextmenu", onContext);
      offMenu();
      offDrop();
    };
  }, []);

  if (!server) {
    return (
      <div className="h-full grid place-items-center">
        <div className="text-center space-y-3">
          <div className="text-3xl font-semibold pop-text">music2</div>
          <div className="text-ink-400 text-sm">{connected ? "Loading…" : "Connecting to the music2 engine…"}</div>
        </div>
      </div>
    );
  }

  if (graph) {
    return (
      <div className="h-full">
        <GraphCanvas key={graph.session} graph={graph} />
        <Toasts />
      </div>
    );
  }

  const showRoll = (() => {
    const f = findClip(server.song, selectedClip);
    return f?.clip.content.type === "midi";
  })();

  return (
    <div className="h-full flex flex-col">
      <TopBar />
      <div className="flex-1 flex min-h-0">
        <main className="flex-1 flex flex-col min-w-0">
          <Timeline />
          {showRoll && <PianoRoll />}
        </main>
        <SidePanel />
      </div>
      {modal === "settings" && <Settings />}
      {modal === "library" && <Library />}
      <Toasts />
      <DropOverlay />
      {!connected && (
        <div className="fixed top-3 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-full bg-red-500/20 text-red-700 text-sm ring-1 ring-red-500/40">
          Lost connection to the engine — reconnecting…
        </div>
      )}
    </div>
  );
}

function editing() {
  const a = document.activeElement;
  return a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement || (a instanceof HTMLElement && a.isContentEditable);
}

/** Every app-level command, from the keyboard, the menu bar (desktop) or buttons. */
export async function appCommand(id: string) {
  const st = useStore.getState();
  const server = st.server;
  if (!server) return;
  switch (id) {
    case "undo":
    case "redo": {
      // In a text field, undo the typing, like any native app.
      if (editing()) return void document.execCommand(id);
      if (st.graph) return void graphUndo(st.graph.session, id);
      const r = await (id === "redo" ? api.redo() : api.undo()).catch(() => null);
      if (r?.label) st.toast(`${id === "redo" ? "Redid" : "Undid"} ${r.label}`);
      return;
    }
    case "save": {
      return void nativeAction("save");
    }
    case "duplicate": {
      if (st.selectedClip == null) return;
      const r = await st.run("clip.duplicate", { clip: st.selectedClip });
      if (r) st.set({ selectedClip: r.clip_ids[0] });
      return;
    }
    case "delete":
      if (st.selectedClip == null) return;
      await st.run("clip.remove", { clip: st.selectedClip });
      st.set({ selectedClip: null });
      return;
    case "play":
      if (st.graph) return void graphApi.transport("toggle");
      return void api.transport("toggle");
    case "home":
      if (st.graph) return void graphApi.seek(0);
      return void api.transport("seek", 0);
    case "loop":
      return void api.loop(!server.looping);
    case "settings":
    case "library":
      return st.set({ modal: id });
    case "panel:track":
      return st.set({ sideTab: "track" });
    case "panel:sounds":
      return st.set({ sideTab: "sounds" });
    case "zoom_in":
    case "zoom_out":
      return st.set({ ppb: Math.min(160, Math.max(6, st.ppb * (id === "zoom_in" ? 1.25 : 0.8))) });
  }
}

function DropOverlay() {
  const over = useStore((s) => s.dropHover);
  if (!over) return null;
  return (
    <div className="fixed inset-0 z-[60] pointer-events-none flex items-end justify-center pb-24 ring-4 ring-inset ring-pop-500/40 bg-pop-500/[0.04]">
      <div className="px-4 py-2 rounded-full glass ring-1 ring-black/10 shadow-xl text-sm text-ink-200">Drop a WAV on a bar to place it · drop a song to open it</div>
    </div>
  );
}

/** Dev automation: `?select=Hook&tab=sounds&modal=settings` (for screenshots/tests). */
function devSetup(server: ServerState) {
  const q = new URLSearchParams(location.search);
  if (![...q.keys()].length) return;
  const { set } = useStore.getState();
  const t = server.song.tracks.find((x) => x.name.toLowerCase() === q.get("select")?.toLowerCase());
  if (t) set({ selectedTrack: t.id, selectedClip: t.clips[0]?.id ?? null, modal: null });
  if (q.get("tab")) set({ sideTab: q.get("tab") as any });
  if (q.get("modal")) set({ modal: q.get("modal") as any });
}

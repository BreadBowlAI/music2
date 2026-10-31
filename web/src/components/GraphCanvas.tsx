import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  attention,
  barAt,
  editorHooks,
  findNoteClip,
  graphApi,
  lockReason,
  type ClipRef,
  type Diagnostic,
  type GraphState,
  type GraphStatus,
  type ModuleView,
  type Problem,
  type TimelineTrack,
  type TimelineView,
} from "../lib/graph";
import { setClock } from "../lib/clock";
import { pickedIn, useClipSelection } from "../lib/clips";
import { useSection } from "../lib/sections";
import { madeHere, onTheirWay, said, tookBack, type StepName } from "../lib/history";
import { forget, stepped } from "../lib/notice";
import { HEADS_W, HEADS_W_SMALL, WIDE } from "../lib/layout";
import { flushLocal } from "../lib/local";
import { canvas, useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import Browser from "./Browser";
import Dock from "./Dock";
import SongTimeline from "./SongTimeline";
import TopBar, { toggleLoop } from "./TopBar";
import { TracksUi } from "./Tracks";

/** The window's width, for what a small window drops (taste.md §3.8). */
function useWindowWidth() {
  const [w, setW] = useState(window.innerWidth);
  useEffect(() => {
    const on = () => setW(window.innerWidth);
    window.addEventListener("resize", on);
    return () => window.removeEventListener("resize", on);
  }, []);
  return w;
}

/**
 * A song-as-program project in the window's shell (taste.md §3): the top bar; the browser down the
 * left; the timeline, with the Song's row at its foot; the dock under it. What is selected in the
 * timeline decides what the dock shows; the dock's view is the producer's own choice (§3.7).
 */
export default function GraphCanvas({ graph }: { graph: GraphState }) {
  const { timeline } = graph;
  const toast = useStore((s) => s.toast);
  const connected = useStore((s) => s.connected);

  const [status, setStatus] = useState<GraphStatus>({ playing: graph.playing, seconds: graph.seconds, beat: graph.beat, bpm: graph.bpm, loop: graph.loop });
  const tempo = useRef(timeline.tempo);
  tempo.current = timeline.tempo;
  const show = useCallback((s: GraphStatus) => {
    setClock(s, tempo.current);
    setStatus(s);
  }, []);
  const poll = useCallback(() => graphApi.status().then(show).catch(() => {}), [show]);
  useEffect(() => {
    void poll();
    const t = setInterval(poll, 200);
    return () => clearInterval(t);
  }, [poll]);
  const failed = useCallback((error: unknown) => toast(error instanceof Error ? error.message : String(error)), [toast]);
  // A seek moves the playhead; playing, it plays on from there (taste.md §5.1).
  const seek = useCallback((beat: number) => void graphApi.seek(beat).then(poll, failed), [poll, failed]);
  // The loop: a range dragged on the ruler (which turns it on), or the toggle in the bar (C).
  const setLoop = useCallback(
    (req: { start?: number; end?: number; on?: boolean }) =>
      void graphApi.loop(req).then(show, (error) => toast(error instanceof Error ? error.message : String(error))),
    [show, toast],
  );
  const loopRange = useCallback((start: number, end: number) => setLoop({ start, end }), [setLoop]);
  const latest = useRef({ status, graph });
  latest.current = { status, graph };
  useEffect(() => {
    canvas.loop = () => toggleLoop(latest.current.graph.timeline.meter, latest.current.status, setLoop);
    canvas.loopSelection = () => loopSelection(latest.current.graph.timeline, latest.current.status.beat, setLoop);
    // Space and Play: stop leaves the playhead where it stopped, and play carries on from there
    // (GarageBand, taste.md §5.1).
    canvas.play = () => void graphApi.transport("toggle").then(poll, failed);
    canvas.home = () => seek(0);
    return () => {
      for (const k of ["loop", "loopSelection", "play", "home"] as const) delete canvas[k];
    };
  }, [setLoop, poll, failed, seek]);
  // The window is called by the song (the desktop app's own title follows the server).
  useEffect(() => {
    document.title = graph.name;
    return () => void (document.title = "music2");
  }, [graph.name]);

  // A new song: nothing selected, every track folded. One opened with code that doesn't compile has
  // nothing to show yet: it opens in its code (WRITE-20), without changing the window's choice.
  useEffect(() => {
    useShell.getState().resetSelection();
    forget();
    if (graph.has_code === false) useShell.setState({ view: "code", dock: true });
    // Code typed and not saved when the song last closed: the code editor offers it back (WRITE-27).
    const left = graph.drafts ?? [];
    if (left.length)
      useStore
        .getState()
        .toast(left.length === 1 ? `${left[0]} has changes that weren't saved.` : `${left.length} code files have changes that weren't saved.`, {
          label: "Show code",
          run: () => useShell.setState({ view: "code", dock: true }),
        });
  }, []);

  // The selection, by module key. A label changed in the code changes nothing here; a key changed
  // in the code keeps its module selected (the module now built from its file); if an edit removes
  // it, the first track shows.
  const modules = graph.view.modules;
  const chosen = useShell((s) => s.module);
  const shown = useRef<{ key: string; file: string | null; keys: string[] } | null>(null);
  const first = timeline.tracks.find((t) => t.track && t.module !== "song")?.module ?? modules.find((m) => m.track)?.key ?? "song";
  /** What shows for the module `chosen`: itself while the song has it, else the one now built from its file, else the first track. */
  const showing = (chosen: string | null): string => {
    let found = chosen && (modules.some((m) => m.key === chosen) || timeline.tracks.some((t) => t.module === chosen)) ? chosen : null;
    if (!found && chosen && shown.current?.key === chosen && shown.current.file) {
      const was = shown.current;
      found = modules.find((m) => !was.keys.includes(m.key) && fileOf(m) === was.file)?.key ?? null;
    }
    return found ?? first;
  };
  const module = showing(chosen);
  useEffect(() => {
    const s = useShell.getState();
    // Nothing chosen yet: the first track, as soon as there is one (a song that gets its first track
    // in its code selects it).
    if (s.auto) {
      if (s.module !== first || s.selected !== first) s.set({ selected: first, module: first });
      return;
    }
    // Renamed or gone: select what shows instead. Worked out from the choice as it is now, not as
    // this render saw it: React can run this effect after a click has chosen another track (the
    // click's own render flushes it), and that click stands.
    const now = showing(s.module);
    if (s.module === now) return;
    if (s.selected === s.module) s.select(now);
    else s.set({ module: now });
  });
  useEffect(() => {
    const m = modules.find((x) => x.key === module);
    if (m) shown.current = { key: m.key, file: fileOf(m), keys: modules.map((x) => x.key) };
  });
  // A selected clip an edit removed is let go.
  const clips = useShell((s) => s.clips);
  useEffect(() => {
    const gone = Object.entries(clips).filter(([, c]) => !findNoteClip(timeline, c));
    for (const [m] of gone) useShell.getState().setClip(m, null);
  }, [timeline, clips]);

  // Double-clicking a notes clip opens Notes on it: the one way a clip changes the dock's view (§3.7).
  const openClip = useCallback((c: ClipRef) => {
    const s = useShell.getState();
    s.select(c.module, c);
    s.setView("notes");
  }, []);
  const showCode = useCallback(
    (p: Problem) => {
      // The module whose code has the problem: the one it names, else the one built from its file.
      const owner = p.module ?? modules.find((m) => m.nodes.some((n) => n.file === p.file))?.key;
      const s = useShell.getState();
      if (owner) s.select(owner);
      s.setView("code");
    },
    [modules],
  );

  // What needs attention in each track, marked on its row.
  const issues = useMemo(() => {
    const by = new Map<string, Diagnostic[]>();
    for (const d of attention(graph)) if (d.module) by.set(d.module, [...(by.get(d.module) ?? []), d]);
    return by;
  }, [graph]);

  // The tracks in the timeline's order, then any module with no lanes, so every module can be
  // selected; the Song's own row at the foot.
  const rows = useMemo<TimelineTrack[]>(
    () => [
      ...timeline.tracks.filter((t) => t.module !== "song"),
      ...modules
        .filter((m) => m.key !== "song" && !timeline.tracks.some((t) => t.module === m.key))
        .map((m) => ({ module: m.key, label: m.label, track: m.track, lanes: [] })),
    ],
    [timeline, modules],
  );
  const song = useMemo<TimelineTrack>(
    () => timeline.tracks.find((t) => t.module === "song") ?? { module: "song", label: modules.find((m) => m.key === "song")?.label ?? "song", track: false, lanes: [] },
    [timeline, modules],
  );

  const width = useWindowWidth();
  const browser = useShell((s) => s.browser);
  const locked = lockReason(graph, connected);
  return (
    <div className="h-full flex flex-col">
      <TopBar graph={graph} status={status} onSeek={seek} onLoop={setLoop} onShowCode={showCode} />
      <div className="flex-1 min-h-0 flex">
        {browser && <Browser graph={graph} module={module} />}
        <div className="flex-1 min-w-0 flex flex-col">
          {/* Toasts sit centred over this, above its foot: above the dock, never over the Mix strips or the roll. */}
          <div data-toast-area className="flex-1 min-h-0 flex flex-col">
            <SongTimeline
              timeline={timeline}
              rows={rows}
              song={song}
              revision={graph.view.revision}
              beat={status.beat}
              playing={status.playing}
              loopStart={status.loop?.start ?? null}
              loopEnd={status.loop?.end ?? null}
              looping={!!status.loop?.on}
              onLoop={loopRange}
              issues={issues}
              locked={locked}
              waiting={graph.has_code === false}
              headerW={width < WIDE ? HEADS_W_SMALL : HEADS_W}
              onOpenClip={openClip}
              onSeek={seek}
            />
          </div>
          <Splitter />
          <Dock graph={graph} module={module} />
        </div>
      </div>
      <TracksUi graph={graph} />
    </div>
  );
}

/**
 * Loop the selection (⌘L): the bars selected on the ruler, else a marquee's bars, else the clips
 * picked on the timeline, else the clip the timeline outlines (the selected track's; the other
 * tracks' remembered clips aren't selected), else the bar the playhead is in. It turns looping on.
 */
function loopSelection(timeline: TimelineView, beat: number, onLoop: (req: { start: number; end: number }) => void) {
  const bars = useSection.getState().bars;
  if (bars) return onLoop({ start: bars[0], end: bars[1] });
  // The timeline's selection: a marquee's bars, or the clips picked; else the clip it outlines.
  const sel = useClipSelection.getState();
  if (sel.range) return onLoop({ start: sel.range[0], end: sel.range[1] });
  const picked = pickedIn(timeline, sel.picked).map((p) => p.clip);
  if (picked.length) {
    return onLoop({ start: Math.min(...picked.map((c) => c.start)), end: Math.max(...picked.map((c) => c.start + c.length)) });
  }
  const s = useShell.getState();
  const ref = s.selected ? s.clips[s.selected] : undefined;
  const shown = ref ? findNoteClip(timeline, ref) : undefined;
  if (shown) return onLoop({ start: shown.start, end: shown.start + shown.length });
  const bar = barAt(timeline.meter, beat);
  onLoop({ start: bar.start, end: bar.start + bar.length });
}

/** Between the timeline and the dock: 1px drawn, 6px to grab. Drag to size the dock; a double-click hides or shows it (E). */
function Splitter() {
  const resize = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    const s = useShell.getState();
    if (!s.dock) s.setDock(true);
    // From the height on screen, which the clamp may hold below the saved one.
    const y0 = e.clientY, h0 = s.height;
    let h = h0;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      h = h0 - (ev.clientY - y0);
      useShell.getState().setHeight(h);
    };
    const end = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", end);
      handle.removeEventListener("pointercancel", end);
      useShell.getState().setHeight(h, true);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  };
  return (
    <div className="relative h-px shrink-0 bg-line">
      <div
        data-testid="split"
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize the dock"
        title="Drag to resize · double-click to hide or show the dock (E)"
        className="absolute inset-x-0 -top-[2.5px] h-1.5 z-40 cursor-row-resize"
        onPointerDown={resize}
        onDoubleClick={() => {
          const s = useShell.getState();
          s.setDock(!s.dock);
        }}
      />
    </div>
  );
}

/** The project file a module is written in: where most of its nodes are. */
function fileOf(m: ModuleView): string | null {
  const count = new Map<string, number>();
  for (const n of m.nodes) if (n.file) count.set(n.file, (count.get(n.file) ?? 0) + 1);
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
}

/**
 * Close the song (⌘W): unsaved code would be lost, so say so, and close only when asked again. The
 * view settings still waiting to be kept go first.
 */
export function closeSong() {
  const n = editorHooks.unsaved?.() ?? 0;
  const close = () => void flushLocal().then(() => graphApi.close());
  if (!n) return close();
  useStore.getState().toast(`${n === 1 ? "A file has" : `${n} files have`} unsaved edits in the code editor.`, { label: "Close anyway", run: close });
}

/**
 * Undo or redo an edit to the open song: the step `step` names, as Undo's tooltip shows it, which
 * the server takes back only while it is the newest (AGENT-2); else it says which is. Its result
 * shows where it happened, so it makes no toast (ST-5), unless it took back a part added or
 * removed, which may be off screen: then the toast says so in the step's words ("Undid adding
 * Reverb"). Never a file or a diff (ST-6). A step undo can't take back (a file it puts back changed
 * since) is said, with Skip it.
 */
export async function graphUndo(session: number, id: "undo" | "redo", step?: number, own = false) {
  const toast = useStore.getState().toast;
  try {
    const { status, json } = await (id === "redo" ? graphApi.redo(session, step) : graphApi.undo(session, step));
    if (status === 409) {
      const stuck = json.stuck?.step;
      if (stuck !== undefined) return toast(json.error ?? "Undo can't take back that change.", { label: "Skip it", run: () => void skipStep(session, stuck) });
      // The newest is a step this window made a moment ago, which it hadn't shown yet: Undo's key
      // means that one. Any other is said, never taken back (AGENT-2).
      const newest = json.newest?.step;
      if (own && newest !== step && madeHere(session, newest)) return graphUndo(session, id, newest);
      return toast(json.error ?? "Nothing was changed: it isn't the newest change now.");
    }
    stepped(json.step);
    tops(session, json);
    const took = tookBack(json);
    if (took?.structural) toast(`${id === "redo" ? "Redid" : "Undid"} ${took.name}`);
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error));
  }
}

/** The tops after an undo, a redo or a skip, so a ⌘Z pressed again before the state is read names the next step. */
function tops(session: number, answer: { undo_name?: StepName | null; redo_name?: StepName | null }) {
  const { graph, set } = useStore.getState();
  if (graph?.session === session && answer.undo_name !== undefined) set({ graph: { ...graph, undo_name: answer.undo_name, redo_name: answer.redo_name } });
}

/** Skip step `step`, which undo can't take back: nothing is written, and Undo reaches the steps before it (AGENT-2). */
async function skipStep(session: number, step: number) {
  const toast = useStore.getState().toast;
  try {
    const { status, json } = await graphApi.skip(session, step);
    if (status === 409) return toast(json.error ?? "Nothing was skipped: it isn't the newest change now.");
    tops(session, json);
    toast(json.undo_name ? `Skipped it. Undo takes back ${said(json.undo_name)} now.` : "Skipped it. There's nothing more to undo.");
  } catch (error) {
    toast(error instanceof Error ? error.message : String(error));
  }
}

/** How long Undo and Redo wait for this window's edits on their way before they give up. */
const EDITS_WAIT_MS = 2000;

/**
 * Undo or Redo of the step on top, as the window shows it (the top bar, ⌘Z, the Edit menu): by its
 * id, so a change that landed elsewhere since is named before it can be taken back. With no step
 * shown, 0, which is never the newest: the server says what is.
 *
 * Pressed while edits of this window are on their way (sent, their answers not back), its step is
 * chosen at the press: the newest step those edits make, once they are answered, or with none (each
 * refused, or changing nothing) the step shown at the press. Never one that lands meanwhile, an
 * agent's say, which the server then names, refusing (AGENT-2). It waits at most 2 s, then takes
 * back nothing and says so; pressed again while it waits, it does nothing.
 */
export function undoTop(graph: GraphState, id: "undo" | "redo") {
  const awaited = onTheirWay();
  if (!awaited.length) {
    // One after another, each naming the top the one before left: ⌘Z held down takes back step by step.
    topped = topped.then(() => {
      const now = useStore.getState().graph ?? graph;
      if (now.session !== graph.session) return;
      return graphUndo(now.session, id, (id === "redo" ? now.redo_name : now.undo_name)?.step ?? 0, true);
    });
    return topped;
  }
  if (waiting) return topped;
  waiting = true;
  const shown = useStore.getState().graph ?? graph;
  const atPress = (id === "redo" ? shown.redo_name : shown.undo_name)?.step ?? 0;
  const answered = Promise.all(awaited.map((e) => e.done)).then(() => true);
  const late = new Promise<false>((resolve) => setTimeout(() => resolve(false), EDITS_WAIT_MS));
  topped = topped
    .then(() => Promise.race([answered, late]))
    .then((all) => {
      waiting = false;
      const now = useStore.getState();
      if (now.graph?.session !== graph.session) return;
      if (!all) return void now.toast(`Nothing was ${id === "redo" ? "redone" : "undone"}: your last edit isn't answered yet. Try again in a moment.`);
      const made = awaited.flatMap((e) => e.steps);
      return graphUndo(graph.session, id, id === "undo" && made.length ? Math.max(...made) : atPress);
    });
  return topped;
}

/** The last Undo or Redo of the top sent: the next waits for its answer. */
let topped: Promise<void> = Promise.resolve();
/** An Undo or Redo waits for this window's edits on their way: another pressed meanwhile does nothing. */
let waiting = false;

/**
 * A toast's Undo for the edit that made undo step `step` (its answer's `step`): the server takes it
 * back only while it is still the newest change, and says which is when it isn't (AGENT-2). None
 * when the edit made no step.
 */
export function undoAction(session: number, step: number | undefined) {
  if (step === undefined) return undefined;
  return { label: "Undo", run: () => void graphUndo(session, "undo", step) };
}

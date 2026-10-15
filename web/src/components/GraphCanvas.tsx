import clsx from "clsx";
import { CodeXml, Download, Lock, Pause, Piano, Play, Redo2, Repeat, SkipBack, TriangleAlert, Undo2, Workflow, X } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  attention,
  barAt,
  barsToBeats,
  editorHooks,
  findLane,
  findNoteClip,
  graphApi,
  keyNote,
  labelOf,
  moduleName,
  position,
  type ClipRef,
  type Diagnostic,
  type GraphState,
  type GraphStatus,
  type Loop,
  type Meter,
  type ModuleView,
  type Problem,
  type ReadOnly,
} from "../lib/graph";
import { exportSong, useExport } from "../lib/exporting";
import { isMac, kbd } from "../lib/native";
import { useStore } from "../lib/store";
import Attention from "./Attention";
import CodeView from "./CodeView";
import { NodeGraph } from "./NodeGraph";
import NoteView from "./NoteView";
import SongTimeline, { trackColor } from "./SongTimeline";
import { Button } from "./ui";

const PANEL_KEY = "m2.graph.panel";
const clampPanel = (h: number) => Math.round(Math.min(Math.max(h, 120), window.innerHeight * 0.8));

/** What the panel shows of the selected module. */
type PanelView = "graph" | "code" | "notes";

const VIEWS = [
  { id: "graph", label: "Graph", Icon: Workflow },
  { id: "code", label: "Code", Icon: CodeXml },
  { id: "notes", label: "Notes", Icon: Piano },
] as const;

/** A song-as-program project: its timeline, and below it the selected module as a graph, its code, or a clip's notes. */
export default function GraphCanvas({ graph }: { graph: GraphState }) {
  const { timeline } = graph;
  const toast = useStore((s) => s.toast);

  const [status, setStatus] = useState<GraphStatus>({ playing: graph.playing, seconds: graph.seconds, beat: graph.beat, loop: graph.loop });
  const poll = useCallback(() => graphApi.status().then(setStatus).catch(() => {}), []);
  useEffect(() => {
    void poll();
    const t = setInterval(poll, 200);
    return () => clearInterval(t);
  }, [poll]);
  const seek = useCallback(
    (beat: number) => void graphApi.seek(beat).then(poll, (error) => toast(error instanceof Error ? error.message : String(error))),
    [poll, toast],
  );
  // The loop: a range dragged on the ruler (which turns it on), or the toggle in the bar.
  const setLoop = useCallback(
    (req: { start?: number; end?: number; on?: boolean }) =>
      void graphApi.loop(req).then(setStatus, (error) => toast(error instanceof Error ? error.message : String(error))),
    [toast],
  );
  const loopRange = useCallback((start: number, end: number) => setLoop({ start, end }), [setLoop]);

  // One selection for the timeline and the panel, by module key. A label changed in the code
  // changes nothing here; a key changed in the code keeps its module selected (the module now
  // built from its file); if an edit removes it, the default shows.
  const [picked, setPicked] = useState<string | null>(null);
  const shown = useRef<{ key: string; file: string | null; keys: string[] } | null>(null);
  if (picked && !graph.view.modules.some((m) => m.key === picked) && shown.current?.key === picked && shown.current.file) {
    const was = shown.current;
    const renamed = graph.view.modules.find((m) => !was.keys.includes(m.key) && fileOf(m) === was.file);
    if (renamed) setPicked(renamed.key);
  }
  const tabs = [...graph.view.modules.filter((m) => m.key === "song"), ...graph.view.modules.filter((m) => m.key !== "song")];
  const module = graph.view.modules.find((m) => m.key === picked) ?? graph.view.modules.find((m) => m.track) ?? tabs[0];
  useEffect(() => {
    if (module) shown.current = { key: module.key, file: fileOf(module), keys: graph.view.modules.map((m) => m.key) };
  });

  // The panel's view, and the notes clip open in it. A clip is the selected module's: selecting
  // another module, or an edit that removes the clip, closes it and its notes view shows the graph.
  // The code view stays and follows the selection.
  // A project opened with code that doesn't compile has no graph yet: it opens in its code (WRITE-20).
  const [view, setView] = useState<PanelView>(graph.has_code === false ? "code" : "graph");
  const [clip, setClip] = useState<ClipRef | null>(null);
  if (clip && !(clip.module === module?.key && findNoteClip(timeline, clip))) {
    setClip(null);
    if (view === "notes") setView("graph");
  }
  const openClip = useCallback((c: ClipRef) => {
    setPicked(c.module);
    setClip(c);
    setView("notes");
  }, []);
  const closeClip = useCallback(() => {
    setClip(null);
    setView((v) => (v === "notes" ? "graph" : v));
  }, []);

  // What needs attention in each track, marked on its row.
  const issues = useMemo(() => {
    const by = new Map<string, Diagnostic[]>();
    for (const d of attention(graph)) if (d.module) by.set(d.module, [...(by.get(d.module) ?? []), d]);
    return by;
  }, [graph]);

  const panel = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(PANEL_KEY));
      if (saved > 0) return clampPanel(saved);
    } catch {}
    return clampPanel(window.innerHeight * 0.45);
  });
  const resize = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !panel.current) return;
    e.preventDefault();
    const handle = e.currentTarget;
    // From the height on screen, which the CSS clamp may hold below the saved one.
    const y0 = e.clientY, h0 = panel.current.offsetHeight;
    let h = h0;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => setHeight((h = clampPanel(h0 - (ev.clientY - y0))));
    const end = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", end);
      handle.removeEventListener("pointercancel", end);
      try {
        localStorage.setItem(PANEL_KEY, String(h));
      } catch {}
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
  };

  return (
    <div className="h-full flex flex-col">
      <GraphBar graph={graph} status={status} onSeek={seek} onLoop={setLoop} />
      {graph.read_only && <ReadOnlyBanner graph={graph} readOnly={graph.read_only} />}
      {graph.view.draft_errors.length > 0 && (
        <CompileBanner
          graph={graph}
          onShow={(p) => {
            // The module whose code has the problem: the one it names, else the one built from its file.
            const owner = p.module ?? graph.view.modules.find((m) => m.nodes.some((n) => n.file === p.file))?.key;
            if (owner) setPicked(owner);
            setView("code");
          }}
        />
      )}
      <Attention graph={graph} />
      <SongTimeline
        timeline={timeline}
        revision={graph.view.revision}
        beat={status.beat}
        playing={status.playing}
        loopStart={status.loop?.start ?? null}
        loopEnd={status.loop?.end ?? null}
        looping={!!status.loop?.on}
        onLoop={loopRange}
        selected={module?.key}
        open={clip}
        issues={issues}
        onSelect={setPicked}
        onOpenClip={openClip}
        onSeek={seek}
      />
      <div
        data-testid="split"
        role="separator"
        aria-orientation="horizontal"
        aria-label="Resize the graph panel"
        title="Drag to resize"
        className="group h-2 shrink-0 grid place-items-center cursor-row-resize bg-ink-900 border-y border-black/[0.06] hover:bg-ink-850"
        onPointerDown={resize}
      >
        <div className="w-10 h-[3px] rounded-full bg-ink-600 group-hover:bg-pop-500/60 transition-colors" />
      </div>
      <div ref={panel} className="relative shrink-0 min-h-[120px] max-h-[80vh] flex flex-col bg-ink-950" style={{ height }}>
        <div className="h-11 shrink-0 flex items-center gap-3 px-3 bg-ink-900 border-b border-black/[0.06]">
          <div role="tablist" aria-label="Modules" className="flex-1 min-w-0 flex items-center gap-1 overflow-x-auto">
            {tabs.map((m) => {
              const color = trackColor(timeline, m.key);
              return (
                <button
                  key={m.key}
                  role="tab"
                  aria-selected={m === module}
                  data-module-tab={m.key}
                  title={keyNote(moduleName(m), m.key) ? `${moduleName(m)} (${m.key})` : undefined}
                  onClick={() => setPicked(m.key)}
                  className={clsx(
                    "h-7 px-3 shrink-0 rounded-lg text-[13px] font-medium inline-flex items-center gap-2 cursor-pointer transition-colors",
                    m === module ? "bg-ink-700 text-ink-100" : "text-ink-400 hover:text-ink-100 hover:bg-ink-850",
                  )}
                >
                  {color && <span className="size-2 rounded-full" style={{ background: color }} />}
                  {moduleName(m)}
                </button>
              );
            })}
          </div>
          <div role="tablist" aria-label="Views" className="shrink-0 flex items-center gap-0.5 p-0.5 rounded-[10px] bg-ink-950 ring-1 ring-black/[0.04]">
            {VIEWS.map(({ id, label, Icon }) => {
              const name = module?.key === "song" ? "the song" : module && moduleName(module);
              const title =
                id === "graph"
                  ? `Play with ${name} as a graph of knobs and cables`
                  : id === "code"
                    ? `Edit ${name}'s code`
                    : clip
                      ? `Edit the notes of ${clipName(graph, clip)}`
                      : "Double-click a notes clip in the timeline to edit its notes";
              return (
                <button
                  key={id}
                  role="tab"
                  aria-selected={view === id}
                  data-view-tab={id}
                  disabled={id === "notes" && !clip}
                  title={title}
                  onClick={() => setView(id)}
                  className={clsx(
                    "h-7 px-2.5 rounded-lg text-[12px] font-medium inline-flex items-center gap-1.5 transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-default",
                    view === id ? "bg-ink-800 text-ink-100 shadow-sm ring-1 ring-black/[0.06]" : "text-ink-400 enabled:hover:text-ink-100",
                  )}
                >
                  <Icon className="size-3.5" />
                  {label}
                </button>
              );
            })}
          </div>
        </div>
        <PanelBody graph={graph} module={module?.key} view={view} clip={clip} onCloseClip={closeClip} />
      </div>
    </div>
  );
}

/** A notes clip as the app names it: `Bass's Notes clip at bar 3`. */
function clipName(graph: GraphState, clip: ClipRef) {
  const track = graph.view.modules.find((m) => m.key === clip.module);
  const lane = findLane(graph.timeline, clip.module, clip.lane);
  const at = findNoteClip(graph.timeline, clip);
  const bar = at ? ` at bar ${barAt(graph.timeline.meter, at.start).bar + 1}` : "";
  return `${track ? moduleName(track) : clip.module}'s ${lane ? labelOf(lane) : clip.lane} clip${bar}`;
}

/** The project file a module is written in: where most of its nodes are. */
function fileOf(m: ModuleView): string | null {
  const count = new Map<string, number>();
  for (const n of m.nodes) if (n.file) count.set(n.file, (count.get(n.file) ?? 0) + 1);
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
}

/** Why the files on disk don't compile, where (project-relative), and a way to the code. */
function CompileBanner({ graph, onShow }: { graph: GraphState; onShow: (p: Problem) => void }) {
  // The diagnostics name files relative to the project; the raw errors are the fallback.
  const problems: Problem[] = graph.diagnostics.length
    ? graph.diagnostics
    : graph.view.draft_errors.map((message): Problem => ({ file: null, line: null, col: null, message, code: "code.build", severity: "error" }));
  return (
    <div className="shrink-0 px-5 py-2 bg-amber-500/10 border-b border-amber-600/20 text-[13px] text-amber-950 flex items-start gap-3" data-testid="compile-banner">
      <TriangleAlert className="size-4 mt-0.5 shrink-0 text-amber-600" />
      <div className="flex-1 min-w-0 space-y-0.5">
        <div>
          <span className="font-medium">The code doesn't compile right now.</span>{" "}
          <span className="text-amber-900/80">
            {graph.has_code === false
              ? "Nothing plays until it does: fix it in the code, and the song plays once it compiles."
              : "Playing and showing the last version that did; knobs unlock when it compiles."}
          </span>
        </div>
        {problems.slice(0, 2).map((p, i) => (
          <div key={i} className="flex items-baseline gap-2 min-w-0 text-xs">
            {p.file && (
              <span className="shrink-0 font-mono text-amber-800">
                {p.file}
                {p.line != null && `:${p.line}${p.col != null ? `:${p.col}` : ""}`}
              </span>
            )}
            <span className="min-w-0 truncate font-mono text-amber-950/90" title={p.message}>{p.message}</span>
            {p.code !== "code.build" && <span className="shrink-0 font-mono text-amber-800/60">{p.code}</span>}
          </div>
        ))}
        {problems.length > 2 && <div className="text-xs text-amber-900/70">and {problems.length - 2} more</div>}
      </div>
      <button
        onClick={() => onShow(problems[0])}
        className="shrink-0 h-7 px-3 rounded-lg bg-amber-600/15 hover:bg-amber-600/25 text-amber-950 text-xs font-medium inline-flex items-center gap-1.5 cursor-pointer transition-colors"
      >
        <CodeXml className="size-3.5" />
        Show in code
      </button>
    </div>
  );
}

/** The project is open for writing in another program (WRITE-17): this window plays it and shows
 * what changes on disk, but edits are refused until the lock is free and Retry takes it. */
function ReadOnlyBanner({ graph, readOnly }: { graph: GraphState; readOnly: ReadOnly }) {
  const toast = useStore((s) => s.toast);
  const [trying, setTrying] = useState(false);
  const holder = readOnly.holder;
  const since = holder ? new Date(holder.since * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null;
  const who = holder ? `${holder.program} (pid ${holder.pid}, since ${since})` : "another program";
  const retry = async () => {
    setTrying(true);
    try {
      const state = await graphApi.retryLock(graph.session);
      if (state.read_only) toast(`Still read-only: the project is open for writing in ${who}.`);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      setTrying(false);
    }
  };
  return (
    <div className="shrink-0 px-5 py-2 bg-sky-500/10 border-b border-sky-600/20 text-[13px] text-sky-950 flex items-center gap-3" data-testid="read-only-banner">
      <Lock className="size-4 shrink-0 text-sky-700" />
      <div className="flex-1 min-w-0">
        <span className="font-medium">Read only.</span>{" "}
        <span className="text-sky-900/80">
          {readOnly.why === "held"
            ? `This project is open for writing in ${who}. Make edits there, or close it there and press Retry.`
            : readOnly.message}
        </span>
      </div>
      <button
        onClick={retry}
        disabled={trying}
        data-testid="read-only-retry"
        className="shrink-0 h-7 px-3 rounded-lg bg-sky-600/15 hover:bg-sky-600/25 text-sky-950 text-xs font-medium cursor-pointer transition-colors disabled:opacity-50"
      >
        Retry
      </button>
    </div>
  );
}

/** Undo or redo an edit to the open graph project's code. */
export async function graphUndo(session: number, id: "undo" | "redo") {
  const toast = useStore.getState().toast;
  await (id === "redo" ? graphApi.redo(session) : graphApi.undo(session))
    .then((c) => toast(`${id === "redo" ? "Redid" : "Undid"}: ${c.file} ${c.before} → ${c.after}`))
    .catch((error) => toast(error instanceof Error ? error.message : String(error)));
}

/**
 * A toast's Undo for the edit that made `revision`: it undoes only while that edit is still the
 * newest change, since later Undo would take back something else.
 */
export function undoAction(session: number, revision: number) {
  return {
    label: "Undo",
    run: () => {
      const now = useStore.getState().graph;
      if (now?.session === session && now.view.revision <= revision) void graphUndo(session, "undo");
      else useStore.getState().toast("The song has changed since, so this can't be undone from here. Use Undo in the top bar.");
    },
  };
}

/** `bars 5–8`, or the beats when the loop isn't on bar lines. */
function loopName(meter: Meter[], l: Loop) {
  const a = barAt(meter, l.start), b = barAt(meter, l.end);
  if (a.start !== l.start || b.start !== l.end) return `beats ${Number(l.start.toFixed(3))}–${Number(l.end.toFixed(3))}`;
  // It ends where bar `b` starts: its last bar is the one before.
  return b.bar - 1 === a.bar ? `bar ${a.bar + 1}` : `bars ${a.bar + 1}–${b.bar}`;
}

/** The loop toggle: loops the range set, or with none set, the 4 bars from the bar the playhead is in. */
function LoopButton({ meter, status, onLoop }: { meter: Meter[]; status: GraphStatus; onLoop: (req: { start?: number; end?: number; on?: boolean }) => void }) {
  const l = status.loop;
  const toggle = () => {
    if (l) return onLoop({ on: !l.on });
    const bar = barAt(meter, status.beat);
    onLoop({ start: bar.start, end: bar.start + barsToBeats(meter, bar.start, 4) });
  };
  const title = l
    ? l.on
      ? `Looping ${loopName(meter, l)}: click to play straight on`
      : `Loop ${loopName(meter, l)} (drag across the ruler to loop somewhere else)`
    : "Loop 4 bars from the playhead (or drag across the ruler to choose where)";
  return (
    <Button
      data-testid="loop-toggle"
      aria-label="Loop"
      aria-pressed={!!l?.on}
      onClick={toggle}
      title={title}
      className={clsx("!px-2.5", l?.on && "!bg-sun/25 !text-ink-100 ring-1 ring-sun/60")}
    >
      <Repeat className="size-4" />
    </Button>
  );
}

function GraphBar({
  graph,
  status,
  onSeek,
  onLoop,
}: {
  graph: GraphState;
  status: GraphStatus;
  onSeek: (beat: number) => void;
  onLoop: (req: { start?: number; end?: number; on?: boolean }) => void;
}) {
  const toast = useStore((s) => s.toast);
  // Unsaved code would be lost: say so, and close only when asked again.
  const close = () => {
    const n = editorHooks.unsaved?.() ?? 0;
    if (!n) return void graphApi.close();
    toast(`${n === 1 ? "A file has" : `${n} files have`} unsaved edits in the code editor.`, { label: "Close anyway", run: () => void graphApi.close() });
  };
  const time = `${Math.floor(status.seconds / 60)}:${(status.seconds % 60).toFixed(1).padStart(4, "0")}`;
  // Playback sits on whole frames, so a seek to a beat can land a hair before it.
  const beat = Math.round(status.beat * 1000) / 1000;
  const at = position(graph.timeline.meter, beat);
  return (
    <header data-tauri-drag-region className={clsx("relative h-16 shrink-0 flex items-center gap-3 px-4 border-b border-black/[0.06] bg-ink-900", isMac && "pl-[88px]")}>
      <div data-tauri-drag-region className="size-8 rounded-xl pop-gradient grid place-items-center shadow-lg shadow-pop-500/15">
        <span className="text-white text-sm font-bold">m2</span>
      </div>
      <button
        onClick={() => graphApi.transport("toggle")}
        title="Play / stop (Space)"
        className={clsx(
          "size-11 rounded-full grid place-items-center transition-all cursor-pointer active:scale-95",
          status.playing ? "bg-ink-200 text-white" : "pop-gradient text-white shadow-lg shadow-pop-500/15 hover:brightness-110",
        )}
      >
        {status.playing ? <Pause className="size-5 fill-current" /> : <Play className="size-5 fill-current translate-x-[1px]" />}
      </button>
      <Button onClick={() => onSeek(0)} title="Back to start" className="!px-2.5">
        <SkipBack className="size-4" />
      </Button>
      <LoopButton meter={graph.timeline.meter} status={status} onLoop={onLoop} />
      <div className="flex items-baseline gap-2.5 font-mono tabular-nums">
        <span className="text-[15px] text-ink-100 min-w-12" title="Bar and beat">{at}</span>
        <span className="text-xs text-ink-400 w-12" title="Time">{time}</span>
      </div>
      <div data-tauri-drag-region className="flex-1 self-stretch" />
      {/* Centred in the window, whatever the transport and buttons beside it take. It keeps clear of
          them (400px a side holds the widest, the loop button and an export running included): a
          long name is cut short. What the engine is busy with shows under the name, inside the
          same room, so it never covers the name or the buttons. */}
      <div className="absolute inset-y-0 left-1/2 -translate-x-1/2 max-w-[calc(100%-800px)] flex flex-col items-center justify-center whitespace-nowrap pointer-events-none">
        <div className="max-w-full flex items-center gap-2">
          <span data-testid="bar-song-name" className="min-w-0 truncate text-[15px] font-semibold text-ink-100 pointer-events-auto" title={`${graph.name}\n${graph.dir}${graph.sample_rate ? `\n${graph.sample_rate / 1000} kHz` : ""}`}>
            {graph.name}
          </span>
          <span className="shrink-0 text-xs text-ink-400 font-mono pointer-events-auto" title="Every accepted change to the code is a new revision">rev {graph.view.revision}</span>
        </div>
        <EngineNote status={status} />
      </div>
      <ExportButton dir={graph.dir} />
      <div className="w-px h-5 bg-black/[0.08] mx-0.5" />
      <Button
        onClick={() => void graphUndo(graph.session, "undo")}
        disabled={!graph.can_undo}
        title={`Undo the last change to the code (${isMac ? "⌘Z" : "Ctrl+Z"})`}
        aria-label="Undo"
        className="!px-2.5"
      >
        <Undo2 className="size-4" />
      </Button>
      <Button
        onClick={() => void graphUndo(graph.session, "redo")}
        disabled={!graph.can_redo}
        title={`Redo (${isMac ? "⇧⌘Z" : "Ctrl+Shift+Z"})`}
        aria-label="Redo"
        className="!px-2.5"
      >
        <Redo2 className="size-4" />
      </Button>
      <Button onClick={close} title="Close the project" aria-label="Close the project" className="!px-2.5">
        <X className="size-4" />
      </Button>
    </header>
  );
}

/** What the engine is busy with besides playing (RENDER-1, RENDER-25): paused while an export borrows the
 * plugins that play, or loading the plugins an edit needs. The song plays on while plugins load. */
function EngineNote({ status }: { status: GraphStatus }) {
  const loading = status.loading ?? [];
  const text = status.paused === "exporting" ? "Paused for export" : loading.length ? `Loading ${loading.join(", ")}…` : null;
  if (!text) return null;
  return (
    <span data-testid="engine-note" role="status" className="max-w-full text-xs text-ink-400 truncate pointer-events-auto" title={text}>
      {text}
    </span>
  );
}

/** Export Audio: the song to a WAV file. While it renders, a ring fills with its progress. Another
 * song's export (one opened before this) shows in a toast instead: see Toasts. */
function ExportButton({ dir }: { dir: string }) {
  const progress = useExport((s) => (s.dir === dir ? s.progress : null));
  if (progress === null) {
    return (
      <Button data-testid="export" onClick={() => void exportSong()} title={`Export the song as a WAV audio file (${kbd("E")})`}>
        <Download className="size-4" />
        Export
      </Button>
    );
  }
  const percent = Math.round(progress * 100);
  // r = 6 in a 16-unit box: the ring's length, for the dash that fills it.
  const length = 2 * Math.PI * 6;
  return (
    <Button data-testid="export" aria-busy title="Exporting the song…" className="!text-ink-100 !bg-ink-850 ring-1 ring-black/[0.06] cursor-default tabular-nums">
      <svg
        data-testid="export-progress"
        role="progressbar"
        aria-label="Export progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        viewBox="0 0 16 16"
        className="size-4 -rotate-90"
      >
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity={0.15} strokeWidth="2.25" />
        <circle
          cx="8"
          cy="8"
          r="6"
          fill="none"
          stroke="var(--color-pop-500)"
          strokeWidth="2.25"
          strokeLinecap="round"
          strokeDasharray={length}
          strokeDashoffset={length * (1 - Math.max(progress, 0.02))}
          className="transition-[stroke-dashoffset] duration-150"
        />
      </svg>
      Exporting <span className="w-8 text-left text-ink-400">{percent}%</span>
    </Button>
  );
}

/** The panel's view of the selected module. Memoized: the playback status re-renders the canvas five times a second. */
const PanelBody = memo(function PanelBody({
  graph,
  module,
  view,
  clip,
  onCloseClip,
}: {
  graph: GraphState;
  module: string | undefined;
  view: PanelView;
  clip: ClipRef | null;
  onCloseClip: () => void;
}) {
  // Once opened, the code editor stays mounted, hidden behind the other views: its unsaved text,
  // history and ⌘S hook outlive a look at the graph or a clip.
  const [coded, setCoded] = useState(view === "code");
  if (view === "code" && !coded) setCoded(true);
  return (
    <>
      {view === "graph" && <NodeGraph graph={graph} module={module ?? ""} />}
      {/* No module before the code first compiles (WRITE-20): the editor opens on the song's files. */}
      {coded && (
        <div className={clsx("flex-1 min-h-0 flex-col", view === "code" ? "flex" : "hidden")}>
          <CodeView graph={graph} module={module ?? "song"} />
        </div>
      )}
      {view === "notes" && clip && (
        <NoteView key={`${clip.module}:${clip.lane}:${clip.id}`} graph={graph} clip={clip} onClose={onCloseClip} />
      )}
    </>
  );
});

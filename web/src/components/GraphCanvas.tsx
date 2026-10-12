import clsx from "clsx";
import { FileCode2, Pause, Play, Plus, Redo2, SkipBack, Trash2, Undo2, X } from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  display,
  fromUnit,
  graphApi,
  layout,
  NODE_W,
  toUnit,
  type Effect,
  type GraphState,
  type GraphStatus,
  type Knob as KnobInfo,
  type ModuleView,
  type ViewNode,
} from "../lib/graph";
import { isMac } from "../lib/native";
import { useStore } from "../lib/store";
import SongTimeline, { trackColor } from "./SongTimeline";
import { Button, Knob } from "./ui";

const PANEL_KEY = "m2.graph.panel";
const clampPanel = (h: number) => Math.round(Math.min(Math.max(h, 120), window.innerHeight * 0.8));

/** A song-as-program project: its timeline, and below it the selected module's code as a graph to play and tweak. */
export default function GraphCanvas({ graph }: { graph: GraphState }) {
  const { view, timeline } = graph;
  const toast = useStore((s) => s.toast);

  const [status, setStatus] = useState<GraphStatus>({ playing: graph.playing, seconds: graph.seconds, beat: graph.beat });
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

  // One selection for the timeline and the panel. If an edit removes the module, the default shows.
  const [picked, setPicked] = useState<string | null>(null);
  const tabs = [...view.modules.filter((m) => m.name === "song"), ...view.modules.filter((m) => m.name !== "song")];
  const module = view.modules.find((m) => m.name === picked) ?? view.modules.find((m) => m.track) ?? tabs[0];

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
      <GraphBar graph={graph} status={status} onSeek={seek} />
      {view.draft_errors.length > 0 && (
        <div className="px-5 py-2.5 bg-amber-500/10 border-b border-amber-600/20 text-[13px] text-amber-900 space-y-0.5">
          <div className="font-medium">The code doesn't compile right now. Playing and showing the last version that did. Knobs will unlock when the code compiles.</div>
          {view.draft_errors.slice(0, 3).map((e, i) => (
            <div key={i} className="font-mono text-xs truncate">{e}</div>
          ))}
        </div>
      )}
      <SongTimeline timeline={timeline} beat={status.beat} playing={status.playing} selected={module?.name} onSelect={setPicked} onSeek={seek} />
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
      <div ref={panel} className="shrink-0 min-h-[120px] max-h-[80vh] flex flex-col bg-ink-950" style={{ height }}>
        <div role="tablist" aria-label="Modules" className="h-11 shrink-0 flex items-center gap-1 px-3 bg-ink-900 border-b border-black/[0.06] overflow-x-auto">
          {tabs.map((m) => {
            const color = trackColor(timeline, m.name);
            return (
              <button
                key={m.name}
                role="tab"
                aria-selected={m === module}
                data-module-tab={m.name}
                onClick={() => setPicked(m.name)}
                className={clsx(
                  "h-7 px-3 shrink-0 rounded-lg text-[13px] font-medium inline-flex items-center gap-2 cursor-pointer transition-colors",
                  m === module ? "bg-ink-700 text-ink-100" : "text-ink-400 hover:text-ink-100 hover:bg-ink-850",
                )}
              >
                {color && <span className="size-2 rounded-full" style={{ background: color }} />}
                {m.name === "song" ? "Song" : m.name}
              </button>
            );
          })}
        </div>
        <div className="flex-1 min-h-0 overflow-auto p-4" data-testid="graph">
          {module && (
            <ModuleSection
              key={`${graph.session}:${module.name}`}
              module={module}
              session={graph.session}
              revision={view.revision}
              editable={view.draft_errors.length === 0}
              effects={graph.effects}
            />
          )}
        </div>
      </div>
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

function GraphBar({ graph, status, onSeek }: { graph: GraphState; status: GraphStatus; onSeek: (beat: number) => void }) {
  const time = `${Math.floor(status.seconds / 60)}:${(status.seconds % 60).toFixed(1).padStart(4, "0")}`;
  const bpb = graph.timeline.beats_per_bar;
  // Playback sits on whole frames, so a seek to a beat can land a hair before it.
  const beat = Math.round(status.beat * 1000) / 1000;
  const position = `${Math.floor(beat / bpb) + 1}.${Math.floor(beat % bpb) + 1}`;
  return (
    <header data-tauri-drag-region className={clsx("h-16 shrink-0 flex items-center gap-3 px-4 border-b border-black/[0.06] bg-ink-900", isMac && "pl-[88px]")}>
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
      <div className="flex items-baseline gap-2.5 font-mono tabular-nums">
        <span className="text-[15px] text-ink-100 min-w-12" title="Bar and beat">{position}</span>
        <span className="text-xs text-ink-400 w-12" title="Time">{time}</span>
      </div>
      <div data-tauri-drag-region className="flex-1 flex items-center justify-center gap-2">
        <span className="text-[15px] font-semibold text-ink-100">{graph.name}</span>
        <span className="text-xs text-ink-400 font-mono" title="Every accepted change to the code is a new revision">rev {graph.view.revision}</span>
      </div>
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
      <span className="text-xs text-ink-400 truncate max-w-[280px]" title={graph.dir}>{graph.dir}</span>
      <Button onClick={() => graphApi.close()} title="Close the project" className="!px-2.5">
        <X className="size-4" />
      </Button>
    </header>
  );
}

/** A cable being dragged from a node's output, in the module's coordinates. */
interface Cable { from: string; name: string; x0: number; y0: number; x: number; y: number }

/** Memoized: the playback status re-renders the canvas five times a second. */
const ModuleSection = memo(function ModuleSection({
  module,
  session,
  revision,
  editable,
  effects,
}: {
  module: ModuleView;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
}) {
  const { placed, width, height } = useMemo(() => layout(module), [module]);
  const toast = useStore((s) => s.toast);
  const area = useRef<HTMLDivElement>(null);
  const [cable, setCable] = useState<Cable | null>(null);

  // Drag from an output onto a knob: the knob's value is replaced by the signal's name.
  const startCable = (node: ViewNode, e: React.PointerEvent) => {
    const box = area.current?.getBoundingClientRect();
    const p = placed.get(node.id);
    if (!box || !p || !node.outlet || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const at = (ev: PointerEvent | React.PointerEvent) => ({ x: ev.clientX - box.left, y: ev.clientY - box.top });
    const gesture = { session, revision, from: node.id, name: node.outlet };
    setCable({ from: node.id, name: node.outlet, x0: p.x + NODE_W, y0: p.y + 23, ...at(e) });
    const move = (ev: PointerEvent) => setCable((c) => c && { ...c, ...at(ev) });
    const end = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setCable(null);
      if (ev.type !== "pointerup") return;
      const target = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-knob-id]");
      if (!target || !area.current?.contains(target)) return;
      graphApi
        .connect(gesture.session, gesture.revision, gesture.from, target.dataset.knobId!)
        .then((c) => toast(`${c.file}: ${c.before} → ${c.after}`))
        .catch((error) => toast(error instanceof Error ? error.message : String(error)));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  return (
    <section className="w-max min-w-full rounded-2xl bg-ink-900 ring-1 ring-black/[0.05] p-4" data-module={module.name}>
      <div className="flex items-baseline gap-2 mb-3">
        <h2 className="text-sm font-semibold text-ink-100">{module.name === "song" ? "Song" : module.name}</h2>
        <span className="text-xs text-ink-400">{module.track ? "track" : "mix"}</span>
      </div>
      <div ref={area} className="relative" style={{ width, height }} data-cabling={cable ? true : undefined}>
        <svg className="absolute inset-0 pointer-events-none overflow-visible z-10" width={width} height={height}>
          {cable && (
            <path
              d={`M ${cable.x0} ${cable.y0} C ${cable.x0 + 60} ${cable.y0}, ${cable.x - 60} ${cable.y}, ${cable.x} ${cable.y}`}
              stroke="var(--color-pop-500)"
              strokeWidth="2"
              strokeDasharray="5 4"
              fill="none"
            />
          )}
        </svg>
        <svg className="absolute inset-0 pointer-events-none overflow-visible" width={width} height={height}>
          {module.edges.map(([a, b]) => {
            const p = placed.get(a), q = placed.get(b);
            if (!p || !q) return null;
            const x0 = p.x + NODE_W, y0 = p.y + 23, x1 = q.x, y1 = q.y + 23;
            const dx = Math.max(30, (x1 - x0) / 2);
            return <path key={`${a}>${b}`} d={`M ${x0} ${y0} C ${x0 + dx} ${y0}, ${x1 - dx} ${y1}, ${x1} ${y1}`} stroke="var(--color-ink-500)" strokeOpacity="0.55" strokeWidth="1.5" fill="none" />;
          })}
        </svg>
        {[...placed.values()].map(({ node, x, y, h }) => (
          <NodeCard
            key={node.id}
            node={node}
            session={session}
            revision={revision}
            editable={editable}
            effects={effects}
            onCable={startCable}
            style={{ left: x, top: y, width: NODE_W, height: h }}
          />
        ))}
      </div>
    </section>
  );
});

const KIND_STYLE: Record<ViewNode["kind"], string> = {
  call: "bg-ink-800",
  value: "bg-ink-800",
  lane: "bg-sky-50 ring-sky-900/10",
  time: "bg-ink-850",
  import: "bg-ink-850",
  export: "bg-ink-850",
};

function NodeCard({
  node,
  session,
  revision,
  editable,
  effects,
  onCable,
  style,
}: {
  node: ViewNode;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  onCable: (node: ViewNode, e: React.PointerEvent) => void;
  style: React.CSSProperties;
}) {
  const toast = useStore((s) => s.toast);
  const [picking, setPicking] = useState(false);
  const edited = (p: Promise<{ file: string; before: string; after: string }>) =>
    p.then((c) => toast(`${c.file}: ${c.before} → ${c.after}`)).catch((error) => toast(error instanceof Error ? error.message : String(error)));
  const disconnect = (id: string) =>
    graphApi
      .disconnect(session, revision, id)
      .then((c) => toast(`${c.file}: ${c.before} → ${c.after}`))
      .catch((error) => toast(error instanceof Error ? error.message : String(error)));
  return (
    <div
      className={clsx("absolute rounded-xl ring-1 ring-black/[0.07] shadow-sm flex flex-col", KIND_STYLE[node.kind])}
      style={style}
      data-node={node.label}
      title={node.file ? `${node.file}:${node.line}` : undefined}
    >
      <div className="h-[46px] shrink-0 px-3 flex items-center gap-2 min-w-0">
        {node.kind === "lane" && <span className="shrink-0 text-[10px] uppercase tracking-wide text-sky-700/70 font-semibold">lane</span>}
        <span className={clsx("font-mono text-[13px] truncate", node.kind === "call" || node.kind === "lane" ? "text-ink-100 font-medium" : "text-ink-300")}>{node.label}</span>
        {node.file && (
          <span className="ml-auto shrink-0 flex items-center gap-1 text-[10px] text-ink-400 font-mono">
            <FileCode2 className="size-3" />
            {node.file.split("/").pop()}:{node.line}
          </span>
        )}
        {node.effect && (
          <button
            className="shrink-0 size-6 grid place-items-center rounded-md text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none"
            title={`Remove ${node.effect}, keeping what it was applied to`}
            aria-label={`Remove ${node.effect}`}
            disabled={!editable}
            onClick={() => void edited(graphApi.removeEffect(session, revision, node.id))}
          >
            <Trash2 className="size-3.5" />
          </button>
        )}
        {node.audio && effects.length > 0 && (
          <button
            className={clsx(
              "shrink-0 size-6 grid place-items-center rounded-md text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none",
              picking && "bg-black/5 text-ink-100",
            )}
            title="Add an effect after this"
            aria-label={`Add an effect after ${node.label}`}
            aria-expanded={picking}
            disabled={!editable}
            onClick={() => setPicking((p) => !p)}
          >
            <Plus className="size-3.5" />
          </button>
        )}
      </div>
      {picking && (
        <div
          className="absolute z-20 right-1 top-10 w-52 rounded-lg bg-ink-900 ring-1 ring-black/10 shadow-lg py-1"
          role="menu"
          onMouseLeave={() => setPicking(false)}
        >
          <div className="px-3 py-1 text-[10px] uppercase tracking-wide text-ink-400">Add after {node.label}</div>
          {effects.map((effect) => (
            <button
              key={`${effect.module}.${effect.name}`}
              role="menuitem"
              data-effect={effect.name}
              className="w-full text-left px-3 py-1.5 hover:bg-black/5 flex items-baseline gap-2"
              onClick={() => {
                setPicking(false);
                void edited(graphApi.addEffect(session, revision, node.id, effect));
              }}
            >
              <span className="font-mono text-xs text-ink-100">{effect.name}</span>
              <span className="text-[11px] text-ink-400 truncate">{effect.description}</span>
            </button>
          ))}
        </div>
      )}
      {node.outlet && (
        <button
          className={clsx(
            "absolute -right-[7px] top-[16px] size-3.5 rounded-full ring-2 ring-ink-900 transition-transform",
            editable ? "bg-pop-500 cursor-crosshair hover:scale-125" : "bg-ink-500 pointer-events-none",
          )}
          data-outlet={node.outlet}
          aria-label={`Connect ${node.outlet}`}
          title={editable ? `Drag onto a knob to connect ${node.outlet}` : "Fix the code errors before connecting."}
          disabled={!editable}
          onPointerDown={(e) => onCable(node, e)}
        />
      )}
      {node.inlets.map((inlet) => (
        <div
          key={inlet.id}
          className="h-8 shrink-0 px-3 flex items-center gap-1.5 border-t border-black/[0.05] text-xs min-w-0"
          data-inlet={inlet.label}
        >
          <span className="text-ink-400 truncate">{inlet.label}</span>
          <span className="text-ink-400">←</span>
          <span className="font-mono text-ink-200 truncate">{inlet.name}</span>
          <button
            className="ml-auto shrink-0 size-5 grid place-items-center rounded text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none"
            title={`Disconnect ${inlet.name} (writes the input's default)`}
            aria-label={`Disconnect ${inlet.name} from ${inlet.label}`}
            disabled={!editable}
            onClick={() => void disconnect(inlet.id)}
          >
            <X className="size-3" />
          </button>
        </div>
      ))}
      {node.knobs.length > 0 && (
        <div className="grid grid-cols-3 auto-rows-[84px] px-1 pb-2">
          {node.knobs.map((k) => (
            <KnobControl key={`${editable}:${k.id}`} knob={k} session={session} revision={revision} editable={editable} />
          ))}
        </div>
      )}
      {node.readonly.map((source, i) => (
        <div
          key={`${source.label}:${i}`}
          className="h-16 shrink-0 px-3 py-2 border-t border-black/[0.05] min-w-0"
          data-expression={source.label}
          title={`${source.reason}${source.file ? `\n${source.file}:${source.line}` : ""}\n${source.expression}`}
        >
          <div className="flex items-center justify-between gap-2 text-[10px] text-ink-400">
            <span className="truncate">{source.label}</span>
            <span className="shrink-0">edit in code</span>
          </div>
          <div className="font-mono text-xs text-ink-200 truncate">{source.expression}</div>
          <div className="text-[10px] text-ink-400 truncate">{source.reason}</div>
        </div>
      ))}
    </div>
  );
}

interface Gesture {
  token: string;
  session: number;
  revision: number;
  knob: KnobInfo;
  start: number;
  previews: Promise<void>;
  previewed: boolean;
  /** Let go: from here it commits, even if the control unmounts. */
  released: boolean;
  cancelled: boolean;
  submitted: boolean;
  failed: boolean;
}

/** A knob over an owned source value: drag to hear it, release to write it. */
function KnobControl({ knob, session, revision, editable }: { knob: KnobInfo; session: number; revision: number; editable: boolean }) {
  const toast = useStore((s) => s.toast);
  const [value, setValue] = useState(knob.value);
  const [saving, setSaving] = useState(false);
  // Each gesture captures its identity, revision and value mapping. A save elsewhere
  // doesn't end it: the server carries it to the new revision, or refuses it as stale
  // if the save touched this value. Session changes remount the control. Unmounting
  // cancels a drag, but a released knob still commits: selecting another module right
  // after letting go must not lose the value.
  const latest = useRef(knob.value);
  const dragging = useRef<Gesture | null>(null);
  const active = useRef<Gesture | null>(null);
  const pending = useRef<{ gesture: Gesture; value: number } | null>(null);
  const timer = useRef<number | null>(null);
  const mounted = useRef(true);
  const committing = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = null;
      pending.current = null;
      dragging.current = null;
      if (active.current && !active.current.released) void cancel(active.current);
    };
  }, []);

  useEffect(() => {
    if (!dragging.current && !committing.current) {
      setValue(knob.value);
      latest.current = knob.value;
    }
  }, [knob.value]);

  const refresh = () => {
    // Never let a late response replace another project's view or a newer revision.
    void graphApi.get().then((graph) => {
      const store = useStore.getState();
      if (store.graph?.session === session && graph.session === session && graph.view.revision >= store.graph.view.revision) {
        store.set({ graph });
      }
    }).catch(() => {});
  };

  const fail = (gesture: Gesture, error: unknown) => {
    if (gesture.failed) return;
    gesture.failed = true;
    void cancel(gesture);
    // A released knob reports even from a closed panel: its value didn't save.
    if (mounted.current || gesture.released) toast(error instanceof Error ? error.message : String(error));
    if (!mounted.current) return;
    setValue(knob.value);
    latest.current = knob.value;
    refresh();
  };

  const cancel = (gesture: Gesture): Promise<void> => {
    // Once a save is sent it owns completion. Cancelling then could race a
    // successful commit; stale cancellation must never undo a newer gesture.
    if (gesture.cancelled || gesture.submitted) return Promise.resolve();
    gesture.cancelled = true;
    if (active.current === gesture) active.current = null;
    return gesture.previews.then(() => graphApi.cancel(gesture.session, gesture.knob.id, gesture.token)).catch((error) => {
      if (mounted.current) toast(error instanceof Error ? error.message : String(error));
    });
  };

  const begin = (): Gesture => {
    const gesture: Gesture = {
      token: crypto.randomUUID(), session, revision, knob, start: knob.value,
      previews: Promise.resolve(), previewed: false, released: false, cancelled: false, submitted: false, failed: false,
    };
    active.current = gesture;
    return gesture;
  };

  const preview = (gesture: Gesture, v: number) => {
    gesture.previewed = true;
    // Preserve preview order, including the final preview before committing.
    gesture.previews = gesture.previews.then(async () => {
      if (!mounted.current || gesture.failed || gesture.cancelled) return;
      await graphApi.preview(gesture.session, gesture.revision, gesture.knob.id, gesture.token, v);
    }).catch((error) => fail(gesture, error));
  };

  const flushPreview = () => {
    timer.current = null;
    const next = pending.current;
    pending.current = null;
    if (next) preview(next.gesture, next.value);
  };

  const commit = async (gesture: Gesture, v: number) => {
    if (!gesture.previewed && v === gesture.start) {
      if (active.current === gesture) active.current = null;
      return;
    }
    gesture.released = true;
    committing.current = true;
    setSaving(true);
    try {
      await gesture.previews;
      if (gesture.failed || gesture.cancelled) {
        void cancel(gesture);
        return;
      }
      // Returning to the starting value still needs a reliable commit: the
      // final audition message may have been dropped by the audio queue.
      gesture.submitted = true;
      const change = await graphApi.set(gesture.session, gesture.revision, gesture.knob.id, gesture.token, v, gesture.start);
      if (change.before !== change.after) toast(`${change.file}: ${change.before} → ${change.after}`);
      refresh();
    } catch (error) {
      // Rejections restore audio on the server. An owner-checked cancellation
      // also covers a lost response without targeting another gesture's preview.
      gesture.submitted = false;
      fail(gesture, error);
    } finally {
      if (active.current === gesture) active.current = null;
      committing.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  const descriptor = dragging.current?.knob ?? knob;
  const disabled = !editable || saving;
  const shared = knob.uses > 1 ? `Shared value: changes all ${knob.uses} uses. ` : "";
  const title = !editable
    ? "Fix the code errors before editing this control."
    : `${shared}${knob.file}:${knob.line}  (written as ${knob.literal})`;

  return (
    <div
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-value={value}
      data-shared={knob.uses > 1 || undefined}
      aria-disabled={disabled}
      title={title}
      className="rounded-lg [[data-cabling]_&]:ring-2 [[data-cabling]_&]:ring-pop-500/40 [[data-cabling]_&:hover]:bg-pop-500/10"
    >
      <div inert={disabled} className={clsx(disabled && "opacity-45 pointer-events-none")}>
        <Knob
          value={Math.min(1, Math.max(0, toUnit(descriptor, value)))}
          label={knob.label}
          display={display(descriptor, value)}
          title={title}
          onStart={() => {
            if (!editable || committing.current) return;
            dragging.current = begin();
            latest.current = knob.value;
          }}
          onChange={(u) => {
            if (!editable || committing.current) return;
            const gesture = dragging.current ?? begin();
            if (gesture.failed) return;
            const v = fromUnit(gesture.knob, u);
            setValue(v);
            latest.current = v;
            if (!dragging.current) {
              // A double-click reset is a complete, revision-bound gesture.
              preview(gesture, v);
              void commit(gesture, v);
              return;
            }
            pending.current = { gesture, value: v };
            if (timer.current == null) timer.current = window.setTimeout(flushPreview, 30);
          }}
          onEnd={() => {
            const gesture = dragging.current;
            if (!gesture) return;
            dragging.current = null;
            if (timer.current != null) clearTimeout(timer.current);
            flushPreview();
            void commit(gesture, latest.current);
          }}
          onCancel={() => {
            const gesture = dragging.current;
            if (!gesture) return;
            dragging.current = null;
            pending.current = null;
            if (timer.current != null) clearTimeout(timer.current);
            timer.current = null;
            setValue(knob.value);
            latest.current = knob.value;
            committing.current = true;
            setSaving(true);
            void cancel(gesture).finally(() => {
              committing.current = false;
              if (mounted.current) setSaving(false);
            });
          }}
        />
        {knob.uses > 1 && <div className="text-[9px] leading-3 text-center text-pop-500 -mt-0.5">shared ×{knob.uses}</div>}
      </div>
    </div>
  );
}

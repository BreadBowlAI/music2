import clsx from "clsx";
import { FileCode2, Pause, Play, SkipBack, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { display, fromUnit, graphApi, layout, NODE_W, toUnit, type GraphState, type Knob as KnobInfo, type ModuleView, type ViewNode } from "../lib/graph";
import { isMac } from "../lib/native";
import { useStore } from "../lib/store";
import { Button, Knob } from "./ui";

/** A song-as-program project: its code, shown as a graph you can play and tweak. */
export default function GraphCanvas({ graph }: { graph: GraphState }) {
  const { view } = graph;
  return (
    <div className="h-full flex flex-col">
      <GraphBar graph={graph} />
      {view.draft_errors.length > 0 && (
        <div className="px-5 py-2.5 bg-amber-500/10 border-b border-amber-600/20 text-[13px] text-amber-900 space-y-0.5">
          <div className="font-medium">The code doesn't compile right now. Playing and showing the last version that did. Knobs will unlock when the code compiles.</div>
          {view.draft_errors.slice(0, 3).map((e, i) => (
            <div key={i} className="font-mono text-xs truncate">{e}</div>
          ))}
        </div>
      )}
      <div className="flex-1 overflow-auto p-6 space-y-6" data-testid="graph">
        {view.modules.map((m) => (
          <ModuleSection key={`${graph.session}:${m.name}`} module={m} session={graph.session} revision={view.revision} editable={view.draft_errors.length === 0} />
        ))}
      </div>
    </div>
  );
}

function GraphBar({ graph }: { graph: GraphState }) {
  const [status, setStatus] = useState({ playing: graph.playing, seconds: graph.seconds });
  useEffect(() => {
    const t = setInterval(() => graphApi.status().then(setStatus).catch(() => {}), 200);
    return () => clearInterval(t);
  }, []);
  const time = `${Math.floor(status.seconds / 60)}:${(status.seconds % 60).toFixed(1).padStart(4, "0")}`;
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
      <Button onClick={() => graphApi.transport("restart")} title="Back to start" className="!px-2.5">
        <SkipBack className="size-4" />
      </Button>
      <span className="font-mono text-[15px] tabular-nums text-ink-100 w-16">{time}</span>
      <div data-tauri-drag-region className="flex-1 flex items-center justify-center gap-2">
        <span className="text-[15px] font-semibold text-ink-100">{graph.name}</span>
        <span className="text-xs text-ink-400 font-mono" title="Every accepted change to the code is a new revision">rev {graph.view.revision}</span>
      </div>
      <span className="text-xs text-ink-400 truncate max-w-[280px]" title={graph.dir}>{graph.dir}</span>
      <Button onClick={() => graphApi.close()} title="Close the project" className="!px-2.5">
        <X className="size-4" />
      </Button>
    </header>
  );
}

function ModuleSection({ module, session, revision, editable }: { module: ModuleView; session: number; revision: number; editable: boolean }) {
  const { placed, width, height } = useMemo(() => layout(module), [module]);
  return (
    <section className="w-max min-w-full rounded-2xl bg-ink-900 ring-1 ring-black/[0.05] p-4" data-module={module.name}>
      <div className="flex items-baseline gap-2 mb-3">
        <h2 className="text-sm font-semibold text-ink-100">{module.name === "song" ? "Song" : module.name}</h2>
        <span className="text-xs text-ink-400">{module.track ? "track" : "mix"}</span>
      </div>
      <div className="relative" style={{ width, height }}>
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
          <NodeCard key={node.id} node={node} session={session} revision={revision} editable={editable} style={{ left: x, top: y, width: NODE_W, height: h }} />
        ))}
      </div>
    </section>
  );
}

const KIND_STYLE: Record<ViewNode["kind"], string> = {
  call: "bg-ink-800",
  value: "bg-ink-800",
  lane: "bg-sky-50 ring-sky-900/10",
  time: "bg-ink-850",
  import: "bg-ink-850",
  export: "bg-ink-850",
};

function NodeCard({ node, session, revision, editable, style }: { node: ViewNode; session: number; revision: number; editable: boolean; style: React.CSSProperties }) {
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
      </div>
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
  // if the save touched this value. Session changes remount the control.
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
      if (active.current) void cancel(active.current);
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
    if (!mounted.current) return;
    toast(error instanceof Error ? error.message : String(error));
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
      previews: Promise.resolve(), previewed: false, cancelled: false, submitted: false, failed: false,
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
    committing.current = true;
    setSaving(true);
    try {
      await gesture.previews;
      if (!mounted.current || gesture.failed || gesture.cancelled) {
        void cancel(gesture);
        return;
      }
      // Returning to the starting value still needs a reliable commit: the
      // final audition message may have been dropped by the audio queue.
      gesture.submitted = true;
      const change = await graphApi.set(gesture.session, gesture.revision, gesture.knob.id, gesture.token, v, gesture.start);
      if (mounted.current && change.before !== change.after) toast(`${change.file}: ${change.before} → ${change.after}`);
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
    <div data-knob={knob.label} data-value={value} data-shared={knob.uses > 1 || undefined} aria-disabled={disabled} title={title}>
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

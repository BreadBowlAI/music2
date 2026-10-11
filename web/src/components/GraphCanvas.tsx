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
          <div className="font-medium">The code doesn't compile right now. Playing and showing the last version that did.</div>
          {view.draft_errors.slice(0, 3).map((e, i) => (
            <div key={i} className="font-mono text-xs truncate">{e}</div>
          ))}
        </div>
      )}
      <div className="flex-1 overflow-auto p-6 space-y-6" data-testid="graph">
        {view.modules.map((m) => (
          <ModuleSection key={m.name} module={m} revision={view.revision} />
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

function ModuleSection({ module, revision }: { module: ModuleView; revision: number }) {
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
          <NodeCard key={node.id} node={node} revision={revision} style={{ left: x, top: y, width: NODE_W, height: h }} />
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

function NodeCard({ node, revision, style }: { node: ViewNode; revision: number; style: React.CSSProperties }) {
  return (
    <div
      className={clsx("absolute rounded-xl ring-1 ring-black/[0.07] shadow-sm flex flex-col", KIND_STYLE[node.kind])}
      style={style}
      data-node={node.label}
      title={node.file ? `${node.file}:${node.line}` : undefined}
    >
      <div className="h-[46px] px-3 flex items-center gap-2 min-w-0">
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
        <div className="grid grid-cols-3 px-1 pb-2 gap-y-1">
          {node.knobs.map((k) => (
            <KnobControl key={k.id} knob={k} revision={revision} />
          ))}
        </div>
      )}
    </div>
  );
}

/** A knob over a number written in the code: drag to hear it, release to write it. */
function KnobControl({ knob, revision }: { knob: KnobInfo; revision: number }) {
  const toast = useStore((s) => s.toast);
  const [value, setValue] = useState(knob.value);
  // The latest value, readable when the drag ends even if React hasn't re-rendered yet
  // (reading `value` there would commit a stale, mid-drag value).
  const latest = useRef(knob.value);
  const dragging = useRef<{ start: number } | null>(null);
  const pending = useRef<number | null>(null);
  const timer = useRef<number | null>(null);
  useEffect(() => {
    if (!dragging.current) {
      setValue(knob.value);
      latest.current = knob.value;
    }
  }, [knob.value]);

  const commit = (v: number, start: number) => {
    if (v === start) return;
    graphApi
      .set(revision, knob.id, v, start)
      .then((c) => toast(`${c.file}: ${c.before} → ${c.after}`))
      .catch((e) => {
        toast((e as Error).message);
        setValue(start);
        latest.current = start;
        void graphApi.preview(knob.origin, start).catch(() => {});
      });
  };

  const flushPreview = () => {
    timer.current = null;
    if (pending.current == null) return;
    const v = pending.current;
    pending.current = null;
    void graphApi.preview(knob.origin, v).catch(() => {});
  };

  return (
    <div data-knob={knob.label} data-value={value}>
      <Knob
        value={Math.min(1, Math.max(0, toUnit(knob, value)))}
        label={knob.label}
        display={display(knob, value)}
        title={`${knob.file}:${knob.line}  (written as ${knob.literal})`}
        onStart={() => (dragging.current = { start: knob.value })}
        onChange={(u) => {
          const v = fromUnit(knob, u);
          setValue(v);
          latest.current = v;
          if (!dragging.current) {
            // Not a drag (a double-click reset): hear it and write it at once.
            void graphApi.preview(knob.origin, v).catch(() => {});
            return commit(v, knob.value);
          }
          pending.current = v;
          if (timer.current == null) timer.current = window.setTimeout(flushPreview, 30);
        }}
        onEnd={() => {
          const start = dragging.current?.start ?? knob.value;
          dragging.current = null;
          if (timer.current != null) clearTimeout(timer.current);
          flushPreview();
          commit(latest.current, start);
        }}
      />
    </div>
  );
}

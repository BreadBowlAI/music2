import clsx from "clsx";
import {
  Activity,
  AudioLines,
  AudioWaveform,
  Clock,
  Hash,
  LogIn,
  LogOut,
  Minus,
  Music2,
  Piano,
  Plus,
  Scan,
  SlidersHorizontal,
  Spline,
  SquareFunction,
  Trash2,
  X,
} from "lucide-react";
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  display,
  fromUnit,
  graphApi,
  keyNote,
  moduleName,
  toUnit,
  type Effect,
  type GraphState,
  type InputPort,
  type Knob as KnobInfo,
  type ModuleView,
  type OutputPort,
  type PortEdge,
  type PortKind,
  type ViewNode,
} from "../lib/graph";
import { useStore } from "../lib/store";
import EffectBrowser from "./EffectBrowser";

/** One module's code as a graph to play and tweak: knobs over its values, cables between its signals, effects after its audio. */
export function NodeGraph({ graph, module }: { graph: GraphState; module: string }) {
  const shown = graph.view.modules.find((m) => m.key === module);
  const laid = useMemo(() => shown && layout(shown), [shown]);

  // The zoom: fitted to the panel until the user picks one, per module.
  const scroller = useRef<HTMLDivElement>(null);
  const [room, setRoom] = useState<{ w: number; h: number } | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const measure = () => setRoom((r) => (r && r.w === el.clientWidth && r.h === el.clientHeight ? r : { w: el.clientWidth, h: el.clientHeight }));
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  const [zooms, setZooms] = useState<Record<string, number>>({});
  const fit = laid && room ? clampZoom(Math.min((room.w - CHROME_W) / laid.width, (room.h - CHROME_H) / laid.height), MIN_FIT, 1) : 1;
  const zoom = zooms[module] ?? fit;
  const setZoom = useCallback(
    (z: number | null) =>
      setZooms((zs) => {
        const next = { ...zs };
        if (z == null) delete next[module];
        else next[module] = clampZoom(z);
        return next;
      }),
    [module],
  );
  // ⌘/Ctrl + wheel zooms, and so does a trackpad pinch (Chrome sends it as Ctrl + wheel,
  // WebKit, the desktop app's web view, as gesture events).
  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      setZoom(zoomRef.current * Math.exp(-e.deltaY / 240));
    };
    let pinched = 1;
    const pinchStart = (e: Event) => {
      e.preventDefault();
      pinched = zoomRef.current;
    };
    const pinch = (e: Event) => {
      e.preventDefault();
      setZoom(pinched * (e as Event & { scale: number }).scale);
    };
    el.addEventListener("wheel", wheel, { passive: false });
    el.addEventListener("gesturestart", pinchStart);
    el.addEventListener("gesturechange", pinch);
    return () => {
      el.removeEventListener("wheel", wheel);
      el.removeEventListener("gesturestart", pinchStart);
      el.removeEventListener("gesturechange", pinch);
    };
  }, [setZoom]);

  return (
    <>
      <div ref={scroller} className="flex-1 min-h-0 overflow-auto p-4 flex flex-col" data-testid="graph">
        {shown && laid && (
          <ModuleSection
            key={`${graph.session}:${shown.key}`}
            module={shown}
            laid={laid}
            zoom={zoom}
            session={graph.session}
            revision={graph.view.revision}
            editable={graph.view.draft_errors.length === 0}
            effects={graph.effects}
          />
        )}
      </div>
      {shown && <ZoomBar zoom={zoom} fitted={zooms[module] == null} onZoom={setZoom} />}
    </>
  );
}

/** Zooms the graph can take; fitting it to the panel never shrinks it below `MIN_FIT` (it scrolls instead). */
const clampZoom = (z: number, lo = 0.3, hi = 2) => Math.min(hi, Math.max(lo, Number.isFinite(z) ? z : 1));
const MIN_FIT = 0.5;
/** The room around a module's nodes in the scroller: its padding, the section's, and the section's title row. */
const CHROME_W = 72;
const CHROME_H = 108;

/** Zoom out, the zoom (click: actual size), zoom in, fit: floating at the panel's bottom right. */
function ZoomBar({ zoom, fitted, onZoom }: { zoom: number; fitted: boolean; onZoom: (z: number | null) => void }) {
  const button = "h-7 grid place-items-center rounded-lg text-ink-300 hover:text-ink-100 hover:bg-black/5 cursor-pointer transition-colors";
  return (
    <div className="absolute right-8 bottom-8 z-20 flex items-center gap-0.5 p-0.5 rounded-[10px] bg-ink-850/90 backdrop-blur ring-1 ring-black/[0.08] shadow-[0_4px_14px_-6px_rgb(0_0_0/0.18)] text-[11px]" data-zoom={zoom.toFixed(2)}>
      <button className={clsx(button, "w-7")} onClick={() => onZoom(zoom / 1.2)} title="Zoom out (⌘ scroll)" aria-label="Zoom out">
        <Minus className="size-3.5" />
      </button>
      <button className={clsx(button, "w-11 font-medium tabular-nums")} onClick={() => onZoom(1)} title="Actual size" aria-label="Actual size">
        {Math.round(zoom * 100)}%
      </button>
      <button className={clsx(button, "w-7")} onClick={() => onZoom(zoom * 1.2)} title="Zoom in (⌘ scroll)" aria-label="Zoom in">
        <Plus className="size-3.5" />
      </button>
      <span className="w-px h-4 mx-0.5 bg-black/[0.08]" />
      <button
        className={clsx(button, "px-2 gap-1 inline-flex items-center", fitted && "text-pop-500 hover:text-pop-500")}
        onClick={() => onZoom(null)}
        title="Fit the graph to the panel"
        aria-label="Fit to panel"
        aria-pressed={fitted}
      >
        <Scan className="size-3.5" />
        Fit
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- ports and layout

/** What flows through a port, by colour: the legend, the port dots and the edges. */
const KIND_COLOR: Record<PortKind, string> = { audio: "#c45c7c", control: "#4a8cc4", notes: "#3e9b78" };
const KIND_LABEL: Record<PortKind, string> = { audio: "Audio", control: "Control", notes: "Notes" };

const NODE_W = 216;
/** A node with only outputs (a lane, an import) or only inputs (an export). */
const SLIM_W = 152;
const GAP_X = 60;
const GAP_Y = 18;
const HEAD_H = 42;
/** Above and below a node's rows. */
const PAD = 6;
/** An output, a signal coming in, or a cable. */
const ROW_H = 28;
const KNOB_H = 40;
const VALUE_H = 48;

const inputHeight = (p: InputPort) => (p.knob ? KNOB_H : p.readonly != null ? VALUE_H : ROW_H);

/**
 * A lane, an import or the time with its one output, or an export with its one input: the header
 * names the signal, so the card is just the header with the circle beside it.
 */
const compact = (n: ViewNode) =>
  n.kind !== "call" && n.inputs.length + n.outputs.length === 1 && n.inputs.every((i) => !i.knob && !i.inlet && i.readonly == null);

/**
 * A node's height and where its ports are, from its top. Each input is a row; output `i` sits on
 * the right of row `i`, so a chain of first inputs and outputs draws as a straight line. Outputs
 * beyond the inputs get rows of their own.
 */
function geometry(n: ViewNode) {
  const ins = new Map<string, number>();
  const outs = new Map<string, number>();
  if (compact(n)) {
    for (const p of n.inputs) ins.set(p.id, HEAD_H / 2);
    for (const p of n.outputs) outs.set(p.id, HEAD_H / 2);
    return { w: SLIM_W, h: HEAD_H, ins, outs };
  }
  const rows: number[] = [];
  let y = HEAD_H + PAD;
  for (const i of n.inputs) {
    const h = inputHeight(i);
    ins.set(i.id, y + h / 2);
    rows.push(y + h / 2);
    y += h;
  }
  n.outputs.forEach((o, j) => {
    if (j < rows.length) return outs.set(o.id, rows[j]);
    outs.set(o.id, y + ROW_H / 2);
    y += ROW_H;
  });
  const w = n.inputs.length && n.outputs.length ? NODE_W : SLIM_W;
  return { w, h: n.outputs.length + n.inputs.length ? y + PAD : HEAD_H, ins, outs };
}

interface Placed { node: ViewNode; x: number; y: number; w: number; h: number; ins: Map<string, number>; outs: Map<string, number> }

/** Room below the nodes for the edges that go back (a `feedback()` loop), and beside the last column. */
const LOOP_ROOM = 44;
const LOOP_OUT = 22;

/**
 * The edges that close a loop, which `feedback()` written in code makes: the ones it reads one
 * block late, and any other found depth first from the sources, each edge into a node still being walked.
 */
function loops(m: ModuleView, edges: PortEdge[]): Set<PortEdge> {
  const back = new Set<PortEdge>(edges.filter((e) => e.late));
  const out = new Map<string, PortEdge[]>();
  for (const e of edges) if (!back.has(e)) (out.get(e.from) ?? out.set(e.from, []).get(e.from)!).push(e);
  const state = new Map<string, "walking" | "done">();
  const visit = (id: string) => {
    state.set(id, "walking");
    for (const e of out.get(id) ?? []) {
      if (state.get(e.to) === "walking") back.add(e);
      else if (!state.has(e.to)) visit(e.to);
    }
    state.set(id, "done");
  };
  const fed = new Set(edges.filter((e) => !back.has(e)).map((e) => e.to));
  for (const n of [...m.nodes.filter((n) => !fed.has(n.id)), ...m.nodes]) if (!state.has(n.id)) visit(n.id);
  return back;
}

/**
 * Columns by depth in the signal flow, sources left and outputs right. A source (a lane, a value)
 * sits just before what reads it. Columns are placed right to left, each node as level with what it
 * feeds as the column allows, so a chain draws as straight lines; then whatever nothing reads (an export, a signal not
 * used yet) moves level with what feeds it, into the nearest free space in its column. Edges that
 * go back round a loop are left out of that, and drawn round below everything.
 */
function layout(m: ModuleView): { placed: Map<string, Placed>; width: number; height: number; back: Set<PortEdge> } {
  const geo = new Map(m.nodes.map((n) => [n.id, geometry(n)]));
  const known = (id: string) => geo.has(id);
  const all = m.port_edges.filter((e) => known(e.from) && known(e.to));
  const back = loops(m, all);
  const links = all.filter((e) => !back.has(e));
  const into = new Map<string, Set<string>>(), from = new Map<string, Set<string>>();
  for (const e of links) {
    (from.get(e.from) ?? from.set(e.from, new Set()).get(e.from)!).add(e.to);
    (into.get(e.to) ?? into.set(e.to, new Set()).get(e.to)!).add(e.from);
  }
  // Longest path from a source: without the edges back, the graph is acyclic.
  const depth = new Map<string, number>(m.nodes.map((n) => [n.id, 0]));
  for (let pass = 0; pass < m.nodes.length; pass++) {
    let changed = false;
    for (const e of links) {
      const d = depth.get(e.from)! + 1;
      if (d > depth.get(e.to)!) {
        depth.set(e.to, d);
        changed = true;
      }
    }
    if (!changed) break;
  }
  for (const n of m.nodes) {
    const readers = [...(from.get(n.id) ?? [])];
    if (!into.get(n.id)?.size && readers.length) depth.set(n.id, Math.max(0, Math.min(...readers.map((r) => depth.get(r)!)) - 1));
  }
  const last = Math.max(0, ...depth.values());
  for (const n of m.nodes) if (n.kind === "export") depth.set(n.id, last);
  const columns: ViewNode[][] = [];
  for (const n of m.nodes) (columns[depth.get(n.id)!] ??= []).push(n);

  const mean = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);
  const top = new Map<string, number>();
  for (let c = columns.length - 1; c >= 0; c--) {
    const col = (columns[c] ?? []).map((n, i) => {
      const out = links.filter((e) => e.from === n.id && top.has(e.to));
      // Where its edges land, which orders the column without crossings, and the top that draws them level.
      const lands = out.map((e) => top.get(e.to)! + (geo.get(e.to)!.ins.get(e.to_port) ?? 0));
      const want = mean(out.map((e, k) => lands[k] - (geo.get(n.id)!.outs.get(e.from_port) ?? 0)));
      return { n, i, land: mean(lands), want, h: geo.get(n.id)!.h };
    });
    col.sort((a, b) => (a.land ?? Infinity) - (b.land ?? Infinity) || a.i - b.i);
    const wanted = col.filter((x) => x.want != null);
    const tops = pack(wanted.map((x) => ({ want: x.want!, h: x.h })));
    wanted.forEach((x, k) => top.set(x.n.id, tops[k]));
    // What nothing to the right reads goes below, for now (see below).
    let next = wanted.length ? tops[tops.length - 1] + wanted[wanted.length - 1].h + GAP_Y : 0;
    for (const x of col.filter((x) => x.want == null)) {
      top.set(x.n.id, next);
      next += x.h + GAP_Y;
    }
  }
  for (const col of columns) {
    for (const n of col ?? []) {
      if (from.get(n.id)?.size) continue;
      const at = links
        .filter((e) => e.to === n.id)
        .map((e) => top.get(e.from)! + (geo.get(e.from)!.outs.get(e.from_port) ?? 0) - (geo.get(n.id)!.ins.get(e.to_port) ?? 0));
      if (!at.length) continue;
      const others = col.filter((o) => o !== n).map((o) => [top.get(o.id)! - GAP_Y, top.get(o.id)! + geo.get(o.id)!.h + GAP_Y]);
      const h = geo.get(n.id)!.h;
      const free = (y: number) => others.every(([a, b]) => y + h <= a || y >= b);
      // Candidates: where it wants to be, or just above or below each neighbour.
      const want = at.reduce((a, b) => a + b, 0) / at.length;
      const spots = [want, ...others.flatMap(([a, b]) => [a - h, b])].filter(free);
      top.set(n.id, spots.reduce((best, y) => (Math.abs(y - want) < Math.abs(best - want) ? y : best)));
    }
  }
  const shift = top.size ? -Math.min(...top.values()) : 0;
  // Columns as wide as their widest node; slim sources sit at the right, by what they feed.
  const widths = columns.map((col) => Math.max(SLIM_W, ...(col ?? []).map((n) => geo.get(n.id)!.w)));
  const xs = widths.map((_, c) => widths.slice(0, c).reduce((a, w) => a + w + GAP_X, 0));
  const placed = new Map<string, Placed>();
  let height = 0;
  for (const n of m.nodes) {
    const g = geo.get(n.id)!;
    const c = depth.get(n.id)!;
    const y = top.get(n.id)! + shift;
    const x = xs[c] + (n.inputs.length ? 0 : widths[c] - g.w);
    placed.set(n.id, { node: n, x, y, ...g });
    height = Math.max(height, y + g.h);
  }
  const width = Math.max(0, ...[...placed.values()].map((p) => p.x + p.w));
  return back.size ? { placed, width: width + LOOP_OUT + 4, height: height + LOOP_ROOM, back } : { placed, width, height, back };
}

/**
 * Tops for a column's nodes, in order and `GAP_Y` apart, as close to the tops they want as they
 * fit (least squares): nodes that would overlap move apart around their middle rather than all
 * pushing down, so fan-ins centre on what they feed and the graph stays compact.
 */
function pack(items: { want: number; h: number }[]): number[] {
  // Runs of nodes that touch: their first, how many, the sum of (want − offset in the run), the run's height.
  const runs: { first: number; count: number; sum: number; span: number; y: number }[] = [];
  items.forEach((it, i) => {
    let run = { first: i, count: 1, sum: it.want, span: it.h + GAP_Y, y: it.want };
    for (let prev = runs.at(-1); prev && prev.y + prev.span > run.y; prev = runs.at(-1)) {
      runs.pop();
      const sum = prev.sum + run.sum - run.count * prev.span;
      const count = prev.count + run.count;
      run = { first: prev.first, count, sum, span: prev.span + run.span, y: sum / count };
    }
    runs.push(run);
  });
  const tops: number[] = [];
  for (const run of runs) {
    let y = run.y;
    for (let k = run.first; k < run.first + run.count; k++) {
      tops.push(y);
      y += items[k].h + GAP_Y;
    }
  }
  return tops;
}

/** A smooth curve from an output (left) to an input (right), bowing out when it runs backwards. */
function curve(x0: number, y0: number, x1: number, y1: number) {
  const dx = Math.max(48, Math.abs(x1 - x0) * 0.45);
  return `M ${x0} ${y0} C ${x0 + dx} ${y0}, ${x1 - dx} ${y1}, ${x1} ${y1}`;
}

/** An edge back round a loop: out to the right, down below the nodes to `yb`, back left and up into the input. */
function loopBack(x0: number, y0: number, x1: number, y1: number, yb: number) {
  const r = 12, a = x0 + LOOP_OUT, b = x1 - LOOP_OUT;
  return [
    `M ${x0} ${y0} H ${a - r} Q ${a} ${y0} ${a} ${y0 + r} V ${yb - r} Q ${a} ${yb} ${a - r} ${yb}`,
    `H ${b + r} Q ${b} ${yb} ${b} ${yb - r} V ${y1 + r} Q ${b} ${y1} ${b + r} ${y1} H ${x1}`,
  ].join(" ");
}

// ---------------------------------------------------------------- the module

/** A cable being dragged from an output, in the module's coordinates; `to` is the knob it would land on. */
interface Cable { from: string; name: string; kind: PortKind; x0: number; y0: number; x: number; y: number; to: string | null }

/** Memoized: the playback status re-renders the canvas five times a second. */
const ModuleSection = memo(function ModuleSection({
  module,
  laid: { placed, width, height, back },
  zoom,
  session,
  revision,
  editable,
  effects,
}: {
  module: ModuleView;
  laid: ReturnType<typeof layout>;
  /** The nodes are laid out at 1 and drawn scaled by this. */
  zoom: number;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
}) {
  const toast = useStore((s) => s.toast);
  const area = useRef<HTMLDivElement>(null);
  const [cable, setCable] = useState<Cable | null>(null);
  // Ports with an edge, to draw them filled.
  const wired = useMemo(() => {
    const s = new Set<string>();
    for (const e of module.port_edges) {
      s.add(`${e.from}>${e.from_port}`);
      s.add(`${e.to}<${e.to_port}`);
    }
    return s;
  }, [module]);
  const kinds = useMemo(() => {
    const s = new Set<PortKind>();
    for (const n of module.nodes) for (const p of [...n.inputs, ...n.outputs]) s.add(p.kind);
    return (["audio", "control", "notes"] as const).filter((k) => s.has(k));
  }, [module]);

  // Drag from a named output onto a knob: the knob's value is replaced by the signal's name.
  // The cable is drawn in the module's own (unzoomed) coordinates.
  const scale = useRef(zoom);
  scale.current = zoom;
  const startCable = useCallback(
    (node: ViewNode, output: OutputPort, e: React.PointerEvent) => {
      const box = area.current?.getBoundingClientRect();
      const p = placed.get(node.id);
      if (!box || !p || !node.outlet || e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      const target = (ev: PointerEvent | React.PointerEvent) => {
        const el = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>("[data-knob-id]");
        return el && area.current?.contains(el) ? el : null;
      };
      // The cable's end: the pointer, or the input it would land on.
      const end = (ev: PointerEvent | React.PointerEvent) => {
        const b = area.current!.getBoundingClientRect();
        const knob = target(ev);
        const dot = knob?.querySelector("[data-port-dot]")?.getBoundingClientRect();
        const z = scale.current;
        if (knob && dot) return { x: (dot.left + dot.width / 2 - b.left) / z, y: (dot.top + dot.height / 2 - b.top) / z, to: knob.dataset.knobId! };
        return { x: (ev.clientX - b.left) / z, y: (ev.clientY - b.top) / z, to: null };
      };
      const gesture = { session, revision, from: node.id, name: node.outlet };
      setCable({ from: node.id, name: node.outlet, kind: output.kind, x0: p.x + p.w, y0: p.y + (p.outs.get(output.id) ?? ROW_H), ...end(e) });
      const move = (ev: PointerEvent) => setCable((c) => c && { ...c, ...end(ev) });
      const up = (ev: PointerEvent) => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", up);
        setCable(null);
        if (ev.type !== "pointerup") return;
        const knob = target(ev);
        if (!knob) return;
        graphApi
          .connect(gesture.session, gesture.revision, gesture.from, knob.dataset.knobId!)
          .then((c) => toast(`${c.file}: ${c.before} → ${c.after}`))
          .catch((error) => toast(error instanceof Error ? error.message : String(error)));
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [placed, session, revision, toast],
  );

  return (
    <section className="grow shrink-0 w-max min-w-full flex flex-col rounded-2xl bg-ink-900 ring-1 ring-black/[0.05] p-5 pt-4" data-module={module.key}>
      <div className="flex items-center gap-2 mb-5">
        <h2 className="text-sm font-semibold text-ink-100">{moduleName(module)}</h2>
        <span className="text-xs text-ink-400">
          {module.track ? "track" : "mix"}
          {/* The key the code calls it by, when the label doesn't say it. */}
          {keyNote(moduleName(module), module.key) && <span className="font-mono text-[11px] text-ink-500"> · {module.key}</span>}
        </span>
        <div className="ml-4 pl-4 border-l border-black/[0.08] flex items-center gap-4 text-[11px] text-ink-400" data-legend>
          {kinds.map((k) => (
            <span key={k} className="inline-flex items-center gap-1.5" data-legend-kind={k}>
              <span className="size-2.5 rounded-full" style={{ background: KIND_COLOR[k] }} />
              {KIND_LABEL[k]}
            </span>
          ))}
          {editable && module.nodes.some((n) => n.outlet) && <span className="pl-3 border-l border-black/[0.08]">Drag a named output onto a knob to connect it</span>}
        </div>
      </div>
      {/* The canvas fills the card, dotted all over; the graph sits in its middle. */}
      <div
        data-canvas
        className="grow grid place-items-center rounded-xl"
        style={{
          minWidth: width * zoom,
          minHeight: height * zoom,
          backgroundImage: "radial-gradient(circle, rgb(0 0 0 / 0.07) 1px, transparent 1.2px)",
          backgroundSize: "20px 20px",
          backgroundPosition: "-10px -10px",
        }}
      >
        <div style={{ width: width * zoom, height: height * zoom }}>
          <div
            ref={area}
            className="relative origin-top-left"
            style={{ width, height, transform: zoom === 1 ? undefined : `scale(${zoom})` }}
            data-cabling={cable ? true : undefined}
          >
            <svg className="absolute inset-0 pointer-events-none overflow-visible" width={width} height={height}>
              {module.port_edges.map((e) => {
                const p = placed.get(e.from), q = placed.get(e.to);
                const y0 = p?.outs.get(e.from_port), y1 = q?.ins.get(e.to_port);
                if (!p || !q || y0 == null || y1 == null) return null;
                const loop = back.has(e);
                return (
                  <path
                    key={`${e.from}>${e.from_port}>${e.to}>${e.to_port}`}
                    d={loop ? loopBack(p.x + p.w, p.y + y0, q.x, q.y + y1, height - LOOP_ROOM / 2) : curve(p.x + p.w, p.y + y0, q.x, q.y + y1)}
                    data-loop={loop || undefined}
                    strokeDasharray={loop ? "6 5" : undefined}
                    data-from={e.from}
                    data-from-port={e.from_port}
                    data-to={e.to}
                    data-to-port={e.to_port}
                    data-kind={e.kind}
                    stroke={KIND_COLOR[e.kind]}
                    strokeOpacity="0.7"
                    strokeWidth="2"
                    strokeLinecap="round"
                    fill="none"
                  />
                );
              })}
            </svg>
            {[...placed.values()].map((p) => (
              <NodeCard
                key={p.node.id}
                node={p.node}
                session={session}
                revision={revision}
                editable={editable}
                effects={effects}
                wired={wired}
                ports={p.outs}
                onCable={startCable}
                style={{ left: p.x, top: p.y, width: p.w, height: p.h }}
              />
            ))}
            <svg className="absolute inset-0 pointer-events-none overflow-visible z-10" width={width} height={height}>
              {cable && (
                <path
                  data-cable={cable.to ?? ""}
                  d={curve(cable.x0, cable.y0, cable.x, cable.y)}
                  stroke={KIND_COLOR[cable.kind]}
                  strokeWidth="2.5"
                  strokeDasharray={cable.to ? undefined : "6 5"}
                  strokeLinecap="round"
                  fill="none"
                />
              )}
            </svg>
          </div>
        </div>
      </div>
    </section>
  );
});

// ---------------------------------------------------------------- nodes

/** The icon for what a node is. */
function NodeIcon({ node }: { node: ViewNode }) {
  const kind = node.outputs[0]?.kind ?? node.inputs[0]?.kind ?? "control";
  const Icon =
    node.kind === "voice"
      ? Piano
      : node.kind === "lane"
      ? kind === "notes" ? Music2 : kind === "audio" ? AudioLines : Spline
      : node.kind === "time"
        ? Clock
        : node.kind === "import"
          ? LogIn
          : node.kind === "export"
            ? LogOut
            : node.kind === "value"
              ? Hash
              : node.effect
                ? SlidersHorizontal
                : kind === "audio"
                  ? AudioWaveform
                  : kind === "control"
                    ? Activity
                    : SquareFunction;
  const color = KIND_COLOR[kind];
  return (
    <span className="size-6 shrink-0 rounded-[7px] grid place-items-center" style={{ background: `color-mix(in oklab, ${color} 14%, var(--color-ink-800))`, color }}>
      <Icon className="size-3.5" />
    </span>
  );
}

/** An input: a small circle on a node's left edge, filled when something is connected to it. */
function Dot({ id, kind, filled, className }: { id: string; kind: PortKind; filled: boolean; className?: string }) {
  const color = KIND_COLOR[kind];
  return (
    <span
      data-port-dot
      data-port="in"
      data-port-id={id}
      data-kind={kind}
      className={clsx("absolute left-[-6px] top-1/2 -mt-[6px] size-3 rounded-full transition-transform", className)}
      style={{ background: filled ? color : "var(--color-ink-800)", boxShadow: `inset 0 0 0 2px ${color}, 0 0 0 2px var(--color-ink-800)` }}
    />
  );
}

const SUBTITLE: Partial<Record<ViewNode["kind"], string>> = { lane: "lane", time: "time", import: "from track", export: "export", value: "value", voice: "each note played" };

function NodeCard({
  node,
  session,
  revision,
  editable,
  effects,
  wired,
  ports,
  onCable,
  style,
}: {
  node: ViewNode;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  wired: Set<string>;
  /** Where its outputs are, from its top. */
  ports: Map<string, number>;
  onCable: (node: ViewNode, output: OutputPort, e: React.PointerEvent) => void;
  style: React.CSSProperties;
}) {
  const toast = useStore((s) => s.toast);
  const [picking, setPicking] = useState<HTMLElement | null>(null);
  const closePicker = useCallback(() => setPicking(null), []);
  const edited = (p: Promise<{ file: string; before: string; after: string }>) =>
    p.then((c) => toast(`${c.file}: ${c.before} → ${c.after}`)).catch((error) => toast(error instanceof Error ? error.message : String(error)));
  const subtitle = SUBTITLE[node.kind];
  // Just the header, its one circle beside it (see `compact`).
  const small = compact(node);
  const where = node.file ? `${node.file.split("/").pop()}:${node.line}` : null;
  return (
    <div
      className={clsx(
        "absolute rounded-xl bg-ink-800 ring-1 shadow-[0_1px_2px_rgb(0_0_0/0.04),0_6px_16px_-8px_rgb(0_0_0/0.12)] flex flex-col",
        node.effect ? "ring-pop-500/25" : "ring-black/[0.08]",
      )}
      style={style}
      data-node={node.label}
      data-node-id={node.id}
      title={node.file ? `${node.file}:${node.line}` : undefined}
    >
      <div className={clsx("h-[42px] shrink-0 pl-2.5 pr-1.5 flex items-center gap-2 min-w-0", !small && "border-b border-black/[0.05]")}>
        <NodeIcon node={node} />
        <div className="min-w-0 flex-1 leading-tight">
          <div className={clsx("font-mono text-[13px] truncate", node.kind === "call" || node.kind === "lane" || node.kind === "voice" ? "text-ink-100 font-medium" : "text-ink-200")}>{node.label}</div>
          {(subtitle || node.effect || where) && (
            <div className="text-[10px] text-ink-400 truncate">
              {node.effect ? "effect" : subtitle}
              {(node.effect || subtitle) && where ? " · " : ""}
              {where && <span className="font-mono">{where}</span>}
            </div>
          )}
        </div>
        {node.effect && (
          <button
            className="shrink-0 size-7 grid place-items-center rounded-lg text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none cursor-pointer"
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
              "shrink-0 size-7 grid place-items-center rounded-lg text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none cursor-pointer",
              picking && "bg-black/5 text-ink-100",
            )}
            title="Add an effect after this"
            aria-label={`Add an effect after ${node.label}`}
            aria-haspopup="dialog"
            aria-expanded={!!picking}
            disabled={!editable}
            onClick={(e) => {
              const button = e.currentTarget;
              setPicking((p) => (p ? null : button));
            }}
          >
            <Plus className="size-4" />
          </button>
        )}
      </div>
      {/* In the module, outside its zoomed nodes: it keeps its size and its place by the +. */}
      {picking &&
        createPortal(
          <EffectBrowser
            effects={effects}
            after={node.label}
            anchor={picking}
            onClose={closePicker}
            onPick={(effect) => {
              setPicking(null);
              void edited(graphApi.addEffect(session, revision, node.id, effect));
            }}
          />,
          picking.closest("section[data-module]") ?? document.body,
        )}
      {small && node.inputs.map((i) => <Dot key={i.id} id={i.id} kind={i.kind} filled={wired.has(`${node.id}<${i.id}`)} />)}
      {!small && (node.outputs.length > 0 || node.inputs.length > 0) && (
        <div className="py-[6px]">
          {node.inputs.map((i) => (
            <InputRow key={i.id} node={node} input={i} filled={wired.has(`${node.id}<${i.id}`)} session={session} revision={revision} editable={editable} />
          ))}
          {node.outputs.slice(node.inputs.length).map((o) => (
            <div key={o.id} className="h-[28px]" />
          ))}
        </div>
      )}
      {node.outputs.map((o, j) => (
        <OutputPortView
          key={o.id}
          node={node}
          output={o}
          y={ports.get(o.id) ?? 0}
          // Only a signal's row (or an output's own) has room for its label beside it.
          labelled={!small && (j >= node.inputs.length || (!node.inputs[j].knob && !node.inputs[j].inlet && node.inputs[j].readonly == null))}
          filled={wired.has(`${node.id}>${o.id}`)}
          editable={editable}
          onCable={onCable}
        />
      ))}
    </div>
  );
}

/** An output on the node's right edge, at height `y`: a cable starts at a named one. */
function OutputPortView({
  node,
  output,
  y,
  labelled,
  filled,
  editable,
  onCable,
}: {
  node: ViewNode;
  output: OutputPort;
  y: number;
  labelled: boolean;
  filled: boolean;
  editable: boolean;
  onCable: (node: ViewNode, output: OutputPort, e: React.PointerEvent) => void;
}) {
  const toast = useStore((s) => s.toast);
  const cable = output.outlet && node.outlet;
  // Only a named signal can be connected: say how to name this one.
  const last = node.label.split(".").pop()!;
  const unnamed =
    node.kind === "call"
      ? `To connect ${node.label} to a knob, name it in code first: const my${last[0].toUpperCase()}${last.slice(1)} = ${node.label}(…)`
      : `To connect ${node.label} to a knob, name it with a const in code first.`;
  return (
    <div className="absolute right-0 h-[28px] -mt-[14px] max-w-[55%] pr-4 flex items-center justify-end text-[11px] pointer-events-none" style={{ top: y }} data-output={output.label}>
      {labelled && <span className={clsx("truncate", cable ? "font-mono text-ink-200" : "text-ink-400")}>{output.label}</span>}
      {cable ? (
        <button
          className={clsx(
            "absolute -right-[9px] top-1/2 -mt-[9px] size-[18px] grid place-items-center rounded-full group/outlet",
            editable ? "cursor-crosshair pointer-events-auto" : "pointer-events-none",
          )}
          data-outlet={node.outlet!}
          data-port="out"
          data-port-id={output.id}
          data-kind={output.kind}
          aria-label={`Connect ${node.outlet}`}
          title={editable ? `Drag onto a knob to connect ${node.outlet}` : "Fix the code errors before connecting."}
          disabled={!editable}
          onPointerDown={(e) => onCable(node, output, e)}
        >
          <span
            className="size-3 rounded-full transition-transform group-hover/outlet:scale-125"
            style={{ background: KIND_COLOR[output.kind], boxShadow: `0 0 0 2px var(--color-ink-800), 0 0 0 4px color-mix(in oklab, ${KIND_COLOR[output.kind]} 25%, transparent)` }}
          />
        </button>
      ) : (
        <span
          className="absolute -right-[9px] top-1/2 -mt-[9px] size-[18px] grid place-items-center cursor-help pointer-events-auto"
          data-port="out"
          data-port-id={output.id}
          data-kind={output.kind}
          title={unnamed}
          onPointerDown={(e) => e.button === 0 && output.kind !== "notes" && toast(unnamed)}
        >
          <span className="size-3 rounded-full" style={{ background: filled ? KIND_COLOR[output.kind] : "var(--color-ink-800)", boxShadow: `inset 0 0 0 2px ${KIND_COLOR[output.kind]}, 0 0 0 2px var(--color-ink-800)` }} />
        </span>
      )}
    </div>
  );
}

function InputRow({
  node,
  input,
  filled,
  session,
  revision,
  editable,
}: {
  node: ViewNode;
  input: InputPort;
  filled: boolean;
  session: number;
  revision: number;
  editable: boolean;
}) {
  const toast = useStore((s) => s.toast);
  const knob = input.knob ? node.knobs.find((k) => k.id === input.knob) : undefined;
  if (knob) return <KnobControl key={`${editable}:${knob.id}`} knob={knob} port={input.id} label={input.label} session={session} revision={revision} editable={editable} />;

  const inlet = input.inlet ? node.inlets.find((i) => i.id === input.inlet) : undefined;
  if (inlet) {
    const disconnect = () =>
      graphApi
        .disconnect(session, revision, inlet.id)
        .then((c) => toast(`${c.file}: ${c.before} → ${c.after}`))
        .catch((error) => toast(error instanceof Error ? error.message : String(error)));
    return (
      <div className="relative h-[28px] pl-4 pr-3.5 flex items-center gap-1.5 text-xs min-w-0" data-inlet={inlet.label}>
        <Dot id={input.id} kind={input.kind} filled />
        <span className="text-ink-400 truncate">{inlet.label}</span>
        <span className="text-ink-500">←</span>
        <span className="font-mono text-ink-200 truncate">{inlet.name}</span>
        <button
          className="ml-auto shrink-0 size-5 grid place-items-center rounded text-ink-400 hover:text-ink-100 hover:bg-black/5 disabled:opacity-40 disabled:pointer-events-none cursor-pointer"
          title={`Disconnect ${inlet.name} (writes the input's default)`}
          aria-label={`Disconnect ${inlet.name} from ${inlet.label}`}
          disabled={!editable}
          onClick={() => void disconnect()}
        >
          <X className="size-3" />
        </button>
      </div>
    );
  }

  const source = input.readonly != null ? node.readonly[input.readonly] : undefined;
  if (source) {
    return (
      <div
        className="relative h-[48px] pl-4 pr-3 py-1.5 min-w-0 flex flex-col justify-center"
        data-expression={source.label}
        title={`${source.reason}${source.file ? `\n${source.file}:${source.line}` : ""}\n${source.expression}`}
      >
        <Dot id={input.id} kind={input.kind} filled={false} />
        <div className="flex items-center justify-between gap-2 text-[11px]">
          <span className="text-ink-300 truncate">{source.label}</span>
          <span className="shrink-0 text-[10px] text-ink-400">edit in code</span>
        </div>
        <div className="font-mono text-[11px] text-ink-200 truncate">{source.expression}</div>
      </div>
    );
  }

  return (
    <div className="relative h-[28px] pl-4 pr-3 flex items-center text-xs min-w-0" data-input={input.label}>
      <Dot id={input.id} kind={input.kind} filled={filled} />
      <span className="text-ink-300 truncate">{input.label}</span>
    </div>
  );
}

// ---------------------------------------------------------------- knobs

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

/** A knob over an owned source value, as one input row: drag to hear it, release to write it. */
function KnobControl({ knob, port, label, session, revision, editable }: { knob: KnobInfo; port: string; label: string; session: number; revision: number; editable: boolean }) {
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
      className={clsx(
        "group/knob relative h-[40px] pl-4 pr-4 flex items-center gap-2 min-w-0 transition-colors",
        "[[data-cabling]_&[data-knob-id]]:bg-pop-500/[0.06] [[data-cabling]_&[data-knob-id]:hover]:bg-pop-500/[0.14]",
      )}
    >
      <Dot
        id={port}
        kind="control"
        filled={false}
        className="[[data-cabling]_[data-knob-id]>&]:scale-[1.35] [[data-cabling]_[data-knob-id]>&]:![box-shadow:inset_0_0_0_2px_var(--color-pop-500),0_0_0_3px_color-mix(in_oklab,var(--color-pop-500)_30%,transparent)]"
      />
      <div className="flex-1 min-w-0 leading-tight">
        <div className="text-xs text-ink-300 truncate">{label}</div>
        {knob.uses > 1 && <div className="text-[9px] text-pop-500">shared ×{knob.uses}</div>}
      </div>
      <span className="shrink-0 text-[11px] text-ink-400 tabular-nums">{display(descriptor, value)}</span>
      <div inert={disabled} className={clsx("shrink-0", disabled && "opacity-45 pointer-events-none")}>
        <MiniKnob
          value={Math.min(1, Math.max(0, toUnit(descriptor, value)))}
          label={label}
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
      </div>
    </div>
  );
}

/** A small rotary knob, 0..1: drag up and down (160px is the whole range), double-click for the middle. */
function MiniKnob({
  value,
  label,
  onChange,
  onStart,
  onEnd,
  onCancel,
}: { value: number; label: string; onChange: (v: number) => void; onStart: () => void; onEnd: () => void; onCancel: () => void }) {
  const drag = useRef<{ pointer: number; y: number; v: number } | null>(null);
  const finish = (pointer: number, cancelled: boolean) => {
    if (drag.current?.pointer !== pointer) return;
    drag.current = null;
    if (cancelled) onCancel();
    else onEnd();
  };
  const a0 = -135, a1 = 135;
  const angle = a0 + (a1 - a0) * value;
  const r = 11, c = 15;
  const at = (a: number, radius = r) => [c + radius * Math.sin((a * Math.PI) / 180), c - radius * Math.cos((a * Math.PI) / 180)];
  const arc = (from: number, to: number) => {
    const [x0, y0] = at(from), [x1, y1] = at(to);
    return `M ${x0} ${y0} A ${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
  };
  const [tx, ty] = at(angle, 5.5);
  return (
    <svg
      width="30"
      height="30"
      role="slider"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={Number(value.toFixed(3))}
      className="block cursor-ns-resize touch-none"
      onPointerDown={(e) => {
        if (drag.current || e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { pointer: e.pointerId, y: e.clientY, v: value };
        onStart();
      }}
      onPointerMove={(e) => {
        if (!drag.current || drag.current.pointer !== e.pointerId) return;
        const v = Math.min(1, Math.max(0, drag.current.v + (drag.current.y - e.clientY) / 160));
        if (Math.abs(v - value) > 0.002) onChange(v);
      }}
      onPointerUp={(e) => finish(e.pointerId, false)}
      onPointerCancel={(e) => finish(e.pointerId, true)}
      onLostPointerCapture={(e) => finish(e.pointerId, true)}
      onDoubleClick={() => onChange(0.5)}
    >
      <path d={arc(a0, a1)} stroke="var(--color-ink-600)" strokeWidth="3" fill="none" strokeLinecap="round" />
      {value > 0.005 && <path d={arc(a0, angle)} stroke={KIND_COLOR.control} strokeWidth="3" fill="none" strokeLinecap="round" />}
      <circle cx={c} cy={c} r="6.5" fill="var(--color-ink-700)" />
      <line x1={c} y1={c} x2={tx} y2={ty} stroke="var(--color-ink-200)" strokeWidth="1.75" strokeLinecap="round" />
    </svg>
  );
}

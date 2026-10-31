import clsx from "clsx";
import {
  ChevronRight,
  ArrowLeft,
  AppWindow,
  ArrowRight,
  AudioLines,
  Check,
  ChevronDown,
  Clock,
  Ellipsis,
  Hash,
  LogIn,
  LogOut,
  Music2,
  Piano,
  Plus,
  Power,
  Scan,
  Spline,
  Trash2,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { createContext, memo, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  controlLabel,
  display,
  findLane,
  fromUnit,
  graphApi,
  laneTitle,
  lockReason,
  moduleName,
  parseValue,
  picks,
  span,
  stepValue,
  toUnit,
  type CardGroup,
  type CardView,
  type Effect,
  type GraphLease,
  type GraphState,
  type InputPort,
  type NumberKnob as KnobInfo,
  type PickKnob,
  type MixView,
  type ModuleView,
  type OutputPort,
  type PortEdge,
  type PortKind,
  type Setting,
  type ViewNode,
} from "../lib/graph";
import { listenTo, moveEffect, turnEffect } from "../lib/effects";
import { laneMade } from "../lib/events";
import { mod, opt } from "../lib/native";
import { useGlows, useNotice } from "../lib/notice";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { Dots } from "./draw";
import EffectBrowser from "./EffectBrowser";
import { focusNext, HomeMarker, KnobMenu, MiniKnob, useKnobDrag, useKnobGesture } from "./Knob";
import { MenuRow, Popover } from "./ui";
import { undoAction } from "./GraphCanvas";
import PointMenu, { unautomate } from "./ClipMenu";
import PresetPopover from "./PresetPopover";
import { MenuValue, SettingRow, SwitchValue, settingHeight, shown, words } from "./Settings";
import { VIEW_H, ViewRow } from "./CardViews";
import { useFolded } from "../lib/folded";

/** Why nothing here can be edited right now (the code doesn't compile, read only, offline), or null. */
const Locked = createContext<string | null>(null);
/** A track's name by its key, for the controls that name one (a mixer channel's `hats.db`: Hats volume). */
const Tracks = createContext<(key: string) => string | undefined>(() => undefined);
/** The Song's mixer, drawn as one compact card (MX-6): its node and how many channels it has. */
const MixerCard = createContext<{ id: string; channels: number } | null>(null);

/** The merged input of the compact mixer card: every channel's input, as one port. */
const CHANNELS = "channels";

/**
 * The Song's own mixer as one compact card (taste.md §3.5, MX-6), "Mixer · 4 channels · Open Mix
 * ⌥3", never a row per channel: its channels' inputs drawn as one port, and no knobs, since its
 * levels and pans are the Mix view's strips. Only the mixer the Mix view shows: a track's own mixer
 * stays a box like any other. Null when `m` has no such mixer.
 */
function compactMixer(m: ModuleView, mix: MixView): { module: ModuleView; card: { id: string; channels: number } } | null {
  if (m.key !== "song" || mix.mixer !== "ok") return null;
  const mixers = m.nodes.filter((n) => n.component === "std:mixer");
  if (mixers.length !== 1) return null;
  const node = mixers[0];
  const channels = new Set(mix.strips.flatMap((s) => (s.channel ? [s.channel] : []))).size;
  const card: ViewNode = { ...node, knobs: [], readonly: [], inlets: [], settings: [], inputs: [{ id: CHANNELS, label: "Channels", kind: "audio" }] };
  // Each signal drawn once into the card, whichever channels it feeds.
  const seen = new Set<string>();
  const port_edges = m.port_edges.flatMap((e) => {
    if (e.to !== node.id) return [e];
    const from = `${e.from}>${e.from_port}`;
    if (seen.has(from)) return [];
    seen.add(from);
    return [{ ...e, to_port: CHANNELS }];
  });
  return { module: { ...m, nodes: m.nodes.map((n) => (n === node ? card : n)), port_edges }, card: { id: node.id, channels } };
}

/** One module's code as a graph to play and tweak: knobs over its values, cables between its signals, effects after its audio. */
export function NodeGraph({ graph, module }: { graph: GraphState; module: string }) {
  const found = graph.view.modules.find((m) => m.key === module);
  // The Song's mixer is one compact card; its levels and pans are the Mix view's (MX-6).
  const compacted = useMemo(() => (found ? compactMixer(found, graph.mix) : null), [found, graph.mix]);
  const shown = compacted?.module ?? found;
  const folded = useFolded((s) => s.ids);
  const laid = useMemo(() => shown && layout(shown, folded), [shown, folded]);
  const locked = useStore((s) => lockReason(graph, s.connected));
  const tracks = useMemo(() => {
    const by = new Map(graph.view.modules.filter((m) => m.track).map((m) => [m.key, moduleName(m)]));
    return (key: string) => by.get(key);
  }, [graph.view.modules]);

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
  const fit = laid && room ? fitZoom(laid, room) : 1;
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
    <Locked.Provider value={locked}>
      <Tracks.Provider value={tracks}>
      <MixerCard.Provider value={compacted?.card ?? null}>
      <div ref={scroller} className="flex-1 min-h-0 overflow-auto p-4 flex flex-col" data-testid="graph">
        {shown && laid && (
          <ModuleSection
            key={`${graph.session}:${shown.key}`}
            module={shown}
            laid={laid}
            zoom={zoom}
            session={graph.session}
            revision={graph.view.revision}
            editable={!locked}
            effects={graph.effects}
            leases={graph.leases}
          />
        )}
      </div>
      {shown && <ZoomBar zoom={zoom} fitted={zooms[module] == null} onZoom={setZoom} />}
      </MixerCard.Provider>
      </Tracks.Provider>
    </Locked.Provider>
  );
}

/** Zooms the graph can take; fitting it to the panel never shrinks it below `MIN_FIT` (it scrolls instead). */
const clampZoom = (z: number, lo = 0.3, hi = 2) => Math.min(hi, Math.max(lo, Number.isFinite(z) ? z : 1));
/** Fit's floor (taste.md DV-5): below it a card's 12px text would draw under the 11px the type scale allows. */
const MIN_FIT = 0.75;
/** The room around a module's nodes in the scroller: its padding (16) and the section's (16), each side. */
const CHROME_W = 64;
const CHROME_H = 64;

/**
 * The zoom Fit picks (taste.md §3.5, DV-5): a sound that fits the panel's height at full size (a
 * chain, one row of cards) opens at 100% and scrolls sideways, the way signal reads; one that
 * doesn't is shrunk to fit, down to 75%, and scrolls past that.
 */
function fitZoom(laid: { width: number; height: number }, room: { w: number; h: number }) {
  const tall = (room.h - CHROME_H) / laid.height;
  if (tall >= 1) return 1;
  return clampZoom(Math.min((room.w - CHROME_W) / laid.width, tall), MIN_FIT, 1);
}

/** Zoom out, the zoom (click: actual size), zoom in, fit: floating at the panel's bottom right. */
function ZoomBar({ zoom, fitted, onZoom }: { zoom: number; fitted: boolean; onZoom: (z: number | null) => void }) {
  const button = "h-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear";
  return (
    // It floats over the canvas: the one thing here with a shadow.
    <div className="absolute right-8 bottom-8 z-20 flex items-center gap-0.5 p-0.5 rounded-md bg-raised shadow-pop text-xs" data-zoom={zoom.toFixed(2)}>
      <button className={clsx(button, "w-ctl-sm")} onClick={() => onZoom(zoom / 1.2)} title={`Zoom out (${mod.cmd} + scroll)`} aria-label="Zoom out">
        <ZoomOut size={14} />
      </button>
      <button className={clsx(button, "w-11 font-medium")} onClick={() => onZoom(1)} title="Actual size" aria-label="Actual size">
        {Math.round(zoom * 100)}%
      </button>
      <button className={clsx(button, "w-ctl-sm")} onClick={() => onZoom(zoom * 1.2)} title={`Zoom in (${mod.cmd} + scroll)`} aria-label="Zoom in">
        <ZoomIn size={14} />
      </button>
      <span className="w-px h-4 mx-0.5 bg-line" />
      <button
        className={clsx(button, "px-2 gap-1 inline-flex items-center", fitted && "!bg-accent-soft !text-accent-strong")}
        onClick={() => onZoom(null)}
        title="Fit the graph to the panel"
        aria-label="Fit to panel"
        aria-pressed={fitted}
      >
        <Scan size={14} />
        Fit
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- ports and layout

/**
 * What flows through a port, by shape and line, never colour (taste.md §2.8): audio a circle, control
 * a diamond, notes a rounded square, all 8px, filled when connected; cables as heavy, light or dashed.
 */
const SHAPE: Record<PortKind, string> = { audio: "size-2 rounded-full", control: "size-1.5 rotate-45", notes: "size-2 rounded-xs" };
/** Where a port's mark sits so that its centre is on the node's edge: half its size out. */
const SHAPE_AT: Record<PortKind, string> = { audio: "-mt-[4px]", control: "-mt-[3px]", notes: "-mt-[4px]" };
const CABLE: Record<PortKind, { stroke: string; width: number; dash?: string }> = {
  audio: { stroke: "var(--color-fg-muted)", width: 2 },
  control: { stroke: "var(--color-fg-faint)", width: 1.5 },
  notes: { stroke: "var(--color-fg-faint)", width: 1.5, dash: "4 3" },
};
/** A port's mark, filled when something is connected to it, else hollow. */
const mark = (kind: PortKind, filled: boolean) =>
  clsx(
    SHAPE[kind],
    filled
      ? "bg-fg-muted shadow-[0_0_0_2px_var(--color-raised)]"
      : "bg-raised shadow-[inset_0_0_0_1.5px_var(--color-fg-muted),0_0_0_2px_var(--color-raised)]",
  );

const NODE_W = 216;
/** A node with only outputs (a lane, an import) or only inputs (an export). */
const SLIM_W = 152;
const GAP_X = 60;
const GAP_Y = 18;
const HEAD_H = 40;
/** Above and below a node's rows. */
const PAD = 6;
/** An output, a signal coming in, or a cable. */
const ROW_H = 28;
const KNOB_H = 40;
const VALUE_H = 48;

const inputHeight = (p: InputPort) => (p.knob ? KNOB_H : p.readonly != null ? VALUE_H : ROW_H);
/** The settings a card shows (CARD-7), under its knobs, in its descriptor's order. */
const settingsOf = (n: ViewNode) => (n.settings ?? []).filter(shown);
/** An instrument's line under its name: how it plays its notes. */
const VOICES_H = 24;

/**
 * A lane, an import or the time with its one output, or an export with its one input: the header
 * names the signal, so the card is just the header with the circle beside it.
 */
const compact = (n: ViewNode) =>
  n.kind !== "call" && n.inputs.length + n.outputs.length === 1 && n.inputs.every((i) => !i.knob && !i.inlet && i.readonly == null);

/** A group's title row (CARD-25). */
const GROUP_H = 24;

/**
 * A card's rows, in the order drawn. With no groups: its inputs, a row for each output beyond them,
 * its settings and its views. With groups (CARD-25): its connections (inputs no group names: notes,
 * audio), then each group's title over its rows; a folded card shows its first group, and a row
 * saying how many more there are, which unfolds it.
 */
type Item =
  | { t: "input"; port: InputPort }
  | { t: "group"; name: string; first: boolean }
  | { t: "setting"; x: Setting }
  | { t: "view"; v: CardView }
  | { t: "fill"; id: string }
  | { t: "fold"; hidden: number; hides: string[] };

function items(n: ViewNode, folded: boolean): Item[] {
  const settings = settingsOf(n);
  const views = n.views ?? [];
  if (!n.groups?.length) {
    return [
      ...n.inputs.map((port): Item => ({ t: "input", port })),
      ...n.outputs.slice(n.inputs.length).map((o): Item => ({ t: "fill", id: o.id })),
      ...settings.map((x): Item => ({ t: "setting", x })),
      ...views.map((v): Item => ({ t: "view", v })),
    ];
  }
  const grouped = new Set(n.groups.flatMap((g) => g.rows.flatMap((r) => ("input" in r ? [r.input] : []))));
  const out: Item[] = n.inputs.filter((p) => !grouped.has(p.id)).map((port) => ({ t: "input", port }));
  const rowsOf = (g: CardGroup): Item[] =>
    g.rows.flatMap((r): Item[] => {
      if ("input" in r) return n.inputs.filter((p) => p.id === r.input).map((port) => ({ t: "input", port }));
      if ("setting" in r) return settings.filter((x) => x.id === r.setting).map((x) => ({ t: "setting", x }));
      return views.filter((v) => v.name === r.view).map((v) => ({ t: "view", v }));
    });
  n.groups.forEach((g, i) => {
    if (folded && i > 0) return;
    out.push({ t: "group", name: g.name, first: i === 0 }, ...rowsOf(g));
  });
  if (folded && n.groups.length > 1) {
    const rest = n.groups.slice(1).flatMap(rowsOf);
    const hides = rest.flatMap((x) => (x.t === "input" && x.port.knob ? [x.port.knob] : []));
    out.push({ t: "fold", hidden: rest.length, hides });
  }
  const rows = out.filter((x) => x.t === "input").length;
  out.push(...n.outputs.slice(rows).map((o): Item => ({ t: "fill", id: o.id })));
  return out;
}

/** How tall a row of a card is. */
const itemHeight = (x: Item) =>
  x.t === "input" ? inputHeight(x.port) : x.t === "group" ? GROUP_H : x.t === "setting" ? settingHeight(x.x) : x.t === "view" ? VIEW_H[x.v.kind] : ROW_H;

/**
 * A node's height and where its ports are, from its top. Each input is a row; output `i` sits on
 * the right of the `i`th input's row, so a chain of first inputs and outputs draws as a straight
 * line. Outputs beyond the inputs get rows of their own. An input a folded card hides has its port
 * on the row that says how many more there are.
 */
function geometry(n: ViewNode, folded = false) {
  const ins = new Map<string, number>();
  const outs = new Map<string, number>();
  if (compact(n)) {
    for (const p of n.inputs) ins.set(p.id, HEAD_H / 2);
    for (const p of n.outputs) outs.set(p.id, HEAD_H / 2);
    return { w: SLIM_W, h: HEAD_H, ins, outs };
  }
  const rows: number[] = [];
  // Under the header: how an instrument plays (RENDER-16), and a hosted plugin's row: its window and latches (HOST-7).
  let y = HEAD_H + (n.voices ? VOICES_H : 0) + (n.plugin ? ROW_H : 0) + PAD;
  const all = items(n, folded);
  for (const x of all) {
    const h = itemHeight(x);
    if (x.t === "input") {
      ins.set(x.port.id, y + h / 2);
      rows.push(y + h / 2);
    } else if (x.t === "fill") outs.set(x.id, y + h / 2);
    else if (x.t === "fold") for (const p of n.inputs) if (!ins.has(p.id)) ins.set(p.id, y + h / 2);
    y += h;
  }
  n.outputs.forEach((o, j) => {
    if (j < rows.length) outs.set(o.id, rows[j]);
  });
  const roomy = n.inputs.length || all.some((x) => x.t === "setting" || x.t === "view");
  const w = roomy && n.outputs.length ? NODE_W : (n.views?.length ? NODE_W : SLIM_W);
  return { w, h: all.length ? y + PAD : HEAD_H + (n.voices ? VOICES_H : 0), ins, outs };
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
function layout(m: ModuleView, folded: Set<string> = new Set()): { placed: Map<string, Placed>; width: number; height: number; back: Set<PortEdge> } {
  const geo = new Map(m.nodes.map((n) => [n.id, geometry(n, folded.has(n.id))]));
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

/**
 * The knob drags this window holds, by gesture: the React keys their knob and its node were
 * mounted with. Ids number the calls in a file in order, so a save that inserts a call above a
 * dragged knob in its file renumbers it, and maybe its node. The server carries the drag
 * (WRITE-22), and the view's `leases` say what the knob is called now: keyed as they were
 * mounted, the knob and its node stay mounted, where new keys would remount them, and the knob's
 * unmount would cancel the drag.
 */
const held = new Map<string, { knob: string; node: string }>();

/** React keys for a module's nodes and knobs: their ids, but a held drag's knob and node keep their keys (see `held`). */
function stableKeys(nodes: ViewNode[], leases: GraphLease[] | undefined) {
  const knobs = new Map<string, string>();
  const keep = new Map<string, string>();
  for (const l of leases ?? []) {
    const h = l.gesture != null ? held.get(l.gesture) : undefined;
    if (!h) continue;
    // The node it was dragged on first, should one value have knobs on two.
    const has = (n: ViewNode) => n.knobs.some((k) => k.id === l.id);
    const node = nodes.find((n) => n.id === h.node && has(n)) ?? nodes.find(has);
    if (!node) continue;
    if (l.id !== h.knob) knobs.set(l.id, h.knob);
    if (node.id !== h.node) keep.set(node.id, h.node);
  }
  // A kept key is taken from whatever has that id now.
  const keyed = (kept: Map<string, string>) => {
    const taken = new Set(kept.values());
    return (id: string) => kept.get(id) ?? (taken.has(id) ? `${id}~` : id);
  };
  return { knob: keyed(knobs), node: keyed(keep) };
}

/** Memoized: the playback status re-renders the canvas five times a second. */
const ModuleSection = memo(function ModuleSection({
  module,
  laid: { placed, width, height, back },
  zoom,
  session,
  revision,
  editable,
  effects,
  leases,
}: {
  module: ModuleView;
  laid: ReturnType<typeof layout>;
  /** The nodes are laid out at 1 and drawn scaled by this. */
  zoom: number;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  leases: GraphLease[] | undefined;
}) {
  const toast = useStore((s) => s.toast);
  const keys = useMemo(() => stableKeys(module.nodes, leases), [module, leases]);
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
        // The knob turns into "← name": that says it, so no toast does (taste.md §2.14).
        graphApi
          .connect(gesture.session, gesture.revision, gesture.from, knob.dataset.knobId!, { name: `connecting ${gesture.name}` })
          .catch((error) => toast(error instanceof Error ? error.message : String(error)));
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [placed, session, revision, toast],
  );

  return (
    // Flat on the dock, edge to edge: no card, no title row, no legend (taste.md DK-3).
    <section className="grow shrink-0 w-max min-w-full flex flex-col p-4" data-module={module.key}>
      {/* The canvas fills the panel, dotted all over; the graph sits at its top left, as a chain reads (taste.md §3.5). */}
      <div data-canvas className="relative grow grid place-items-start" style={{ minWidth: width * zoom, minHeight: height * zoom }}>
        <Dots />
        <div className="relative" style={{ width: width * zoom, height: height * zoom }}>
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
                    // From an effect turned off: its sound goes past it, so the line is dashed and quiet.
                    data-bypassed={e.bypassed || undefined}
                    strokeDasharray={loop || e.bypassed ? "6 5" : CABLE[e.kind].dash}
                    data-from={e.from}
                    data-from-port={e.from_port}
                    data-to={e.to}
                    data-to-port={e.to_port}
                    data-kind={e.kind}
                    stroke={e.bypassed ? "var(--color-line-strong)" : CABLE[e.kind].stroke}
                    strokeWidth={CABLE[e.kind].width}
                    strokeLinecap="round"
                    fill="none"
                  />
                );
              })}
            </svg>
            {[...placed.values()].map((p) => (
              <NodeCard
                key={keys.node(p.node.id)}
                module={module.key}
                nodeKey={keys.node(p.node.id)}
                knobKey={keys.knob}
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
                  stroke="var(--color-accent)"
                  strokeWidth="2"
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

/**
 * The glyph for a card that isn't a call: a lane, the time, a track's signal coming in, an output, a
 * value, a voice. A call's card has none: its title says what it is, and one glyph on every effect
 * would repeat the dock's or a category's (taste.md §2.12, one icon per meaning).
 */
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
                : null;
  // A plain glyph: no tile, no colour (taste.md §2.12).
  return Icon && <Icon size={14} className="shrink-0 text-fg-faint" />;
}

/** A track's signal coming in, by the track's name: `Kick.out` is Kick, its other outputs Kick · env. */
function importTitle(label: string) {
  const dot = label.lastIndexOf(".");
  if (dot < 0) return label;
  const port = label.slice(dot + 1);
  return port === "out" ? label.slice(0, dot) : `${label.slice(0, dot)} · ${port}`;
}

/** An input: its kind's mark centred on a node's left edge, filled when something is connected to it. */
function Dot({ id, kind, filled, className }: { id: string; kind: PortKind; filled: boolean; className?: string }) {
  return (
    <span
      data-port-dot
      data-port="in"
      data-port-id={id}
      data-kind={kind}
      className={clsx("absolute top-1/2 -translate-x-1/2 left-0 transition-transform duration-80", SHAPE_AT[kind], mark(kind, filled), className)}
    />
  );
}

function NodeCard({
  module,
  nodeKey,
  knobKey,
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
  /** Its module's key: where its scope reads and its keys play (CARD-28, CARD-29). */
  module: string;
  /** Its React key, and its knobs' (see `held`). */
  nodeKey: string;
  knobKey: (id: string) => string;
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
  // The effect card's ⋯ menu, open from this button. A card kept across an edit may show
  // another call now: its menu closes then.
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  // Its Presets popover (FMT-22), open from the header's Presets button.
  const [presets, setPresets] = useState<HTMLElement | null>(null);
  const call = `${node.id} ${node.label}`;
  useEffect(() => {
    setMenu(null);
    setPresets(null);
  }, [call]);
  const why = useContext(Locked);
  /** A structural edit: said in words, with its Undo (taste.md §2.14, ST-5); a refusal says why. */
  const edited = (p: Promise<{ revision: number; step?: number }>, words: string) =>
    p.then((c) => toast(words, undoAction(session, c.step))).catch((error) => toast(error instanceof Error ? error.message : String(error)));
  // Just the header, its one circle beside it (see `compact`).
  const small = compact(node);
  // The Song's mixer, as one compact card (MX-6).
  const card = useContext(MixerCard);
  const mixing = card?.id === node.id ? card : null;
  const where = node.file ? `${node.file}:${node.line}` : null;
  // A call by its component's label ("LFO", "Reverb"); a track's signal by the track; a lane or an
  // output by the rule controls follow (`cutoff` → Cutoff). No caption under it: the glyph and the
  // place say what kind it is (principle 4).
  const titled =
    node.kind === "import"
      ? importTitle(node.label)
      : node.kind === "lane" || node.kind === "export"
        ? controlLabel(node.label)
        : (node.effect ?? node.title ?? effects.find((e) => e.component === node.component)?.label ?? node.label);
  // Turned off: the title says so in `fg-muted`, and the body is half seen (taste.md §3.5).
  const off = !!node.bypassed;
  // An instrument (it takes notes) or an effect of a component has Presets, and so does a plugin (HOST-8).
  // Its settings (CARD-7); a card with one recording takes a file dropped anywhere on it, as a Sampler's does.
  const settings = mixing ? [] : settingsOf(node);
  // Its rows as drawn: in its groups, with its first alone when folded (CARD-25).
  const folded = useFolded((s) => s.ids.has(node.id));
  const rows = mixing ? [] : items(node, folded);
  const drawnInputs = rows.flatMap((x) => (x.t === "input" ? [x.port] : []));
  const recordings = settings.filter((x) => x.kind === "recording" && !x.readonly);
  const tracks = useContext(Tracks);
  const sound =
    node.kind === "call" &&
    !mixing &&
    (!!node.plugin || (!!node.component && (node.inputs.some((i) => i.kind === "notes") || effects.some((e) => e.component === node.component))));
  return (
    <div
      className="group/card absolute rounded-md bg-raised ring-1 ring-line flex flex-col"
      style={style}
      data-node={node.label}
      data-node-id={node.id}
      data-bypassed={off || undefined}
      data-recording-call={recordings.length === 1 ? node.id : undefined}
      data-recording-setting={recordings.length === 1 ? recordings[0].slot : undefined}
      data-recording-title={recordings.length === 1 ? titled : undefined}
      title={where ?? undefined}
    >
      <div className={clsx("h-10 shrink-0 pl-3 pr-1.5 flex items-center gap-2 min-w-0", !small && "border-b border-line")}>
        <NodeIcon node={node} />
        <div className="min-w-0 flex-1">
          {/* Its label (an effect's descriptor says it), the code's name in the tooltip; its file and line only there. */}
          <div
            data-node-title
            className={clsx(
              "text-sm truncate",
              off ? "font-semibold text-fg-muted" : node.kind === "call" || node.kind === "lane" || node.kind === "voice" ? "font-semibold text-fg" : "font-medium text-fg-muted",
            )}
            title={titled !== node.label ? node.label : undefined}
          >
            {off ? `${titled} · Off` : titled}
          </div>
        </div>
        {(node.groups?.length ?? 0) > 1 && (
          <button
            className="shrink-0 size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
            data-fold={node.id}
            title={folded ? `Show all of ${titled}'s groups` : `Fold ${titled} to its first group, ${node.groups![0].name}`}
            aria-label={folded ? `Unfold ${titled}` : `Fold ${titled}`}
            aria-expanded={!folded}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => useFolded.getState().flip(node.id)}
          >
            <ChevronRight size={12} absoluteStrokeWidth strokeWidth={1.5} className={clsx("transition-transform duration-140 ease-out", !folded && "rotate-90")} />
          </button>
        )}
        {sound && (
          <button
            className={clsx(
              "shrink-0 h-ctl-sm px-1.5 rounded-sm text-xs font-medium text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear",
              presets && "bg-accent-soft text-accent-strong",
            )}
            data-presets={node.id}
            title={`Presets for ${titled}: load one, or save these settings`}
            aria-haspopup="dialog"
            aria-expanded={!!presets}
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => {
              const button = e.currentTarget;
              setPresets((p) => (p ? null : button));
            }}
          >
            Presets
          </button>
        )}
        {node.effect && node.menu && (
          <button
            className={clsx(
              "shrink-0 size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-[opacity,color,background-color] duration-80 ease-linear",
              menu ? "opacity-100 bg-accent-soft text-accent-strong" : "opacity-0 group-hover/card:opacity-100 focus-visible:opacity-100",
            )}
            data-effect-menu={node.id}
            title={`More for ${titled}`}
            aria-label={`${node.effect} menu`}
            aria-haspopup="menu"
            aria-expanded={!!menu}
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => {
              const button = e.currentTarget;
              setMenu((m) => (m ? null : button));
            }}
          >
            <Ellipsis size={14} />
          </button>
        )}
        {node.effect && (
          <button
            className="shrink-0 size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover disabled:text-fg-off disabled:pointer-events-none cursor-pointer transition-colors duration-80 ease-linear"
            title={why ?? `Remove ${titled}, keeping what it was applied to`}
            aria-label={`Remove ${node.effect}`}
            disabled={!editable}
            onClick={() => void edited(graphApi.removeEffect(session, revision, node.id, { name: `removing ${titled}`, structural: true }), `Removed ${titled}`)}
          >
            <Trash2 size={14} />
          </button>
        )}
        {node.audio && effects.length > 0 && (
          <button
            className={clsx(
              "shrink-0 size-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover disabled:text-fg-off disabled:pointer-events-none cursor-pointer transition-colors duration-80 ease-linear",
              picking && "bg-accent-soft text-accent-strong",
            )}
            title={why ?? "Add an effect after this"}
            aria-label={`Add an effect after ${node.label}`}
            aria-haspopup="dialog"
            aria-expanded={!!picking}
            disabled={!editable}
            onClick={(e) => {
              const button = e.currentTarget;
              setPicking((p) => (p ? null : button));
            }}
          >
            <Plus size={14} />
          </button>
        )}
      </div>
      {node.voices && !small && <VoicesLine words={node.voices} />}
      {node.plugin && <PluginRow node={node.plugin} title={titled} session={session} />}
      {presets && (
        <PresetPopover
          node={node.id}
          title={titled}
          session={session}
          revision={revision}
          anchor={presets}
          locked={why ?? (editable ? null : "This song can't be edited here")}
          onClose={() => setPresets(null)}
        />
      )}
      {menu && node.menu && (
        <EffectMenuPopover node={node} title={titled} anchor={menu} locked={why ?? (editable ? null : "This song can't be edited here")} onClose={() => setMenu(null)} />
      )}
      {/* In the module, outside its zoomed nodes: it keeps its size and its place by the +. */}
      {picking &&
        createPortal(
          <EffectBrowser
            effects={effects}
            after={node.label}
            named={titled}
            anchor={picking}
            onClose={closePicker}
            onPick={(effect) => {
              setPicking(null);
              void edited(graphApi.addEffect(session, revision, node.id, effect), `Added ${effect.label} after ${titled}`);
            }}
          />,
          picking.closest("section[data-module]") ?? document.body,
        )}
      {small && node.inputs.map((i) => <Dot key={i.id} id={i.id} kind={i.kind} filled={wired.has(`${node.id}<${i.id}`)} />)}
      {mixing && (
        <div className="py-1.5">
          <div className="relative h-7 pl-4 pr-4 flex items-center gap-2 text-xs min-w-0" data-input={CHANNELS}>
            <Dot id={CHANNELS} kind="audio" filled={wired.has(`${node.id}<${CHANNELS}`)} />
            <span className="text-fg-muted truncate">
              {mixing.channels} {mixing.channels === 1 ? "channel" : "channels"}
            </span>
            <button
              data-testid="open-mix"
              className="ml-auto shrink-0 font-medium text-accent hover:text-accent-strong cursor-pointer"
              title="Levels and pans are in the Mix view"
              onClick={() => useShell.getState().setView("mix")}
            >
              Open Mix {opt("3")}
            </button>
          </div>
        </div>
      )}
      {!small && !mixing && rows.length > 0 && (
        <div data-node-body className={clsx("py-1.5", off && "opacity-50")}>
          {rows.map((x) =>
            x.t === "input" ? (
              <InputRow
                key={x.port.knob ? `knob:${knobKey(x.port.knob)}` : x.port.id}
                nodeKey={nodeKey}
                knobKey={knobKey}
                node={node}
                owner={titled}
                input={x.port}
                filled={wired.has(`${node.id}<${x.port.id}`)}
                session={session}
                revision={revision}
                editable={editable}
              />
            ) : x.t === "fill" ? (
              <div key={`fill:${x.id}`} className="h-7" />
            ) : x.t === "setting" ? (
              <SettingRow key={x.x.id} x={x.x} owner={titled} card={titled} call={node.id} session={session} revision={revision} editable={editable} locked={why} tracks={tracks} />
            ) : x.t === "view" ? (
              <ViewRow key={`view:${x.v.name}`} view={x.v} node={node} module={module} session={session} revision={revision} editable={editable} locked={why} owner={titled} />
            ) : x.t === "group" ? (
              <div key={`group:${x.name}`} data-group={x.name} className={clsx("h-6 px-4 flex items-end pb-1 text-xs font-semibold text-fg-muted", !x.first && "border-t border-line")}>
                {x.name}
              </div>
            ) : (
              <FoldRow key="fold" node={node} hides={x.hides} hidden={x.hidden} titled={titled} />
            ),
          )}
        </div>
      )}
      {node.outputs.map((o, j) => (
        <OutputPortView
          key={o.id}
          node={node}
          output={o}
          y={ports.get(o.id) ?? 0}
          // Only a signal's row (or an output's own) has room for its label beside it; the mixer card's has its link.
          labelled={!small && !mixing && (j >= drawnInputs.length || (!drawnInputs[j].knob && !drawnInputs[j].inlet && drawnInputs[j].readonly == null))}
          filled={wired.has(`${node.id}>${o.id}`)}
          editable={editable}
          onCable={onCable}
        />
      ))}
    </div>
  );
}

/**
 * How an instrument plays its notes (RENDER-16), under its name: its first sentence ("Plays one
 * note at a time."), and all of it in the tooltip. Read-only: its code sets it.
 */
function VoicesLine({ words }: { words: string }) {
  const first = words.split(/(?<=\.) /)[0];
  return (
    <div data-testid="voices" className="h-6 shrink-0 px-3 flex items-center text-xs text-fg-muted min-w-0 border-b border-line" title={words} aria-label={words}>
      <span className="truncate">{first}</span>
    </div>
  );
}

/**
 * A plugin's own window and its latches (HOST-7, MON-6), a row under its card's header: Open
 * window or Close window, and what a gesture there latched, with Re-enable, which gives it back
 * to its lane or cable.
 */
function PluginRow({ node, title, session }: { node: string; title: string; session: number }) {
  const toast = useStore((s) => s.toast);
  const box = useStore((s) => s.graph?.plugins?.[node]);
  const [busy, setBusy] = useState(false);
  const said = (p: Promise<unknown>) => void p.catch((error) => toast(error instanceof Error ? error.message : String(error)));
  const open = box?.window === "open";
  const latched = box?.latched ?? [];
  return (
    <div className="h-7 shrink-0 pl-1 pr-3 flex items-center gap-2 border-b border-line text-xs min-w-0" data-plugin={node}>
      {box && box.window !== "none" ? (
        <button
          className="shrink-0 h-6 px-2 flex items-center gap-1.5 rounded-sm font-medium text-accent hover:text-accent-strong hover:bg-hover disabled:text-fg-off cursor-pointer transition-colors duration-80 ease-linear"
          data-plugin-window={open ? "open" : "closed"}
          title={open ? `Close ${title}'s own window` : `Open ${title}'s own window, to change its sound there`}
          aria-label={open ? "Close window" : "Open window"}
          disabled={busy}
          onClick={() => {
            setBusy(true);
            said(graphApi.pluginWindow(session, node, !open).finally(() => setBusy(false)));
          }}
        >
          <AppWindow size={14} />
          {/* With a latch beside it, the icon alone: the row has room for one label. */}
          {latched.length === 0 && (open ? "Close window" : "Open window")}
        </button>
      ) : (
        <span className="pl-2 text-fg-faint truncate">{box ? "No window of its own" : ""}</span>
      )}
      {latched.length > 0 && (
        <>
          <span
            className="ml-auto shrink-0 text-fg-muted"
            data-latched={latched.map((l) => l.id).join(" ")}
            title={`${latched.map((l) => l.name).join(", ")}: set in the window, so ${latched.length === 1 ? "it doesn't" : "they don't"} follow ${latched.length === 1 ? "its" : "their"} lane or cable now. Only while this song is open, never in an export.`}
          >
            {latched.length === 1 ? "Latched" : `${latched.length} latched`}
          </span>
          <button
            className="shrink-0 font-medium text-accent hover:text-accent-strong cursor-pointer"
            data-reenable
            title={`Follow the lane or cable again: ${latched.map((l) => l.name).join(", ")}`}
            onClick={() => said(graphApi.pluginReenable(session, node))}
          >
            Re-enable
          </button>
        </>
      )}
    </div>
  );
}

/**
 * An effect card's ⋯ menu (taste.md §3.5, next X11): Turn off or Turn on, Move left and Move
 * right. A row that can't run here is greyed, with why in its tooltip.
 */
function EffectMenuPopover({ node, title, anchor, locked, onClose }: { node: ViewNode; title: string; anchor: HTMLElement; locked: string | null; onClose: () => void }) {
  const off = !!node.bypassed;
  const bypass = locked ?? node.menu?.bypass ?? null;
  return (
    <Popover anchor={anchor} onClose={onClose} align="end" role="menu" aria-label={`${title} menu`} data-testid="effect-menu" className="w-[220px]">
      <MenuRow
        icon={<Power size={14} />}
        disabled={!!bypass}
        title={bypass ?? (off ? `Turn ${title} back on` : `Turn ${title} off (keeps its settings)`)}
        data-testid="effect-bypass"
        onClick={() => {
          onClose();
          void turnEffect(node, title, !off);
        }}
      >
        {off ? "Turn on" : "Turn off"}
      </MenuRow>
      {([true, false] as const).map((earlier) => {
        const row = earlier ? node.menu?.left : node.menu?.right;
        const why = locked ?? row?.refused ?? null;
        const other = row?.with;
        return (
          <MenuRow
            key={String(earlier)}
            icon={earlier ? <ArrowLeft size={14} /> : <ArrowRight size={14} />}
            disabled={!!why || !other}
            title={why ?? `Move ${title} ${earlier ? "before" : "after"} ${other}`}
            data-testid={earlier ? "effect-left" : "effect-right"}
            onClick={() => {
              onClose();
              if (other) void moveEffect(node, title, other, earlier);
            }}
          >
            {earlier ? "Move left" : "Move right"}
          </MenuRow>
        );
      })}
    </Popover>
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
  const why = useContext(Locked);
  const cable = output.outlet && node.outlet;
  // Only a named signal can be connected: say how to name this one.
  const last = node.label.split(".").pop()!;
  const unnamed =
    node.kind === "call"
      ? `To connect ${node.label} to a knob, name it in code first: const my${last[0].toUpperCase()}${last.slice(1)} = ${node.label}(…)`
      : `To connect ${node.label} to a knob, name it with a const in code first.`;
  return (
    <div className="absolute right-0 h-7 -mt-3.5 max-w-[55%] pr-4 flex items-center justify-end text-xs pointer-events-none" style={{ top: y }} data-output={output.label}>
      {labelled && <span className={clsx("truncate", cable ? "text-fg-muted" : "text-fg-faint")}>{output.label}</span>}
      {cable ? (
        <button
          className={clsx(
            "absolute -right-3 top-1/2 -mt-3 size-6 grid place-items-center rounded-full group/outlet",
            editable ? "cursor-crosshair pointer-events-auto" : "pointer-events-none",
          )}
          data-outlet={node.outlet!}
          data-port="out"
          data-port-id={output.id}
          data-kind={output.kind}
          aria-label={`Connect ${node.outlet}`}
          title={editable ? `Drag onto a knob to connect ${node.outlet}` : (why ?? "")}
          disabled={!editable}
          onPointerDown={(e) => onCable(node, output, e)}
        >
          {/* Where a cable starts: filled, with a ring that says it can be dragged. */}
          <span className={clsx(SHAPE[output.kind], "bg-fg-muted shadow-[0_0_0_2px_var(--color-raised),0_0_0_3.5px_var(--color-line-strong)] transition-transform duration-80 group-hover/outlet:scale-125")} />
        </button>
      ) : (
        <span
          className="absolute -right-3 top-1/2 -mt-3 size-6 grid place-items-center cursor-help pointer-events-auto"
          data-port="out"
          data-port-id={output.id}
          data-kind={output.kind}
          title={unnamed}
          onPointerDown={(e) => e.button === 0 && output.kind !== "notes" && toast(unnamed)}
        >
          <span className={mark(output.kind, filled)} />
        </span>
      )}
    </div>
  );
}

function InputRow({
  nodeKey,
  knobKey,
  node,
  owner,
  input,
  filled,
  session,
  revision,
  editable,
}: {
  nodeKey: string;
  knobKey: (id: string) => string;
  node: ViewNode;
  /** What the card is called, to name an edit of its knobs ("Reverb size"). */
  owner: string;
  input: InputPort;
  filled: boolean;
  session: number;
  revision: number;
  editable: boolean;
}) {
  const toast = useStore((s) => s.toast);
  const tracks = useContext(Tracks);
  const why = useContext(Locked);
  // The menu of an input an automation lane drives (KN-2): Show lane, Remove automation.
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const knob = input.knob ? node.knobs.find((k) => k.id === input.knob) : undefined;
  // Keyed by the knob alone: a save that breaks the code mid-drag must not remount it, which
  // would cancel the drag (WRITE-22: the server keeps it, and a release then is saved once the
  // code compiles); nor may one that renumbers it (see `held`).
  if (knob) {
    const key = knobKey(knob.id);
    // A menu or a switch (CARD-20, CARD-6): one click, one edit; never dragged.
    if (picks(knob)) return <PickControl key={key} knob={knob} port={input.id} label={controlLabel(input.label, tracks)} owner={owner} session={session} revision={revision} editable={editable} />;
    return (
      <KnobControl key={key} mountKey={key} nodeKey={nodeKey} knob={knob} port={input.id} label={controlLabel(input.label, tracks)} owner={owner} session={session} revision={revision} editable={editable} />
    );
  }

  const inlet = input.inlet ? node.inlets.find((i) => i.id === input.inlet) : undefined;
  if (inlet) {
    // The knob comes back in its place: that says it, so no toast does.
    const disconnect = () =>
      graphApi.disconnect(session, revision, inlet.id, { name: `disconnecting ${inlet.name}` }).catch((error) => toast(error instanceof Error ? error.message : String(error)));
    return (
      <div className="relative h-[28px] pl-4 pr-3 flex items-center gap-1.5 text-xs min-w-0" data-inlet={inlet.label}>
        <Dot id={input.id} kind={input.kind} filled />
        <span className="text-fg-muted truncate">{controlLabel(inlet.label, tracks)}</span>
        {/* Its home is a cable (HOME-1): what drives it, in place of a knob. */}
        <span className="text-fg-muted truncate" title={`Driven by ${inlet.name}.`}>← {inlet.name}</span>
        <button
          className="ml-auto shrink-0 size-ctl-xs grid place-items-center rounded-sm text-fg-faint hover:text-fg hover:bg-hover disabled:text-fg-off disabled:pointer-events-none cursor-pointer transition-colors duration-80 ease-linear"
          title={`Disconnect ${inlet.name} (writes the input's default)`}
          aria-label={`Disconnect ${inlet.name} from ${inlet.label}`}
          disabled={!editable}
          onClick={() => void disconnect()}
        >
          <X size={12} />
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
        <div className="flex items-center justify-between gap-2 text-xs">
          <span className="text-fg-muted truncate">{controlLabel(source.label, tracks)}</span>
          <span className="shrink-0 text-fg-faint">edit in code</span>
        </div>
        <div data-code className="font-mono text-xs text-fg truncate">{source.expression}</div>
      </div>
    );
  }

  // A Duck's side input: what it listens to, and a list of the tracks it could (next D3).
  if (node.listens && node.listens.port === input.id) return <ListensRow node={node} input={input} filled={filled} editable={editable} />;

  // Driven by an automation lane: its home is the lane (HOME-1), and its menu shows the lane or
  // takes the automation out (KN-2).
  const driven = input.lane;
  const label = controlLabel(input.label, tracks);
  const lane = driven && useStore.getState().graph ? findLane(useStore.getState().graph!.timeline, driven.module, driven.lane) : undefined;
  const laneName = lane ? laneTitle(lane) : label;
  return (
    <div
      className="relative h-[28px] pl-4 pr-3 flex items-center gap-2 text-xs min-w-0"
      data-input={input.label}
      data-lane-input={driven ? `${driven.module}.${driven.lane}` : undefined}
      title={driven ? `Follows the ${laneName} lane. Right-click to show it, or to take the automation out.` : undefined}
      onContextMenu={
        driven
          ? (e) => {
              e.preventDefault();
              e.stopPropagation();
              setMenu({ x: e.clientX, y: e.clientY });
            }
          : undefined
      }
    >
      <Dot id={input.id} kind={input.kind} filled={filled} />
      <span className="text-fg-muted truncate">{label}</span>
      {driven && <span aria-hidden className="ml-auto shrink-0 size-[5px] rounded-full bg-accent" />}
      {menu && driven && (
        <PointMenu
          at={menu}
          label={`${label} input`}
          items={[
            { id: "show", label: `Show ${laneName} lane`, run: () => laneMade(driven.module, driven.lane) },
            { id: "unautomate", label: "Remove automation", off: editable ? null : `${why ?? "It can't be edited right now"}.`, run: () => unautomate(driven) },
          ]}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/**
 * A Duck's side input (next D3, X28): "Listens to: Kick ▾". Its list has the song's other tracks;
 * one picked is written in the Duck's call. Where it can't change (a Duck in a track's file), the
 * button is greyed and its tooltip says why.
 */
function ListensRow({ node, input, filled, editable }: { node: ViewNode; input: InputPort; filled: boolean; editable: boolean }) {
  const tracks = useContext(Tracks);
  const why = useContext(Locked);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const l = node.listens!;
  const name = (key: string) => l.choices.find(([k]) => k === key)?.[1] ?? tracks(key) ?? key;
  const now = l.track ? name(l.track) : "another sound";
  const off = why ?? (editable ? null : "This song can't be edited here") ?? l.refused ?? null;
  return (
    <div className="relative h-[28px] pl-4 pr-3 flex items-center gap-1.5 text-xs min-w-0" data-input={input.label} data-listens>
      <Dot id={input.id} kind={input.kind} filled={filled} />
      <span className="shrink-0 text-fg-muted">Listens to:</span>
      <button
        data-testid="listens-to"
        className={clsx(
          "min-w-0 h-ctl-xs px-1 -mx-1 flex items-center gap-1 rounded-sm text-xs font-medium transition-colors duration-80 ease-linear",
          off ? "text-fg-muted cursor-default" : "text-fg hover:bg-hover cursor-pointer",
          anchor && "bg-accent-soft text-accent-strong",
        )}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        aria-disabled={!!off || undefined}
        title={off ?? `It ducks when ${now} plays. Choose another track`}
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          if (off) return;
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
      >
        <span className="truncate">{now}</span>
        <ChevronDown size={12} className="shrink-0" />
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} role="menu" aria-label="Listens to" data-testid="listens-menu" className="w-[200px]">
          {l.choices.map(([key, label]) => (
            <MenuRow
              key={key}
              data-track={key}
              icon={<Check size={14} className={key === l.track ? undefined : "invisible"} />}
              onClick={() => {
                setAnchor(null);
                if (key !== l.track) void listenTo(node, key, label);
              }}
            >
              {label}
            </MenuRow>
          ))}
        </Popover>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- knobs

/** A knob over an owned source value, as one input row: drag to hear it, release to write it. */
function KnobControl({
  mountKey,
  nodeKey,
  knob,
  port,
  label,
  owner,
  session,
  revision,
  editable,
}: {
  /** The React keys it and its node are mounted with, kept while it is dragged (see `held`). */
  mountKey: string;
  nodeKey: string;
  knob: KnobInfo;
  port: string;
  /** Its name on screen ("Feedback", "Hats volume"). */
  label: string;
  /** Its card's ("Delay"). */
  owner: string;
  session: number;
  revision: number;
  editable: boolean;
}) {
  const why = useContext(Locked);
  // Its drag and lease (Knob.tsx). A save that renumbers it mid-drag doesn't remount it (see `held`).
  const g = useKnobGesture({
    session,
    revision,
    knob,
    editable,
    // Its undo step's name: the card and the control ("Delay feedback"), or a channel's own ("Hats volume").
    name: knob.label.includes(".") ? label : `${owner} ${label.toLowerCase()}`,
    label,
    hold: (token, on) => (on ? held.set(token, { knob: mountKey, node: nodeKey }) : held.delete(token)),
  });
  const value = g.value;
  // Changed elsewhere just now (AGENT-5): outlined until the next click or key press.
  const glows = useGlows("knobs", knob.id);
  // The knob's menu (right-click), anchored to the knob: its left edge, below it (or above it, the room).
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const readout = useRef<HTMLSpanElement>(null);
  // A value being typed in place of the readout (taste.md §2.14): its text, the readout's width, and
  // whether Enter found it unreadable. The ref says whether it is still open, for a late blur.
  const [typing, setTyping] = useState<{ text: string; width: number; bad: boolean } | null>(null);
  const typingOpen = useRef(false);

  /** A move of the knob to `u` (0..1): heard while dragged; on its own (a double-click, the menu's reset) one whole gesture. */
  const change = (u: number) => g.change(fromUnit(g.descriptor, u));

  const automate = () => {
    setMenu(null);
    automateKnob(session, revision, knob.id);
  };

  /** `n` steps from where it is (taste.md §5.2), written as one edit half a second after the last. */
  const step = (n: number) => g.step((v, k) => fromUnit(k, Math.min(1, Math.max(0, toUnit(k, stepValue(k, v, n))))));
  /** Type a value: the readout turns into a field of its size, its text selected. */
  const type = () => {
    // Steps just made are written first.
    g.settle();
    if (!editable || g.dragging()) return;
    typingOpen.current = true;
    setTyping({ text: display(knob, value), width: Math.max(48, Math.ceil(readout.current?.getBoundingClientRect().width ?? 0) + 8), bad: false });
  };
  const closeTyping = () => {
    typingOpen.current = false;
    setTyping(null);
  };
  /** Write what was typed, if it reads as a value; false (and the field stays) if it doesn't. */
  const typed = (text: string) => {
    const v = parseValue(knob, text);
    if (v === null) return false;
    closeTyping();
    if (v !== knob.value) change(Math.min(1, Math.max(0, toUnit(knob, v))));
    return true;
  };

  const descriptor = g.descriptor;
  // A drag already in progress goes on when the code breaks under it; only new ones wait.
  const disabled = (!editable && !g.dragging()) || g.saving;
  // Drawn from the middle when its span is centred on zero (a pan, a bipolar amount).
  const range = span(descriptor);
  const bipolar = range.lo < 0 && range.hi > 0 && Math.abs(range.lo + range.hi) <= 1e-9 * range.hi;
  const shared = knob.uses > 1 ? `Shared value: changes all ${knob.uses} uses. ` : "";
  // A control the call leaves out plays its default until its first edit writes it into the call (PKG-3).
  const written = knob.written ? `written as ${knob.literal}` : "the default: not written in the code yet, turning it writes it in";
  const how = `Drag (${mod.shift} for fine) · double-click to type a value · ${mod.alt}-click to reset · right-click to automate it`;
  const title = !editable
    ? `${why ?? "It can't be edited right now"}.`
    : `${shared}${how}\nIn your code: ${knob.file}, line ${knob.line} (${written})`;
  const unit = Math.min(1, Math.max(0, toUnit(descriptor, value)));
  const reset = descriptor.default != null ? Math.min(1, Math.max(0, toUnit(descriptor, descriptor.default))) : undefined;
  // The whole row drags it, its label and value too, not only the 28px circle (taste.md §2.2).
  const grip = useKnobDrag({ value: unit, reset, disabled, onChange: change, onStart: () => g.start(), onEnd: g.end, onCancel: g.abandon, focus: () => svg.current?.focus({ preventScroll: true }) });

  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        if (!g.dragging()) setMenu({ x: r.left, y: r.bottom + 4, above: r.top - 4 });
      }}
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-glow={glows || undefined}
      data-value={value}
      data-shared={knob.uses > 1 || undefined}
      data-default={!knob.written || undefined}
      aria-disabled={disabled}
      title={title}
      data-kind="number"
      {...grip.handlers}
      // A double-click anywhere on it types a value (taste.md §2.14), but on its input's port.
      onDoubleClick={(e) => e.currentTarget.contains(e.target as Node) && !(e.target as HTMLElement).closest("[data-port-dot], input") && type()}
      className={clsx(
        "group/knob relative h-10 pl-4 pr-4 flex items-center gap-2 min-w-0 transition-colors duration-80",
        !disabled && "cursor-ns-resize touch-none",
        // While a cable is dragged, the knobs it can land on: washed, and the one under it more.
        "[[data-cabling]_&[data-knob-id]]:bg-accent-wash [[data-cabling]_&[data-knob-id]:hover]:bg-accent-soft",
      )}
    >
      <Dot
        id={port}
        kind="control"
        filled={false}
        className="[[data-cabling]_[data-knob-id]>&]:scale-[1.35] [[data-cabling]_[data-knob-id]>&]:![box-shadow:inset_0_0_0_2px_var(--color-accent),0_0_0_3px_var(--color-accent-range)]"
      />
      <div className="flex-1 min-w-0">
        <div className="text-xs text-fg-muted truncate">{label}</div>
        {knob.uses > 1 && <div className="text-xs text-fg-faint">shared ×{knob.uses}</div>}
      </div>
      {/* Its home (HOME-1): a value the code leaves out plays its default, in muted text; written, in full. */}
      {typing ? (
        <span className="relative shrink-0">
          <input
            autoFocus
            data-testid="knob-type-in"
            aria-label={`${label}: type a value`}
            aria-invalid={typing.bad || undefined}
            value={typing.text}
            spellCheck={false}
            style={{ width: typing.width }}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setTyping({ ...typing, text: e.target.value, bad: false })}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") {
                if (!typed(typing.text)) setTyping({ ...typing, bad: true });
              } else if (e.key === "Escape") closeTyping();
              else if (e.key === "Tab") {
                // On to the next control (or back, with ⇧), as a form's fields go.
                e.preventDefault();
                if (!typed(typing.text)) return setTyping({ ...typing, bad: true });
                focusNext(svg.current, e.shiftKey);
              }
            }}
            // A click away writes what reads as a value, and drops what doesn't.
            onBlur={() => typingOpen.current && !typed(typing.text) && closeTyping()}
            onPointerDown={(e) => e.stopPropagation()}
            className={clsx(
              "h-5 -my-0.5 px-1 rounded-sm bg-raised border text-right text-xs text-fg outline-none",
              typing.bad ? "border-danger" : "border-accent",
            )}
          />
          {typing.bad && (
            <span role="alert" className="absolute right-0 top-full mt-0.5 z-10 whitespace-nowrap text-xs text-danger">
              Type a value, like −6, 2k or 120 ms
            </span>
          )}
        </span>
      ) : (
        <span
          ref={readout}
          className={clsx("shrink-0 text-xs tabular-nums", knob.written ? "text-fg" : "text-fg-muted")}
          title={knob.written ? undefined : "Default. Not written in your code yet: changing it writes it in."}
        >
          {display(descriptor, value)}
        </span>
      )}
      <HomeMarker home={knob.home} label={label} />
      {/* Inert only while it can't be edited: a save between steps mustn't take the focus from it. */}
      <div inert={!editable && !g.dragging()} className={clsx("shrink-0", disabled && "pointer-events-none")}>
        <MiniKnob
          value={unit}
          bipolar={bipolar}
          disabled={disabled}
          readout={display(descriptor, value)}
          label={label}
          reset={reset}
          svg={svg}
          pointer={grip.pointer}
          onChange={change}
          onStep={step}
          onType={type}
        />
      </div>
      {menu && (
        <KnobMenu
          knob={knob}
          label={label}
          value={value}
          at={menu}
          editable={editable && !g.saving}
          locked={why}
          onAutomate={automate}
          onReset={descriptor.default != null ? () => change(Math.min(1, Math.max(0, toUnit(descriptor, descriptor.default!)))) : undefined}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/** A knob's value becomes a new, empty control lane's fallback (HOME-2): the song sounds the same, and the timeline shows the lane to draw on. */
function automateKnob(session: number, revision: number, id: string) {
  const toast = useStore.getState().toast;
  graphApi
    .automate(session, revision, id)
    .then((r) => {
      toast(`Automating ${r.name} in a new lane`, undoAction(session, r.step));
      laneMade(r.lane.module, r.lane.lane);
    })
    .catch((error) => toast(error instanceof Error ? error.message : String(error)));
}

/**
 * A knob that is a menu or a switch (CARD-20): a choice control's names, a switch's off and on, or a
 * plugin's stepped parameter (CARD-6). A pick writes its option's value, a name, true or false, or a
 * plugin's step number, into the code: one click, one edit, one undo step, never a drag. It stays a
 * knob to the rest of the app: its id for the focus and the pointer, its glow, cable drops, and its
 * menu's Automate.
 */
function PickControl({
  knob,
  port,
  label,
  owner,
  session,
  revision,
  editable,
}: {
  knob: PickKnob;
  port: string;
  label: string;
  owner: string;
  session: number;
  revision: number;
  editable: boolean;
}) {
  const toast = useStore((s) => s.toast);
  const why = useContext(Locked);
  const glows = useGlows("knobs", knob.id);
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const [saving, setSaving] = useState(false);
  const off = editable ? null : `${why ?? "It can't be edited right now"}.`;
  const shared = knob.uses > 1 ? `Shared value: changes all ${knob.uses} uses. ` : "";
  const written = knob.written ? `written as ${knob.literal}` : "the default: not written in the code yet, picking one writes it in";
  const how = `Click to ${knob.kind === "switch" ? "turn it on or off" : "choose"} · right-click to automate it`;
  const title = off ?? `${shared}${how}\nIn your code: ${knob.file}, line ${knob.line} (${written})`;
  /** A pick: its option's value written into the code, one undo step named "Wobble shape". */
  const pick = (v: number | string | boolean) => {
    setSaving(true);
    graphApi
      .setValue(session, revision, knob.id, v, { name: knob.label.includes(".") ? label : `${owner} ${label.toLowerCase()}` })
      .catch((error) => toast(words(error)))
      .finally(() => setSaving(false));
  };
  const blocked = off ?? (saving ? "Saving…" : null);
  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        setMenu({ x: r.left, y: r.bottom + 4, above: r.top - 4 });
      }}
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-glow={glows || undefined}
      data-value={JSON.stringify(knob.value)}
      data-shared={knob.uses > 1 || undefined}
      data-default={!knob.written || undefined}
      data-kind={knob.kind}
      aria-disabled={!!off}
      title={title}
      className={clsx(
        "group/knob relative h-10 pl-4 pr-4 flex items-center gap-2 min-w-0 transition-colors duration-80",
        "[[data-cabling]_&[data-knob-id]]:bg-accent-wash [[data-cabling]_&[data-knob-id]:hover]:bg-accent-soft",
      )}
    >
      <Dot
        id={port}
        kind="control"
        filled={false}
        className="[[data-cabling]_[data-knob-id]>&]:scale-[1.35] [[data-cabling]_[data-knob-id]>&]:![box-shadow:inset_0_0_0_2px_var(--color-accent),0_0_0_3px_var(--color-accent-range)]"
      />
      <div className="flex-1 min-w-0">
        <div className="text-xs text-fg-muted truncate">{label}</div>
        {knob.uses > 1 && <div className="text-xs text-fg-faint">shared ×{knob.uses}</div>}
      </div>
      {knob.kind === "switch" ? (
        <SwitchValue value={knob} label={label} off={blocked} onFlip={pick} />
      ) : (
        <MenuValue value={knob} label={label} off={blocked} title={title} onPick={pick} />
      )}
      <HomeMarker home={knob.home} label={label} />
      {menu && (
        <KnobMenu
          knob={knob}
          label={label}
          value={knob.value}
          at={menu}
          editable={editable && !saving}
          locked={why}
          onAutomate={() => {
            setMenu(null);
            automateKnob(session, revision, knob.id);
          }}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/**
 * A folded card's last row (CARD-25): how many more rows it has, a click unfolding it. It stands for
 * the knobs it hides: an agent's pointer at one shows here (`data-hides`), and so does its glow.
 */
function FoldRow({ node, hides, hidden, titled }: { node: ViewNode; hides: string[]; hidden: number; titled: string }) {
  const glows = useNotice((s) => hides.some((id) => !!s.glow?.knobs.has(id)));
  return (
    <button
      data-unfold
      data-hides={hides.join(" ") || undefined}
      data-glow={glows || undefined}
      className="h-7 w-full px-4 flex items-center gap-1 text-xs text-fg-muted hover:text-fg cursor-pointer"
      title={`Show ${titled}'s other groups`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => useFolded.getState().flip(node.id)}
    >
      <ChevronRight size={12} absoluteStrokeWidth strokeWidth={1.5} />
      {hidden} more
    </button>
  );
}

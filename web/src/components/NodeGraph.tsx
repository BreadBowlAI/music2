// The Graph (taste.md §3.5, DV-12 to DV-16; the graph note): a sound's code as nodes and wires. A
// call's node is collapsed (the default) or expanded, the window's own, kept by its key (CARD-25);
// every value is a bar (DV-14). A click on a header selects a node, Enter expands it, Delete
// removes an effect, Esc clears the selection (DV-15).

import clsx from "clsx";
import {
  AppWindow,
  ArrowLeft,
  ArrowRight,
  AudioLines,
  Check,
  ChevronDown,
  ChevronsDownUp,
  Clock,
  Code,
  Copy,
  Drum,
  Ellipsis,
  Hash,
  LogIn,
  LogOut,
  Maximize2,
  Minimize2,
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
import { create } from "zustand";
import {
  controlLabel,
  display,
  findLane,
  fromUnit,
  graphApi,
  knobStep,
  laneTitle,
  lockReason,
  moduleName,
  parseValue,
  picks,
  span,
  stepValue,
  toUnit,
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
  type ViewNode,
} from "../lib/graph";
import { listenTo, moveEffect, turnEffect } from "../lib/effects";
import { laneMade } from "../lib/events";
import { useExpanded } from "../lib/expanded";
import { mod, opt } from "../lib/native";
import { useGlows, useNotice } from "../lib/notice";
import { COL_W, EXPR_H, NODE_W, PACK_H, packOf, GROUP_H, LINE_H, PAD, PLUGIN_H, ROW_H, VALUE_H, gridRows, segHeight, shapeOf, type Column, type HeadRoom, type Shape, type Value } from "../lib/nodes";
import { trackColors } from "../lib/palette";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { Bar } from "./Bar";
import { Dots } from "./draw";
import EffectBrowser from "./EffectBrowser";
import { focusNext, HomeMarker, KnobMenu, useKnobDrag, useKnobGesture } from "./Knob";
import { MenuRow, Popover } from "./ui";
import { undoAction } from "./GraphCanvas";
import PointMenu, { unautomate } from "./ClipMenu";
import PresetPopover from "./PresetPopover";
import { makeKit } from "./Tracks";
import { MenuValue, Segmented, SettingRow, SwitchValue, words } from "./Settings";
import { ViewRow } from "./CardViews";
import { useKeyAsk } from "./KeyField";

/** Why nothing here can be edited right now (the code doesn't compile, read only, offline), or null. */
export const Locked = createContext<string | null>(null);
/** A track's name by its key, for the controls that name one (a mixer channel's `hats.db`: Hats volume). */
export const Tracks = createContext<(key: string) => string | undefined>(() => undefined);
/** The Song's mixer, drawn as one compact node (MX-6): its node and how many channels it has. */
export const MixerCard = createContext<{ id: string; channels: number } | null>(null);
/** Whether a wire is being dragged in this graph: a value that takes none then says why (DV-6). */
const Cabling = createContext(false);

/** The merged input of the compact mixer node: every channel's input, as one port. */
const CHANNELS = "channels";

/**
 * The Song's own mixer as one compact node (taste.md §3.5, MX-6), "4 channels · Open Mix ⌥3",
 * never a row per channel: its channels' inputs drawn as one port, and no values, since its
 * levels and pans are the Mix view's strips. Only the mixer the Mix view shows: a track's own mixer
 * stays a node like any other. Null when `m` has no such mixer.
 */
function compactMixer(m: ModuleView, mix: MixView): { module: ModuleView; card: { id: string; channels: number } } | null {
  if (m.key !== "song" || mix.mixer !== "ok") return null;
  const mixers = m.nodes.filter((n) => n.component === "std:mixer");
  if (mixers.length !== 1) return null;
  const node = mixers[0];
  const channels = new Set(mix.strips.flatMap((s) => (s.channel ? [s.channel] : []))).size;
  const card: ViewNode = { ...node, knobs: [], readonly: [], inlets: [], settings: [], groups: [], face: [], views: [], code: undefined, inputs: [{ id: CHANNELS, label: "Channels", kind: "audio" }] };
  // Each signal drawn once into the node, whichever channels it feeds.
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

/** What a node's header holds beside its label (see `HeadTools`): the room its label has (DV-12). */
function headRoom(effects: Effect[], perNote: Set<string>) {
  return (n: ViewNode, expanded: boolean, hidden: number, w: number): HeadRoom => {
    const wide = w > NODE_W;
    const words: string[] = [];
    if (hidden > 0) words.push(`+${hidden}`);
    if (perNote.has(n.id)) words.push("per note");
    if (expanded && packOf(n) && wide) words.push(packOf(n)!);
    if (expanded && hasPresets(n, effects)) words.push("Presets");
    const buttons = (bypassable(n) ? 1 : 0) + (n.kind === "call" ? 2 : 0) + (n.kind !== "call" ? 1 : 0);
    // Room for "· Off" whether it is off or on, so turning it off never moves the node (taste.md §3.5).
    return { title: bypassable(n) || n.bypassed ? `${titleOf(n, effects)} · Off` : titleOf(n, effects), buttons, words };
  };
}

/** The nodes that play inside an instrument's voices, once for each note, as the view says (CARD-25). */
const perNoteOf = (m: ModuleView) => new Set(m.nodes.filter((n) => n.per_note).map((n) => n.id));

/** One module's code as a graph to play and tweak: bars over its values, wires between its signals. */
export function NodeGraph({ graph, module }: { graph: GraphState; module: string }) {
  const found = graph.view.modules.find((m) => m.key === module);
  // The Song's mixer is one compact node; its levels and pans are the Mix view's (MX-6).
  const compacted = useMemo(() => (found ? compactMixer(found, graph.mix) : null), [found, graph.mix]);
  const shown = compacted?.module ?? found;
  const expanded = useExpanded((s) => s.keys);
  const perNote = useMemo(() => (shown ? perNoteOf(shown) : new Set<string>()), [shown]);
  const room = useMemo(() => headRoom(graph.effects, perNote), [graph.effects, perNote]);
  const single = compacted?.card.id;
  const laid = useMemo(() => shown && layout(shown, expanded, room, single), [shown, expanded, room, single]);
  // Fit is worked out from the graph with every node collapsed: expanding one never changes it (DV-13).
  const collapsed = useMemo(() => shown && layout(shown, new Set(), room, single), [shown, room, single]);
  const locked = useStore((s) => lockReason(graph, s.connected));
  const tracks = useMemo(() => {
    const by = new Map(graph.view.modules.filter((m) => m.track).map((m) => [m.key, moduleName(m)]));
    return (key: string) => by.get(key);
  }, [graph.view.modules]);

  // The zoom: fitted to the panel until the user picks one, per module.
  const scroller = useRef<HTMLDivElement>(null);
  const [room0, setRoom] = useState<{ w: number; h: number } | null>(null);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // The panel's whole box, less a scrollbar each way: never the room the scrollbars showing now
    // leave. Those come and go with the zoom, so a fit to them flickers: a scrollbar shows, the fit
    // shrinks the graph, the scrollbar goes, the fit grows it, every frame (DV-5).
    const measure = () => {
      const bar = scrollbarRoom();
      const w = el.offsetWidth - bar, h = el.offsetHeight - bar;
      setRoom((r) => (r && r.w === w && r.h === h ? r : { w, h }));
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  const [zooms, setZooms] = useState<Record<string, number>>({});
  const fit = collapsed && room0 ? fitZoom(collapsed, room0) : 1;
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
                perNote={perNote}
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
/**
 * Fit (taste.md DV-5): 100%, so a node's 12 px label and 11 px values draw at the type scale's
 * sizes. Under it they wouldn't (at 75% a title drew at 9 px: review Z8); a graph bigger than the
 * dock scrolls instead.
 */
const FIT = 1;

/**
 * How much room a scrollbar takes from what it scrolls: none where scrollbars float over it (a Mac
 * with a trackpad), else its width (a mouse, or "Show scroll bars: Always").
 */
function scrollbarRoom() {
  const probe = document.createElement("div");
  probe.style.cssText = "position:absolute;visibility:hidden;overflow:scroll;width:40px;height:40px";
  document.body.append(probe);
  const room = probe.offsetWidth - probe.clientWidth;
  probe.remove();
  return room;
}

/**
 * The zoom Fit picks (taste.md §3.5, DV-5): a sound that fits the panel's height at full size (a
 * line of nodes) opens at 100% and scrolls sideways, the way signal reads; one that doesn't is
 * shrunk to fit, down to 75%, and scrolls past that. `room` leaves a scrollbar's room each way, so
 * what fits never needs one.
 */
function fitZoom(_laid: { width: number; height: number }, _room: { w: number; h: number }) {
  return FIT;
}

/** Zoom out, the zoom (click: actual size), zoom in, fit, and Collapse all: floating at the panel's bottom right. */
function ZoomBar({ zoom, fitted, onZoom }: { zoom: number; fitted: boolean; onZoom: (z: number | null) => void }) {
  const button = "h-ctl-sm grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear";
  const any = useExpanded((s) => s.keys.size > 0);
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
      <button
        className={clsx(button, "w-ctl-sm disabled:text-fg-off disabled:pointer-events-none")}
        data-testid="collapse-all"
        disabled={!any}
        onClick={() => useExpanded.getState().collapseAll()}
        title={any ? "Collapse every node" : "Every node is collapsed"}
        aria-label="Collapse all"
      >
        <ChevronsDownUp size={14} />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- inputs, outputs and the layout

/**
 * What flows through an input or an output, by shape and line, never colour (taste.md §2.8): audio
 * a circle, control a diamond, notes a rounded square, all 8px, filled when connected; wires as
 * heavy, light or dashed.
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

const GAP_X = 60;
const GAP_Y = 18;

interface Placed { node: ViewNode; x: number; y: number; w: number; h: number; ins: Map<string, number>; outs: Map<string, number>; shape: Shape }

/** A wire's own height, as a gap a skipping wire passes through holds it (DV-5). */
const GAP_H = 4;
/** An edge by its ends, as the graph draws and routes it. */
const edgeKey = (e: PortEdge) => `${e.from}>${e.from_port}>${e.to}>${e.to_port}`;

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
 * feeds as the column allows, so a line of nodes draws straight; then whatever nothing reads (an
 * export, a signal not used yet) moves level with what feeds it, into the nearest free space in its
 * column. Edges that go back round a loop are left out of that, and drawn round below everything.
 * A pure function of the graph and each node's size (DV-13): expanding a node only changes its size.
 */
function layout(
  m: ModuleView,
  expanded: ReadonlySet<string>,
  room: (n: ViewNode, expanded: boolean, hidden: number, w: number) => HeadRoom,
  /** A node with one size: the Song's compact mixer (MX-6). */
  single?: string,
): { placed: Map<string, Placed>; width: number; height: number; back: Set<PortEdge>; routes: Map<string, { x0: number; x1: number; y: number }[]> } {
  const geo = new Map<string, { w: number; h: number; ins: Map<string, number>; outs: Map<string, number> }>(
    m.nodes.map((n) => [n.id, shapeOf(n, expanded.has(n.key ?? n.id), room, n.id === single)]),
  );
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
  // A column of two sources or more (what nothing feeds, and something reads) hangs from where its
  // wires land, never above it, so what they feed keeps the top of the graph's first view (DV-5).
  const hanging = new Set<number>();
  for (let c = 0; c <= last; c++) if (m.nodes.filter((n) => depth.get(n.id) === c && !into.get(n.id)?.size && from.get(n.id)?.size).length > 1) hanging.add(c);
  // A wire that skips a column passes through a gap there, held open by a stand-in the height of a
  // wire, placed with the column's nodes, so it never runs behind one (DV-5, review Z7).
  const gaps = new Map<string, string[]>();
  const linked: PortEdge[] = [];
  for (const e of links) {
    const [a, b] = [depth.get(e.from)!, depth.get(e.to)!];
    if (b - a < 2) {
      linked.push(e);
      continue;
    }
    const ids: string[] = [];
    for (let c = a + 1; c < b; c++) {
      const id = `gap:${edgeKey(e)}:${c}`;
      ids.push(id);
      depth.set(id, c);
      geo.set(id, { w: 0, h: GAP_H, ins: new Map([["gap", GAP_H / 2]]), outs: new Map([["gap", GAP_H / 2]]) });
    }
    gaps.set(edgeKey(e), ids);
    const chain = [e.from, ...ids, e.to];
    for (let k = 0; k + 1 < chain.length; k++)
      linked.push({ ...e, from: chain[k], from_port: k === 0 ? e.from_port : "gap", to: chain[k + 1], to_port: k + 2 === chain.length ? e.to_port : "gap" });
  }
  links.splice(0, links.length, ...linked);
  into.clear();
  from.clear();
  for (const e of links) {
    (from.get(e.from) ?? from.set(e.from, new Set()).get(e.from)!).add(e.to);
    (into.get(e.to) ?? into.set(e.to, new Set()).get(e.to)!).add(e.from);
  }
  const columns: { id: string }[][] = [];
  for (const n of m.nodes) (columns[depth.get(n.id)!] ??= []).push(n);
  for (const ids of gaps.values()) for (const id of ids) (columns[depth.get(id)!] ??= []).push({ id });

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
    let tops = pack(wanted.map((x) => ({ want: x.want!, h: x.h })));
    const lowest = Math.min(...wanted.map((x) => x.want!));
    if (hanging.has(c) && tops.length && tops[0] < lowest) tops = tops.map((y) => y + lowest - tops[0]);
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
  const widths = columns.map((col) => Math.max(...(col ?? []).map((n) => geo.get(n.id)!.w), 0));
  // A column of gaps alone still has room for its wires to pass.
  for (let c = 0; c < widths.length; c++) if (!widths[c]) widths[c] = GAP_X;
  const xs = widths.map((_, c) => widths.slice(0, c).reduce((a, w) => a + w + GAP_X, 0));
  const placed = new Map<string, Placed>();
  let height = 0;
  for (const n of m.nodes) {
    const g = geo.get(n.id)!;
    const c = depth.get(n.id)!;
    const y = top.get(n.id)! + shift;
    const x = xs[c] + (n.inputs.length ? 0 : widths[c] - g.w);
    placed.set(n.id, { node: n, x, y, w: g.w, h: g.h, ins: g.ins, outs: g.outs, shape: g as Shape });
    height = Math.max(height, y + g.h);
  }
  // A loop comes back into its input from the left, LOOP_OUT before it: room for it there too.
  const left = Math.min(0, ...[...back].map((e) => (placed.get(e.to)?.x ?? LOOP_OUT) - LOOP_OUT - 4));
  if (left < 0) for (const p of placed.values()) p.x -= left;
  // Each skipping wire's way through the columns it skips: across each, level at its gap.
  const routes = new Map<string, { x0: number; x1: number; y: number }[]>();
  for (const [k, ids] of gaps) {
    routes.set(
      k,
      ids.map((id) => {
        const c = depth.get(id)!;
        const y = top.get(id)! + shift + GAP_H / 2;
        height = Math.max(height, y + GAP_H);
        return { x0: xs[c] - left, x1: xs[c] + widths[c] - left, y };
      }),
    );
  }
  const width = Math.max(0, ...[...placed.values()].map((p) => p.x + p.w));
  return back.size ? { placed, width: width + LOOP_OUT + 4, height: height + LOOP_ROOM, back, routes } : { placed, width, height, back, routes };
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

/** How far apart the + of a selected node's wires stay, at the least (DV-7): a button and a little. */
const PLUS_GAP = 24;

/** The point a fraction `t` along `curve` (or a routed wire's stretch, its bow at least `bow`). */
function along(x0: number, y0: number, x1: number, y1: number, bow: number, t: number) {
  const dx = Math.max(bow, Math.abs(x1 - x0) * 0.45);
  const [ax, bx] = [x0 + dx, x1 - dx];
  const u = 1 - t;
  return { x: u * u * u * x0 + 3 * u * u * t * ax + 3 * u * t * t * bx + t * t * t * x1, y: u * u * u * y0 + 3 * u * u * t * y0 + 3 * u * t * t * y1 + t * t * t * y1 };
}

/** A smooth curve from an output (left) to an input (right), bowing out when it runs backwards. */
function curve(x0: number, y0: number, x1: number, y1: number) {
  const dx = Math.max(48, Math.abs(x1 - x0) * 0.45);
  return `M ${x0} ${y0} C ${x0 + dx} ${y0}, ${x1 - dx} ${y1}, ${x1} ${y1}`;
}

/**
 * A wire through the gaps of the columns it skips (DV-5): a curve into each gap, straight across
 * the column there, and a curve from the last one into its input.
 */
function routed(x0: number, y0: number, x1: number, y1: number, via: { x0: number; x1: number; y: number }[]) {
  let d = `M ${x0} ${y0}`;
  let [x, y] = [x0, y0];
  const to = (bx: number, by: number) => {
    const dx = Math.max(24, Math.abs(bx - x) * 0.45);
    d += ` C ${x + dx} ${y}, ${bx - dx} ${by}, ${bx} ${by}`;
    [x, y] = [bx, by];
  };
  for (const g of via) {
    to(g.x0, g.y);
    d += ` H ${g.x1}`;
    x = g.x1;
  }
  to(x1, y1);
  return d;
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

/** A wire being dragged from an output, in the module's coordinates; `to` is the value it would land on. */
interface Cable { from: string; name: string; kind: PortKind; x0: number; y0: number; x: number; y: number; to: string | null }

/**
 * The value drags this window holds, by gesture: the React key their bar was mounted with. Ids
 * number the calls in a file in order, so a save that inserts a call above a dragged value in its
 * file renumbers it. The server carries the drag (WRITE-22), and the view's `leases` say what the
 * value is called now: keyed as it was mounted, the bar stays mounted, where a new key would
 * remount it, and its unmount would cancel the drag. Its node keeps its key anyway (CARD-25).
 */
const held = new Map<string, { knob: string }>();

/** React keys for a module's values: their ids, but a held drag's value keeps its key (see `held`). */
export function stableKeys(nodes: ViewNode[], leases: GraphLease[] | undefined) {
  const knobs = new Map<string, string>();
  for (const l of leases ?? []) {
    const h = l.gesture != null ? held.get(l.gesture) : undefined;
    if (!h || !nodes.some((n) => n.knobs.some((k) => k.id === l.id))) continue;
    if (l.id !== h.knob) knobs.set(l.id, h.knob);
  }
  // A kept key is taken from whatever has that id now.
  const taken = new Set(knobs.values());
  return { knob: (id: string) => knobs.get(id) ?? (taken.has(id) ? `${id}~` : id) };
}

/** Where a wire was dropped on a collapsed node: that node, and where, for its menu of values (DV-6). */
interface Dropped { node: ViewNode; at: { x: number; y: number }; from: string; name: string; session: number; revision: number }

/** Memoized: the playback status re-renders the canvas five times a second. */
const ModuleSection = memo(function ModuleSection({
  module,
  laid: { placed, width, height, back, routes },
  zoom,
  session,
  revision,
  editable,
  effects,
  leases,
  perNote,
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
  perNote: Set<string>;
}) {
  const toast = useStore((s) => s.toast);
  const keys = useMemo(() => stableKeys(module.nodes, leases), [module, leases]);
  const area = useRef<HTMLDivElement>(null);
  const [cable, setCable] = useState<Cable | null>(null);
  const [dropped, setDropped] = useState<Dropped | null>(null);
  // Ports with an edge, to draw them filled.
  const wired = useMemo(() => {
    const s = new Set<string>();
    for (const e of module.port_edges) {
      s.add(`${e.from}>${e.from_port}`);
      s.add(`${e.to}<${e.to_port}`);
    }
    return s;
  }, [module]);
  // The nodes in layout order, left to right and top to bottom: the order Tab takes (DV-15).
  const ordered = useMemo(() => [...placed.values()].sort((a, b) => a.x - b.x || a.y - b.y), [placed]);
  // Each wire's path, and where its + sits: halfway along its last stretch, into its input (CARD-36).
  const drawn = useMemo(
    () =>
      module.port_edges.flatMap((e) => {
        const p = placed.get(e.from), q = placed.get(e.to);
        const y0 = p?.outs.get(e.from_port), y1 = q?.ins.get(e.to_port);
        if (!p || !q || y0 == null || y1 == null) return [];
        const [x0, x1, ya, yb] = [p.x + p.w, q.x, p.y + y0, q.y + y1];
        if (back.has(e)) {
          const low = height - LOOP_ROOM / 2;
          const at = { x: (x0 + x1) / 2, y: low };
          return [{ e, loop: true, d: loopBack(x0, ya, x1, yb, low), at, out: at, into: at }];
        }
        const via = routes.get(edgeKey(e));
        const [first, last] = [via?.[0], via?.at(-1)];
        const [sx, sy] = last ? [last.x1, last.y] : [x0, ya];
        // Near each end, for a selected node's own wires (DV-7): a fifth of the way along the stretch there.
        const out = first ? along(x0, ya, first.x0, first.y, 24, 0.2) : along(x0, ya, x1, yb, 48, 0.2);
        const into = last ? along(sx, sy, x1, yb, 24, 0.8) : along(x0, ya, x1, yb, 48, 0.8);
        return [{ e, loop: false, d: via ? routed(x0, ya, x1, yb, via) : curve(x0, ya, x1, yb), at: { x: (sx + x1) / 2, y: (sy + yb) / 2 }, out, into }];
      }),
    [module, placed, back, routes, height],
  );
  // A wire's + shows under the pointer (`useWireHover`, so a hover draws only its +), on the selected
  // node's own wires near its end, each 24 px from the next or only under the pointer (DV-7), and
  // while its browser is open.
  const keys0 = useMemo(() => new Map(module.nodes.map((n) => [n.id, n.key ?? n.id])), [module]);
  const keyOf = (id: string) => keys0.get(id) ?? id;
  const selected = useExpanded((s) => s.selected);
  const near = useMemo(() => {
    const shown = new Map<string, { x: number; y: number }>();
    if (selected == null) return shown;
    const placedAt: { x: number; y: number }[] = [];
    const own = drawn.flatMap((w) => {
      const [a, b] = [keyOf(w.e.from), keyOf(w.e.to)];
      return a === selected ? [{ k: edgeKey(w.e), at: w.out }] : b === selected ? [{ k: edgeKey(w.e), at: w.into }] : [];
    });
    for (const w of own.sort((p, q) => p.at.y - q.at.y || p.at.x - q.at.x)) {
      if (placedAt.some((q) => Math.hypot(q.x - w.at.x, q.y - w.at.y) < PLUS_GAP)) continue;
      placedAt.push(w.at);
      shown.set(w.k, w.at);
    }
    return shown;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawn, selected, keys0]);
  const [picking, setPicking] = useState<{ edge: PortEdge; anchor: HTMLElement } | null>(null);
  // A hover ends with its module: another module's (or song's) wire may have the same key.
  useEffect(() => () => useWireHover.setState({ hovered: null }), []);

  // Drag from a named output onto a bar: the value is replaced by the signal's name. Dropped on a
  // collapsed node, a menu offers its values (DV-6). The wire is drawn in the module's own
  // (unzoomed) coordinates.
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
        // A value no wire can drive is no place to drop one (a note effect's, DV-6).
        return el && area.current?.contains(el) && el.dataset.noCable == null ? el : null;
      };
      // The wire's end: the pointer, or the bar it would land on, at its left end.
      const end = (ev: PointerEvent | React.PointerEvent) => {
        const b = area.current!.getBoundingClientRect();
        const knob = target(ev);
        const z = scale.current;
        const at = knob?.querySelector("[role=slider], button, [data-bar]")?.getBoundingClientRect() ?? knob?.getBoundingClientRect();
        if (knob && at) return { x: (at.left - b.left) / z, y: (at.top + at.height / 2 - b.top) / z, to: knob.dataset.knobId! };
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
        if (!knob) {
          const under = document.elementFromPoint(ev.clientX, ev.clientY);
          // A value no wire can drive says why, in its node's words (a note effect's, RENDER-41).
          const refused = under?.closest<HTMLElement>("[data-no-cable]");
          if (refused && area.current?.contains(refused)) return toast(refused.dataset.noCable!);
          // On a node, not on a value: a menu of its values (DV-6), hidden ones too.
          const on = under?.closest<HTMLElement>("[data-node-id]");
          const dest = on && area.current?.contains(on) ? module.nodes.find((n) => n.id === on.dataset.nodeId) : undefined;
          if (dest && dest.id !== node.id && dest.kind === "call" && dest.knobs.length) setDropped({ node: dest, at: { x: ev.clientX, y: ev.clientY }, ...gesture });
          return;
        }
        // The bar turns into "← name": that says it, so no toast does (taste.md §2.14).
        graphApi
          .connect(gesture.session, gesture.revision, gesture.from, knob.dataset.knobId!, { name: `connecting ${gesture.name}` })
          .catch((error) => toast(error instanceof Error ? error.message : String(error)));
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [placed, session, revision, toast, module],
  );

  // The selection's keys (DV-15): Delete removes the selected node where Remove is offered; Esc
  // clears the selection. Not while a value, a field or a menu has the keyboard.
  const moving = useExpanded((s) => s.moving);
  const onKeyDown = (e: React.KeyboardEvent) => {
    const t = e.target as HTMLElement;
    if (t.closest("input, textarea, [role=slider], [role=menu], [contenteditable=true]")) return;
    const selected = useExpanded.getState().selected;
    if (!selected) return;
    if (e.key === "Escape") {
      useExpanded.getState().select(null);
      e.stopPropagation();
      return;
    }
    if (e.key !== "Delete" && e.key !== "Backspace") return;
    const node = module.nodes.find((n) => (n.key ?? n.id) === selected);
    e.preventDefault();
    e.stopPropagation();
    if (!node || !editable) return;
    // Where Remove can't, Delete says why in the view's words (DV-15, CARD-36).
    if (node.kind === "call" && !node.remove) void removeNode(node, titleOf(node, effects), session, revision);
    else toast(node.remove ?? `${titleOf(node, effects)} isn't an effect: change it in code.`);
  };

  return (
    // Flat on the dock, edge to edge: no card, no title row, no legend (taste.md DK-3). While a wire
    // is dragged, a value that takes none says why (DV-6).
    <Cabling.Provider value={!!cable}>
      <section className="grow shrink-0 w-max min-w-full flex flex-col p-4" data-module={module.key} data-moving={moving || undefined} onKeyDown={onKeyDown}>
        {/* The canvas fills the panel, dotted all over; the graph sits at its top left (taste.md §3.5). A click on it clears the selection. */}
        <div
          data-canvas
          className="relative grow grid place-items-start"
          style={{ minWidth: width * zoom, minHeight: height * zoom }}
          onPointerDown={(e) => !(e.target as HTMLElement).closest("[data-node-id]") && useExpanded.getState().select(null)}
        >
          <Dots />
          <div className="relative" style={{ width: width * zoom, height: height * zoom }}>
            <div
              ref={area}
              className="relative origin-top-left"
              style={{ width, height, transform: zoom === 1 ? undefined : `scale(${zoom})` }}
              data-cabling={cable ? true : undefined}
            >
              <svg className="absolute inset-0 pointer-events-none overflow-visible" width={width} height={height}>
                {drawn.map(({ e, loop, d }) => (
                  <path
                    key={edgeKey(e)}
                    d={d}
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
                ))}
                {/* Under the pointer, a wire shows its + (CARD-36): a wide, unseen line to hover, under the nodes so it never covers a port. */}
                {!cable && (
                  <g data-wire-hits aria-hidden>
                    {drawn.map(({ e, d }) => (
                      <path
                        key={edgeKey(e)}
                        d={d}
                        data-wire-hit={edgeKey(e)}
                        stroke="transparent"
                        strokeWidth={14}
                        fill="none"
                        style={{ pointerEvents: "stroke" }}
                        onPointerEnter={() => useWireHover.getState().set(edgeKey(e), true)}
                        onPointerLeave={() => useWireHover.getState().set(edgeKey(e), false)}
                      />
                    ))}
                  </g>
                )}
              </svg>
              {ordered.map((p) => (
                <Node
                  key={p.node.key ?? p.node.id}
                  module={module.key}
                  knobKey={keys.knob}
                  node={p.node}
                  shape={p.shape}
                  session={session}
                  revision={revision}
                  editable={editable}
                  effects={effects}
                  wired={wired}
                  perNote={perNote.has(p.node.id)}
                  onCable={startCable}
                  style={{ left: p.x, top: p.y, width: p.w, height: p.h }}
                />
              ))}
              {!cable &&
                drawn.map(({ e, at }) => {
                  const k = edgeKey(e);
                  return (
                    <WirePlus
                      key={k}
                      edge={e}
                      from={module.nodes.find((n) => n.id === e.from)!}
                      to={module.nodes.find((n) => n.id === e.to)!}
                      at={at}
                      near={near.get(k)}
                      open={picking?.edge === e}
                      locked={editable ? null : "This song can't be edited here"}
                      effects={effects}
                      onOpen={(anchor) => setPicking((p) => (p?.edge === e ? null : { edge: e, anchor }))}
                    />
                  );
                })}
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
        {dropped && <DropMenu dropped={dropped} onClose={() => setDropped(null)} />}
        {picking && (
          <WireBrowser
            edge={picking.edge}
            anchor={picking.anchor}
            from={module.nodes.find((n) => n.id === picking.edge.from)}
            to={module.nodes.find((n) => n.id === picking.edge.to)}
            effects={effects}
            session={session}
            revision={revision}
            onClose={() => setPicking(null)}
          />
        )}
      </section>
    </Cabling.Provider>
  );
});

/** The wire under the pointer, by its key: only its + draws again when it changes. */
const useWireHover = create<{ hovered: string | null; set: (key: string, on: boolean) => void }>((set) => ({
  hovered: null,
  set: (key, on) => set((s) => ({ hovered: on ? key : s.hovered === key ? null : s.hovered })),
}));

/** What a wire takes: a note effect on notes, an effect on a sound (CARD-36). */
const wireTakes = (kind: PortKind) => (kind === "notes" ? "a note effect" : "an effect");

/**
 * A wire's + (CARD-36, taste.md DV-7): a small round button halfway along its last stretch, seen
 * under the pointer and on the selected node's wires. It opens the effect browser with what fits the
 * wire; where nothing can go, it is greyed, its why in its tooltip, and a click says it.
 */
function WirePlus({
  edge,
  from,
  to,
  at,
  near,
  open,
  locked,
  effects,
  onOpen,
}: {
  edge: PortEdge;
  from: ViewNode;
  to: ViewNode;
  /** Halfway along its last stretch, where it shows under the pointer. */
  at: { x: number; y: number };
  /** Near the selected node's end, when it shows for the selection (DV-7). */
  near?: { x: number; y: number };
  open: boolean;
  locked: string | null;
  effects: Effect[];
  onOpen: (anchor: HTMLElement) => void;
}) {
  const toast = useStore((s) => s.toast);
  const k = edgeKey(edge);
  const hovered = useWireHover((s) => s.hovered === k);
  const shown = hovered || !!near;
  const spot = near ?? at;
  const why = useContext(Locked) ?? locked ?? (edge.plus && "refused" in edge.plus ? edge.plus.refused : null) ?? (edge.plus ? null : "This wire takes no effect.");
  const takes = wireTakes(edge.kind);
  return (
    <button
      className={clsx(
        "absolute z-10 size-5 -ml-2.5 -mt-2.5 grid place-items-center rounded-full bg-raised ring-1 ring-line-strong shadow-sm transition-opacity duration-80 ease-linear",
        shown || open ? "opacity-100" : "opacity-0 focus-visible:opacity-100",
        why ? "text-fg-off cursor-default" : "text-fg-muted hover:text-accent-strong hover:ring-accent cursor-pointer",
        open && "text-accent-strong ring-accent",
      )}
      style={{ left: spot.x, top: spot.y }}
      data-wire-plus={edgeKey(edge)}
      data-from={edge.from}
      data-to={edge.to}
      data-kind={edge.kind}
      data-shown={shown || open || undefined}
      data-refused={why ?? undefined}
      aria-label={`Add ${takes} after ${titleOf(from, effects)}, before ${titleOf(to, effects)}`}
      aria-disabled={!!why || undefined}
      aria-haspopup="dialog"
      aria-expanded={open}
      title={why ?? `Add ${takes} on this wire: after ${titleOf(from, effects)}, before ${titleOf(to, effects)}`}
      tabIndex={shown ? 0 : -1}
      onPointerEnter={() => useWireHover.getState().set(k, true)}
      onPointerLeave={() => useWireHover.getState().set(k, false)}
      onMouseDown={(e) => e.preventDefault()}
      onClick={(e) => (why ? toast(why) : onOpen(e.currentTarget))}
    >
      <Plus size={12} strokeWidth={2.5} />
    </button>
  );
}

/**
 * The effect browser for one wire (CARD-36): what fits it, effects on a sound and note effects on
 * notes, the rest greyed with why. A pick puts it there, one undo step, said with its Undo.
 */
function WireBrowser({
  edge,
  anchor,
  from,
  to,
  effects,
  session,
  revision,
  onClose,
}: {
  edge: PortEdge;
  anchor: HTMLElement;
  from: ViewNode | undefined;
  to: ViewNode | undefined;
  effects: Effect[];
  session: number;
  revision: number;
  onClose: () => void;
}) {
  const toast = useStore((s) => s.toast);
  const notes = edge.kind === "notes";
  const fits = useCallback(
    (e: Effect) => (notes && !e.notes ? `${e.label} goes on a sound, and this wire carries notes` : !notes && e.notes ? `${e.label} goes on notes, and this wire carries a sound` : null),
    [notes],
  );
  if (!from || !to || !edge.plus || "refused" in edge.plus) return null;
  const plus = edge.plus;
  const [before, after] = [titleOf(to, effects), titleOf(from, effects)];
  const where = notes ? `before ${before}` : `after ${after}`;
  return (
    <EffectBrowser
      effects={effects}
      // What it goes around, as the code writes it: a call's name, or the `const` a wire reads it by.
      wraps={"reader" in plus && from.outlet ? from.outlet : from.kind === "call" ? `${from.label}(…)` : "…"}
      label={`Add ${wireTakes(edge.kind)} ${where}`}
      placeholder={`Add ${where}…`}
      fits={fits}
      anchor={anchor}
      onClose={onClose}
      onPick={(effect) => {
        onClose();
        graphApi
          .addEffectOn(session, revision, plus, effect)
          .then((c) => toast(`Added ${effect.label} ${where}`, undoAction(session, c.step)))
          .catch((error) => toast(error instanceof Error ? error.message : String(error)));
      }}
    />
  );
}

/**
 * A wire dropped on a node, not on a value (DV-6, DV-16): a menu of the node's values, hidden ones
 * too; a value no wire can drive is dimmed, saying why. A pick connects it, as a drop on its bar does.
 */
function DropMenu({ dropped, onClose }: { dropped: Dropped; onClose: () => void }) {
  const toast = useStore((s) => s.toast);
  const tracks = useContext(Tracks);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const { node } = dropped;
  const values = node.inputs.flatMap((p) => {
    const k = p.knob ? node.knobs.find((x) => x.id === p.knob) : undefined;
    return k ? [{ p, k }] : [];
  });
  return (
    <>
      {createPortal(<span ref={setAnchor} aria-hidden className="fixed size-px pointer-events-none" style={{ left: dropped.at.x, top: dropped.at.y }} />, document.body)}
      {anchor && (
        <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={`Connect ${dropped.name} to`} data-testid="drop-menu" className="w-[240px] max-h-[320px] overflow-y-auto">
          <div className="h-ctl-sm px-2 flex items-center text-xs text-fg-muted">Connect {dropped.name} to</div>
          {values.map(({ p, k }) => {
            const off = k.cable_refused ? `${k.cable_refused}.` : null;
            return (
              <MenuRow
                key={k.id}
                data-drop-knob={k.label}
                data-knob-id={k.id}
                disabled={!!off}
                title={off ?? `Its id: ${k.id}`}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => {
                  onClose();
                  graphApi.connect(dropped.session, dropped.revision, dropped.from, k.id, { name: `connecting ${dropped.name}` }).catch((error) => toast(error instanceof Error ? error.message : String(error)));
                }}
              >
                {controlLabel(p, tracks)}
              </MenuRow>
            );
          })}
        </Popover>
      )}
    </>
  );
}

// ---------------------------------------------------------------- nodes

/**
 * The glyph for a node that isn't a call: a lane, the time, a track's signal coming in, an output, a
 * value, a voice. A call's node has none: its title says what it is, and one glyph on every effect
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


/** Whether ⏻ works on `n`: the server offers Turn off for it (a component call, sound in and sound out alike, PKG-3). */
const bypassable = (n: ViewNode) => !!n.menu && !n.menu.bypass;

/** An instrument (it takes notes) or an effect of a component has Presets, and so does a plugin (HOST-8). */
const hasPresets = (n: ViewNode, effects: Effect[]) =>
  n.kind === "call" && (!!n.plugin || (!!n.component && (n.inputs.some((i) => i.kind === "notes") || effects.some((e) => e.component === n.component))));

/** Remove: the node unwrapped, `b(x(a))` to `b(a)`, keeping what it worked on (the graph note §3). */
function removeNode(node: ViewNode, titled: string, session: number, revision: number) {
  const toast = useStore.getState().toast;
  return graphApi
    .removeEffect(session, revision, node.id, { name: `removing ${titled}`, structural: true })
    .then((c) => toast(`Removed ${titled}`, undoAction(session, c.step)))
    .catch((error) => toast(error instanceof Error ? error.message : String(error)));
}

function Node({
  module,
  knobKey,
  node,
  shape,
  session,
  revision,
  editable,
  effects,
  wired,
  perNote,
  onCable,
  style,
}: {
  /** Its module's key: where its scope reads and its keys play (taste.md DV-9, DV-10). */
  module: string;
  knobKey: (id: string) => string;
  node: ViewNode;
  shape: Shape;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  wired: Set<string>;
  /** It plays inside an instrument's voices, once for each note. */
  perNote: boolean;
  onCable: (node: ViewNode, output: OutputPort, e: React.PointerEvent) => void;
  style: React.CSSProperties;
}) {
  const key = node.key ?? node.id;
  const el = useRef<HTMLDivElement>(null);
  const card = useContext(MixerCard);
  const mixing = card?.id === node.id ? card : null;
  // A call by its component's label ("LFO", "Reverb"); a track's signal by the track; a lane or an
  // output by the rule controls follow (`cutoff` → Cutoff). No caption under it: the glyph and the
  // place say what kind it is (principle 4).
  const titled = titleOf(node, effects);
  // Turned off: the title says so in `fg-muted`, and the body is half seen (taste.md §3.5).
  const off = !!node.bypassed;
  const selected = useExpanded((s) => s.selected === key);
  // The others move aside over 280 ms when one expands (DV-13); an edit's new layout lands at once.
  const moving = useExpanded((s) => s.moving);
  const lit = useExpanded((s) => (s.lit?.node === key ? s.lit.group : null));
  // A node with one recording takes a file dropped anywhere on it, as a Sampler's does.
  const recordings = (node.settings ?? []).filter((x) => x.kind === "recording" && !x.readonly);
  const pack = packOf(node);
  // An instrument (it takes notes) wears its track's colour along its top (DV-12).
  const instrument = node.kind === "call" && node.inputs.some((i) => i.kind === "notes");
  const colour = useStore((s) => (instrument && s.graph ? (trackColors(s.graph.timeline, module)?.bar ?? null) : null));
  const why = useContext(Locked);
  // Expanded, its top comes into view once the others have moved aside (DV-13).
  const was = useRef(shape.expanded);
  useEffect(() => {
    if (shape.expanded && !was.current) {
      const t = setTimeout(() => el.current?.scrollIntoView({ block: "nearest", inline: "nearest" }), 290);
      was.current = true;
      return () => clearTimeout(t);
    }
    was.current = shape.expanded;
  }, [shape.expanded]);
  const value = (v: Value) => <ValueRow key={valueKey(v)} v={v} node={node} owner={titled} module={module} knobKey={knobKey} session={session} revision={revision} editable={editable} expanded={shape.expanded} />;
  return (
    <div
      ref={el}
      className={clsx(
        "group/card absolute rounded-md bg-raised flex flex-col",
        moving && "transition-[left,top] duration-280 ease-out",
        selected ? "ring-2 ring-accent" : "ring-1 ring-line",
      )}
      style={style}
      data-node={node.label}
      data-node-id={node.id}
      data-node-key={key}
      data-size={shape.sized ? (shape.expanded ? "expanded" : "collapsed") : undefined}
      data-selected={selected || undefined}
      data-bypassed={off || undefined}
      data-recording-call={recordings.length === 1 ? node.id : undefined}
      data-recording-setting={recordings.length === 1 ? recordings[0].slot : undefined}
      data-recording-title={recordings.length === 1 ? titled : undefined}
    >
      {colour && <div data-colour-bar aria-hidden className="absolute inset-x-0 top-0 h-[3px] rounded-t-md" style={{ background: colour }} />}
      <Head node={node} module={module} shape={shape} titled={titled} pack={pack} perNote={perNote} session={session} revision={revision} editable={editable} effects={effects} mixing={!!mixing} />
      {node.voices && !shape.pill && <VoicesLine words={node.voices} />}
      {node.reads_key && !shape.pill && <KeyLine />}
      {node.plugin && <PluginRow node={node.plugin} title={titled} session={session} />}
      {shape.pill && node.inputs.map((i) => <Dot key={i.id} id={i.id} kind={i.kind} filled={wired.has(`${node.id}<${i.id}`)} />)}
      {mixing ? (
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
      ) : (
        !shape.pill && (
          <div data-node-body className={clsx("flex-1 min-h-0 flex flex-col min-w-0", off && "opacity-50")}>
            {(shape.conns.length > 0 || shape.fills.length > 0) && (
              <div style={{ paddingTop: PAD, paddingBottom: PAD }}>
                {shape.conns.map((c) =>
                  c.t === "listens" ? (
                    <ListensRow key="listens" node={node} filled={false} editable={editable} />
                  ) : (
                    <ConnRow key={c.port.id} node={node} input={c.port} filled={wired.has(`${node.id}<${c.port.id}`)} session={session} revision={revision} editable={editable} />
                  ),
                )}
                {shape.fills.map((id) => (
                  <div key={`fill:${id}`} style={{ height: ROW_H }} />
                ))}
              </div>
            )}
            {shape.views.length > 0 && (
              <div data-views className={clsx("grid", shape.views.length > 1 && shape.sized && "grid-cols-2")} style={{ height: shape.sized ? shape.viewH : undefined }}>
                {shape.views.map((v) => (
                  <ViewRow key={`view:${v.name}`} view={v} height={shape.viewH} node={node} module={module} session={session} revision={revision} editable={editable} locked={why} owner={titled} />
                ))}
              </div>
            )}
            {shape.expanded ? (
              <Columns columns={shape.columns} lit={lit} render={value} onPick={(group) => useExpanded.getState().light(key, group)} />
            ) : (
              shape.values.length > 0 && (
                <div data-values style={{ paddingTop: PAD, paddingBottom: PAD }} className="px-2">
                  {shape.sized
                    ? gridRows(shape.values).map((row, i) => (
                        <div key={i} className={clsx("grid gap-2", row.length > 1 && "grid-cols-2")}>
                          {row.map(value)}
                        </div>
                      ))
                    : shape.values.map(value)}
                </div>
              )
            )}
            {shape.expanded && node.code && (
              // The whole call, wrapped to the node's width (CARD-41).
              <div data-code-line data-code className="px-3 border-t border-line font-mono text-xs text-fg-muted whitespace-pre-wrap break-words flex items-center" style={{ height: shape.code - (shape.expanded && !!pack && shape.w <= NODE_W ? PACK_H : 0) }} title={node.code}>
                {node.code}
              </div>
            )}
            {shape.expanded && !!pack && shape.w <= NODE_W && (
              <div data-pack className="px-3 flex items-center text-xs text-fg-faint" style={{ height: PACK_H }}>
                {pack}
              </div>
            )}
          </div>
        )
      )}
      {node.outputs.map((o, j) => (
        <OutputPortView
          key={o.id}
          node={node}
          output={o}
          y={shape.outs.get(o.id) ?? 0}
          // Only a connection's row (or an output's own) has room for its label beside it; the header's and the mixer's don't.
          labelled={!shape.pill && !mixing && (j >= shape.conns.length ? shape.fills.includes(o.id) : shape.conns[j].t === "input" && !(shape.conns[j] as { port: InputPort }).port.inlet && node.listens?.port !== (shape.conns[j] as { port: InputPort }).port.id)}
          filled={wired.has(`${node.id}>${o.id}`)}
          editable={editable}
          onCable={onCable}
        />
      ))}
    </div>
  );
}

const valueKey = (v: Value) => (v.t === "setting" ? `setting:${v.x.id}` : v.t === "view" ? `view:${v.v.name}` : `${v.t}:${v.port.id}`);

/** An expanded node's columns (DV-12): side by side, 176 px each, wrapping after four; a lit one in the accent's wash (DV-18). */
function Columns({ columns, lit, render, onPick }: { columns: Column[]; lit: string | null; render: (v: Value) => React.ReactNode; onPick: (group: string) => void }) {
  return (
    <div className="grid border-t border-line" style={{ gridTemplateColumns: `repeat(${Math.min(columns.length, 4)}, ${COL_W}px)` }}>
      {columns.map((c, i) => (
        <section
          key={c.name}
          data-column={c.name}
          data-lit={lit === c.name || undefined}
          aria-label={c.name}
          className={clsx("min-w-0 flex flex-col px-2", i % 4 !== 0 && "border-l border-line", i >= 4 && "border-t border-line", lit === c.name && "bg-accent-wash")}
          style={{ paddingBottom: PAD }}
          // A value picked here lights its column, and so marks the dot whose `opens` names it (DV-18).
          onPointerDownCapture={(e) => (e.target as HTMLElement).closest("[data-knob], [data-setting]") && lit !== c.name && onPick(c.name)}
          onFocusCapture={(e) => (e.target as HTMLElement).closest("[data-knob], [data-setting]") && lit !== c.name && onPick(c.name)}
        >
          <div className="shrink-0 px-1 flex items-end pb-1 text-xs font-semibold text-fg-muted truncate" style={{ height: GROUP_H }}>
            {c.name}
          </div>
          <div style={{ paddingTop: PAD }}>{c.items.map(render)}</div>
        </section>
      ))}
    </div>
  );
}

/**
 * A node's header (DV-12): ⏻ where bypass works, its label (wrapped, never cut; its pack in its
 * tooltip), +N when it hides values, "per note" inside a voice; expanded, its pack and Presets; then
 * + (an effect after it), ⋯ and ⤢. A click selects the node; Enter expands it (DV-15).
 */
function Head({
  node,
  module,
  shape,
  titled,
  pack,
  perNote,
  session,
  revision,
  editable,
  effects,
  mixing,
}: {
  node: ViewNode;
  module: string;
  shape: Shape;
  titled: string;
  pack: string | null;
  perNote: boolean;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  mixing: boolean;
}) {
  const key = node.key ?? node.id;
  const off = !!node.bypassed;
  const where = node.file ? `${node.file}:${node.line}` : null;
  const tip = [titled !== node.label ? node.label : null, pack, where].filter(Boolean).join(" · ");
  const sized = shape.sized;
  return (
    <div
      data-node-head
      tabIndex={0}
      aria-label={sized ? `${titled}, ${shape.expanded ? "expanded" : "collapsed"}` : titled}
      aria-expanded={sized ? shape.expanded : undefined}
      className={clsx(
        "shrink-0 pl-3 pr-1 flex items-center gap-0.5 min-w-0 rounded-t-md outline-none focus-visible:bg-hover cursor-default",
        !shape.pill && "border-b border-line",
      )}
      style={{ height: shape.head }}
      onPointerDown={(e) => !(e.target as HTMLElement).closest("button") && useExpanded.getState().select(key)}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return;
        if (e.key === "Enter" && sized) {
          e.preventDefault();
          useExpanded.getState().toggle(key);
        } else if (e.key === " ") {
          e.preventDefault();
          useExpanded.getState().select(key);
        }
      }}
      onFocus={(e) => e.target === e.currentTarget && useExpanded.getState().select(key)}
    >
      {bypassable(node) && (
        <button
          data-bypass={node.id}
          aria-pressed={!off}
          aria-label={off ? `Turn ${titled} on` : `Turn ${titled} off`}
          title={off ? `Turn ${titled} back on` : `Turn ${titled} off (keeps its settings)`}
          disabled={!editable}
          className={clsx(
            "shrink-0 size-5 -ml-1.5 mr-1 grid place-items-center rounded-sm hover:bg-hover disabled:text-fg-off cursor-pointer transition-colors duration-80 ease-linear",
            off ? "text-fg-faint" : "text-fg-muted hover:text-fg",
          )}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => void turnEffect(node, titled, !off)}
        >
          <Power size={12} absoluteStrokeWidth strokeWidth={1.5} />
        </button>
      )}
      <NodeIcon node={node} />
      <div className="min-w-0 flex-1 flex items-center gap-1.5">
        {/* Its label (an effect's descriptor says it): wrapped, never cut (DV-12); the code's name and its pack in the tooltip. */}
        <div
          data-node-title
          className={clsx(
            "min-w-[64px] text-sm leading-4 break-words line-clamp-3",
            off ? "font-semibold text-fg-muted" : node.kind === "call" || node.kind === "lane" || node.kind === "voice" ? "font-semibold text-fg" : "font-medium text-fg-muted",
          )}
          title={tip || undefined}
        >
          {off ? `${titled} · Off` : titled}
        </div>
        {perNote && (
          <span data-per-note className="shrink-0 text-xs text-fg-faint" title="It plays inside the instrument's voices, once for each note">
            per note
          </span>
        )}
      </div>
      {/* Its pack, when the node is wide enough to keep the label's room; it is in the label's tooltip anyway. */}
      {shape.expanded && pack && shape.w > NODE_W && (
        <span data-pack className="shrink-0 px-1 text-xs text-fg-faint">
          {pack}
        </span>
      )}
      {sized && !shape.expanded && shape.hidden > 0 && <More node={node} hides={shape.hides} hidden={shape.hidden} titled={titled} />}
      {!mixing && <HeadTools node={node} module={module} titled={titled} session={session} revision={revision} editable={editable} effects={effects} mixing={mixing} expanded={shape.expanded} />}
      {sized && (
        <button
          data-expand={node.id}
          aria-label={shape.expanded ? `Collapse ${titled}` : `Expand ${titled}`}
          aria-expanded={shape.expanded}
          title={shape.expanded ? `Collapse ${titled}: this window only, the song is unchanged` : `Expand ${titled}: every value, its pictures and its code (this window only)`}
          className="shrink-0 size-5 grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => useExpanded.getState().toggle(node.key ?? node.id)}
        >
          {shape.expanded ? <Minimize2 size={12} absoluteStrokeWidth strokeWidth={1.5} /> : <Maximize2 size={12} absoluteStrokeWidth strokeWidth={1.5} />}
        </button>
      )}
    </div>
  );
}

/**
 * +N (DV-16): how many values a collapsed node hides, standing for each of them. It lights when an
 * agent changes one (AGENT-5); an agent's pointer at one shows on it (AGENT-6); a click expands the node.
 */
function More({ node, hides, hidden, titled }: { node: ViewNode; hides: string[]; hidden: number; titled: string }) {
  const glows = useNotice((s) => hides.some((id) => !!s.glow?.knobs.has(id)));
  return (
    <button
      data-more
      data-hides={hides.join(" ") || undefined}
      data-glow={glows || undefined}
      className={clsx(
        "shrink-0 h-5 px-1 rounded-sm text-xs font-medium tabular-nums cursor-pointer transition-colors duration-80 ease-linear",
        glows ? "bg-accent-soft text-accent-strong" : "text-fg-muted hover:text-fg hover:bg-hover",
      )}
      title={`${hidden} more ${hidden === 1 ? "value" : "values"}: expand ${titled} to see ${hidden === 1 ? "it" : "them"}`}
      aria-label={`Expand ${titled}: ${hidden} more ${hidden === 1 ? "value" : "values"}`}
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => useExpanded.getState().expand(node.key ?? node.id)}
    >
      +{hidden}
    </button>
  );
}

/**
 * A node's tools, in its header: Presets (FMT-22) when expanded, and ⋯ (Remove, Move, Make it a kit,
 * Show in code, Copy its id), each with its popover. An effect goes in from + on a wire (CARD-36),
 * never from a node's header.
 */
function HeadTools({
  node,
  module,
  titled,
  session,
  revision,
  editable,
  effects,
  mixing,
  expanded,
}: {
  node: ViewNode;
  module: string;
  titled: string;
  session: number;
  revision: number;
  editable: boolean;
  effects: Effect[];
  /** The Song's compact mixer node (MX-6), which has no Presets. */
  mixing: boolean;
  expanded: boolean;
}) {
  const toast = useStore((s) => s.toast);
  // The ⋯ menu, open from its button. A node kept across an edit may show another call now: its menu closes then.
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  const [presets, setPresets] = useState<HTMLElement | null>(null);
  const call = `${node.id} ${node.label}`;
  useEffect(() => {
    setMenu(null);
    setPresets(null);
  }, [call]);
  const why = useContext(Locked);
  /** A structural edit: said in words, with its Undo (taste.md §2.14, ST-5); a refusal says why. */
  const edited = (p: Promise<{ revision?: number; step?: number }>, words: string) =>
    p.then((c) => toast(words, undoAction(session, c.step))).catch((error) => toast(error instanceof Error ? error.message : String(error)));
  const sound = !mixing && hasPresets(node, effects);
  const button = "shrink-0 size-5 grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover disabled:text-fg-off disabled:pointer-events-none cursor-pointer transition-colors duration-80 ease-linear";
  return (
    <>
      {expanded && sound && (
        <button
          className={clsx(
            "shrink-0 h-5 px-1.5 rounded-sm text-xs font-medium text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear",
            presets && "bg-accent-soft text-accent-strong",
          )}
          data-presets={node.id}
          title={`Presets for ${titled}: load one, or save these settings`}
          aria-haspopup="dialog"
          aria-expanded={!!presets}
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            const b = e.currentTarget;
            setPresets((p) => (p ? null : b));
          }}
        >
          Presets
        </button>
      )}
      {node.kind === "call" && (
        <button
          className={clsx(button, menu && "bg-accent-soft text-accent-strong")}
          data-node-menu={node.id}
          data-effect-menu={node.effect ? node.id : undefined}
          title={`More for ${titled}`}
          aria-label={`${titled} menu`}
          aria-haspopup="menu"
          aria-expanded={!!menu}
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            const b = e.currentTarget;
            setMenu((m) => (m ? null : b));
          }}
        >
          <Ellipsis size={14} />
        </button>
      )}
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
      {menu && (
        <NodeMenu
          node={node}
          module={module}
          title={titled}
          anchor={menu}
          locked={why ?? (editable ? null : "This song can't be edited here")}
          onRemove={() => void edited(graphApi.removeEffect(session, revision, node.id, { name: `removing ${titled}`, structural: true }), `Removed ${titled}`)}
          onClose={() => setMenu(null)}
        />
      )}
    </>
  );
}

/**
 * What a node is called: a call by its component's label ("LFO", "Reverb"); a track's signal by the
 * track; a lane or an output by the rule controls follow (`cutoff` → Cutoff).
 */
export function titleOf(node: ViewNode, effects: Effect[]) {
  return node.kind === "import"
    ? importTitle(node.label)
    : node.kind === "lane" || node.kind === "export"
      ? controlLabel({ label: node.label })
      : (node.effect ?? node.title ?? effects.find((e) => e.component === node.component)?.label ?? node.label);
}

/**
 * How an instrument plays its notes (RENDER-16), under its name: its first sentence ("Plays one
 * note at a time."), and all of it in the tooltip. Read-only: its code sets it.
 */
export function VoicesLine({ words }: { words: string }) {
  const first = words.split(/(?<=\.) /)[0];
  return (
    <div data-testid="voices" className="shrink-0 px-3 flex items-center text-xs text-fg-muted min-w-0 border-b border-line" style={{ height: LINE_H }} title={words} aria-label={words}>
      <span className="truncate">{first}</span>
    </div>
  );
}

/**
 * The key a sound follows (VOX-26), under its name: "Key: A minor", read from the timeline (NOTE-2);
 * with none, "Every note allowed" and Set the key, which opens the key field in the top bar.
 */
export function KeyLine() {
  const key = useStore((s) => s.graph?.timeline.key ?? null);
  return (
    <div
      data-testid="card-key"
      className="shrink-0 px-3 flex items-center gap-2 text-xs text-fg-muted min-w-0 border-b border-line"
      style={{ height: LINE_H }}
      title={key ? `It follows the song's key, ${key.name}` : "No key set: every note is allowed. Set the key to choose the notes it moves to"}
    >
      <span className="truncate">{key ? `Key: ${key.name}` : "Every note allowed"}</span>
      {!key && (
        <button
          data-testid="card-set-key"
          className="ml-auto shrink-0 font-medium text-accent hover:text-accent-strong cursor-pointer"
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => useKeyAsk.getState().ask()}
        >
          Set the key
        </button>
      )}
    </div>
  );
}

/**
 * A plugin's own window and its latches (HOST-7, MON-6), a row under its node's header: Open
 * window or Close window, and what a gesture there latched, with Re-enable, which gives it back
 * to its lane or wire.
 */
export function PluginRow({ node, title, session }: { node: string; title: string; session: number }) {
  const toast = useStore((s) => s.toast);
  const box = useStore((s) => s.graph?.plugins?.[node]);
  const [busy, setBusy] = useState(false);
  const said = (p: Promise<unknown>) => void p.catch((error) => toast(error instanceof Error ? error.message : String(error)));
  const open = box?.window === "open";
  const latched = box?.latched ?? [];
  return (
    <div className="shrink-0 pl-1 pr-3 flex items-center gap-2 border-b border-line text-xs min-w-0" style={{ height: PLUGIN_H }} data-plugin={node}>
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
            title={`${latched.map((l) => l.name).join(", ")}: set in the window, so ${latched.length === 1 ? "it doesn't" : "they don't"} follow ${latched.length === 1 ? "its" : "their"} lane or wire now. Only while this song is open, never in an export.`}
          >
            {latched.length === 1 ? "Latched" : `${latched.length} latched`}
          </span>
          <button
            className="shrink-0 font-medium text-accent hover:text-accent-strong cursor-pointer"
            data-reenable
            title={`Follow the lane or wire again: ${latched.map((l) => l.name).join(", ")}`}
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
 * A node's ⋯ menu (the graph note §3): Remove, Move left and Move right, Make it a kit on the drum
 * machine (KIT-24), Show in code and Copy its id for `m2 add-effect`. A row that can't run here is
 * greyed, with why in its tooltip.
 */
function NodeMenu({
  node,
  module,
  title,
  anchor,
  locked,
  onRemove,
  onClose,
}: {
  node: ViewNode;
  module: string;
  title: string;
  anchor: HTMLElement;
  locked: string | null;
  onRemove: () => void;
  onClose: () => void;
}) {
  const toast = useStore((s) => s.toast);
  const machine = useStore((s) => node.component === "@m2/drums:drum-kit" && !!s.graph?.drum_machines?.includes(module));
  const keeps = useStore((s) => s.graph?.make_kit_keeps ?? "");
  // Where Remove can't take it out, the view says why (CARD-36).
  const remove = locked ?? (node.kind === "call" && !node.remove ? null : (node.remove ?? `${title} isn't an effect: change it in code.`));
  return (
    <Popover anchor={anchor} onClose={onClose} align="end" role="menu" aria-label={`${title} menu`} data-testid="effect-menu" className="w-[240px]">
      <MenuRow
        icon={<Trash2 size={14} />}
        disabled={!!remove}
        title={remove ?? `Remove ${title}, keeping what it was applied to`}
        aria-label={`Remove ${node.effect ?? title}`}
        data-testid="node-remove"
        onClick={() => {
          onClose();
          onRemove();
        }}
      >
        Remove
      </MenuRow>
      {node.menu &&
        ([true, false] as const).map((earlier) => {
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
      {machine && (
        <MenuRow
          icon={<Drum size={14} />}
          disabled={!!locked}
          data-make-kit={module}
          title={locked ?? `Make it a kit: each drum on a track of its own. ${keeps}`}
          onClick={() => {
            onClose();
            void makeKit(module);
          }}
        >
          Make it a kit
        </MenuRow>
      )}
      <div role="separator" className="my-1 h-px bg-line" />
      <MenuRow
        icon={<Code size={14} />}
        disabled={!node.file}
        data-testid="node-code"
        title={node.file ? `${node.file}, line ${node.line}` : "It isn't written in a file"}
        onClick={() => {
          onClose();
          if (!node.file) return;
          useShell.getState().set({ codeAt: { file: node.file, line: node.line ?? 1 } });
          useShell.getState().setView("code");
        }}
      >
        Show in code
      </MenuRow>
      <MenuRow
        icon={<Copy size={14} />}
        data-testid="node-copy-id"
        title={`Its id, ${node.id}: m2 add-effect puts an effect after it`}
        onClick={() => {
          onClose();
          void navigator.clipboard
            ?.writeText(node.id)
            .then(() => toast(`Copied ${node.id}`))
            .catch(() => toast(`Its id: ${node.id}`));
        }}
      >
        Copy its id
      </MenuRow>
    </Popover>
  );
}

/** An output on the node's right edge, at height `y`: a wire starts at a named one. */
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
      ? `To connect ${node.label} to a value, name it in code first: const my${last[0].toUpperCase()}${last.slice(1)} = ${node.label}(…)`
      : `To connect ${node.label} to a value, name it with a const in code first.`;
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
          title={editable ? `Drag onto a value to connect ${node.outlet}` : (why ?? "")}
          disabled={!editable}
          tabIndex={-1}
          onPointerDown={(e) => onCable(node, output, e)}
        >
          {/* Where a wire starts: filled, with a ring that says it can be dragged. */}
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

/**
 * A connection's row (DV-12), its input's shape on the node's left edge: a sound or notes coming in,
 * a value a wire drives ("← wobble", with ✕ to disconnect), or what a "Listens to" row stands for.
 */
function ConnRow({ node, input, filled, session, revision, editable }: { node: ViewNode; input: InputPort; filled: boolean; session: number; revision: number; editable: boolean }) {
  const toast = useStore((s) => s.toast);
  const tracks = useContext(Tracks);
  // A sound or notes coming in is a connection however it is written, a const's name too: no ✕,
  // since rewiring a sound is for code (the graph note's G3).
  const inlet = input.inlet && input.kind === "control" ? node.inlets.find((i) => i.id === input.inlet) : undefined;
  if (inlet) {
    // The bar comes back in its place: that says it, so no toast does.
    const disconnect = () =>
      graphApi.disconnect(session, revision, inlet.id, { name: `disconnecting ${inlet.name}` }).catch((error) => toast(error instanceof Error ? error.message : String(error)));
    return (
      <div className="relative pl-4 pr-3 flex items-center gap-1.5 text-xs min-w-0" style={{ height: ROW_H }} data-inlet={inlet.label}>
        <Dot id={input.id} kind={input.kind} filled />
        <span className="text-fg-muted truncate">{controlLabel(inlet, tracks)}</span>
        {/* Its home is a wire (HOME-1): what drives it, in place of a bar. */}
        <span className="text-fg truncate" title={`Driven by ${inlet.name}.`}>
          ← {inlet.name}
        </span>
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
  // An input that listens (a duck's side, a key): what it hears, and a list of the tracks it could (CARD-35).
  if (node.listens && node.listens.port === input.id) return <ListensRow node={node} input={input} filled={filled} editable={editable} />;
  return (
    <div className="relative pl-4 pr-3 flex items-center gap-2 text-xs min-w-0" style={{ height: ROW_H }} data-input={input.label}>
      <Dot id={input.id} kind={input.kind} filled={filled} />
      <span className="text-fg-muted truncate">{controlLabel(input, tracks)}</span>
    </div>
  );
}

/**
 * A value (DV-14): a bar, or a menu or a switch, for one the node can set; a row naming its source
 * for one a lane or the code drives; a setting (DV-8) or a view (DV-19).
 */
function ValueRow({
  v,
  node,
  owner,
  module,
  knobKey,
  session,
  revision,
  editable,
  expanded,
}: {
  v: Value;
  node: ViewNode;
  owner: string;
  module: string;
  knobKey: (id: string) => string;
  session: number;
  revision: number;
  editable: boolean;
  expanded: boolean;
}) {
  const tracks = useContext(Tracks);
  const why = useContext(Locked);
  if (v.t === "setting")
    return <SettingRow x={v.x} owner={owner} card={owner} call={node.id} session={session} revision={revision} editable={editable} locked={why} tracks={tracks} ided />;
  if (v.t === "view") return <ViewRow view={v.v} node={node} module={module} session={session} revision={revision} editable={editable} locked={why} owner={owner} height={v.v.kind === "picture" ? 132 : undefined} />;
  const input = v.port;
  if (v.t === "knob") {
    const knob = node.knobs.find((k) => k.id === input.knob);
    if (!knob) return null;
    // Keyed by the value alone: a save that breaks the code mid-drag must not remount it, which
    // would cancel the drag (WRITE-22: the server keeps it, and a release then is saved once the
    // code compiles); nor may one that renumbers it (see `held`).
    const key = knobKey(knob.id);
    if (picks(knob))
      return <PickControl key={key} knob={knob} label={controlLabel(input, tracks)} owner={owner} session={session} revision={revision} editable={editable} expanded={expanded} />;
    return <KnobControl key={key} mountKey={key} nodeKey={node.key ?? node.id} knob={knob} label={controlLabel(input, tracks)} owner={owner} session={session} revision={revision} editable={editable} />;
  }
  if (v.t === "expr") {
    const source = input.readonly != null ? node.readonly[input.readonly] : undefined;
    if (!source) return null;
    return (
      <div
        className="relative px-1 py-1.5 min-w-0 flex flex-col justify-center"
        style={{ height: EXPR_H }}
        data-expression={source.label}
        title={`${source.reason}${source.file ? `\n${source.file}:${source.line}` : ""}\n${source.expression}`}
      >
        <div className="flex items-center justify-between gap-2 text-xs">
          <span className="text-fg-muted truncate">{controlLabel(source, tracks)}</span>
          <span className="shrink-0 text-fg-faint">edit in code</span>
        </div>
        <div data-code className="font-mono text-xs text-fg truncate">
          {source.expression}
        </div>
      </div>
    );
  }
  return <LaneRow node={node} input={input} editable={editable} />;
}

/**
 * A value an automation lane drives (DV-14): its home is the lane (HOME-1), so it is a row naming
 * the lane, and its menu shows the lane or takes the automation out (KN-2).
 */
function LaneRow({ node, input, editable }: { node: ViewNode; input: InputPort; editable: boolean }) {
  const tracks = useContext(Tracks);
  const why = useContext(Locked);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const driven = input.lane;
  const label = controlLabel(input, tracks);
  const graph = useStore((s) => s.graph);
  const lane = driven && graph ? findLane(graph.timeline, driven.module, driven.lane) : undefined;
  const laneName = lane ? laneTitle(lane) : label;
  return (
    <div
      className="relative px-1 flex items-center gap-2 text-xs min-w-0"
      style={{ height: VALUE_H }}
      data-input={input.label}
      data-lane-input={driven ? `${driven.module}.${driven.lane}` : undefined}
      data-node-of={node.id}
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
      <span className="text-fg-muted truncate">{label}</span>
      {driven && (
        <span className="ml-auto min-w-0 flex items-center gap-1.5 text-fg truncate">
          <span className="truncate">← {laneName} lane</span>
          <span aria-hidden className="shrink-0 size-[5px] rounded-full bg-accent" />
        </span>
      )}
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
 * An input that listens (CARD-35): "Listens to: Kick ▾", a duck's side or a compressor's or a
 * gate's key, whatever the effect. Its list has the song's tracks but the one the effect works on,
 * and None when the call may leave the input out; one picked is written in the call. With the
 * input left out the row has no port and says None. Where it can't change (an effect in a track's
 * file), the button is greyed and its tooltip says why.
 */
export function ListensRow({ node, input, filled, editable }: { node: ViewNode; input?: InputPort; filled: boolean; editable: boolean }) {
  const tracks = useContext(Tracks);
  const why = useContext(Locked);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const l = node.listens!;
  const name = (key: string) => l.choices.find(([k]) => k === key)?.[1] ?? tracks(key) ?? key;
  const now = l.track ? name(l.track) : l.heard ? "another sound" : l.nothing;
  const off = why ?? (editable ? null : "This song can't be edited here") ?? l.refused ?? null;
  const what = node.title ?? "It";
  const title = l.label
    ? l.heard
      ? `${l.label}: ${now}. Choose another track, or None`
      : `${l.label}: no track yet. Choose one`
    : l.heard
      ? `${what} listens to ${now}. Choose another track`
      : `${what} listens to no other track. Choose one`;
  return (
    <div className="relative pl-4 pr-3 flex items-center gap-1.5 text-xs min-w-0" style={{ height: ROW_H }} data-input={input?.label ?? l.input} data-listens>
      {input && <Dot id={input.id} kind={input.kind} filled={filled} />}
      <span className="shrink-0 text-fg-muted">{l.label ?? "Listens to"}:</span>
      <button
        data-testid="listens-to"
        className={clsx(
          "min-w-0 h-ctl-xs px-1 -mx-1 flex items-center gap-1 rounded-sm text-xs font-medium transition-colors duration-80 ease-linear",
          off ? "text-fg-muted cursor-default" : "text-fg hover:bg-hover cursor-pointer",
          !l.heard && !off && "text-fg-muted",
          anchor && "bg-accent-soft text-accent-strong",
        )}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        aria-disabled={!!off || undefined}
        title={off ?? title}
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
        <Popover anchor={anchor} onClose={() => setAnchor(null)} role="menu" aria-label={l.label ?? "Listens to"} data-testid="listens-menu" className="w-[200px]">
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
          {l.optional && (
            <MenuRow
              key="none"
              data-track=""
              icon={<Check size={14} className={l.heard ? "invisible" : undefined} />}
              onClick={() => {
                setAnchor(null);
                if (l.given) void listenTo(node, null, l.nothing === "None" ? "nothing" : l.nothing.toLowerCase());
              }}
            >
              {l.nothing}
            </MenuRow>
          )}
        </Popover>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- values

/**
 * The bar whose own keys or menu just wrote it while it had the keyboard's focus: the first write of
 * a value the code left out gives it a new id, so it mounts again, and only that remount takes the
 * focus back (DV-15). Kept for the write's round trip, and dropped when the bar is left (review F1).
 */
let focused: { knob: string; el: Element; at: number } | null = null;
/** How long a write's remount may take to take the focus back. */
const REFOCUS_MS = 4000;

/** A bar over an owned source value: drag to hear it, release to write it (DV-14). */
function KnobControl({
  mountKey,
  nodeKey,
  knob,
  label,
  owner,
  session,
  revision,
  editable,
}: {
  /** The React key it is mounted with, kept while it is dragged (see `held`). */
  mountKey: string;
  nodeKey: string;
  knob: KnobInfo;
  /** Its name on screen ("Feedback", "Hats volume"). */
  label: string;
  /** Its node's ("Delay"). */
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
    // Its undo step's name: the node and the control ("Delay feedback"), or a channel's own ("Hats volume").
    name: knobStep(owner, knob, label),
    label,
    hold: (token, on) => (on ? held.set(token, { knob: mountKey }) : held.delete(token)),
  });
  const value = g.value;
  // Changed elsewhere just now (AGENT-5): outlined until the next click or key press.
  const glows = useGlows("knobs", knob.id);
  // The bar's menu (right-click), anchored to it: its left edge, below it (or above it, the room).
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  const readout = useRef<HTMLSpanElement>(null);
  // A value being typed in place of the number (taste.md §2.14): its text, the number's width, and
  // whether Enter found it unreadable. The ref says whether it is still open, for a late blur.
  const [typing, setTyping] = useState<{ text: string; width: number; bad: boolean } | null>(null);
  const typingOpen = useRef(false);

  // Its focus kept across the remount its own first write makes (see `focused`): by its node's key
  // and its control, so no other bar, nor another copy of it, takes it (review F9).
  const self = `${nodeKey}|${knob.slot ?? knob.label}`;
  useEffect(() => {
    const el = bar.current;
    const was = focused;
    if (el && was?.knob === self && was.el !== el && !was.el.isConnected && Date.now() - was.at < REFOCUS_MS && (!document.activeElement || document.activeElement === document.body)) {
      focused = null;
      el.focus({ preventScroll: true });
    }
    // Left for something else (not taken away by its remount): nothing to take back.
    const left = () => queueMicrotask(() => el?.isConnected && focused?.el === el && (focused = null));
    el?.addEventListener("blur", left);
    return () => el?.removeEventListener("blur", left);
  }, [self]);
  /** This bar writes while it has the focus: its remount may take the focus back. */
  const mine = () => {
    if (bar.current && bar.current === document.activeElement) focused = { knob: self, el: bar.current, at: Date.now() };
  };
  /** A move of the bar to `u` (0..1): heard while dragged; on its own (a double-click, the menu's reset) one whole gesture. */
  const change = (u: number) => {
    mine();
    g.change(fromUnit(g.descriptor, u));
  };
  const automate = () => {
    setMenu(null);
    automateKnob(session, revision, knob.id);
  };
  /** `n` steps from where it is (taste.md §5.2), written as one edit half a second after the last. */
  const step = (n: number) => {
    mine();
    g.step((v, k) => fromUnit(k, Math.min(1, Math.max(0, toUnit(k, stepValue(k, v, n))))));
  };
  /** Type a value: the number turns into a field of its size, its text selected. */
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
  const disabled = !editable && !g.dragging();
  // Filled from the middle when its span is centred on zero (a pan, a bipolar amount).
  const range = span(descriptor);
  const bipolar = range.lo < 0 && range.hi > 0 && Math.abs(range.lo + range.hi) <= 1e-9 * range.hi;
  const shared = knob.uses > 1 ? `Shared value: changes all ${knob.uses} uses. ` : "";
  // A control the call leaves out plays its default until its first edit writes it into the call (PKG-3).
  const written = knob.written ? `written as ${knob.literal}` : "the default: not written in the code yet, changing it writes it in";
  const how = `Drag (${mod.shift} for fine) · double-click to type a value · ${mod.alt}-click to reset · right-click to automate it`;
  const cabling = useContext(Cabling);
  // Its whole label, its value and its range, which a narrow bar may cut (review Z6).
  const said = `${label}: ${display(descriptor, value)}, from ${display(descriptor, range.lo)} to ${display(descriptor, range.hi)}`;
  const title = !editable ? `${why ?? "It can't be edited right now"}.` : `${shared}${how}\n${said}\nIn your code: ${knob.file}, line ${knob.line} (${written})\nIts id: ${knob.id}`;
  const unit = Math.min(1, Math.max(0, toUnit(descriptor, value)));
  const reset = descriptor.default != null ? Math.min(1, Math.max(0, toUnit(descriptor, descriptor.default))) : undefined;
  // The whole row drags it, its label and value too (taste.md §2.2).
  const grip = useKnobDrag({ value: unit, reset, disabled, onChange: change, onStart: () => g.start(), onEnd: g.end, onCancel: g.abandon, focus: () => bar.current?.focus({ preventScroll: true }) });

  const typingField = typing && (
    <span className="relative ml-auto shrink-0">
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
            // On to the next value (or back, with ⇧), as a form's fields go.
            e.preventDefault();
            if (!typed(typing.text)) return setTyping({ ...typing, bad: true });
            focusNext(bar.current, e.shiftKey);
          }
        }}
        // A click away writes what reads as a value, and drops what doesn't.
        onBlur={() => typingOpen.current && !typed(typing.text) && closeTyping()}
        onPointerDown={(e) => e.stopPropagation()}
        className={clsx("h-4 px-1 rounded-sm bg-raised border text-right text-xs text-fg outline-none", typing.bad ? "border-danger" : "border-accent")}
      />
      {typing.bad && (
        <span role="alert" className="absolute right-0 top-full mt-0.5 z-10 whitespace-nowrap text-xs text-danger">
          Type a value, like −6, 2k or 120 ms
        </span>
      )}
    </span>
  );
  const knobMenu = menu && (
    <KnobMenu
      knob={knob}
      label={label}
      value={value}
      at={menu}
      editable={editable}
      locked={why}
      onAutomate={automate}
      onReset={descriptor.default != null ? () => change(Math.min(1, Math.max(0, toUnit(descriptor, descriptor.default!)))) : undefined}
      onClose={() => setMenu(null)}
    />
  );
  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        if (!g.dragging()) setMenu({ x: r.left, y: r.bottom + 4, above: r.top - 4 });
      }}
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-no-cable={knob.cable_refused ? `${knob.cable_refused}.` : undefined}
      data-glow={glows || undefined}
      data-value={value}
      data-shared={knob.uses > 1 || undefined}
      data-default={!knob.written || undefined}
      aria-disabled={disabled}
      title={cabling && knob.cable_refused ? `${knob.cable_refused}.` : title}
      data-kind="number"
      {...grip.handlers}
      // A double-click anywhere on it types a value (taste.md §2.14).
      onDoubleClick={(e) => e.currentTarget.contains(e.target as Node) && !(e.target as HTMLElement).closest("input") && type()}
      className={clsx(
        "group/knob relative min-w-0 flex items-center rounded-sm transition-colors duration-80",
        !disabled && "cursor-ns-resize touch-none",
        glows && "ring-1 ring-accent",
        // While a wire is dragged, the bars it can land on: washed, and the one under it more.
        "[[data-cabling]_&[data-knob-id]:not([data-no-cable])]:bg-accent-wash [[data-cabling]_&[data-knob-id]:not([data-no-cable]):hover]:bg-accent-soft",
        // And one it can't: dimmed, its words on hover (DV-6).
        "[[data-cabling]_&[data-no-cable]]:opacity-40",
      )}
      style={{ height: VALUE_H }}
    >
      <Bar value={unit} bipolar={bipolar} disabled={disabled} readout={display(descriptor, value)} label={label} reset={reset} bar={bar} pointer={grip.pointer} dragging={!!grip.pointer} onChange={change} onStep={step} onType={type}>
        <span className="min-w-0 flex-1 truncate text-fg-muted">{label}</span>
        {knob.uses > 1 && <span className="shrink-0 text-fg-faint">×{knob.uses}</span>}
        {typingField || (
          <span
            ref={readout}
            className={clsx("shrink-0 tabular-nums", knob.written ? "text-fg-strong" : "text-fg-muted")}
            title={knob.written ? undefined : "Default. Not written in your code yet: changing it writes it in."}
          >
            {display(descriptor, value)}
          </span>
        )}
        <HomeMarker home={knob.home} label={label} />
      </Bar>
      {knobMenu}
    </div>
  );
}

/** A value's number becomes a new, empty control lane's fallback (HOME-2): the song sounds the same, and the timeline shows the lane to draw on. */
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
 * A value that is a menu or a switch (CARD-20): a choice control's names, a switch's off and on, or
 * a plugin's stepped parameter (CARD-6). A pick writes its option's value, a name, true or false, or
 * a plugin's step number, into the code: one click, one edit, one undo step, never a drag. On an
 * expanded node a choice of five or fewer is a segmented control (DV-14). It stays a knob to the
 * rest of the app: its id for the focus and the pointer, its glow, wire drops, and its menu's Automate.
 */
function PickControl({
  knob,
  label,
  owner,
  session,
  revision,
  editable,
  expanded,
}: {
  knob: PickKnob;
  label: string;
  owner: string;
  session: number;
  revision: number;
  editable: boolean;
  expanded: boolean;
}) {
  const toast = useStore((s) => s.toast);
  const why = useContext(Locked);
  const glows = useGlows("knobs", knob.id);
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const off = editable ? null : `${why ?? "It can't be edited right now"}.`;
  const shared = knob.uses > 1 ? `Shared value: changes all ${knob.uses} uses. ` : "";
  const written = knob.written ? `written as ${knob.literal}` : "the default: not written in the code yet, picking one writes it in";
  const how = `Click to ${knob.kind === "switch" ? "turn it on or off" : "choose"} · right-click to automate it`;
  const cabling = useContext(Cabling);
  const title = off ?? `${shared}${how}\nIn your code: ${knob.file}, line ${knob.line} (${written})\nIts id: ${knob.id}`;
  /**
   * A pick: its option's value written into the code, one undo step named "Wobble shape". Picked
   * while the window doesn't show its own last edit yet, it goes once it does (the gate).
   */
  const pick = (v: number | string | boolean) =>
    void graphApi.setValue(session, revision, knob.id, v, { name: knobStep(owner, knob, label) }).catch((error) => toast(words(error)));
  const segmented = expanded && knob.kind !== "switch" && (knob.options?.length ?? 0) <= 5;
  return (
    <div
      onContextMenu={(e) => {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        setMenu({ x: r.left, y: r.bottom + 4, above: r.top - 4 });
      }}
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-no-cable={knob.cable_refused ? `${knob.cable_refused}.` : undefined}
      data-glow={glows || undefined}
      data-value={JSON.stringify(knob.value)}
      data-shared={knob.uses > 1 || undefined}
      data-default={!knob.written || undefined}
      data-kind={knob.kind}
      aria-disabled={!!off}
      title={cabling && knob.cable_refused ? `${knob.cable_refused}.` : title}
      className={clsx(
        "group/knob relative min-w-0 rounded-sm transition-colors duration-80",
        segmented ? "py-1 flex flex-col justify-center gap-1" : "flex items-center gap-1.5 px-1.5",
        glows && "ring-1 ring-accent",
        "[[data-cabling]_&[data-knob-id]:not([data-no-cable])]:bg-accent-wash [[data-cabling]_&[data-knob-id]:not([data-no-cable]):hover]:bg-accent-soft",
        // And one it can't: dimmed, its words on hover (DV-6).
        "[[data-cabling]_&[data-no-cable]]:opacity-40",
      )}
      style={{ height: segmented ? segHeight((knob.options ?? []).map((o) => o.label)) : VALUE_H }}
    >
      <div className={clsx("min-w-0 text-xs text-fg-muted truncate", !segmented && "flex-1")}>{label}</div>
      {knob.kind === "switch" ? (
        <SwitchValue value={knob} label={label} off={off} onFlip={pick} />
      ) : segmented ? (
        // Its options wrap onto a second row rather than spill past the column (DV-12, DV-14).
        <div className="min-w-0 w-full [&>[role=radiogroup]]:ml-0 [&>[role=radiogroup]]:h-auto [&>[role=radiogroup]]:flex-wrap [&>[role=radiogroup]]:w-fit [&>[role=radiogroup]]:max-w-full">
          <Segmented value={knob} label={label} off={off} title={title} onPick={pick} />
        </div>
      ) : (
        <MenuValue value={knob} label={label} off={off} title={title} onPick={pick} />
      )}
      <HomeMarker home={knob.home} label={label} />
      {menu && (
        <KnobMenu
          knob={knob}
          label={label}
          value={knob.value}
          at={menu}
          editable={editable}
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

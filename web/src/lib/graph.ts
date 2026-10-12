// The graph view of a song-as-program project (the new core): types, API, layout.

import { token } from "./native";

export interface Origin { site: number; arg?: string[]; scale?: number }

export interface Knob {
  id: string;
  label: string;
  value: number;
  literal: string;
  unit: string | null;
  min: number;
  max: number;
  log: boolean;
  origin: Origin;
  uses: number;
  file: string;
  line: number;
}

export interface ViewNode {
  id: string;
  label: string;
  kind: "call" | "lane" | "time" | "import" | "value" | "export";
  file: string | null;
  line: number | null;
  knobs: Knob[];
  readonly: { label: string; expression: string; reason: string; file: string | null; line: number | null }[];
  /** The `const` this node's signal is bound to, if a cable can start here. */
  outlet: string | null;
  /** Inputs connected by a cable: `label ← name`. */
  inlets: { id: string; label: string; name: string; from: string }[];
  /** Whether its result is audio, which an effect can go after. */
  audio: boolean;
  /** The effect this node is, if it can be removed. */
  effect: string | null;
}

export interface Effect { module: string; name: string; description: string }

export interface ModuleView { name: string; track: boolean; nodes: ViewNode[]; edges: [string, string][] }

export interface GraphView { revision: number; modules: ModuleView[]; draft_errors: string[] }

/** Note starts and control point beats are relative to their clip's start. */
export interface NoteClip { start: number; length: number; notes: { pitch: number; start: number; length: number; velocity: number }[] }
export interface ControlClip { start: number; length: number; points: [number, number][] }
export interface AudioClip { start: number; length: number; asset: string }

export type TimelineLane =
  | { id: string; kind: "notes"; clips: NoteClip[] }
  | {
      id: string;
      kind: "control";
      unit?: string | null;
      default?: number | null;
      /** The range to draw values in: the unit's, or what the values span. */
      min: number;
      max: number;
      clips: ControlClip[];
    }
  | { id: string; kind: "audio"; channels: number; clips: AudioClip[] };

/** A module that is a track or declares lanes; `module` is its ModuleView name. */
export interface TimelineTrack { module: string; track: boolean; lanes: TimelineLane[] }

/** What the accepted revision plays, in beats. */
export interface TimelineView {
  beats_per_bar: number;
  tempo: { beat: number; bpm: number }[];
  /** Where the last clip ends, at least one bar. */
  end: number;
  tracks: TimelineTrack[];
}

/** Where playback is; `beat` counts with the accepted revision's tempo map. */
export interface GraphStatus { playing: boolean; seconds: number; beat: number }

/** The open project, and where playback was when it was read. */
export interface GraphState extends GraphStatus {
  session: number;
  name: string;
  dir: string;
  view: GraphView;
  timeline: TimelineView;
  /** Whether an edit made here can be undone or redone. */
  can_undo: boolean;
  can_redo: boolean;
  /** Effects the packages offer to put after audio. */
  effects: Effect[];
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function call<T = any>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api/graph${path}`, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json", ...auth },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error ?? `HTTP ${res.status}`);
  return data as T;
}

export const graphApi = {
  get: () => call<GraphState>(""),
  open: (path: string) => call<GraphState>("/open", { path }),
  close: () => call("/close", {}),
  status: () => call<GraphStatus>("/status"),
  /** Move playback to a beat, playing or not. */
  seek: (beat: number) => call("/seek", { beat }),
  preview: (session: number, revision: number, id: string, gesture: string, value: number) => call("/preview", { session, revision, id, gesture, value }),
  cancel: (session: number, id: string, gesture: string) => call("/preview/cancel", { session, id, gesture }),
  set: (session: number, revision: number, id: string, gesture: string, value: number, expect: number) =>
    call<{ file: string; before: string; after: string; revision: number }>("/set", { session, revision, id, gesture, value, expect }),
  connect: (session: number, revision: number, from: string, to: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/connect", { session, revision, from, to }),
  disconnect: (session: number, revision: number, id: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/disconnect", { session, revision, id }),
  addEffect: (session: number, revision: number, node: string, effect: Effect) =>
    call<{ file: string; before: string; after: string; revision: number }>("/effect/add", { session, revision, call: node, module: effect.module, name: effect.name }),
  removeEffect: (session: number, revision: number, node: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/effect/remove", { session, revision, call: node }),
  undo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/undo", { session }),
  redo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/redo", { session }),
  transport: (action: "play" | "stop" | "toggle" | "restart") => call("/transport", { action }),
};

// ---------------------------------------------------------------- knob values

/** Knob position 0..1 ↔ value, linear or logarithmic. */
export function toUnit(k: Knob, v: number) {
  if (k.max <= k.min) return 0.5;
  if (k.log && k.min > 0) return Math.log(v / k.min) / Math.log(k.max / k.min);
  return (v - k.min) / (k.max - k.min);
}

export function fromUnit(k: Knob, u: number) {
  const value = k.log && k.min > 0 ? k.min * Math.pow(k.max / k.min, u) : k.min + (k.max - k.min) * u;
  // Three significant figures: what gets written into the code.
  return Number(value.toPrecision(3));
}

export function display(k: Knob, v: number) {
  const n = (x: number, d = 2) => Number(x.toFixed(d)).toString();
  switch (k.unit) {
    case "hz":
      return v >= 1000 ? `${n(v / 1000)} kHz` : `${n(v, 0)} Hz`;
    case "db":
      return `${n(v, 1)} dB`;
    case "seconds":
      return v < 1 ? `${n(v * 1000, 0)} ms` : `${n(v)} s`;
    case "beats":
      return `${n(v, 3)} beats`;
    case "semitones":
      return `${n(v, 1)} st`;
    default:
      return n(v);
  }
}

// ---------------------------------------------------------------- layout

export const NODE_W = 216;
const GAP_X = 72;
const GAP_Y = 20;
const HEAD_H = 46;
const KNOB_ROW_H = 84;
const KNOBS_PER_ROW = 3;
const EXPRESSION_ROW_H = 64;
const INLET_ROW_H = 32;

export const nodeHeight = (n: ViewNode) =>
  HEAD_H + Math.ceil(n.knobs.length / KNOBS_PER_ROW) * KNOB_ROW_H + (n.knobs.length ? 8 : 0) + n.readonly.length * EXPRESSION_ROW_H + n.inlets.length * INLET_ROW_H;

export interface Placed { node: ViewNode; x: number; y: number; h: number }

/** Columns by depth in the signal flow (sources left, outputs right). */
export function layout(m: ModuleView): { placed: Map<string, Placed>; width: number; height: number } {
  const depth = new Map<string, number>(m.nodes.map((n) => [n.id, 0]));
  // Longest path from a source; the graph is acyclic (feedback is inside calls).
  for (let pass = 0; pass < m.nodes.length; pass++) {
    let changed = false;
    for (const [a, b] of m.edges) {
      const d = (depth.get(a) ?? 0) + 1;
      if (d > (depth.get(b) ?? 0)) {
        depth.set(b, d);
        changed = true;
      }
    }
    if (!changed) break;
  }
  // Outputs sit in the last column.
  const last = Math.max(0, ...depth.values());
  for (const n of m.nodes) if (n.kind === "export") depth.set(n.id, last);
  const columns: ViewNode[][] = [];
  for (const n of m.nodes) (columns[depth.get(n.id)!] ??= []).push(n);
  const placed = new Map<string, Placed>();
  let height = 0;
  columns.forEach((col, i) => {
    let y = 0;
    for (const n of col ?? []) {
      const h = nodeHeight(n);
      placed.set(n.id, { node: n, x: i * (NODE_W + GAP_X), y, h });
      y += h + GAP_Y;
    }
    height = Math.max(height, y - GAP_Y);
  });
  return { placed, width: columns.length * (NODE_W + GAP_X) - GAP_X, height };
}

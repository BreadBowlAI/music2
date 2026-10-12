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
  /** `voice`: the note a `poly` voice plays (its pitch, gate, …), beside the `poly` call. */
  kind: "call" | "lane" | "time" | "import" | "value" | "export" | "voice";
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
  /** Where signals and values come in, in the order the code writes them. */
  inputs: InputPort[];
  /** The signals it gives to other nodes, its own result first. */
  outputs: OutputPort[];
}

/** What flows through a port. */
export type PortKind = "audio" | "control" | "notes";

/** An input: set by a knob, connected by a cable, a value only code can change, or a signal from another node. */
export interface InputPort { id: string; label: string; kind: PortKind; knob?: string; inlet?: string; readonly?: number }

/** An output; `outlet` is where a cable can start (the result has a `const` name). */
export interface OutputPort { id: string; label: string; kind: PortKind; outlet: boolean }

/** A connection from an output of `from` to an input of `to` (node and port ids); `late`: a `feedback()` reads it a block late, closing a loop. */
export interface PortEdge { from: string; from_port: string; to: string; to_port: string; kind: PortKind; late?: boolean }

/** `args` are written after the audio when it is added: `lowpass(x, 1000)`. */
export interface Effect { module: string; name: string; description: string; category: string; args: string | null }

export interface ModuleView { name: string; track: boolean; nodes: ViewNode[]; edges: [string, string][]; port_edges: PortEdge[] }

export interface GraphView { revision: number; modules: ModuleView[]; draft_errors: string[] }

/** Note starts and control point beats are relative to their clip's start. */
export interface NoteClip { start: number; length: number; notes: Note[] }
/** `pitch` in semitones (60 is middle C); `velocity` 0..1. */
export interface Note { pitch: number; start: number; length: number; velocity: number }
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

/** A clip in the timeline: its module's name, its lane's id, its place among the lane's clips. */
export interface ClipRef { module: string; lane: string; index: number }

/** The notes clip a reference points at, if the timeline (still) has it. */
export function findNoteClip(timeline: TimelineView, ref: ClipRef): NoteClip | undefined {
  const lane = timeline.tracks.find((t) => t.module === ref.module)?.lanes.find((l) => l.id === ref.lane);
  return lane?.kind === "notes" ? lane.clips[ref.index] : undefined;
}

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
  /** Why the files on disk don't compile, if they don't: the draft's problems. */
  diagnostics: Problem[];
}

/** One reason the draft doesn't compile; `file` is project-relative (or a package's, `std/src/dsp.ts`), line and col 1-based. */
export interface Problem { file: string | null; line: number | null; col: number | null; message: string; module?: string }

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

/**
 * Hooks an open editor registers for app commands. `save` returns true if it handled
 * ⌘S; App's "save" command tries it before the old engine's save. `saveAll` saves every
 * edited file (the desktop app asks for it before it closes or replaces the project).
 * `duplicate` is the piano roll's ⌘D (in the desktop app the menu bar takes ⌘D before the
 * page sees it). `undo` is the code editor's, while it has the keyboard: the menu's Undo and
 * Redo reach its own history rather than the page's. `unsaved` counts the editor's files with
 * edits not yet saved, so closing the project can ask first.
 */
export const editorHooks: {
  save?: () => boolean;
  saveAll?: () => void;
  duplicate?: () => boolean;
  undo?: (redo: boolean) => boolean;
  unsaved?: () => number;
} = {};

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
  /** Rewrite a notes lane with every clip it is to have. Prefer `writeLane`, which queues. */
  lane: (session: number, revision: number, module: string, lane: string, clips: NoteClip[]) =>
    call<{ file: string; before: string; after: string; revision: number }>("/lane", { session, revision, module, lane, clips }),
  undo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/undo", { session }),
  redo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/redo", { session }),
  transport: (action: "play" | "stop" | "toggle" | "restart") => call("/transport", { action }),
  /** Tell the app which files the code editor has unsaved edits in, so it asks before closing the project. */
  unsaved: (session: number, files: string[]) => call("/unsaved", { session, files }),
};

// ---------------------------------------------------------------- lane writes

/** Lane writes of the open project, one at a time, and the newest revision one made. */
let lanes = { session: 0, revision: 0, pending: 0, queue: Promise.resolve() as Promise<unknown> };

/**
 * Rewrite `module`'s notes lane `lane` with every clip it is to have: one write, one undo step.
 * `base` is the revision those clips were read from (a graph's, or the one an earlier write
 * made): the server refuses the write if the lane changed since, so it never overwrites a
 * change the clips don't include.
 */
export function writeLane(graph: GraphState, module: string, lane: string, clips: NoteClip[], base: number) {
  if (lanes.session !== graph.session) lanes = { session: graph.session, revision: 0, pending: 0, queue: Promise.resolve() };
  const q = lanes;
  q.pending++;
  const write = q.queue
    .then(() => graphApi.lane(graph.session, base, module, lane, clips))
    .then((r) => ((q.revision = Math.max(q.revision, r.revision)), r))
    .finally(() => q.pending--);
  q.queue = write.catch(() => {});
  return write;
}

/** Whether a lane write is on its way, or made a revision `graph` doesn't show yet. */
export const laneWriting = (graph: GraphState) => lanes.session === graph.session && (lanes.pending > 0 || graph.view.revision < lanes.revision);

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
      // Rates below hearing (a chorus's 0.8 Hz) keep their decimals.
      return v >= 1000 ? `${n(v / 1000)} kHz` : `${n(v, v < 20 ? 2 : 0)} Hz`;
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

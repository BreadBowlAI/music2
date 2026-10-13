// The graph view of the open song project: types, API, layout.

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

/**
 * A module: the song (`song`) or a track. `key` is what the code calls it (`track("lead-vox", …)`):
 * the one address of the module, for selection, its tabs, its lanes and its data (ID-11). `label`
 * is what to call it (`{ name: "Lead Vox" }`, else the key): shown, never an address (ID-10).
 */
export interface ModuleView { key: string; label: string; track: boolean; nodes: ViewNode[]; edges: [string, string][]; port_edges: PortEdge[] }

export interface GraphView { revision: number; modules: ModuleView[]; draft_errors: string[] }

/** What to show for a lane or a module: its label, which defaults to its key. */
export const labelOf = (x: { key: string; label?: string | null }) => x.label || x.key;
/** What to show for a module: its label, and "Song" for the root module unless it has one. */
export const moduleName = (m: { key: string; label?: string | null }) => (m.key === "song" && (!m.label || m.label === "song") ? "Song" : labelOf(m));
/** The key, to show beside a name that doesn't already say it (`Lead Vox` · `lead-vox`, not `Bass` · `bass`); "" otherwise. */
export const keyNote = (name: string, key: string) => (name.toLowerCase() === key || key === "song" ? "" : key);

/**
 * A clip's id: unique in its lane, the clip's address with the lane's (FMT-11, FMT-12). A clip
 * without one, or whose id breaks the rule or is another clip's too, is anonymous: see `ClipRef`.
 * Note starts and control point beats are relative to their clip's start.
 */
export interface NoteClip { id?: string; start: number; length: number; notes: Note[] }
/** `pitch` in semitones (60 is middle C); `velocity` 0..1. */
export interface Note { pitch: number; start: number; length: number; velocity: number }
export interface ControlClip { id?: string; start: number; length: number; points: [number, number][] }
/** `asset` is a path under `assets/`; `hash` pins its bytes (FMT-15). */
export interface AudioClip {
  id?: string;
  start: number;
  length: number;
  asset: string;
  hash?: string;
  offset?: number;
  gain_db?: number;
  fade_in?: number;
  fade_out?: number;
  reverse?: boolean;
  pitch?: number;
}

/** A declared lane: `key` addresses it within its module (ID-4, ID-6), `label` is its `name`, or the key. */
export type TimelineLane =
  | { key: string; label: string; kind: "notes"; clips: NoteClip[] }
  | {
      key: string;
      label: string;
      kind: "control";
      unit?: string | null;
      default?: number | null;
      /** The range to draw values in: the unit's, or what the values span. */
      min: number;
      max: number;
      clips: ControlClip[];
    }
  | { key: string; label: string; kind: "audio"; channels: number; clips: AudioClip[] };

/** A module that is a track or declares lanes, by its key (as `ModuleView.key`), with its label. */
export interface TimelineTrack { module: string; label: string; track: boolean; lanes: TimelineLane[] }

/** A time signature from `beat` on: `beats` in a bar of `unit` notes (FMT-13). A beat is a quarter note. */
export interface Meter { beat: number; beats: number; unit: number }

/** What the accepted revision plays, in beats. */
export interface TimelineView {
  tempo: { beat: number; bpm: number }[];
  /** The time signatures; empty is 4/4 throughout. Bars are counted from them (`barAt`, `bars`). */
  meter: Meter[];
  /** Where the last clip ends, at least one bar. */
  end: number;
  tracks: TimelineTrack[];
}

// ---------------------------------------------------------------- clips by id

/** The ids readers accept (FMT-11). */
const CLIP_ID = /^[0-9a-z]{1,16}$/;
/** The lowercase Crockford base32 alphabet new ids are drawn from (no i, l, o or u). */
const CROCKFORD = "0123456789abcdefghjkmnpqrstvwxyz";

/** A new clip id: 8 random Crockford characters, as a builder outside a sandbox may draw it (FMT-11). */
export function newClipId(taken: Iterable<string | undefined> = []) {
  const used = new Set(taken);
  for (;;) {
    const id = Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => CROCKFORD[b & 31]).join("");
    if (!used.has(id)) return id;
  }
}

type Clip = { id?: string; start: number; length: number };

/** Whether clip `i` can be named by its id: a valid id no other clip in its lane has (FMT-12). */
export function hasOwnId(clips: readonly Clip[], i: number) {
  const id = clips[i]?.id;
  return !!id && CLIP_ID.test(id) && clips.every((c, j) => j === i || c.id !== id);
}

/**
 * A clip in the timeline: its lane's address (module key, lane key) and its id (FMT-12), never
 * its place in the lane. A clip without an id of its own can only be found in the revision it
 * was read from: `at` finds it there by where it is, and `id` is the id the next write gives it.
 */
export interface ClipRef { module: string; lane: string; id: string; at?: { start: number; length: number } }

/** The id each anonymous clip is to get, by lane and place, so every reference to it names it alike. */
const pending = new Map<string, string>();

/** The reference to clip `i` of `module`'s lane `lane` (keys). */
export function clipRef(module: string, lane: string, clips: readonly Clip[], i: number): ClipRef {
  const c = clips[i];
  if (hasOwnId(clips, i)) return { module, lane, id: c.id! };
  const place = `${module}/${lane}@${c.start}+${c.length}`;
  let id = pending.get(place);
  if (!id || clips.some((x) => x.id === id)) pending.set(place, (id = newClipId(clips.map((x) => x.id))));
  return { module, lane, id, at: { start: c.start, length: c.length } };
}

/** Where `ref`'s clip is in `clips`: by its id, else (an anonymous clip) by where it was; -1 if it is gone. */
export function clipIndex(clips: readonly Clip[], ref: ClipRef) {
  const own = clips.findIndex((c, i) => c.id === ref.id && hasOwnId(clips, i));
  if (own >= 0 || !ref.at) return own;
  const { start, length } = ref.at;
  return clips.findIndex((c, i) => c.start === start && c.length === length && !hasOwnId(clips, i));
}

/** Whether two references name the same clip. */
export const sameClip = (a: ClipRef | null | undefined, b: ClipRef | null | undefined) =>
  a === b || (!!a && !!b && a.module === b.module && a.lane === b.lane && a.id === b.id);

/** A lane of the timeline, by module and lane key. */
export function findLane(timeline: TimelineView, module: string, lane: string): TimelineLane | undefined {
  return timeline.tracks.find((t) => t.module === module)?.lanes.find((l) => l.key === lane);
}

/** The notes clip a reference points at, if the timeline (still) has it. */
export function findNoteClip(timeline: TimelineView, ref: ClipRef): NoteClip | undefined {
  const lane = findLane(timeline, ref.module, ref.lane);
  return lane?.kind === "notes" ? lane.clips[clipIndex(lane.clips, ref)] : undefined;
}

// ---------------------------------------------------------------- bars

const FOUR_FOUR: Meter = { beat: 0, beats: 4, unit: 4 };
/** Quarter notes in a bar of `m`. */
const barLength = (m: Meter) => (m.beats * 4) / m.unit;
const meters = (meter: readonly Meter[]) => (meter.length ? meter : [FOUR_FOUR]);

/**
 * The bar `beat` falls in, counted as the arrangement counts them (FMT-13): its number from 0,
 * the beat it starts at and its length in quarter notes.
 */
export function barAt(meter: readonly Meter[], beat: number): { bar: number; start: number; length: number } {
  const ms = meters(meter);
  let bar = 0;
  for (let i = 0; ; i++) {
    const m = ms[i], length = barLength(m), next = ms[i + 1]?.beat ?? Infinity;
    if (beat < next) {
      // Float noise just under a bar line is still on it.
      const k = Math.max(0, Math.floor((beat - m.beat) / length + 1e-9));
      return { bar: bar + k, start: m.beat + k * length, length };
    }
    // Each change falls on a bar line of the meter before it.
    bar += Math.round((next - m.beat) / length);
  }
}

/** Every bar that starts before `end`, in order. */
export function bars(meter: readonly Meter[], end: number) {
  const out: { bar: number; start: number; length: number }[] = [];
  for (let b = barAt(meter, 0); b.start < end; b = barAt(meter, b.start + b.length)) out.push(b);
  return out;
}

/** The stretches of one bar length between `from` and `to`, for drawing a grid: each starts on a bar line or at `from`. */
export function meterSpans(meter: readonly Meter[], from: number, to: number) {
  const ms = meters(meter);
  return ms
    .map((m, i) => ({ start: Math.max(m.beat, from), end: Math.min(ms[i + 1]?.beat ?? Infinity, to), origin: m.beat, bar: barLength(m) }))
    .filter((s) => s.end > s.start);
}

/** How many bars `beats` beats from `from` span, each bar as long as the meter has it there; part of one counts in part. */
export function beatsToBars(meter: readonly Meter[], from: number, beats: number) {
  let [at, left, count] = [from, beats, 0];
  while (left > 1e-9) {
    const b = barAt(meter, at);
    const take = Math.min(b.start + b.length - at, left);
    [at, left, count] = [at + take, left - take, count + take / b.length];
  }
  return count;
}

/** How many beats `n` bars from `from` last, each bar as long as the meter has it there. */
export function barsToBeats(meter: readonly Meter[], from: number, n: number) {
  let [at, left] = [from, n];
  while (left > 1e-9) {
    const b = barAt(meter, at);
    const take = Math.min((b.start + b.length - at) / b.length, left);
    [at, left] = [at + take * b.length, left - take];
  }
  return at - from;
}

/** `3.2`: the bar and the beat in it, counted from 1, as the transport shows a position. */
export function position(meter: readonly Meter[], beat: number) {
  const b = barAt(meter, beat);
  return `${b.bar + 1}.${Math.floor(beat - b.start + 1e-9) + 1}`;
}

/** A loop between two beats; its range is kept while it is off. */
export interface Loop { start: number; end: number; on: boolean }

/** Where playback is; `beat` counts with the accepted revision's tempo map. `loop` is null until one is set. */
export interface GraphStatus { playing: boolean; seconds: number; beat: number; loop: Loop | null }

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
  /** Why the files on disk don't compile, if they don't: the draft's problems, placed in their files. */
  diagnostics: Problem[];
  /**
   * What needs attention in the revision playing, none of which stops it (DIAG-4), with the
   * fixes: data no lane reads (`data.unclaimed`), a lane file that doesn't read (FMT-9), clip ids,
   * asset pins, plugin states, unknown files under `data/`.
   */
  problems: Diagnostic[];
  /** The rate the song runs and exports at (`[project] sample_rate`). */
  sample_rate: number;
}

export type Severity = "error" | "warning" | "notice";

/**
 * A problem, as every part of music2 reports it (DIAG-1): how bad it is, a stable `code` from the
 * contract's table (`lane.invalid`), what is wrong and what to do in plain words, the file it is
 * about (`src`, project-relative; `file:line:col` for code), the key of its module, and its fixes,
 * the likeliest first.
 */
export interface Diagnostic {
  severity: Severity;
  code: string;
  message: string;
  src?: string | null;
  module?: string | null;
  fixes?: Fix[];
}

/**
 * A way to fix a problem, applied only when a person asks (DIAG-5), as one undoable edit. `code` is
 * from the contract's closed vocabulary (`relink`, `delete-data`, `repin-asset`, …); `title` says
 * what it does ("Move to Kicks › hits"); `likely` marks the one fix the problem plainly calls for,
 * offered in one click. `edit` is what it changes: whole files, each with the SHA-256 it expects
 * (null: a file that must not exist yet). The app applies a fix by sending it back as it came
 * (`graphApi.fix`); the session applies it as a named edit until fixes are Tx (step 6).
 */
export interface Fix {
  code: string;
  title: string;
  likely: boolean;
  edit: { base: Record<string, string | null>; ops: FixOp[] };
}
export type FixOp = { op: "write"; file: string; text: string } | { op: "remove"; file: string } | { op: "rename"; from: string; to: string };

/**
 * One reason the draft doesn't compile (a `code.build`, `key.duplicate`, … Error): `file` is
 * project-relative (or a package's, `std/src/dsp.ts`), line and col 1-based; `module` is a key.
 */
export interface Problem {
  file: string | null;
  line: number | null;
  col: number | null;
  message: string;
  module?: string;
  code: string;
  severity: Severity;
}

/** Everything that needs attention in the revision playing, errors first (DIAG-4). */
export function attention(graph: GraphState): Diagnostic[] {
  const rank: Record<Severity, number> = { error: 0, warning: 1, notice: 2 };
  return [...(graph.problems ?? [])].sort((a, b) => rank[a.severity] - rank[b.severity]);
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

async function call<T = any>(path: string, body?: unknown): Promise<T> {
  return request<T>(`/api/graph${path}`, body);
}

/** GET, or POST `body` as JSON; an error answer throws with the server's reason. */
async function request<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
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
 * ⌘S; App's save command tries it first. `saveAll` saves every edited file (the desktop
 * app asks for it before it closes or replaces the project).
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
  /** Rewrite a notes lane (module and lane keys) with every clip it is to have. Prefer `writeLane`, which queues and gives clips ids. */
  lane: (session: number, revision: number, module: string, lane: string, clips: NoteClip[]) =>
    call<{ file: string; before: string; after: string; revision: number }>("/lane", { session, revision, module, lane, clips }),
  /** Apply a problem's fix, offered at `revision`, exactly as it came: one undoable edit. */
  fix: (session: number, revision: number, fix: Fix) =>
    call<{ file: string; before: string; after: string; revision: number }>("/fix", { session, revision, fix }),
  undo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/undo", { session }),
  redo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/redo", { session }),
  transport: (action: "play" | "stop" | "toggle" | "restart") => call("/transport", { action }),
  /** Set the loop's range (beats; it turns looping on unless `on` says), or turn looping on or off over the range set. Answers with the transport. */
  loop: (req: { start?: number; end?: number; on?: boolean }) => call<GraphStatus>("/loop", req),
  /** Tell the app which files the code editor has unsaved edits in, so it asks before closing the project. */
  unsaved: (session: number, files: string[]) => call("/unsaved", { session, files }),
  /** Start rendering the song to a WAV file: `path`, or a new file in the songs folder's Exports. Progress comes as `export` events. */
  export: (path?: string) => call<{ path: string }>("/export", path ? { path } : {}),
};

// ---------------------------------------------------------------- new songs

/** A template or an example a new song can start from. */
export interface Starter { id: string; name: string; description: string }

/** What a new song can start from; `songs` is the folder new songs go in unless another is picked. */
export interface Catalog { templates: Starter[]; examples: Starter[]; songs: string | null; home: string | null }

export const projectApi = {
  catalog: () => request<Catalog>("/api/project/templates"),
  /** Make a song from a template (or a copy of an example) in `dir` (default the songs folder), and open it. */
  create: (song: { name: string; template?: string; example?: string; dir?: string }) => request<GraphState>("/api/project/new", song),
};

/** `/Users/me/Music/music2` → `~/Music/music2`. */
export function tilde(path: string, home: string | null) {
  return home && (path === home || path.startsWith(`${home}/`)) ? `~${path.slice(home.length)}` : path;
}

// ---------------------------------------------------------------- lane writes

/** Lane writes of the open project, one at a time, and the newest revision one made. */
let lanes = { session: 0, revision: 0, pending: 0, queue: Promise.resolve() as Promise<unknown> };

/**
 * Rewrite `module`'s notes lane `lane` (keys) with every clip it is to have: one write, one undo
 * step. `base` is the revision those clips were read from (a graph's, or the one an earlier write
 * made): the server refuses the write if the lane changed since, so it never overwrites a change
 * the clips don't include.
 *
 * The app builds the write, so it chooses the ids (FMT-11): a clip keeps the id it has, and one
 * without a valid id gets a new one here, so what comes back is exactly what was sent (`clips`).
 */
export function writeLane(graph: GraphState, module: string, lane: string, clips: NoteClip[], base: number) {
  if (lanes.session !== graph.session) lanes = { session: graph.session, revision: 0, pending: 0, queue: Promise.resolve() };
  const taken = clips.map((c) => c.id);
  const sent = clips.map((c) => {
    if (c.id && CLIP_ID.test(c.id)) return c;
    const id = newClipId(taken);
    taken.push(id);
    return { ...c, id };
  });
  const q = lanes;
  q.pending++;
  const write = q.queue
    .then(() => graphApi.lane(graph.session, base, module, lane, sent))
    .then((r) => ((q.revision = Math.max(q.revision, r.revision)), { ...r, clips: sent }))
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

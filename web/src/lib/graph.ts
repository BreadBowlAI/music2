// The graph view of the open song project: types, API, layout.

import { token } from "./native";

/** A declared control a constant is (PKG-3): its instance's frame, its slot, and whether the call leaves it out. */
export interface Slot { frame: number; slot: string; inherited?: boolean; smooth?: boolean }

export interface Origin { site: number; arg?: string[]; scale?: number; control?: Slot }

/** Where a value lives (HOME-1): the code, a lane, a plugin's state, `layout.json`, or this machine's `.music2/`. */
export type Home = "code" | "lane" | "state" | "layout" | "local";

export interface Knob {
  id: string;
  label: string;
  value: number;
  /** The number written in the code; empty while the control plays its default. */
  literal: string;
  unit: string | null;
  /** The range its control declares, for its unit: a hint, never a clamp (PKG-8). Null when none is declared, so the page picks a span. */
  range: [number, number] | null;
  log: boolean;
  /** Written in the code; false while the call leaves it out and it plays its declared default (its first edit writes it in). */
  written: boolean;
  default: number | null;
  /** The declared control (`cutoff`, `channels.hats.db`) and its component (`std:lowpass`), when it is exactly one. */
  slot: string | null;
  component: string | null;
  origin: Origin;
  uses: number;
  file: string;
  line: number;
  home: Home;
  /** Why it can't be automated (HOME-2), when it can't: its menu says so instead of offering it. */
  automate_refused?: string;
}

export interface ViewNode {
  id: string;
  label: string;
  /** `voice`: the note a `poly` voice plays (its pitch, gate, …), beside the `poly` call. */
  kind: "call" | "lane" | "time" | "import" | "value" | "export" | "voice";
  file: string | null;
  line: number | null;
  knobs: Knob[];
  readonly: { label: string; expression: string; reason: string; file: string | null; line: number | null; home: Home }[];
  /** The `const` this node's signal is bound to, if a cable can start here. */
  outlet: string | null;
  /** Inputs connected by a cable: `label ← name`. */
  inlets: { id: string; label: string; name: string; from: string }[];
  /** Whether its result is audio, which an effect can go after. */
  audio: boolean;
  /** The component it is a call of (`std:delay`), whatever name the code calls it by (PKG-9). */
  component: string | null;
  /** The effect this node is, by its label (`Delay`), if it can be removed. */
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

/**
 * An effect the packages offer: a component (`std:delay`) imported as `name` from `module`. `insert` is
 * what adding it writes after the audio (`, 1000` gives `lowpass(x, 1000)`). `category` is a free string
 * the browser groups by (PKG-7).
 */
export interface Effect { component: string; label: string; name: string; module: string; description: string; category: string; insert: string }

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

/**
 * Where playback is; `beat` counts with the accepted revision's tempo map. `loop` is null until one is set.
 * `paused` is "exporting" while an export borrows the plugins that play, and `loading` names the plugins
 * being loaded for an edit (RENDER-1, RENDER-25).
 */
export interface GraphStatus { playing: boolean; seconds: number; beat: number; loop: Loop | null; paused?: string | null; loading?: string[] }

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
  /**
   * False while the project was opened with code that doesn't compile (WRITE-20): nothing is
   * shown or played until a save makes it compile. Absent from older servers.
   */
  has_code?: boolean;
  /**
   * Why this app can't write the project (WRITE-17): another program holds its lock, or its folder
   * can't be locked. Null when it can.
   */
  read_only?: ReadOnly | null;
  /**
   * The knob drags in progress (WRITE-22's gesture leases), each with its control's id as this
   * view names it: a save that inserts a call above a dragged knob in its file renumbers it.
   */
  leases?: GraphLease[];
}

/** Why the open project is read-only here, with WRITE-18's message. */
export interface ReadOnly {
  why: "held" | "unsupported";
  message: string;
  /** Who holds the lock, as its file says. */
  holder?: { pid: number; program: string; host: string; since: number } | null;
}

/** A knob drag in progress, as `GET /api/graph/leases` and the graph's state list it. */
export interface GraphLease {
  lease: number;
  /** The control's id in `revision`. */
  id: string;
  file: string;
  line: number;
  /** The window that began it, and the gesture the knob routes name it by. */
  client: string | null;
  gesture: string | null;
  state: "live" | "committing" | "pending";
  /** What plays, and the code's value. */
  value: number;
  accepted: number;
  revision: number;
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
 * offered in one click. `tx` is what it changes: a ready-to-commit Tx (DIAG-1), each file it
 * touches with the content it expects. The app applies a fix by sending it back as it came
 * (`graphApi.fix`): the session commits its Tx, and one whose files changed since is a conflict.
 */
export interface Fix {
  code: string;
  title: string;
  likely: boolean;
  tx: FixTx;
}
/** A fix's Tx, as far as the app reads it: its ops (the rest it sends back as it came). */
export interface FixTx {
  id: string;
  ops: FixOp[];
  [more: string]: unknown;
}
export type FixOp =
  | { op: "write"; file: string; text?: string; base64?: string }
  | { op: "remove"; file: string }
  | { op: "rename"; from: string; to: string }
  | { op: "text_edit"; file: string }
  | { op: "clips"; lane: [string, string] };

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

/**
 * This window, one per page load. Its knob drags carry it, and so does its event socket: when the
 * window closes or reloads, the server ends the drags it left open (WRITE-22's leases, C6).
 */
export const clientId: string = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `c${Date.now()}${Math.random()}`;

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
  /** A knob drag's update, straight to the engine; the first one begins the gesture's lease. Also its keepalive while held still. */
  preview: (session: number, revision: number, id: string, gesture: string, value: number) =>
    call("/preview", { session, revision, id, gesture, value, client: clientId }),
  cancel: (session: number, id: string, gesture: string) => call("/preview/cancel", { session, id, gesture, client: clientId }),
  /** Release: one value edit. While the code doesn't compile it is `pending`, and saved once it does. */
  set: (session: number, revision: number, id: string, gesture: string, value: number, expect: number) =>
    call<{ file: string; before: string; after: string; revision: number; pending?: boolean; errors?: unknown[] }>("/set", {
      session,
      revision,
      id,
      gesture,
      value,
      expect,
      client: clientId,
    }),
  connect: (session: number, revision: number, from: string, to: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/connect", { session, revision, from, to }),
  disconnect: (session: number, revision: number, id: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/disconnect", { session, revision, id }),
  addEffect: (session: number, revision: number, node: string, effect: Effect) =>
    call<{ file: string; before: string; after: string; revision: number }>("/effect/add", { session, revision, call: node, component: effect.component }),
  removeEffect: (session: number, revision: number, node: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/effect/remove", { session, revision, call: node }),
  /** Rewrite a notes lane (module and lane keys) with every clip it is to have. Prefer `writeLane`, which queues and gives clips ids. */
  lane: (session: number, revision: number, module: string, lane: string, clips: NoteClip[]) =>
    call<{ file: string; before: string; after: string; revision: number }>("/lane", { session, revision, module, lane, clips }),
  /** Show a track, by its key, as `label` (ID-10): its `track()` call's name, nothing that plays. One undoable edit. */
  label: (session: number, module: string, label: string) =>
    call<{ file: string; before: string; after: string; revision: number }>("/label", { session, module, label }),
  /**
   * Automate a knob (HOME-2): its value becomes the fallback of a new, empty control lane read where
   * it was, so the song sounds the same and the timeline shows a lane to draw on. One undoable edit.
   */
  automate: (session: number, revision: number, id: string) =>
    call<{ file: string; before: string; after: string; revision: number; lane: { module: string; lane: string }; name: string }>(
      "/automate",
      { session, revision, id },
    ),
  /** Apply a problem's fix, offered at `revision`, exactly as it came: its Tx, one undoable edit. */
  fix: (session: number, revision: number, fix: Fix) =>
    call<{ file: string; before: string; after: string; revision: number }>("/fix", { session, revision, fix }),
  undo: (session: number) => call<{ file: string; before: string; after: string; revision: number }>("/undo", { session }),
  /** Try the project's lock again (the read-only banner's Retry). Answers with the state. */
  retryLock: (session: number) => call<GraphState>("/lock/retry", { session }),
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

/**
 * A span for a knob whose control declares no range in its unit: by the unit, else around its value.
 * The page's own choice (core guesses no ranges, P10), so a value in an unexpected unit still turns.
 */
function unitSpan(unit: string | null, v: number): [number, number, boolean] {
  switch (unit) {
    case "hz":
      return [20, 20000, true];
    case "db":
      return [-60, 12, false];
    case "seconds":
      return [0.001, 10, true];
    case "beats":
      return [1 / 16, 16, true];
    case "unit01":
      return [0, 1, false];
    case "gain":
      return [0, 2, false];
    case "bipolar":
      return [-1, 1, false];
    case "semitones":
      return [v - 24, v + 24, false];
    default:
      return Math.abs(v) <= 1 ? [v < 0 ? -1 : 0, 1, false] : [Math.min(v, 0) * 2, Math.max(v, 0) * 2, false];
  }
}

/**
 * The span a knob turns through: its declared range (or the page's own for its unit), widened to show
 * its value if that lies outside, never clamping it (PKG-8). It depends on the knob's value in the
 * code, not on one being dragged, so a knob turned to an end of it turns back.
 */
export function span(k: Knob): { lo: number; hi: number; log: boolean } {
  const [lo, hi, log] = k.range ? [k.range[0], k.range[1], k.log] : unitSpan(k.unit, k.value);
  const low = Math.min(lo, k.value);
  return { lo: low, hi: Math.max(hi, k.value), log: log && low > 0 };
}

/** Knob position 0..1 ↔ value, linear or logarithmic. */
export function toUnit(k: Knob, v: number) {
  const { lo, hi, log } = span(k);
  if (hi <= lo) return 0.5;
  if (log) return Math.log(v / lo) / Math.log(hi / lo);
  return (v - lo) / (hi - lo);
}

export function fromUnit(k: Knob, u: number) {
  const { lo, hi, log } = span(k);
  const value = log ? lo * Math.pow(hi / lo, u) : lo + (hi - lo) * u;
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

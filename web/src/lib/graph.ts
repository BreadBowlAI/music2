// The graph view of the open song project: types, API, layout.

import { editing, recorded, type Editing, type Step, type StepName } from "./history";
import { token } from "./native";

/** A declared control a constant is (PKG-3): its instance's frame, its slot, and whether the call leaves it out. */
export interface Slot { frame: number; slot: string; inherited?: boolean; smooth?: boolean }

export interface Origin { site: number; arg?: string[]; scale?: number; control?: Slot }

/** Where a value lives (HOME-1): the code, a lane, a plugin's state, `layout.json`, `data/arrangement.json` (the tempo and time signature), or this machine's `.music2/`. */
export type Home = "code" | "lane" | "state" | "layout" | "arrangement" | "local";

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
  /** The component it is a call of (`@m2/effects:delay`), whatever name the code calls it by (PKG-9). */
  component: string | null;
  /** The effect this node is, by its label (`Delay`), if it can be removed. */
  effect: string | null;
  /** What a call is called on screen: its component's label (`LFO` for `lfo(…)`); `label` is the code's name for it. */
  title: string | null;
  /** Where signals and values come in, in the order the code writes them. */
  inputs: InputPort[];
  /** The signals it gives to other nodes, its own result first. */
  outputs: OutputPort[];
  /** An instrument's card: how it plays its notes, in plain words ("Plays one note at a time. …"). Read-only. */
  voices?: string;
  /** A hosted plugin's call: its node in the graph, the key of the state's `plugins` (HOST-7). */
  plugin?: string;
}

/** A plugin's box (HOST-7, MON-6): its own window, the parameters a gesture in it latched, and whether its window changed its sound, not saved yet (AGENT-3). */
export interface PluginBox {
  window: "open" | "closed" | "none";
  latched: { id: number; name: string; value: number }[];
  unsaved: boolean;
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
 * An effect the packages offer: a component (`@m2/effects:delay`) imported as `name` from `module`. `insert` is
 * what adding it writes after the audio (`, 1000` gives `lowpass(x, 1000)`). `category` is a free string
 * the browser groups by (PKG-7).
 */
export interface Effect { component: string; label: string; name: string; module: string; description: string; category: string; insert: string; pack?: PackFrom }

/** The pack a browser item is from (PACK-11): none for std's and the song's own. */
export interface PackFrom { name: string; version: string }

/** What a browser row's tooltip says of where it is from: `\nFrom @alice/felt-piano 1.2.0`, or nothing. */
export function fromPack(item: { pack?: PackFrom }): string {
  return item.pack ? `\nFrom ${item.pack.name} ${item.pack.version}` : "";
}

/**
 * An instrument a new track can play (+ Track, the browser's Instruments): its component, what it
 * is called, where it is imported from, and the lane a new track plays it from (`hits` for drums).
 */
export interface Instrument { component: string; label: string; name: string; module: string; description: string; category: string; lane: string; pack?: PackFrom }

/** What reads a track a delete would take out: the track it is in, the call, and where. */
export interface Dependent { module: string; label: string; component: string; file: string; line: number; removable: boolean }

/**
 * A module: the song (`song`) or a track. `key` is what the code calls it (`track("lead-vox", …)`):
 * the one address of the module, for selection, its tabs, its lanes and its data (ID-11). `label`
 * is what to call it (`{ name: "Lead Vox" }`, else the key): shown, never an address (ID-10).
 */
export interface ModuleView { key: string; label: string; track: boolean; nodes: ViewNode[]; edges: [string, string][]; port_edges: PortEdge[] }

export interface GraphView { revision: number; modules: ModuleView[]; draft_errors: string[] }

/** What to show for a lane or a module: its label, which defaults to its key. */
export const labelOf = (x: { key: string; label?: string | null }) => x.label || x.key;
/** A word as a label starts: `cutoff` → Cutoff. */
const sentence = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
/** A lane's name on screen: its label, else its key as words in sentence case (`cutoff` → Cutoff, `hats-volume` → Hats volume; taste.md §3.4). */
export const laneTitle = (lane: { key: string; label?: string | null }) => (lane.label && lane.label !== lane.key ? lane.label : sentence(lane.key.replace(/-/g, " ")));
/** What a mixer channel's control is called (taste.md's words: `channels.hats.db` is "Hats volume"); its input is the channel itself. */
const CHANNEL: Record<string, string> = { db: "volume", pan: "pan", in: "" };
/**
 * A control's or an input's name on screen, by the one rule lane headers use too (taste.md §3.4):
 * the key in sentence case (`feedback` → Feedback); a level (`db`: the master's, a gain's) is
 * Volume, never "Db", which reads as a note. A mixer channel's (`hats.db`) names its track, by
 * `track` when it is one: Hats volume. Never the slot path.
 */
export function controlLabel(label: string, track?: (key: string) => string | undefined) {
  const dot = label.indexOf(".");
  if (dot < 0) return sentence(label === "db" ? "volume" : label);
  const ch = label.slice(0, dot), rest = label.slice(dot + 1);
  const who = track?.(ch) ?? sentence(ch);
  const word = CHANNEL[rest] ?? rest;
  return word ? `${who} ${word}` : who;
}
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
  /** Warp markers, `[file's second, beat in the clip]`: set by Follow tempo (the sx plan's D11). */
  warp?: [number, number][];
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
export interface TimelineTrack {
  module: string;
  label: string;
  track: boolean;
  /** It is a bus (RENDER-34): no notes or audio lanes; it plays what the tracks send it. */
  bus?: boolean;
  /** Its colour by palette name (taste.md §2.7), from `layout.json` or by its place; `graphite` for the song. */
  colour?: string;
  lanes: TimelineLane[];
}

/** A time signature from `beat` on: `beats` in a bar of `unit` notes (FMT-13). A beat is a quarter note. */
export interface Meter { beat: number; beats: number; unit: number }

/** What the accepted revision plays, in beats. */
export interface TimelineView {
  tempo: { beat: number; bpm: number }[];
  /** The time signatures; empty is 4/4 throughout. Bars are counted from them (`barAt`, `bars`). */
  meter: Meter[];
  /** Where the tempo and time signatures live (HOME-1): `data/arrangement.json`. */
  home: Home;
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

/** `3.2.1`: the bar, the beat in it and the sixteenth in that, counted from 1, as the transport shows a position. */
export function position(meter: readonly Meter[], beat: number) {
  const b = barAt(meter, beat);
  const into = beat - b.start + 1e-9;
  const q = Math.floor(into);
  return `${b.bar + 1}.${q + 1}.${Math.min(3, Math.floor((into - q) * 4)) + 1}`;
}

/**
 * A strip of the Mix view (design-8b §5.4): a mixer channel and the tracks it plays, or a track
 * with no channel of its own. Its fader and pan are knobs of the graph view, by id, so they drag,
 * lease and write as every knob does.
 */
export interface MixStrip {
  /** The tracks it plays, by key, in the order its channel reads them; none for a channel no track reaches. */
  modules: string[];
  /** Its channel's key in the mixer, when it has one. */
  channel: string | null;
  /** Its tracks' names ("Kick + Snare"), else its channel's key. */
  label: string;
  /** Its first track's colour by name, else graphite. */
  colour: string;
  /** The fader's knob id; null when code sets the level (a signal), or the track has no channel. */
  db: string | null;
  /** The pan's knob id, when the channel writes a number there. */
  pan: string | null;
  /** The channel writes no pan, and its first pan can write one (`graphApi.mixInsert`). */
  pan_insert: boolean;
  /** Why a track has no fader: "Not in the mixer", or "In 2 channels: set its level in code". */
  why: string | null;
  /** The automation lane its level follows: the fader shows it, read only (taste.md §2.10). */
  db_lane: LaneRef | null;
  /** The automation lane its pan follows. */
  pan_lane: LaneRef | null;
  /** It is a bus's strip (RENDER-34): these come after the tracks'. */
  bus: boolean;
  /** Its sends to bus channels, as its entry writes them. */
  sends: StripSend[];
  /** The bus channel it goes to instead of the master; null for the master. */
  output: string | null;
  /** The bus channels it may send to or go to without a loop, by key. */
  may_use: string[];
  /** The bus channels it can't use, each with why in words: for a send, and for its output. */
  loops: { bus: string; send: string; to: string }[];
}

/** A strip's send to a bus channel: its level's knob id (null when code sets it), before the fader or not, the lane its level follows. */
export interface StripSend {
  bus: string;
  knob: string | null;
  pre: boolean;
  lane: LaneRef | null;
}

/** What the Add bus menu makes (RENDER-34). */
export type BusKind = "reverb" | "delay" | "group";

/** A lane, by its module's key and its own. */
export interface LaneRef { module: string; lane: string }

/**
 * The Mix view of the accepted revision: a strip per track in the track order, then one per channel
 * no track reaches, and std's master. When `mixer` isn't `ok` (no std mixer, several, or channels
 * not written as an object), `message` says why the levels are in code.
 */
export interface MixView {
  mixer: "ok" | "none" | "several" | "unwritten";
  message: string | null;
  strips: MixStrip[];
  /** std's master: its fader's and its ceiling's knob ids, and the lane its level follows. Null without exactly one master. */
  master: { db: string | null; ceiling: string | null; db_lane: LaneRef | null } | null;
}

/** A loop between two beats; its range is kept while it is off. */
export interface Loop { start: number; end: number; on: boolean }

/**
 * Where playback is; `beat` counts with the accepted revision's tempo map. `loop` is null until one is set.
 * `paused` is "exporting" while an export borrows the plugins that play, and `loading` names the plugins
 * being loaded for an edit (RENDER-1, RENDER-25).
 */
export interface GraphStatus {
  playing: boolean;
  seconds: number;
  beat: number;
  /**
   * The tempo the engine plays at `beat`: a tempo edit reaches it at its switch, after the
   * timeline says it (and after an export's hold). Null with no player.
   */
  bpm?: number | null;
  loop: Loop | null;
  paused?: string | null;
  loading?: string[];
}

/** The open project, and where playback was when it was read. */
export interface GraphState extends GraphStatus {
  session: number;
  /**
   * The revision the code editor's libraries were made at: the SDK and the sources of std and the
   * song's packs. It moves when the song's packs change (PACK-9), and the editor loads them again.
   */
  types?: number;
  name: string;
  dir: string;
  view: GraphView;
  timeline: TimelineView;
  /** The Mix view's strips and master (design-8b §5.4). */
  mix: MixView;
  /** Whether an edit made here can be undone or redone. */
  can_undo: boolean;
  can_redo: boolean;
  /** Undo can't take the newest step back (a file it puts back changed since): it can be skipped (AGENT-2). */
  undo_stuck?: boolean;
  /** Effects the packages offer to put after audio. */
  effects: Effect[];
  /** Instruments a new track can play, drums first. Absent from older servers. */
  instruments?: Instrument[];
  /** The track colours' names, in their default order. */
  palette?: string[];
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
  /** Each hosted plugin by its node (`ViewNode.plugin`): its window, and what is latched. */
  plugins?: Record<string, PluginBox>;
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

/**
 * Everything that needs attention in the revision playing, errors first (DIAG-4). Not the lock
 * another program holds: read-only is said once, by the status pill (taste.md ST-2).
 */
export function attention(graph: GraphState): Diagnostic[] {
  const rank: Record<Severity, number> = { error: 0, warning: 1, notice: 2 };
  return [...(graph.problems ?? [])].filter((d) => d.code !== "lock.held").sort((a, b) => rank[a.severity] - rank[b.severity]);
}

/**
 * Why nothing can be edited right now, or null when it can: the code doesn't compile (C7), another
 * program holds the project (WRITE-17), or the connection to the app is gone. Edit affordances show
 * their values then, without drag cursors, and say this instead (taste.md ST-2, ST-7).
 */
export function lockReason(graph: GraphState, connected: boolean): string | null {
  if (!connected) return "Reconnecting to music2: editing is back once it is";
  if (graph.view.draft_errors.length > 0) return "Fix the code first: it doesn't compile right now";
  if (graph.read_only) return "Read only: this song is open for writing somewhere else";
  return null;
}

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/**
 * This window, one per page load. Its knob drags carry it, and so does its event socket: when the
 * window closes or reloads, the server ends the drags it left open (WRITE-22's leases, C6).
 */
export const clientId: string = typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `c${Date.now()}${Math.random()}`;

/**
 * An answer that said OK but whose body never arrived whole: the window reloading while it waited,
 * or the server going away. It isn't an empty answer, so the caller keeps what it has.
 */
export class CutOff extends Error {
  constructor() {
    super("The server's answer was cut off. Try again.");
  }
}

/** An answer's JSON. An error answer with none reads as {}; an OK one cut off on its way throws `CutOff`. */
export async function readJson(res: Response): Promise<any> {
  try {
    return await res.json();
  } catch {
    if (res.ok) throw new CutOff();
    return {};
  }
}

async function call<T = any>(path: string, body?: unknown): Promise<T> {
  return request<T>(`/api/graph${path}`, body);
}

/** POST `body`, answering the status with the JSON: for a route whose 409 says something to ask. */
async function answer<T>(path: string, body: unknown): Promise<{ status: number; json: T & { error?: string } }> {
  const res = await fetch(`/api/graph${path}`, { method: "POST", headers: { "content-type": "application/json", ...auth }, body: JSON.stringify(body) });
  const json = await readJson(res);
  if (!res.ok && res.status !== 409) throw new Error(json.error ?? `HTTP ${res.status}`);
  return { status: res.status, json };
}

/** GET, or POST `body` as JSON; an error answer throws with the server's reason. */
async function request<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method: body === undefined ? "GET" : "POST",
    headers: { "content-type": "application/json", ...auth },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await readJson(res);
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
  /** Edit ▸ Quantize (Q): the roll's while it has the keyboard (X2). */
  quantize?: () => boolean;
  /** View ▸ Zoom In / Zoom Out (`dir` 1 or -1): the roll's own zoom while it has the keyboard (X3). */
  zoom?: (dir: 1 | -1) => boolean;
  undo?: (redo: boolean) => boolean;
  /** A menu command whose chord the code editor binds (⌘D): its own action while it has the keyboard. */
  chord?: (id: string) => boolean;
  unsaved?: () => number;
} = {};

/** An edit's call: once it commits a revision, that revision is Undo's, named `step` (lib/history). */
function edit<T extends { revision: number; before: string; after: string; pending?: boolean }>(
  session: number,
  step: Step | ((r: T) => Step),
  p: Promise<T>,
  within?: Editing,
) {
  // On its way: as part of `within` (a lane write in its queue, which ends it), else as one of its own.
  const ours = within ?? editing();
  // A value held while the code doesn't compile (`pending`) commits nothing yet; nor does an edit that changes nothing.
  return p
    .then((r) => {
      if (!r.pending && r.before !== r.after) recorded(session, r, typeof step === "function" ? step(r) : step, ours);
      return r;
    })
    .finally(() => within || ours.end());
}

/** An edit's answer. `step`: the undo step it made, by its id (AGENT-2); an undo's or a redo's: the step it took back. */
type Edit = { file: string; before: string; after: string; revision: number; step?: number };
/**
 * An undo's or a redo's answer: the edit, the tops after it; or, refused, the newest step (`newest`),
 * or the step undo can't take back, which can be skipped (`stuck`).
 */
type Stepped = Edit & { undo_name?: StepName | null; redo_name?: StepName | null; newest?: StepName | null; stuck?: StepName };

export const graphApi = {
  get: () => call<GraphState>(""),
  /** Open or close a plugin's own window (HOST-7); a plugin without one, or no screen, says why. */
  pluginWindow: (session: number, node: string, open: boolean) => call<{ window: string }>("/plugin/window", { session, node, open }),
  /** Re-enable: the lane or cable drives a latched parameter again (every one without `id`, MON-6). */
  pluginReenable: (session: number, node: string, id?: number) => call("/plugin/latch", { session, node, id }),
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
  // Each edit below names its undo step (`step`): what the Undo button's tooltip and an undo's toast say.
  set: (session: number, revision: number, id: string, gesture: string, value: number, expect: number, step: Step) =>
    edit(
      session,
      step,
      call<Edit & { pending?: boolean; errors?: unknown[] }>("/set", { session, revision, id, gesture, value, expect, client: clientId }),
    ),
  connect: (session: number, revision: number, from: string, to: string, step: Step) =>
    edit(session, step, call<Edit>("/connect", { session, revision, from, to })),
  disconnect: (session: number, revision: number, id: string, step: Step) => edit(session, step, call<Edit>("/disconnect", { session, revision, id })),
  addEffect: (session: number, revision: number, node: string, effect: Effect) =>
    edit(session, { name: `adding ${effect.label}`, structural: true }, call<Edit>("/effect/add", { session, revision, call: node, component: effect.component })),
  removeEffect: (session: number, revision: number, node: string, step: Step) =>
    edit(session, step, call<Edit>("/effect/remove", { session, revision, call: node })),
  /** Rewrite a notes lane (module and lane keys) with every clip it is to have. Prefer `writeLane`, which queues and gives clips ids. */
  lane: (session: number, revision: number, module: string, lane: string, clips: NoteClip[], step: Step, within?: Editing) =>
    edit(session, step, call<Edit>("/lane", { session, revision, module, lane, clips }), within),
  /** Show a track, by its key, as `label` (ID-10): its `track()` call's name, nothing that plays. One undoable edit. */
  label: (session: number, module: string, label: string, step: Step) => edit(session, step, call<Edit>("/label", { session, module, label })),
  /**
   * Automate a knob (HOME-2): its value becomes the fallback of a new, empty control lane read where
   * it was, so the song sounds the same and the timeline shows a lane to draw on. One undoable edit.
   */
  automate: (session: number, revision: number, id: string) =>
    edit(
      session,
      (r) => ({ name: `automating ${r.name}`, structural: true }),
      call<Edit & { lane: { module: string; lane: string }; name: string }>("/automate", { session, revision, id }),
    ),
  /** Apply a problem's fix, offered at `revision`, exactly as it came: its Tx, one undoable edit. */
  fix: (session: number, revision: number, fix: Fix) =>
    // Named by its own title, quoted: "Undid “Move all to Kick (kicks)”".
    edit(session, { name: `“${fix.title}”`, structural: true }, call<Edit>("/fix", { session, revision, fix })),
  /**
   * Add a track playing `instrument` (WRITE-10): its file, its import and mixer channel, its lane and
   * its place below `after` in the layout. One undoable edit; the answer names the new track.
   */
  addTrack: (session: number, instrument: Instrument, opts: { label?: string; after?: string | null } = {}) =>
    edit(
      session,
      (r) => ({ name: `adding ${r.label}`, structural: true }),
      call<Edit & { module: string; label: string }>("/track/add", { session, component: instrument.component, label: opts.label, after: opts.after ?? undefined }),
    ),
  /**
   * A mixer channel's first pan (design-8b §5.3): optional control `control` (`pan`) of channel
   * `channel` written into its entry as `value`. One undoable edit; the answer names the new knob
   * (`control`) in the revision it made, which the drag then leases.
   */
  mixInsert: (session: number, channel: string, control: string, value: number, step: Step) =>
    edit(session, step, call<Edit & { control: string }>("/mix/insert", { session, channel, control, value })),
  /** Add a bus (RENDER-34): its file, its import and its mixer channel, last in the layout. One undoable edit. */
  mixBus: (session: number, kind: BusKind, label: string) =>
    edit(session, { name: `adding the ${label} bus`, structural: true }, call<Edit & { module: string; label: string }>("/mix/bus", { session, kind })),
  /** Add channel `channel`'s send to bus channel `bus`, remove it, or set it before the fader (`pre`) or after it. One undoable edit. */
  mixSend: (session: number, channel: string, bus: string, action: "add" | "remove" | "pre", pre: boolean, step: Step) =>
    edit(session, step, call<Edit>("/mix/send", { session, channel, bus, action, pre })),
  /** Send channel `channel` to bus channel `to` instead of the master, or (null) back to the master. One undoable edit. */
  mixRoute: (session: number, channel: string, to: string | null, step: Step) => edit(session, step, call<Edit>("/mix/route", { session, channel, to })),
  /** Duplicate a track: its code, clips (with new ids) and channel, below it in its colour. */
  duplicateTrack: (session: number, module: string, name: string) =>
    edit(
      session,
      { name: `duplicating ${name}`, structural: true },
      call<Edit & { module: string; label: string; new_channel: boolean }>("/track/duplicate", { session, module }),
    ),
  /**
   * Delete a track. When other code reads it, nothing is written and the answer lists what: `blocked`
   * when that can't be taken out from here, else asking again with `cascade` takes it out too.
   */
  deleteTrack: async (session: number, module: string, name: string, cascade = false) => {
    const ours = editing();
    try {
      const r = await answer<Edit & { excised?: Dependent[]; dependents?: Dependent[]; blocked?: boolean }>("/track/delete", { session, module, cascade });
      if (r.status === 409) return { asks: { dependents: r.json.dependents ?? [], blocked: !!r.json.blocked } };
      await edit(session, { name: `deleting ${name}`, structural: true }, Promise.resolve(r.json), ours);
      return { done: r.json };
    } finally {
      ours.end();
    }
  },
  /** A layout write (no revision, PLANE-3): every track in a new order, one track's colour, or the defaults. */
  // Always a step of its own, though a move says "Kick" before and after: what Undo takes back next.
  layout: (session: number, change: { order?: string[]; colour?: { module: string; colour: string }; reset?: boolean }, step: Step) => {
    const ours = editing();
    return call<Edit>("/layout", { session, ...change })
      .then((r) => (recorded(session, r, step, ours), r))
      .finally(() => ours.end());
  },
  /**
   * Undo the step `step` names, only while it is the newest (AGENT-2); without it, the newest. A
   * step that isn't the newest is a 409: nothing changed, and `newest` names the newest.
   */
  undo: (session: number, step?: number) => answer<Stepped>("/undo", { session, step }),
  /** Try the project's lock again (the read-only banner's Retry). Answers with the state. */
  retryLock: (session: number) => call<GraphState>("/lock/retry", { session }),
  redo: (session: number, step?: number) => answer<Stepped>("/redo", { session, step }),
  /**
   * Skip the newest step, `step`, which undo can't take back: it leaves the history and nothing is
   * written, so Undo reaches the steps before it. Answers with the tops after it.
   */
  skip: (session: number, step: number) => answer<Stepped>("/undo/skip", { session, step }),
  /** Play, stop, or the one of them it isn't doing (Space). A stop leaves the playhead where it stopped. */
  transport: (action: "play" | "stop" | "toggle" | "restart") => call("/transport", { action }),
  /**
   * The song bar's tempo (design-8b §4): the tempo in effect at beat `at` set to `bpm`; with `add`, a
   * change at `at`; with `remove`, the change at `at` taken out, and with `meter` too the time
   * signature change there. One undo step, and no toast: what it did shows where it was done.
   * `loop_off`: the new tempo made the loop too short, so it is off.
   */
  tempo: (session: number, change: { at: number; bpm?: number; add?: boolean; remove?: boolean; meter?: boolean }, step: Step) =>
    edit(session, step, call<Edit & { loop_off?: boolean }>("/tempo", { session, ...change })),
  /** The time signature in effect at `at`; with `add`, a change on the bar line at or before it; with `remove`, the change at `at` taken out. */
  meter: (session: number, change: { at: number; beats?: number; unit?: number; add?: boolean; remove?: boolean }, step: Step) =>
    edit(session, step, call<Edit>("/meter", { session, ...change })),
  /**
   * Remove automation (KN-2): the lane's value at `beat` (the playhead's) is written back where the
   * code reads the lane, and the lane's file goes. One undoable edit.
   */
  removeAutomation: (session: number, lane: LaneRef, beat: number, step: Step) =>
    edit(session, step, call<Edit>("/automate/remove", { session, module: lane.module, lane: lane.lane, beat })),
  /** Call the song `name`: its manifest's name. The folder keeps its own. */
  title: (session: number, name: string, step: Step) => edit(session, step, call<Edit>("/title", { session, name })),
  /** Stop all sound (⌘.): every note off, every instance reset. Not a write. */
  panic: () => call("/panic", {}),
  /** Set the loop's range (beats; it turns looping on unless `on` says), or turn looping on or off over the range set. Answers with the transport. */
  loop: (req: { start?: number; end?: number; on?: boolean }) => call<GraphStatus>("/loop", req),
  /** Tell the app which files the code editor has unsaved edits in, so it asks before closing the project. */
  unsaved: (session: number, files: string[]) => call("/unsaved", { session, files }),
  /** Start rendering the song to a WAV file: `path`, or a new file in the songs folder's Exports. Progress comes as `export` events. */
  export: (path?: string) => call<{ path: string }>("/export", path ? { path } : {}),
};

// ---------------------------------------------------------------- new songs

/** A template or an example a new song can start from, with its tempo (the New Song dialog's Tempo starts there). */
export interface Starter { id: string; name: string; description: string; tempo?: number }

/** What a new song can start from; `songs` is the folder new songs go in unless another is picked. */
export interface Catalog { templates: Starter[]; examples: Starter[]; songs: string | null; home: string | null }

export const projectApi = {
  catalog: () => request<Catalog>("/api/project/templates"),
  /** Make a song from a template (or a copy of an example) in `dir` (default the songs folder), and open it. */
  create: (song: { name: string; template?: string; example?: string; dir?: string; tempo?: number }) => request<GraphState>("/api/project/new", song),
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
export function writeLane(graph: GraphState, module: string, lane: string, clips: NoteClip[], base: number, step: Step) {
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
  // On its way from now, while it waits in the queue too (Undo waits for it).
  const ours = editing();
  const write = q.queue
    .then(() => graphApi.lane(graph.session, base, module, lane, sent, step, ours))
    .then((r) => {
      q.revision = Math.max(q.revision, r.revision);
      wrote.set(`${module}.${lane}`, { session: graph.session, revision: r.revision, clips: shapeOf(sent) });
      return { ...r, clips: sent };
    })
    .finally(() => (q.pending--, ours.end()));
  q.queue = write.catch(() => {});
  return write;
}

/**
 * The lanes the roll wrote last, by `module.lane`: the clips it wrote, as `shapeOf` says them, and
 * the revision the write made.
 */
const wrote = new Map<string, { session: number; revision: number; clips: string }>();

/** Notes clips as what they hold: their ids, places and notes, in the order the file keeps them. */
function shapeOf(clips: NoteClip[]): string {
  const notes = (c: NoteClip) =>
    c.notes.map((n) => [n.pitch, n.start, n.length, n.velocity]).sort((a, b) => a[1] - b[1] || a[0] - b[0]);
  return JSON.stringify(clips.map((c) => [c.id ?? null, c.start, c.length, notes(c)]).sort((a, b) => (a[1] as number) - (b[1] as number) || String(a[0]).localeCompare(String(b[0]))));
}

/**
 * Whether notes lane `module.lane`, holding `clips` in `graph`, is as the roll's last write of it
 * made it, a write that made a revision after `since`: what changed there since `since` was this
 * window's own writing, nothing another writer did after it.
 */
export function wroteHere(graph: GraphState, module: string, lane: string, clips: NoteClip[], since: number): boolean {
  const w = wrote.get(`${module}.${lane}`);
  return !!w && w.session === graph.session && w.revision > since && w.revision <= graph.view.revision && w.clips === shapeOf(clips);
}

/**
 * Run `f` (a write of a notes lane by another route: a drag's lease, 8b.7) after the lane writes on
 * their way, in their queue, counted as one of them: it never races a roll write still on its way.
 */
export function afterLaneWrites<T>(graph: GraphState, f: (within: Editing) => Promise<T & { revision: number }>): Promise<T & { revision: number }> {
  if (lanes.session !== graph.session) lanes = { session: graph.session, revision: 0, pending: 0, queue: Promise.resolve() };
  const q = lanes;
  q.pending++;
  const ours = editing();
  const write = q.queue
    .then(() => f(ours))
    .then((r) => ((q.revision = Math.max(q.revision, r.revision)), r))
    .finally(() => (q.pending--, ours.end()));
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
  return formatValue(k.unit, v, k.label);
}

/** A pan control (`pan`, `hats.pan`, a `hats-pan` lane): it reads "C", "L 20" or "R 35". */
const isPan = (unit: string | null | undefined, name: string | undefined) => unit === "bipolar" && !!name && /(^|[.\-_])pan$/.test(name);

/**
 * A value in its unit, as the screen writes it (taste.md, Words and numbers): "−6.0 dB" ("+6.0 dB"), "350 Hz",
 * "1.2 kHz", "120 ms", "1.50 s", "0.25", "+7 st", "C" or "L 20" for a pan. Negative values take a
 * true minus (U+2212); code keeps its hyphen. `name` (the control's or lane's key) tells a pan.
 */
export function formatValue(unit: string | null | undefined, v: number, name?: string) {
  // `d` decimals, fixed or (`trim`) with trailing zeros dropped; never a "−0.0".
  const num = (x: number, d: number, trim = false) => {
    const r = Number(x.toFixed(d));
    const t = Object.is(r, -0) || r === 0 ? (0).toFixed(trim ? 0 : d) : trim ? String(r) : r.toFixed(d);
    return t.replace(/^-/, "\u2212");
  };
  if (isPan(unit, name)) {
    const p = Math.round(v * 100);
    return p === 0 ? "C" : `${p < 0 ? "L" : "R"} ${Math.abs(p)}`;
  }
  switch (unit) {
    case "hz":
      if (v >= 1000) return `${num(v / 1000, 1)} kHz`;
      // Rates below hearing (a chorus's 0.8 Hz) keep their decimals; heard pitches are whole.
      return v < 20 ? `${num(v, 2, true)} Hz` : `${num(v, 0)} Hz`;
    case "db":
      // Gain is signed, as a mixer reads it: "+6.0 dB", "−6.0 dB", "0.0 dB".
      return `${Number(v.toFixed(1)) > 0 ? "+" : ""}${num(v, 1)} dB`;
    case "seconds":
      return v < 1 ? `${num(v * 1000, 0)} ms` : `${num(v, 2)} s`;
    case "beats":
      return `${num(v, 3, true)} beats`;
    case "semitones":
      return `${v > 0 ? "+" : ""}${num(v, 1, true)} st`;
    case "unit01":
    case "bipolar":
      return num(v, 2);
    default:
      return num(v, 2, true);
  }
}

/** Semitones from C of each note name, for a typed pitch ("C4", "F#3", "Bb2"). */
const NOTE: Record<string, number> = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 };

/**
 * A value typed into a knob (taste.md §2.14), in its unit: "-3", "-3db", "2k", "2.5 khz", "120ms",
 * "0.5s", "50%", "L20", "+7" (semitones) and "C4" (a pitch, for a frequency). A bare number is in
 * the unit the knob shows ("3" on a knob reading 2.5 kHz is 3 kHz). Held to the knob's span and
 * written to three significant figures, as a drag writes it; null if it doesn't read as a value.
 */
export function parseValue(k: Knob, text: string): number | null {
  const t = text.trim().toLowerCase().replace(/\u2212/g, "-").replace(/\s+/g, "");
  let v: number | null = null;
  const pitch = /^([a-g])([#b]?)(-?\d)$/.exec(t);
  const pan = /^([lr])(\d+(?:\.\d+)?)%?$/.exec(t);
  const m = /^([+-]?(?:\d+\.?\d*|\.\d+))([a-z%]*)$/.exec(t);
  if (isPan(k.unit, k.label) && (t === "c" || pan)) v = t === "c" ? 0 : ((pan![1] === "l" ? -1 : 1) * Number(pan![2])) / 100;
  else if (pitch && k.unit === "hz") {
    const midi = 12 * (Number(pitch[3]) + 1) + NOTE[pitch[1]] + (pitch[2] === "#" ? 1 : pitch[2] === "b" ? -1 : 0);
    v = 440 * 2 ** ((midi - 69) / 12);
  } else if (m) {
    const [n, unit] = [Number(m[1]), m[2]];
    switch (k.unit) {
      case "hz":
        if (unit === "k" || unit === "khz") v = n * 1000;
        else if (unit === "hz") v = n;
        // Bare: as shown; a kHz reading takes a small number as kHz.
        else if (unit === "") v = k.value >= 1000 && Math.abs(n) < 20 ? n * 1000 : n;
        break;
      case "seconds":
        if (unit === "ms") v = n / 1000;
        else if (unit === "s") v = n;
        else if (unit === "") v = k.value < 1 ? n / 1000 : n;
        break;
      case "db":
        if (unit === "" || unit === "db") v = n;
        break;
      case "semitones":
        if (unit === "" || unit === "st") v = n;
        break;
      case "beats":
        if (unit === "" || unit === "beats" || unit === "beat") v = n;
        break;
      default:
        if (unit === "%") v = n / 100;
        else if (unit === "") v = n;
    }
  }
  if (v === null || !Number.isFinite(v)) return null;
  const { lo, hi } = span(k);
  return Number(Math.min(hi, Math.max(lo, v)).toPrecision(3));
}

/**
 * One step of a knob from `v` (taste.md §5.2), `n` of them (negative is down): a 1% ratio on a
 * logarithmic span (Hz), 0.1 dB, a semitone, or else 1% of the span. Held to the span.
 */
export function stepValue(k: Knob, v: number, n: number) {
  const { lo, hi, log } = span(k);
  const next = log ? v * 1.01 ** n : k.unit === "db" ? v + 0.1 * n : k.unit === "semitones" ? v + n : v + (hi - lo) * 0.01 * n;
  return Number(Math.min(hi, Math.max(lo, next)).toPrecision(3));
}

/**
 * What a control lane plays at `beat`: inside a clip (the last one there wins, as they are drawn),
 * its points joined by straight lines, held before the first and after the last; between clips its
 * fallback. `follows` says whether a clip is what sets it (HOME-1: its home is the lane).
 */
export function laneValue(lane: Extract<TimelineLane, { kind: "control" }>, beat: number): { value: number | null; follows: boolean } {
  for (let i = lane.clips.length - 1; i >= 0; i--) {
    const c = lane.clips[i];
    if (beat < c.start || beat >= c.start + c.length || !c.points.length) continue;
    const t = beat - c.start, ps = c.points;
    if (t <= ps[0][0]) return { value: ps[0][1], follows: true };
    for (let k = 1; k < ps.length; k++) {
      const [b0, v0] = ps[k - 1], [b1, v1] = ps[k];
      if (t <= b1) return { value: b1 > b0 ? v0 + ((v1 - v0) * (t - b0)) / (b1 - b0) : v1, follows: true };
    }
    return { value: ps[ps.length - 1][1], follows: true };
  }
  return { value: lane.default ?? null, follows: false };
}

// @m2/core: the TypeScript half of the music2 core.
//
// Running a song builds a graph; nothing here makes sound. Each function records
// nodes into the module being built, and `__build` returns the IR as JSON for the
// Rust checker, which is what actually enforces kinds and units. The types below
// exist so editors can help.

export type Unit = "hz" | "db" | "gain" | "semitones" | "seconds" | "beats" | "unit01" | "bipolar" | "scalar";

export type AudioKind = { readonly audio: true };
export type ControlKind<U extends Unit = Unit> = { readonly control: U };
export type EventsKind = { readonly events: "note" };

/** A signal: one output of a node in the graph being built. */
export class Signal<K = unknown> {
  declare readonly __kind?: K;
  /** @internal */
  constructor(
    readonly ref: string,
    /** @internal */ readonly scope: Scope,
    /** The unit, when known here (lanes, constants, note fields). */
    readonly unit?: Unit,
  ) {}
}

export type Audio = Signal<AudioKind>;
export type Control<U extends Unit = Unit> = Signal<ControlKind<U>>;
export type Events = Signal<EventsKind>;
export type Mask = Control<"unit01">;

/** A kernel instance. As a signal it is its `out` port; `.port(name)` gives the others. */
export class KernelNode<K = unknown> extends Signal<K> {
  port<P = unknown>(name: string): Signal<P> {
    return new Signal<P>(`${this.ref}.${name}`, this.scope);
  }
}

/** The same signal, marked with a unit so helpers like `osc.saw` know what it is. */
export function withUnit<S extends Signal>(s: S, unit: Unit | undefined): S {
  return unit ? (new Signal(s.ref, s.scope, unit) as S) : s;
}

/** The unit of a value, if known here. */
export function unitOf(v: unknown): Unit | undefined {
  return v instanceof Signal || v instanceof Quantity ? v.unit : undefined;
}

/** A number with a unit, from `hz(440)`, `db(-6)`, … */
export class Quantity<U extends Unit = Unit> {
  constructor(
    readonly value: number,
    readonly unit: U,
  ) {}
}

export const hz = (v: number) => new Quantity(v, "hz");
export const khz = (v: number) => new Quantity(v * 1000, "hz");
export const db = (v: number) => new Quantity(v, "db");
export const linear = (v: number) => new Quantity(v, "gain");
export const semis = (v: number) => new Quantity(v, "semitones");
export const secs = (v: number) => new Quantity(v, "seconds");
export const ms = (v: number) => new Quantity(v / 1000, "seconds");
export const beats = (v: number) => new Quantity(v, "beats");
export const frac = (v: number) => new Quantity(v, "unit01");
export const bipolar = (v: number) => new Quantity(v, "bipolar");
export const scalar = (v: number) => new Quantity(v, "scalar");

/** Anything that can feed an input: a signal, a plain number (fits any unit), or a quantity. */
export type Value<K = unknown> = Signal<K> | number | Quantity;

/** One sounding note, as seen by a voice inside `poly`. */
export type Note = {
  pitch: Control<"semitones">;
  gate: Control<"unit01">;
  velocity: Control<"unit01">;
  pressure: Control<"unit01">;
  timbre: Control<"unit01">;
};

// ---------------------------------------------------------------- building

type IrNode = { id: string; op: string; src?: string; [k: string]: unknown };
type LaneDecl = { id: string; kind: string; default?: number };

class ModuleBuild {
  counter = 0;
  lanes: LaneDecl[] = [];
  root: Scope;
  exports: Record<string, string> = {};
  imports = new Map<string, Signal>();
  time = new Map<string, Signal>();
  constructor(
    readonly name: string,
    readonly track: boolean,
    readonly src: string | undefined,
  ) {
    this.root = new Scope(this, null);
  }
}

/** @internal */
export class Scope {
  nodes: IrNode[] = [];
  constructor(
    readonly module: ModuleBuild,
    readonly parent: Scope | null,
  ) {}
}

let current: Scope | null = null;

function need(what: string): Scope {
  if (!current) throw new Error(`${what} can only be used while a track or song is being built`);
  return current;
}

const PACKAGES: string[] = (globalThis as any).__m2_packages ?? [];
const PROJECT: string = (globalThis as any).__m2_project ?? "";

/** Where the user's code called in: the first stack frame in the project. */
function callsite(): string | undefined {
  const frames = (new Error().stack ?? "").split("\n");
  let fallback: string | undefined;
  for (const f of frames) {
    const m = /([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(f) ?? /([^\s()]+):(\d+)\)?\s*$/.exec(f);
    if (!m || m[1] === "@m2/core") continue;
    const at = m[3] ? `${m[1]}:${m[2]}:${m[3]}` : `${m[1]}:${m[2]}`;
    if (PROJECT && m[1].startsWith(PROJECT)) return at;
    if (!fallback && !PACKAGES.some((p) => m[1].startsWith(p))) fallback = at;
  }
  return fallback;
}

function push(scope: Scope, op: Record<string, unknown>): string {
  const id = `n${scope.module.counter++}`;
  const node: IrNode = { id, op: op.op as string, ...op };
  const src = callsite();
  if (src) node.src = src;
  scope.nodes.push(node);
  return id;
}

function visible(s: Signal, from: Scope): boolean {
  for (let at: Scope | null = from; at; at = at.parent) if (at === s.scope) return true;
  return false;
}

/** Turn a value into a ref usable in the current scope. */
function ref(v: Value, what: string): string {
  const scope = need(what);
  if (v instanceof Signal) {
    if (v.scope.module !== scope.module) {
      throw new Error(`${what}: this signal belongs to "${v.scope.module.name}". Export it from that track and read it as <track>.<export>`);
    }
    if (!visible(v, scope)) throw new Error(`${what}: this signal only exists inside a poly voice`);
    return v.ref;
  }
  if (typeof v === "number") {
    if (!Number.isFinite(v)) throw new Error(`${what}: ${v} is not a finite number`);
    return push(scope, { op: "const", value: v });
  }
  if (v instanceof Quantity) return push(scope, { op: "const", value: v.value, unit: v.unit });
  throw new TypeError(`${what}: expected a signal, a number or a quantity like hz(440), got ${typeof v}`);
}

/**
 * An instance of a package kernel, e.g. `kernel("std:svf", { in: x, cutoff: hz(800) })`.
 * Unset inputs use the kernel's defaults.
 */
export function kernel<K = unknown>(id: string, inputs: Record<string, Value | undefined> = {}): KernelNode<K> {
  const scope = need(`kernel("${id}")`);
  const ins: Record<string, string> = {};
  for (const [k, v] of Object.entries(inputs)) if (v !== undefined) ins[k] = ref(v, `${id} input "${k}"`);
  return new KernelNode<K>(push(scope, { op: "kernel", kernel: id, inputs: ins }), scope);
}

/**
 * A hosted plugin (CLAP), e.g. `plugin("clap:org.surge-synth-team.surge-xt", { notes, state: "lead" })`.
 * Inputs are `in` (audio), `notes` (a notes lane) and any parameter by its port name
 * (`m2 plugins <id>` lists them). `state` names a saved state in `assets/plugin-state/`.
 */
export function plugin(id: string, inputs: { state?: string; [port: string]: Value | string | undefined } = {}): KernelNode<AudioKind> {
  const scope = need(`plugin("${id}")`);
  const ins: Record<string, string> = {};
  let state: string | undefined;
  for (const [k, v] of Object.entries(inputs)) {
    if (v === undefined) continue;
    if (k === "state") {
      if (typeof v !== "string") throw new Error(`plugin("${id}"): state must be the name of a saved state`);
      state = v;
    } else {
      if (typeof v === "string") throw new Error(`plugin("${id}") input "${k}": expected a signal or a number, got a string`);
      ins[k] = ref(v, `${id} input "${k}"`);
    }
  }
  const op: Record<string, unknown> = { op: "plugin", plugin: id, inputs: ins };
  if (state !== undefined) op.state = state;
  return new KernelNode<AudioKind>(push(scope, op), scope);
}

/**
 * Run `voice` once per sounding note and sum the results. Inside the voice, `note`
 * holds that note's pitch, gate and velocity, and every signal from outside is
 * still readable.
 */
export function poly(notes: Events, voice: (note: Note) => Audio, opts: { voices?: number } = {}): Audio {
  const outer = need("poly");
  const events = ref(notes, "poly notes");
  const body = new Scope(outer.module, outer);
  const field = (lane: string, unit: Unit) => {
    const id = `n${outer.module.counter++}`;
    body.nodes.push({ id, op: "lane", lane });
    return new Signal<any>(id, body, unit);
  };
  const note: Note = {
    pitch: field("pitch", "semitones"),
    gate: field("gate", "unit01"),
    velocity: field("velocity", "unit01"),
    pressure: field("pressure", "unit01"),
    timbre: field("timbre", "unit01"),
  };
  current = body;
  let out: string;
  try {
    const sig = voice(note);
    if (!(sig instanceof Signal)) throw new Error("a poly voice must return a signal");
    out = ref(sig, "poly voice output");
  } finally {
    current = outer;
  }
  const id = push(outer, { op: "poly", events, voices: opts.voices ?? 16, body: { nodes: body.nodes, out } });
  return new Signal<AudioKind>(id, outer);
}

/** A signal read one block late, which is the only way to close a loop. Call `.set()` with its input. */
export class Feedback<K = unknown> extends Signal<K> {
  set(input: Signal<K>): void {
    const node = this.scope.nodes.find((n) => n.id === this.ref);
    if (!node) throw new Error("feedback: node not found");
    if (node.input) throw new Error("feedback: input already set");
    node.input = ref(input, "feedback input");
  }
}

const feedbacks: { node: () => IrNode | undefined; module: string }[] = [];

export function feedback<K = unknown>(kind: "audio<1>" | "audio<2>" | `control<${Unit}>`): Feedback<K> {
  const scope = need("feedback");
  const id = push(scope, { op: "feedback", kind, input: null });
  feedbacks.push({ node: () => scope.nodes.find((n) => n.id === id), module: scope.module.name });
  return new Feedback<K>(id, scope);
}

// ---------------------------------------------------------------- modules

/** What a track or song function receives. */
export type Ctx = {
  name: string;
  lane: {
    /** Notes from the timeline. */
    notes(id: string): Events;
    /** An automation lane; `fallback` applies where no clip is. */
    control<U extends Unit>(id: string, unit: U, fallback?: number | Quantity<U>): Control<U>;
    /** A 0..1 lane that is 0 where no clip is. Use it to switch behaviour on and off over time. */
    mask(id: string): Mask;
    /** Audio clips from the timeline (stereo unless `channels` is 1). */
    audio(id: string, channels?: 1 | 2): Audio;
  };
  time: { readonly beats: Control<"beats">; readonly seconds: Control<"seconds"> };
};

function declareLane(m: ModuleBuild, id: string, kind: string, fallback?: number) {
  const have = m.lanes.find((l) => l.id === id);
  if (have) {
    if (have.kind !== kind || have.default !== fallback) throw new Error(`lane "${id}" is declared twice with different settings`);
    return;
  }
  const d: LaneDecl = { id, kind };
  if (fallback !== undefined) d.default = fallback;
  m.lanes.push(d);
}

function ctxFor(m: ModuleBuild): Ctx {
  const laneNode = (id: string, unit?: Unit) => new Signal<any>(push(m.root, { op: "lane", lane: id }), m.root, unit);
  const time = (of: "beats" | "seconds") => {
    let s = m.time.get(of);
    if (!s) m.time.set(of, (s = new Signal(push(m.root, { op: "time", of }), m.root, of)));
    return s as any;
  };
  return {
    name: m.name,
    lane: {
      notes(id) {
        declareLane(m, id, "events<note>");
        return laneNode(id);
      },
      control(id, unit, fallback) {
        const v = fallback instanceof Quantity ? fallback.value : fallback;
        declareLane(m, id, `control<${unit}>`, v ?? 0);
        return laneNode(id, unit);
      },
      mask(id) {
        declareLane(m, id, "control<unit01>", 0);
        return laneNode(id, "unit01");
      },
      audio(id, channels = 2) {
        declareLane(m, id, `audio<${channels}>`);
        return laneNode(id);
      },
    },
    time: {
      get beats() {
        return time("beats");
      },
      get seconds() {
        return time("seconds");
      },
    },
  };
}

type Exports = Signal | Record<string, Signal>;
type TrackFn<E extends Exports> = (ctx: Ctx) => E;
type Handle<E extends Exports> = E extends Signal ? { out: E } : { [K in keyof E]: E[K] };

const tracks = new Map<string, { fn: TrackFn<any>; src?: string }>();
const TRACK = Symbol("m2.track");

/**
 * A track: a module with lanes on the timeline. Return a signal (its `out`) or an
 * object of named exports. Other modules read exports as `track.exportName`.
 */
export function track<E extends Exports>(name: string, fn: TrackFn<E>): Handle<E> {
  if (typeof name !== "string" || !name) throw new Error("a track needs a name");
  if (name === "song") throw new Error(`"song" is reserved for the root module`);
  if (tracks.has(name)) throw new Error(`two tracks are named "${name}"`);
  tracks.set(name, { fn, src: callsite() });
  return new Proxy({} as Handle<E>, {
    get(_, prop) {
      if (prop === TRACK) return name;
      if (typeof prop !== "string" || prop === "then" || prop === "toJSON") return undefined;
      const scope = need(`${name}.${prop}`);
      const m = scope.module;
      const key = `${name}.${prop}`;
      let s = m.imports.get(key);
      if (!s) m.imports.set(key, (s = new Signal(push(m.root, { op: "import", module: name, export: prop }), m.root)));
      return s;
    },
  });
}

/** The root module: whatever it returns is the song's output. */
export function song(fn: (ctx: Ctx) => Audio): { [TRACK]: "song"; fn: (ctx: Ctx) => Audio; src?: string } {
  return { [TRACK]: "song", fn, src: callsite() };
}

function exportsOf(m: ModuleBuild, value: unknown) {
  const one = (k: string, v: unknown) => {
    if (!(v instanceof Signal)) throw new Error(`${m.name}: export "${k}" must be a signal`);
    if (v.scope !== m.root) throw new Error(`${m.name}: export "${k}" must not come from inside a poly voice`);
    m.exports[k] = v.ref;
  };
  if (value instanceof Signal) one("out", value);
  else if (value && typeof value === "object") for (const [k, v] of Object.entries(value)) one(k, v);
  else throw new Error(`${m.name} must return a signal or an object of signals`);
}

function run(name: string, isTrack: boolean, fn: (ctx: Ctx) => unknown, src?: string): ModuleBuild {
  const m = new ModuleBuild(name, isTrack, src);
  const prev = current;
  current = m.root;
  try {
    exportsOf(m, fn(ctxFor(m)));
  } finally {
    current = prev;
  }
  return m;
}

/** @internal Called by the host with the song module's default export. */
export function __build(root: unknown): string {
  const r = root as any;
  if (!r || r[TRACK] !== "song") throw new Error("song.ts must `export default song(...)`");
  const built = [run("song", false, r.fn, r.src)];
  for (const [name, t] of tracks) built.push(run(name, true, t.fn, t.src));
  for (const f of feedbacks) {
    const n = f.node();
    if (n && n.input === null) throw new Error(`${f.module}: a feedback() was never given its input with .set()`);
  }
  return JSON.stringify({
    ir: 0,
    root: { module: "song", export: "out" },
    modules: built.map((m) => ({ name: m.name, track: m.track, lanes: m.lanes, nodes: m.root.nodes, exports: m.exports, src: m.src })),
  });
}

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
    /** @internal The call that wrote it, so the editor can find the number. */
    readonly site?: number,
    /** @internal `value` is the written number times this (`khz` is 1000). */
    readonly scale: number = 1,
  ) {}
}

/** The call site the running call was made from (set by instrumentation), once. */
function takeSite(): number | undefined {
  const g = globalThis as any;
  const s = g.__m2_site;
  g.__m2_site = -1;
  return typeof s === "number" && s >= 0 ? s : undefined;
}

const unitFn =
  <U extends Unit>(unit: U, scale = 1) =>
  (v: number): Quantity<U> => {
    const site = takeSite();
    return new Quantity(v * scale, unit, site, scale);
  };

export const hz = unitFn("hz");
export const khz = unitFn("hz", 1000);
export const db = unitFn("db");
export const linear = unitFn("gain");
export const semis = unitFn("semitones");
export const secs = unitFn("seconds");
export const ms = unitFn("seconds", 0.001);
export const beats = unitFn("beats");
export const frac = unitFn("unit01");
export const bipolar = unitFn("bipolar");
export const scalar = unitFn("scalar");

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

type Origin = { site: number; arg?: string[]; scale?: number };
type IrNode = { id: string; op: string; src?: string; origin?: Origin; [k: string]: unknown };
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

function push(scope: Scope, op: Record<string, unknown>, origin?: Origin): string {
  const id = `n${scope.module.counter++}`;
  const node: IrNode = { id, op: op.op as string, ...op };
  if (origin) node.origin = origin;
  else {
    const src = callsite();
    if (src) node.src = src;
  }
  scope.nodes.push(node);
  return id;
}

const at = (site: number | undefined): Origin | undefined => (site === undefined ? undefined : { site });

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
  if (v instanceof Quantity) {
    const origin = v.site === undefined ? undefined : { site: v.site, arg: ["0"], ...(v.scale !== 1 ? { scale: v.scale } : {}) };
    return push(scope, { op: "const", value: v.value, unit: v.unit }, origin);
  }
  throw new TypeError(`${what}: expected a signal, a number or a quantity like hz(440), got ${typeof v}`);
}

/**
 * An instance of a package kernel, e.g. `kernel("std:svf", { in: x, cutoff: hz(800) })`.
 * Unset inputs use the kernel's defaults.
 */
export function kernel<K = unknown>(id: string, inputs: Record<string, Value | undefined> = {}): KernelNode<K> {
  const site = takeSite();
  const scope = need(`kernel("${id}")`);
  const ins: Record<string, string> = {};
  for (const [k, v] of Object.entries(inputs)) if (v !== undefined) ins[k] = ref(v, `${id} input "${k}"`);
  return new KernelNode<K>(push(scope, { op: "kernel", kernel: id, inputs: ins }, at(site)), scope);
}

/**
 * A hosted plugin (CLAP), e.g. `plugin("clap:org.surge-synth-team.surge-xt", { notes, state: "lead" })`.
 * Inputs are `in` (audio), `notes` (a notes lane) and any parameter by its port name
 * (`m2 plugins <id>` lists them). `state` names a saved state in `assets/plugin-state/`.
 */
export function plugin(id: string, inputs: { state?: string; [port: string]: Value | string | undefined } = {}): KernelNode<AudioKind> {
  const site = takeSite();
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
  return new KernelNode<AudioKind>(push(scope, op, at(site)), scope);
}

/**
 * Run `voice` once per sounding note and sum the results. Inside the voice, `note`
 * holds that note's pitch, gate and velocity, and every signal from outside is
 * still readable.
 */
export function poly(notes: Events, voice: (note: Note) => Audio, opts: { voices?: number } = {}): Audio {
  const site = takeSite();
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
  const id = push(outer, { op: "poly", events, voices: opts.voices ?? 16, body: { nodes: body.nodes, out } }, at(site));
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
  const site = takeSite();
  const scope = need("feedback");
  const id = push(scope, { op: "feedback", kind, input: null }, at(site));
  feedbacks.push({ node: () => scope.nodes.find((n) => n.id === id), module: scope.module.name });
  return new Feedback<K>(id, scope);
}

// ---------------------------------------------------------------- components

/**
 * Declare which arguments of a function are *controls*: numbers that can change
 * while the song plays (a cutoff, a gain), as opposed to settings that shape the
 * graph (a voice count, a filter mode). Paths are a positional index and then
 * property keys, with `*` for any key: `"1"`, `"1.cutoff"`, `"0.*.pan"`.
 *
 * Each plain number found at a control path becomes a constant node that remembers
 * where it was written, so the editor can show it as a knob and change it live. The
 * function receives a signal there instead of the number.
 */
export function component<F extends (...args: any[]) => any>(desc: { controls: string[] }, fn: F): F {
  const patterns = desc.controls.map((p) => p.split("."));
  return ((...args: unknown[]) => {
    const site = takeSite();
    if (site !== undefined && current) {
      const scope = current;
      for (const [first, ...rest] of patterns) {
        const i = Number(first);
        if (i < args.length) args[i] = controlAt(args[i], rest, [first], site, scope);
      }
    }
    return fn(...args);
  }) as F;
}

function controlAt(v: unknown, rest: string[], path: string[], site: number, scope: Scope): unknown {
  if (rest.length === 0) {
    if (typeof v === "number" && Number.isFinite(v)) return new Signal(push(scope, { op: "const", value: v }, { site, arg: path }), scope);
    return v;
  }
  if (!v || typeof v !== "object" || v instanceof Signal || v instanceof Quantity || Array.isArray(v)) return v;
  const [key, ...more] = rest;
  const copy: Record<string, unknown> = { ...(v as Record<string, unknown>) };
  for (const k of key === "*" ? Object.keys(copy) : [key]) {
    if (k in copy) copy[k] = controlAt(copy[k], more, [...path, k], site, scope);
  }
  return copy;
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
  const laneNode = (id: string, unit?: Unit, site?: number) => new Signal<any>(push(m.root, { op: "lane", lane: id }, at(site)), m.root, unit);
  const time = (of: "beats" | "seconds") => {
    let s = m.time.get(of);
    if (!s) m.time.set(of, (s = new Signal(push(m.root, { op: "time", of }), m.root, of)));
    return s as any;
  };
  return {
    name: m.name,
    lane: {
      notes(id) {
        const site = takeSite();
        declareLane(m, id, "events<note>");
        return laneNode(id, undefined, site);
      },
      control(id, unit, fallback) {
        const site = takeSite();
        const v = fallback instanceof Quantity ? fallback.value : fallback;
        declareLane(m, id, `control<${unit}>`, v ?? 0);
        return laneNode(id, unit, site);
      },
      mask(id) {
        const site = takeSite();
        declareLane(m, id, "control<unit01>", 0);
        return laneNode(id, "unit01", site);
      },
      audio(id, channels = 2) {
        const site = takeSite();
        declareLane(m, id, `audio<${channels}>`);
        return laneNode(id, undefined, site);
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

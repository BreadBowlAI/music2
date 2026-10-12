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

  [Symbol.toPrimitive](): never {
    throw new TypeError(notANumber("a signal"));
  }
}

/**
 * Values in the graph are not JavaScript numbers: the graph only sees what is built
 * from them, so arithmetic in JavaScript would hide them from knobs and the editor.
 */
function notANumber(what: string): string {
  return `${what} is part of the sound graph, not a JavaScript number. Use graph operations such as scale(x, 2) or add(x, y) from std/math instead of arithmetic`;
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

/** The numbers inside quantities. Only the core reads them, when it builds the graph. */
const amounts = new WeakMap<Quantity, number>();

/**
 * A number with a unit, from `hz(440)`, `db(-6)`, … It is opaque: its number goes
 * into the graph, never into JavaScript, so every use of it stays editable.
 */
export class Quantity<U extends Unit = Unit> {
  constructor(
    value: number,
    readonly unit: U,
    /** @internal The call that wrote it, so the editor can find the number. */
    readonly site?: number,
    /** @internal The number is the written one times this (`khz` is 1000). */
    readonly scale: number = 1,
  ) {
    amounts.set(this, value);
  }

  [Symbol.toPrimitive](): never {
    throw new TypeError(notANumber(`${this.unit} value`));
  }
}

// Not in the type, so editors flag `.value`; at run time it explains what to do instead.
Object.defineProperty(Quantity.prototype, "value", {
  get(this: Quantity): never {
    throw new TypeError(notANumber(`${this.unit} value`));
  },
});

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
type IrNode = { id: string; op: string; src?: string; origin?: Origin; within?: number[]; [k: string]: unknown };

/** Component calls being built right now, outermost first. */
const frames: number[] = [];

class ModuleBuild {
  counter = 0;
  root: Scope;
  exports: Record<string, string> = {};
  imports = new Map<string, Signal>();
  time = new Map<string, Signal>();
  constructor(
    /** `song`, or the track's key. Code may see it; labels it never sees (ID-10). */
    readonly key: string,
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
  if (frames.length) node.within = [...frames];
  scope.nodes.push(node);
  return id;
}

const at = (site: number | undefined): Origin | undefined => (site === undefined ? undefined : { site });

/** What each call returned, when it was a signal: how the editor finds what a binding holds. */
type Output = { site: number; module: string; ref: string; top: boolean };
let outputs: Output[] = [];

function produced<T>(site: number | undefined, value: T): T {
  if (site !== undefined && value instanceof Signal) {
    outputs.push({ site, module: value.scope.module.key, ref: value.ref, top: value.scope.parent === null });
  }
  return value;
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
      throw new Error(`${what}: this signal belongs to "${v.scope.module.key}". Export it from that track and read it as <track>.<export>`);
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
    return push(scope, { op: "const", value: amounts.get(v)!, unit: v.unit }, origin);
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
  return produced(site, new KernelNode<K>(push(scope, { op: "kernel", kernel: id, inputs: ins }, at(site)), scope));
}

/**
 * A hosted plugin (CLAP), e.g. `plugin("clap:org.surge-synth-team.surge-xt", { key: "lead", notes })`.
 * Inputs are `in` (audio), `notes` (a notes lane) and any parameter by its port name
 * (`m2 plugins <id>` lists them). `key` names the plugin's saved state: it is filed under
 * the track, the `key()` frames around the call, then this key (ID-8). A plugin without
 * a key inside a `key()` frame takes the frame's key; one with neither has no saved state.
 */
export function plugin(id: string, inputs: { key?: string; [port: string]: Value | string | undefined } = {}): KernelNode<AudioKind> {
  const site = takeSite();
  const scope = need(`plugin("${id}")`);
  const m = scope.module;
  const ins: Record<string, string> = {};
  let key: string | undefined;
  for (const [k, v] of Object.entries(inputs)) {
    if (v === undefined) continue;
    if (k === "key") key = checkKey("the plugin key", v, m.key);
    else if (k === "state") throw new Error(`plugin("${id}"): \`state\` was replaced by \`key: "<key>"\`, which files the plugin's state in its track's data`);
    else {
      if (typeof v === "string") throw new Error(`plugin("${id}") input "${k}": expected a signal or a number, got a string`);
      ins[k] = ref(v, `${id} input "${k}"`);
    }
  }
  const op: Record<string, unknown> = { op: "plugin", plugin: id, inputs: ins };
  const chain = keyFrames(m);
  if (key !== undefined) {
    const frame = chain[chain.length - 1];
    const here = place(site);
    const first = frame.plugins.get(key);
    if (first) {
      const where = chain.length > 1 ? `the frame ${chainName(chain)} of "${m.key}"` : `"${m.key}"`;
      throw new BuildError("key.duplicate", `two plugins have the key "${key}" in ${where}: at ${describe(first)}, and at ${describe(here)}. Rename one, or wrap one in key()`, m.key);
    }
    frame.plugins.set(key, here);
    op.key = key;
  }
  if (chain.length > 1) op.frames = chain.slice(1).map((f) => f.key);
  return produced(site, new KernelNode<AudioKind>(push(scope, op, at(site)), scope));
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
  return produced(site, new Signal<AudioKind>(id, outer));
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
  feedbacks.push({ node: () => scope.nodes.find((n) => n.id === id), module: scope.module.key });
  return produced(site, new Feedback<K>(id, scope));
}

// ---------------------------------------------------------------- keys

/**
 * A build error with a stable code from the contract (DIAG-2), such as `key.duplicate`.
 * The first one fails the build even if song code catches it.
 */
class BuildError extends Error {
  constructor(
    readonly code: string,
    message: string,
    /** The key of the module it is about. */
    readonly module?: string,
  ) {
    super(message);
    firstError ??= this;
  }
}

let firstError: BuildError | undefined;

const host = globalThis as any;

/** A key, checked against the key rule (ID-1) and never normalised: `key.invalid` suggests a spelling (ID-2). */
function checkKey(what: string, k: unknown, module?: string): string {
  if (typeof k !== "string") throw new BuildError("key.invalid", `${what} must be a string such as "lead-vox", not ${k === null ? "null" : typeof k}`, module);
  const invalid: string | undefined = host.__m2_key(what, k);
  if (invalid) throw new BuildError("key.invalid", invalid, module);
  return k;
}

/** An evaluation of a call: its site, and the stack it ran in. Described only for an error. */
type Place = { site?: number; stack: string };

function place(site: number | undefined): Place {
  return { site, stack: new Error().stack ?? "" };
}

/**
 * Where an evaluation was, in the TypeScript: the call, then what called it. A call in a
 * helper or a loop has one site that runs more than once; the callers tell them apart (ID-5).
 */
function describe(p: Place): string {
  const at: string[] = [];
  for (const line of p.stack.split("\n")) {
    const m = /([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(line) ?? /([^\s()]+):(\d+)\)?\s*$/.exec(line);
    if (!m || m[1] === "@m2/core" || m[1] === "<main>") continue;
    at.push(host.__m2_where(`${m[1]}:${m[2]}:${m[3] ?? 1}`));
  }
  const call: string | undefined = p.site === undefined ? undefined : host.__m2_site_src(p.site);
  const [first, ...callers] = at;
  const here = call ?? first ?? "an unknown place";
  return callers.length ? `${here} (called from ${callers.join(", ")})` : here;
}

/** A `key()` frame, or a module's top level: the keys used directly inside it, and where first. */
type KeyFrame = { key: string; frames: Map<string, Place>; plugins: Map<string, Place> };

/** A lane as declared so far. Kept here, out of reach of song code, since it holds a label. */
type LaneRecord = { kind: string; default?: number; label?: string; chains: Map<string, Place>; shared: boolean; first: Place };

/** What a module build keeps that song code must not reach: lanes (with labels) and open frames. */
const moduleInfo = new WeakMap<ModuleBuild, { lanes: Map<string, LaneRecord>; frames: KeyFrame[] }>();

function infoOf(m: ModuleBuild) {
  let i = moduleInfo.get(m);
  if (!i) moduleInfo.set(m, (i = { lanes: new Map(), frames: [{ key: "", frames: new Map(), plugins: new Map() }] }));
  return i;
}

/** The open frames of a module: its top level, then each `key()` frame, outermost first. */
function keyFrames(m: ModuleBuild): KeyFrame[] {
  return infoOf(m).frames;
}

function chainName(chain: KeyFrame[]): string {
  return chain.length > 1 ? chain.slice(1).map((f) => `key("${f.key}")`).join(" › ") : "the top level";
}

/**
 * A keyed frame (ID-7): `key("chain-l", () => vocalChain(x))` runs `fn` and returns what
 * it returns. A key is unique within its parent frame (ID-5). Plugins built inside file
 * their state under the frame's key (ID-8); lanes don't move, since they are filed by
 * track and lane key only (ID-6).
 */
export function key<T>(k: string, fn: () => T): T {
  const site = takeSite();
  const m = need("key()").module;
  checkKey("the key() frame key", k, m.key);
  if (typeof fn !== "function") throw new Error(`key("${k}", fn): fn must be a function that builds what the key names`);
  const chain = keyFrames(m);
  const parent = chain[chain.length - 1];
  const here = place(site);
  const first = parent.frames.get(k);
  if (first) {
    throw new BuildError("key.duplicate", `key("${k}") is used twice in ${chainName(chain)} of "${m.key}": at ${describe(first)}, and at ${describe(here)}. Rename one`, m.key);
  }
  parent.frames.set(k, here);
  chain.push({ key: k, frames: new Map(), plugins: new Map() });
  try {
    return produced(site, fn());
  } finally {
    chain.pop();
  }
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
    if (site === undefined) return fn(...args);
    frames.push(site);
    try {
      return produced(site, fn(...args));
    } finally {
      frames.pop();
    }
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

/** A lane's options. */
export type LaneOptions = {
  /** Display text, such as "Hook". The key is the lane's identity; this is never an address. */
  name?: string;
  /** Allow declaring the lane from several `key()` frames, which then all read it (ID-6). */
  shared?: boolean;
};

/** What a track or song function receives. */
export type Ctx = {
  /** The module's key: `song`, or the track's key. Code never sees a label (ID-10). */
  key: string;
  lane: {
    /** Notes from the timeline. */
    notes(key: string, opts?: LaneOptions): Events;
    /** An automation lane; `fallback` applies where no clip is. */
    control<U extends Unit>(key: string, unit: U, fallback?: number | Quantity<U>, opts?: LaneOptions): Control<U>;
    /** A 0..1 lane that is 0 where no clip is. Use it to switch behaviour on and off over time. */
    mask(key: string, opts?: LaneOptions): Mask;
    /** Audio clips from the timeline (stereo unless `channels` is 1). */
    audio(key: string, channels?: 1 | 2, opts?: LaneOptions): Audio;
  };
  time: { readonly beats: Control<"beats">; readonly seconds: Control<"seconds"> };
};

/** Options in `what`, checked against their allowed types, so a misspelt one isn't ignored. */
function options(what: string, o: unknown, allowed: Record<string, "string" | "boolean">): Record<string, unknown> {
  if (o === undefined) return {};
  if (o === null || typeof o !== "object" || Array.isArray(o)) throw new Error(`${what}: options must be an object such as { name: "Lead Vox" }`);
  for (const [k, v] of Object.entries(o)) {
    const want = allowed[k];
    if (!want) throw new Error(`${what}: unknown option "${k}" (options: ${Object.keys(allowed).join(", ")})`);
    if (v !== undefined && typeof v !== want) throw new Error(`${what}: option "${k}" must be a ${want}`);
    if (k === "name" && v === "") throw new Error(`${what}: a name can't be empty; leave it out to show the key`);
  }
  return o as Record<string, unknown>;
}

function laneKind(kind: string, fallback: number | undefined): string {
  return fallback === undefined ? kind : `${kind} with fallback ${fallback}`;
}

/**
 * Declare a lane of `m`. Declared again with the same kind and settings, it is the same
 * lane; with others it is `lane.conflict`. Declared from two `key()` frame chains it is
 * `lane.shared`, unless every declaration says `shared: true` (ID-6).
 */
function declareLane(m: ModuleBuild, k: string, kind: string, fallback: number | undefined, opts: unknown, site: number | undefined) {
  checkKey("the lane key", k, m.key);
  const o = options(`lane "${k}"`, opts, { name: "string", shared: "boolean" }) as LaneOptions;
  const lanes = infoOf(m).lanes;
  const chain = keyFrames(m);
  const where = chain.slice(1).map((f) => f.key).join("/");
  const here = place(site);
  const have = lanes.get(k);
  if (!have) {
    lanes.set(k, { kind, default: fallback, label: o.name, chains: new Map([[where, here]]), shared: o.shared === true, first: here });
    return;
  }
  if (have.kind !== kind || have.default !== fallback || (o.name !== undefined && have.label !== undefined && o.name !== have.label)) {
    const was = laneKind(have.kind, have.default) + (have.label !== undefined ? ` named "${have.label}"` : "");
    const now = laneKind(kind, fallback) + (o.name !== undefined ? ` named "${o.name}"` : "");
    throw new BuildError(
      "lane.conflict",
      `lane "${k}" of "${m.key}" is declared as ${was} at ${describe(have.first)}, and again as ${now} at ${describe(here)}. A lane has one kind and one set of settings; give the other its own key`,
      m.key,
    );
  }
  have.label ??= o.name;
  have.shared &&= o.shared === true;
  const other = [...have.chains].find(([c]) => c !== where);
  if (!have.chains.has(where)) have.chains.set(where, here);
  if (other && !have.shared) {
    const name = (c: string) => (c === "" ? "the top level" : c.split("/").map((f) => `key("${f}")`).join(" › "));
    throw new BuildError(
      "lane.shared",
      `lane "${k}" of "${m.key}" is declared in ${name(other[0])} at ${describe(other[1])}, and in ${name(where)} at ${describe(here)}. ` +
        `Lanes are filed by track and lane key only, so both would read one lane: give each its own key, or say shared: true in every declaration`,
      m.key,
    );
  }
}

function ctxFor(m: ModuleBuild): Ctx {
  const laneNode = (id: string, unit?: Unit, site?: number) => new Signal<any>(push(m.root, { op: "lane", lane: id }, at(site)), m.root, unit);
  const time = (of: "beats" | "seconds") => {
    let s = m.time.get(of);
    if (!s) m.time.set(of, (s = new Signal(push(m.root, { op: "time", of }), m.root, of)));
    return s as any;
  };
  return {
    key: m.key,
    lane: {
      notes(key, opts) {
        const site = takeSite();
        declareLane(m, key, "events<note>", undefined, opts, site);
        return produced(site, laneNode(key, undefined, site));
      },
      control(key, unit, fallback, opts) {
        const site = takeSite();
        const v = fallback instanceof Quantity ? amounts.get(fallback)! : fallback;
        declareLane(m, key, `control<${unit}>`, v ?? 0, opts, site);
        return produced(site, laneNode(key, unit, site));
      },
      mask(key, opts) {
        const site = takeSite();
        declareLane(m, key, "control<unit01>", 0, opts, site);
        return produced(site, laneNode(key, "unit01", site));
      },
      audio(key, channels = 2, opts) {
        const site = takeSite();
        declareLane(m, key, `audio<${channels}>`, undefined, opts, site);
        return produced(site, laneNode(key, undefined, site));
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

/** A track's options. */
export type TrackOptions = {
  /** Display text, such as "Lead Vox": what the app shows. Never an address, and code can't read it (ID-10). */
  name?: string;
};

/** Registered tracks by key. Private: it holds labels, which song code must never see. */
const tracks = new Map<string, { fn: TrackFn<any>; label?: string; src?: string; at: Place }>();
const TRACK = Symbol("m2.track");

/**
 * A track: a module with lanes on the timeline, named by its key (ID-4):
 * `track("lead-vox", { name: "Lead Vox" }, fn)`, or `track("kick", fn)` to show the key.
 * The key files the track's data and must be unique in the project (ID-5). Return a
 * signal (its `out`) or an object of named exports. Other modules read exports as
 * `track.exportName`.
 */
export function track<E extends Exports>(key: string, fn: TrackFn<E>): Handle<E>;
export function track<E extends Exports>(key: string, opts: TrackOptions, fn: TrackFn<E>): Handle<E>;
export function track<E extends Exports>(key: string, a: TrackOptions | TrackFn<E>, b?: TrackFn<E>): Handle<E> {
  const site = takeSite();
  checkKey("the track key", key);
  if (key === "song") throw new BuildError("key.invalid", `"song" is reserved for the root module; give the track another key`);
  const [opts, fn]: [unknown, unknown] = typeof a === "function" ? [undefined, a] : [a, b];
  if (typeof fn !== "function") throw new Error(`track("${key}"): write track("${key}", fn), or track("${key}", { name: "…" }, fn)`);
  const o = options(`track("${key}")`, opts, { name: "string" }) as TrackOptions;
  const here = place(site);
  const first = tracks.get(key);
  if (first) {
    throw new BuildError("key.duplicate", `two tracks have the key "${key}": one at ${describe(first.at)}, another at ${describe(here)}. Track keys are unique in the project; rename one`, key);
  }
  tracks.set(key, { fn: fn as TrackFn<E>, label: o.name, src: callsite(), at: here });
  return new Proxy({} as Handle<E>, {
    get(_, prop) {
      if (prop === TRACK) return key;
      if (typeof prop !== "string" || prop === "then" || prop === "toJSON") return undefined;
      const scope = need(`${key}.${prop}`);
      const m = scope.module;
      const id = `${key}.${prop}`;
      let s = m.imports.get(id);
      if (!s) m.imports.set(id, (s = new Signal(push(m.root, { op: "import", module: key, export: prop }), m.root)));
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
    if (!(v instanceof Signal)) throw new Error(`${m.key}: export "${k}" must be a signal`);
    if (v.scope !== m.root) throw new Error(`${m.key}: export "${k}" must not come from inside a poly voice`);
    m.exports[k] = v.ref;
  };
  if (value instanceof Signal) one("out", value);
  else if (value && typeof value === "object") for (const [k, v] of Object.entries(value)) one(k, v);
  else throw new Error(`${m.key} must return a signal or an object of signals`);
}

function run(key: string, isTrack: boolean, fn: (ctx: Ctx) => unknown, src?: string): ModuleBuild {
  const m = new ModuleBuild(key, isTrack, src);
  const prev = current;
  current = m.root;
  try {
    exportsOf(m, fn(ctxFor(m)));
  } finally {
    current = prev;
  }
  return m;
}

let built = false;

/** @internal Called by the host with the song module's default export, once. */
export function __build(root: unknown): string {
  // Its result holds every label; song code calling it would see them (ID-10).
  if (built) throw new Error("__build is the host's: song code must not call it");
  built = true;
  if (firstError) throw firstError;
  const r = root as any;
  if (!r || r[TRACK] !== "song") throw new Error("song.ts must `export default song(...)`");
  outputs = [];
  const modules = [run("song", false, r.fn, r.src)];
  for (const [key, t] of tracks) modules.push(run(key, true, t.fn, t.src));
  if (firstError) throw firstError;
  for (const f of feedbacks) {
    const n = f.node();
    if (n && n.input === null) throw new Error(`${f.module}: a feedback() was never given its input with .set()`);
  }
  (globalThis as any).__m2_outputs = JSON.stringify(outputs);
  return JSON.stringify({
    ir: 1,
    root: { module: "song", export: "out" },
    modules: modules.map((m) => {
      const label = tracks.get(m.key)?.label;
      const lanes = [...infoOf(m).lanes].map(([key, l]) => ({ key, kind: l.kind, default: l.default, label: l.label }));
      return { key: m.key, label: m.track ? label : undefined, track: m.track, lanes, nodes: m.root.nodes, exports: m.exports, src: m.src };
    }),
  });
}

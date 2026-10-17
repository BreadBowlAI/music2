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

/**
 * The call site the running call was made from (set by instrumentation), once. It is the
 * call's only when `args`, what the SDK function received, end as that call's arguments do:
 * a function handed to native code (`xs.map(lowpass)` passes an index and the array) never
 * takes the site of the call that handed it over (ID-15).
 */
function takeSite(args: ArrayLike<unknown>): number | undefined {
  const g = globalThis as any;
  const s = g.__m2_site;
  const ends: unknown = g.__m2_ends;
  const last: unknown = g.__m2_last;
  g.__m2_site = -1;
  g.__m2_last = undefined;
  if (typeof s !== "number" || s < 0) return undefined;
  if (ends === 1) return args.length === 0 ? s : undefined;
  if (ends === 2) {
    if (!Array.isArray(last) || last.length > args.length) return undefined;
    const from = args.length - last.length;
    for (let i = 0; i < last.length; i++) if (!Object.is(args[from + i], last[i])) return undefined;
    return s;
  }
  return args.length > 0 && Object.is(args[args.length - 1], last) ? s : undefined;
}

/**
 * Every unit constructor: the unit, what its argument is multiplied by, and whether it is the
 * unit's canonical spelling, the one a value in base units is written with (exactly one per
 * unit). The one table of spellings: the constructors below, the `control.<name>` builders and
 * the table the build emits for editors (PKG-11) are all made from it. Units themselves are the
 * IR's closed set (PKG-10): a package may add a spelling of a unit, never a unit.
 */
const UNITS = {
  hz: ["hz", 1, true],
  khz: ["hz", 1000, false],
  db: ["db", 1, true],
  linear: ["gain", 1, true],
  semis: ["semitones", 1, true],
  secs: ["seconds", 1, true],
  ms: ["seconds", 0.001, false],
  beats: ["beats", 1, true],
  frac: ["unit01", 1, true],
  bipolar: ["bipolar", 1, true],
  scalar: ["scalar", 1, true],
} as const satisfies Record<string, readonly [Unit, number, boolean]>;

/** A unit constructor's name, such as `hz` or `ms`. */
export type Spelling = keyof typeof UNITS;
type UnitOf<S extends Spelling> = (typeof UNITS)[S][0];

const unitFn = <S extends Spelling>(name: S) => {
  const [unit, scale] = UNITS[name];
  return function (v: number): Quantity<UnitOf<S>> {
    const site = takeSite(arguments);
    return new Quantity(v * scale, unit as UnitOf<S>, site, scale);
  };
};

export const hz = unitFn("hz");
export const khz = unitFn("khz");
export const db = unitFn("db");
export const linear = unitFn("linear");
export const semis = unitFn("semis");
export const secs = unitFn("secs");
export const ms = unitFn("ms");
export const beats = unitFn("beats");
export const frac = unitFn("frac");
export const bipolar = unitFn("bipolar");
export const scalar = unitFn("scalar");

/** The unit table as the build emits it (PKG-11): every spelling, its unit and scale, and the canonical one per unit. */
function unitTable(): string {
  return JSON.stringify(Object.entries(UNITS).map(([name, [unit, scale, canonical]]) => ({ name, unit, scale, canonical })));
}

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

/** As the IR has it: the call, the argument path, and the declared control the constant is (PKG-3). */
type Origin = { site: number; arg?: string[]; scale?: number; control?: { frame: number; slot: string; inherited?: boolean; smooth?: boolean } };
type IrNode = { id: string; op: string; src?: string; origin?: Origin; frame?: number; [k: string]: unknown };

class ModuleBuild {
  counter = 0;
  root: Scope;
  exports: Record<string, string> = {};
  imports = new Map<string, Signal>();
  /** The module's own time nodes: those read outside any component call or `key()` frame. */
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
  const open = infoOf(scope.module).open;
  if (open.length) node.frame = open[open.length - 1];
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
  const site = takeSite(arguments);
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
  const site = takeSite(arguments);
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
  const site = takeSite(arguments);
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
  const site = takeSite(arguments);
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

/** One evaluation of a component call or a `key()` frame, as the IR has it (`Module.frames`, ID-15). */
type IrFrame = {
  site: number;
  component?: string;
  occurrence: number;
  key?: string;
  parent?: number;
  options?: number;
  sidechain?: { input: string; ref: string }[];
};

/**
 * What a module build keeps that song code must not reach: lanes (with labels), the open
 * `key()` frames, and the frame table with the frames open now and how often each site has
 * run in each parent frame.
 */
type ModuleInfo = { lanes: Map<string, LaneRecord>; frames: KeyFrame[]; table: IrFrame[]; open: number[]; runs: Map<string, number> };
const moduleInfo = new WeakMap<ModuleBuild, ModuleInfo>();

function infoOf(m: ModuleBuild): ModuleInfo {
  let i = moduleInfo.get(m);
  if (!i) {
    i = { lanes: new Map(), frames: [{ key: "", frames: new Map(), plugins: new Map() }], table: [], open: [], runs: new Map() };
    moduleInfo.set(m, i);
  }
  return i;
}

/** Open a frame for the call at `site` in `m`; nodes made until it closes are made in it. None without a site. */
function openFrame(m: ModuleBuild, site: number | undefined, what: { component?: string; key?: string }): number | undefined {
  if (site === undefined) return undefined;
  const info = infoOf(m);
  const parent = info.open.length ? info.open[info.open.length - 1] : undefined;
  const runs = `${parent ?? ""}:${site}`;
  const occurrence = info.runs.get(runs) ?? 0;
  info.runs.set(runs, occurrence + 1);
  const f: IrFrame = { site, occurrence };
  if (what.component !== undefined) f.component = what.component;
  if (what.key !== undefined) f.key = what.key;
  if (parent !== undefined) f.parent = parent;
  info.table.push(f);
  info.open.push(info.table.length - 1);
  return info.table.length - 1;
}

function closeFrame(m: ModuleBuild, frame: number | undefined): void {
  if (frame === undefined) return;
  const open = infoOf(m).open;
  const at = open.lastIndexOf(frame);
  if (at >= 0) open.length = at;
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
  const site = takeSite(arguments);
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
  const frame = openFrame(m, site, { key: k });
  try {
    return produced(site, fn());
  } finally {
    closeFrame(m, frame);
    chain.pop();
  }
}

// ---------------------------------------------------------------- components

/*
 * A component is a function a package (or a project) declares with a descriptor: the signals
 * it reads (inputs), the values that can change while the song plays (controls: knobs, or
 * signals connected to them), the values that shape the graph (settings), its output, how a
 * call is written, and what an editor needs to offer it (PKG-1):
 *
 *   export const delay = component({
 *     id: "delay", label: "Delay",
 *     inputs: { in: port.audio() },
 *     controls: { time: control.secs(0.25).range(0.001, 2).log().smooth(), mix: control.frac(0.3) },
 *     output: port.audio(),
 *     editor: { category: "Delay & Reverb", insert: { time: 0.25, mix: 0.3 } },
 *   }, (x: Audio, { time, mix }: Params) => kernel("std:delay", { in: x, time, mix }));
 *
 * A call is `delay(x)`, `delay(x, { time: 0.5 })`: first the inputs `positional` doesn't name,
 * then the `positional` slots (`lowpass(x, 1000, 0.7)`), then one options object naming any
 * control or setting. Every declared control that isn't optional and isn't connected to a
 * signal becomes exactly one constant, made inside the call's frame, whose origin names the
 * instance and the slot, written or inherited (PKG-3). The body always receives a signal for a
 * control. The defaults are what songs see (PKG-4).
 */

/** Internal state of the declaration objects, out of reach of their types and of song code. */
const specs = new WeakMap<object, any>();
const specOf = <T>(d: object): T => specs.get(d) as T;

declare const PHANTOM: unique symbol;

/** What a control declaration holds. Its numbers are in its spelling (`control.ms(45)`: 45). */
type ControlSpec = {
  unit?: Unit;
  spelled?: Spelling;
  scale: number;
  /** As written in the spelling; absent when the control is required. */
  default?: number;
  range?: [number, number];
  log: boolean;
  strict: boolean;
  /** Other units a value may have, with the range a knob shows for each, in base units. */
  accepts: { unit: Unit; range?: [number, number]; log: boolean }[];
  smooth: boolean;
  optional: boolean;
  label?: string;
};

/**
 * A control: a value that may be written as a number (a knob), connected to a signal, or left
 * out (its default: "inherited"). Every number given to its builder is in its spelling:
 * `control.ms(45).range(5, 1000)` is 45 ms within 5 ms…1 s. `R` is whether a call must give it.
 */
export class ControlDecl<R extends boolean = boolean> {
  declare readonly [PHANTOM]?: R;
  /** @internal */
  constructor(spec: ControlSpec) {
    specs.set(this, spec);
  }
  private with<Q extends boolean = R>(more: Partial<ControlSpec>): ControlDecl<Q> {
    return new ControlDecl<Q>({ ...specOf<ControlSpec>(this), ...more });
  }
  /** The range a knob shows, in the control's spelling. A hint, never a clamp (PKG-8). */
  range(lo: number, hi: number): ControlDecl<R> {
    return this.with({ range: [lo, hi] });
  }
  /** Make the range a validation constraint too: a value outside it is a build error (PKG-8). */
  strict(): ControlDecl<R> {
    return this.with({ strict: true });
  }
  /** A knob that moves logarithmically; needs a range above 0. */
  log(): ControlDecl<R> {
    return this.with({ log: true });
  }
  /** Another unit a value may have (`semitones` for a frequency), and the range, in that unit, a knob shows for it. */
  accepts(unit: Unit, lo?: number, hi?: number, log = false): ControlDecl<R> {
    const range: [number, number] | undefined = lo === undefined && hi === undefined ? undefined : [lo as number, hi as number];
    return this.with({ accepts: [...specOf<ControlSpec>(this).accepts, { unit, range, log }] });
  }
  /** Its value ramps over one block when it changes while playing, for controls that click when stepped (PKG-14). */
  smooth(): ControlDecl<R> {
    return this.with({ smooth: true });
  }
  /** Left out, it makes no node at all: its presence changes the graph (PKG-5). */
  optional(): ControlDecl<false> {
    return this.with<false>({ optional: true });
  }
  /** Display text for its knob; the name is the default. */
  label(text: string): ControlDecl<R> {
    return this.with({ label: text });
  }
}

type ControlBuilders = {
  /** A control with no unit and no default, typed by what flows in (math helpers). */
  any(): ControlDecl<true>;
} & {
  /** A control in this spelling's unit, with a default in this spelling, or none (required). */
  [S in Spelling]: { (): ControlDecl<true>; (v: number): ControlDecl<false> };
};

/** Declare a control: `control.hz(1000)`, `control.ms(45)`, `control.frac(0.3)`, `control.any()`. */
export const control: ControlBuilders = (() => {
  const base = { log: false, strict: false, accepts: [], smooth: false, optional: false };
  const builders: Record<string, (v?: number) => ControlDecl> = { any: () => new ControlDecl({ ...base, scale: 1 }) };
  for (const name of Object.keys(UNITS) as Spelling[]) {
    const [unit, scale] = UNITS[name];
    builders[name] = (v?: number) => new ControlDecl({ ...base, unit, spelled: name, scale, default: v });
  }
  return builders as unknown as ControlBuilders;
})();

/** What a port declaration holds. */
type PortSpec = { kind: "audio" | "control" | "notes" | "any"; unit?: Unit; sidechain: boolean; many: boolean };

/** An input or the output: `S` is the signal it carries, `M` whether it takes any number of signals. */
export class PortDecl<S = unknown, M extends boolean = false> {
  declare readonly [PHANTOM]?: [S, M];
  /** @internal */
  constructor(spec: PortSpec) {
    specs.set(this, spec);
  }
  /** Any number of signals (`sum(a, b, c)`): the last input of a call only. The body gets them as an array. */
  many(): PortDecl<S, true> {
    return new PortDecl<S, true>({ ...specOf<PortSpec>(this), many: true });
  }
}

type PortOptions = {
  /** It only listens (a detector or key input): monitoring mute and solo never act on it (PKG-13, MON-2). */
  sidechain?: boolean;
};

/** Declare an input or the output. */
export const port = {
  audio(o: PortOptions = {}): PortDecl<Audio> {
    return new PortDecl({ kind: "audio", sidechain: o.sidechain === true, many: false });
  },
  control<U extends Unit = Unit>(unit?: U, o: PortOptions = {}): PortDecl<Control<U>> {
    return new PortDecl({ kind: "control", unit, sidechain: o.sidechain === true, many: false });
  },
  notes(): PortDecl<Events> {
    return new PortDecl({ kind: "notes", sidechain: false, many: false });
  },
  any(o: PortOptions = {}): PortDecl<Signal> {
    return new PortDecl({ kind: "any", sidechain: o.sidechain === true, many: false });
  },
};

/** What a setting declaration holds. */
type SettingSpec = { kind: "number" | "int" | "bool" | "choice"; options?: readonly string[]; default?: number | boolean | string };

/** A build-time value: changing one rebuilds the graph. Never a knob or a cell. `R` is whether a call must give it. */
export class SettingDecl<T = unknown, R extends boolean = boolean> {
  declare readonly [PHANTOM]?: [T, R];
  /** @internal */
  constructor(spec: SettingSpec) {
    specs.set(this, spec);
  }
}

/** Declare a setting: `setting.int(12)`, `setting.choice(["sine", "tri"], "sine")`. */
export const setting = {
  number: ((d?: number) => new SettingDecl({ kind: "number", default: d })) as { (): SettingDecl<number, true>; (d: number): SettingDecl<number, false> },
  int: ((d?: number) => new SettingDecl({ kind: "int", default: d })) as { (): SettingDecl<number, true>; (d: number): SettingDecl<number, false> },
  bool: ((d?: boolean) => new SettingDecl({ kind: "bool", default: d })) as { (): SettingDecl<boolean, true>; (d: boolean): SettingDecl<boolean, false> },
  choice: ((options: readonly string[], d?: string) => new SettingDecl({ kind: "choice", options: [...options], default: d })) as {
    <const T extends string>(options: readonly T[]): SettingDecl<T, true>;
    <const T extends string>(options: readonly T[], d: T): SettingDecl<T, false>;
  },
};

type EntryDecl = Record<string, PortDecl<any, false> | ControlDecl<any>>;

/** A dictionary slot: one entry per key the caller writes, each with these inputs and controls (PKG-6). */
export class EachDecl<E extends EntryDecl = EntryDecl> {
  declare readonly [PHANTOM]?: E;
  /** @internal */
  constructor(entry: E) {
    specs.set(this, entry);
  }
}

/**
 * A dictionary slot, such as the mixer's channels: `each({ in: port.audio(), db: control.db(0).optional() })`.
 * The caller's keys are identity on purpose (ID-18) and follow the key rule (ID-1), since they
 * name slots such as `channels.hats.db`. An entry may be a bare signal when the entry declares
 * exactly one input: `kick: kick.out` is `{ in: kick.out }`.
 */
export function each<const E extends EntryDecl>(entry: E): EachDecl<E> {
  return new EachDecl(entry);
}

/** For typing only: a call signature an editor shows, with its parameter names. */
export class Signature<F> {
  declare readonly [PHANTOM]?: F;
}

const SIGNATURE = new Signature<never>();

/**
 * The call signature editors show for a component, for typing only: its parameter names are
 * what signature help shows. It lists the inputs `positional` doesn't name, then the
 * `positional` slots, and leaves out the options object, which is added for you:
 * `signature<(x: Audio, cutoff?: Value, q?: Value) => Audio>()`.
 */
export function signature<F extends (...args: any[]) => any>(): Signature<F> {
  return SIGNATURE as Signature<any>;
}

/** A component's descriptor. */
export type Descriptor = {
  /** Unique in its package, following the key rule (`delay`). The ComponentId is `<package>:<id>` (PKG-9). */
  id: string;
  /** Display text; the id is the default. */
  label?: string;
  /** For editors only (see `signature`). */
  signature?: Signature<any>;
  /** The signals a call reads, in call order. Never knobs. */
  inputs?: Record<string, PortDecl<any, boolean>>;
  /** Values that may change while the song plays: knobs, or signals connected to them. */
  controls?: Record<string, ControlDecl<boolean> | EachDecl<any>>;
  /** Build-time values: changing one rebuilds. */
  settings?: Record<string, SettingDecl<any, boolean>>;
  output?: PortDecl<any, false>;
  /** Slots written in order after the inputs it doesn't name: `["cutoff", "q"]` makes `lowpass(x, 1000)`. */
  positional?: readonly string[];
  /**
   * What an editor offers: `category`, a free string the UI groups (PKG-7), lists it as an
   * effect when it takes audio first, or as an instrument a new track can play when it takes
   * notes first; `insert` is written when the effect is added, each value in its control's
   * spelling.
   */
  editor?: { category?: string; description?: string; insert?: Record<string, number> };
};

/** What a component's body receives after its inputs: a signal per control, a value per setting. */
export type Params = { readonly [name: string]: any };

// The call type: derived from the descriptor (and its `signature`), so an editor checks calls
// against what the component declares, every form an editor writes included.

type Ins<D> = D extends { inputs: infer I extends Record<string, PortDecl<any, boolean>> } ? I : {};
type Ctls<D> = D extends { controls: infer C extends Record<string, ControlDecl<boolean> | EachDecl<any>> } ? C : {};
type Sets<D> = D extends { settings: infer S extends Record<string, SettingDecl<any, boolean>> } ? S : {};
type Pos<D> = D extends { positional: infer P extends readonly string[] } ? P : [];

/** What a call may pass for an input: audio and notes are signals; a control or any input also takes a number. */
type InArg<P> = P extends PortDecl<infer S, any> ? (S extends Audio | Events ? S : Value) : never;
type PortKeys<E> = { [K in keyof E]: E[K] extends PortDecl<any, any> ? K : never }[keyof E];
type CtlKeys<E> = { [K in keyof E]: E[K] extends ControlDecl<any> ? K : never }[keyof E];
type EntryObject<E> = { [K in PortKeys<E>]: InArg<E[K]> } & { [K in CtlKeys<E>]?: Value };
type EntryArg<E> = [PortKeys<E>] extends [never] ? EntryObject<E> : IsUnion<PortKeys<E>> extends true ? EntryObject<E> : InArg<E[PortKeys<E>]> | EntryObject<E>;
type CtlArg<C> = C extends EachDecl<infer E> ? Record<string, EntryArg<E>> : Value;
type SetArg<S> = S extends SettingDecl<infer T, any> ? T : never;
type ArgOf<D, K> = K extends keyof Ins<D>
  ? InArg<Ins<D>[K]>
  : K extends keyof Ctls<D>
    ? CtlArg<Ctls<D>[K]>
    : K extends keyof Sets<D>
      ? SetArg<Sets<D>[K]>
      : never;
type Req<D, K> = K extends keyof Ins<D>
  ? true
  : K extends keyof Ctls<D>
    ? Ctls<D>[K] extends ControlDecl<infer R> ? R : true
    : K extends keyof Sets<D>
      ? Sets<D>[K] extends SettingDecl<any, infer R> ? R : false
      : false;

/**
 * The options object of a call: any control or setting by name. Resolved through `infer`, so an
 * editor's signature help and hover list the options in the order declared (`{ time?: Value;
 * mix?: Value }`) instead of naming this type with the whole descriptor as its argument.
 */
export type CallOptions<D> = { [K in keyof Ctls<D>]?: ArgOf<D, K> } & { [K in keyof Sets<D>]?: ArgOf<D, K> } extends infer O
  ? { -readonly [K in keyof O]: O[K] }
  : never;

type IsUnion<T, U = T> = T extends unknown ? ([U] extends [T] ? false : true) : false;
type NotPositional<D> = Exclude<keyof Ctls<D> | keyof Sets<D>, Pos<D>[number]>;
// Mapped over a type parameter, so a tuple maps to a tuple and `[number]` reads its elements.
type OptionalIn<D, P> = { [I in keyof P]: Req<D, P[I]> extends true ? never : P[I] }[number & keyof P];
type OptionalPositional<D> = OptionalIn<D, Pos<D>>;
/** Whether a call needs the options form: some control or setting isn't positional, or two positional slots may be left out. */
type NeedsOptions<D> = [NotPositional<D>] extends [never] ? (true extends IsUnion<OptionalPositional<D>> ? true : false) : true;

/** Every prefix of a parameter list that still gives each required parameter; a left-out one may be `undefined`. */
type Prefixes<A extends unknown[]> = A extends [] ? [] : A extends [...infer H, unknown] ? A | Prefixes<H> : A;
type RequiredKeys<A extends unknown[]> = { [I in keyof A]-?: {} extends Pick<A, I> ? never : I }[number];
type OrUndefined<A extends unknown[], K> = { [I in keyof A]: A[I] | (I extends K ? never : undefined) };
type Forms<A extends unknown[], O> = Prefixes<OrUndefined<Required<A>, RequiredKeys<A>>> extends infer P ? (P extends A ? [...P, options?: O] : never) : never;

// Without a signature: the inputs `positional` doesn't name, then the positional slots. Their
// order can be read from types only when one input leads; a component with more needs a signature.
type Lead<D> = { [K in keyof Ins<D>]: K extends Pos<D>[number] ? never : K }[keyof Ins<D>];
type LeadTuple<D> = [Lead<D>] extends [never]
  ? []
  : Ins<D>[Lead<D>] extends PortDecl<any, infer M>
      ? M extends true ? InArg<Ins<D>[Lead<D>]>[] : [InArg<Ins<D>[Lead<D>]>]
      : [];
type Positional<D, P extends readonly string[]> = P extends readonly [infer H, ...infer R extends readonly string[]]
  ? Req<D, H> extends true ? [ArgOf<D, H>, ...Positional<D, R>] : [(ArgOf<D, H> | undefined)?, ...Positional<D, R>]
  : [];
type Derived<D> = [...LeadTuple<D>, ...Positional<D, Pos<D>>];
type Out<D> = D extends { output: PortDecl<infer S, any> } ? S : Signal;

/**
 * The type of a component's calls: its signature at each length the required slots allow, each
 * optionally followed by the options object; or the signature itself when no call needs one.
 * Without a signature it is derived from the descriptor, unless several inputs lead the call,
 * whose order the types can't tell: then any arguments are accepted.
 */
export type Call<D> = D extends { signature: Signature<infer F extends (...args: any[]) => any> }
  ? NeedsOptions<D> extends true
    ? (...args: Forms<Parameters<F>, CallOptions<D>>) => ReturnType<F>
    : F
  : true extends IsUnion<Lead<D>>
    ? (...args: any[]) => Out<D>
    : NeedsOptions<D> extends true
      ? (...args: Forms<Derived<D>, CallOptions<D>>) => Out<D>
      : (...args: Derived<D>) => Out<D>;

// A signature lists the inputs `positional` doesn't name, then the positional slots: one that
// lists another number of parameters is a type error at its `component()` call.
type UnionToIntersection<U> = (U extends unknown ? (k: U) => void : never) extends (k: infer I) => void ? I : never;
type LastOf<U> = UnionToIntersection<U extends unknown ? () => U : never> extends () => infer R ? R : never;
type Slots<U, N extends unknown[] = []> = [U] extends [never] ? N : Slots<Exclude<U, LastOf<U>>, [...N, unknown]>;
type SlotCount<D> = [...Slots<Lead<D>>, ...Pos<D>]["length"] & number;
type SignatureFits<D> = D extends { signature: Signature<infer F extends (...args: any[]) => any> }
  ? number extends Required<Parameters<F>>["length"]
    ? unknown
    : Required<Parameters<F>>["length"] extends SlotCount<D>
      ? unknown
      : { signature: `the signature lists ${Required<Parameters<F>>["length"]} parameters, but the descriptor has ${SlotCount<D>}: the inputs positional doesn't name, then the positional slots` }
  : unknown;

/**
 * Declare a component (PKG-1). The old form, `component({ controls: ["1.cutoff"] }, fn)`,
 * still works: it names the argument paths whose numbers are knobs, and gives editors less.
 */
export function component<F extends (...args: any[]) => any>(desc: { controls: string[] }, fn: F): F;
export function component<const D extends Descriptor>(desc: D & SignatureFits<D>, body: (...args: any[]) => unknown): Call<D>;
export function component(desc: any, fn: any): any {
  const site = takeSite(arguments);
  if (desc !== null && typeof desc === "object" && Object.prototype.hasOwnProperty.call(desc, "id")) return declared(desc as Descriptor, fn, site);
  return paths(desc, fn);
}

/**
 * The old form: declare which arguments are controls. Paths are a positional index and then
 * property keys, with `*` for any key: `"1"`, `"1.cutoff"`, `"0.*.pan"`. Each plain number found
 * at a control path becomes a constant node that remembers where it was written, so the editor
 * can show it as a knob and change it live. The function receives a signal there instead.
 */
function paths<F extends (...args: any[]) => any>(desc: { controls: string[] }, fn: F): F {
  const patterns = desc.controls.map((p) => p.split("."));
  return ((...args: unknown[]) => {
    const site = takeSite(args);
    const m = current?.module;
    if (site !== undefined && current) {
      const scope = current;
      for (const [first, ...rest] of patterns) {
        const i = Number(first);
        if (i < args.length) args[i] = controlAt(args[i], rest, [first], site, scope);
      }
    }
    if (site === undefined) return fn(...args);
    // Its consts are made before its frame opens, as they always were: they are the caller's.
    const frame = m && openFrame(m, site, {});
    try {
      return produced(site, fn(...args));
    } finally {
      if (m) closeFrame(m, frame);
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

// ------------------------------------------------ declared components: registration

/** A checked input. */
type In = { name: string; spec: PortSpec };
/** A checked control; a dictionary slot's entry has inputs and controls of its own. */
type Ctl = { name: string; spec: ControlSpec; each?: { inputs: In[]; controls: Ctl[] } };
/** A checked setting. */
type Set_ = { name: string; spec: SettingSpec };
/** A call's slot, in call order. */
type Param = { name: string; kind: "input" | "control" | "setting"; required: boolean; many: boolean; each: boolean };

/** A component as registered: its descriptor checked, and its JSON for editors (PKG-11). */
type Component = {
  id: string;
  /** The ComponentId (PKG-9). */
  cid: string;
  inputs: In[];
  controls: Ctl[];
  settings: Set_[];
  params: Param[];
  /** Where `component()` was called, as `file:line:col`. */
  at: string;
  json: string;
};

/** Every component registered while this build or describe run loaded its modules, by ComponentId. */
const registry = new Map<string, Component>();

/** A component function carries its ComponentId here, so the describe run finds where each is exported. */
const COMPONENT = Symbol("m2.component");

/** Where `component()` was called, from the stack when its call has no site: the first frame outside the SDK. */
function definedAt(): string | undefined {
  for (const line of (new Error().stack ?? "").split("\n")) {
    const m = /([^\s()]+):(\d+):(\d+)\)?\s*$/.exec(line) ?? /([^\s()]+):(\d+)\)?\s*$/.exec(line);
    if (!m || m[1] === "@m2/core" || m[1] === "<main>") continue;
    return m[3] ? `${m[1]}:${m[2]}:${m[3]}` : `${m[1]}:${m[2]}`;
  }
  return undefined;
}

/** The package a module file belongs to, as the host knows it: `{ package: "std", module: "std/fx", file: "std/src/fx.ts" }`. */
type Home = { package: string | null; module: string; file: string };

const UNIT_NAMES: readonly Unit[] = ["hz", "db", "gain", "semitones", "seconds", "beats", "unit01", "bipolar", "scalar"];
const NAME = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

function declared(desc: Descriptor, body: (...args: any[]) => unknown, site: number | undefined): unknown {
  const where = definedAt();
  const file = where?.replace(/:\d+(:\d+)?$/, "") ?? "";
  const home: Home = JSON.parse(host.__m2_home(file));
  // The definition site: the `component(` call itself, which identifies the component across runs.
  const at: string = (site !== undefined ? host.__m2_site_src(site) : undefined) ?? (where ? host.__m2_where(where) : home.file);
  const fail = (message: string): never => {
    throw new BuildError("code.build", `component "${String(desc.id)}" at ${at}: ${message}`);
  };
  const c = check(desc, home, at, fail);
  const known = registry.get(c.cid);
  if (known) {
    // One definition site with one descriptor is one component, however often it runs (a
    // factory); anything else is two components with one id (PKG-9).
    if (known.at !== c.at) fail(`another component has the id "${desc.id}" in ${home.package ?? "the project"}, at ${known.at}. Ids are unique in their package (PKG-9)`);
    if (known.json !== c.json) fail(`it is declared again here with another descriptor (are its defaults computed?). One definition site declares one component`);
  } else registry.set(c.cid, c);
  const call = function (...args: unknown[]): unknown {
    return instance(c, body, args);
  };
  Object.defineProperty(call, COMPONENT, { value: c.cid });
  return call;
}

/** Check a descriptor (a descriptor error is a build error at its `component()` call) and make its JSON. */
function check(desc: Descriptor, home: Home, at: string, fail: (m: string) => never): Component {
  const allowed = ["id", "label", "signature", "inputs", "controls", "settings", "output", "positional", "editor"];
  for (const k of Object.keys(desc)) if (!allowed.includes(k)) fail(`no field "${k}" (fields: ${allowed.join(", ")})`);
  if (typeof desc.id !== "string") fail("its id must be a string such as \"delay\"");
  const bad: string | undefined = host.__m2_key("the component id", desc.id);
  if (bad) fail(bad);
  if (desc.label !== undefined && typeof desc.label !== "string") fail("its label must be a string");
  if (desc.signature !== undefined && !(desc.signature instanceof Signature)) fail("signature must be signature<(...) => …>()");
  const names = new Set<string>();
  const named = (what: string, name: string) => {
    if (!NAME.test(name)) fail(`the ${what} name "${name}" must be a plain name such as cutoff`);
    if (names.has(name)) fail(`"${name}" is declared twice: inputs, controls and settings share one set of names`);
    names.add(name);
  };
  const record = (what: string, v: unknown): [string, unknown][] => {
    if (v === undefined) return [];
    if (v === null || typeof v !== "object" || Array.isArray(v)) fail(`${what} must be an object`);
    return Object.entries(v as object);
  };
  const input = (name: string, d: unknown, where: string): In => {
    if (!(d instanceof PortDecl)) fail(`${where} "${name}" must be declared with port.audio(), port.control(), port.notes() or port.any()`);
    const spec = specOf<PortSpec>(d as object);
    if (spec.unit !== undefined && !UNIT_NAMES.includes(spec.unit)) fail(`${where} "${name}" names no unit "${String(spec.unit)}" (units: ${UNIT_NAMES.join(", ")})`);
    return { name, spec };
  };
  const ctl = (name: string, d: unknown, where: string): Ctl => {
    if (!(d instanceof ControlDecl)) fail(`${where} "${name}" must be declared with control.<unit>(…), control.any() or each(…)`);
    const s = specOf<ControlSpec>(d as object);
    const what = `${where} "${name}"`;
    if (s.default !== undefined && !Number.isFinite(s.default)) fail(`${what}: its default must be a finite number`);
    if (s.range) {
      const [lo, hi] = s.range;
      if (!Number.isFinite(lo) || !Number.isFinite(hi) || !(lo < hi)) fail(`${what}: a range needs finite lo < hi, not ${lo}..${hi}`);
      if (s.log && !(lo > 0)) fail(`${what}: .log() needs a range above 0, not ${lo}..${hi}`);
      if (s.strict && s.default !== undefined && !(s.default >= lo && s.default <= hi)) fail(`${what}: its default ${s.default} is outside its strict range ${lo}..${hi}`);
    } else if (s.log) fail(`${what}: .log() needs a range`);
    else if (s.strict) fail(`${what}: .strict() needs a range`);
    for (const a of s.accepts) {
      if (!UNIT_NAMES.includes(a.unit)) fail(`${what}: accepts names no unit "${String(a.unit)}" (units: ${UNIT_NAMES.join(", ")})`);
      if (a.unit === s.unit) fail(`${what}: accepts its own unit, ${a.unit}; give the range with .range()`);
      if (a.range) {
        const [lo, hi] = a.range;
        if (!Number.isFinite(lo) || !Number.isFinite(hi) || !(lo < hi)) fail(`${what}: accepts ${a.unit} needs finite lo < hi`);
        if (a.log && !(lo > 0)) fail(`${what}: accepts ${a.unit} as log needs a range above 0`);
      } else if (a.log) fail(`${what}: accepts ${a.unit} as log needs a range`);
    }
    if (s.label !== undefined && typeof s.label !== "string") fail(`${what}: its label must be a string`);
    return { name, spec: s };
  };

  const inputs = record("inputs", desc.inputs).map(([name, d]) => (named("input", name), input(name, d, "the input")));
  const controls = record("controls", desc.controls).map(([name, d]): Ctl => {
    named("control", name);
    if (!(d instanceof EachDecl)) return ctl(name, d, "the control");
    const entry = specOf<EntryDecl>(d);
    const ins: In[] = [];
    const ctls: Ctl[] = [];
    const seen = new Set<string>();
    for (const [k, e] of record(`the entry of "${name}"`, entry)) {
      if (!NAME.test(k) || seen.has(k)) fail(`the entry of "${name}" has a bad or repeated name "${k}"`);
      seen.add(k);
      if (e instanceof PortDecl) {
        const i = input(k, e, `the entry input of "${name}"`);
        if (i.spec.many) fail(`the entry input "${name}.${k}" can't take many signals`);
        ins.push(i);
      } else ctls.push(ctl(k, e, `the entry control of "${name}"`));
    }
    return { name, spec: { scale: 1, log: false, strict: false, accepts: [], smooth: false, optional: false }, each: { inputs: ins, controls: ctls } };
  });
  const settings = record("settings", desc.settings).map(([name, d]): Set_ => {
    named("setting", name);
    if (!(d instanceof SettingDecl)) fail(`the setting "${name}" must be declared with setting.number(), .int(), .bool() or .choice()`);
    const s = specOf<SettingSpec>(d as object);
    if (s.default !== undefined) settingValue(s, s.default, (m) => fail(`the setting "${name}": its default ${m}`));
    if (s.kind === "choice" && (!s.options?.length || s.options.some((o) => typeof o !== "string"))) fail(`the setting "${name}": a choice needs a list of strings`);
    return { name, spec: s };
  });
  if (desc.output !== undefined && !(desc.output instanceof PortDecl)) fail("its output must be declared with port.audio(), port.control(), …");
  const output = desc.output && input("output", desc.output, "the output").spec;
  if (output?.many) fail("its output is one signal, not many");

  // Call order: the inputs `positional` doesn't name, then `positional`.
  const positional = desc.positional ?? [];
  if (!Array.isArray(positional)) fail("positional must be a list of names");
  const kindOf = (n: string): Param => {
    const i = inputs.find((x) => x.name === n);
    if (i) return { name: n, kind: "input", required: !i.spec.many, many: i.spec.many, each: false };
    const c = controls.find((x) => x.name === n);
    if (c) return { name: n, kind: "control", required: !!c.each || (!c.spec.optional && c.spec.default === undefined), many: false, each: !!c.each };
    const s = settings.find((x) => x.name === n);
    if (s) return { name: n, kind: "setting", required: s.spec.default === undefined, many: false, each: false };
    return fail(`positional names "${n}", which isn't an input, a control or a setting`);
  };
  if (new Set(positional).size !== positional.length) fail("positional names a slot twice");
  const lead = inputs.filter((i) => !positional.includes(i.name));
  if (lead.length !== inputs.length && lead.length) {
    // Then adding an input could move the arguments of calls already written (D16).
    fail(`positional names some inputs but not ${lead.map((i) => `"${i.name}"`).join(", ")}: it names every input or none`);
  }
  const params = [...lead.map((i) => kindOf(i.name)), ...positional.map(kindOf)];
  params.forEach((p, i) => {
    if (p.many && i !== params.length - 1) fail(`the input "${p.name}" takes many signals, so it must come last in the call`);
  });
  const firstOptional = params.findIndex((p) => !p.required && !p.many);
  if (firstOptional >= 0 && params.slice(firstOptional).some((p) => p.required)) {
    fail(`a positional slot that may be left out ("${params[firstOptional].name}") comes before one that may not`);
  }

  // Editor metadata.
  const editor = desc.editor ?? {};
  if (typeof editor !== "object" || editor === null) fail("editor must be an object");
  for (const k of Object.keys(editor)) if (!["category", "description", "insert"].includes(k)) fail(`editor has no field "${k}" (fields: category, description, insert)`);
  if (editor.category !== undefined && (typeof editor.category !== "string" || !editor.category)) fail("editor.category must be a non-empty string");
  if (editor.description !== undefined && typeof editor.description !== "string") fail("editor.description must be a string");
  const insert = record("editor.insert", editor.insert).map(([slot, v]) => {
    const c = controls.find((x) => x.name === slot && !x.each);
    if (!c) fail(`editor.insert names "${slot}", which isn't a control`);
    if (typeof v !== "number" || !Number.isFinite(v)) fail(`editor.insert's "${slot}" must be a finite number`);
    const s = c!.spec;
    if (s.strict && s.range && !((v as number) >= s.range[0] && (v as number) <= s.range[1])) fail(`editor.insert's "${slot}" ${v} is outside its strict range`);
    return { slot, ...spelled(s, v as number) };
  });
  // A category on a component that takes notes and makes audio lists it as an instrument, which
  // a new track plays as `<name>(lane.notes(…))`, so it must build from its notes alone.
  const instrument = editor.category !== undefined && inputs[0]?.spec.kind === "notes";
  if (instrument) {
    const [first, ...rest] = inputs;
    if (rest.length) fail(`an instrument (editor.category, notes first) has one input, not "${rest[0].name}" too`);
    if (lead[0] !== first) fail("an instrument (editor.category, notes first) takes its notes as its first argument");
    if (output?.kind !== "audio") fail("an instrument (editor.category, notes first) outputs audio");
    if (insert.length) fail("an instrument (editor.category, notes first) has no editor.insert: a new track writes none");
    const required = [
      ...controls.filter((c) => c.each || (!c.spec.optional && c.spec.default === undefined)).map((c) => c.name),
      ...settings.filter((s) => s.spec.default === undefined).map((s) => s.name),
    ];
    if (required.length) fail(`an instrument (editor.category, notes first) must build from its notes alone, which leaves out "${required[0]}"`);
  } else if (editor.category !== undefined) {
    // An effect is added after any audio as `<name>(<audio>, <insert>)`, so that must be a whole call.
    const [first, ...rest] = inputs;
    if (!first || first.spec.kind !== "audio" || lead[0] !== first) fail("an effect (editor.category) takes audio as its first argument");
    if (rest.some((i) => !i.spec.many)) fail(`an effect (editor.category) has one input, not "${rest[0].name}" too`);
    if (output?.kind !== "audio") fail("an effect (editor.category) outputs audio");
    const required = [
      ...controls.filter((c) => c.each || (!c.spec.optional && c.spec.default === undefined)).map((c) => c.name),
      ...settings.filter((s) => s.spec.default === undefined).map((s) => s.name),
    ];
    const needed = required.filter((n) => !insert.some((x) => x.slot === n));
    if (needed.length) fail(`an effect (editor.category) must build from its insert alone, which leaves out "${needed[0]}"`);
  }

  const cid = home.package === null ? desc.id : `${home.package}:${desc.id}`;
  const portJson = (s: PortSpec) => ({ kind: s.kind, unit: s.unit ?? null, sidechain: s.sidechain, many: s.many });
  const ctlJson = (c: Ctl): unknown => ({
    name: c.name,
    unit: c.spec.unit ?? null,
    default: c.spec.default === undefined ? null : spelled(c.spec, c.spec.default),
    range: c.spec.range ? c.spec.range.map((x) => x * c.spec.scale) : null,
    log: c.spec.log,
    strict: c.spec.strict,
    accepts: c.spec.accepts.map((a) => ({ unit: a.unit, range: a.range ?? null, log: a.log })),
    smooth: c.spec.smooth,
    optional: c.spec.optional,
    label: c.spec.label ?? null,
    each: c.each ? { inputs: c.each.inputs.map((i) => ({ name: i.name, ...portJson(i.spec) })), controls: c.each.controls.map(ctlJson) } : null,
  });
  const json = JSON.stringify({
    id: cid,
    label: desc.label ?? desc.id,
    module: home.module,
    file: home.file,
    at,
    inputs: inputs.map((i) => ({ name: i.name, ...portJson(i.spec) })),
    controls: controls.map(ctlJson),
    settings: settings.map((s) => ({ name: s.name, kind: s.spec.kind, default: s.spec.default ?? null, options: s.spec.options ?? null })),
    output: output ? portJson(output) : null,
    positional: [...positional],
    editor: { category: editor.category ?? null, description: editor.description ?? "", insert },
  });
  return { id: desc.id, cid, inputs, controls, settings, params, at, json };
}

/** A number written in a control's spelling, as the JSON carries it: its value in base units too. */
function spelled(s: ControlSpec, written: number): { value: number; spelled: Spelling | null; written: number } {
  // The same expression as the unit constructor's, so `control.ms(45)` is exactly `ms(45)` (PKG-4).
  return { value: written * s.scale, spelled: s.spelled ?? null, written };
}

/** Check a setting's value against its kind; `fail` gets what is wrong. */
function settingValue(s: SettingSpec, v: unknown, fail: (m: string) => never): unknown {
  switch (s.kind) {
    case "number":
      if (typeof v !== "number" || !Number.isFinite(v)) fail(`must be a finite number, not ${shown(v)}`);
      break;
    case "int":
      if (typeof v !== "number" || !Number.isInteger(v)) fail(`must be a whole number, not ${shown(v)}`);
      break;
    case "bool":
      if (typeof v !== "boolean") fail(`must be true or false, not ${shown(v)}`);
      break;
    case "choice":
      if (typeof v !== "string" || !s.options?.includes(v)) fail(`must be one of ${(s.options ?? []).map((o) => JSON.stringify(o)).join(", ")}, not ${shown(v)}`);
      break;
  }
  return v;
}

function shown(v: unknown): string {
  if (typeof v === "string") return JSON.stringify(v);
  if (v instanceof Signal) return "a signal";
  if (v instanceof Quantity) return `a ${v.unit} value`;
  return v === null ? "null" : typeof v === "object" ? "an object" : String(v);
}

// ------------------------------------------------ declared components: calls

/** Whether `v` is a track's handle (or the song's): never a signal itself. Reading the symbol makes no import. */
function trackOf(v: unknown): string | undefined {
  if (v === null || (typeof v !== "object" && typeof v !== "function")) return undefined;
  const k = (v as any)[TRACK];
  return typeof k === "string" ? k : undefined;
}

/** A plain object, read only through its own keys: an options object or a dictionary. */
function isPlain(v: unknown): v is Record<string, unknown> {
  if (v === null || typeof v !== "object") return false;
  const proto = Object.getPrototypeOf(v);
  return proto === Object.prototype || proto === null;
}

/** A value given for a slot, and where it is written in the call (an argument path, as origins have). */
type Given = { value: unknown; path: string[] };

/** One call of a declared component: parse, normalise, run the body in the call's frame. */
function instance(c: Component, body: (...args: any[]) => unknown, args: unknown[]): unknown {
  const site = takeSite(args);
  const scope = need(`${c.id}()`);
  const m = scope.module;
  const frame = openFrame(m, site, { component: c.cid });
  try {
    const { inputs, given, options } = parse(c, args, site === undefined);
    const made: Made = { site, frame, scope, id: c.id };
    const params: Record<string, unknown> = {};
    for (const ctl of c.controls) {
      const g = given.get(ctl.name);
      params[ctl.name] = ctl.each ? normaliseEach(made, ctl, g) : normalise(made, ctl.name, ctl.spec, g);
    }
    for (const s of c.settings) {
      const g = given.get(s.name);
      if (g !== undefined) params[s.name] = settingValue(s.spec, g.value, (msg) => fail(c.id, `${s.name} ${msg}`));
      else if (s.spec.default !== undefined) params[s.name] = s.spec.default;
      else fail(c.id, `the setting ${s.name} is missing`);
    }
    if (frame !== undefined) {
      const record = infoOf(m).table[frame];
      // Which argument held the options: only the call knows whether `lowpass(x, opts)` gave a
      // cutoff or options, and an editor writes a control the call leaves out there (PKG-3).
      if (options !== undefined) record.options = options;
      // What each sidechain input received: where monitoring mute and solo must not reach (PKG-13).
      const listens = (input: string, v: unknown) => {
        for (const x of Array.isArray(v) ? v : [v]) {
          if (x instanceof Signal && x.scope.module === m) (record.sidechain ??= []).push({ input, ref: x.ref });
        }
      };
      for (const i of c.inputs.filter((i) => i.spec.sidechain)) listens(i.name, inputs.get(i.name));
      // A dictionary entry's, named by its path as its controls are: `others.hats.side` (PKG-6).
      for (const ctl of c.controls) {
        for (const i of ctl.each?.inputs.filter((i) => i.spec.sidechain) ?? []) {
          for (const [key, entry] of Object.entries(params[ctl.name] as Record<string, Record<string, unknown>>)) {
            listens(`${ctl.name}.${key}.${i.name}`, entry[i.name]);
          }
        }
      }
    }
    return produced(site, body(...c.inputs.map((i) => inputs.get(i.name)), params));
  } finally {
    closeFrame(m, frame);
  }
}

function fail(id: string, message: string): never {
  throw new Error(`${id}(): ${message}`);
}

/**
 * Split a call's arguments into inputs and given controls and settings (the calling
 * convention above), and say which argument held the options, if one did. `native`: the call
 * has no site, so native code may have made it.
 */
function parse(c: Component, args: unknown[], native: boolean): { inputs: Map<string, unknown>; given: Map<string, Given>; options?: number } {
  // `xs.map(component)` passes each element, then its index and the array (D1).
  const n = args.length;
  if (native && n >= 3 && typeof args[n - 2] === "number" && Array.isArray(args[n - 1]) && (args[n - 1] as unknown[]).includes(args[0])) {
    fail(c.id, `was called by Array.map or forEach, which pass an index and the array too. Write xs.map((x) => ${c.id}(x))`);
  }
  const inputs = new Map<string, unknown>();
  const given = new Map<string, Given>();
  const names = () => [...c.controls.map((x) => x.name), ...c.settings.map((x) => x.name)].join(", ") || "none";
  const options = (o: Record<string, unknown>, at: number) => {
    for (const k of Object.keys(o)) {
      const isSlot = c.controls.some((x) => x.name === k) || c.settings.some((x) => x.name === k);
      if (!isSlot) {
        const input = c.inputs.some((x) => x.name === k) ? `; "${k}" is an input, given as an argument` : "";
        fail(c.id, `no option "${k}" (options: ${names()})${input}`);
      }
      const v = o[k];
      if (v === undefined) continue;
      if (given.has(k)) fail(c.id, `"${k}" is given twice: as an argument and as an option`);
      given.set(k, { value: notTrack(c.id, k, v), path: [String(at), k] });
    }
  };
  // The call type's options are `options?`, so a helper may forward ones it wasn't given
  // (`delay(x, echo)` with `echo` left out): `undefined` where the options go is no options.
  const hasOptions = c.controls.length + c.settings.length > 0;
  let at: number | undefined;
  let p = 0;
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    const param = c.params[p];
    const last = i === args.length - 1;
    if (param?.many) {
      if (last && ((isPlain(a) && trackOf(a) === undefined) || (a === undefined && hasOptions))) {
        if (a !== undefined) options(a, i);
        at = i;
        break;
      }
      const xs = (inputs.get(param.name) as unknown[] | undefined) ?? [];
      xs.push(inputValue(c.id, param.name, a));
      inputs.set(param.name, xs);
      continue;
    }
    if (isPlain(a) && trackOf(a) === undefined && !param?.each) {
      if (!last) fail(c.id, `the options object must come last, after every argument`);
      options(a, i);
      at = i;
      break;
    }
    if (!param) {
      if (a === undefined && last && hasOptions) {
        at = i;
        break;
      }
      const most = c.params.length;
      fail(c.id, `takes at most ${most} argument${most === 1 ? "" : "s"} and an options object, but was given ${args.length}`);
    }
    p++;
    if (a === undefined) continue;
    if (param.kind === "input") inputs.set(param.name, inputValue(c.id, param.name, a));
    else given.set(param.name, { value: notTrack(c.id, param.name, a), path: [String(i)] });
  }
  for (const i of c.inputs) {
    if (i.spec.many) inputs.set(i.name, inputs.get(i.name) ?? []);
    else if (!inputs.has(i.name)) fail(c.id, `the input ${i.name} is missing`);
  }
  return { inputs, given, options: at };
}

function notTrack(id: string, slot: string, v: unknown): unknown {
  const k = trackOf(v);
  if (k !== undefined) fail(id, `${slot}: \`${k}\` is a track; use \`${k}.out\` (or another of its exports)`);
  return v;
}

function inputValue(id: string, name: string, v: unknown): unknown {
  notTrack(id, name, v);
  if (v instanceof Signal || v instanceof Quantity || (typeof v === "number" && Number.isFinite(v))) return v;
  return fail(id, `the input ${name} must be a signal, not ${shown(v)}`);
}

/** The call a constant is made for: its site, its frame, and the scope it builds in. */
type Made = { site: number | undefined; frame: number | undefined; scope: Scope; id: string };

/**
 * One declared control of a call (PKG-3): a connected signal passes through; a number, a
 * quantity or the default becomes exactly one constant whose origin names the instance and the
 * slot; an optional control left out makes nothing (PKG-5).
 */
function normalise(made: Made, slot: string, s: ControlSpec, g: Given | undefined): Signal | undefined {
  const v = g?.value;
  if (v instanceof Signal) return v;
  const control = made.frame === undefined ? undefined : { frame: made.frame, slot, ...(s.smooth ? { smooth: true } : {}) };
  const constant = (value: number, unit: Unit | undefined, origin: Origin | undefined) => {
    if (s.strict) {
      // A value is held to the range declared for its own unit: the control's, or the one
      // `accepts` gives that unit (`semis(10)` against 0..127, never against 20..20000 Hz).
      const own = s.unit === undefined || unit === s.unit;
      const range = own ? s.range?.map((x) => x * s.scale) : s.accepts.find((a) => a.unit === unit)?.range;
      if (range && !(value >= range[0] && value <= range[1])) {
        fail(made.id, `${slot} ${value}${own ? "" : ` ${unit}`} is outside its range ${range[0]}..${range[1]}${own ? "" : ` ${unit}`}`);
      }
    }
    const op: Record<string, unknown> = { op: "const", value };
    if (unit) op.unit = unit;
    return new Signal(push(made.scope, op, origin), made.scope, unit);
  };
  if (v === undefined) {
    if (s.optional) return undefined;
    if (s.default === undefined) return fail(made.id, `${slot} is missing`);
    const origin = made.site === undefined ? undefined : { site: made.site, control: { ...control!, inherited: true } };
    return constant(s.default * s.scale, s.unit, origin);
  }
  if (typeof v === "number") {
    if (!Number.isFinite(v)) fail(made.id, `${slot}: ${v} is not a finite number`);
    return constant(v, s.unit, made.site === undefined ? undefined : { site: made.site, arg: g!.path, control });
  }
  if (v instanceof Quantity) {
    const scale = v.scale !== 1 ? { scale: v.scale } : {};
    const origin =
      v.site !== undefined ? { site: v.site, arg: ["0"], ...scale, control } : made.site === undefined ? undefined : { site: made.site, arg: g!.path, control };
    return constant(amounts.get(v)!, v.unit, origin);
  }
  return fail(made.id, `${slot}: expected a number, a quantity like hz(440) or a signal, not ${shown(v)}`);
}

/** A dictionary slot (PKG-6): each entry's inputs pass through and its controls are normalised, in the caller's key order. */
function normaliseEach(made: Made, c: Ctl, g: Given | undefined): Record<string, Record<string, unknown>> {
  const { inputs, controls } = c.each!;
  if (g === undefined) return fail(made.id, `${c.name} is missing`);
  if (!isPlain(g.value) || trackOf(g.value) !== undefined) {
    return fail(made.id, `${c.name} must be an object of entries such as { kick: kick.out }, not ${shown(g.value)}`);
  }
  const out: Record<string, Record<string, unknown>> = {};
  for (const key of Object.keys(g.value)) {
    const bad: string | undefined = host.__m2_key(`the ${c.name} key`, key);
    if (bad) fail(made.id, bad);
    const raw = g.value[key];
    const k = trackOf(raw);
    if (k !== undefined) fail(made.id, `${c.name}.${key}: \`${k}\` is a track; use \`${k}.out\` (or another of its exports)`);
    let entry: Record<string, unknown>;
    if (isPlain(raw)) entry = raw;
    else if (inputs.length === 1) entry = { [inputs[0].name]: raw };
    else return fail(made.id, `${c.name}.${key} must be an object with ${inputs.map((i) => i.name).join(", ")}`);
    for (const f of Object.keys(entry)) {
      if (!inputs.some((i) => i.name === f) && !controls.some((x) => x.name === f)) {
        fail(made.id, `${c.name}.${key} has no "${f}" (it has ${[...inputs, ...controls].map((x) => x.name).join(", ")})`);
      }
    }
    const normal: Record<string, unknown> = {};
    for (const i of inputs) {
      if (entry[i.name] === undefined) fail(made.id, `${c.name}.${key}: the input ${i.name} is missing`);
      normal[i.name] = inputValue(made.id, `${c.name}.${key}.${i.name}`, entry[i.name]);
    }
    for (const x of controls) {
      const v = entry[x.name];
      const given = v === undefined ? undefined : { value: v, path: [...g.path, key, x.name] };
      const signal = normalise(made, `${c.name}.${key}.${x.name}`, x.spec, given);
      if (signal !== undefined) normal[x.name] = signal;
    }
    out[key] = normal;
  }
  return out;
}

/** @internal The describe run's entry: every component registered while one package module (and what it imports) loaded, and where the module exports each. */
export function __describe(spec: string, ns: object): string {
  if (firstError) throw firstError;
  const exports: { component: string; module: string; name: string }[] = [];
  const component = (v: unknown): string | undefined => (typeof v === "function" ? (v as any)[COMPONENT] : undefined);
  for (const [name, v] of Object.entries(ns)) {
    const c = component(v);
    if (c) exports.push({ component: c, module: spec, name });
    else if (isPlain(v)) {
      for (const [k, w] of Object.entries(v)) {
        const d = component(w);
        if (d) exports.push({ component: d, module: spec, name: `${name}.${k}` });
      }
    }
  }
  return JSON.stringify({ components: [...registry.values()].map((c) => JSON.parse(c.json)), exports });
}

// ---------------------------------------------------------------- modules

/** A lane's options. */
export type LaneOptions = {
  /** Display text, such as "Hook". The key is the lane's identity; this is never an address. */
  name?: string;
  /** Allow declaring the lane from several `key()` frames, which then all read it (ID-6). */
  shared?: boolean;
};

/** Song time: where the song is, and whether it plays. Never the engine's own clock (RENDER-7). */
export type Time = {
  /** The song's position in beats. */
  readonly beats: Control<"beats">;
  /** The song's position in seconds. */
  readonly seconds: Control<"seconds">;
  /** 1 while the song plays, 0 while it is stopped. An LFO that steps only while it is 1 waits while stopped. */
  readonly playing: Control<"unit01">;
};

/** Each component call's time nodes, by the scope they are made in, then `<frame>:<what>`. */
const callTimes = new WeakMap<Scope, Map<string, Signal>>();

/**
 * The node that reads `of`. Outside any call (a track's or the song's own code), one per module,
 * as it always was. Inside a component call or a `key()` frame, one per frame, made in the scope
 * being built (a voice has its own): so the node sits inside the call that reads it, in the graph
 * view and in the delta's names (WRITE-8), and adding a call beside another renames neither's.
 */
function timeNode(of: "beats" | "seconds" | "playing"): Signal {
  const scope = need(`time.${of}`);
  const m = scope.module;
  const unit: Unit = of === "playing" ? "unit01" : of;
  const open = infoOf(m).open;
  if (!open.length) {
    let s = m.time.get(of);
    if (!s) m.time.set(of, (s = new Signal(push(m.root, { op: "time", of }), m.root, unit)));
    return s;
  }
  let made = callTimes.get(scope);
  if (!made) callTimes.set(scope, (made = new Map()));
  const k = `${open[open.length - 1]}:${of}`;
  let s = made.get(k);
  if (!s) made.set(k, (s = new Signal(push(scope, { op: "time", of }), scope, unit)));
  return s;
}

/**
 * Song time, for a component's body as for a track's: `time.beats`, `time.seconds` and
 * `time.playing`. The same as `ctx.time`.
 */
export const time: Time = Object.freeze({
  get beats() {
    return timeNode("beats") as Control<"beats">;
  },
  get seconds() {
    return timeNode("seconds") as Control<"seconds">;
  },
  get playing() {
    return timeNode("playing") as Control<"unit01">;
  },
});

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
  /** Song time: the same as `time` from `@m2/core`. */
  time: Time;
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
  return {
    key: m.key,
    lane: {
      notes(key, opts) {
        const site = takeSite(arguments);
        declareLane(m, key, "events<note>", undefined, opts, site);
        return produced(site, laneNode(key, undefined, site));
      },
      control(key, unit, fallback, opts) {
        const site = takeSite(arguments);
        const v = fallback instanceof Quantity ? amounts.get(fallback)! : fallback;
        declareLane(m, key, `control<${unit}>`, v ?? 0, opts, site);
        return produced(site, laneNode(key, unit, site));
      },
      mask(key, opts) {
        const site = takeSite(arguments);
        declareLane(m, key, "control<unit01>", 0, opts, site);
        return produced(site, laneNode(key, "unit01", site));
      },
      audio(key, channels = 2, opts) {
        const site = takeSite(arguments);
        declareLane(m, key, `audio<${channels}>`, undefined, opts, site);
        return produced(site, laneNode(key, undefined, site));
      },
    },
    time,
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
  const site = takeSite(arguments);
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
  // For editors and gestures (PKG-11): every descriptor this build loaded, and the unit table.
  (globalThis as any).__m2_components = `[${[...registry.values()].map((c) => c.json).join(",")}]`;
  (globalThis as any).__m2_units = unitTable();
  return JSON.stringify({
    ir: 2,
    root: { module: "song", export: "out" },
    modules: modules.map((m) => {
      const label = tracks.get(m.key)?.label;
      const lanes = [...infoOf(m).lanes].map(([key, l]) => ({ key, kind: l.kind, default: l.default, label: l.label }));
      const frames = infoOf(m).table;
      return { key: m.key, label: m.track ? label : undefined, track: m.track, lanes, nodes: m.root.nodes, exports: m.exports, src: m.src, frames: frames.length ? frames : undefined };
    }),
  });
}

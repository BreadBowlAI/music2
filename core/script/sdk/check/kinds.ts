// The kinds a descriptor declares (CARD-10 to CARD-17): a choice and a switch among its controls,
// its views and its groups, each typed against this very descriptor, as the code editor types
// them; and a label (CARD-34). A line that should fail carries `@ts-expect-error`, so a type that stops refusing it fails
// the check too. `kinds_tests.rs` builds the same sound and its mistakes, and the build refuses
// each of them again, even through `as any`.

import { component, control, kernel, port, setting, time, view, type Audio, type Choice, type Control, type Ctx, type Events, type Keyset, type Switch } from "@m2/core";
import { lowpass } from "std/filter";
import { curvePicture, envelope, envelopePicture, envelopeWords, lowpass as drawLowpass, lowpassWords } from "std/pictures";

const ENV = { attack: "attack", decay: "decay", sustain: "sustain", release: "release" } as const;

/** A sound that uses every kind a descriptor declares. */
export const every = component(
  {
    id: "every",
    label: "Every kind",
    inputs: { notes: port.notes() },
    controls: {
      shape: control.choice(["saw", "square", "sine"], "saw"),
      sub: control.bool(false),
      cutoff: control.hz(800).range(40, 8000).log(),
      q: control.scalar(0.7),
      attack: control.secs(0.005),
      decay: control.ms(200),
      sustain: control.frac(0.6),
      release: control.secs(0.3),
    },
    settings: { voices: setting.int(8), regions: setting.regions() },
    output: port.audio(),
    editor: {
      category: "Instruments",
      // An envelope and a filter are std's pictures (CARD-96, CARD-97).
      views: { amp: envelopePicture(), filter: curvePicture({ live: false }), keys: view.keys("regions"), scope: view.scope() },
      groups: { Sound: ["shape", "sub", "scope"], Filter: ["filter", "cutoff", "q"], Amp: ["amp", "attack", "decay", "sustain", "release"] },
    },
  },
  // The body is typed from the descriptor: a choice is a `Choice` of its options, a switch a `Switch`.
  (notes, { shape, sub, cutoff, voices }) => {
    const s: Choice<readonly ["saw", "square", "sine"]> = shape;
    const on: Switch = sub;
    const n: number = voices;
    return kernel<Audio>("test:every", { notes, shape: s, sub: on, cutoff, voices: n }) as Audio;
  },
  {
    amp: { draw: (v, p) => envelope(p, v, ENV), words: (v) => envelopeWords(v, ENV) },
    filter: { draw: (v, p) => drawLowpass(p, v, { cutoff: "cutoff", q: "q" }), words: (v) => lowpassWords(v, { cutoff: "cutoff", q: "q" }) },
  },
);

declare const notes: Events, x: Audio, ctx: Ctx;
declare const shapes: Choice<readonly ["saw", "square", "sine"]>, filters: Choice<readonly ["lp", "hp"]>, on: Switch;

// What songs write: a name, a boolean, a choice's or a switch's signal, a lane of names.
export const ok: Audio[] = [
  every(notes),
  every(notes, { shape: "square", sub: true }),
  every(notes, { shape: shapes, sub: on }),
  every(notes, { shape: ctx.lane.control("shape", "choice", "sine"), sub: ctx.lane.control("sub", "switch", true) }),
  every(notes, { shape: ctx.lane.control("shape", "choice") }),
];

// @ts-expect-error a name the choice doesn't have
every(notes, { shape: "squar" });
// @ts-expect-error a choice is written by its name, not its place
every(notes, { shape: 1 });
// @ts-expect-error a choice of other options
every(notes, { shape: filters });
// @ts-expect-error a switch is true or false
every(notes, { sub: 1 });
// @ts-expect-error a choice where a switch goes
every(notes, { sub: shapes });
// @ts-expect-error a choice wired into a frequency
lowpass(x, { cutoff: shapes });
// @ts-expect-error a switch wired into a frequency
lowpass(x, on);
// @ts-expect-error a switch wired into a frequency, in the options
lowpass(x, { cutoff: on });
// @ts-expect-error a lane of names wired into a frequency
lowpass(x, ctx.lane.control("shape", "choice", "sine"));
// @ts-expect-error a switch's lane falls back to true or false
ctx.lane.control("sub", "switch", 1);

// A choice's default is one of its names.
// @ts-expect-error a default the choice doesn't have
control.choice(["saw", "square"], "sine");
// @ts-expect-error a choice has no range: it isn't a number
control.choice(["saw", "square"], "saw").range(0, 1);
// @ts-expect-error a switch has no range either
control.bool(true).log();

/** In a body, a choice wired into a frequency. */
export const wired = component(
  { id: "wired", inputs: { in: port.audio() }, controls: { shape: control.choice(["saw", "square"], "saw") }, output: port.audio() },
  // @ts-expect-error a choice wired into a frequency
  (x, { shape }) => lowpass(x, { cutoff: shape }),
);

/** In a body, a choice handed on to a choice of the same options, and one of other options. */
export const handed = component(
  { id: "handed", inputs: { notes: port.notes() }, controls: { shape: control.choice(["saw", "square", "sine"], "saw"), wave: control.choice(["sine", "saw"], "sine") }, output: port.audio() },
  (notes, { shape, wave }) => {
    every(notes, { shape });
    // @ts-expect-error a choice of other options handed on
    return every(notes, { shape: wave });
  },
);

// Views name settings of this very descriptor (a key map's regions or keymap). Each mistake is
// marked on the view itself, by the code editor's TypeScript as by the project's: the directive
// stands on the line before the view, so a mistake marked on `component` would leave it unused.
// An envelope and a filter are no views since @m2/core 2.4.0 (CARD-100): std's pictures, whose
// dots name the descriptor's own controls, marked where they are named (CARD-62).
export const views = [
  component(
    {
      id: "views-ok",
      controls: { f: control.khz(2), q: control.scalar(1) },
      settings: { zones: setting.keymap() },
      editor: { views: { map: view.keys("zones"), out: view.scope() } },
    },
    () => x,
  ),
  component(
    {
      id: "envelope-missing",
      controls: { a: control.secs(0.1), d: control.secs(0.1), s: control.frac(1), r: control.secs(0.1) },
      editor: { views: { env: envelopePicture() } },
    },
    () => x,
    {
      env: {
        // @ts-expect-error an envelope's dot naming a control this descriptor doesn't have
        draw: (v, p) => envelope(p, v, { attack: "a", decay: "d", sustain: "s", release: "rel" }),
        words: () => "an envelope",
      },
    },
  ),
  component(
    {
      id: "resonance-ok",
      controls: { cutoff: control.hz(800), resonance: control.frac(0.3) },
      editor: { views: { lp: curvePicture({ live: false }) } },
    },
    () => x,
    { lp: { draw: (v, p) => drawLowpass(p, v, { cutoff: "cutoff", resonance: "resonance" }), words: (v) => lowpassWords(v, { cutoff: "cutoff", resonance: "resonance" }) } },
  ),
  component(
    {
      id: "cutoff-a-time",
      controls: { cutoff: control.secs(1), q: control.scalar(1) },
      editor: { views: { lp: curvePicture({ live: false }) } },
    },
    () => x,
    {
      lp: {
        // @ts-expect-error a cutoff that is a time, moved across a frequency
        draw: (v, p) => drawLowpass(p, v, { cutoff: "cutoff", q: "q" }),
        words: () => "a filter",
      },
    },
  ),
  component(
    {
      id: "not-regions",
      settings: { file: setting.asset("a.wav") },
      editor: {
        views: {
          // @ts-expect-error keys over a setting that isn't regions or a keymap
          map: view.keys("file"),
        },
      },
    },
    () => x,
  ),
  component(
    {
      id: "taken",
      controls: { cutoff: control.hz(800), q: control.scalar(1) },
      editor: {
        views: {
          // @ts-expect-error a view named as one of the sound's values
          cutoff: view.scope(),
        },
      },
    },
    () => x,
  ),
];

// @ts-expect-error view.envelope is gone: an envelope is std's picture
view.envelope("a", "d", "s", "r");
// @ts-expect-error view.filter is gone: a filter is std's picture
view.filter("f", "q");

// Groups name this descriptor's values and views, each once. A mistake is marked on `groups`.
export const groups = [
  component({ id: "groups-ok", controls: { a: control.hz(1), b: control.hz(1) }, settings: { c: setting.int(1) }, editor: { groups: { One: ["a", "c"], Two: ["b"] } } }, () => x),
  component({
    id: "unknown",
    controls: { a: control.hz(1) },
    editor: {
      // @ts-expect-error a group naming a key this descriptor doesn't have
      groups: { One: ["a", "b"] },
    },
  }, () => x),
  component({
    id: "twice",
    controls: { a: control.hz(1), b: control.hz(1) },
    editor: {
      // @ts-expect-error a value in two groups
      groups: { One: ["a"], Two: ["b", "a"] },
    },
  }, () => x),
  component({
    id: "twice-in-one",
    controls: { a: control.hz(1) },
    editor: {
      // @ts-expect-error a value twice in one group
      groups: { One: ["a", "a"] },
    },
  }, () => x),
];

// A face names this descriptor's values and views, each once (CARD-40). A mistake is marked on `face`.
export const faces = [
  component({
    id: "face-ok",
    controls: { a: control.secs(0.01), s: control.frac(0.5), b: control.hz(1) },
    settings: { c: setting.int(1) },
    editor: { views: { amp: view.scope() }, groups: { One: ["a", "b"] }, face: ["amp", "a", "c"] },
  }, () => x),
  component({
    id: "face-unknown",
    controls: { a: control.hz(1) },
    editor: {
      // @ts-expect-error a face naming a key this descriptor doesn't have
      face: ["a", "b"],
    },
  }, () => x),
  component({
    id: "face-twice",
    controls: { a: control.hz(1) },
    editor: {
      // @ts-expect-error a value twice on the face
      face: ["a", "a"],
    },
  }, () => x),
];

// A body may type its knobs by their units, as the descriptor declares them.
export const annotated = component(
  { id: "annotated", inputs: { in: port.audio() }, controls: { cutoff: control.hz(800), q: control.scalar(0.7).optional() }, output: port.audio() },
  (x: Audio, { cutoff, q }: { cutoff: Control<"hz">; q: Control<"scalar"> | undefined }) => kernel<Audio>("std:svf", { in: x, cutoff, q }) as Audio,
);
export const unannotated = component(
  { id: "unannotated", controls: { cutoff: control.hz(800), any: control.any() }, output: port.audio() },
  ({ cutoff, any }) => {
    const hz: Control<"hz"> = cutoff;
    const some: Control = any;
    return kernel<Audio>("std:svf", { cutoff: hz, q: some }) as Audio;
  },
);

// A setting's label, as a control's (CARD-34): one `.label()`, which takes text and gives back the
// same declaration, so a labelled value is typed as it was, in its descriptor and in its calls.
export const labelled = component(
  {
    id: "labelled",
    controls: { level: control.db(0).label("loudness") },
    settings: { tableA: setting.choice(["saw", "sine"], "saw").label("table A"), voices: setting.int(8).label("notes at once").optional() },
    output: port.audio(),
  },
  ({ level, tableA, voices }) => {
    const table: "saw" | "sine" = tableA;
    const n: number | undefined = voices;
    return kernel<Audio>("test:labelled", { level, table: table === "saw" ? 0 : 1, voices: n ?? 8 }) as Audio;
  },
);
export const labelledCalls: Audio[] = [labelled(), labelled({ tableA: "sine", voices: 4, level: -6 })];
// @ts-expect-error a labelled choice still takes only its names
labelled({ tableA: "square" });
// @ts-expect-error a label is text
setting.int(8).label(8);
// @ts-expect-error a label is text, a control's too
control.db(0).label(true);

// The song's key as a signal (VOX-12): a kernel takes it, typed `Keyset`; no number does, nor math.
export const tuned = component(
  { id: "tuned", inputs: { in: port.audio() }, output: port.audio() },
  (x: Audio) => kernel<Audio>("test:tune", { in: x, key: time.key }),
);
export const songKey: Keyset = time.key;
// @ts-expect-error a key isn't a number
lowpass(tuned(lowpass(kernel<Audio>("std:saw"))), { cutoff: time.key });
// @ts-expect-error a key isn't a level
export const level: Control<"unit01"> = time.key;

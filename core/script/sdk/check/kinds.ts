// The kinds a descriptor declares (CARD-10 to CARD-17): a choice and a switch among its controls,
// its views and its groups, each typed against this very descriptor, as the code editor types
// them. A line that should fail carries `@ts-expect-error`, so a type that stops refusing it fails
// the check too. `kinds_tests.rs` builds the same sound and its mistakes, and the build refuses
// each of them again, even through `as any`.

import { component, control, kernel, port, setting, view, type Audio, type Choice, type Control, type Ctx, type Events, type Switch } from "@m2/core";
import { lowpass } from "std/filter";

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
      views: { amp: view.envelope("attack", "decay", "sustain", "release"), filter: view.filter("cutoff", "q"), keys: view.keys("regions"), scope: view.scope() },
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

// Views name controls of this very descriptor, in a unit that fits. Each mistake is marked on the
// view itself, by the code editor's TypeScript as by the project's: the directive stands on the
// line before the view, so a mistake marked on `component` would leave it unused.
export const views = [
  component(
    {
      id: "views-ok",
      controls: { a: control.ms(5), d: control.beats(1), s: control.db(-6), r: control.secs(0.2), f: control.khz(2), q: control.scalar(1) },
      settings: { zones: setting.keymap() },
      editor: { views: { env: view.envelope("a", "d", "s", "r"), lp: view.filter("f", "q"), map: view.keys("zones") } },
    },
    () => x,
  ),
  component(
    {
      id: "missing",
      controls: { a: control.secs(0.1), d: control.secs(0.1), s: control.frac(1), r: control.secs(0.1) },
      editor: {
        views: {
          // @ts-expect-error a view naming a control this descriptor doesn't have
          env: view.envelope("a", "d", "s", "rel"),
        },
      },
    },
    () => x,
  ),
  // A filter's q may be a resonance from 0 to 1, as a synth's knob says it (CARD-30).
  component(
    {
      id: "resonance-ok",
      controls: { cutoff: control.hz(800), resonance: control.frac(0.3) },
      editor: { views: { lp: view.filter("cutoff", "resonance") } },
    },
    () => x,
  ),
  component(
    {
      id: "q-in-db",
      controls: { cutoff: control.hz(800), boost: control.db(0) },
      editor: {
        views: {
          // @ts-expect-error a q that is a level in dB
          lp: view.filter("cutoff", "boost"),
        },
      },
    },
    () => x,
  ),
  component(
    {
      id: "wrong-unit",
      controls: { cutoff: control.secs(1), q: control.scalar(1) },
      editor: {
        views: {
          // @ts-expect-error a cutoff that is a time
          lp: view.filter("cutoff", "q"),
        },
      },
    },
    () => x,
  ),
  component(
    {
      id: "a-choice",
      controls: { a: control.secs(0.1), s: control.choice(["low", "high"], "low") },
      editor: {
        views: {
          // @ts-expect-error a sustain that is a choice
          env: view.envelope("a", "a", "s", "a"),
        },
      },
    },
    () => x,
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
          cutoff: view.filter("cutoff", "q"),
        },
      },
    },
    () => x,
  ),
];

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

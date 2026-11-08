// A picture's code, typed by the very descriptor it belongs to (CARD-62), as the code editor types
// it: `v` by the descriptor's values, and each dot's `moves` by its controls and the picture's
// axes. A line that should fail carries `@ts-expect-error`, so a type that stops refusing it fails
// the check too. `pictures_tests.rs` draws the same picture, and the runner refuses each mistake
// again, in words.

import { axis, component, control, kernel, knob, live, port, setting, view, type Audio } from "@m2/core";

const BANDS = [1, 2] as const;
const desc = {
  id: "curve",
  inputs: { x: port.audio() },
  controls: {
    freq1: control.hz(200).range(20, 20000).log(),
    gain1: control.db(0).range(-24, 24),
    q1: control.scalar(1),
    on1: control.bool(true),
    freq2: control.hz(2000).range(20, 20000).log(),
    gain2: control.db(0).range(-24, 24),
    q2: control.scalar(1),
    on2: control.bool(true),
    shape: control.choice(["bell", "shelf"], "bell"),
  },
  settings: { voices: setting.int(4) },
  output: port.audio(),
  editor: {
    views: { curve: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), live: [live.spectrum("in", { tone: "faint" }), live.history(live.meter("reduction"))] }) },
    groups: { "Band 1": ["on1", "freq1", "gain1", "q1"], "Band 2": ["on2", "freq2", "gain2", "q2", "shape"] },
    face: ["curve"],
  },
} as const;

/** A two-band curve: names written out, and names worked out (`freq${b}`), each fitting. */
export const curve = component(desc, (x, v) => kernel<Audio>("test:curve", { x, ...v }), {
  curve: {
    draw: (v, p) => {
      const f: number | null = v.freq1;
      const on: boolean | null = v.on1;
      const shape: "bell" | "shelf" | null = v.shape;
      const voices: number | null = v.voices;
      return [
        p.line([20, f ?? 0, 20000, voices ?? 0], { name: "curve" }),
        p.handle(v.freq1, v.gain1, { x: "freq1", y: "gain1", wheel: "q1", toggles: "on1", mark: "1", opens: "Band 1", off: !on || shape === "shelf" }),
        ...BANDS.map((b) => p.handle(v[`freq${b}`], v[`gain${b}`], { x: `freq${b}`, y: `gain${b}`, wheel: `q${b}`, toggles: `on${b}` })),
        p.part("knobs", () => [p.handle(1, 1, { y: knob("q2") })]),
      ];
    },
    words: (v) => `Band 1: ${v.gain1} dB at ${v.freq1} Hz`,
  },
});

const k = () => kernel<Audio>("test:w", {});
export const wrong = [
  component(desc, k, {
    curve: {
      draw: (v, p) => [
        // @ts-expect-error a value this sound doesn't have
        p.label(1, 1, `${v.freq9}`),
        // @ts-expect-error a switch isn't a number
        p.line([1, v.on1]),
        // @ts-expect-error a dot names freq9, which isn't a control of this sound
        p.handle(1, 1, { x: "freq9" }),
        // @ts-expect-error gain1 is in dB, and the x axis in Hz
        p.handle(1, 1, { x: "gain1" }),
        // @ts-expect-error only a switch flips
        p.handle(1, 1, { toggles: "q1" }),
        // @ts-expect-error the wheel moves a number, not a switch
        p.handle(1, 1, { wheel: "on1" }),
        // @ts-expect-error a knob's drag moves a number, not a switch
        p.handle(1, 1, { y: knob("on2") }),
        // @ts-expect-error a name worked out must fit: on1 and on2 are switches, not frequencies
        ...BANDS.map((b) => p.handle(1, 1, { x: `on${b}` })),
        // @ts-expect-error a dot opens a group of the card
        p.handle(1, 1, { opens: "Band 9" }),
      ],
      words: () => "x",
    },
  }),
  // @ts-expect-error a picture's code is required: words too
  component(desc, k, { curve: { draw: () => [] } }),
  // @ts-expect-error a sound with a picture takes its code
  component(desc, k),
  // @ts-expect-error the third argument names only pictures
  component(desc, k, { curve: { draw: () => [], words: () => "x" }, other: { draw: () => [], words: () => "x" } }),
];

// @ts-expect-error a tone is main, soft or faint, never a colour
live.spectrum("in", { tone: "red" });

/** The descriptor written in the call, as a pack writes it: typed the same. */
export const inline = component(
  {
    id: "inline",
    inputs: { x: port.audio() },
    controls: { cutoff: control.hz(1000), on: control.bool(true) },
    output: port.audio(),
    editor: { views: { curve: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }) } },
  },
  (x, { cutoff }) => kernel<Audio>("test:inline", { x, cutoff }),
  {
    curve: {
      // @ts-expect-error cutof isn't a value of this sound
      draw: (v, p) => [p.handle(v.cutoff, 0, { x: "cutoff", toggles: "on" }), p.label(1, 1, `${v.cutof}`)],
      words: (v) => `cut at ${v.cutoff} Hz`,
    },
  },
);

/** The layers music2 measures (CARD-80), each only on the axes it draws them on; a response's click at a level in dB. */
export const measured = component(
  {
    id: "measured",
    inputs: { notes: port.notes() },
    controls: { decay: control.scalar(1) },
    output: port.audio(),
    editor: {
      views: {
        curve: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }),
        hit: view.picture({ x: axis.time(0.5), y: axis.lin("bipolar", -1, 1) }),
        quiet: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24) }),
      },
    },
  },
  () => kernel<Audio>("test:measured", {}),
  {
    curve: {
      // @ts-expect-error a hit is drawn across seconds and up bipolar, not across hz
      draw: (_v, p) => [p.response({ name: "curve", tone: "soft" }), p.hit({ note: 36 })],
      words: () => "its curve",
    },
    hit: {
      // @ts-expect-error a response is drawn across hz and up db, not across seconds
      draw: (_v, p) => [p.hit({ note: 36, seconds: 0.5, tone: "faint" }), p.response()],
      words: () => "its hit",
    },
    quiet: {
      draw: (_v, p) => {
        // @ts-expect-error a picture reads the song's rate (2.5), and can't change it
        p.rate = 44100;
        return [
          // A click quieter than its thresholds (2.5).
          p.response({ level: -80 }),
          // @ts-expect-error a click's level is a number of dB
          p.response({ level: "quiet" }),
          // The song's rate, a number (2.5).
          p.mark("x", p.rate / 2, "half the rate"),
        ];
      },
      // Words read the song's rate too (2.5), and can't change it either.
      words: (_v, p) => {
        // @ts-expect-error the words' p holds the rate, read only
        p.rate = 44100;
        // @ts-expect-error the words' p draws nothing
        p.line([0, 0]);
        return `its curve under its thresholds, at ${p.rate / 1000} kHz`;
      },
    },
  },
);

// An axis drawn bare (CARD-60, 2.5): with no grid and no numbers, for a picture whose shapes say
// their own words, as a keyboard's keys do.
export const bare = view.picture({ x: axis.lin("scalar", 0, 15, { bare: true }), y: axis.time(1, { bare: false }) });
// @ts-expect-error an axis's only option is bare
export const typo = axis.log("hz", 20, 20000, { bars: true });
// @ts-expect-error bare is true or false
export const one = axis.lin("db", -24, 24, { bare: 1 });

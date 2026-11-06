// A picture's dots, typed against the very descriptor they are declared in (CARD-62), as the code
// editor types them: each name a control of the sound, of the kind its move needs. A line that
// should fail carries `@ts-expect-error`, so a type that stops refusing it fails the check too.
// `pictures_tests.rs` draws the same picture, and the runner refuses each mistake again, in words.

import { axis, component, control, kernel, knob, live, port, view, type Audio } from "@m2/core";

const BANDS = [1, 2] as const;
const controls = {
  freq1: control.hz(200).range(20, 20000).log(),
  gain1: control.db(0).range(-24, 24),
  q1: control.scalar(1),
  on1: control.bool(true),
  freq2: control.hz(2000).range(20, 20000).log(),
  gain2: control.db(0).range(-24, 24),
  q2: control.scalar(1),
  on2: control.bool(true),
};

/** A two-band curve: names written out, and names worked out (`freq${b}`), each fitting. */
export const curve = component(
  {
    id: "curve",
    inputs: { x: port.audio() },
    controls,
    output: port.audio(),
    editor: {
      views: {
        curve: view.picture({
          x: axis.log("hz", 20, 20000),
          y: axis.lin("db", -24, 24),
          live: [live.spectrum("in", { tone: "faint" }), live.history(live.meter("reduction"))],
          draw: (v, p) => [
            p.line([20, 0, 20000, 0], { name: "curve" }),
            p.handle(v.freq1, v.gain1, { x: "freq1", y: "gain1", wheel: "q1", toggles: "on1", mark: "1" }),
            ...BANDS.map((b) => p.handle(v[`freq${b}`], v[`gain${b}`], { x: `freq${b}`, y: `gain${b}`, wheel: `q${b}`, toggles: `on${b}` })),
            p.part("knobs", () => [p.handle(1, 1, { y: knob("q2") })]),
          ],
          words: (v) => `Band 1: ${v.gain1} dB at ${v.freq1} Hz`,
        }),
      },
      face: ["curve"],
    },
  },
  (x, v) => kernel<Audio>("test:curve", { x, ...v }),
);

/** The same sound with its picture's dots `draw` gives: what each mistake below declares. */
const at = <D>(d: D) => d;

export const wrong = [
  component(
    // @ts-expect-error its picture's dot names "freq9", which isn't a control of this sound
    at({ id: "w1", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => [p.handle(1, 1, { x: "freq9" })], words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
  component(
    // @ts-expect-error its picture's dot moves "gain1" on an axis in hz, and "gain1" isn't in hz
    at({ id: "w2", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => [p.handle(1, 1, { x: "gain1" })], words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
  component(
    // @ts-expect-error its picture's dot flips "q1", which isn't a switch
    at({ id: "w3", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => [p.handle(1, 1, { toggles: "q1" })], words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
  component(
    // @ts-expect-error its picture's dot moves "on1" as a number, which it isn't
    at({ id: "w4", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => [p.handle(1, 1, { wheel: "on1" })], words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
  component(
    // @ts-expect-error a name worked out as the code runs must fit every control it can match: `on${b}` is a switch, not a frequency
    at({ id: "w5", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => BANDS.map((b) => p.handle(1, 1, { x: `on${b}` })), words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
  component(
    // @ts-expect-error a dot inside a part is checked as one outside it is
    at({ id: "w6", controls, editor: { views: { c: view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: (v, p) => [p.part("a", () => [p.handle(1, 1, { y: knob("on2") })])], words: () => "x" }) } } }),
    () => kernel<Audio>("test:w", {}),
  ),
];

// @ts-expect-error a tone is main, soft or faint, never a colour
live.spectrum("in", { tone: "red" });
// @ts-expect-error words are required: a picture says itself in words
view.picture({ x: axis.log("hz", 20, 20000), y: axis.lin("db", -24, 24), draw: () => [] });

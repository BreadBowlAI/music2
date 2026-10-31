// @m2/mixing: the tools a mix needs (PACK-26): an eight-band parametric EQ, a gate, a transient
// shaper, a de-esser and a multiband compressor. std keeps the four every mix starts with (eq3,
// compress, limit, duck, in std/fx).
//
// Each declares an `editor.category`, so the editor offers it after any audio and writes its
// `insert` when it is added. Each plays the pack's own kernel (`@m2/mixing:eq` …, built from
// kernels-src/, PACK-21), which smooths its own controls where a step would click, so none is
// `.smooth()`. The pack uses none of std's code.

import { component, control, kernel, port, signature, view, type Audio, type Params } from "@m2/core";

// ---------------------------------------------------------------- the parametric EQ

/** A band's shapes, in the order its kernel reads them (a choice's place, CARD-10). */
const SHAPES = ["low cut", "low shelf", "bell", "high shelf", "high cut"] as const;
type Shape = (typeof SHAPES)[number];

// One band's five values. Each label says its band, so an undo step or a lane names it alone.
const shape = (band: number, d: Shape) => control.choice(SHAPES, d).label(`Shape ${band}`);
const freq = (band: number, hz: number) => control.hz(hz).range(20, 20000).log().label(`Frequency ${band}`);
const gain = (band: number) => control.db(0).range(-24, 24).label(`Gain ${band}`);
const q = (band: number, v: number) => control.scalar(v).range(0.1, 18).log().label(`Q ${band}`);
const on = (band: number, v: boolean) => control.bool(v).label(`On ${band}`);

/**
 * Parametric EQ: eight bands, each a `shape` (low cut, low shelf, bell, high shelf or high cut), a
 * `freq` in Hz, a `gain` in dB (a cut has none), a `q` (how narrow a bell is, how sharp a cut's
 * corner) and an `on` switch, numbered 1 to 8: `eq(x, { gain3: 4, freq3: 300 })`. It starts flat: the
 * low cut (band 1) and the high cut (band 8) off, the rest at 0 dB.
 */
export const eq = component(
  {
    id: "eq",
    label: "Parametric EQ",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      shape1: shape(1, "low cut"), freq1: freq(1, 30), gain1: gain(1), q1: q(1, 0.71), on1: on(1, false),
      shape2: shape(2, "low shelf"), freq2: freq(2, 100), gain2: gain(2), q2: q(2, 0.71), on2: on(2, true),
      shape3: shape(3, "bell"), freq3: freq(3, 250), gain3: gain(3), q3: q(3, 1), on3: on(3, true),
      shape4: shape(4, "bell"), freq4: freq(4, 1000), gain4: gain(4), q4: q(4, 1), on4: on(4, true),
      shape5: shape(5, "bell"), freq5: freq(5, 2500), gain5: gain(5), q5: q(5, 1), on5: on(5, true),
      shape6: shape(6, "bell"), freq6: freq(6, 6000), gain6: gain(6), q6: q(6, 1), on6: on(6, true),
      shape7: shape(7, "high shelf"), freq7: freq(7, 10000), gain7: gain(7), q7: q(7, 0.71), on7: on(7, true),
      shape8: shape(8, "high cut"), freq8: freq(8, 18000), gain8: gain(8), q8: q(8, 0.71), on8: on(8, false),
    },
    output: port.audio(),
    editor: {
      category: "EQ",
      description: "Eight bands to shape the tone: cut the lows or highs, or turn any part up or down",
      // No view draws an EQ's whole curve; the filter view draws a low-pass, which band 8 is at its
      // start (a high cut), so it alone has one (PACK-26, CARD-27).
      views: { curve: view.filter("freq8", "q8") },
      groups: {
        "Band 1": ["on1", "shape1", "freq1", "gain1", "q1"],
        "Band 2": ["on2", "shape2", "freq2", "gain2", "q2"],
        "Band 3": ["on3", "shape3", "freq3", "gain3", "q3"],
        "Band 4": ["on4", "shape4", "freq4", "gain4", "q4"],
        "Band 5": ["on5", "shape5", "freq5", "gain5", "q5"],
        "Band 6": ["on6", "shape6", "freq6", "gain6", "q6"],
        "Band 7": ["on7", "shape7", "freq7", "gain7", "q7"],
        "Band 8": ["on8", "shape8", "freq8", "gain8", "q8", "curve"],
      },
    },
  },
  // Its kernel's inputs are its controls, by the same names.
  (x: Audio, bands: Params) => kernel("@m2/mixing:eq", { in: x, ...bands }),
);

// ---------------------------------------------------------------- dynamics

/**
 * Gate: silences a sound between its notes or hits. It opens when its key gets to `threshold` (dB),
 * and closes once the key has stayed 6 dB under it for `hold` (at least 25 ms), over `release`,
 * down to `range` (dB: −80 is near silence, −10 only a little quieter). `attack` is how fast it
 * opens. Its key is the sound itself, or another track's sound given after it:
 * `gate(pad.out, kick.out)` opens the pad only while the kick plays.
 */
export const gate = component(
  {
    id: "gate",
    label: "Gate",
    signature: signature<(x: Audio, key?: Audio) => Audio>(),
    // `key` only listens (PKG-13): muting the track it comes from must not shut the gate. Given as
    // `many()` so it can be left out; TypeScript allows one, and the body refuses more.
    inputs: { in: port.audio(), key: port.audio({ sidechain: true }).many() },
    controls: {
      threshold: control.db(-40).range(-80, 0),
      attack: control.ms(1).range(0.1, 100).log(),
      hold: control.ms(50).range(0, 500),
      release: control.ms(100).range(5, 2000).log(),
      range: control.db(-80).range(-80, 0),
    },
    output: port.audio(),
    editor: { category: "Dynamics", description: "Silences a track between its sounds", insert: { threshold: -40 } },
  },
  (x: Audio, key: Audio[], { threshold, attack, hold, release, range }: Params) => {
    if (key.length > 1) throw new Error(`gate() listens to one key, not ${key.length}: gate(x, kick.out)`);
    return kernel("@m2/mixing:gate", { in: x, key: key[0] ?? x, threshold, attack, hold, release, range });
  },
);

/**
 * Transient shaper: `attack` (dB) makes each hit's start louder, or softer when below 0; `sustain`
 * (dB) does the same to its tail. 0 and 0 leave the sound as it is.
 */
export const transient = component(
  {
    id: "transient",
    label: "Transient shaper",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      attack: control.db(0).range(-24, 24),
      sustain: control.db(0).range(-24, 24),
    },
    output: port.audio(),
    editor: { category: "Dynamics", description: "Makes each hit punchier or softer, and its tail longer or shorter", insert: { attack: 6, sustain: 0 } },
  },
  (x: Audio, { attack, sustain }: Params) => kernel("@m2/mixing:transient", { in: x, attack, sustain }),
);

/**
 * De-esser: tames the hiss of S and T sounds in a voice. The band around `freq` (Hz: 5 to 8 kHz for
 * most voices) is turned down while it is louder than `threshold` (dB), by at most `amount` (dB).
 * The rest of the sound is never touched.
 */
export const deesser = component(
  {
    id: "deesser",
    label: "De-esser",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      freq: control.hz(6000).range(2000, 16000).log().label("Frequency"),
      threshold: control.db(-30).range(-60, 0),
      amount: control.db(6).range(0, 24),
    },
    output: port.audio(),
    editor: { category: "Dynamics", description: "Tames harsh S sounds in a voice, and nothing else", insert: { freq: 6000, threshold: -30, amount: 6 } },
  },
  (x: Audio, { freq, threshold, amount }: Params) => kernel("@m2/mixing:deesser", { in: x, freq, threshold, amount }),
);

// One band's compressor, as std's `compress` has it: above `threshold` (dB) the level rises only
// 1/`ratio` as fast; `attack` and `release` are how fast it grabs and lets go; `gain` (dB) turns
// the band up or down after.
const threshold = (band: string) => control.db(-18).range(-60, 0).label(`${band} threshold`);
const ratio = (band: string) => control.scalar(2).range(1, 20).log().label(`${band} ratio`);
const attack = (band: string, ms: number) => control.ms(ms).range(0.1, 1000).log().label(`${band} attack`);
const release = (band: string, ms: number) => control.ms(ms).range(5, 5000).log().label(`${band} release`);
const level = (band: string) => control.db(0).range(-24, 24).label(`${band} gain`);

/**
 * Multiband compressor: splits the sound into lows, mids and highs at `lowMid` and `midHigh` (Hz),
 * and compresses each on its own, so a booming bass doesn't squeeze the voice above it. Each band
 * has its `threshold`, `ratio`, `attack`, `release` and `gain` (`lowRatio`, `midGain` …). At ratio
 * 1 and gain 0 everywhere it leaves the level of every frequency as it is.
 */
export const multiband = component(
  {
    id: "multiband",
    label: "Multiband compressor",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      lowMid: control.hz(200).range(40, 2000).log().label("Low to mid"),
      midHigh: control.hz(2500).range(500, 16000).log().label("Mid to high"),
      lowThreshold: threshold("Low"), lowRatio: ratio("Low"), lowAttack: attack("Low", 20), lowRelease: release("Low", 200), lowGain: level("Low"),
      midThreshold: threshold("Mid"), midRatio: ratio("Mid"), midAttack: attack("Mid", 10), midRelease: release("Mid", 150), midGain: level("Mid"),
      highThreshold: threshold("High"), highRatio: ratio("High"), highAttack: attack("High", 5), highRelease: release("High", 100), highGain: level("High"),
    },
    output: port.audio(),
    editor: {
      category: "Dynamics",
      description: "Compresses the lows, mids and highs each on its own",
      groups: {
        Split: ["lowMid", "midHigh"],
        Low: ["lowThreshold", "lowRatio", "lowAttack", "lowRelease", "lowGain"],
        Mid: ["midThreshold", "midRatio", "midAttack", "midRelease", "midGain"],
        High: ["highThreshold", "highRatio", "highAttack", "highRelease", "highGain"],
      },
    },
  },
  (x: Audio, c: Params) =>
    kernel("@m2/mixing:multiband", {
      in: x,
      low_mid: c.lowMid,
      mid_high: c.midHigh,
      low_threshold: c.lowThreshold,
      low_ratio: c.lowRatio,
      low_attack: c.lowAttack,
      low_release: c.lowRelease,
      low_gain: c.lowGain,
      mid_threshold: c.midThreshold,
      mid_ratio: c.midRatio,
      mid_attack: c.midAttack,
      mid_release: c.midRelease,
      mid_gain: c.midGain,
      high_threshold: c.highThreshold,
      high_ratio: c.highRatio,
      high_attack: c.highAttack,
      high_release: c.highRelease,
      high_gain: c.highGain,
    }),
);

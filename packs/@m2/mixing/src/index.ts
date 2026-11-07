// @m2/mixing: the tools a mix needs (PACK-26): an eight-band parametric EQ, a gate, a transient
// shaper, a de-esser, a multiband compressor, and Pump (PACK-28). std keeps the four every mix
// starts with (eq3, compress, limit, duck, in std/fx).
//
// Each declares an `editor.category`, so the editor offers it after any audio and writes its
// `insert` when it is added. Each card's face is its picture (CARD-110 to CARD-116, ./pictures):
// the EQ's curve measured from its own sound with a dot for each band; the gate's levels with the
// line where it opens; one hit through the transient shaper, before and after; the de-esser's
// band over the spectrum; the multiband's split; Pump's dip over a bar. The first five play the pack's own kernel (`@m2/mixing:eq` …, built
// from kernels-src/, PACK-21), which smooths its own controls where a step would click, so none is
// `.smooth()`; they use none of std's code. Pump is std's blocks alone, in TypeScript.

import { component, control, kernel, knob, port, setting, signature, time, type Audio, type Params } from "@m2/core";
import { lfo, range } from "std/control";
import { duck } from "std/fx";
import { blend, scale } from "std/math";
import { curvePicture, dbWords, hzWords } from "std/pictures";
import { band, bandPicture, bandWords, dip, dipPicture, dipWords, hit, hitPicture, hitWords, levelsPicture, RATES, split, splitWords, threshold as thresholdLine } from "./pictures";

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
 * corner, how big a shelf's bump at its corner) and an `on` switch, numbered 1 to 8:
 * `eq(x, { gain3: 4, freq3: 300 })`. It starts flat: the low cut (band 1) and the high cut (band 8)
 * off, the rest at 0 dB. A change never booms: a band crossfades to its new setting.
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
      // Its curve, measured from its own sound, over the spectrum in and out (CARD-111).
      views: { curve: curvePicture() },
      face: ["curve"],
      groups: {
        "Band 1": ["on1", "shape1", "freq1", "gain1", "q1"],
        "Band 2": ["on2", "shape2", "freq2", "gain2", "q2"],
        "Band 3": ["on3", "shape3", "freq3", "gain3", "q3"],
        "Band 4": ["on4", "shape4", "freq4", "gain4", "q4"],
        "Band 5": ["on5", "shape5", "freq5", "gain5", "q5"],
        "Band 6": ["on6", "shape6", "freq6", "gain6", "q6"],
        "Band 7": ["on7", "shape7", "freq7", "gain7", "q7"],
        "Band 8": ["on8", "shape8", "freq8", "gain8", "q8"],
      },
    },
  },
  // Its kernel's inputs are its controls, by the same names.
  (x: Audio, bands: Params) => kernel("@m2/mixing:eq", { in: x, ...bands }),
  {
    // A dot for each band, numbered, in one colour (D14): across its frequency, up and down its
    // gain (a cut's q, at its peak), the wheel its q, a double-click on or off, and its tab.
    curve: {
      draw: (v, p) => [
        p.response({ name: "curve" }),
        ...BANDS.map((b) => {
          const cut = v[`shape${b}`] === "low cut" || v[`shape${b}`] === "high cut";
          const q = v[`q${b}`];
          const y = cut ? (q === null ? null : 20 * Math.log10(q)) : v[`gain${b}`];
          const at = y === null ? null : Math.min(24, Math.max(-24, y));
          const common = { x: `freq${b}`, wheel: `q${b}`, toggles: `on${b}`, opens: `Band ${b}`, mark: `${b}`, off: v[`on${b}`] !== true } as const;
          return cut ? p.handle(v[`freq${b}`], at, { ...common, y: knob(`q${b}`) }) : p.handle(v[`freq${b}`], at, { ...common, y: `gain${b}` });
        }),
      ],
      words: (v) => eqWords(v),
    },
  },
);

/** The EQ's bands, by number. */
const BANDS = [1, 2, 3, 4, 5, 6, 7, 8] as const;

/** What the EQ's curve says (CARD-111): how many bands are on, and what each that changes the sound does. */
function eqWords(v: { readonly [k: string]: unknown }): string {
  const on = BANDS.filter((b) => v[`on${b}`] === true);
  const said = on.flatMap((b) => {
    const [shape, f, g] = [v[`shape${b}`], v[`freq${b}`], v[`gain${b}`]];
    const at = typeof f === "number" ? hzWords(f) : "a frequency set elsewhere";
    if (shape === "low cut") return [`band ${b} cuts the lows below ${at}`];
    if (shape === "high cut") return [`band ${b} cuts the highs above ${at}`];
    if (g === 0) return [];
    return [`band ${b} ${shape} ${typeof g === "number" ? dbWords(g) : "set elsewhere"} at ${at}`];
  });
  const count = `${on.length} ${on.length === 1 ? "band" : "bands"} on`;
  return said.length ? `${count}: ${said.join("; ")}` : `${count}, all at 0 dB: the sound as it is`;
}

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
    // `key` only listens (PKG-13): muting the track it comes from must not shut the gate. A call
    // may leave it out (PKG-18), and its card's Listens to picks it (CARD-35).
    inputs: { in: port.audio(), key: port.audio({ sidechain: true }).optional() },
    controls: {
      threshold: control.db(-40).range(-80, 0),
      attack: control.ms(1).range(0.1, 100).log(),
      hold: control.ms(50).range(0, 500),
      release: control.ms(100).range(5, 2000).log(),
      range: control.db(-80).range(-80, 0),
    },
    output: port.audio(),
    editor: {
      category: "Dynamics",
      description: "Silences a track between its sounds",
      insert: { threshold: -40 },
      // Its levels over the last seconds, with the line where it opens (CARD-112).
      views: { levels: levelsPicture() },
      face: ["levels", "threshold", "attack", "hold", "release", "range"],
    },
  },
  (x: Audio, key: Audio | undefined, { threshold, attack, hold, release, range }: Params) =>
    kernel("@m2/mixing:gate", { in: x, key: key ?? x, threshold, attack, hold, release, range }),
  {
    levels: {
      draw: (v, p) => thresholdLine(p, v, { threshold: "threshold" }),
      words: (v) =>
        v.threshold === null
          ? "The last 3 seconds in and out: its threshold is set by a cable or the code"
          : `Opens when the sound reaches ${dbWords(v.threshold)}; below that it turns it down to ${v.range === null ? "a level set elsewhere" : dbWords(v.range)}. Drag the line`,
    },
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
    editor: {
      category: "Dynamics",
      description: "Makes each hit punchier or softer, and its tail longer or shorter",
      insert: { attack: 6, sustain: 0 },
      // One hit, before and after, from the kernel's own sums (CARD-113).
      views: { hit: hitPicture() },
      face: ["hit", "attack", "sustain"],
    },
  },
  (x: Audio, { attack, sustain }: Params) => kernel("@m2/mixing:transient", { in: x, attack, sustain }),
  { hit: { draw: (v, p) => hit(p, v, { attack: "attack", sustain: "sustain" }), words: (v) => hitWords(v, { attack: "attack", sustain: "sustain" }) } },
);

/**
 * De-esser: tames the hiss of S and T sounds in a voice. The band around `freq` (Hz: 5 to 8 kHz for
 * most voices; a bell about an octave and a half wide) is turned down while it is louder than
 * `threshold` (dB), by at most `amount` (dB), most at `freq`. A loud tone an octave away loses a dB
 * or so while it works; two octaves away, nothing to speak of.
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
    editor: {
      category: "Dynamics",
      description: "Tames harsh S sounds in a voice: turns down only the band they sit in",
      insert: { freq: 6000, threshold: -30, amount: 6 },
      // The band it listens to over the spectrum in and out (CARD-114).
      views: { band: bandPicture() },
      face: ["band", "freq", "threshold", "amount"],
    },
  },
  (x: Audio, { freq, threshold, amount }: Params) => kernel("@m2/mixing:deesser", { in: x, freq, threshold, amount }),
  {
    band: {
      draw: (v, p) => band(p, v, { freq: "freq", threshold: "threshold" }),
      words: (v) => bandWords(v, { freq: "freq", threshold: "threshold", amount: "amount" }),
    },
  },
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
 * Multiband compressor: splits the sound into lows, mids and highs at `lowMid` and `midHigh` (Hz;
 * crossed, the lower one splits low from mid), and compresses each on its own, so a booming bass
 * doesn't squeeze the voice above it. Each band has its `threshold`, `ratio`, `attack`, `release`
 * and `gain` (`lowRatio`, `midGain` …). At ratio 1 and gain 0 everywhere it leaves the level of every
 * frequency as it is.
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
      // The split over the spectrum in and out (CARD-115).
      views: { split: curvePicture() },
      face: ["split", "lowMid", "midHigh", "lowGain", "midGain", "highGain"],
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
  {
    split: {
      draw: (v, p) =>
        split(p, v, { lowMid: "lowMid", midHigh: "midHigh", gains: ["lowGain", "midGain", "highGain"], tabs: ["Low", "Mid", "High"] }),
      words: (v) => splitWords(v, { lowMid: "lowMid", midHigh: "midHigh", gains: ["lowGain", "midGain", "highGain"] }),
    },
  },
);

// ---------------------------------------------------------------- the beat

/**
 * Pump: the sound dips on every beat and swells back, in time with the song, with no sidechain
 * (the pop "pump"). `depth` (dB) is how far it dips at the start of each step, `shape` (0 to 1)
 * how fast it comes back (0: slowly, in a straight line over the whole step; 1: most of the way in
 * its first third), and `rate` how often: every `1/4` note (a beat), `1/8`, `1/16` or `1/2`. At
 * depth 0 it plays the sound as it is (PACK-28).
 */
export const pump = component(
  {
    id: "pump",
    label: "Pump",
    signature: signature<(x: Audio) => Audio>(),
    inputs: { in: port.audio() },
    controls: {
      depth: control.db(-8).range(-24, 0),
      // Read every sample (a blend), so a step would click (PKG-14).
      shape: control.frac(0.5).range(0, 1).smooth(),
    },
    // A menu: how often it dips. Fixed when the song builds, as an LFO's period is.
    settings: { rate: setting.choice(["1/2", "1/4", "1/8", "1/16"], "1/4") },
    output: port.audio(),
    editor: {
      category: "Dynamics",
      description: "Makes the sound dip on every beat and swell back, in time with the song",
      insert: { depth: -8, shape: 0.5 },
      // One bar of its dip, from its own sum, with the song's playhead (CARD-116).
      views: { dip: dipPicture() },
      face: ["dip", "depth", "shape", "rate"],
    },
  },
  (x: Audio, { depth, shape, rate }: Params) => {
    // How far through the step the song is, 0 to 1, and what is left of it.
    const p = lfo(time.beats, { period: RATES[rate as keyof typeof RATES], shape: "saw" });
    const left = range(p, 1, 0);
    const squared = scale(left, left);
    // What is left in a straight line at shape 0, its fourth power at 1: the dip in dB, times depth.
    return duck(x, blend(left, scale(squared, squared), shape), { depth });
  },
  { dip: { draw: (v, p) => dip(p, v, { depth: "depth", shape: "shape", rate: "rate" }), words: (v) => dipWords(v, { depth: "depth", shape: "shape", rate: "rate" }) } },
);

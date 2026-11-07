// std/pictures: the pictures sounds share (CARD-90): their axes, and the shapes drawn from a
// sound's own maths where music2 can't measure it. TypeScript only: no kernel, nothing the song
// plays. Packs and songs import it as any std module: `import { envelope } from "std/pictures"`.
//
// A picture is two parts (CARD-60): its declaration in `editor.views`, made here by a `…Picture()`
// function (its axes and its live layers), and its code in `component()`'s third argument, whose
// `draw` calls the shape functions here. Every shape function takes the card's `p`, so TypeScript
// checks the names of the values each dot moves against the sound's own descriptor (CARD-62).
//
// | Picture | Declared by | Drawn by |
// |---|---|---|
// | a curve across frequency, measured from the sound (an EQ, a filter) | `curvePicture()` | `p.response()` and dots |
// | how a compressor turns loud sounds down: level in across, level out up | `transferPicture()` | `transfer(p, v, names)` |
// | how far a sound is turned down, as it plays, over the last 3 s | `reductionPicture()` | `reduction(p, …)` |
// | an envelope: how loud a note is over time | `envelopePicture()` | `envelope(p, v, names)` |
// | an LFO's shape over one bar, with the song's playhead | `lfoPicture()` | `lfo(p, v, names)` |

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Played, type Shape, type Unit } from "@m2/core";

/** The name of one of the sound's number controls: what a dot moves by the wheel, or as its knob (CARD-62). */
export type Num<D> = NonNullable<Moves<D>["wheel"]>;
/** What a dot may move along an axis in `U`: a control in that unit, or `knob(name)` (CARD-62). */
export type Across<D, U extends Unit> = NonNullable<Moves<D, U, U>["x"]>;
/** The name of one of the sound's controls in `U` (CARD-62): a time in seconds, a q, a level from 0 to 1. */
export type In<D, U extends Unit> = Extract<Across<D, U>, string>;
/** The name of one of the sound's values a picture only reads (CARD-63): a control's or a setting's. */
export type Read<D> = Extract<keyof Played<D>, string>;
/** What plays, by the name of each of the card's values (CARD-63), as a shape function reads it. */
export type Values = { readonly [k: string]: unknown };

// ---------------------------------------------------------------- words

/** A number in plain words: at most three figures, a minus sign as taste.md writes it. */
export function figures(v: number): string {
  const r = Math.abs(v) >= 100 ? Math.round(v) : Math.abs(v) >= 10 ? Math.round(v * 10) / 10 : Math.round(v * 100) / 100;
  return String(r).replace("-", "−");
}
/** A frequency in words: "800 Hz", "2.4 kHz". */
export const hzWords = (hz: number) => (hz >= 1000 ? `${figures(hz / 1000)} kHz` : `${figures(hz)} Hz`);
/** A level in words: "−3.5 dB". */
export const dbWords = (db: number) => `${figures(db)} dB`;
/** A time in words: "5 ms", "1.2 s". */
export const secondsWords = (s: number) => (s < 1 ? `${figures(s * 1000)} ms` : `${figures(s)} s`);

/**
 * The number `v` holds by `name` (CARD-63), shared by every pack's pictures: one a cable sets or the
 * code works out (null) greys the picture, saying which; a name the sound hasn't got says so.
 */
export function known(v: Values, name: string, what: string): number {
  const x = v[name];
  if (x === undefined) throw new Error(`names ${name}, which this sound doesn't have`);
  if (typeof x !== "number") throw new Error(`its ${what} is set by a cable or worked out by the code, so it can't be drawn`);
  return x;
}

// ---------------------------------------------------------------- curves across frequency

/**
 * A curve across frequency (CARD-91): 20 Hz to 20 kHz on a log scale, ±`range` dB (24), and, unless
 * `live` is false, the sound's spectrum under it as it plays, faint before the sound and full after.
 */
export function curvePicture(o: { range?: number; live?: boolean } = {}): PictureDecl<"hz", "db"> {
  const range = o.range ?? 24;
  return view.picture({
    x: axis.log("hz", 20, 20000),
    y: axis.lin("db", -range, range),
    ...(o.live === false ? {} : { live: [live.spectrum("in", { tone: "faint" }), live.spectrum("out")] }),
  });
}

/** The name a `moves` entry moves: its own, or a `knob(…)`'s. */
function nameOf(m: unknown): string {
  return typeof m === "string" ? m : (m as { name: string }).name;
}

// ---------------------------------------------------------------- dynamics

/** The compressor's soft knee, in dB around its threshold: std:compress's own (KNEE, fx.rs). */
export const KNEE = 6;

/**
 * How many dB a compressor turns a steady level `over` dB above its threshold down, at `ratio`, with
 * a soft knee `knee` dB wide: the kernel's own sum (std:compress), so the picture is the sound.
 */
export function turnedDown(over: number, ratio: number, knee = KNEE): number {
  const slope = 1 - 1 / Math.min(50, Math.max(1, ratio));
  if (over <= -knee / 2) return 0;
  if (over < knee / 2) return (slope * (over + knee / 2) ** 2) / (2 * knee);
  return slope * over;
}

/**
 * How a compressor treats a steady sound (CARD-93): the level going in across, from −60 to 0 dB, and
 * the level coming out up, from −60 to +12 dB. Its live dot rides the curve at the level going in,
 * as it plays, and `meter` (`reduction`), how far it turns down, is a bar at its right, from 0 dB up.
 */
export function transferPicture(o: { meter?: string } = {}): PictureDecl<"db", "db"> {
  return view.picture({ x: axis.lin("db", -60, 0), y: axis.lin("db", -60, 12), live: [live.level("in", { on: "transfer" }), live.meter(o.meter ?? "reduction")] });
}

/**
 * A compressor's transfer curve (CARD-93), from its own sum (`turnedDown`): the line `transfer`, a
 * faint line where in is out, a dot at the threshold's knee that moves the threshold across, and a
 * dot on the line above the knee, halfway to 0 dB in, that moves the ratio across, as its knob's drag
 * does (to the right, more), and by the wheel. The makeup is its knob's.
 */
export function transfer<D>(
  p: Painter<D, "db", "db">,
  v: { readonly [k: string]: unknown },
  names: { threshold: Across<D, "db">; ratio: In<D, "scalar">; makeup: string },
): Shape[] {
  const threshold = Math.min(12, Math.max(-80, known(v, nameOf(names.threshold), "threshold")));
  const ratio = known(v, names.ratio, "ratio");
  const makeup = Math.min(24, Math.max(-60, known(v, nameOf(names.makeup), "makeup")));
  const out = (level: number) => level - turnedDown(level - threshold, ratio) + makeup;
  const points: number[] = [];
  for (let i = 0; i <= 120; i++) points.push(-60 + i / 2, out(-60 + i / 2));
  return [
    p.line([-60, -60, 0, 0], { tone: "faint" }),
    p.line(points, { name: "transfer" }),
    p.handle(threshold, out(threshold), { x: names.threshold } as Moves<D, "db", "db">),
    p.handle(ratioAt(threshold), out(ratioAt(threshold)), { x: knob(names.ratio), wheel: names.ratio } as unknown as Moves<D, "db", "db">),
  ];
}

/** Where the ratio's dot sits on the line, in dB going in: halfway from the threshold's knee to 0 dB, past the knee. */
export const ratioAt = (threshold: number) => Math.min(0, Math.max(threshold + KNEE / 2, threshold / 2));

/** What `transfer` draws, in words. */
export function transferWords(v: { readonly [k: string]: unknown }, names: { threshold: string; ratio: string; makeup: string }): string {
  const [t, r, m] = [v[names.threshold], v[names.ratio], v[names.makeup]];
  if (typeof t !== "number" || typeof r !== "number") return "Turns loud moments down: its threshold or ratio is set by a cable or the code";
  const up = typeof m === "number" && m !== 0 ? `, then up ${dbWords(m)}` : "";
  return `Above ${dbWords(t)} the level rises only 1 dB for each ${figures(r)}${up}. Drag the corner for the threshold, the line for the ratio`;
}

/**
 * How far a sound is turned down, as it plays (CARD-94): a meter's bar at its right, and its last
 * 3 seconds as a line, from 0 dB at the foot to `most` dB (24) at the top. `meter` names the meter
 * the sound's kernel sends (std's compress, limit and duck send `reduction`, ABI-14).
 */
export function reductionPicture(o: { most?: number; meter?: string } = {}): PictureDecl<"seconds", "db"> {
  const meter = o.meter ?? "reduction";
  return view.picture({ x: axis.time(3), y: axis.lin("db", 0, o.most ?? 24), live: [live.history(live.meter(meter)), live.meter(meter)] });
}

/**
 * A reduction picture's shapes (CARD-94): a guide line at `most`, the most it may turn down, when
 * the sound has one (a duck's depth), with its words.
 */
export function reduction<D>(p: Painter<D, "seconds", "db">, o: { most?: number | null; top?: number } = {}): Shape[] {
  if (typeof o.most !== "number") return [];
  // Kept on the picture: past its top, the line rests there and its words say so (review S9).
  const top = o.top ?? 24;
  const past = o.most > top ? `, past this picture's top (${dbWords(top)})` : "";
  return [p.mark("y", Math.min(top, Math.max(0, o.most)), `The most it turns down: ${dbWords(o.most)}${past}`)];
}

/**
 * The last 3 seconds of a dynamics effect (CARD-93): the level going in (faint) and coming out, in dB
 * below 0, and how far it turned the sound down (its `meter`), above 0 dB, up to `most` (24). Every
 * layer is on the picture's own axis (CARD-103).
 */
export function historyPicture(o: { most?: number; meter?: string } = {}): PictureDecl<"seconds", "db"> {
  const meter = o.meter ?? "reduction";
  return view.picture({
    x: axis.time(3),
    y: axis.lin("db", -60, o.most ?? 24),
    live: [live.history(live.level("in"), { tone: "faint" }), live.history(live.level("out")), live.history(live.meter(meter)), live.meter(meter)],
  });
}

// ---------------------------------------------------------------- envelopes

/** How an envelope's attack rises and its decay and release fall: std:adsr's own sums. */
const ENVELOPE_POINTS = 40;
/** The time a decay or a release takes to fall 60 dB toward where it goes: std:adsr's (decay_coef). */
const FALL = 6.9;

/** The longest an envelope's picture reaches, in seconds: a whole envelope of 10 s times and its hold. */
const ENVELOPE_SECONDS = 40;

/**
 * An envelope (CARD-96): how loud a note is over time, in seconds across, from its start to the end
 * of its release, and its level from 0 to 1 up. Its x axis is one of time, so the window draws it
 * from 0 to the end of the release, whatever its length, and holds that while a corner is dragged
 * (CARD-102).
 */
export function envelopePicture(): PictureDecl<"seconds", "unit01"> {
  return view.picture({ x: axis.time(ENVELOPE_SECONDS), y: axis.lin("unit01", 0, 1) });
}

/** How long an envelope's picture holds its sustain: a quarter of the attack and decay, 50 ms at least (CARD-96). */
export const holdOf = (attack: number, decay: number) => Math.max(0.05, 0.25 * (attack + decay));

/**
 * An envelope's shape (CARD-96), in seconds, as std:adsr plays it: a straight rise over the attack,
 * then a fall toward the sustain that has come 60 dB of the way in the decay, held a while
 * (`holdOf`), then a fall to silence that has come 60 dB in the release. Its three dots, each moving
 * its time across in seconds, from where it was: the peak the attack, the decay's corner the decay
 * (and the sustain up and down), the end the release. Its times are controls in seconds, its
 * sustain a level from 0 to 1 (checked by type, CARD-62, and as it draws, CARD-65).
 */
export function envelope<D>(
  p: Painter<D, "seconds", "unit01">,
  v: { readonly [k: string]: unknown },
  names: { attack: In<D, "seconds">; decay: In<D, "seconds">; sustain: In<D, "unit01">; release: In<D, "seconds"> },
): Shape[] {
  const a = Math.max(0.0005, known(v, names.attack, "attack"));
  const d = Math.max(0.0005, known(v, names.decay, "decay"));
  const s = Math.min(1, Math.max(0, known(v, names.sustain, "sustain")));
  const r = Math.max(0.0005, known(v, names.release, "release"));
  const hold = holdOf(a, d);
  const level = (t: number) => (t <= a ? t / a : s + (1 - s) * Math.exp((-FALL * (t - a)) / d));
  const points = [0, 0];
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push((a * i) / ENVELOPE_POINTS, level((a * i) / ENVELOPE_POINTS));
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push(a + ((d + hold) * i) / ENVELOPE_POINTS, level(a + ((d + hold) * i) / ENVELOPE_POINTS));
  const from = level(a + d + hold);
  const end = a + d + hold + r;
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push(a + d + hold + (r * i) / ENVELOPE_POINTS, from * Math.exp((-FALL * i) / ENVELOPE_POINTS));
  points.push(end, 0);
  return [
    p.fill(points, { base: 0, tone: "faint" }),
    p.line(points, { name: "envelope" }),
    p.handle(a, 1, { x: names.attack } as Moves<D, "seconds", "unit01">),
    p.handle(a + d, level(a + d), { x: names.decay, y: names.sustain } as Moves<D, "seconds", "unit01">),
    p.handle(end, 0, { x: names.release } as Moves<D, "seconds", "unit01">),
  ];
}

/** What `envelope` draws, in words. */
export function envelopeWords(v: { readonly [k: string]: unknown }, names: { attack: string; decay: string; sustain: string; release: string }): string {
  const [a, d, s, r] = [v[names.attack], v[names.decay], v[names.sustain], v[names.release]];
  const t = (x: unknown) => (typeof x === "number" ? secondsWords(x) : "set elsewhere");
  return `How loud a note is over time: attack ${t(a)}, decay ${t(d)}, sustain ${typeof s === "number" ? figures(s) : "set elsewhere"}, release ${t(r)}`;
}

// ---------------------------------------------------------------- LFOs

/** The least an LFO's picture shows, in beats: whole cycles of it, at least a bar of 4/4. */
export const LFO_BEATS = 4;
/** The most: two cycles of its knob's longest period, 16 beats. */
const LFO_MOST = 32;

/**
 * An LFO's shape (CARD-98): whole cycles of it across, in beats from the song's first, 0 to 1 up, and
 * the song's playhead on it as it plays. Its x axis is one of beats, so the window draws it to the
 * end of its last cycle and its playhead goes round with the song's beat (CARD-102, CARD-104).
 */
export function lfoPicture(): PictureDecl<"beats", "unit01"> {
  return view.picture({ x: axis.lin("beats", 0, LFO_MOST), y: axis.lin("unit01", 0, 1), live: [live.playhead("beat")] });
}

/** Where an LFO of `shape` is at phase `p` (0 to 1), from 0 to 1: std:lfo's own sum. */
export function lfoAt(shape: string, p: number): number {
  if (shape === "tri") return 1 - Math.abs(2 * p - 1);
  if (shape === "saw") return p;
  if (shape === "square") return p < 0.5 ? 1 : 0;
  return 0.5 - 0.5 * Math.cos(p * 2 * Math.PI);
}

/** How many points an LFO's cycles are drawn with. */
const LFO_POINTS = 512;

/** How many beats an LFO of `period` is drawn over: the fewest whole cycles that reach a bar of 4/4. */
export const lfoSpan = (period: number) => Math.max(1, Math.ceil(LFO_BEATS / period - 1e-9)) * period;

/**
 * An LFO's shape (CARD-98), from the song's first beat: the kernel's own sum at each beat (its phase
 * the beat over the period, plus its phase), over whole cycles (`lfoSpan`), so the song's beat, gone
 * round the picture, is where the LFO is, whatever its period; and a dot at the end of its first
 * cycle that moves the period across.
 */
export function lfo<D>(
  p: Painter<D, "beats", "unit01">,
  v: { readonly [k: string]: unknown },
  names: { period: Across<D, "beats">; shape: string; phase: string },
): Shape[] {
  const period = Math.max(1e-4, known(v, nameOf(names.period), "period"));
  const shape = typeof v[names.shape] === "string" ? (v[names.shape] as string) : "sine";
  const phase = typeof v[names.phase] === "number" ? (v[names.phase] as number) : 0;
  const at = (beat: number) => lfoAt(shape, (((beat / period + phase) % 1) + 1) % 1);
  const span = lfoSpan(period);
  const points: number[] = [];
  for (let i = 0; i < LFO_POINTS; i++) {
    const beat = (span * i) / (LFO_POINTS - 1);
    points.push(beat, at(beat));
  }
  return [p.line(points, { name: "shape" }), p.handle(period, at(period - 1e-9), { x: names.period } as Moves<D, "beats", "unit01">)];
}

/** What `lfo` draws, in words. */
export function lfoWords(v: { readonly [k: string]: unknown }, names: { period: string; shape: string }): string {
  const period = v[names.period], shape = v[names.shape];
  const every = typeof period === "number" ? `every ${figures(period)} ${period === 1 ? "beat" : "beats"}` : "at a rate set elsewhere";
  return `A ${typeof shape === "string" ? shape : "sine"} wave from 0 to 1, once ${every}`;
}

// ---------------------------------------------------------------- a ceiling

/**
 * A limiter's last 3 seconds (CARD-143): the level going in (faint) and coming out, from −36 dB at
 * the foot to +6 dB at the top, on the picture's own axis in dB (CARD-103). How far it turned down
 * is the gap between the two where the sound going in rises past the ceiling: drawn above 0 dB on
 * the same axis, it put the ceiling's line on the 0 dB one (review U11).
 */
export function ceilingPicture(): PictureDecl<"seconds", "db"> {
  return view.picture({ x: axis.time(3), y: axis.lin("db", -36, 6), live: [live.history(live.level("in"), { tone: "faint" }), live.history(live.level("out"))] });
}

/**
 * A ceiling's shapes (CARD-143): a guide line at the right edge (now), the ceiling's line, and a
 * dot on it, in the middle, that moves the ceiling up and down, under the pointer. Its ceiling is a
 * control in dB (checked by type, CARD-62).
 */
export function ceiling<D>(p: Painter<D, "seconds", "db">, v: { readonly [k: string]: unknown }, names: { ceiling: In<D, "db"> }): Shape[] {
  const c = known(v, names.ceiling, "ceiling");
  return [
    p.mark("x", 3, "Now: the last 3 seconds run from the left"),
    p.mark("y", c, `The ceiling: ${dbWords(c)}`),
    p.handle(1.5, c, { y: names.ceiling } as Moves<D, "seconds", "db">),
  ];
}

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
// | a filter drawn from its maths, inside a voice where nothing can be measured | `curvePicture({ live: false })` | `lowpass(p, v, names)` |
// | how a compressor turns loud sounds down: level in across, level out up | `transferPicture()` | `transfer(p, v, names)` |
// | how far a sound is turned down, as it plays, over the last 3 s | `reductionPicture()` | `reduction(p, …)` |
// | an envelope: how loud a note is over time | `envelopePicture()` | `envelope(p, v, names)` |
// | an LFO's shape over one bar, with the song's playhead | `lfoPicture()` | `lfo(p, v, names)` |

import { axis, knob, live, view, type Moves, type Painter, type PictureDecl, type Shape, type Unit } from "@m2/core";

/** The name of one of the sound's number controls: what a dot moves by the wheel, or as its knob (CARD-62). */
type Num<D> = NonNullable<Moves<D>["wheel"]>;
/** What a dot may move along an axis in `U`: a control in that unit, or `knob(name)` (CARD-62). */
type Across<D, U extends Unit> = NonNullable<Moves<D, U, U>["x"]>;

// ---------------------------------------------------------------- words

/** A number in plain words: at most three figures, a minus sign as taste.md writes it. */
function figures(v: number): string {
  const r = Math.abs(v) >= 100 ? Math.round(v) : Math.abs(v) >= 10 ? Math.round(v * 10) / 10 : Math.round(v * 100) / 100;
  return String(r).replace("-", "−");
}
/** A frequency in words: "800 Hz", "2.4 kHz". */
export const hzWords = (hz: number) => (hz >= 1000 ? `${figures(hz / 1000)} kHz` : `${figures(hz)} Hz`);
/** A level in words: "−3.5 dB". */
export const dbWords = (db: number) => `${figures(db)} dB`;
/** A time in words: "5 ms", "1.2 s". */
export const secondsWords = (s: number) => (s < 1 ? `${figures(s * 1000)} ms` : `${figures(s)} s`);

/** `v`, which a cable or the code may hold (CARD-63): else the picture can't be drawn, said in words. */
function known(v: number | boolean | string | null | undefined, what: string): number {
  if (typeof v !== "number") throw new Error(`its ${what} is set by a cable or worked out by the code, so it can't be drawn`);
  return v;
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

/** How many points a curve drawn from maths has: one each sixtieth of the 10 octaves. */
const CURVE_POINTS = 241;

/** A two-pole lowpass's gain at `f`, in dB: 1 / √((1 − r²)² + (r / q)²), r = f / cutoff (an analog prototype). */
export function lowpassDb(f: number, cutoff: number, q: number): number {
  const r = f / cutoff;
  return -10 * Math.log10((1 - r * r) ** 2 + (r / q) ** 2);
}

/** A resonance from 0 to 1 as the q it is drawn with (CARD-30): 0.5, no bump, to 30, where a synth's filter sings. */
export const resonanceQ = (r: number) => 0.5 * 60 ** Math.min(1, Math.max(0, r));

/**
 * A lowpass drawn from its maths (CARD-97): a two-pole (12 dB per octave) curve at `cutoff` with a
 * bump the `q` makes taller, or, with `resonance`, a level from 0 to 1 drawn as `resonanceQ`. For a
 * filter inside an instrument's voices, where music2 can't play a click through it (CARD-81); a
 * filter a sound plays on its own is measured (`p.response()`). Its one dot moves the cutoff across
 * and the q (or the resonance) up and down, and by the wheel.
 */
export function lowpass<D>(
  p: Painter<D, "hz", "db">,
  v: { readonly [k: string]: unknown },
  names: { cutoff: Across<D, "hz">; q: Num<D> } | { cutoff: Across<D, "hz">; resonance: Num<D> },
): Shape[] {
  const cutoff = known(v[nameOf(names.cutoff)] as number | null, "cutoff");
  const resonant = "resonance" in names;
  const qName = resonant ? names.resonance : names.q;
  const raw = known(v[qName] as number | null, resonant ? "resonance" : "q");
  const q = resonant ? resonanceQ(raw) : raw;
  const points: number[] = [];
  for (let i = 0; i < CURVE_POINTS; i++) {
    const f = 20 * 1000 ** (i / (CURVE_POINTS - 1));
    points.push(f, Math.max(-120, lowpassDb(f, cutoff, q)));
  }
  return [
    p.fill(points, { base: -120, tone: "faint" }),
    p.line(points, { name: "curve" }),
    p.handle(cutoff, 20 * Math.log10(q), { x: names.cutoff, y: knob(qName), wheel: qName } as Moves<D, "hz", "db">),
  ];
}

/** What `lowpass` draws, in words. */
export function lowpassWords(v: { readonly [k: string]: unknown }, names: { cutoff: string; q?: string; resonance?: string }): string {
  const c = v[names.cutoff], q = v[names.resonance ?? names.q ?? ""];
  if (typeof c !== "number") return "A filter that cuts the highs: its cutoff is set by a cable or the code";
  const bump = typeof q === "number" ? `, ${names.resonance ? "resonance" : "q"} ${figures(q)}` : "";
  return `Cuts the highs above ${hzWords(c)}${bump}: a drawing of a 12 dB per octave filter, not measured from the sound`;
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
 * as it plays.
 */
export function transferPicture(): PictureDecl<"db", "db"> {
  return view.picture({ x: axis.lin("db", -60, 0), y: axis.lin("db", -60, 12), live: [live.level("in", { on: "transfer" })] });
}

/**
 * A compressor's transfer curve (CARD-93), from its own sum (`turnedDown`): the line `transfer`, a
 * faint line where in is out, a dot at the threshold's knee that moves the threshold across and the
 * ratio by the wheel, and a dot at the curve's end that moves the makeup up and down and the ratio
 * by the wheel.
 */
export function transfer<D>(
  p: Painter<D, "db", "db">,
  v: { readonly [k: string]: unknown },
  names: { threshold: Across<D, "db">; ratio: Num<D>; makeup: Across<D, "db"> },
): Shape[] {
  const threshold = Math.min(12, Math.max(-80, known(v[nameOf(names.threshold)] as number | null, "threshold")));
  const ratio = known(v[names.ratio] as number | null, "ratio");
  const makeup = Math.min(24, Math.max(-60, known(v[nameOf(names.makeup)] as number | null, "makeup")));
  const out = (level: number) => level - turnedDown(level - threshold, ratio) + makeup;
  const points: number[] = [];
  for (let i = 0; i <= 120; i++) points.push(-60 + i / 2, out(-60 + i / 2));
  return [
    p.line([-60, -60, 0, 0], { tone: "faint" }),
    p.line(points, { name: "transfer" }),
    p.handle(threshold, out(threshold), { x: names.threshold, wheel: names.ratio } as Moves<D, "db", "db">),
    p.handle(0, out(0), { y: names.makeup, wheel: names.ratio } as Moves<D, "db", "db">),
  ];
}

/** What `transfer` draws, in words. */
export function transferWords(v: { readonly [k: string]: unknown }, names: { threshold: string; ratio: string; makeup: string }): string {
  const [t, r, m] = [v[names.threshold], v[names.ratio], v[names.makeup]];
  if (typeof t !== "number" || typeof r !== "number") return "Turns loud moments down: its threshold or ratio is set by a cable or the code";
  const up = typeof m === "number" && m !== 0 ? `, then up ${dbWords(m)}` : "";
  return `Above ${dbWords(t)} the level rises only 1 dB for each ${figures(r)}${up}`;
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
export function reduction<D>(p: Painter<D, "seconds", "db">, o: { most?: number | null } = {}): Shape[] {
  return typeof o.most === "number" ? [p.mark("y", Math.max(0, o.most), `The most it turns down: ${dbWords(o.most)}`)] : [];
}

// ---------------------------------------------------------------- envelopes

/** How an envelope's attack rises and its decay and release fall: std:adsr's own sums. */
const ENVELOPE_POINTS = 40;
/** The time a decay or a release takes to fall 60 dB toward where it goes: std:adsr's (decay_coef). */
const FALL = 6.9;

/**
 * An envelope (CARD-96): how loud a note is over time, from its start to the end of its release,
 * across the whole picture whatever its length, and its level from 0 to 1 up.
 */
export function envelopePicture(): PictureDecl<"unit01", "unit01"> {
  return view.picture({ x: axis.lin("unit01", 0, 1), y: axis.lin("unit01", 0, 1) });
}

/** How long an envelope's picture holds its sustain: a quarter of the rest, 50 ms at least (CARD-96). */
export const holdOf = (attack: number, decay: number, release: number) => Math.max(0.05, 0.25 * (attack + decay + release));

/**
 * An envelope's shape (CARD-96), as std:adsr plays it: a straight rise over the attack, then a fall
 * toward the sustain that has come 60 dB of the way in the decay, held a while (`holdOf`), then a
 * fall to silence that has come 60 dB in the release. Its three dots: the peak moves the attack,
 * the decay's corner the decay (across) and the sustain (up and down), and the end the release,
 * each time as its knob's drag does.
 */
export function envelope<D>(
  p: Painter<D, "unit01", "unit01">,
  v: { readonly [k: string]: unknown },
  names: { attack: Num<D>; decay: Num<D>; sustain: Across<D, "unit01">; release: Num<D> },
): Shape[] {
  const a = Math.max(0.0005, known(v[names.attack] as number | null, "attack"));
  const d = Math.max(0.0005, known(v[names.decay] as number | null, "decay"));
  const s = Math.min(1, Math.max(0, known(v[nameOf(names.sustain)] as number | null, "sustain")));
  const r = Math.max(0.0005, known(v[names.release] as number | null, "release"));
  const hold = holdOf(a, d, r);
  const total = a + d + hold + r;
  const x = (t: number) => t / total;
  const level = (t: number) => (t <= a ? t / a : s + (1 - s) * Math.exp((-FALL * (t - a)) / d));
  const points = [0, 0];
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push(x((a * i) / ENVELOPE_POINTS), level((a * i) / ENVELOPE_POINTS));
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push(x(a + ((d + hold) * i) / ENVELOPE_POINTS), level(a + ((d + hold) * i) / ENVELOPE_POINTS));
  const from = level(a + d + hold);
  for (let i = 1; i <= ENVELOPE_POINTS; i++) points.push(x(a + d + hold + (r * i) / ENVELOPE_POINTS), from * Math.exp((-FALL * i) / ENVELOPE_POINTS));
  points.push(1, 0);
  return [
    p.fill(points, { base: 0, tone: "faint" }),
    p.line(points, { name: "envelope" }),
    p.handle(x(a), 1, { x: knob(names.attack) } as Moves<D, "unit01", "unit01">),
    p.handle(x(a + d), level(a + d), { x: knob(names.decay), y: names.sustain } as Moves<D, "unit01", "unit01">),
    p.handle(1, 0, { x: knob(names.release) } as Moves<D, "unit01", "unit01">),
  ];
}

/** What `envelope` draws, in words. */
export function envelopeWords(v: { readonly [k: string]: unknown }, names: { attack: string; decay: string; sustain: string; release: string }): string {
  const [a, d, s, r] = [v[names.attack], v[names.decay], v[names.sustain], v[names.release]];
  const t = (x: unknown) => (typeof x === "number" ? secondsWords(x) : "set elsewhere");
  return `How loud a note is over time: attack ${t(a)}, decay ${t(d)}, sustain ${typeof s === "number" ? figures(s) : "set elsewhere"}, release ${t(r)}`;
}

// ---------------------------------------------------------------- LFOs

/** The beats an LFO's picture shows: one bar of 4/4. */
export const LFO_BEATS = 4;

/** An LFO's shape (CARD-98): one bar of 4 beats across, 0 to 1 up, and the song's playhead on it as it plays. */
export function lfoPicture(): PictureDecl<"beats", "unit01"> {
  return view.picture({ x: axis.lin("beats", 0, LFO_BEATS), y: axis.lin("unit01", 0, 1), live: [live.playhead("beat")] });
}

/** Where an LFO of `shape` is at phase `p` (0 to 1), from 0 to 1: std:lfo's own sum. */
export function lfoAt(shape: string, p: number): number {
  if (shape === "tri") return 1 - Math.abs(2 * p - 1);
  if (shape === "saw") return p;
  if (shape === "square") return p < 0.5 ? 1 : 0;
  return 0.5 - 0.5 * Math.cos(p * 2 * Math.PI);
}

/** How many points an LFO's bar is drawn with. */
const LFO_POINTS = 512;

/**
 * An LFO's shape over one bar (CARD-98), from the song's first beat: the kernel's own sum at each
 * beat (its phase the beat over the period, plus its phase), and a dot at the end of its first cycle
 * that moves the period across.
 */
export function lfo<D>(
  p: Painter<D, "beats", "unit01">,
  v: { readonly [k: string]: unknown },
  names: { period: Across<D, "beats">; shape: string; phase: string },
): Shape[] {
  const period = Math.max(1e-4, known(v[nameOf(names.period)] as number | null, "period"));
  const shape = typeof v[names.shape] === "string" ? (v[names.shape] as string) : "sine";
  const phase = typeof v[names.phase] === "number" ? (v[names.phase] as number) : 0;
  const at = (beat: number) => lfoAt(shape, (((beat / period + phase) % 1) + 1) % 1);
  const points: number[] = [];
  for (let i = 0; i < LFO_POINTS; i++) {
    const beat = (LFO_BEATS * i) / (LFO_POINTS - 1);
    points.push(beat, at(beat));
  }
  const end = Math.min(LFO_BEATS, period);
  return [p.line(points, { name: "shape" }), p.handle(end, at(end - 1e-9), { x: names.period } as Moves<D, "beats", "unit01">)];
}

/** What `lfo` draws, in words. */
export function lfoWords(v: { readonly [k: string]: unknown }, names: { period: string; shape: string }): string {
  const period = v[names.period], shape = v[names.shape];
  const every = typeof period === "number" ? `every ${figures(period)} ${period === 1 ? "beat" : "beats"}` : "at a rate set elsewhere";
  return `A ${typeof shape === "string" ? shape : "sine"} wave from 0 to 1, once ${every}`;
}

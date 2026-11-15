// A lane's clips in the zoom picture (taste.md VIEW-13), as the page draws them at the zoom shown:
// their boxes (placed by `spanBox`, as the camera places each), and inside each what the page puts
// there with the same functions (`paintNotes`, `paintCurve`, `paintWave`), in the page's order: the
// boxes and what each draws itself, then the lane's canvas (its still clips' notes and curves), then
// what goes over that (a curve's points, the hatching), then a fade's handles and the take while it
// records.

import { spanBox } from "../../camera";
import { audibleOf } from "../../lib/arrangement";
import { beatNow } from "../../lib/clock";
import { clipRef, hasOwnId, sameClip, type AudioClip, type ControlClip, type NoteClip, type StepClip, type TimelineLane } from "../../lib/graph";
import { isPicked } from "../../lib/clips";
import { laneScale, type LaneScale } from "../../lib/laneScale";
import { MISSING, beatAt, peaksNow, secondsAt } from "../../lib/peaks";
import { pickedIn } from "../../lib/points";
import { useRecord } from "../../lib/record";
import { pickedStep, stepLabel } from "../../lib/steps";
import { compsOf, takeClip } from "../../lib/takes";
import { AREA_TOP, areaHeight, BODY_BOTTOM, BODY_TOP, BORDER, CLIP_END, CLIP_INSET, CLIP_MIN, fineAt, paintCurve, paintNotes, shortestOf, type Pen } from "../ClipDrawing";
import { audioLabel, INSET, paintFades, paintWave, shapeOf, STRIP, TILE } from "../Wave";
import { mixOklab, rrect, type Brush } from "./brush";
import type { Row, Scene } from "./scene";

/** A clip as the picture drew it, in px from the lanes' left: for the suites (VIEW-13). */
export interface DrawnClip {
  lane: string;
  id: string;
  start: number;
  length: number;
  x: number;
  w: number;
}

/** A box's corners, as `rounded-sm`, and a fade's handle (Wave's). */
const ROUND = 4;
const HANDLE = 7;
const HANDLES_FROM = 24;
/** The audio clip whose file is missing: Lucide's AudioWaveform, on its 24 px square. */
const WAVE_ICON = "M2 13a2 2 0 0 0 2-2V7a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0V4a2 2 0 0 1 4 0v13a2 2 0 0 0 4 0v-4a2 2 0 0 1 2-2";
let icon: Path2D | null = null;

/** Each lane's scale, kept by its lane (a curve's values up the clip, D3). */
const scales = new WeakMap<TimelineLane, LaneScale | null>();
const scaleOf = (lane: TimelineLane) => {
  if (!scales.has(lane)) scales.set(lane, lane.kind === "control" ? laneScale(lane) : null);
  return scales.get(lane)!;
};

/** What goes on later than a clip's own drawing, in the page's stacking order: the lane's canvas, then z 2, then z 10. */
interface Later {
  canvas: (() => void)[];
  over: (() => void)[];
  top: (() => void)[];
}

/** The rounded inside of a box (its border's inside): what a clip's own drawing is cut to. */
function inside(p: Brush, x: number, y: number, w: number, h: number) {
  p.g.beginPath();
  rrect(p.g, x + BORDER, y + BORDER, w - 2 * BORDER, h - 2 * BORDER, ROUND - BORDER);
  p.g.clip();
}

/**
 * A clip's box: its body (listed as a `clip`, `of` which), its 1 px border, and a picked clip's
 * outline or a changed lane's glow (AGENT-5).
 */
function box(p: Brush, x: number, y: number, w: number, h: number, body: string, border: string, outline: { color: string; off: number } | null, of: Record<string, unknown>) {
  if (outline) {
    const o = outline.off;
    p.round(x - o - 2, y - o - 2, w + 2 * o + 4, h + 2 * o + 4, ROUND + o + 2, outline.color, [x - o, y - o, w + 2 * o, h + 2 * o, ROUND + o]);
  }
  p.as("clip", () => p.round(x, y, w, h, ROUND, body), of);
  p.round(x, y, w, h, ROUND, border, [x + BORDER, y + BORDER, w - 2 * BORDER, h - 2 * BORDER, ROUND - BORDER]);
}

/** 45° hatching over a box's inside, as the page's `Hatch` pattern: 1.5 px lines 8 px apart, from its corner. */
function hatch(p: Brush, sc: Scene, x: number, y: number, w: number, h: number) {
  const g = p.g;
  g.save();
  inside(p, x, y, w, h);
  g.translate(x + BORDER, y + BORDER);
  g.rotate(Math.PI / 4);
  g.fillStyle = p.c(sc.colors.line);
  // The box in the turned space: its corners' reach along the lines' normal.
  const far = w + h;
  for (let k = Math.floor(-far / 8) - 1; k * 8 < far; k++) g.fillRect(k * 8, -far, 1.5, 3 * far);
  g.restore();
}

/** A drawing pen (ClipDrawing's) that draws on the picture: a clip's notes and curve in its area from `top` down, in `ink`. */
function drawing(p: Brush, ink: string, top: number): Pen {
  return {
    clip() {},
    note(x, y, w, h, alpha, mark) {
      p.fill(x, top + y, w, h, ink, alpha);
      if (mark) p.fill(x + w / 2 - 1, top + y, 2, h, ink, alpha);
    },
    curve(xy, foot) {
      if (!xy.length) return;
      const g = p.g;
      const path = () => {
        g.beginPath();
        xy.forEach(([x, y], i) => (i ? g.lineTo(x, top + y) : g.moveTo(x, top + y)));
      };
      path();
      g.lineTo(xy[xy.length - 1][0], top + foot);
      g.lineTo(xy[0][0], top + foot);
      g.closePath();
      g.fillStyle = g.strokeStyle = p.c(ink);
      g.globalAlpha = 0.1;
      g.fill();
      path();
      g.globalAlpha = 1;
      g.lineWidth = 1.5;
      g.lineJoin = "round";
      g.stroke();
    },
  };
}

/** Cut what comes next to a clip's drawing area, `x` to `x + w` across and `y` to `y + h` down. */
function area(p: Brush, x: number, y: number, w: number, h: number) {
  p.g.beginPath();
  p.g.rect(x, y, w, h);
  p.g.clip();
}

/** A round point of a curve, at its box's place on the screen's pixels (`size-1.5`), ringed white when picked. */
function point(p: Brush, sc: Scene, cx: number, cy: number, ink: string, picked: boolean) {
  const x = p.snap(cx - 3) + 3, y = p.snap(cy - 3) + 3;
  const g = p.g;
  if (picked) {
    g.fillStyle = p.c("#ffffff");
    g.beginPath();
    g.arc(x, y, 5, 0, Math.PI * 2);
    g.fill();
  }
  g.fillStyle = p.c(picked ? sc.colors.fgStrong : ink);
  g.beginPath();
  g.arc(x, y, 3, 0, Math.PI * 2);
  g.fill();
}

/** A lane of names' clip inside its area (CARD-23): each name held until the next, each step marked at its start. */
function steps(p: Brush, sc: Scene, c: StepClip, lane: TimelineLane & { kind: "steps" }, ax: number, at: number, aw: number, ah: number, ink: string, picked: number) {
  const { points, length } = c;
  const font = sc.type.font(400, 11);
  const base = at + (ah - 14) / 2 + sc.type.baseline(400, 11, 14);
  points.forEach(([b, v], i) => {
    const from = i === 0 ? 0 : Math.min(length, Math.max(0, b));
    const to = i + 1 < points.length ? Math.min(length, Math.max(0, points[i + 1][0])) : length;
    if (to <= from && i + 1 < points.length) return;
    const x0 = ax + (from / length) * aw, w = (Math.max(0, to - from) / length) * aw;
    const l = p.snap(x0), r = p.snap(x0 + w);
    if (r <= 0 || l >= sc.w) return;
    p.g.save();
    area(p, l, at, r - l, ah);
    const known = lane.options.some((o) => o.value === v);
    const label = stepLabel(lane.options, v);
    p.text(label, x0 + 6, base, font, ink, w - 6);
    // A name the choice no longer has: struck through, as the page strikes it (its label's `after:`
    // line, 1 px, 7 px down its line, across its words or as far as they show).
    if (!known) {
      p.g.font = font;
      const y = at + (ah - 14) / 2 + 7;
      p.box(x0 + 6, y, x0 + 6 + Math.max(0, Math.min(p.g.measureText(label).width, w - 6)), y + 1, ink);
    }
    const tick = x0 + (Math.max(0, b - from) / Math.max(to - from, 1e-9)) * w;
    p.box(tick, at, tick + (i === picked ? 2 : 1), at + ah, i === picked ? sc.colors.fgStrong : ink);
    p.g.restore();
  });
}

/**
 * An audio clip's inside (Wave's): its name (unless `bare`), its waveform's tiles as the page lays
 * them (`paintWave`, each tile's bitmap on the screen's pixels), its fades; a missing file's icon
 * and name. `px`/`pt`: its inside's corner; `pw`/`ph`: its size. `z` is the zoom shown.
 */
function wave(p: Brush, sc: Scene, c: AudioClip, px: number, pt: number, pw: number, ph: number, ink: string, z: number, bare = false) {
  const peaks = peaksNow(c);
  const type = sc.type;
  if (peaks === MISSING) {
    const t = pt + 4, h = ph - 4;
    icon ??= new Path2D(WAVE_ICON);
    const g = p.g;
    g.save();
    g.translate(px + 8, t + (h - 14) / 2);
    g.scale(14 / 24, 14 / 24);
    g.strokeStyle = p.c(ink);
    g.lineWidth = 2;
    g.lineCap = g.lineJoin = "round";
    g.stroke(icon);
    g.restore();
    p.text(audioLabel(c), px + 8 + 14 + 6, t + (h - 14) / 2 + type.baseline(500, 11, 14), type.font(500, 11), ink, pw - 8 - 14 - 6 - 8);
    return;
  }
  if (!bare) p.text(audioLabel(c), px + 6, pt + type.baseline(400, 11, 14), type.font(400, 11), ink, pw - 12);
  if (!peaks) return;
  const top = pt + (bare ? 2 : STRIP), hh = Math.round(ph - (bare ? 2 : STRIP) - 2);
  const width = Math.max(0, c.length * z - 2 * INSET);
  if (hh <= 0 || width <= 0) return;
  const shape = shapeOf(c, sc.a.timeline.tempo, z);
  const dpr = sc.dpr, g = p.g;
  for (let x0 = 0; x0 < width; x0 += TILE) {
    const tw = Math.min(TILE, width - x0);
    const left = px + x0;
    if (left > sc.w || left + tw < 0) continue;
    // The tile's bitmap, `bw` × `bh`, drawn as the page draws it, then laid over `tw` × `hh` px as the
    // browser lays the page's (scaled a hair, smoothly): its columns of what shows only.
    const bw = Math.max(1, Math.round(tw * dpr)), bh = Math.max(1, Math.round(hh * dpr));
    const sx = (tw * dpr) / bw;
    const from = Math.max(0, Math.floor((-left * dpr) / sx) - 1), to = Math.min(bw, Math.ceil(((sc.w - left) * dpr) / sx) + 1);
    if (to <= from) continue;
    const t = tileOf(to - from, bh);
    if (!t) continue;
    t.setTransform(1, 0, 0, 1, 0, 0);
    t.clearRect(0, 0, to - from, bh);
    t.setTransform(1, 0, 0, 1, -from, 0);
    paintWave(t, peaks, shape, x0, bw, bh, p.c(ink), dpr, from, to);
    paintFades(t, shape, c.fade_in ?? 0, c.fade_out ?? 0, x0, bw, bh, p.c(sc.colors.fgMuted), dpr);
    // The page's tile is drawn into its box on the screen's pixels: each edge of its layout's box
    // rounded to them, its bitmap scaled smoothly to that.
    const x0d = Math.round(left * dpr), x1d = Math.round((left + tw) * dpr), y0d = Math.round((top - p.top) * dpr), y1d = Math.round((top + hh - p.top) * dpr);
    const kx = (x1d - x0d) / bw, ky = (y1d - y0d) / bh;
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.drawImage(t.canvas, 0, 0, to - from, bh, x0d + from * kx, y0d, (to - from) * kx, bh * ky);
    g.restore();
  }
}

/** A canvas to draw a waveform's tile on, at least `w` × `h` device px, kept for the next. */
let tile: CanvasRenderingContext2D | null = null;
function tileOf(w: number, h: number): CanvasRenderingContext2D | null {
  tile ??= document.createElement("canvas").getContext("2d");
  if (!tile) return null;
  const c = tile.canvas;
  if (c.width < w || c.height < h) {
    c.width = Math.max(c.width, w);
    c.height = Math.max(c.height, h);
  }
  return tile;
}

/** An audio clip's fade handles (Q10), shown while it is picked: 7 px squares at its top. */
function handles(p: Brush, sc: Scene, c: AudioClip, px: number, pt: number, z: number, ink: string) {
  const tempo = sc.a.timeline.tempo;
  const width = Math.max(0, c.length * z - 2 * INSET);
  if (width + 2 * INSET < HANDLES_FROM) return;
  const shape = shapeOf(c, tempo, z);
  const xOf = (s: number) => Math.max(0, (beatAt(tempo, secondsAt(tempo, c.start) + s) - c.start) * z - INSET);
  for (const x of [Math.min(xOf(c.fade_in ?? 0), width - HANDLE), Math.max(0, xOf(shape.seconds - (c.fade_out ?? 0)) - HANDLE)]) {
    const l = p.snap(px + x);
    const g = p.g;
    g.fillStyle = p.c(sc.colors.raised);
    g.beginPath();
    rrect(g, l, pt, HANDLE, HANDLE, 2);
    g.fill();
    g.fillStyle = p.c(ink);
    g.beginPath();
    rrect(g, l, pt, HANDLE, HANDLE, 2);
    rrect(g, l + 1, pt + 1, HANDLE - 2, HANDLE - 2, 1);
    g.fill("evenodd");
  }
}

/** The take recording on this lane, if one is (lane B, Q24), at the zoom shown and the playhead now. */
function recording(p: Brush, sc: Scene, row: Row, z: number, X: number, top: number, bottom: number) {
  const take = useRecord.getState().take;
  const lane = row.lane!;
  if (!take || take.phase !== "take" || !(take.lanes ?? [take]).some((l) => l.module === row.module && l.lane === lane.key)) return;
  const mine = take.lanes?.find((l) => l.module === row.module && l.lane === lane.key);
  const now = beatNow();
  const end = Math.max(take.from, take.until === null ? now : Math.min(now, take.until));
  const audio = (mine?.kind ?? take.kind) === "audio";
  const start = mine?.start ?? take.start;
  const length = Math.max(end - start, 1 / 64);
  const x0 = start * z + 1 - X, w = Math.max(length * z - 2, 4);
  const l = p.snap(x0), r = p.snap(x0 + w), t = top + CLIP_INSET, b = bottom - CLIP_INSET;
  if (r < 0 || l > sc.w) return;
  const g = p.g;
  g.save();
  g.beginPath();
  rrect(g, l, t, r - l, b - t, ROUND);
  g.clip();
  p.fill(l, t, r - l, b - t, sc.colors.dangerSoft);
  // Its outline, 1.5 px inside its edge.
  g.fillStyle = p.c(sc.colors.danger);
  g.beginPath();
  rrect(g, l, t, r - l, b - t, ROUND);
  rrect(g, l + 1.5, t + 1.5, r - l - 3, b - t - 3, ROUND - 1.5);
  g.fill("evenodd");
  // Its words where the page's box puts them: from its left as laid out, not as drawn on the pixels.
  p.text("Recording", x0 + 6, t + 2 + sc.type.baseline(500, 11, 14), sc.type.font(500, 11), sc.colors.dangerStrong);
  if (audio) {
    const meter = document.querySelector<HTMLCanvasElement>(`[data-testid=take-level]`);
    if (meter) g.drawImage(meter, r - 4 - 3, t + 4, 3, b - t - 8);
  } else {
    const notes = take.notes
      .filter((n) => !n.module || (n.module === row.module && n.lane === lane.key))
      .map((n) => ({ pitch: n.pitch, start: n.start - start, length: n.held ? Math.max(end - n.start, 1 / 64) : n.length, velocity: n.velocity }));
    const at = t + 12 + BODY_TOP, ah = b - t - 12 - BODY_TOP - BODY_BOTTOM;
    area(p, l, at, r - l, ah);
    paintNotes(drawing(p, row.said.color.ink, at), notes, l, (r - l) / length, ah, -Infinity, Infinity, true);
  }
  g.restore();
}

/**
 * A lane's clips over its row's inside, `top` to `bottom` down (`said` its track), at zoom `z` seen
 * from `X`: what the clips' group draws, faded as a group where its track is (opacity, grayscale).
 */
export function laneClips(p: Brush, sc: Scene, row: Row, z: number, X: number, top: number, bottom: number) {
  const lane = row.lane!;
  const said = row.said;
  const { ink, bar, tint } = said.color;
  const faded = said.waiting ? { alpha: 0.5, gray: 1 } : said.heard !== "heard" ? { alpha: 0.4, gray: 0.6 } : null;
  if (faded) p.begin(faded.alpha, faded.gray, top, bottom);
  const g = p.g;
  g.save();
  area(p, 0, top, sc.w, bottom - top);
  const inner = bottom - top;
  const ah = areaHeight(inner);
  const at = top + AREA_TOP;
  const scale = scaleOf(lane);
  const clips = lane.clips as (NoteClip | ControlClip | StepClip | AudioClip)[];
  // What lies under the clips: a lane of names' fallback name at the start of each gap, an automation
  // lane's dashed line where no clip is (its value outside them).
  const gaps = () => {
    const out: [number, number][] = [];
    let end = 0;
    for (const c of [...clips].sort((a, b) => a.start - b.start)) {
      if (c.start > end) out.push([end, c.start]);
      end = Math.max(end, c.start + c.length);
    }
    out.push([end, Infinity]);
    return out;
  };
  if (lane.kind === "steps") {
    const name = stepLabel(lane.options, lane.default);
    const base = top + inner / 2 - 7 + sc.type.baseline(400, 11, 14);
    for (const [from] of gaps()) p.text(name, spanBox(from, null, z).left - X + 6, base, sc.type.font(400, 11), sc.colors.fgFaint);
  }
  if (lane.kind === "control" && lane.default != null) {
    const y = CLIP_INSET + BORDER + BODY_TOP + (1 - (scale?.level(lane.default) ?? 0)) * (row.h - 2 * (CLIP_INSET + BORDER) - BODY_TOP - BODY_BOTTOM);
    const ly = top + Math.round(y) + 0.5;
    g.save();
    g.strokeStyle = p.c(sc.colors.lineStrong);
    g.lineWidth = 1;
    g.setLineDash([4, 4]);
    for (const [from, to] of gaps()) {
      const x0 = spanBox(from, null, z).left - X;
      const x1 = to === Infinity ? sc.w + 8 : x0 + (spanBox(from, to, z).width ?? 0);
      if (x1 < 0 || x0 > sc.w) continue;
      g.beginPath();
      g.moveTo(x0, ly);
      g.lineTo(x1, ly);
      g.stroke();
    }
    g.restore();
  }
  // A clip a later one lies over draws on a canvas of its own, under that one (VIEW-12).
  const covered = clips.map((c, i) => clips.some((o, j) => j > i && o.start < c.start + c.length && o.start + o.length > c.start));
  const byLane = lane.kind === "notes" || lane.kind === "control";
  const later: Later = { canvas: [], over: [], top: [] };
  const from = (X - 8) / z, to = (X + sc.w + 8) / z;
  const glow = sc.glows.has(`${row.module}/${lane.key}`);
  const bt = top + CLIP_INSET, bh = inner - 2 * CLIP_INSET;
  clips.forEach((c, i) => {
    if (c.start + c.length < from || c.start > to) return;
    const sb = spanBox(c.start, c.start + c.length, z, CLIP_END, CLIP_MIN);
    const x = sb.left - X, w = sb.width ?? 0;
    const ref = clipRef(row.module, lane.key, lane.clips, i);
    const picked = isPicked(sc.picked, ref);
    // A changed lane's glow wins over the picked outline, as the page's cascade has it (index.css's
    // `[data-clip][data-glow]` lies outside Tailwind's layers).
    const outline = glow ? { color: sc.colors.accent, off: 1 } : picked ? { color: sc.colors.fgStrong, off: 0 } : null;
    const of = { lane: `${row.module}.${lane.key}`, id: hasOwnId(lane.clips, i) ? lane.clips[i].id! : `#${i}`, start: c.start, length: c.length };
    box(p, x, bt, w, bh, picked ? mixOklab(bar, tint, 0.35) : tint, bar, outline, of);
    const ax = x + BORDER, aw = w - 2 * BORDER;
    const k = aw / c.length;
    const lo = from - c.start, hi = to - c.start;
    const own = covered[i] || !byLane;
    // Its own drawing, cut to its inside: where the lane's canvas doesn't draw it, and what goes over that.
    const mineNow = (f: () => void) => {
      g.save();
      inside(p, x, bt, w, bh);
      f();
      g.restore();
    };
    const hatched = said.heard === "muted-in-song";
    if ("notes" in c) {
      const notes = audibleOf(c);
      const paint = () => {
        g.save();
        area(p, ax, at, aw, ah);
        paintNotes(drawing(p, ink, at), notes, ax, k, ah, lo, hi, fineAt(shortestOf(notes), z));
        g.restore();
      };
      if (covered[i]) mineNow(paint);
      else later.canvas.push(paint);
    } else if (lane.kind === "steps") {
      const step = sc.step && sameClip(sc.step.clip, ref) ? pickedStep(sc.step, (c as StepClip).points, false) : -1;
      mineNow(() => steps(p, sc, c as StepClip, lane, ax, at, aw, ah, ink, step));
    } else if ("points" in c) {
      const curve = c as ControlClip;
      const paint = () => {
        g.save();
        area(p, ax, at, aw, ah);
        paintCurve(drawing(p, ink, at), curve.points, curve.length, ax, k, ah, scale);
        g.restore();
      };
      const pickedPoint = sc.point && sameClip(sc.point.clip, ref) ? pickedIn(sc.point, curve.points, false) : -1;
      const points = () => curve.points.forEach(([b, v], j) => point(p, sc, ax + (b / curve.length) * aw, at + (1 - (scale?.level(v) ?? 0)) * ah, ink, j === pickedPoint));
      if (covered[i]) mineNow(() => (paint(), points()));
      else {
        later.canvas.push(paint);
        later.over.push(() => mineNow(points));
      }
    } else {
      const audio = c as AudioClip;
      mineNow(() => wave(p, sc, audio, ax, bt + BORDER, aw, bh - 2 * BORDER, ink, z));
      if (picked && said.editable) later.top.push(() => mineNow(() => handles(p, sc, audio, ax, bt + BORDER, z, ink)));
    }
    if (hatched) {
      if (own) hatch(p, sc, x, bt, w, bh);
      else later.over.push(() => hatch(p, sc, x, bt, w, bh));
    }
  });
  for (const f of later.canvas) f();
  for (const f of later.over) f();
  for (const f of later.top) f();
  if (lane.kind === "notes" || lane.kind === "audio") recording(p, sc, row, z, X, top, bottom);
  g.restore();
  if (faded) p.end();
}

/** A take's row (VOX-7): over each comp of its lane, the whole take, faint, its parts that play lit in the track's tint. */
export function takeRow(p: Brush, sc: Scene, row: Row, z: number, X: number, top: number, bottom: number) {
  const lane = row.lane;
  if (!lane) return;
  const { bar, tint, ink } = row.said.color;
  const g = p.g;
  for (const c of compsOf(lane, sc.a.timeline.tempo)) {
    if (row.take >= c.takes.length) continue;
    // Where its layout puts it: in the 64ths of a px a layout keeps.
    const x0 = Math.round((c.start * z + 1) * 64) / 64 - X, w = Math.max((c.end - c.start) * z - 2, 4);
    const l = p.snap(x0), r = p.snap(x0 + w), t = top + 4, b = bottom - 4;
    if (r < 0 || l > sc.w) continue;
    g.save();
    g.beginPath();
    rrect(g, l + 1, t + 1, r - l - 2, b - t - 2, ROUND - 1);
    g.clip();
    for (const part of c.parts) if (part.take === row.take) p.box(l + 1 + (part.lo - c.start) * z, t + 1, l + 1 + (part.hi - c.start) * z, b - 1, tint);
    // Its waveform's tiles are where the comp's layout puts them, not where its box is drawn.
    wave(p, sc, takeClip(lane, c, row.take), x0 + 1, t + 1, r - l - 2, b - t - 2, ink, z, true);
    for (const part of c.parts) if (part.take !== row.take) p.box(l + 1 + (part.lo - c.start) * z, t + 1, l + 1 + (part.hi - c.start) * z, b - 1, sc.colors.canvas, 0.6);
    g.restore();
    g.fillStyle = p.c(bar);
    g.beginPath();
    rrect(g, l, t, r - l, b - t, ROUND);
    rrect(g, l + 1, t + 1, r - l - 2, b - t - 2, ROUND - 1);
    g.fill("evenodd");
  }
}

/** A bus's row's words (RENDER-34), at its lane's start. */
export function busRow(p: Brush, sc: Scene, words: string, X: number, top: number, bottom: number) {
  p.text(words, 12 - X, top + (bottom - top - 14) / 2 + sc.type.baseline(400, 11, 14), sc.type.font(400, 11), sc.colors.fgFaint);
}


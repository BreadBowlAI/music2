// The lanes in the zoom picture (taste.md VIEW-13): each track's rows, their grid and clips, the line
// under each track, the grid under the last, what lies over the lanes (the loop's shade, the selected
// bars, the playhead) and the Song row at the foot, with its own. In the page's px and order.

import { spanBox } from "../../camera";
import { BEAT_LINES, busWords, fineStep } from "../../lib/arrangement";
import { meterSpans } from "../../lib/graph";
import { RULER_H } from "../../lib/layout";
import { useStore } from "../../lib/store";
import { busRow, laneClips, takeRow, type DrawnClip } from "./clips";
import { rrect, type Pen } from "./pen";
import type { Row, Scene } from "./scene";

/** Lines 1 px wide, `every` px apart from `from` (as an SVG pattern draws them), across `x0` to `x1`, `y0` to `y1` down. */
function lines(p: Pen, from: number, every: number, x0: number, x1: number, y0: number, y1: number, color: string) {
  if (every <= 0) return;
  for (let i = Math.max(0, Math.floor((x0 - from) / every) - 1); ; i++) {
    const x = from + i * every;
    if (x >= x1) break;
    if (x + 1 > x0) p.fill(x, y0, Math.min(1, x1 - x), y1 - y0, color);
  }
}

/** The lanes' grid from `y0` to `y1` down: each time signature's stretch, its snap lines, beats and bars, from where the zoom places it. */
export function grid(p: Pen, sc: Scene, z: number, X: number, y0: number, y1: number) {
  const { a, colors } = sc;
  for (const s of meterSpans(a.timeline.meter, 0, Infinity)) {
    const left = spanBox(s.start, null, z).left - X;
    const right = s.end === Infinity ? Infinity : left + (spanBox(s.start, s.end, z).width ?? 0);
    if (left > sc.w || right < 0) continue;
    const x0 = Math.max(left, 0), x1 = Math.min(right, sc.w + 1);
    const step = fineStep(a.grid, s.bar, z);
    if (step) lines(p, left, step * z, x0, x1, y0, y1, colors.hover);
    if (z >= BEAT_LINES) lines(p, left, z, x0, x1, y0, y1, colors.line);
    lines(p, left, s.bar * z, x0, x1, y0, y1, colors.lineStrong);
  }
}

/** One row of a track: its lane's wash and top line, its grid and its clips; a take's row; or the line under hidden lanes. */
function row(p: Pen, sc: Scene, r: Row, z: number, X: number, drawn: DrawnClip[]) {
  const c = sc.colors;
  const top = r.y + (r.border ? 1 : 0), bottom = r.y + r.h;
  if (r.kind === "take" || r.kind === "hidden") {
    p.fill(0, r.y, sc.w, 1, c.hover);
    if (r.kind === "take") takeRow(p, sc, r, z, X, top, bottom);
    return;
  }
  if (r.said.selected) p.fill(0, r.y, sc.w, r.h, c.accentWash);
  if (r.border) p.fill(0, r.y, sc.w, 1, c.hover);
  grid(p, sc, z, X, top, bottom);
  if (r.lane) laneClips(p, sc, r, z, X, top, bottom, drawn);
  else if (r.kind === "first" && r.said.track.bus) busRow(p, sc, busWords(useStore.getState().graph, r.module), X, top, bottom);
}

/** What the lanes' picture drew, in px from the lanes' left: for the suites (VIEW-13). */
export interface DrawnLanes {
  clips: DrawnClip[];
  /** The loop's shade, the selected bars and the playhead over the lanes, and over the Song row; where the first row's grid starts (beat 0). */
  loop: { x: number; w: number } | null;
  section: { x: number; w: number } | null;
  playhead: number;
  songLoop: { x: number; w: number } | null;
  songSection: { x: number; w: number } | null;
  songPlayhead: number | null;
  grid: number | null;
}

/** What `over` drew. */
type Over = Pick<DrawnLanes, "loop" | "section" | "playhead">;

/** What lies over the lanes from `y0` to `y1`: the loop's shade (multiplied in), the selected bars, the playhead. */
function over(p: Pen, sc: Scene, z: number, X: number, y0: number, y1: number, beat: number, d: Over) {
  const { a, colors } = sc;
  if (a.loop?.on) {
    p.multiply(a.loop.start * z - X, y0, a.loop.start * z - X + (a.loop.end - a.loop.start) * z, y1, colors.accent, 0.08);
    d.loop = { x: a.loop.start * z - X, w: (a.loop.end - a.loop.start) * z };
  }
  if (a.bars) {
    d.section = { x: a.bars[0] * z - X, w: (a.bars[1] - a.bars[0]) * z };
    const l = p.snap(a.bars[0] * z - X), r = p.snap(a.bars[0] * z - X + (a.bars[1] - a.bars[0]) * z);
    const g = p.g;
    g.fillStyle = p.c(colors.accent);
    g.globalAlpha = 0.1;
    g.beginPath();
    rrect(g, l, y0, r - l, y1 - y0, 2);
    g.fill();
    g.globalAlpha = 1;
    g.beginPath();
    rrect(g, l, y0, r - l, y1 - y0, 2);
    rrect(g, l + 1, y0 + 1, r - l - 2, y1 - y0 - 2, 1);
    g.fill("evenodd");
  }
  p.fill(beat * z - X, y0, 1, y1 - y0, colors.fgStrong);
  d.playhead = beat * z - X;
}

/** The lanes' picture at zoom `z` seen from `X` (the box's scroll across at the zoom's end), the playhead at `beat`. */
export function paintLanes(p: Pen, sc: Scene, z: number, X: number, beat: number): DrawnLanes {
  const c = sc.colors;
  const d: DrawnLanes = { clips: [], loop: null, section: null, playhead: beat * z - X, songLoop: null, songSection: null, songPlayhead: null, grid: null };
  p.fill(0, RULER_H, sc.w, sc.h - RULER_H, c.canvas);
  for (const r of sc.rows) row(p, sc, r, z, X, d.clips);
  for (const y of sc.feet) p.fill(0, y, sc.w, 1, c.line);
  if (sc.filler) grid(p, sc, z, X, sc.filler.y, sc.filler.y + sc.filler.h);
  if (sc.rows.length) d.grid = spanBox(0, null, z).left - X;
  over(p, sc, z, X, RULER_H, sc.h, beat, d);
  const s = sc.song;
  if (s) {
    p.fill(0, s.y, sc.w, s.h, c.canvas);
    p.fill(0, s.y, sc.w, 1, c.line);
    for (const r of s.rows) row(p, sc, r, z, X, d.clips);
    const o: Over = { loop: null, section: null, playhead: 0 };
    over(p, sc, z, X, s.y + 1, s.y + s.h, beat, o);
    d.songLoop = o.loop;
    d.songSection = o.section;
    d.songPlayhead = o.playhead;
  }
  return d;
}

// The lanes in the zoom picture (taste.md VIEW-13): each track's rows, their grid and clips, the line
// under each track, the grid under the last, what lies over the lanes (the loop's shade, the selected
// bars, the playhead) and the Song row at the foot, with its own. In the page's px and order.

import { spanBox } from "../../camera";
import { BEAT_LINES, busWords, fineStep } from "../../lib/arrangement";
import { meterSpans } from "../../lib/graph";
import { RULER_H } from "../../lib/layout";
import { useStore } from "../../lib/store";
import { busRow, laneClips, takeRow } from "./clips";
import type { Brush } from "./brush";
import type { Row, Scene } from "./scene";

/**
 * The lanes' grid from `y0` to `y1` down: each time signature's stretch, its snap lines, beats and
 * bars, from where the zoom places it. With `listed`, each bar line is listed (`grid`) at its beat.
 */
export function grid(p: Brush, sc: Scene, z: number, X: number, y0: number, y1: number, listed = false) {
  const { a, colors } = sc;
  for (const s of meterSpans(a.timeline.meter, 0, Infinity)) {
    const left = spanBox(s.start, null, z).left - X;
    const right = s.end === Infinity ? Infinity : left + (spanBox(s.start, s.end, z).width ?? 0);
    if (left > sc.w || right < 0) continue;
    const x0 = Math.max(left, 0), x1 = Math.min(right, sc.w + 1);
    const step = fineStep(a.grid, s.bar, z);
    if (step) p.lines(left, step * z, x0, x1, y0, y1, colors.hover);
    if (z >= BEAT_LINES) p.lines(left, z, x0, x1, y0, y1, colors.line);
    p.lines(left, s.bar * z, x0, x1, y0, y1, colors.lineStrong, listed ? { kind: "grid", at: [s.start, s.bar] } : undefined);
  }
}

/** One row of a track: its lane's wash and top line, its grid and its clips; a take's row; or the line under hidden lanes. */
function row(p: Brush, sc: Scene, r: Row, z: number, X: number) {
  const c = sc.colors;
  const top = r.y + (r.border ? 1 : 0), bottom = r.y + r.h;
  if (r.kind === "take" || r.kind === "hidden") {
    p.fill(0, r.y, sc.w, 1, c.hover);
    if (r.kind === "take") takeRow(p, sc, r, z, X, top, bottom);
    return;
  }
  if (r.said.selected) p.fill(0, r.y, sc.w, r.h, c.accentWash);
  if (r.border) p.fill(0, r.y, sc.w, 1, c.hover);
  grid(p, sc, z, X, top, bottom, r === sc.rows[0]);
  if (r.lane) laneClips(p, sc, r, z, X, top, bottom);
  else if (r.kind === "first" && r.said.track.bus) busRow(p, sc, busWords(useStore.getState().graph, r.module), X, top, bottom);
}

/**
 * What lies over the lanes from `y0` to `y1` (the Song row's, with `song`): the loop's shade
 * (multiplied in), the selected bars, the playhead. Each listed by kind (`songLoop`, … for the Song row's).
 */
function over(p: Brush, sc: Scene, z: number, X: number, y0: number, y1: number, beat: number, song: boolean) {
  const { a, colors } = sc;
  const kind = (k: string) => (song ? `song${k[0].toUpperCase()}${k.slice(1)}` : k);
  const loop = a.loop;
  if (loop?.on) p.as(kind("loop"), () => p.multiply(loop.start * z - X, y0, loop.start * z - X + (loop.end - loop.start) * z, y1, colors.accent, 0.08));
  const bars = a.bars;
  if (bars)
    p.as(kind("section"), () => {
      const l = p.snap(bars[0] * z - X), r = p.snap(bars[0] * z - X + (bars[1] - bars[0]) * z);
      p.g.globalAlpha = 0.1;
      p.round(l, y0, r - l, y1 - y0, 2, colors.accent);
      p.g.globalAlpha = 1;
      p.round(l, y0, r - l, y1 - y0, 2, colors.accent, [l + 1, y0 + 1, r - l - 2, y1 - y0 - 2, 1]);
    });
  p.as(kind("playhead"), () => p.fill(beat * z - X, y0, 1, y1 - y0, colors.fgStrong));
}

/** The lanes' picture at zoom `z` seen from `X` (the box's scroll across at the zoom's end), the playhead at `beat`. What it put where is in `p.put`. */
export function paintLanes(p: Brush, sc: Scene, z: number, X: number, beat: number) {
  const c = sc.colors;
  p.fill(0, RULER_H, sc.w, sc.h - RULER_H, c.canvas);
  for (const r of sc.rows) row(p, sc, r, z, X);
  for (const y of sc.feet) p.fill(0, y, sc.w, 1, c.line);
  if (sc.filler) grid(p, sc, z, X, sc.filler.y, sc.filler.y + sc.filler.h);
  over(p, sc, z, X, RULER_H, sc.h, beat, false);
  const s = sc.song;
  if (s) {
    p.fill(0, s.y, sc.w, s.h, c.canvas);
    p.fill(0, s.y, sc.w, 1, c.line);
    for (const r of s.rows) row(p, sc, r, z, X);
    over(p, sc, z, X, s.y + 1, s.y + s.h, beat, true);
  }
}

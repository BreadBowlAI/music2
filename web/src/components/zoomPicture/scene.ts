// The scene the arrangement's zoom picture draws (taste.md VIEW-5, VIEW-13): read as a zoom by the
// hand begins, from what the view renders from (`Said`: the song, each track's rows' state) and the
// stores it reads (the selection, the picked point, what an agent just changed). Where each row is
// down the view, and where the page puts a line of text's baseline, are the page's: read once a zoom,
// at a frame's start, where its layout is the last frame's and costs nothing. Nothing of it depends
// on the zoom, which the paint takes.

import { useClipSelection } from "../../lib/clips";
import type { TimelineLane } from "../../lib/graph";
import { useNotice } from "../../lib/notice";
import { usePoints } from "../../lib/points";
import { useSteps } from "../../lib/steps";
import type { ArrangementSaid, Said, TrackSaid } from "./said";

/** A row of a track, where it is down the view. */
export interface Row {
  /** A lane's row (the track's own first), a take's (VOX-7), or the line saying lanes are hidden (D4). */
  kind: "first" | "lane" | "take" | "hidden";
  module: string;
  said: TrackSaid;
  /** Its lane (a take row's audio lane); null for a bus's own row. */
  lane: TimelineLane | null;
  /** A take row's take, from 0. */
  take: number;
  /** Its box down the view, in px: from `y`, `h` tall, a 1 px line at its top (`border`) but on a track's first. */
  y: number;
  h: number;
  border: boolean;
}

/** Each colour the page draws with, by its token's name. */
export type Colors = Record<
  "canvas" | "chrome" | "hover" | "line" | "lineStrong" | "fgFaint" | "fgMuted" | "fg" | "fgStrong" | "accent" | "accentWash" | "accentSoft" | "accentRange" | "danger" | "dangerSoft" | "dangerStrong" | "raised",
  string
>;

/** The page's words: `font` for each size and weight, and where its baseline is in a line box `lineH` px tall. */
export interface Type {
  family: string;
  /** "500 11px Inter…" */
  font(weight: number, size: number): string;
  baseline(weight: number, size: number, lineH: number): number;
}

export interface Scene {
  dpr: number;
  /** The lanes' clear part, in px: from the headers' edge (`x` 0) across, from the view's top down. */
  w: number;
  h: number;
  /** Where the ruler's tools start across: the ruler's picture stops there. */
  tools: number;
  /** Each track's rows in view, top to bottom; the Song row's apart. */
  rows: Row[];
  /** The line under each track. */
  feet: number[];
  /** The grid under the last track. */
  filler: { y: number; h: number } | null;
  /** The Song row at the foot: its box, its line at its top, and its rows. */
  song: { y: number; h: number; rows: Row[] } | null;
  /** A word in the empty view's middle (no tracks, or the code not compiled yet): the picture leaves its box to the page. */
  hole: { x: number; y: number; w: number; h: number } | null;
  colors: Colors;
  type: Type;
  a: ArrangementSaid;
  /** What the stores say now: the picked clips, the picked point of a curve and of a lane of names, the lanes an agent just changed. */
  picked: ReturnType<typeof useClipSelection.getState>["picked"];
  point: ReturnType<typeof usePoints.getState>["picked"];
  step: ReturnType<typeof useSteps.getState>["picked"];
  glows: ReadonlySet<string>;
}

const css = (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/** Each baseline measured, by family, size, weight and line height: the page's never changes. */
const baselines = new Map<string, number>();

/**
 * Where the page puts the baseline of text in a line box `lineH` px tall, from its top: an empty
 * inline block on the line sits on the baseline (each engine rounds a font's ascent and its
 * half-leading its own way). Measured once in the box `host`.
 */
function measureBaseline(host: HTMLElement, family: string, weight: number, size: number, lineH: number) {
  const key = `${family}|${weight}|${size}|${lineH}|${window.devicePixelRatio}`;
  const known = baselines.get(key);
  if (known !== undefined) return known;
  const line = document.createElement("div");
  line.style.cssText = `position: absolute; left: 0; top: 0; visibility: hidden; white-space: nowrap; font: ${weight} ${size}px/${lineH}px ${family}`;
  const mark = document.createElement("span");
  mark.style.cssText = "display: inline-block; width: 0; height: 0";
  line.append(mark, "0");
  host.appendChild(line);
  const at = mark.getBoundingClientRect().top - line.getBoundingClientRect().top;
  line.remove();
  baselines.set(key, at);
  return at;
}

/** The rows of one track's element `el` in view, down from the view's `top` (its `h` px). */
function rowsOf(el: HTMLElement, said: TrackSaid, top: number, h: number): Row[] {
  const rows: Row[] = [];
  const module = said.track.module;
  for (const child of el.children) {
    const row = child as HTMLElement;
    const kind = row.dataset.row as Row["kind"] | undefined;
    if (!kind) continue;
    const r = row.getBoundingClientRect();
    const y = r.top - top;
    if (y > h || y + r.height < 0 || r.height <= 0) continue;
    const key = row.dataset.lane?.slice(module.length + 1);
    const lane = key === undefined ? null : (said.track.lanes.find((l) => l.key === key) ?? null);
    rows.push({ kind, module, said, lane, take: Number(row.dataset.take ?? 1) - 1, y, h: r.height, border: kind !== "first" });
  }
  return rows;
}

/**
 * The scene of the arrangement in the scroll box `box` (its headers `headerW` wide), as it renders
 * now. Null before the arrangement has said what it renders.
 */
export function readScene(box: HTMLElement, said: Said): Scene | null {
  const a = said.arrangement;
  if (!a) return null;
  const b = box.getBoundingClientRect();
  const left = b.left + box.clientLeft + a.headerW, top = b.top + box.clientTop;
  const w = box.clientWidth - a.headerW, h = box.clientHeight;
  const rows: Row[] = [];
  const feet: number[] = [];
  let song: Scene["song"] = null;
  const songEl = box.querySelector<HTMLElement>("[data-song-row]");
  for (const el of box.querySelectorAll<HTMLElement>("[data-track]")) {
    const t = said.tracks.get(el.dataset.track!);
    if (!t) continue;
    if (t.last && songEl?.contains(el)) {
      const s = songEl.getBoundingClientRect();
      song = { y: s.top - top, h: s.height, rows: rowsOf(el, t, top, h) };
      continue;
    }
    const r = el.getBoundingClientRect();
    if (r.bottom < top || r.top > top + h) continue;
    rows.push(...rowsOf(el, t, top, h));
    if (!t.last) feet.push(r.bottom - top - 1);
  }
  const rect = (sel: string) => {
    const el = box.querySelector<HTMLElement>(sel);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.left - left, y: r.top - top, w: r.width, h: r.height };
  };
  const filler = rect("[data-lanes-end]");
  const tools = rect("[data-testid=view-tools]");
  const family = getComputedStyle(box).fontFamily;
  const type: Type = {
    family,
    font: (weight, size) => `${weight} ${size}px ${family}`,
    baseline: (weight, size, lineH) => measureBaseline(box, family, weight, size, lineH),
  };
  const colors: Colors = {
    canvas: css("--color-canvas"),
    chrome: css("--color-chrome"),
    hover: css("--color-hover"),
    line: css("--color-line"),
    lineStrong: css("--color-line-strong"),
    fgFaint: css("--color-fg-faint"),
    fgMuted: css("--color-fg-muted"),
    fg: css("--color-fg"),
    fgStrong: css("--color-fg-strong"),
    accent: css("--color-accent"),
    accentWash: css("--color-accent-wash"),
    accentSoft: css("--color-accent-soft"),
    accentRange: css("--color-accent-range"),
    danger: css("--color-danger"),
    dangerSoft: css("--color-danger-soft"),
    dangerStrong: css("--color-danger-strong"),
    raised: css("--color-raised"),
  };
  return {
    dpr: window.devicePixelRatio || 1,
    w,
    h,
    tools: tools ? tools.x : w,
    rows,
    feet,
    filler: filler && { y: filler.y, h: filler.h },
    song,
    hole: rect("[data-testid=waiting], [data-testid=no-tracks]"),
    colors,
    type,
    a,
    picked: useClipSelection.getState().picked,
    point: usePoints.getState().picked,
    step: useSteps.getState().picked,
    glows: useNotice.getState().glow?.lanes ?? new Set(),
  };
}

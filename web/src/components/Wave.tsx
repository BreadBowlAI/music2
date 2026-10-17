// An audio clip's waveform (lane A's A1, taste.md §2.8): the louder of its file's two channels,
// mirrored about the middle, in the track's `ink`, under a 14 px strip with the file's name. It
// shows what plays: the part of the file the clip starts at (`offset`), backwards when reversed,
// scaled by its volume (clipped at the body's edge), tapered by its fades, each fade a thin
// `fg-muted` curve. Drawn on canvas tiles of 1,024 px, each only while it is on screen.

import { AudioWaveform } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import type { AudioClip } from "../lib/graph";
import type { TrackColor } from "../lib/palette";
import { clipSeconds, MISSING, secondsAt, usePeaks, type Peaks, type Tempo } from "../lib/peaks";
import { useStore } from "../lib/store";
import { useZoom } from "../lib/zoom";

const TILE = 1024;
/** From a clip's left edge to its inside: the 1 px gap before it and its 1 px border. */
const INSET = 2;
/** The name's strip at the clip's top (taste.md §2.8). */
export const STRIP = 14;
const NO_TEMPO: Tempo = [];

/** "−6 dB", "+1.5 dB": a clip's volume, as its label says it (Q11). */
export function dbWords(db: number): string {
  const n = Math.round(db * 10) / 10;
  return `${n < 0 ? "−" : "+"}${Math.abs(n)} dB`;
}

/** The clip's label: its file, and its volume when it isn't 0 dB (Q11). */
export const audioLabel = (c: AudioClip) => (c.gain_db ? `${c.asset} · ${dbWords(c.gain_db)}` : c.asset);

/** The gain of a half-cosine fade `t` of the way in (as the renderer makes it). */
const curve = (t: number) => 0.5 - 0.5 * Math.cos(Math.PI * Math.min(1, Math.max(0, t)));

/** What the clip plays at each point: its file's seconds, and the gain there. */
interface Shape {
  /** Seconds into the clip at x px from its start. */
  at: (x: number) => number;
  /** The file's second the clip plays `s` seconds in. */
  source: (s: number) => number;
  /** The gain `s` seconds in: volume and fades. */
  gain: (s: number) => number;
  seconds: number;
}

function shapeOf(c: AudioClip, tempo: Tempo, ppb: number): Shape {
  const t0 = secondsAt(tempo, c.start);
  const seconds = clipSeconds(tempo, c);
  const rate = 2 ** ((c.pitch ?? 0) / 12);
  const offset = c.offset ?? 0;
  const volume = 10 ** ((c.gain_db ?? 0) / 20);
  const fadeIn = c.fade_in ?? 0;
  const fadeOut = c.fade_out ?? 0;
  return {
    at: (x) => secondsAt(tempo, c.start + (x + INSET) / ppb) - t0,
    // A reversed clip plays its placed span backwards (`render_clip`).
    source: (s) => offset + (c.reverse ? seconds - s : s) * rate,
    gain: (s) => volume * (fadeIn > 0 && s < fadeIn ? curve(s / fadeIn) : 1) * (fadeOut > 0 && seconds - s < fadeOut ? curve((seconds - s) / fadeOut) : 1),
    seconds,
  };
}

/** The loudest sample of the file between seconds `a` and `b`, 0 to 1, over every channel. */
function loudest(p: Peaks, a: number, b: number): number {
  const lo = Math.min(a, b) * p.rate;
  const hi = Math.max(a, b) * p.rate;
  if (hi <= 0 || lo >= p.frames) return 0;
  const buckets = Math.ceil(p.frames / p.bucket);
  const first = Math.max(0, Math.floor(lo / p.bucket));
  const last = Math.min(buckets - 1, Math.max(first, Math.ceil(hi / p.bucket) - 1));
  let most = 0;
  for (let k = first; k <= last; k++) {
    const i = k * p.channels * 2;
    for (let j = 0; j < p.channels * 2; j++) most = Math.max(most, Math.abs(p.data[i + j]));
  }
  return most / 127;
}

/** Draw the tile `x0`..`x0 + width` (px from the clip's start) of the waveform on `canvas`. */
function draw(canvas: HTMLCanvasElement, p: Peaks, shape: Shape, x0: number, width: number, height: number, ink: string) {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(width * dpr));
  const h = Math.max(1, Math.round(height * dpr));
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  const g = canvas.getContext("2d");
  if (!g) return;
  g.clearRect(0, 0, w, h);
  g.fillStyle = ink;
  const mid = h / 2;
  for (let d = 0; d < w; d++) {
    const s0 = shape.at(x0 + d / dpr);
    const s1 = shape.at(x0 + (d + 1) / dpr);
    const amp = Math.min(1, loudest(p, shape.source(s0), shape.source(s1)) * shape.gain((s0 + s1) / 2));
    // Never thinner than a device pixel: silence is a flat line.
    const half = Math.max(0.5, amp * mid);
    g.fillRect(d, mid - half, 1, 2 * half);
  }
}

/** The fades as thin curves, drawn over the waveform, from silence at the clip's edge to full. */
function drawFades(canvas: HTMLCanvasElement, shape: Shape, fadeIn: number, fadeOut: number, x0: number, line: string) {
  const g = canvas.getContext("2d");
  if (!g || (!fadeIn && !fadeOut)) return;
  const dpr = window.devicePixelRatio || 1;
  const { width: w, height: h } = canvas;
  g.strokeStyle = line;
  g.lineWidth = dpr;
  for (const [on, of] of [
    [fadeIn > 0, (s: number) => (s < fadeIn ? curve(s / fadeIn) : null)],
    [fadeOut > 0, (s: number) => (shape.seconds - s < fadeOut ? curve((shape.seconds - s) / fadeOut) : null)],
  ] as const) {
    if (!on) continue;
    g.beginPath();
    let open = false;
    for (let d = 0; d <= w; d++) {
      const v = of(shape.at(x0 + d / dpr));
      if (v === null) {
        open = false;
        continue;
      }
      const y = h - 0.5 * dpr - v * (h - dpr);
      if (open) g.lineTo(d, y);
      else g.moveTo(d, y);
      open = true;
    }
    g.stroke();
  }
}

/** One tile: drawn once it is on screen, and again when what it shows changes. */
function Tile({ x0, width, height, peaks, shape, clip, ink }: { x0: number; width: number; height: number; peaks: Peaks; shape: Shape; clip: AudioClip; ink: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [seen, setSeen] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || seen) return;
    const io = new IntersectionObserver((es) => es.some((e) => e.isIntersecting) && setSeen(true), { rootMargin: "0px 256px" });
    io.observe(el);
    return () => io.disconnect();
  }, [seen]);
  useEffect(() => {
    const el = ref.current;
    if (!el || !seen || height <= 0) return;
    draw(el, peaks, shape, x0, width, height, ink);
    const muted = getComputedStyle(el).getPropertyValue("--color-fg-muted").trim() || "#6b7076";
    drawFades(el, shape, clip.fade_in ?? 0, clip.fade_out ?? 0, x0, muted);
  }, [seen, peaks, shape, x0, width, height, ink, clip.fade_in, clip.fade_out]);
  return <canvas ref={ref} data-wave-tile={x0 / TILE} className="absolute top-0 h-full" style={{ left: x0, width }} />;
}

/** The inside of audio clip `clip`: its name, then its waveform. */
export const Wave = memo(function Wave({ clip, color }: { clip: AudioClip; color: TrackColor }) {
  const peaks = usePeaks(clip);
  const ppb = useZoom();
  const tempo = useStore((s) => s.graph?.timeline.tempo) ?? NO_TEMPO;
  const box = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  const { start, length, offset, reverse, pitch, gain_db, fade_in, fade_out } = clip;
  // Made again only when what it shows changes, so a tile redraws only then.
  const shape = useMemo(
    () => shapeOf({ asset: "", start, length, offset, reverse, pitch, gain_db, fade_in, fade_out }, tempo, ppb),
    [start, length, offset, reverse, pitch, gain_db, fade_in, fade_out, tempo, ppb],
  );
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setHeight(el.clientHeight));
    ro.observe(el);
    setHeight(el.clientHeight);
    return () => ro.disconnect();
  }, [peaks === MISSING]);
  if (peaks === MISSING)
    return (
      <div className="absolute inset-x-0 bottom-0 top-1 flex items-center gap-1.5 px-2 text-xs font-medium" style={{ color: color.ink }}>
        <AudioWaveform size={14} className="shrink-0" />
        <span className="truncate">{audioLabel(clip)}</span>
      </div>
    );
  const width = Math.max(0, clip.length * ppb - 2 * INSET);
  const tiles = Array.from({ length: Math.ceil(width / TILE) }, (_, i) => i * TILE);
  return (
    <>
      <div data-clip-name className="absolute inset-x-0 top-0 px-1.5 text-xs leading-[14px] truncate pointer-events-none" style={{ height: STRIP, color: color.ink }}>
        {audioLabel(clip)}
      </div>
      <div ref={box} data-wave={peaks ? "" : "loading"} aria-hidden className="absolute inset-x-0 bottom-0.5 pointer-events-none" style={{ top: STRIP }}>
        {peaks && tiles.map((x0) => <Tile key={x0} x0={x0} width={Math.min(TILE, width - x0)} height={height} peaks={peaks} shape={shape} clip={clip} ink={color.ink} />)}
      </div>
    </>
  );
});

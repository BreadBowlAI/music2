// An audio clip's waveform (lane A's A1, taste.md §2.8): the louder of its file's two channels,
// mirrored about the middle, in the track's `ink`, under a 14 px strip with the file's name. It
// shows what plays: the part of the file the clip starts at (`offset`), backwards when reversed,
// scaled by its volume (clipped at the body's edge), tapered by its fades, each fade a thin
// `fg-muted` curve. Drawn on canvas tiles of 1,024 px, each only while it is on screen.

import clsx from "clsx";
import { AudioWaveform } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import { send, serverRef, type Gesture } from "../lib/clips";
import { clipIndex, findLane, moduleName, type AudioClip, type ClipRef, type TimelineLane } from "../lib/graph";
import type { TrackColor } from "../lib/palette";
import { beatAt, clipSeconds, MISSING, secondsAt, usePeaks, type Peaks, type Tempo } from "../lib/peaks";
import { request } from "../lib/projects";
import { editing, shown } from "../lib/history";
import { useStore } from "../lib/store";
import { useZoom } from "../lib/zoom";
import type { MenuItem } from "./ClipMenu";
import { Popover } from "./ui";

/** A clip's volume, in dB (Q11): as the server takes it. */
const GAIN = [-48, 12] as const;

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
  const warp = c.warp ?? [];
  return {
    at: (x) => secondsAt(tempo, c.start + (x + INSET) / ppb) - t0,
    // A reversed clip plays its placed span backwards (`render_clip`); a warped one (Follow tempo)
    // plays its file by its markers, whatever its pitch.
    source: (s) => {
      const t = c.reverse ? seconds - s : s;
      return warp.length > 0 ? warpedAt(warp, beatAt(tempo, t0 + t) - c.start, c.start, tempo) : offset + t * rate;
    },
    gain: (s) => volume * (fadeIn > 0 && s < fadeIn ? curve(s / fadeIn) : 1) * (fadeOut > 0 && seconds - s < fadeOut ? curve((seconds - s) / fadeOut) : 1),
    seconds,
  };
}

/**
 * The file's second a warped clip plays `b` beats in (the runtime's `time_map`): in a line between
 * its markers, and past them the nearest two go on; one marker pins a moment, and the file runs at
 * its own speed around it.
 */
function warpedAt(w: [number, number][], b: number, start: number, tempo: Tempo): number {
  if (w.length === 1) return w[0][0] + secondsAt(tempo, start + b) - secondsAt(tempo, start + w[0][1]);
  const after = w.findIndex((m) => m[1] > b);
  const i = Math.min(Math.max(after < 0 ? w.length : after, 1), w.length - 1);
  const [a, z] = [w[i - 1], w[i]];
  return a[0] + ((b - a[1]) * (z[0] - a[0])) / (z[1] - a[1]);
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
function Tile({ x0, width, height, peaks, shape, fades, ink }: { x0: number; width: number; height: number; peaks: Peaks; shape: Shape; fades: [number, number]; ink: string }) {
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
    drawFades(el, shape, fades[0], fades[1], x0, muted);
  }, [seen, peaks, shape, x0, width, height, ink, fades]);
  return <canvas ref={ref} data-wave-tile={x0 / TILE} className="absolute top-0 h-full" style={{ left: x0, width }} />;
}

/** What lets a clip's waveform be edited in place: its lane, how a gesture names it, and its track's name. */
export interface WaveEdit {
  ref: ClipRef;
  lane: TimelineLane;
  editable: boolean;
  picked: boolean;
}

/** "Vox": a track's name, as undo steps say it. */
function trackName(module: string) {
  const t = useStore.getState().graph?.timeline.tracks.find((x) => x.module === module);
  return moduleName({ key: module, label: t?.label });
}

/** The narrowest clip that shows its fade handles (D33). */
const HANDLES_FROM = 24;
/** A fade handle: 7 px, its top 8 px of the clip's corner (D33). */
const HANDLE = 7;

/** "0.40 s". */
const secondsWords = (s: number) => `${s.toFixed(2)} s`;

/**
 * Send an audio gesture on `ref` (one undo step, named by `name` from its track's name), saying a
 * refusal in a toast. An edit on its way from now, naming the clip as the timeline draws it once
 * the window shows its own last edit (the gate, lib/history): a fade right after a trim goes from it.
 */
export async function sendAudio(lane: TimelineLane, ref: ClipRef, set: Omit<Extract<Gesture, { op: "audio" }>, "op" | "clip">, name: (track: string) => string) {
  const { graph, toast } = useStore.getState();
  if (!graph || !serverRef(lane, ref)) return null;
  const ours = editing();
  try {
    const now = await shown(graph.session, ours);
    const drawn = now && findLane(now.timeline, ref.module, ref.lane);
    const clip = drawn && serverRef(drawn, ref);
    if (!now || !clip) return null;
    return await send(now, { op: "audio", clip, ...set }, { name: name(trackName(ref.module)) }, undefined, ours);
  } catch (error) {
    toast(error instanceof Error ? error.message.replace(/^edit rejected: /, "") : String(error));
    return null;
  } finally {
    ours.end();
  }
}

/** The inside of audio clip `clip`: its name, then its waveform, and (`edit`) its fade handles. */
export const Wave = memo(function Wave({ clip, color, edit }: { clip: AudioClip; color: TrackColor; edit?: WaveEdit }) {
  const peaks = usePeaks(clip);
  const ppb = useZoom();
  const tempo = useStore((s) => s.graph?.timeline.tempo) ?? NO_TEMPO;
  const box = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(0);
  // A fade being dragged, or just let go of: drawn so until the clip says it.
  const [fading, setFading] = useState<{ which: "in" | "out"; seconds: number; at: { x: number; y: number } | null } | null>(null);
  const fadeIn = fading?.which === "in" ? fading.seconds : (clip.fade_in ?? 0);
  const fadeOut = fading?.which === "out" ? fading.seconds : (clip.fade_out ?? 0);
  const { start, length, offset, reverse, pitch, gain_db } = clip;
  // Its markers by value: each graph update brings a new array.
  const warp = JSON.stringify(clip.warp ?? []);
  // Made again only when what it shows changes, so a tile redraws only then.
  const shape = useMemo(
    () => shapeOf({ asset: "", start, length, offset, reverse, pitch, gain_db, fade_in: fadeIn, fade_out: fadeOut, warp: JSON.parse(warp) }, tempo, ppb),
    [start, length, offset, reverse, pitch, gain_db, fadeIn, fadeOut, warp, tempo, ppb],
  );
  const fades = useMemo<[number, number]>(() => [fadeIn, fadeOut], [fadeIn, fadeOut]);
  // A fade let go of shows as written once the clip says it.
  useEffect(() => {
    if (fading && !fading.at && (fading.which === "in" ? clip.fade_in ?? 0 : clip.fade_out ?? 0) === fading.seconds) setFading(null);
  }, [clip.fade_in, clip.fade_out, fading]);
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
  /** Where `seconds` into the clip is, in px from its inside's left. */
  const xOf = (seconds: number) => Math.max(0, (beatAt(tempo, secondsAt(tempo, clip.start) + seconds) - clip.start) * ppb - INSET);

  /** A press on a fade handle: drag it in for a fade (Q10), one Tx when let go; Esc puts it back. */
  const down = (which: "in" | "out", e: React.PointerEvent<HTMLElement>) => {
    if (e.button !== 0 || !edit?.editable) return;
    // The handle's own drag: never the clip's move.
    e.stopPropagation();
    e.preventDefault();
    const el = e.currentTarget;
    const inside = el.parentElement!.getBoundingClientRect();
    const pointer = e.pointerId;
    el.setPointerCapture(pointer);
    const was = which === "in" ? (clip.fade_in ?? 0) : (clip.fade_out ?? 0);
    const other = which === "in" ? (clip.fade_out ?? 0) : (clip.fade_in ?? 0);
    let now = was;
    let moved = false;
    const at = (ev: PointerEvent) => {
      const s = shape.at(ev.clientX - inside.left);
      const seconds = which === "in" ? s : shape.seconds - s;
      return Math.round(Math.min(Math.max(0, seconds), Math.max(0, shape.seconds - other)) * 1000) / 1000;
    };
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      moved = true;
      now = at(ev);
      setFading({ which, seconds: now, at: { x: ev.clientX, y: ev.clientY } });
    };
    const off = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      window.removeEventListener("keydown", esc, true);
    };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      off();
      if (!moved || now === was || ev.type !== "pointerup") return setFading(null);
      setFading({ which, seconds: now, at: null });
      void sendAudio(edit.lane, edit.ref, which === "in" ? { fade_in: now } : { fade_out: now }, (t) => `${t} fade ${which}`).then((done) => {
        if (!done) setFading(null);
      });
    };
    const esc = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopImmediatePropagation();
      off();
      if (el.hasPointerCapture(pointer)) el.releasePointerCapture(pointer);
      setFading(null);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
    window.addEventListener("keydown", esc, true);
  };
  /** A double-click on a fade handle: no fade. */
  const none = (which: "in" | "out", e: React.MouseEvent) => {
    e.stopPropagation();
    if (!edit?.editable || (which === "in" ? !clip.fade_in : !clip.fade_out)) return;
    void sendAudio(edit.lane, edit.ref, which === "in" ? { fade_in: 0 } : { fade_out: 0 }, (t) => `${t} fade ${which}`);
  };
  const handles = !!edit?.editable && width + 2 * INSET >= HANDLES_FROM;
  const handle = (which: "in" | "out") => {
    const seconds = which === "in" ? fadeIn : fadeOut;
    const x = which === "in" ? Math.min(xOf(seconds), width - HANDLE) : Math.max(0, xOf(shape.seconds - seconds) - HANDLE);
    return (
      <span
        key={which}
        data-fade={which}
        title={`Fade ${which}: drag ${which === "in" ? "right" : "left"} · double-click for none`}
        onPointerDown={(e) => down(which, e)}
        onDoubleClick={(e) => none(which, e)}
        className={clsx(
          "absolute top-0 z-10 rounded-[2px] border bg-raised cursor-ew-resize touch-none",
          // Shown on hover or while the clip is selected (Q10), and while dragged.
          edit?.picked || fading?.which === which ? "opacity-100" : "opacity-0 in-[[data-clip]:hover]:opacity-100",
        )}
        style={{ left: x, width: HANDLE, height: HANDLE, borderColor: color.ink }}
      />
    );
  };
  return (
    <>
      <div data-clip-name className="absolute inset-x-0 top-0 px-1.5 text-xs leading-[14px] truncate pointer-events-none" style={{ height: STRIP, color: color.ink }}>
        {audioLabel(clip)}
      </div>
      <div ref={box} data-wave={peaks ? "" : "loading"} aria-hidden className="absolute inset-x-0 bottom-0.5 pointer-events-none" style={{ top: STRIP }}>
        {peaks && tiles.map((x0) => <Tile key={x0} x0={x0} width={Math.min(TILE, width - x0)} height={height} peaks={peaks} shape={shape} fades={fades} ink={color.ink} />)}
      </div>
      {handles && (
        <div className="absolute inset-x-0 top-0 h-0">
          {handle("in")}
          {handle("out")}
        </div>
      )}
      {fading?.at && <Bubble at={fading.at}>{`Fade ${fading.which} ${secondsWords(fading.seconds)}`}</Bubble>}
    </>
  );
});

/** The value bubble at the pointer while a fade is dragged (taste.md §2.14), as the timeline's. */
function Bubble({ at, children }: { at: { x: number; y: number }; children: ReactNode }) {
  return createPortal(
    <div
      aria-hidden
      data-testid="fade-bubble"
      className="fixed z-50 pointer-events-none -translate-x-1/2 -translate-y-full px-1.5 py-0.5 rounded-sm bg-fg-strong/92 text-white text-xs tabular-nums whitespace-nowrap"
      style={{ left: at.x, top: at.y - 12 }}
    >
      {children}
    </div>,
    document.body,
  );
}

// ---------------------------------------------------------------- the clip menu's rows and Volume…

/** Volume… open: where, and on which clip. */
const useVolume = create<{ at: { x: number; y: number }; lane: TimelineLane; ref: ClipRef } | null>(() => null);

/**
 * The clip menu's rows for one audio clip (Q11, Q12, A3), after Split: Volume…, Reverse (checked
 * while on), Follow tempo (checked while on, sx's D11) and Remove fades (greyed with none). `off`: why nothing can be edited now.
 */
export function audioRows(picked: { ref: ClipRef; lane: TimelineLane; clip: unknown }[], at: { x: number; y: number }, off: string | null): (MenuItem | "line" | null)[] {
  if (picked.length !== 1 || picked[0].lane.kind !== "audio") return [];
  const { ref, lane } = picked[0];
  const c = picked[0].clip as AudioClip;
  const following = (c.warp?.length ?? 0) > 0;
  return [
    { id: "volume", label: "Volume…", off, run: () => useVolume.setState({ at, lane, ref }) },
    {
      id: "reverse",
      label: "Reverse",
      checked: !!c.reverse,
      off,
      run: () => void sendAudio(lane, ref, { reverse: !c.reverse }, (t) => (c.reverse ? `playing the ${t} clip forwards` : `reversing the ${t} clip`)),
    },
    {
      id: "follow",
      label: "Follow tempo",
      checked: following,
      detail: "Stretch this clip with the song's tempo. Its pitch stays the same.",
      off,
      run: () => void sendAudio(lane, ref, { follow: !following }, () => (following ? "Follow tempo off" : "Follow tempo")),
    },
    {
      id: "unfade",
      label: "Remove fades",
      off: off ?? (c.fade_in || c.fade_out ? null : "This clip has no fades."),
      run: () => void sendAudio(lane, ref, { fade_in: 0, fade_out: 0 }, (t) => `removing the ${t} clip's fades`),
    },
    ...takeRows(lane, ref, c, off),
  ];
}

/**
 * The Takes menu (MON-11): a clip over a loop whose file holds a take for each time round lists
 * them, the one it plays checked. Choosing one is one undo step ("Vox take 2"): the clip's offset
 * moves to the same place in that take.
 */
function takeRows(lane: TimelineLane, ref: ClipRef, c: AudioClip, off: string | null): (MenuItem | "line")[] {
  const takes = lane.kind === "audio" ? (lane.takes?.[c.asset] ?? []) : [];
  if (takes.length < 2) return [];
  const offset = c.offset ?? 0;
  let now = 0;
  takes.forEach((t, i) => {
    if (t <= offset + 1e-6) now = i;
  });
  /** Its take: one edit on its way from the pick, naming the clip as drawn once the window shows its own last edit (the gate, lib/history). */
  const choose = async (take: number) => {
    const { graph, toast } = useStore.getState();
    if (!graph) return;
    const ours = editing();
    try {
      const now = await shown(graph.session, ours);
      const drawn = now && findLane(now.timeline, ref.module, ref.lane);
      const clip = drawn && serverRef(drawn, ref);
      if (now && clip) await request("/api/graph/take", { session: now.session, ...clip, take });
    } catch (error) {
      toast(error instanceof Error ? error.message : String(error));
    } finally {
      ours.end();
    }
  };
  const rows: MenuItem[] = takes.map((_, i) => ({
    id: `take-${i + 1}`,
    label: `Take ${i + 1}`,
    checked: i === now,
    detail: i === 0 ? "Each time round the loop is a take. The one ticked plays." : undefined,
    off,
    run: () => void choose(i + 1),
  }));
  return ["line", ...rows];
}

/** dB as the field shows it: "−3.0". */
const dbField = (db: number) => `${db < 0 ? "−" : ""}${Math.abs(db).toFixed(1)}`;

/**
 * Volume… (Q11, D20): "Clip volume" and one field, in dB, which has the focus. ↑ ↓ step 0.5 dB,
 * ⇧↑ ⇧↓ 3 dB; Return sets it (one undo step, "Vox clip volume"); Esc closes. −48 to +12 dB.
 */
export function VolumePopover() {
  const open = useVolume();
  const graph = useStore((s) => s.graph);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [draft, setDraft] = useState("0.0");
  const input = useRef<HTMLInputElement>(null);
  // The clip as the timeline draws it now.
  const lane = open && graph ? (findLane(graph.timeline, open.ref.module, open.ref.lane) ?? open.lane) : open?.lane;
  const i = lane && open ? clipIndex(lane.clips, open.ref) : -1;
  const clip = lane && i >= 0 ? (lane.clips[i] as AudioClip) : undefined;
  useEffect(() => {
    if (open) setDraft(dbField(clip?.gain_db ?? 0));
    // Only when it opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  useEffect(() => input.current?.select(), [anchor]);
  if (!open) return null;
  const close = () => useVolume.setState(null, true);
  const n = Number(draft.trim().replace("−", "-").replace(/\s*db$/i, ""));
  const ok = draft.trim() !== "" && Number.isFinite(n) && n >= GAIN[0] && n <= GAIN[1];
  const go = () => {
    if (!ok) return;
    close();
    const db = Math.round(n * 10) / 10;
    if (lane && db !== (clip?.gain_db ?? 0)) void sendAudio(lane, open.ref, { gain_db: db }, (t) => `${t} clip volume`);
  };
  const step = (by: number) => {
    const base = Number.isFinite(n) ? n : (clip?.gain_db ?? 0);
    setDraft(dbField(Math.min(GAIN[1], Math.max(GAIN[0], Math.round((base + by) * 10) / 10))));
  };
  return (
    <>
      {createPortal(<span ref={setAnchor} aria-hidden className="fixed size-px pointer-events-none" style={{ left: open.at.x, top: open.at.y - 4 }} />, document.body)}
      {anchor && (
        <Popover anchor={anchor} onClose={close} data-testid="volume-popover" aria-label="Clip volume" className="w-[220px]">
          <div className="px-2 py-1.5 flex items-center gap-2 text-sm text-fg">
            <label htmlFor="clip-volume" className="flex-1">
              Clip volume
            </label>
            <input
              ref={input}
              id="clip-volume"
              data-testid="clip-volume"
              type="text"
              inputMode="decimal"
              value={draft}
              autoFocus
              aria-invalid={!ok}
              title="↑ ↓ 0.5 dB · Shift with them 3 dB · Return sets it"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                // Its keys are its own: never the timeline's.
                e.stopPropagation();
                if (e.key === "Enter") go();
                else if (e.key === "Escape") close();
                else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
                  e.preventDefault();
                  step((e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 3 : 0.5));
                }
              }}
              className={clsx("w-16 h-ctl px-2 rounded-sm bg-raised border text-sm text-fg text-right tabular-nums outline-none", ok ? "border-line-strong focus:border-accent" : "border-danger")}
            />
            <span className="text-fg-muted">dB</span>
          </div>
          {!ok && <div role="alert" className="px-2 pb-1.5 text-xs text-danger">From −48 to +12 dB</div>}
        </Popover>
      )}
    </>
  );
}

import clsx from "clsx";
import { Plus, ZoomIn, ZoomOut } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { send } from "../lib/clips";
import { barAt, bars as barList, findLane, graphApi, lockReason, moduleName, position, type GraphState, type DrumKit, type DrumRow, type Meter, type TimelineView } from "../lib/graph";
import { clean, GRID_VIEW, useGrid } from "../lib/grid";
import { ROLL_RULER_H } from "../lib/layout";
import { chord, mod } from "../lib/native";
import { GRAPHITE, trackColors, type TrackColor } from "../lib/palette";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { DockSlot } from "./Dock";
import { undoAction } from "./GraphCanvas";
import GridChip from "./GridChip";
import { arrived, refreshed } from "./Tracks";
import { PlayLine, Ruler } from "./NoteView";
import { Button, EmptyState, keepFocus, MenuRow, Popover } from "./ui";

// The beat grid (KIT-10 to KIT-15): a drum kit's Drum tracks as rows of steps, as FL Studio's step
// sequencer has them. Each row is its track's own notes lane, so the timeline and Notes show the
// same notes. The grid writes them only through the server's `hits` gesture (KIT-12): one undo step
// for each click, drag along a row, or velocity drag. Where a hit goes and at what pitch is the
// server's to say; the grid shows what it asked for until the song shows it.

/** A row is 28 px (taste.md §2.2's control height); its cells leave 3 px above and below. */
const ROW_H = 28;
/** The row names' column. */
const NAMES_W = 160;
const VEL_H = 56;
/** The velocity lane's bars stop this far from its edges. */
const VEL_PAD = 8;
/**
 * Two places in beats this close or closer are one: the lane format writes beats to a millionth
 * (KIT-11). The editor's `SAME_PLACE` (`crates/editor/src/clips.rs`) is the same, so the grid and
 * the `hits` gesture agree on which step a hit starts in.
 */
const SAME_PLACE = 1e-6;
/** How far place `x` is after place `y`, in whole `SAME_PLACE`s (before: negative). */
const apart = (x: number, y: number) => Math.round((x - y) / SAME_PLACE);
/** Whether `x` and `y` are one place. */
const same = (x: number, y: number) => Math.abs(apart(x, y)) <= 1;

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** A note a row's lane plays, cut to its clip (KIT-11): where it starts, how long it plays, how hard. */
interface Hit { start: number; length: number; velocity: number }

/** A step: from one line of the grid to the next, cut at its bar's end (KIT-11). `odd`: in an odd beat of its bar, shaded. */
interface Step { start: number; end: number; odd: boolean }

/** What a gesture asked of steps of a row, shown until the song shows it: on, off, or a velocity. */
interface Asked { track: string; at: number[]; to: "on" | "off" | { velocity: number }; revision: number }

/** The notes row `row`'s lane plays, by start. */
function hitsOf(timeline: TimelineView, row: DrumRow): Hit[] {
  const lane = findLane(timeline, row.track, row.lane);
  if (lane?.kind !== "notes") return [];
  const out: Hit[] = [];
  for (const c of lane.clips) {
    for (const n of c.notes) {
      const s = Math.max(n.start, 0), e = Math.min(n.start + n.length, c.length);
      if (e > s) out.push({ start: c.start + s, length: e - s, velocity: n.velocity });
    }
  }
  return out.sort((a, b) => a.start - b.start);
}

/** The steps of the bars before `end`, `step` beats each, starting again at each bar line. */
function stepsTo(meter: Meter[], end: number, step: number): Step[] {
  const out: Step[] = [];
  for (const b of barList(meter, end)) {
    const barEnd = b.start + b.length;
    for (let k = 0; apart(barEnd, b.start + k * step) > 1; k++) {
      const start = clean(b.start + k * step);
      out.push({ start, end: Math.min(clean(b.start + (k + 1) * step), barEnd), odd: Math.floor(k * step + SAME_PLACE) % 2 === 1 });
    }
  }
  return out;
}

/** Each step's hits, by its start: the hits that start in it. */
function byStep(steps: Step[], hits: Hit[]): Map<number, Hit[]> {
  const out = new Map<number, Hit[]>();
  for (const h of hits) {
    const s = stepAt(steps, h.start);
    if (s && apart(h.start, s.end) < -1) out.set(s.start, [...(out.get(s.start) ?? []), h]);
  }
  return out;
}

/** The step that holds beat `beat`: the last that starts at or before it (a millionth before it is at it). */
function stepAt(steps: Step[], beat: number): Step | null {
  let lo = 0, hi = steps.length - 1, found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (apart(beat, steps[mid].start) >= -1) [found, lo] = [mid, mid + 1];
    else hi = mid - 1;
  }
  return found < 0 ? null : steps[found];
}

/** A hit's colour: the track's bar, mixed toward its ink the harder it is played (as the roll's notes are). */
const fill = (tone: TrackColor, velocity: number) => `color-mix(in oklab, ${tone.ink} ${Math.round(clamp(velocity, 0, 1) * 45)}%, ${tone.bar})`;

/** What an off-grid hit's mark says (KIT-11): where it starts, by bar, beat and sixteenth as the transport says it, and how long it is. */
function offGridWords(meter: Meter[], h: Hit) {
  const bar = barAt(meter, h.start);
  const sixteenths = (h.start - bar.start) * 4;
  const onOne = same(sixteenths / 4, Math.round(sixteenths) / 4);
  const at = onOne ? `at ${position(meter, bar.start + Math.round(sixteenths) / 4)}` : `a little after ${position(meter, h.start)}`;
  const n = Number(h.length.toFixed(3));
  return `It starts ${at} and is ${n} ${n > 1 ? "beats" : "beat"} long. The grid leaves it where it is; click its step to take it away.`;
}

/** `n` hits of track `name`, as an undo step says them: "a Kick hit", "3 Kick hits". */
const hitsSaid = (n: number, name: string) => (n === 1 ? `a ${name} hit` : `${n} ${name} hits`);

/** A sound's name as the grid says it: `snare` → Snare. */
const said = (sound: string) => sound.charAt(0).toUpperCase() + sound.slice(1);

/**
 * The sounds a Drum plays, as its menu offers them: from any Drum's sound knob in the view (they
 * share one descriptor). Empty when no Drum shows one.
 */
function drumSounds(graph: GraphState): string[] {
  for (const m of graph.view.modules) {
    for (const n of m.nodes) {
      const k = n.component === "@m2/drums:drum" ? n.knobs.find((x) => x.slot === "drum") : undefined;
      if (k?.options) return k.options.map((o) => String(o.value));
    }
  }
  return [];
}

/** The beat grid of `kit`, with `module` (a row's track, or the bus) selected. */
export default function BeatView({ graph, kit, module }: { graph: GraphState; kit: DrumKit; module: string }) {
  const toast = useStore((s) => s.toast);
  const { timeline } = graph;
  const { meter } = timeline;
  const grid = useGrid((s) => s.beat);
  const step = grid.step;
  const [ppb, setPpb] = useState(88);
  const [visible, setVisible] = useState(0);
  // How far it is scrolled: only the steps near what shows are drawn.
  const [left, setLeft] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const anchor = useRef<{ beat: number; x: number } | null>(null);
  // What gestures asked, shown until the song shows it; and a drag along a row, or of a velocity, under
  // way (in refs, as pointer events come faster than renders; `redraw` shows them).
  const [asked, setAsked] = useState<Asked[]>([]);
  const drag = useRef<(Asked & { pointer: number }) | null>(null);
  const vel = useRef<{ pointer: number; at: number; velocity: number } | null>(null);
  const [, setDrawn] = useState(0);
  const redraw = () => setDrawn((n) => n + 1);
  const latest = useRef(graph);
  latest.current = graph;
  const blocked = useStore((s) => lockReason(graph, s.connected));
  const locked = graph.view.draft_errors.length > 0 ? "Read only until the code compiles" : blocked;

  // The velocity lane's row: the selected track's, or the kit's first while its bus is selected (KIT-13).
  const selected = kit.rows.find((r) => r.track === module) ?? kit.rows[0];
  const name = (track: string) => moduleName(graph.view.modules.find((m) => m.key === track) ?? { key: track, label: timeline.tracks.find((t) => t.module === track)?.label });

  // The song's bars and one bar more (KIT-10), at least as wide as the view.
  const last = barAt(meter, Math.max(0, timeline.end - SAME_PLACE));
  let end = last.start + last.length;
  end += barAt(meter, end).length;
  while (end * ppb < visible) end += barAt(meter, end).length;
  const steps = stepsTo(meter, end, step);
  const width = end * ppb;
  // What shows, a bar's room either side: the steps drawn, and the hits.
  const from = left / ppb - 4, to = (left + visible) / ppb + 4;
  const near = (start: number, length = 0) => start + length > from && start < to;
  const shown = steps.filter((s) => near(s.start, s.end - s.start));

  // Once the song shows what a gesture asked, the grid shows the song.
  useEffect(() => {
    if (asked.some((a) => a.revision <= graph.view.revision)) setAsked((all) => all.filter((a) => a.revision > graph.view.revision));
  }, [graph.view.revision, asked]);

  useEffect(() => {
    const el = scroller.current!;
    const observer = new ResizeObserver(() => setVisible(el.clientWidth - NAMES_W));
    observer.observe(el);
    // ⌘/Ctrl + wheel zooms time around the pointer, as Notes does; plain wheels scroll.
    const wheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const x = e.clientX - el.getBoundingClientRect().left - NAMES_W;
      setPpb((p) => {
        anchor.current = { beat: (el.scrollLeft + x) / p, x };
        return clamp(p * Math.exp(-e.deltaY * 0.004), 16, 400);
      });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      observer.disconnect();
      el.removeEventListener("wheel", wheel);
    };
  }, []);
  useLayoutEffect(() => {
    const el = scroller.current, a = anchor.current;
    anchor.current = null;
    if (el && a) el.scrollLeft = Math.max(0, a.beat * ppb - a.x);
  }, [ppb]);
  const zoom = (by: number) => {
    const el = scroller.current;
    if (el) anchor.current = { beat: (el.scrollLeft + el.clientWidth / 2 - NAMES_W) / ppb, x: el.clientWidth / 2 - NAMES_W };
    setPpb((p) => clamp(p * by, 16, 400));
  };

  // ---------------------------------------------------------------- writing

  /** The grid's gestures on their way, one after another, so they land in the order made. */
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  /**
   * Send what `a` asks of row `row` as one `hits` gesture: one undo step, named `step`. It goes once
   * the gestures before it have landed: each was decided from what the grid showed, theirs included.
   */
  const write = (row: DrumRow, a: Omit<Asked, "revision">, step: string) => {
    const mine: Asked = { ...a, revision: Infinity };
    setAsked((all) => [...all, mine]);
    const size = grid.step;
    queue.current = queue.current
      .then(() => send(latest.current, { op: "hits", module: row.track, lane: row.lane, at: a.at, step: size, to: a.to }, { name: step }))
      .then((done) => setAsked((all) => all.map((x) => (x === mine ? { ...x, revision: done.revision } : x))))
      .catch((error) => {
        setAsked((all) => all.filter((x) => x !== mine));
        toast(message(error));
      });
  };

  /** Whether step `s` of row `track` is on: as the newest gesture on it asked, else whether a hit starts in it (`hits`, by step). */
  const isOn = (track: string, hits: Map<number, Hit[]>, s: Step) => {
    const asking = [...asked, ...(drag.current ? [drag.current] : [])].filter((a) => a.track === track && (a.to === "on" || a.to === "off"));
    const last = asking.findLast((a) => a.at.some((x) => same(x, s.start)));
    return last ? last.to === "on" : hits.has(s.start);
  };

  // ---------------------------------------------------------------- the rows

  const rowsHits = kit.rows.map((r) => hitsOf(timeline, r));
  const rowsSteps = rowsHits.map((h) => byStep(steps, h));

  const stepOf = (e: React.PointerEvent<HTMLElement>) => {
    const x = e.clientX - e.currentTarget.getBoundingClientRect().left;
    return stepAt(steps, x / ppb);
  };

  const onRowDown = (e: React.PointerEvent<HTMLDivElement>, row: DrumRow, hits: Map<number, Hit[]>) => {
    if (e.button !== 0 || locked || drag.current) return;
    const s = stepOf(e);
    if (!s) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    // The row worked on is the selected one: the velocity lane shows it.
    if (row.track !== module) useShell.getState().select(row.track);
    // A click turns its step on, or off when it is on; a drag along the row does the same to each step it crosses.
    drag.current = { track: row.track, at: [s.start], to: isOn(row.track, hits, s) ? "off" : "on", revision: Infinity, pointer: e.pointerId };
    redraw();
  };
  const onRowMove = (e: React.PointerEvent<HTMLDivElement>, row: DrumRow, hits: Map<number, Hit[]>) => {
    const d = drag.current;
    if (!d || d.pointer !== e.pointerId || d.track !== row.track) return;
    const s = stepOf(e);
    if (!s || d.at.some((x) => same(x, s.start))) return;
    // Only the steps it changes: on paints the empty ones, off clears the full ones.
    if (isOn(row.track, hits, s) === (d.to === "on")) return;
    d.at.push(s.start);
    redraw();
  };
  const onRowUp = (e: React.PointerEvent<HTMLDivElement>, row: DrumRow, cancelled = false) => {
    const d = drag.current;
    if (!d || d.pointer !== e.pointerId) return;
    drag.current = null;
    redraw();
    if (cancelled) return;
    const at = [...d.at].sort((a, b) => a - b);
    write(row, { track: row.track, at, to: d.to }, `${d.to === "on" ? "adding" : "removing"} ${hitsSaid(at.length, name(row.track))}`);
  };

  // ---------------------------------------------------------------- velocity

  const velocityAt = (clientY: number, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return Math.round(clamp(1 - (clientY - r.top - VEL_PAD) / (r.height - 2 * VEL_PAD), 0.01, 1) * 100) / 100;
  };
  const selectedHits = rowsHits[kit.rows.indexOf(selected)] ?? [];
  const onVelDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const bar = (e.target as HTMLElement).closest<HTMLElement>("[data-velocity]");
    if (e.button !== 0 || !bar || locked || vel.current) return;
    const s = stepAt(steps, Number(bar.dataset.start));
    if (!s) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    vel.current = { pointer: e.pointerId, at: s.start, velocity: velocityAt(e.clientY, e.currentTarget) };
    redraw();
  };
  const onVelMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (vel.current?.pointer !== e.pointerId) return;
    vel.current.velocity = velocityAt(e.clientY, e.currentTarget);
    redraw();
  };
  const onVelUp = (e: React.PointerEvent<HTMLDivElement>, cancelled = false) => {
    const v = vel.current;
    if (v?.pointer !== e.pointerId) return;
    vel.current = null;
    redraw();
    if (!cancelled) write(selected, { track: selected.track, at: [v.at], to: { velocity: v.velocity } }, `${name(selected.track)} velocity`);
  };
  /** The velocity a hit shows: dragged, asked for, or its own. */
  const velocityOf = (h: Hit) => {
    const s = stepAt(steps, h.start);
    const v = vel.current;
    if (s && v && same(v.at, s.start)) return v.velocity;
    const a = [...asked].reverse().find((x) => x.track === selected.track && typeof x.to === "object" && s && x.at.some((y) => same(y, s.start)));
    return a && typeof a.to === "object" ? a.to.velocity : h.velocity;
  };

  const empty = rowsHits.every((h) => h.length === 0) && asked.length === 0 && !drag.current;
  const tone = (track: string) => trackColors(timeline, track) ?? GRAPHITE;

  return (
    <div data-testid="beat" {...{ [GRID_VIEW]: "beat" }} className="flex-1 min-h-0 flex flex-col">
      <DockSlot at="title">
        <span data-testid="beat-kit">{kit.label} kit</span>
      </DockSlot>
      <DockSlot at="tools">
        <GridChip view="beat" />
        <AddDrum graph={graph} kit={kit} locked={locked} />
        <div className="shrink-0 flex items-center">
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(0.8)} title={`Zoom out (${mod.cmd} + scroll)`} aria-label="Zoom out" className="w-ctl-sm !px-0">
            <ZoomOut size={16} />
          </Button>
          <Button size="sm" onMouseDown={keepFocus} onClick={() => zoom(1.25)} title={`Zoom in (${mod.cmd} + scroll)`} aria-label="Zoom in" className="w-ctl-sm !px-0">
            <ZoomIn size={16} />
          </Button>
        </div>
      </DockSlot>

      <div className="flex-1 min-h-0 relative">
        <div
          ref={scroller}
          data-testid="beat-scroller"
          className="absolute inset-0 overflow-auto"
          onScroll={(e) => {
            if (strip.current) strip.current.style.transform = `translateX(${-e.currentTarget.scrollLeft}px)`;
            setLeft(e.currentTarget.scrollLeft);
          }}
        >
          <div className="relative" style={{ width: NAMES_W + width, height: ROLL_RULER_H + kit.rows.length * ROW_H }}>
            <div className="sticky top-0 z-30 flex" style={{ height: ROLL_RULER_H }}>
              <div className="sticky left-0 z-40 shrink-0 bg-chrome border-r border-b border-line" style={{ width: NAMES_W }} />
              <Ruler width={width} ppb={ppb} meter={meter} start={0} />
            </div>
            {/* A line at each bar, behind the steps. */}
            <div className="absolute bottom-0 pointer-events-none" style={{ top: ROLL_RULER_H, left: NAMES_W, width }}>
              {barList(meter, end).map((b) => (
                <div key={b.bar} className="absolute inset-y-0 w-px bg-line-strong" style={{ left: b.start * ppb }} />
              ))}
            </div>
            {kit.rows.map((row, i) => {
              const hits = rowsSteps[i];
              const t = tone(row.track);
              const on = selected.track === row.track;
              return (
                <div key={row.track} className="flex" style={{ height: ROW_H }}>
                  <button
                    data-testid="beat-row-name"
                    data-row={row.track}
                    aria-pressed={on}
                    onMouseDown={keepFocus}
                    onClick={() => useShell.getState().select(row.track)}
                    title={`${name(row.track)}: click to select it${row.sound ? "" : ". Its sound is set by its code"}`}
                    className={clsx(
                      "sticky left-0 z-20 shrink-0 flex items-center gap-2 pl-3 pr-2 text-left border-r border-b border-line cursor-pointer",
                      on ? "bg-hover" : "bg-chrome hover:bg-hover",
                    )}
                    style={{ width: NAMES_W }}
                  >
                    <span className="size-2 shrink-0 rounded-xs" style={{ background: t.bar }} />
                    <span className={clsx("min-w-0 flex-1 truncate text-sm", on ? "font-semibold text-fg-strong" : "font-medium text-fg")}>{name(row.track)}</span>
                    {/* Its sound, where its name doesn't say it already (taste.md principle 2). */}
                    {row.sound?.toLowerCase() !== name(row.track).toLowerCase() && (
                      <span data-testid="beat-sound" className="shrink-0 text-xs text-fg-muted">
                        {row.sound ? said(row.sound) : "In code"}
                      </span>
                    )}
                  </button>
                  <Cells
                    row={row}
                    meter={meter}
                    steps={steps}
                    shown={shown}
                    hits={rowsHits[i].filter((h) => near(h.start, h.length))}
                    inSteps={hits}
                    ppb={ppb}
                    tone={t}
                    width={width}
                    locked={locked}
                    isOn={(s) => isOn(row.track, hits, s)}
                    onDown={(e) => onRowDown(e, row, hits)}
                    onMove={(e) => onRowMove(e, row, hits)}
                    onUp={(e, c) => onRowUp(e, row, c)}
                  />
                </div>
              );
            })}
            <div className="absolute bottom-0 pointer-events-none" style={{ top: ROLL_RULER_H, left: NAMES_W, width }}>
              <PlayLine start={0} ppb={ppb} width={width} />
            </div>
          </div>
        </div>
        {empty && (
          <EmptyState className="absolute left-1/2 bottom-3 -translate-x-1/2 pointer-events-none whitespace-nowrap">
            Click a step to add a hit · drag along a row to add several
          </EmptyState>
        )}
      </div>

      <div data-testid="beat-velocity" data-row={selected.track} className="shrink-0 flex border-t border-line bg-chrome" style={{ height: VEL_H }}>
        <div className="shrink-0 flex flex-col justify-center gap-0.5 pl-3 border-r border-line" style={{ width: NAMES_W }}>
          <span className="text-xs font-medium text-fg-muted">Velocity</span>
          <span className="text-xs text-fg-faint truncate">{name(selected.track)}</span>
        </div>
        <div className="relative flex-1 min-w-0 overflow-hidden">
          <div
            ref={strip}
            className="absolute inset-y-0 left-0 touch-none"
            style={{ width, transform: `translateX(${-(scroller.current?.scrollLeft ?? 0)}px)` }}
            onPointerDown={onVelDown}
            onPointerMove={onVelMove}
            onPointerUp={(e) => onVelUp(e)}
            onPointerCancel={(e) => onVelUp(e, true)}
            onLostPointerCapture={(e) => onVelUp(e, true)}
          >
            <div className="absolute inset-x-0 border-t border-dashed border-line pointer-events-none" style={{ top: VEL_PAD }} />
            <div className="absolute inset-x-0 border-t border-dashed border-line pointer-events-none" style={{ top: VEL_H / 2 }} />
            {selectedHits.filter((h) => near(h.start)).map((h, i) => {
              const v = velocityOf(h);
              const height = (VEL_H - 2 * VEL_PAD) * clamp(v, 0, 1);
              const t = tone(selected.track);
              return (
                <div
                  key={i}
                  data-velocity={v}
                  data-start={h.start}
                  title={`${name(selected.track)}: velocity ${Math.round(v * 100)}% (drag up or down)`}
                  className={clsx("absolute inset-y-0 w-3 -ml-[5px] group", !locked && "cursor-ns-resize")}
                  style={{ left: h.start * ppb }}
                >
                  <div className="absolute left-[5px] w-[2px] rounded-full" style={{ bottom: VEL_PAD, height, background: locked ? "var(--color-fg-off)" : t.ink }} />
                  <div
                    className={clsx("absolute left-[2px] size-2 rounded-full ring-1 ring-(--note-ink)", !locked && "group-hover:scale-125")}
                    style={{ bottom: VEL_PAD + height - 4, background: locked ? "var(--color-line-strong)" : fill(t, v), "--note-ink": locked ? "var(--color-fg-off)" : t.ink } as React.CSSProperties}
                  />
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * One row's steps: a cell each, shaded by beat; an on step filled in the track's colour by its
 * velocity; a hit off the grid (KIT-11) a mark where it starts, as long as it plays, over its step's
 * tint. Only the steps near what shows are drawn; `steps` are all of them, to place a hit in its own.
 */
function Cells({
  row,
  meter,
  steps,
  shown,
  hits,
  inSteps,
  ppb,
  tone,
  width,
  locked,
  isOn,
  onDown,
  onMove,
  onUp,
}: {
  row: DrumRow;
  meter: Meter[];
  steps: Step[];
  /** The steps drawn: those near what shows. */
  shown: Step[];
  /** The hits near what shows. */
  hits: Hit[];
  /** The hits that start in each step, by its start. */
  inSteps: Map<number, Hit[]>;
  ppb: number;
  tone: TrackColor;
  width: number;
  locked: string | null;
  isOn: (s: Step) => boolean;
  onDown: (e: React.PointerEvent<HTMLDivElement>) => void;
  onMove: (e: React.PointerEvent<HTMLDivElement>) => void;
  onUp: (e: React.PointerEvent<HTMLDivElement>, cancelled?: boolean) => void;
}) {
  return (
    <div
      data-testid="beat-row"
      data-row={row.track}
      data-lane={row.lane}
      className={clsx("relative shrink-0 touch-none border-b border-line", locked ? "cursor-default" : "cursor-pointer")}
      style={{ width, height: ROW_H }}
      title={locked ?? `Click a step to add a hit, or take one away · drag along the row to do several · ${chord("1")} and ${chord("2")} make the steps shorter or longer`}
      onPointerDown={onDown}
      onPointerMove={onMove}
      onPointerUp={(e) => onUp(e)}
      onPointerCancel={(e) => onUp(e, true)}
      onLostPointerCapture={(e) => onUp(e, true)}
    >
      {shown.map((s) => {
        const on = isOn(s);
        const inStep = inSteps.get(s.start) ?? [];
        // On the grid: a hit at the step's start, however long (KIT-11). One after it shows where it is.
        const exact = inStep.find((h) => same(h.start, s.start));
        const filled = on && (exact || inStep.length === 0);
        return (
          <div
            key={s.start}
            data-step={s.start}
            data-on={on || undefined}
            className="absolute rounded-xs"
            style={{
              left: s.start * ppb + 1,
              top: 3,
              width: Math.max((s.end - s.start) * ppb - 2, 1),
              height: ROW_H - 7,
              background: filled ? (locked ? "var(--color-line-strong)" : fill(tone, exact?.velocity ?? 0.8)) : on ? tone.tint : s.odd ? "var(--color-hover)" : "var(--color-chrome)",
            }}
          />
        );
      })}
      {hits
        .filter((h) => {
          const s = stepAt(steps, h.start);
          return !s || !same(h.start, s.start);
        })
        .map((h, i) => (
          <div
            key={`off${i}`}
            data-off-grid={h.start}
            title={offGridWords(meter, h)}
            className="absolute rounded-xs pointer-events-none"
            style={{ left: h.start * ppb, top: ROW_H / 2 - 5, width: Math.max(h.length * ppb, 3), height: 8, background: locked ? "var(--color-fg-off)" : tone.ink }}
          />
        ))}
    </div>
  );
}

/** + Drum (KIT-15): a menu of the Drum's sounds; the one picked is a new row of the kit, as one undo step. */
function AddDrum({ graph, kit, locked }: { graph: GraphState; kit: DrumKit; locked: string | null }) {
  const toast = useStore((s) => s.toast);
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  const sounds = drumSounds(graph);
  const add = (sound: string) => {
    setMenu(null);
    graphApi
      .kitDrum(graph.session, kit.bus, sound)
      .then(async (r) => {
        // Selected once the song shows it, as + Track's new tracks are: its row is the one worked on.
        await refreshed();
        arrived(r.module, graph.instruments?.find((i) => i.component === "@m2/drums:drum")?.lane);
        toast(`Added ${r.label} to ${kit.label}`, undoAction(graph.session, r.step));
      })
      .catch((error) => toast(message(error)));
  };
  return (
    <>
      <Button
        size="sm"
        data-testid="add-drum"
        disabled={!!locked || sounds.length === 0}
        aria-haspopup="menu"
        aria-expanded={!!menu}
        onMouseDown={keepFocus}
        onClick={(e) => setMenu(menu ? null : e.currentTarget)}
        title={locked ?? `Add a drum to ${kit.label}: a track of its own, going to the ${kit.label} bus`}
        className="shrink-0"
      >
        <Plus size={14} />
        Drum
      </Button>
      {menu && (
        <Popover anchor={menu} onClose={() => setMenu(null)} align="end" role="menu" aria-label="Add a drum" data-testid="add-drum-menu" className="w-[160px]">
          {sounds.map((s) => (
            <MenuRow key={s} data-sound={s} onClick={() => add(s)}>
              {said(s)}
            </MenuRow>
          ))}
        </Popover>
      )}
    </>
  );
}

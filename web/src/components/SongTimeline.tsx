import clsx from "clsx";
import { ArrowRightFromLine, ChevronRight, Plus, ZoomIn, ZoomOut } from "lucide-react";
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import {
  barAt,
  bars,
  clipIndex,
  clipRef,
  findLane,
  formatValue,
  hasOwnId,
  laneTitle,
  laneValue,
  meterSpans,
  moduleName,
  position,
  sameClip,
  type AudioClip,
  type ClipRef,
  type ControlClip,
  type Diagnostic,
  type GraphState,
  type Meter,
  type NoteClip,
  type TimelineLane,
  type TimelineTrack,
  type TimelineView,
} from "../lib/graph";
import { beatNow, onFrame } from "../lib/clock";
import { LANE_MADE, TRACK_MADE } from "../lib/events";
import { useHeard } from "../lib/graph8b";
import { after, before, markerAt, markersOf, useMarkerEdit } from "../lib/markers";
import { heightsNow, laneHeight, setHeights, snapHeight, stepHeight, LANE_HS, TRACK_H, trackHeight, useHeights } from "../lib/heights";
import { LANE_H, ROW_H, RULER_H } from "../lib/layout";
import { GRAPHITE, trackColors, type TrackColor } from "../lib/palette";
import { canvas, useShell } from "../lib/shell";
import {
  busy,
  copiedOf,
  isPicked,
  pickedIn,
  playhead,
  refIn,
  send,
  serverRef,
  useClipboard,
  useClipSelection,
  type Done,
  type Gesture,
  type LaneAddr,
  type ServerRef,
} from "../lib/clips";
import { clean, gridOp, gridTarget, pickName, skipsSnap, snapSong, stepOf, useGrid, type Grid as GridSetting } from "../lib/grid";
import { added, bounded, hit, line, moved, painted, pickedIn as pointIn, usePoints, type Point } from "../lib/points";
import { begin as leaseDrag, type LaneLease } from "../lib/laneLease";
import { laneScale, type LaneScale } from "../lib/laneScale";
import { hide as hideLane, useAllHidden, useHidden } from "../lib/hiddenLanes";
import { HIDDEN_ROW_H, HiddenRow } from "./HiddenLanes";
import { chord, keyName, mod, opt } from "../lib/native";
import { followNow, roundZoom, setFollow, setZoom, stepped, useFollow, useZoom, ZOOM, zoomNow } from "../lib/zoom";
import { useStore } from "../lib/store";
import { addEffectTo, dragsEffect, EFFECT_DRAG } from "./Browser";
import { addTrack, END, INSTRUMENT_DRAG, openInstruments, openTrackMenu, startReorder, useReorder } from "./Tracks";
import { MarkPopover, placeOf, RulerMenu } from "./SongBar";
import { RulerMarkers } from "./Markers";
import { Hatch, VLines } from "./draw";
import GridChip from "./GridChip";
import PointMenu, { unautomate, type MenuItem } from "./ClipMenu";
import TrackHeader from "./TrackHeader";
import { Button, EmptyState, keepFocus, Popover } from "./ui";
import { audioLabel, audioRows, VolumePopover, Wave, type WaveEdit } from "./Wave";
import { edgeLimits, fileSeconds, respanned } from "../lib/peaks";
import Take from "./Take";
import { exportMidi } from "../lib/midiFiles";

/** A clip sits this far inside its row, inside its 1px border; its drawing this far inside that. */
const CLIP_INSET = 4;
const BORDER = 1;
const BODY_TOP = 4;
const BODY_BOTTOM = 4;

const beats = (n: number) => Number(n.toFixed(3));

const span = (c: { start: number; length: number }) => `beats ${beats(c.start)}–${beats(c.start + c.length)}`;
/** The clips drawn here are a track's muted in the song: hatched (taste.md §2.9). */
const Hatched = createContext(false);

/** A clip the timeline made, to select (and open) once the graph shows the write that made it. */
interface Made { revision: number; clip: ClipRef }

/**
 * Editing clips in place (design-8b §7): select them, drag them (to another lane of their kind too;
 * ⌥ copies), drag an edge, make a notes clip, delete. Each edit is one gesture, one undo step.
 * Stable across renders.
 */
interface ClipEdits {
  /** A press on a clip: on its body or (`data-edge`) one of its edges. */
  down: (clip: ClipRef, e: React.PointerEvent<HTMLElement>) => void;
  /** A double-click on empty space in a notes or automation lane (module and lane keys), at `beat`. */
  create: (module: string, lane: string, beat: number) => void;
  unpick: (module: string) => void;
  /** A press in a lane: on its empty space (not on a clip), the start of a click that may place play's start. */
  press: (e: React.PointerEvent<HTMLElement> | null) => void;
  /**
   * Let go in a lane, at `beat`: after a press on its empty space that didn't move (a click), play
   * starts there (taste.md §5.1). Only while stopped, as the ruler's click moves it: then the
   * playhead goes there. While playing it does nothing, so a click to deselect never makes the song
   * jump, and no start point moves unseen (nothing marks one).
   */
  place: (e: React.PointerEvent<HTMLElement>, beat: number) => void;
  /** Delete the selected clips. */
  remove: () => void;
  /** Send the nudges waiting for the timeline to show the last edit. */
  flush: () => void;
  /** ⌘D: copies of the selected clips right after them (a marquee's: after its bars). False with none selected. */
  duplicate: () => boolean;
  /** Repeat…: `times` copies of the selected clips, one after another (Q12). */
  repeat: (times: number) => void;
  /** ⌘E: the selected clips that cross the playhead, else every clip under it on the selected track. */
  split: () => void;
  /** ⌘J: the selected notes clips of one lane, as one. */
  join: () => void;
  /** Q: every note of the selected clips onto the roll's grid. */
  quantize: () => void;
  /** ← and →: the selected clips by the grid, or (`bar`) by a bar. */
  nudge: (dir: -1 | 1, bar: boolean) => void;
  /** ⌘C, or (`cut`) ⌘X: the selected clips to the clipboard. */
  copy: (cut: boolean) => void;
  /** ⌘V: the clipboard's clips at `beat` (the playhead), onto lane `to` (the selected track's) when it takes them. */
  paste: (beat?: number, to?: LaneAddr) => void;
  /** Loop over the selected clips (the clip menu's Loop this clip). */
  loop: () => void;
  /** A right-click on a clip (`clip`), a lane's empty space at `beat`, or (`header`) an automation lane's header: its menu. */
  menu: (e: React.MouseEvent, module: string, lane: string, beat: number, clip?: ClipRef | "header") => void;
  /** Remove automation: the lane's value at the playhead goes back into the code, its file goes. */
  unautomate: (module: string, lane: string) => void;
  /** ⌫ with an automation point selected: it goes. False with none. */
  unpoint: () => boolean;
}

/** A menu open on a clip, a lane's empty space or an automation lane's header: where the pointer was, the lane and the beat. */
interface MenuAt {
  x: number;
  y: number;
  module: string;
  lane: string;
  beat: number;
  on: "clip" | "lane" | "header";
}

/** A marquee being dragged (design-8b §7.4): its box in the content's pixels, and the bars it covers. */
interface Marquee {
  left: number;
  top: number;
  width: number;
  height: number;
  /** Where the pointer is, for the bubble ("Bars 1–8"). */
  at: { x: number; y: number };
  text: string;
}

/** The shortest clip: 1/64 of a beat, as the server has it. */
const SHORTEST = 1 / 64;

/**
 * Clips being dragged: moved by `by` beats (onto lane `to`; copied while ⌥ is held), or one clip's
 * left or right edge, to `start` and `length`. Each clip as drawn when the drag began.
 */
interface Drag {
  kind: "move" | "left" | "right";
  clips: { ref: ClipRef; clip: AnyClip; lane: TimelineLane; color: TrackColor }[];
  by: number;
  to: LaneAddr | null;
  copy: boolean;
  /** Why the lane under the pointer can't take the clips: the bubble says it, and letting go does nothing. */
  refused: string | null;
  start: number;
  length: number;
  /** The pointer, for the value bubble, and what the bubble says: where the clip lands ("9.3.1"). */
  at: { x: number; y: number };
  text: string;
  /** Where the dashed guide crosses the lanes: the new start, or the edge. */
  guide: number;
  /** Let go: the revision that shows the edit (Infinity until the server answers). */
  landed?: number;
}

type AnyClip = NoteClip | ControlClip | AudioClip;

/** The lane under a point, by its row's `data-lane` (`module.lane`), or null. */
function laneUnder(x: number, y: number): LaneAddr | null {
  const id = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-lane]")?.dataset.lane;
  const dot = id ? id.indexOf(".") : -1;
  return id && dot > 0 ? { module: id.slice(0, dot), lane: id.slice(dot + 1) } : null;
}

/** Each unit's name, for "Only Hz automation". */
const UNITS: Record<string, string> = { hz: "Hz", db: "dB", seconds: "seconds", beats: "beats", semitones: "semitones", unit01: "0–1", bipolar: "−1–1", gain: "gain" };

/**
 * Why lane `to` can't take clips from lane `from` (design-8b §7.1): kinds never cross, and
 * automation crosses only within its unit. Null when it can.
 */
function refusal(from: TimelineLane, to: TimelineLane): string | null {
  if (from.kind !== to.kind) return { notes: "Notes clips only", audio: "Audio clips only", control: "Automation only" }[to.kind];
  if (from.kind === "control" && to.kind === "control" && (from.unit ?? null) !== (to.unit ?? null)) return `Only ${UNITS[to.unit ?? ""] ?? "plain-number"} automation`;
  return null;
}

/** A clip given a new start and length, what is inside it staying where it plays (a left edge dragged). */
function reshaped<C extends AnyClip>(c: C, start: number, length: number): C {
  const d = start - c.start;
  if ("notes" in c) return { ...c, start, length, notes: c.notes.map((n) => ({ ...n, start: n.start - d })) };
  if ("points" in c) return { ...c, start, length, points: c.points.map(([b, v]) => [b - d, v] as [number, number]) };
  // An audio clip's offset moves as the server moves it, so its waveform stays where it plays (A3).
  return respanned(c, useStore.getState().graph?.timeline.tempo ?? [], start, length) as C;
}

/** The value bubble at the pointer while clips are dragged (taste.md §2.14), as the song bar's. */
function DragBubble({ at, children }: { at: { x: number; y: number }; children: ReactNode }) {
  return createPortal(
    <div
      aria-hidden
      data-testid="clip-bubble"
      className="fixed z-50 pointer-events-none -translate-x-1/2 -translate-y-full px-1.5 py-0.5 rounded-sm bg-fg-strong/92 text-white text-xs tabular-nums whitespace-nowrap"
      style={{ left: at.x, top: at.y - 12 }}
    >
      {children}
    </div>,
    document.body,
  );
}

/** The part of each note that plays: cut to its clip, as playback cuts it. Notes wholly outside are silent. */
function audible(clip: NoteClip) {
  return clip.notes.flatMap((n) => {
    const start = Math.max(n.start, 0), end = Math.min(n.start + n.length, clip.length);
    return end > start ? [{ ...n, start, length: end - start }] : [];
  });
}

/** The tempo and time signature changes after the start, by beat. */
function changes(timeline: TimelineView) {
  const at = new Map<number, { beat: number; bpm?: number; meter?: string }>();
  const get = (beat: number) => at.get(beat) ?? at.set(beat, { beat }).get(beat)!;
  for (const t of timeline.tempo.slice(1)) get(t.beat).bpm = beats(t.bpm);
  for (const m of timeline.meter.slice(1)) get(m.beat).meter = `${m.beats}/${m.unit}`;
  return [...at.values()].sort((a, b) => a.beat - b.beat);
}

/** Where the timeline ends: a bar after the bar the last clip, or the playhead, is in. */
function room(meter: Meter[], end: number, beat: number) {
  // A clip ending on a bar line is in the bar before it.
  const last = barAt(meter, Math.max(end, beat, 1e-6) - 1e-6);
  const next = last.start + last.length;
  return next + barAt(meter, next).length;
}

/**
 * A track's rows (taste.md §3.4): its own row draws its primary lane, the first notes or audio
 * lane in code order; its other notes and audio lanes are full rows under it, always shown; its
 * automation lanes are 32px rows under those, folded until asked for (A).
 */
function rowsOf(track: TimelineTrack) {
  const primary = track.lanes.find((l) => l.kind !== "control") ?? null;
  return {
    primary,
    others: track.lanes.filter((l) => l !== primary && l.kind !== "control"),
    controls: track.lanes.filter((l) => l.kind === "control"),
  };
}

/** Draws the playhead on every frame from the transport clock: never eased between polls (TL-9). */
function usePlayhead(ref: React.RefObject<HTMLElement | null>, attr = false) {
  const ppb = useZoom();
  useEffect(
    () =>
      onFrame((beat) => {
        const el = ref.current;
        if (!el) return;
        el.style.transform = `translateX(${beat * ppb}px)`;
        if (attr) el.dataset.beat = String(beats(beat));
      }),
    [ref, attr, ppb],
  );
}

function PlayLine({ testid, className, style }: { testid?: string; className: string; style: React.CSSProperties }) {
  const ref = useRef<HTMLDivElement>(null);
  usePlayhead(ref, !!testid);
  return <div ref={ref} data-testid={testid} aria-hidden className={className} style={style} />;
}

/**
 * The song as the accepted revision plays it: a row per track, its lanes under it, under a ruler
 * that seeks; the Song's row pinned at the foot. Tracks, lanes and clips are named by their keys and
 * ids, never by their place (ID-11, FMT-12). What is selected is the shell's (§3.7).
 */
export default function SongTimeline({
  timeline,
  rows,
  song,
  revision,
  beat,
  playing,
  loopStart,
  loopEnd,
  looping,
  onLoop,
  issues,
  locked,
  waiting,
  headerW,
  onOpenClip,
  onSeek,
}: {
  timeline: TimelineView;
  /** The tracks, in order, and every other module with no lanes. */
  rows: TimelineTrack[];
  /** The Song's own row: the root module's lanes. */
  song: TimelineTrack;
  /** The revision `timeline` is from: what a lane write rewritten from it is based on. */
  revision: number;
  beat: number;
  playing: boolean;
  /** The loop's range in beats (null: none set), and whether playback loops there. */
  loopStart: number | null;
  loopEnd: number | null;
  looping: boolean;
  /** A range dragged across the ruler, in whole beats: loop there. */
  onLoop: (start: number, end: number) => void;
  /** What needs attention, by module key: marked on the track. */
  issues: ReadonlyMap<string, Diagnostic[]>;
  /** Why clips can't be edited now, if they can't. */
  locked: string | null;
  /** The code has never compiled (WRITE-20): the tracks and clips are what `data/` holds, greyed. */
  waiting: boolean;
  headerW: number;
  /** Double-clicking a notes clip opens it in Notes. */
  onOpenClip: (clip: ClipRef) => void;
  onSeek: (beat: number) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(0);
  useEffect(() => {
    const el = scroller.current!;
    const observer = new ResizeObserver(() => setVisible(Math.floor(el.clientWidth - headerW)));
    observer.observe(el);
    return () => observer.disconnect();
  }, [headerW]);
  // A bar of room after the last clip and after the playhead, which plays on past the
  // song's end; never narrower than the window. Grown a bar at a time, the rows redraw
  // once a bar rather than on every poll. While it follows the playhead, a view's width more,
  // so a page can always put the playhead at its left.
  const ppb = useZoom();
  const follow = useFollow();
  const reach = playing && follow ? beat + visible / ppb : beat;
  // What a zoom scrolled to keeps its room: the beat it kept in place stays there.
  const [spare, setSpare] = useState(0);
  // The ruler's tools sit over its end: the song's room has their width more, so its last bar
  // number scrolls clear of them (next D4b).
  const width = Math.max(room(timeline.meter, timeline.end, reach) * ppb + TOOLS_W, visible, spare);

  // Where the timeline scrolled itself last (a page, a zoom): any other scroll across is by hand.
  const ours = useRef<number | null>(null);
  const scrollTo = (left: number) => {
    const el = scroller.current;
    if (!el) return;
    el.scrollLeft = Math.max(0, left);
    ours.current = el.scrollLeft;
  };

  // Follow the playhead while playing (Q11, taste.md §5.1): when it passes 85% of the view, or is
  // out of it, the view pages so it sits at 15%. Scrolling across by hand pauses it until play
  // starts again or the playhead is back in view.
  const paused = useRef(false);
  useEffect(() => {
    paused.current = false;
    if (!playing || !follow) return;
    return onFrame((b) => {
      const el = scroller.current;
      if (!el) return;
      const view = el.clientWidth - latest.current.headerW;
      const at = b * zoomNow() - el.scrollLeft;
      if (paused.current) {
        if (at < 0 || at > view) return;
        paused.current = false;
      }
      if (at < 0 || at > view * 0.85) scrollTo(b * zoomNow() - view * 0.15);
    });
  }, [playing, follow]);
  const across = useRef(0);
  const onScroll = () => {
    const el = scroller.current;
    if (!el || el.scrollLeft === across.current) return;
    across.current = el.scrollLeft;
    if (ours.current !== null && Math.abs(el.scrollLeft - ours.current) <= 1) return;
    ours.current = null;
    if (latest.current.playing) paused.current = true;
  };

  // Zoom (Q8, Q9): `next` px a beat, the beat `at` px into the lanes' visible width staying put.
  // Z's way back, while the zoom is still the one it set.
  const fitted = useRef<{ zoom: number; scroll: number; to: number } | null>(null);
  /** Zoom to `z` px a beat and scroll to `left`, the lanes wide enough to. */
  const zoomScroll = (z: number, left: number) => {
    const el = scroller.current;
    if (!el) return;
    const view = el.clientWidth - latest.current.headerW;
    flushSync(() => {
      setZoom(z);
      setSpare(Math.max(0, left) + view);
    });
    scrollTo(left);
  };
  const zoomTo = (next: number, at: number) => {
    const el = scroller.current;
    const z0 = zoomNow(), z1 = roundZoom(next);
    if (!el || z1 === z0) return;
    const beat = (el.scrollLeft + at) / z0;
    fitted.current = null;
    zoomScroll(z1, beat * z1 - at);
  };
  /** ⌘= and ⌘−: around the playhead while it is in view, else the view's middle. */
  const zoomStep = (dir: 1 | -1) => {
    const el = scroller.current;
    if (!el) return;
    const view = el.clientWidth - latest.current.headerW;
    const at = beatNow() * zoomNow() - el.scrollLeft;
    zoomTo(stepped(zoomNow(), dir), at >= 0 && at <= view ? at : view / 2);
  };
  /** Z: the selected clips (a marquee's bars) fill the view, or with none the whole song; again, back. */
  const zoomFit = () => {
    const el = scroller.current;
    if (!el) return;
    const back = fitted.current;
    if (back && zoomNow() === back.to) {
      fitted.current = null;
      return zoomScroll(back.zoom, back.scroll);
    }
    const { timeline } = latest.current;
    const sel = useClipSelection.getState();
    const picked = pickedIn(timeline, sel.picked).map((p) => p.clip);
    const [a, b] =
      sel.range ?? (picked.length ? [Math.min(...picked.map((c) => c.start)), Math.max(...picked.map((c) => c.start + c.length))] : [0, timeline.end]);
    const view = el.clientWidth - latest.current.headerW;
    const was = { zoom: zoomNow(), scroll: el.scrollLeft };
    const to = roundZoom((view - 2 * FIT_PAD) / Math.max(b - a, 1 / 64));
    zoomScroll(to, a * to - FIT_PAD);
    fitted.current = { ...was, to };
  };
  /** ⌘↓ (1) and ⌘↑ (-1): the selected track a step taller or shorter, or every track with none selected. */
  const height = (dir: 1 | -1) => {
    const m = useShell.getState().selected;
    const tracks = m && m !== "song" ? [m] : latest.current.timeline.tracks.filter((t) => t.track && t.module !== "song").map((t) => t.module);
    const now = heightsNow();
    setHeights(Object.fromEntries(tracks.map((t) => [t, stepHeight(trackHeight(now, t), dir)])));
  };
  /**
   * Transport ▸ Add Marker, at the playhead's bar; ⌥← and ⌥→, to the marker before or after it (as
   * a ruler click). From the marker just gone to, until the transport's clock has heard of it, so
   * ⌥→ pressed twice goes on to the second.
   */
  const jumped = useRef<{ beat: number; at: number } | null>(null);
  const marker = (op: "add" | "prev" | "next") => {
    const { timeline, locked, onSeek } = latest.current;
    const list = markersOf(timeline);
    const j = jumped.current;
    const now = j && performance.now() - j.at < 600 ? j.beat : beatNow();
    if (op !== "add") {
      const m = op === "prev" ? before(list, now) : after(list, now);
      if (!m) return;
      jumped.current = { beat: m.beat, at: performance.now() };
      return onSeek(m.beat);
    }
    if (locked) return useStore.getState().toast(`${locked}.`);
    const bar = barAt(timeline.meter, now);
    if (markerAt(list, bar.start)) return useStore.getState().toast(`A marker is at bar ${bar.bar + 1} already`);
    // Its field opens at the bar: in view.
    const el = scroller.current;
    if (el) {
      const view = el.clientWidth - latest.current.headerW;
      const x = bar.start * zoomNow() - el.scrollLeft;
      if (x < 0 || x > view - 200) scrollTo(bar.start * zoomNow() - view * 0.15);
    }
    useMarkerEdit.setState({ editing: { beat: bar.start, from: null } });
  };
  useEffect(() => {
    Object.assign(canvas, { zoom: zoomStep, zoomFit, follow: () => setFollow(!followNow()), height, marker });
    return () => {
      for (const k of ["zoom", "zoomFit", "follow", "height", "marker"] as const) delete canvas[k];
    };
  });
  // A pinch, or ⌘-scroll (Ctrl-scroll): around the pointer. Chrome and WebKitGTK send a pinch as a
  // wheel with Ctrl held; WebKit on the Mac as gesture events, which must never zoom the page.
  useEffect(() => {
    const el = scroller.current!;
    const at = (x: number) => Math.max(0, x - el.getBoundingClientRect().left - latest.current.headerW);
    const wheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const dy = Math.max(-100, Math.min(100, e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY));
      zoomTo(zoomNow() * ZOOM.step ** (-dy / 100), at(e.clientX));
    };
    let z0 = zoomNow();
    const gesture = (e: Event) => {
      e.preventDefault();
      const g = e as Event & { scale: number; clientX: number };
      if (e.type === "gesturestart") z0 = zoomNow();
      else if (e.type === "gesturechange") zoomTo(z0 * g.scale, at(g.clientX));
    };
    el.addEventListener("wheel", wheel, { passive: false });
    for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.addEventListener(t, gesture);
    return () => {
      el.removeEventListener("wheel", wheel);
      for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.removeEventListener(t, gesture);
    };
  }, []);

  // Clips being dragged (moved, copied or resized), drawn where they go until the timeline shows
  // the edit. Named by id, so an edit that reorders a lane (a move, an undo, an edit elsewhere)
  // keeps them.
  const [drag, setDrag] = useState<Drag | null>(null);
  const latest = useRef({ timeline, revision, onOpenClip, locked, playing, onSeek, headerW });
  latest.current = { timeline, revision, onOpenClip, locked, playing, onSeek, headerW };
  // A press on a lane's empty space, until it is let go or becomes a marquee.
  const pressed = useRef<{ pointer: number; x: number } | null>(null);
  const made = useRef<Made | null>(null);
  // The lanes and the ruler, in whose pixels a marquee is drawn.
  const content = useRef<HTMLDivElement>(null);
  const [marquee, setMarquee] = useState<Marquee | null>(null);
  const [menu, setMenu] = useState<MenuAt | null>(null);
  // Repeat…'s popover, where the clip menu was.
  const [repeating, setRepeating] = useState<{ x: number; y: number } | null>(null);
  // An automation point being dragged or painted: what the bubble at the pointer says.
  const [bubble, setBubble] = useState<{ at: { x: number; y: number }; text: string } | null>(null);
  // Beats ← and → have asked for and that wait for the timeline to show the last edit.
  const nudged = useRef(0);

  /** Once the graph shows the gesture that made a clip, select it, and open a notes clip in Notes. */
  const settle = () => {
    const m = made.current;
    const graph = useStore.getState().graph;
    if (!m || !graph || graph.view.revision < m.revision) return;
    made.current = null;
    const lane = findLane(graph.timeline, m.clip.module, m.clip.lane);
    if (!lane || clipIndex(lane.clips, m.clip) < 0) return;
    useClipSelection.getState().only([m.clip]);
    if (lane.kind === "notes") latest.current.onOpenClip(m.clip);
  };
  useEffect(settle, [timeline]);
  // A dropped drag is drawn where it went until the timeline shows the edit; so are edited points.
  useEffect(() => {
    if (drag?.landed !== undefined && revision >= drag.landed) setDrag(null);
    const editing = usePoints.getState().editing;
    if (editing?.landed !== undefined && revision >= editing.landed) usePoints.setState({ editing: null });
  }, [revision, drag]);
  // A new song starts with nothing selected; the second Esc (the track deselected) clears it too.
  useEffect(() => {
    useClipSelection.getState().clear();
    return useShell.subscribe((s, was) => s.selected === null && was.selected !== null && useClipSelection.getState().clear());
  }, []);

  const edits = useMemo<ClipEdits>(() => {
    const toast = (error: unknown) => useStore.getState().toast(error instanceof Error ? error.message : String(error));
    /** What a track is called, for the words of an edit to its clips ("moving a Hook clip"). */
    const named = (module: string) => moduleName({ key: module, label: latest.current.timeline.tracks.find((t) => t.module === module)?.label });
    /** "a Bass clip", or "3 clips". */
    const clipsWord = (refs: { module: string }[]) => (refs.length === 1 ? `a ${named(refs[0].module)} clip` : `${refs.length} clips`);
    /** The graph to send a gesture on, when the timeline can take one now. */
    const ready = () => {
      const graph = useStore.getState().graph;
      // A gesture names clips as the timeline draws them, so it waits until it shows the last edit.
      return graph && !busy(graph) && !latest.current.locked ? graph : null;
    };
    /**
     * What a drag writes when let go, and its undo step's name: null when it changes nothing, or
     * names a clip the timeline doesn't draw now. Over a lane that can't take the clips, the clips
     * where they were (letting go there writes nothing).
     */
    const gestureOf = (d: Drag): { gesture: Gesture; step: string } | null => {
      const refs = d.clips.flatMap((c) => serverRef(c.lane, c.ref) ?? []);
      if (refs.length !== d.clips.length) return null;
      const first = d.clips[0].ref;
      if (d.kind !== "move") return { gesture: { op: "resize", clip: refs[0], start: d.start, length: d.length }, step: `resizing ${clipsWord([first])}` };
      const [by, to, copy] = d.refused ? [0, null, false] : [d.by, d.to, d.copy];
      if (by === 0 && !to && !copy) return { gesture: { op: "move", clips: refs, by: 0 }, step: "" };
      const onto = to && to.module !== first.module ? ` to ${named(to.module)}` : "";
      const gesture: Gesture = { op: "move", clips: refs, by, ...(to ? { to } : {}), ...(copy ? { copy: true } : {}) };
      return { gesture, step: `${copy ? "copying" : "moving"} ${clipsWord(d.clips.map((c) => c.ref))}${onto}` };
    };
    /**
     * A dropped drag: one gesture, through its lease when one lives (heard as it went, 8b.7). Drawn
     * where it went until the timeline shows it; refused, it goes back.
     */
    const drop = (d: Drag, lease: LaneLease | null) => {
      const graph = ready();
      const made = gestureOf(d);
      if (!graph || !made || !made.step) {
        lease?.abort();
        return setDrag(null);
      }
      const { gesture, step } = made;
      setDrag({ ...d, landed: Infinity });
      (lease ? lease.finish(gesture, { name: step }) : send(graph, gesture, { name: step })).then(
        (done) => {
          setDrag((now) => (now?.landed === Infinity ? { ...now, landed: done.revision } : now));
          // Copies and clips moved to another lane are new there: they are the selection now.
          if (done.made.length === 0) return;
          const refs = done.made.map((m) => ({ module: m.module, lane: m.lane, id: m.id }));
          useClipSelection.getState().only(refs);
          const notes = findLane(latest.current.timeline, refs[0].module, refs[0].lane)?.kind === "notes";
          useShell.getState().select(refs[0].module, notes ? refs[0] : undefined);
        },
        (error) => {
          setDrag(null);
          toast(error);
        },
      );
    };
    /** The selected clips as the timeline draws them now, each with how a gesture names it. */
    const chosen = () =>
      pickedIn(latest.current.timeline, useClipSelection.getState().picked).flatMap((p) => {
        const server = serverRef(p.lane, p.ref);
        return server ? [{ ...p, server }] : [];
      });
    /** After a gesture: what it made (copies, pasted clips, second halves) and `keep` are the selection. */
    const pick = (done: Done, keep: ClipRef[] = [], range: [number, number] | null = null) => {
      const refs = [...keep, ...done.made.map((m) => ({ module: m.module, lane: m.lane, id: m.id }))];
      if (refs.length) useClipSelection.getState().only(refs, range);
    };
    /**
     * `times` copies of the selected clips, one after another (⌘D: one; Repeat…: more). False with
     * none selected.
     */
    const copies = (times: number) => {
      const sel = useClipSelection.getState();
      const picked = chosen();
      if (!picked.length) return false;
      const graph = ready();
      if (!graph) return true;
      const { timeline } = latest.current;
      // A span shorter than a bar lands on the next line of the grid (exactly, with snap off).
      const lo = sel.range?.[0] ?? Math.min(...picked.map((p) => p.clip.start));
      const g = useGrid.getState().song;
      const grid = g.on ? stepOf(g, zoomNow(), barAt(timeline.meter, lo).length) : undefined;
      const range = sel.range;
      const what = clipsWord(picked.map((p) => p.ref));
      const gesture: Gesture = { op: "duplicate", clips: picked.map((p) => p.server), ...(range ? { range } : {}), ...(grid ? { grid } : {}), ...(times > 1 ? { times } : {}) };
      send(graph, gesture, { name: times > 1 ? `repeating ${what}` : `duplicating ${what}` }).then(
        // The copies are the selection now; a marquee's bars move on with them, so ⌘D again goes on.
        (done) => pick(done, [], range ? [range[1], range[1] + times * (range[1] - range[0])] : null),
        toast,
      );
      return true;
    };
    /** Delete the selected clips (⌫, or ⌘X after copying them). */
    const remove = (verb: string) => {
      const picked = useClipSelection.getState().picked;
      const { timeline } = latest.current;
      const refs = picked.flatMap((p) => refIn(timeline, p) ?? []);
      const graph = ready();
      if (!refs.length || !graph) return;
      send(graph, { op: "delete", clips: refs }, { name: `${verb} ${clipsWord(refs)}` }).catch(toast);
      // Notes closes on a clip deleted (taste.md §3.7).
      const shell = useShell.getState();
      for (const p of picked) if (sameClip(shell.clips[p.module], p)) shell.setClip(p.module, null);
      useClipSelection.getState().clear();
    };
    /**
     * Send the nudges asked for, as one move, once the timeline shows the last edit: a held arrow
     * key moves the clips on, never from where they were drawn before.
     */
    const flush = () => {
      const by = nudged.current;
      const graph = by ? ready() : null;
      if (!graph) return;
      nudged.current = 0;
      const picked = chosen();
      if (!picked.length) return;
      const lo = Math.min(...picked.map((p) => p.clip.start));
      const move = clean(Math.max(by, -lo));
      if (move) send(graph, { op: "move", clips: picked.map((p) => p.server), by: move }, { name: `moving ${clipsWord(picked.map((p) => p.ref))}` }).catch(toast);
    };
    /**
     * A press on a lane's empty space that moves: a marquee across the lanes it crosses, its range on
     * bar lines (design-8b §7.4). Its clips are the selection as it is dragged, and ⌘D copies its bars.
     */
    const marqueeFrom = (e: React.PointerEvent<HTMLElement>) => {
      const pointer = e.pointerId, x0 = e.clientX, y0 = e.clientY;
      const box = content.current;
      if (!box) return;
      let on = false;
      const move = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return;
        if (!on && Math.abs(ev.clientX - x0) < 4 && Math.abs(ev.clientY - y0) < 4) return;
        // A drag, not a click: play's start stays where it is, and no text is selected as it goes
        // (not by `user-select` on the timeline: in WebKit that can stop typing in a rename field).
        on = true;
        pressed.current = null;
        window.getSelection()?.removeAllRanges();
        const { timeline, headerW } = latest.current;
        const c = box.getBoundingClientRect();
        const beatAt = (x: number) => Math.max(0, (x - c.left - headerW) / zoomNow());
        const a = Math.min(beatAt(x0), beatAt(ev.clientX)), b = Math.max(beatAt(x0), beatAt(ev.clientX));
        const first = barAt(timeline.meter, a), last = barAt(timeline.meter, Math.max(a, b - 1e-6));
        const range: [number, number] = [first.start, last.start + last.length];
        const top = Math.min(y0, ev.clientY), bottom = Math.max(y0, ev.clientY);
        const rows = [...box.querySelectorAll<HTMLElement>("[data-lane]")].flatMap((row) => {
          const r = row.getBoundingClientRect();
          return r.bottom > top && r.top < bottom ? [{ id: row.dataset.lane!, r }] : [];
        });
        const picked = rows.flatMap(({ id }) => {
          const dot = id.indexOf(".");
          const module = id.slice(0, dot), key = id.slice(dot + 1);
          const lane = findLane(timeline, module, key);
          return (lane?.clips ?? []).flatMap((cl: AnyClip, i) => (cl.start < range[1] && cl.start + cl.length > range[0] ? [clipRef(module, key, lane!.clips, i)] : []));
        });
        useClipSelection.getState().only(picked, range);
        usePoints.setState({ picked: null });
        const y1 = rows.length ? Math.min(...rows.map((x) => x.r.top)) : top, y2 = rows.length ? Math.max(...rows.map((x) => x.r.bottom)) : bottom;
        setMarquee({
          left: headerW + range[0] * zoomNow(),
          width: (range[1] - range[0]) * zoomNow(),
          top: y1 - c.top,
          height: y2 - y1,
          at: { x: ev.clientX, y: ev.clientY },
          text: first.bar === last.bar ? `Bar ${first.bar + 1}` : `Bars ${first.bar + 1}–${last.bar + 1}`,
        });
      };
      const end = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return;
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", end);
        window.removeEventListener("pointercancel", end);
        setMarquee(null);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", end);
      window.addEventListener("pointercancel", end);
    };
    /** An automation clip's new points: one gesture (through the drag's lease when one lives), drawn as sent until the timeline shows it. */
    const sendPoints = (graph: GraphState, clip: ClipRef, server: ServerRef, points: Point[], step: string, lease: LaneLease | null = null) => {
      usePoints.setState({ editing: { clip, points, landed: Infinity } });
      const gesture: Gesture = { op: "points", clip: server, points };
      (lease ? lease.finish(gesture, { name: step }) : send(graph, gesture, { name: step })).then(
        (done) => usePoints.setState((s) => (s.editing?.clip === clip ? { editing: { ...s.editing, landed: done.revision } } : s)),
        (error) => {
          usePoints.setState((s) => (s.editing?.clip === clip ? { editing: null } : s));
          toast(error);
        },
      );
    };
    /**
     * A press on an automation clip's curve (TL-6): on a point, it is picked and a drag moves it; on
     * the line, a point is added there and a drag moves it; in Draw mode (B), a drag paints a point
     * on each line of the grid it crosses. One gesture when let go.
     */
    const pointsFrom = (ref: ClipRef, lane: Extract<TimelineLane, { kind: "control" }>, clip: ControlClip, r: DOMRect, e: React.PointerEvent<HTMLElement>, on: number | null) => {
      const graph = ready();
      const server = serverRef(lane, ref);
      if (!graph || !server) return;
      // A point is the selection now, not the clip.
      useClipSelection.getState().clear();
      scroller.current?.focus({ preventScroll: true });
      const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX, y0 = e.clientY;
      el.setPointerCapture(pointer);
      const { meter } = latest.current.timeline;
      const scale = laneScale(lane);
      const title = laneTitle(lane);
      // The lane's drawing spans its scale (D3: Hz on a log scale): a point dragged past the clip's
      // top or bottom goes on past it, as far as the unit allows. The next drawing widens.
      const valueAt = (y: number) => scale.round(bounded(lane.unit, scale.value(1 - (y - r.top) / r.height)));
      const beatAt = (x: number, free: boolean) => {
        const b = clip.start + ((x - r.left) / r.width) * clip.length;
        return Math.min(clip.length, Math.max(0, clean(snapSong(meter, b, zoomNow(), free) - clip.start)));
      };
      const say = (ev: { clientX: number; clientY: number }, p: Point) =>
        setBubble({ at: { x: ev.clientX, y: ev.clientY }, text: `${formatValue(lane.unit, p[1], lane.key)} · ${position(meter, clip.start + p[0])}` });
      const draw = useShell.getState().draw;
      let points: Point[] = clip.points;
      let index = on ?? -1;
      let step = `moving a ${title} point`;
      let changed = false;
      // While the song plays, the points are heard as they are drawn (8b.7): a lease from the first change.
      let lease: LaneLease | null = null;
      let begun = false;
      const heard = () => {
        const gesture: Gesture = { op: "points", clip: server, points };
        if (begun) return lease?.update(gesture);
        begun = true;
        lease = leaseDrag(graph, gesture, (why) => {
          toast(why);
          cancel();
        });
      };
      // Draw mode: the value painted on each line of the grid, filled in between moves.
      const cells = new Map<number, number>();
      let last: Point | null = null;
      const paint = (ev: { clientX: number; clientY: number }) => {
        const p: Point = [beatAt(ev.clientX, false), valueAt(ev.clientY)];
        const grid = stepOf(useGrid.getState().song, zoomNow(), barAt(meter, clip.start + p[0]).length);
        if (last && Math.abs(p[0] - last[0]) > grid) {
          const dir = Math.sign(p[0] - last[0]);
          // Filled in as the pointer went: straight on the drawing, so along the lane's scale.
          const [l0, l1] = [scale.level(last[1]), scale.level(p[1])];
          for (let b = last[0] + dir * grid; dir * (p[0] - b) > 1e-9; b += dir * grid) {
            cells.set(clean(b), scale.round(bounded(lane.unit, scale.value(l0 + ((l1 - l0) * (b - last[0])) / (p[0] - last[0])))));
          }
        }
        cells.set(p[0], p[1]);
        last = p;
        points = painted(clip.points, cells);
        changed = true;
        usePoints.setState({ editing: { clip: ref, points } });
        say(ev, p);
        heard();
      };
      if (draw) {
        step = `drawing ${title} automation`;
        paint(e);
      } else if (on === null) {
        // On the line: a point there, picked.
        const p: Point = [beatAt(e.clientX, skipsSnap(e)), valueAt(e.clientY)];
        ({ points, index } = added(points, p));
        step = `adding a ${title} point`;
        changed = true;
        usePoints.setState({ editing: { clip: ref, points } });
        heard();
      }
      if (index >= 0) usePoints.setState({ picked: { clip: ref, index, at: points[index] } });
      let moving = false;
      const move = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return;
        if (draw) return paint(ev);
        if (!moving && Math.abs(ev.clientX - x0) < 3 && Math.abs(ev.clientY - y0) < 3) return;
        moving = true;
        const p: Point = [beatAt(ev.clientX, skipsSnap(ev)), valueAt(ev.clientY)];
        points = moved(points, index, p);
        changed = true;
        usePoints.setState({ editing: { clip: ref, points } });
        say(ev, points[index]);
        heard();
      };
      const off = () => {
        for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
        window.removeEventListener("keydown", esc, true);
        setBubble(null);
      };
      /** Put the points back as they were: nothing is written, and the lease's preview ends. */
      const cancel = () => {
        off();
        lease?.abort();
        usePoints.setState({ editing: null });
        if (el.hasPointerCapture(pointer)) el.releasePointerCapture(pointer);
      };
      const end = (ev: PointerEvent) => {
        if (ev.pointerId !== pointer) return;
        if (ev.type !== "pointerup" || !changed) return cancel();
        off();
        // The point picked is where it was let go.
        if (index >= 0 && !draw) usePoints.setState({ picked: { clip: ref, index, at: points[index] } });
        sendPoints(graph, ref, server, points, step, lease);
      };
      // Esc during the drag puts it back, and is the drag's alone (not the page's deselect).
      const esc = (ev: KeyboardEvent) => {
        if (ev.key !== "Escape") return;
        ev.preventDefault();
        ev.stopImmediatePropagation();
        cancel();
      };
      const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
      for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      window.addEventListener("keydown", esc, true);
    };
    return {
      down(ref, e) {
        if (e.button !== 0) return;
        pressed.current = null;
        const lane = findLane(latest.current.timeline, ref.module, ref.lane);
        const i = lane ? clipIndex(lane.clips, ref) : -1;
        if (!lane || i < 0) return;
        const c = lane.clips[i];
        // ⇧-click adds a clip, ⌘-click (Ctrl) toggles one when let go; a click picks only it, unless
        // it is picked already: then a drag moves every picked clip (design-8b §7.4).
        const add = e.shiftKey, toggle = e.metaKey || e.ctrlKey;
        const onEdge = !!(e.target as HTMLElement).closest("[data-edge]");
        // On an automation clip's curve, or anywhere on it in Draw mode: its points (TL-6). A point
        // wins over an edge; the line doesn't.
        if (lane.kind === "control" && !latest.current.locked && !add && !toggle) {
          // The drawing (`Body`), from the clip's own box: inside its border, 4px in at top and foot.
          const box = e.currentTarget.getBoundingClientRect();
          const r = new DOMRect(box.left + BORDER, box.top + BORDER + BODY_TOP, box.width - 2 * BORDER, box.height - 2 * BORDER - BODY_TOP - BODY_BOTTOM);
          if (r.width > 0 && r.height > 0) {
            const scale = laneScale(lane);
            const at = hit((c as ControlClip).points, c.length, scale.level, r.width, r.height, e.clientX - r.left, e.clientY - r.top, scale.path);
            const draw = useShell.getState().draw;
            const point = at?.point ?? null;
            if (draw || point !== null || (at && !onEdge)) return pointsFrom(ref, lane, c as ControlClip, r, e, point);
          }
        }
        usePoints.setState({ picked: null });
        const sel = useClipSelection.getState();
        if (add) sel.add(ref);
        else if (!toggle && !isPicked(sel.picked, ref)) sel.only([ref]);
        // The track follows the click, and Notes the notes clip. The timeline keeps the keyboard (⌫,
        // Q, the arrows): a roll that opens on the clip opens now, so the focus it takes comes back.
        flushSync(() => useShell.getState().select(ref.module, lane.kind === "notes" ? ref : undefined));
        scroller.current?.focus({ preventScroll: true });
        const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX, y0 = e.clientY;
        const edge = (e.target as HTMLElement).closest<HTMLElement>("[data-edge]")?.dataset.edge as "left" | "right" | undefined;
        // What moves: every picked clip when this one is picked, each as drawn now (its hash is
        // what this window saw: one changed since is refused when it lands).
        const { timeline } = latest.current;
        const group = (isPicked(useClipSelection.getState().picked, ref) && !edge ? useClipSelection.getState().picked : [ref]).flatMap((r) => {
          const l = findLane(timeline, r.module, r.lane);
          const k = l ? clipIndex(l.clips, r) : -1;
          return l && k >= 0 ? [{ ref: r, clip: l.clips[k], lane: l, color: trackColors(timeline, r.module) ?? GRAPHITE }] : [];
        });
        const oneLane = group.every((g) => g.ref.module === ref.module && g.ref.lane === ref.lane);
        el.setPointerCapture(pointer);
        let moved = false;
        let now: Drag | null = null;
        // While the song plays, the drag is heard where it is (8b.7): a lease from the threshold on.
        let lease: LaneLease | null = null;
        const move = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer || latest.current.locked) return;
          if (!moved && Math.abs(ev.clientX - x0) < 4 && Math.abs(ev.clientY - y0) < 4) return;
          const first = !moved;
          moved = true;
          const meter = latest.current.timeline.meter;
          const free = skipsSnap(ev);
          const raw = (ev.clientX - x0) / zoomNow();
          const at = { x: ev.clientX, y: ev.clientY };
          const base = { clips: group, by: 0, to: null, copy: false, refused: null, start: c.start, length: c.length, at };
          // An audio clip's edges stop at its file (Q13, D3): the bubble says which end.
          const file = lane.kind === "audio" ? edgeLimits(c as AudioClip, latest.current.timeline.tempo, fileSeconds(useStore.getState().graph?.session ?? 0, c as AudioClip)) : null;
          if (edge === "left") {
            // The left edge on the grid; what is inside stays where it plays (the server shifts it).
            const end = c.start + c.length;
            const wanted = Math.max(0, Math.min(snapSong(meter, c.start + raw, zoomNow(), free), end - SHORTEST));
            const start = file?.left != null ? Math.max(wanted, clean(file.left)) : wanted;
            const text = start > wanted && file ? file.leftWord : position(meter, start);
            now = { ...base, kind: "left", start, length: clean(end - start), text, guide: start };
          } else if (edge === "right") {
            const wanted = Math.max(snapSong(meter, c.start + c.length + raw, zoomNow(), free), c.start + SHORTEST);
            const end = file?.right != null ? Math.min(wanted, Math.max(clean(file.right), c.start + SHORTEST)) : wanted;
            const text = end < wanted && file ? file.rightWord : position(meter, end);
            now = { ...base, kind: "right", length: clean(end - c.start), text, guide: end };
          } else {
            // The grabbed clip's start lands on the grid; the others keep their distance from it.
            const lo = Math.min(...group.map((g) => g.clip.start));
            const by = Math.max(clean(snapSong(meter, c.start + raw, zoomNow(), free) - c.start), -lo);
            // Onto the lane under the pointer, when the clips come from one lane and it takes them.
            const over = oneLane ? laneUnder(ev.clientX, ev.clientY) : null;
            const target = over && !(over.module === ref.module && over.lane === ref.lane) ? over : null;
            const into = target ? findLane(latest.current.timeline, target.module, target.lane) : undefined;
            const refused = into ? refusal(lane, into) : null;
            const to = into && !refused ? target : null;
            const where = to && to.module !== ref.module ? ` · ${named(to.module)}` : "";
            now = { ...base, kind: "move", by, to, copy: ev.altKey, refused, text: refused ?? `${position(meter, c.start + by)}${where}`, guide: c.start + by };
          }
          setDrag(now);
          const heard = gestureOf(now);
          const graph = heard && first ? ready() : null;
          if (graph && heard) {
            lease = leaseDrag(graph, heard.gesture, (why) => {
              toast(why);
              cancel();
            });
          } else if (heard) lease?.update(heard.gesture);
        };
        const off = () => {
          for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
          window.removeEventListener("keydown", esc, true);
        };
        /** The drag put back: nothing is written, and its lease's preview ends. */
        const cancel = () => {
          off();
          lease?.abort();
          setDrag(null);
          if (el.hasPointerCapture(pointer)) el.releasePointerCapture(pointer);
        };
        const end = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer) return;
          off();
          if (!moved) {
            // A click: the selection's rules.
            if (ev.type === "pointerup" && toggle) useClipSelection.getState().toggle(ref);
            else if (ev.type === "pointerup" && !add) useClipSelection.getState().only([ref]);
            return;
          }
          // Let go over a lane that can't take the clips, or the drag cancelled: nothing happens.
          if (ev.type !== "pointerup" || !now || now.refused) return cancel();
          drop(now, lease);
        };
        // Esc during the drag puts it back, and is the drag's alone (not the page's deselect).
        const esc = (ev: KeyboardEvent) => {
          if (ev.key !== "Escape" || !moved) return;
          ev.preventDefault();
          ev.stopImmediatePropagation();
          cancel();
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
        window.addEventListener("keydown", esc, true);
      },
      create(module, key, beat) {
        const lane = findLane(latest.current.timeline, module, key);
        const graph = ready();
        if (!lane || lane.kind === "audio" || !graph) return;
        // The bar clicked, after any clip before it and up to the next; one bar at most.
        const bar = barAt(latest.current.timeline.meter, beat);
        const before = Math.max(0, ...lane.clips.filter((c) => c.start + c.length <= beat).map((c) => c.start + c.length));
        const next = Math.min(Infinity, ...lane.clips.filter((c) => c.start >= beat).map((c) => c.start));
        const start = Math.max(bar.start, before);
        const length = Math.min(bar.length, next - start);
        if (!(length > 0) || lane.clips.some((c) => c.start < start + length && c.start + c.length > start)) return;
        // The server gives it a new id (FMT-12); it is selected and opens once the timeline shows it.
        const m: Made = { revision: Infinity, clip: { module, lane: key, id: "" } };
        made.current = m;
        const what = lane.kind === "control" ? `${laneTitle(lane)} automation` : named(module);
        send(graph, { op: "create", module, lane: key, start, length }, { name: `adding a ${what} clip` }).then(
          (done) => {
            const id = done.made.find((x) => x.module === module && x.lane === key)?.id;
            if (made.current !== m || !id) return;
            m.clip = { module, lane: key, id };
            m.revision = done.revision;
            settle();
          },
          (error) => {
            if (made.current === m) made.current = null;
            toast(error);
          },
        );
      },
      unpick(module) {
        useShell.getState().setClip(module, null);
        useClipSelection.getState().clear();
        usePoints.setState({ picked: null });
      },
      press(e) {
        pressed.current = e && e.button === 0 ? { pointer: e.pointerId, x: e.clientX } : null;
        if (e && e.button === 0) marqueeFrom(e);
      },
      place(e, beat) {
        const p = pressed.current;
        pressed.current = null;
        if (!p || p.pointer !== e.pointerId || Math.abs(e.clientX - p.x) >= 4 || latest.current.playing) return;
        // On the grid, as a dragged clip lands (anywhere while snap is off).
        latest.current.onSeek(snapSong(latest.current.timeline.meter, beat, zoomNow()));
      },
      remove: () => remove("deleting"),
      duplicate: () => copies(1),
      repeat: (times) => void copies(times),
      split() {
        const graph = ready();
        if (!graph) return;
        const { timeline } = latest.current;
        const at = playhead();
        const crosses = (c: { start: number; length: number }) => c.start < at - 1e-9 && at < c.start + c.length - 1e-9;
        const any = useClipSelection.getState().picked.length > 0;
        let targets: { ref: ClipRef; server: ServerRef }[];
        if (any) targets = chosen().filter((p) => crosses(p.clip));
        else {
          // Nothing selected: every clip under the playhead on the selected track, in the lanes it shows.
          const m = useShell.getState().selected;
          const track = m ? timeline.tracks.find((t) => t.module === m) : undefined;
          const shown = track?.lanes.filter((l) => l.kind !== "control" || useShell.getState().open.has(track.module)) ?? [];
          targets = shown.flatMap((l) =>
            l.clips.flatMap((c: AnyClip, i) => {
              const ref = clipRef(track!.module, l.key, l.clips, i);
              const server = crosses(c) ? serverRef(l, ref) : null;
              return server ? [{ ref, server }] : [];
            }),
          );
        }
        if (!targets.length) return toast(`No ${any ? "selected " : ""}clip crosses the playhead at ${position(timeline.meter, at)}, so there is nothing to split.`);
        send(graph, { op: "split", clips: targets.map((t) => t.server), at }, { name: `splitting ${clipsWord(targets.map((t) => t.ref))}` }).then(
          // Both halves of each are the selection.
          (done) => pick(done, targets.map((t) => t.ref)),
          toast,
        );
      },
      join() {
        const graph = ready();
        const picked = chosen();
        if (!graph || !picked.length) return;
        // The earliest keeps its id, and is the selection after.
        const first = [...picked].sort((a, b) => a.clip.start - b.clip.start)[0];
        const step = `joining ${picked.length === 1 ? clipsWord([first.ref]) : `${picked.length} ${named(first.ref.module)} clips`}`;
        send(graph, { op: "join", clips: picked.map((p) => p.server) }, { name: step }).then(() => useClipSelection.getState().only([first.ref]), toast);
      },
      quantize() {
        const graph = ready();
        const picked = chosen();
        if (!graph || !picked.length) return;
        const notes = picked.filter((p) => p.lane.kind === "notes");
        if (!notes.length) return toast("Only notes can be quantized: pick a notes clip.");
        // The roll's grid: the one notes are placed on.
        const g = useGrid.getState().notes;
        const step = `quantizing ${clipsWord(notes.map((p) => p.ref))} to ${pickName(g.pick, g.triplet)}`;
        send(graph, { op: "quantize", clips: notes.map((p) => p.server), grid: stepOf(g, zoomNow()) }, { name: step }).catch(toast);
      },
      nudge(dir, bar) {
        const picked = chosen();
        if (!picked.length || latest.current.locked) return;
        const { meter } = latest.current.timeline;
        const lo = Math.min(...picked.map((p) => p.clip.start));
        const here = barAt(meter, lo).length;
        nudged.current = clean(nudged.current + dir * (bar ? here : stepOf(useGrid.getState().song, zoomNow(), here)));
        flush();
      },
      copy(cut) {
        const { timeline } = latest.current;
        const copied = copiedOf(timeline, useClipSelection.getState().picked);
        if (!copied.length) return;
        useClipboard.setState({ copied });
        if (cut) remove("cutting");
      },
      paste(beat, to) {
        const { copied } = useClipboard.getState();
        const graph = ready();
        if (!copied.length || !graph) return;
        const { timeline } = latest.current;
        const at = beat ?? playhead();
        let target = to;
        // From one lane: onto the selected track's lane of their kind (theirs, when it is their track).
        const from = findLane(timeline, copied[0].module, copied[0].lane);
        if (!target && from && copied.every((c) => c.module === copied[0].module && c.lane === copied[0].lane)) {
          const m = useShell.getState().selected;
          const track = m ? timeline.tracks.find((t) => t.module === m) : undefined;
          const lane = track && (m === copied[0].module ? from : track.lanes.find((l) => !refusal(from, l)));
          if (track && lane) target = { module: track.module, lane: lane.key };
        }
        const into = target ?? copied[0];
        const step = `pasting ${copied.length === 1 ? `a ${named(into.module)} clip` : `${copied.length} clips`}`;
        send(graph, { op: "paste", clips: copied, at, ...(target ? { to: target } : {}) }, { name: step }).then((done) => pick(done), toast);
      },
      loop: () => canvas.loopSelection?.(),
      menu(e, module, lane, beat, clip) {
        e.preventDefault();
        if (clip === "header") return setMenu({ x: e.clientX, y: e.clientY, module, lane, beat, on: "header" });
        if (clip) {
          // A right-click on a clip that isn't selected picks it alone, as a click would.
          const sel = useClipSelection.getState();
          if (!isPicked(sel.picked, clip)) sel.only([clip]);
          const l = findLane(latest.current.timeline, clip.module, clip.lane);
          flushSync(() => useShell.getState().select(clip.module, l?.kind === "notes" ? clip : undefined));
          scroller.current?.focus({ preventScroll: true });
        }
        setMenu({ x: e.clientX, y: e.clientY, module, lane, beat, on: clip ? "clip" : "lane" });
      },
      unautomate: (module, key) => !latest.current.locked && unautomate({ module, lane: key }),
      unpoint() {
        const { picked } = usePoints.getState();
        if (!picked) return false;
        const graph = ready();
        const lane = findLane(latest.current.timeline, picked.clip.module, picked.clip.lane);
        const i = lane ? clipIndex(lane.clips, picked.clip) : -1;
        const c = lane?.kind === "control" ? lane.clips[i] : undefined;
        const server = lane && c ? serverRef(lane, picked.clip) : null;
        if (!lane || !c || !server || pointIn(picked, c.points, false) < 0) {
          usePoints.setState({ picked: null });
          return false;
        }
        if (!graph) return true;
        const points = c.points.filter((_, k) => k !== picked.index);
        sendPoints(graph, picked.clip, server, points, `deleting a ${laneTitle(lane)} point`);
        usePoints.setState({ picked: null });
        return true;
      },
      flush,
    };
    // `latest` carries everything that changes; the edits themselves never do.
  }, []);

  // ⌘1 to ⌘4 (View's grid rows) change the grid of the view last pointed at or focused, while it
  // is on screen: this one's or the roll's. The timeline's zoom says what Auto is.
  useEffect(() => {
    canvas.grid = (op) => gridOp(gridTarget(), op, zoomNow());
    return () => void delete canvas.grid;
  }, []);

  // Edit ▸ Split, Join and Quantize (⌘E, ⌘J), ⌘D after the roll's notes, and ⌘C ⌘X ⌘V: the
  // timeline's while clips are selected (or, for ⌘V, copied) and the roll doesn't have the keyboard.
  useEffect(() => {
    const takes = () => !document.activeElement?.closest("#dock") && (useClipSelection.getState().picked.length > 0 || useClipboard.getState().copied.length > 0);
    const run = (op: "copy" | "cut" | "paste") => (op === "paste" ? edits.paste() : edits.copy(op === "cut"));
    Object.assign(canvas, { split: edits.split, join: edits.join, quantize: edits.quantize, duplicate: edits.duplicate, takesClipboard: takes, clipboard: run });
    // In the app the menu bar's Cut, Copy and Paste (and Linux's own keys) run the web view's
    // commands, which do nothing with no text selected unless the page takes them first (M7).
    const typing = () => {
      const a = document.activeElement;
      return a instanceof HTMLInputElement || a instanceof HTMLTextAreaElement || (a instanceof HTMLElement && a.isContentEditable);
    };
    const mine = (op: "copy" | "cut" | "paste") => !typing() && takes() && (op === "paste" ? useClipboard.getState().copied.length > 0 : useClipSelection.getState().picked.length > 0);
    const before = (op: "copy" | "cut" | "paste") => (e: Event) => void (mine(op) && e.preventDefault());
    const take = (op: "copy" | "cut" | "paste") => (e: Event) => {
      if (!mine(op)) return;
      e.preventDefault();
      run(op);
    };
    const listeners = (["copy", "cut", "paste"] as const).flatMap((op) => [
      [`before${op}`, before(op)],
      [op, take(op)],
    ]) as [string, (e: Event) => void][];
    for (const [type, f] of listeners) document.addEventListener(type, f);
    return () => {
      for (const k of ["split", "join", "quantize", "duplicate", "takesClipboard", "clipboard"] as const) delete canvas[k];
      for (const [type, f] of listeners) document.removeEventListener(type, f);
    };
  }, [edits]);

  // Once the timeline shows the last edit: clips an edit took away leave the selection, and nudges
  // asked for meanwhile go out.
  useEffect(() => {
    const graph = useStore.getState().graph;
    if (graph && !busy(graph)) useClipSelection.getState().prune(timeline);
    edits.flush();
  }, [timeline, edits]);

  // A lane just made from the canvas (automate): its track unfolds, and the lane is scrolled to
  // and lit once the timeline shows it.
  const open = useShell((s) => s.open);
  const [lit, setLit] = useState<string | null>(null);
  const wanted = useRef<string | null>(null);
  const reveal = () => {
    const id = wanted.current;
    const row = id && scroller.current?.querySelector<HTMLElement>(`[data-lane="${CSS.escape(id)}"]`);
    if (!id || !row) return;
    wanted.current = null;
    // `nearest`, inside the scroll padding the ruler and the Song row take: nothing lands under them (D12).
    row.scrollIntoView({ block: "nearest" });
    setLit(id);
    setTimeout(() => setLit((l) => (l === id ? null : l)), 1800);
  };
  // A hidden lane asked for (Automate on its control) shows first, then is revealed (D4).
  const hiddenLanes = useAllHidden();
  useEffect(reveal, [timeline, open, hiddenLanes]);
  useEffect(() => {
    const made = (e: Event) => {
      const id = (e as CustomEvent<string>).detail;
      wanted.current = id;
      useShell.getState().unfold(id.slice(0, id.indexOf(".")));
      reveal();
    };
    // A new track: lit where it is, its automation still folded (taste.md §3.4).
    const track = (e: Event) => {
      wanted.current = (e as CustomEvent<string>).detail;
      reveal();
    };
    window.addEventListener(LANE_MADE, made);
    window.addEventListener(TRACK_MADE, track);
    return () => {
      window.removeEventListener(LANE_MADE, made);
      window.removeEventListener(TRACK_MADE, track);
    };
  }, []);

  // The timeline's own keys while it has the keyboard: ⌫ deletes the selected clips, Q quantizes
  // them, ← and → nudge them (⇧: by a bar), Esc lets go of them (and of the clip Notes shows).
  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = useShell.getState();
    const sel = useClipSelection.getState();
    const shown = s.selected ? s.clips[s.selected] : undefined;
    const plain = !e.metaKey && !e.ctrlKey && !e.altKey;
    // A point picked is the selection while no clip is.
    const point = sel.picked.length ? null : usePoints.getState().picked;
    if ((e.key === "Delete" || e.key === "Backspace") && !e.metaKey && !e.ctrlKey && point) edits.unpoint();
    else if ((e.key === "Delete" || e.key === "Backspace") && !e.metaKey && !e.ctrlKey && sel.picked.length) edits.remove();
    else if (e.key.toLowerCase() === "q" && plain && !e.shiftKey && sel.picked.length) edits.quantize();
    else if ((e.key === "ArrowLeft" || e.key === "ArrowRight") && plain && sel.picked.length) edits.nudge(e.key === "ArrowLeft" ? -1 : 1, e.shiftKey);
    else if (e.key === "Escape" && point) usePoints.setState({ picked: null });
    else if (e.key === "Escape" && (sel.picked.length || shown)) {
      sel.clear();
      if (shown) s.deselect();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const songRows = rowsOf(song);
  // The Song's automation lanes, less those hidden, and the line that says so (D4).
  const songHidden = useHidden(song.module);
  const songShown = songRows.controls.filter((l) => !songHidden.has(l.key)).length;
  const songLanesH = songShown * LANE_H + (songShown < songRows.controls.length ? HIDDEN_ROW_H : 0);
  const songH = ROW_H + 1 + songRows.others.length * ROW_H + (open.has(song.module) ? songLanesH : 0);
  const all = [...rows, song].filter((t) => t.lanes.some((l) => l.kind === "control")).map((t) => t.module);
  const anyOpen = all.some((m) => open.has(m));
  return (
    <div
      ref={scroller}
      data-testid="timeline"
      data-grid-view="song"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onScroll={onScroll}
      className="flex-1 min-h-0 overflow-auto relative bg-canvas outline-none"
      // Revealing a row never tucks it under the ruler, the headers or the Song row (TL-16).
      style={{ scrollPaddingTop: RULER_H, scrollPaddingLeft: headerW, scrollPaddingBottom: songH }}
    >
      <div ref={content} className="relative flex flex-col" style={{ width: headerW + width, minHeight: "100%" }}>
        <div className="sticky top-0 z-30 shrink-0 flex" style={{ height: RULER_H }}>
          {/* The corner (taste.md §3.4): + Track (⌘T); on the right, every track's automation (A). */}
          <div className="sticky left-0 z-40 shrink-0 bg-chrome border-b border-r border-line flex items-center justify-between px-1.5" style={{ width: headerW }}>
            <Button
              size="sm"
              data-testid="add-track"
              disabled={!!locked}
              title={locked ? `${locked}.` : `Add a track (${chord("T")})`}
              onMouseDown={keepFocus}
              onClick={(e) => {
                const s = useShell.getState();
                openInstruments(e.currentTarget, s.selected && s.selected !== "song" ? s.selected : null);
              }}
              className="!px-1.5"
            >
              <Plus size={14} />
              Track
            </Button>
            <div className="flex items-center gap-0.5">
              {/* The timeline's grid and snap (design-8b §7.5): between + Track and the fold-all chevron. */}
              <GridChip view="song" ppb={ppb} />
              {all.length > 0 && (
                <button
                  data-testid="fold-all"
                  aria-label={anyOpen ? "Hide all automation" : "Show all automation"}
                  aria-expanded={anyOpen}
                  title={`${anyOpen ? "Hide" : "Show"} every track's automation (A)`}
                  onMouseDown={(e) => e.preventDefault()}
                  onClick={() => useShell.getState().toggleLanes(null, all)}
                  className="size-ctl-xs grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
                >
                  <ChevronRight size={12} className={clsx("transition-transform duration-140 ease-out", anyOpen && "rotate-90")} />
                </button>
              )}
            </div>
          </div>
          <Ruler timeline={timeline} width={width} loopStart={loopStart} loopEnd={loopEnd} looping={looping} locked={locked} onSeek={onSeek} onLoop={onLoop} />
          <ViewTools ppb={ppb} follow={follow} onZoom={zoomStep} />
          <PlayCap headerW={headerW} />
        </div>
        {/* Over the clips, multiplied in: lanes full of clips still show where the loop is (TL-10).
            Under the headers and the playhead. */}
        {looping && loopStart !== null && loopEnd !== null && (
          <div
            data-testid="loop-shade"
            aria-hidden
            className="absolute bottom-0 z-[14] pointer-events-none bg-accent/8 mix-blend-multiply"
            style={{ top: RULER_H, left: headerW + loopStart * ppb, width: (loopEnd - loopStart) * ppb }}
          />
        )}
        <Tracks
          timeline={timeline}
          rows={rows}
          width={width}
          visible={visible}
          headerW={headerW}
          drag={drag}
          lit={lit}
          issues={issues}
          edits={edits}
          locked={locked}
          waiting={waiting}
          onOpenClip={onOpenClip}
        />
        <PlayLine
          testid="playhead"
          className="absolute bottom-0 z-[15] w-px bg-fg-strong pointer-events-none"
          style={{ top: RULER_H, left: headerW }}
        />
        {/* While clips are dragged: a dashed guide at the new start (or the edge), and the bubble saying where (design-8b §7.5). */}
        {drag && drag.landed === undefined && (
          <>
            <div data-testid="clip-guide" aria-hidden className="absolute bottom-0 z-[16] border-l border-dashed border-fg-faint pointer-events-none" style={{ top: RULER_H, left: headerW + drag.guide * ppb }} />
            <DragBubble at={drag.at}>{drag.text}</DragBubble>
          </>
        )}
        {/* A marquee being dragged across the lanes, on bar lines, and the bubble naming its bars. */}
        {marquee && (
          <>
            <div
              data-testid="marquee"
              aria-hidden
              className="absolute z-[16] pointer-events-none rounded-xs border border-accent bg-accent/10"
              style={{ left: marquee.left, top: marquee.top, width: marquee.width, height: marquee.height }}
            />
            <DragBubble at={marquee.at}>{marquee.text}</DragBubble>
          </>
        )}
        {bubble && <DragBubble at={bubble.at}>{bubble.text}</DragBubble>}
        {menu && <TimelineMenu at={menu} timeline={timeline} edits={edits} locked={locked} onOpenClip={onOpenClip} onRepeat={setRepeating} onClose={() => setMenu(null)} />}
        {repeating && <RepeatPopover at={repeating} onRepeat={(n) => edits.repeat(n)} onClose={() => setRepeating(null)} />}
        <VolumePopover />
        {/* The Song's row, Live's Main track: pinned at the foot of the track list, with its own playhead and loop wash. */}
        <div className="sticky bottom-0 z-[26] shrink-0 border-t border-line bg-canvas" data-song-row>
          {looping && loopStart !== null && loopEnd !== null && (
            <div aria-hidden className="absolute inset-y-0 z-[14] pointer-events-none bg-accent/8 mix-blend-multiply" style={{ left: headerW + loopStart * ppb, width: (loopEnd - loopStart) * ppb }} />
          )}
          <TrackRows
            track={song}
            color={GRAPHITE}
            width={width}
            headerW={headerW}
            grid={<Grid meter={timeline.meter} width={width} />}
            drag={drag && involves(drag, song.module) ? drag : null}
            lit={lit?.startsWith(`${song.module}.`) ? lit.slice(song.module.length + 1) : null}
            issues={issues.get(song.module)}
            edits={edits}
            locked={locked}
            waiting={waiting}
            last
            onOpenClip={onOpenClip}
          />
          <PlayLine className="absolute top-0 bottom-0 z-[15] w-px bg-fg-strong pointer-events-none" style={{ left: headerW }} />
        </div>
      </div>
    </div>
  );
}

/** Z leaves this much room either side of what it fits, in px. */
const FIT_PAD = 16;
/** The ruler's tools: 3 buttons of 24 px, 2 px apart, 4 px in, and the 1 px edge. */
const TOOLS_W = 3 * 24 + 2 * 2 + 2 * 4 + 1;

/**
 * Follow, Zoom out and Zoom in (Q31), held at the right of the ruler's row as it scrolls (X25): on
 * `chrome`, a 1 px line at its left. Over the ruler's end, which doesn't take clicks under them.
 */
function ViewTools({ ppb, follow, onZoom }: { ppb: number; follow: boolean; onZoom: (dir: 1 | -1) => void }) {
  const zoomIn = chord("="), zoomOut = chord("−"), toggle = chord("F", { shift: true });
  return (
    <div
      data-testid="view-tools"
      className="sticky right-0 z-40 shrink-0 flex items-center gap-0.5 px-1 bg-chrome border-b border-l border-line"
      style={{ width: TOOLS_W, marginLeft: -TOOLS_W }}
    >
      <Button
        size="sm"
        data-testid="follow"
        active={follow}
        aria-pressed={follow}
        aria-label="Follow the playhead"
        title={follow ? `Follow the playhead (${toggle})` : `Not following the playhead (${toggle})`}
        onMouseDown={keepFocus}
        onClick={() => setFollow(!follow)}
        className="w-ctl-sm !px-0"
      >
        <ArrowRightFromLine size={16} />
      </Button>
      <Button
        size="sm"
        data-testid="zoom-out"
        // Not just "Zoom out": the Device view's graph has its own.
        aria-label="Zoom the timeline out"
        disabled={ppb <= ZOOM.min}
        title={ppb <= ZOOM.min ? "Zoomed out all the way" : `Zoom out (${zoomOut})`}
        onMouseDown={keepFocus}
        onClick={() => onZoom(-1)}
        className="w-ctl-sm !px-0"
      >
        <ZoomOut size={16} />
      </Button>
      <Button
        size="sm"
        data-testid="zoom-in"
        aria-label="Zoom the timeline in"
        disabled={ppb >= ZOOM.max}
        title={ppb >= ZOOM.max ? "Zoomed in all the way" : `Zoom in (${zoomIn})`}
        onMouseDown={keepFocus}
        onClick={() => onZoom(1)}
        className="w-ctl-sm !px-0"
      >
        <ZoomIn size={16} />
      </Button>
    </div>
  );
}

/** The playhead's mark in the ruler: an 8×6 triangle (taste.md §2.8). */
function PlayCap({ headerW }: { headerW: number }) {
  const ref = useRef<HTMLDivElement>(null);
  usePlayhead(ref);
  return (
    <div ref={ref} className="absolute bottom-0 z-[35] pointer-events-none" style={{ left: headerW }}>
      <div className="w-0 h-0 -translate-x-1/2 border-x-[4px] border-x-transparent border-t-[6px] border-t-fg-strong" />
    </div>
  );
}

/**
 * Bar and beat lines from beat 0 to `width`: one stretch per time signature, each with its own bar
 * length. Under them the snap grid's own lines, where it is finer than a beat, its lines are at
 * least 12px apart and they meet every bar line (a triplet grid in 3/4 doesn't: none then).
 */
const Grid = memo(function Grid({ meter, width }: { meter: Meter[]; width: number }) {
  const grid = useGrid((s) => s.song);
  const ppb = useZoom();
  return meterSpans(meter, 0, width / ppb).map((s) => {
    const step = fineStep(grid, s.bar, ppb);
    return (
      <VLines
        key={s.start}
        className="inset-y-0"
        style={{ left: s.start * ppb, width: (s.end - s.start) * ppb }}
        layers={[
          ...(step ? [{ every: step * ppb, color: "var(--color-hover)" }] : []),
          // Zoomed far out, beats are too close to draw: bars only.
          ...(ppb >= BEAT_LINES ? [{ every: ppb, color: "var(--color-line)" }] : []),
          { every: s.bar * ppb, color: "var(--color-line-strong)" },
        ]}
      />
    );
  });
});

/** The closest beat lines the timeline draws, in px: zoomed out further, bars only. */
const BEAT_LINES = 6;

/** The grid's step where the timeline draws its lines, in a bar of `bar` beats at `ppb`; null where it doesn't. */
function fineStep(grid: GridSetting, bar: number, ppb: number) {
  const step = stepOf(grid, ppb, bar);
  const fits = Math.abs(bar / step - Math.round(bar / step)) < 1e-6;
  return step < 1 && step * ppb >= 12 && fits ? step : null;
}

/**
 * Bars and beats along the top. A click moves the playhead there; a drag across it sets a loop over
 * the beats it covered, and turns looping on. Its tempo and time signature marks open their change
 * in a popover; a right-click adds one (design-8b §4.4).
 */
const Ruler = memo(function Ruler({
  timeline,
  width,
  loopStart,
  loopEnd,
  looping,
  locked,
  onSeek,
  onLoop,
}: {
  timeline: TimelineView;
  width: number;
  loopStart: number | null;
  loopEnd: number | null;
  looping: boolean;
  locked: string | null;
  onSeek: (beat: number) => void;
  onLoop: (start: number, end: number) => void;
}) {
  const ppb = useZoom();
  // A loop being dragged, shown until it is let go.
  const [drag, setDrag] = useState<{ start: number; end: number } | null>(null);
  const shown = drag ?? (loopStart !== null && loopEnd !== null ? { start: loopStart, end: loopEnd } : null);
  const on = drag ? true : looping;
  const session = useStore((s) => s.graph?.session);
  // The change whose popover is open (by its beat; `typing` when just made), and the right-click menu.
  const [mark, setMark] = useState<{ beat: number; typing: boolean } | null>(null);
  const [menu, setMenu] = useState<{ beat: number; x: number } | null>(null);
  const [menuAt, setMenuAt] = useState<HTMLElement | null>(null);
  const [markAt, setMarkAt] = useState<HTMLElement | null>(null);
  const marks = changes(timeline);
  // A change undone or removed elsewhere closes its popover.
  const open = mark && marks.some((c) => c.beat === mark.beat) ? mark : null;
  const ruled = bars(timeline.meter, width / ppb);
  // Where the tempo or the time signature changes: both at one beat share a mark, which opens them.
  const chip = (c: (typeof marks)[number]) => (
    <span
      ref={open?.beat === c.beat ? setMarkAt : undefined}
      role="button"
      data-mark={c.beat}
      aria-label={`Changes from ${placeOf(timeline.meter, c.beat)}`}
      aria-expanded={open?.beat === c.beat}
      className={clsx(
        "pointer-events-auto cursor-pointer px-0.5 -mx-0.5 rounded-xs text-xs leading-none whitespace-nowrap transition-colors duration-80 ease-linear",
        open?.beat === c.beat ? "text-fg bg-hover" : "text-fg-muted hover:text-fg hover:bg-hover",
      )}
      title={[
        c.meter && `${c.meter} from ${placeOf(timeline.meter, c.beat)}`,
        c.bpm && `${c.bpm} BPM from ${placeOf(timeline.meter, c.beat)}`,
        locked ?? "Click to change or remove",
      ]
        .filter(Boolean)
        .join("\n")}
      // Not a seek or a loop: the ruler's own drag starts under it.
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        setMark(open?.beat === c.beat ? null : { beat: c.beat, typing: false });
      }}
    >
      {[c.meter, c.bpm && `♩${c.bpm}`].filter(Boolean).join(" ")}
    </span>
  );
  return (
    <div
      data-testid="ruler"
      className="relative overflow-hidden bg-chrome border-b border-line cursor-pointer touch-none"
      style={{ width }}
      title={`Click to move the playhead · drag across to loop${locked ? "" : " · right-click to change the tempo or time signature here, or add a marker"}`}
      onContextMenu={(e) => {
        e.preventDefault();
        // Its popovers are portals, whose events reach the ruler through React: only its own.
        if (locked || session === undefined || !e.currentTarget.contains(e.target as Node)) return;
        const x = e.clientX - e.currentTarget.getBoundingClientRect().left;
        setMenu({ beat: Math.max(0, Math.round(x / ppb)), x });
      }}
      onPointerDown={(e) => {
        if (e.button !== 0 || !e.currentTarget.contains(e.target as Node)) return;
        const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX;
        const left = el.getBoundingClientRect().left;
        const beatAt = (x: number) => Math.max(0, Math.round((x - left) / ppb));
        el.setPointerCapture(pointer);
        let range: { start: number; end: number } | null = null;
        const move = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer || (range === null && Math.abs(ev.clientX - x0) < 4)) return;
          const a = beatAt(x0), b = beatAt(ev.clientX);
          range = { start: Math.min(a, b), end: Math.max(a, b) };
          setDrag(range);
        };
        const end = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer) return;
          for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
          setDrag(null);
          if (ev.type !== "pointerup") return;
          if (range === null) onSeek(beatAt(x0));
          else if (range.end > range.start) onLoop(range.start, range.end);
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      }}
    >
      {shown && (
        <div
          data-testid="loop-range"
          data-start={shown.start}
          data-end={shown.end}
          data-on={on}
          aria-hidden
          // A brace along the ruler's foot: lit while it loops, a grey bar while it is only set.
          className={clsx("absolute bottom-0 h-1.5 z-[1] pointer-events-none", on ? "bg-accent-range border-x border-accent" : "bg-line-strong")}
          style={{ left: shown.start * ppb, width: (shown.end - shown.start) * ppb }}
        />
      )}
      {/* Bar lines down the ruler, and a tick at each beat along its foot. */}
      {meterSpans(timeline.meter, 0, width / ppb).map((s) => (
        <div key={s.start} aria-hidden className="absolute inset-y-0 pointer-events-none" style={{ left: s.start * ppb, width: (s.end - s.start) * ppb }}>
          <VLines layers={[{ every: s.bar * ppb, color: "var(--color-line-strong)" }]} />
          {ppb >= BEAT_LINES && <VLines className="inset-x-0 bottom-0 h-[5px]" layers={[{ every: ppb, color: "var(--color-line-strong)" }]} />}
        </div>
      ))}
      {/*
        The bar numbers on the line under the markers' band (next D4a), and each tempo or time
        signature change on that line: after its bar's number when it is at a bar line, 6 px on,
        else 3 px after its beat. Bars under 40 px are numbered every second, fourth or eighth
        (taste.md §3.2).
      */}
      {ruled.map((b) => {
        const w = b.length * ppb;
        const every = w >= 40 ? 1 : w >= 20 ? 2 : w >= 10 ? 4 : 8;
        // None under the ruler's tools at its very end, where no scroll can bring it out.
        const numbered = b.bar % every === 0 && b.start * ppb + 6 < width - TOOLS_W - 16;
        const change = marks.find((c) => Math.abs(c.beat - b.start) < 1e-6);
        return numbered || change ? (
          <div key={b.bar} className="absolute top-[17px] flex items-center gap-1.5 pointer-events-none" style={{ left: b.start * ppb + 6 }}>
            {numbered && (
              <span data-bar={b.bar + 1} className="text-xs leading-none font-medium text-fg-faint">
                {b.bar + 1}
              </span>
            )}
            {change && chip(change)}
          </div>
        ) : null;
      })}
      <RulerMarkers timeline={timeline} ppb={ppb} width={width} locked={locked} onSeek={onSeek} onLoop={onLoop} />
      {marks
        .filter((c) => !ruled.some((b) => Math.abs(c.beat - b.start) < 1e-6))
        .map((c) => (
          <div key={c.beat} className="absolute top-[17px] flex pointer-events-none" style={{ left: c.beat * ppb + 3 }}>
            {chip(c)}
          </div>
        ))}
      {/* Placed once, at its own mark: never at the one open before. */}
      {open && markAt?.dataset.mark === String(open.beat) && session !== undefined && (
        <MarkPopover
          key={open.beat}
          session={session}
          timeline={timeline}
          beat={open.beat}
          anchor={markAt}
          locked={locked}
          typing={open.typing}
          onClose={() => setMark(null)}
        />
      )}
      {menu && <span ref={setMenuAt} aria-hidden className="absolute bottom-0 w-px h-px pointer-events-none" style={{ left: menu.x }} />}
      {menu && menuAt && session !== undefined && (
        <RulerMenu
          session={session}
          timeline={timeline}
          beat={menu.beat}
          anchor={menuAt}
          locked={locked}
          onClose={() => {
            setMenu(null);
            setMenuAt(null);
          }}
          onMade={(beat) => setMark({ beat, typing: true })}
          onMarker={(beat) => useMarkerEdit.setState({ editing: { beat, from: null } })}
        />
      )}
    </div>
  );
});

/** Every track's rows, then empty grid to the foot. Memoized: the playhead moves without redrawing clips. */
const Tracks = memo(function Tracks({
  timeline,
  rows,
  width,
  visible,
  headerW,
  drag,
  lit,
  issues,
  edits,
  locked,
  waiting,
  onOpenClip,
}: {
  timeline: TimelineView;
  rows: TimelineTrack[];
  width: number;
  /** The lanes' width on screen, to centre a line in. */
  visible: number;
  headerW: number;
  drag: Drag | null;
  /** A lane just made, lit for a moment: `module.lane`. */
  lit: string | null;
  issues: ReadonlyMap<string, Diagnostic[]>;
  edits: ClipEdits;
  locked: string | null;
  waiting: boolean;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const grid = <Grid meter={timeline.meter} width={width} />;
  const said = waiting || rows.length === 0;
  return (
    <div role="tablist" aria-label="Tracks" aria-orientation="vertical" className="contents">
      {rows.map((t) => (
        <TrackRows
          key={t.module}
          track={t}
          color={trackColors(timeline, t.module) ?? GRAPHITE}
          width={width}
          headerW={headerW}
          grid={grid}
          drag={drag && involves(drag, t.module) ? drag : null}
          lit={lit?.startsWith(`${t.module}.`) ? lit.slice(t.module.length + 1) : null}
          issues={issues.get(t.module)}
          edits={edits}
          locked={locked}
          waiting={waiting}
          onOpenClip={onOpenClip}
        />
      ))}
      <div
        className={clsx("relative flex-1 flex", said ? "min-h-24" : "min-h-6")}
        // An instrument dragged from the browser onto the space below the tracks adds a track there (BR-1).
        onDragOver={(e) => {
          if (!e.dataTransfer.types.includes(INSTRUMENT_DRAG) || locked) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = "copy";
        }}
        onDrop={(e) => {
          const component = e.dataTransfer.getData(INSTRUMENT_DRAG);
          const instrument = useStore.getState().graph?.instruments?.find((i) => i.component === component);
          if (!instrument) return;
          e.preventDefault();
          void addTrack(instrument, rows.filter((r) => r.track).at(-1)?.module ?? null);
        }}
      >
        <DropLine before={END} />
        <div className="sticky left-0 z-20 shrink-0 bg-chrome border-r border-line" style={{ width: headerW }} />
        <div className="relative" style={{ width }}>
          {grid}
          {/* Under the last track, in the lanes' visible middle: a line here never pushes a row down (principle 8). */}
          {said && (
            <div className="sticky h-full grid place-items-center pointer-events-none" style={{ left: headerW, width: visible }}>
              {waiting ? (
                <EmptyState data-testid="waiting">Waiting for the code to compile.</EmptyState>
              ) : (
                <EmptyState className="pointer-events-auto" data-testid="no-tracks">
                  <Button
                    variant="primary"
                    data-testid="add-first-track"
                    disabled={!!locked}
                    title={locked ? `${locked}.` : `Add a track (${chord("T")})`}
                    onMouseDown={keepFocus}
                    onClick={(e) => openInstruments(e.currentTarget, null)}
                  >
                    <Plus size={14} />
                    Add a track
                  </Button>
                  <button
                    className="block mx-auto text-sm text-fg-muted hover:text-fg cursor-pointer"
                    onMouseDown={keepFocus}
                    onClick={() => useShell.getState().setView("code")}
                  >
                    or write one in code <span className="text-fg-faint">{opt("4")}</span>
                  </button>
                </EmptyState>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
});

const TrackRows = memo(function TrackRows({
  track,
  color,
  width,
  headerW,
  grid,
  drag,
  lit,
  issues,
  edits,
  locked,
  waiting,
  last,
  onOpenClip,
}: {
  track: TimelineTrack;
  color: TrackColor;
  width: number;
  headerW: number;
  grid: ReactNode;
  /** Clips being dragged from or onto this track's lanes. */
  drag: Drag | null;
  /** The key of a lane of this track just made, lit for a moment. */
  lit: string | null;
  /** What needs attention in this track. */
  issues: Diagnostic[] | undefined;
  edits: ClipEdits;
  locked: string | null;
  waiting: boolean;
  /** The Song's row: no border under it, it sits at the foot. */
  last?: boolean;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const module = track.module;
  const ppb = useZoom();
  const selected = useShell((s) => s.selected === module);
  // Notes outlines the clip it shows.
  const shown = useShell((s) => (s.view === "notes" && s.module === module ? (s.clips[module] ?? null) : null));
  const open = useShell((s) => s.open.has(module));
  // A muted track's clips, or one a solo leaves off, draw faint and grey; muted in the song, hatched too.
  const heard = useHeard(module);
  const { primary, others, controls } = rowsOf(track);
  // Lanes hidden one by one (D4): A never shows them; a line under the others says so.
  const hidden = useHidden(module);
  const hiddenHere = controls.filter((l) => hidden.has(l.key)).map((l) => l.key);
  // Its rows' heights (Q10): the Song's row, pinned at the foot, keeps the usual ones.
  const heights = useHeights();
  const h = last ? ROW_H : trackHeight(heights, module);
  const laneH = (l: TimelineLane) => (last ? LANE_H : laneHeight(heights, module, l.key));
  /** Where a reference's clip is in `lane`, if it is one of its clips. */
  const at = (ref: ClipRef | null, lane: TimelineLane) => (ref?.lane === lane.key ? clipIndex(lane.clips, ref) : -1);
  const select = () => useShell.getState().select(module);

  /** A row: its header (sticky at the left) and its lane. */
  const row = (lane: TimelineLane | null, header: ReactNode, opts: { first?: boolean; height: number; movable?: boolean; edge?: string }) => (
    <div
      key={lane?.key ?? ""}
      data-lane={lane ? `${module}.${lane.key}` : undefined}
      className={clsx("flex", lane && lane.key === lit && "lit")}
      style={{ height: opts.height }}
    >
      <div
        {...(opts.first
          ? // The track's header selects it, as the module tabs did: what the dock shows follows it (§3.7).
            {
                "data-module-tab": module,
                role: "tab",
                "aria-selected": selected,
                "aria-controls": "dock",
                title: opts.movable
                  ? `${moduleName({ key: module, label: track.label })}: click to show it in the dock below · drag to move it · right-click for more`
                  : "Song: click to show what the whole song plays through",
              }
          : {})}
        className={clsx(
          "group/head sticky left-0 z-20 shrink-0 flex items-center border-r border-line transition-colors duration-80 ease-linear",
          selected ? "bg-accent-soft" : clsx("bg-chrome", !locked && "group-hover:bg-hover"),
        )}
        style={{ width: headerW }}
        // A track's own header: drag it to move the track, right-click it for its menu.
        onPointerDown={opts.movable ? (e) => startReorder(e, module) : undefined}
        onContextMenu={
          opts.movable
            ? (e) => {
                e.preventDefault();
                const dots = e.currentTarget.querySelector<HTMLElement>("[data-track-menu]");
                if (dots) openTrackMenu(module, dots);
              }
            : // An automation lane's header: Remove automation, Hide automation.
              lane?.kind === "control" && !waiting
              ? (e) => edits.menu(e, module, lane.key, 0, "header")
              : undefined
        }
      >
        {/* The colour bar runs down every row of the track, at 40% beside its lanes. */}
        <div className="absolute left-0 inset-y-0 w-1" style={{ background: color.bar, opacity: opts.first ? 1 : 0.4 }} />
        {/* The selected track's edge where it meets its lanes. */}
        {selected && <div className="absolute -right-px inset-y-0 w-0.5 bg-accent" />}
        {header}
        {opts.edge && <HeightEdge id={opts.edge} height={opts.height} name={opts.first ? moduleName({ key: module, label: track.label }) : laneTitle(lane!)} />}
      </div>
      <div
        // The lane dragged clips would land on is lit; one that can't take them isn't (design-8b §7.5).
        className={clsx("relative overflow-hidden", !opts.first && "border-t border-hover", (selected || (lane && drag?.to?.module === module && drag.to.lane === lane.key)) && "bg-accent-wash")}
        style={{ width }}
        title={!lane || locked ? undefined : lane.kind === "audio" ? `Drop WAV files here, or use File ▸ Import Audio (${chord("I", { shift: true })})` : "Double-click to add a one-bar clip"}
        onPointerDown={(e) => {
          const empty = !(e.target as HTMLElement).closest("[data-clip]");
          if (empty) edits.unpick(module);
          edits.press(empty ? e : null);
        }}
        onPointerUp={(e) => edits.place(e, (e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb)}
        onDoubleClick={(e) => {
          if (!lane || lane.kind === "audio" || (e.target as HTMLElement).closest("[data-clip]")) return;
          edits.create(module, lane.key, (e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb);
        }}
        // A right-click on a clip opens the clip's menu (its own handler); on empty space, the lane's.
        onContextMenu={(e) => {
          if (!lane || waiting || (e.target as HTMLElement).closest("[data-clip]")) return;
          edits.menu(e, module, lane.key, (e.clientX - e.currentTarget.getBoundingClientRect().left) / ppb);
        }}
      >
        {grid}
        {lane && (
          <div className={clsx("absolute inset-0", waiting ? "opacity-50 grayscale" : heard !== "heard" && "opacity-40 grayscale-60")}>
            <Hatched.Provider value={heard === "muted-in-song"}>
              <LaneClips
                module={module}
                lane={lane}
                height={opts.height}
                color={color}
                shown={at(shown, lane)}
                drag={drag}
                editable={!locked}
                onOpen={(k) => {
                  onOpenClip(clipRef(module, lane.key, lane.clips, k));
                  // A double-click is to edit the notes: the roll takes the keyboard.
                  requestAnimationFrame(() => document.querySelector<HTMLElement>("[data-testid=notes]:not([data-empty])")?.focus({ preventScroll: true }));
                }}
                onDown={(ref, e) => edits.down(ref, e)}
                onMenu={(ref, e) => !waiting && edits.menu(e, module, lane.key, (e.clientX - e.currentTarget.parentElement!.getBoundingClientRect().left) / ppb, ref)}
              />
            </Hatched.Provider>
          </div>
        )}
      </div>
    </div>
  );

  // Dragged to a new place: it follows the pointer, lifted (taste.md §3.4).
  const dy = useReorder((s) => (s.module === module ? s.dy : null));
  const movable = track.track && !last;
  return (
    <div
      data-track={module}
      data-selected={selected || undefined}
      className={clsx("group relative", !last && "border-b border-line", dy !== null && "z-30 shadow-lift")}
      style={dy !== null ? { transform: `translateY(${dy}px)` } : undefined}
      onPointerDown={select}
      // An effect dragged from the browser goes at the end of this track's sound.
      onDragOver={(e) => {
        if (!dragsEffect(e) || locked) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
      }}
      onDrop={(e) => {
        const component = e.dataTransfer.getData(EFFECT_DRAG);
        if (!component) return;
        e.preventDefault();
        select();
        addEffectTo(module, component);
      }}
    >
      {movable && <DropLine before={module} />}
      {row(
        primary,
        <TrackHeader track={track} folds={controls.length > 0} open={open} selected={selected} song={!!last} issues={issues} height={h} />,
        { first: true, height: h, movable, edge: track.track && !last ? module : undefined },
      )}
      {others.map((l) => row(l, <LaneLabel lane={l} height={h} />, { height: h }))}
      {open &&
        controls
          .filter((l) => !hidden.has(l.key))
          .map((l) => row(l, <LaneLabel lane={l} height={laneH(l)} />, { height: laneH(l), edge: last ? undefined : `${module}.${l.key}` }))}
      {open && hiddenHere.length > 0 && <HiddenRow module={module} lanes={hiddenHere} color={color} selected={selected} headerW={headerW} width={width} />}
    </div>
  );
});

/** Where a dragged header would land: a 2px accent line on the top edge of the track it goes before. */
function DropLine({ before }: { before: string }) {
  const here = useReorder((s) => s.module !== null && s.before === before);
  return here ? <div aria-hidden data-testid="drop-line" className="absolute -top-px inset-x-0 h-0.5 z-40 bg-accent pointer-events-none" /> : null;
}

/**
 * A lane's header (taste.md §3.4): its control's label at x = 28; for automation, the value at the
 * playhead on the right in `fg-faint`, with the lane's marker while a clip is what sets it.
 */
function LaneLabel({ lane, height }: { lane: TimelineLane; height: number }) {
  const name = laneTitle(lane);
  // In a taller row, at the top, where it sits at the usual height.
  return (
    <div className="absolute inset-x-0 top-0 flex items-center" style={{ height: Math.min(height, lane.kind === "control" ? LANE_H : ROW_H) }}>
      <span className="absolute left-7 right-[64px] text-sm text-fg-muted truncate" title={name === lane.key ? lane.key : `${name} (${lane.key})`}>
        {name}
      </span>
      {lane.kind === "control" && <LaneValue lane={lane} name={name} />}
    </div>
  );
}

/**
 * A row's bottom edge, on its header (Q10): drag it for a height in 4 px steps, double-click it for
 * the usual one. `id` is the track's key, or an automation lane's `module.lane`.
 */
function HeightEdge({ id, height, name }: { id: string; height: number; name: string }) {
  const lane = id.includes(".");
  const range = lane ? LANE_HS : TRACK_H;
  return (
    <div
      data-height-edge={id}
      aria-hidden
      title={`Drag to change ${name}'s height · double-click for the usual height`}
      className="absolute inset-x-0 bottom-0 h-1.5 z-10 cursor-row-resize"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        // Not a reorder of the track, nor a selection.
        e.stopPropagation();
        e.preventDefault();
        const el = e.currentTarget, pointer = e.pointerId, y0 = e.clientY;
        el.setPointerCapture(pointer);
        const move = (ev: PointerEvent) => {
          if (ev.pointerId === pointer) setHeights({ [id]: snapHeight(height + ev.clientY - y0, range) });
        };
        const end = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer) return;
          for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      }}
      onDoubleClick={(e) => {
        e.stopPropagation();
        setHeights({ [id]: null });
      }}
    />
  );
}

/** The value a control lane plays at the playhead, redrawn each frame it changes. */
function LaneValue({ lane, name }: { lane: Extract<TimelineLane, { kind: "control" }>; name: string }) {
  const text = useRef<HTMLSpanElement>(null);
  const dot = useRef<HTMLSpanElement>(null);
  useEffect(
    () =>
      onFrame((beat) => {
        const { value, follows } = laneValue(lane, beat);
        const t = text.current, d = dot.current;
        if (!t || !d) return;
        const shown = value === null ? "" : formatValue(lane.unit, value, lane.key);
        if (t.textContent !== shown) t.textContent = shown;
        d.style.visibility = follows ? "visible" : "hidden";
      }),
    [lane],
  );
  return (
    <>
      <span ref={text} className="absolute right-[18px] text-xs text-fg-faint" title={`${name} at the playhead`} />
      {/* Its home is the lane while a clip sets it (HOME-1); between clips it plays the fallback. */}
      <span ref={dot} className="absolute right-2 size-[5px] rounded-full bg-accent" title={`Follows the ${name} lane.`} />
    </>
  );
}

/**
 * The menu on a clip (Cut, Copy, Paste, Duplicate, Split, Join, Quantize, Delete, Loop, Open in
 * Notes), acting on the selection the right-click left; or on a lane's empty space (Add clip, Paste
 * here, Hide automation). Each row with its key (design-8b §7.5).
 */
function TimelineMenu({
  at,
  timeline,
  edits,
  locked,
  onOpenClip,
  onRepeat,
  onClose,
}: {
  at: MenuAt;
  timeline: TimelineView;
  edits: ClipEdits;
  locked: string | null;
  onOpenClip: (clip: ClipRef) => void;
  /** Repeat…: its popover opens where the menu was. */
  onRepeat: (at: { x: number; y: number }) => void;
  onClose: () => void;
}) {
  const picked = pickedIn(timeline, useClipSelection((s) => s.picked));
  const copied = useClipboard((s) => s.copied.length > 0);
  const grid = useGrid((s) => s.notes);
  const lane = findLane(timeline, at.module, at.lane);
  if (!lane) return null;
  const off = locked ? `${locked}.` : null;
  const none = off ?? (copied ? null : "Copy or cut a clip first.");
  let items: (MenuItem | "line" | null)[];
  if (at.on === "header") {
    items = [
      { id: "unautomate", label: "Remove automation", off: off ?? (lane.kind === "control" ? null : "Only an automation lane can be removed."), run: () => edits.unautomate(at.module, at.lane) },
      // One lane out of sight, kept so with the song (D4); a line under the track's lanes brings it back.
      lane.kind === "control" ? { id: "hide-lane", label: "Hide this lane", run: () => hideLane(at.module, at.lane) } : null,
      { id: "hide", label: "Hide automation", shortcut: "A", run: () => useShell.getState().toggleLanes(at.module, []) },
    ];
  } else if (at.on === "clip") {
    const beat = playhead();
    const inside = picked.some((p) => p.clip.start < beat && beat < p.clip.start + p.clip.length);
    const notes = picked.filter((p) => p.lane.kind === "notes");
    const oneLane = notes.every((p) => p.ref.module === notes[0].ref.module && p.ref.lane === notes[0].ref.lane);
    const these = picked.length === 1 ? "this clip" : "these clips";
    items = [
      { id: "cut", label: "Cut", shortcut: chord("X"), off, run: () => edits.copy(true) },
      { id: "copy", label: "Copy", shortcut: chord("C"), run: () => edits.copy(false) },
      { id: "paste", label: "Paste", shortcut: chord("V"), off: none, run: () => edits.paste() },
      { id: "duplicate", label: "Duplicate", shortcut: chord("D"), off, run: () => void edits.duplicate() },
      { id: "repeat", label: "Repeat…", off, run: () => onRepeat({ x: at.x, y: at.y }) },
      picked.length === 1 && picked[0].lane.kind === "notes" ? { id: "export-midi", label: "Export as MIDI…", run: () => void exportMidi(picked[0].ref) } : null,
      "line",
      { id: "split", label: "Split at playhead", shortcut: chord("E"), off: off ?? (inside ? null : `The playhead (${position(timeline.meter, beat)}) isn't inside ${these}.`), run: edits.split },
      ...audioRows(picked, { x: at.x, y: at.y }, off),
      notes.length > 1 ? { id: "join", label: "Join", shortcut: chord("J"), off: off ?? (oneLane && notes.length === picked.length ? null : "Only notes clips of one lane join."), run: edits.join } : null,
      notes.length ? { id: "quantize", label: `Quantize to ${pickName(grid.pick, grid.triplet)}`, shortcut: "Q", off, run: edits.quantize } : null,
      { id: "delete", label: "Delete", shortcut: keyName("Backspace"), off, run: edits.remove },
      "line",
      { id: "loop", label: picked.length > 1 ? "Loop these clips" : "Loop this clip", shortcut: chord("L"), run: edits.loop },
      picked.length === 1 && picked[0].lane.kind === "notes" ? { id: "open", label: "Open in Notes", run: () => onOpenClip(picked[0].ref) } : null,
    ];
  } else {
    const taken = lane.clips.some((c: AnyClip) => c.start <= at.beat && at.beat < c.start + c.length);
    items = [
      lane.kind !== "audio" ? { id: "add", label: "Add clip", off: off ?? (taken ? "A clip is here already." : null), run: () => edits.create(at.module, at.lane, at.beat) } : null,
      { id: "paste", label: "Paste here", off: none, run: () => edits.paste(snapSong(timeline.meter, at.beat, zoomNow()), { module: at.module, lane: at.lane }) },
      lane.kind === "control" ? { id: "hide", label: "Hide automation", shortcut: "A", run: () => useShell.getState().toggleLanes(at.module, []) } : null,
    ];
  }
  return <PointMenu at={at} label={at.on === "clip" ? "Clip menu" : `${laneTitle(lane)} lane menu`} items={items} onClose={onClose} />;
}

/**
 * Repeat… (Q12): "Repeat [3] more times" and [Repeat], at the clip menu's place. Return repeats; Esc
 * closes. 1 to 64.
 */
function RepeatPopover({ at, onRepeat, onClose }: { at: { x: number; y: number }; onRepeat: (times: number) => void; onClose: () => void }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [draft, setDraft] = useState("3");
  const input = useRef<HTMLInputElement>(null);
  const n = Number(draft);
  const ok = Number.isInteger(n) && n >= 1 && n <= REPEATS;
  useEffect(() => input.current?.select(), [anchor]);
  const go = () => {
    if (!ok) return;
    onClose();
    onRepeat(n);
  };
  return (
    <>
      {createPortal(<span ref={setAnchor} aria-hidden className="fixed size-px pointer-events-none" style={{ left: at.x, top: at.y - 4 }} />, document.body)}
      {anchor && (
        <Popover anchor={anchor} onClose={onClose} data-testid="repeat-popover" aria-label="Repeat" className="w-[260px]">
          <div className="px-2 py-1.5 flex items-center gap-2 text-sm text-fg">
            <label htmlFor="repeat-times">Repeat</label>
            <input
              ref={input}
              id="repeat-times"
              data-testid="repeat-times"
              type="number"
              min={1}
              max={REPEATS}
              value={draft}
              autoFocus
              aria-invalid={!ok}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                // Its keys are its own: never the timeline's (⌫ deletes clips there).
                e.stopPropagation();
                if (e.key === "Enter") go();
              }}
              className={clsx("w-14 h-ctl px-2 rounded-sm bg-raised border text-sm text-fg tabular-nums outline-none", ok ? "border-line-strong focus:border-accent" : "border-danger")}
            />
            <span>more times</span>
          </div>
          {!ok && <div role="alert" className="px-2 pb-1 text-xs text-danger">Repeat 1 to {REPEATS} times</div>}
          <div className="px-2 pt-1 pb-1.5 flex justify-end">
            <Button variant="primary" size="sm" data-testid="repeat-go" disabled={!ok} onClick={go}>
              Repeat
            </Button>
          </div>
        </Popover>
      )}
    </>
  );
}

/** The most copies Repeat… makes (Q12), as the server takes them. */
const REPEATS = 64;

/** Whether a drag moves a clip of track `module`, or onto one of its lanes. */
const involves = (d: Drag, module: string) => d.to?.module === module || d.clips.some((c) => c.ref.module === module);

function LaneClips({
  module,
  lane,
  height,
  color,
  shown,
  drag,
  editable,
  onOpen,
  onDown,
  onMenu,
}: {
  module: string;
  lane: TimelineLane;
  /** Its row's height. */
  height: number;
  color: TrackColor;
  /** Where the clip Notes shows is in the lane's clips, or -1. */
  shown: number;
  /** Clips being dragged from or onto this track's lanes. */
  drag: Drag | null;
  editable: boolean;
  onOpen: (index: number) => void;
  onDown: (ref: ClipRef, e: React.PointerEvent<HTMLElement>) => void;
  onMenu: (ref: ClipRef, e: React.MouseEvent<HTMLElement>) => void;
}) {
  const picked = useClipSelection((s) => s.picked);
  // An automation point picked, and points being edited: drawn in place of the clip's own.
  const point = usePoints((s) => (s.picked && s.picked.clip.module === module && s.picked.clip.lane === lane.key ? s.picked : null));
  const editing = usePoints((s) => (s.editing && s.editing.clip.module === module && s.editing.clip.lane === lane.key ? s.editing : null));
  // Draw mode (B) paints on automation clips: the pointer says so there.
  const painting = useShell((s) => s.draw) && lane.kind === "control" && editable;
  // A clip is named by its id; one without its own (FMT-12) only by where it is.
  const id = (i: number) => (hasOwnId(lane.clips, i) ? lane.clips[i].id! : "");
  const key = (i: number) => id(i) || `#${i}`;
  const here = (a: LaneAddr) => a.module === module && a.lane === lane.key;

  // What a clip of this lane's kind draws inside it.
  // 0 at the bottom of a clip's drawing, 1 at its top, on the lane's scale (D3).
  const scale = lane.kind === "control" ? laneScale(lane) : null;
  const level = (v: number) => scale?.level(v) ?? 0;
  const inside = (c: AnyClip, tone: TrackColor, pickedPoint = -1, wave?: WaveEdit) =>
    "notes" in c ? (
      <Notes notes={audible(c)} length={c.length} color={tone} />
    ) : "points" in c ? (
      <Curve clip={c} level={level} path={scale?.path ?? line} color={tone} picked={pickedPoint} />
    ) : (
      <Wave clip={c} color={tone} edit={wave} />
    );
  const words = (c: AnyClip) => {
    const how = editable ? `drag to move · ${mod.alt}-drag to copy · drag an edge to resize · Delete removes` : "";
    if ("notes" in c) return `${audible(c).length} notes, ${span(c)}\nDouble-click to edit the notes${how ? ` · ${how}` : ""}`;
    if ("points" in c) return `${span(c)}${editable ? `\nClick the line to add a point · drag a point to move it · B to draw · drag elsewhere to move the clip · ${mod.alt}-drag to copy` : ""}`;
    return `${audioLabel(c)}, ${span(c)}${how ? `\n${how[0].toUpperCase()}${how.slice(1)} · drag the top corners to fade` : ""}`;
  };

  // The lane's own clips: one being moved away (not copied) isn't drawn here; one moved along this
  // lane, or resized, is drawn where it goes.
  const own = lane.clips.map((c: AnyClip, i) => {
    const ref = clipRef(module, lane.key, lane.clips, i);
    // Drawn from the clip as the drag found it, so the timeline showing the edit before the drag
    // is cleared draws it in the same place, not moved twice.
    const d = drag?.clips.find((x) => sameClip(x.ref, ref));
    let drawn: AnyClip = c;
    let hidden = false;
    if (editing && "points" in c && sameClip(editing.clip, ref)) drawn = { ...c, points: editing.points };
    if (d && drag) {
      if (drag.kind === "move" && !drag.copy) {
        // Gone to another lane: hidden here, but kept, so the drag keeps the pointer it captured.
        if (drag.to) hidden = true;
        else drawn = { ...d.clip, start: clean(d.clip.start + drag.by) };
      } else if (drag.kind !== "move") drawn = reshaped(d.clip, drag.start, drag.length);
    }
    const lifted = !!d && !!drag && (drag.kind !== "move" || !drag.copy);
    return (
      <Clip
        key={key(i)}
        id={id(i)}
        clip={drawn}
        color={color}
        title={words(c)}
        open={i === shown}
        picked={isPicked(picked, ref)}
        dragged={lifted}
        hidden={hidden}
        editable={editable}
        painting={painting}
        onDoubleClick={lane.kind === "notes" ? () => onOpen(i) : undefined}
        onPointerDown={(e) => onDown(ref, e)}
        onContextMenu={(e) => onMenu(ref, e)}
      >
        {inside(
          drawn,
          color,
          point && sameClip(point.clip, ref) && "points" in drawn ? pointIn(point, drawn.points, !!editing && sameClip(editing.clip, ref)) : -1,
          lane.kind === "audio" ? { ref, lane, editable, picked: isPicked(picked, ref) } : undefined,
        )}
      </Clip>
    );
  });
  // Copies, and clips moved here from another lane: drawn where they land, in their own colour.
  const ghosts =
    drag?.kind === "move" && (drag.copy || drag.to)
      ? drag.clips
          .filter((d) => (drag.to ? here(drag.to) : here(d.ref)))
          .map((d) => {
            const c = { ...d.clip, start: clean(d.clip.start + drag.by) };
            return (
              <Clip key={`ghost:${d.ref.module}.${d.ref.lane}:${d.ref.id}`} ghost clip={c} color={d.color} title="" dragged>
                {inside(c, d.color)}
              </Clip>
            );
          })
      : [];
  return (
    <>
      {lane.kind === "control" && lane.default != null && (
        <Fallback clips={lane.clips} y={CLIP_INSET + BORDER + BODY_TOP + (1 - level(lane.default)) * (height - 2 * (CLIP_INSET + BORDER) - BODY_TOP - BODY_BOTTOM)} />
      )}
      {own}
      {ghosts}
      {(lane.kind === "notes" || lane.kind === "audio") && <Take module={module} lane={lane.key} color={color} />}
    </>
  );
}

function Clip({
  id,
  ghost,
  hidden,
  clip,
  color,
  title,
  open,
  picked,
  dragged,
  editable,
  painting,
  onDoubleClick,
  onPointerDown,
  onContextMenu,
  children,
}: {
  /** Its id, or "" for a clip without one of its own. */
  id?: string;
  /** A copy or a moved clip drawn where it lands, under the pointer: not a clip yet. */
  ghost?: boolean;
  /** Moved to another lane while dragged: not drawn here. */
  hidden?: boolean;
  clip: { start: number; length: number };
  color: TrackColor;
  title: string;
  /** The clip Notes shows. */
  open?: boolean;
  /** Selected: ⌫ deletes it, a drag moves it with the others. */
  picked?: boolean;
  dragged?: boolean;
  editable?: boolean;
  /** Draw mode on an automation clip: a drag paints its points. */
  painting?: boolean;
  onDoubleClick?: () => void;
  /** Given, the clip can be selected (and, editable, dragged by its body or either edge). */
  onPointerDown?: (e: React.PointerEvent<HTMLElement>) => void;
  onContextMenu?: (e: React.MouseEvent<HTMLElement>) => void;
  children: ReactNode;
}) {
  const hatched = useContext(Hatched);
  const ppb = useZoom();
  const handles = !!onPointerDown && editable;
  return (
    <div
      data-clip={ghost ? undefined : id}
      data-ghost={ghost || undefined}
      data-start={clip.start}
      data-length={clip.length}
      data-open={open || undefined}
      data-picked={picked || undefined}
      title={title || undefined}
      // Its body is the tint, its border the bar; the selected clip is the strongest outline on screen.
      className={clsx(
        "absolute rounded-sm overflow-hidden touch-none border border-(--clip-bar)",
        picked && "outline-2 outline-fg-strong",
        ghost && "pointer-events-none",
        hidden && "invisible",
        dragged ? "z-10 shadow-lift opacity-90" : handles && "hover:border-(--clip-ink)",
        handles && (painting ? "cursor-crosshair" : dragged ? "cursor-grabbing" : "cursor-grab"),
      )}
      onDoubleClick={onDoubleClick}
      onPointerDown={onPointerDown}
      onContextMenu={onContextMenu}
      style={
        {
          top: CLIP_INSET,
          bottom: CLIP_INSET,
          left: clip.start * ppb + 1,
          width: Math.max(clip.length * ppb - 2, 4),
          background: picked ? `color-mix(in oklab, ${color.bar} 35%, ${color.tint})` : color.tint,
          "--clip-bar": color.bar,
          "--clip-ink": color.ink,
        } as React.CSSProperties
      }
    >
      {children}
      {hatched && <Hatch />}
      {/* Either edge resizes: 6 px each (design-8b §7.5). */}
      {handles && (
        <>
          <span data-edge="left" aria-hidden className="absolute inset-y-0 left-0 w-1.5 cursor-ew-resize" />
          <span data-edge="right" aria-hidden className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize" />
        </>
      )}
    </div>
  );
}

/** The clip's own drawing area: beats across, 0..100 down. `curve`: an automation clip's, where a press edits points. */
function Body({ length, curve, children }: { length: number; curve?: boolean; children: ReactNode }) {
  return (
    <svg
      data-curve={curve || undefined}
      className="absolute left-0 w-full"
      style={{ top: BODY_TOP, height: `calc(100% - ${BODY_TOP + BODY_BOTTOM}px)` }}
      viewBox={`0 0 ${length} 100`}
      preserveAspectRatio="none"
    >
      {children}
    </svg>
  );
}

/** A mini piano roll of a clip's audible notes, over their own pitch range. */
export function Notes({ notes, length, color }: { notes: NoteClip["notes"]; length: number; color: TrackColor }) {
  const ppb = useZoom();
  if (notes.length === 0) return null;
  const lo = Math.min(...notes.map((n) => n.pitch));
  const hi = Math.max(...notes.map((n) => n.pitch));
  const rows = hi - lo + 1;
  // Few pitches stay thin and centered rather than filling the clip.
  const h = Math.min(100 / rows, 16);
  const top = (100 - rows * h) / 2;
  return (
    <Body length={length}>
      {notes.map((n, i) => (
        <rect
          key={i}
          x={n.start}
          width={Math.max(n.length, 2 / ppb)}
          y={top + (hi - n.pitch) * h + h * 0.1}
          height={h * 0.8}
          fill={color.ink}
          fillOpacity={0.35 + 0.65 * n.velocity}
        />
      ))}
    </Body>
  );
}

/**
 * The automation curve, its points marked; values hold before the first point and after the last.
 * The point `picked` is `fg-strong` in a 2px white ring (taste.md §2.8).
 */
function Curve({
  clip,
  level,
  path,
  color,
  picked,
}: {
  clip: ControlClip;
  level: (v: number) => number;
  /** The line through the points, as the lane's scale draws it (D3). */
  path: LaneScale["path"];
  color: TrackColor;
  picked: number;
}) {
  const { points } = clip;
  if (points.length === 0) return <Body length={clip.length} curve>{null}</Body>;
  const drawn = path(points, clip.length);
  const xy = drawn.map(([b, v]) => `${b},${(1 - level(v)) * 100}`).join(" ");
  return (
    <>
      <Body length={clip.length} curve>
        <polygon points={`${xy} ${drawn[drawn.length - 1][0]},100 ${drawn[0][0]},100`} fill={color.ink} fillOpacity={0.1} />
        <polyline points={xy} fill="none" stroke={color.ink} strokeWidth={1.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      </Body>
      {/* Circles stay round: drawn over the stretched drawing, not in it. */}
      <div aria-hidden className="absolute left-0 w-full pointer-events-none" style={{ top: BODY_TOP, height: `calc(100% - ${BODY_TOP + BODY_BOTTOM}px)` }}>
        {points.map(([b, v], i) => (
          <span
            key={i}
            data-point={i}
            data-picked={i === picked || undefined}
            className={clsx("absolute size-1.5 -ml-[3px] -mt-[3px] rounded-full", i === picked && "bg-fg-strong ring-2 ring-white")}
            style={{ left: `${(b / clip.length) * 100}%`, top: `${(1 - level(v)) * 100}%`, background: i === picked ? undefined : color.ink }}
          />
        ))}
      </div>
    </>
  );
}

/** A control lane's value outside its clips: a dashed line where no clip is, at `y` in the lane. */
function Fallback({ clips, y }: { clips: ControlClip[]; y: number }) {
  const ppb = useZoom();
  const gaps: [number, number][] = [];
  let at = 0;
  for (const c of [...clips].sort((a, b) => a.start - b.start)) {
    if (c.start > at) gaps.push([at, c.start]);
    at = Math.max(at, c.start + c.length);
  }
  gaps.push([at, Infinity]);
  const top = Math.round(y) + 0.5;
  return (
    <svg aria-hidden className="absolute inset-0 pointer-events-none" width="100%" height="100%">
      {gaps.map(([a, b]) => (
        <line key={a} x1={a * ppb} x2={b === Infinity ? "100%" : b * ppb} y1={top} y2={top} stroke="var(--color-line-strong)" strokeDasharray="4 4" />
      ))}
    </svg>
  );
}

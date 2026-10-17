import clsx from "clsx";
import { AudioWaveform, ChevronRight, Plus } from "lucide-react";
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import {
  barAt,
  bars,
  clipIndex,
  clipRef,
  findLane,
  findNoteClip,
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
  type Meter,
  type NoteClip,
  type TimelineLane,
  type TimelineTrack,
  type TimelineView,
} from "../lib/graph";
import { onFrame } from "../lib/clock";
import { LANE_MADE, TRACK_MADE } from "../lib/events";
import { useHeard } from "../lib/graph8b";
import { LANE_H, ROW_H, RULER_H } from "../lib/layout";
import { GRAPHITE, trackColors, type TrackColor } from "../lib/palette";
import { canvas, useShell } from "../lib/shell";
import { busy, isPicked, refIn, send, serverRef, useClipSelection, type Gesture, type LaneAddr } from "../lib/clips";
import { clean, gridOp, skipsSnap, snapSong, stepOf, useGrid, type Grid as GridSetting } from "../lib/grid";
import { chord, mod, opt } from "../lib/native";
import { useStore } from "../lib/store";
import { addEffectTo, dragsEffect, EFFECT_DRAG } from "./Browser";
import { addTrack, END, INSTRUMENT_DRAG, openInstruments, openTrackMenu, startReorder, useReorder } from "./Tracks";
import { MarkPopover, placeOf, RulerMenu } from "./SongBar";
import { Hatch, VLines } from "./draw";
import GridChip from "./GridChip";
import TrackHeader from "./TrackHeader";
import { Button, EmptyState, keepFocus } from "./ui";

/** Pixels per beat. */
const PPB = 24;
/** A clip sits this far inside its row, inside its 1px border; its drawing this far inside that. */
const CLIP_INSET = 4;
const BORDER = 1;
const BODY_TOP = 4;
const BODY_BOTTOM = 4;

const beats = (n: number) => Number(n.toFixed(3));
const span = (c: { start: number; length: number }) => `beats ${beats(c.start)}–${beats(c.start + c.length)}`;
const rowHeight = (lane: TimelineLane) => (lane.kind === "control" ? LANE_H : ROW_H);
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
  /** A double-click on empty space in a notes lane (module and lane keys), at `beat`. */
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
  return { ...c, start, length };
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
  useEffect(
    () =>
      onFrame((beat) => {
        const el = ref.current;
        if (!el) return;
        el.style.transform = `translateX(${beat * PPB}px)`;
        if (attr) el.dataset.beat = String(beats(beat));
      }),
    [ref, attr],
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
  // once a bar rather than on every poll.
  const width = Math.max(room(timeline.meter, timeline.end, beat) * PPB, visible);
  const x = beat * PPB;

  // Follow the playhead while playing.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !playing) return;
    if (x < el.scrollLeft || x > el.scrollLeft + el.clientWidth - headerW - 40) el.scrollLeft = Math.max(0, x - 80);
  }, [x, playing, headerW]);

  // Clips being dragged (moved, copied or resized), drawn where they go until the timeline shows
  // the edit. Named by id, so an edit that reorders a lane (a move, an undo, an edit elsewhere)
  // keeps them.
  const [drag, setDrag] = useState<Drag | null>(null);
  const latest = useRef({ timeline, revision, onOpenClip, locked, playing, onSeek });
  latest.current = { timeline, revision, onOpenClip, locked, playing, onSeek };
  // A press on a lane's empty space, until it is let go.
  const pressed = useRef<{ pointer: number; x: number } | null>(null);
  const made = useRef<Made | null>(null);

  /** Once the graph shows the gesture that made a clip, select it and open it. */
  const settle = () => {
    const m = made.current;
    const graph = useStore.getState().graph;
    if (!m || !graph || graph.view.revision < m.revision) return;
    made.current = null;
    if (!findNoteClip(graph.timeline, m.clip)) return;
    useClipSelection.getState().only([m.clip]);
    latest.current.onOpenClip(m.clip);
  };
  useEffect(settle, [timeline]);
  // A dropped drag is drawn where it went until the timeline shows the edit.
  useEffect(() => {
    if (drag?.landed !== undefined && revision >= drag.landed) setDrag(null);
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
    /** A dropped drag: one gesture. Drawn where it went until the timeline shows it; refused, it goes back. */
    const drop = (d: Drag) => {
      const graph = ready();
      const refs = d.clips.flatMap((c) => serverRef(c.lane, c.ref) ?? []);
      if (!graph || refs.length !== d.clips.length) return setDrag(null);
      const first = d.clips[0].ref;
      let gesture: Gesture, step: string;
      if (d.kind === "move") {
        if (d.by === 0 && !d.to && !d.copy) return setDrag(null);
        gesture = { op: "move", clips: refs, by: d.by, ...(d.to ? { to: d.to } : {}), ...(d.copy ? { copy: true } : {}) };
        const onto = d.to && d.to.module !== first.module ? ` to ${named(d.to.module)}` : "";
        step = `${d.copy ? "copying" : "moving"} ${clipsWord(d.clips.map((c) => c.ref))}${onto}`;
      } else {
        gesture = { op: "resize", clip: refs[0], start: d.start, length: d.length };
        step = `resizing ${clipsWord([first])}`;
      }
      setDrag({ ...d, landed: Infinity });
      send(graph, gesture, { name: step }).then(
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
        const sel = useClipSelection.getState();
        if (add) sel.add(ref);
        else if (!toggle && !isPicked(sel.picked, ref)) sel.only([ref]);
        // The track follows the click, and Notes the notes clip.
        useShell.getState().select(ref.module, lane.kind === "notes" ? ref : undefined);
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
        const move = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer || latest.current.locked) return;
          if (!moved && Math.abs(ev.clientX - x0) < 4 && Math.abs(ev.clientY - y0) < 4) return;
          moved = true;
          const meter = latest.current.timeline.meter;
          const free = skipsSnap(ev);
          const raw = (ev.clientX - x0) / PPB;
          const at = { x: ev.clientX, y: ev.clientY };
          const base = { clips: group, by: 0, to: null, copy: false, refused: null, start: c.start, length: c.length, at };
          if (edge === "left") {
            // The left edge on the grid; what is inside stays where it plays (the server shifts it).
            const end = c.start + c.length;
            const start = Math.max(0, Math.min(snapSong(meter, c.start + raw, PPB, free), end - SHORTEST));
            now = { ...base, kind: "left", start, length: clean(end - start), text: position(meter, start), guide: start };
          } else if (edge === "right") {
            const end = Math.max(snapSong(meter, c.start + c.length + raw, PPB, free), c.start + SHORTEST);
            now = { ...base, kind: "right", length: clean(end - c.start), text: position(meter, end), guide: end };
          } else {
            // The grabbed clip's start lands on the grid; the others keep their distance from it.
            const lo = Math.min(...group.map((g) => g.clip.start));
            const by = Math.max(clean(snapSong(meter, c.start + raw, PPB, free) - c.start), -lo);
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
        };
        const end = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer) return;
          for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
          if (!moved) {
            // A click: the selection's rules.
            if (ev.type === "pointerup" && toggle) useClipSelection.getState().toggle(ref);
            else if (ev.type === "pointerup" && !add) useClipSelection.getState().only([ref]);
            return;
          }
          // Let go over a lane that can't take the clips, or the drag cancelled: nothing happens.
          if (ev.type !== "pointerup" || !now || now.refused) return setDrag(null);
          drop(now);
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      },
      create(module, key, beat) {
        const lane = findLane(latest.current.timeline, module, key);
        const graph = ready();
        if (lane?.kind !== "notes" || !graph) return;
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
        send(graph, { op: "create", module, lane: key, start, length }, { name: `adding a ${named(module)} clip` }).then(
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
      },
      press: (e) => void (pressed.current = e && e.button === 0 ? { pointer: e.pointerId, x: e.clientX } : null),
      place(e, beat) {
        const p = pressed.current;
        pressed.current = null;
        if (!p || p.pointer !== e.pointerId || Math.abs(e.clientX - p.x) >= 4 || latest.current.playing) return;
        // On the grid, as a dragged clip lands (anywhere while snap is off).
        latest.current.onSeek(snapSong(latest.current.timeline.meter, beat, PPB));
      },
      remove() {
        const picked = useClipSelection.getState().picked;
        const { timeline } = latest.current;
        const refs = picked.flatMap((p) => refIn(timeline, p) ?? []);
        const graph = ready();
        if (!refs.length || !graph) return;
        send(graph, { op: "delete", clips: refs }, { name: `deleting ${clipsWord(refs)}` }).catch(toast);
        // Notes closes on a clip deleted (taste.md §3.7).
        const shell = useShell.getState();
        for (const p of picked) if (sameClip(shell.clips[p.module], p)) shell.setClip(p.module, null);
        useClipSelection.getState().clear();
      },
    };
    // `latest` carries everything that changes; the edits themselves never do.
  }, []);

  // ⌘1 to ⌘4 (View's grid rows) change the grid of the view last pointed at or focused: this
  // one's or the roll's. The timeline's zoom says what Auto is.
  useEffect(() => {
    canvas.grid = (op) => gridOp(useGrid.getState().last, op, PPB);
    return () => void delete canvas.grid;
  }, []);

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
  useEffect(reveal, [timeline, open]);
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

  // The timeline's own keys while it has the keyboard: ⌫ deletes the selected clips, Esc lets go of
  // them (and, as before, of the clip Notes shows).
  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = useShell.getState();
    const sel = useClipSelection.getState();
    const shown = s.selected ? s.clips[s.selected] : undefined;
    if ((e.key === "Delete" || e.key === "Backspace") && !e.metaKey && !e.ctrlKey && sel.picked.length) edits.remove();
    else if (e.key === "Escape" && (sel.picked.length || shown)) {
      sel.clear();
      if (shown) s.deselect();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };

  const songRows = rowsOf(song);
  const songH = ROW_H + 1 + songRows.others.length * ROW_H + (open.has(song.module) ? songRows.controls.length * LANE_H : 0);
  const all = [...rows, song].filter((t) => t.lanes.some((l) => l.kind === "control")).map((t) => t.module);
  const anyOpen = all.some((m) => open.has(m));
  return (
    <div
      ref={scroller}
      data-testid="timeline"
      data-grid-view="song"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className="flex-1 min-h-0 overflow-auto relative bg-canvas outline-none"
      // Revealing a row never tucks it under the ruler, the headers or the Song row (TL-16).
      style={{ scrollPaddingTop: RULER_H, scrollPaddingLeft: headerW, scrollPaddingBottom: songH }}
    >
      <div className="relative flex flex-col" style={{ width: headerW + width, minHeight: "100%" }}>
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
              <GridChip view="song" ppb={PPB} />
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
          <PlayCap headerW={headerW} />
        </div>
        {/* Over the clips, multiplied in: lanes full of clips still show where the loop is (TL-10).
            Under the headers and the playhead. */}
        {looping && loopStart !== null && loopEnd !== null && (
          <div
            data-testid="loop-shade"
            aria-hidden
            className="absolute bottom-0 z-[14] pointer-events-none bg-accent/8 mix-blend-multiply"
            style={{ top: RULER_H, left: headerW + loopStart * PPB, width: (loopEnd - loopStart) * PPB }}
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
            <div data-testid="clip-guide" aria-hidden className="absolute bottom-0 z-[16] border-l border-dashed border-fg-faint pointer-events-none" style={{ top: RULER_H, left: headerW + drag.guide * PPB }} />
            <DragBubble at={drag.at}>{drag.text}</DragBubble>
          </>
        )}
        {/* The Song's row, Live's Main track: pinned at the foot of the track list, with its own playhead and loop wash. */}
        <div className="sticky bottom-0 z-[26] shrink-0 border-t border-line bg-canvas" data-song-row>
          {looping && loopStart !== null && loopEnd !== null && (
            <div aria-hidden className="absolute inset-y-0 z-[14] pointer-events-none bg-accent/8 mix-blend-multiply" style={{ left: headerW + loopStart * PPB, width: (loopEnd - loopStart) * PPB }} />
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
  return meterSpans(meter, 0, width / PPB).map((s) => {
    const step = fineStep(grid, s.bar);
    return (
      <VLines
        key={s.start}
        className="inset-y-0"
        style={{ left: s.start * PPB, width: (s.end - s.start) * PPB }}
        layers={[
          ...(step ? [{ every: step * PPB, color: "var(--color-hover)" }] : []),
          { every: PPB, color: "var(--color-line)" },
          { every: s.bar * PPB, color: "var(--color-line-strong)" },
        ]}
      />
    );
  });
});

/** The grid's step where the timeline draws its lines, in a bar of `bar` beats; null where it doesn't. */
function fineStep(grid: GridSetting, bar: number) {
  const step = stepOf(grid, PPB, bar);
  const fits = Math.abs(bar / step - Math.round(bar / step)) < 1e-6;
  return step < 1 && step * PPB >= 12 && fits ? step : null;
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
  return (
    <div
      data-testid="ruler"
      className="relative overflow-hidden bg-chrome border-b border-line cursor-pointer touch-none"
      style={{ width }}
      title={`Click to move the playhead · drag across to loop${locked ? "" : " · right-click to change the tempo or time signature here"}`}
      onContextMenu={(e) => {
        e.preventDefault();
        // Its popovers are portals, whose events reach the ruler through React: only its own.
        if (locked || session === undefined || !e.currentTarget.contains(e.target as Node)) return;
        const x = e.clientX - e.currentTarget.getBoundingClientRect().left;
        setMenu({ beat: Math.max(0, Math.round(x / PPB)), x });
      }}
      onPointerDown={(e) => {
        if (e.button !== 0 || !e.currentTarget.contains(e.target as Node)) return;
        const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX;
        const left = el.getBoundingClientRect().left;
        const beatAt = (x: number) => Math.max(0, Math.round((x - left) / PPB));
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
          style={{ left: shown.start * PPB, width: (shown.end - shown.start) * PPB }}
        />
      )}
      {/* Bar lines down the ruler, and a tick at each beat along its foot. */}
      {meterSpans(timeline.meter, 0, width / PPB).map((s) => (
        <div key={s.start} aria-hidden className="absolute inset-y-0 pointer-events-none" style={{ left: s.start * PPB, width: (s.end - s.start) * PPB }}>
          <VLines layers={[{ every: s.bar * PPB, color: "var(--color-line-strong)" }]} />
          <VLines className="inset-x-0 bottom-0 h-[5px]" layers={[{ every: PPB, color: "var(--color-line-strong)" }]} />
        </div>
      ))}
      {bars(timeline.meter, width / PPB).map((b) => {
        // Narrow bars are numbered every second or fourth.
        const w = b.length * PPB;
        return w > 36 || b.bar % (w > 18 ? 2 : 4) === 0 ? (
          <span key={b.bar} className="absolute top-1 text-xs font-medium text-fg-faint pointer-events-none" style={{ left: b.start * PPB + 6 }}>
            {b.bar + 1}
          </span>
        ) : null;
      })}
      {/* Where the tempo or the time signature changes: both at one beat share a mark, which opens them. */}
      {marks.map((c) => (
        <span
          key={c.beat}
          ref={open?.beat === c.beat ? setMarkAt : undefined}
          role="button"
          data-mark={c.beat}
          aria-label={`Changes from ${placeOf(timeline.meter, c.beat)}`}
          aria-expanded={open?.beat === c.beat}
          className={clsx(
            "absolute bottom-1.5 px-0.5 -mx-0.5 rounded-xs text-xs leading-none whitespace-nowrap transition-colors duration-80 ease-linear",
            open?.beat === c.beat ? "text-fg bg-hover" : "text-fg-muted hover:text-fg hover:bg-hover",
          )}
          style={{ left: c.beat * PPB + 3 }}
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
  const selected = useShell((s) => s.selected === module);
  // Notes outlines the clip it shows.
  const shown = useShell((s) => (s.view === "notes" && s.module === module ? (s.clips[module] ?? null) : null));
  const open = useShell((s) => s.open.has(module));
  // A muted track's clips, or one a solo leaves off, draw faint and grey; muted in the song, hatched too.
  const heard = useHeard(module);
  const { primary, others, controls } = rowsOf(track);
  /** Where a reference's clip is in `lane`, if it is one of its clips. */
  const at = (ref: ClipRef | null, lane: TimelineLane) => (ref?.lane === lane.key ? clipIndex(lane.clips, ref) : -1);
  const select = () => useShell.getState().select(module);

  /** A row: its header (sticky at the left) and its lane. */
  const row = (lane: TimelineLane | null, header: ReactNode, opts: { first?: boolean; height: number; movable?: boolean }) => (
    <div
      key={lane?.key ?? ""}
      data-lane={lane ? `${module}.${lane.key}` : undefined}
      className={clsx("flex", lane && lane.key === lit && "lit")}
      style={{ height: opts.height }}
    >
      <div
        {...(opts.first
          ? // The track's header selects it, as the module tabs did: what the dock shows follows it (§3.7).
            { "data-module-tab": module, role: "tab", "aria-selected": selected, "aria-controls": "dock" }
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
            : undefined
        }
      >
        {/* The colour bar runs down every row of the track, at 40% beside its lanes. */}
        <div className="absolute left-0 inset-y-0 w-1" style={{ background: color.bar, opacity: opts.first ? 1 : 0.4 }} />
        {/* The selected track's edge where it meets its lanes. */}
        {selected && <div className="absolute -right-px inset-y-0 w-0.5 bg-accent" />}
        {header}
      </div>
      <div
        // The lane dragged clips would land on is lit; one that can't take them isn't (design-8b §7.5).
        className={clsx("relative overflow-hidden", !opts.first && "border-t border-hover", (selected || (lane && drag?.to?.module === module && drag.to.lane === lane.key)) && "bg-accent-wash")}
        style={{ width }}
        title={lane?.kind === "notes" && !locked ? "Double-click to add a one-bar clip" : undefined}
        onPointerDown={(e) => {
          const empty = !(e.target as HTMLElement).closest("[data-clip]");
          if (empty) edits.unpick(module);
          edits.press(empty ? e : null);
        }}
        onPointerUp={(e) => edits.place(e, (e.clientX - e.currentTarget.getBoundingClientRect().left) / PPB)}
        onDoubleClick={(e) => {
          if (lane?.kind !== "notes" || (e.target as HTMLElement).closest("[data-clip]")) return;
          edits.create(module, lane.key, (e.clientX - e.currentTarget.getBoundingClientRect().left) / PPB);
        }}
      >
        {grid}
        {lane && (
          <div className={clsx("absolute inset-0", waiting ? "opacity-50 grayscale" : heard !== "heard" && "opacity-40 grayscale-60")}>
            <Hatched.Provider value={heard === "muted-in-song"}>
              <LaneClips
                module={module}
                lane={lane}
                color={color}
                shown={at(shown, lane)}
                drag={drag}
                editable={!locked}
                onOpen={(k) => onOpenClip(clipRef(module, lane.key, lane.clips, k))}
                onDown={(ref, e) => edits.down(ref, e)}
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
        <TrackHeader track={track} folds={controls.length > 0} open={open} selected={selected} song={!!last} issues={issues} />,
        { first: true, height: ROW_H, movable },
      )}
      {others.map((l) => row(l, <LaneLabel lane={l} />, { height: rowHeight(l) }))}
      {open && controls.map((l) => row(l, <LaneLabel lane={l} />, { height: LANE_H }))}
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
function LaneLabel({ lane }: { lane: TimelineLane }) {
  const name = laneTitle(lane);
  return (
    <>
      <span className="absolute left-7 right-[64px] text-sm text-fg-muted truncate" title={name === lane.key ? lane.key : `${name} (${lane.key})`}>
        {name}
      </span>
      {lane.kind === "control" && <LaneValue lane={lane} name={name} />}
    </>
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

/** Whether a drag moves a clip of track `module`, or onto one of its lanes. */
const involves = (d: Drag, module: string) => d.to?.module === module || d.clips.some((c) => c.ref.module === module);

function LaneClips({
  module,
  lane,
  color,
  shown,
  drag,
  editable,
  onOpen,
  onDown,
}: {
  module: string;
  lane: TimelineLane;
  color: TrackColor;
  /** Where the clip Notes shows is in the lane's clips, or -1. */
  shown: number;
  /** Clips being dragged from or onto this track's lanes. */
  drag: Drag | null;
  editable: boolean;
  onOpen: (index: number) => void;
  onDown: (ref: ClipRef, e: React.PointerEvent<HTMLElement>) => void;
}) {
  const picked = useClipSelection((s) => s.picked);
  // A clip is named by its id; one without its own (FMT-12) only by where it is.
  const id = (i: number) => (hasOwnId(lane.clips, i) ? lane.clips[i].id! : "");
  const key = (i: number) => id(i) || `#${i}`;
  const height = rowHeight(lane);
  const here = (a: LaneAddr) => a.module === module && a.lane === lane.key;

  // What a clip of this lane's kind draws inside it.
  const range = lane.kind === "control" ? lane.max - lane.min || 1 : 1;
  // 0 at the bottom of a clip's drawing, 1 at its top.
  const level = (v: number) => (lane.kind === "control" ? (v - lane.min) / range : 0);
  const inside = (c: AnyClip, tone: TrackColor) =>
    "notes" in c ? (
      <Notes notes={audible(c)} length={c.length} color={tone} />
    ) : "points" in c ? (
      <Curve clip={c} level={level} color={tone} />
    ) : (
      <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 px-2 text-xs font-medium" style={{ top: BODY_TOP, color: tone.ink }}>
        <AudioWaveform size={14} className="shrink-0" />
        <span className="truncate">{c.asset}</span>
      </div>
    );
  const words = (c: AnyClip) => {
    const how = editable ? `drag to move · ${mod.alt}-drag to copy · drag an edge to resize · Delete removes` : "";
    if ("notes" in c) return `${audible(c).length} notes, ${span(c)}\nDouble-click to edit the notes${how ? ` · ${how}` : ""}`;
    if ("points" in c) return `${span(c)}${how ? `\n${how[0].toUpperCase()}${how.slice(1)}` : ""}`;
    return `${c.asset}, ${span(c)}${how ? `\n${how[0].toUpperCase()}${how.slice(1)}` : ""}`;
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
        onDoubleClick={lane.kind === "notes" ? () => onOpen(i) : undefined}
        onPointerDown={(e) => onDown(ref, e)}
      >
        {inside(drawn, color)}
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
  onDoubleClick,
  onPointerDown,
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
  onDoubleClick?: () => void;
  /** Given, the clip can be selected (and, editable, dragged by its body or either edge). */
  onPointerDown?: (e: React.PointerEvent<HTMLElement>) => void;
  children: ReactNode;
}) {
  const hatched = useContext(Hatched);
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
        handles && (dragged ? "cursor-grabbing" : "cursor-grab"),
      )}
      onDoubleClick={onDoubleClick}
      onPointerDown={onPointerDown}
      style={
        {
          top: CLIP_INSET,
          bottom: CLIP_INSET,
          left: clip.start * PPB + 1,
          width: Math.max(clip.length * PPB - 2, 4),
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

/** The clip's own drawing area: beats across, 0..100 down. */
function Body({ length, children }: { length: number; children: ReactNode }) {
  return (
    <svg
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
function Notes({ notes, length, color }: { notes: NoteClip["notes"]; length: number; color: TrackColor }) {
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
          width={Math.max(n.length, 2 / PPB)}
          y={top + (hi - n.pitch) * h + h * 0.1}
          height={h * 0.8}
          fill={color.ink}
          fillOpacity={0.35 + 0.65 * n.velocity}
        />
      ))}
    </Body>
  );
}

/** The automation curve, its points marked; values hold before the first point and after the last. */
function Curve({ clip, level, color }: { clip: ControlClip; level: (v: number) => number; color: TrackColor }) {
  const { points } = clip;
  if (points.length === 0) return null;
  const first = points[0], last = points[points.length - 1];
  const line = [...(first[0] > 0 ? [[0, first[1]]] : []), ...points, ...(last[0] < clip.length ? [[clip.length, last[1]]] : [])];
  const xy = line.map(([b, v]) => `${b},${(1 - level(v)) * 100}`).join(" ");
  return (
    <>
      <Body length={clip.length}>
        <polygon points={`${xy} ${line[line.length - 1][0]},100 ${line[0][0]},100`} fill={color.ink} fillOpacity={0.1} />
        <polyline points={xy} fill="none" stroke={color.ink} strokeWidth={1.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
      </Body>
      {/* Circles stay round: drawn over the stretched drawing, not in it. */}
      <div aria-hidden className="absolute left-0 w-full pointer-events-none" style={{ top: BODY_TOP, height: `calc(100% - ${BODY_TOP + BODY_BOTTOM}px)` }}>
        {points.map(([b, v], i) => (
          <span key={i} className="absolute size-1.5 -ml-[3px] -mt-[3px] rounded-full" style={{ left: `${(b / clip.length) * 100}%`, top: `${(1 - level(v)) * 100}%`, background: color.ink }} />
        ))}
      </div>
    </>
  );
}

/** A control lane's value outside its clips: a dashed line where no clip is, at `y` in the lane. */
function Fallback({ clips, y }: { clips: ControlClip[]; y: number }) {
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
        <line key={a} x1={a * PPB} x2={b === Infinity ? "100%" : b * PPB} y1={top} y2={top} stroke="var(--color-line-strong)" strokeDasharray="4 4" />
      ))}
    </svg>
  );
}

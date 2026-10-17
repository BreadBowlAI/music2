import clsx from "clsx";
import { AudioWaveform, ChevronRight, Plus } from "lucide-react";
import { createContext, memo, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
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
  laneWriting,
  meterSpans,
  moduleName,
  newClipId,
  writeLane,
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
import { gridOp, skipsSnap, snapSong, stepOf, useGrid, type Grid as GridSetting } from "../lib/grid";
import { chord, opt } from "../lib/native";
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

/** Editing notes clips in place: select one (and drag it along the beats), or make one. Stable across renders. */
interface ClipEdits {
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
  /** Delete the selected clip. */
  remove: () => void;
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

  // The selected notes clip (Delete removes it), and one being dragged to a new start. Both are
  // named by id, so an edit that reorders the lane (a move, an undo, an edit elsewhere) keeps them.
  const [moving, setMoving] = useState<{ clip: ClipRef; start: number } | null>(null);
  const latest = useRef({ timeline, revision, onOpenClip, locked, playing, onSeek });
  latest.current = { timeline, revision, onOpenClip, locked, playing, onSeek };
  // A press on a lane's empty space, until it is let go.
  const pressed = useRef<{ pointer: number; x: number } | null>(null);
  const made = useRef<Made | null>(null);

  /** Once the graph shows the write that made a clip, select it and open it. */
  const settle = () => {
    const m = made.current;
    const graph = useStore.getState().graph;
    if (!m || !graph || graph.view.revision < m.revision) return;
    made.current = null;
    if (!findNoteClip(graph.timeline, m.clip)) return;
    latest.current.onOpenClip(m.clip);
  };
  useEffect(settle, [timeline]);

  const edits = useMemo<ClipEdits>(() => {
    const laneOf = (module: string, lane: string) => {
      const l = findLane(latest.current.timeline, module, lane);
      return l?.kind === "notes" ? l : null;
    };
    /** What a track is called, for the words of an edit to its clips ("moving a Hook clip"). */
    const named = (module: string) => moduleName({ key: module, label: latest.current.timeline.tracks.find((t) => t.module === module)?.label });
    /** One lane write: every clip the lane is to have; `doing` names its undo step. */
    const write = (module: string, lane: string, clips: NoteClip[], doing: string) => {
      const graph = useStore.getState().graph;
      // Clips are rewritten from the timeline shown, so wait until it shows the last write.
      if (!graph || laneWriting(graph) || latest.current.locked) return null;
      const w = writeLane(graph, module, lane, clips, latest.current.revision, { name: `${doing} ${named(module)} clip` });
      w.catch((error) => useStore.getState().toast(error instanceof Error ? error.message : String(error)));
      return w;
    };
    return {
      down(clip, e) {
        if (e.button !== 0) return;
        pressed.current = null;
        useShell.getState().select(clip.module, clip);
        scroller.current?.focus({ preventScroll: true });
        const lane = laneOf(clip.module, clip.lane);
        const c = lane?.clips[clipIndex(lane.clips, clip)];
        if (!c || latest.current.locked) return;
        const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX;
        el.setPointerCapture(pointer);
        let to: number | null = null;
        const move = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer || (to === null && Math.abs(ev.clientX - x0) < 4)) return;
          // Its start lands on the grid, unless snap is off or ⌘ is held (design-8b §7.3).
          to = snapSong(latest.current.timeline.meter, c.start + (ev.clientX - x0) / PPB, PPB, skipsSnap(ev));
          setMoving({ clip, start: to });
        };
        const end = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer) return;
          for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
          setMoving(null);
          const lane = laneOf(clip.module, clip.lane);
          const i = lane ? clipIndex(lane.clips, clip) : -1;
          if (ev.type !== "pointerup" || to === null || to === c.start || !lane || i < 0) return;
          // Moved, the clip keeps its id; an anonymous one gets the id its reference names.
          write(clip.module, clip.lane, lane.clips.map((x, k) => (k === i ? { ...x, id: clip.id, start: to! } : x)), "moving a");
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      },
      create(module, key, beat) {
        const lane = laneOf(module, key);
        if (!lane || latest.current.locked) return;
        // The bar clicked, after any clip before it and up to the next; one bar at most.
        const bar = barAt(latest.current.timeline.meter, beat);
        const before = Math.max(0, ...lane.clips.filter((c) => c.start + c.length <= beat).map((c) => c.start + c.length));
        const next = Math.min(Infinity, ...lane.clips.filter((c) => c.start >= beat).map((c) => c.start));
        const start = Math.max(bar.start, before);
        const length = Math.min(bar.length, next - start);
        if (!(length > 0) || lane.clips.some((c) => c.start < start + length && c.start + c.length > start)) return;
        // A new clip always gets a new id (FMT-12).
        const id = newClipId(lane.clips.map((c) => c.id));
        const m: Made = { revision: Infinity, clip: { module, lane: key, id } };
        made.current = m;
        write(module, key, [...lane.clips, { id, start, length, notes: [] }], "adding a")?.then(
          (r) => {
            m.revision = r.revision;
            settle();
          },
          () => made.current === m && (made.current = null),
        );
      },
      unpick: (module) => useShell.getState().setClip(module, null),
      press: (e) => void (pressed.current = e && e.button === 0 ? { pointer: e.pointerId, x: e.clientX } : null),
      place(e, beat) {
        const p = pressed.current;
        pressed.current = null;
        if (!p || p.pointer !== e.pointerId || Math.abs(e.clientX - p.x) >= 4 || latest.current.playing) return;
        // On the grid, as a dragged clip lands (anywhere while snap is off).
        latest.current.onSeek(snapSong(latest.current.timeline.meter, beat, PPB));
      },
      remove() {
        const s = useShell.getState();
        const p = s.selected ? s.clips[s.selected] : undefined;
        const lane = p && laneOf(p.module, p.lane);
        const i = lane ? clipIndex(lane.clips, p!) : -1;
        if (!p || !lane || i < 0) return;
        if (write(p.module, p.lane, lane.clips.filter((_, k) => k !== i), "deleting a")) s.setClip(p.module, null);
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

  const onKeyDown = (e: React.KeyboardEvent) => {
    const s = useShell.getState();
    const picked = s.selected ? s.clips[s.selected] : undefined;
    if (!picked) return;
    if (e.key === "Delete" || e.key === "Backspace") edits.remove();
    else if (e.key === "Escape") s.deselect();
    else return;
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
          moving={moving}
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
            moving={null}
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
  moving,
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
  moving: { clip: ClipRef; start: number } | null;
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
          moving={moving?.clip.module === t.module ? moving : null}
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
  moving,
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
  /** A clip of this track's being moved. */
  moving: { clip: ClipRef; start: number } | null;
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
  // The selected clip is the selected track's; Notes outlines the one it shows.
  const picked = useShell((s) => (s.selected === module ? (s.clips[module] ?? null) : null));
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
        className={clsx("relative overflow-hidden", !opts.first && "border-t border-hover", selected && "bg-accent-wash")}
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
                lane={lane}
                color={color}
                shown={at(shown, lane)}
                picked={at(picked, lane)}
                moving={moving && at(moving.clip, lane) >= 0 ? { index: at(moving.clip, lane), start: moving.start } : null}
                editable={!locked}
                onOpen={(k) => onOpenClip(clipRef(module, lane.key, lane.clips, k))}
                onDown={(k, e) => edits.down(clipRef(module, lane.key, lane.clips, k), e)}
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

function LaneClips({
  lane,
  color,
  shown,
  picked,
  moving,
  editable,
  onOpen,
  onDown,
}: {
  lane: TimelineLane;
  color: TrackColor;
  /** Where the clip Notes shows is in the lane's clips, or -1; likewise the selected one (notes clips only). */
  shown: number;
  picked: number;
  /** A notes clip being dragged to a new start. */
  moving: { index: number; start: number } | null;
  editable: boolean;
  onOpen: (index: number) => void;
  onDown: (index: number, e: React.PointerEvent<HTMLElement>) => void;
}) {
  // A clip is named by its id; one without its own (FMT-12) only by where it is.
  const id = (i: number) => (hasOwnId(lane.clips, i) ? lane.clips[i].id! : "");
  const key = (i: number) => id(i) || `#${i}`;
  const height = rowHeight(lane);
  switch (lane.kind) {
    case "notes":
      return lane.clips.map((c, i) => {
        const notes = audible(c);
        const dragged = moving?.index === i;
        return (
          <Clip
            key={key(i)}
            id={id(i)}
            clip={dragged ? { start: moving.start, length: c.length } : c}
            color={color}
            title={`${notes.length} notes, ${span(c)}\nDouble-click to edit the notes${editable ? " · drag to move · Delete removes" : ""}`}
            open={i === shown}
            picked={i === picked}
            dragged={dragged}
            editable={editable}
            onDoubleClick={() => onOpen(i)}
            onPointerDown={(e) => onDown(i, e)}
          >
            <Notes notes={notes} length={c.length} color={color} />
          </Clip>
        );
      });
    case "control": {
      const range = lane.max - lane.min || 1;
      // 0 at the bottom of a clip's drawing, 1 at its top.
      const level = (v: number) => (v - lane.min) / range;
      const body = height - 2 * (CLIP_INSET + BORDER) - BODY_TOP - BODY_BOTTOM;
      return (
        <>
          {lane.default != null && <Fallback clips={lane.clips} y={CLIP_INSET + BORDER + BODY_TOP + (1 - level(lane.default)) * body} />}
          {lane.clips.map((c, i) => (
            <Clip key={key(i)} id={id(i)} clip={c} color={color} title={span(c)}>
              <Curve clip={c} level={level} color={color} />
            </Clip>
          ))}
        </>
      );
    }
    case "audio":
      return lane.clips.map((c, i) => (
        <Clip key={key(i)} id={id(i)} clip={c} color={color} title={`${c.asset}, ${span(c)}`}>
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 px-2 text-xs font-medium" style={{ top: BODY_TOP, color: color.ink }}>
            <AudioWaveform size={14} className="shrink-0" />
            <span className="truncate">{c.asset}</span>
          </div>
        </Clip>
      ));
  }
}

function Clip({
  id,
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
  id: string;
  clip: { start: number; length: number };
  color: TrackColor;
  title: string;
  /** The clip Notes shows. */
  open?: boolean;
  /** Selected by a click: Delete removes it. */
  picked?: boolean;
  dragged?: boolean;
  editable?: boolean;
  onDoubleClick?: () => void;
  /** Given, the clip can be selected (and, editable, dragged). */
  onPointerDown?: (e: React.PointerEvent<HTMLElement>) => void;
  children: ReactNode;
}) {
  const hatched = useContext(Hatched);
  return (
    <div
      data-clip={id}
      data-start={clip.start}
      data-open={open || undefined}
      data-picked={picked || undefined}
      title={title}
      // Its body is the tint, its border the bar; the selected clip is the strongest outline on screen.
      className={clsx(
        "absolute rounded-sm overflow-hidden touch-none border border-(--clip-bar)",
        picked && "outline-2 outline-fg-strong",
        onPointerDown && editable && (dragged ? "cursor-grabbing z-10 shadow-lift opacity-90" : "cursor-grab hover:border-(--clip-ink)"),
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

import clsx from "clsx";
import { AudioWaveform, CircleAlert, Info, TriangleAlert } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  barAt,
  bars,
  clipIndex,
  clipRef,
  findLane,
  findNoteClip,
  hasOwnId,
  keyNote,
  labelOf,
  moduleName,
  laneWriting,
  meterSpans,
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
import { useStore } from "../lib/store";

const HEADER_W = 236;
const RULER_H = 34;
/** Pixels per beat. */
const PPB = 24;
const LANE_H: Record<TimelineLane["kind"], number> = { notes: 64, control: 48, audio: 52 };
/** A clip sits this far inside its lane; its drawing starts below the color bar. */
const CLIP_INSET = 4;
const BODY_TOP = 7;
const BODY_BOTTOM = 3;

/** Track color, muted toward gray to fit the light theme. */
const mute = (v: number) => Math.round(v * 0.5 + 155 * 0.5);
const rgb = (c: [number, number, number], a = 1) => `rgba(${mute(c[0])},${mute(c[1])},${mute(c[2])},${a})`;

type Rgb = [number, number, number];
const COLORS: Rgb[] = [[255, 94, 135], [255, 159, 67], [255, 214, 92], [94, 214, 140], [72, 196, 230], [124, 131, 253], [196, 113, 237]];
const dark = (c: Rgb) => rgb([c[0] * 0.55, c[1] * 0.55, c[2] * 0.55]);

/** A module's color, by its key's place in the timeline (null if it has no row there). */
export function trackColor(timeline: TimelineView, module: string) {
  const i = timeline.tracks.findIndex((t) => t.module === module);
  return i < 0 ? null : rgb(COLORS[i % COLORS.length]);
}

/** The same colour at full strength, for small marks such as the piano roll's notes. */
export function trackHue(timeline: TimelineView, module: string) {
  const i = timeline.tracks.findIndex((t) => t.module === module);
  return i < 0 ? null : `rgb(${COLORS[i % COLORS.length].join(",")})`;
}

const UNITS: Record<string, string> = { hz: "Hz", db: "dB", unit01: "0…1", bipolar: "±1", seconds: "s", semitones: "st" };
const beats = (n: number) => Number(n.toFixed(3));
const span = (c: { start: number; length: number }) => `beats ${beats(c.start)}–${beats(c.start + c.length)}`;

/** A clip the timeline made, to pick (and open) once the graph shows the write that made it. */
interface Made { revision: number; clip: ClipRef }

/** Editing notes clips in place: pick one (and drag it along the beats), or make one. Stable across renders. */
interface ClipEdits {
  down: (clip: ClipRef, e: React.PointerEvent<HTMLElement>) => void;
  /** A double-click on empty space in a notes lane (module and lane keys), at `beat`. */
  create: (module: string, lane: string, beat: number) => void;
  unpick: () => void;
  /** Delete the picked clip. */
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
 * The song as the accepted revision plays it: a row per lane, grouped by track, under a ruler that
 * seeks. Tracks, lanes and clips are named by their keys and ids, never by their place (ID-11, FMT-12).
 */
export default function SongTimeline({
  timeline,
  revision,
  beat,
  playing,
  loopStart,
  loopEnd,
  looping,
  onLoop,
  selected,
  open,
  issues,
  onSelect,
  onOpenClip,
  onSeek,
}: {
  timeline: TimelineView;
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
  /** The selected module's key. */
  selected: string | undefined;
  /** The notes clip open for editing, outlined. */
  open: ClipRef | null;
  /** What needs attention, by module key: marked on the track. */
  issues: ReadonlyMap<string, Diagnostic[]>;
  onSelect: (module: string) => void;
  /** Double-clicking a notes clip opens it. */
  onOpenClip: (clip: ClipRef) => void;
  onSeek: (beat: number) => void;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(0);
  useEffect(() => {
    const el = scroller.current!;
    const observer = new ResizeObserver(() => setVisible(Math.floor(el.clientWidth - HEADER_W)));
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  // A bar of room after the last clip and after the playhead, which plays on past the
  // song's end; never narrower than the window. Grown a bar at a time, the rows redraw
  // once a bar rather than on every poll.
  const width = Math.max(room(timeline.meter, timeline.end, beat) * PPB, visible);
  const x = beat * PPB;

  // Follow the playhead while playing.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !playing) return;
    if (x < el.scrollLeft || x > el.scrollLeft + el.clientWidth - HEADER_W - 40) el.scrollLeft = Math.max(0, x - 80);
  }, [x, playing]);

  // The picked notes clip (Delete removes it), and one being dragged to a new start. Both are
  // named by id, so an edit that reorders the lane (a move, an undo, an edit elsewhere) keeps
  // them; one that is gone is unpicked here, and closed by the canvas if it was open.
  const [picked, setPicked] = useState<ClipRef | null>(null);
  const [moving, setMoving] = useState<{ clip: ClipRef; start: number } | null>(null);
  if (picked && !findNoteClip(timeline, picked)) setPicked(null);
  const latest = useRef({ timeline, revision, open, onOpenClip, picked });
  latest.current = { timeline, revision, open, onOpenClip, picked };
  const made = useRef<Made | null>(null);

  /** Once the graph shows the write that made a clip, pick it and open it. */
  const settle = () => {
    const m = made.current;
    const graph = useStore.getState().graph;
    if (!m || !graph || graph.view.revision < m.revision) return;
    made.current = null;
    if (!findNoteClip(graph.timeline, m.clip)) return;
    setPicked(m.clip);
    latest.current.onOpenClip(m.clip);
  };
  useEffect(settle, [timeline]);

  const edits = useMemo<ClipEdits>(() => {
    const laneOf = (module: string, lane: string) => {
      const l = findLane(latest.current.timeline, module, lane);
      return l?.kind === "notes" ? l : null;
    };
    /** One lane write: every clip the lane is to have. */
    const write = (module: string, lane: string, clips: NoteClip[]) => {
      const graph = useStore.getState().graph;
      // Clips are rewritten from the timeline shown, so wait until it shows the last write.
      if (!graph || laneWriting(graph)) return null;
      const w = writeLane(graph, module, lane, clips, latest.current.revision);
      w.catch((error) => useStore.getState().toast(error instanceof Error ? error.message : String(error)));
      return w;
    };
    return {
      down(clip, e) {
        if (e.button !== 0) return;
        setPicked(clip);
        scroller.current?.focus({ preventScroll: true });
        const lane = laneOf(clip.module, clip.lane);
        const c = lane?.clips[clipIndex(lane.clips, clip)];
        if (!c) return;
        const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX;
        el.setPointerCapture(pointer);
        let to: number | null = null;
        const move = (ev: PointerEvent) => {
          if (ev.pointerId !== pointer || (to === null && Math.abs(ev.clientX - x0) < 4)) return;
          // Whole beats: a clip moves along the beat grid.
          to = Math.max(0, Math.round(c.start + (ev.clientX - x0) / PPB));
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
          write(clip.module, clip.lane, lane.clips.map((x, k) => (k === i ? { ...x, id: clip.id, start: to! } : x)));
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      },
      create(module, key, beat) {
        const lane = laneOf(module, key);
        if (!lane) return;
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
        write(module, key, [...lane.clips, { id, start, length, notes: [] }])?.then(
          (r) => {
            m.revision = r.revision;
            settle();
          },
          () => made.current === m && (made.current = null),
        );
      },
      unpick: () => setPicked(null),
      remove() {
        const p = latest.current.picked, lane = p && laneOf(p.module, p.lane);
        const i = lane ? clipIndex(lane.clips, p!) : -1;
        if (!p || !lane || i < 0) return;
        if (write(p.module, p.lane, lane.clips.filter((_, k) => k !== i))) setPicked(null);
      },
    };
    // `latest` carries everything that changes; the edits themselves never do.
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!picked) return;
    if (e.key === "Delete" || e.key === "Backspace") edits.remove();
    else if (e.key === "Escape") setPicked(null);
    else return;
    e.preventDefault();
    e.stopPropagation();
  };

  // The time signature at the start (an empty list is 4/4).
  const sig = timeline.meter[0] ?? { beats: 4, unit: 4 };
  // Polls arrive every 200ms; easing between them keeps the playhead moving smoothly. A jump (a
  // seek, or a loop going round) moves it at once rather than sliding it back across the song.
  const last = useRef(beat);
  const jumped = beat < last.current - 1e-6 || beat > last.current + 4;
  last.current = beat;
  const glide = { transform: `translateX(${x}px)`, transitionDuration: jumped ? "0ms" : undefined };
  return (
    <div ref={scroller} data-testid="timeline" tabIndex={-1} onKeyDown={onKeyDown} className="flex-1 min-h-0 overflow-auto relative bg-ink-950 outline-none">
      <div className="relative flex flex-col" style={{ width: HEADER_W + width, minHeight: "100%" }}>
        <div className="sticky top-0 z-30 shrink-0 flex" style={{ height: RULER_H }}>
          <div
            className="sticky left-0 z-40 shrink-0 bg-ink-900 border-b border-r border-black/[0.06] flex items-center justify-between px-4 text-[11px] font-semibold tracking-wider text-ink-400"
            style={{ width: HEADER_W }}
          >
            TRACKS
            <span className="font-mono font-medium tracking-normal tabular-nums">
              <span title="Time signature">{`${sig.beats}/${sig.unit}`}</span>
              {" · "}
              {timeline.tempo[0] && `${beats(timeline.tempo[0].bpm)} BPM`}
            </span>
          </div>
          <Ruler timeline={timeline} width={width} loopStart={loopStart} loopEnd={loopEnd} looping={looping} onSeek={onSeek} onLoop={onLoop} />
          <div className="absolute bottom-0 z-[35] pointer-events-none transition-transform duration-200 ease-linear" style={{ left: HEADER_W, ...glide }}>
            <div className="w-0 h-0 -translate-x-1/2 border-x-[6px] border-x-transparent border-t-[8px] border-t-sun" />
          </div>
        </div>
        <Tracks timeline={timeline} width={width} selected={selected} open={open} picked={picked} moving={moving} issues={issues} edits={edits} onSelect={onSelect} onOpenClip={onOpenClip} />
        {looping && loopStart !== null && loopEnd !== null && (
          <div
            data-testid="loop-shade"
            aria-hidden
            className="absolute bottom-0 z-[14] pointer-events-none bg-sun/[0.07] border-x border-sun/50"
            style={{ top: RULER_H, left: HEADER_W + loopStart * PPB, width: (loopEnd - loopStart) * PPB }}
          />
        )}
        <div
          data-testid="playhead"
          data-beat={beat}
          className="absolute bottom-0 z-[15] w-px bg-sun shadow-[0_0_8px_rgba(201,165,78,0.35)] pointer-events-none transition-transform duration-200 ease-linear"
          style={{ top: RULER_H, left: HEADER_W, ...glide }}
        />
      </div>
    </div>
  );
}

/** Bar and beat lines from beat 0 to `width`: one stretch per time signature, each with its own bar length. */
const Grid = memo(function Grid({ meter, width, bar, beat }: { meter: Meter[]; width: number; bar: string; beat: string }) {
  return meterSpans(meter, 0, width / PPB).map((s) => (
    <div
      key={s.start}
      aria-hidden
      className="absolute inset-y-0 pointer-events-none"
      style={{
        left: s.start * PPB,
        width: (s.end - s.start) * PPB,
        backgroundImage: `linear-gradient(to right, ${bar} 1px, transparent 1px), linear-gradient(to right, ${beat} 1px, transparent 1px)`,
        backgroundSize: `${s.bar * PPB}px 100%, ${PPB}px 100%`,
      }}
    />
  ));
});

/**
 * Bars and beats along the top. A click moves the playhead there; a drag across it sets a loop over
 * the beats it covered, and turns looping on.
 */
const Ruler = memo(function Ruler({
  timeline,
  width,
  loopStart,
  loopEnd,
  looping,
  onSeek,
  onLoop,
}: {
  timeline: TimelineView;
  width: number;
  loopStart: number | null;
  loopEnd: number | null;
  looping: boolean;
  onSeek: (beat: number) => void;
  onLoop: (start: number, end: number) => void;
}) {
  // A loop being dragged, shown until it is let go.
  const [drag, setDrag] = useState<{ start: number; end: number } | null>(null);
  const shown = drag ?? (loopStart !== null && loopEnd !== null ? { start: loopStart, end: loopEnd } : null);
  const on = drag ? true : looping;
  return (
    <div
      data-testid="ruler"
      className="relative overflow-hidden bg-ink-900/95 backdrop-blur border-b border-black/[0.06] cursor-pointer touch-none"
      style={{ width }}
      title="Click to move the playhead · drag across to loop"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
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
          className={clsx(
            "absolute inset-y-0 pointer-events-none",
            on ? "bg-sun/25 border-x-2 border-sun" : "bg-black/[0.05] border-x border-dashed border-ink-500/50",
          )}
          style={{ left: shown.start * PPB, width: (shown.end - shown.start) * PPB }}
        />
      )}
      {/* Bar lines down the ruler, and a tick at each beat along its foot. */}
      {meterSpans(timeline.meter, 0, width / PPB).map((s) => (
        <div
          key={s.start}
          aria-hidden
          className="absolute inset-y-0 pointer-events-none"
          style={{
            left: s.start * PPB,
            width: (s.end - s.start) * PPB,
            backgroundImage: "linear-gradient(to right, rgba(0,0,0,0.1) 1px, transparent 1px), linear-gradient(to right, rgba(0,0,0,0.12) 1px, transparent 1px)",
            backgroundSize: `${s.bar * PPB}px 100%, ${PPB}px 5px`,
            backgroundPosition: "0 0, 0 100%",
            backgroundRepeat: "repeat-x",
          }}
        />
      ))}
      {bars(timeline.meter, width / PPB).map((b) => {
        // Narrow bars are numbered every second or fourth.
        const w = b.length * PPB;
        return w > 36 || b.bar % (w > 18 ? 2 : 4) === 0 ? (
          <span key={b.bar} className="absolute top-2 text-[11px] font-medium text-ink-400 tabular-nums pointer-events-none" style={{ left: b.start * PPB + 6 }}>
            {b.bar + 1}
          </span>
        ) : null;
      })}
      {/* Where the tempo or the time signature changes: both at one beat share a mark. */}
      {changes(timeline).map((c) => (
        <span
          key={c.beat}
          className="absolute bottom-1 text-[9px] font-mono text-ink-500 pointer-events-none whitespace-nowrap"
          style={{ left: c.beat * PPB + 3 }}
          title={[c.meter && `${c.meter} from bar ${barAt(timeline.meter, c.beat).bar + 1}`, c.bpm && `${c.bpm} BPM from beat ${beats(c.beat)}`].filter(Boolean).join("\n")}
        >
          {[c.meter, c.bpm && `♩${c.bpm}`].filter(Boolean).join(" ")}
        </span>
      ))}
    </div>
  );
});

/** Every track's rows, then empty grid to the bottom. Memoized: the playhead moves without redrawing clips. */
const Tracks = memo(function Tracks({
  timeline,
  width,
  selected,
  open,
  picked,
  moving,
  issues,
  edits,
  onSelect,
  onOpenClip,
}: {
  timeline: TimelineView;
  width: number;
  selected: string | undefined;
  open: ClipRef | null;
  picked: ClipRef | null;
  moving: { clip: ClipRef; start: number } | null;
  issues: ReadonlyMap<string, Diagnostic[]>;
  edits: ClipEdits;
  onSelect: (module: string) => void;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const grid = <Grid meter={timeline.meter} width={width} bar="rgba(0,0,0,0.056)" beat="rgba(0,0,0,0.025)" />;
  return (
    <>
      {timeline.tracks.map((t, i) => (
        <TrackRows
          key={t.module}
          track={t}
          index={i}
          selected={t.module === selected}
          open={open?.module === t.module ? open : null}
          picked={picked?.module === t.module ? picked : null}
          moving={moving?.clip.module === t.module ? moving : null}
          issues={issues.get(t.module)}
          edits={edits}
          width={width}
          grid={grid}
          onSelect={onSelect}
          onOpenClip={onOpenClip}
        />
      ))}
      <div className="flex-1 flex min-h-6">
        <div className="sticky left-0 z-20 shrink-0 bg-ink-900 border-r border-black/[0.06]" style={{ width: HEADER_W }} />
        <div className="relative" style={{ width }}>
          {grid}
          {timeline.tracks.length === 0 && (
            <div className="absolute left-12 top-8 max-w-md space-y-1 text-ink-400">
              <div className="text-base font-semibold text-ink-200">No tracks yet</div>
              <div className="text-sm">Tracks, and the lanes the code declares, show up here.</div>
            </div>
          )}
        </div>
      </div>
    </>
  );
});

const TrackRows = memo(function TrackRows({
  track,
  index,
  selected,
  open,
  picked,
  moving,
  issues,
  edits,
  width,
  grid,
  onSelect,
  onOpenClip,
}: {
  track: TimelineTrack;
  index: number;
  selected: boolean;
  /** The clip open for editing, if it is one of this track's; likewise the picked one and one being moved. */
  open: ClipRef | null;
  picked: ClipRef | null;
  moving: { clip: ClipRef; start: number } | null;
  /** What needs attention in this track. */
  issues: Diagnostic[] | undefined;
  edits: ClipEdits;
  width: number;
  grid: ReactNode;
  onSelect: (module: string) => void;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const color = COLORS[index % COLORS.length];
  const name = moduleName({ key: track.module, label: track.label });
  // A track without lanes still gets a row, for its name.
  const rows = track.lanes.length ? track.lanes : [null];
  /** Where a reference's clip is in `lane`, if it is one of its clips. */
  const at = (ref: ClipRef | null, lane: TimelineLane) => (ref?.lane === lane.key ? clipIndex(lane.clips, ref) : -1);
  return (
    <div data-track={track.module} data-selected={selected || undefined} className="group border-b border-black/[0.07]" onPointerDown={() => onSelect(track.module)}>
      {rows.map((lane, i) => (
        <div key={lane?.key ?? ""} data-lane={lane ? `${track.module}.${lane.key}` : undefined} className="flex" style={{ height: lane ? LANE_H[lane.kind] : 44 }}>
          <div
            className={clsx(
              "sticky left-0 z-20 shrink-0 flex items-center gap-2 pl-4 pr-3 border-r border-black/[0.06] transition-colors",
              selected ? "bg-ink-800" : "bg-ink-900 group-hover:bg-ink-850",
            )}
            style={{ width: HEADER_W }}
          >
            <div className="absolute left-0 inset-y-0 w-1" style={{ background: rgb(color, selected ? 1 : 0.6) }} />
            <div className="flex-1 min-w-0">
              {i === 0 && (
                <>
                  <div className="flex items-center gap-1.5 min-w-0">
                    <span data-testid="track-label" className="text-sm font-semibold text-ink-100 truncate" title={name === track.module ? name : `${name} (${track.module})`}>
                      {name}
                    </span>
                    {issues && <Issues issues={issues} />}
                  </div>
                  <div className="text-xs text-ink-400 truncate">
                    {track.track ? "track" : "mix"}
                    {/* The key the code and the data files use, when the label doesn't say it. */}
                    {keyNote(name, track.module) && <span className="font-mono text-[11px] text-ink-500"> · {track.module}</span>}
                  </div>
                </>
              )}
            </div>
            {lane && <LaneLabel lane={lane} />}
          </div>
          <div
            className={clsx("relative overflow-hidden", i > 0 && "border-t border-black/[0.04]", selected ? "bg-ink-800/80" : index % 2 ? "bg-ink-900/40" : "bg-ink-900/70")}
            style={{ width }}
            title={lane?.kind === "notes" ? "Double-click to add a one-bar clip" : undefined}
            onPointerDown={(e) => !(e.target as HTMLElement).closest("[data-clip]") && edits.unpick()}
            onDoubleClick={(e) => {
              if (lane?.kind !== "notes" || (e.target as HTMLElement).closest("[data-clip]")) return;
              edits.create(track.module, lane.key, (e.clientX - e.currentTarget.getBoundingClientRect().left) / PPB);
            }}
          >
            {grid}
            {lane && (
              <LaneClips
                lane={lane}
                color={color}
                open={at(open, lane)}
                picked={at(picked, lane)}
                moving={moving && at(moving.clip, lane) >= 0 ? { index: at(moving.clip, lane), start: moving.start } : null}
                onOpen={(k) => onOpenClip(clipRef(track.module, lane.key, lane.clips, k))}
                onDown={(k, e) => edits.down(clipRef(track.module, lane.key, lane.clips, k), e)}
              />
            )}
          </div>
        </div>
      ))}
    </div>
  );
});

/** A mark on a track that something in it needs attention: the worst of it, and every message on hover. */
function Issues({ issues }: { issues: Diagnostic[] }) {
  const error = issues.some((d) => d.severity === "error");
  const warning = !error && issues.some((d) => d.severity === "warning");
  const Icon = error ? CircleAlert : warning ? TriangleAlert : Info;
  return (
    <span
      data-testid="track-issues"
      data-severity={error ? "error" : warning ? "warning" : "notice"}
      className={clsx("shrink-0 inline-flex", error ? "text-red-600" : warning ? "text-amber-600" : "text-ink-400")}
      title={issues.map((d) => d.message).join("\n\n")}
    >
      <Icon className="size-3.5" />
    </span>
  );
}

function LaneLabel({ lane }: { lane: TimelineLane }) {
  const detail =
    lane.kind === "notes"
      ? `${lane.clips.reduce((n, c) => n + audible(c).length, 0)} notes`
      : lane.kind === "control"
        ? lane.unit ? UNITS[lane.unit] ?? lane.unit : "control"
        : lane.channels === 1 ? "mono audio" : "stereo audio";
  const name = labelOf(lane);
  const named = name === lane.key ? lane.key : `${name} (${lane.key})`;
  const title = lane.kind === "control" && lane.default != null ? `${named}: ${beats(lane.default)} outside clips` : named;
  return (
    <div className="shrink-0 max-w-[104px] text-right leading-tight" title={title}>
      {/* A key reads as code; a label the code gave reads as a name. */}
      <div className={clsx("text-ink-200 truncate", name === lane.key ? "font-mono text-[11px]" : "text-[12px] font-medium")}>{name}</div>
      <div className="text-[10px] text-ink-400 truncate">{detail}</div>
    </div>
  );
}

function LaneClips({
  lane,
  color,
  open,
  picked,
  moving,
  onOpen,
  onDown,
}: {
  lane: TimelineLane;
  color: Rgb;
  /** Where the open clip is in the lane's clips, or -1; likewise the picked one (notes clips only). */
  open: number;
  picked: number;
  /** A notes clip being dragged to a new start. */
  moving: { index: number; start: number } | null;
  onOpen: (index: number) => void;
  onDown: (index: number, e: React.PointerEvent<HTMLElement>) => void;
}) {
  // A clip is named by its id; one without its own (FMT-12) only by where it is.
  const id = (i: number) => (hasOwnId(lane.clips, i) ? lane.clips[i].id! : "");
  const key = (i: number) => id(i) || `#${i}`;
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
            title={`${notes.length} notes, ${span(c)}\nDouble-click to edit the notes · drag to move · Delete removes`}
            open={i === open}
            picked={i === picked}
            dragged={dragged}
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
      const body = LANE_H.control - 2 * CLIP_INSET - BODY_TOP - BODY_BOTTOM;
      return (
        <>
          {lane.default != null && (
            <div
              className="absolute inset-x-0 border-t border-dashed pointer-events-none"
              style={{ top: CLIP_INSET + BODY_TOP + (1 - level(lane.default)) * body, borderColor: rgb(color, 0.7) }}
              title={`${beats(lane.default)} outside clips`}
            />
          )}
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
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 px-2 text-[11px] font-medium text-black/70" style={{ top: BODY_TOP }}>
            <AudioWaveform className="size-3.5 shrink-0" style={{ color: dark(color) }} />
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
  onDoubleClick,
  onPointerDown,
  children,
}: {
  /** Its id, or "" for a clip without one of its own. */
  id: string;
  clip: { start: number; length: number };
  color: Rgb;
  title: string;
  open?: boolean;
  /** Picked by a click: Delete removes it. */
  picked?: boolean;
  dragged?: boolean;
  onDoubleClick?: () => void;
  /** Given, the clip can be picked and dragged. */
  onPointerDown?: (e: React.PointerEvent<HTMLElement>) => void;
  children: ReactNode;
}) {
  return (
    <div
      data-clip={id}
      data-start={clip.start}
      data-open={open || undefined}
      data-picked={picked || undefined}
      title={title}
      className={clsx(
        "absolute rounded-lg overflow-hidden touch-none",
        picked ? "ring-2 ring-pop-500 shadow-md" : open ? "ring-2 ring-ink-100/70 shadow-md" : "ring-1 ring-black/15",
        onPointerDown && (dragged ? "cursor-grabbing z-10 shadow-lg opacity-90" : "cursor-grab"),
      )}
      onDoubleClick={onDoubleClick}
      onPointerDown={onPointerDown}
      style={{
        top: CLIP_INSET,
        bottom: CLIP_INSET,
        left: clip.start * PPB + 1,
        width: Math.max(clip.length * PPB - 2, 4),
        background: `linear-gradient(180deg, ${rgb(color, 0.42)}, ${rgb(color, 0.22)})`,
      }}
    >
      <div className="h-[3px]" style={{ background: rgb(color) }} />
      {children}
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
function Notes({ notes, length, color }: { notes: NoteClip["notes"]; length: number; color: Rgb }) {
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
          fill={dark(color)}
          fillOpacity={0.35 + 0.65 * n.velocity}
        />
      ))}
    </Body>
  );
}

/** The automation curve; values hold before the first point and after the last. */
function Curve({ clip, level, color }: { clip: ControlClip; level: (v: number) => number; color: Rgb }) {
  const { points } = clip;
  if (points.length === 0) return null;
  const first = points[0], last = points[points.length - 1];
  const line = [...(first[0] > 0 ? [[0, first[1]]] : []), ...points, ...(last[0] < clip.length ? [[clip.length, last[1]]] : [])];
  const xy = line.map(([b, v]) => `${b},${(1 - level(v)) * 100}`).join(" ");
  return (
    <Body length={clip.length}>
      <polygon points={`${xy} ${line[line.length - 1][0]},100 ${line[0][0]},100`} fill={rgb(color, 0.35)} />
      <polyline points={xy} fill="none" stroke={dark(color)} strokeWidth={1.5} strokeLinejoin="round" vectorEffect="non-scaling-stroke" />
    </Body>
  );
}

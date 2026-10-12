import clsx from "clsx";
import { AudioWaveform } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { findNoteClip, laneWriting, writeLane, type ClipRef, type ControlClip, type NoteClip, type TimelineLane, type TimelineTrack, type TimelineView } from "../lib/graph";
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

/** A module's color, by its place in the timeline (null if it has no row there). */
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

/** A clip by where it is, which a lane write keeps while its index may change. */
interface Span { start: number; length: number }

/**
 * What a lane write from the timeline leaves picked and open, once the graph shows it (its
 * revision, unknown while it is on its way). `open` is where the open clip ends up: null if
 * the write removes it, undefined if it isn't in this lane.
 */
interface Follow { module: string; lane: string; revision: number; picked: Span | null; open?: Span | null; opens: boolean }

/**
 * Where notes clip `was` (at `index` of `before`) is in `after`, the same lane changed: at its
 * span (moved in the lane's order), at its index with its start (made longer or shorter), or a
 * clip new to the lane with its length and notes (moved by an undo or elsewhere). Null if it is gone.
 */
function follows(was: NoteClip, index: number, before: NoteClip[], after: NoteClip[]): number | null {
  const at = (c: NoteClip | undefined) => !!c && c.start === was.start && c.length === was.length;
  if (at(after[index])) return index;
  const same = after.findIndex(at);
  if (same >= 0) return same;
  if (after[index]?.start === was.start) return index;
  const notes = JSON.stringify(was.notes);
  const moved = after.findIndex(
    (c) => !before.some((b) => b.start === c.start && b.length === c.length) && c.length === was.length && JSON.stringify(c.notes) === notes,
  );
  return moved >= 0 ? moved : null;
}

/** Editing notes clips in place: pick one (and drag it along the beats), or make one. Stable across renders. */
interface ClipEdits {
  down: (clip: ClipRef, e: React.PointerEvent<HTMLElement>) => void;
  /** A double-click on empty space in a notes lane, at `beat`. */
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

/** The song as the accepted revision plays it: a row per lane, grouped by track, under a ruler that seeks. */
export default function SongTimeline({
  timeline,
  revision,
  beat,
  playing,
  selected,
  open,
  onSelect,
  onOpenClip,
  onCloseClip,
  onSeek,
}: {
  timeline: TimelineView;
  /** The revision `timeline` is from: what a lane write rewritten from it is based on. */
  revision: number;
  beat: number;
  playing: boolean;
  /** The selected module's name. */
  selected: string | undefined;
  /** The notes clip open for editing, outlined. */
  open: ClipRef | null;
  onSelect: (module: string) => void;
  /** Double-clicking a notes clip opens it. */
  onOpenClip: (clip: ClipRef) => void;
  /** The open clip is gone (deleted here, undone, edited elsewhere). */
  onCloseClip: () => void;
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
  const bpb = timeline.beats_per_bar;
  const width = Math.max((Math.max(timeline.end, Math.ceil(beat / bpb) * bpb) + bpb) * PPB, visible);
  const x = beat * PPB;

  // Follow the playhead while playing.
  useEffect(() => {
    const el = scroller.current;
    if (!el || !playing) return;
    if (x < el.scrollLeft || x > el.scrollLeft + el.clientWidth - HEADER_W - 40) el.scrollLeft = Math.max(0, x - 80);
  }, [x, playing]);

  // The picked notes clip (Delete removes it), and one being dragged to a new start.
  const [picked, setPicked] = useState<ClipRef | null>(null);
  const [moving, setMoving] = useState<{ clip: ClipRef; start: number } | null>(null);
  if (picked && !findNoteClip(timeline, picked)) setPicked(null);
  const latest = useRef({ timeline, revision, open, onOpenClip, onCloseClip, picked });
  latest.current = { timeline, revision, open, onOpenClip, onCloseClip, picked };
  const follow = useRef<Follow | null>(null);

  /** Once the graph shows a write made here, pick and open the clips it was about, wherever they now sort. */
  const settle = () => {
    const f = follow.current;
    const graph = useStore.getState().graph;
    if (!f || !graph || graph.view.revision < f.revision) return;
    follow.current = null;
    const lane = graph.timeline.tracks.find((t) => t.module === f.module)?.lanes.find((l) => l.id === f.lane);
    if (lane?.kind !== "notes") return;
    const find = (s: Span | null | undefined) => (s ? lane.clips.findIndex((c) => c.start === s.start && c.length === s.length) : -1);
    const p = find(f.picked), o = find(f.open);
    setPicked(p < 0 ? null : { module: f.module, lane: f.lane, index: p });
    const now = latest.current.open;
    const here = !!now && now.module === f.module && now.lane === f.lane;
    if (o >= 0 && (f.opens || !now || (here && now.index !== o))) {
      latest.current.onOpenClip({ module: f.module, lane: f.lane, index: o });
    } else if (f.open === null && here) {
      latest.current.onCloseClip();
    }
  };

  // Clips are named by their index, which an edit that reorders a lane changes (an undo, an
  // edit elsewhere). So when the timeline changes, the open and the picked clip follow their
  // clip (see `follows`), and close or unpick if it is gone; a clip closed only because its
  // index ran out opens again. A lane the timeline is writing follows that write instead
  // (`settle`). Compared with the previous render's values, kept after each render.
  const prev = useRef({ timeline, open, picked });
  useEffect(() => {
    const was = prev.current;
    if (was.timeline !== timeline) {
      const same = (a: ClipRef | null, b: ClipRef | null) => a === b || (!!a && !!b && a.module === b.module && a.lane === b.lane && a.index === b.index);
      const writing = (ref: ClipRef) => follow.current?.module === ref.module && follow.current.lane === ref.lane;
      const notes = (t: TimelineView, ref: ClipRef) => {
        const lane = t.tracks.find((x) => x.module === ref.module)?.lanes.find((l) => l.id === ref.lane);
        return lane?.kind === "notes" ? lane.clips : null;
      };
      /** The clip's new place: undefined if it stays where it is, null if it is gone. */
      const moved = (ref: ClipRef | null): ClipRef | null | undefined => {
        const before = ref && findNoteClip(was.timeline, ref);
        if (!ref || !before || writing(ref)) return undefined;
        const after = notes(timeline, ref);
        const index = after ? follows(before, ref.index, notes(was.timeline, ref) ?? [], after) : null;
        return index === ref.index ? undefined : index === null ? null : { ...ref, index };
      };
      if (!open || same(open, was.open)) {
        const o = moved(was.open);
        if (o) onOpenClip(o);
        else if (o === null && open) onCloseClip();
      }
      if (!picked || same(picked, was.picked)) {
        const p = moved(was.picked);
        if (p !== undefined) setPicked(p);
      }
    }
    settle();
  }, [timeline]);
  useEffect(() => {
    prev.current = { timeline, open, picked };
  });

  const edits = useMemo<ClipEdits>(() => {
    const laneOf = (module: string, id: string) => {
      const l = latest.current.timeline.tracks.find((t) => t.module === module)?.lanes.find((x) => x.id === id);
      return l?.kind === "notes" ? l : null;
    };
    /** One lane write: every clip the lane is to have, and the spans to pick and open afterwards. */
    const write = (module: string, lane: string, clips: NoteClip[], after: Omit<Follow, "module" | "lane" | "revision">) => {
      const graph = useStore.getState().graph;
      // Clips are rewritten from the timeline shown, so wait until it shows the last write.
      if (!graph || laneWriting(graph)) return;
      const f: Follow = { module, lane, revision: Infinity, ...after };
      follow.current = f;
      writeLane(graph, module, lane, clips, latest.current.revision).then(
        (r) => {
          f.revision = r.revision;
          settle();
        },
        (error) => {
          if (follow.current === f) follow.current = null;
          useStore.getState().toast(error instanceof Error ? error.message : String(error));
        },
      );
    };
    /** Where the open clip will be after an edit of its lane (null: gone); undefined if it isn't in this lane. */
    const openAfter = (module: string, lane: string, change: (i: number, c: NoteClip) => Span | null) => {
      const o = latest.current.open, l = laneOf(module, lane);
      return o && o.module === module && o.lane === lane && l?.clips[o.index] ? change(o.index, l.clips[o.index]) : undefined;
    };
    const span = (c: Span): Span => ({ start: c.start, length: c.length });
    return {
      down(clip, e) {
        if (e.button !== 0) return;
        setPicked(clip);
        scroller.current?.focus({ preventScroll: true });
        const c = laneOf(clip.module, clip.lane)?.clips[clip.index];
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
          if (ev.type !== "pointerup" || to === null || to === c.start || !lane) return;
          const moved = { start: to, length: c.length };
          const clips = lane.clips.map((x, i) => (i === clip.index ? { ...x, start: to! } : x));
          write(clip.module, clip.lane, clips, { picked: moved, open: openAfter(clip.module, clip.lane, (i, x) => (i === clip.index ? moved : span(x))), opens: false });
        };
        const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
        for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
      },
      create(module, id, beat) {
        const lane = laneOf(module, id);
        if (!lane) return;
        // The bar clicked, after any clip before it and up to the next; one bar at most.
        const bpb = latest.current.timeline.beats_per_bar;
        const before = Math.max(0, ...lane.clips.filter((c) => c.start + c.length <= beat).map((c) => c.start + c.length));
        const next = Math.min(Infinity, ...lane.clips.filter((c) => c.start >= beat).map((c) => c.start));
        const start = Math.max(Math.floor(beat / bpb) * bpb, before);
        const made = { start, length: Math.min(bpb, next - start) };
        if (!(made.length > 0) || lane.clips.some((c) => c.start < start + made.length && c.start + c.length > start)) return;
        write(module, id, [...lane.clips, { ...made, notes: [] }], { picked: made, open: made, opens: true });
      },
      unpick: () => setPicked(null),
      remove() {
        const p = latest.current.picked, lane = p && laneOf(p.module, p.lane);
        if (!p || !lane) return;
        const clips = lane.clips.filter((_, i) => i !== p.index);
        write(p.module, p.lane, clips, { picked: null, open: openAfter(p.module, p.lane, (i, x) => (i === p.index ? null : span(x))), opens: false });
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

  // Polls arrive every 200ms; easing between them keeps the playhead moving smoothly.
  const glide = { transform: `translateX(${x}px)` };
  return (
    <div ref={scroller} data-testid="timeline" tabIndex={-1} onKeyDown={onKeyDown} className="flex-1 min-h-0 overflow-auto relative bg-ink-950 outline-none">
      <div className="relative flex flex-col" style={{ width: HEADER_W + width, minHeight: "100%" }}>
        <div className="sticky top-0 z-30 shrink-0 flex" style={{ height: RULER_H }}>
          <div
            className="sticky left-0 z-40 shrink-0 bg-ink-900 border-b border-r border-black/[0.06] flex items-center justify-between px-4 text-[11px] font-semibold tracking-wider text-ink-400"
            style={{ width: HEADER_W }}
          >
            TRACKS
            {timeline.tempo[0] && <span className="font-mono font-medium tracking-normal tabular-nums">{beats(timeline.tempo[0].bpm)} BPM</span>}
          </div>
          <Ruler timeline={timeline} width={width} onSeek={onSeek} />
          <div className="absolute bottom-0 z-[35] pointer-events-none transition-transform duration-200 ease-linear" style={{ left: HEADER_W, ...glide }}>
            <div className="w-0 h-0 -translate-x-1/2 border-x-[6px] border-x-transparent border-t-[8px] border-t-sun" />
          </div>
        </div>
        <Tracks
          timeline={timeline}
          width={width}
          selected={selected}
          open={open}
          picked={picked}
          moving={moving}
          edits={edits}
          onSelect={onSelect}
          onOpenClip={onOpenClip}
        />
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

const Ruler = memo(function Ruler({ timeline, width, onSeek }: { timeline: TimelineView; width: number; onSeek: (beat: number) => void }) {
  const bar = timeline.beats_per_bar * PPB;
  return (
    <div
      data-testid="ruler"
      className="relative overflow-hidden bg-ink-900/95 backdrop-blur border-b border-black/[0.06] cursor-pointer"
      style={{
        width,
        backgroundImage: "linear-gradient(to right, rgba(0,0,0,0.1) 1px, transparent 1px), linear-gradient(to right, rgba(0,0,0,0.12) 1px, transparent 1px)",
        backgroundSize: `${bar}px 100%, ${PPB}px 5px`,
        backgroundPosition: "0 0, 0 100%",
        backgroundRepeat: "repeat-x",
      }}
      title="Click to move the playhead"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const left = e.currentTarget.getBoundingClientRect().left;
        onSeek(Math.max(0, Math.round((e.clientX - left) / PPB)));
      }}
    >
      {Array.from({ length: Math.ceil(width / bar) }, (_, i) =>
        bar > 36 || i % (bar > 18 ? 2 : 4) === 0 ? (
          <span key={i} className="absolute top-2 text-[11px] font-medium text-ink-400 tabular-nums pointer-events-none" style={{ left: i * bar + 6 }}>
            {i + 1}
          </span>
        ) : null,
      )}
      {timeline.tempo.slice(1).map((t, i) => (
        <span
          key={i}
          className="absolute bottom-1 text-[9px] font-mono text-ink-500 pointer-events-none"
          style={{ left: t.beat * PPB + 3 }}
          title={`${beats(t.bpm)} BPM from beat ${beats(t.beat)}`}
        >
          ♩{beats(t.bpm)}
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
  edits: ClipEdits;
  onSelect: (module: string) => void;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const bpb = timeline.beats_per_bar;
  const grid = useMemo(
    () => ({
      backgroundImage: "linear-gradient(to right, rgba(0,0,0,0.056) 1px, transparent 1px), linear-gradient(to right, rgba(0,0,0,0.025) 1px, transparent 1px)",
      backgroundSize: `${bpb * PPB}px 100%, ${PPB}px 100%`,
    }),
    [bpb],
  );
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
          edits={edits}
          width={width}
          grid={grid}
          onSelect={onSelect}
          onOpenClip={onOpenClip}
        />
      ))}
      <div className="flex-1 flex min-h-6">
        <div className="sticky left-0 z-20 shrink-0 bg-ink-900 border-r border-black/[0.06]" style={{ width: HEADER_W }} />
        <div className="relative" style={{ width, ...grid }}>
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
  edits: ClipEdits;
  width: number;
  grid: React.CSSProperties;
  onSelect: (module: string) => void;
  onOpenClip: (clip: ClipRef) => void;
}) {
  const color = COLORS[index % COLORS.length];
  // A track without lanes still gets a row, for its name.
  const rows = track.lanes.length ? track.lanes : [null];
  return (
    <div data-track={track.module} data-selected={selected || undefined} className="group border-b border-black/[0.07]" onPointerDown={() => onSelect(track.module)}>
      {rows.map((lane, i) => (
        <div key={lane?.id ?? ""} data-lane={lane ? `${track.module}.${lane.id}` : undefined} className="flex" style={{ height: lane ? LANE_H[lane.kind] : 44 }}>
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
                  <div className="text-sm font-semibold text-ink-100 truncate">{track.module === "song" ? "Song" : track.module}</div>
                  <div className="text-xs text-ink-400">{track.track ? "track" : "mix"}</div>
                </>
              )}
            </div>
            {lane && <LaneLabel lane={lane} />}
          </div>
          <div
            className={clsx("relative overflow-hidden", i > 0 && "border-t border-black/[0.04]", selected ? "bg-ink-800/80" : index % 2 ? "bg-ink-900/40" : "bg-ink-900/70")}
            style={{ width, ...grid }}
            title={lane?.kind === "notes" ? "Double-click to add a one-bar clip" : undefined}
            onPointerDown={(e) => !(e.target as HTMLElement).closest("[data-clip]") && edits.unpick()}
            onDoubleClick={(e) => {
              if (lane?.kind !== "notes" || (e.target as HTMLElement).closest("[data-clip]")) return;
              edits.create(track.module, lane.id, (e.clientX - e.currentTarget.getBoundingClientRect().left) / PPB);
            }}
          >
            {lane && (
              <LaneClips
                lane={lane}
                color={color}
                open={open?.lane === lane.id ? open.index : null}
                picked={picked?.lane === lane.id ? picked.index : null}
                moving={moving?.clip.lane === lane.id ? moving : null}
                onOpen={(i) => onOpenClip({ module: track.module, lane: lane.id, index: i })}
                onDown={(i, e) => edits.down({ module: track.module, lane: lane.id, index: i }, e)}
              />
            )}
          </div>
        </div>
      ))}
    </div>
  );
});

function LaneLabel({ lane }: { lane: TimelineLane }) {
  const detail =
    lane.kind === "notes"
      ? `${lane.clips.reduce((n, c) => n + audible(c).length, 0)} notes`
      : lane.kind === "control"
        ? lane.unit ? UNITS[lane.unit] ?? lane.unit : "control"
        : lane.channels === 1 ? "mono audio" : "stereo audio";
  const title = lane.kind === "control" && lane.default != null ? `${lane.id}: ${beats(lane.default)} outside clips` : lane.id;
  return (
    <div className="shrink-0 max-w-[104px] text-right leading-tight" title={title}>
      <div className="font-mono text-[11px] text-ink-200 truncate">{lane.id}</div>
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
  open: number | null;
  /** Notes clips only: the picked one, and one being dragged to a new start. */
  picked: number | null;
  moving: { clip: ClipRef; start: number } | null;
  onOpen: (index: number) => void;
  onDown: (index: number, e: React.PointerEvent<HTMLElement>) => void;
}) {
  switch (lane.kind) {
    case "notes":
      return lane.clips.map((c, i) => {
        const notes = audible(c);
        const dragged = moving?.clip.index === i;
        return (
          <Clip
            key={i}
            index={i}
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
            <Clip key={i} index={i} clip={c} color={color} title={span(c)}>
              <Curve clip={c} level={level} color={color} />
            </Clip>
          ))}
        </>
      );
    }
    case "audio":
      return lane.clips.map((c, i) => (
        <Clip key={i} index={i} clip={c} color={color} title={`${c.asset}, ${span(c)}`}>
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 px-2 text-[11px] font-medium text-black/70" style={{ top: BODY_TOP }}>
            <AudioWaveform className="size-3.5 shrink-0" style={{ color: dark(color) }} />
            <span className="truncate">{c.asset}</span>
          </div>
        </Clip>
      ));
  }
}

function Clip({
  index,
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
  index: number;
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
      data-clip={index}
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

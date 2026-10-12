import clsx from "clsx";
import { AudioWaveform } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ControlClip, NoteClip, TimelineLane, TimelineTrack, TimelineView } from "../lib/graph";
import { rgb } from "../lib/music";

const HEADER_W = 236;
const RULER_H = 34;
/** Pixels per beat. */
const PPB = 24;
const LANE_H: Record<TimelineLane["kind"], number> = { notes: 64, control: 48, audio: 52 };
/** A clip sits this far inside its lane; its drawing starts below the color bar. */
const CLIP_INSET = 4;
const BODY_TOP = 7;
const BODY_BOTTOM = 3;

type Rgb = [number, number, number];
const COLORS: Rgb[] = [[255, 94, 135], [255, 159, 67], [255, 214, 92], [94, 214, 140], [72, 196, 230], [124, 131, 253], [196, 113, 237]];
const dark = (c: Rgb) => rgb([c[0] * 0.55, c[1] * 0.55, c[2] * 0.55]);

/** A module's color, by its place in the timeline (null if it has no row there). */
export function trackColor(timeline: TimelineView, module: string) {
  const i = timeline.tracks.findIndex((t) => t.module === module);
  return i < 0 ? null : rgb(COLORS[i % COLORS.length]);
}

const UNITS: Record<string, string> = { hz: "Hz", db: "dB", unit01: "0…1", bipolar: "±1", seconds: "s", semitones: "st" };
const beats = (n: number) => Number(n.toFixed(3));
const span = (c: { start: number; length: number }) => `beats ${beats(c.start)}–${beats(c.start + c.length)}`;

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
  beat,
  playing,
  selected,
  onSelect,
  onSeek,
}: {
  timeline: TimelineView;
  beat: number;
  playing: boolean;
  /** The selected module's name. */
  selected: string | undefined;
  onSelect: (module: string) => void;
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

  // Polls arrive every 200ms; easing between them keeps the playhead moving smoothly.
  const glide = { transform: `translateX(${x}px)` };
  return (
    <div ref={scroller} data-testid="timeline" className="flex-1 min-h-0 overflow-auto relative bg-ink-950">
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
        <Tracks timeline={timeline} width={width} selected={selected} onSelect={onSelect} />
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
  onSelect,
}: {
  timeline: TimelineView;
  width: number;
  selected: string | undefined;
  onSelect: (module: string) => void;
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
        <TrackRows key={t.module} track={t} index={i} selected={t.module === selected} width={width} grid={grid} onSelect={onSelect} />
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
  width,
  grid,
  onSelect,
}: {
  track: TimelineTrack;
  index: number;
  selected: boolean;
  width: number;
  grid: React.CSSProperties;
  onSelect: (module: string) => void;
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
          >
            {lane && <LaneClips lane={lane} color={color} />}
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

function LaneClips({ lane, color }: { lane: TimelineLane; color: Rgb }) {
  switch (lane.kind) {
    case "notes":
      return lane.clips.map((c, i) => {
        const notes = audible(c);
        return (
          <Clip key={i} clip={c} color={color} title={`${notes.length} notes, ${span(c)}`}>
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
            <Clip key={i} clip={c} color={color} title={span(c)}>
              <Curve clip={c} level={level} color={color} />
            </Clip>
          ))}
        </>
      );
    }
    case "audio":
      return lane.clips.map((c, i) => (
        <Clip key={i} clip={c} color={color} title={`${c.asset}, ${span(c)}`}>
          <div className="absolute inset-x-0 bottom-0 flex items-center gap-1.5 px-2 text-[11px] font-medium text-black/70" style={{ top: BODY_TOP }}>
            <AudioWaveform className="size-3.5 shrink-0" style={{ color: dark(color) }} />
            <span className="truncate">{c.asset}</span>
          </div>
        </Clip>
      ));
  }
}

function Clip({ clip, color, title, children }: { clip: { start: number; length: number }; color: Rgb; title: string; children: ReactNode }) {
  return (
    <div
      data-clip=""
      title={title}
      className="absolute rounded-lg overflow-hidden ring-1 ring-black/15"
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

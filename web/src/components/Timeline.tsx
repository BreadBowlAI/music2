import clsx from "clsx";
import { Activity, AudioWaveform, Drum, KeyboardMusic, Layers, Music2, Piano, Plus, Sparkles, Trash2, Waves, Wind } from "lucide-react";
import { AUTO_H, AutomationHeader, AutomationLaneView } from "./AutomationLane";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { contextMenu, kbd, menu, nativeAction, type MenuEntry } from "../lib/native";
import { rgb, songEnd, ticksPerBar, ticksPerBeat } from "../lib/music";
import { useSong, useStore } from "../lib/store";
import type { Asset, Clip, Track } from "../lib/types";

const HEADER_W = 236;
const RULER_H = 34;
const LANE_H = 76;

export const ADD_TRACK = [
  { label: "Drums", plugin: "m2/drums", icon: Drum },
  { label: "Bass", plugin: "m2/bass", icon: Waves },
  { label: "Chords", plugin: "m2/pad", icon: Sparkles },
  { label: "Keys", plugin: "m2/keys", icon: Piano },
  { label: "Pluck", plugin: "m2/pluck", icon: KeyboardMusic },
  { label: "Lead", plugin: "m2/lead", icon: Music2 },
  { label: "Strings", plugin: "m2/strings", icon: Wind },
  { label: "Audio", plugin: "audio", icon: AudioWaveform },
  { label: "Bus", plugin: "bus", icon: Layers },
];

const peaksCache = new Map<number, number[]>();

function usePeaks(asset: number) {
  const [peaks, setPeaks] = useState(peaksCache.get(asset) ?? null);
  useEffect(() => {
    if (peaks) return;
    api.peaks(asset).then((r) => {
      peaksCache.set(asset, r.peaks);
      setPeaks(r.peaks);
    }).catch(() => {});
  }, [asset, peaks]);
  return peaks;
}

function MidiPreview({ clip, color }: { clip: Clip; color: string }) {
  if (clip.content.type !== "midi" || clip.content.notes.length === 0) return null;
  const notes = clip.content.notes;
  const lo = Math.min(...notes.map((n) => n.pitch));
  const hi = Math.max(...notes.map((n) => n.pitch));
  const span = Math.max(hi - lo, 8);
  return (
    <svg className="absolute left-1.5 top-6 w-[calc(100%-12px)] h-[calc(100%-30px)]" viewBox={`0 0 ${clip.length} 100`} preserveAspectRatio="none">
      {notes.filter((n) => n.start < clip.length).map((n, i) => (
        <rect key={i} x={n.start} width={Math.max(Math.min(n.length, clip.length - n.start), clip.length / 400)} y={92 - ((n.pitch - lo) / span) * 84} height={8} rx={1} fill={color} />
      ))}
    </svg>
  );
}

function AudioPreview({ clip, assets, tpb, bpm, color }: { clip: Clip; assets: Asset[]; tpb: number; bpm: number; color: string }) {
  const asset = clip.content.type === "audio" ? clip.content.asset : -1;
  const peaks = usePeaks(asset);
  const a = assets.find((x) => x.id === asset);
  if (!peaks || !a || clip.content.type !== "audio") return null;
  const secs = (clip.length / tpb) * (60 / bpm);
  const n = 160;
  const pts = Array.from({ length: n }, (_, i) => {
    const t = clip.content.type === "audio" ? clip.content.offset + (i / n) * secs : 0;
    const bin = Math.floor((t / Math.max(a.duration, 0.01)) * peaks.length);
    return bin < peaks.length ? peaks[bin] : 0;
  });
  return (
    <svg className="absolute left-1.5 top-6 w-[calc(100%-12px)] h-[calc(100%-30px)]" viewBox={`0 0 ${n} 100`} preserveAspectRatio="none">
      {pts.map((p, i) => <rect key={i} x={i} width={0.7} y={50 - p * 46} height={Math.max(p * 92, 1)} fill={color} />)}
    </svg>
  );
}

type Drag = { clip: number; resize: boolean; grabTicks: number; lastKey: string };

export default function Timeline() {
  const song = useSong()!;
  const server = useStore((s) => s.server!);
  const ppb = useStore((s) => s.ppb);
  const selectedClip = useStore((s) => s.selectedClip);
  const selectedTrack = useStore((s) => s.selectedTrack);
  const { set, run } = useStore.getState();
  const scroller = useRef<HTMLDivElement>(null);
  const lanesRef = useRef<HTMLDivElement>(null);
  const drag = useRef<Drag | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [renaming, setRenaming] = useState<number | null>(null);

  const tpb = ticksPerBeat(song);
  const bar = ticksPerBar(song);
  const bpb = song.time_sig.num;
  const bars = Math.max(Math.ceil(songEnd(song) / bar) + 8, 32);
  const width = bars * bpb * ppb;
  const xOf = (t: number) => (t / tpb) * ppb;
  const tickAt = (clientX: number) => {
    const r = lanesRef.current!.getBoundingClientRect();
    return Math.max(0, ((clientX - r.left) / ppb) * tpb);
  };

  // Pinch / ctrl-scroll to zoom.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      const z = Math.exp(-e.deltaY * 0.01);
      set({ ppb: Math.min(160, Math.max(6, useStore.getState().ppb * z)) });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [set]);

  const startDrag = (e: React.PointerEvent, track: Track, clip: Clip) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const el = e.currentTarget as HTMLElement;
    const rect = el.getBoundingClientRect();
    const resize = e.clientX > rect.right - 10;
    drag.current = { clip: clip.id, resize, grabTicks: tickAt(e.clientX) - clip.start, lastKey: "" };
    set({ selectedClip: clip.id, selectedTrack: track.id });
    api.group("begin", resize ? "Resize clip" : "Move clip");
    const move = (ev: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      const s = useStore.getState().server!.song;
      const loc = s.tracks.flatMap((t, ti) => t.clips.filter((c) => c.id === d.clip).map((c) => ({ c, ti })))[0];
      if (!loc) return;
      const snap = ev.altKey ? tpb : bar;
      if (d.resize) {
        const end = Math.round(tickAt(ev.clientX) / snap) * snap;
        const len = Math.max(end - loc.c.start, Math.min(snap, tpb));
        const key = `len${len}`;
        if (key !== d.lastKey && len !== loc.c.length) {
          d.lastKey = key;
          run("clip.set", { clip: d.clip, length_bars: len / bar });
        }
      } else {
        const start = Math.max(0, Math.round((tickAt(ev.clientX) - d.grabTicks) / snap) * snap);
        const r = lanesRef.current!.getBoundingClientRect();
        // Rows can be taller when an automation lane is open under them.
        const st = useStore.getState();
        let row = 0;
        for (let i = 0, top = 0; i < s.tracks.length; i++) {
          if (ev.clientY - r.top >= top) row = i;
          top += LANE_H + (st.autoOpen[s.tracks[i].id] ? AUTO_H : 0);
        }
        const target = s.tracks[row];
        const params: Record<string, unknown> = { clip: d.clip };
        if (start !== loc.c.start) params.start_bar = start / bar + 1;
        if (row !== loc.ti && target.kind === s.tracks[loc.ti].kind) {
          params.track = target.id;
          set({ selectedTrack: target.id });
        }
        const key = JSON.stringify(params);
        if (Object.keys(params).length > 1 && key !== d.lastKey) {
          d.lastKey = key;
          run("clip.set", params);
        }
      }
    };
    const up = () => {
      drag.current = null;
      api.group("end");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // Stable across renders so memoized lanes don't re-render when only other tracks change.
  const handlers = useRef<LaneHandlers>(null!);
  handlers.current = {
    startDrag,
    addClipAt: async (track, clientX) => {
      const b = Math.floor(tickAt(clientX) / bar) + 1;
      const r = await run("clip.add", { track: track.id, start_bar: b, length_bars: 1 });
      if (r) set({ selectedClip: r.clip_id, selectedTrack: track.id });
    },
    barAt: (clientX) => Math.floor(tickAt(clientX) / bar) + 1,
  };
  const soloing = song.tracks.some((x) => x.solo);
  const autoOpen = useStore((s) => s.autoOpen);
  const plugins = server.plugins;
  const laneOf = (t: Track) => autoOpen[t.id];

  const gridBg = useMemo(
    () => ({
      backgroundImage: `linear-gradient(to right, rgba(0,0,0,0.056) 1px, transparent 1px), linear-gradient(to right, rgba(0,0,0,${ppb > 10 ? 0.025 : 0}) 1px, transparent 1px)`,
      backgroundSize: `${bpb * ppb}px 100%, ${ppb}px 100%`,
    }),
    [ppb, bpb],
  );

  return (
    <div ref={scroller} className="flex-1 overflow-auto relative bg-ink-950 min-h-0">
      <div className="relative" style={{ width: HEADER_W + width, minHeight: "100%" }}>
        {/* Ruler */}
        <div className="sticky top-0 z-30 flex" style={{ height: RULER_H }}>
          <div className="sticky left-0 z-40 shrink-0 bg-ink-900 border-b border-r border-black/[0.06] flex items-center px-4 text-[11px] font-semibold tracking-wider text-ink-400" style={{ width: HEADER_W }}>
            TRACKS
          </div>
          <div
            className="relative bg-ink-900/95 backdrop-blur border-b border-black/[0.06] cursor-pointer"
            style={{ width }}
            onPointerDown={(e) => {
              const r = e.currentTarget.getBoundingClientRect();
              const seek = (x: number) => api.transport("seek", Math.round(((x - r.left) / ppb) * 1) * tpb);
              seek(e.clientX);
              const mv = (ev: PointerEvent) => seek(ev.clientX);
              const up = () => (window.removeEventListener("pointermove", mv), window.removeEventListener("pointerup", up));
              window.addEventListener("pointermove", mv);
              window.addEventListener("pointerup", up);
            }}
          >
            {server.looping && (
              <div className="absolute bottom-0 h-1 rounded-full pop-gradient opacity-70" style={{ left: 0, width: xOf(server.end_tick) }} />
            )}
            {Array.from({ length: bars }, (_, i) =>
              ppb * bpb > 36 || i % (ppb * bpb > 18 ? 2 : 4) === 0 ? (
                <span key={i} className="absolute top-2 text-[11px] font-medium text-ink-400 tabular-nums" style={{ left: i * bpb * ppb + 6 }}>
                  {i + 1}
                </span>
              ) : null,
            )}
          </div>
        </div>

        {/* Tracks */}
        <div className="flex">
          <div
            className="sticky left-0 z-20 shrink-0 bg-ink-900 border-r border-black/[0.06]"
            style={{ width: HEADER_W }}
            onContextMenu={(e) => contextMenu(e, [addTrackMenu()])}
          >
            {song.tracks.map((t, i) => (
              <div key={t.id}>
                <TrackHeader
                  track={t}
                  index={i}
                  count={song.tracks.length}
                  soloing={soloing}
                  selected={selectedTrack === t.id}
                  renaming={renaming === t.id}
                  setRenaming={setRenaming}
                  autoOpen={!!laneOf(t)}
                />
                {laneOf(t) && <AutomationHeader track={t} laneKey={laneOf(t)!} plugins={plugins} />}
              </div>
            ))}
            <div className="relative p-3">
              <button
                onClick={() => setAddOpen(!addOpen)}
                className="w-full h-10 rounded-xl border border-dashed border-ink-600 text-ink-300 hover:text-ink-100 hover:border-ink-400 text-sm font-medium flex items-center justify-center gap-1.5 cursor-pointer transition"
              >
                <Plus className="size-4" /> Add track
              </button>
              {addOpen && (
                <>
                  <div className="fixed inset-0 z-40" onClick={() => setAddOpen(false)} />
                  <div className="absolute left-3 top-14 z-50 w-52 p-1.5 rounded-2xl glass ring-1 ring-black/10 shadow-2xl rise">
                    {ADD_TRACK.map((a) => (
                      <button
                        key={a.plugin}
                        onClick={async () => {
                          setAddOpen(false);
                          const r = await run("track.add", { name: a.label, instrument: a.plugin });
                          if (r) set({ selectedTrack: r.track_id });
                        }}
                        className="w-full flex items-center gap-2.5 px-3 h-9 rounded-lg text-sm text-ink-200 hover:bg-ink-700 cursor-pointer"
                      >
                        <a.icon className="size-4 text-ink-400" /> {a.label}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>

          <div ref={lanesRef} data-bar-px={bpb * ppb} className="relative" style={{ width }}>
            {song.tracks.map((t, i) => (
              <div key={t.id}>
              <Lane
                track={t}
                index={i}
                selected={selectedTrack === t.id}
                selectedClip={t.clips.some((c) => c.id === selectedClip) ? selectedClip : null}
                muted={t.mute || (soloing && !t.solo)}
                ppb={ppb}
                tpb={tpb}
                bpm={song.bpm}
                assets={song.assets}
                gridBg={gridBg}
                handlers={handlers}
              />
              {laneOf(t) && <AutomationLaneView track={t} laneKey={laneOf(t)!} ppb={ppb} tpb={tpb} barTicks={bar} width={width} />}
              </div>
            ))}
            {song.tracks.length === 0 && (
              <div className="absolute left-12 top-10 max-w-md space-y-2 text-ink-400">
                <div className="text-lg font-semibold text-ink-200">Your song is empty</div>
                <div className="text-sm leading-relaxed">Add a track on the left, or drag in a WAV file.</div>
              </div>
            )}
          </div>
        </div>

        <Playhead scroller={scroller} />
      </div>

    </div>
  );
}

function Playhead({ scroller }: { scroller: React.RefObject<HTMLDivElement | null> }) {
  const playhead = useStore((s) => s.tick.playhead);
  const playing = useStore((s) => s.tick.playing);
  const song = useSong()!;
  const ppb = useStore((s) => s.ppb);
  const x = (playhead / ticksPerBeat(song)) * ppb;
  useEffect(() => {
    const el = scroller.current;
    if (!el || !playing) return;
    const visible = x - el.scrollLeft;
    if (visible > el.clientWidth - HEADER_W - 40 || visible < 0) el.scrollLeft = Math.max(0, x - 80);
  }, [x, playing, scroller]);
  return (
    <div className="absolute top-0 bottom-0 z-[25] pointer-events-none" style={{ left: HEADER_W + x }}>
      <div className="absolute top-0 -translate-x-1/2 w-3 h-3 rotate-45 -mt-1.5 bg-sun rounded-sm" style={{ top: RULER_H - 6 }} />
      <div className="absolute top-0 bottom-0 w-px bg-sun shadow-[0_0_8px_rgba(201,165,78,0.35)]" />
    </div>
  );
}

function laneKeyOf(t: Track["automation"][number]["target"]) {
  return t.param === "macro" ? `macro:${t.device}:${t.macro}` : t.param;
}

const TrackHeader = memo(function TrackHeader({
  track: t,
  soloing,
  index,
  count,
  selected,
  renaming,
  setRenaming,
  autoOpen,
}: {
  track: Track;
  index: number;
  count: number;
  soloing: boolean;
  selected: boolean;
  renaming: boolean;
  setRenaming: (id: number | null) => void;
  /** Whether the automation lane under this track is open. */
  autoOpen: boolean;
}) {
  const plugins = useStore((s) => s.server!.plugins);
  const peak = useStore((s) => s.tick.peaks[String(t.id)] ?? 0);
  const { set, run } = useStore.getState();
  const plugin = t.instrument ? plugins.find((p) => p.id === t.instrument!.plugin) : null;
  const output = useStore((s) => (t.output ? s.server!.song.tracks.find((x) => x.id === t.output)?.name : null));
  const sub = t.kind === "audio" ? "Audio" : t.kind === "bus" ? "Bus" : plugin?.name ?? `missing: ${t.instrument?.plugin}`;
  const [name, setName] = useState(t.name);
  return (
    <div
      className={clsx("relative flex items-center gap-3 pl-4 pr-3 border-b border-black/[0.05] group transition-colors", selected ? "bg-ink-800" : "hover:bg-ink-850")}
      style={{ height: LANE_H }}
      onPointerDown={() => set({ selectedTrack: t.id, selectedClip: null })}
      onDoubleClick={() => (setName(t.name), setRenaming(t.id))}
      onContextMenu={(e) => {
        set({ selectedTrack: t.id, selectedClip: null });
        contextMenu(e, trackMenu(t, index, count, () => (setName(t.name), setRenaming(t.id))));
      }}
    >
      <div className="absolute left-0 top-3 bottom-3 w-1 rounded-r-full" style={{ background: rgb(t.color) }} />
      <div className="flex-1 min-w-0">
        {renaming ? (
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            onBlur={() => (setRenaming(null), name.trim() && name !== t.name && run("track.set", { track: t.id, name }))}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            className="w-full h-7 px-2 -ml-2 rounded-lg bg-ink-950 ring-1 ring-pop-500/60 outline-none text-sm font-semibold"
          />
        ) : (
          <div className={clsx("text-sm font-semibold truncate", t.mute || (soloing && !t.solo) ? "text-ink-400" : "text-ink-100")}>{t.name}</div>
        )}
        <div className="text-xs text-ink-400 truncate">
          {sub}
          {output && <span className="text-ink-500"> → {output}</span>}
        </div>
      </div>
      <div className="flex items-center gap-1">
        <button
          title="Automation"
          onClick={(e) => {
            e.stopPropagation();
            const st = useStore.getState();
            const next = { ...st.autoOpen };
            if (autoOpen) delete next[t.id];
            else next[t.id] = t.automation[0] ? laneKeyOf(t.automation[0].target) : "volume";
            set({ autoOpen: next });
          }}
          className={clsx(
            "size-6 rounded-md grid place-items-center cursor-pointer",
            autoOpen || t.automation.length ? "text-pop-400" : "text-ink-500 hover:text-ink-200 opacity-0 group-hover:opacity-100",
          )}
        >
          <Activity className="size-3.5" />
        </button>
        <button
          title="Delete track"
          onClick={(e) => (e.stopPropagation(), run("track.remove", { track: t.id }), useStore.getState().toast(`Deleted ${t.name} — ${kbd("Z")} to undo`))}
          className="size-6 rounded-md grid place-items-center text-ink-500 hover:text-red-700 opacity-0 group-hover:opacity-100 cursor-pointer"
        >
          <Trash2 className="size-3.5" />
        </button>
        <button
          title="Mute"
          onClick={(e) => (e.stopPropagation(), run("track.set", { track: t.id, mute: !t.mute }))}
          className={clsx("size-6 rounded-md text-[11px] font-bold cursor-pointer transition", t.mute ? "bg-ink-300 text-white" : "bg-ink-700/70 text-ink-400 hover:text-ink-100")}
        >
          M
        </button>
        <button
          title="Solo"
          onClick={(e) => (e.stopPropagation(), run("track.set", { track: t.id, solo: !t.solo }))}
          className={clsx("size-6 rounded-md text-[11px] font-bold cursor-pointer transition", t.solo ? "bg-sun text-white" : "bg-ink-700/70 text-ink-400 hover:text-ink-100")}
        >
          S
        </button>
      </div>
      <div className="w-1 h-10 rounded-full bg-ink-700 overflow-hidden flex items-end">
        <div className="w-full rounded-full transition-[height] duration-75" style={{ height: `${Math.min(100, Math.sqrt(peak) * 100)}%`, background: rgb(t.color) }} />
      </div>
    </div>
  );
});

type LaneHandlers = {
  startDrag: (e: React.PointerEvent, track: Track, clip: Clip) => void;
  addClipAt: (track: Track, clientX: number) => void;
  barAt: (clientX: number) => number;
};

const addTrackMenu = (): MenuEntry => ({
  label: "Add Track",
  submenu: ADD_TRACK.map((a) => ({
    label: a.label,
    action: async () => {
      const r = await useStore.getState().run("track.add", { name: a.label, instrument: a.plugin });
      if (r) useStore.getState().set({ selectedTrack: r.track_id });
    },
  })),
});

function clipMenu(t: Track, c: Clip): MenuEntry[] {
  const { set, run } = useStore.getState();
  const midi = c.content.type === "midi";
  const transpose = (semitones: number) => () => run("notes.transpose", { clip: c.id, semitones });
  return menu(
    midi && { label: "Edit Notes", action: () => set({ selectedClip: c.id, selectedTrack: t.id }) },
    { label: "Rename…", action: () => set({ renamingClip: c.id }) },
    {
      label: "Duplicate",
      shortcut: kbd("D"),
      action: async () => {
        const r = await run("clip.duplicate", { clip: c.id });
        if (r) set({ selectedClip: r.clip_ids[0] });
      },
    },
    "-",
    midi && {
      label: "Transpose",
      submenu: [
        { label: "Octave Up", action: transpose(12) },
        { label: "Semitone Up", action: transpose(1) },
        { label: "Semitone Down", action: transpose(-1) },
        { label: "Octave Down", action: transpose(-12) },
      ],
    },
    midi && { label: "Quantize to 1/16", action: () => run("notes.quantize", { clip: c.id, grid: 0.25 }) },
    midi && { label: "Humanize", action: () => run("notes.velocity", { clip: c.id, humanize: 0.15 }) },
    midi && "-",
    { label: "Delete", shortcut: "⌫", danger: true, action: () => (run("clip.remove", { clip: c.id }), set({ selectedClip: null })) },
  );
}

function laneMenu(t: Track, clientX: number, handlers: LaneHandlers): MenuEntry[] {
  const bar = handlers.barAt(clientX);
  if (t.kind === "bus") return [addTrackMenu()];
  return menu(
    t.kind === "instrument" && { label: `Add Clip at Bar ${bar}`, action: () => handlers.addClipAt(t, clientX) },
    { label: `Import Audio at Bar ${bar}…`, action: () => nativeAction("import", { bar }) },
    "-",
    addTrackMenu(),
  );
}

function trackMenu(t: Track, index: number, count: number, rename: () => void): MenuEntry[] {
  const { run } = useStore.getState();
  return [
    { label: "Rename…", action: rename },
    "-",
    { label: "Mute", checked: t.mute, action: () => run("track.set", { track: t.id, mute: !t.mute }) },
    { label: "Solo", checked: t.solo, action: () => run("track.set", { track: t.id, solo: !t.solo }) },
    "-",
    { label: "Move Up", disabled: index === 0, action: () => run("track.set", { track: t.id, index: index - 1 }) },
    { label: "Move Down", disabled: index === count - 1, action: () => run("track.set", { track: t.id, index: index + 1 }) },
    "-",
    { label: "Delete Track", danger: true, action: () => run("track.remove", { track: t.id }) },
  ];
}

/** One track's lane. Memoized: with patch-based sync, unchanged tracks keep their
 *  object identity and skip re-rendering entirely. */
const Lane = memo(function Lane({
  track: t,
  index,
  selected,
  selectedClip,
  muted,
  ppb,
  tpb,
  bpm,
  assets,
  gridBg,
  handlers,
}: {
  track: Track;
  index: number;
  selected: boolean;
  selectedClip: number | null;
  muted: boolean;
  ppb: number;
  tpb: number;
  bpm: number;
  assets: Asset[];
  gridBg: React.CSSProperties;
  handlers: React.RefObject<LaneHandlers>;
}) {
  const xOf = (ticks: number) => (ticks / tpb) * ppb;
  const col = rgb(t.color);
  const { set } = useStore.getState();
  const renamingClip = useStore((s) => (t.clips.some((c) => c.id === s.renamingClip) ? s.renamingClip : null));
  return (
    <div
      className={clsx("relative border-b border-black/[0.05]", index % 2 ? "bg-ink-900/40" : "bg-ink-900/70", selected && "!bg-ink-800/80")}
      style={{ height: LANE_H, ...gridBg }}
      onPointerDown={() => set({ selectedTrack: t.id, selectedClip: null })}
      onDoubleClick={(e) => t.kind === "instrument" && handlers.current.addClipAt(t, e.clientX)}
      onContextMenu={(e) => contextMenu(e, laneMenu(t, e.clientX, handlers.current))}
    >
      {t.kind === "bus" && (
        <span className="absolute left-4 top-1/2 -translate-y-1/2 text-xs text-ink-500 pointer-events-none">Bus — route tracks here in the Track inspector → Routing</span>
      )}
      {t.clips.length === 0 && t.kind === "instrument" && (
        <span className="absolute left-4 top-1/2 -translate-y-1/2 text-xs text-ink-500 pointer-events-none">Double-click to add a clip</span>
      )}
      {t.clips.map((c) => (
        <div
          key={c.id}
          onPointerDown={(e) => handlers.current.startDrag(e, t, c)}
          onContextMenu={(e) => {
            set({ selectedClip: c.id, selectedTrack: t.id });
            contextMenu(e, clipMenu(t, c));
          }}
          className={clsx(
            "absolute top-1.5 bottom-1.5 rounded-xl overflow-hidden cursor-grab active:cursor-grabbing transition-shadow group",
            selectedClip === c.id ? "ring-2 ring-ink-100 shadow-lg shadow-black/15" : "ring-1 ring-black/20",
            muted && "opacity-40",
          )}
          style={{ left: xOf(c.start) + 1, width: Math.max(xOf(c.length) - 2, 6), background: `linear-gradient(180deg, ${rgb(t.color, 0.42)}, ${rgb(t.color, 0.22)})` }}
        >
          <div className="h-5 px-2 flex items-center text-[11px] font-semibold text-black/75 truncate" style={{ background: col }}>
            {renamingClip === c.id ? <ClipRename clip={c} /> : c.name}
          </div>
          {c.content.type === "midi" ? <MidiPreview clip={c} color={col} /> : <AudioPreview clip={c} assets={assets} tpb={tpb} bpm={bpm} color={col} />}
          <div className="absolute right-0 inset-y-0 w-2.5 cursor-ew-resize opacity-0 group-hover:opacity-100 bg-black/10" />
        </div>
      ))}
    </div>
  );
});

function ClipRename({ clip }: { clip: Clip }) {
  const [name, setName] = useState(clip.name);
  const done = (save: boolean) => {
    const { set, run } = useStore.getState();
    set({ renamingClip: null });
    if (save && name.trim() && name !== clip.name) run("clip.set", { clip: clip.id, name: name.trim() });
  };
  return (
    <input
      autoFocus
      value={name}
      onFocus={(e) => e.target.select()}
      onChange={(e) => setName(e.target.value)}
      onPointerDown={(e) => e.stopPropagation()}
      onBlur={() => done(true)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") done(true);
        if (e.key === "Escape") done(false);
      }}
      className="w-full h-4 px-1 -mx-1 rounded bg-white/80 outline-none text-[11px] font-semibold text-ink-100"
    />
  );
}

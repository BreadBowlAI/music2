import clsx from "clsx";
import { Download, Metronome, PanelLeft, Play, Redo2, Repeat, SkipBack, Square, Undo2 } from "lucide-react";
import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { barAt, barsToBeats, lockReason, position, type GraphState, type GraphStatus, type Loop, type Meter, type Problem } from "../lib/graph";
import { exportSong, useExport } from "../lib/exporting";
import { next } from "../lib/history";
import { useMetronome, toggleMetronome } from "../lib/metronome";
import { chord, keyName, shifted } from "../lib/native";
import { canvas, useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { graphUndo } from "./GraphCanvas";
import KeyField from "./KeyField";
import MidiStatus from "./MidiStatus";
import { MeterField, SongTitle, TempoField } from "./SongBar";
import OverlayPill from "./OverlayPill";
import Status from "./Status";
import { Button, keepFocus } from "./ui";
import RecordButton from "./RecordButton";

/** `bars 5–8`, or the beats when the loop isn't on bar lines. */
function loopName(meter: Meter[], l: Loop) {
  const a = barAt(meter, l.start), b = barAt(meter, l.end);
  if (a.start !== l.start || b.start !== l.end) return `beats ${Number(l.start.toFixed(3))}–${Number(l.end.toFixed(3))}`;
  // It ends where bar `b` starts: its last bar is the one before.
  return b.bar - 1 === a.bar ? `bar ${a.bar + 1}` : `bars ${a.bar + 1}–${b.bar}`;
}

/** The loop toggle (C): loops the range set, or with none set, the 4 bars from the bar the playhead is in. */
export function toggleLoop(meter: Meter[], status: GraphStatus, onLoop: (req: { start?: number; end?: number; on?: boolean }) => void) {
  const l = status.loop;
  if (l) return onLoop({ on: !l.on });
  const bar = barAt(meter, status.beat);
  onLoop({ start: bar.start, end: bar.start + barsToBeats(meter, bar.start, 4) });
}

const Divider = () => <div className="w-px h-5 mx-3 shrink-0 self-center bg-line" />;

/** The least room a song's name keeps from the bar's groups either side of it, in px. */
const NAME_GAP = 8;
/** Below this much room the name goes (taste.md §3.8): a few letters say nothing. */
const NAME_MIN = 56;

/**
 * How wide the song's name may be, centred in the bar: twice the room from the middle to the
 * nearer of the groups either side (the free space between them is `gap`'s), less a gap each
 * side (Q23). Measured, so a wider group (the key's name, the time, a status pill) cuts it sooner.
 */
function useNameRoom(bar: React.RefObject<HTMLElement | null>, gap: React.RefObject<HTMLElement | null>) {
  const [room, setRoom] = useState<number | null>(null);
  useLayoutEffect(() => {
    const b = bar.current, g = gap.current;
    if (!b || !g) return;
    const measure = () => {
      const r = b.getBoundingClientRect(), free = g.getBoundingClientRect();
      const mid = r.left + r.width / 2;
      setRoom(Math.max(0, Math.floor(2 * Math.min(mid - free.left, free.right - mid) - 2 * NAME_GAP)));
    };
    measure();
    const seen = new ResizeObserver(measure);
    seen.observe(b);
    seen.observe(g);
    return () => seen.disconnect();
  }, []);
  return room;
}

/** A top-bar button: it doesn't take focus from a click, so Space stays the transport (taste.md §2.11). */
const BarButton = (props: React.ComponentProps<typeof Button>) => <Button onMouseDown={keepFocus} {...props} />;

/**
 * The top bar (taste.md §3.3), 44px: the transport, where playback is, the tempo and meter, the
 * song's name in the middle, then status, undo and Export. Every empty part of it drags the
 * window. Items drop as the window narrows: the time first, then the song's name (§3.8).
 */
export default function TopBar({
  graph,
  status,
  onSeek,
  onLoop,
  onShowCode,
}: {
  graph: GraphState;
  status: GraphStatus;
  onSeek: (beat: number) => void;
  onLoop: (req: { start?: number; end?: number; on?: boolean }) => void;
  onShowCode: (p: Problem) => void;
}) {
  const { meter } = graph.timeline;
  const browser = useShell((s) => s.browser);
  const bar = useRef<HTMLElement>(null);
  const gap = useRef<HTMLDivElement>(null);
  const room = useNameRoom(bar, gap);
  const connected = useStore((s) => s.connected);
  const locked = lockReason(graph, connected);
  const time = `${Math.floor(status.seconds / 60)}:${(status.seconds % 60).toFixed(1).padStart(4, "0")}`;
  // Playback sits on whole frames, so a seek to a beat can land a hair before it.
  const at = position(meter, Math.round(status.beat * 1000) / 1000);
  const l = status.loop;
  const metronome = useMetronome();
  // What they would take back, as the server names it (lib/history): "Undo Reverb size (⌘Z)".
  const undoing = next(graph, false);
  const redoing = next(graph, true);
  const loopTitle = l
    ? l.on
      ? `Looping ${loopName(meter, l)}: click to play straight on (C)`
      : `Loop ${loopName(meter, l)} (C). Drag across the ruler to loop somewhere else`
    : "Loop 4 bars from the playhead (C), or drag across the ruler to choose where";
  return (
    <header
      ref={bar}
      data-tauri-drag-region
      // Above the timeline: a field's error shows under it, over the ruler. The Mac app
      // (`data-app="mac"`, main.tsx) starts 80px in, for the window's buttons: 68px more than a page.
      className="relative z-[45] h-bar shrink-0 flex items-center pl-3 pr-3 border-b border-line bg-chrome [:root[data-app=mac]_&]:pl-20"
    >
      {/* The browser's toggle, on screen so a window that starts without it (under 1280px) can show it. */}
      <BarButton
        data-testid="browser-toggle"
        active={browser}
        aria-pressed={browser}
        onClick={() => useShell.getState().setBrowser(!browser)}
        title={`Browser (${chord("B", { alt: true })})`}
        aria-label="Browser"
        className="w-ctl !px-0"
      >
        <PanelLeft size={16} />
      </BarButton>
      <Divider />
      <BarButton onClick={() => onSeek(0)} title={`Go to start (${keyName("Enter")})`} aria-label="Back to start" className="w-ctl !px-0">
        <SkipBack size={16} fill="currentColor" />
      </BarButton>
      {/* Playing is the lit state; it stops (there is no pause), so the glyph is a square. */}
      <button
        data-testid="play"
        onMouseDown={keepFocus}
        onClick={() => canvas.play?.()}
        title={status.playing ? "Stop, back to where play started (Space)" : `Play (Space) · continue from where it stopped (${shifted("Space")})`}
        aria-label={status.playing ? "Stop" : "Play"}
        className={clsx(
          "ml-1 size-ctl rounded-sm grid place-items-center cursor-pointer transition-colors duration-80 ease-linear",
          status.playing ? "bg-accent text-white hover:bg-accent-hover active:bg-accent-strong" : "text-gray-700 hover:bg-hover active:bg-press",
        )}
      >
        {status.playing ? <Square size={14} fill="currentColor" /> : <Play size={16} fill="currentColor" />}
      </button>
      {/* Record (R), right after Play / Stop (lane B): everything to its right moves 32 px. */}
      <RecordButton />
      <BarButton
        data-testid="loop-toggle"
        aria-label="Loop"
        aria-pressed={!!l?.on}
        active={!!l?.on}
        onClick={() => toggleLoop(meter, status, onLoop)}
        title={loopTitle}
        className="ml-1 w-ctl !px-0"
      >
        <Repeat size={16} />
      </BarButton>
      {/* The metronome (gaps #19), after Loop (taste.md §3.3). */}
      <BarButton
        data-testid="metronome-toggle"
        aria-label="Metronome"
        aria-pressed={metronome}
        active={metronome}
        onClick={toggleMetronome}
        title="Metronome: a click on each beat (K)"
        className="ml-1 w-ctl !px-0"
      >
        <Metronome size={16} />
      </BarButton>
      <Divider />
      {/* Where playback is, on one baseline. The position is right-aligned in its 72px, so its last
          digits stay put as it counts (5.3.2 → 10.1.1). */}
      <div className="flex items-baseline">
        <span data-testid="position" className="w-18 text-right text-lg font-medium text-fg" title="Bar, beat and sixteenth">{at}</span>
        {/* It goes first as the window narrows (§3.8); in the Mac app, whose bar starts further in, sooner.
            From 1212 px (1312 in the Mac app): Record took 32 px more of the bar (lane B, D39). */}
        <span className="w-13 ml-3 text-sm text-fg-muted max-[1212px]:hidden [:root[data-app=mac]_&]:max-[1312px]:hidden" title="Time">
          {time}
        </span>
      </div>
      <Divider />
      {/* The tempo and time signature in effect at the playhead, to drag, type or pick (TB-5). */}
      <div className="flex items-center gap-1 -ml-2" data-testid="tempo">
        <TempoField session={graph.session} timeline={graph.timeline} at={status.beat} locked={locked} />
        <MeterField session={graph.session} timeline={graph.timeline} at={status.beat} locked={locked} />
        {/* The song's key (D2), after the time signature (taste.md §3.3). */}
        <KeyField graph={graph} />
      </div>
      <div ref={gap} data-tauri-drag-region className="flex-1 self-stretch" />
      {/* Centred in the window, whatever the transport and buttons beside it take: kept clear of
          them as measured (Q23), a long name is cut short, and with too little room it goes (§3.8).
          Not while it is being typed: hidden, the field would lose the keyboard, and with it write
          what was typed half way. The field keeps its width then, over its neighbours (PL-T1). */}
      <div
        className={clsx(
          "absolute inset-y-0 left-1/2 -translate-x-1/2 flex items-center whitespace-nowrap pointer-events-none max-w-(--room) has-[input]:max-w-none has-[input]:z-10",
          (room ?? 0) < NAME_MIN && "hidden has-[input]:flex",
        )}
        style={{ "--room": `${room ?? 0}px` } as CSSProperties}
      >
        <SongTitle session={graph.session} name={graph.name} dir={graph.dir} rate={graph.sample_rate} locked={locked} room={room ?? 0} />
      </div>
      {/* A MIDI keyboard plugged in (8b.10), before the status pill. */}
      <MidiStatus graph={graph} />
      <Status graph={graph} status={status} onShowCode={onShowCode} />
      <OverlayPill graph={graph} />
      <Divider />
      <BarButton
        onClick={() => void graphUndo(graph.session, "undo")}
        disabled={!graph.can_undo}
        title={`Undo${undoing ? ` ${undoing.name}` : ""} (${chord("Z")})`}
        aria-label="Undo"
        className="w-ctl !px-0"
      >
        <Undo2 size={16} />
      </BarButton>
      <BarButton
        onClick={() => void graphUndo(graph.session, "redo")}
        disabled={!graph.can_redo}
        title={`Redo${redoing ? ` ${redoing.name}` : ""} (${chord("Z", { shift: true })})`}
        aria-label="Redo"
        className="w-ctl !px-0"
      >
        <Redo2 size={16} />
      </BarButton>
      <ExportButton dir={graph.dir} />
    </header>
  );
}

/** Export (⇧⌘E): the Export dialog (the song, its stems or its notes). While it renders, a ring fills with
 * its progress. Another song's export (one opened before this) shows in a toast instead: see Toasts. */
function ExportButton({ dir }: { dir: string }) {
  const progress = useExport((s) => (s.dir === dir ? s.progress : null));
  const stem = useExport((s) => (s.dir === dir ? s.stem : null));
  // One width, idle or rendering: Undo and Redo beside it never move while it renders (principle 8).
  if (progress === null) {
    return (
      <BarButton data-testid="export" onClick={() => void exportSong()} title={`Export the song, its stems or its notes (${chord("E", { shift: true })})`} className="ml-1 w-21">
        <Download size={14} />
        Export
      </BarButton>
    );
  }
  const percent = Math.round(progress * 100);
  // r = 6 in a 16-unit box: the ring's length, for the dash that fills it.
  const length = 2 * Math.PI * 6;
  return (
    <BarButton data-testid="export" aria-busy title={stem ? `Exporting stems: ${stem.name} (${stem.n} of ${stem.of})` : `Exporting the song… ${percent}%`} className="ml-1 w-21 !text-fg !bg-hover cursor-default">
      <svg
        data-testid="export-progress"
        role="progressbar"
        aria-label="Export progress"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        viewBox="0 0 16 16"
        className="size-3.5 -rotate-90"
      >
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" strokeOpacity={0.15} strokeWidth="2.25" />
        <circle
          cx="8"
          cy="8"
          r="6"
          fill="none"
          stroke="var(--color-accent)"
          strokeWidth="2.25"
          strokeLinecap="round"
          strokeDasharray={length}
          strokeDashoffset={length * (1 - Math.max(progress, 0.02))}
          className="transition-[stroke-dashoffset] duration-140"
        />
      </svg>
      {/* The ring takes the icon's place, the percentage the label's (taste.md §3.3). */}
      <span className="w-8 text-left">{percent}%</span>
    </BarButton>
  );
}

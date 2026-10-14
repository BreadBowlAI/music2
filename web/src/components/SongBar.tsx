import clsx from "clsx";
import { Minus, Plus } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { barAt, graphApi, type Meter, type TimelineView } from "../lib/graph";
import { canvas } from "../lib/shell";
import { useStore } from "../lib/store";
import { Button, MenuRow, Popover, Segment } from "./ui";

// The song bar (design-8b §4, taste.md §3.3, TB-5): the tempo and the time signature in effect at
// the playhead, and the song's name, each edited where it shows. Each edit is one undo step and
// makes no toast: what it did is on screen (taste.md §2.14). The same fields edit a change on the
// ruler, in a popover at its mark.

/** Where the tempo and time signature are kept (HOME-1): no marker, said in the tooltip (taste.md §2.10). */
const HOME = "Saved in data/arrangement.json";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** `124`, `128.5`: BPM with up to 2 decimals (taste.md, Words and numbers). */
export const bpmText = (bpm: number) => String(Number(bpm.toFixed(2)));

/** The index of the entry in effect at `beat`: the last at or before it. */
function inEffect(entries: readonly { beat: number }[], beat: number) {
  let i = 0;
  entries.forEach((e, j) => e.beat <= beat + 1e-9 && (i = j));
  return i;
}

/** The tempo entry in effect at `beat`. */
export const tempoAt = (timeline: TimelineView, beat: number) => timeline.tempo[inEffect(timeline.tempo, beat)] ?? { beat: 0, bpm: 120 };

/** The time signature in effect at `beat`: 4/4 from the start when none is written. */
export const meterAt = (timeline: TimelineView, beat: number): Meter =>
  timeline.meter.length ? timeline.meter[inEffect(timeline.meter, beat)] : { beat: 0, beats: 4, unit: 4 };

/** Where a change starts, as the ruler counts: `bar 9`, or `9.3` off a bar line. */
export function placeOf(meter: readonly Meter[], beat: number) {
  const b = barAt(meter, beat);
  const into = Number((beat - b.start).toFixed(3));
  return into === 0 ? `bar ${b.bar + 1}` : `${b.bar + 1}.${Number((into + 1).toFixed(3))}`;
}

/** A typed tempo ("128", "128.5", "128 bpm"), when it is one FMT-13 allows. */
function typedBpm(text: string) {
  const m = /^\s*(\d+(?:\.\d+)?)\s*(?:bpm)?\s*$/i.exec(text);
  const bpm = m ? Number(m[1]) : NaN;
  return bpm >= 20 && bpm <= 999 ? Number(bpm.toFixed(2)) : null;
}

/**
 * The value bubble over the pointer while a value is dragged (taste.md §2.14); under it when the
 * pointer is too near the window's top for it to show (the top bar's tempo, dragged up).
 */
function Bubble({ at, children }: { at: { x: number; y: number }; children: React.ReactNode }) {
  const under = at.y < 40;
  return createPortal(
    <div
      aria-hidden
      data-testid="value-bubble"
      className={clsx(
        "fixed z-50 pointer-events-none -translate-x-1/2 px-1.5 py-0.5 rounded-sm bg-fg-strong/92 text-white text-xs tabular-nums whitespace-nowrap",
        !under && "-translate-y-full",
      )}
      style={{ left: at.x, top: under ? Math.max(at.y + 20, 48) : at.y - 12 }}
    >
      {children}
    </div>,
    document.body,
  );
}

/**
 * The tempo field (taste.md §3.3): "124" and "BPM". Drag up or down, 1 BPM per 4px (⇧: 0.1);
 * double-click or Return to type "128", "128.5" or "128 bpm". It edits the tempo in effect at
 * `at`. Until the tempo lease (8b.7) a drag is heard when it is let go. No ⌥-click reset: a
 * stray click would reset the song's tempo (taste.md §5.2).
 */
export function TempoField({
  session,
  timeline,
  at,
  locked,
  testid = "tempo-field",
  typing = false,
}: {
  session: number;
  timeline: TimelineView;
  at: number;
  locked: string | null;
  testid?: string;
  /** Open as a field to type in (a new change's popover). */
  typing?: boolean;
}) {
  const toast = useStore((s) => s.toast);
  const entry = tempoAt(timeline, at);
  const [pending, setPending] = useState<number | null>(null);
  const [drag, setDrag] = useState<{ value: number; x: number; y: number } | null>(null);
  const [draft, setDraft] = useState<string | null>(typing ? bpmText(entry.bpm) : null);
  const [error, setError] = useState<string | null>(null);
  const held = useRef<{ pointer: number; y: number; tenths: number; start: number; value: number } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (pending !== null && Math.abs(entry.bpm - pending) < 1e-9) setPending(null);
  }, [entry.bpm, pending]);
  const editing = draft !== null;
  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);
  const shown = drag?.value ?? pending ?? entry.bpm;

  const commit = (bpm: number) => {
    if (Math.abs(bpm - entry.bpm) < 1e-9) return;
    setPending(bpm);
    graphApi
      .tempo(session, { at: entry.beat, bpm }, { name: "Tempo" })
      .then((r) => r.loop_off && toast(`Loop turned off: it's too short to play at ${bpmText(bpm)} BPM`))
      .catch((e) => {
        setPending(null);
        toast(message(e));
      });
  };
  const type = (text: string, leaving: boolean) => {
    const bpm = typedBpm(text);
    if (bpm === null) {
      // Leaving the field with nothing it can write puts the tempo back.
      if (leaving) setDraft(null);
      setError(leaving ? null : "A tempo is 20 to 999 BPM");
      return;
    }
    setDraft(null);
    setError(null);
    commit(bpm);
  };

  const many = timeline.tempo.length > 1;
  const title = locked
    ? `Tempo: ${bpmText(shown)} BPM\n${locked}`
    : [
        many ? `Tempo from ${placeOf(timeline.meter, entry.beat).replace("bar 1", "the start")}: ${bpmText(shown)} BPM` : "Tempo",
        "Drag up or down (⇧ for fine), or double-click to type. Heard when you let go",
        HOME,
      ].join("\n");

  if (draft !== null) {
    return (
      <span className="relative inline-flex">
        <input
          ref={input}
          data-testid={`${testid}-input`}
          aria-label="Tempo in BPM"
          aria-invalid={!!error}
          value={draft}
          autoFocus
          spellCheck={false}
          onChange={(e) => {
            setDraft(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") type(draft, false);
            else if (e.key === "Escape") {
              setDraft(null);
              setError(null);
            }
          }}
          onBlur={() => type(draft, true)}
          className={clsx(
            "w-19 h-ctl px-2 rounded-sm bg-raised border text-base font-medium text-fg tabular-nums outline-none",
            error ? "border-danger" : "border-accent",
          )}
        />
        {error && (
          <span role="alert" className="absolute left-0 top-full mt-1 z-10 px-1.5 py-0.5 rounded-sm bg-raised shadow-pop text-xs text-danger whitespace-nowrap">
            {error}
          </span>
        )}
      </span>
    );
  }
  return (
    <span
      role="spinbutton"
      tabIndex={locked ? -1 : 0}
      data-testid={testid}
      data-bpm={shown}
      aria-label="Tempo"
      aria-valuemin={20}
      aria-valuemax={999}
      aria-valuenow={shown}
      aria-valuetext={`${bpmText(shown)} BPM`}
      aria-disabled={!!locked}
      title={title}
      className={clsx(
        "w-19 h-ctl px-2 rounded-sm inline-flex items-center justify-center select-none touch-none",
        !locked && "cursor-ns-resize hover:bg-hover transition-colors duration-80 ease-linear",
        drag && "bg-press",
      )}
      // Not focused by a click, so Space stays the transport (taste.md §2.11).
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={(e) => {
        // ⌥-click does nothing here: no reset of the song's tempo (taste.md §5.2).
        if (locked || e.button !== 0 || e.altKey || held.current) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        held.current = { pointer: e.pointerId, y: e.clientY, tenths: 0, start: entry.bpm, value: entry.bpm };
      }}
      onPointerMove={(e) => {
        const h = held.current;
        if (!h || h.pointer !== e.pointerId) return;
        // In tenths of a BPM: 4px is 1 BPM, or 0.1 with ⇧.
        h.tenths += ((h.y - e.clientY) * (e.shiftKey ? 1 : 10)) / 4;
        h.y = e.clientY;
        h.value = Math.min(999, Math.max(20, Number((h.start + Math.trunc(h.tenths) / 10).toFixed(1))));
        if (drag || Math.abs(h.tenths) >= 10 || (e.shiftKey && Math.abs(h.tenths) >= 1)) setDrag({ value: h.value, x: e.clientX, y: e.clientY });
      }}
      onPointerUp={(e) => {
        const h = held.current;
        if (!h || h.pointer !== e.pointerId) return;
        held.current = null;
        setDrag(null);
        if (h.value !== h.start) commit(h.value);
      }}
      onPointerCancel={() => {
        held.current = null;
        setDrag(null);
      }}
      onDoubleClick={() => !locked && setDraft(bpmText(shown))}
      onKeyDown={(e) => {
        if (e.key !== "Enter" || locked) return;
        e.preventDefault();
        e.stopPropagation();
        setDraft(bpmText(shown));
      }}
    >
      <span className="inline-flex items-baseline gap-1">
        <span className="text-base font-medium text-fg tabular-nums">{bpmText(shown)}</span>
        <span className="text-xs text-fg-faint">BPM</span>
      </span>
      {drag && <Bubble at={drag}>{bpmText(drag.value)} BPM</Bubble>}
    </span>
  );
}

const UNITS = [2, 4, 8, 16];

/**
 * The time signature picker: beats in a bar (1 to 16 by the stepper, up to 64 typed) and the note
 * a beat is (2, 4, 8 or 16). Each change is written at once, to the one in effect at `at`.
 */
export function MeterPicker({ session, timeline, at, locked }: { session: number; timeline: TimelineView; at: number; locked: string | null }) {
  const toast = useStore((s) => s.toast);
  const m = meterAt(timeline, at);
  const [shown, setShown] = useState({ beats: m.beats, unit: m.unit });
  const [typed, setTyped] = useState<string | null>(null);
  // Edits sent and not yet answered: until the last is, the picker shows what was clicked, not an
  // earlier click's graph (three quick clicks on + are three beats more).
  const inFlight = useRef(0);
  useEffect(() => {
    if (inFlight.current === 0) setShown({ beats: m.beats, unit: m.unit });
  }, [m.beats, m.unit]);
  const set = (beats: number, unit: number) => {
    if (locked || (beats === shown.beats && unit === shown.unit)) return;
    setShown({ beats, unit });
    inFlight.current++;
    graphApi.meter(session, { at: m.beat, beats, unit }, { name: "Time signature" }).then(
      () => inFlight.current--,
      (e) => {
        // Refused: back to the song as it is.
        if (--inFlight.current === 0) {
          const now = useStore.getState().graph?.timeline;
          const was = now ? meterAt(now, at) : m;
          setShown({ beats: was.beats, unit: was.unit });
        }
        toast(message(e));
      },
    );
  };
  const typedBeats = (text: string) => {
    setTyped(null);
    const n = Number(text.trim());
    if (Number.isInteger(n) && n >= 1 && n <= 64) set(n, shown.unit);
  };
  return (
    <div data-testid="meter-picker" className="w-[232px] px-2 py-1.5 space-y-2.5">
      <div className="flex items-center justify-between gap-3">
        <span className="text-sm text-fg-muted">Beats in a bar</span>
        <div className="flex items-center gap-1">
          <Button size="sm" aria-label="One beat fewer" disabled={!!locked || shown.beats <= 1} onClick={() => set(shown.beats - 1, shown.unit)} className="!px-1">
            <Minus size={14} />
          </Button>
          <input
            data-testid="meter-beats"
            aria-label="Beats in a bar"
            inputMode="numeric"
            disabled={!!locked}
            value={typed ?? String(shown.beats)}
            onChange={(e) => setTyped(e.target.value)}
            onFocus={(e) => e.target.select()}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                typedBeats(e.currentTarget.value);
              }
            }}
            onBlur={(e) => typed !== null && typedBeats(e.target.value)}
            className="w-9 h-ctl-sm rounded-sm bg-raised border border-line-strong hover:border-fg-off focus:border-accent text-center text-sm text-fg tabular-nums outline-none disabled:bg-chrome disabled:text-fg-off"
          />
          <Button size="sm" aria-label="One beat more" disabled={!!locked || shown.beats >= 16} onClick={() => set(shown.beats + 1, shown.unit)} className="!px-1">
            <Plus size={14} />
          </Button>
        </div>
      </div>
      <div className="flex items-center justify-between gap-3">
        <span className="text-sm text-fg-muted">Each beat</span>
        <div role="radiogroup" aria-label="Each beat is a" className="p-0.5 rounded-sm bg-hover inline-flex">
          {UNITS.map((u) => (
            <Segment
              key={u}
              role="radio"
              aria-checked={shown.unit === u}
              data-unit={u}
              on={shown.unit === u}
              disabled={!!locked}
              title={`1/${u} note`}
              onClick={() => set(shown.beats, u)}
            >
              {u}
            </Segment>
          ))}
        </div>
      </div>
    </div>
  );
}

/** The time signature in the top bar, "4/4": a click opens its picker. */
export function MeterField({ session, timeline, at, locked }: { session: number; timeline: TimelineView; at: number; locked: string | null }) {
  const m = meterAt(timeline, at);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const many = timeline.meter.length > 1;
  return (
    <>
      <button
        data-testid="meter-field"
        aria-label="Time signature"
        aria-expanded={!!anchor}
        aria-disabled={!!locked}
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => !locked && setAnchor(anchor ? null : e.currentTarget)}
        title={[many ? `Time signature from ${placeOf(timeline.meter, m.beat).replace("bar 1", "the start")}: ${m.beats}/${m.unit}` : "Time signature", locked ?? "Click to change it", HOME].join("\n")}
        className={clsx(
          "w-10 h-ctl rounded-sm text-base text-fg tabular-nums transition-colors duration-80 ease-linear",
          locked ? "cursor-default" : "cursor-pointer hover:bg-hover",
          anchor && "bg-hover",
        )}
      >
        {`${m.beats}/${m.unit}`}
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} aria-label="Time signature">
          <div className="px-2 pt-1 text-sm font-medium text-fg">Time signature</div>
          <MeterPicker session={session} timeline={timeline} at={m.beat} locked={locked} />
        </Popover>
      )}
    </>
  );
}

/**
 * The song's name, centred in the top bar: a double-click (or File ▸ Rename Song…) edits it in
 * place. Enter writes it into music2.toml, Esc puts it back; an empty one is refused in the
 * field's error style (taste.md §2.9). The folder keeps its own name.
 */
export function SongTitle({ session, name, dir, rate, locked }: { session: number; name: string; dir: string; rate?: number; locked: string | null }) {
  const toast = useStore((s) => s.toast);
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (pending !== null && name === pending) setPending(null);
  }, [name, pending]);
  useEffect(() => {
    canvas.renameSong = () => !locked && setDraft(pending ?? name);
    return () => void delete canvas.renameSong;
  }, [name, pending, locked]);
  const editing = draft !== null;
  useLayoutEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);
  const commit = (value: string, leaving: boolean) => {
    const next = value.trim();
    if (!next) {
      if (leaving) {
        setDraft(null);
        setError(null);
      } else setError("A song needs a name");
      return;
    }
    setDraft(null);
    setError(null);
    if (next === name) return;
    setPending(next);
    graphApi.title(session, next, { name: "renaming the song" }).catch((e) => {
      setPending(null);
      toast(message(e));
    });
  };
  const shown = pending ?? name;
  const where = `${dir}${rate ? `\n${rate / 1000} kHz` : ""}`;
  if (draft !== null) {
    return (
      <span className="relative inline-flex pointer-events-auto">
        <input
          ref={input}
          data-testid="song-title-input"
          aria-label="Song name"
          aria-invalid={!!error}
          value={draft}
          autoFocus
          spellCheck={false}
          maxLength={80}
          onChange={(e) => {
            setDraft(e.target.value);
            setError(null);
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") commit(draft, false);
            else if (e.key === "Escape") {
              setDraft(null);
              setError(null);
            }
          }}
          onBlur={() => commit(draft, true)}
          style={{ width: `${Math.max(12, draft.length + 2)}ch` }}
          className={clsx(
            "max-w-[min(480px,calc(100vw-960px))] min-w-40 h-ctl px-2 rounded-sm bg-raised border text-center text-base font-semibold text-fg-strong outline-none",
            error ? "border-danger" : "border-accent",
          )}
        />
        {error && (
          <span role="alert" className="absolute left-1/2 -translate-x-1/2 top-full mt-1 z-10 px-1.5 py-0.5 rounded-sm bg-raised shadow-pop text-xs text-danger whitespace-nowrap">
            {error}
          </span>
        )}
      </span>
    );
  }
  return (
    <span
      data-testid="bar-song-name"
      className={clsx("min-w-0 truncate text-base font-semibold text-fg-strong pointer-events-auto", !locked && "cursor-text", pending !== null && "opacity-70")}
      title={`${shown}\n${where}${locked ? "" : "\nDouble-click to rename it"}`}
      onDoubleClick={() => !locked && setDraft(shown)}
    >
      {shown}
    </span>
  );
}

/**
 * A tempo or time signature change on the ruler, opened at its mark: the same fields as the top
 * bar's, editing that change, and Remove (taste.md §3.3, design-8b §4.4).
 */
export function MarkPopover({
  session,
  timeline,
  beat,
  anchor,
  locked,
  typing,
  onClose,
}: {
  session: number;
  timeline: TimelineView;
  beat: number;
  anchor: HTMLElement;
  locked: string | null;
  /** Just made: the tempo opens as a field to type in. */
  typing?: boolean;
  onClose: () => void;
}) {
  const toast = useStore((s) => s.toast);
  const tempo = timeline.tempo.some((t) => t.beat === beat && t.beat > 0);
  const meter = timeline.meter.some((m) => m.beat === beat && m.beat > 0);
  const remove = async () => {
    onClose();
    try {
      if (tempo) await graphApi.tempo(session, { at: beat, remove: true }, { name: "removing a tempo change" });
      if (meter) await graphApi.meter(session, { at: beat, remove: true }, { name: "removing a time signature change" });
    } catch (e) {
      toast(message(e));
    }
  };
  return (
    <Popover anchor={anchor} onClose={onClose} data-testid="mark-popover" aria-label={`Changes from ${placeOf(timeline.meter, beat)}`}>
      <div className="w-[232px]">
        <div className="px-2 pt-1 pb-1.5 text-sm font-medium text-fg">From {placeOf(timeline.meter, beat)}</div>
        {tempo && (
          <div className="px-2 pb-1.5 flex items-center justify-between gap-3">
            <span className="text-sm text-fg-muted">Tempo</span>
            <TempoField session={session} timeline={timeline} at={beat} locked={locked} testid="mark-tempo" typing={typing} />
          </div>
        )}
        {meter && <MeterPicker session={session} timeline={timeline} at={beat} locked={locked} />}
        <div className="h-px my-1 bg-line" />
        <MenuRow data-testid="mark-remove" disabled={!!locked} onClick={() => void remove()}>
          {tempo && meter ? "Remove these changes" : tempo ? "Remove this tempo change" : "Remove this time signature change"}
        </MenuRow>
      </div>
    </Popover>
  );
}

/** The ruler's right-click menu: a tempo change at the beat clicked, or a time signature change at its bar. */
export function RulerMenu({
  session,
  timeline,
  beat,
  anchor,
  locked,
  onClose,
  onMade,
}: {
  session: number;
  timeline: TimelineView;
  beat: number;
  anchor: HTMLElement;
  locked: string | null;
  onClose: () => void;
  /** Where the change was made, to open it. */
  onMade: (beat: number) => void;
}) {
  const toast = useStore((s) => s.toast);
  const bar = barAt(timeline.meter, beat);
  const at = `${bar.bar + 1}.${Math.floor(beat - bar.start + 1e-9) + 1}`;
  const add = (p: Promise<unknown>, where: number) => {
    onClose();
    p.then(() => onMade(where), (e) => toast(message(e)));
  };
  return (
    <Popover anchor={anchor} onClose={onClose} role="menu" data-testid="ruler-menu" className="w-[260px]">
      <MenuRow
        disabled={!!locked}
        onClick={() => add(graphApi.tempo(session, { at: beat, bpm: tempoAt(timeline, beat).bpm, add: true }, { name: "adding a tempo change" }), beat)}
      >
        Add tempo change at {at}
      </MenuRow>
      <MenuRow
        disabled={!!locked}
        onClick={() => {
          const m = meterAt(timeline, beat);
          add(graphApi.meter(session, { at: bar.start, beats: m.beats, unit: m.unit, add: true }, { name: "adding a time signature change" }), bar.start);
        }}
      >
        Add time signature change at bar {bar.bar + 1}
      </MenuRow>
    </Popover>
  );
}

// A call's settings on its card (CARD-7), with its knobs, in its descriptor's order: a menu for a
// choice, a switch for on or off, a number box for a number (it commits on Enter or when it loses
// the focus, never on each key), a recording's waveform (a WAV or FLAC dropped on it replaces it), a
// menu of the song's tracks, and a sentence for regions and a keymap. A setting decides what is
// built, so a change rebuilds the sound: pointing at one says so. One the code works out shows what
// it is, with "edit in code" and why. Each change is one edit of the call, one undo step (CARD-4).
// A plugin's stepped parameter uses the same menu and switch, in its knob's row (CARD-6, NodeGraph's
// KnobControl, which keeps it a knob to the rest of the app).

import clsx from "clsx";
import { Check, ChevronDown, FileAudio } from "lucide-react";
import { useMemo, useRef, useState, type ReactNode } from "react";
import { controlLabel, graphApi, inSentence, moduleName, type Choice, type Setting } from "../lib/graph";
import { MISSING, usePeaks, type Peaks } from "../lib/peaks";
import { useStore } from "../lib/store";
import { MenuRow, Popover } from "./ui";

/** A setting's row; a recording's is taller, for its waveform. */
const ROW_H = 28;
const RECORDING_H = 32;

/** What the card says of every setting when pointed at (CARD-7): a change rebuilds the sound. */
export const REBUILDS = "Changing it rebuilds the sound, so a note held now may stop.";

/** The height of a setting's row. */
export const settingHeight = (x: Setting) => (x.kind === "recording" && !x.readonly ? RECORDING_H : ROW_H);

/** Whether a value says nothing: none, or an empty list or map. */
const empty = (v: unknown) => v == null || (Array.isArray(v) && v.length === 0) || (typeof v === "object" && Object.keys(v as object).length === 0);

/**
 * Whether the card shows setting `x`: every one but a setting left out that can't be changed here
 * and has nothing to show (a keymap with no notes; a Sampler's file while it plays regions).
 */
export const shown = (x: Setting) => x.written || !x.readonly || !empty(x.value) || !!x.said;

/** A value as the card says it: a word, On or Off, a number, a track by its name. */
function said(x: { value: unknown; options?: Choice[] }): string {
  const option = x.options?.find((o) => o.value === x.value);
  if (option) return option.label;
  if (typeof x.value === "boolean") return x.value ? "On" : "Off";
  if (typeof x.value === "number") return String(x.value).replace("-", "−");
  if (typeof x.value === "string") return x.value;
  return "—";
}

/** An error's words, without "edit rejected: " in front. */
export const words = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

interface Props {
  x: Setting;
  /** What the card is called ("LFO"), to name the undo step ("LFO shape"). */
  owner: string;
  session: number;
  revision: number;
  editable: boolean;
  /** Why nothing can be edited right now, when that is so. */
  locked: string | null;
  /** A track's name by its key, for a dictionary entry's setting (`vox.to`: Vox to). */
  tracks: (key: string) => string | undefined;
  /** The card's title, as a recording's drop says it ("Play it in Sampler"). */
  card: string;
  /** The card's id: what a recording's drop names. */
  call: string;
}

/** One setting of a card, as its kind draws it. */
export function SettingRow(p: Props) {
  const { x, owner, session, revision, editable, locked, tracks } = p;
  const toast = useStore((s) => s.toast);
  const label = controlLabel(x, tracks);
  const where = `In your code: ${x.file}, line ${x.line}`;
  const off = !editable ? `${locked ?? "It can't be edited right now"}.` : null;
  /** Write `value`: one edit, named "LFO shape" by its label (CARD-34); a refusal says why. */
  const write = (value: Choice["value"]) =>
    graphApi.setValue(session, revision, x.id, value, { name: `${owner} ${inSentence(label)}` }).catch((error) => toast(words(error)));
  const row = (control: ReactNode, title: string) => (
    <div
      className="relative h-7 pl-4 pr-3 flex items-center gap-2 text-xs min-w-0"
      data-setting={x.slot}
      data-kind={x.kind}
      data-value={JSON.stringify(x.value ?? null)}
      data-written={x.written || undefined}
      title={title}
    >
      <span className="flex-1 min-w-0 text-fg-muted truncate">{label}</span>
      {control}
    </div>
  );
  if (x.readonly) {
    // Its value, or the code that works it out; regions and a keymap in words.
    const shownAs = x.said ?? (x.value != null && !empty(x.value) ? said(x) : x.expression);
    // A sentence, once: a reason may end with its own full stop.
    const why = (x.readonly.charAt(0).toUpperCase() + x.readonly.slice(1)).replace(/\.?$/, ".");
    return row(
      <>
        <span className={clsx("min-w-0 max-w-[55%] truncate text-right", x.expression && !x.said ? "font-mono text-fg" : "text-fg")} data-code={x.expression ? "" : undefined}>
          {shownAs ?? "—"}
        </span>
        <span className="shrink-0 text-fg-faint">edit in code</span>
      </>,
      `${why}\n${where}${x.expression ? `\n${x.expression}` : ""}`,
    );
  }
  const title = `${off ?? REBUILDS}\n${where}${x.written ? "" : " (the default: not written in the code yet, changing it writes it in)"}`;
  if (x.kind === "choice") return row(<MenuValue value={x} label={label} off={off} title={title} onPick={write} />, title);
  // A track: the list its setting offers (a mixer channel's: Master and its bus channels), else the
  // song's tracks, top to bottom, which the view gives once (its timeline).
  if (x.kind === "track") return row(<MenuValue value={{ ...x, options: x.options?.length ? x.options : songTracks() }} label={label} off={off} title={title} onPick={write} />, title);
  if (x.kind === "switch") return row(<SwitchValue value={x} label={label} off={off} onFlip={write} />, title);
  if (x.kind === "number") return row(<NumberBox x={x} label={label} off={off} onCommit={write} />, title);
  if (x.kind === "recording") return <RecordingRow {...p} label={label} tip={title} />;
  return row(<span className="text-fg truncate">{x.said ?? "—"}</span>, title);
}

/** The song's tracks as a menu offers them, top to bottom as the timeline has them: each by its key, called by its name. */
function songTracks(): Choice[] {
  return (useStore.getState().graph?.timeline.tracks ?? []).filter((t) => t.track).map((t) => ({ value: t.module, label: moduleName({ key: t.module, label: t.label }) }));
}

/** A choice's menu, or the song's tracks: the option now, and the others to pick from. */
export function MenuValue({ value, label, off, title, onPick }: { value: { value: unknown; written?: boolean; options?: Choice[] }; label: string; off: string | null; title?: string; onPick: (v: Choice["value"]) => void }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const options = value.options ?? [];
  return (
    <>
      <button
        data-testid="setting-menu"
        className={clsx(
          "min-w-0 max-w-[60%] h-ctl-xs px-1 -mr-1 flex items-center gap-1 rounded-sm text-xs font-medium transition-colors duration-80 ease-linear",
          off ? "text-fg-muted cursor-default" : "hover:bg-hover cursor-pointer",
          value.written === false ? "text-fg-muted" : "text-fg",
          anchor && "bg-accent-soft text-accent-strong",
        )}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        aria-label={`${label}: ${said(value)}`}
        aria-disabled={!!off || undefined}
        title={off ?? title}
        onMouseDown={(e) => e.preventDefault()}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          if (off) return;
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
      >
        <span className="truncate">{said(value)}</span>
        <ChevronDown size={12} className="shrink-0" />
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} align="end" role="menu" aria-label={label} data-testid="setting-options" className="w-[220px] max-h-[320px] overflow-y-auto">
          {options.map((o) => (
            <MenuRow
              key={String(o.value)}
              data-option={o.label}
              icon={<Check size={14} className={o.value === value.value ? undefined : "invisible"} />}
              onClick={() => {
                setAnchor(null);
                if (o.value !== value.value) onPick(o.value);
              }}
            >
              {o.label}
            </MenuRow>
          ))}
        </Popover>
      )}
    </>
  );
}

/** A switch: off or on, its two options in that order; a click (or Space) flips it. */
export function SwitchValue({ value, label, off, onFlip }: { value: { value: unknown; written?: boolean; options?: Choice[] }; label: string; off: string | null; onFlip: (v: Choice["value"]) => void }) {
  const [no, yes] = value.options ?? [];
  const on = !!yes && value.value === yes.value;
  return (
    <button
      role="switch"
      data-testid="setting-switch"
      aria-checked={on}
      aria-label={label}
      aria-disabled={!!off || undefined}
      title={off ?? `${on ? yes?.label : no?.label}: click to turn it ${on ? "off" : "on"}`}
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={() => !off && no && yes && onFlip(on ? no.value : yes.value)}
      className={clsx(
        "shrink-0 relative w-7 h-4 rounded-full transition-colors duration-140 ease-out",
        on ? "bg-accent" : "bg-line-strong",
        off ? "opacity-50 cursor-default" : "cursor-pointer",
      )}
    >
      <span className={clsx("absolute top-0.5 size-3 rounded-full bg-raised shadow-chip transition-[left] duration-140 ease-out", on ? "left-[14px]" : "left-0.5")} />
    </button>
  );
}

/** A number box: typed, then written on Enter or when it loses the focus; Esc puts it back. A whole number refuses a fraction, said under it. */
function NumberBox({ x, label, off, onCommit }: { x: Setting; label: string; off: string | null; onCommit: (v: number) => void }) {
  const now = typeof x.value === "number" ? String(x.value).replace("-", "−") : "";
  const [text, setText] = useState<string | null>(null);
  const [bad, setBad] = useState<string | null>(null);
  // What is typed and not yet written, read by the handlers at once (a blur follows an Enter in the same turn).
  const draft = useRef<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const typed = (t: string | null) => {
    draft.current = t;
    setText(t);
  };
  /** What `t` reads as: a number, or why it isn't one this setting takes. */
  const read = (t: string): number | string => {
    const v = Number(t.trim().replace("−", "-"));
    if (t.trim() === "" || !Number.isFinite(v)) return x.whole ? "A whole number, like 8" : "A number, like 0.5";
    if (x.whole && !Number.isInteger(v)) return "A whole number, like 8";
    return v;
  };
  /** Write what is typed, if it reads as a number; false (and it stays, said under it) if it doesn't. */
  const commit = () => {
    if (draft.current === null) return true;
    const v = read(draft.current);
    if (typeof v === "string") {
      setBad(v);
      return false;
    }
    typed(null);
    setBad(null);
    if (v !== x.value) onCommit(v);
    return true;
  };
  return (
    <span className="relative shrink-0">
      <input
        ref={field}
        data-testid="setting-number"
        aria-label={label}
        aria-invalid={!!bad || undefined}
        disabled={!!off}
        inputMode={x.whole ? "numeric" : "decimal"}
        spellCheck={false}
        value={text ?? now}
        onFocus={(e) => e.currentTarget.select()}
        onChange={(e) => {
          typed(e.target.value);
          setBad(null);
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter" && commit()) field.current?.blur();
          else if (e.key === "Escape") {
            typed(null);
            setBad(null);
            field.current?.blur();
          }
        }}
        onBlur={() => void commit()}
        onPointerDown={(e) => e.stopPropagation()}
        className={clsx(
          "w-14 h-5 px-1 rounded-sm bg-raised border text-right text-xs tabular-nums outline-none disabled:bg-chrome disabled:text-fg-off",
          x.written ? "text-fg" : "text-fg-muted",
          bad ? "border-danger" : "border-line-strong hover:border-fg-off focus:border-accent",
        )}
      />
      {bad && (
        <span role="alert" className="absolute right-0 top-full mt-0.5 z-10 whitespace-nowrap text-xs text-danger">
          {bad}
        </span>
      )}
    </span>
  );
}

/** A recording: its file's waveform and name, or where to drop one. A WAV or FLAC dropped on it plays there instead (CARD-5). */
function RecordingRow({ x, label, tip, call, card }: Props & { label: string; tip: string }) {
  const name = typeof x.value === "string" ? x.value : null;
  return (
    <div className="relative h-8 pl-4 pr-3 flex items-center gap-2 text-xs min-w-0" data-setting={x.slot} data-kind={x.kind} data-value={JSON.stringify(x.value ?? null)} title={tip}>
      <span className="shrink-0 text-fg-muted truncate max-w-[35%]">{label}</span>
      <div
        data-testid="recording"
        data-recording-call={call}
        data-recording-setting={x.slot}
        data-recording-title={card}
        className={clsx(
          "flex-1 min-w-0 h-6 px-1.5 flex items-center gap-1.5 rounded-sm",
          name ? "bg-chrome text-fg" : "border border-dashed border-line-strong text-fg-muted",
        )}
        title={name ? `Plays ${name}. Drop another WAV or FLAC on it to play that instead.\n${REBUILDS}` : "Drop a WAV or FLAC file from your computer here to play it"}
      >
        {name ? <Waveform asset={name} /> : <FileAudio size={12} className="shrink-0" />}
        <span className="truncate">{name ?? "Drop a WAV or FLAC here"}</span>
      </div>
    </div>
  );
}

/** A file's waveform, small: the louder of its channels in bars, mirrored about the middle. Its name while it loads, or if it doesn't read. */
function Waveform({ asset }: { asset: string }) {
  const peaks = usePeaks({ asset });
  const bars = useMemo(() => (peaks && peaks !== MISSING ? levels(peaks, 24) : null), [peaks]);
  if (!bars) return <FileAudio size={12} className="shrink-0" />;
  return (
    <svg aria-hidden data-wave width={36} height={16} viewBox="0 0 36 16" className="shrink-0 text-fg-muted">
      {bars.map((v, i) => {
        const h = Math.max(1, v * 14);
        return <rect key={i} x={i * 1.5} y={8 - h / 2} width={1} height={h} fill="currentColor" />;
      })}
    </svg>
  );
}

/** `n` levels, 0 to 1, across the whole file: each the loudest sample of its part, of either channel. */
function levels(p: Peaks, n: number): number[] {
  const buckets = Math.ceil(p.frames / p.bucket);
  return Array.from({ length: n }, (_, i) => {
    let most = 0;
    for (let b = Math.floor((i * buckets) / n); b < Math.max(Math.floor(((i + 1) * buckets) / n), Math.floor((i * buckets) / n) + 1) && b < buckets; b++) {
      for (let c = 0; c < p.channels; c++) {
        const at = (b * p.channels + c) * 2;
        most = Math.max(most, Math.abs(p.data[at]), Math.abs(p.data[at + 1]));
      }
    }
    return most / 127;
  });
}

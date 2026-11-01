import clsx from "clsx";
import { ArrowLeftRight, Check, CodeXml, Drum, Merge, Pencil, Plus, Spline, Trash2, VolumeX } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState } from "react";
import { beatNow, onFrame } from "../lib/clock";
import {
  display,
  findLane,
  formatValue,
  fromUnit,
  graphApi,
  laneTitle,
  laneValue,
  lockReason,
  parseValue,
  picks,
  stepValue,
  toUnit,
  type BusKind,
  type GraphState,
  type NumberKnob as KnobInfo,
  type LaneRef,
  type MixStrip,
  type StripSend,
} from "../lib/graph";
import { laneMade } from "../lib/events";
import { channelMeter, OUT, useLoudness } from "../lib/meters";
import { mod } from "../lib/native";
import { muteInSongCall } from "../lib/overlays";
import { tones } from "../lib/palette";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { Bubble, FINE, focusNext, KnobMenu, MiniKnob, NOTCH, useKnobDrag, useKnobGesture } from "./Knob";
import { useGlows } from "../lib/notice";
import Meter, { ClipDot } from "./Meter";
import { StripMuteSolo } from "./MuteSolo";
import { Colours, makeKit, openChangeInstrument } from "./Tracks";
import TrackName from "./TrackName";
import { Button, EmptyState, MenuRow, Popover } from "./ui";

// The Mix view (taste.md §3.5, design-8b §5.5, MX-1, MX-2, MX-5): a strip per track in track order,
// each with its colour, name, pan, M and S, a fader with its level, and a meter beside the fader;
// Master pinned on the right. A fader and a pan are the graph view's knobs, by id: they are heard
// while they move and written once when let go, one undo step each, with no toast (§2.14).

/** Where 0 dB sits on a fader's travel, and its ends: +12 dB at the top, −60 dB just above the foot. */
const UNITY = 0.75;
const TOP = 12;
const FLOOR = -60;
/** The foot: a fader's last 4 px write std's silence, −96 dB, which reads "−∞" (design-8b §5.1). */
const FOOT = 4;
const SILENT = -96;
/** Where a fader's track runs across its strip: left of the middle, so the meter fits beside it. */
const TRACK_X = "40%";
/** Where a strip's meter starts: 5 px right of the fader's thumb. */
const METER_X = `calc(${TRACK_X} + 15px)`;
/** The marks beside the track, in dB; 0 dB's is longer. */
const MARKS = [6, 0, -6, -12, -24, -48];

/** A tenth of a dB, as a fader writes and steps it; never −0. */
const tenth = (db: number) => Math.round(db * 10) / 10 || 0;

/**
 * Where `db` sits on a fader's travel, 0 (−60 dB) to 1 (+12 dB): linear above 0 dB, which is at
 * 75%; below it the square of the way up from −60 dB, so −6 dB is at 61% and −20 dB at 33%.
 */
export function faderAt(db: number) {
  if (db >= 0) return Math.min(1, UNITY + ((1 - UNITY) * db) / TOP);
  if (db <= FLOOR) return 0;
  return UNITY * ((db - FLOOR) / -FLOOR) ** 2;
}

/** The level at `p` of the travel (0..1), to a tenth of a dB: `faderAt`'s inverse. */
export function faderDb(p: number) {
  return tenth(p >= UNITY ? (TOP * (p - UNITY)) / (1 - UNITY) : FLOOR * (1 - Math.sqrt(Math.max(0, p) / UNITY)));
}

/** `n` steps of the wheel or ↑ ↓ from `v`: 0.1 dB each. Down past −60 dB is −∞; up from −∞ starts at −60 dB. */
function faderStep(v: number, n: number) {
  if (v <= SILENT) return n > 0 ? tenth(FLOOR + 0.1 * (n - 1)) : SILENT;
  const next = tenth(v + 0.1 * n);
  if (n < 0 && next < FLOOR) return SILENT;
  return Math.min(Math.max(TOP, v), next);
}

/** A level as taste.md's words write it: "−6.0 dB", "+3.0 dB", "0.0 dB", or "−∞" at the foot. */
const level = (v: number) => (v <= SILENT ? "−∞" : formatValue("db", v));

/** A level typed in (taste.md §2.14): "-3", "-3db", "-3 dB", "-inf" or "−∞" (silence); held to −96…+12 dB. Null if it doesn't read. */
function parseLevel(text: string): number | null {
  const t = text.trim().toLowerCase().replace(/−/g, "-").replace(/\s+/g, "");
  if (/^-?(inf|∞)$/.test(t)) return SILENT;
  const m = /^([+-]?(?:\d+\.?\d*|\.\d+))(db)?$/.exec(t);
  if (!m) return null;
  const v = Number(m[1]);
  return Number.isFinite(v) ? Math.min(TOP, Math.max(SILENT, tenth(v))) : null;
}

/** An error in words: a builder's refusal says why, without the "edit rejected" in front. */
const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** Edit in code: the song's code (where the mixer is), at `at` when known. */
function editInCode(at: { file: string; line: number } | null) {
  const s = useShell.getState();
  s.select("song");
  s.set({ codeAt: at });
  s.setView("code");
}

/** Where a knob is written, for Edit in code. */
const placeOf = (k: KnobInfo | null | undefined) => (k?.file ? { file: k.file, line: k.line } : null);

/** A one-line empty state: why there is nothing to mix here, and the way to the code. */
function Empty({ children, at }: { children: React.ReactNode; at?: { file: string; line: number } | null }) {
  return (
    <div data-testid="mix" data-empty className="flex-1 min-h-0 grid place-items-center px-6">
      <EmptyState>
        <p>{children}</p>
        {at !== undefined && (
          <Button variant="secondary" size="sm" onClick={() => editInCode(at)}>
            Edit in code
          </Button>
        )}
      </EmptyState>
    </div>
  );
}

/** The Mix view of the open song. */
export default function Mix({ graph }: { graph: GraphState }) {
  const locked = useStore((s) => lockReason(graph, s.connected));
  const selected = useShell((s) => s.selected);
  // Every knob of the view, by id: a strip names its fader and pan by them. Only numbers have faders.
  const knobs = useMemo(
    () => new Map(graph.view.modules.flatMap((m) => m.nodes.flatMap((n) => n.knobs.flatMap((k) => (picks(k) ? [] : [[k.id, k] as const]))))),
    [graph.view.modules],
  );
  const mix = graph.mix;
  // The bus channels, in the mixer's order: each strip has a row for each (RENDER-34).
  const buses = useMemo(() => mix.strips.flatMap((s) => (s.bus && s.channel ? [{ key: s.channel, label: s.label }] : [])), [mix.strips]);
  if (!graph.view.modules.length) return <Empty>Nothing to mix until the code compiles.</Empty>;
  if (mix.mixer !== "ok") return <Empty at={null}>{mix.message}</Empty>;
  // Where the mixer is written: any strip's fader or pan says.
  const mixer = placeOf(mix.strips.map((s) => knobs.get(s.db ?? "") ?? knobs.get(s.pan ?? "")).find(Boolean));
  const master = mix.master?.db ? knobs.get(mix.master.db) : undefined;
  const props = { knobs, session: graph.session, revision: graph.view.revision, locked, mixer, buses };
  return (
    <div data-testid="mix" className="flex-1 min-h-0 flex bg-canvas">
      {/* The strips scroll sideways when they don't fit; Master stays (taste.md §3.5). */}
      <div data-testid="mix-strips" className="flex-1 min-w-0 flex overflow-x-auto overflow-y-hidden">
        {mix.strips.map((s) => (
          <Strip key={s.channel ? `channel:${s.channel}` : `track:${s.modules[0]}`} strip={s} selected={!!selected && s.modules.includes(selected)} {...props} />
        ))}
        <AddBus session={graph.session} locked={locked} />
      </div>
      {mix.master && <MasterStrip knob={master} lane={mix.master.db_lane} {...props} />}
    </div>
  );
}

interface StripProps {
  knobs: Map<string, KnobInfo>;
  session: number;
  revision: number;
  /** Why nothing can be edited now, or null. */
  locked: string | null;
  /** Where the mixer is written. */
  mixer: { file: string; line: number } | null;
  /** The bus channels, in the mixer's order: a strip has a send row for each. */
  buses: Bus[];
}

/** A bus channel, by its key, and what it is called. */
interface Bus {
  key: string;
  label: string;
}

/** A send row's height: a strip has one for each bus. */
const SEND_H = 20;

/**
 * One strip: 76 px, its colour, name, pan, M and S, and its fader with its level and its meter. A
 * channel no track reaches has no M, S or meter.
 */
const Strip = memo(function Strip({ strip, selected, knobs, session, revision, locked, mixer, buses }: StripProps & { strip: MixStrip; selected: boolean }) {
  const one = strip.modules.length === 1 ? strip.modules[0] : null;
  const tracks = strip.modules.length > 0;
  const db = strip.db ? (knobs.get(strip.db) ?? null) : null;
  const pan = strip.pan ? (knobs.get(strip.pan) ?? null) : null;
  const name = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const at = placeOf(db ?? pan) ?? mixer;
  const editable = !locked;
  // After its fader when it has a channel (TAP-5); else its tracks, where they enter the song.
  const meter = tracks ? <StripMeter channels={strip.channel ? [channelMeter(strip.channel)] : strip.modules} name={strip.label} fader={!!strip.channel} /> : null;
  return (
    <div
      data-strip={strip.channel ?? one ?? ""}
      data-selected={selected || undefined}
      data-bus={strip.bus || undefined}
      aria-label={strip.bus ? `${strip.label} bus` : strip.label}
      title={strip.bus ? `${strip.label} is a bus: it plays what other tracks send it.` : undefined}
      className={clsx("w-[76px] shrink-0 flex flex-col border-r border-line", selected ? "bg-accent-wash" : strip.bus && "bg-chrome")}
      // Pressing a strip selects its track; the dock stays on Mix (taste.md §3.7).
      onPointerDownCapture={(e) => e.button === 0 && strip.modules.length > 0 && useShell.getState().select(strip.modules[0])}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu(true);
      }}
    >
      <div className="h-[3px] shrink-0" style={{ background: tones(strip.colour).bar }} />
      <div ref={name} className="h-5 shrink-0 px-1.5 flex items-center justify-center gap-1 min-w-0">
        {strip.bus && <BusTag />}
        {one ? (
          <TrackName
            module={one}
            label={strip.label}
            track
            listen="strip"
            testid="strip-name"
            className={clsx("block max-w-full text-sm text-center", selected ? "font-semibold text-fg-strong" : "font-medium text-fg")}
          />
        ) : (
          <span data-testid="strip-name" className="block max-w-full truncate text-sm font-medium text-fg" title={strip.label}>
            {strip.label}
          </span>
        )}
      </div>
      <div className="h-[46px] shrink-0">
        {strip.channel && (pan || strip.pan_insert) ? (
          <Pan channel={strip.channel} name={strip.label} knob={pan} session={session} revision={revision} editable={editable} locked={locked} />
        ) : (
          strip.channel && <PanInCode lane={strip.pan_lane} />
        )}
      </div>
      <div data-slot="mute-solo" className="h-6 shrink-0 flex items-center justify-center">
        {tracks && <StripMuteSolo modules={strip.modules} name={strip.label} onSongMenu={() => setMenu(true)} />}
      </div>
      <div data-slot="sends" className="shrink-0 flex flex-col" style={{ height: buses.length * SEND_H }}>
        {strip.channel &&
          buses.map((b) =>
            b.key === strip.channel ? (
              <div key={b.key} style={{ height: SEND_H }} />
            ) : (
              <SendRow
                key={b.key}
                strip={strip}
                bus={b}
                send={strip.sends.find((x) => x.bus === b.key) ?? null}
                knobs={knobs}
                session={session}
                revision={revision}
                locked={locked}
                at={at}
              />
            ),
          )}
      </div>
      {db ? (
        <Fader knob={db} name={strip.label} session={session} revision={revision} editable={editable} locked={locked} meter={meter} />
      ) : strip.db_lane ? (
        <LaneFader lane={strip.db_lane} name={strip.label} session={session} locked={locked} at={at} meter={meter} />
      ) : (
        <NoFader why={strip.why ?? "Its level is set in code"} at={at} meter={meter} />
      )}
      <div className="h-6 shrink-0 px-1.5 flex items-center justify-center">
        {strip.channel && <Output strip={strip} buses={buses} session={session} locked={locked} />}
      </div>
      {menu && name.current && (
        <StripMenu name={strip.label} module={one} bus={strip.bus} modules={strip.modules} anchor={name.current} at={at} locked={locked} onClose={() => setMenu(false)} />
      )}
    </div>
  );
});

/** Master (MX-5): 96 px, graphite, pinned right after a 12 px gap; no pan, M or S (MON-1). */
function MasterStrip({ knob, lane, session, revision, locked, mixer, buses }: StripProps & { knob: KnobInfo | undefined; lane: LaneRef | null }) {
  const name = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const at = placeOf(knob) ?? mixer;
  const meter = <StripMeter channels={[OUT]} name="Master" master />;
  return (
    <div
      data-strip="master"
      aria-label="Master"
      className="ml-3 w-24 shrink-0 flex flex-col border-l border-line"
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu(true);
      }}
    >
      <div className="h-[3px] shrink-0" style={{ background: tones("graphite").bar }} />
      <div ref={name} className="h-5 shrink-0 px-1.5 flex items-center justify-center">
        <span data-testid="strip-name" className="text-sm font-medium text-fg">Master</span>
      </div>
      <MasterLoudness />
      <div className="h-6 shrink-0" />
      <div className="shrink-0" style={{ height: buses.length * SEND_H }} />
      {knob ? (
        <Fader knob={knob} name="Master" session={session} revision={revision} editable={!locked} locked={locked} meter={meter} />
      ) : lane ? (
        <LaneFader lane={lane} name="Master" session={session} locked={locked} at={at} meter={meter} />
      ) : (
        <NoFader why="Its level is set in code" at={at} meter={meter} />
      )}
      <div className="h-6 shrink-0" />
      {menu && name.current && <StripMenu name="Master" module={null} bus={false} modules={[]} anchor={name.current} at={at} locked={locked} onClose={() => setMenu(false)} />}
    </div>
  );
}

/** A number as taste.md writes it: "−14", with a true minus. */
const minus = (n: number) => (n < 0 ? `−${-n}` : String(n));

/**
 * Master's loudness (TAP-6), where a track's pan would be: the last 3 seconds in LUFS, and the
 * loudest point of them between samples (the true peak), while the song plays; "—" until there is
 * one (it takes 3 seconds) or in silence.
 */
function MasterLoudness() {
  const { short, peak } = useLoudness();
  return (
    <div
      data-testid="master-loudness"
      className="h-[46px] shrink-0 px-1.5 flex flex-col items-center justify-center gap-0.5 tabular-nums cursor-default"
      title="How loud the song is, as ears hear it: the last 3 seconds, in LUFS. Streaming services play songs at about −14 LUFS. Peak is the loudest point between samples: over 0 dB, it can distort when it's played or streamed."
    >
      <span data-testid="master-lufs" className="text-sm font-medium text-fg whitespace-nowrap">
        {short === null ? "— LUFS" : `${minus(short)} LUFS`}
      </span>
      <span data-testid="master-peak" className="text-xs text-fg-muted whitespace-nowrap">
        {peak === null ? "Peak —" : `Peak ${minus(peak)} dB`}
      </span>
    </div>
  );
}

/**
 * A strip's meter (taste.md §2.8; Q1 to Q3, X23; TAP-5): two bars, left and right, 3 px each with
 * 1 px between (Master's 5 px), beside the fader, its travel tall. A strip with a channel reads it
 * after its fader, so the fader moves it; one without reads its tracks where they enter the song,
 * the louder of them on each side. Above it, a clip dot stays lit until clicked. Master's is the
 * song's output, with no dot: it ends in a limiter. Placed in the fader's column, so a drag on it
 * moves the fader.
 */
function StripMeter({ channels, name, master = false, fader = false }: { channels: string[]; name: string; master?: boolean; fader?: boolean }) {
  const width = master ? 11 : 7;
  return (
    <>
      <div data-slot="strip-meter" className="absolute top-[9px] bottom-[5px]" style={{ left: METER_X, width }}>
        <Meter
          channels={channels}
          bars={2}
          label={`${name} level`}
          title={master ? "The song's level" : fader ? `${name}'s level, after its fader.` : `${name}'s level.`}
          className="w-full h-full"
        />
      </div>
      {!master && (
        <div className="absolute top-[-1px] h-3 flex justify-center" style={{ left: METER_X, width }}>
          <ClipDot channels={channels} name={name} className="shrink-0" />
        </div>
      )}
    </>
  );
}

/** What a lane is called on screen ("Hats volume"), as its header in the timeline says it. */
function useLaneName(lane: LaneRef | null) {
  return useStore((s) => {
    const found = lane && s.graph ? findLane(s.graph.timeline, lane.module, lane.lane) : undefined;
    return found ? laneTitle(found) : (lane?.lane ?? "");
  });
}

/** A pan the code doesn't write as a number: "—", and what sets it. */
function PanInCode({ lane }: { lane: LaneRef | null }) {
  const name = useLaneName(lane);
  return (
    <div data-testid="pan-in-code" className="h-full grid place-items-center text-xs text-fg-faint" title={lane ? `Follows the ${name} lane.` : "Its pan follows a signal in the code"}>
      —
    </div>
  );
}

/**
 * A level an automation lane sets (taste.md §2.10, KN-2): the fader shows the lane's value at the
 * playhead, read only, with the lane's marker; it says "Follows the Hats volume lane" and changes
 * nothing when dragged. Its menu shows the lane, or removes the automation (the value at the playhead
 * goes back into the code), or opens the code.
 */
function LaneFader({
  lane,
  name,
  session,
  locked,
  at,
  meter,
}: {
  lane: LaneRef;
  name: string;
  session: number;
  locked: string | null;
  at: { file: string; line: number } | null;
  /** The strip's meter, drawn in the fader's column. */
  meter: React.ReactNode;
}) {
  const toast = useStore((s) => s.toast);
  const shown = useLaneName(lane);
  const found = useStore((s) => (s.graph ? findLane(s.graph.timeline, lane.module, lane.lane) : undefined));
  const control = found?.kind === "control" ? found : undefined;
  const [value, setValue] = useState<number | null>(null);
  useEffect(() => {
    if (!control) return setValue(null);
    return onFrame((beat) => {
      const v = laneValue(control, beat).value;
      setValue((was) => (was === v ? was : v));
    });
  }, [control]);
  const column = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const [held, setHeld] = useState<{ x: number; y: number } | null>(null);
  const follows = `Follows the ${shown} lane.`;
  const v = value ?? 0;
  const top = v <= SILENT ? `calc(100% - ${FOOT / 2}px)` : `calc((100% - ${FOOT}px) * ${1 - faderAt(v)})`;
  const remove = () => {
    setMenu(false);
    graphApi.removeAutomation(session, lane, beatNow(), { name: `removing ${shown}'s automation`, structural: true }).catch((e) => toast(message(e)));
  };
  return (
    <div
      className="flex-1 min-h-0 flex flex-col"
      data-fader={name}
      data-lane-fader={`${lane.module}.${lane.lane}`}
      data-value={value ?? undefined}
      aria-disabled
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setMenu(true);
      }}
    >
      <div
        ref={column}
        title={follows}
        className="relative flex-1 min-h-0 py-[5px] touch-none"
        onPointerDown={(e) => e.button === 0 && (e.currentTarget.setPointerCapture(e.pointerId), setHeld({ x: e.clientX, y: e.clientY }))}
        onPointerMove={(e) => held && setHeld({ x: e.clientX, y: e.clientY })}
        onPointerUp={() => setHeld(null)}
        onPointerCancel={() => setHeld(null)}
      >
        <div data-testid="fader-travel" className="relative h-full">
          <Rails />
          <div
            role="slider"
            aria-label={`${name} volume`}
            aria-readonly
            aria-valuemin={SILENT}
            aria-valuemax={TOP}
            aria-valuenow={v}
            aria-valuetext={`${level(v)}, ${follows}`}
            data-testid="fader-thumb"
            className="absolute w-5 h-2.5 -translate-x-1/2 -translate-y-1/2 rounded-xs border border-line bg-raised shadow-chip"
            style={{ left: TRACK_X, top }}
          >
            <span className="absolute inset-x-[3px] top-1/2 h-px -translate-y-1/2 bg-fg-off" />
          </div>
        </div>
        {meter}
      </div>
      <div className="relative h-5 shrink-0 flex items-center justify-center gap-1">
        <span data-testid="fader-level" className="text-xs tabular-nums text-fg" title={follows}>
          {value === null ? "—" : level(v)}
        </span>
        <span data-testid="lane-marker" className="shrink-0 size-[5px] rounded-full bg-accent" title={follows} />
      </div>
      {held && <Bubble at={held}>{follows}</Bubble>}
      {menu && column.current && (
        <Popover anchor={column.current} onClose={() => setMenu(false)} role="menu" aria-label={`${name} volume menu`} data-testid="lane-fader-menu" className="w-[240px]">
          <MenuRow
            onClick={() => {
              setMenu(false);
              laneMade(lane.module, lane.lane);
            }}
          >
            Show {shown} lane
          </MenuRow>
          <MenuRow disabled={!!locked} title={locked ?? undefined} onClick={remove}>
            Remove automation
          </MenuRow>
          <div role="separator" className="my-1 h-px bg-line" />
          <MenuRow
            icon={<CodeXml size={14} />}
            detail={at ? <span data-code className="font-mono">{at.file}:{at.line}</span> : undefined}
            onClick={() => {
              setMenu(false);
              editInCode(at);
            }}
          >
            Edit in code
          </MenuRow>
        </Popover>
      )}
    </div>
  );
}

/** A fader's track, its foot and its marks: +12 dB at the top to −60 dB above the foot, which is darker: silence. */
function Rails() {
  return (
    <>
      <div className="absolute top-0 w-1 -translate-x-1/2 rounded-t-full bg-line-strong" style={{ left: TRACK_X, bottom: FOOT }} />
      <div className="absolute bottom-0 w-1 -translate-x-1/2 rounded-b-full bg-fg-off" style={{ left: TRACK_X, height: FOOT }} />
      {MARKS.map((m) => (
        <div
          key={m}
          aria-hidden
          className={clsx("absolute h-px", m === 0 ? "w-2 bg-fg-off" : "w-1 bg-line-strong")}
          style={{ left: `calc(${TRACK_X} - ${m === 0 ? 14 : 10}px)`, top: `calc((100% - ${FOOT}px) * ${1 - faderAt(m)})` }}
        />
      ))}
    </>
  );
}

/**
 * A strip with no fader: why, in words ("Not in the mixer"), and the way to the code. With a
 * meter, the words sit under it, where the level would read.
 */
function NoFader({ why, at, meter }: { why: string; at: { file: string; line: number } | null; meter: React.ReactNode }) {
  const words = (
    <div data-testid="no-fader" className={clsx("px-1.5 flex flex-col items-center gap-1.5 text-center", meter ? "shrink-0 pt-1.5 pb-2" : "flex-1 min-h-0 justify-center")}>
      <span className="text-xs text-fg-muted">{why}</span>
      <button className="text-xs font-medium text-accent hover:text-accent-strong cursor-pointer" onClick={() => editInCode(at)}>
        Edit in code
      </button>
    </div>
  );
  if (!meter)
    return (
      <>
        {words}
        <div className="h-5 shrink-0" />
      </>
    );
  return (
    <>
      <div className="relative flex-1 min-h-0">{meter}</div>
      {words}
    </>
  );
}

/**
 * Mute `modules` in the song, or unmute them (MON-5): a strip's tracks, as the header menu's row does
 * it for one: their `muted` option, one undo step, said in a toast with Undo.
 */
async function muteStripInSong(name: string, modules: string[], muted: boolean) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  try {
    const r = await muteInSongCall(graph.session, modules, muted);
    if (r.before !== r.after) toast(`${name} ${muted ? "muted" : "unmuted"} in the song`, undoAction(graph.session, r.step));
  } catch (error) {
    toast(message(error));
  }
}

/**
 * A strip's menu (right-click; taste.md §3.5): Rename, Change instrument… and Colour for one track's
 * strip; Mute in song for a strip of tracks (Unmute in song first when they all are); and Edit in
 * code.
 */
function StripMenu({
  name,
  module,
  bus,
  modules,
  anchor,
  at,
  locked,
  onClose,
}: {
  name: string;
  /** The track it is, when it is one track's. */
  module: string | null;
  /** It is a bus's strip: it has no instrument to change. */
  bus: boolean;
  /** The tracks it plays: none for Master and a channel no track reaches. */
  modules: string[];
  anchor: HTMLElement;
  at: { file: string; line: number } | null;
  locked: string | null;
  onClose: () => void;
}) {
  const [colours, setColours] = useState<HTMLElement | null>(null);
  const current = useStore((s) => s.graph?.timeline.tracks.find((t) => t.module === module)?.colour);
  const inSong = useStore((s) => modules.length > 0 && modules.every((m) => s.graph?.overlays?.heard[m] === "muted-in-song"));
  // It plays the drum machine: Make it a kit (KIT-24).
  const machine = useStore((s) => !!module && !!s.graph?.drum_machines?.includes(module));
  const keeps = useStore((s) => s.graph?.make_kit_keeps);
  const run = (f: () => void) => () => {
    onClose();
    f();
  };
  const sep = <div role="separator" className="my-1 h-px bg-line" />;
  const songMute = modules.length > 0 && (
    <MenuRow
      icon={<VolumeX size={14} />}
      disabled={!!locked}
      title={locked ?? undefined}
      data-testid="mute-in-song"
      detail={inSong ? undefined : `Saved in ${modules.length > 1 ? "their" : "its"} code: the export leaves ${modules.length > 1 ? "them" : "it"} out too`}
      onClick={run(() => void muteStripInSong(name, modules, !inSong))}
    >
      {inSong ? "Unmute in song" : "Mute in song"}
    </MenuRow>
  );
  return (
    <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={`${name} menu`} data-testid="strip-menu" className="w-[240px]">
      {inSong && (
        <>
          {songMute}
          {sep}
        </>
      )}
      {module && (
        <>
          <MenuRow icon={<Pencil size={14} />} disabled={!!locked} title={locked ?? undefined} onClick={run(() => useShell.getState().set({ renaming: { module, at: "strip" } }))}>
            Rename
          </MenuRow>
          {!bus && (
            <MenuRow
              icon={<ArrowLeftRight size={14} />}
              disabled={!!locked}
              title={locked ?? undefined}
              data-testid="change-instrument"
              onClick={run(() => openChangeInstrument(module, anchor))}
            >
              Change instrument…
            </MenuRow>
          )}
          {machine && (
            <MenuRow icon={<Drum size={14} />} disabled={!!locked} title={locked ?? undefined} data-testid="make-kit" detail={keeps} onClick={run(() => void makeKit(module))}>
              Make it a kit
            </MenuRow>
          )}
          <MenuRow
            icon={<span className="block size-3.5 rounded-xs" style={{ background: tones(current).bar }} />}
            disabled={!!locked}
            aria-haspopup="menu"
            aria-expanded={!!colours}
            data-testid="colour-row"
            onClick={(e) => setColours(colours ? null : e.currentTarget)}
            shortcut="›"
          >
            Colour
          </MenuRow>
        </>
      )}
      {!inSong && songMute}
      {modules.length > 0 && sep}
      {/* No key: ⌥4 opens the selected track's own file, not the channel's line. */}
      <MenuRow
        icon={<CodeXml size={14} />}
        detail={at ? <span data-code className="font-mono">{at.file}:{at.line}</span> : undefined}
        onClick={run(() => editInCode(at))}
      >
        Edit in code
      </MenuRow>
      {colours && module && <Colours module={module} current={current} anchor={colours} onClose={() => (setColours(null), onClose())} />}
    </Popover>
  );
}

/** Its value becomes a new, empty control lane's fallback (HOME-2), as a knob's menu does it. */
function automate(session: number, revision: number, knob: KnobInfo) {
  const toast = useStore.getState().toast;
  graphApi
    .automate(session, revision, knob.id)
    .then((r) => {
      toast(`Automating ${r.name} in a new lane`, undoAction(session, r.step));
      laneMade(r.lane.module, r.lane.lane);
    })
    .catch((error) => toast(message(error)));
}

/**
 * A fader (taste.md §3.5, §5.2): a 4 px track and a 20×10 px thumb, 0 dB at 75% of the travel and
 * −∞ in the 4 px foot. Drag anywhere on it, along the travel (⇧ for a tenth as far); ⌥-click for
 * 0 dB; an up-and-down scroll moves it 0.1 dB a notch, and a sideways one scrolls the strips (lead
 * decision T6); ↑ ↓ when focused, Return or a double-click on the level to type it, ⌫ for 0 dB;
 * right-click for the knob menu. A level the code leaves out reads in grey, and its first move
 * writes it in (design-8b §5.2).
 */
function Fader({
  knob,
  name,
  session,
  revision,
  editable,
  locked,
  meter,
}: {
  knob: KnobInfo;
  /** The strip's name: "Kick", "Master". */
  name: string;
  session: number;
  revision: number;
  editable: boolean;
  locked: string | null;
  /** The strip's meter, drawn in the fader's column. */
  meter: React.ReactNode;
}) {
  const label = `${name} volume`;
  const g = useKnobGesture({ session, revision, knob, editable, name: label, label });
  const glows = useGlows("knobs", knob.id);
  const value = g.value;
  const column = useRef<HTMLDivElement>(null);
  const travel = useRef<HTMLDivElement>(null);
  const thumb = useRef<HTMLDivElement>(null);
  // The drag: the pointer, where it was last, where that has moved the thumb (px up from the foot's
  // top; below 0 is the foot), the travel's length, and the level last sent.
  const drag = useRef<{ pointer: number; y: number; at: number; room: number; v: number } | null>(null);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);
  const [typing, setTyping] = useState<{ text: string; bad: boolean } | null>(null);
  const typingOpen = useRef(false);
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const disabled = (!editable && !g.dragging()) || g.saving;

  /** `n` steps (taste.md §5.2), written as one edit half a second after the last. */
  const step = (n: number) => g.step((v) => faderStep(v, n));
  const stepping = useRef(step);
  stepping.current = step;
  const wheel = useRef(0);
  useEffect(() => {
    const el = column.current;
    if (!el || disabled) return;
    const turn = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey || drag.current) return;
      // Sideways: the strips scroll (T6).
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      e.preventDefault();
      // Shift with the wheel never moves a fader, on any OS (next D4f): it scrolls the strips.
      // A browser that sends it as up and down (Linux, Windows) gets it turned sideways here.
      if (e.shiftKey) {
        const strips = el.closest<HTMLElement>("[data-testid=mix-strips]");
        if (strips) strips.scrollLeft += e.deltaMode === 1 ? e.deltaY * NOTCH : e.deltaY;
        return;
      }
      wheel.current -= e.deltaMode === 1 ? e.deltaY * NOTCH : e.deltaY;
      const n = Math.trunc(wheel.current / NOTCH);
      if (n === 0) return;
      wheel.current -= n * NOTCH;
      stepping.current(n);
    };
    el.addEventListener("wheel", turn, { passive: false });
    return () => el.removeEventListener("wheel", turn);
  }, [disabled]);

  const finish = (id: number, cancelled: boolean) => {
    if (drag.current?.pointer !== id) return;
    drag.current = null;
    setPointer(null);
    if (cancelled) g.abandon();
    else g.end();
  };
  const handlers = {
    onPointerDown: (e: React.PointerEvent<HTMLDivElement>) => {
      if (disabled || drag.current || e.button !== 0) return;
      // ⌥-click: 0 dB, as one edit (Logic).
      if (e.altKey) {
        e.preventDefault();
        if (value !== 0) g.change(0);
        return;
      }
      const room = Math.max(1, (travel.current?.getBoundingClientRect().height ?? 0) - FOOT);
      e.currentTarget.setPointerCapture(e.pointerId);
      drag.current = { pointer: e.pointerId, y: e.clientY, at: value <= SILENT ? -FOOT / 2 : faderAt(value) * room, room, v: value };
      setPointer({ x: e.clientX, y: e.clientY });
      thumb.current?.focus({ preventScroll: true });
      g.start();
    },
    onPointerMove: (e: React.PointerEvent<HTMLDivElement>) => {
      const d = drag.current;
      if (!d || d.pointer !== e.pointerId) return;
      setPointer({ x: e.clientX, y: e.clientY });
      // Each move by its own distance, coarse or (⇧) fine, so ⇧ pressed or let go never jumps.
      d.at = Math.min(d.room, Math.max(-FOOT, d.at + (d.y - e.clientY) * (e.shiftKey ? FINE : 1)));
      d.y = e.clientY;
      const v = d.at < 0 ? SILENT : faderDb(d.at / d.room);
      if (v === d.v) return;
      d.v = v;
      g.change(v);
    },
    onPointerUp: (e: React.PointerEvent<HTMLDivElement>) => finish(e.pointerId, false),
    onPointerCancel: (e: React.PointerEvent<HTMLDivElement>) => finish(e.pointerId, true),
    onLostPointerCapture: (e: React.PointerEvent<HTMLDivElement>) => finish(e.pointerId, true),
  };

  /** Type the level: the field turns into an input, its text selected. */
  const type = () => {
    // Steps just made are written first.
    g.settle();
    if (!editable || g.dragging()) return;
    typingOpen.current = true;
    setTyping({ text: level(value), bad: false });
  };
  const closeTyping = () => {
    typingOpen.current = false;
    setTyping(null);
  };
  /** Write what was typed, if it reads as a level; false (and the field stays) if it doesn't. */
  const typed = (text: string) => {
    const v = parseLevel(text);
    if (v === null) return false;
    closeTyping();
    if (v !== knob.value) g.change(v);
    return true;
  };

  // A level the code leaves out plays its default until its first move writes it in (PKG-3, MX-2).
  const written = knob.written ? `written as ${knob.literal}` : "not written in your code yet: moving it writes it in";
  const title = !editable
    ? `${locked ?? "It can't be edited right now"}.`
    : `${label}, ${level(value)}: ${written}\nDrag (${mod.shift} for fine) · scroll · double-click to type a level · ${mod.alt}-click for 0 dB · the foot is silence`;
  const top = value <= SILENT ? `calc(100% - ${FOOT / 2}px)` : `calc((100% - ${FOOT}px) * ${1 - faderAt(value)})`;
  return (
    <div
      className="flex-1 min-h-0 flex flex-col"
      data-knob={knob.label}
      data-knob-id={editable ? knob.id : undefined}
      data-glow={glows || undefined}
      data-value={value}
      data-default={!knob.written || undefined}
      data-fader={name}
      aria-disabled={disabled}
      // A double-click anywhere on it types a level (taste.md §5.2), but in the field itself.
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("input") && type()}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        const r = e.currentTarget.getBoundingClientRect();
        if (!g.dragging()) setMenu({ x: r.left + 4, y: r.bottom + 4, above: r.top - 4 });
      }}
    >
      <div ref={column} title={title} {...handlers} className={clsx("relative flex-1 min-h-0 py-[5px] touch-none", !disabled && "cursor-ns-resize")}>
        <div ref={travel} data-testid="fader-travel" className="relative h-full">
          <Rails />
          <div
            ref={thumb}
            role="slider"
            tabIndex={disabled ? -1 : 0}
            aria-label={label}
            aria-valuemin={SILENT}
            aria-valuemax={TOP}
            aria-valuenow={value}
            aria-valuetext={value <= SILENT ? "−∞, silent" : level(value)}
            data-testid="fader-thumb"
            onKeyDown={(e) => {
              if (e.metaKey || e.ctrlKey || e.altKey || pointer) return;
              if (e.key === "ArrowUp" || e.key === "ArrowDown") step((e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 10 : 1));
              else if (e.key === "Enter") type();
              else if (e.key === "Backspace" || e.key === "Delete") {
                if (value !== 0) g.change(0);
              } else return;
              e.preventDefault();
              e.stopPropagation();
            }}
            className={clsx(
              "absolute w-5 h-2.5 -translate-x-1/2 -translate-y-1/2 rounded-xs border bg-raised shadow-chip outline-none focus-visible:ring-2 focus-visible:ring-accent-range",
              disabled ? "border-line" : pointer ? "border-accent" : "border-line-strong",
            )}
            style={{ left: TRACK_X, top }}
          >
            <span className={clsx("absolute inset-x-[3px] top-1/2 h-px -translate-y-1/2", disabled ? "bg-fg-off" : "bg-fg-strong")} />
          </div>
        </div>
        {meter}
      </div>
      <div className="relative h-5 shrink-0 flex items-center justify-center">
        {typing ? (
          <>
            <input
              autoFocus
              data-testid="knob-type-in"
              aria-label={`${label}: type a level`}
              aria-invalid={typing.bad || undefined}
              value={typing.text}
              spellCheck={false}
              onFocus={(e) => e.currentTarget.select()}
              onChange={(e) => setTyping({ text: e.target.value, bad: false })}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter") {
                  if (!typed(typing.text)) setTyping({ ...typing, bad: true });
                } else if (e.key === "Escape") closeTyping();
                else if (e.key === "Tab") {
                  e.preventDefault();
                  if (!typed(typing.text)) return setTyping({ ...typing, bad: true });
                  focusNext(thumb.current, e.shiftKey);
                }
              }}
              onPointerDown={(e) => e.stopPropagation()}
              // A click away writes what reads as a level, and drops what doesn't.
              onBlur={() => typingOpen.current && !typed(typing.text) && closeTyping()}
              className={clsx("w-14 h-5 px-1 rounded-sm bg-raised border text-center text-xs text-fg outline-none", typing.bad ? "border-danger" : "border-accent")}
            />
            {typing.bad && (
              <span role="alert" className="absolute left-1 bottom-full mb-0.5 z-10 whitespace-nowrap px-1 rounded-sm bg-raised text-xs text-danger">
                Type a level, like −6 or −inf
              </span>
            )}
          </>
        ) : (
          <span
            data-testid="fader-level"
            className={clsx("text-xs tabular-nums", knob.written ? "text-fg" : "text-fg-muted", editable && "cursor-text")}
            title={knob.written ? `${label}: double-click to type it` : "Default. Not written in your code yet: changing it writes it in"}
          >
            {level(value)}
          </span>
        )}
      </div>
      {pointer && <Bubble at={pointer}>{level(value)}</Bubble>}
      {menu && (
        <KnobMenu
          knob={knob}
          label={label}
          value={value}
          at={menu}
          editable={editable && !g.saving}
          locked={locked}
          onAutomate={() => {
            setMenu(null);
            automate(session, revision, knob);
          }}
          onReset={() => g.change(0)}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/** A channel's pan that no code writes yet: at the centre, until its first move writes it (design-8b §5.3). */
function centre(channel: string): KnobInfo {
  return {
    id: "", label: `${channel}.pan`, value: 0, literal: "", unit: "bipolar", range: [-1, 1], log: false, written: false, default: 0,
    slot: `channels.${channel}.pan`, component: "std:mixer", origin: { site: -1 }, uses: 1, file: "", line: 0, home: "code",
  };
}

/**
 * A strip's pan (24 px) and its value: "C", "L 20", "R 35". A channel that writes no pan reads "C"
 * in grey. Its first move writes `pan: 0` into the channel's entry once a drag passes 4 px (one
 * undo step), and the drag goes on from there as a lease on the new knob; letting go writes its
 * value (another). So two undos put the entry back as it was (design-8b §5.3).
 */
function Pan({
  channel,
  name,
  knob,
  session,
  revision,
  editable,
  locked,
}: {
  channel: string;
  name: string;
  /** The pan's knob, or null while the channel writes none. */
  knob: KnobInfo | null;
  session: number;
  revision: number;
  editable: boolean;
  locked: string | null;
}) {
  const label = `${name} pan`;
  const toast = useStore((s) => s.toast);
  const blank = useMemo(() => centre(channel), [channel]);
  // The pan its first move wrote, in the revision that wrote it, until the view has it as a knob.
  const [made, setMade] = useState<{ knob: KnobInfo; revision: number } | null>(null);
  useEffect(() => {
    if (knob) setMade(null);
  }, [knob]);
  const live = knob ? { knob, revision } : (made ?? { knob: blank, revision });
  const first = !knob && !made;
  const g = useKnobGesture({ session, revision: live.revision, knob: live.knob, editable, name: label, label });
  const glows = useGlows("knobs", knob?.id);
  // The first pan being written: the value to go on with, how it goes on, and whether the drag was let go (or lost) meanwhile.
  const writing = useRef<{ value: number; how: "drag" | "step" | "value"; released: boolean; cancelled: boolean } | null>(null);
  // The first pan was refused during this press: the rest of it asks no more (one toast, not one a move).
  const refused = useRef(false);
  const svg = useRef<SVGSVGElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; above: number } | null>(null);
  const [typing, setTyping] = useState<{ text: string; bad: boolean } | null>(null);
  const typingOpen = useRef(false);
  const k = g.descriptor;
  const value = g.value;

  /**
   * Write `pan: 0`, then go on: a drag holds on as a lease on the new knob; a run of steps (the
   * wheel, ↑ ↓) goes on as one gesture, written half a second after the last, so its notches are
   * one undo step however long they last (T6); a typed value is written as one more step.
   */
  const writeFirst = (v: number, how: "drag" | "step" | "value") => {
    if (writing.current) {
      writing.current.value = v;
      return;
    }
    const now = { value: v, how, released: how !== "drag", cancelled: false };
    writing.current = now;
    graphApi
      .mixInsert(session, channel, "pan", 0, { name: label })
      .then((r) => {
        const m = { knob: { ...blank, id: r.control, written: true, literal: "0" }, revision: r.revision };
        setMade(m);
        if (now.cancelled) return g.show(0);
        g.start(m);
        // The notches that came meanwhile, and the ones after: one gesture, which the idle timer ends.
        if (now.how === "step") return g.step(() => now.value);
        g.change(now.value);
        if (now.released) g.end();
      })
      .catch((error) => {
        refused.current = true;
        g.show(0);
        toast(message(error));
      })
      .finally(() => {
        if (writing.current === now) writing.current = null;
      });
  };

  /** A move to `v`: the first one writes the pan first (`drag` past 4 px, or a step or a typed value). */
  const change = (v: number) => (first ? writeFirst(v, "value") : g.change(v));
  const step = (n: number) => {
    if (!first) return g.step((v, k) => fromUnit(k, Math.min(1, Math.max(0, toUnit(k, stepValue(k, v, n))))));
    const v = stepValue(blank, writing.current?.value ?? 0, n);
    g.show(v);
    writeFirst(v, "step");
  };
  const type = () => {
    g.settle();
    if (!editable || g.dragging()) return;
    typingOpen.current = true;
    setTyping({ text: display(k, value), bad: false });
  };
  const closeTyping = () => {
    typingOpen.current = false;
    setTyping(null);
  };
  const typed = (text: string) => {
    const v = parseValue(k, text);
    if (v === null) return false;
    closeTyping();
    if (v !== k.value) change(v);
    return true;
  };

  const disabled = (!editable && !g.dragging()) || g.saving;
  const unit = Math.min(1, Math.max(0, toUnit(k, value)));
  const grip = useKnobDrag({
    value: unit,
    reset: 0.5,
    disabled,
    onStart: () => {
      refused.current = false;
      if (!first) g.start();
    },
    onChange: (u, moved) => {
      const v = fromUnit(k, u);
      if (!first || g.dragging()) return void g.change(v);
      if (refused.current) return;
      // Not written yet: shown as it goes, and written once the drag passes 4 px.
      g.show(v);
      if (writing.current) writing.current.value = v;
      else if (moved >= 4) writeFirst(v, "drag");
    },
    onEnd: () => {
      if (g.dragging()) return g.end();
      if (writing.current) writing.current.released = true;
      // A press that never passed 4 px writes nothing.
      else if (first) g.show(0);
    },
    onCancel: () => {
      if (g.dragging()) return g.abandon();
      // Lost while `pan: 0` is being written: it stays written, at the centre.
      if (writing.current) writing.current.cancelled = true;
      g.show(0);
    },
    focus: () => svg.current?.focus({ preventScroll: true }),
  });
  const written = !!knob?.written || !!made;
  const title = !editable
    ? `${locked ?? "It can't be edited right now"}.`
    : `${label}, ${display(k, value)}${written ? "" : ": the centre, not written in your code yet: moving it writes it in"}\nDrag (${mod.shift} for fine) · double-click to type a value · ${mod.alt}-click for the centre`;
  return (
    <div
      data-knob={k.label}
      data-knob-id={editable && knob ? knob.id : undefined}
      data-glow={glows || undefined}
      data-value={value}
      data-default={!written || undefined}
      aria-disabled={disabled}
      title={title}
      {...grip.handlers}
      onDoubleClick={(e) => !(e.target as HTMLElement).closest("input") && type()}
      onContextMenu={(e) => {
        // A written pan has the knob menu; one not written yet, the strip's.
        if (!knob) return;
        e.preventDefault();
        e.stopPropagation();
        const r = e.currentTarget.getBoundingClientRect();
        if (!g.dragging()) setMenu({ x: r.left + 4, y: r.bottom + 4, above: r.top - 4 });
      }}
      className={clsx("group/knob relative h-full flex flex-col items-center", !disabled && "cursor-ns-resize touch-none")}
    >
      <div inert={!editable && !g.dragging()} className={clsx("h-8 grid place-items-center", disabled && "pointer-events-none")}>
        <MiniKnob
          value={unit}
          bipolar
          size={24}
          vertical
          disabled={disabled}
          readout={display(k, value)}
          label={label}
          reset={0.5}
          svg={svg}
          pointer={grip.pointer}
          onChange={(u) => change(fromUnit(k, u))}
          onStep={step}
          onType={type}
        />
      </div>
      {typing ? (
        <span className="relative">
          <input
            autoFocus
            data-testid="knob-type-in"
            aria-label={`${label}: type a value`}
            aria-invalid={typing.bad || undefined}
            value={typing.text}
            spellCheck={false}
            onFocus={(e) => e.currentTarget.select()}
            onChange={(e) => setTyping({ text: e.target.value, bad: false })}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") {
                if (!typed(typing.text)) setTyping({ ...typing, bad: true });
              } else if (e.key === "Escape") closeTyping();
              else if (e.key === "Tab") {
                e.preventDefault();
                if (!typed(typing.text)) return setTyping({ ...typing, bad: true });
                focusNext(svg.current, e.shiftKey);
              }
            }}
            onBlur={() => typingOpen.current && !typed(typing.text) && closeTyping()}
            onPointerDown={(e) => e.stopPropagation()}
            className={clsx("block w-14 h-3.5 px-1 rounded-sm bg-raised border text-center text-xs leading-none text-fg outline-none", typing.bad ? "border-danger" : "border-accent")}
          />
          {typing.bad && (
            <span role="alert" className="absolute left-1/2 -translate-x-1/2 top-full mt-0.5 z-10 whitespace-nowrap px-1 rounded-sm bg-raised text-xs text-danger">
              Type a pan, like L20, C or R35
            </span>
          )}
        </span>
      ) : (
        <span className={clsx("h-3.5 text-xs leading-[14px] tabular-nums", written ? "text-fg" : "text-fg-muted")}>{display(k, value)}</span>
      )}
      {menu && knob && (
        <KnobMenu
          knob={knob}
          label={label}
          value={value}
          at={menu}
          editable={editable && !g.saving}
          locked={locked}
          onAutomate={() => {
            setMenu(null);
            automate(session, revision, knob);
          }}
          onReset={() => g.change(0)}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- buses (RENDER-34)

/** A bus strip's mark, before its name: tracks merge into it. Read as "Bus". */
function BusTag() {
  return (
    <span data-testid="bus-tag" className="shrink-0 inline-flex text-fg-muted" title="A bus: it plays what other tracks send it">
      <Merge size={12} aria-hidden />
      <span className="sr-only">Bus</span>
    </span>
  );
}

/** What the Add bus menu makes, and what each is for, in words. */
const BUS_KINDS: { kind: BusKind; label: string; detail: string }[] = [
  { kind: "reverb", label: "Reverb", detail: "A shared room. Turn up a track's Reverb knob to send it there." },
  { kind: "delay", label: "Delay", detail: "Shared echoes. Turn up a track's Delay knob to send it there." },
  { kind: "group", label: "Group", detail: "One fader for several tracks. Set a track's output to it." },
];

/** After the strips: Add bus, and its menu (Reverb, Delay, Group). One undo step; a toast says what to do next. */
function AddBus({ session, locked }: { session: number; locked: string | null }) {
  const toast = useStore((s) => s.toast);
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  const add = (k: (typeof BUS_KINDS)[number]) => {
    setMenu(null);
    graphApi
      .mixBus(session, k.kind, k.label)
      .then((r) => {
        const next = k.kind === "group" ? "Set a track's output to it." : `Turn up a track's ${r.label} knob to send to it.`;
        toast(`Added the ${r.label} bus. ${next}`, undoAction(session, r.step));
      })
      .catch((error) => toast(message(error)));
  };
  return (
    <div className="w-[76px] shrink-0 flex flex-col items-center pt-1">
      <button
        data-testid="add-bus"
        disabled={!!locked}
        aria-haspopup="menu"
        aria-expanded={!!menu}
        title={locked ? `${locked}.` : "Add a bus: a shared reverb, a delay, or a group"}
        onClick={(e) => setMenu(menu ? null : e.currentTarget)}
        className="h-5 px-1.5 rounded-sm inline-flex items-center gap-1 text-xs font-medium text-fg-muted enabled:hover:text-fg enabled:hover:bg-hover disabled:text-fg-off cursor-pointer disabled:cursor-default"
      >
        <Plus size={12} />
        Add bus
      </button>
      {menu && (
        <Popover anchor={menu} onClose={() => setMenu(null)} role="menu" aria-label="Add bus" data-testid="add-bus-menu" className="w-[280px]">
          {BUS_KINDS.map((k) => (
            <MenuRow key={k.kind} data-bus-kind={k.kind} detail={k.detail} onClick={() => add(k)}>
              {k.label}
            </MenuRow>
          ))}
        </Popover>
      )}
    </div>
  );
}

/**
 * A strip's row for one bus: its send's knob when it has one (a dot on it when the send is before
 * the fader), else a + that adds one, at −12 dB after the fader. A + that would make a loop is
 * greyed and says why. A send whose level code sets reads "—".
 */
function SendRow({
  strip,
  bus,
  send,
  knobs,
  session,
  revision,
  locked,
  at,
}: {
  strip: MixStrip;
  bus: Bus;
  send: StripSend | null;
  knobs: Map<string, KnobInfo>;
  session: number;
  revision: number;
  locked: string | null;
  at: { file: string; line: number } | null;
}) {
  const toast = useStore((s) => s.toast);
  const loop = strip.loops.find((l) => l.bus === bus.key);
  const name = `${strip.label} send to ${bus.label}`;
  const knob = send?.knob ? (knobs.get(send.knob) ?? null) : null;
  const add = () =>
    graphApi.mixSend(session, strip.channel!, bus.key, "add", false, { name: `adding ${name}`, structural: true }).catch((error) => toast(message(error)));
  return (
    <div
      data-send={bus.key}
      data-state={send ? "on" : loop ? "loop" : "off"}
      data-pre={send?.pre || undefined}
      title={!send && loop ? loop.send : undefined}
      className="shrink-0 px-1.5 flex items-center gap-1 min-w-0"
      style={{ height: SEND_H }}
    >
      {send && knob ? (
        <SendKnob knob={knob} name={name} send={send} strip={strip} bus={bus} session={session} revision={revision} locked={locked} at={at} />
      ) : send ? (
        <span className="w-4 shrink-0 text-center text-xs text-fg-faint" title={send.lane ? "Its level follows a lane" : "Its level is set in code"}>
          —
        </span>
      ) : (
        <button
          data-send-add
          disabled={!!locked || !!loop}
          aria-disabled={!!locked || !!loop || undefined}
          aria-label={`Send ${strip.label} to ${bus.label}`}
          title={loop ? loop.send : locked ? `${locked}.` : `Send ${strip.label} to ${bus.label}: −12 dB, after its fader`}
          onClick={add}
          className="size-4 shrink-0 grid place-items-center rounded-xs text-fg-muted enabled:hover:text-fg enabled:hover:bg-hover disabled:text-fg-off cursor-pointer disabled:cursor-default"
        >
          <Plus size={12} />
        </button>
      )}
      <span className={clsx("min-w-0 truncate text-xs", send ? "text-fg-muted" : "text-fg-faint")}>{bus.label}</span>
    </div>
  );
}

/**
 * A send's level: a 16 px knob, heard while it turns and written once let go, one undo step, as a
 * fader is. Right-click: Before the fader, Automate, Remove send, Edit in code.
 */
function SendKnob({
  knob,
  name,
  send,
  strip,
  bus,
  session,
  revision,
  locked,
  at,
}: {
  knob: KnobInfo;
  name: string;
  send: StripSend;
  strip: MixStrip;
  bus: Bus;
  session: number;
  revision: number;
  locked: string | null;
  at: { file: string; line: number } | null;
}) {
  const toast = useStore((s) => s.toast);
  const editable = !locked;
  const g = useKnobGesture({ session, revision, knob, editable, name, label: name });
  const glows = useGlows("knobs", knob.id);
  const k = g.descriptor;
  const value = g.value;
  const svg = useRef<SVGSVGElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const disabled = (!editable && !g.dragging()) || g.saving;
  const clamp = (u: number) => Math.min(1, Math.max(0, u));
  const unit = clamp(toUnit(k, value));
  const reset = clamp(toUnit(k, k.default ?? value));
  const grip = useKnobDrag({
    value: unit,
    reset,
    disabled,
    onStart: () => g.start(),
    onChange: (u) => g.change(fromUnit(k, u)),
    onEnd: () => g.end(),
    onCancel: () => g.abandon(),
    focus: () => svg.current?.focus({ preventScroll: true }),
  });
  const send_ = (action: "remove" | "pre", pre: boolean, step: string) => {
    setMenu(false);
    graphApi.mixSend(session, strip.channel!, bus.key, action, pre, { name: step, structural: true }).catch((error) => toast(message(error)));
  };
  const where = send.pre ? `before ${strip.label}'s fader` : `after ${strip.label}'s fader`;
  return (
    <div
      ref={root}
      data-knob={k.label}
      data-knob-id={editable ? knob.id : undefined}
      data-glow={glows || undefined}
      data-value={value}
      aria-disabled={disabled}
      title={!editable ? `${locked}.` : `${name}, ${display(k, value)}, ${where}\nDrag (${mod.shift} for fine) · scroll · right-click for more`}
      {...grip.handlers}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (!g.dragging()) setMenu(true);
      }}
      className={clsx("relative shrink-0", !disabled && "cursor-ns-resize touch-none")}
    >
      <div inert={!editable && !g.dragging()} className={clsx(disabled && "pointer-events-none")}>
        <MiniKnob
          value={unit}
          bipolar={false}
          size={16}
          vertical
          disabled={disabled}
          readout={display(k, value)}
          label={name}
          reset={reset}
          svg={svg}
          pointer={grip.pointer}
          onChange={(u) => g.change(fromUnit(k, u))}
          onStep={(n) => g.step((v, d) => fromUnit(d, clamp(toUnit(d, stepValue(d, v, n)))))}
          onType={() => setMenu(true)}
        />
      </div>
      {send.pre && <span data-testid="pre-dot" className="absolute -top-px -right-px size-[5px] rounded-full bg-fg-strong" title="Before the fader" />}
      {menu && root.current && (
        <Popover anchor={root.current} onClose={() => setMenu(false)} role="menu" aria-label={`${name} menu`} data-testid="send-menu" className="w-[260px]">
          <div className="h-ctl px-2 flex items-center gap-2">
            <span className="text-sm font-semibold text-fg truncate">{name}</span>
            <span className="ml-auto shrink-0 text-xs text-fg-muted">{display(k, value)}</span>
          </div>
          <div role="separator" className="my-1 h-px bg-line" />
          <MenuRow
            role="menuitemcheckbox"
            aria-checked={send.pre}
            data-menu-item="pre"
            icon={send.pre ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />}
            disabled={!!locked}
            title={locked ?? undefined}
            detail={send.pre ? `${strip.label}'s fader doesn't change it` : `Now ${strip.label}'s fader turns it down too`}
            onClick={() => send_("pre", !send.pre, `${name} ${send.pre ? "after" : "before"} the fader`)}
          >
            Before the fader
          </MenuRow>
          <MenuRow
            icon={<Spline size={14} />}
            data-menu-item="automate"
            disabled={!!locked || !!knob.automate_refused}
            title={locked ?? knob.automate_refused ?? undefined}
            onClick={() => {
              setMenu(false);
              automate(session, revision, knob);
            }}
          >
            Automate
          </MenuRow>
          <MenuRow icon={<Trash2 size={14} />} data-menu-item="remove" disabled={!!locked} title={locked ?? undefined} onClick={() => send_("remove", false, `removing ${name}`)}>
            Remove send
          </MenuRow>
          <div role="separator" className="my-1 h-px bg-line" />
          <MenuRow
            icon={<CodeXml size={14} />}
            detail={at ? <span data-code className="font-mono">{at.file}:{at.line}</span> : undefined}
            onClick={() => {
              setMenu(false);
              editInCode(at);
            }}
          >
            Edit in code
          </MenuRow>
        </Popover>
      )}
    </div>
  );
}

/**
 * Where a strip's sound goes, under its fader: Master, or a bus. A choice that would make a loop is
 * greyed and says why. One undo step.
 */
function Output({ strip, buses, session, locked }: { strip: MixStrip; buses: Bus[]; session: number; locked: string | null }) {
  const toast = useStore((s) => s.toast);
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  const named = (key: string | null) => (key ? (buses.find((b) => b.key === key)?.label ?? key) : "Master");
  const here = named(strip.output);
  const choose = (key: string | null) => {
    setMenu(null);
    if (key === strip.output) return;
    graphApi.mixRoute(session, strip.channel!, key, { name: `${strip.label} to ${named(key)}`, structural: true }).catch((error) => toast(message(error)));
  };
  const tick = (on: boolean) => (on ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />);
  return (
    <>
      <button
        data-testid="strip-output"
        aria-haspopup="menu"
        aria-expanded={!!menu}
        aria-label={`${strip.label} goes to ${here}`}
        title={`${strip.label} goes to ${here}. Click to change where it goes`}
        onClick={(e) => setMenu(menu ? null : e.currentTarget)}
        className="max-w-full h-5 px-1.5 rounded-sm truncate text-xs text-fg-muted hover:text-fg hover:bg-hover cursor-pointer"
      >
        {here}
      </button>
      {menu && (
        <Popover anchor={menu} onClose={() => setMenu(null)} role="menu" aria-label={`Where ${strip.label} goes`} data-testid="output-menu" className="w-[280px]">
          <MenuRow role="menuitemradio" aria-checked={!strip.output} data-output="master" icon={tick(!strip.output)} disabled={!!locked} onClick={() => choose(null)}>
            Master
          </MenuRow>
          {buses
            .filter((b) => b.key !== strip.channel)
            .map((b) => {
              const loop = strip.loops.find((l) => l.bus === b.key);
              return (
                <MenuRow
                  key={b.key}
                  role="menuitemradio"
                  aria-checked={strip.output === b.key}
                  aria-disabled={!!loop || undefined}
                  data-output={b.key}
                  icon={tick(strip.output === b.key)}
                  disabled={!!locked || !!loop}
                  title={loop?.to}
                  detail={loop?.to}
                  onClick={() => choose(b.key)}
                >
                  {b.label}
                </MenuRow>
              );
            })}
        </Popover>
      )}
    </>
  );
}

import clsx from "clsx";
import { CircleAlert, CodeXml, Download, Gauge, HardDrive, Hourglass, Info, Lock, Pause, TriangleAlert, VolumeX, WifiOff } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { attention, graphApi, lockReason, type GraphState, type GraphStatus, type Problem, type ReadOnly } from "../lib/graph";
import { cancelGet, getMissing, missingPacks, sizeWords, useGetting, type MissingPack } from "../lib/library";
import { usePill } from "../lib/overlays";
import { openSettings } from "../lib/settings";
import { placeOf, technical, Words } from "../lib/plain";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import Attention from "./Attention";
import { Button, Details, keepFocus, Popover } from "./ui";

type Tone = "danger" | "warn" | "neutral";
interface State { id: "offline" | "compile" | "problems" | "no-output" | "dropout" | "disk" | "meters" | "load" | "read-only" | "paused" | "loading"; tone: Tone; label: string; Icon: typeof Lock }

// Hover darkens every tone, as it does a ghost button: the soft fill mixed toward its strong colour.
const TONE: Record<Tone, string> = {
  danger: "bg-danger-soft text-danger-strong hover:bg-[color-mix(in_oklab,var(--color-danger-soft)_88%,var(--color-danger-strong))]",
  warn: "bg-warn-soft text-warn-strong hover:bg-[color-mix(in_oklab,var(--color-warn-soft)_88%,var(--color-warn-strong))]",
  neutral: "bg-hover text-fg hover:bg-press",
};
const ICON: Record<Tone, string> = { danger: "text-danger", warn: "text-warn", neutral: "text-fg-muted" };

/** The plugins loading, as the pill says it: "Loading Surge XT…". */
const loadingLabel = (names: string[]) => `Loading ${names.join(", ")}…`;

/** "Heavy load" from this share of the time the sound has (8b.8; taste.md's DSP turned `warn` here too). */
const HEAVY = 0.7;
/** How long "Sound dropped out" stays after a dropout (Q24): long enough to read. */
const DROPPED_MS = 10_000;

/** The dropouts since play started, and whether one came in the last 10 s. */
interface Dropouts {
  since: number;
  recent: boolean;
}

/**
 * What the pill can say, the most severe first (taste.md ST-1). While playing, a dropout in the last
 * 10 s and a heavy load (8b.8) come after the song's problems. What the engine is busy with
 * (RENDER-1, RENDER-25) comes last: it is no problem, so any problem shows over it.
 */
function states(graph: GraphState, status: GraphStatus | undefined, connected: boolean, dropouts: Dropouts): State[] {
  const out: State[] = [];
  if (!connected) out.push({ id: "offline", tone: "danger", label: "Reconnecting…", Icon: WifiOff });
  // Danger when nothing plays at all (WRITE-20); otherwise the last version that compiled plays on.
  if (graph.view.draft_errors.length > 0)
    out.push({ id: "compile", tone: graph.has_code === false ? "danger" : "warn", label: "Code doesn't compile", Icon: graph.has_code === false ? CircleAlert : TriangleAlert });
  const items = attention(graph);
  if (items.length) {
    const worst = items[0].severity;
    out.push({
      id: "problems",
      tone: worst === "error" ? "danger" : worst === "warning" ? "warn" : "neutral",
      label: `${items.length} ${items.length === 1 ? "problem" : "problems"}`,
      Icon: worst === "error" ? CircleAlert : worst === "warning" ? TriangleAlert : Info,
    });
  }
  // No sound device started: the song plays silently until one does (gaps #13).
  if (status?.no_output) out.push({ id: "no-output", tone: "warn", label: "No sound output", Icon: VolumeX });
  if (status?.playing && dropouts.recent) out.push({ id: "dropout", tone: "warn", label: "Sound dropped out", Icon: TriangleAlert });
  // An audio clip read from the disk too late played a moment of silence (RENDER-30).
  if (status?.disk) out.push({ id: "disk", tone: "warn", label: "Disk too slow", Icon: HardDrive });
  // The meters couldn't be placed after the faders (TAP-5): a bug, said, not hidden.
  if (status?.meters) out.push({ id: "meters", tone: "neutral", label: "Meters before faders", Icon: Info });
  if (status?.playing && (status.load ?? 0) >= HEAVY) out.push({ id: "load", tone: "warn", label: "Heavy load", Icon: Gauge });
  if (graph.read_only) out.push({ id: "read-only", tone: "neutral", label: "Read only", Icon: Lock });
  if (status?.paused === "exporting") out.push({ id: "paused", tone: "neutral", label: "Paused for export", Icon: Pause });
  if (status?.loading?.length) out.push({ id: "loading", tone: "neutral", label: loadingLabel(status.loading), Icon: Hourglass });
  return out;
}

/**
 * Dropouts as the status counts them (`xruns` the device reported, and `late` callbacks: both are
 * sound lost), from when play started, and whether one came in the last 10 s (Q24). Counted while
 * playing only: a stopped song has nothing to drop.
 */
function useDropouts(status: GraphStatus | undefined): Dropouts {
  const playing = !!status?.playing;
  const count = (status?.xruns ?? 0) + (status?.late ?? 0);
  const seen = useRef({ playing: false, base: 0, last: 0, at: 0 });
  const s = seen.current;
  const now = Date.now();
  if (playing && !s.playing) Object.assign(s, { base: count, last: count, at: 0 });
  else if (playing && count > s.last) Object.assign(s, { last: count, at: now });
  // Stopped, or the player started again (a new device) and its counts with it: no dropout.
  else if (count !== s.last) Object.assign(s, { base: s.base + count - s.last, last: count });
  s.playing = playing;
  return { since: s.last - s.base, recent: playing && s.at > 0 && now - s.at < DROPPED_MS };
}

/**
 * The one place for status (taste.md §3.3, ST-1): a pill in the top bar, there only while something
 * needs saying, taking no room in the layout. It shows the most severe state and counts the rest;
 * a click opens a popover with each of them and what fixes it. Nothing else repeats them.
 */
export default function Status({ graph, status, onShowCode }: { graph: GraphState; status?: GraphStatus; onShowCode: (p: Problem) => void }) {
  const connected = useStore((s) => s.connected);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  // The mutes pill beside it takes room the song's name keeps clear (taste.md §3.3, Q23).
  const pill = usePill();
  const dropouts = useDropouts(status);
  const all = states(graph, status, connected, dropouts);
  if (!all.length) return null;
  const [main, ...rest] = all;
  const readOnly = all.some((s) => s.id === "read-only");
  return (
    <>
      <button
        // The pill keeps the read-only banner's id: it is what says so now.
        data-testid={readOnly ? "read-only-banner" : "status"}
        data-status={all.map((s) => s.id).join(" ")}
        aria-haspopup="dialog"
        aria-expanded={!!anchor}
        title={all.map((s) => s.label).join(" · ")}
        onMouseDown={keepFocus}
        onClick={(e) => {
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
        className={clsx("h-ctl-sm px-2 shrink-0 rounded-sm inline-flex items-center gap-1.5 text-sm font-medium cursor-pointer transition-colors duration-80 ease-linear", TONE[main.tone])}
      >
        <main.Icon size={14} className={clsx("shrink-0", ICON[main.tone])} />
        {/* A long list of plugins loading is cut short, shorter while the mutes pill shows; the title has it whole. */}
        <span className={clsx("truncate", pill ? "max-w-[100px]" : "max-w-60")}>{main.label}</span>
        {rest.length > 0 && <span className="opacity-70">+{rest.length}</span>}
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} align="end" role="dialog" aria-label="Status" data-testid="status-popover" className="w-[360px] max-h-[70vh] overflow-y-auto">
          <div className="divide-y divide-line">
            {all.map((s) => (
              <div key={s.id} className="py-1 first:pt-0 last:pb-0">
                {s.id === "offline" && (
                  <Item state={s} title="Lost the connection to music2">
                    Reconnecting… Editing is off until it is back.
                  </Item>
                )}
                {s.id === "compile" && (
                  <Compile
                    graph={graph}
                    state={s}
                    onShow={(p) => {
                      setAnchor(null);
                      onShowCode(p);
                    }}
                  />
                )}
                {s.id === "problems" && (
                  <div>
                    {/* One problem under it: its own mark says how bad, so the heading needs none. */}
                    <Item state={s} title={`${s.label} in the song`} icon={attention(graph).length > 1}>
                      {problemsLine(graph)}
                    </Item>
                    <Attention graph={graph} locked={lockReason(graph, connected)} />
                  </div>
                )}
                {s.id === "no-output" && (
                  <Item state={s} title="No sound output" action={<SettingsLink onOpen={() => setAnchor(null)}>Settings…</SettingsLink>}>
                    No sound device was found, so the song plays silently. Plug one in: within a few seconds the song moves to it, and stops.
                  </Item>
                )}
                {s.id === "dropout" && (
                  <Item state={s} title="Sound dropped out">
                    {`The sound dropped out ${dropouts.since === 1 ? "once" : `${dropouts.since} times`} since play started. Closing other apps or removing a plugin helps.`}
                  </Item>
                )}
                {s.id === "disk" && (
                  <Item state={s} title="Disk too slow">
                    {status?.disk}
                  </Item>
                )}
                {s.id === "meters" && (
                  <Item state={s} title="Meters before faders">
                    {status?.meters}
                  </Item>
                )}
                {s.id === "load" && (
                  <Item state={s} title="Heavy load" action={<SettingsLink onOpen={() => setAnchor(null)}>Audio settings…</SettingsLink>}>
                    {`At its busiest moment in the last second, the sound took ${Math.round((status?.load ?? 0) * 100)}% of the time the computer has for it. At 100% it drops out.`}
                    <span className="block mt-1 text-xs text-fg-faint">{`Sample rate ${(graph.sample_rate / 1000).toLocaleString("en", { maximumFractionDigits: 1 })} kHz`}</span>
                  </Item>
                )}
                {s.id === "read-only" && graph.read_only && <ReadOnlyItem graph={graph} state={s} readOnly={graph.read_only} />}
                {s.id === "paused" && (
                  <Item state={s} title="Paused for export">
                    The export is using the song's plugins. It plays on from here when the export is done.
                  </Item>
                )}
                {s.id === "loading" && (
                  <Item state={s} title={`Loading ${status?.loading?.length === 1 ? "a plugin" : "plugins"}`}>
                    {`${status?.loading?.join(", ")}. The song plays on meanwhile, and your change joins it once they're ready.`}
                  </Item>
                )}
              </div>
            ))}
          </div>
        </Popover>
      )}
    </>
  );
}

/** The sound output's settings, from a state about the sound (taste.md §3.3's DSP click). */
function SettingsLink({ onOpen, children }: { onOpen: () => void; children: ReactNode }) {
  return (
    <Button
      variant="secondary"
      size="sm"
      data-testid="status-settings"
      onClick={() => {
        onOpen();
        openSettings();
      }}
      className="shrink-0"
    >
      {children}
    </Button>
  );
}

/** What the problems need, in as many words as there are of them: a decision only where a fix is a choice. */
function problemsLine(graph: GraphState) {
  const items = attention(graph);
  const choices = items.filter((d) => d.fixes?.length).length;
  if (!choices) return "It still plays.";
  if (choices === items.length) return items.length === 1 ? "It plays; this needs a decision." : "It plays; these need a decision.";
  return choices === 1 ? "It plays; one needs a decision." : `It plays; ${choices} need a decision.`;
}

/** A state in the popover: its mark, its title, a line, and what to do under that (never beside the title, which would wrap it). */
function Item({ state, title, children, action, icon = true }: { state: State; title: string; children?: ReactNode; action?: ReactNode; icon?: boolean }) {
  return (
    <div className="px-2 py-2 flex items-start gap-2">
      {icon ? <state.Icon size={14} className={clsx("mt-0.5 shrink-0", ICON[state.tone])} /> : <span className="w-3.5 shrink-0" />}
      <div className="flex-1 min-w-0">
        <div className="text-base font-semibold text-fg">{title}</div>
        {children && <div className="text-sm text-fg-muted">{children}</div>}
        {action && <div className="mt-2 flex">{action}</div>}
      </div>
    </div>
  );
}

/** Why the files on disk don't compile, where (project-relative), and a way to the code. */
function Compile({ graph, state, onShow }: { graph: GraphState; state: State; onShow: (p: Problem) => void }) {
  // The diagnostics name files relative to the project; the raw errors are the fallback.
  const problems: Problem[] = graph.diagnostics.length
    ? graph.diagnostics
    : graph.view.draft_errors.map((message): Problem => ({ file: null, line: null, col: null, message, code: "code.build", severity: "error" }));
  // The codes of those not in plain words: each in plain words has its own Details.
  const codes = [...new Set(problems.filter((p) => !p.plain).map((p) => p.code).filter((c) => c !== "code.build"))];
  return (
    <div data-testid="compile-banner">
      <Item
        state={state}
        title="The code doesn't compile right now"
        action={
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              // At its line, marked: your own line, for a mistake made through std (the studio plan's Q46).
              const p = problems[0];
              onShow(p);
              if (p.file && p.line != null) useShell.getState().set({ codeAt: { file: p.file, line: p.line } });
            }}
            className="shrink-0"
          >
            <CodeXml size={14} />
            Show in code
          </Button>
        }
      >
        {graph.has_code === false
          ? "Nothing plays until it does: fix it in the code, and the song plays once it compiles."
          : "Playing the last version that did; editing waits until it compiles."}
      </Item>
      <ul className="px-2 pb-2 pl-8 space-y-1">
        {problems.slice(0, 3).map((p, i) =>
          // In plain words (the studio plan's D2): what went wrong, where in your code, and the
          // fix; the problem's own words, its code and the place inside a package under Details.
          p.plain ? (
            <li key={i} data-plain className="text-sm text-fg select-text break-words">
              <span className="font-semibold">
                <Words text={p.plain} />
              </span>
              {p.file && !p.plain.includes(p.file) && <span className="text-fg-muted"> {placeOf(p)}.</span>}
              {p.fix && (
                <>
                  {" "}
                  <Words text={p.fix} />
                </>
              )}
              <Details>
                <div data-code className="mt-1 font-mono select-text break-words">
                  {technical(p)}
                </div>
              </Details>
            </li>
          ) : (
            <li key={i} className="text-sm text-fg select-text break-words">
              {p.file && <span className="text-fg-muted">{placeOf(p)}: </span>}
              {p.message}
            </li>
          ),
        )}
        {problems.length > 3 && <li className="text-xs text-fg-muted">and {problems.length - 3} more, in the code view</li>}
        {problems.some((p) => p.code === "pkg.missing") && <MissingPacks />}
        {codes.length > 0 && (
          <li>
            <Details>
              <div data-code className="mt-1 font-mono select-text">{codes.join(" · ")}</div>
            </Details>
          </li>
        )}
      </ul>
    </div>
  );
}

/** Who holds the project, in the words a person uses: another window of this app, m2, or the program. */
function holderName(readOnly: ReadOnly) {
  const program = readOnly.holder?.program;
  if (!program || program === "music2") return "another window";
  return program === "m2" ? "m2" : `“${program}”`;
}

/** The project is open for writing in another program (WRITE-17): this window plays it and shows
 * what changes on disk; editing waits until the lock is free and Retry takes it. */
function ReadOnlyItem({ graph, state, readOnly }: { graph: GraphState; state: State; readOnly: ReadOnly }) {
  const toast = useStore((s) => s.toast);
  const [trying, setTrying] = useState(false);
  const retry = async () => {
    setTrying(true);
    try {
      const now = await graphApi.retryLock(graph.session);
      if (now.read_only) toast(`Still read only: it is still open in ${now.read_only.holder ? holderName(now.read_only) : "another program"}.`);
    } catch (e) {
      toast(e instanceof Error ? e.message : String(e));
    } finally {
      setTrying(false);
    }
  };
  return (
    <Item
      state={state}
      title="Read only"
      action={
        <Button variant="secondary" size="sm" onClick={retry} disabled={trying} data-testid="read-only-retry" className="shrink-0">
          Retry
        </Button>
      }
    >
      {readOnly.why === "held"
        ? `Open in ${holderName(readOnly)}. You can listen here; to edit, close it there, then Retry.`
        : readOnly.message}
    </Item>
  );
}

/**
 * The packs the song's `music2.lock` names that aren't installed (REG-17), each with Get when the
 * library has that very version, its progress while it comes and Cancel; or why it can't be got.
 * Once got, the song builds again and plays.
 */
function MissingPacks() {
  const [missing, setMissing] = useState<MissingPack[] | null>(null);
  const [said, setSaid] = useState<string | null>(null);
  const getting = useGetting();
  const toast = useStore((s) => s.toast);
  // Asked again every 2 s while it shows: the library's catalog may still be coming as the app
  // starts, and `m2 pack get` may get one meanwhile.
  useEffect(() => {
    let alive = true;
    const ask = () =>
      void missingPacks().then(
        (m) => alive && setMissing(m),
        () => alive && setMissing((was) => was ?? []),
      );
    ask();
    const t = window.setInterval(ask, 2000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);
  if (!missing?.length) return null;
  const get = (m: MissingPack) => {
    setSaid(null);
    void getMissing(m.pack, m.version).then(
      () => {
        toast(`Got ${m.pack} ${m.version}: the song builds with it again`);
        setMissing((all) => all?.filter((x) => x !== m) ?? null);
      },
      (e) => setSaid(e instanceof Error ? e.message : String(e)),
    );
  };
  return (
    <li className="pt-1 space-y-1" data-missing-packs>
      {missing.map((m) => {
        const id = `${m.pack}@${m.version}`;
        const mine = getting?.id === id;
        return (
          <div key={id} className="flex items-center gap-2 text-sm" data-missing-pack={id}>
            <span className="flex-1 min-w-0 truncate text-fg">
              {m.pack} {m.version}
              {!m.can && m.why && <span className="block text-xs text-fg-muted whitespace-normal">{m.why}</span>}
            </span>
            {m.can && mine && getting.installing && <span className="text-xs text-fg-muted">Installing…</span>}
            {m.can && mine && !getting.installing && (
              <>
                <span className="text-xs text-fg-muted tabular-nums">{getting.total ? `${Math.round((getting.done / getting.total) * 100)}%` : "…"}</span>
                <Button variant="secondary" size="sm" onClick={() => void cancelGet().then((why) => why && setSaid(why))} data-missing-cancel>
                  Cancel
                </Button>
              </>
            )}
            {m.can && !mine && (
              <Button variant="secondary" size="sm" disabled={!!getting} onClick={() => get(m)} data-missing-get={id} title={`Get it from the library${m.size ? ` (${sizeWords(m.size)})` : ""}`}>
                <Download size={14} />
                Get
              </Button>
            )}
          </div>
        );
      })}
      {said && <p className="text-xs text-danger-strong">{said}</p>}
    </li>
  );
}

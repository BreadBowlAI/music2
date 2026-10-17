import clsx from "clsx";
import { CircleAlert, CodeXml, Hourglass, Info, Lock, Pause, TriangleAlert, WifiOff } from "lucide-react";
import { useState, type ReactNode } from "react";
import { attention, graphApi, lockReason, type GraphState, type GraphStatus, type Problem, type ReadOnly } from "../lib/graph";
import { useStore } from "../lib/store";
import Attention from "./Attention";
import { Button, Details, keepFocus, Popover } from "./ui";

type Tone = "danger" | "warn" | "neutral";
interface State { id: "offline" | "compile" | "problems" | "read-only" | "paused" | "loading"; tone: Tone; label: string; Icon: typeof Lock }

// Hover darkens every tone, as it does a ghost button: the soft fill mixed toward its strong colour.
const TONE: Record<Tone, string> = {
  danger: "bg-danger-soft text-danger-strong hover:bg-[color-mix(in_oklab,var(--color-danger-soft)_88%,var(--color-danger-strong))]",
  warn: "bg-warn-soft text-warn-strong hover:bg-[color-mix(in_oklab,var(--color-warn-soft)_88%,var(--color-warn-strong))]",
  neutral: "bg-hover text-fg hover:bg-press",
};
const ICON: Record<Tone, string> = { danger: "text-danger", warn: "text-warn", neutral: "text-fg-muted" };

/** The plugins loading, as the pill says it: "Loading Surge XT…". */
const loadingLabel = (names: string[]) => `Loading ${names.join(", ")}…`;

/** What the pill can say, the most severe first (taste.md ST-1). What the engine is busy with
 * (RENDER-1, RENDER-25) comes last: it is no problem, so any problem shows over it. */
function states(graph: GraphState, status: GraphStatus | undefined, connected: boolean): State[] {
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
  if (graph.read_only) out.push({ id: "read-only", tone: "neutral", label: "Read only", Icon: Lock });
  if (status?.paused === "exporting") out.push({ id: "paused", tone: "neutral", label: "Paused for export", Icon: Pause });
  if (status?.loading?.length) out.push({ id: "loading", tone: "neutral", label: loadingLabel(status.loading), Icon: Hourglass });
  return out;
}

/**
 * The one place for status (taste.md §3.3, ST-1): a pill in the top bar, there only while something
 * needs saying, taking no room in the layout. It shows the most severe state and counts the rest;
 * a click opens a popover with each of them and what fixes it. Nothing else repeats them.
 */
export default function Status({ graph, status, onShowCode }: { graph: GraphState; status?: GraphStatus; onShowCode: (p: Problem) => void }) {
  const connected = useStore((s) => s.connected);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const all = states(graph, status, connected);
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
        {/* A long list of plugins loading is cut short; the title has it whole. */}
        <span className="max-w-60 truncate">{main.label}</span>
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
  const codes = [...new Set(problems.map((p) => p.code).filter((c) => c !== "code.build"))];
  return (
    <div data-testid="compile-banner">
      <Item
        state={state}
        title="The code doesn't compile right now"
        action={
          <Button variant="secondary" size="sm" onClick={() => onShow(problems[0])} className="shrink-0">
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
        {problems.slice(0, 3).map((p, i) => (
          <li key={i} className="text-sm text-fg select-text break-words">
            {p.file && (
              <span data-code className="font-mono text-xs text-fg-muted">
                {p.file}
                {p.line != null && `:${p.line}${p.col != null ? `:${p.col}` : ""}`}{" "}
              </span>
            )}
            {p.message}
          </li>
        ))}
        {problems.length > 3 && <li className="text-xs text-fg-muted">and {problems.length - 3} more, in the code view</li>}
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

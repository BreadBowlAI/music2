import clsx from "clsx";
import { CircleAlert, Info, MoveRight, Trash2, TriangleAlert, Wand2 } from "lucide-react";
import { useState } from "react";
import { attention, graphApi, type Diagnostic, type Fix, type GraphState, type Severity } from "../lib/graph";
import { useStore } from "../lib/store";
import { graphUndo } from "./GraphCanvas";

/** How many problems show before "Show all". */
const SHOWN = 2;

const LOOK: Record<Severity, { Icon: typeof CircleAlert; color: string; word: string }> = {
  error: { Icon: CircleAlert, color: "text-red-600", word: "Error" },
  warning: { Icon: TriangleAlert, color: "text-amber-600", word: "Warning" },
  notice: { Icon: Info, color: "text-ink-400", word: "Notice" },
};

/** What a fix renames, deletes or rewrites: the first file it touches, as its edit says. */
function target(fix: Fix) {
  const op = fix.edit.ops[0];
  return !op ? null : op.op === "rename" ? { from: op.from, to: op.to } : { from: op.file, to: null };
}

/** What the toast says once a fix is made. */
function done(fix: Fix, problem: Diagnostic) {
  const t = target(fix);
  if (fix.code === "relink" && t?.to) return `Moved ${t.from} to ${t.to}.`;
  if (fix.code === "delete-data") return `Deleted ${problem.src ?? t?.from ?? "it"}.`;
  return `Fixed ${t?.from ?? problem.src ?? "it"}: ${fix.title.charAt(0).toLowerCase()}${fix.title.slice(1)}.`;
}

/**
 * What needs attention in the song though it plays (DIAG-4): data no lane reads since a track's or
 * a lane's key was renamed in code, a lane file that doesn't read, a missing asset, clips without
 * ids. Each says what is wrong in plain words, the worst first, with its fixes a click away: the
 * one it plainly calls for, then the others. Nothing is fixed until asked (DIAG-5), and undo takes
 * a fix back.
 */
export default function Attention({ graph }: { graph: GraphState }) {
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const toast = useStore((s) => s.toast);
  const items = attention(graph);
  if (!items.length) return null;
  // Fixes are edits: like knobs, they wait for code that compiles.
  const locked = graph.view.draft_errors.length > 0;
  const session = graph.session;

  const apply = (problem: Diagnostic, fix: Fix) => {
    setBusy(true);
    graphApi
      .fix(session, graph.view.revision, fix)
      .then(({ revision }) =>
        toast(done(fix, problem), {
          label: "Undo",
          run: () => {
            // Only while the fix is still the newest change: later, Undo would take back something else.
            const now = useStore.getState().graph;
            if (now?.session === session && now.view.revision <= revision) void graphUndo(session, "undo");
            else toast("The song has changed since, so this can't be undone from here. Use Undo in the top bar.");
          },
        }),
      )
      .catch((error) => toast(error instanceof Error ? error.message : String(error)))
      .finally(() => setBusy(false));
  };

  // Tinted by the worst of it: the strip is at its loudest only when something is silent.
  const worst = items[0].severity;
  const shown = all ? items : items.slice(0, SHOWN);
  return (
    <section
      data-testid="attention"
      aria-label="Needs attention"
      className={clsx(
        "shrink-0 max-h-[30vh] overflow-y-auto px-5 py-2 border-b text-[13px] text-ink-100",
        worst === "error" ? "bg-red-500/[0.05] border-red-600/15" : worst === "warning" ? "bg-pop-500/[0.07] border-pop-500/20" : "bg-ink-900 border-black/[0.06]",
      )}
    >
      <div className={clsx("flex items-center gap-2 text-xs font-medium", worst === "error" ? "text-red-700" : worst === "warning" ? "text-pop-600" : "text-ink-400")}>
        <CircleAlert className="size-3.5" />
        Needs attention{items.length > 1 && ` · ${items.length}`}
      </div>
      <ul className="mt-1 divide-y divide-black/[0.06]">
        {shown.map((d, i) => (
          <Row key={`${d.code}:${d.src ?? ""}:${i}`} problem={d} disabled={busy || locked} locked={locked} onFix={(f) => apply(d, f)} />
        ))}
      </ul>
      {items.length > SHOWN && (
        <button onClick={() => setAll(!all)} className="mt-0.5 text-xs text-pop-600 hover:text-pop-500 cursor-pointer">
          {all ? "Show fewer" : `Show ${items.length - SHOWN} more`}
        </button>
      )}
    </section>
  );
}

/** A fix's mark: a move, a delete, or a file rewritten. */
function FixIcon({ fix }: { fix: Fix }) {
  const I = fix.code === "relink" ? MoveRight : fix.code === "delete-data" ? Trash2 : Wand2;
  return <I className="size-3.5 shrink-0" />;
}

function Row({ problem, disabled, locked, onFix }: { problem: Diagnostic; disabled: boolean; locked: boolean; onFix: (f: Fix) => void }) {
  const fixes = problem.fixes ?? [];
  // The fix it plainly calls for is one click; other places the data could go are a pick; the
  // rest (delete, or another rewrite) are buttons of their own.
  const likely = fixes.find((f) => f.likely);
  const moves = fixes.filter((f) => f !== likely && f.code === "relink");
  const others = fixes.filter((f) => f !== likely && f.code !== "relink");
  const why = locked ? "Fix the code first: it doesn't compile right now" : undefined;
  const { Icon, color, word } = LOOK[problem.severity] ?? LOOK.warning;
  const src = problem.src && !problem.message.includes(problem.src) ? problem.src : null;
  const button =
    "h-7 px-2.5 shrink-0 rounded-lg text-xs font-medium inline-flex items-center gap-1.5 cursor-pointer transition-colors disabled:opacity-40 disabled:cursor-default";
  return (
    <li data-problem={problem.code} data-severity={problem.severity} data-src={problem.src ?? undefined} className="py-1.5 flex items-start gap-3">
      <Icon className={clsx("size-3.5 mt-[3px] shrink-0", color)} aria-label={word} />
      <div className="flex-1 min-w-0">
        <p data-testid="problem-message" className="select-text leading-snug break-words">
          {problem.message}
        </p>
        <p className="flex items-baseline gap-2 min-w-0 text-[11px] text-ink-400">
          {src && <span className="font-mono truncate">{src}</span>}
          <span className="font-mono shrink-0 text-ink-500" title="The problem's code">
            {problem.code}
          </span>
        </p>
      </div>
      <div className="shrink-0 flex items-center gap-1.5">
        {likely && (
          <button
            data-fix={likely.code}
            data-to={target(likely)?.to ?? undefined}
            data-likely
            disabled={disabled}
            title={why ?? likely.title}
            onClick={() => onFix(likely)}
            className={`${button} max-w-72 pop-gradient text-white enabled:hover:brightness-110`}
          >
            <FixIcon fix={likely} />
            <span className="truncate">{likely.title}</span>
          </button>
        )}
        {moves.length > 0 && (
          // No place plainly its own, or others that could take it: the person picks.
          <select
            data-fix="relink-pick"
            aria-label={likely ? "Move it somewhere else" : "Move it"}
            disabled={disabled}
            title={why}
            value=""
            onChange={(e) => {
              const f = moves[Number(e.target.value)];
              if (f) onFix(f);
            }}
            className={`${button} w-32 pl-2 pr-1 bg-ink-800 ring-1 ring-black/10 text-ink-100`}
          >
            <option value="" disabled>
              {likely ? "Elsewhere…" : "Move to…"}
            </option>
            {moves.map((f, i) => (
              <option key={`${f.title}:${i}`} value={i} data-to={target(f)?.to ?? undefined}>
                {f.title}
              </option>
            ))}
          </select>
        )}
        {others.map((f, i) => (
          <button
            key={`${f.code}:${i}`}
            data-fix={f.code}
            disabled={disabled}
            title={why ?? (f.code === "delete-data" ? `${f.title} (undo puts it back)` : f.title)}
            onClick={() => onFix(f)}
            className={`${button} max-w-56 text-ink-300 enabled:hover:text-ink-100 enabled:hover:bg-black/[0.05]`}
          >
            <FixIcon fix={f} />
            <span className="truncate">{f.code === "delete-data" ? "Delete" : f.title}</span>
          </button>
        ))}
      </div>
    </li>
  );
}

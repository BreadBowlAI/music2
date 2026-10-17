import clsx from "clsx";
import { ChevronDown, CircleAlert, Info, MoveRight, Trash2, TriangleAlert, Wand2 } from "lucide-react";
import { useState } from "react";
import { attention, graphApi, type Diagnostic, type Fix, type GraphState, type Severity } from "../lib/graph";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { Words } from "../lib/plain";
import { Details, MenuRow, Popover } from "./ui";

export const LOOK: Record<Severity, { Icon: typeof CircleAlert; color: string; word: string }> = {
  error: { Icon: CircleAlert, color: "text-danger", word: "Error" },
  warning: { Icon: TriangleAlert, color: "text-warn", word: "Warning" },
  notice: { Icon: Info, color: "text-fg-faint", word: "Notice" },
};

/** What a fix renames, deletes or rewrites: the first file it touches, as its Tx says. */
function target(fix: Fix) {
  const op = fix.tx.ops[0];
  if (!op) return null;
  if (op.op === "rename") return { from: op.from, to: op.to };
  if (op.op === "clips") return { from: `data/${op.lane[0]}/${op.lane[1]}.json`, to: null };
  return { from: op.file, to: null };
}

/** What the toast says once a fix is made. */
function done(fix: Fix, problem: Diagnostic) {
  const t = target(fix);
  if (fix.code === "relink" && t?.to) return `Moved ${t.from} to ${t.to}.`;
  if (fix.code === "delete-data") return `Deleted ${problem.src ?? t?.from ?? "it"}.`;
  return `Fixed ${t?.from ?? problem.src ?? "it"}: ${fix.title.charAt(0).toLowerCase()}${fix.title.slice(1)}.`;
}

/**
 * What needs attention in the song though it plays (DIAG-4), in the status popover: data no lane
 * reads since a track's or a lane's key was renamed in code, a lane file that doesn't read, a
 * missing asset, clips without ids. Each says what is wrong in plain words, the worst first, with
 * its fixes a click away: the one it plainly calls for, then the others. Its code is behind
 * Details. Nothing is fixed until asked (DIAG-5), and undo takes a fix back.
 */
export default function Attention({ graph, locked }: { graph: GraphState; /** Why fixes can't be made now, if they can't. */ locked: string | null }) {
  const [busy, setBusy] = useState(false);
  const toast = useStore((s) => s.toast);
  const items = attention(graph);
  if (!items.length) return null;
  const session = graph.session;

  const apply = (problem: Diagnostic, fix: Fix) => {
    setBusy(true);
    graphApi
      .fix(session, graph.view.revision, fix)
      .then(({ revision }) => toast(done(fix, problem), undoAction(session, revision)))
      .catch((error) => toast(error instanceof Error ? error.message : String(error)))
      .finally(() => setBusy(false));
  };

  return (
    <section data-testid="attention" aria-label="Needs attention" className="text-base text-fg">
      <ul className="divide-y divide-line">
        {items.map((d, i) => (
          <Row key={`${d.code}:${d.src ?? ""}:${i}`} problem={d} disabled={busy || !!locked} why={locked} onFix={(f) => apply(d, f)} />
        ))}
      </ul>
    </section>
  );
}

/** A fix's mark: a move, a delete, or a file rewritten. */
function FixIcon({ fix }: { fix: Fix }) {
  const I = fix.code === "relink" ? MoveRight : fix.code === "delete-data" ? Trash2 : Wand2;
  return <I size={14} className="shrink-0" />;
}

const BUTTON = "h-ctl-sm px-2 shrink-0 rounded-sm text-sm font-medium inline-flex items-center gap-1.5 cursor-pointer transition-colors duration-80 ease-linear disabled:cursor-default";

function Row({ problem, disabled, why, onFix }: { problem: Diagnostic; disabled: boolean; why: string | null; onFix: (f: Fix) => void }) {
  const fixes = problem.fixes ?? [];
  // The fix it plainly calls for is one click; other places the data could go are a menu; the
  // rest (delete, or another rewrite) are buttons of their own.
  const likely = fixes.find((f) => f.likely);
  const moves = fixes.filter((f) => f !== likely && f.code === "relink");
  const others = fixes.filter((f) => f !== likely && f.code !== "relink");
  const { Icon, color, word } = LOOK[problem.severity] ?? LOOK.warning;
  const src = problem.src && !problem.message.includes(problem.src) ? problem.src : null;
  const [menu, setMenu] = useState<HTMLElement | null>(null);
  return (
    <li data-problem={problem.code} data-severity={problem.severity} data-src={problem.src ?? undefined} className="px-2 py-2 flex items-start gap-2">
      <Icon size={14} className={clsx("mt-0.5 shrink-0", color)} aria-label={word} />
      <div className="flex-1 min-w-0 space-y-1.5">
        {/* In plain words, with a fix, where a rule knows it (the studio plan's D2); its own words under Details. */}
        <p data-testid="problem-message" data-plain={problem.plain ? "" : undefined} className="select-text break-words">
          {problem.plain ? <Words text={problem.plain} /> : problem.message}
        </p>
        {problem.fix && (
          <p data-testid="problem-fix" className="text-sm text-fg-muted select-text break-words">
            <Words text={problem.fix} />
          </p>
        )}
        {fixes.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            {/* In one row: the plain fix, the other places, and Delete as just its icon (named in its tooltip). */}
            {likely && (
              <button
                data-fix={likely.code}
                data-to={target(likely)?.to ?? undefined}
                data-likely
                disabled={disabled}
                title={why ?? likely.title}
                onClick={() => onFix(likely)}
                className={`${BUTTON} max-w-full bg-accent text-white enabled:hover:bg-accent-hover enabled:active:bg-accent-strong disabled:bg-press disabled:text-fg-off`}
              >
                <FixIcon fix={likely} />
                <span className="truncate">{likely.title}</span>
              </button>
            )}
            {moves.length > 0 && (
              // No place plainly its own, or others that could take it: the person picks, from a menu.
              <button
                data-fix="relink-pick"
                aria-haspopup="menu"
                aria-expanded={!!menu}
                disabled={disabled}
                title={why ?? undefined}
                onClick={(e) => {
                  const b = e.currentTarget;
                  setMenu((m) => (m ? null : b));
                }}
                className={`${BUTTON} bg-raised border border-line-strong text-fg enabled:hover:border-fg-off disabled:bg-chrome disabled:text-fg-off`}
              >
                {likely ? "Elsewhere…" : "Move to…"}
                <ChevronDown size={12} className="text-fg-muted" />
              </button>
            )}
            {others.map((f, i) => (
              <button
                key={`${f.code}:${i}`}
                data-fix={f.code}
                disabled={disabled}
                title={why ?? (f.code === "delete-data" ? `${f.title} (undo puts it back)` : f.title)}
                aria-label={f.code === "delete-data" ? "Delete" : undefined}
                onClick={() => onFix(f)}
                className={clsx(BUTTON, "max-w-56 text-fg-muted enabled:hover:text-fg enabled:hover:bg-hover disabled:text-fg-off", f.code === "delete-data" && "w-ctl-sm !px-0 justify-center")}
              >
                <FixIcon fix={f} />
                {f.code !== "delete-data" && <span className="truncate">{f.title}</span>}
              </button>
            ))}
          </div>
        )}
        {/* The problem's code and its file: literal, for agents and power users. */}
        <Details>
          <div className="mt-1 space-y-0.5 select-text">
            {problem.plain && <div className="break-words">{problem.message}</div>}
            <div data-code className="font-mono" title="The problem's code">{problem.code}</div>
            {src && <div data-code className="font-mono break-all">{src}</div>}
          </div>
        </Details>
      </div>
      {menu && (
        <Popover anchor={menu} onClose={() => setMenu(null)} role="menu" data-relink-menu className="w-[280px]">
          {moves.map((f, i) => (
            <MenuRow
              key={`${f.title}:${i}`}
              data-to={target(f)?.to ?? undefined}
              icon={<MoveRight size={14} />}
              onClick={() => {
                setMenu(null);
                onFix(f);
              }}
            >
              {f.title}
            </MenuRow>
          ))}
        </Popover>
      )}
    </li>
  );
}

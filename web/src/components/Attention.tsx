import { CircleAlert, MoveRight, Trash2 } from "lucide-react";
import { useState } from "react";
import { graphApi, type Fix, type GraphState, type Warning } from "../lib/graph";
import { useStore } from "../lib/store";
import { graphUndo } from "./GraphCanvas";

type Relink = Extract<Fix, { fix: "relink" }>;

/** How many warnings show before "Show all". */
const SHOWN = 2;

/**
 * What needs attention in the song though it plays, such as notes no lane reads since a track or
 * lane was renamed in code: each in plain words, with its fixes a click away. Nothing is fixed
 * until asked, and undo takes a fix back.
 */
export default function Attention({ graph }: { graph: GraphState }) {
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const toast = useStore((s) => s.toast);
  const warnings = graph.warnings;
  if (!warnings.length) return null;
  // Fixes are edits: like knobs, they wait for code that compiles.
  const locked = graph.view.draft_errors.length > 0;
  const session = graph.session;

  const run = (edit: Promise<{ revision: number }>, done: string) => {
    setBusy(true);
    edit
      .then(({ revision }) =>
        toast(done, {
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
  const relink = (f: Relink) =>
    run(graphApi.relink(session, graph.view.revision, f.from, f.module, f.lane), `Moved ${f.from} to ${f.module} › ${f.lane}.`);
  const remove = (file: string) => run(graphApi.deleteData(session, graph.view.revision, file), `Deleted ${file}.`);

  const shown = all ? warnings : warnings.slice(0, SHOWN);
  return (
    <section
      data-testid="attention"
      aria-label="Needs attention"
      className="shrink-0 max-h-[30vh] overflow-y-auto px-5 py-2 bg-pop-500/[0.07] border-b border-pop-500/20 text-[13px] text-ink-100"
    >
      <div className="flex items-center gap-2 text-xs font-medium text-pop-600">
        <CircleAlert className="size-3.5" />
        Needs attention{warnings.length > 1 && ` · ${warnings.length}`}
      </div>
      <ul className="mt-1 divide-y divide-pop-500/10">
        {shown.map((w) => (
          <Row key={w.file} warning={w} disabled={busy || locked} locked={locked} onRelink={relink} onDelete={remove} />
        ))}
      </ul>
      {warnings.length > SHOWN && (
        <button onClick={() => setAll(!all)} className="mt-0.5 text-xs text-pop-600 hover:text-pop-500 cursor-pointer">
          {all ? "Show fewer" : `Show ${warnings.length - SHOWN} more`}
        </button>
      )}
    </section>
  );
}

function Row({
  warning,
  disabled,
  locked,
  onRelink,
  onDelete,
}: {
  warning: Warning;
  disabled: boolean;
  locked: boolean;
  onRelink: (f: Relink) => void;
  onDelete: (file: string) => void;
}) {
  const relinks = warning.fixes.filter((f): f is Relink => f.fix === "relink");
  // The lane it plainly belongs to is one click; any other lane that could take it is a pick.
  const likely = relinks.find((f) => f.likely);
  const others = relinks.filter((f) => f !== likely);
  const deletes = warning.fixes.filter((f) => f.fix === "delete_data");
  const why = locked ? "Fix the code first: it doesn't compile right now" : undefined;
  const button =
    "h-7 px-2.5 shrink-0 rounded-lg text-xs font-medium inline-flex items-center gap-1.5 cursor-pointer transition-colors disabled:opacity-40 disabled:cursor-default";
  return (
    <li data-warning={warning.file} data-code={warning.code} className="py-1.5 flex items-start gap-3">
      <div className="flex-1 min-w-0">
        <p data-testid="warning-message" className="select-text leading-snug">
          <span className="font-medium">{warning.title}.</span> {warning.message}
        </p>
        <p className="font-mono text-[11px] text-ink-400 truncate">{warning.file}</p>
      </div>
      <div className="shrink-0 flex items-center gap-1.5">
        {likely && (
          <button
            data-fix="relink"
            data-to={`${likely.module} › ${likely.lane}`}
            disabled={disabled}
            title={why ?? `Rename ${likely.from} to ${likely.to}`}
            onClick={() => onRelink(likely)}
            className={`${button} max-w-72 pop-gradient text-white enabled:hover:brightness-110`}
          >
            <MoveRight className="size-3.5 shrink-0" />
            <span className="truncate">
              Move to {likely.module} › {likely.lane}
            </span>
          </button>
        )}
        {others.length > 0 && (
          // No lane plainly its own, or others that could take it: the person picks.
          <select
            data-fix="relink-pick"
            aria-label={likely ? "Move to another lane" : "Move to a lane"}
            disabled={disabled}
            title={why}
            value=""
            onChange={(e) => {
              const f = others[Number(e.target.value)];
              if (f) onRelink(f);
            }}
            className={`${button} w-30 pl-2 pr-1 bg-ink-800 ring-1 ring-black/10 text-ink-100`}
          >
            <option value="" disabled>
              {likely ? "Other lane…" : "Move to…"}
            </option>
            {others.map((f, i) => (
              <option key={f.to} value={i}>
                {f.module} › {f.lane}
              </option>
            ))}
          </select>
        )}
        {deletes.length > 0 && (
          <button
            data-fix="delete"
            disabled={disabled}
            title={why ?? `Delete ${warning.file} (undo puts it back)`}
            onClick={() => onDelete(warning.file)}
            className={`${button} text-ink-300 enabled:hover:text-ink-100 enabled:hover:bg-black/[0.05]`}
          >
            <Trash2 className="size-3.5" />
            Delete
          </button>
        )}
      </div>
    </li>
  );
}

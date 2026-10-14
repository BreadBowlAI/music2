import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { graphApi, moduleName } from "../lib/graph";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";

/**
 * A track's name, renamed where it is shown (the timeline's header, the canvas's title): double-click
 * it, type, and Enter (or click away) writes it as the track's `name` in its code; Escape keeps the
 * old one. Only the label changes (ID-10): the key, its data files and the sound stay, and Undo takes
 * it back. The song itself has no name to change.
 */
export default function TrackName({
  module,
  label,
  track,
  className,
  testid,
}: {
  /** The module's key, which the rename names it by. */
  module: string;
  label: string;
  track: boolean;
  className: string;
  testid?: string;
}) {
  const name = moduleName({ key: module, label });
  const session = useStore((s) => s.graph?.session);
  const broken = useStore((s) => (s.graph?.view.draft_errors.length ?? 0) > 0);
  const toast = useStore((s) => s.toast);
  const [draft, setDraft] = useState<string | null>(null);
  // The name being written, shown until the graph says it (no flicker back to the old one).
  const [pending, setPending] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const renamable = track && module !== "song" && session !== undefined;

  useEffect(() => {
    if (pending !== null && name === pending) setPending(null);
  }, [name, pending]);
  // Selected when editing starts, so typing replaces it; never again while typing.
  const editing = draft !== null;
  useEffect(() => {
    if (editing) input.current?.select();
  }, [editing]);

  const commit = (value: string) => {
    setDraft(null);
    const next = value.trim();
    if (!next || next === name || session === undefined) return;
    setPending(next);
    graphApi
      .label(session, module, next)
      .then((r) => {
        if (r.before === r.after) setPending(null);
        else toast(`Renamed ${r.before} to ${r.after}`, undoAction(session, r.revision));
      })
      .catch((error) => {
        setPending(null);
        toast(error instanceof Error ? error.message : String(error));
      });
  };

  if (draft !== null) {
    return (
      <input
        ref={input}
        data-testid="track-rename"
        aria-label={`Rename ${name}`}
        value={draft}
        autoFocus
        spellCheck={false}
        size={Math.max(6, draft.length + 1)}
        maxLength={80}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") commit(draft);
          else if (e.key === "Escape") setDraft(null);
        }}
        onBlur={() => commit(draft)}
        onPointerDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        className={clsx(className, "min-w-0 max-w-full -mx-1 px-1 rounded-md bg-ink-800 outline-none ring-1 ring-pop-500/60 shadow-[0_0_0_3px] shadow-pop-500/15")}
      />
    );
  }
  const shown = pending ?? name;
  const why = !renamable ? undefined : broken ? "Fix the code first to rename it: it doesn't compile right now" : "Double-click to rename";
  return (
    <span
      data-testid={testid}
      data-renaming={pending !== null || undefined}
      className={clsx(className, "truncate", renamable && !broken && "cursor-text", pending !== null && "opacity-70")}
      title={[shown === module ? shown : `${shown} (${module})`, why].filter(Boolean).join("\n")}
      onDoubleClick={(e) => {
        if (!renamable) return;
        e.stopPropagation();
        if (broken) toast("Fix the code first to rename it: it doesn't compile right now.");
        else setDraft(shown);
      }}
    >
      {shown}
    </span>
  );
}

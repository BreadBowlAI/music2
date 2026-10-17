import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { graphApi, lockReason, moduleName } from "../lib/graph";
import { kbd } from "../lib/native";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";

/**
 * A track's name, renamed where it is shown (the timeline's header, the dock's chip): double-click
 * it, or ⌘R on the selected track, type, and Enter (or click away) writes it as the track's `name`
 * in its code; Escape keeps the old one. Only the label changes (ID-10): the key, its data files
 * and the sound stay, and Undo takes it back. The name on screen says it is done, so no toast
 * does (taste.md ST-5). The song itself has no name to change.
 */
export default function TrackName({
  module,
  label,
  track,
  className,
  testid,
  listen,
}: {
  /** The module's key, which the rename names it by. */
  module: string;
  label: string;
  track: boolean;
  className: string;
  testid?: string;
  /** This is the name ⌘R renames (the timeline header's). */
  listen?: boolean;
}) {
  const name = moduleName({ key: module, label });
  const session = useStore((s) => s.graph?.session);
  const locked = useStore((s) => (s.graph ? lockReason(s.graph, s.connected) : null));
  const toast = useStore((s) => s.toast);
  const asked = useShell((s) => (listen && s.renaming === module ? s.renaming : null));
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
  // ⌘R: the selected track's name turns into a field.
  useEffect(() => {
    if (!asked) return;
    useShell.getState().set({ renaming: null });
    if (renamable && !locked) setDraft(pending ?? name);
  }, [asked]);

  const commit = (value: string) => {
    setDraft(null);
    const next = value.trim();
    if (!next || next === name || session === undefined) return;
    setPending(next);
    graphApi
      .label(session, module, next, { name: `renaming ${name}` })
      .then((r) => r.before === r.after && setPending(null))
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
        className={clsx(className, "w-full min-w-0 -mx-1 px-1 rounded-sm bg-raised border border-accent outline-none")}
      />
    );
  }
  const shown = pending ?? name;
  const why = !renamable ? undefined : locked ? `${locked}, so it can't be renamed` : `Double-click to rename (${kbd("R")})`;
  return (
    <span
      data-testid={testid}
      data-renaming={pending !== null || undefined}
      className={clsx(className, "truncate", renamable && !locked && "cursor-text", pending !== null && "opacity-70")}
      title={[shown, why].filter(Boolean).join("\n")}
      onDoubleClick={(e) => {
        if (!renamable || locked) return;
        e.stopPropagation();
        setDraft(shown);
      }}
    >
      {shown}
    </span>
  );
}

import clsx from "clsx";
import { Ellipsis, Play, Star, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useLooked } from "../lib/focus";
import { findNoteClip, lockReason, type ClipRef, type GraphState } from "../lib/graph";
import { usePiano } from "../lib/piano";
import { closePlayground, duplicateVersion, flip, keepVersion, playVersion, removeVersion, renameVersion, starVersion, usePlayground, versionsOf } from "../lib/playground";
import { useStore } from "../lib/store";
import { Button, keepFocus, MenuRow, Popover, popoverOpen } from "./ui";

/** A place that takes typing: its keys are its own. */
const typing = (t: EventTarget | null) =>
  t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || (t instanceof HTMLElement && (t.isContentEditable || !!t.closest(".monaco-editor")));

/**
 * The Playground (VER-6): one notes clip's versions, in a narrow list at the left of the roll, so
 * the notes stay in sight while versions are tried. A click on a row, or ← and →, plays that
 * version; S or a row's star stars it; Duplicate copies the one that plays and plays the copy;
 * Keep this one removes the others and closes it. Esc or its close button closes it, and the loop
 * goes back as it was. Each act is one undo step. The list is the song's: versions an agent adds
 * show as they land, and nothing here takes the keyboard's focus when they do.
 */
export default function Playground({ graph, clip: ref }: { graph: GraphState; clip: ClipRef }) {
  const clip = findNoteClip(graph.timeline, ref);
  const locked = useStore((s) => lockReason(graph, s.connected));
  const [renaming, setRenaming] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ name: string; anchor: HTMLElement } | null>(null);

  // Its keys, before the page's (S is Solo there, Esc deselects) and the views' (← → move clips).
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || !usePlayground.getState().clip) return;
      // Typing, a menu and a dialog keep their keys.
      if (typing(e.target) || typing(document.activeElement) || popoverOpen() || document.querySelector("[role=dialog], [aria-modal=true]")) return;
      // The roll's selected notes are what its arrows move and its Esc lets go of.
      const notes = !!useLooked.getState().notes?.notes.length && !!document.activeElement?.closest("[data-testid=notes]");
      const k = e.key;
      if (k === "ArrowLeft" || k === "ArrowRight") {
        if (notes) return;
        flip(k === "ArrowLeft" ? -1 : 1);
      } else if (k === "Escape") {
        if (notes) return;
        closePlayground();
      } else if (k.toLowerCase() === "s") {
        // With the keyboard piano on, S is a note.
        if (usePiano.getState().on || e.repeat) return;
        starVersion();
      } else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, []);

  if (!clip) return null;
  const versions = versionsOf(clip);
  const some = !!clip.versions?.length;
  const off = locked ? `${locked}` : undefined;
  return (
    <aside data-testid="playground" aria-label="Playground" className="w-56 shrink-0 min-h-0 flex flex-col bg-chrome border-r border-line">
      <div className="h-ctl shrink-0 flex items-center pl-3 pr-1 border-b border-line">
        <h2 className="flex-1 min-w-0 truncate text-sm font-semibold text-fg">Playground</h2>
        <Button size="sm" data-testid="playground-close" onMouseDown={keepFocus} onClick={() => closePlayground()} title="Close the Playground (Esc)" aria-label="Close the Playground" className="w-ctl-sm !px-0">
          <X size={14} />
        </Button>
      </div>
      <div role="listbox" aria-label="Versions" className="flex-1 min-h-0 overflow-y-auto py-1">
        {versions.map((v) => {
          const plays = !v.notes;
          return (
            <div
              key={v.name}
              role="option"
              aria-selected={plays}
              data-testid="version-row"
              data-version={v.name}
              data-plays={plays || undefined}
              data-star={v.star || undefined}
              title={renaming === v.name ? undefined : plays ? `${v.name} plays` : `Play ${v.name}`}
              onMouseDown={keepFocus}
              onClick={() => !off && renaming !== v.name && void playVersion(v.name)}
              className={clsx("group h-ctl mx-1 pl-1.5 pr-0.5 rounded-sm flex items-center gap-1.5 text-sm cursor-pointer transition-colors duration-80 ease-linear", plays ? "bg-accent-soft text-fg-strong" : "text-fg hover:bg-hover")}
            >
              {/* Which one plays: its room is every row's, so names line up. */}
              <span className="w-3 shrink-0 text-accent-strong">{plays && <Play size={10} fill="currentColor" />}</span>
              {renaming === v.name ? (
                <Rename name={v.name} onDone={() => setRenaming(null)} />
              ) : (
                <span className="flex-1 min-w-0 flex items-baseline gap-1.5">
                  <span className={clsx("min-w-0 truncate", plays && "font-medium")}>{v.name}</span>
                  {v.by && (
                    <span className="shrink-0 max-w-20 truncate text-xs text-fg-faint" title={`Added by ${v.by}`}>
                      by <span data-by>{v.by}</span>
                    </span>
                  )}
                </span>
              )}
              {some && renaming !== v.name && (
                <>
                  <button
                    type="button"
                    data-testid="version-star"
                    aria-pressed={!!v.star}
                    disabled={!!off}
                    title={off ?? (v.star ? `Take the star off ${v.name}${plays ? " (S)" : ""}` : `Star ${v.name}${plays ? " (S)" : ""}`)}
                    aria-label={v.star ? `Take the star off ${v.name}` : `Star ${v.name}`}
                    onMouseDown={keepFocus}
                    onClick={(e) => {
                      e.stopPropagation();
                      starVersion(v.name);
                    }}
                    className={clsx(
                      "size-ctl-sm shrink-0 grid place-items-center rounded-sm cursor-pointer transition-colors duration-80 ease-linear hover:bg-press",
                      v.star ? "text-accent-strong" : "text-fg-off hover:text-fg",
                    )}
                  >
                    <Star size={14} fill={v.star ? "currentColor" : "none"} />
                  </button>
                  <button
                    type="button"
                    data-testid="version-more"
                    aria-haspopup="menu"
                    aria-expanded={menu?.name === v.name}
                    title={`Rename or remove ${v.name}`}
                    aria-label={`More for ${v.name}`}
                    onMouseDown={keepFocus}
                    onClick={(e) => {
                      e.stopPropagation();
                      const anchor = e.currentTarget;
                      setMenu((m) => (m?.name === v.name ? null : { name: v.name, anchor }));
                    }}
                    className="size-ctl-sm shrink-0 grid place-items-center rounded-sm cursor-pointer text-fg-muted transition-colors duration-80 ease-linear hover:bg-press hover:text-fg"
                  >
                    <Ellipsis size={14} />
                  </button>
                </>
              )}
            </div>
          );
        })}
        {!some && (
          <p data-testid="playground-empty" className="px-3 py-2 text-sm text-fg-muted">
            This clip has one version. An agent can add more to choose from, or press Duplicate and change the copy.
          </p>
        )}
      </div>
      <div className="shrink-0 flex items-center gap-2 p-2 border-t border-line">
        <Button
          variant="secondary"
          size="sm"
          data-testid="playground-duplicate"
          disabled={!!off}
          onMouseDown={keepFocus}
          onClick={duplicateVersion}
          title={off ?? "Copy the version that plays, and play the copy"}
          className="flex-1 min-w-0"
        >
          Duplicate
        </Button>
        <Button
          variant="primary"
          size="sm"
          data-testid="playground-keep"
          disabled={!some || !!off}
          onMouseDown={keepFocus}
          onClick={() => void keepVersion()}
          title={off ?? (some ? "Keep the version that plays, remove the others, and close the Playground" : "There is one version: nothing else to remove")}
          className="flex-1 min-w-0"
        >
          Keep this one
        </Button>
      </div>
      {menu && (
        <Popover anchor={menu.anchor} onClose={() => setMenu(null)} align="end" role="menu" data-testid="version-menu" aria-label={`Version ${menu.name}`} className="w-44">
          <MenuRow
            data-menu-item="rename"
            disabled={!!off}
            title={off}
            onClick={() => {
              setRenaming(menu.name);
              setMenu(null);
            }}
          >
            Rename
          </MenuRow>
          {(() => {
            const plays = !versions.find((v) => v.name === menu.name)?.notes;
            const why = off ?? (plays ? "This version plays. Play another first, or keep it and remove the others." : undefined);
            return (
              <MenuRow
                data-menu-item="remove"
                disabled={!!why}
                title={why}
                onClick={() => {
                  void removeVersion(menu.name);
                  setMenu(null);
                }}
              >
                Remove
              </MenuRow>
            );
          })()}
        </Popover>
      )}
    </aside>
  );
}

/** A version's name as a field: Enter (or a click away) writes it, Esc keeps the old one. */
function Rename({ name, onDone }: { name: string; onDone: () => void }) {
  const [draft, setDraft] = useState(name);
  const input = useRef<HTMLInputElement>(null);
  const done = useRef(false);
  useEffect(() => input.current?.select(), []);
  const commit = () => {
    if (done.current) return;
    done.current = true;
    const next = draft.trim();
    if (next && next !== name) void renameVersion(name, next);
    onDone();
  };
  return (
    <input
      ref={input}
      data-testid="version-name"
      aria-label={`Rename version ${name}`}
      value={draft}
      autoFocus
      spellCheck={false}
      maxLength={40}
      onChange={(e) => setDraft(e.target.value)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") commit();
        else if (e.key === "Escape") {
          done.current = true;
          onDone();
        }
      }}
      onBlur={commit}
      onClick={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      className="flex-1 min-w-0 h-ctl-sm px-1 -ml-1 rounded-xs bg-raised text-sm text-fg outline-none shadow-[inset_0_0_0_1px_var(--color-accent)]"
    />
  );
}

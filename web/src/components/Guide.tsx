import clsx from "clsx";
import { ChevronDown, ChevronRight, CircleCheck, X } from "lucide-react";
import { closeGuide, foldGuide, openGuide, STEPS, useExported, useGuide } from "../lib/guide";
import { native, nativeAction, revealLabel } from "../lib/native";
import { useStore } from "../lib/store";
import { openNewSong } from "./NewSongDialog";
import { Button, keepFocus } from "./ui";

/**
 * Where the song went, in the done line. A browser's export always goes to the Exports folder; the
 * app's goes where its save panel says, so the line names the file (the review's F6).
 */
function doneWords(exported: string | null) {
  if (!native) return "You made a song. It is in your Exports folder.";
  if (!exported) return "You made a song.";
  return `You made a song. It is saved as ${exported.split(/[\\/]/).pop()}.`;
}

/**
 * Make Your First Song (the studio plan's Q47): a card at the top of the side browser, so it never
 * covers the timeline. Seven steps that tick themselves: the current one in `fg` 600 with what to
 * do under it, the ones done `fg-muted` with a check, the rest `fg-faint`. Folded, one line.
 */
export default function Guide() {
  const guide = useGuide();
  const exported = useExported((s) => s.path);
  if (!guide?.open) return null;
  const done = new Set(guide.done);
  const count = STEPS.filter((s) => done.has(s.id)).length;
  const current = STEPS.find((s) => !done.has(s.id));
  const folded = !!guide.folded;
  const reveal = () => exported && void nativeAction("reveal_file", exported).catch((e) => useStore.getState().toast(e instanceof Error ? e.message : String(e)));
  return (
    <section data-testid="guide" aria-label="Your first song" className="mx-2 mt-2 shrink-0 rounded-md bg-raised border border-line">
      <div className="h-8 pl-1 pr-1 flex items-center gap-1">
        <Button
          size="sm"
          data-testid="guide-fold"
          aria-expanded={!folded}
          onMouseDown={keepFocus}
          onClick={() => foldGuide(!folded)}
          title={folded ? "Show the steps" : "Fold the guide to one line"}
          aria-label={folded ? "Show the steps" : "Fold the guide"}
          className="w-ctl-sm !px-0 shrink-0"
        >
          {folded ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        </Button>
        <span data-testid="guide-title" className="flex-1 min-w-0 truncate text-sm font-semibold text-fg">
          Your first song{folded && <span className="font-normal text-fg-muted"> · {count} of {STEPS.length}</span>}
        </span>
        <Button
          size="sm"
          data-testid="guide-close"
          onMouseDown={keepFocus}
          onClick={closeGuide}
          title="Close the guide. Help ▸ Make Your First Song brings it back"
          aria-label="Close the guide"
          className="w-ctl-sm !px-0 shrink-0"
        >
          <X size={14} />
        </Button>
      </div>
      {!folded && (
        <div className="px-2 pb-2">
          <ol className="space-y-1.5">
            {STEPS.map((s, i) => {
              const ok = done.has(s.id);
              const now = s === current;
              return (
                <li key={s.id} data-step={s.id} data-done={ok || undefined} data-current={now || undefined} className="flex items-start gap-2 text-sm">
                  {ok ? (
                    <CircleCheck size={14} className="mt-0.5 shrink-0 text-accent" aria-label="Done" />
                  ) : (
                    <span className={clsx("w-3.5 mt-px shrink-0 text-center text-xs font-medium", now ? "text-fg" : "text-fg-faint")}>{i + 1}</span>
                  )}
                  <span className="min-w-0">
                    <span className={clsx(ok ? "text-fg-muted" : now ? "font-semibold text-fg" : "text-fg-faint")}>{s.title}</span>
                    {now && <span className="block text-fg-muted">{s.hint()}</span>}
                  </span>
                </li>
              );
            })}
          </ol>
          {!current && (
            <div data-testid="guide-done" className="mt-2 pt-2 border-t border-line text-sm text-fg">
              {doneWords(exported)}
              {native && exported && (
                <Button variant="secondary" size="sm" onClick={reveal} className="mt-1.5">
                  {revealLabel}
                </Button>
              )}
            </div>
          )}
          <button
            data-testid="guide-empty"
            onClick={() => openNewSong({ template: "empty", after: openGuide })}
            className="mt-2 text-xs text-fg-muted hover:text-fg cursor-pointer"
          >
            Start from an empty song…
          </button>
        </div>
      )}
    </section>
  );
}

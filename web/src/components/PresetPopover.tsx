import { Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { keptWords, presetsApi, type PresetList } from "../lib/presets";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { Button, Popover } from "./ui";

/**
 * A sound card's Presets (FMT-22, gaps #26): a search field; your presets, then music2's own; a row
 * loads on click, as one undoable edit. Save as preset… names the card's settings (the list makes
 * way for the name) and keeps them in your Presets folder; a name that is there asks before it
 * replaces it.
 */
export default function PresetPopover({
  node,
  title,
  session,
  revision,
  anchor,
  locked,
  onClose,
}: {
  node: string;
  title: string;
  session: number;
  revision: number;
  anchor: HTMLElement;
  /** Why loading can't change the song here, or null. Saving still works. */
  locked: string | null;
  onClose: () => void;
}) {
  const toast = useStore((s) => s.toast);
  const [list, setList] = useState<PresetList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  // Save as preset…: the name being typed, and the name it would replace once asked.
  const [naming, setNaming] = useState<string | null>(null);
  const [taken, setTaken] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    presetsApi.list(session, revision, node).then(
      (l) => live && setList(l),
      (e) => live && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      live = false;
    };
  }, [session, revision, node]);

  const q = query.trim().toLowerCase();
  const groups = useMemo(
    () =>
      list
        ? (
            [
              ["Yours", "yours", list.yours],
              ["music2", "std", list.std],
            ] as const
          ).map(([name, from, rows]) => ({
            name,
            from,
            rows: rows.filter((r) => r.name.toLowerCase().includes(q)),
          }))
        : [],
    [list, q],
  );

  const load = (from: "std" | "yours", name: string) => {
    onClose();
    presetsApi.load(session, revision, node, from, name).then(
      (r) => {
        const kept = keptWords(r.kept);
        if (!r.changed)
          toast(
            `${title} has ${name}'s settings already.${kept ? ` ${kept}` : ""}`,
          );
        else
          toast(
            `Loaded ${name} into ${title}${kept ? `. ${kept}` : ""}`,
            undoAction(session, r.revision),
          );
      },
      (e) => toast(e instanceof Error ? e.message : String(e)),
    );
  };

  const save = (replace: boolean) => {
    const name = (naming ?? "").trim();
    presetsApi.save(session, revision, node, name, replace).then(
      (r) => {
        if (r.taken) {
          setTaken(r.taken);
          return;
        }
        onClose();
        const left = keptWords(r.left ?? [], true);
        toast(
          `Saved ${title}'s settings as ${r.name}${left ? `. ${left}` : ""}`,
        );
      },
      (e) => setSaveError(e instanceof Error ? e.message : String(e)),
    );
  };

  return (
    <Popover
      anchor={anchor}
      onClose={onClose}
      align="end"
      role="dialog"
      aria-label={`${title} presets`}
      data-testid="preset-popover"
      className="w-[260px] py-1"
    >
      {naming === null && (
        <>
          <label className="mx-2 my-1 h-ctl-sm px-2 flex items-center gap-1.5 rounded-sm bg-raised border border-line-strong hover:not-focus-within:border-fg-off focus-within:border-accent transition-colors duration-80 ease-linear">
            <Search size={13} className="shrink-0 text-fg-faint" />
            <input
              autoFocus
              className="min-w-0 flex-1 bg-transparent text-sm text-fg outline-none placeholder:text-fg-off"
              placeholder="Search presets"
              aria-label="Search presets"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </label>
          <div className="max-h-[300px] overflow-y-auto">
            {error && (
              <div className="px-3 py-2 text-sm text-fg-muted">{error}</div>
            )}
            {!list && !error && (
              <div className="px-3 py-2 text-sm text-fg-muted">
                Looking for presets…
              </div>
            )}
            {groups.map((g) => (
              <div
                key={g.from}
                role="group"
                aria-label={g.name}
                data-preset-group={g.from}
              >
                <div className="px-3 pt-2 pb-1 text-xs font-medium text-fg-muted">
                  {g.name}
                </div>
                {g.rows.length === 0 && (
                  <div className="px-3 pb-1 text-sm text-fg-faint">
                    {q
                      ? "None match."
                      : g.from === "yours"
                        ? "None yet. Save one below."
                        : "None for this sound yet."}
                  </div>
                )}
                {g.rows.map((r) => (
                  <button
                    key={r.name}
                    className="w-full h-ctl-sm px-3 text-left text-sm text-fg truncate hover:bg-hover disabled:text-fg-off disabled:pointer-events-none cursor-pointer"
                    data-preset={r.name}
                    title={locked ?? `Load ${r.name} into ${title}`}
                    disabled={!!locked}
                    onClick={() => load(g.from, r.name)}
                  >
                    {r.name}
                  </button>
                ))}
              </div>
            ))}
          </div>
        </>
      )}
      <div
        className={
          naming === null ? "mt-1 border-t border-line pt-1" : undefined
        }
      >
        {naming === null ? (
          <button
            className="w-full h-ctl-sm px-3 text-left text-sm text-fg hover:bg-hover cursor-pointer"
            data-testid="preset-save"
            title={`Keep ${title}'s settings as a preset of yours`}
            onClick={() => setNaming("")}
          >
            Save as preset…
          </button>
        ) : (
          <form
            className="px-2 py-1 flex flex-col gap-1.5"
            onSubmit={(e) => {
              e.preventDefault();
              if (naming.trim()) save(!!taken);
            }}
          >
            <input
              autoFocus
              className="h-ctl-sm px-2 text-sm text-fg outline-none placeholder:text-fg-off rounded-sm bg-raised border border-line-strong hover:not-focus:border-fg-off focus:border-accent transition-colors duration-80 ease-linear"
              placeholder="Preset name"
              aria-label="Preset name"
              data-testid="preset-name"
              maxLength={40}
              value={naming}
              onChange={(e) => {
                setNaming(e.target.value);
                setTaken(null);
                setSaveError(null);
              }}
            />
            {taken && (
              <div className="text-xs text-fg-muted">
                A preset called {taken} is there. Replace it?
              </div>
            )}
            {saveError && (
              <div className="text-xs text-fg-muted">{saveError}</div>
            )}
            <div className="flex justify-end gap-1.5">
              <Button type="button" size="sm" onClick={() => setNaming(null)}>
                Cancel
              </Button>
              <Button
                type="submit"
                size="sm"
                variant="primary"
                data-testid="preset-save-confirm"
                disabled={!naming.trim()}
              >
                {taken ? "Replace" : "Save"}
              </Button>
            </div>
          </form>
        )}
      </div>
    </Popover>
  );
}

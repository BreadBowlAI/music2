import clsx from "clsx";
import { BookOpen, ChevronDown, FolderOpen, LoaderCircle, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { projectApi, tilde, type Catalog, type Starter } from "../lib/graph";
import { chord, native, nativeAction } from "../lib/native";
import { useStore } from "../lib/store";
import { openExample, openNewSong } from "./NewSongDialog";
import { Button } from "./ui";

const Key = ({ children }: { children: string }) => <kbd className="ml-1 font-sans text-xs font-normal opacity-70">{children}</kbd>;

/** What the app shows when no song is open: make a new one, open one, or try an example. */
export default function StartScreen() {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  useEffect(() => {
    projectApi.catalog().then(setCatalog, () => {});
  }, []);

  return (
    <div className="h-full flex flex-col" data-testid="start">
      {/* The macOS traffic lights sit in this strip, and the window drags by it. */}
      <div data-tauri-drag-region className="h-16 shrink-0" />
      {/* No logo: the window and the Dock say which app this is. Three actions in a row, then where songs go. */}
      <div className="flex-1 grid place-items-center pb-8">
        <div className="flex flex-col items-center text-center animate-rise">
          <h1 className="text-lg font-semibold text-fg-strong">Make a song, or open one you’ve made.</h1>

          <div className="mt-6 flex items-start gap-2">
            <Button variant="primary" size="lg" data-testid="new-song" onClick={openNewSong}>
              <Plus size={14} />
              New Song…
              {native && <Key>{chord("N")}</Key>}
            </Button>
            {native && (
              // The same native action as the menu item: the folder dialog, then the open.
              <Button variant="secondary" size="lg" data-testid="open-project" onClick={() => void nativeAction("open_project")}>
                <FolderOpen size={14} />
                Open Project Folder…
                <Key>{chord("O")}</Key>
              </Button>
            )}
            <Examples catalog={catalog} />
          </div>

          <p className="mt-4 text-xs text-fg-faint">
            {catalog?.songs ? (
              <>
                New songs are saved in <span className="text-fg-muted">{tilde(catalog.songs, catalog.home)}</span>.
              </>
            ) : (
              "Each song is a folder of code and notes."
            )}
            {!native && (
              // Outside the app there is no folder dialog to show.
              <>
                <br />
                To open a folder you already have, use File → Open Project Folder… in the music2 app.
              </>
            )}
          </p>
        </div>
      </div>
    </div>
  );
}

/** "Open an example": a list of the examples. Picking one opens a copy of it in the songs folder. */
function Examples({ catalog }: { catalog: Catalog | null }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const examples = catalog?.examples ?? [];

  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, [open]);

  const pick = async (example: Starter) => {
    if (busy) return;
    setBusy(example.id);
    try {
      await openExample(example, catalog?.home ?? null);
    } catch (error) {
      useStore.getState().toast(`Couldn't open ${example.name}: ${error instanceof Error ? error.message : String(error)}`);
      setBusy(null);
    }
  };

  return (
    <div
      ref={box}
      className="relative"
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
        }
      }}
    >
      <Button
        variant="secondary"
        size="lg"
        data-testid="open-example"
        aria-expanded={open}
        aria-haspopup="menu"
        disabled={!examples.length}
        onClick={() => setOpen((o) => !o)}
      >
        <BookOpen size={14} />
        Examples
        <ChevronDown size={12} className={clsx("transition-transform duration-140 ease-out", open && "rotate-180")} />
      </Button>
      {open && (
        <div role="menu" aria-label="Examples" data-testid="examples" className="absolute z-20 left-0 top-full mt-1 w-80 p-1 rounded-md bg-raised shadow-pop text-left animate-rise">
          <div className="px-2 pt-1 pb-1.5 text-xs text-fg-muted">A copy opens in your songs folder, so the example stays as it is.</div>
          {examples.map((ex) => (
            <button
              key={ex.id}
              role="menuitem"
              data-example={ex.id}
              disabled={!!busy}
              onClick={() => void pick(ex)}
              className="w-full px-2 py-1.5 rounded-sm flex items-start gap-2 text-left cursor-pointer transition-colors duration-80 ease-linear hover:bg-accent-soft focus-visible:bg-accent-soft outline-none disabled:cursor-default"
            >
              <span className="flex-1 min-w-0">
                <span className="block text-sm font-medium text-fg">{ex.name}</span>
                {ex.description && <span className="block text-xs text-fg-muted">{ex.description}</span>}
              </span>
              {busy === ex.id && <LoaderCircle size={14} className="mt-px shrink-0 animate-spin text-fg-muted" />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

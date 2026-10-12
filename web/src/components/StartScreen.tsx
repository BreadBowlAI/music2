import clsx from "clsx";
import { BookOpen, ChevronDown, FolderOpen, LoaderCircle, Music, Plus } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { projectApi, tilde, type Catalog, type Starter } from "../lib/graph";
import { kbd, native, nativeAction } from "../lib/native";
import { useStore } from "../lib/store";
import { openExample, openNewSong } from "./NewSongDialog";
import { Button } from "./ui";

const Key = ({ children }: { children: string }) => (
  <kbd className="ml-auto h-6 px-1.5 grid place-items-center rounded-md bg-black/[0.06] font-sans text-[11px] font-medium opacity-80">{children}</kbd>
);

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
      <div className="flex-1 grid place-items-center pb-16">
        <div className="w-[340px] flex flex-col items-center text-center rise">
          <div className="size-16 rounded-[20px] pop-gradient grid place-items-center shadow-xl shadow-pop-500/20">
            <span className="text-white text-2xl font-bold">m2</span>
          </div>
          <h1 className="mt-5 text-3xl font-semibold tracking-tight text-ink-100">music2</h1>
          <p className="mt-2 text-[15px] text-ink-400">Make a song, or open one you’ve made.</p>

          <div className="mt-8 w-full flex flex-col gap-2.5">
            <Button variant="pop" size="lg" data-testid="new-song" onClick={openNewSong} className="w-full !justify-start">
              <Plus className="size-[18px]" />
              New Song…
              {native && <Key>{kbd("N")}</Key>}
            </Button>
            {native && (
              // The same native action as the menu item: the folder dialog, then the open.
              <Button variant="outline" size="lg" data-testid="open-project" onClick={() => void nativeAction("open_project")} className="w-full !justify-start bg-ink-850">
                <FolderOpen className="size-[18px]" />
                Open Project Folder…
                <Key>{kbd("O")}</Key>
              </Button>
            )}
            <Examples catalog={catalog} />
          </div>

          <p className="mt-6 text-xs leading-5 text-ink-500">
            {catalog?.songs ? (
              <>
                New songs are saved in <span className="text-ink-400">{tilde(catalog.songs, catalog.home)}</span>.
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
        variant="outline"
        size="lg"
        data-testid="open-example"
        aria-expanded={open}
        aria-haspopup="menu"
        disabled={!examples.length}
        onClick={() => setOpen((o) => !o)}
        className="w-full !justify-start bg-ink-850"
      >
        <BookOpen className="size-[18px]" />
        Open an Example
        <ChevronDown className={clsx("ml-auto size-4 transition-transform", open && "rotate-180")} />
      </Button>
      {open && (
        <div
          role="menu"
          aria-label="Examples"
          data-testid="examples"
          className="absolute z-20 left-0 right-0 top-full mt-1.5 p-1.5 rounded-2xl bg-ink-850 ring-1 ring-black/10 shadow-2xl shadow-black/20 text-left rise"
        >
          <div className="px-2.5 pt-1.5 pb-2 text-[11px] leading-4 text-ink-400">A copy opens in your songs folder, so the example stays as it is.</div>
          {examples.map((ex) => (
            <button
              key={ex.id}
              role="menuitem"
              data-example={ex.id}
              disabled={!!busy}
              onClick={() => void pick(ex)}
              className="w-full px-2.5 py-2 rounded-xl flex items-center gap-3 text-left cursor-pointer transition-colors hover:bg-ink-700/50 focus-visible:bg-ink-700/50 outline-none disabled:cursor-default"
            >
              <span className="size-8 shrink-0 rounded-[10px] grid place-items-center bg-pop-500/10 text-pop-500">
                {busy === ex.id ? <LoaderCircle className="size-4 animate-spin" /> : <Music className="size-4" />}
              </span>
              <span className="flex-1 min-w-0">
                <span className="block text-[13px] font-medium text-ink-100 leading-5">{ex.name}</span>
                {ex.description && <span className="block text-xs text-ink-400 leading-4">{ex.description}</span>}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

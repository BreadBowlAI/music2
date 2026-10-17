import clsx from "clsx";
import { BookOpen, ChevronDown, FolderOpen, LoaderCircle, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { graphApi, projectApi, tilde, type Catalog, type Starter } from "../lib/graph";
import { chord, native, nativeAction } from "../lib/native";
import { bpmWords, openedWords, recentApi, type RecentSong } from "../lib/projects";
import { useStore } from "../lib/store";
import { openExample, openNewSong } from "./NewSongDialog";
import { Button } from "./ui";

const Key = ({ children }: { children: string }) => <kbd className="ml-1 font-sans text-xs font-normal opacity-70">{children}</kbd>;

/** What the app shows when no song is open: make a new one, open one, try an example, or pick a recent one. */
export default function StartScreen() {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  useEffect(() => {
    projectApi.catalog().then(setCatalog, () => {});
  }, []);

  return (
    <div className="h-full flex flex-col" data-testid="start">
      {/* The macOS traffic lights sit in this strip, and the window drags by it. */}
      <div data-tauri-drag-region className="h-16 shrink-0" />
      {/* No logo: the window and the Dock say which app this is. Three actions in a row, then where songs go, then the recent songs. */}
      <div className="flex-1 min-h-0 overflow-y-auto">
        <div className="min-h-full grid place-items-center pb-8">
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
            <Recent home={catalog?.home ?? null} />
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * The recent songs (taste.md SS-2, polish C2), newest first: a 560 px column of 44 px rows, each
 * the song's name and "124 BPM · opened yesterday", its folder in the tooltip. A click opens it;
 * Return opens the newest while no button has the focus (Return on a button presses that button).
 * A song whose folder is gone says "Moved or deleted", and its × takes it off the list. No
 * picture of the song yet (left).
 */
function Recent({ home }: { home: string | null }) {
  const [songs, setSongs] = useState<RecentSong[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => {
    recentApi.list().then(setSongs, () => {});
  }, []);
  const open = async (song: RecentSong) => {
    if (busy || song.missing) return;
    setBusy(song.path);
    try {
      await graphApi.open(song.path);
    } catch (error) {
      useStore.getState().toast(`Couldn't open ${song.name}: ${error instanceof Error ? error.message : String(error)}`);
      setBusy(null);
    }
  };
  const forget = async (song: RecentSong) => {
    try {
      setSongs(await recentApi.forget(song.path));
    } catch (error) {
      useStore.getState().toast(error instanceof Error ? error.message : String(error));
    }
  };
  const newest = songs.find((s) => !s.missing);
  useEffect(() => {
    if (!newest) return;
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      // Return on a button presses it; in a field (a dialog's), it is the field's.
      const at = document.activeElement;
      if (at instanceof HTMLButtonElement || at instanceof HTMLInputElement || at instanceof HTMLTextAreaElement || document.querySelector("[aria-modal]")) return;
      e.preventDefault();
      void open(newest);
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  });
  if (!songs.length) return null;
  return (
    <section aria-labelledby="recent-title" data-testid="recent" className="mt-10 w-[560px] max-w-full text-left">
      <h2 id="recent-title" className="px-2 pb-1 text-xs font-medium text-fg-muted">
        Recent
      </h2>
      <ul>
        {songs.map((song) => (
          <li key={song.path} className="relative">
            <button
              data-recent={song.path}
              data-missing={song.missing || undefined}
              aria-disabled={song.missing || undefined}
              title={tilde(song.path, home)}
              onClick={() => void open(song)}
              // A list row (taste.md §2.9): accent-soft under the pointer, and the same with the
              // keyboard's focus, a gone song's row too (review PL-T5).
              className={clsx(
                "w-full h-11 px-2 pr-9 rounded-sm flex flex-col justify-center text-left outline-none transition-colors duration-80 ease-linear focus-visible:bg-accent-soft",
                song.missing ? "cursor-default" : "cursor-pointer hover:bg-accent-soft",
              )}
            >
              <span className={clsx("block w-full truncate text-base font-medium", song.missing ? "text-fg-faint" : "text-fg")}>{song.name}</span>
              <span className="block w-full truncate text-xs text-fg-muted">
                {song.missing ? "Moved or deleted" : `${song.tempo !== null ? `${bpmWords(song.tempo)} · ` : ""}${openedWords(song.opened)}`}
              </span>
            </button>
            {busy === song.path && <LoaderCircle size={14} className="absolute right-3 top-[15px] animate-spin text-fg-muted" />}
            {song.missing && (
              <Button
                size="sm"
                data-forget={song.path}
                aria-label={`Take ${song.name} off the list`}
                title="Take it off the list"
                onClick={() => void forget(song)}
                className="absolute right-2 top-2.5 w-ctl-sm !px-0"
              >
                <X size={14} />
              </Button>
            )}
          </li>
        ))}
      </ul>
    </section>
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
      <Button variant="secondary" size="lg" data-testid="open-example" aria-expanded={open} aria-haspopup="menu" disabled={!examples.length} onClick={() => setOpen((o) => !o)}>
        <BookOpen size={14} />
        Examples
        <ChevronDown size={12} className={clsx("transition-transform duration-140 ease-out", open && "rotate-180")} />
      </Button>
      {open && (
        <div
          role="menu"
          aria-label="Examples"
          data-testid="examples"
          className="absolute z-20 left-0 top-full mt-1 w-80 p-1 rounded-md bg-raised shadow-pop text-left animate-rise"
        >
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

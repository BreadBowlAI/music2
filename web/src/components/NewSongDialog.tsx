import clsx from "clsx";
import { Check, CornerDownLeft, FolderOpen, LoaderCircle, Music, Plus, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { create } from "zustand";
import { editorHooks, projectApi, tilde, type Catalog, type GraphState, type Starter } from "../lib/graph";
import { isMac, native, nativeAction } from "../lib/native";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/** Whether the New Song dialog is showing. */
const useNewSong = create<{ open: boolean }>(() => ({ open: false }));

/** Show the New Song dialog: the start screen's button, File → New Song… and ⌘N. */
export const openNewSong = () => useNewSong.setState({ open: true });

const DEFAULT_NAME = "My song";

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** A new song is open: show it, and say where its files went. */
function opened(graph: GraphState, home: string | null) {
  const { set, toast } = useStore.getState();
  set({ graph });
  const where = tilde(graph.dir.slice(0, graph.dir.lastIndexOf("/")), home);
  const playable = graph.timeline.tracks.some((t) => t.track);
  toast(
    `Made “${graph.name}” in ${where}.${playable ? " Press Space to play it." : ""}`,
    native ? { label: isMac ? "Show in Finder" : "Show Folder", run: () => void nativeAction("reveal") } : undefined,
  );
}

/** Copy an example into the songs folder and open the copy, so the example itself stays as it is. */
export async function openExample(example: Starter, home: string | null) {
  opened(await projectApi.create({ name: example.name, example: example.id }), home);
}

export default function NewSongDialog() {
  const open = useNewSong((s) => s.open);
  return open ? <Dialog onClose={() => useNewSong.setState({ open: false })} /> : null;
}

/** A name, a template and where to save it. Enter creates the song and opens it; Esc cancels. */
function Dialog({ onClose }: { onClose: () => void }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [name, setName] = useState(DEFAULT_NAME);
  const [template, setTemplate] = useState("starter");
  // The folder picked with Choose Folder…; otherwise the songs folder.
  const [dir, setDir] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Making a song closes the open one: unsaved code in its editor would be lost.
  const [unsaved] = useState(() => (useStore.getState().graph ? (editorHooks.unsaved?.() ?? 0) : 0));
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.select();
    projectApi
      .catalog()
      .then((c) => {
        setCatalog(c);
        if (!c.templates.some((t) => t.id === "starter") && c.templates[0]) setTemplate(c.templates[0].id);
      })
      .catch((e) => setError(message(e)));
  }, []);

  const folder = dir ?? catalog?.songs ?? null;
  const templates = catalog?.templates ?? [];

  const make = async () => {
    if (busy || !catalog) return;
    setBusy(true);
    setError(null);
    try {
      const graph = await projectApi.create({ name: name.trim() || DEFAULT_NAME, template, dir: dir ?? undefined });
      onClose();
      opened(graph, catalog.home);
    } catch (e) {
      setError(message(e));
      setBusy(false);
    }
  };

  const choose = async () => {
    try {
      const picked = await nativeAction("song_folder", folder ?? undefined);
      if (typeof picked === "string") setDir(picked);
    } catch (e) {
      setError(message(e));
    }
  };

  const onKey = (e: KeyboardEvent) => {
    // The dialog has the keyboard: Space, Enter and ⌘Z don't reach the song behind it.
    e.stopPropagation();
    const target = e.target as HTMLElement;
    if (e.key === "Escape") {
      e.preventDefault();
      if (!busy) onClose();
    } else if (e.key === "Enter" && (target === input.current || target.dataset.template)) {
      // Other buttons (Cancel, Choose Folder…) do what they say.
      e.preventDefault();
      void make();
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && target.dataset.template && templates.length) {
      e.preventDefault();
      const i = templates.findIndex((t) => t.id === template);
      const next = templates[(i + (e.key === "ArrowRight" ? 1 : templates.length - 1)) % templates.length];
      setTemplate(next.id);
      (target.parentElement?.querySelector(`[data-template="${next.id}"]`) as HTMLElement | null)?.focus();
    }
  };

  return (
    <div
      className="fixed inset-0 z-[70] grid place-items-center p-4 bg-ink-100/15 backdrop-blur-[3px]"
      onPointerDown={(e) => e.target === e.currentTarget && !busy && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-song-title"
        data-testid="new-song-dialog"
        className="w-[560px] max-w-full rounded-[22px] bg-ink-850 ring-1 ring-black/10 shadow-2xl shadow-black/25 overflow-hidden rise cursor-default"
        onKeyDown={onKey}
      >
        <div className="px-6 pt-6">
          <h2 id="new-song-title" className="text-[19px] font-semibold tracking-tight text-ink-100">New song</h2>
          <p className="mt-1 text-[13px] text-ink-400">Give it a name and pick how it starts. You can change everything later.</p>
        </div>

        <div className="px-6 pt-5 space-y-5">
          <label className="block">
            <span className="text-[12px] font-medium text-ink-300">Name</span>
            <input
              ref={input}
              autoFocus
              value={name}
              placeholder={DEFAULT_NAME}
              maxLength={80}
              spellCheck={false}
              aria-label="Song name"
              data-testid="song-name"
              onChange={(e) => setName(e.target.value)}
              className="mt-1.5 w-full h-11 px-3.5 rounded-xl bg-ink-800 ring-1 ring-black/10 outline-none focus:ring-2 focus:ring-pop-500/50 text-[15px] text-ink-100 placeholder:text-ink-500 transition-shadow"
            />
          </label>

          <div>
            <span id="new-song-start" className="text-[12px] font-medium text-ink-300">Start from</span>
            <div role="radiogroup" aria-labelledby="new-song-start" className="mt-1.5 grid grid-cols-2 gap-3 min-h-[156px]">
              {templates.map((t) => (
                <TemplateCard key={t.id} starter={t} selected={t.id === template} onSelect={() => setTemplate(t.id)} onOpen={() => void make()} />
              ))}
              {!catalog && !error && (
                <div className="col-span-2 grid place-items-center text-[13px] text-ink-400">
                  <LoaderCircle className="size-4 animate-spin" />
                </div>
              )}
            </div>
          </div>

          {unsaved > 0 && (
            <div className="flex items-start gap-2.5 px-3.5 py-2.5 rounded-xl bg-amber-500/10 ring-1 ring-amber-600/20 text-[12px] text-amber-950">
              <TriangleAlert className="size-4 shrink-0 text-amber-600 mt-px" />
              <span>
                {unsaved === 1 ? "A file has" : `${unsaved} files have`} unsaved edits in the code editor. Making a new song closes this one
                without saving them.
              </span>
            </div>
          )}
          {error && (
            <div role="alert" className="flex items-start gap-2.5 px-3.5 py-2.5 rounded-xl bg-red-50 ring-1 ring-red-500/25 text-[12px] text-red-800">
              <TriangleAlert className="size-4 shrink-0 text-red-500 mt-px" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          )}
        </div>

        <div className="mt-6 px-6 py-3.5 flex items-center gap-2 border-t border-black/[0.06] bg-ink-800">
          <div className="flex-1 min-w-0 flex items-center gap-2 text-[12px] text-ink-400" title={folder ?? undefined}>
            <FolderOpen className="size-4 shrink-0" />
            <span className="truncate">
              Saves in <span className="font-medium text-ink-200" data-testid="song-folder">{folder ? tilde(folder, catalog?.home ?? null) : "…"}</span>
            </span>
          </div>
          {native && (
            <Button size="sm" variant="ghost" onClick={() => void choose()} disabled={busy} title="Pick another folder to save the song in">
              Choose Folder…
            </Button>
          )}
          <Button variant="ghost" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="pop" data-testid="create-song" onClick={() => void make()} disabled={busy || !catalog} className="min-w-[104px]">
            {busy ? <LoaderCircle className="size-4 animate-spin" /> : null}
            {unsaved > 0 ? "Create Anyway" : "Create"}
            {!busy && <CornerDownLeft className="size-3.5 opacity-70" />}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** A template: a small picture of how the song starts, its name and what it is. */
function TemplateCard({ starter, selected, onSelect, onOpen }: { starter: Starter; selected: boolean; onSelect: () => void; onOpen: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      tabIndex={selected ? 0 : -1}
      data-template={starter.id}
      data-testid={`template-${starter.id}`}
      onClick={onSelect}
      onDoubleClick={onOpen}
      className={clsx(
        "text-left rounded-2xl p-2.5 transition-all cursor-pointer outline-none focus-visible:ring-2 focus-visible:ring-pop-500/50",
        selected ? "bg-ink-800 ring-2 ring-pop-500/70 shadow-lg shadow-pop-500/10" : "bg-ink-850 ring-1 ring-black/[0.08] hover:ring-black/20 hover:bg-ink-800",
      )}
    >
      <Preview id={starter.id} />
      <div className="px-1 pt-2.5 pb-1">
        <div className="flex items-center gap-2">
          <span className="flex-1 min-w-0 truncate text-[14px] font-semibold text-ink-100">{starter.name}</span>
          <span className={clsx("size-[18px] shrink-0 rounded-full grid place-items-center transition-all", selected ? "pop-gradient shadow-md shadow-pop-500/20" : "ring-1 ring-inset ring-ink-600")}>
            {selected && <Check className="size-3 text-white" strokeWidth={3} />}
          </span>
        </div>
        <div className="mt-0.5 text-[12px] leading-[17px] text-ink-400 line-clamp-2">{starter.description}</div>
      </div>
    </button>
  );
}

/** The timeline's track colours (see SongTimeline). */
const HUES = ["255 94 135", "255 159 67", "255 214 92", "94 214 140", "72 196 230", "124 131 253"];

/** Clips per row, as [start, length] in eighths of the song, in the timeline's order (Bass, Chords, Clap, Hats, Hook,
 * Kick): every part all along, except the hook, which comes in halfway. */
const STARTER_ROWS: [number, number][][] = [
  [[0, 4], [4, 4]],
  [[0, 4], [4, 4]],
  [[0, 4], [4, 4]],
  [[0, 4], [4, 4]],
  [[4, 4]],
  [[0, 4], [4, 4]],
];

function Preview({ id }: { id: string }) {
  if (id === "starter") {
    return (
      <div aria-hidden className="h-[76px] rounded-xl bg-ink-900 ring-1 ring-black/[0.05] px-2 py-2 flex flex-col justify-between">
        {STARTER_ROWS.map((clips, row) => (
          <div key={row} className="relative h-[7px]">
            {clips.map(([start, length]) => (
              <span
                key={start}
                className="absolute inset-y-0 rounded-[3px]"
                style={{ left: `calc(${(start / 8) * 100}% + 1px)`, width: `calc(${(length / 8) * 100}% - 2px)`, background: `rgb(${HUES[row]} / 0.75)` }}
              />
            ))}
          </div>
        ))}
      </div>
    );
  }
  if (id === "empty") {
    return (
      <div aria-hidden className="h-[76px] rounded-xl bg-ink-900 ring-1 ring-black/[0.05] p-2">
        <div className="h-full rounded-lg border border-dashed border-ink-500/60 grid place-items-center text-ink-500">
          <Plus className="size-5" />
        </div>
      </div>
    );
  }
  return (
    <div aria-hidden className="h-[76px] rounded-xl bg-ink-900 ring-1 ring-black/[0.05] grid place-items-center text-ink-400">
      <Music className="size-6" />
    </div>
  );
}

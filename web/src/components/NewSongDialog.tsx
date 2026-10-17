import clsx from "clsx";
import { Check, CircleAlert, CornerDownLeft, FolderOpen, LoaderCircle, Music, Plus, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { create } from "zustand";
import { editorHooks, projectApi, tilde, type Catalog, type GraphState, type Starter } from "../lib/graph";
import { native, nativeAction, revealLabel } from "../lib/native";
import { DEFAULT_ORDER, TRACK_COLORS } from "../lib/palette";
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
    native ? { label: revealLabel, run: () => void nativeAction("reveal") } : undefined,
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
  // The template's tempo until one is typed (NS-3).
  const [tempo, setTempo] = useState<string | null>(null);
  const [tempoError, setTempoError] = useState(false);
  const bpmField = useRef<HTMLInputElement>(null);
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
  const templateTempo = templates.find((t) => t.id === template)?.tempo ?? null;
  const bpmText = tempo ?? (templateTempo === null ? "" : String(Number(templateTempo.toFixed(2))));

  const make = async () => {
    if (busy || !catalog) return;
    // "128", "128.5" or "128 bpm", 20 to 999 (FMT-13); the template's when left as it was.
    const typed = /^\s*(\d+(?:\.\d+)?)\s*(?:bpm)?\s*$/i.exec(bpmText);
    const bpm = typed ? Number(typed[1]) : NaN;
    if (tempo !== null && !(bpm >= 20 && bpm <= 999)) {
      setTempoError(true);
      bpmField.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const graph = await projectApi.create({
        name: name.trim() || DEFAULT_NAME,
        template,
        dir: dir ?? undefined,
        tempo: tempo !== null && bpm !== templateTempo ? Number(bpm.toFixed(2)) : undefined,
      });
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
    } else if (e.key === "Enter" && (target === input.current || target === bpmField.current || target.dataset.template)) {
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
      className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20"
      onPointerDown={(e) => e.target === e.currentTarget && !busy && onClose()}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-song-title"
        data-testid="new-song-dialog"
        className="w-[560px] max-w-full rounded-md bg-raised shadow-pop overflow-hidden animate-dialog cursor-default"
        onKeyDown={onKey}
      >
        <div className="px-6 pt-6">
          <h2 id="new-song-title" className="text-lg font-semibold text-fg-strong">New song</h2>
          <p className="mt-1 text-base text-fg-muted">Give it a name and pick how it starts. You can change everything later.</p>
        </div>

        <div className="px-6 pt-4 space-y-4">
          <div className="flex items-start gap-3">
            <label className="block flex-1 min-w-0">
              <span className="text-sm font-medium text-fg-muted">Name</span>
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
                className="mt-1.5 w-full h-8 px-3 rounded-sm bg-raised border border-line-strong hover:border-fg-off focus:border-accent text-base text-fg placeholder:text-fg-off transition-colors duration-80 ease-linear"
              />
            </label>
            {/* NS-3: the tempo it starts at, the template's unless typed; Tab moves here from the name. */}
            <label className="block shrink-0 relative" title="Beats per minute, 20 to 999. You can change it later in the top bar">
              <span className="block text-sm font-medium text-fg-muted">Tempo</span>
              <input
                ref={bpmField}
                value={bpmText}
                inputMode="decimal"
                spellCheck={false}
                aria-label="Tempo in BPM"
                aria-invalid={tempoError}
                data-testid="song-tempo"
                onChange={(e) => {
                  setTempo(e.target.value);
                  setTempoError(false);
                }}
                onFocus={(e) => e.target.select()}
                className={clsx(
                  "mt-1.5 w-16 h-8 px-3 rounded-sm bg-raised border text-base text-fg tabular-nums transition-colors duration-80 ease-linear",
                  tempoError ? "border-danger" : "border-line-strong hover:border-fg-off focus:border-accent",
                )}
              />
              {tempoError && <span role="alert" className="absolute left-0 top-full mt-0.5 text-xs text-danger whitespace-nowrap">20 to 999 BPM</span>}
            </label>
          </div>

          <div>
            <span id="new-song-start" className="text-sm font-medium text-fg-muted">Start from</span>
            <div role="radiogroup" aria-labelledby="new-song-start" className="mt-1.5 grid grid-cols-2 gap-3 min-h-[124px]">
              {templates.map((t) => (
                <TemplateCard key={t.id} starter={t} selected={t.id === template} onSelect={() => setTemplate(t.id)} onOpen={() => void make()} />
              ))}
              {!catalog && !error && (
                <div className="col-span-2 grid place-items-center text-fg-muted">
                  <LoaderCircle size={16} className="animate-spin" />
                </div>
              )}
            </div>
          </div>

          {unsaved > 0 && (
            <div className="flex items-start gap-2 px-3 py-2 rounded-sm bg-warn-soft text-sm text-warn-strong">
              <TriangleAlert size={14} className="shrink-0 text-warn mt-px" />
              <span>
                {unsaved === 1 ? "A file has" : `${unsaved} files have`} unsaved edits in the code editor. Making a new song closes this one
                without saving them.
              </span>
            </div>
          )}
          {error && (
            <div role="alert" className="flex items-start gap-2 px-3 py-2 rounded-sm bg-danger-soft text-sm text-danger-strong">
              <CircleAlert size={14} className="shrink-0 text-danger mt-px" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          )}
        </div>

        <div className="mt-6 px-6 py-3 flex items-center gap-2 border-t border-line bg-chrome">
          <div className="flex-1 min-w-0 flex items-center gap-2 text-sm text-fg-muted" title={folder ?? undefined}>
            <FolderOpen size={14} className="shrink-0" />
            <span className="truncate">
              Saves in <span className="font-medium text-fg" data-testid="song-folder">{folder ? tilde(folder, catalog?.home ?? null) : "…"}</span>
            </span>
          </div>
          {native && (
            <Button size="lg" variant="ghost" onClick={() => void choose()} disabled={busy} title="Pick another folder to save the song in">
              Choose Folder…
            </Button>
          )}
          <Button size="lg" variant="secondary" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button size="lg" variant="primary" data-testid="create-song" onClick={() => void make()} disabled={busy || !catalog} className="min-w-[104px]">
            {busy ? <LoaderCircle size={14} className="animate-spin" /> : null}
            {unsaved > 0 ? "Create Anyway" : "Create"}
            {!busy && <CornerDownLeft size={14} className="opacity-70" />}
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
        "text-left rounded-md p-2 bg-raised transition-shadow duration-80 ease-linear cursor-pointer",
        selected ? "ring-2 ring-accent" : "ring-1 ring-line hover:ring-line-strong",
      )}
    >
      <Preview id={starter.id} />
      <div className="px-1 pt-2 pb-1">
        <div className="flex items-center gap-2">
          <span className="flex-1 min-w-0 truncate text-base font-semibold text-fg">{starter.name}</span>
          <span className={clsx("size-4 shrink-0 rounded-full grid place-items-center", selected ? "bg-accent text-white" : "ring-1 ring-inset ring-line-strong")}>
            {selected && <Check size={12} />}
          </span>
        </div>
        <div className="mt-0.5 text-sm text-fg-muted line-clamp-2">{starter.description}</div>
      </div>
    </button>
  );
}

/** The timeline's track colours in its order (lib/palette): what the song will look like. */
const BARS = DEFAULT_ORDER.map((name) => TRACK_COLORS[name].bar);

/** Clips per row, as [start, length] in eighths of the song, in the timeline's order of the starter's tracks (`bass`,
 * `chords`, `clap`, `hats`, `hook`, `kick`): every part all along, except the hook, which comes in halfway. */
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
      <div aria-hidden className="h-12 rounded-sm bg-canvas ring-1 ring-line px-1.5 py-1.5 flex flex-col justify-between">
        {STARTER_ROWS.map((clips, row) => (
          <div key={row} className="relative h-1">
            {clips.map(([start, length]) => (
              <span
                key={start}
                className="absolute inset-y-0 rounded-xs"
                style={{ left: `calc(${(start / 8) * 100}% + 1px)`, width: `calc(${(length / 8) * 100}% - 2px)`, background: BARS[row] }}
              />
            ))}
          </div>
        ))}
      </div>
    );
  }
  if (id === "empty") {
    return (
      <div aria-hidden className="h-12 rounded-sm bg-canvas ring-1 ring-line p-1.5">
        <div className="h-full rounded-xs border border-dashed border-line-strong grid place-items-center text-fg-faint">
          <Plus size={16} />
        </div>
      </div>
    );
  }
  return (
    <div aria-hidden className="h-12 rounded-sm bg-canvas ring-1 ring-line grid place-items-center text-fg-faint">
      <Music size={16} />
    </div>
  );
}

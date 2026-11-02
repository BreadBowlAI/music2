import { CircleAlert, FolderOpen, LoaderCircle, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { editorHooks, tilde } from "../lib/graph";
import { flushLocal } from "../lib/local";
import { useModalKeys } from "../lib/modal";
import { native, nativeAction } from "../lib/native";
import { songApi, type SaveAsStart } from "../lib/projects";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/** Save As's dialog, while it shows: where it starts (the suggested name and folder). */
const useSaveAs = create<{ start: SaveAsStart | null }>(() => ({ start: null }));

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** File ▸ Save As… (⇧⌘S): ask the server where the copy starts, then show the dialog. */
export async function openSaveAs() {
  if (!useStore.getState().graph) return;
  try {
    useSaveAs.setState({ start: await songApi.saveAsStart() });
  } catch (error) {
    useStore.getState().toast(message(error));
  }
}

export default function SaveAsDialog() {
  const start = useSaveAs((s) => s.start);
  const graph = useStore((s) => s.graph);
  // Another song open meanwhile: the dialog was for the last one.
  useEffect(() => () => useSaveAs.setState({ start: null }), [graph?.session]);
  return start && graph ? <Dialog start={start} onClose={() => useSaveAs.setState({ start: null })} /> : null;
}

/**
 * Save a copy of the open song under a new name (Q19), and open the copy: "Save a copy of Night
 * Drive", the name ("Night Drive 2", selected), the folder it goes in (Choose Folder… in the app),
 * [Cancel] [Save Copy]. The song itself stays as it was. The copy has no `.music2/` (mutes, view,
 * undo). Unsaved code in the editor is said first, as New Song says it: the copy has the saved
 * files. It works in a read-only window too: it only reads the song. The keys are every dialog's
 * (`useModalKeys`): Return saves from the field.
 */
function Dialog({ start, onClose }: { start: SaveAsStart; onClose: () => void }) {
  const graph = useStore((s) => s.graph)!;
  const [name, setName] = useState(start.name);
  const [dir, setDir] = useState(start.dir);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unsaved] = useState(() => editorHooks.unsaved?.() ?? 0);
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => input.current?.select(), []);
  const was = graph.name;

  const save = async () => {
    if (busy) return;
    const wanted = name.trim();
    if (!wanted) {
      setError("The copy needs a name.");
      input.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // The view settings of this song, written before it closes (plan X37).
      await flushLocal();
      const copy = await songApi.saveAs(graph.session, wanted, dir === start.dir ? undefined : dir);
      onClose();
      useStore.getState().set({ graph: copy });
      useStore.getState().toast(`Now working on ${copy.name}. ${was} is kept as it was.`);
    } catch (e) {
      setError(message(e));
      setBusy(false);
      input.current?.focus();
    }
  };
  const choose = async () => {
    try {
      const picked = await nativeAction("song_folder", dir);
      if (typeof picked === "string") setDir(picked);
    } catch (e) {
      setError(message(e));
    }
  };
  useModalKeys(box, { onEscape: () => !busy && onClose(), onDefault: () => void save() });

  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby="save-as-title"
        data-testid="save-as-dialog"
        className="w-[480px] max-w-full rounded-md bg-raised shadow-pop overflow-hidden animate-dialog cursor-default"
      >
        <div className="px-6 pt-6">
          <h2 id="save-as-title" className="text-lg font-semibold text-fg-strong truncate">
            Save a copy of {was}
          </h2>
          <p className="mt-1 text-base text-fg-muted">The copy opens. {was} stays as it was.</p>
        </div>
        <div className="px-6 pt-4 space-y-4">
          <label className="block">
            <span className="text-sm font-medium text-fg-muted">Name</span>
            <input
              ref={input}
              autoFocus
              value={name}
              maxLength={80}
              spellCheck={false}
              aria-label="Name of the copy"
              aria-invalid={!!error}
              data-testid="save-as-name"
              onChange={(e) => setName(e.target.value)}
              className="mt-1.5 w-full h-8 px-3 rounded-sm bg-raised border border-line-strong hover:border-fg-off focus:border-accent text-base text-fg transition-colors duration-80 ease-linear"
            />
          </label>
          {unsaved > 0 && (
            <div className="flex items-start gap-2 px-3 py-2 rounded-sm bg-warn-soft text-sm text-warn-strong">
              <TriangleAlert size={14} className="shrink-0 text-warn mt-px" />
              <span>
                {unsaved === 1 ? "A file has" : `${unsaved} files have`} unsaved edits in the code editor. The copy has the saved files only: the edits stay with this song, which offers them back when it next opens.
              </span>
            </div>
          )}
          {error && (
            <div role="alert" data-testid="save-as-error" className="flex items-start gap-2 px-3 py-2 rounded-sm bg-danger-soft text-sm text-danger-strong">
              <CircleAlert size={14} className="shrink-0 text-danger mt-px" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          )}
        </div>
        <div className="mt-6 px-6 py-3 flex items-center gap-2 border-t border-line bg-chrome">
          <div className="flex-1 min-w-0 flex items-center gap-2 text-sm text-fg-muted" title={dir}>
            <FolderOpen size={14} className="shrink-0" />
            <span className="truncate">
              in{" "}
              <span className="font-medium text-fg" data-testid="save-as-folder">
                {tilde(dir, start.home)}
              </span>
            </span>
          </div>
          {native && (
            <Button size="lg" variant="ghost" onClick={() => void choose()} disabled={busy} title="Pick another folder for the copy">
              Choose Folder…
            </Button>
          )}
          <Button size="lg" variant="secondary" data-testid="save-as-cancel" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button size="lg" variant="primary" data-testid="save-as-save" onClick={() => void save()} disabled={busy} className="min-w-[104px]">
            {busy && <LoaderCircle size={14} className="animate-spin" />}
            {unsaved > 0 ? "Save Copy Anyway" : "Save Copy"}
          </Button>
        </div>
      </div>
    </div>
  );
}

import { CircleAlert, LoaderCircle, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { editorHooks, lockReason } from "../lib/graph";
import { flushLocal } from "../lib/local";
import { useModalKeys } from "../lib/modal";
import { songApi } from "../lib/projects";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/** Whether Rename Song's dialog shows. */
const useRenameSong = create<{ open: boolean }>(() => ({ open: false }));

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * File ▸ Rename Song…: the dialog, unless the song can't be written here now (read only, the code
 * doesn't compile): then the reason, in a toast (X21). The top bar's double-click on the name stays
 * the quick rename of the name only (Q20).
 */
export function openRenameSong() {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const locked = lockReason(graph, connected);
  if (locked) return toast(`Can't rename the song now. ${locked}.`);
  useRenameSong.setState({ open: true });
}

export default function RenameSongDialog() {
  const open = useRenameSong((s) => s.open);
  const graph = useStore((s) => s.graph);
  useEffect(() => () => useRenameSong.setState({ open: false }), [graph?.session]);
  return open && graph ? <Dialog onClose={() => useRenameSong.setState({ open: false })} /> : null;
}

/**
 * Rename the open song (Q20): "Rename Night Drive", the name (selected), and "Also rename its
 * folder" (on). The name is one undo step. With the folder, the song closes and opens again from
 * the renamed folder, so Undo can't go back past it: the dialog says so while the box is on. A
 * folder name that is taken is refused in words, and nothing is changed. Unsaved code in the editor
 * is said first while the box is on, as Save As says it: closing the song loses it, so the button
 * says Rename Anyway (review PL-R1). The keys are every dialog's (`useModalKeys`): Return renames
 * from the field or the box.
 */
function Dialog({ onClose }: { onClose: () => void }) {
  const graph = useStore((s) => s.graph)!;
  const [name, setName] = useState(graph.name);
  const [folder, setFolder] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [unsaved] = useState(() => editorHooks.unsaved?.() ?? 0);
  const losing = folder && unsaved > 0;
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => input.current?.select(), []);
  const was = graph.name;

  const rename = async () => {
    if (busy) return;
    const wanted = name.trim();
    if (!wanted) {
      setError("A song needs a name.");
      input.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // The view settings of this song, written before it closes (plan X37).
      if (folder) await flushLocal();
      const r = await songApi.rename(graph.session, wanted, folder, losing);
      onClose();
      const { toast } = useStore.getState();
      if (r.warning) toast(r.warning);
      else if (folder && wanted !== was) toast(`Renamed ${was} to ${r.name}, and its folder too.`);
    } catch (e) {
      setError(message(e));
      setBusy(false);
      input.current?.focus();
    }
  };
  useModalKeys(box, { onEscape: () => !busy && onClose(), onDefault: () => void rename() });

  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby="rename-song-title"
        data-testid="rename-song-dialog"
        className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default"
      >
        <h2 id="rename-song-title" className="text-lg font-semibold text-fg-strong truncate">
          Rename {was}
        </h2>
        <label className="mt-4 block">
          <span className="text-sm font-medium text-fg-muted">Name</span>
          <input
            ref={input}
            autoFocus
            value={name}
            maxLength={80}
            spellCheck={false}
            aria-label="Song name"
            aria-invalid={!!error}
            data-testid="rename-song-name"
            onChange={(e) => setName(e.target.value)}
            className="mt-1.5 w-full h-8 px-3 rounded-sm bg-raised border border-line-strong hover:border-fg-off focus:border-accent text-base text-fg transition-colors duration-80 ease-linear"
          />
        </label>
        <label className="mt-3 flex items-center gap-2 text-base text-fg cursor-pointer select-none">
          <input
            type="checkbox"
            checked={folder}
            data-testid="rename-song-folder"
            onChange={(e) => setFolder(e.target.checked)}
            className="size-3.5 accent-accent cursor-pointer"
          />
          Also rename its folder
        </label>
        {folder && (
          <p className="mt-1 pl-5.5 text-sm text-fg-muted" data-testid="rename-song-note">
            The song closes and opens again. After that, Undo can't go back past this.
          </p>
        )}
        {losing && (
          <div data-testid="rename-song-unsaved" className="mt-3 flex items-start gap-2 px-3 py-2 rounded-sm bg-warn-soft text-sm text-warn-strong">
            <TriangleAlert size={14} className="shrink-0 text-warn mt-px" />
            <span>
              {unsaved === 1 ? "A file has" : `${unsaved} files have`} unsaved edits in the code editor. Renaming the folder closes the song without
              saving them.
            </span>
          </div>
        )}
        {error && (
          <div role="alert" data-testid="rename-song-error" className="mt-3 flex items-start gap-2 px-3 py-2 rounded-sm bg-danger-soft text-sm text-danger-strong">
            <CircleAlert size={14} className="shrink-0 text-danger mt-px" />
            <span className="min-w-0 break-words">{error}</span>
          </div>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="secondary" size="lg" data-testid="rename-song-cancel" onClick={onClose} disabled={busy}>
            Cancel
          </Button>
          <Button variant="primary" size="lg" data-testid="rename-song-ok" onClick={() => void rename()} disabled={busy} className="min-w-[88px]">
            {busy && <LoaderCircle size={14} className="animate-spin" />}
            {losing ? "Rename Anyway" : "Rename"}
          </Button>
        </div>
      </div>
    </div>
  );
}

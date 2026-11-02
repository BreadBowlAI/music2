import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { exportChosen, stemsRefused, useExportDialog, type ExportChoice } from "../lib/exporting";
import { useModalKeys } from "../lib/modal";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/** The three choices, in plain words (Q42, D34). */
const CHOICES: { id: ExportChoice; name: string; line: string }[] = [
  { id: "song", name: "Song", line: "The whole song as one WAV file" },
  { id: "stems", name: "Stems", line: "Each track as its own WAV file, before its volume and pan" },
  { id: "midi", name: "MIDI", line: "The notes, for other music apps" },
];

export default function ExportDialog() {
  const open = useExportDialog((s) => s.open);
  const graph = useStore((s) => s.graph);
  // Another song opened, or none: the dialog goes.
  useEffect(() => () => useExportDialog.setState({ open: false }), [graph?.session]);
  return open && graph ? <Dialog session={graph.session} onClose={() => useExportDialog.setState({ open: false })} /> : null;
}

/**
 * Export (⇧⌘E, the Export button, File ▸ Export…; studio C3, Q42): Song, Stems or MIDI, each with
 * what it makes in one line. Song is chosen and Export… has the focus, so Return exports the song
 * as before (its question first while mutes or solos change what you hear, then the save panel in
 * the app). A choice is a radio: a click, Space or the arrow keys pick it, and Return from it goes
 * on. Stems are greyed, with why, for a song that plays plugins (Q40). The song's loudness shows
 * when it is known: the last export measured it, and the song hasn't changed since (TAP-7). Esc
 * closes; Tab goes round; no key reaches the page behind (`useModalKeys`).
 */
function Dialog({ session, onClose }: { session: number; onClose: () => void }) {
  const [choice, setChoice] = useState<ExportChoice>("song");
  // The song's loudness, while it is as the last export measured it (TAP-7).
  const loudness = useStore((s) => s.graph?.loudness ?? null);
  // Why Stems can't be made: unknown until the server says, then null when they can.
  const [why, setWhy] = useState<string | null | undefined>(undefined);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let live = true;
    stemsRefused(session).then(
      (w) => live && setWhy(w),
      () => live && setWhy(null),
    );
    return () => {
      live = false;
    };
  }, [session]);
  const off = (id: ExportChoice) => id === "stems" && !!why;
  const go = () => {
    if (!off(choice)) void exportChosen(choice);
  };
  useModalKeys(box, { onEscape: onClose, onDefault: go });
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={box} role="dialog" aria-modal="true" aria-labelledby="export-title" data-testid="export-dialog" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="export-title" className="text-lg font-semibold text-fg-strong">
          Export
        </h2>
        <div role="radiogroup" aria-labelledby="export-title" className="mt-4 flex flex-col gap-2">
          {CHOICES.map((c) => {
            const disabled = off(c.id);
            return (
              <label
                key={c.id}
                data-export-choice={c.id}
                className={clsx(
                  "flex items-start gap-3 px-3 py-2.5 rounded-sm transition-shadow duration-80 ease-linear",
                  disabled ? "ring-1 ring-line cursor-default" : "cursor-pointer",
                  !disabled && (choice === c.id ? "ring-2 ring-accent" : "ring-1 ring-line hover:ring-line-strong"),
                )}
              >
                <input
                  type="radio"
                  name="export-what"
                  value={c.id}
                  checked={choice === c.id}
                  disabled={disabled}
                  onChange={() => setChoice(c.id)}
                  onDoubleClick={go}
                  className="mt-0.5 size-3.5 shrink-0 accent-accent cursor-pointer disabled:cursor-default"
                />
                <span className="min-w-0">
                  <span className={clsx("block text-base font-medium", disabled ? "text-fg-off" : "text-fg")}>{c.name}</span>
                  <span className={clsx("block text-sm", disabled ? "text-fg-off" : "text-fg-muted")}>{c.line}</span>
                  {disabled && (
                    <span className="block mt-0.5 text-sm text-fg-muted" data-testid="export-stems-why">
                      {why}
                    </span>
                  )}
                </span>
              </label>
            );
          })}
        </div>
        {loudness && (
          <p data-testid="export-loudness" className="mt-4 text-sm text-fg-muted" title="Loudness as ears hear it (LUFS). Streaming services play songs at about −14 LUFS.">
            The song's loudness is {loudness.lufs < 0 ? `−${-loudness.lufs}` : loudness.lufs} LUFS, measured when it was last exported.
          </p>
        )}
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="secondary" size="lg" data-testid="export-dialog-cancel" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" size="lg" autoFocus data-testid="export-dialog-go" disabled={off(choice)} onClick={go}>
            Export…
          </Button>
        </div>
      </div>
    </div>
  );
}

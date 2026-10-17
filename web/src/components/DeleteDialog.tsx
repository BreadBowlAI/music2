import { useRef } from "react";
import { useModalKeys } from "../lib/modal";
import { useShell } from "../lib/shell";
import { deleteTrack, what, type Asking } from "./Tracks";
import { Button } from "./ui";

/** "Bass and Chords", from the tracks the reads are in. */
function joined(names: string[]) {
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : (names[0] ?? "");
}

/** What the delete dialog says (design-8b §3.5), in words: who reads the track, and what goes with it. */
function said({ name, dependents, blocked }: Asking) {
  const readers = [...new Set(dependents.map((d) => d.label))];
  const many = readers.length > 1;
  if (blocked) {
    const d = dependents[0];
    return `${joined(readers)} ${many ? "read" : "reads"} it in ${many ? "their" : "its"} code (${d?.file}, line ${d?.line}).`;
  }
  const these = dependents.length === 1 ? "that" : "those";
  const off = what(dependents).replace(/^its /, "");
  if (dependents.every((d) => d.component === "Duck")) return `${joined(readers)} ${many ? "duck" : "ducks"} to ${name}. Deleting ${name} also takes off ${these} ${off}.`;
  return `${joined(readers)} ${many ? "read" : "reads"} ${name} through ${off}. Deleting ${name} also takes ${these} off.`;
}

/**
 * The delete dialog (taste.md §2.14): the ducks a delete takes off too, or why it can't. Its keys
 * are every dialog's (`useModalKeys`): the main button has the focus at first, and Return presses
 * the button with the focus ring, so ⇧Tab to Cancel and Return deletes nothing. Esc cancels.
 */
export default function DeleteDialog({ asking, onClose }: { asking: Asking; onClose: () => void }) {
  const { module, name, dependents, blocked } = asking;
  const confirm = () => {
    onClose();
    if (!blocked) void deleteTrack(module, true);
  };
  const show = () => {
    onClose();
    const d = dependents[0];
    if (d) useShell.getState().select(d.module);
    useShell.getState().setView("code");
  };
  const box = useRef<HTMLDivElement>(null);
  useModalKeys(box, { onEscape: onClose, onDefault: blocked ? onClose : confirm });
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={box} role="alertdialog" aria-modal aria-labelledby="delete-title" data-testid="delete-dialog" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="delete-title" className="text-lg font-semibold text-fg-strong">
          {blocked ? `${name} can't be deleted from here` : `Delete ${name}?`}
        </h2>
        <p className="mt-2 text-base text-fg-muted">{said(asking)}</p>
        <div className="mt-6 flex justify-end gap-2">
          {blocked ? (
            <>
              <Button variant="secondary" size="lg" onClick={show}>
                Show in code
              </Button>
              <Button variant="primary" size="lg" autoFocus onClick={onClose}>
                Close
              </Button>
            </>
          ) : (
            <>
              <Button variant="secondary" size="lg" data-testid="delete-cancel" onClick={onClose}>
                Cancel
              </Button>
              <Button variant="danger" size="lg" autoFocus data-testid="delete-confirm" onClick={confirm}>
                {`Delete ${name} and ${dependents.length === 1 ? `the ${dependents[0].component}` : what(dependents)}`}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { useModalKeys } from "../lib/modal";
import { changeId, checkId, useTrackId } from "../lib/rekey";
import { Button } from "./ui";

/**
 * Change a track's ID (polish A5, design-8b §11.4, taste.md §2.14): opened from the Code view's
 * file list, where IDs are literal code. A mono field holding the ID, checked as you type in ID
 * words ("lead vox isn't an ID: …", "low is taken: …"), and [Cancel] [Change ID]. Return in the
 * field presses Change ID; Tab and ⇧Tab go round the field and the buttons; Esc cancels; no key
 * reaches the page behind (Space would play, M would mute).
 */
export default function TrackIdDialog() {
  const asking = useTrackId((s) => s.asking);
  if (!asking) return null;
  return <Dialog key={asking.module} module={asking.module} name={asking.name} />;
}

function Dialog({ module, name }: { module: string; name: string }) {
  const [id, setId] = useState(module);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const close = () => useTrackId.setState({ asking: null });
  // Checked as you type, a moment after the last key; only the newest answer counts.
  const asked = useRef(0);
  useEffect(() => {
    const n = ++asked.current;
    if (id === module || !id) return setProblem(null);
    const t = setTimeout(() => {
      checkId(module, id).then(
        (why) => n === asked.current && setProblem(why),
        () => {},
      );
    }, 120);
    return () => clearTimeout(t);
  }, [id, module]);
  const ready = !!id && id !== module && !problem && !busy;
  const submit = async () => {
    if (!ready) return;
    setBusy(true);
    const why = await changeId(module, id);
    setBusy(false);
    if (why) setProblem(why);
    else close();
  };
  const submitting = useRef(submit);
  submitting.current = submit;
  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);
  // The keys of every dialog (lib/modal): Return in the field presses Change ID.
  useModalKeys(box, { onEscape: close, onDefault: () => void submitting.current() });
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div ref={box} role="dialog" aria-modal aria-labelledby="track-id-title" data-testid="track-id-dialog" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="track-id-title" className="text-lg font-semibold text-fg-strong">
          {`Change ${name}'s track ID`}
        </h2>
        <div className="mt-4 relative">
          <input
            ref={field}
            data-code
            data-testid="track-id-field"
            aria-label="Track ID"
            aria-invalid={!!problem || undefined}
            aria-describedby="track-id-help"
            value={id}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            onChange={(e) => {
              setId(e.target.value);
              setProblem(null);
            }}
            className={clsx(
              "w-full h-8 px-2 rounded-sm bg-raised border font-mono text-sm text-fg outline-none focus:ring-2 focus:ring-accent-range",
              problem ? "border-danger" : "border-line-strong focus:border-accent",
            )}
          />
          {problem && (
            <p role="alert" data-testid="track-id-problem" className="mt-1 text-xs text-danger">
              {problem}
            </p>
          )}
        </div>
        <p id="track-id-help" className="mt-3 text-sm text-fg-muted">
          {`The track ID names this track in code and files its data (data/${module}/). Changing it moves both, as one change you can undo.`}
        </p>
        <div className="mt-6 flex justify-end gap-2">
          <Button variant="secondary" size="lg" data-testid="track-id-cancel" onClick={close}>
            Cancel
          </Button>
          <Button variant="primary" size="lg" data-testid="track-id-change" disabled={!ready} onClick={() => void submit()}>
            Change ID
          </Button>
        </div>
      </div>
    </div>
  );
}

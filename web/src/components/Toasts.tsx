import { useExport } from "../lib/exporting";
import { useStore } from "../lib/store";

export default function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    // Toasts let the pointer through (only an action button takes it): a knob or a clip under one
    // stays in reach while it fades.
    <div className="fixed right-4 bottom-28 z-[60] flex flex-col gap-2 items-end pointer-events-none">
      <OtherExport />
      {toasts.map((t) => (
        <div key={t.id} className="max-w-sm px-4 py-3 rounded-2xl glass ring-1 ring-black/10 shadow-2xl text-sm text-ink-100 rise flex items-center gap-3">
          <span className="flex-1 min-w-0 line-clamp-3 break-words">{t.text}</span>
          {t.action && (
            <button onClick={t.action.run} className="pointer-events-auto text-pop-400 font-medium cursor-pointer">
              {t.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

/** An export of a song that isn't the open one (it was closed, or another opened): it runs on, and
 * shows here until it's done. The open song's own export shows in its Export button. */
function OtherExport() {
  const { progress, song, dir } = useExport();
  const open = useStore((s) => s.graph?.dir);
  if (progress === null || dir === open) return null;
  const percent = Math.round(progress * 100);
  return (
    <div data-testid="export-elsewhere" className="w-72 px-4 py-3 rounded-2xl glass ring-1 ring-black/10 shadow-2xl text-sm text-ink-100 rise">
      <div className="flex items-baseline gap-2">
        <span className="min-w-0 truncate">
          Exporting <span className="font-medium">“{song}”</span>
        </span>
        <span className="ml-auto text-xs text-ink-400 tabular-nums">{percent}%</span>
      </div>
      <div
        role="progressbar"
        aria-label={`Exporting ${song}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="mt-2 h-1 rounded-full bg-black/[0.06] overflow-hidden"
      >
        <div className="h-full rounded-full pop-gradient transition-[width] duration-150" style={{ width: `${Math.max(progress, 0.02) * 100}%` }} />
      </div>
    </div>
  );
}

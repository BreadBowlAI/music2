import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useExport } from "../lib/exporting";
import { useStore, type Toast } from "../lib/store";

/** Where toasts sit: centred over the arrangement (the element marked `data-toast-area`), 16px above its foot, so above the dock while it is open. */
function useAnchor() {
  const graph = useStore((s) => s.graph);
  const [at, setAt] = useState<{ x: number; bottom: number } | null>(null);
  useLayoutEffect(() => {
    const area = document.querySelector<HTMLElement>("[data-toast-area]");
    if (!area) return setAt(null);
    const measure = () => {
      const r = area.getBoundingClientRect();
      setAt({ x: r.left + r.width / 2, bottom: window.innerHeight - r.bottom + 16 });
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(area);
    window.addEventListener("resize", measure);
    return () => {
      watch.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [graph?.session, graph === null]);
  return at;
}

export default function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const at = useAnchor();
  return (
    // Toasts let the pointer through (only a toast itself takes it): a knob or a clip beside one
    // stays in reach while it shows.
    <div
      // Over the layout, under menus and popovers (z-50) and dialogs.
      className="fixed z-[45] flex flex-col gap-2 items-center pointer-events-none -translate-x-1/2"
      style={at ? { left: at.x, bottom: at.bottom } : { left: "50%", bottom: 24 }}
    >
      <OtherExport />
      {toasts.map((t) => (
        <ToastView key={t.id} toast={t} />
      ))}
    </div>
  );
}

/** One toast: 4 s, or 6 s with an action; hovering it holds it. A newer one replaces it. */
function ToastView({ toast }: { toast: Toast }) {
  const dismiss = useStore((s) => s.dismiss);
  const [held, setHeld] = useState(false);
  const left = useRef(toast.action ? 6000 : 4000);
  useEffect(() => {
    if (held) return;
    const from = performance.now();
    const timer = setTimeout(() => dismiss(toast.id), left.current);
    return () => {
      clearTimeout(timer);
      left.current -= performance.now() - from;
    };
  }, [held, toast.id, dismiss]);
  return (
    <div
      data-testid="toast"
      role="status"
      onPointerEnter={() => setHeld(true)}
      onPointerLeave={() => setHeld(false)}
      // Written to be one line; a refusal that is longer wraps rather than being cut, so its reason always shows.
      className="pointer-events-auto max-w-[440px] min-h-ctl px-3 py-1.5 rounded-md bg-raised shadow-pop text-base text-fg animate-rise flex items-center gap-4"
    >
      <span className="min-w-0 break-words">{toast.text}</span>
      {toast.action && (
        <button onClick={toast.action.run} className="shrink-0 text-accent hover:text-accent-strong font-medium cursor-pointer transition-colors duration-80 ease-linear">
          {toast.action.label}
        </button>
      )}
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
    <div data-testid="export-elsewhere" className="w-72 px-3 py-2 rounded-md bg-raised shadow-pop text-base text-fg animate-rise">
      <div className="flex items-baseline gap-2">
        <span className="min-w-0 truncate">
          Exporting <span className="font-medium">“{song}”</span>
        </span>
        <span className="ml-auto text-xs text-fg-muted">{percent}%</span>
      </div>
      <div
        role="progressbar"
        aria-label={`Exporting ${song}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="mt-2 h-1 rounded-full bg-line overflow-hidden"
      >
        <div className="h-full rounded-full bg-accent transition-[width] duration-140" style={{ width: `${Math.max(progress, 0.02) * 100}%` }} />
      </div>
    </div>
  );
}

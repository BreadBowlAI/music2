import { useStore } from "../lib/store";

export default function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="fixed right-4 bottom-28 z-[60] flex flex-col gap-2 items-end pointer-events-none">
      {toasts.map((t) => (
        <div key={t.id} className="pointer-events-auto max-w-sm px-4 py-3 rounded-2xl glass ring-1 ring-black/10 shadow-2xl text-sm text-ink-100 rise flex items-center gap-3">
          <span className="flex-1">{t.text}</span>
          {t.action && (
            <button onClick={t.action.run} className="text-pop-400 font-medium cursor-pointer">
              {t.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

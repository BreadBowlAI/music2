import { useEffect, useRef } from "react";
import { frames, measureFrames, useFrameMeter } from "../lib/frames";
import { BAR_H, RULER_H } from "../lib/layout";

/** The views the meter counts, in the window's order: each one's name and words, and its rows'. */
const VIEWS = [
  ["arrangement", "Timeline", "arrangement.rows", "rows"],
  ["graph", "Graph", "graph.nodes", "nodes"],
  ["notes", "Notes", "notes.keys", "keys"],
  ["beat", "Beat", "beat.rows", "rows"],
] as const;

/** How often its numbers change. */
const EVERY = 250;

/**
 * The frame meter (taste.md VIEW-10, the command palette's Show frame rate): frames a second, the
 * slowest frame and each view's redraws in the last second, at the window's right under the ruler,
 * taking no clicks. React draws it once; its numbers are written into its text four times a second,
 * so its updates redraw nothing, itself included.
 */
export default function FrameMeter() {
  const on = useFrameMeter((s) => s.on);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!on) return;
    const stop = measureFrames();
    const show = () => {
      const el = box.current;
      if (!el) return;
      const f = frames();
      const put = (key: string, value: string) => {
        const n = el.querySelector(`[data-n="${key}"]`);
        if (n && n.textContent !== value) n.textContent = value;
      };
      put("fps", String(f.fps ?? 0));
      put("slowest", String(f.slowest ?? 0));
      for (const [view, , rows] of VIEWS) {
        put(view, String(f.renders[view] ?? 0));
        put(rows, String(f.renders[rows] ?? 0));
      }
      el.dataset.fps = String(f.fps ?? 0);
      el.dataset.slowest = String(f.slowest ?? 0);
      el.dataset.at = String(Math.round(performance.now()));
    };
    show();
    const t = window.setInterval(show, EVERY);
    return () => {
      window.clearInterval(t);
      stop();
    };
  }, [on]);
  if (!on) return null;
  return (
    <div
      ref={box}
      data-testid="frame-meter"
      aria-live="off"
      className="fixed right-3 z-[45] w-[188px] px-3 py-2 rounded-md bg-raised shadow-pop pointer-events-none text-xs text-fg-muted tabular-nums"
      style={{ top: BAR_H + RULER_H + 8 }}
    >
      <div className="text-sm text-fg">
        <span data-n="fps" className="font-semibold text-fg-strong">
          0
        </span>{" "}
        frames a second
      </div>
      <div>
        Slowest frame <span data-n="slowest" className="text-fg">0</span> ms
      </div>
      <div className="mt-1.5 mb-0.5">Redrawn in the last second</div>
      {VIEWS.map(([view, name, rows, words]) => (
        <div key={view} className="flex gap-1">
          <span className="w-14 text-fg">{name}</span>
          <span data-n={view} className="w-8 text-right text-fg">
            0
          </span>
          <span className="flex-1 text-right">
            {words} <span data-n={rows}>0</span>
          </span>
        </div>
      ))}
    </div>
  );
}

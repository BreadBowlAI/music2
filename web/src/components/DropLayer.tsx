// The drop hint (P0.4, built in lane A): while files are dragged over the window, a 2 px accent
// outline where they'd land (the lane's row from the bar for a bar, or a dashed box for a new
// track) and a bubble at the pointer saying what happens (taste.md §2.14). Gone on leave or drop.

import { useEffect } from "react";
import { createPortal } from "react-dom";
import { barAt } from "../lib/graph";
import { listenForDrops, useDropHint, type DropTarget } from "../lib/drop";
import { ROW_H } from "../lib/layout";
import { useStore } from "../lib/store";
import { zoomNow } from "../lib/zoom";

/** Where `t` lands on screen, and whether it makes something new (dashed). */
function outline(t: DropTarget): { left: number; top: number; width: number; height: number; dashed: boolean } | null {
  if (t.kind === "sampler") {
    const card = document.querySelector<HTMLElement>(`[data-sampler-call="${CSS.escape(t.call)}"]`)?.getBoundingClientRect();
    return card ? { left: card.left, top: card.top, width: card.width, height: card.height, dashed: false } : null;
  }
  const ruler = document.querySelector<HTMLElement>("[data-testid=ruler]")?.getBoundingClientRect();
  const graph = useStore.getState().graph;
  if (t.kind === "none" || !ruler || !graph) return null;
  const ppb = zoomNow();
  const bar = barAt(graph.timeline.meter, t.beat);
  const left = ruler.left + t.beat * ppb;
  const width = Math.max(8, bar.length * ppb);
  if (t.kind === "lane") {
    const row = document.querySelector<HTMLElement>(`[data-lane="${CSS.escape(`${t.module}.${t.lane}`)}"]`)?.getBoundingClientRect();
    return row ? { left, top: row.top, width, height: row.height, dashed: false } : null;
  }
  if (t.kind === "track") {
    const track = document.querySelector<HTMLElement>(`[data-track="${CSS.escape(t.module)}"]`)?.getBoundingClientRect();
    return track ? { left, top: track.bottom - 4, width, height: 8, dashed: true } : null;
  }
  const tracks = [...document.querySelectorAll<HTMLElement>("[data-track]")].filter((e) => e.dataset.track !== "song");
  const last = tracks.at(-1)?.getBoundingClientRect();
  const top = (last?.bottom ?? ruler.bottom) + 4;
  return { left, top, width, height: ROW_H - 8, dashed: true };
}

export default function DropLayer() {
  const hint = useDropHint((s) => s.hint);
  useEffect(() => listenForDrops(), []);
  if (!hint) return null;
  const shown = hint.shown ?? hint.target;
  const box = outline(shown);
  return createPortal(
    <>
      {box && (
        <div
          aria-hidden
          data-testid="drop-outline"
          data-kind={shown.kind}
          className={`fixed z-40 pointer-events-none rounded-sm border-2 border-accent ${box.dashed ? "border-dashed" : ""}`}
          style={{ left: box.left, top: box.top, width: box.width, height: box.height }}
        />
      )}
      <div
        role="status"
        data-testid="drop-hint"
        className="fixed z-50 pointer-events-none -translate-x-1/2 -translate-y-full px-1.5 py-0.5 rounded-sm bg-fg-strong/92 text-white text-xs whitespace-nowrap"
        style={{ left: hint.at.x, top: hint.at.y - 12 }}
      >
        {hint.words}
      </div>
    </>,
    document.body,
  );
}

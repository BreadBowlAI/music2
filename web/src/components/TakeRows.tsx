// The Takes rows (VOX-7): under an audio lane whose clips play a loop's takes, one row per take,
// "Take 1", "Take 2"…, while its track's Takes is open. Each row draws the whole take over each comp
// of the lane, faint, with the parts that play lit in the track's tint. A swipe along a row (press,
// drag, let go) plays that take over the beats swiped, snapped to the grid unless ⌘ (Ctrl) is held;
// a click on a faint part plays that take over the piece there. Each is one undo step, "Comp on
// Vox": `POST /api/graph/take` with `from` and `to` (VOX-1). Nothing here is saved: the rows are
// worked out from the clips and their file's note (`lib/takes`).

import clsx from "clsx";
import { useState } from "react";
import { serverRef } from "../lib/clips";
import { clipRef, findLane, type TimelineLane } from "../lib/graph";
import { skipsSnap, snapSong } from "../lib/grid";
import { editing, goesFrom, lanesOf } from "../lib/history";
import type { TrackColor } from "../lib/palette";
import { request } from "../lib/projects";
import { useStore } from "../lib/store";
import { compsOf, takeClip, type Comp } from "../lib/takes";
import { useZoom } from "../lib/zoom";
import { Wave } from "./Wave";

/** A press that moves less than this, in px, is a click. */
const CLICK_PX = 4;
const NO_TEMPO: never[] = [];

/** Take `take` (from 0) plays from beat `from` to `to` of the comp clip `index` of `lane` is in: one undo step. */
async function compTake(module: string, lane: TimelineLane, index: number, take: number, from: number, to: number) {
  const { graph, toast } = useStore.getState();
  if (!graph) return;
  const ref = clipRef(module, lane.key, lane.clips, index);
  const ours = editing();
  try {
    const f = await goesFrom(graph.session, graph.view.revision, [lanesOf(module)], ours, true);
    const drawn = f.shown ? findLane(f.graph.timeline, module, lane.key) : lane;
    const clip = drawn && serverRef(drawn, ref);
    if (clip) await request("/api/graph/take", { session: graph.session, ...clip, take: take + 1, from, to, revision: f.base });
  } catch (error) {
    toast(error instanceof Error ? error.message.replace(/^edit rejected: /, "") : String(error));
  } finally {
    ours.end();
  }
}

/** The rows of `lane`'s takes, one per take of its comps, or nothing when it has none. */
export function TakeRows({
  module,
  lane,
  color,
  selected,
  headerW,
  width,
  height,
  editable,
}: {
  module: string;
  lane: TimelineLane;
  color: TrackColor;
  selected: boolean;
  headerW: number;
  width: number;
  /** Each row's height: the track's (taste.md §3.2). */
  height: number;
  editable: boolean;
}) {
  const tempo = useStore((s) => s.graph?.timeline.tempo) ?? NO_TEMPO;
  const comps = compsOf(lane, tempo);
  const count = Math.max(0, ...comps.map((c) => c.takes.length));
  return (
    <>
      {Array.from({ length: count }, (_, take) => (
        <TakeRow key={take} module={module} lane={lane} comps={comps} take={take} color={color} selected={selected} headerW={headerW} width={width} height={height} editable={editable} />
      ))}
    </>
  );
}

function TakeRow({
  module,
  lane,
  comps,
  take,
  color,
  selected,
  headerW,
  width,
  height,
  editable,
}: {
  module: string;
  lane: TimelineLane;
  comps: Comp[];
  take: number;
  color: TrackColor;
  selected: boolean;
  headerW: number;
  width: number;
  height: number;
  editable: boolean;
}) {
  const ppb = useZoom();
  // A swipe on its way: the beats it covers so far.
  const [swipe, setSwipe] = useState<[number, number] | null>(null);
  const meter = () => useStore.getState().graph?.timeline.meter ?? [];

  /** A press on the row: a swipe until let go, or a click. */
  const down = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || !editable) return;
    const box = e.currentTarget.getBoundingClientRect();
    const x0 = e.clientX;
    const raw = (x: number) => (x - box.left) / ppb;
    const at = (ev: { clientX: number; metaKey: boolean; ctrlKey: boolean }) => snapSong(meter(), raw(ev.clientX), ppb, skipsSnap(ev));
    const b0 = at(e);
    const el = e.currentTarget;
    const pointer = e.pointerId;
    el.setPointerCapture(pointer);
    let b1 = b0;
    let moved = false;
    const move = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      if (Math.abs(ev.clientX - x0) >= CLICK_PX) moved = true;
      b1 = at(ev);
      if (moved) setSwipe([Math.min(b0, b1), Math.max(b0, b1)]);
    };
    const up = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", cancel);
      setSwipe(null);
      if (!moved) {
        // A click: this take over the piece under it, where another take plays.
        const beat = raw(x0);
        const comp = comps.find((c) => c.start <= beat && beat < c.end && take < c.takes.length);
        const part = comp?.parts.find((p) => p.lo <= beat && beat < p.hi);
        if (comp && part && part.take !== take) void compTake(module, lane, comp.parts[0].index, take, part.lo, part.hi);
        return;
      }
      const [from, to] = [Math.min(b0, b1), Math.max(b0, b1)];
      // Across two comps, the one it starts in (VOX-7).
      const mine = (c: Comp) => take < c.takes.length;
      const comp = comps.find((c) => mine(c) && c.start <= b0 && b0 < c.end) ?? comps.find((c) => mine(c) && c.start < to && from < c.end);
      if (comp && to > from) void compTake(module, lane, comp.parts[0].index, take, from, to);
    };
    const cancel = (ev: PointerEvent) => {
      if (ev.pointerId !== pointer) return;
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", cancel);
      setSwipe(null);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", cancel);
  };

  const name = `Take ${take + 1}`;
  return (
    <div data-testid="take-row" data-lane={`${module}.${lane.key}`} data-take={take + 1} className="flex" style={{ height }}>
      <div
        className={clsx("sticky left-0 z-20 shrink-0 flex items-center border-r border-line text-xs text-fg-muted", selected ? "bg-accent-soft" : "bg-chrome")}
        style={{ width: headerW }}
      >
        <div className="absolute left-0 inset-y-0 w-1" style={{ background: color.bar, opacity: 0.4 }} />
        <span className="pl-7 truncate">{name}</span>
      </div>
      <div
        className={clsx("relative overflow-hidden border-t border-hover touch-none", editable && "cursor-pointer")}
        style={{ width }}
        title={editable ? `Swipe along ${name} to play it there, or click a faint part` : undefined}
        onPointerDown={down}
      >
        {comps
          .filter((c) => take < c.takes.length)
          .map((c) => (
            <div
              key={`${c.asset}@${c.start}`}
              data-comp={c.asset}
              className="absolute top-1 bottom-1 rounded-sm overflow-hidden border"
              style={{ left: c.start * ppb + 1, width: Math.max((c.end - c.start) * ppb - 2, 4), borderColor: color.bar }}
            >
              {c.parts
                .filter((p) => p.take === take)
                .map((p) => (
                  <div
                    key={p.index}
                    data-lit={`${p.lo}-${p.hi}`}
                    className="absolute inset-y-0"
                    style={{ left: (p.lo - c.start) * ppb, width: (p.hi - p.lo) * ppb, background: color.tint }}
                  />
                ))}
              <Wave clip={takeClip(lane, c, take)} color={color} bare />
              {/* The parts another take plays: faint. */}
              {c.parts
                .filter((p) => p.take !== take)
                .map((p) => (
                  <div key={p.index} data-dim className="absolute inset-y-0 bg-canvas/60" style={{ left: (p.lo - c.start) * ppb, width: (p.hi - p.lo) * ppb }} />
                ))}
            </div>
          ))}
        {swipe && (
          <div
            data-testid="take-swipe"
            className="absolute inset-y-0 bg-accent-range/60 border-x border-accent pointer-events-none"
            style={{ left: swipe[0] * ppb, width: Math.max((swipe[1] - swipe[0]) * ppb, 1) }}
          />
        )}
      </div>
    </div>
  );
}

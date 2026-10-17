// The line under a track's automation lanes while some are hidden (the polish plan's D4): "1
// hidden lane · Show", `text-xs` `fg-muted`, Show a link button that brings back every hidden lane
// of the track. Shown only while the track's lanes are (X44).

import clsx from "clsx";
import { show } from "../lib/hiddenLanes";
import type { TrackColor } from "../lib/palette";
import { keepFocus } from "./ui";

/** The line's height, in px: counted where the timeline sums its rows. */
export const HIDDEN_ROW_H = 20;

export function HiddenRow({
  module,
  lanes,
  color,
  selected,
  headerW,
  width,
}: {
  module: string;
  /** The keys of the track's hidden lanes. */
  lanes: readonly string[];
  color: TrackColor;
  selected: boolean;
  headerW: number;
  width: number;
}) {
  const n = lanes.length;
  return (
    <div data-testid="hidden-lanes" data-module={module} className="flex" style={{ height: HIDDEN_ROW_H }}>
      <div
        className={clsx("sticky left-0 z-20 shrink-0 flex items-center border-r border-line text-xs text-fg-muted", selected ? "bg-accent-soft" : "bg-chrome")}
        style={{ width: headerW }}
      >
        <div className="absolute left-0 inset-y-0 w-1" style={{ background: color.bar, opacity: 0.4 }} />
        <span className="pl-7 truncate">
          {n} hidden {n === 1 ? "lane" : "lanes"} ·{" "}
          <button
            data-testid="show-hidden"
            onMouseDown={keepFocus}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => show(module)}
            title={`Show ${n === 1 ? "it" : `all ${n}`} again`}
            className="font-medium text-accent hover:text-accent-strong cursor-pointer"
          >
            Show
          </button>
        </span>
      </div>
      <div className="border-t border-hover" style={{ width }} />
    </div>
  );
}

// The timeline's right-click menus (design-8b §7.5, KN-3): on a clip, and on a lane's empty space.
// One popover style (taste.md §2.14), opened where the pointer is, its rows the commands with
// their keys. A row that can't run now is greyed, its tooltip saying why.

import { Check } from "lucide-react";
import { useState } from "react";
import { createPortal } from "react-dom";
import { playhead, removeAutomation, type LaneAddr } from "../lib/clips";
import { findLane, laneTitle } from "../lib/graph";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { keepFocus, MenuRow, Popover } from "./ui";

/**
 * Remove automation (HOME-2), from the lane's header or the input it drives: the value it plays at
 * the playhead goes back into the code, and its file goes. A toast says so, with Undo.
 */
export function unautomate(addr: LaneAddr) {
  const { graph, toast } = useStore.getState();
  if (!graph) return;
  const lane = findLane(graph.timeline, addr.module, addr.lane);
  const name = lane ? laneTitle(lane) : addr.lane;
  removeAutomation(graph, addr, playhead(), name).then(
    (done) => toast(`Removed the ${name} automation`, undoAction(graph.session, done.revision)),
    (error) => toast(error instanceof Error ? error.message : String(error)),
  );
}

/** A row of the menu. In a menu's list, "line" is a line between groups, and null a row left out. */
export interface MenuItem {
  id: string;
  label: string;
  /** A row that is on or off (Reverse): a check while on. */
  checked?: boolean;
  shortcut?: string;
  /** Why it can't run now: the row is greyed and its tooltip says this. */
  off?: string | null;
  run: () => void;
}

/** A menu at a point on screen: the pointer's, where the right-click was. */
export default function PointMenu({
  at,
  label,
  items,
  onClose,
}: {
  at: { x: number; y: number };
  label: string;
  items: (MenuItem | "line" | null)[];
  onClose: () => void;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  // No line first or last, nor two in a row, when rows are left out.
  const kept = items.filter((it) => it !== null);
  const rows = kept.filter((it, i) => it !== "line" || (i > 0 && kept[i - 1] !== "line" && kept.slice(i + 1).some((x) => x !== "line")));
  // A menu with a row that can be checked gives every row the check's room, so the words line up.
  const checks = rows.some((it) => it !== "line" && it.checked !== undefined);
  return (
    <>
      {createPortal(<span ref={setAnchor} aria-hidden className="fixed size-px pointer-events-none" style={{ left: at.x, top: at.y - 4 }} />, document.body)}
      {anchor && (
        <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={label} data-testid="clip-menu" className="w-[240px]">
          {rows.map((it, i) =>
            it !== "line" ? (
              <MenuRow
                key={it.id}
                data-menu-item={it.id}
                {...(it.checked !== undefined ? { role: "menuitemcheckbox", "aria-checked": it.checked } : {})}
                icon={checks ? <Check size={14} className={it.checked ? undefined : "invisible"} /> : undefined}
                shortcut={it.shortcut}
                disabled={!!it.off}
                title={it.off ?? undefined}
                // The timeline keeps the keyboard: Q and the arrows still work after a row.
                onMouseDown={keepFocus}
                onClick={() => {
                  onClose();
                  it.run();
                }}
              >
                {it.label}
              </MenuRow>
            ) : (
              <div key={`line${i}`} role="separator" className="my-1 h-px bg-line" />
            ),
          )}
        </Popover>
      )}
    </>
  );
}

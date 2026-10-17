// The timeline's right-click menus (design-8b §7.5, KN-3): on a clip, and on a lane's empty space.
// One popover style (taste.md §2.14), opened where the pointer is, its rows the commands with
// their keys. A row that can't run now is greyed, its tooltip saying why.

import { useState } from "react";
import { createPortal } from "react-dom";
import { keepFocus, MenuRow, Popover } from "./ui";

/** A row of the menu. In a menu's list, "line" is a line between groups, and null a row left out. */
export interface MenuItem {
  id: string;
  label: string;
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

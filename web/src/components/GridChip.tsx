import clsx from "clsx";
import { Check, ChevronDown, Magnet } from "lucide-react";
import { useState } from "react";
import { GRIDS, useGrid, type GridView } from "../lib/grid";
import { Button, keepFocus, MenuRow, Popover } from "./ui";

/** Each view's words and test ids: the roll's are the ones it had. */
const VIEWS: Record<GridView, { grid: string; snap: string; about: string; on: string; off: string }> = {
  notes: {
    grid: "grid",
    snap: "snap",
    about: "The grid: snapped notes land on it, and new notes are a step long",
    on: "Snap to the grid: on. Notes you add or drag land on it",
    off: "Snap to the grid: off. Notes go where you put them",
  },
  song: {
    grid: "song-grid",
    snap: "song-snap",
    about: "The grid: clips you drag land on it",
    on: "Snap to the grid: on. Clips you drag land on it",
    off: "Snap to the grid: off. Clips go where you put them",
  },
};

/**
 * A view's grid (taste.md §3.5): `1/16 ▾`, a menu of its steps with the current one checked, and
 * the snap toggle (the magnet). Neither takes focus from a click, so the keys stay the view's.
 */
export default function GridChip({ view }: { view: GridView }) {
  const grid = useGrid((s) => s[view]);
  const words = VIEWS[view];
  const set = (g: Partial<typeof grid>) => useGrid.getState().set(view, { ...grid, ...g });
  return (
    <div className="shrink-0 flex items-center gap-0.5">
      <GridMenu step={grid.step} testid={words.grid} title={words.about} onPick={(step) => set({ step })} />
      <Button
        size="sm"
        data-testid={words.snap}
        active={grid.on}
        aria-pressed={grid.on}
        onMouseDown={keepFocus}
        onClick={() => set({ on: !grid.on })}
        title={grid.on ? words.on : words.off}
        aria-label="Snap to the grid"
        className="w-ctl-sm !px-0"
      >
        <Magnet size={16} />
      </Button>
    </div>
  );
}

function GridMenu({ step, testid, title, onPick }: { step: number; testid: string; title: string; onPick: (step: number) => void }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const label = GRIDS.find((g) => g.step === step)?.label ?? "1/16";
  return (
    <>
      <Button
        size="sm"
        data-testid={testid}
        data-grid={label}
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        onMouseDown={keepFocus}
        onClick={(e) => {
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
        title={title}
        className={clsx("!px-1.5 !gap-1 text-fg", anchor && "bg-hover")}
      >
        {label}
        <ChevronDown size={12} className="text-fg-muted" />
      </Button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} role="menu" aria-label="Grid" className="w-[220px]">
          {GRIDS.map((g) => (
            <MenuRow
              key={g.label}
              role="menuitemradio"
              data-grid={g.label}
              aria-checked={g.step === step}
              icon={g.step === step ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />}
              onClick={() => {
                setAnchor(null);
                onPick(g.step);
              }}
            >
              {g.label}
            </MenuRow>
          ))}
        </Popover>
      )}
    </>
  );
}

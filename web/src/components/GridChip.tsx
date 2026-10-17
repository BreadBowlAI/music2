import clsx from "clsx";
import { Check, ChevronDown, Magnet } from "lucide-react";
import { useState } from "react";
import { autoName, PICKS, pickName, useGrid, type Grid, type GridView } from "../lib/grid";
import { chord, mod } from "../lib/native";
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
    on: `Snap to the grid: on. Clips you drag land on it; hold ${mod.cmd} while you drag to place one freely`,
    off: "Snap to the grid: off. Clips go where you put them",
  },
};

/**
 * A view's grid (taste.md §3.5, design-8b §7.3): `Auto ▾` or `1/16 ▾`, a menu of its choices with
 * the current one checked and Triplets, and the snap toggle (the magnet). Neither takes focus from
 * a click, so the keys stay the view's. `ppb`: the view's zoom, which says what Auto is now.
 */
export default function GridChip({ view, ppb }: { view: GridView; ppb?: number }) {
  const grid = useGrid((s) => s[view]);
  const words = VIEWS[view];
  const set = (g: Partial<Grid>) => useGrid.getState().set(view, { ...grid, ...g });
  const auto = grid.pick === "auto" && ppb ? ` Auto is ${autoName(grid, ppb)} at this zoom.` : "";
  return (
    <div className="shrink-0 flex items-center gap-0.5">
      <GridMenu
        view={view}
        grid={grid}
        testid={words.grid}
        title={`${words.about}.${auto}\nNarrower ${chord("1")} · wider ${chord("2")} · triplets ${chord("3")}`}
        onPick={(g) => set(g)}
      />
      <Button
        size="sm"
        data-testid={words.snap}
        active={grid.on}
        aria-pressed={grid.on}
        onMouseDown={keepFocus}
        onClick={() => set({ on: !grid.on })}
        title={`${grid.on ? words.on : words.off} (${chord("4")})`}
        aria-label="Snap to the grid"
        className="w-ctl-sm !px-0"
      >
        <Magnet size={16} />
      </Button>
    </div>
  );
}

function GridMenu({ view, grid, testid, title, onPick }: { view: GridView; grid: Grid; testid: string; title: string; onPick: (g: Partial<Grid>) => void }) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const label = pickName(grid.pick, grid.triplet);
  const tick = (on: boolean) => (on ? <Check size={14} className="text-fg" /> : <span className="block size-3.5" />);
  const pick = (g: Partial<Grid>) => {
    setAnchor(null);
    onPick(g);
  };
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
          {PICKS[view].map((p) => (
            <MenuRow key={p} role="menuitemradio" data-grid={pickName(p)} aria-checked={p === grid.pick} icon={tick(p === grid.pick)} onClick={() => pick({ pick: p })}>
              {pickName(p)}
            </MenuRow>
          ))}
          <div role="separator" className="my-1 h-px bg-line" />
          <MenuRow
            role="menuitemcheckbox"
            data-grid="triplet"
            aria-checked={grid.triplet}
            icon={tick(grid.triplet)}
            shortcut={chord("3")}
            onClick={() => pick({ triplet: !grid.triplet })}
          >
            Triplets
          </MenuRow>
        </Popover>
      )}
    </>
  );
}

// The roll's right-click menu (D1): what it acts on, then the note tools with their keys. One
// popover style (taste.md §2.14), opened where the pointer is. A row that can't run now is greyed,
// its tooltip saying why. The roll keeps the keyboard: Esc closes the menu and the keys still work.

import { useState } from "react";
import { createPortal } from "react-dom";
import { chord, keyName, shifted } from "../lib/native";
import type { Swing } from "../lib/noteTools";
import { keepFocus, MenuRow, Popover, Segment } from "./ui";

/** What a row of the menu does. */
export type NoteTool =
  | { kind: "transpose"; by: number }
  | { kind: "quantize" }
  | { kind: "legato" }
  | { kind: "humanize" }
  | { kind: "swing"; amount: Swing }
  | { kind: "duplicate" }
  | { kind: "delete" };

/** The tooltips that say what each tool does, in plain words. */
export const TOOL_HELP = {
  quantize: "Move the notes onto the grid",
  legato: "Make each note last until the next one starts",
  humanize: "Change timing and loudness a little, so it sounds played",
  swing: "Push every second note a little later, for a bouncy feel",
} as const;

const SWING_NAMES: [Swing, string][] = [
  ["light", "Light"],
  ["medium", "Medium"],
  ["heavy", "Heavy"],
];

export default function NoteMenu({
  at,
  count,
  all,
  grid,
  off,
  onRun,
  onClose,
}: {
  /** Where the right-click was. */
  at: { x: number; y: number };
  /** How many notes the tools act on. */
  count: number;
  /** Every note of the clip (none was selected), or the selected ones. */
  all: boolean;
  /** The roll's grid, as its chip names it: "1/16". */
  grid: string;
  /** Why the tools can't run now (read only), or null. */
  off: string | null;
  onRun: (tool: NoteTool) => void;
  onClose: () => void;
}) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const why = off ?? (count === 0 ? "This clip has no notes yet: double-click in the grid to add one" : null);
  const run = (tool: NoteTool) => {
    onClose();
    onRun(tool);
  };
  // The arrow keys move only selected notes (X28): with none selected, the rows act on every note, and show no keys.
  const keys = !all;
  const heading = count === 0 ? "No notes" : all ? `All ${count} ${count === 1 ? "note" : "notes"}` : `${count} ${count === 1 ? "note" : "notes"}`;
  const row = (id: string, label: string, tool: NoteTool, shortcut?: string, title?: string) => (
    <MenuRow
      key={id}
      data-menu-item={id}
      shortcut={shortcut}
      disabled={!!why}
      title={why ?? title}
      onMouseDown={keepFocus}
      onClick={() => run(tool)}
    >
      {label}
    </MenuRow>
  );
  const line = (k: string) => <div key={k} role="separator" className="my-1 h-px bg-line" />;
  return (
    <>
      {createPortal(<span ref={setAnchor} aria-hidden className="fixed size-px pointer-events-none" style={{ left: at.x, top: at.y - 4 }} />, document.body)}
      {anchor && (
        <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={`Note tools: ${heading}`} data-testid="note-menu" className="w-[240px]">
          <div data-testid="note-menu-heading" className="px-2 pt-1 pb-1.5 text-xs font-medium text-fg-muted">
            {heading}
          </div>
          {row("up", "Up a semitone", { kind: "transpose", by: 1 }, keys ? "↑" : undefined)}
          {row("down", "Down a semitone", { kind: "transpose", by: -1 }, keys ? "↓" : undefined)}
          {row("octave_up", "Up an octave", { kind: "transpose", by: 12 }, keys ? shifted("↑") : undefined)}
          {row("octave_down", "Down an octave", { kind: "transpose", by: -12 }, keys ? shifted("↓") : undefined)}
          {line("l1")}
          {row("quantize", `Quantize to ${grid}`, { kind: "quantize" }, "Q", TOOL_HELP.quantize)}
          {row("legato", "Legato", { kind: "legato" }, undefined, TOOL_HELP.legato)}
          {row("humanize", "Humanize", { kind: "humanize" }, undefined, TOOL_HELP.humanize)}
          <div role="group" aria-label="Swing" data-menu-item="swing" title={why ?? TOOL_HELP.swing} className="min-h-ctl pl-2 pr-1 flex items-center gap-2 text-sm">
            <span className={why ? "flex-1 text-fg-off" : "flex-1 text-fg"}>Swing</span>
            <div className="h-ctl-sm flex items-center gap-0.5 p-0.5 rounded-sm bg-hover">
              {SWING_NAMES.map(([amount, name]) => (
                <Segment key={amount} role="menuitem" on={false} data-swing={amount} disabled={!!why} onClick={() => run({ kind: "swing", amount })}>
                  {name}
                </Segment>
              ))}
            </div>
          </div>
          {line("l2")}
          {row("duplicate", "Duplicate", { kind: "duplicate" }, keys ? chord("D") : undefined)}
          {row("delete", "Delete", { kind: "delete" }, keys ? keyName("Backspace") : undefined)}
        </Popover>
      )}
    </>
  );
}

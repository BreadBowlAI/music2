import { useEffect } from "react";
import { chord, keyName, mod, native, opt, shifted } from "../lib/native";
import { useShell } from "../lib/shell";
import { Button } from "./ui";

/**
 * The keys that work now, grouped as taste.md §5.1 groups them. Only these: a key that does
 * nothing yet isn't listed (principle 7), and each group grows as its features land.
 */
const GROUPS: { name: string; keys: [string, string][] }[] = [
  {
    name: "Transport",
    keys: [
      ["Play / stop", "Space"],
      ["Go to start", keyName("Enter")],
      ["Loop on / off", "C"],
    ],
  },
  {
    name: "Edit",
    keys: [
      ["Undo / redo", `${chord("Z")} / ${chord("Z", { shift: true })}`],
      ["Delete the selected clip or notes", keyName("Backspace")],
      ["Duplicate the selected notes or track", chord("D")],
      ["Select all notes", chord("A")],
      ["Deselect", "Esc"],
      ["Draw mode in Notes", "B"],
      ["Move notes by the grid / by a bar", `← → / ${shifted("←")} ${shifted("→")}`],
      ["Transpose a semitone / an octave", `↑ ↓ / ${shifted("↑")} ${shifted("↓")}`],
    ],
  },
  {
    name: "Tracks",
    keys: [
      ["New track", chord("T")],
      ["Delete the selected track", chord("Backspace")],
      ["Rename the selected track", `${chord("R")} / ${shifted("Enter")}`],
      ["Show / hide automation", "A"],
    ],
  },
  {
    // A focused knob's keys; the wheel steps it too.
    name: "Knobs",
    keys: [
      ["Fine adjust", `${mod.shift}-drag`],
      ["Type a value", `Double-click / ${keyName("Enter")}`],
      ["Step / ten steps", `↑ ↓ / ${shifted("↑")} ${shifted("↓")}`],
      ["Reset to its default", `${mod.alt}-click / ${keyName("Backspace")}`],
    ],
  },
  {
    name: "View",
    keys: [
      ["Device / Notes / Mix / Code", [1, 2, 3, 4].map((n) => opt(String(n))).join(" / ")],
      ["Mix", "X"],
      ["Show / hide the dock", "E"],
      ["Show / hide the browser", chord("B", { alt: true })],
      ["Keyboard shortcuts", opt("K")],
    ],
  },
  {
    name: "File",
    keys: [
      ["New song", chord("N")],
      ...(native ? ([["Open a song folder", chord("O")]] as [string, string][]) : []),
      ["Close the song", chord("W")],
      ["Save (in Code)", chord("S")],
      ["Export audio", chord("E", { shift: true })],
    ],
  },
];

/** The keyboard shortcuts sheet (⌥K, as in Logic): every key there is, in one place. */
export default function Shortcuts() {
  const open = useShell((s) => s.sheet);
  const close = () => useShell.getState().set({ sheet: false });
  useEffect(() => {
    if (!open) return;
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" && e.key !== "Enter") return;
      e.preventDefault();
      e.stopPropagation();
      close();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [open]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div role="dialog" aria-modal aria-label="Keyboard shortcuts" data-testid="shortcuts" className="w-[800px] max-w-full max-h-[85vh] flex flex-col rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <div className="px-6 pt-6 pb-3 text-lg font-semibold text-fg-strong">Keyboard shortcuts</div>
        {/* Two columns that each group flows down, so a short group never leaves a hole beside a long one. */}
        <div className="flex-1 min-h-0 overflow-y-auto px-6 columns-2 gap-x-8">
          {GROUPS.map((g) => (
            <section key={g.name} className="break-inside-avoid mb-6">
              <h3 className="mb-1.5 text-sm font-semibold text-fg-muted">{g.name}</h3>
              <dl className="divide-y divide-line">
                {g.keys.map(([what, keys]) => (
                  <div key={what} className="h-ctl flex items-center gap-3 text-sm">
                    <dt className="flex-1 min-w-0 truncate text-fg">{what}</dt>
                    <dd className="shrink-0 text-fg-muted">{keys}</dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
        <div className="px-6 py-4 flex justify-end">
          <Button variant="primary" size="lg" onClick={close}>
            Done
          </Button>
        </div>
      </div>
    </div>
  );
}

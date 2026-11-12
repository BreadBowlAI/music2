import clsx from "clsx";
import { CornerDownLeft, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toggleFrameMeter, useFrameMeter } from "../lib/frames";
import { chord } from "../lib/native";
import { openProbe } from "../lib/probe";
import { useShell } from "../lib/shell";
import { EmptyState } from "./ui";

/** A command the palette offers: its id, its words and a line of what it does. */
interface Command {
  id: string;
  words: string;
  detail: string;
  run: () => void;
}

/**
 * The command palette (⇧⌘P, taste.md §5.1): a field and the commands whose words hold what is typed;
 * ↑ and ↓ pick, Return runs, Esc closes. Today it holds the camera note's two measures (VIEW-10).
 */
export default function CommandPalette() {
  const open = useShell((s) => s.palette);
  return open ? <Palette /> : null;
}

function Palette() {
  const meter = useFrameMeter((s) => s.on);
  const commands: Command[] = [
    { id: "frames", words: meter ? "Hide frame rate" : "Show frame rate", detail: "Frames a second and each view's redraws", run: toggleFrameMeter },
    { id: "probe", words: "Swipe probe", detail: "Measure what your trackpad sends", run: openProbe },
  ];
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const panel = useRef<HTMLDivElement>(null);
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = commands.filter((c) => words.every((w) => c.words.toLowerCase().includes(w)));
  const current = shown[Math.min(active, shown.length - 1)];
  const close = () => useShell.getState().set({ palette: false });
  const run = (c: Command | undefined) => {
    if (!c) return;
    close();
    c.run();
  };
  // A press outside closes it.
  useEffect(() => {
    const outside = (e: PointerEvent) => !panel.current?.contains(e.target as Node) && close();
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, []);
  return (
    <div
      ref={panel}
      role="dialog"
      aria-label="Command palette"
      data-testid="palette"
      className="fixed z-[60] left-1/2 top-[14vh] -translate-x-1/2 w-[480px] max-w-[calc(100vw-32px)] flex flex-col rounded-md bg-raised shadow-pop overflow-hidden animate-rise cursor-default"
    >
      <div className="h-11 shrink-0 flex items-center gap-2 px-3 border-b border-line">
        <Search size={16} className="text-fg-faint shrink-0" />
        <input
          autoFocus
          value={query}
          placeholder="Type a command"
          aria-label="Command"
          role="combobox"
          aria-expanded
          aria-controls="palette-list"
          aria-activedescendant={current ? `palette-${current.id}` : undefined}
          spellCheck={false}
          className="flex-1 min-w-0 bg-transparent outline-none text-base text-fg placeholder:text-fg-off"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") setActive((a) => (Math.min(a, shown.length - 1) + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % Math.max(1, shown.length));
            else if (e.key === "Enter") run(current);
            // Esc, or ⇧⌘P again, closes it.
            else if (e.key === "Escape" || ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "p")) close();
            else return;
            e.preventDefault();
            e.stopPropagation();
          }}
        />
        <span className="shrink-0 text-xs text-fg-faint">{chord("P", { shift: true })}</span>
      </div>
      <div id="palette-list" role="listbox" aria-label="Commands" className="p-1">
        {shown.length === 0 && (
          <EmptyState bare className="mx-auto my-4">
            Nothing matches “{query.trim()}”
          </EmptyState>
        )}
        {shown.map((c) => {
          const on = c === current;
          return (
            <div
              key={c.id}
              id={`palette-${c.id}`}
              role="option"
              aria-selected={on}
              data-command={c.id}
              className={clsx("h-ctl px-2 rounded-sm flex items-center gap-2 transition-colors duration-80 ease-linear", on ? "bg-accent-soft" : "hover:bg-hover")}
              onPointerEnter={() => setActive(shown.indexOf(c))}
              onClick={() => run(c)}
            >
              <span className="text-sm text-fg">{c.words}</span>
              <span className="flex-1 min-w-0 truncate text-right text-xs text-fg-muted">{c.detail}</span>
              <span className="w-3.5 shrink-0">{on && <CornerDownLeft size={14} className="text-fg-faint" />}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

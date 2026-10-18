import clsx from "clsx";
import { AudioWaveform, ChartSpline, CornerDownLeft, Flame, Gauge, Orbit, Plug, Search, Sparkles, Waves, Wrench } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Effect } from "../lib/graph";
import { isPlugin, pluginEffects, PLUGINS, usePluginList, wantPlugins } from "../lib/plugins";
import PluginsFoot from "./PluginsFoot";
import { EmptyState } from "./ui";

/**
 * The categories the browser knows, in the order it lists them, with an icon each, shown once in
 * the category's header (never on each item). A category is a free string a package's descriptor
 * gives (PKG-7): any other is a group of its own after these, with a plain icon.
 */
// No dock view's icon (Device's sliders, Mix's faders): one icon, one meaning (taste.md §2.12).
export const CATEGORIES: { name: string; Icon: typeof Waves }[] = [
  { name: "Filter", Icon: AudioWaveform },
  { name: "EQ", Icon: ChartSpline },
  { name: "Dynamics", Icon: Gauge },
  { name: "Distortion", Icon: Flame },
  { name: "Delay & Reverb", Icon: Waves },
  { name: "Modulation", Icon: Orbit },
  { name: "Utility", Icon: Wrench },
  // The installed plugins (gaps #32), last.
  { name: PLUGINS, Icon: Plug },
];
export const category = (name: string) => CATEGORIES.find((c) => c.name === name) ?? { name, Icon: Sparkles };

export interface Match { effect: Effect; score: number }

/** What a search that finds nothing says, in the side browser and the + picker alike. */
export function NoMatch({ query, className }: { query: string; className?: string }) {
  return (
    <EmptyState bare className={className}>
      <div>
        <p>No effects match “{query}”</p>
        <p className="mt-1 text-xs">Try a word like “filter”, “echo” or “louder”.</p>
      </div>
    </EmptyState>
  );
}

/**
 * How well `query` finds `effect`, or null. Every word must match: in the name as letters in
 * order (best as a prefix, then as a run), or as the start of a word of its label, category or description.
 */
function match(effect: Effect, query: string): Match | null {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  let score = 0;
  const name = effect.name.toLowerCase();
  const starts = (text: string, w: string) => text.toLowerCase().split(/[^a-z0-9]+/).some((t) => t.startsWith(w));
  for (const w of words) {
    let best = 0;
    const at = name.indexOf(w);
    if (at >= 0) {
      best = (at === 0 ? 100 : 70) + w.length;
    } else {
      // Letters in order: tighter is better.
      const found: number[] = [];
      for (let i = 0, j = 0; i < name.length && j < w.length; i++) if (name[i] === w[j]) (found.push(i), j++);
      if (found.length === w.length) best = Math.max(1, 40 - (found[found.length - 1] - found[0] - w.length + 1) * 3);
    }
    // Its label is what it is called ("Limiter" for `limit`).
    if (starts(effect.label, w)) best = Math.max(best, 60);
    if (starts(effect.category, w)) best = Math.max(best, 50);
    if (starts(effect.description, w)) best = Math.max(best, 30);
    if (!best) return null;
    score += best;
  }
  return { effect, score };
}

/** The effects `query` finds, grouped by category: best group first, best effect first in it. */
export function search(effects: Effect[], query: string) {
  const found = effects.map((e) => match(e, query)).filter((m): m is Match => m !== null);
  const groups = new Map<string, Match[]>();
  for (const c of CATEGORIES) groups.set(c.name, []);
  // Categories the browser doesn't know follow, alphabetically.
  for (const c of [...new Set(found.map((m) => m.effect.category))].sort()) if (!groups.has(c)) groups.set(c, []);
  for (const m of found) groups.get(m.effect.category)!.push(m);
  const listed = [...groups.entries()].filter(([, items]) => items.length).map(([name, items]) => ({ name, items: [...items].sort((a, b) => b.score - a.score) }));
  if (query.trim()) listed.sort((a, b) => b.items[0].score - a.items[0].score);
  return listed;
}

const WIDTH = 320;

/**
 * Where the panel goes: under the anchor, right edges aligned, or above it if there's no room below.
 * Its height is set where it opens, so it stays put while the list filters.
 */
function place(anchor: HTMLElement) {
  const r = anchor.getBoundingClientRect();
  const left = Math.min(Math.max(8, r.right - WIDTH), window.innerWidth - WIDTH - 8);
  const below = window.innerHeight - r.bottom - 12;
  const above = r.top - 12;
  if (below >= 300 || below >= above) return { left, top: r.bottom + 6, height: Math.min(460, below) };
  const height = Math.min(460, above);
  return { left, top: r.top - 6 - height, height };
}

/**
 * A searchable list of the effects that can go after `after` (the code's name for it, as the preview
 * writes it; `named`: what the screen calls it): type to filter, arrows to move, Enter to add. The
 * installed effect plugins are its last group (gaps #32), asked for as it opens.
 */
export default function EffectBrowser({
  effects,
  after,
  named = after,
  anchor,
  onPick,
  onClose,
}: {
  effects: Effect[];
  after: string;
  named?: string;
  anchor: HTMLElement;
  onPick: (effect: Effect) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState(() => place(anchor));
  const panel = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const plugins = usePluginList((s) => s.plugins);
  useEffect(() => wantPlugins(), []);
  const groups = useMemo(() => search([...effects, ...pluginEffects(plugins)], query), [effects, plugins, query]);
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const current = flat[Math.min(active, flat.length - 1)];

  // Follow the anchor while the graph scrolls; close on a click anywhere else.
  useLayoutEffect(() => {
    const follow = () => setPos(place(anchor));
    const outside = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!panel.current?.contains(t) && !anchor.contains(t)) onClose();
    };
    window.addEventListener("scroll", follow, true);
    window.addEventListener("resize", follow);
    document.addEventListener("pointerdown", outside, true);
    return () => {
      window.removeEventListener("scroll", follow, true);
      window.removeEventListener("resize", follow);
      document.removeEventListener("pointerdown", outside, true);
    };
  }, [anchor, onClose]);

  useEffect(() => {
    list.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest" });
  }, [active, query]);

  const choose = (m: Match | undefined) => m && onPick(m.effect);
  const close = () => {
    onClose();
    anchor.focus();
  };

  let index = 0;
  return (
    <div
      ref={panel}
      role="dialog"
      aria-label={`Add an effect after ${after}`}
      data-effect-browser
      className="fixed z-50 flex flex-col rounded-md bg-raised shadow-pop overflow-hidden animate-rise cursor-default"
      style={{ ...pos, width: WIDTH }}
      onPointerDown={(e) => e.stopPropagation()}
    >
      <div className="h-10 shrink-0 flex items-center gap-2 px-3 border-b border-line">
        <Search size={14} className="text-fg-faint shrink-0" />
        <input
          autoFocus
          value={query}
          placeholder={`Add after ${named}…`}
          aria-label="Search effects"
          role="combobox"
          aria-expanded
          aria-controls="effect-list"
          aria-activedescendant={current ? `effect-${current.effect.component}` : undefined}
          className="flex-1 min-w-0 bg-transparent outline-none text-base text-fg placeholder:text-fg-off"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || (e.key === "n" && e.ctrlKey)) {
              e.preventDefault();
              setActive((a) => (flat.length ? (Math.min(a, flat.length - 1) + 1) % flat.length : 0));
            } else if (e.key === "ArrowUp" || (e.key === "p" && e.ctrlKey)) {
              e.preventDefault();
              setActive((a) => (flat.length ? (Math.min(a, flat.length - 1) - 1 + flat.length) % flat.length : 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              choose(current);
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              close();
            }
          }}
        />
      </div>
      <div ref={list} id="effect-list" role="listbox" aria-label="Effects" className="flex-1 min-h-0 overflow-y-auto p-1">
        {flat.length === 0 && <NoMatch query={query} className="mx-auto my-6" />}
        {groups.map((g) => {
          const { Icon } = category(g.name);
          return (
            <div key={g.name} role="group" aria-label={g.name} data-category={g.name}>
              <div className="h-ctl-sm px-2 flex items-center gap-1.5 text-sm font-semibold text-fg-muted">
                <Icon size={14} className="text-fg-faint" />
                {g.name}
              </div>
              {g.items.map((m) => {
                const i = index++;
                const on = m === current;
                return (
                  <div
                    key={m.effect.component}
                    id={`effect-${m.effect.component}`}
                    role="option"
                    aria-selected={on}
                    data-effect={m.effect.name}
                    data-component={m.effect.component}
                    data-active={on || undefined}
                    // Named by its label ("Lowpass"); the code's name is in the tooltip and the footer.
                    className={clsx("px-2 py-1.5 rounded-sm flex items-center gap-2 cursor-pointer transition-colors duration-80 ease-linear", on && "bg-accent-soft")}
                    title={m.effect.name}
                    onPointerMove={() => i !== active && setActive(i)}
                    onClick={() => choose(m)}
                  >
                    <span className="flex-1 min-w-0">
                      <span className="block text-sm font-medium text-fg">{m.effect.label}</span>
                      <span className="block text-xs text-fg-muted truncate">{m.effect.description}</span>
                    </span>
                    {on && <CornerDownLeft size={14} className="shrink-0 text-fg-faint" />}
                  </div>
                );
              })}
            </div>
          );
        })}
        <PluginsFoot kind="effect" />
      </div>
      {/* Literally code: what adding it writes. Nothing listed, nothing to write: no empty band. */}
      {current && (
        <div className="h-ctl shrink-0 flex items-center px-3 border-t border-line bg-chrome text-xs text-fg-muted">
          <span data-code className="flex-1 min-w-0 truncate font-mono" title="What adding it writes in your code">
            {isPlugin(current.effect.component)
              ? `plugin("${current.effect.component}", { in: ${after}(…) })`
              : `${current.effect.name}(${after}(…)${current.effect.insert})`}
          </span>
        </div>
      )}
    </div>
  );
}

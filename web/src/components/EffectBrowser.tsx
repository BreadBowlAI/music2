import clsx from "clsx";
import { AudioWaveform, CornerDownLeft, Flame, Gauge, Orbit, Search, SlidersHorizontal, SlidersVertical, Sparkles, Waves } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Effect } from "../lib/graph";

/** The categories in the order the browser lists them, with an icon and a colour each. */
const CATEGORIES: { name: string; Icon: typeof Waves; color: string }[] = [
  { name: "Filter", Icon: AudioWaveform, color: "#4a8cc4" },
  { name: "EQ", Icon: SlidersVertical, color: "#3e9b9b" },
  { name: "Dynamics", Icon: Gauge, color: "#c08a2e" },
  { name: "Distortion", Icon: Flame, color: "#cf6a4c" },
  { name: "Delay & Reverb", Icon: Waves, color: "#8a6fc4" },
  { name: "Modulation", Icon: Orbit, color: "#c45c9a" },
  { name: "Utility", Icon: SlidersHorizontal, color: "#7b7f87" },
  { name: "Other", Icon: Sparkles, color: "#7b7f87" },
];
const category = (name: string) => CATEGORIES.find((c) => c.name === name) ?? CATEGORIES[CATEGORIES.length - 1];

interface Match { effect: Effect; score: number; hits: number[] }

/**
 * How well `query` finds `effect`, or null. Every word must match: in the name as letters in
 * order (best as a prefix, then as a run), or as the start of a word of its category or description.
 */
function match(effect: Effect, query: string): Match | null {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const hits = new Set<number>();
  let score = 0;
  const name = effect.name.toLowerCase();
  const starts = (text: string, w: string) => text.toLowerCase().split(/[^a-z0-9]+/).some((t) => t.startsWith(w));
  for (const w of words) {
    let best = 0;
    const at = name.indexOf(w);
    if (at >= 0) {
      best = (at === 0 ? 100 : 70) + w.length;
      for (let i = at; i < at + w.length; i++) hits.add(i);
    } else {
      // Letters in order: tighter is better.
      const found: number[] = [];
      for (let i = 0, j = 0; i < name.length && j < w.length; i++) if (name[i] === w[j]) (found.push(i), j++);
      if (found.length === w.length) {
        best = Math.max(1, 40 - (found[found.length - 1] - found[0] - w.length + 1) * 3);
        found.forEach((i) => hits.add(i));
      }
    }
    if (starts(effect.category, w)) best = Math.max(best, 50);
    if (starts(effect.description, w)) best = Math.max(best, 30);
    if (!best) return null;
    score += best;
  }
  return { effect, score, hits: [...hits] };
}

/** The effects `query` finds, grouped by category: best group first, best effect first in it. */
function search(effects: Effect[], query: string) {
  const found = effects.map((e) => match(e, query)).filter((m): m is Match => m !== null);
  const groups = new Map<string, Match[]>();
  for (const c of CATEGORIES) groups.set(c.name, []);
  for (const m of found) {
    const name = category(m.effect.category).name;
    groups.get(name)!.push(m);
  }
  const listed = [...groups.entries()].filter(([, items]) => items.length).map(([name, items]) => ({ name, items: [...items].sort((a, b) => b.score - a.score) }));
  if (query.trim()) listed.sort((a, b) => b.items[0].score - a.items[0].score);
  return listed;
}

/** Where the panel goes: under the anchor, right edges aligned, or above it if there's no room below. */
function place(anchor: HTMLElement) {
  const r = anchor.getBoundingClientRect();
  const width = 380;
  const left = Math.min(Math.max(8, r.right - width), window.innerWidth - width - 8);
  const below = window.innerHeight - r.bottom - 12;
  const above = r.top - 12;
  return below >= 300 || below >= above
    ? { left, top: r.bottom + 6, maxHeight: Math.min(460, below) }
    : { left, bottom: window.innerHeight - r.top + 6, maxHeight: Math.min(460, above) };
}

/** A searchable list of the effects that can go after `after`: type to filter, arrows to move, Enter to add. */
export default function EffectBrowser({
  effects,
  after,
  anchor,
  onPick,
  onClose,
}: {
  effects: Effect[];
  after: string;
  anchor: HTMLElement;
  onPick: (effect: Effect) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState(() => place(anchor));
  const panel = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const groups = useMemo(() => search(effects, query), [effects, query]);
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
      className="fixed z-50 w-[380px] flex flex-col rounded-2xl bg-ink-850 ring-1 ring-black/10 shadow-2xl shadow-black/20 overflow-hidden rise cursor-default"
      style={pos}
      onPointerDown={(e) => e.stopPropagation()}
    >
      <div className="h-12 shrink-0 flex items-center gap-2.5 px-4 border-b border-black/[0.06] bg-ink-800">
        <Search className="size-4 text-ink-400 shrink-0" />
        <input
          autoFocus
          value={query}
          placeholder={`Add after ${after}…`}
          aria-label="Search effects"
          role="combobox"
          aria-expanded
          aria-controls="effect-list"
          aria-activedescendant={current ? `effect-${current.effect.module}-${current.effect.name}` : undefined}
          className="flex-1 min-w-0 bg-transparent outline-none text-[14px] text-ink-100 placeholder:text-ink-400"
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
        <kbd className="shrink-0 h-5 px-1.5 rounded-md bg-ink-700 text-[10px] font-sans text-ink-400 grid place-items-center">esc</kbd>
      </div>
      <div ref={list} id="effect-list" role="listbox" aria-label="Effects" className="flex-1 min-h-0 overflow-y-auto py-1.5">
        {flat.length === 0 && (
          <div className="px-6 py-10 text-center">
            <div className="text-[13px] text-ink-200">No effects match “{query}”</div>
            <div className="mt-1 text-xs text-ink-400">Try a word like “filter”, “echo” or “louder”.</div>
          </div>
        )}
        {groups.map((g) => {
          const { Icon, color } = category(g.name);
          return (
            <div key={g.name} role="group" aria-label={g.name} data-category={g.name}>
              <div className="px-4 pt-2.5 pb-1 text-[10px] font-semibold uppercase tracking-[0.08em] text-ink-400">{g.name}</div>
              {g.items.map((m) => {
                const i = index++;
                const on = m === current;
                return (
                  <div
                    key={`${m.effect.module}.${m.effect.name}`}
                    id={`effect-${m.effect.module}-${m.effect.name}`}
                    role="option"
                    aria-selected={on}
                    data-effect={m.effect.name}
                    data-active={on || undefined}
                    className={clsx("mx-1.5 px-2.5 py-2 rounded-xl flex items-center gap-3 cursor-pointer transition-colors", on ? "bg-ink-700/70" : "hover:bg-ink-700/40")}
                    onPointerMove={() => i !== active && setActive(i)}
                    onClick={() => choose(m)}
                  >
                    <span className="size-8 shrink-0 rounded-[10px] grid place-items-center" style={{ background: `color-mix(in oklab, ${color} 14%, var(--color-ink-800))`, color }}>
                      <Icon className="size-4" />
                    </span>
                    <span className="flex-1 min-w-0">
                      <span className="block text-[13px] font-medium text-ink-100 leading-5">
                        {[...m.effect.name].map((ch, k) => (
                          <span key={k} className={m.hits.includes(k) ? "text-pop-500 font-semibold" : undefined}>{ch}</span>
                        ))}
                      </span>
                      <span className="block text-xs text-ink-400 truncate leading-4">{m.effect.description}</span>
                    </span>
                    {on && <CornerDownLeft className="size-3.5 shrink-0 text-ink-400" />}
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>
      <div className="h-9 shrink-0 flex items-center gap-3 px-4 border-t border-black/[0.06] bg-ink-800 text-[11px] text-ink-400">
        <span className="flex-1 min-w-0 truncate font-mono" title="What adding it writes in your code">
          {current ? `${current.effect.name}(${after}(…)${current.effect.args ? `, ${current.effect.args}` : ""})` : ""}
        </span>
        <span className="shrink-0">↑↓ to move · ↵ to add</span>
      </div>
    </div>
  );
}

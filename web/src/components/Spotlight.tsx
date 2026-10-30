import clsx from "clsx";
import { CornerDownLeft, Loader2, Search, Volume2 } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { lockReason, moduleName, type GraphState } from "../lib/graph";
import { fromWords, GROUPS, searchLibrary, stopPreview, togglePreview, useFinding, usePreview, type LibraryItem } from "../lib/library";
import { chord } from "../lib/native";
import { keptWords, presetsApi } from "../lib/presets";
import { listedInstrument, usePluginList, wantPlugins } from "../lib/plugins";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { addEffectTo, EFFECT_DRAG } from "./Browser";
import { undoAction } from "./GraphCanvas";
import { addTrack, INSTRUMENT_DRAG } from "./Tracks";
import { EmptyState } from "./ui";

/** Open or close ⌘F's panel. */
export function toggleFinding() {
  const open = !useFinding.getState().open;
  if (!open) stopPreview();
  useFinding.setState({ open });
}

/** The track an item goes to: the one the dock shows (the selected track, else the last one). */
const target = () => {
  const s = useShell.getState();
  return s.module && s.module !== "song" ? s.module : null;
};

/**
 * Load sound `item` (a preset) onto the instrument or effect it is for on the selected track: one
 * Undo step, as the card's Presets does. A track that doesn't play it is said in words.
 */
function loadSound(item: LibraryItem, graph: GraphState) {
  const toast = useStore.getState().toast;
  const preset = item.preset;
  if (!preset) return;
  const module = target();
  const sound = item.tags[0] || item.component;
  const m = module ? graph.view.modules.find((x) => x.key === module) : undefined;
  const node = m?.nodes.find((n) => n.kind === "call" && n.component === item.component);
  if (!m || !node) {
    const where = m ? `${moduleName(m)} has no ${sound}` : "No track is selected";
    return toast(`${where}: select a track that plays ${sound} to load ${item.label} onto it.`);
  }
  presetsApi.load(graph.session, graph.view.revision, node.id, preset.from, preset.name).then(
    (r) => {
      const kept = keptWords(r.kept);
      if (!r.changed) toast(`${sound} on ${moduleName(m)} has ${item.label}'s settings already.${kept ? ` ${kept}` : ""}`);
      else toast(`Loaded ${item.label} into ${sound} on ${moduleName(m)}${kept ? `. ${kept}` : ""}`, undoAction(graph.session, r.step));
    },
    (e) => toast(e instanceof Error ? e.message : String(e)),
  );
}

/**
 * Add `item` as the side browser does, each one Undo step that says what it did: an instrument (or
 * an instrument plugin) makes a new track below the selected one; an effect (or an effect plugin)
 * goes after the last thing on the selected track; a sound loads onto the selected track's
 * instrument or effect it is for.
 */
export function addItem(item: LibraryItem) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  const why = lockReason(graph, connected);
  if (why) return toast(`${why}.`);
  if (item.kind === "instrument" || (item.kind === "plugin" && item.instrument)) {
    const ins = graph.instruments?.find((i) => i.component === item.component) ?? listedInstrument(item.component);
    if (!ins) return toast(`${item.label} isn't there any more.`);
    return void addTrack(ins, target());
  }
  if (item.kind === "effect" || item.kind === "plugin") {
    const module = target();
    if (!module) return toast(`Select a track to add ${item.label} to.`);
    return addEffectTo(module, item.component);
  }
  if (item.kind === "sound") return loadSound(item, graph);
  toast(`${item.label} can't be added yet.`);
}

/** What Enter does with `item`, in words, for the panel's foot. */
function enterWords(item: LibraryItem, graph: GraphState): string {
  const module = target();
  const name = module ? moduleName(graph.view.modules.find((m) => m.key === module) ?? { key: module }) : null;
  if (item.kind === "instrument" || (item.kind === "plugin" && item.instrument)) return "adds a track";
  if (item.kind === "effect" || item.kind === "plugin") return name ? `adds it to ${name}` : "select a track first";
  if (item.kind === "sound") return name ? `loads it on ${name}` : "select a track first";
  return "adds it";
}

/** How long typing rests before the search is asked. */
const DEBOUNCE_MS = 60;

/**
 * ⌘F's panel (LIB-1): one search over everything that can be added, Spotlight-like. Typing filters;
 * ↑↓ move across the groups; Space plays the selected row's preview once the arrows have moved
 * there (again, or moving, stops it); Enter adds it; Esc closes. A row drags onto the timeline or
 * a track, as the side browser's rows do.
 */
export default function Spotlight({ graph }: { graph: GraphState }) {
  const open = useFinding((s) => s.open);
  if (!open) return null;
  return <Panel graph={graph} />;
}

function Panel({ graph }: { graph: GraphState }) {
  const [query, setQuery] = useState("");
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [searched, setSearched] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  // The arrows moved: Space plays the row, until a key types again.
  const [browsing, setBrowsing] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  const preview = usePreview();
  const plugins = usePluginList((s) => s.plugins);
  const panel = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const close = () => {
    stopPreview();
    useFinding.setState({ open: false });
  };

  // The installed plugins are asked for as it opens, as + Track's list asks.
  useEffect(() => wantPlugins(), []);
  useLayoutEffect(() => field.current?.focus(), []);
  // The search, as typing rests; an answer to an older query is dropped.
  const asked = useRef(0);
  useEffect(() => {
    const n = ++asked.current;
    const t = window.setTimeout(() => {
      searchLibrary(query)
        .then((found) => {
          if (n !== asked.current) return;
          setItems(found);
          setSearched(true);
          // The best hit, wherever its group is.
          setActive(found[0]?.id ?? null);
        })
        .catch((e) => n === asked.current && setSaid(e instanceof Error ? e.message : String(e)));
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [query, plugins]);
  // A click anywhere else closes it.
  useEffect(() => {
    const outside = (e: PointerEvent) => {
      if (!panel.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, []);

  const groups = useMemo(() => GROUPS.map((g) => ({ ...g, items: items.filter((i) => i.kind === g.kind) })).filter((g) => g.items.length), [items]);
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const current = flat.find((i) => i.id === active) ?? flat[0];
  useEffect(() => {
    list.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest" });
  }, [current?.id]);

  const move = (by: number) => {
    if (!flat.length) return;
    const at = current ? flat.indexOf(current) : -1;
    setActive(flat[(at + by + flat.length) % flat.length].id);
    setBrowsing(true);
    setSaid(null);
    stopPreview();
  };
  const play = (item: LibraryItem) => void togglePreview(item).then((why) => setSaid(why));
  const add = (item: LibraryItem | undefined) => {
    if (!item) return;
    close();
    addItem(item);
  };

  return (
    <div
      ref={panel}
      role="dialog"
      aria-label="Find sounds"
      data-spotlight
      className="fixed z-[60] left-1/2 top-[14vh] -translate-x-1/2 w-[560px] max-w-[calc(100vw-32px)] max-h-[64vh] flex flex-col rounded-md bg-raised shadow-pop overflow-hidden animate-rise cursor-default"
    >
      <div className="h-11 shrink-0 flex items-center gap-2 px-3 border-b border-line">
        <Search size={16} className="text-fg-faint shrink-0" />
        <input
          ref={field}
          value={query}
          placeholder="Find instruments, sounds and effects"
          aria-label="Find sounds"
          role="combobox"
          aria-expanded
          aria-controls="spotlight-list"
          aria-activedescendant={current ? `spotlight-${current.id}` : undefined}
          spellCheck={false}
          className="flex-1 min-w-0 bg-transparent outline-none text-base text-fg placeholder:text-fg-off"
          onChange={(e) => {
            setQuery(e.target.value);
            setBrowsing(false);
            setSaid(null);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") move(1);
            else if (e.key === "ArrowUp") move(-1);
            else if (e.key === " " && browsing && current) play(current);
            else if (e.key === "Enter") add(current);
            else if (e.key === "Escape") close();
            else return;
            e.preventDefault();
            e.stopPropagation();
          }}
        />
        <span className="shrink-0 text-xs text-fg-faint">{chord("F")}</span>
      </div>
      <div ref={list} id="spotlight-list" role="listbox" aria-label="Found" className="flex-1 min-h-0 overflow-y-auto p-1">
        {searched && flat.length === 0 && (
          <EmptyState bare className="mx-auto my-6">
            <div>
              <p>{query.trim() ? `Nothing matches “${query.trim()}”` : "Nothing to add"}</p>
              <p className="mt-1 text-xs">Try a word like “pad”, “drums” or “echo”.</p>
            </div>
          </EmptyState>
        )}
        {groups.map((g) => (
          <div key={g.kind} role="group" aria-label={g.name} data-spotlight-group={g.kind}>
            <div className="h-ctl-sm px-2 flex items-center text-sm font-semibold text-fg-muted">{g.name}</div>
            {g.items.map((item) => {
              const on = item === current;
              const drag = item.kind === "instrument" || (item.kind === "plugin" && item.instrument) ? INSTRUMENT_DRAG : item.kind === "effect" || item.kind === "plugin" ? EFFECT_DRAG : null;
              const sound = item.kind === "sound" ? item.tags[0] : item.category;
              return (
                <div
                  key={item.id}
                  id={`spotlight-${item.id}`}
                  role="option"
                  aria-selected={on}
                  data-spotlight-item={item.id}
                  data-active={on || undefined}
                  draggable={!!drag}
                  title={item.description}
                  className={clsx("h-ctl px-2 rounded-sm flex items-center gap-2 cursor-default transition-colors duration-80 ease-linear", on ? "bg-accent-soft" : "hover:bg-hover")}
                  onPointerDown={() => setActive(item.id)}
                  onDoubleClick={() => add(item)}
                  onDragStart={(ev) => {
                    if (!drag) return;
                    ev.dataTransfer.setData(drag, item.component);
                    ev.dataTransfer.effectAllowed = "copy";
                  }}
                  onDragEnd={close}
                >
                  <span className="w-4 shrink-0 grid place-items-center text-fg-faint">
                    {preview.loading === item.id ? <Loader2 size={14} className="animate-spin" /> : preview.playing === item.id ? <Volume2 size={14} className="text-accent" /> : null}
                  </span>
                  <span className="flex-1 min-w-0 truncate text-sm text-fg">{item.label}</span>
                  <span className="shrink-0 max-w-[45%] truncate text-xs text-fg-muted">
                    {sound} · {fromWords(item)}
                  </span>
                  {on && <CornerDownLeft size={14} className="shrink-0 text-fg-faint" />}
                </div>
              );
            })}
          </div>
        ))}
      </div>
      <div className="h-ctl shrink-0 flex items-center gap-3 px-3 border-t border-line bg-chrome text-xs text-fg-muted">
        <span data-spotlight-said className="flex-1 min-w-0 truncate">
          {said ?? (current ? `Enter ${enterWords(current, graph)}${current.preview ? " · Space plays it" : " · no preview"}` : "Type to find")}
        </span>
        <span className="shrink-0">↑↓ · Esc</span>
      </div>
    </div>
  );
}

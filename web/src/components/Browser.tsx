import clsx from "clsx";
import { ChevronRight, Search } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { fromPack, graphApi, lockReason, moduleName, type Effect, type GraphState, type Instrument, type ViewNode } from "../lib/graph";
import { useSearch, type LibraryItem } from "../lib/library";
import { listedEffect, listedInstrument, pluginEffects, pluginInstruments, PLUGINS, usePluginList, wantPlugins } from "../lib/plugins";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { category, NoMatch, search } from "./EffectBrowser";
import { undoAction } from "./GraphCanvas";
import Guide from "./Guide";
import PacksList from "./PacksList";
import PluginsFoot from "./PluginsFoot";
import { addTrack, INSTRUMENT_DRAG, instrumentGroups } from "./Tracks";

/** A row of the browser's one keyboard list: an instrument or an effect. */
type Row = { instrument: Instrument } | { effect: Effect };

/** A row's key in the list: its kind and component (a plugin may be both an instrument and an effect). */
const rowKey = (r: Row) => ("instrument" in r ? `instrument:${r.instrument.component}` : `effect:${r.effect.component}`);

/** What a dragged browser row carries: the effect's component (`std:reverb`). */
export const EFFECT_DRAG = "application/x-m2-effect";

const FOLDED_KEY = "m2.browser.folded";
/** The Plugins groups opened: folded until opened, as opening one is what asks for the list. */
const PLUGINS_KEY = "m2.browser.plugins";

/**
 * Where an effect added to a track goes: after the last thing in its sound, the node whose audio
 * the module's `out` takes (taste.md §3.6: "after the last effect"); and what else hears that
 * node, which an effect wrapped round it would change too (a kick's sound that a sidechain follows).
 */
function chainEnd(graph: GraphState, module: string) {
  const m = graph.view.modules.find((x) => x.key === module);
  if (!m) return null;
  const outs = m.nodes.filter((n) => n.kind === "export");
  const into = (ids: string[]) => m.port_edges.find((e) => ids.includes(e.to) && e.kind === "audio");
  const edge = into(outs.filter((n) => n.label === "out").map((n) => n.id)) ?? into(outs.map((n) => n.id));
  const node = edge && m.nodes.find((n) => n.id === edge.from);
  if (!edge || !node?.audio) return null;
  const others = m.port_edges.filter((e) => e.from === node.id && e.from_port === edge.from_port && e.to !== edge.to);
  const also = [...new Set(others.map((e) => m.nodes.find((n) => n.id === e.to)).filter((n): n is ViewNode => !!n).map((n) => title(graph, n)))];
  return { node, also };
}

/** A call's name on screen, its component's label ("Pluck"), as its card in Device says it. */
const title = (graph: GraphState, n: ViewNode) => n.effect ?? n.title ?? graph.effects.find((e) => e.component === n.component)?.label ?? n.label;

/**
 * Add `effect` at the end of `module`'s sound: one edit, said in words with its Undo, as the
 * Device view's + says it ("Added Reverb after Pluck"). When what it goes after also feeds
 * something else (a sidechain's follower), it asks first: the effect would reach that too.
 */
export function addEffectTo(module: string, component: string) {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return;
  // A std effect, or an installed plugin (gaps #32).
  const effect = graph.effects.find((e) => e.component === component) ?? listedEffect(component);
  const why = lockReason(graph, connected);
  if (!effect) return;
  if (why) return toast(`${why}.`);
  const name = moduleName(graph.view.modules.find((m) => m.key === module) ?? { key: module });
  const end = chainEnd(graph, module);
  if (!end) return toast(`${name} has no sound to put ${effect.label} after.`);
  const after = title(graph, end.node);
  const add = () =>
    graphApi
      .addEffect(graph.session, graph.view.revision, end.node.id, effect)
      .then((r) => toast(`Added ${effect.label} after ${after}`, undoAction(graph.session, r.step)))
      .catch((error) => toast((error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "")));
  if (!end.also.length) return void add();
  toast(`${effect.label} after ${after} would reach ${end.also.join(" and ")} too`, {
    label: "Add anyway",
    // Its own toast then replaces this one.
    run: () => {
      // Only while the song is as it was asked about.
      if (useStore.getState().graph?.view.revision === graph.view.revision) void add();
      else toast("The song has changed since: drop it again.");
    },
  });
}

/** Whether a drag carries an effect from the browser. */
export const dragsEffect = (e: React.DragEvent) => e.dataTransfer.types.includes(EFFECT_DRAG);

/** `list` in groups by category, each group where its first item is: a search's best group first. */
function byCategory<T extends { category: string }>(list: T[]) {
  const groups: { name: string; items: T[] }[] = [];
  for (const x of list) {
    const g = groups.find((g) => g.name === x.category) ?? (groups.push({ name: x.category, items: [] }), groups[groups.length - 1]);
    g.items.push(x);
  }
  return groups;
}

function readFolded(key = FOLDED_KEY): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(key) ?? "[]"));
  } catch {
    return new Set();
  }
}

/**
 * The side browser (taste.md §3.6): what can be added, by its label, grouped by category. Typing
 * filters it through the library's one search (LIB-1), as ⌘F does: at once in the window, then by
 * the search's answer for the words typed. ↓ moves into the list and Enter adds the row to the
 * selected track, once the search answered the words typed; a row can be dragged onto a track or
 * the Device view. Instruments add a track: Enter or a double-click, below
 * the selected one; dragged, below the tracks (BR-1). The installed plugins (gaps #32) are a Plugins
 * group under the instruments and one under the effects, folded until opened: opening one asks for
 * the list, which the plugin host process scans.
 */
export default function Browser({ graph, module }: { graph: GraphState; module: string }) {
  const [query, setQuery] = useState("");
  // The row the keyboard is on: the one the arrows or a click picked, by its key, else the first
  // (typing puts it there); none until then.
  const [picked, setPicked] = useState<string | null>(null);
  const [cursor, setCursor] = useState(false);
  const [folded, setFolded] = useState(() => readFolded());
  const [opened, setOpened] = useState(() => readFolded(PLUGINS_KEY));
  const plugins = usePluginList();
  const list = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const connected = useStore((s) => s.connected);
  const locked = lockReason(graph, connected);
  const blank = !query.trim();
  // The search is the library's (LIB-1), asked as typing rests. Until it answers the words in the
  // field, the window filters as it always did, at once; then the hits keep their rank in their
  // groups. An answer to older words is never shown as the newer's.
  const { found, now } = useSearch(blank ? null : query, plugins.plugins);
  const hits = !blank && found?.q === query ? found.items : null;
  // The words in the field now: an Enter that waits for the search acts only on these.
  const typed = useRef(query);
  typed.current = query;
  // The Plugins groups: open when opened, or searched once the list is there (a search doesn't scan).
  const pluginsOpen = (k: string) => opened.has(k) || (!blank && plugins.state === "listed");
  /** The browser's groups and its one keyboard list, from `hits` (the search's), else the window's filter of the words typed. */
  const lists = (hits: LibraryItem[] | null) => {
    const ranked = <T,>(kind: LibraryItem["kind"], find: (component: string) => T | undefined): T[] =>
      (hits ?? []).filter((h) => h.kind === kind).flatMap((h) => find(h.component) ?? []);
    const groups = hits
      ? byCategory(ranked("effect", (c) => graph.effects.find((e) => e.component === c))).map((g) => ({ name: g.name, items: g.items.map((effect) => ({ effect, score: 0 })) }))
      : search(graph.effects, query);
    const instruments = hits
      ? instrumentGroups(ranked("instrument", (c) => graph.instruments?.find((i) => i.component === c)), "")
      : instrumentGroups(graph.instruments ?? [], query);
    // Folds hold while browsing; a search shows every match.
    const shown = groups.map((g) => ({ ...g, open: !blank || !folded.has(g.name) }));
    const playable = instruments.map((g) => ({ ...g, open: !blank || !folded.has(`instruments:${g.name}`) }));
    const fxPlugins = hits ? ranked("plugin", listedEffect) : search(pluginEffects(plugins.plugins), query).flatMap((g) => g.items.map((m) => m.effect));
    const insPlugins = hits ? ranked("plugin", listedInstrument) : instrumentGroups(pluginInstruments(plugins.plugins), query).flatMap((g) => g.items);
    // Instruments first, then effects, each with its plugins last: one keyboard list.
    const flat: Row[] = [
      ...playable.flatMap((g) => (g.open ? g.items.map((i) => ({ instrument: i })) : [])),
      ...(pluginsOpen("instruments") ? insPlugins.map((i) => ({ instrument: i })) : []),
      ...shown.flatMap((g) => (g.open ? g.items.map((m) => ({ effect: m.effect })) : [])),
      ...(pluginsOpen("effects") ? fxPlugins.map((e) => ({ effect: e })) : []),
    ];
    return { shown, playable, fxPlugins, insPlugins, flat };
  };
  const { shown, playable, fxPlugins, insPlugins, flat } = lists(hits);
  const current = cursor ? (flat.find((r) => rowKey(r) === picked) ?? flat[0] ?? null) : null;
  const currentId = current && rowKey(current);
  const target = moduleName(graph.view.modules.find((m) => m.key === module) ?? { key: module });

  const fold = (name: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      try {
        localStorage.setItem(FOLDED_KEY, JSON.stringify([...next]));
      } catch {}
      return next;
    });
  const openPlugins = (k: string) => {
    if (!blank) return;
    if (!opened.has(k)) wantPlugins();
    setOpened((o) => {
      const next = new Set(o);
      if (next.has(k)) next.delete(k);
      else next.add(k);
      try {
        localStorage.setItem(PLUGINS_KEY, JSON.stringify([...next]));
      } catch {}
      return next;
    });
  };
  // A Plugins group opened in an earlier window asks for the list as this one opens.
  useEffect(() => {
    if (opened.size > 0) wantPlugins();
  }, [opened]);
  const move = (step: number) => {
    if (!flat.length) return;
    const at = current ? flat.indexOf(current) : -1;
    const next = at < 0 ? (step > 0 ? 0 : flat.length - 1) : (at + step + flat.length) % flat.length;
    setPicked(rowKey(flat[next]));
    setCursor(true);
    requestAnimationFrame(() => list.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest" }));
  };
  const add = (r: Row) => {
    if ("effect" in r) return addEffectTo(useShell.getState().module ?? module, r.effect.component);
    // A new track goes below the selected one.
    const s = useShell.getState();
    void addTrack(r.instrument, s.selected && s.selected !== "song" ? s.selected : null);
  };
  // Enter: the row the keyboard is on, once the search answered the words in the field; before
  // that, once it does (asked at once), the row picked if the answer has it, else its first, if
  // the words are still the ones in the field. Never a row of an older search.
  const enter = () => {
    if (!current) return;
    if (blank || hits) return add(current);
    const [words, want] = [query, picked];
    void now().then(
      (answer) => {
        if (typed.current !== words) return;
        const rows = lists(answer).flat;
        const row = rows.find((r) => rowKey(r) === want) ?? rows[0];
        if (row) add(row);
      },
      () => {},
    );
  };

  const instrumentRow = (ins: Instrument) => {
    const key = `instrument:${ins.component}`;
    const on = currentId === key;
    return (
      <div
        key={ins.component}
        id={`browser-${ins.component}`}
        role="option"
        aria-selected={on}
        data-browser-instrument={ins.component}
        data-active={on || undefined}
        draggable={!locked}
        title={`${ins.description}\n${ins.name}${fromPack(ins)}${locked ? `\n\n${locked}.` : "\n\nEnter or double-click adds a track with it; drag it below the tracks"}`}
        onPointerDown={() => {
          setPicked(key);
          setCursor(true);
        }}
        onDoubleClick={() => add({ instrument: ins })}
        onDragStart={(ev) => {
          ev.dataTransfer.setData(INSTRUMENT_DRAG, ins.component);
          ev.dataTransfer.effectAllowed = "copy";
        }}
        className={clsx(
          "h-ctl-sm pl-4 pr-2 flex items-center rounded-sm text-sm cursor-default transition-colors duration-80 ease-linear",
          on ? "bg-accent-soft text-fg" : "text-fg hover:bg-hover",
        )}
      >
        <span className="truncate">{ins.label}</span>
      </div>
    );
  };
  const effectRow = (e: Effect, indent = "pl-10") => {
    const key = `effect:${e.component}`;
    const on = currentId === key;
    return (
      <div
        key={e.component}
        id={`browser-${e.component}`}
        role="option"
        aria-selected={on}
        data-browser-item={e.component}
        data-active={on || undefined}
        draggable={!locked}
        title={`${e.description}\n${e.name}${fromPack(e)}${locked ? `\n\n${locked}.` : `\n\nEnter or double-click adds it to ${target}; drag it onto a track`}`}
        onPointerDown={() => {
          setPicked(key);
          setCursor(true);
        }}
        onDoubleClick={() => add({ effect: e })}
        onDragStart={(ev) => {
          ev.dataTransfer.setData(EFFECT_DRAG, e.component);
          ev.dataTransfer.effectAllowed = "copy";
        }}
        className={clsx(
          // Its name under its category's (4 + 12 + 4 + 16 + 4 px in).
          `h-ctl-sm ${indent} pr-2 flex items-center rounded-sm text-sm cursor-default transition-colors duration-80 ease-linear`,
          on ? "bg-accent-soft text-fg" : "text-fg hover:bg-hover",
        )}
      >
        <span className="truncate">{e.label}</span>
      </div>
    );
  };
  /** A Plugins group (gaps #32): its header opens it, which asks for the list; its foot says what it found, and Look again. */
  const pluginGroup = (k: "instruments" | "effects", rows: () => ReactNode) => {
    const open = pluginsOpen(k);
    return (
      <div role="group" aria-label={PLUGINS} data-browser-plugins={k} className="mt-2">
        <button
          aria-expanded={open}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => openPlugins(k)}
          className={clsx(
            "w-full h-ctl-sm px-1 flex items-center gap-1 rounded-sm text-sm font-semibold hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear",
            k === "effects" ? "text-fg-muted" : "text-fg",
          )}
        >
          <ChevronRight size={12} className={clsx("shrink-0 transition-transform duration-140 ease-out", open && "rotate-90")} />
          {PLUGINS}
        </button>
        {open && rows()}
        {open && <PluginsFoot kind={k === "effects" ? "effect" : "instrument"} className={k === "effects" ? "pl-10" : "pl-4"} />}
      </div>
    );
  };
  return (
    <aside data-browser aria-label="Browser" className="w-side shrink-0 flex flex-col bg-chrome border-r border-line min-h-0">
      {/* Make Your First Song, when open (the studio plan's Q47): first, above the search. */}
      <Guide />
      <div className="p-2 shrink-0">
        <label className="h-ctl px-2 flex items-center gap-2 rounded-sm bg-raised border border-line-strong hover:not-focus-within:border-fg-off focus-within:border-accent has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-accent has-[:focus-visible]:outline-offset-1 transition-colors duration-80 ease-linear">
          <Search size={14} className="shrink-0 text-fg-faint" />
          <input
            ref={field}
            value={query}
            placeholder="Search"
            aria-label="Search the browser"
            spellCheck={false}
            className="flex-1 min-w-0 bg-transparent outline-none text-base text-fg placeholder:text-fg-off"
            onChange={(e) => {
              setQuery(e.target.value);
              setPicked(null);
              setCursor(!!e.target.value.trim());
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") move(1);
              else if (e.key === "ArrowUp") move(-1);
              else if (e.key === "Enter" && current) enter();
              else if (e.key === "Escape") {
                if (query) setQuery("");
                else e.currentTarget.blur();
                setPicked(null);
                setCursor(false);
              } else return;
              e.preventDefault();
              e.stopPropagation();
            }}
          />
        </label>
      </div>
      <div
        ref={list}
        role="listbox"
        aria-label="Instruments and effects"
        tabIndex={0}
        aria-activedescendant={current ? `browser-${"instrument" in current ? current.instrument.component : current.effect.component}` : undefined}
        className="flex-1 min-h-0 overflow-y-auto px-2 pb-2 outline-none"
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") move(1);
          else if (e.key === "ArrowUp") move(-1);
          else if (e.key === "Enter" && current) enter();
          else if (e.key === "Escape") {
            setPicked(null);
            setCursor(false);
          }
          else return;
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        {/* Instruments by their groups (Drums, Instruments, Sound design), each a section as Effects is: no group in a group. */}
        {playable.map((g, k) => (
          <div key={g.name} role="group" aria-label={g.name} data-browser-category={`instruments:${g.name}`} className={clsx(k > 0 && "mt-2")}>
            <button
              aria-expanded={g.open}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => blank && fold(`instruments:${g.name}`)}
              className="w-full h-ctl-sm px-1 flex items-center rounded-sm text-sm font-semibold text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
            >
              {g.name}
            </button>
            {g.open && g.items.map(instrumentRow)}
          </div>
        ))}
        {pluginGroup("instruments", () => insPlugins.map(instrumentRow))}
        <div className={clsx("h-ctl-sm px-1 flex items-center text-sm font-semibold text-fg", playable.length > 0 && "mt-2")}>Effects</div>
        {flat.length === 0 && query && <NoMatch query={query} className="mt-2" />}
        {shown.map((g) => {
          const { Icon } = category(g.name);
          return (
            <div key={g.name} role="group" aria-label={g.name} data-browser-category={g.name}>
              <button
                aria-expanded={g.open}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => blank && fold(g.name)}
                className="w-full h-ctl-sm px-1 flex items-center gap-1 rounded-sm text-sm font-semibold text-fg-muted hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
              >
                <ChevronRight size={12} className={clsx("shrink-0 transition-transform duration-140 ease-out", g.open && "rotate-90")} />
                <span className="w-4 shrink-0 grid place-items-center">
                  <Icon size={14} className="text-fg-faint" />
                </span>
                {g.name}
              </button>
              {g.open && g.items.map(({ effect: e }) => effectRow(e))}
            </div>
          );
        })}
        {pluginGroup("effects", () => fxPlugins.map((e) => effectRow(e)))}
        {!query.trim() && <PacksList />}
      </div>
    </aside>
  );
}

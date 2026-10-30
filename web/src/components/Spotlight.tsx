import clsx from "clsx";
import { CornerDownLeft, FolderPlus, Loader2, Search, Volume2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { lockReason, moduleName, type GraphState } from "../lib/graph";
import { addSample, sampleTarget } from "../lib/audioImport";
import { beatNow } from "../lib/clock";
import {
  dragSample,
  fromWords,
  GROUPS,
  SAMPLE_DRAG,
  sampleFolders,
  sampleWords,
  stopPreview,
  togglePreview,
  useFinding,
  usePreview,
  useSearch,
  type LibraryItem,
  type SampleFolder,
} from "../lib/library";
import { chord, native, nativeAction } from "../lib/native";
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

/** What a dragged sound row carries: the item, as JSON. */
export const SOUND_DRAG = "application/x-m2-sound";

/** Whether a drag carries a sound from ⌘F's panel. */
export const dragsSound = (e: React.DragEvent) => e.dataTransfer.types.includes(SOUND_DRAG);

/** The sound a drop carries, if any. */
export function droppedSound(e: React.DragEvent): LibraryItem | null {
  try {
    const item = JSON.parse(e.dataTransfer.getData(SOUND_DRAG) || "null");
    return item?.kind === "sound" && item.preset ? item : null;
  } catch {
    return null;
  }
}

/**
 * Load sound `item` (a preset) onto the instrument or effect it is for on track `module` (the
 * selected track when not given): one Undo step, as the card's Presets does. A track that doesn't
 * play it is said in words.
 */
export function loadSound(item: LibraryItem, graph: GraphState, module = target()) {
  const toast = useStore.getState().toast;
  const preset = item.preset;
  if (!preset) return;
  const why = lockReason(graph, useStore.getState().connected);
  if (why) return toast(`${why}.`);
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
  if (item.kind === "sample") return void addSample(item, sampleTarget(graph));
  toast(`${item.label} can't be added yet.`);
}

/** Add a folder of your samples (LIB-5): the app's own folder dialog, else its path typed. */
export async function addSampleFolder(): Promise<SampleFolder[] | null> {
  let path: string | null = null;
  if (native) {
    const picked = await nativeAction("song_folder", JSON.stringify({ title: "Add a Folder of Samples" }));
    path = typeof picked === "string" ? picked : null;
  } else path = window.prompt("The folder of samples to add (its whole path):");
  if (!path?.trim()) return null;
  return sampleFolders({ add: path.trim() });
}

/** What Enter does with `item`, in words, for the panel's foot. */
function enterWords(item: LibraryItem, graph: GraphState): string {
  const module = target();
  const name = module ? moduleName(graph.view.modules.find((m) => m.key === module) ?? { key: module }) : null;
  if (item.kind === "instrument" || (item.kind === "plugin" && item.instrument)) return "adds a track";
  if (item.kind === "effect" || item.kind === "plugin") return name ? `adds it to ${name}` : "select a track first";
  if (item.kind === "sound") return name ? `loads it on ${name}` : "select a track first";
  if (item.kind === "sample") return "adds it at the playhead";
  return "adds it";
}

/**
 * ⌘F's panel (LIB-1): one search over everything that can be added, Spotlight-like. Typing filters;
 * ↑↓ move across the groups; Space plays the selected row's preview once the arrows (or a click)
 * picked it (again, or moving, stops it); Enter adds it, or, when the search hasn't answered the
 * words typed yet, their best hit once it has; Esc closes. A row drags onto the timeline or a
 * track, as the side browser's rows do; a sound onto a track loads into it. A click keeps the
 * keys in the panel.
 */
export default function Spotlight({ graph }: { graph: GraphState }) {
  const open = useFinding((s) => s.open);
  if (!open) return null;
  return <Panel graph={graph} />;
}

function Panel({ graph }: { graph: GraphState }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState<string | null>(null);
  // The arrows or a click picked the row: Space plays it, until a key types again.
  const [browsing, setBrowsing] = useState(false);
  const [said, setSaid] = useState<string | null>(null);
  // Fits my song (LIB-1): the playhead's beat when it was turned on.
  const [fits, setFits] = useState<number | null>(null);
  const [folders, setFolders] = useState<SampleFolder[] | null>(null);
  const preview = usePreview();
  const plugins = usePluginList((s) => s.plugins);
  const panel = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  // The words in the field now, and whether the panel is still open: an Enter that waits for the
  // search acts only on these.
  const typed = useRef(query);
  typed.current = query;
  const alive = useRef(true);
  useEffect(() => () => void (alive.current = false), []);
  const close = () => {
    stopPreview();
    useFinding.setState({ open: false });
  };

  // The installed plugins are asked for as it opens, as + Track's list asks.
  useEffect(() => wantPlugins(), []);
  useLayoutEffect(() => field.current?.focus(), []);
  // The search, as typing rests, and again when the plugin list comes, and while samples are being
  // indexed (LIB-5: a search never waits for them), until they are.
  const [round, setRound] = useState(0);
  const again = useMemo(() => ({ plugins, round }), [plugins, round]);
  const { found, now } = useSearch(query, again, 60, fits);
  const items = useMemo(() => found?.items ?? [], [found]);
  const indexing = (found?.folders?.indexing ?? 0) > 0;
  useEffect(() => {
    if (!indexing) return;
    const t = window.setTimeout(() => setRound((r) => r + 1), 1000);
    return () => window.clearTimeout(t);
  }, [indexing, found]);
  // Other words: their best hit is selected, wherever its group is, and nothing is picked yet. The
  // same words answered again (the plugin list came): the row selected stays, while it is there.
  const shownFor = useRef<string | null>(null);
  useEffect(() => {
    if (!found) return;
    if (found.error) setSaid(found.error);
    if (found.q !== shownFor.current) {
      shownFor.current = found.q;
      setActive(found.items[0]?.id ?? null);
      setBrowsing(false);
    } else setActive((a) => (found.items.some((i) => i.id === a) ? a : (found.items[0]?.id ?? null)));
  }, [found]);

  /** Type a filter word, or take it out (a chip clicked): what is typed and clicked is one query. */
  const toggleWord = (word: string, others: string[]) => {
    const words = query.split(/\s+/).filter(Boolean);
    const on = words.some((w) => w.toLowerCase() === word);
    const next = words.filter((w) => !others.includes(w.toLowerCase()) && w.toLowerCase() !== word);
    // Added at the end, with a space after it, so typing goes on after it.
    setQuery(on ? next.join(" ") : [...next, word].join(" ") + " ");
    setBrowsing(false);
    field.current?.focus();
  };
  const typedWord = (w: string) => query.toLowerCase().split(/\s+/).includes(w);
  const showFolders = () => void sampleFolders().then(setFolders, (e) => setSaid(e instanceof Error ? e.message : String(e)));
  // A folder being looked through is asked about again until it is done, and the search with it.
  const looking = folders?.some((f) => f.indexing) ?? false;
  useEffect(() => {
    if (!looking) return;
    const t = window.setTimeout(() => {
      void sampleFolders().then(
        (f) => {
          if (!alive.current) return;
          setFolders((shown) => (shown ? f : shown));
          if (!f.some((x) => x.indexing)) setRound((r) => r + 1);
        },
        () => {},
      );
    }, 1000);
    return () => window.clearTimeout(t);
  }, [looking, folders]);
  // A folder added or removed: the search is asked again, and again while it is looked through.
  const addFolder = () =>
    void addSampleFolder().then(
      (f) => {
        if (!f) return;
        setFolders(f);
        setRound((r) => r + 1);
      },
      (e) => setSaid(e instanceof Error ? e.message : String(e)),
    );
  const removeFolder = (path: string) =>
    void sampleFolders({ remove: path }).then(
      (f) => {
        setFolders(f);
        setRound((r) => r + 1);
      },
      (e) => setSaid(e instanceof Error ? e.message : String(e)),
    );

  const groups = useMemo(() => GROUPS.map((g) => ({ ...g, items: items.filter((i) => i.kind === g.kind) })).filter((g) => g.items.length), [items]);
  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups]);
  const current = flat.find((i) => i.id === active) ?? flat[0];
  useEffect(() => {
    list.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest" });
  }, [current?.id]);

  /** Select `item`, picked: what was playing stops. */
  const pick = (item: LibraryItem) => {
    setActive(item.id);
    setBrowsing(true);
    setSaid(null);
    stopPreview();
  };
  const move = (by: number) => {
    if (!flat.length) return;
    const at = current ? flat.indexOf(current) : -1;
    pick(flat[(at + by + flat.length) % flat.length]);
  };
  // A newer ask (or a stop) says its own: an older one's answer never overwrites it.
  const play = (item: LibraryItem) => void togglePreview(item).then((why) => why !== undefined && setSaid(why));
  const add = (item: LibraryItem | undefined) => {
    if (!item) return;
    close();
    addItem(item);
  };
  // Enter: the row selected, when the search answered the words in the field; else their best hit,
  // once the search answers them, if they are still the words in the field.
  const enter = () => {
    if (found?.q === query) return add(current);
    const words = query;
    void now().then(
      (best) => alive.current && typed.current === words && add(best[0]),
      (e) => alive.current && setSaid(e instanceof Error ? e.message : String(e)),
    );
  };

  return (
    <div
      ref={panel}
      role="dialog"
      aria-label="Find sounds"
      data-spotlight
      tabIndex={-1}
      className="fixed z-[60] left-1/2 top-[14vh] -translate-x-1/2 w-[560px] max-w-[calc(100vw-32px)] max-h-[64vh] flex flex-col rounded-md bg-raised shadow-pop overflow-hidden animate-rise cursor-default outline-none"
      // The panel's keys, wherever in it the focus is; a click anywhere in it gives the keys back to
      // the field, so Space, Enter and Esc stay the panel's.
      onClick={() => field.current?.focus()}
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") move(1);
        else if (e.key === "ArrowUp") move(-1);
        else if (e.key === " " && browsing && current) play(current);
        else if (e.key === "Enter") enter();
        else if (e.key === "Escape") close();
        else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      <div className="h-11 shrink-0 flex items-center gap-2 px-3 border-b border-line">
        <Search size={16} className="text-fg-faint shrink-0" />
        <input
          ref={field}
          value={query}
          placeholder="Find instruments, sounds, effects and samples"
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
        />
        <span className="shrink-0 text-xs text-fg-faint">{chord("F")}</span>
      </div>
      <div className="h-ctl shrink-0 flex items-center gap-1.5 px-3 border-b border-line text-xs" role="toolbar" aria-label="Filters">
        {[
          ["loop", "Loops", ["one-shot"]],
          ["one-shot", "One-shots", ["loop"]],
        ].map(([word, label, others]) => (
          <button
            key={word as string}
            type="button"
            data-spotlight-filter={word as string}
            aria-pressed={typedWord(word as string)}
            className={clsx("h-ctl-sm px-2 rounded-sm transition-colors duration-80", typedWord(word as string) ? "bg-accent-soft text-fg" : "text-fg-muted hover:bg-hover")}
            onClick={(e) => {
              e.stopPropagation();
              toggleWord(word as string, others as string[]);
            }}
          >
            {label as string}
          </button>
        ))}
        <button
          type="button"
          data-spotlight-filter="fits"
          aria-pressed={fits !== null}
          title="Loops near the song's tempo at the playhead, in its key or the key's relative"
          className={clsx("h-ctl-sm px-2 rounded-sm transition-colors duration-80", fits !== null ? "bg-accent-soft text-fg" : "text-fg-muted hover:bg-hover")}
          onClick={(e) => {
            e.stopPropagation();
            setFits(fits === null ? Math.max(0, beatNow()) : null);
            field.current?.focus();
          }}
        >
          Fits my song
        </button>
        <span className="flex-1" />
        <button
          type="button"
          data-spotlight-folders
          className="h-ctl-sm px-2 rounded-sm flex items-center gap-1 text-fg-muted hover:bg-hover"
          title="Your folders of samples"
          onClick={(e) => {
            e.stopPropagation();
            if (folders) setFolders(null);
            else showFolders();
          }}
        >
          <FolderPlus size={13} />
          Your folders
        </button>
      </div>
      {folders && (
        <div className="shrink-0 px-3 py-2 border-b border-line bg-chrome text-xs" data-spotlight-folder-list>
          {folders.length === 0 && <p className="text-fg-muted">No folders of yours yet. Add one, and its WAV and FLAC files are found here: nothing is copied until you add one to a song.</p>}
          {folders.map((f) => (
            <div key={f.path} className="h-ctl-sm flex items-center gap-2" data-spotlight-folder={f.path}>
              <span className="flex-1 min-w-0 truncate text-fg" title={f.path}>
                {f.path}
              </span>
              <span className="shrink-0 text-fg-muted">
                {f.missing
                  ? "not there"
                  : f.indexing
                    ? "looking…"
                    : `${f.samples} samples${f.unreadable ? ` · ${f.unreadable} don't read` : ""}${f.not_wav ? ` · ${f.not_wav} skipped: WAV and FLAC only for now` : ""}`}
              </span>
              <button
                type="button"
                aria-label={`Remove ${f.path}`}
                title="Remove: its files stay where they are"
                className="shrink-0 grid place-items-center w-5 h-5 rounded-sm text-fg-faint hover:bg-hover"
                onClick={(e) => {
                  e.stopPropagation();
                  removeFolder(f.path);
                }}
              >
                <X size={12} />
              </button>
            </div>
          ))}
          <button
            type="button"
            data-spotlight-add-folder
            className="mt-1 h-ctl-sm px-2 rounded-sm text-accent hover:bg-hover"
            onClick={(e) => {
              e.stopPropagation();
              addFolder();
            }}
          >
            Add a Folder…
          </button>
        </div>
      )}
      <div ref={list} id="spotlight-list" role="listbox" aria-label="Found" className="flex-1 min-h-0 overflow-y-auto p-1">
        {found?.q === query && flat.length === 0 && (
          <EmptyState bare className="mx-auto my-6">
            <div>
              <p>{query.trim() ? `Nothing matches “${query.trim()}”` : "Nothing to add"}</p>
              <p className="mt-1 text-xs">
                {indexing
                  ? "Looking through your samples…"
                  : fits !== null
                    ? "Fits my song keeps only loops near the song's tempo, in its key."
                    : "Try a word like “pad”, “drums”, “kick” or “echo”."}
              </p>
            </div>
          </EmptyState>
        )}
        {groups.map((g) => (
          <div key={g.kind} role="group" aria-label={g.name} data-spotlight-group={g.kind}>
            <div className="h-ctl-sm px-2 flex items-center text-sm font-semibold text-fg-muted">{g.name}</div>
            {g.items.map((item) => {
              const on = item === current;
              const drag =
                item.kind === "instrument" || (item.kind === "plugin" && item.instrument)
                  ? INSTRUMENT_DRAG
                  : item.kind === "effect" || item.kind === "plugin"
                    ? EFFECT_DRAG
                    : item.kind === "sound" && item.preset
                      ? SOUND_DRAG
                      : item.kind === "sample"
                        ? SAMPLE_DRAG
                        : null;
              const sound = item.kind === "sound" ? item.tags[0] : item.kind === "sample" ? sampleWords(item) : item.category;
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
                  onPointerDown={() => pick(item)}
                  onDoubleClick={() => add(item)}
                  onDragStart={(ev) => {
                    if (!drag) return;
                    ev.dataTransfer.setData(drag, drag === SOUND_DRAG ? JSON.stringify(item) : drag === SAMPLE_DRAG ? item.id : item.component);
                    ev.dataTransfer.effectAllowed = "copy";
                    if (drag === SAMPLE_DRAG) dragSample(item);
                  }}
                  onDragEnd={() => {
                    dragSample(null);
                    close();
                  }}
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
          {said ??
            (current
              ? `Enter ${enterWords(current, graph)}${current.preview ? " · Space plays it" : " · no preview"}`
              : indexing
                ? "Looking through your samples…"
                : "Type to find")}
        </span>
        <span className="shrink-0">↑↓ · Esc</span>
      </div>
    </div>
  );
}

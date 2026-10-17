import clsx from "clsx";
import { ArrowLeftRight, Check, CodeXml, Copy, Ellipsis, Pencil, Search, SlidersHorizontal, Trash2, VolumeX } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { create } from "zustand";
import { graphApi, lockReason, moduleName, type Dependent, type GraphState, type Instrument } from "../lib/graph";
import { trackMade } from "../lib/events";
import { changeInstrument, currentInstrument, notesLane } from "../lib/instrument";
import { chord, opt } from "../lib/native";
import { muteInSongCall } from "../lib/overlays";
import { DEFAULT_ORDER, TRACK_COLORS, type TrackColorName } from "../lib/palette";
import { useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { undoAction } from "./GraphCanvas";
import { Button, MenuRow, Popover } from "./ui";

// Tracks (design-8b §3, taste.md §3.4, TL-2, TL-3): + Track and its instrument list, the header
// menu, duplicate, delete with its dialog, reorder by dragging a header, and colours by name.
// Every one is one undo step on the server; the timeline shows the result, so only what may be off
// screen or is structural (add, duplicate, delete) says so in a toast (taste.md §2.14).

/** What a dragged instrument row of the browser carries: its component (`std:synth-pluck`). */
export const INSTRUMENT_DRAG = "application/x-m2-instrument";

/** ⌘⌫ as this platform writes it. */
const DELETE_KEY = chord("Backspace");

/** The instrument this window added last: the list opens on it, so ⌘T then Enter adds it again (Live's speed). */
let lastAdded: string | null = null;

/** A delete that asks first: other code reads the track. */
interface Asking {
  module: string;
  name: string;
  dependents: Dependent[];
  blocked: boolean;
}

interface TracksUi {
  /**
   * The instrument list, anchored to what opened it, and the track the new one goes below; or, to
   * change a track's instrument (`swap`), that track.
   */
  picker: { anchor: HTMLElement; after: string | null; swap?: string } | null;
  /** A track's header menu, anchored to its ⋯. */
  menu: { module: string; anchor: HTMLElement } | null;
  asking: Asking | null;
}

const useTracksUi = create<TracksUi>(() => ({ picker: null, menu: null, asking: null }));

/** A header being dragged to a new place: the track, how far it moved, and the track it would go before. */
export const END = "\0end";
export const useReorder = create<{ module: string | null; dy: number; before: string | null }>(() => ({ module: null, dy: 0, before: null }));

/** An error in words: a builder's refusal says why, without the "edit rejected" in front. */
const message = (error: unknown) => (error instanceof Error ? error.message : String(error)).replace(/^edit rejected: /, "");

/** The open song, and why it can't be edited now (null: it can), said once in a toast. */
function writable(): GraphState | null {
  const { graph, connected, toast } = useStore.getState();
  if (!graph) return null;
  const why = lockReason(graph, connected);
  if (why) {
    toast(`${why}.`);
    return null;
  }
  return graph;
}

/** What a track is called on screen. */
function nameOf(graph: GraphState, module: string) {
  const t = graph.timeline.tracks.find((x) => x.module === module);
  return moduleName({ key: module, label: t?.label ?? graph.view.modules.find((m) => m.key === module)?.label });
}

/** The graph as the server has it now, so what an edit made can be selected at once. */
async function refreshed() {
  const graph = await graphApi.get();
  useStore.getState().set({ graph });
  return graph;
}

/** Select a track an edit made and light its row (taste.md §2.13): nothing else opens, so Space, M and S work at once. */
function arrived(module: string, lane: string | undefined) {
  useShell.getState().select(module);
  if (lane) trackMade(module, lane);
}

/** Open the instrument list at `anchor`; the new track goes below `after`. */
export function openInstruments(anchor: HTMLElement, after: string | null) {
  useTracksUi.setState({ picker: { anchor, after }, menu: null });
}

/** Change instrument…: the list, at `anchor`, to change `module`'s instrument (polish A3). */
export function openChangeInstrument(module: string, anchor: HTMLElement) {
  const graph = writable();
  if (!graph || module === "song") return;
  useTracksUi.setState({ picker: { anchor, after: null, swap: module }, menu: null });
}

/** ⌘T: the list, at + Track, for a track below the selected one. */
export function newTrack() {
  const anchor = document.querySelector<HTMLElement>("[data-testid=add-track]");
  if (!anchor || !writable()) return;
  const s = useShell.getState();
  openInstruments(anchor, s.selected && s.selected !== "song" ? s.selected : null);
}

/** Add a track playing `instrument` below `after` (else last): selected and lit, with an Undo. */
export async function addTrack(instrument: Instrument, after: string | null) {
  const graph = writable();
  if (!graph) return;
  lastAdded = instrument.component;
  const toast = useStore.getState().toast;
  try {
    const r = await graphApi.addTrack(graph.session, instrument, { after });
    await refreshed();
    arrived(r.module, instrument.lane);
    toast(r.label === instrument.label ? `Added ${r.label}` : `Added ${r.label} (${instrument.label})`, undoAction(graph.session, r.revision));
  } catch (error) {
    toast(message(error));
  }
}

/** Duplicate `module` (⌘D on its header): the copy goes below it, in its colour, selected and lit. */
export async function duplicateTrack(module: string) {
  const graph = writable();
  if (!graph || module === "song") return;
  const name = nameOf(graph, module);
  const lane = graph.timeline.tracks.find((t) => t.module === module)?.lanes.find((l) => l.kind !== "control")?.key;
  const toast = useStore.getState().toast;
  try {
    const r = await graphApi.duplicateTrack(graph.session, module, name);
    await refreshed();
    arrived(r.module, lane);
    toast(r.new_channel ? `Duplicated ${name} into the mixer` : `Duplicated ${name}`, undoAction(graph.session, r.revision));
  } catch (error) {
    toast(message(error));
  }
}

/** "2 Ducks", "the Duck": what a cascade takes off with the track. */
function what(deps: Dependent[]) {
  const names = [...new Set(deps.map((d) => d.component))];
  if (deps.length === 1) return `its ${names[0]}`;
  return names.length === 1 ? `${deps.length} ${names[0]}s` : `${deps.length} effects`;
}

/** Delete `module` (⌘⌫): at once when nothing else reads it, else the dialog asks (design-8b §3.5). */
export async function deleteTrack(module: string, cascade = false) {
  const graph = writable();
  if (!graph || module === "song") return;
  const name = nameOf(graph, module);
  const toast = useStore.getState().toast;
  try {
    const r = await graphApi.deleteTrack(graph.session, module, name, cascade);
    if (r.asks) return void useTracksUi.setState({ asking: { module, name, ...r.asks }, menu: null });
    const taken = r.done.excised ?? [];
    toast(taken.length ? `Deleted ${name} and ${what(taken)}` : `Deleted ${name}`, undoAction(graph.session, r.done.revision));
  } catch (error) {
    toast(message(error));
  }
}

/**
 * Mute `module` in the song, or unmute it (MON-5, design-8b §2.4): its `muted` option in its code,
 * so the export leaves it out too. One undo step, said in a toast, since it lasts: "Kick muted in
 * the song", with Undo.
 */
export async function muteInSong(module: string, muted: boolean) {
  const graph = writable();
  if (!graph || module === "song") return;
  const name = nameOf(graph, module);
  const toast = useStore.getState().toast;
  try {
    const r = await muteInSongCall(graph.session, [module], muted);
    if (r.before !== r.after) toast(`${name} ${muted ? "muted" : "unmuted"} in the song`, undoAction(graph.session, r.revision));
  } catch (error) {
    toast(message(error));
  }
}

/** Give `module` the colour `colour`: a layout write, no toast (the header shows it). */
export function recolour(module: string, colour: string) {
  const graph = writable();
  if (!graph) return;
  graphApi
    .layout(graph.session, { colour: { module, colour } }, { name: `${nameOf(graph, module)} colour` })
    .catch((error) => useStore.getState().toast(message(error)));
}

/** The tracks, top to bottom, with `module` moved before `before` (END: last). */
export function moved(order: string[], module: string, before: string) {
  const rest = order.filter((k) => k !== module);
  const at = before === END ? rest.length : rest.indexOf(before);
  return at < 0 ? order : [...rest.slice(0, at), module, ...rest.slice(at)];
}

/** The tracks of the timeline, top to bottom. */
const trackOrder = (graph: GraphState) => graph.timeline.tracks.filter((t) => t.track && t.module !== "song").map((t) => t.module);

/**
 * Drag a track's header up or down to move it (taste.md §3.4): after 4px, the header follows the
 * pointer and a 2px line shows where it lands; letting go writes the new order (no toast; ⌘Z puts
 * it back). The Song row is pinned and never a target.
 */
export function startReorder(e: React.PointerEvent, module: string) {
  if (e.button !== 0 || module === "song") return;
  const graph = useStore.getState().graph;
  if (!graph || lockReason(graph, useStore.getState().connected)) return;
  const y0 = e.clientY;
  // Where each track's rows are, before anything moves.
  const rows = [...document.querySelectorAll<HTMLElement>("[data-testid=timeline] [data-track]")]
    .filter((el) => el.dataset.track !== "song" && !el.closest("[data-song-row]"))
    .map((el) => ({ module: el.dataset.track!, mid: el.getBoundingClientRect().top + el.getBoundingClientRect().height / 2 }));
  const order = trackOrder(graph);
  let dragging = false;
  let before: string | null = null;
  const move = (ev: PointerEvent) => {
    if (!dragging && Math.abs(ev.clientY - y0) < 4) return;
    dragging = true;
    const next = rows.find((r) => r.module !== module && r.mid > ev.clientY)?.module ?? END;
    // A drop that changes nothing shows no line.
    before = moved(order, module, next).join() === order.join() ? null : next;
    useReorder.setState({ module, dy: ev.clientY - y0, before });
  };
  const end = (ev: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", end);
    window.removeEventListener("pointercancel", end);
    useReorder.setState({ module: null, dy: 0, before: null });
    if (!dragging || ev.type !== "pointerup" || !before) return;
    const name = nameOf(graph, module);
    graphApi.layout(graph.session, { order: moved(order, module, before) }, { name: `moving ${name}` }).catch((error) => useStore.getState().toast(message(error)));
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", end);
  window.addEventListener("pointercancel", end);
}

/** The header's ⋯: its menu (also on a right-click anywhere on the header). */
export function openTrackMenu(module: string, anchor: HTMLElement) {
  useTracksUi.setState({ menu: { module, anchor }, picker: null });
}

/** The project file a track's code is in: where most of its nodes are. */
function fileOf(graph: GraphState, module: string) {
  const count = new Map<string, number>();
  for (const n of graph.view.modules.find((m) => m.key === module)?.nodes ?? []) if (n.file) count.set(n.file, (count.get(n.file) ?? 0) + 1);
  return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
}

/** A key the way a new track's is proposed (ID-2), for the list's foot: close enough to say where it goes. */
function proposed(label: string, taken: Set<string>) {
  const base = label.normalize("NFKD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 32) || "track";
  if (!taken.has(base)) return base;
  for (let n = 2; ; n++) if (!taken.has(`${base}-${n}`)) return `${base}-${n}`;
}

/**
 * Instruments matching `query`, in their groups (drums first): by name, as a typeahead takes it
 * ("plu" is Pluck, though a description says "plus"), else by description or category.
 */
export function instrumentGroups(list: Instrument[], query: string) {
  const q = query.trim().toLowerCase();
  const by = (fields: (i: Instrument) => string[]) => list.filter((i) => fields(i).some((t) => t.toLowerCase().includes(q)));
  const named = by((i) => [i.label, i.name]);
  const hits = !q ? list : named.length ? named : by((i) => [i.description, i.category]);
  const groups: { name: string; items: Instrument[] }[] = [];
  for (const i of hits) {
    const g = groups.find((x) => x.name === i.category) ?? (groups.push({ name: i.category, items: [] }), groups[groups.length - 1]);
    g.items.push(i);
  }
  return groups;
}

/**
 * The instrument list (TL-3, BR-1, taste.md §3.6): the browser's Instruments as a popover at + Track.
 * Typing filters, ↑↓ move, Enter adds the keyboard-active row: the instrument added last in this
 * window, else the first. The foot shows the code it writes.
 *
 * To change a track's instrument (`swap`, polish A3): titled for the track, the instrument it plays
 * ticked and active, and Enter changes it.
 */
function InstrumentPicker({
  graph,
  anchor,
  after,
  swap,
  onClose,
}: {
  graph: GraphState;
  anchor: HTMLElement;
  after: string | null;
  swap?: string;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const all = graph.instruments ?? [];
  const groups = useMemo(() => instrumentGroups(all, query), [all, query]);
  const flat = groups.flatMap((g) => g.items);
  const playing = swap ? currentInstrument(graph, swap) : undefined;
  const [active, setActive] = useState(() => Math.max(0, all.findIndex((i) => i.component === (swap ? playing?.component : lastAdded))));
  const current = flat[Math.min(active, flat.length - 1)];
  const list = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    list.current?.querySelector("[data-active]")?.scrollIntoView({ block: "nearest" });
  }, [active, query]);
  // Typing filters at once, and Enter adds, however soon after it opens.
  useLayoutEffect(() => field.current?.focus(), []);
  const taken = new Set(graph.view.modules.map((m) => m.key));
  const name = swap ? nameOf(graph, swap) : "";
  const pick = (i: Instrument | undefined) => {
    if (!i) return;
    onClose();
    if (!swap) return void addTrack(i, after);
    // The one it plays: nothing to change.
    if (i.component !== playing?.component) void changeInstrument(swap, i);
  };
  const step = (by: number) => setActive((a) => (flat.length ? (Math.min(a, flat.length - 1) + by + flat.length) % flat.length : 0));
  let index = 0;
  return (
    <Popover
      anchor={anchor}
      onClose={onClose}
      role="dialog"
      aria-label={swap ? `Change ${name}'s instrument` : "Add a track"}
      data-testid="instrument-list"
      data-swap={swap}
      className="w-[280px] !p-0 flex flex-col"
    >
      {swap && (
        <div data-testid="instrument-list-title" className="h-8 shrink-0 flex items-center px-3 border-b border-line text-sm font-semibold text-fg-strong truncate">
          {`Change ${name}'s instrument`}
        </div>
      )}
      <div className="h-10 shrink-0 flex items-center gap-2 px-3 border-b border-line">
        <Search size={14} className="shrink-0 text-fg-faint" />
        <input
          ref={field}
          value={query}
          placeholder={swap ? "Find an instrument…" : "Add a track…"}
          aria-label="Search instruments"
          role="combobox"
          aria-expanded
          aria-controls="instrument-options"
          aria-activedescendant={current ? `instrument-${current.component}` : undefined}
          spellCheck={false}
          className="flex-1 min-w-0 bg-transparent outline-none text-base text-fg placeholder:text-fg-off"
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") step(1);
            else if (e.key === "ArrowUp") step(-1);
            else if (e.key === "Enter") pick(current);
            else return;
            e.preventDefault();
            e.stopPropagation();
          }}
        />
      </div>
      <div ref={list} id="instrument-options" role="listbox" aria-label="Instruments" className="max-h-[340px] overflow-y-auto p-1">
        {flat.length === 0 && <p className="px-2 py-4 text-center text-sm text-fg-muted">No instrument is called “{query.trim()}”</p>}
        {groups.map((g) => (
          <div key={g.name} role="group" aria-label={g.name}>
            <div className="h-ctl-sm px-2 flex items-center text-sm font-semibold text-fg-muted">{g.name}</div>
            {g.items.map((i) => {
              const k = index++;
              const on = i === current;
              const ticked = !!swap && i.component === playing?.component;
              return (
                <div
                  key={i.component}
                  id={`instrument-${i.component}`}
                  role="option"
                  aria-selected={on}
                  aria-checked={swap ? ticked : undefined}
                  data-instrument={i.component}
                  data-active={on || undefined}
                  className={clsx("min-h-ctl px-2 py-1 rounded-sm cursor-pointer transition-colors duration-80 ease-linear flex gap-2", on && "bg-accent-soft")}
                  onPointerMove={() => k !== active && setActive(k)}
                  onClick={() => pick(i)}
                >
                  {/* What it plays now: a check at the left, as a menu's chosen row (taste.md §2.9). */}
                  {swap && <span className="w-3.5 shrink-0 mt-[3px] text-fg">{ticked && <Check size={14} strokeWidth={1.5} absoluteStrokeWidth />}</span>}
                  <span className="flex-1 min-w-0">
                    <span className="block text-sm text-fg">{i.label}</span>
                    {i.description && <span className="block text-xs text-fg-muted truncate">{i.description}</span>}
                  </span>
                </div>
              );
            })}
          </div>
        ))}
      </div>
      {/* Literally code: what adding it writes (BR-5). */}
      {current && (
        <div className="h-ctl shrink-0 flex items-center px-3 border-t border-line bg-chrome rounded-b-md text-xs text-fg-muted">
          <span data-code data-testid="instrument-code" className="flex-1 min-w-0 truncate font-mono" title={swap ? "What changing it writes in your code" : "What adding it writes in your code"}>
            {swap
              ? `${fileOf(graph, swap) ?? "its code"} · ${current.name}(lane.notes("${notesLane(graph, swap)}"))`
              : `tracks/${proposed(current.label, taken)}.ts · ${current.name}(lane.notes("${current.lane}"))`}
          </span>
        </div>
      )}
    </Popover>
  );
}

/** Colour ▸: a 6×2 grid of the palette's swatches, a check on the current one (taste.md §2.7 rule 6). */
export function Colours({ module, current, anchor, onClose }: { module: string; current: string | undefined; anchor: HTMLElement; onClose: () => void }) {
  return (
    <Popover anchor={anchor} onClose={onClose} side="right" role="menu" aria-label="Colour" data-testid="colours" className="grid grid-cols-6 gap-1.5 !p-2">
      {DEFAULT_ORDER.map((name) => (
        <button
          key={name}
          role="menuitemradio"
          aria-checked={name === current}
          aria-label={name}
          data-colour={name}
          title={name[0].toUpperCase() + name.slice(1)}
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => {
            onClose();
            if (name !== current) recolour(module, name);
          }}
          className="size-4 rounded-xs grid place-items-center cursor-pointer outline-offset-1"
          style={{ background: TRACK_COLORS[name as TrackColorName].bar }}
        >
          {name === current && <Check size={12} strokeWidth={2} absoluteStrokeWidth className="text-white" />}
        </button>
      ))}
    </Popover>
  );
}

/**
 * A track's header menu (taste.md §3.4): Rename, Duplicate, Delete; Colour ▸, automation, Mute in
 * song; Edit in code. On a track muted in the song, Unmute in song comes first: a click on its M
 * opens this menu for it (taste.md §2.9).
 */
function TrackMenu({ graph, module, anchor, onClose }: { graph: GraphState; module: string; anchor: HTMLElement; onClose: () => void }) {
  const [colours, setColours] = useState<HTMLElement | null>(null);
  const locked = lockReason(graph, useStore.getState().connected);
  const track = graph.timeline.tracks.find((t) => t.module === module);
  const automation = !!track?.lanes.some((l) => l.kind === "control");
  const open = useShell((s) => s.open.has(module));
  const file = fileOf(graph, module);
  const run = (f: () => void) => () => {
    onClose();
    f();
  };
  const sep = <div role="separator" className="my-1 h-px bg-line" />;
  const inSong = graph.overlays?.heard[module] === "muted-in-song";
  const songMute = (
    <MenuRow
      // One icon either way: the row is about the song's mute, and its words say which way.
      icon={<VolumeX size={14} />}
      disabled={!!locked}
      title={locked ?? undefined}
      data-testid="mute-in-song"
      detail={inSong ? undefined : "Saved in its code: the export leaves it out too"}
      onClick={run(() => void muteInSong(module, !inSong))}
    >
      {inSong ? "Unmute in song" : "Mute in song"}
    </MenuRow>
  );
  return (
    <Popover anchor={anchor} onClose={onClose} role="menu" aria-label={`${nameOf(graph, module)} menu`} data-testid="track-menu" className="w-[240px]">
      {inSong && (
        <>
          {songMute}
          {sep}
        </>
      )}
      <MenuRow
        icon={<Pencil size={14} />}
        shortcut={chord("R")}
        disabled={!!locked}
        title={locked ?? undefined}
        onClick={run(() => {
          useShell.getState().select(module);
          useShell.getState().set({ renaming: { module, at: "header" } });
        })}
      >
        Rename
      </MenuRow>
      <MenuRow
        icon={<ArrowLeftRight size={14} />}
        disabled={!!locked}
        title={locked ?? undefined}
        data-testid="change-instrument"
        onClick={() => openChangeInstrument(module, anchor)}
      >
        Change instrument…
      </MenuRow>
      <MenuRow icon={<Copy size={14} />} shortcut={chord("D")} disabled={!!locked} onClick={run(() => void duplicateTrack(module))}>
        Duplicate
      </MenuRow>
      <MenuRow icon={<Trash2 size={14} />} shortcut={DELETE_KEY} disabled={!!locked} onClick={run(() => void deleteTrack(module))}>
        Delete
      </MenuRow>
      {sep}
      <MenuRow
        icon={<span className="block size-3.5 rounded-xs" style={{ background: TRACK_COLORS[track?.colour as TrackColorName]?.bar }} />}
        disabled={!!locked}
        aria-haspopup="menu"
        aria-expanded={!!colours}
        data-testid="colour-row"
        onClick={(e) => setColours(colours ? null : e.currentTarget)}
        shortcut="›"
      >
        Colour
      </MenuRow>
      {automation && (
        <MenuRow icon={<SlidersHorizontal size={14} />} shortcut="A" onClick={run(() => useShell.getState().toggleLanes(module, []))}>
          {open ? "Hide automation" : "Show automation"}
        </MenuRow>
      )}
      {!inSong && songMute}
      {sep}
      <MenuRow
        icon={<CodeXml size={14} />}
        shortcut={opt("4")}
        detail={file ? <span data-code className="font-mono">{file}</span> : undefined}
        onClick={run(() => {
          useShell.getState().select(module);
          useShell.getState().setView("code");
        })}
      >
        Edit in code
      </MenuRow>
      {colours && <Colours module={module} current={track?.colour} anchor={colours} onClose={() => (setColours(null), onClose())} />}
    </Popover>
  );
}

/** "Bass and Chords", from the tracks the reads are in. */
function joined(names: string[]) {
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : (names[0] ?? "");
}

/** What the delete dialog says (design-8b §3.5), in words: who reads the track, and what goes with it. */
function said({ name, dependents, blocked }: Asking) {
  const readers = [...new Set(dependents.map((d) => d.label))];
  const many = readers.length > 1;
  if (blocked) {
    const d = dependents[0];
    return `${joined(readers)} ${many ? "read" : "reads"} it in ${many ? "their" : "its"} code (${d?.file}, line ${d?.line}).`;
  }
  const these = dependents.length === 1 ? "that" : "those";
  const off = what(dependents).replace(/^its /, "");
  if (dependents.every((d) => d.component === "Duck")) return `${joined(readers)} ${many ? "duck" : "ducks"} to ${name}. Deleting ${name} also takes off ${these} ${off}.`;
  return `${joined(readers)} ${many ? "read" : "reads"} ${name} through ${off}. Deleting ${name} also takes ${these} off.`;
}

/** The delete dialog (taste.md §2.14): the ducks a delete takes off too, or why it can't. Enter confirms, Esc cancels. */
function DeleteDialog({ asking, onClose }: { asking: Asking; onClose: () => void }) {
  const { module, name, dependents, blocked } = asking;
  const confirm = () => {
    onClose();
    if (!blocked) void deleteTrack(module, true);
  };
  const show = () => {
    onClose();
    const d = dependents[0];
    if (d) useShell.getState().select(d.module);
    useShell.getState().setView("code");
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      else if (e.key === "Enter") (blocked ? onClose : confirm)();
      else return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  });
  return (
    <div className="fixed inset-0 z-[70] grid place-items-center p-4 bg-fg-strong/20" onPointerDown={(e) => e.target === e.currentTarget && onClose()}>
      <div role="alertdialog" aria-modal aria-labelledby="delete-title" data-testid="delete-dialog" className="w-[440px] max-w-full p-6 rounded-md bg-raised shadow-pop animate-dialog cursor-default">
        <h2 id="delete-title" className="text-lg font-semibold text-fg-strong">
          {blocked ? `${name} can't be deleted from here` : `Delete ${name}?`}
        </h2>
        <p className="mt-2 text-base text-fg-muted">{said(asking)}</p>
        <div className="mt-6 flex justify-end gap-2">
          {blocked ? (
            <>
              <Button variant="secondary" size="lg" onClick={show}>
                Show in code
              </Button>
              <Button variant="primary" size="lg" autoFocus onClick={onClose}>
                Close
              </Button>
            </>
          ) : (
            <>
              <Button variant="secondary" size="lg" onClick={onClose}>
                Cancel
              </Button>
              <Button variant="danger" size="lg" autoFocus data-testid="delete-confirm" onClick={confirm}>
                {`Delete ${name} and ${dependents.length === 1 ? `the ${dependents[0].component}` : what(dependents)}`}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** Where the track popovers and the delete dialog show: mounted once with the song. */
export function TracksUi({ graph }: { graph: GraphState }) {
  const { picker, menu, asking } = useTracksUi();
  const close = () => useTracksUi.setState({ picker: null, menu: null });
  // A new song: nothing of the last one's stays open.
  useEffect(() => () => useTracksUi.setState({ picker: null, menu: null, asking: null }), [graph.session]);
  return (
    <>
      {picker && <InstrumentPicker graph={graph} anchor={picker.anchor} after={picker.after} swap={picker.swap} onClose={close} />}
      {menu && <TrackMenu graph={graph} module={menu.module} anchor={menu.anchor} onClose={close} />}
      {asking && <DeleteDialog asking={asking} onClose={() => useTracksUi.setState({ asking: null })} />}
    </>
  );
}

/** The ⋯ on a track's header, shown on hover: its menu. */
export function TrackMenuButton({ module, name }: { module: string; name: string }) {
  return (
    <button
      data-track-menu={module}
      aria-label={`${name} menu`}
      aria-haspopup="menu"
      title="More (or right-click the header)"
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => openTrackMenu(module, e.currentTarget)}
      className="size-ctl-xs shrink-0 grid place-items-center rounded-sm text-fg-muted opacity-0 group-hover/head:opacity-100 focus-visible:opacity-100 hover:text-fg hover:bg-press cursor-pointer transition-[opacity,color,background-color] duration-80 ease-linear"
    >
      <Ellipsis size={14} />
    </button>
  );
}

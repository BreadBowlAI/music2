// The ruler's markers (Q13, design-8b §7.6): a 1 px tick at each in the ruler's top 14 px band,
// its name beside it on `chrome`, cut where the next one starts; the bar numbers are on the line
// under the band (next D4a), so a marker never hides one. A click
// goes there; a drag moves it on the timeline's grid (⌘ held, Ctrl on a PC: freely); a
// double-click renames it; its right-click menu renames, deletes or loops to the next one. A new
// one opens as a field with a short list of names, and is written once its name is kept.

import clsx from "clsx";
import { useEffect, useRef, useState } from "react";
import { barAt, type TimelineView } from "../lib/graph";
import { skipsSnap, snapSong } from "../lib/grid";
import { barOf, markerAt, markersOf, NAME_MAX, nextName, PICKS, useMarkerEdit, usePendingMarkers, writeMarkers, type Marker } from "../lib/markers";
import { isMac, mod, opt } from "../lib/native";
import { useStore } from "../lib/store";
import PointMenu from "./ClipMenu";
import { MenuRow, Popover } from "./ui";

/** The markers as this window draws them: what it just sent, until the timeline shows it. */
export function useShownMarkers(timeline: TimelineView) {
  const list = markersOf(timeline);
  const pending = usePendingMarkers((s) => s.pending);
  return pending && pending.since === list ? pending.list : list;
}

/** Each marker's words for Undo: "Chorus at bar 17", "moving Chorus". */
const at = (timeline: TimelineView, m: Marker) => `${m.name} at bar ${barOf(timeline.meter, m.beat)}`;

export function RulerMarkers({
  timeline,
  ppb,
  width,
  locked,
  onSeek,
  onLoop,
}: {
  timeline: TimelineView;
  ppb: number;
  /** The ruler's width, in px. */
  width: number;
  locked: string | null;
  onSeek: (beat: number) => void;
  onLoop: (start: number, end: number) => void;
}) {
  const list = useShownMarkers(timeline);
  const editing = useMarkerEdit((s) => s.editing);
  // A marker being dragged, drawn where it goes until let go.
  const [drag, setDrag] = useState<{ from: Marker; to: number } | null>(null);
  const [menu, setMenu] = useState<{ marker: Marker; x: number; y: number } | null>(null);
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  // Each as drawn, with the marker of the list it is (`own`): one being dragged where it goes, and
  // while a new one is named, it too where it goes (no `own` yet).
  const drawn: { m: Marker; own: Marker | null }[] = [
    ...list.map((m) => ({ m: drag && m === drag.from ? { ...m, beat: drag.to } : m, own: m })),
    ...(editing && !editing.from && !markerAt(list, editing.beat) ? [{ m: { beat: editing.beat, name: nextName(list) }, own: null }] : []),
  ].sort((a, b) => a.m.beat - b.m.beat);
  const write = (next: Marker[], step: string) => void writeMarkers(next, { name: step });
  const keep = (name: string) => {
    if (!editing) return;
    const from = editing.from;
    useMarkerEdit.setState({ editing: null });
    const n = name.trim();
    if (from) {
      if (n && n !== from.name) write(list.map((m) => (m === from ? { ...m, name: n } : m)), `renaming ${from.name}`);
      return;
    }
    const made = { beat: editing.beat, name: n || nextName(list) };
    write([...list, made], at(timeline, made));
  };
  /** Loop from `m` to the next marker, or the song's end; a marker past the end, its bar. */
  const loopFrom = (m: Marker) => {
    const end = list.find((x) => x.beat > m.beat + 1e-6)?.beat ?? timeline.end;
    onLoop(m.beat, end > m.beat ? end : m.beat + barAt(timeline.meter, m.beat).length);
  };
  return (
    <>
      {drawn.map(({ m, own }, i) => {
        const next = drawn[i + 1]?.m;
        const span = ((next?.beat ?? width / ppb) - m.beat) * ppb;
        const isNew = !own;
        return (
          <div
            // By its place in the list, so a drag keeps its element (and the pointer it holds).
            key={own ? `m${list.indexOf(own)}` : "new"}
            data-marker={m.beat}
            data-marker-name={m.name}
            ref={editing && Math.abs(editing.beat - m.beat) < 1e-6 ? setAnchor : undefined}
            className={clsx("absolute top-0 h-3.5 z-[2] flex items-stretch", isNew ? "pointer-events-none" : locked ? "cursor-pointer" : "cursor-grab")}
            style={{ left: m.beat * ppb, maxWidth: Math.max(span, 1) }}
            title={`${at(timeline, m)}\nClick to go here · ${locked ? "" : `drag to move (hold ${mod.cmd} to place it off the grid) · double-click to rename · `}${opt("←")} and ${opt("→")} go to the one before or after`}
            onPointerDown={(e) => {
              e.stopPropagation();
              // A Ctrl-click on a Mac is a right-click: WebKit sends it as button 0 with Ctrl, then
              // the menu's event. It opens the menu only, as in the roll (X7).
              if (e.button !== 0 || (isMac && e.ctrlKey) || !own) return;
              const el = e.currentTarget, pointer = e.pointerId, x0 = e.clientX;
              el.setPointerCapture(pointer);
              let to: number | null = null;
              const move = (ev: PointerEvent) => {
                if (ev.pointerId !== pointer || locked || (to === null && Math.abs(ev.clientX - x0) < 3)) return;
                to = snapSong(timeline.meter, own.beat + (ev.clientX - x0) / ppb, ppb, skipsSnap(ev));
                setDrag({ from: own, to });
              };
              const end = (ev: PointerEvent) => {
                if (ev.pointerId !== pointer) return;
                for (const [type, f] of listeners) el.removeEventListener(type, f as EventListener);
                setDrag(null);
                if (ev.type !== "pointerup") return;
                if (to === null) return onSeek(own.beat);
                if (Math.abs(to - own.beat) < 1e-6) return;
                if (markerAt(list, to)) return useStore.getState().toast(`A marker is at bar ${barOf(timeline.meter, to)} already`);
                write(list.map((x) => (x === own ? { ...x, beat: to! } : x)), `moving ${own.name}`);
              };
              const listeners = [["pointermove", move], ["pointerup", end], ["pointercancel", end], ["lostpointercapture", end]] as const;
              for (const [type, f] of listeners) el.addEventListener(type, f as EventListener);
            }}
            onDoubleClick={(e) => {
              e.stopPropagation();
              if (own && !locked) useMarkerEdit.setState({ editing: { beat: own.beat, from: own } });
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              if (own) setMenu({ marker: own, x: e.clientX, y: e.clientY });
            }}
          >
            <span aria-hidden className="w-px shrink-0 bg-fg-faint" />
            <span className="min-w-0 px-1 bg-chrome text-xs leading-[14px] text-fg truncate">{m.name}</span>
          </div>
        );
      })}
      {/* Placed once, at its own flag: never at the one open before. */}
      {editing && anchor?.isConnected && anchor.dataset.marker === String(editing.beat) && (
        <MarkerField key={`${editing.beat}:${editing.from?.name ?? ""}`} anchor={anchor} start={editing.from?.name ?? nextName(list)} isNew={!editing.from} onKeep={keep} onCancel={() => useMarkerEdit.setState({ editing: null })} />
      )}
      {menu && (
        <PointMenu
          at={menu}
          label={`${menu.marker.name} marker menu`}
          onClose={() => setMenu(null)}
          items={[
            { id: "rename", label: "Rename", off: locked ? `${locked}.` : null, run: () => useMarkerEdit.setState({ editing: { beat: menu.marker.beat, from: menu.marker } }) },
            { id: "delete", label: "Delete", off: locked ? `${locked}.` : null, run: () => write(list.filter((m) => m !== menu.marker), `deleting ${menu.marker.name}`) },
            "line",
            { id: "loop", label: "Loop to next marker", run: () => loopFrom(menu.marker) },
          ]}
        />
      )}
    </>
  );
}

/**
 * A marker's name, as a field under it, with a short list of names to pick (Q13). Return keeps
 * what is typed; a pick keeps that name; a click away keeps the field's; Esc keeps nothing.
 */
function MarkerField({
  anchor,
  start,
  isNew,
  onKeep,
  onCancel,
}: {
  anchor: HTMLElement;
  start: string;
  isNew: boolean;
  onKeep: (name: string) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(start);
  const input = useRef<HTMLInputElement>(null);
  // Esc closes the popover before the field hears it: noted on its way down, so the close is a cancel.
  const cancelled = useRef(false);
  useEffect(() => {
    input.current?.select();
    const esc = (e: KeyboardEvent) => void (e.key === "Escape" && (cancelled.current = true));
    window.addEventListener("keydown", esc, true);
    return () => window.removeEventListener("keydown", esc, true);
  }, []);
  const latest = useRef(draft);
  latest.current = draft;
  return (
    <Popover
      anchor={anchor}
      onClose={() => (cancelled.current ? onCancel() : onKeep(latest.current))}
      data-testid="marker-field"
      aria-label={isNew ? "New marker" : `Rename ${start}`}
      className="w-[200px]"
    >
      <input
        ref={input}
        data-testid="marker-name"
        aria-label="Marker name"
        value={draft}
        autoFocus
        spellCheck={false}
        maxLength={NAME_MAX}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          // The field's keys are its own: never the timeline's (⌫ deletes clips there).
          e.stopPropagation();
          if (e.key === "Enter") onKeep(draft);
        }}
        className="w-full h-ctl px-2 rounded-sm bg-raised border border-accent text-sm text-fg outline-none"
      />
      {isNew && (
        <div role="listbox" aria-label="Names" className="mt-1 pt-1 border-t border-line">
          {PICKS.map((p) => (
            <MenuRow key={p} role="option" data-pick={p} onMouseDown={(e) => e.preventDefault()} onClick={() => onKeep(p)}>
              {p}
            </MenuRow>
          ))}
        </div>
      )}
    </Popover>
  );
}

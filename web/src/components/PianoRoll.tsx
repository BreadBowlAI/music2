import clsx from "clsx";
import { Eraser, Magnet, Music, Wand2, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { contextMenu, menu, type MenuEntry } from "../lib/native";
import { findClip, inKey, keyName, noteName, notesParam, rgb, ticksPerBar, ticksPerBeat } from "../lib/music";
import { useStore } from "../lib/store";
import { PPQ, type Note } from "../lib/types";
import { Button } from "./ui";

const GRIDS = [
  { label: "1/4", t: PPQ },
  { label: "1/8", t: PPQ / 2 },
  { label: "1/16", t: PPQ / 4 },
  { label: "1/32", t: PPQ / 8 },
  { label: "1/8T", t: PPQ / 3 },
  { label: "1/16T", t: PPQ / 6 },
];
const KEYS_W = 84;


export default function PianoRoll() {
  const server = useStore((s) => s.server!);
  const selectedClip = useStore((s) => s.selectedClip);
  const { set, run } = useStore.getState();
  const song = server.song;
  const found = findClip(song, selectedClip);
  const [scaleLock, setScaleLock] = useState(false);
  const [grid, setGrid] = useState(PPQ / 4);
  const [height, setHeight] = useState(320);
  const [selected, setSelected] = useState<number | null>(null); // note id
  const [lastLen, setLastLen] = useState(PPQ);
  const [gridW, setGridW] = useState(800);
  const scroller = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const centeredFor = useRef<number | null>(null);
  const audition = useRef<{ track: number; pitch: number } | null>(null);

  const track = found?.track;
  const clip = found?.clip;
  const notes = clip?.content.type === "midi" ? clip.content.notes : [];
  const plugin = track?.instrument ? server.plugins.find((p) => p.id === track.instrument!.plugin) : null;
  const pads = plugin?.kind === "kit" ? plugin.pads : null;
  const rowH = pads ? 30 : 18;

  const rows = useMemo(() => {
    let r: number[];
    if (pads) r = pads.map((p) => p.pitch);
    else {
      r = [];
      for (let p = 24; p <= 96; p++) if (!scaleLock || inKey(song, p)) r.push(p);
    }
    for (const n of notes) if (!r.includes(n.pitch)) r.push(n.pitch);
    return [...new Set(r)].sort((a, b) => b - a);
  }, [pads, scaleLock, song, notes]);

  useLayoutEffect(() => {
    const el = gridRef.current?.parentElement;
    if (!el) return;
    const ro = new ResizeObserver(() => setGridW(el.clientWidth - KEYS_W));
    ro.observe(el);
    return () => ro.disconnect();
  }, [clip?.id]);

  // Center on the notes when a clip opens.
  useEffect(() => {
    if (!clip || centeredFor.current === clip.id || !scroller.current) return;
    centeredFor.current = clip.id;
    const center = notes.length ? notes.reduce((a, n) => a + n.pitch, 0) / notes.length : plugin?.tags.includes("bass") ? 40 : 64;
    const row = Math.max(0, rows.findIndex((r) => r <= center));
    scroller.current.scrollTop = row * rowH - scroller.current.clientHeight / 2;
  }, [clip, notes, rows, rowH, plugin]);

  // Delete the selected note before the app-level handler deletes the whole clip.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!selected || !clip || e.target instanceof HTMLInputElement) return;
      if (e.key === "Delete" || e.key === "Backspace") {
        e.stopImmediatePropagation();
        run("notes.remove", { clip: clip.id, ids: [selected] });
        setSelected(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [selected, clip, notes, run]);

  if (!track || !clip) return null;

  const tpb = ticksPerBeat(song);
  const ppt = gridW / clip.length; // px per tick
  const color = rgb(track.color);
  const play = (pitch: number) => {
    if (useStore.getState().tick.playing) return;
    stopAudition();
    audition.current = { track: track.id, pitch };
    api.note(track.id, pitch, true);
    setTimeout(() => audition.current?.pitch === pitch && stopAudition(), 260);
  };
  const stopAudition = () => {
    if (audition.current) api.note(audition.current.track, audition.current.pitch, false);
    audition.current = null;
  };

  const pointToCell = (clientX: number, clientY: number) => {
    const r = gridRef.current!.getBoundingClientRect();
    const tick = Math.max(0, Math.min(clip.length - 1, (clientX - r.left) / ppt));
    const row = Math.floor((clientY - r.top) / rowH);
    return { tick, row: Math.max(0, Math.min(rows.length - 1, row)) };
  };

  const write = (list: Note[]) => run("notes.set", { clip: clip.id, notes: notesParam(song, list) });

  /** Drag-edit `orig[idx]`. Writes are chained so they reach the engine in order, and so a
   *  freshly drawn note has its engine-assigned id before the next write refers to it. */
  const beginEdit = (e: React.PointerEvent, orig: Note[], idx: number, resize: boolean, first?: () => Promise<unknown>) => {
    const start = pointToCell(e.clientX, e.clientY);
    const n0 = orig[idx];
    const grab = start.tick - n0.start;
    let last = "";
    // The whole gesture (including drawing the note) is one undo step.
    api.group("begin", resize && first ? "Draw note" : "Edit notes");
    let chain: Promise<unknown> = first ? first() : Promise.resolve();
    const move = (ev: PointerEvent) => {
      const c = pointToCell(ev.clientX, ev.clientY);
      const n = { ...n0 };
      if (resize) {
        const end = Math.min(clip.length, Math.max(n.start + grid, Math.round(c.tick / grid) * grid));
        n.length = end - n.start;
      } else {
        n.start = Math.max(0, Math.min(clip.length - 1, Math.round((c.tick - grab) / grid) * grid));
        n.length = Math.min(n.length, clip.length - n.start);
        n.pitch = rows[c.row];
      }
      const key = JSON.stringify(n);
      if (key !== last) {
        if (last && n.pitch !== JSON.parse(last).pitch) play(n.pitch);
        last = key;
        chain = chain.then(() => write(orig.map((x, i) => (i === idx ? { ...n, id: orig[idx].id } : x))));
        if (!pads) setLastLen(n.length);
      }
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      chain.then(() => api.group("end"));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const onGridDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const c = pointToCell(e.clientX, e.clientY);
    const startT = Math.min(clip.length - 1, Math.floor(c.tick / grid) * grid);
    const len = Math.min(pads ? PPQ / 4 : lastLen, clip.length - startT);
    const n: Note = { id: 0, pitch: rows[c.row], start: startT, length: Math.max(1, len), velocity: 100 };
    const orig = [...notes, n];
    const last = orig.length - 1;
    // The engine assigns the new note's id; later drag writes reuse it.
    const first = () =>
      write(orig).then((r) => {
        if (r?.ids) {
          orig[last] = { ...orig[last], id: r.ids[last] };
          setSelected(r.ids[last]);
        }
      });
    play(n.pitch);
    beginEdit(e, orig, last, true, first);
  };

  const setNote = (id: number, change: Partial<Note>) => write(notes.map((x) => (x.id === id ? { ...x, ...change } : x)));
  const removeNote = (id: number) => (run("notes.remove", { clip: clip.id, ids: [id] }), setSelected(null));

  const noteMenu = (n: Note): MenuEntry[] =>
    menu(
    { label: "Delete Note", shortcut: "⌫", danger: true, action: () => removeNote(n.id) },
    "-",
    {
      label: "Loudness",
      submenu: [
        ["Soft", 60],
        ["Medium", 90],
        ["Loud", 115],
        ["Max", 127],
      ].map(([label, v]) => ({ label: label as string, checked: Math.abs(n.velocity - (v as number)) < 8, action: () => setNote(n.id, { velocity: v as number }) })),
    },
    !pads && { label: "Octave Up", disabled: n.pitch + 12 > 127, action: () => setNote(n.id, { pitch: n.pitch + 12 }) },
    !pads && { label: "Octave Down", disabled: n.pitch - 12 < 0, action: () => setNote(n.id, { pitch: n.pitch - 12 }) },
  );

  const gridMenu = (): MenuEntry[] =>
    menu(
    { label: `Snap All Notes to ${GRIDS.find((g) => g.t === grid)?.label ?? "Grid"}`, disabled: !notes.length, action: () => run("notes.quantize", { clip: clip.id, grid: grid / PPQ }) },
    { label: "Humanize", disabled: !notes.length, action: () => run("notes.velocity", { clip: clip.id, humanize: 0.15 }) },
    "-",
    !pads && { label: "Show Only In-Key Notes", checked: scaleLock, action: () => setScaleLock(!scaleLock) },
    !pads && "-",
    { label: "Clear All Notes", danger: true, disabled: !notes.length, action: () => run("notes.set", { clip: clip.id, notes: [] }) },
  );

  const totalH = rows.length * rowH;
  const lines: { x: number; kind: 0 | 1 | 2 }[] = [];
  for (let t = 0; t <= clip.length; t += grid) {
    lines.push({ x: t * ppt, kind: t % ticksPerBar(song) === 0 ? 2 : t % tpb === 0 ? 1 : 0 });
  }

  return (
    <section className="shrink-0 flex flex-col bg-ink-900 border-t border-black/[0.06]" style={{ height }}>
      <div
        className="h-1.5 -mt-0.5 cursor-ns-resize hover:bg-pop-500/40 transition"
        onPointerDown={(e) => {
          const y0 = e.clientY, h0 = height;
          const mv = (ev: PointerEvent) => setHeight(Math.min(window.innerHeight - 240, Math.max(180, h0 + y0 - ev.clientY)));
          const up = () => (window.removeEventListener("pointermove", mv), window.removeEventListener("pointerup", up));
          window.addEventListener("pointermove", mv);
          window.addEventListener("pointerup", up);
        }}
      />
      <div className="h-12 shrink-0 flex items-center gap-2 px-4 overflow-x-auto">
        <span className="size-2.5 rounded-full" style={{ background: color }} />
        <span className="text-sm font-semibold whitespace-nowrap shrink-0">{clip.name}</span>
        <span className="text-sm text-ink-400 whitespace-nowrap shrink-0">on {track.name}</span>
        <div className="w-px h-6 bg-black/[0.07] mx-2" />
        {!pads && (
          <Button size="sm" active={scaleLock} onClick={() => setScaleLock(!scaleLock)} title={`Only show notes in ${keyName(song)}`} className={clsx(scaleLock && "!text-pop-400")}>
            <Music className="size-3.5" /> {scaleLock ? `In-key only · ${keyName(song)}` : "All notes"}
          </Button>
        )}
        <div className="flex items-center gap-0.5 p-0.5 rounded-lg bg-ink-950 ring-1 ring-black/[0.06]">
          <Magnet className="size-3.5 text-ink-400 mx-1.5" />
          {GRIDS.map((g) => (
            <button
              key={g.label}
              onClick={() => setGrid(g.t)}
              className={clsx("h-6 px-2 rounded-md text-[11px] font-medium cursor-pointer whitespace-nowrap", grid === g.t ? "bg-ink-700 text-ink-100" : "text-ink-400 hover:text-ink-200")}
            >
              {g.label}
            </button>
          ))}
        </div>
        <Button size="sm" onClick={() => run("notes.quantize", { clip: clip.id, grid: grid / tpb })} title="Snap notes to the grid">
          <Magnet className="size-3.5" /> Quantize
        </Button>
        <Button size="sm" onClick={() => run("notes.velocity", { clip: clip.id, humanize: 0.4 })} title="Vary velocities so it feels played by a human">
          <Wand2 className="size-3.5" /> Humanize
        </Button>
        {notes.length > 0 && (
          <Button size="sm" onClick={() => run("notes.set", { clip: clip.id, notes: [] })}>
            <Eraser className="size-3.5" /> Clear
          </Button>
        )}
        <div className="flex-1" />
        <Button size="sm" onClick={() => set({ selectedClip: null })} className="!px-2">
          <X className="size-4" />
        </Button>
      </div>

      <div ref={scroller} className="flex-1 overflow-y-auto overflow-x-hidden min-h-0">
        <div className="flex relative" style={{ height: totalH }}>
          {/* Keys */}
          <div className="sticky left-0 z-10 shrink-0 bg-ink-900" style={{ width: KEYS_W }}>
            {rows.map((p) => {
              const black = [1, 3, 6, 8, 10].includes(p % 12);
              const tonic = (p - song.key.root) % 12 === 0;
              const label = pads ? pads.find((x) => x.pitch === p)?.name ?? noteName(p) : noteName(p);
              return (
                <button
                  key={p}
                  onPointerDown={() => play(p)}
                  className={clsx(
                    "w-full flex items-center px-2.5 text-[11px] font-medium border-b border-ink-950/60 cursor-pointer transition active:brightness-125",
                    pads ? "bg-ink-700 text-ink-100 hover:bg-ink-600" : tonic ? "bg-pop-400/90 text-white" : black && !scaleLock ? "bg-ink-600 text-ink-200" : "bg-white text-ink-300 hover:bg-ink-850",
                    !pads && !inKey(song, p) && "!text-red-500",
                  )}
                  style={{ height: rowH }}
                >
                  {label}
                </button>
              );
            })}
          </div>

          {/* Grid */}
          <div
            ref={gridRef}
            className="relative flex-1 cursor-crosshair"
            title="Click to draw · drag to move · drag the end to resize · double-click to delete · right-click for more"
            onPointerDown={onGridDown}
            onContextMenu={(e) => contextMenu(e, gridMenu())}
          >
            {rows.map((p, i) => (
              <div
                key={p}
                className={clsx(
                  "absolute inset-x-0 border-b border-ink-950/50",
                  pads ? (i % 2 ? "bg-ink-850" : "bg-ink-800/70") : (p - song.key.root) % 12 === 0 ? "bg-pop-500/[0.07]" : !inKey(song, p) ? "bg-ink-950" : [1, 3, 6, 8, 10].includes(p % 12) && !scaleLock ? "bg-ink-850" : "bg-ink-800/60",
                )}
                style={{ top: i * rowH, height: rowH }}
              />
            ))}
            {lines.map((l, i) => (
              <div key={i} className="absolute top-0 bottom-0 w-px pointer-events-none" style={{ left: l.x, background: l.kind === 2 ? "rgba(0,0,0,0.128)" : l.kind === 1 ? "rgba(0,0,0,0.064)" : "rgba(0,0,0,0.024)" }} />
            ))}
            {notes.map((n, i) => {
              const row = rows.indexOf(n.pitch);
              if (row < 0 || n.start >= clip.length) return null;
              const w = Math.max(4, Math.min(n.length, clip.length - n.start) * ppt - 1);
              const sel = n.id === selected;
              return (
                <div
                  key={n.id}
                  className={clsx("absolute rounded-md cursor-grab active:cursor-grabbing overflow-hidden", sel ? "ring-2 ring-ink-100 z-10" : "ring-1 ring-black/30")}
                  style={{ left: n.start * ppt + 0.5, top: row * rowH + 1, width: w, height: rowH - 2, background: color, opacity: 0.45 + (0.55 * n.velocity) / 127 }}
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    setSelected(n.id);
                    if (e.button !== 0) return;
                    play(n.pitch);
                    const r = e.currentTarget.getBoundingClientRect();
                    beginEdit(e, notes, i, e.clientX > r.right - Math.min(8, r.width / 3));
                  }}
                  onDoubleClick={(e) => {
                    e.stopPropagation();
                    removeNote(n.id);
                  }}
                  onContextMenu={(e) => contextMenu(e, noteMenu(n))}
                >
                  {!pads && w > 30 && <span className="absolute left-1.5 top-1/2 -translate-y-1/2 text-[9px] font-semibold text-black/70">{noteName(n.pitch)}</span>}
                  <div className="absolute right-0 inset-y-0 w-1.5 cursor-ew-resize bg-black/15" />
                </div>
              );
            })}
            <RollPlayhead clipStart={clip.start} clipLen={clip.length} ppt={ppt} />
          </div>
        </div>
      </div>
    </section>
  );
}

function RollPlayhead({ clipStart, clipLen, ppt }: { clipStart: number; clipLen: number; ppt: number }) {
  const t = useStore((s) => s.tick.playhead) - clipStart;
  if (t < 0 || t > clipLen) return null;
  return <div className="absolute top-0 bottom-0 w-px bg-sun pointer-events-none z-20" style={{ left: t * ppt }} />;
}

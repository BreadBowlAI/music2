import clsx from "clsx";
import { ChevronDown, Pause, Play, Redo2, Repeat, Settings, SkipBack, Undo2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { isMac } from "../lib/native";
import { NOTE_NAMES, SCALES, position } from "../lib/music";
import { useStore } from "../lib/store";
import { Button } from "./ui";

function Position() {
  const playhead = useStore((s) => s.tick.playhead);
  const song = useStore((s) => s.server!.song);
  return <span className="font-mono text-[15px] tabular-nums text-ink-100 w-14">{position(song, playhead)}</span>;
}

function Bpm() {
  const bpm = useStore((s) => s.server!.song.bpm);
  const run = useStore((s) => s.run);
  const [v, setV] = useState(String(bpm));
  useEffect(() => setV(String(Math.round(bpm * 10) / 10)), [bpm]);
  return (
    <label className="flex items-center gap-1.5 h-9 px-3 rounded-xl bg-ink-800 ring-1 ring-black/[0.06]">
      <input
        value={v}
        onChange={(e) => setV(e.target.value)}
        onBlur={() => Number(v) && run("song.set", { bpm: Number(v) })}
        onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
        className="w-10 bg-transparent outline-none text-sm font-medium tabular-nums text-right"
      />
      <span className="text-xs text-ink-400">BPM</span>
    </label>
  );
}

function Select({ value, options, onChange, className }: { value: string; options: { id: string; label: string }[]; onChange: (v: string) => void; className?: string }) {
  return (
    <div className={clsx("relative", className)}>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="appearance-none h-9 pl-3 pr-8 rounded-xl bg-ink-800 ring-1 ring-black/[0.06] text-sm font-medium outline-none cursor-pointer hover:bg-ink-700 w-full"
      >
        {options.map((o) => (
          <option key={o.id} value={o.id}>{o.label}</option>
        ))}
      </select>
      <ChevronDown className="size-3.5 absolute right-2.5 top-1/2 -translate-y-1/2 text-ink-400 pointer-events-none" />
    </div>
  );
}

export default function TopBar() {
  const server = useStore((s) => s.server!);
  const playing = useStore((s) => s.tick.playing);
  const run = useStore((s) => s.run);
  const set = useStore((s) => s.set);
  const song = server.song;
  const [title, setTitle] = useState(song.title);
  useEffect(() => setTitle(song.title), [song.title]);

  return (
    // The bar doubles as the title bar: drag it to move the window, and leave room for
    // the macOS traffic lights.
    <header
      data-tauri-drag-region
      className={clsx("h-16 shrink-0 flex items-center gap-3 px-4 border-b border-black/[0.06] bg-ink-900", isMac && "pl-[88px]")}
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pr-2">
        <div className="size-8 rounded-xl pop-gradient grid place-items-center shadow-lg shadow-pop-500/15">
          <span className="text-white text-sm font-bold">m2</span>
        </div>
      </div>
      <button
        onClick={() => api.transport("toggle")}
        title="Play / stop (Space)"
        className={clsx(
          "size-11 rounded-full grid place-items-center transition-all cursor-pointer active:scale-95",
          playing ? "bg-ink-200 text-white" : "pop-gradient text-white shadow-lg shadow-pop-500/15 hover:brightness-110",
        )}
      >
        {playing ? <Pause className="size-5 fill-current" /> : <Play className="size-5 fill-current translate-x-[1px]" />}
      </button>
      <Button onClick={() => api.transport("seek", 0)} title="Back to start (Enter)" className="!px-2.5">
        <SkipBack className="size-4" />
      </Button>
      <Position />
      <div className="w-px h-7 bg-black/[0.07] mx-1" />
      <Bpm />
      <Select
        className="w-[72px]"
        value={String(song.key.root)}
        options={NOTE_NAMES.map((n, i) => ({ id: String(i), label: n }))}
        onChange={(v) => run("song.set", { key: `${NOTE_NAMES[Number(v)]} ${SCALES.find((s) => s.id === song.key.scale)!.name}` })}
      />
      <Select
        className="w-[150px]"
        value={song.key.scale}
        options={SCALES.map((s) => ({ id: s.id, label: s.name }))}
        onChange={(v) => run("song.set", { key: `${NOTE_NAMES[song.key.root]} ${SCALES.find((s) => s.id === v)!.name}` })}
      />
      <Button active={server.looping} onClick={() => api.loop(!server.looping)} title="Loop the whole song" className={clsx(server.looping && "!text-pop-400")}>
        <Repeat className="size-4" /> Loop
      </Button>

      <div data-tauri-drag-region className="flex-1 flex justify-center">
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title !== song.title && run("song.set", { title })}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          className="w-72 text-center bg-transparent outline-none text-[15px] font-semibold text-ink-100 rounded-lg hover:bg-ink-800 focus:bg-ink-800 h-9 px-2"
        />
        {server.dirty && <span className="self-center size-1.5 rounded-full bg-pop-500 -ml-2" title="Unsaved changes" />}
      </div>

      <Button onClick={() => api.undo()} disabled={!server.undo} title={server.undo ? `Undo ${server.undo}` : "Nothing to undo"} className="!px-2.5">
        <Undo2 className="size-4" />
      </Button>
      <Button onClick={() => api.redo()} disabled={!server.redo} title={server.redo ? `Redo ${server.redo}` : "Nothing to redo"} className="!px-2.5">
        <Redo2 className="size-4" />
      </Button>
      <Button onClick={() => set({ modal: "settings" })} title="Settings" className="!px-2.5">
        <Settings className="size-4" />
      </Button>
    </header>
  );
}

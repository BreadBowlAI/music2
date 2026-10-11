import clsx from "clsx";
import { AudioLines, CirclePlus, Plus, Power, SlidersHorizontal, Upload, X } from "lucide-react";
import { useState } from "react";
import { api } from "../lib/api";
import { rgb, ticksPerBar } from "../lib/music";
import { nativeAction } from "../lib/native";
import { useStore } from "../lib/store";
import type { Device, Plugin, Track } from "../lib/types";
import { Button, Knob, Segmented, inputClass } from "./ui";

export default function SidePanel() {
  const tab = useStore((s) => s.sideTab);
  const set = useStore((s) => s.set);
  const options = [
    { id: "track" as const, label: <><SlidersHorizontal className="size-3.5" /> Track</> },
    { id: "sounds" as const, label: <><AudioLines className="size-3.5" /> Sounds</> },
  ];
  return (
    <aside className="w-[360px] shrink-0 flex flex-col border-l border-black/[0.06] bg-ink-900">
      <div className="p-3 border-b border-black/[0.06]">
        <Segmented value={tab} options={options} onChange={(v) => set({ sideTab: v })} />
      </div>
      <div className="flex-1 overflow-y-auto p-4 min-h-0">
        {tab === "track" && <DevicePanel />}
        {tab === "sounds" && <Sounds />}
      </div>
    </aside>
  );
}

// ------------------------------------------------------------------ sounds

function Sounds() {
  const server = useStore((s) => s.server!);
  const { run, set } = useStore.getState();
  const bar = () => Math.floor(useStore.getState().tick.playhead / ticksPerBar(server.song)) + 1;

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold">Sounds</h2>
        <p className="text-sm text-ink-400">Recordings in this song. Place them on the timeline or play them as an instrument.</p>
      </div>
      <div className="space-y-2">
        {server.song.assets.length === 0 && <p className="text-sm text-ink-500">No sounds yet.</p>}
        {server.song.assets.map((a) => (
          <div key={a.id} className="rounded-2xl p-3 bg-ink-800 ring-1 ring-black/[0.06]">
            <div className="text-sm font-medium">{a.name}</div>
            {a.prompt && <div className="text-xs text-ink-400 italic mt-0.5">“{a.prompt}”</div>}
            <div className="flex gap-1.5 mt-2">
              <Button size="sm" variant="solid" onClick={async () => { const r = await run("sampler.load", { asset: a.id }); if (r) set({ selectedTrack: r.track_id }); }}>
                Make instrument
              </Button>
              <Button size="sm" variant="solid" onClick={() => run("audio.place", { asset: a.id, start_bar: bar() })}>
                Place at playhead
              </Button>
            </div>
          </div>
        ))}
        <Button variant="outline" className="w-full" onClick={() => nativeAction("import", { bar: bar() })}>
          <Upload className="size-4" /> Import a WAV
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ track inspector

function DevicePanel() {
  const server = useStore((s) => s.server!);
  const selectedTrack = useStore((s) => s.selectedTrack);
  const { run } = useStore.getState();
  const track = server.song.tracks.find((t) => t.id === selectedTrack);
  const plugins = server.plugins;

  return (
    <div className="space-y-5">
      {!track ? (
        <p className="text-sm text-ink-400">Select a track to shape its sound.</p>
      ) : (
        <>
          <div className="flex items-center gap-2.5">
            <span className="size-3 rounded-full" style={{ background: rgb(track.color) }} />
            <h2 className="text-lg font-semibold">{track.name}</h2>
          </div>
          <div className="flex gap-2 justify-around rounded-2xl bg-ink-800 p-3 ring-1 ring-black/[0.06]">
            <Knob
              label="Volume"
              display={`${track.volume_db > 0 ? "+" : ""}${track.volume_db.toFixed(1)} dB`}
              value={(track.volume_db + 40) / 46}
              onStart={() => api.group("begin", "Volume")}
              onEnd={() => api.group("end")}
              onChange={(v) => run("track.set", { track: track.id, volume_db: Math.round((v * 46 - 40) * 10) / 10 })}
            />
            <Knob
              label="Pan"
              display={track.pan === 0 ? "center" : track.pan < 0 ? `L ${Math.round(-track.pan * 100)}` : `R ${Math.round(track.pan * 100)}`}
              value={(track.pan + 1) / 2}
              color="var(--color-violet-glow)"
              onStart={() => api.group("begin", "Pan")}
              onEnd={() => api.group("end")}
              onChange={(v) => run("track.set", { track: track.id, pan: Math.round((v * 2 - 1) * 100) / 100 })}
            />
          </div>

          <Routing track={track} tracks={server.song.tracks} />

          {track.kind === "instrument" && (
            <section className="space-y-2">
              <div className="text-xs font-semibold tracking-wider text-ink-400">INSTRUMENT</div>
              <select
                value={track.instrument?.plugin ?? ""}
                onChange={(e) => run("track.setInstrument", { track: track.id, plugin: e.target.value })}
                className={clsx(inputClass, "cursor-pointer")}
              >
                {plugins.filter((p) => p.kind !== "effect" && p.asset_slots.length === 0).map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
                {track.instrument && !plugins.find((p) => p.id === track.instrument!.plugin && p.asset_slots.length === 0) && (
                  <option value={track.instrument.plugin}>{plugins.find((p) => p.id === track.instrument!.plugin)?.name ?? track.instrument.plugin}</option>
                )}
              </select>
              {track.instrument && <DeviceCard device={track.instrument} plugin={plugins.find((p) => p.id === track.instrument!.plugin)} track={track.id} index="instrument" />}
            </section>
          )}

          <section className="space-y-2">
            <div className="text-xs font-semibold tracking-wider text-ink-400">EFFECTS</div>
            {track.effects.map((d, i) => (
              <DeviceCard key={i} device={d} plugin={plugins.find((p) => p.id === d.plugin)} track={track.id} index={i} />
            ))}
            <AddEffect track={track.id} plugins={plugins} />
          </section>
        </>
      )}

      <section className="space-y-2 pt-3 border-t border-black/[0.06]">
        <div className="text-xs font-semibold tracking-wider text-ink-400">MASTER</div>
        <div className="flex justify-around rounded-2xl bg-ink-800 p-3 ring-1 ring-black/[0.06]">
          <Knob
            label="Master"
            display={`${server.song.master.volume_db.toFixed(1)} dB`}
            value={(server.song.master.volume_db + 30) / 36}
            onStart={() => api.group("begin", "Master volume")}
            onEnd={() => api.group("end")}
            onChange={(v) => run("master.set", { volume_db: Math.round((v * 36 - 30) * 10) / 10 })}
          />
        </div>
        {server.song.master.effects.map((d, i) => (
          <DeviceCard key={i} device={d} plugin={plugins.find((p) => p.id === d.plugin)} track="master" index={i} />
        ))}
        <AddEffect track="master" plugins={plugins} />
      </section>
    </div>
  );
}

function DeviceCard({ device, plugin, track, index }: { device: Device; plugin?: Plugin; track: number | "master"; index: number | "instrument" }) {
  const { run } = useStore.getState();
  const [showGraph, setShowGraph] = useState(false);
  if (!plugin) return <div className="rounded-2xl p-3 bg-red-500/10 text-red-700 text-sm">Missing plugin {device.plugin}</div>;
  const isFx = index !== "instrument";
  return (
    <div className={clsx("rounded-2xl p-3 bg-ink-800 ring-1 ring-black/[0.06]", device.bypass && "opacity-50")}>
      <div className="flex items-center gap-2">
        {isFx && (
          <button
            title={device.bypass ? "Turn on" : "Turn off"}
            onClick={() => run("effect.bypass", { track, index, bypass: !device.bypass })}
            className={clsx("size-6 rounded-md grid place-items-center cursor-pointer", device.bypass ? "text-ink-500" : "text-pop-400")}
          >
            <Power className="size-3.5" />
          </button>
        )}
        <div className="flex-1 min-w-0">
          <div className="text-sm font-semibold">{plugin.name}</div>
          <div className="text-[11px] text-ink-400 truncate" title={plugin.description}>{plugin.description}</div>
        </div>
        <button onClick={() => setShowGraph(!showGraph)} className="text-[11px] text-ink-400 hover:text-ink-200 cursor-pointer px-1">
          {"{ }"}
        </button>
        {isFx && (
          <button onClick={() => run("effect.remove", { track, index })} className="size-6 rounded-md grid place-items-center text-ink-500 hover:text-red-700 cursor-pointer" title="Remove">
            <X className="size-3.5" />
          </button>
        )}
      </div>
      {plugin.macros.length > 0 && (
        <div className="flex flex-wrap gap-1 justify-start mt-3">
          {plugin.macros.map((m) => (
            <Knob
              key={m.id}
              label={m.name}
              title={m.description}
              value={device.macros[m.id] ?? m.default}
              onStart={() => api.group("begin", `Adjust ${m.name}`)}
              onEnd={() => api.group("end")}
              onChange={(v) => run("device.setMacros", { track, device: index, macros: { [m.id]: Math.round(v * 1000) / 1000 } })}
            />
          ))}
        </div>
      )}
      {plugin.uses_sidechain && track !== "master" && <SidechainPicker device={device} track={track} index={index} />}
      {showGraph && <GraphView id={plugin.id} />}
    </div>
  );
}

function SidechainPicker({ device, track, index }: { device: Device; track: number; index: number | "instrument" }) {
  const tracks = useStore((s) => s.server!.song.tracks);
  const { run } = useStore.getState();
  return (
    <label className="mt-3 flex items-center gap-2 text-xs text-ink-300">
      <span className="shrink-0">Sidechain from</span>
      <select
        value={device.sidechain ?? ""}
        onChange={(e) => run("device.setSidechain", { track, device: index, source: e.target.value ? Number(e.target.value) : null })}
        className="flex-1 h-8 px-2 rounded-lg bg-ink-950 ring-1 ring-black/[0.08] text-ink-100 outline-none cursor-pointer"
      >
        <option value="">— itself (plain compressor) —</option>
        {tracks.filter((t) => t.id !== track).map((t) => (
          <option key={t.id} value={t.id}>{t.name}</option>
        ))}
      </select>
    </label>
  );
}

/** Where a track's signal goes: its output (master or a bus) and sends into buses. */
function Routing({ track, tracks }: { track: Track; tracks: Track[] }) {
  const { run } = useStore.getState();
  const buses = tracks.filter((t) => t.kind === "bus" && t.id !== track.id);
  const unsent = buses.filter((b) => !track.sends.some((x) => x.to === b.id));
  return (
    <section className="space-y-2">
      <div className="text-xs font-semibold tracking-wider text-ink-400">ROUTING</div>
      <div className="rounded-2xl bg-ink-800 p-3 ring-1 ring-black/[0.06] space-y-3">
        <label className="flex items-center gap-2 text-sm">
          <span className="text-ink-300 w-16 shrink-0">Output</span>
          <select
            value={track.output ?? ""}
            onChange={(e) => run("track.set", { track: track.id, output: e.target.value ? Number(e.target.value) : "master" })}
            className="flex-1 h-8 px-2 rounded-lg bg-ink-950 ring-1 ring-black/[0.08] text-ink-100 outline-none cursor-pointer"
          >
            <option value="">Master</option>
            {buses.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
          </select>
        </label>
        {track.sends.map((snd) => {
          const bus = tracks.find((t) => t.id === snd.to);
          return (
            <div key={snd.to} className="flex items-center gap-2">
              <span className="text-sm text-ink-300 w-16 shrink-0 truncate">Send</span>
              <span className="flex-1 text-sm truncate" style={{ color: bus ? rgb(bus.color) : undefined }}>{bus?.name ?? "?"}</span>
              <Knob
                label={`${snd.gain_db.toFixed(1)} dB`}
                value={(snd.gain_db + 40) / 46}
                onStart={() => api.group("begin", "Send level")}
                onEnd={() => api.group("end")}
                onChange={(v) => run("send.set", { track: track.id, to: snd.to, gain_db: Math.round((v * 46 - 40) * 10) / 10 })}
              />
              <button
                title={snd.pre_fader ? "Pre-fader (ignores track volume)" : "Post-fader (follows track volume)"}
                onClick={() => run("send.set", { track: track.id, to: snd.to, pre_fader: !snd.pre_fader })}
                className={clsx("h-6 px-1.5 rounded-md text-[10px] font-bold cursor-pointer", snd.pre_fader ? "bg-sun text-white" : "bg-ink-700 text-ink-400")}
              >
                PRE
              </button>
              <button onClick={() => run("send.remove", { track: track.id, to: snd.to })} className="text-ink-500 hover:text-red-700 cursor-pointer" title="Remove send">
                <X className="size-3.5" />
              </button>
            </div>
          );
        })}
        {unsent.length > 0 ? (
          <select
            value=""
            onChange={(e) => e.target.value && run("send.set", { track: track.id, to: Number(e.target.value), gain_db: -6 })}
            className="w-full h-8 px-2 rounded-lg bg-ink-950 ring-1 ring-black/[0.08] text-ink-300 text-sm outline-none cursor-pointer"
          >
            <option value="">+ Send to a bus…</option>
            {unsent.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
          </select>
        ) : (
          buses.length === 0 && <p className="text-[11px] text-ink-500">Add a Bus track to group tracks or share a reverb.</p>
        )}
      </div>
    </section>
  );
}

function GraphView({ id }: { id: string }) {
  const [text, setText] = useState<string | null>(null);
  if (text === null) {
    api.plugin(id).then((def) => setText(JSON.stringify(def, null, 2)));
  }
  return (
    <pre className="mt-3 max-h-60 overflow-auto rounded-xl bg-ink-950 p-3 text-[11px] font-mono text-ink-300 select-text">
      {text ?? "…"}
      {"\n\n// Plugins are m2plug JSON graphs — see docs/music2-design.md, Part 5"}
    </pre>
  );
}

function AddEffect({ track, plugins }: { track: number | "master"; plugins: Plugin[] }) {
  const [open, setOpen] = useState(false);
  const { run } = useStore.getState();
  return (
    <div className="relative">
      <Button variant="outline" size="sm" className="w-full" onClick={() => setOpen(!open)}>
        <Plus className="size-3.5" /> Add effect
      </Button>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute left-0 right-0 top-9 z-50 p-1.5 rounded-2xl glass ring-1 ring-black/10 shadow-2xl rise">
            {plugins.filter((p) => p.kind === "effect" && p.id !== "m2/master").map((p) => (
              <button
                key={p.id}
                onClick={() => (setOpen(false), run("effect.add", { track, plugin: p.id }))}
                className="w-full text-left px-3 py-2 rounded-lg hover:bg-ink-700 cursor-pointer"
              >
                <div className="text-sm font-medium flex items-center gap-1.5"><CirclePlus className="size-3.5 text-ink-400" /> {p.name}</div>
                <div className="text-[11px] text-ink-400 pl-5">{p.description}</div>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

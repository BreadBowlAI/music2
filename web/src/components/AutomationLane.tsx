import clsx from "clsx";
import { X } from "lucide-react";
import { memo, useRef, useState } from "react";
import { api } from "../lib/api";
import { rgb } from "../lib/music";
import { useStore } from "../lib/store";
import type { AutoPoint, Plugin, Track } from "../lib/types";

export const AUTO_H = 72;

/** "volume" | "pan" | "macro:<deviceId>:<macroId>" */
export type LaneKey = string;

export function laneOptions(track: Track, plugins: Plugin[]) {
  const opts: { key: LaneKey; label: string }[] = [
    { key: "volume", label: "Volume" },
    { key: "pan", label: "Pan" },
  ];
  for (const d of [...(track.instrument ? [track.instrument] : []), ...track.effects]) {
    const p = plugins.find((x) => x.id === d.plugin);
    for (const m of p?.macros ?? []) opts.push({ key: `macro:${d.id}:${m.id}`, label: `${p!.name} · ${m.name}` });
  }
  return opts;
}

function range(key: LaneKey): [number, number, (v: number) => string] {
  if (key === "volume") return [-60, 6, (v) => `${v > 0 ? "+" : ""}${v.toFixed(1)} dB`];
  if (key === "pan") return [-1, 1, (v) => (Math.abs(v) < 0.01 ? "C" : v < 0 ? `L${Math.round(-v * 100)}` : `R${Math.round(v * 100)}`)];
  return [0, 1, (v) => `${Math.round(v * 100)}%`];
}

function lanePoints(track: Track, key: LaneKey): AutoPoint[] {
  const lane = track.automation.find((l) =>
    key === "volume" ? l.target.param === "volume" : key === "pan" ? l.target.param === "pan" : l.target.param === "macro" && `macro:${l.target.device}:${l.target.macro}` === key,
  );
  return lane?.points ?? [];
}

/** Command params identifying the lane's target. */
function targetParams(track: Track, key: LaneKey) {
  if (key === "volume" || key === "pan") return { param: key };
  const [, dev, macro] = key.split(":");
  const id = Number(dev);
  const device = track.instrument?.id === id ? "instrument" : track.effects.findIndex((d) => d.id === id);
  return { param: "macro", device, macro };
}

export function hasLane(track: Track, key: LaneKey) {
  return lanePoints(track, key).length > 0;
}

export const AutomationHeader = memo(function AutomationHeader({ track, laneKey, plugins }: { track: Track; laneKey: LaneKey; plugins: Plugin[] }) {
  const { set } = useStore.getState();
  const autoOpen = useStore((s) => s.autoOpen);
  return (
    <div className="flex items-center gap-1.5 pl-6 pr-2 border-b border-black/[0.05] bg-ink-950/60" style={{ height: AUTO_H }}>
      <select
        value={laneKey}
        onChange={(e) => set({ autoOpen: { ...autoOpen, [track.id]: e.target.value } })}
        className="flex-1 min-w-0 h-7 px-1.5 rounded-lg bg-ink-900 ring-1 ring-black/[0.08] text-xs text-ink-200 outline-none cursor-pointer"
      >
        {laneOptions(track, plugins).map((o) => (
          <option key={o.key} value={o.key}>
            {hasLane(track, o.key) ? "● " : ""}
            {o.label}
          </option>
        ))}
      </select>
      <button
        title="Hide automation"
        onClick={() => {
          const next = { ...autoOpen };
          delete next[track.id];
          set({ autoOpen: next });
        }}
        className="size-6 grid place-items-center rounded-md text-ink-500 hover:text-ink-200 cursor-pointer"
      >
        <X className="size-3.5" />
      </button>
    </div>
  );
});

/** Envelope editor: click to add a point, drag to move, right-click or double-click to delete. */
export const AutomationLaneView = memo(function AutomationLaneView({
  track,
  laneKey,
  ppb,
  tpb,
  barTicks,
  width,
}: { track: Track; laneKey: LaneKey; ppb: number; tpb: number; barTicks: number; width: number }) {
  const [lo, hi, fmt] = range(laneKey);
  const serverPoints = lanePoints(track, laneKey);
  const [dragPts, setDragPts] = useState<AutoPoint[] | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const ref = useRef<SVGSVGElement>(null);
  const points = dragPts ?? serverPoints;
  const h = AUTO_H - 12;
  const x = (t: number) => (t / tpb) * ppb;
  const y = (v: number) => 6 + (1 - (v - lo) / (hi - lo)) * h;
  const snap = tpb / 4;
  const color = rgb(track.color);

  const send = (pts: AutoPoint[]) =>
    useStore.getState().run("automation.set", {
      track: track.id,
      ...targetParams(track, laneKey),
      points: pts.map((p) => ({ bar: p.tick / barTicks + 1, value: p.value })),
    });

  const at = (e: { clientX: number; clientY: number }) => {
    const r = ref.current!.getBoundingClientRect();
    const tick = Math.max(0, Math.round(((e.clientX - r.left) / ppb) * tpb / snap) * snap);
    const value = Math.min(hi, Math.max(lo, lo + (1 - (e.clientY - r.top - 6) / h) * (hi - lo)));
    return { tick, value: Math.round(value * 1000) / 1000 };
  };

  const drag = (start: AutoPoint[], idx: number) => {
    api.group("begin", "Automation");
    let pts = start;
    const move = (ev: PointerEvent) => {
      const p = at(ev);
      pts = start.map((q, i) => (i === idx ? p : q));
      setDragPts([...pts].sort((a, b) => a.tick - b.tick));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const sorted = [...pts].sort((a, b) => a.tick - b.tick);
      send(sorted).finally(() => {
        api.group("end");
        setDragPts(null);
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // Polyline across the whole lane: flat before the first and after the last point.
  const line = points.length
    ? [`0,${y(points[0].value)}`, ...points.map((p) => `${x(p.tick)},${y(p.value)}`), `${width},${y(points[points.length - 1].value)}`].join(" ")
    : "";

  return (
    <div className="relative border-b border-black/[0.05] bg-ink-950/60" style={{ height: AUTO_H }}>
      <svg
        ref={ref}
        width={width}
        height={AUTO_H}
        className="absolute inset-0 cursor-crosshair"
        onPointerDown={(e) => {
          if (e.button !== 0 || e.target !== e.currentTarget) return;
          const p = at(e);
          const next = [...points, p].sort((a, b) => a.tick - b.tick);
          setDragPts(next);
          drag(next, next.indexOf(p));
        }}
        onContextMenu={(e) => e.preventDefault()}
      >
        {!points.length && (
          <text x={12} y={AUTO_H / 2 + 4} className="fill-ink-500 text-[11px]">
            Click to draw automation
          </text>
        )}
        {line && <polyline points={line} fill="none" stroke={color} strokeWidth={2} opacity={0.9} />}
        {line && <polygon points={`0,${AUTO_H} ${line} ${width},${AUTO_H}`} fill={color} opacity={0.08} />}
        {points.map((p, i) => (
          <g key={`${p.tick}-${i}`}>
            <circle
              cx={x(p.tick)}
              cy={y(p.value)}
              r={hover === i ? 6 : 4.5}
              fill="var(--color-ink-950)"
              stroke={color}
              strokeWidth={2}
              className="cursor-grab"
              onPointerEnter={() => setHover(i)}
              onPointerLeave={() => setHover(null)}
              onPointerDown={(e) => {
                e.stopPropagation();
                if (e.button === 2) {
                  send(points.filter((_, j) => j !== i));
                  return;
                }
                drag(points, i);
              }}
              onDoubleClick={() => send(points.filter((_, j) => j !== i))}
            />
            {hover === i && (
              <text x={x(p.tick) + 8} y={Math.max(14, y(p.value) - 8)} className={clsx("fill-ink-100 text-[10px] font-medium")}>
                {fmt(p.value)}
              </text>
            )}
          </g>
        ))}
      </svg>
    </div>
  );
});

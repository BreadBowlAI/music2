import clsx from "clsx";
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import type { GraphState } from "../lib/graph";
import { clearAll, drives, inSong, pillWords, release, trackName, useSets } from "../lib/overlays";
import { tones } from "../lib/palette";
import ExportAsk from "./ExportAsk";
import { Button, keepFocus, Popover } from "./ui";

/** What the pill and its popover say mute and solo are: they never reach the file. */
const ABOUT = "Mutes and solos while you listen. The export has every track.";
const NOT_SAVED = "Not saved: this song is open in another window.";

/**
 * The mutes pill (taste.md §3.3, TB-7; design-8b §6.7): "2 muted · 1 solo", just right of the
 * status pill, only while a track of the song is muted or soloed while listening. They are kept
 * when the song closes (MON-4), so a song can reopen half silent: the pill says so. A click opens
 * the list: each track with its own ×, and Clear all at the foot. The pill has no × of its own:
 * mutes aren't undo steps, so clearing them all takes two clicks on purpose.
 */
export default function OverlayPill({ graph }: { graph: GraphState }) {
  const sets = useSets();
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const o = graph.overlays;
  const words = pillWords(o, sets);
  // With nothing left on, the pill goes, and its list with it.
  useEffect(() => {
    if (!words) setAnchor(null);
  }, [words]);
  // The export's question is about these same mutes: it lives here, with the song open.
  if (!words) return <ExportAsk />;
  const saved = o?.saved !== false;
  // In the song first, top to bottom; then any that are gone (commented out), as kept.
  const tracks = graph.timeline.tracks.filter((t) => t.track && t.module !== "song").map((t) => t.module);
  const on = new Set([...sets.mute, ...sets.solo]);
  const rows = [...tracks.filter((k) => on.has(k)), ...[...on].filter((k) => !tracks.includes(k)).sort()];
  return (
    <>
      <button
        data-testid="overlay-pill"
        aria-haspopup="dialog"
        aria-expanded={!!anchor}
        title={saved ? ABOUT : `${NOT_SAVED} ${ABOUT}`}
        onMouseDown={keepFocus}
        onClick={(e) => {
          const b = e.currentTarget;
          setAnchor((a) => (a ? null : b));
        }}
        className="ml-2 h-ctl-sm px-2 shrink-0 rounded-sm inline-flex items-center text-sm font-medium whitespace-nowrap cursor-pointer bg-accent-soft text-accent-strong hover:bg-[color-mix(in_oklab,var(--color-accent-soft)_88%,var(--color-accent-strong))] transition-colors duration-80 ease-linear"
      >
        {words}
      </button>
      {anchor && (
        <Popover anchor={anchor} onClose={() => setAnchor(null)} align="end" role="dialog" aria-label="Mutes and solos" data-testid="overlay-popover" className="w-[360px] max-h-[70vh] overflow-y-auto">
          <div className="py-0.5">
            {rows.map((k) => (
              <Row key={k} graph={graph} module={k} muted={sets.mute.includes(k)} soloed={sets.solo.includes(k)} gone={!inSong(o, k)} />
            ))}
          </div>
          <div className="mt-1 pt-2 pb-1 px-2 border-t border-line flex items-center gap-3">
            <span className="flex-1 text-xs text-fg-muted">{saved ? "Only while you listen: the export has every track." : NOT_SAVED}</span>
            <Button variant="secondary" size="sm" data-testid="overlay-clear-all" onMouseDown={keepFocus} onClick={clearAll} className="shrink-0">
              Clear all
            </Button>
          </div>
        </Popover>
      )}
      <ExportAsk />
    </>
  );
}

/** A track in the list: its colour, its name, what is on, what it still drives, and its ×. */
function Row({ graph, module, muted, soloed, gone }: { graph: GraphState; module: string; muted: boolean; soloed: boolean; gone: boolean }) {
  const name = trackName(graph, module);
  const colour = graph.timeline.tracks.find((t) => t.module === module)?.colour;
  const what = gone ? "Not in the song now" : muted && soloed ? "Muted and soloed" : muted ? "Muted" : "Soloed";
  const still = muted && !gone ? drives(graph.overlays, module) : null;
  return (
    <div data-overlay-row={module} className="min-h-ctl px-2 py-1 flex items-start gap-2">
      <span className={clsx("mt-[5px] size-2.5 shrink-0 rounded-xs", gone && "opacity-40")} style={{ background: tones(colour).bar }} />
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2">
          <span className={clsx("text-sm font-medium truncate", gone ? "text-fg-off" : "text-fg")}>{name}</span>
          <span className={clsx("shrink-0 text-sm", gone ? "text-fg-off" : "text-fg-muted")}>{what}</span>
        </div>
        {still && <div className="text-xs text-fg-muted">Still drives {still}</div>}
      </div>
      <button
        data-overlay-clear={module}
        aria-label={`${muted && soloed ? "Unmute and unsolo" : muted ? "Unmute" : "Unsolo"} ${name}`}
        title={muted && soloed ? "Unmute and unsolo" : muted ? "Unmute" : "Unsolo"}
        onMouseDown={keepFocus}
        onClick={() => release(module)}
        className="size-ctl-xs shrink-0 grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover active:bg-press cursor-pointer transition-colors duration-80 ease-linear"
      >
        <X size={12} />
      </button>
    </div>
  );
}

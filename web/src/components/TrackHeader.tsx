import clsx from "clsx";
import { ChevronRight, CircleAlert, Info, TriangleAlert } from "lucide-react";
import { moduleName, type Diagnostic, type TimelineTrack } from "../lib/graph";
import { useHeard } from "../lib/graph8b";
import { useShell } from "../lib/shell";
import MuteSolo from "./MuteSolo";
import TrackMeter from "./TrackMeter";
import { TrackMenuButton } from "./Tracks";
import TrackName from "./TrackName";

/**
 * What a track's header row shows (taste.md §3.4), placed from its right edge so it fits the
 * 200 px headers of a small window too: the fold chevron at x = 12, the name from x = 28, M and S
 * 60 and 38 px in from the right, the meter 9 px in. The ⋯ shows on hover, just left of M. The
 * Song row has no M or S (MON-1); its meter is the song's output. Offsets from the right count
 * from inside the header's 1 px right border: at 240 px, M is at x = 180.
 */
export default function TrackHeader({
  track,
  folds,
  open,
  selected,
  song,
  issues,
}: {
  track: TimelineTrack;
  /** It has automation lanes to show or hide. */
  folds: boolean;
  /** Its automation lanes show. */
  open: boolean;
  selected: boolean;
  /** The Song's row, at the foot. */
  song: boolean;
  issues: Diagnostic[] | undefined;
}) {
  const module = track.module;
  const name = moduleName({ key: module, label: track.label });
  // M, S, the ⋯ and a meter: a track's. The Song row has the meter only.
  const mine = track.track && !song;
  const heard = useHeard(module);
  // Muted in the song: struck through; muted while listening: muted; off by another's solo: fainter.
  const tone =
    heard === "muted-in-song" ? "text-fg-muted line-through" : heard === "muted" ? "text-fg-muted" : heard === "silenced" ? "text-fg-faint" : selected ? "text-fg-strong" : "text-fg";
  return (
    <>
      {folds && (
        <button
          data-fold={module}
          aria-label={open ? `Hide ${track.label || module}'s automation` : `Show ${track.label || module}'s automation`}
          aria-expanded={open}
          title={`${open ? "Hide" : "Show"} automation (A)`}
          onMouseDown={(e) => e.preventDefault()}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => useShell.getState().toggleLanes(module, [])}
          className="absolute left-2 size-ctl-xs grid place-items-center rounded-sm text-fg-muted hover:text-fg hover:bg-hover cursor-pointer transition-colors duration-80 ease-linear"
        >
          <ChevronRight size={12} className={clsx("transition-transform duration-140 ease-out", open && "rotate-90")} />
        </button>
      )}
      {/* From x = 28 to 12 px before M; while the ⋯ shows, to just before it. */}
      <div
        className={clsx(
          "absolute left-7 flex items-center gap-1.5 min-w-0",
          mine ? "right-[71px] group-hover/head:right-[87px] group-focus-within/head:right-[87px]" : song ? "right-[20px]" : "right-3",
        )}
      >
        <TrackName
          module={module}
          label={track.label}
          track={track.track}
          testid="track-label"
          listen
          className={clsx("text-base", selected ? "font-semibold" : "font-medium", tone)}
        />
        {issues && <Issues issues={issues} />}
      </div>
      {mine && (
        <>
          <div className="absolute right-[63px]">
            <TrackMenuButton module={module} name={track.label || module} />
          </div>
          <div data-slot="mute-solo" className="absolute right-[17px] inset-y-0 w-[42px] flex items-center">
            <MuteSolo module={module} name={name} />
          </div>
        </>
      )}
      {(mine || song) && (
        <div data-slot="meter" className="absolute right-[5px] top-2 bottom-2 w-[3px]">
          <TrackMeter module={module} name={name} />
        </div>
      )}
    </>
  );
}

/** A mark on a track that something in it needs attention: the worst of it, and every message on hover. */
function Issues({ issues }: { issues: Diagnostic[] }) {
  const error = issues.some((d) => d.severity === "error");
  const warning = !error && issues.some((d) => d.severity === "warning");
  const Icon = error ? CircleAlert : warning ? TriangleAlert : Info;
  return (
    <span
      data-testid="track-issues"
      data-severity={error ? "error" : warning ? "warning" : "notice"}
      className={clsx("shrink-0 inline-flex", error ? "text-danger" : warning ? "text-warn" : "text-fg-faint")}
      title={issues.map((d) => d.message).join("\n\n")}
    >
      <Icon size={14} />
    </span>
  );
}

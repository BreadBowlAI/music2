import clsx from "clsx";
import { useEffect, useRef } from "react";
import { clearClips, FLOOR, mount, TOP, useClipped } from "../lib/meters";

/**
 * A peak meter (taste.md §2.8): one canvas, drawn by the meters' shared loop, never by React.
 * `channels` are the meters it shows the louder of: a track's key, a strip's tracks, or `OUT` for
 * the song's output. One bar (the louder side), or two for left and right. Screen readers and the
 * suites read its level as `aria-valuenow`, in dB, at most 10 times a second; a two-bar meter also
 * says each side in `data-left` and `data-right`.
 */
export default function Meter({
  channels,
  label,
  title,
  bars = 1,
  className,
}: {
  channels: string[];
  label: string;
  title?: string;
  bars?: 1 | 2;
  className?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const keys = channels.join(" ");
  useEffect(() => mount(keys.split(" "), ref.current!, bars), [keys, bars]);
  return <canvas ref={ref} role="meter" aria-label={label} title={title} aria-valuemin={FLOOR} aria-valuemax={TOP} className={clsx("block", className)} />;
}

/**
 * A strip's clip dot (taste.md §2.8): 4 px `danger`, lit when one of `channels` reached 0 dBFS, until
 * clicked. Its button is 12 px, so the dot is easy to hit; it never takes focus, so Space still plays.
 */
export function ClipDot({ channels, name, className }: { channels: string[]; name: string; className?: string }) {
  const lit = useClipped(channels);
  if (!lit) return null;
  return (
    <button
      data-testid="clip-dot"
      aria-label={`${name} went over the top. Clear`}
      title="Too loud: it went over the top. Click to clear."
      onMouseDown={(e) => e.preventDefault()}
      onPointerDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onClick={() => clearClips(channels)}
      className={clsx("size-3 grid place-items-center cursor-pointer", className)}
    >
      <span className="block size-1 rounded-full bg-danger" />
    </button>
  );
}

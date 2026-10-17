import clsx from "clsx";
import { useEffect, useRef } from "react";
import { FLOOR, mount, TOP } from "../lib/meters";

/**
 * A peak meter (taste.md §2.8): one canvas, drawn by the meters' shared loop, never by React.
 * `id` is a track's key, or `OUT` for the song's output. Screen readers and the suites read its
 * level as `aria-valuenow`, in dB, at most 10 times a second.
 */
export default function Meter({ id, label, className }: { id: string; label: string; className?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => mount(id, ref.current!), [id]);
  return <canvas ref={ref} role="meter" aria-label={label} aria-valuemin={FLOOR} aria-valuemax={TOP} className={clsx("block", className)} />;
}

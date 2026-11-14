// Grid lines and hatching, drawn as SVG patterns: CSS gradients are out of the design system
// (taste.md §0, §6), and a pattern can take the colour tokens.

import clsx from "clsx";
import { useId } from "react";

/** A pattern id for this component: React's ids hold characters a `url(#…)` reference can't. */
/** An id for a pattern, fit for `url(#…)`. */
export const usePatternId = () => `p${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;

/** One set of evenly spaced 1px lines: every `every` px, the first `at` px from the start. */
export interface Lines { every: number; at?: number; color: string }

/** Vertical lines across the whole box, a layer per spacing (bars over beats over sixteenths: last on top). */
export function VLines({ layers, className, style }: { layers: Lines[]; className?: string; style?: React.CSSProperties }) {
  const id = usePatternId();
  return (
    <svg aria-hidden className={clsx("absolute pointer-events-none", className ?? "inset-0")} style={style} width="100%" height="100%">
      <defs>
        {layers.map((l, i) => (
          <pattern key={i} id={`${id}-${i}`} patternUnits="userSpaceOnUse" x={l.at ?? 0} y={0} width={l.every} height={1}>
            <rect width={1} height={1} fill={l.color} />
          </pattern>
        ))}
      </defs>
      {layers.map((_, i) => (
        <rect key={i} width="100%" height="100%" fill={`url(#${id}-${i})`} />
      ))}
    </svg>
  );
}

/** Horizontal bands repeating every `period` px down the box: each band a fill from `y`, `h` tall. */
export function HBands({ period, bands }: { period: number; bands: { y: number; h: number; color: string }[] }) {
  const id = usePatternId();
  return (
    <svg aria-hidden className="absolute inset-0 pointer-events-none" width="100%" height="100%">
      <defs>
        <pattern id={id} patternUnits="userSpaceOnUse" x={0} y={0} width={1} height={period}>
          {bands.map((b, i) => (
            <rect key={i} y={b.y} width={1} height={b.h} fill={b.color} />
          ))}
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id})`} />
    </svg>
  );
}

/** 45° hatching, for what lies outside a clip. */
export function Hatch({ color = "var(--color-line)" }: { color?: string }) {
  const id = usePatternId();
  return (
    <svg aria-hidden className="absolute inset-0 pointer-events-none" width="100%" height="100%">
      <defs>
        <pattern id={id} patternUnits="userSpaceOnUse" width={8} height={8} patternTransform="rotate(45)">
          <rect width={1.5} height={8} fill={color} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id})`} />
    </svg>
  );
}

/**
 * The patch canvas's grid: a dot every `every` of the graph's px, times its `zoom`, from the graph's
 * top left, so a zoom or a pan carries the dots with the nodes (taste.md VIEW-5). A dot stays 1 px in
 * radius; where they would be under 12 px apart every other one is drawn, so they never become a fill.
 */
export function Dots({ every = 20, zoom = 1, color = "var(--color-line-strong)" }: { every?: number; zoom?: number; color?: string }) {
  const id = usePatternId();
  let gap = every * zoom;
  while (gap < 12) gap *= 2;
  return (
    <svg aria-hidden className="absolute inset-0 pointer-events-none" width="100%" height="100%">
      <defs>
        <pattern id={id} patternUnits="userSpaceOnUse" x={-gap / 2} y={-gap / 2} width={gap} height={gap}>
          <circle cx={gap / 2} cy={gap / 2} r={1} fill={color} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id})`} />
    </svg>
  );
}

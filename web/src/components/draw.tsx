// Grid lines and hatching, drawn as SVG patterns: CSS gradients are out of the design system
// (taste.md §0, §6), and a pattern can take the colour tokens.

import clsx from "clsx";
import { useId } from "react";

/** A pattern id for this component: React's ids hold characters a `url(#…)` reference can't. */
const usePatternId = () => `p${useId().replace(/[^a-zA-Z0-9_-]/g, "")}`;

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

/** A dot every `every` px: the patch canvas's grid. */
export function Dots({ every = 20, color = "var(--color-line-strong)" }: { every?: number; color?: string }) {
  const id = usePatternId();
  return (
    <svg aria-hidden className="absolute inset-0 pointer-events-none" width="100%" height="100%">
      <defs>
        <pattern id={id} patternUnits="userSpaceOnUse" x={-every / 2} y={-every / 2} width={every} height={every}>
          <circle cx={every / 2} cy={every / 2} r={1} fill={color} />
        </pattern>
      </defs>
      <rect width="100%" height="100%" fill={`url(#${id})`} />
    </svg>
  );
}

// An automation lane's scale (the polish plan's D3, X17): how its values sit in a clip's drawing,
// and the line drawn between its points. A lane in Hz draws on a log scale from 20 Hz to 20 kHz,
// widened to hold every point, as a filter's knob turns: the middle is about 632 Hz, and 1 kHz a
// little above it. Its points still play straight in value from one to the next, so that
// line curves on the log scale. Every other unit draws as it did: linear, over the range the
// server gives (`min`, `max`: the unit's own, widened to the values there are).

import type { TimelineLane } from "./graph";
import { line, rounded, type Point } from "./points";

type ControlLane = Extract<TimelineLane, { kind: "control" }>;

export interface LaneScale {
  /** True on a log scale. */
  log: boolean;
  /** Where value `v` sits: 0 at the bottom of the drawing, 1 at its top. */
  level: (v: number) => number;
  /** The value at `level` (0 the bottom, 1 the top; past them, on past the range). */
  value: (level: number) => number;
  /** A value read off the drawing, rounded as a person would set it: 3 figures on a log scale. */
  round: (v: number) => number;
  /**
   * The line through `points` over a clip `length` beats long, the first value held from its start
   * and the last to its end. On a log scale each sloped piece is cut into 16, so it curves as it plays.
   */
  path: (points: readonly Point[], length: number) => Point[];
}

/** The pieces each sloped segment is cut into on a log scale. */
const PIECES = 16;
/** A frequency lane's range, before it widens to its points (Hz). */
const HZ: [number, number] = [20, 20000];

/** The scale lane `lane` draws on. */
export function laneScale(lane: ControlLane): LaneScale {
  if (lane.unit === "hz") return logScale(lane);
  const range = lane.max - lane.min || 1;
  return {
    log: false,
    level: (v) => (v - lane.min) / range,
    value: (l) => lane.min + l * range,
    round: (v) => rounded(v, range),
    path: line,
  };
}

function logScale(lane: ControlLane): LaneScale {
  const values = [...lane.clips.flatMap((c) => c.points.map((p) => p[1])), ...(lane.default != null ? [lane.default] : [])].filter((v) => v > 0);
  const lo = Math.log(Math.min(HZ[0], ...values)), hi = Math.log(Math.max(HZ[1], ...values));
  const floor = Math.exp(lo);
  return {
    log: true,
    // Nothing is below the bottom: 0 Hz sits there.
    level: (v) => (Math.log(Math.max(v, floor)) - lo) / (hi - lo),
    value: (l) => Math.exp(lo + l * (hi - lo)),
    round: (v) => (v > 0 ? Number(v.toPrecision(3)) : 0),
    path: (points, length) => {
      const drawn = line(points, length);
      const out: Point[] = drawn.slice(0, 1);
      for (let i = 1; i < drawn.length; i++) {
        const [b0, v0] = drawn[i - 1], [b1, v1] = drawn[i];
        // A step or a flat piece is straight on any scale.
        if (v0 !== v1 && b1 > b0) for (let k = 1; k < PIECES; k++) out.push([b0 + ((b1 - b0) * k) / PIECES, v0 + ((v1 - v0) * k) / PIECES]);
        out.push(drawn[i]);
      }
      return out;
    },
  };
}

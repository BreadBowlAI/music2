// The bar (taste.md DV-14): the graph's one control for a number, at both sizes of a node. It shows
// the value's label, its number, and a fill for how far along its range it is. The fill follows
// the knob's own curve (the unit `value` is already on it: a frequency's is a log scale's), and a
// value with two sides fills from the middle, so 0 reads as empty. Its drag is its row's
// (`useKnobDrag`: up and down only, ⇧ for fine steps, ⌥-click to reset); its keys are DV-15's.

import clsx from "clsx";
import { useEffect, useRef } from "react";
import { Bubble, NOTCH } from "./Knob";

/**
 * Where the fill runs, from `lo` to `hi` (0..1 of the bar): from the left for a value with one side,
 * from the middle for one with two.
 */
export function fillOf(value: number, bipolar: boolean): [number, number] {
  const v = Math.min(1, Math.max(0, value));
  if (!bipolar) return [0, v];
  return v < 0.5 ? [v, 0.5] : [0.5, v];
}

export function Bar({
  value,
  bipolar,
  disabled,
  readout,
  label,
  reset = 0.5,
  bar,
  pointer,
  dragging,
  children,
  onChange,
  onStep,
  onType,
}: {
  /** Where it is along its range, 0..1, on its own curve. */
  value: number;
  /** Its range is centred on 0: it fills from the middle. */
  bipolar: boolean;
  disabled: boolean;
  /** The value as it reads, for the bubble while dragging and for a screen reader. */
  readout: string;
  label: string;
  /** Where ⌫ puts it: its declared default, else the middle. */
  reset?: number;
  bar?: React.RefObject<HTMLDivElement | null>;
  /** Where the pointer is while it is dragged. */
  pointer: { x: number; y: number } | null;
  dragging: boolean;
  /** What it says over its fill: its label and its number, or the field a value is typed in. */
  children: React.ReactNode;
  onChange: (v: number) => void;
  /** `n` steps up (negative: down), from the wheel or the arrow keys. */
  onStep: (n: number) => void;
  /** Type a value in place of the number. */
  onType: () => void;
}) {
  const own = useRef<HTMLDivElement>(null);
  const el = bar ?? own;
  const wheel = useRef(0);
  const step = useRef(onStep);
  step.current = onStep;
  const held = useRef(false);
  held.current = !!pointer;
  // The wheel steps it, one step a notch, as a knob's did (taste.md §5.2); only an up-and-down
  // scroll does, so a sideways one scrolls the graph; ⌘ or ⌃ with it zooms the graph instead.
  useEffect(() => {
    const b = el.current;
    if (!b || disabled) return;
    const turn = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey || held.current || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      e.preventDefault();
      wheel.current -= e.deltaMode === 1 ? e.deltaY * NOTCH : e.deltaY;
      const n = Math.trunc(wheel.current / NOTCH);
      if (n === 0) return;
      wheel.current -= n * NOTCH;
      step.current(n);
    };
    b.addEventListener("wheel", turn, { passive: false });
    return () => b.removeEventListener("wheel", turn);
  }, [el, disabled]);
  const [lo, hi] = fillOf(value, bipolar);
  return (
    <div
      ref={el}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={Number(value.toFixed(4))}
      aria-valuetext={readout}
      aria-disabled={disabled || undefined}
      data-bar
      data-lo={Number(lo.toFixed(4))}
      data-hi={Number(hi.toFixed(4))}
      className={clsx(
        "relative h-5 min-w-0 flex-1 rounded-sm overflow-hidden outline-none bg-chrome",
        !disabled && "group-hover/knob:bg-hover focus-visible:ring-2 focus-visible:ring-accent",
        dragging && "ring-1 ring-accent",
      )}
      onKeyDown={(e) => {
        if (e.metaKey || e.ctrlKey || e.altKey || pointer) return;
        // One step, ten with ⇧ (taste.md §5.2); a tenth of its range; its lowest and highest (DV-15).
        const up = e.key === "ArrowUp" || e.key === "ArrowRight";
        if (up || e.key === "ArrowDown" || e.key === "ArrowLeft") onStep((up ? 1 : -1) * (e.shiftKey ? 10 : 1));
        else if (e.key === "PageUp" || e.key === "PageDown") onChange(Math.min(1, Math.max(0, value + (e.key === "PageUp" ? 0.1 : -0.1))));
        else if (e.key === "Home" || e.key === "End") onChange(e.key === "Home" ? 0 : 1);
        else if (e.key === "Enter") onType();
        else if (e.key === "Backspace" || e.key === "Delete") {
          if (Math.abs(reset - value) > 1e-6) onChange(reset);
        } else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {/* The fill, then a hairline where it starts from the middle; the words over both. */}
      <span
        aria-hidden
        data-fill
        className={clsx("absolute inset-y-0 pointer-events-none", disabled ? "bg-line" : "bg-accent-soft")}
        style={{ left: `${lo * 100}%`, width: `${(hi - lo) * 100}%` }}
      />
      {bipolar && <span aria-hidden className="absolute inset-y-1 left-1/2 w-px bg-line-strong" />}
      <span className="relative h-full px-1.5 flex items-center gap-1.5 text-xs min-w-0">{children}</span>
      {pointer && <Bubble at={pointer}>{readout}</Bubble>}
    </div>
  );
}

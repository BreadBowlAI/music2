import clsx from "clsx";
import { Keyboard, X } from "lucide-react";
import { useLayoutEffect, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { chord } from "../lib/native";
import { tones } from "../lib/palette";
import { BLACK, HIGHEST, LOWEST, NOTE_KEYS, WHITE, pitchOf, setPiano, usePiano } from "../lib/piano";
import { usePlayTarget } from "../lib/playTarget";
import { pitchName } from "../lib/rowNames";
import { useStore } from "../lib/store";
import { Button } from "./ui";

/** A cap's width and height, and the step from one white cap to the next. */
const CAP_W = 22;
const CAP_H = 20;
const STEP = CAP_W + 2;

/** Where the card sits: 16 px in from the timeline's lower right corner, whatever the dock and the browser take. */
function useCorner() {
  const session = useStore((s) => s.graph?.session);
  const [at, setAt] = useState<{ right: number; bottom: number } | null>(null);
  useLayoutEffect(() => {
    const timeline = document.querySelector<HTMLElement>("[data-testid=timeline]");
    if (!timeline) return setAt(null);
    const measure = () => {
      const r = timeline.getBoundingClientRect();
      setAt({ right: window.innerWidth - r.right + 16, bottom: window.innerHeight - r.bottom + 16 });
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(timeline);
    window.addEventListener("resize", measure);
    return () => {
      watch.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [session]);
  return at;
}

/**
 * The keyboard piano's card (the next track's B2): while it is on, at the timeline's lower right, the
 * keys it takes and the notes they play, the octave, and which track plays. A held key lights in that
 * track's colour, as the roll's keys do. Mounted once, with a song open (App.tsx).
 */
export default function KeyboardPiano() {
  const on = usePiano((s) => s.on);
  const open = useStore((s) => !!s.graph);
  return on && open ? <Card /> : null;
}

function Card() {
  const octave = usePiano((s) => s.octave);
  const held = usePiano((s) => s.held);
  const target = usePlayTarget();
  const at = useCorner();
  const tone = tones(target?.colour);
  const lit = (code: string): CSSProperties | undefined => (held.has(code) && target ? { background: tone.tint, borderColor: tone.bar, color: tone.ink } : undefined);
  const name = (code: string) => pitchName(pitchOf(code, octave));
  const low = name("KeyA"), high = pitchName(pitchOf("Quote", octave));
  return createPortal(
    <div
      data-testid="keyboard-piano"
      role="group"
      aria-label="Keyboard piano"
      className="fixed z-[44] p-3 rounded-md border border-line bg-raised shadow-lift animate-rise cursor-default"
      style={at ? { right: at.right, bottom: at.bottom } : { right: 16, bottom: 16 }}
    >
      <div className="h-ctl-xs flex items-center gap-1.5" style={{ width: WHITE.length * STEP - 2 }}>
        <Keyboard size={14} className="shrink-0 text-fg-muted" />
        <span className="shrink-0 text-sm font-medium text-fg">Keyboard piano</span>
        <span data-testid="piano-plays" className="flex-1 min-w-0 truncate text-xs text-fg-muted" title={target ? `Your keys play ${target.label}` : undefined}>
          {target ? `Plays ${target.label}` : "Select a track with notes to play it"}
        </span>
        <Button size="sm" aria-label="Keyboard piano off" title={`Keyboard piano off (${chord("K")})`} onClick={() => setPiano(false)} className="-mr-1 w-ctl-xs !h-ctl-xs !px-0 shrink-0">
          <X size={14} />
        </Button>
      </div>
      {/* The black keys over the white ones, as on a piano; each cap says its key. */}
      <div className="relative mt-2" style={{ width: WHITE.length * STEP - 2, height: 2 * CAP_H + 4 }}>
        {BLACK.map(([code, cap, after]) => (
          <span
            key={code}
            data-piano-key={code}
            data-pitch={pitchOf(code, octave)}
            data-held={held.has(code) || undefined}
            title={`${cap}: ${name(code)}`}
            className={clsx("absolute top-0 grid place-items-center rounded-sm border text-xs font-medium", !lit(code) && "bg-gray-700 border-gray-700 text-gray-0")}
            style={{ left: (after + 1) * STEP - 1 - CAP_W / 2, width: CAP_W, height: CAP_H, ...lit(code) }}
          >
            {cap}
          </span>
        ))}
        {WHITE.map(([code, cap], i) => (
          <span
            key={code}
            data-piano-key={code}
            data-pitch={pitchOf(code, octave)}
            data-held={held.has(code) || undefined}
            title={`${cap}: ${name(code)}`}
            className={clsx("absolute grid place-items-center rounded-sm border text-xs font-medium", !lit(code) && "bg-canvas border-line-strong text-fg")}
            style={{ left: i * STEP, top: CAP_H + 4, width: CAP_W, height: CAP_H, ...lit(code) }}
          >
            {cap}
          </span>
        ))}
      </div>
      {/* The notes the white keys play: each C with its octave. */}
      <div className="mt-1 flex text-xs text-fg-faint" aria-hidden>
        {WHITE.map(([code]) => (
          <span key={code} className="shrink-0 text-center" style={{ width: STEP }}>
            {NOTE_KEYS[code] % 12 === 0 ? name(code) : name(code).replace(/-?\d+$/, "")}
          </span>
        ))}
      </div>
      <div data-testid="piano-octave" className="mt-2 flex items-center justify-center gap-2 text-xs text-fg-muted">
        <Cap title={`Octave down (Z)${octave === LOWEST ? ": the lowest" : ""}`} off={octave === LOWEST}>
          Z
        </Cap>
        <span className="min-w-14 text-center text-fg">
          {low}–{high}
        </span>
        <Cap title={`Octave up (X)${octave === HIGHEST ? ": the highest" : ""}`} off={octave === HIGHEST}>
          X
        </Cap>
      </div>
    </div>,
    document.body,
  );
}

const Cap = ({ title, off, children }: { title: string; off: boolean; children: string }) => (
  <span title={title} className={clsx("grid place-items-center rounded-sm border text-xs font-medium", off ? "border-line text-fg-off" : "border-line-strong bg-canvas text-fg")} style={{ width: CAP_W, height: CAP_H }}>
    {children}
  </span>
);

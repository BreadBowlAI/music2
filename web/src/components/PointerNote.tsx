import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { onViewMoved } from "../camera";
import { behindFold, dismissPointer, elsewhere, reveal, seenIn, targetBox, usePointer, type Pointer } from "../lib/pointer";
import { useRenderCount } from "../lib/frames";
import { canvas, useShell } from "../lib/shell";
import { useStore } from "../lib/store";
import { useExpanded } from "../lib/expanded";
import { useLocalStore } from "../lib/local";
import { keepFocus } from "./ui";

type Edge = "left" | "right" | "up" | "down";

/** Where the note goes: the ring round its target while that is in view, the note's own place,
 * the edge it waits at when the target is out of view, and whether Show can bring it in; and the
 * knob it names, when it stands beside the header of a track the dock doesn't show. */
interface Place {
  mark: { left: number; top: number; width: number; height: number } | null;
  style: CSSProperties;
  edge: Edge | null;
  show: boolean;
  names?: string | null;
}

const NOTE_W = 320;
const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));

/** Where pointer `p`'s note goes now; null when nothing it would sit in is on screen. */
function placeOf(p: Pointer): Place | null {
  const area = seenIn(p);
  if (!area || area.width <= 0 || area.height <= 0) return null;
  // A knob of a track the dock doesn't show: its track's header stands for it, naming it.
  const aside = elsewhere(p);
  const place = placed(p, area, aside);
  return place && { ...place, names: aside ? (p.label ?? p.knob) : null };
}

/** [`placeOf`], once it knows where `p` can be seen (`area`), and whether it stands `aside`. */
function placed(p: Pointer, area: DOMRect, aside: boolean): Place | null {
  const left = (x: number) => clamp(x, 8, window.innerWidth - NOTE_W - 8);
  let box = targetBox(p);
  // A lane in folded automation isn't drawn: its track's header stands for it, and Show unfolds it.
  const folded = !box && (p.kind === "lane" || p.kind === "clip") && p.track ? targetBox({ ...p, kind: "track" }) : null;
  box ??= folded;
  if (!box) {
    // Not drawn now (a knob of the dock's track in another of its views): it waits at the top of
    // the dock, and Show shows the Graph.
    return { mark: null, style: { left: left(area.left + area.width / 2 - NOTE_W / 2), top: area.top + 8 }, edge: "down", show: true };
  }
  const seen = box.right > area.left && box.left < area.right && box.bottom > area.top && box.top < area.bottom;
  if (seen) {
    const l = Math.max(box.left, area.left), t = Math.max(box.top, area.top);
    const mark = { left: l, top: t, width: Math.min(box.right, area.right) - l, height: Math.min(box.bottom, area.bottom) - t };
    let style: CSSProperties;
    if (p.kind === "track" || p.kind === "lane" || folded || aside) {
      // Beside a header, over its lane: the header stays readable.
      style = { left: left(mark.left + mark.width + 8), top: mark.top + mark.height / 2, transform: "translateY(-50%)" };
    } else if (p.kind === "bars") {
      // Under the ruler's bars.
      style = { left: left(mark.left), top: mark.top + mark.height + 6 };
    } else {
      // Above a clip or a knob while there is room in what shows it, else under it.
      const above = mark.top - area.top > 48;
      style = above ? { left: left(mark.left), top: mark.top - 6, transform: "translateY(-100%)" } : { left: left(mark.left), top: mark.top + mark.height + 6 };
    }
    // A knob behind a folded card's "N more" row: Show unfolds it.
    return { mark, style, edge: null, show: !!folded || behindFold(p) };
  }
  // Out of view: it waits at the edge nearest where its target is (AGENT-6); bars under the ruler.
  const edge: Edge = box.right <= area.left ? "left" : box.left >= area.right ? "right" : box.bottom <= area.top ? "up" : "down";
  const lanes = p.kind === "bars" ? canvas.lanesBox?.() : null;
  const y = lanes ? lanes.top + 28 : clamp(box.top + box.height / 2, area.top + 24, area.bottom - 24);
  const x = left(clamp(box.left, area.left + 8, area.right - 8) - NOTE_W / 2);
  const style: CSSProperties =
    edge === "left"
      ? { left: left(area.left + 8), top: y, transform: "translateY(-50%)" }
      : edge === "right"
        ? { right: Math.max(8, window.innerWidth - area.right + 8), top: y, transform: "translateY(-50%)" }
        : edge === "up"
          ? { left: x, top: area.top + 8 }
          : { left: x, top: area.bottom - 8, transform: "translateY(-100%)" };
  return { mark: null, style, edge, show: true };
}

const ARROWS = { left: ArrowLeft, right: ArrowRight, up: ArrowUp, down: ArrowDown };

/** What of a place only a render can show: whether it rings its target, its edge, Show, the knob it names, which sides it is placed by. */
const shapeOf = (p: Place | null) =>
  p && JSON.stringify({ mark: !!p.mark, edge: p.edge, show: p.show, names: p.names ?? null, sides: Object.keys(p.style), transform: p.style.transform ?? null });

/** Write where a place is into the note's and the ring's own style: a move needs no render. */
function write(p: Place, mark: HTMLElement | null, note: HTMLElement | null) {
  if (mark && p.mark) for (const [k, v] of Object.entries(p.mark)) mark.style.setProperty(k, `${v}px`);
  if (note) for (const [k, v] of Object.entries(p.style)) if (k !== "transform" && typeof v === "number") note.style.setProperty(k, `${v}px`);
}

/**
 * An agent's pointer (AGENT-6): a small note at its target, "Claude: listen to the hats here", with
 * OK. It takes no focus, and no press but its buttons': one on its words reaches what is under it.
 * It never scrolls, zooms or selects: out of view, it waits at the edge with Show, and only that
 * click moves the view.
 */
export default function PointerNote({ session }: { session: number }) {
  useRenderCount("pointer.note");
  const pointer = usePointer((s) => s.pointer);
  const p = pointer?.session === session ? pointer : null;
  const [place, setPlace] = useState<Place | null>(null);
  const markEl = useRef<HTMLDivElement>(null);
  const noteEl = useRef<HTMLDivElement>(null);
  const placeNow = useRef<Place | null>(null);
  // Follows its target on each frame a view moved (taste.md VIEW-8, the camera note's §7), and as
  // the window, the song or the shell changes: where it is written straight into its own style; a
  // render only when what it shows changes (its ring, its edge, Show, the knob it names).
  useEffect(() => {
    if (!p) return setPlace(null);
    let shown: string | null = null;
    const follow = () => {
      const next = placeOf(p);
      placeNow.current = next;
      const shape = shapeOf(next);
      if (shape !== shown) {
        shown = shape;
        setPlace(next);
      } else if (next) write(next, markEl.current, noteEl.current);
    };
    // After React has drawn a change of the song or the shell.
    let frame = 0;
    const soon = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(follow);
    };
    follow();
    // The song, the shell (the dock, a track unfolded), the view's settings (track heights), a node expanded.
    const offs = [onViewMoved(follow), useStore.subscribe(soon), useShell.subscribe(soon), useLocalStore.subscribe(soon), useExpanded.subscribe(soon)];
    // A box the camera doesn't move (Mix's strips), the window's size, and the end of a move that
    // eases (the Graph's nodes moving aside).
    document.addEventListener("scroll", follow, { capture: true, passive: true });
    document.addEventListener("transitionend", soon, true);
    window.addEventListener("resize", soon);
    return () => {
      cancelAnimationFrame(frame);
      offs.forEach((off) => off());
      document.removeEventListener("scroll", follow, { capture: true });
      document.removeEventListener("transitionend", soon, true);
      window.removeEventListener("resize", soon);
    };
  }, [p]);
  // A render writes where it is now: a move written since may have left React's own style behind.
  useLayoutEffect(() => {
    if (placeNow.current) write(placeNow.current, markEl.current, noteEl.current);
  });
  if (!p || !place) return null;
  const Arrow = place.edge ? ARROWS[place.edge] : null;
  return (
    <>
      {place.mark && (
        <div
          ref={markEl}
          data-testid="pointer-mark"
          aria-hidden
          className="fixed z-[44] pointer-events-none rounded-sm outline-2 outline-dashed outline-accent outline-offset-2"
          style={{ left: place.mark.left, top: place.mark.top, width: place.mark.width, height: place.mark.height }}
        />
      )}
      {/* Placed by the outer box; the card rises in it, so its animation never moves where it sits. */}
      <div ref={noteEl} className="fixed z-[44] max-w-[320px] pointer-events-none" style={place.style}>
        <div
          data-testid="pointer-note"
          data-edge={place.edge ?? undefined}
          role="note"
          aria-label={`${p.by ?? "A pointer"}: ${p.words}`}
          className="min-h-ctl px-3 py-1.5 rounded-md bg-raised shadow-pop text-base text-fg animate-rise flex items-center gap-3"
        >
          {Arrow && <Arrow size={14} className="shrink-0 text-accent" aria-hidden />}
          <span className="min-w-0 break-words">
            {p.by && <span className="font-medium">{p.by}: </span>}
            {p.words || "here"}
            {place.names && <span className="text-fg-muted"> · {place.names}</span>}
          </span>
          {place.show && (
            <button
              data-testid="pointer-show"
              onMouseDown={keepFocus}
              onClick={() => reveal(p)}
              className="shrink-0 pointer-events-auto text-accent hover:text-accent-strong font-medium cursor-pointer transition-colors duration-80 ease-linear"
            >
              Show
            </button>
          )}
          <button
            data-testid="pointer-ok"
            onMouseDown={keepFocus}
            onClick={dismissPointer}
            className="shrink-0 pointer-events-auto text-accent hover:text-accent-strong font-medium cursor-pointer transition-colors duration-80 ease-linear"
          >
            OK
          </button>
        </div>
      </div>
    </>
  );
}

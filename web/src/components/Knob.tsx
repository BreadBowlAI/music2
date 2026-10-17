import clsx from "clsx";
import { RotateCcw, Spline } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { display, graphApi, type Home, type Knob as KnobInfo } from "../lib/graph";
import { useStore } from "../lib/store";
import { guideDid } from "../lib/guide";

// What every knob and fader shares (taste.md §5.2, KN-1, KN-2): a drag heard while it moves and
// written once when it is let go (WRITE-22's leases), the drag itself, the small rotary knob, its
// menu, and the value bubble over the pointer. The Device view's knob rows (NodeGraph) and the Mix
// view's strips (Mix) are built from these.

/**
 * The value bubble over the pointer while a value is dragged (taste.md §2.14); under it when the
 * pointer is too near the window's top for it to show (the top bar's tempo, dragged up).
 */
export function Bubble({ at, children }: { at: { x: number; y: number }; children: React.ReactNode }) {
  const under = at.y < 40;
  return createPortal(
    <div
      aria-hidden
      data-testid="value-bubble"
      className={clsx(
        "fixed z-50 pointer-events-none -translate-x-1/2 px-1.5 py-0.5 rounded-sm bg-fg-strong/92 text-white text-xs tabular-nums whitespace-nowrap",
        !under && "-translate-y-full",
      )}
      style={{ left: at.x, top: under ? Math.max(at.y + 20, 48) : at.y - 12 }}
    >
      {children}
    </div>,
    document.body,
  );
}

/** One drag (or run of steps) of one control: the lease it holds on the server, and how it ends. */
interface Gesture {
  token: string;
  session: number;
  revision: number;
  knob: KnobInfo;
  start: number;
  previews: Promise<void>;
  previewed: boolean;
  /** Let go: from here it commits, even if the control unmounts. */
  released: boolean;
  cancelled: boolean;
  submitted: boolean;
  failed: boolean;
}

/**
 * A control over an owned source value (WRITE-22): each move is heard at once (`/preview`, which
 * begins the gesture's lease and keeps it alive while held still), and letting go writes it once
 * (`/set`), one undo step named `name`. A save elsewhere doesn't end a gesture: the server carries
 * it to the new revision, or refuses it as stale if the save touched this value. Unmounting cancels
 * a drag, but a released one still commits: selecting another module right after letting go must
 * not lose the value. Values here are the control's own (dB, Hz), never a position.
 */
export function useKnobGesture({
  session,
  revision,
  knob,
  editable,
  name,
  label,
  hold,
}: {
  session: number;
  revision: number;
  knob: KnobInfo;
  editable: boolean;
  /** Its undo step's name: "Delay feedback", "Hats volume". */
  name: string;
  /** Its name on screen ("Feedback"), for what a toast says of it. */
  label: string;
  /** Told when a gesture begins (true) and when it is over (false), by its token. */
  hold?: (token: string, on: boolean) => void;
}) {
  const toast = useStore((s) => s.toast);
  const [value, setValue] = useState(knob.value);
  const [saving, setSaving] = useState(false);
  const latest = useRef(knob.value);
  const dragging = useRef<Gesture | null>(null);
  const active = useRef<Gesture | null>(null);
  const pending = useRef<{ gesture: Gesture; value: number } | null>(null);
  const timer = useRef<number | null>(null);
  // While a control is held still, its drag is kept alive on the server, which ends drags nobody
  // moves for 10 s (WRITE-22's leases, C6).
  const keepalive = useRef<number | null>(null);
  const mounted = useRef(true);
  const committing = useRef(false);
  // A run of steps (the wheel, ↑ ↓) is one gesture, written once they stop for a moment.
  const idle = useRef<number | null>(null);
  const held = useRef(hold);
  held.current = hold;

  const stopKeepalive = () => {
    if (keepalive.current != null) clearInterval(keepalive.current);
    keepalive.current = null;
  };

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current != null) clearTimeout(timer.current);
      timer.current = null;
      if (idle.current != null) clearTimeout(idle.current);
      idle.current = null;
      stopKeepalive();
      pending.current = null;
      dragging.current = null;
      if (active.current && !active.current.released) void cancel(active.current);
    };
  }, []);

  useEffect(() => {
    if (!dragging.current && !committing.current) {
      setValue(knob.value);
      latest.current = knob.value;
    }
  }, [knob.value]);

  const refresh = () => {
    // Never let a late response replace another project's view or a newer revision.
    void graphApi.get().then((graph) => {
      const store = useStore.getState();
      if (store.graph?.session === session && graph.session === session && graph.view.revision >= store.graph.view.revision) {
        store.set({ graph });
      }
    }).catch(() => {});
  };

  const fail = (gesture: Gesture, error: unknown) => {
    if (gesture.failed) return;
    gesture.failed = true;
    void cancel(gesture);
    // A released control reports even from a closed panel: its value didn't save.
    if (mounted.current || gesture.released) toast(error instanceof Error ? error.message : String(error));
    if (!mounted.current) return;
    setValue(knob.value);
    latest.current = knob.value;
    refresh();
  };

  const cancel = (gesture: Gesture): Promise<void> => {
    // Once a save is sent it owns completion. Cancelling then could race a
    // successful commit; stale cancellation must never undo a newer gesture.
    if (gesture.cancelled || gesture.submitted) return Promise.resolve();
    gesture.cancelled = true;
    if (active.current === gesture) active.current = null;
    held.current?.(gesture.token, false);
    return gesture.previews.then(() => graphApi.cancel(gesture.session, gesture.knob.id, gesture.token)).catch((error) => {
      if (mounted.current) toast(error instanceof Error ? error.message : String(error));
    });
  };

  /** A gesture on this control, or (`on`) on a control the page has no view of yet: a first pan's new knob, in the revision that wrote it. */
  const begin = (on?: { knob: KnobInfo; revision: number }): Gesture => {
    const k = on?.knob ?? knob;
    const gesture: Gesture = {
      token: crypto.randomUUID(), session, revision: on?.revision ?? revision, knob: k, start: k.value,
      previews: Promise.resolve(), previewed: false, released: false, cancelled: false, submitted: false, failed: false,
    };
    active.current = gesture;
    held.current?.(gesture.token, true);
    return gesture;
  };

  const preview = (gesture: Gesture, v: number) => {
    gesture.previewed = true;
    // Preserve preview order, including the final preview before committing.
    gesture.previews = gesture.previews.then(async () => {
      if (!mounted.current || gesture.failed || gesture.cancelled) return;
      await graphApi.preview(gesture.session, gesture.revision, gesture.knob.id, gesture.token, v);
    }).catch((error) => fail(gesture, error));
  };

  const flushPreview = () => {
    timer.current = null;
    const next = pending.current;
    pending.current = null;
    if (next) preview(next.gesture, next.value);
  };

  const commit = async (gesture: Gesture, v: number) => {
    if (!gesture.previewed && v === gesture.start) {
      if (active.current === gesture) active.current = null;
      held.current?.(gesture.token, false);
      return;
    }
    gesture.released = true;
    committing.current = true;
    setSaving(true);
    try {
      await gesture.previews;
      if (gesture.failed || gesture.cancelled) {
        void cancel(gesture);
        return;
      }
      // Returning to the starting value still needs a reliable commit: the
      // final audition message may have been dropped by the audio queue.
      gesture.submitted = true;
      const change = await graphApi.set(gesture.session, gesture.revision, gesture.knob.id, gesture.token, v, gesture.start, { name });
      // The code doesn't compile right now: the value plays on, and is saved once it does (C5).
      // The control shows the value, so a change makes no toast (taste.md §2.14); one that waits for the code to compile says so.
      if (change.pending) toast(`${label} is saved once the code compiles again`);
      refresh();
      guideDid("sound");
    } catch (error) {
      // Rejections restore audio on the server. An owner-checked cancellation
      // also covers a lost response without targeting another gesture's preview.
      gesture.submitted = false;
      fail(gesture, error);
    } finally {
      if (active.current === gesture) active.current = null;
      held.current?.(gesture.token, false);
      committing.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  /** A move to value `v`: heard while dragged; on its own (a double-click, the menu's reset, a typed value) one whole gesture. */
  const change = (v: number) => {
    if ((!editable && !dragging.current) || committing.current) return;
    const gesture = dragging.current ?? begin();
    if (gesture.failed) return;
    setValue(v);
    latest.current = v;
    if (!dragging.current) {
      // A double-click reset is a complete, revision-bound gesture.
      preview(gesture, v);
      void commit(gesture, v);
      return;
    }
    pending.current = { gesture, value: v };
    if (timer.current == null) timer.current = window.setTimeout(flushPreview, 30);
  };

  /** A drag (or a run of steps) begins: one gesture, heard as it goes and kept alive on the server. `on`: see `begin`. */
  const start = (on?: { knob: KnobInfo; revision: number }) => {
    // A run of steps still open goes on as this drag: one gesture, one edit.
    if (idle.current != null) clearTimeout(idle.current);
    idle.current = null;
    if (dragging.current || !editable || committing.current) return;
    dragging.current = begin(on);
    latest.current = dragging.current.knob.value;
    stopKeepalive();
    keepalive.current = window.setInterval(() => {
      const gesture = dragging.current;
      // Only a drag that began on the server, and has nothing on its way already.
      if (gesture && gesture.previewed && !gesture.failed && !gesture.cancelled && timer.current == null) preview(gesture, latest.current);
    }, 2000);
  };

  /** Let go: what it shows is written. */
  const end = () => {
    stopKeepalive();
    const gesture = dragging.current;
    if (!gesture) return;
    dragging.current = null;
    if (timer.current != null) clearTimeout(timer.current);
    flushPreview();
    void commit(gesture, latest.current);
  };

  /** One step from where it is (taste.md §5.2), `next` of the value and its gesture's control; a run of them is written as one edit half a second after the last. */
  const step = (next: (from: number, knob: KnobInfo) => number) => {
    if (!dragging.current) start();
    const gesture = dragging.current;
    if (!gesture) return;
    change(next(latest.current, gesture.knob));
    if (idle.current != null) clearTimeout(idle.current);
    idle.current = window.setTimeout(() => {
      idle.current = null;
      end();
    }, 500);
  };

  /** Write a run of steps now, if one is open (before a value is typed). */
  const settle = () => {
    if (idle.current == null) return;
    clearTimeout(idle.current);
    idle.current = null;
    end();
  };

  /** A drag let go of without its pointer (lost, or cancelled): the control goes back to where it was. */
  const abandon = () => {
    stopKeepalive();
    const gesture = dragging.current;
    if (!gesture) return;
    dragging.current = null;
    pending.current = null;
    if (timer.current != null) clearTimeout(timer.current);
    timer.current = null;
    setValue(knob.value);
    latest.current = knob.value;
    committing.current = true;
    setSaving(true);
    void cancel(gesture).finally(() => {
      committing.current = false;
      if (mounted.current) setSaving(false);
    });
  };

  /** Show `v` with no gesture on the server yet (a first pan, while its pan is being written). */
  const show = (v: number) => {
    setValue(v);
    latest.current = v;
  };

  return {
    value,
    saving,
    /** The control the drag in progress began on (its id and range), else this one. */
    descriptor: dragging.current?.knob ?? knob,
    dragging: () => dragging.current !== null,
    change,
    start,
    end,
    step,
    settle,
    abandon,
    show,
  };
}

/**
 * Tab in a type-in (taste.md §2.14): on to the next knob or fader (or back, with ⇧), in the page's
 * order, from the one `from` is in.
 */
export function focusNext(from: Element | null, back: boolean) {
  const all = [...document.querySelectorAll<HTMLElement | SVGElement>("[data-knob] [role=slider][tabindex='0']")];
  const at = from ? all.findIndex((e) => e === from || from.contains(e)) : -1;
  requestAnimationFrame(() => all[at + (back ? -1 : 1)]?.focus());
}

/** Pixels of drag for a knob's whole range (taste.md KN-1), and what ⇧ scales a drag by. */
export const THROW = 300;
export const FINE = 0.1;
/** Wheel travel (pixels) that is one step, as a mouse wheel's notch is. */
export const NOTCH = 40;

/**
 * A knob's drag (taste.md §5.2), on whatever element holds it (its whole row): drag up and down
 * (300px is the whole range, ⇧ for a tenth of that), ⌥-click to reset (its default, else the
 * middle). Not from its input's port, nor from a field typed in. `pointer` is where the pointer is
 * while it drags, for the value bubble. `onChange` also gets how far the pointer is from where it
 * pressed, in pixels (a first pan waits for 4).
 */
export function useKnobDrag({
  value,
  reset = 0.5,
  disabled,
  onChange,
  onStart,
  onEnd,
  onCancel,
  focus,
}: {
  value: number;
  reset?: number;
  disabled: boolean;
  onChange: (v: number, moved: number) => void;
  onStart: () => void;
  onEnd: () => void;
  onCancel: () => void;
  /** Give the knob the keyboard, for ↑ ↓ after a drag. */
  focus: () => void;
}) {
  // The drag: where the pointer pressed and was last, and where that has turned the knob. Each move
  // turns it by its own distance, coarse or (⇧) fine, so pressing or letting go of ⇧ never makes it jump.
  const drag = useRef<{ pointer: number; y0: number; y: number; u: number } | null>(null);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);
  const finish = (id: number, cancelled: boolean) => {
    if (drag.current?.pointer !== id) return;
    drag.current = null;
    setPointer(null);
    if (cancelled) onCancel();
    else onEnd();
  };
  const handlers = {
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => {
      // Only on the row itself: its menu is a portal, whose events reach the row through React.
      if (disabled || drag.current || e.button !== 0 || !e.currentTarget.contains(e.target as Node)) return;
      if ((e.target as HTMLElement).closest("[data-port-dot], input, button")) return;
      // ⌥-click: back to its default, as one edit (Logic).
      if (e.altKey) {
        e.preventDefault();
        if (Math.abs(reset - value) > 1e-6) onChange(reset, 0);
        return;
      }
      e.currentTarget.setPointerCapture(e.pointerId);
      drag.current = { pointer: e.pointerId, y0: e.clientY, y: e.clientY, u: value };
      setPointer({ x: e.clientX, y: e.clientY });
      focus();
      onStart();
    },
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
      const d = drag.current;
      if (!d || d.pointer !== e.pointerId) return;
      setPointer({ x: e.clientX, y: e.clientY });
      d.u = Math.min(1, Math.max(0, d.u + ((d.y - e.clientY) / THROW) * (e.shiftKey ? FINE : 1)));
      d.y = e.clientY;
      if (Math.abs(d.u - value) > (e.shiftKey ? 0.0002 : 0.002)) onChange(d.u, Math.abs(e.clientY - d.y0));
    },
    onPointerUp: (e: React.PointerEvent<HTMLElement>) => finish(e.pointerId, false),
    onPointerCancel: (e: React.PointerEvent<HTMLElement>) => finish(e.pointerId, true),
    onLostPointerCapture: (e: React.PointerEvent<HTMLElement>) => finish(e.pointerId, true),
  };
  return { pointer, handlers };
}

/**
 * A small rotary knob, 0..1: drawn, with the keys and the wheel of taste.md §5.2 (its drag is the
 * row's, see `useKnobDrag`): Return to type a value, the wheel or ↑ ↓ (⇧ for ten) to step it, ⌫ to
 * reset. Its arc runs from the minimum, or from 12 o'clock when it is bipolar; while dragged
 * (`pointer`) it is lit and its value shows above the pointer. 28px, or `size`. In a row of strips
 * (`vertical`), only an up-and-down scroll turns it: a sideways one scrolls the strips (lead decision T6).
 */
export function MiniKnob({
  value,
  bipolar,
  disabled,
  readout,
  label,
  reset = 0.5,
  size = 28,
  vertical = false,
  svg,
  pointer,
  onChange,
  onStep,
  onType,
}: {
  value: number;
  bipolar: boolean;
  disabled: boolean;
  /** The value as it reads, for the bubble while dragging. */
  readout: string;
  label: string;
  /** Where ⌫ puts it: its declared default, else the middle. */
  reset?: number;
  size?: number;
  vertical?: boolean;
  svg?: React.RefObject<SVGSVGElement | null>;
  /** Where the pointer is while it is dragged. */
  pointer: { x: number; y: number } | null;
  onChange: (v: number) => void;
  /** `n` steps up (negative: down), from the wheel or the arrow keys. */
  onStep: (n: number) => void;
  /** Type a value in place of the readout. */
  onType: () => void;
}) {
  const own = useRef<SVGSVGElement>(null);
  const el = svg ?? own;
  const wheel = useRef(0);
  const step = useRef(onStep);
  step.current = onStep;
  const dragging = useRef(false);
  dragging.current = !!pointer;
  // The wheel steps it (one step a notch); ⌘ or ⌃ with it zooms the canvas instead. Not passive, so
  // the canvas under it doesn't scroll as well.
  useEffect(() => {
    const knob = el.current;
    if (!knob || disabled) return;
    const turn = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey || dragging.current) return;
      const across = Math.abs(e.deltaX) > Math.abs(e.deltaY);
      if (vertical && across) return;
      e.preventDefault();
      const d = across ? e.deltaX : e.deltaY;
      wheel.current -= e.deltaMode === 1 ? d * NOTCH : d;
      const n = Math.trunc(wheel.current / NOTCH);
      if (n === 0) return;
      wheel.current -= n * NOTCH;
      step.current(n);
    };
    knob.addEventListener("wheel", turn, { passive: false });
    return () => knob.removeEventListener("wheel", turn);
  }, [el, disabled, vertical]);
  const a0 = -135, a1 = 135;
  const angle = a0 + (a1 - a0) * value;
  const from = bipolar ? 0 : a0;
  // Drawn at 28px and scaled: the ring 3px in, the pointer from 3 to 8.5px out.
  const c = size / 2, r = c - 3, s = size / 28;
  const at = (a: number, radius = r) => [c + radius * Math.sin((a * Math.PI) / 180), c - radius * Math.cos((a * Math.PI) / 180)];
  const arc = (from: number, to: number) => {
    const [x0, y0] = at(from), [x1, y1] = at(to);
    return `M ${x0} ${y0} A ${r} ${r} 0 ${to - from > 180 ? 1 : 0} 1 ${x1} ${y1}`;
  };
  const [px, py] = at(angle, 3 * s), [tx, ty] = at(angle, 8.5 * s);
  return (
    <svg
      ref={el}
      width={size}
      height={size}
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={Number(value.toFixed(3))}
      aria-valuetext={readout}
      className="block rounded-full"
      onKeyDown={(e) => {
        if (e.metaKey || e.ctrlKey || e.altKey || pointer) return;
        if (e.key === "ArrowUp" || e.key === "ArrowDown") onStep((e.key === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 10 : 1));
        else if (e.key === "Enter") onType();
        else if (e.key === "Backspace" || e.key === "Delete") {
          if (Math.abs(reset - value) > 1e-6) onChange(reset);
        } else return;
        e.preventDefault();
        e.stopPropagation();
      }}
    >
      {/* The track, the value from its start, and the pointer (taste.md §2.8). */}
      <path
        d={arc(a0, a1)}
        className={disabled ? "stroke-line" : clsx("stroke-line-strong", !pointer && "group-hover/knob:stroke-fg-off")}
        strokeWidth="2"
        fill="none"
        strokeLinecap="round"
      />
      {Math.abs(angle - from) > 1.5 && (
        <path
          d={arc(Math.min(from, angle), Math.max(from, angle))}
          className={disabled ? "stroke-fg-off" : pointer ? "stroke-accent" : "stroke-fg"}
          strokeWidth="2"
          fill="none"
          strokeLinecap="round"
        />
      )}
      <line x1={px} y1={py} x2={tx} y2={ty} className={disabled ? "stroke-fg-off" : "stroke-fg-strong"} strokeWidth="1.5" strokeLinecap="round" />
      {pointer && <Bubble at={pointer}>{readout}</Bubble>}
    </svg>
  );
}

/**
 * A knob's menu (right-click on it): Automate, which turns its value into a lane to draw on, and
 * Reset to its declared default. What can't be done says why in place of what it would do:
 * `locked` is why nothing can be edited now.
 */
export function KnobMenu({
  knob,
  label,
  value,
  at,
  editable,
  locked,
  onAutomate,
  onReset,
  onClose,
}: {
  knob: KnobInfo;
  label: string;
  value: number;
  at: { x: number; y: number; above: number };
  editable: boolean;
  locked: string | null;
  onAutomate: () => void;
  onReset?: () => void;
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: at.x, top: at.y });
  const why = locked ?? "It can't be edited right now";
  const sentence = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
  const items = [
    {
      id: "automate",
      Icon: Spline,
      title: "Automate",
      detail: !editable ? why : knob.automate_refused ? `${sentence(knob.automate_refused)}.` : "Draw it over time in a new lane. It sounds the same until you do.",
      disabled: !editable || !!knob.automate_refused,
      run: onAutomate,
    },
    ...(onReset && knob.default != null
      ? [{
          id: "reset",
          Icon: RotateCcw,
          title: "Reset to default",
          detail: knob.default === value ? `It is at its default, ${display(knob, knob.default)}` : display(knob, knob.default),
          disabled: !editable || knob.default === value,
          run: () => {
            onClose();
            onReset();
          },
        }]
      : []),
  ];
  const [active, setActive] = useState(() => items.findIndex((i) => !i.disabled));

  // Opened at the knob, below it unless there's no room, kept on screen; it stays where it opened.
  useLayoutEffect(() => {
    const r = box.current!.getBoundingClientRect();
    setPos({
      left: Math.max(8, Math.min(at.x, window.innerWidth - r.width - 8)),
      top: at.y + r.height + 8 > window.innerHeight ? Math.max(8, at.above - r.height) : at.y,
    });
    box.current!.focus({ preventScroll: true });
  }, [at.x, at.y, at.above]);
  useEffect(() => {
    const outside = (e: Event) => !box.current?.contains(e.target as Node) && onClose();
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    document.addEventListener("scroll", onClose, true);
    return () => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
      document.removeEventListener("scroll", onClose, true);
    };
  }, [onClose]);

  const move = (step: number) => {
    const enabled = items.map((i, k) => (i.disabled ? -1 : k)).filter((k) => k >= 0);
    if (!enabled.length) return;
    const at = enabled.indexOf(active);
    setActive(enabled[(at + step + enabled.length) % enabled.length]);
  };

  return createPortal(
    <div
      ref={box}
      role="menu"
      tabIndex={-1}
      data-testid="knob-menu"
      aria-label={`${label} knob`}
      style={pos}
      className="fixed z-50 w-[272px] p-1 rounded-md bg-raised shadow-pop animate-rise outline-none cursor-default"
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onClose();
        else if (e.key === "ArrowDown") move(1);
        else if (e.key === "ArrowUp") move(-1);
        else if ((e.key === "Enter" || e.key === " ") && items[active] && !items[active].disabled) items[active].run();
        else return;
        e.preventDefault();
      }}
    >
      <div className="h-ctl px-2 flex items-center gap-2">
        <span className="text-sm font-semibold text-fg truncate">{label}</span>
        <span className="ml-auto shrink-0 text-xs text-fg-muted">{display(knob, value)}</span>
      </div>
      <div className="h-px my-1 bg-line" />
      {items.map((item, k) => (
        <div
          key={item.id}
          role="menuitem"
          data-menu-item={item.id}
          aria-disabled={item.disabled || undefined}
          onPointerEnter={() => !item.disabled && setActive(k)}
          onClick={() => !item.disabled && item.run()}
          className={clsx(
            "min-h-ctl px-2 py-1.5 rounded-sm flex items-start gap-2 transition-colors duration-80 ease-linear",
            item.disabled ? "text-fg-off" : clsx("cursor-pointer text-fg", k === active && "bg-accent-soft"),
          )}
        >
          <item.Icon size={14} className={clsx("mt-px shrink-0", !item.disabled && "text-fg-muted")} />
          <span className="flex-1 min-w-0">
            <span className="block text-sm">{item.title}</span>
            {/* The detail decides the choice: what it will do, or why it can't. */}
            <span data-detail className="block text-xs text-fg-muted">{item.detail}</span>
          </span>
        </div>
      ))}
      <div className="h-px my-1 bg-line" />
      <div className="h-ctl-sm px-2 flex items-center gap-2 text-xs text-fg-faint">
        <span data-code className="font-mono truncate">{knob.file}:{knob.line}</span>
        <span className="ml-auto shrink-0 truncate">{knob.written ? <span data-code className="font-mono">{knob.literal}</span> : "its default"}</span>
      </div>
    </div>,
    document.body,
  );
}

/**
 * Where a value lives, when that isn't the code (HOME-1, taste.md §2.10): a lane, a filled accent
 * circle; a plugin's state, a small square. Code, written or inherited, has none.
 */
export function HomeMarker({ home, label }: { home: Home; label: string }) {
  if (home === "lane") return <span className="shrink-0 size-[5px] rounded-full bg-accent" title={`Follows the ${label} lane.`} />;
  if (home === "state") return <span className="shrink-0 size-1 bg-fg-faint" title="Saved in the plugin's state." />;
  return null;
}

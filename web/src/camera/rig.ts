// A view's camera at work on its scroll box: the recognizer's events summed and written once a
// frame, a zoom written in the render that draws it, the browser's own moves taken in, and the
// app's moves (VIEW-9). One for each mounted view, made by `useCamera`.

import type { StoreApi } from "zustand";
import { fit, reveal, to, toContent, zoomAround, type Align, type Camera, type Edges, type Shape, type Span } from "./math";
import { Recognizer } from "./recognizer";
import { hearInput, moved, pointerHeld } from "./state";
import type { CameraState, MovedBy } from "./store";
import { Writer } from "./writer";

/** What a view tells its camera, each time it renders. */
export interface Fixed {
  /** What covers its box's edges (its sticky ruler, names, keys…), in px. */
  margins?: Partial<Edges>;
  /** Where the part the zoom stretches starts, in the content's px. */
  origin?: { x?: number; y?: number };
  /** Which ways the zoom stretches its content: across only, by default. */
  stretch?: { x: boolean; y: boolean };
  /** Its zooms: from `min` to `max`; `round` keeps one as the view keeps it. */
  zoom?: { min: number; max: number; round?: (z: number) => number };
}

/** The app's moves of a view (VIEW-9). Points are in the box, px from its top-left corner. */
export interface CameraControls {
  /** The camera now, and what it keeps of its view: its box's size, margins, origin. Reads nothing of the page. */
  now(): CameraState;
  /** Look at (x, y) (either left out: as it is), at `zoom` if given. */
  to(x?: number, y?: number, zoom?: number): void;
  /** Zoom to `zoom` around `at` (the clear view's middle if left out), what is there staying there. */
  zoomAround(zoom: number, at?: { x: number; y: number }): void;
  /** The units [from, to] across fill the view, `pad` px each side. */
  fit(span: [number, number], pad?: number): void;
  /** The least move that shows an element, or spans of the content's px, clear of the margins; `ways` it may move (both by default). */
  reveal(what: Element | { x?: Span; y?: Span }, opts?: { pad?: number; align?: Align; ways?: "x" | "y" | "xy" }): void;
  /** A point on screen as the box's point, and as the content's units. */
  boxPoint(clientX: number, clientY: number): { x: number; y: number };
  toContent(clientX: number, clientY: number): { x: number; y: number };
  /** Whether a pointer drag holds the camera still (VIEW-8). */
  held(): boolean;
  /**
   * Take in a move the browser made to the box and the camera hasn't heard yet (its `scroll` comes a
   * frame late): before the app decides from where the view is, so a move made before it listened
   * is not heard as one made after.
   */
  sync(): void;
}

/** The views at work now, by camera name. */
export const rigs = new Map<string, Rig>();

/** Who hears a view's box move: after the camera wrote it, or took in the browser's move. */
const moves = new Set<() => void>();

/**
 * Hear every move of a view's box, right after it is written (in the frame it moves) or taken in:
 * for what is placed over a view by where things in it are on screen (an agent's pointer). Returns
 * how to stop.
 */
export function onViewMoved(f: () => void): () => void {
  moves.add(f);
  return () => void moves.delete(f);
}
const moved_ = () => {
  for (const f of moves) f();
};

export class Rig {
  fixed: Fixed = {};
  private el: HTMLElement | null = null;
  private writer: Writer | null = null;
  private raf = 0;
  /** A zoom set and not yet drawn: its render writes the position. */
  private zoomed = false;
  private offs: (() => void)[] = [];
  /** Who sees whether elements are near the view: one IntersectionObserver for the box (VIEW-12). */
  private near: { io: IntersectionObserver; seen: Map<Element, (near: boolean) => void> } | null = null;
  readonly hands: Recognizer;
  readonly controls: CameraControls;

  constructor(
    readonly name: string,
    readonly cam: StoreApi<CameraState>,
  ) {
    this.hands = new Recognizer(
      () => this.el,
      () => this.cam.getState().zoom,
    );
    this.controls = {
      now: () => ({ ...this.cam.getState() }),
      to: (x, y, zoom) =>
        this.app((c, s) => {
          if (zoom === undefined || zoom === c.zoom) return to(c, x ?? c.x, y ?? c.y, s);
          // A new zoom: the render that draws it writes the position, where the box then has room.
          return { x: Math.max(0, x ?? c.x), y: Math.max(0, y ?? c.y), zoom: this.clampZoom(zoom) };
        }),
      zoomAround: (zoom, at) => this.zoomTo(zoom, at ?? this.middle(), "app"),
      fit: (span, pad = 0) => {
        const s = this.shape();
        this.set({ ...fit(this.camera(), span, s, { pad, zoomOf: (z) => this.clampZoom(z) }) }, "app");
      },
      reveal: (what, opts) => {
        const box = this.el;
        if (!box) return;
        let spans: { x?: Span; y?: Span };
        if (what instanceof Element) {
          const r = what.getBoundingClientRect(), b = this.boxRect();
          const x = r.left - b.left + box.scrollLeft, y = r.top - b.top + box.scrollTop;
          spans = { x: [x, x + r.width], y: [y, y + r.height] };
        } else spans = what;
        const ways = opts?.ways ?? "xy";
        this.app((c, s) => reveal(c, { x: ways.includes("x") ? spans.x : undefined, y: ways.includes("y") ? spans.y : undefined }, s, opts));
      },
      boxPoint: (cx, cy) => {
        const b = this.boxRect();
        return { x: cx - b.left, y: cy - b.top };
      },
      toContent: (cx, cy) => toContent(this.camera(), this.controls.boxPoint(cx, cy), this.shape()),
      held: pointerHeld,
      sync: () => this.takeIn(),
    };
  }

  // ---------------------------------------------------------------- the box

  attach(el: HTMLElement) {
    this.el = el;
    this.writer = new Writer(el);
    rigs.set(this.name, this);
    const wheel = (e: WheelEvent) => this.hands.wheel(e) && this.schedule();
    const gesture = (e: Event) => this.hands.gesture(e) && this.schedule();
    const scroll = () => this.takeIn();
    el.addEventListener("wheel", wheel, { capture: true, passive: false });
    for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.addEventListener(t, gesture, { capture: true, passive: false });
    el.addEventListener("scroll", scroll, { passive: true });
    const size = new ResizeObserver(() => this.measure(true));
    size.observe(el);
    this.offs = [
      () => el.removeEventListener("wheel", wheel, { capture: true }),
      () => {
        for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.removeEventListener(t, gesture, { capture: true });
      },
      () => el.removeEventListener("scroll", scroll),
      () => size.disconnect(),
      hearInput(() => this.hands.stop()),
    ];
    this.measure(true);
    // Where the camera looks, written at once: a view that comes back shows what it showed.
    this.write();
  }

  detach() {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.offs.forEach((off) => off());
    this.offs = [];
    this.near?.io.disconnect();
    this.near = null;
    if (rigs.get(this.name) === this) rigs.delete(this.name);
    this.el = null;
    this.writer = null;
  }

  /** Whether `el` is in this view's box. */
  holds(el: Element) {
    return !!this.el && this.el !== el && this.el.contains(el);
  }

  private boxRect() {
    const box = this.el!;
    const r = box.getBoundingClientRect();
    return { left: r.left + box.clientLeft, top: r.top + box.clientTop };
  }

  /** What the view said of its margins and origin, and (`sized`) the box's size, into the camera. */
  measure(sized = false) {
    const box = this.el;
    if (!box) return;
    const f = this.fixed;
    const s = this.cam.getState();
    const view = sized ? { w: box.clientWidth, h: box.clientHeight } : s.view;
    const margins = { left: f.margins?.left ?? 0, top: f.margins?.top ?? 0, right: f.margins?.right ?? 0, bottom: f.margins?.bottom ?? 0 };
    const origin = { x: f.origin?.x ?? 0, y: f.origin?.y ?? 0 };
    const stretch = f.stretch ?? { x: true, y: false };
    const same =
      s.view.w === view.w &&
      s.view.h === view.h &&
      (Object.keys(margins) as (keyof Edges)[]).every((k) => s.margins[k] === margins[k]) &&
      s.origin.x === origin.x &&
      s.origin.y === origin.y &&
      s.stretch.x === stretch.x &&
      s.stretch.y === stretch.y;
    if (!same) this.cam.setState({ view, margins, origin, stretch });
  }

  private camera(): Camera {
    const { x, y, zoom } = this.cam.getState();
    return { x, y, zoom };
  }

  shape(): Shape {
    const box = this.el;
    const s = this.cam.getState();
    return {
      view: box ? { w: box.clientWidth, h: box.clientHeight } : s.view,
      content: box ? { w: box.scrollWidth, h: box.scrollHeight } : s.view,
      margins: s.margins,
      origin: s.origin,
      stretch: s.stretch,
    };
  }

  private middle() {
    const s = this.cam.getState();
    return { x: (s.margins.left + s.view.w - s.margins.right) / 2, y: (s.margins.top + s.view.h - s.margins.bottom) / 2 };
  }

  private clampZoom(z: number) {
    const lim = this.fixed.zoom;
    if (!lim) return z;
    const c = Math.min(lim.max, Math.max(lim.min, Number.isFinite(z) ? z : lim.min));
    return lim.round ? lim.round(c) : c;
  }

  // ---------------------------------------------------------------- moving

  /** Set the camera, as `by` moved it. A new zoom is written by the render that draws it; else at once. */
  private set(c: Camera, by: MovedBy) {
    const s = this.cam.getState();
    if (c.x === s.x && c.y === s.y && c.zoom === s.zoom) return;
    const zoom = c.zoom !== s.zoom;
    if (zoom) this.zoomed = true;
    // A zoom asks for room to look where it put the camera: the view sizes its content by it.
    const asked = zoom ? { w: c.x + s.view.w, h: c.y + s.view.h } : s.asked;
    this.cam.setState({ x: c.x, y: c.y, zoom: c.zoom, by, asked });
    if (!this.zoomed) this.write();
    else this.schedule();
  }

  /** The app's move (VIEW-9): the browser's move taken in first, then set and written; a swipe still arriving is dropped. */
  private app(f: (c: Camera, s: Shape) => Camera) {
    if (!this.el) return;
    this.takeIn();
    this.hands.appMoved();
    this.set(f(this.camera(), this.shape()), "app");
  }

  private zoomTo(zoom: number, at: { x: number; y: number }, by: MovedBy) {
    if (!this.el) return;
    if (by === "app") {
      this.takeIn();
      this.hands.appMoved();
    }
    const z = this.clampZoom(zoom);
    const s = this.cam.getState();
    if (z === s.zoom) return;
    this.set(zoomAround(this.camera(), at, z, s), by);
  }

  /** A move the browser made to the box itself, taken in (VIEW-9). Not while a zoom waits for its render: that one clamped the box. */
  private takeIn() {
    if (this.zoomed || !this.writer) return;
    const theirs = this.writer.theirs();
    if (!theirs) return;
    this.hands.stopGlide();
    this.cam.setState({ x: theirs.x, y: theirs.y, by: "browser" });
    moved_();
  }

  /** Write the camera to the box, and keep it where the box stopped it. */
  private write() {
    if (!this.writer) return;
    const c = this.camera();
    const was = this.writer.written;
    const w = this.writer.write(c);
    if (this.writer.written.x !== was.x || this.writer.written.y !== was.y) moved_();
    if (w.x !== c.x || w.y !== c.y) {
      // At an edge (VIEW-6): a glide stops there.
      this.hands.stopGlide();
      this.cam.setState({ x: w.x, y: w.y });
    }
  }

  /** After each render of the view: a zoom it drew is written now, in the frame it lands in (VIEW-8). */
  afterRender() {
    this.measure();
    if (!this.zoomed) return;
    this.zoomed = false;
    this.write();
  }

  private schedule() {
    if (!this.raf && this.el) this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  /** Once a frame: the events since the last, summed and written once (VIEW-3). */
  private frame(t: number) {
    this.raf = 0;
    if (!this.el?.isConnected) return;
    // A zoom no render drew (its view doesn't draw by it): written now.
    if (this.zoomed) {
      this.zoomed = false;
      this.write();
    }
    this.takeIn();
    const asked = this.hands.frame(t);
    if (asked.dx || asked.dy || asked.zoom) moved();
    if (asked.zoom) {
      const s = this.cam.getState();
      const z = this.clampZoom(asked.zoom.to ?? s.zoom * asked.zoom.by);
      const c = z === s.zoom ? this.camera() : zoomAround(this.camera(), asked.zoom.at, z, s);
      this.set({ ...c, x: c.x + asked.dx, y: c.y + asked.dy }, z === s.zoom ? "hand" : "zoom");
    } else if (asked.dx || asked.dy) {
      const c = this.camera();
      const s = this.shape();
      this.set(to(c, c.x + asked.dx, c.y + asked.dy, s), "hand");
    }
    if (this.hands.busy()) this.schedule();
  }

  // ---------------------------------------------------------------- near the view

  /** Hear whether `el` is in view, or within a margin of it (`margin` of the box's size each way). */
  watchNear(el: Element, margin: number, f: (near: boolean) => void): () => void {
    const box = this.el;
    if (!box) return () => {};
    if (!this.near) {
      const seen = new Map<Element, (near: boolean) => void>();
      const io = new IntersectionObserver((entries) => entries.forEach((e) => seen.get(e.target)?.(e.isIntersecting)), { root: box, rootMargin: `${margin * 100}%` });
      this.near = { io, seen };
    }
    const near = this.near;
    near.seen.set(el, f);
    near.io.observe(el);
    return () => {
      near.seen.delete(el);
      near.io.unobserve(el);
    };
  }
}

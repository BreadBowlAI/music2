// A view's camera at work on its scroll box: the recognizer's events summed and written once a
// frame, a zoom written in the render that draws it, a zoom by the hand shown by the view's picture
// (a drawn zoom) or by a stretch (a live zoom) until it ends (VIEW-5), the browser's own moves taken
// in, and the app's moves (VIEW-9). One for each mounted view, made by `useCamera`.

import { cull, fit, holds, reveal, to, toContent, zoomAround, type Align, type Camera, type Edges, type Shape, type Span } from "./math";
import { FRAME_MOST, LAYERS_KEPT, SWIPE_GAP } from "./numbers";
import { Recognizer } from "./recognizer";
import { drawsZoom, hidePicture, pictureShown, showPicture } from "./picture";
import { hearInput, moved, movingNow, pointerHeld, takePress } from "./state";
import { drawnOf, type CameraStore, type MovedBy, type CameraState } from "./store";
import { markedOf, onMarked, shownScroll, stretchOf, unstretch } from "./stretch";
import { Writer } from "./writer";

/** What a view tells its camera, each time it renders. */
export interface Fixed {
  /** What covers its box's edges (its sticky ruler, names, keys…), in px. */
  margins?: Partial<Edges>;
  /** Where the part the zoom stretches starts, in the content's px. */
  origin?: { x?: number; y?: number };
  /** Which ways the zoom stretches its content: across only, by default. */
  stretch?: { x: boolean; y: boolean };
  /** Whether a nearly straight swipe keeps to its rail (VIEW-3): yes, but a canvas pans freely. */
  rails?: boolean;
  /** Its zooms: from `min` to `max`; `round` keeps one as the view keeps it. */
  zoom?: { min: number; max: number; round?: (z: number) => number };
}

/** The app's moves of a view (VIEW-9). Points are in the box, px from its top-left corner. */
export interface CameraControls {
  /** The camera's name, and its store: for the view's own hooks (`useZoomOf(camera)`) and `<CameraScope camera>`. */
  readonly name: string;
  readonly store: CameraStore;
  /** The camera now, and what it keeps of its view: its box's size, margins, origin. Reads nothing of the page. */
  now(): CameraState;
  /**
   * Look at (x, y) (either left out: as it is), at `zoom` if given. A swipe still arriving is
   * dropped, as after a click; `{ swipe: "keep" }` stops only the camera's own glide (Follow's page).
   */
  to(x?: number, y?: number, zoom?: number, opts?: { swipe?: "drop" | "keep" }): void;
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
  /** Whether the hand moves the camera now: a swipe's events less than G apart, or its glide. */
  swiping(): boolean;
  /**
   * The content's size in px, as the view renders it now (either way left out: not known): one that
   * gets smaller has the camera check, in that render's commit, whether the box's edge came in,
   * before the frame is drawn (VIEW-9). Without it, a ResizeObserver tells, a frame later.
   */
  content(size: { w?: number; h?: number }): void;
  /**
   * Take in a move the browser made to the box and the camera hasn't heard yet (its `scroll` comes a
   * frame late): before the app decides from where the view is, so a move made before it listened
   * is not heard as one made after.
   */
  sync(): void;
}

/** The views at work now, by camera name. */
export const rigs = new Map<string, Rig>();
/** And by camera. */
const byStore = new WeakMap<CameraStore, Rig>();
/** The view at work on the camera `cam` now, if one is. */
export const rigOf = (cam: CameraStore) => byStore.get(cam) ?? null;

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

/**
 * A zoom by the hand under way (VIEW-5): the view stays laid out where it was, and its picture shows
 * the camera (`drawn`: a drawn zoom), or what it marks is stretched to (a live zoom).
 */
interface Live {
  /** The view draws it: its picture shows, and nothing is stretched. */
  drawn: boolean;
  /** The content's origin on screen as it started: where each marked element's stretch is centred. */
  origin: { x: number; y: number };
  /** Each marked element stretched, and its transform's origin: the content's, in its own px. */
  els: Map<HTMLElement, string>;
  /** The content laid out, in its units each way: what shows past it isn't drawn. */
  extent: { x: [number, number]; y: [number, number] };
}

export class Rig {
  fixed: Fixed = {};
  private el: HTMLElement | null = null;
  private writer: Writer | null = null;
  private raf = 0;
  /** A zoom set and not yet drawn: its render writes the position. */
  private zoomed = false;
  private offs: (() => void)[] = [];
  /** Who sees whether elements are near the view, and in it: two IntersectionObservers for the box (VIEW-12). */
  private near: { io: IntersectionObserver; inView: IntersectionObserver; seen: Map<Element, (near: boolean, inView: boolean) => void> } | null = null;
  /** When the hand last moved the camera (`performance.now()`). */
  private handAt = -Infinity;
  /** The content's size the view says it renders now, and as its last commit drew it. */
  private contentNow: { w?: number; h?: number } | null = null;
  private contentDrawn: { w?: number; h?: number } | null = null;
  /** A live zoom under way (VIEW-5), or null. */
  private live: Live | null = null;
  /** A live zoom laid out, its stretch to take off in the render that draws the new zoom. */
  private gone: Live | null = null;
  /** The hand's zoom ended (a pinch's `gestureend`, or G with no ⌘-scroll): laid out at the next frame. */
  private ending = false;
  /**
   * A run of ⌘-scroll under way (VIEW-5): when its last event came or the last frame since it was
   * (`at`), and the quiet since its last event, counted frame by frame (`quiet`, ms).
   */
  private wheelZoom: { at: number; quiet: number } | null = null;
  /** The view's picture couldn't paint this zoom (it threw): laid out each frame instead, until the zoom ends (VIEW-5). */
  private undrawn = false;
  /** Elements a live zoom stretched that keep their layer a while after it (`LAYERS_KEPT`). */
  private layers = new Set<HTMLElement>();
  private layersTimer = 0;
  readonly hands: Recognizer;
  readonly controls: CameraControls;

  constructor(
    readonly name: string,
    readonly cam: CameraStore,
  ) {
    this.hands = new Recognizer(
      () => this.el,
      () => this.cam.getState().zoom,
      () => this.fixed.rails !== false,
    );
    this.controls = {
      name,
      store: cam,
      now: () => ({ ...this.cam.getState() }),
      to: (x, y, zoom, opts) =>
        this.app((c, s) => {
          if (zoom === undefined || zoom === c.zoom) return to(c, x ?? c.x, y ?? c.y, s);
          // A new zoom: the render that draws it writes the position, where the box then has room.
          return { x: Math.max(0, x ?? c.x), y: Math.max(0, y ?? c.y), zoom: this.clampZoom(zoom) };
        }, opts?.swipe !== "keep"),
      zoomAround: (zoom, at) => this.zoomTo(zoom, at ?? this.middle(), "app"),
      fit: (span, pad = 0) => {
        // Laid out at once, a live zoom first (VIEW-5).
        this.layOut();
        const s = this.shape();
        this.set({ ...fit(this.camera(), span, s, { pad, zoomOf: (z) => this.clampZoom(z) }) }, "app");
      },
      reveal: (what, opts) => {
        const box = this.el;
        if (!box) return;
        let spans: { x?: Span; y?: Span };
        if (what instanceof Element) {
          // Its place as it shows (stretched, in a live zoom), in the content's px of the zoom shown:
          // the zoom the move is made from, once `app` has laid it out.
          const r = what.getBoundingClientRect(), b = this.boxRect();
          const at = shownScroll(this.cam.getState(), { x: box.scrollLeft, y: box.scrollTop });
          const x = r.left - b.left + at.x, y = r.top - b.top + at.y;
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
      swiping: () => this.hands.busy() || performance.now() - this.handAt < SWIPE_GAP,
      content: (size) => void (this.contentNow = size),
      sync: () => this.takeIn(),
    };
  }

  // ---------------------------------------------------------------- the box

  /** On the box `el` the view rendered now: put on it, moved to another one, or (null) taken off. */
  hold(el: HTMLElement | null) {
    if (el === this.el) return;
    if (this.el) this.detach();
    if (el) this.attach(el);
  }

  attach(el: HTMLElement) {
    this.el = el;
    this.writer = new Writer(el);
    rigs.set(this.name, this);
    byStore.set(this.cam, this);
    const wheel = (e: WheelEvent) => {
      const asked = this.hands.wheel(e);
      // A run of ⌘-scroll ends with no ⌘-scroll for G, counted in the frames after its last event (VIEW-5).
      // Its own time, never later than it is heard (a suite's events may say a time to come).
      if (asked === "zoom") this.wheelZoom = { at: Math.min(e.timeStamp, performance.now()), quiet: 0 };
      if (asked) this.schedule();
    };
    const gesture = (e: Event) => {
      if (e.type === "gestureend") {
        this.ending = true;
        this.schedule();
      }
      if (this.hands.gesture(e)) this.schedule();
    };
    const scroll = () => this.takeIn();
    el.addEventListener("wheel", wheel, { capture: true, passive: false });
    for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.addEventListener(t, gesture, { capture: true, passive: false });
    el.addEventListener("scroll", scroll, { passive: true });
    const size = new ResizeObserver(() => {
      // The box resized under a stretch: laid out first, where it is now.
      this.layOut();
      this.measure(true);
      this.clamped();
    });
    size.observe(el);
    // The content's size (each box in the view's box): one that shrinks may stop the box at a new edge.
    const content = new ResizeObserver(() => this.clamped());
    for (const child of el.children) content.observe(child);
    const children = new MutationObserver((records) => {
      for (const r of records) for (const n of r.addedNodes) if (n instanceof Element) content.observe(n);
    });
    children.observe(el, { childList: true });
    this.offs = [
      () => el.removeEventListener("wheel", wheel, { capture: true }),
      () => {
        for (const t of ["gesturestart", "gesturechange", "gestureend"]) el.removeEventListener(t, gesture, { capture: true });
      },
      () => el.removeEventListener("scroll", scroll),
      () => size.disconnect(),
      () => content.disconnect(),
      () => children.disconnect(),
      // A press or a key stops the hand's moves, lays a zoom by the hand out, and lets its layers go
      // now: in this same task as the render that lays it out, so no frame shows the one without the
      // other. A press on the view while its picture shows does nothing else (VIEW-5).
      hearInput((e) => {
        const took = e instanceof PointerEvent && pictureShown(this.cam) && e.target instanceof Node && el.contains(e.target);
        this.hands.stop();
        this.layOut();
        this.dropStretch();
        this.dropLayers();
        if (took) takePress(e);
      }),
      onMarked(this.cam, (el) => void (this.live && !this.live.drawn && this.join(this.live, el))),
    ];
    this.measure(true);
    // Where the camera looks, written at once: a view that comes back shows what it showed.
    this.write();
  }

  detach() {
    cancelAnimationFrame(this.raf);
    this.raf = 0;
    this.wheelZoom = null;
    for (const l of [this.live, this.gone]) if (l) for (const el of l.els.keys()) unstretch(el);
    hidePicture(this.cam);
    if (this.live) this.cam.setState({ laid: null });
    this.live = this.gone = null;
    this.ending = false;
    this.dropLayers();
    this.offs.forEach((off) => off());
    this.offs = [];
    this.near?.io.disconnect();
    this.near?.inView.disconnect();
    this.near = null;
    if (rigs.get(this.name) === this) rigs.delete(this.name);
    if (byStore.get(this.cam) === this) byStore.delete(this.cam);
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

  /** The camera shown. */
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

  /** The shape with room without end: for a move just after a live zoom was laid out, which the writer stops at the edges once that is drawn. */
  private unbounded(): Shape {
    const s = this.cam.getState();
    return { view: s.view, content: { w: Infinity, h: Infinity }, margins: s.margins, origin: s.origin, stretch: s.stretch };
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

  /**
   * The app's move (VIEW-9): the browser's move taken in first, then set and written; a swipe still
   * arriving is dropped (`drop`), else only the camera's own glide stops.
   */
  private app(f: (c: Camera, s: Shape) => Camera, drop = true) {
    if (!this.el) return;
    // A live zoom is laid out first (VIEW-5): the move is from what it showed.
    const laid = this.layOut();
    this.takeIn();
    if (drop) this.hands.appMoved();
    else this.hands.stopGlide();
    this.set(f(this.camera(), laid ? this.unbounded() : this.shape()), "app");
  }

  private zoomTo(zoom: number, at: { x: number; y: number }, by: MovedBy) {
    if (!this.el) return;
    if (by === "app") {
      // Laid out at once, a live zoom first (VIEW-5).
      this.layOut();
      this.takeIn();
      this.hands.appMoved();
    }
    const z = this.clampZoom(zoom);
    const s = this.cam.getState();
    if (z === s.zoom) return;
    this.set(zoomAround(this.camera(), at, z, s), by);
  }

  /**
   * A move the browser made to the box itself, taken in (VIEW-9). Not while a zoom waits for its
   * render: that one clamped the box. One to the box's new edge, the camera beyond it, is the
   * content getting smaller (`clamp`, no one's move): its `scroll` may come before the
   * ResizeObserver that would say so, when something read the page's layout before the frame.
   */
  private takeIn() {
    if (this.zoomed || !this.writer || !this.el) return;
    const theirs = this.writer.theirs();
    if (!theirs) return;
    const live = this.cam.getState();
    if (this.live && live.laid) {
      // The browser moved the box under a stretch (a scrollbar's drag, a key's scroll): it moved what
      // the stretch showed by as much. Laid out there, as it would be with no live zoom (VIEW-9).
      this.cam.setState({ x: live.x + theirs.x - live.laid.x, y: live.y + theirs.y - live.laid.y, by: "browser" });
      this.layOut();
      return;
    }
    const box = this.el;
    const s = this.cam.getState();
    const edge = (at: number, was: number, room: number) => was - room > 1 && Math.abs(at - room) <= 1;
    const clamp =
      edge(theirs.x, s.x, Math.max(0, box.scrollWidth - box.clientWidth)) || edge(theirs.y, s.y, Math.max(0, box.scrollHeight - box.clientHeight));
    this.hands.stopGlide();
    this.cam.setState({ x: theirs.x, y: theirs.y, by: clamp ? "clamp" : "browser" });
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

  /** After each render of the view: a zoom it drew is written now, in the frame it lands in (VIEW-8). Reads nothing of the page. */
  afterRender() {
    this.measure();
    const now = this.contentNow, was = this.contentDrawn;
    this.contentDrawn = now;
    if (this.zoomed) {
      // The stretch off first: the writer's read of the box lays the page out once, as it shows.
      this.zoomed = false;
      this.dropStretch();
      this.write();
    } else if (now && was && ((now.w ?? Infinity) < (was.w ?? Infinity) || (now.h ?? Infinity) < (was.h ?? Infinity))) this.clamped();
  }

  /**
   * The content got smaller than where the camera looks (Fold, a shorter song), or the box bigger:
   * the browser stopped the box at its new edge, and may send no `scroll` for it. The camera stops
   * there too, moved by `clamp`: no one's move. Heard from ResizeObservers, after layout.
   */
  private clamped() {
    const box = this.el;
    if (!box || !this.writer || this.zoomed || this.live) return;
    const s = this.cam.getState();
    const x = Math.min(s.x, Math.max(0, box.scrollWidth - box.clientWidth));
    const y = Math.min(s.y, Math.max(0, box.scrollHeight - box.clientHeight));
    if (s.x - x <= 1 && s.y - y <= 1) return;
    this.writer.settle();
    this.cam.setState({ x, y, by: "clamp" });
    moved_();
  }

  private schedule() {
    if (!this.raf && this.el) this.raf = requestAnimationFrame((t) => this.frame(t));
  }

  /** Once a frame: the events since the last, summed and written once (VIEW-3). */
  private frame(t: number) {
    this.raf = 0;
    if (!this.el?.isConnected) return;
    // A zoom no render drew (its view doesn't draw by it): written now, its stretch gone.
    if (this.zoomed) {
      this.zoomed = false;
      this.dropStretch();
      this.write();
    }
    this.takeIn();
    const asked = this.hands.frame(t);
    if (this.quietFor(t, !!asked.zoom)) this.ending = true;
    const ending = this.ending;
    this.ending = false;
    if (ending) this.undrawn = false;
    if (asked.dx || asked.dy || asked.zoom) {
      moved();
      this.handAt = performance.now();
    }
    if (asked.zoom) {
      const z = this.clampZoom(asked.zoom.to ?? this.cam.getState().zoom * asked.zoom.by);
      // A zoom by the hand alone, still under way: shown by a stretch (VIEW-5). Else laid out now.
      if (!asked.dx && !asked.dy && !ending && this.stretch(z, asked.zoom.at)) {
        // Shown.
      } else {
        this.layOut();
        const s = this.cam.getState();
        const c = z === s.zoom ? this.camera() : zoomAround(this.camera(), asked.zoom.at, z, s);
        this.set({ ...c, x: c.x + asked.dx, y: c.y + asked.dy }, z === s.zoom ? "hand" : "zoom");
      }
    } else if (asked.dx || asked.dy) {
      // A move after a live zoom: laid out first, from what it showed.
      const laid = this.layOut();
      const c = this.camera();
      const want = { x: c.x + asked.dx, y: c.y + asked.dy };
      const got = to(c, want.x, want.y, laid ? this.unbounded() : this.shape());
      // At an edge (VIEW-6): the glide stops that way.
      if (got.x !== want.x) this.hands.stopGlide("x");
      if (got.y !== want.y) this.hands.stopGlide("y");
      this.set(got, "hand");
    }
    if (ending) this.layOut();
    if (this.hands.busy() || this.wheelZoom) this.schedule();
  }

  /**
   * Whether a run of ⌘-scroll has ended by the frame at `t`: no ⌘-scroll for G, counted frame by
   * frame since its last event (`zoomed`: one came for this frame), each frame at most `FRAME_MOST`.
   * Never by a timer: a frame that ran long (a busy page, a big song's layout) may come before the
   * events sent meanwhile are heard, so it never ends a zoom alone (VIEW-5).
   */
  private quietFor(t: number, zoomed: boolean): boolean {
    const w = this.wheelZoom;
    if (!w || zoomed) return false;
    w.quiet += Math.min(Math.max(0, t - w.at), FRAME_MOST);
    w.at = Math.max(w.at, t);
    if (w.quiet < SWIPE_GAP) return false;
    this.wheelZoom = null;
    return true;
  }

  // ---------------------------------------------------------------- the live zoom (VIEW-5)

  /**
   * Show a zoom by the hand to `zoom`, around the box's point `at`: by the view's picture (a drawn
   * zoom), or by stretching what the view marks (a live zoom). The view stays laid out as it is, and
   * renders nothing (VIEW-8). A live zoom is laid out at it instead when that would show past what
   * the view has drawn. Returns false where the view neither draws its zoom nor marks anything: then
   * it is laid out on each frame, as before.
   */
  private stretch(zoom: number, at: { x: number; y: number }): boolean {
    if (!this.live && !this.drawsNow() && !markedOf(this.cam).size) return false;
    if (zoom === this.cam.getState().zoom) return true;
    const live = this.live ?? this.begin();
    if (!live) return false;
    // From where the box shows the view: `begin` took the camera to it.
    const s = this.cam.getState();
    const c = zoomAround(this.camera(), at, zoom, s);
    if (!this.covered(c, live)) {
      this.layOut();
      this.set(c, "zoom");
      return true;
    }
    this.cam.setState({ x: c.x, y: c.y, zoom: c.zoom, by: "zoom" });
    if (!this.apply(live)) {
      // The picture couldn't paint: laid out at the zoom shown, now and each frame till the zoom
      // ends, so the view never freezes (VIEW-5).
      this.undrawn = true;
      this.layOut();
      return true;
    }
    moved_();
    return true;
  }

  /** Whether the view draws this zoom by the hand: it says it does, and its picture hasn't failed in it. */
  private drawsNow() {
    return drawsZoom(this.cam) && !this.undrawn;
  }

  /**
   * A zoom by the hand begins: the camera laid out is where the box is scrolled now and the zoom it
   * draws; the view's picture shows (a drawn zoom), or each marked element takes its stretch's origin
   * (a live zoom). Read at a frame's start, where the page's
   * layout is still the last frame's: it costs nothing. The camera shown starts there too, the
   * fractions of a px it kept let go of: so the zoom keeps what the box shows under the fingers, not
   * a point up to half a px off it, which the zoom would carry ×(its stretch) off. That place is the
   * writer's (whole CSS px, as WebKit keeps a scroll position), not a box's on the screen's pixels
   * (`spanBox`, `onPixels`): the zoom's end scrolls the box there again, by the writer.
   */
  private begin(): Live | null {
    const els = markedOf(this.cam);
    const box = this.el, writer = this.writer;
    const drawn = this.drawsNow();
    if ((!drawn && !els.size) || !box || !writer) return null;
    // Everything read before anything is written: one layout, the last frame's.
    const b = this.boxRect();
    const content = { w: box.scrollWidth, h: box.scrollHeight };
    const places = drawn ? [] : [...els].map((el) => [el, el.getBoundingClientRect()] as const);
    const s = this.cam.getState();
    const at = writer.written;
    const per = (way: "x" | "y") => (s.stretch[way] ? s.zoom : 1);
    const live: Live = {
      drawn,
      origin: { x: b.left - at.x + s.origin.x, y: b.top - at.y + s.origin.y },
      els: new Map(),
      extent: {
        x: [-s.origin.x / per("x"), (content.w - s.origin.x) / per("x")],
        y: [-s.origin.y / per("y"), (content.h - s.origin.y) / per("y")],
      },
    };
    this.live = live;
    this.cam.setState({ x: at.x, y: at.y, by: "zoom", laid: { x: at.x, y: at.y, zoom: s.zoom } });
    for (const [el, r] of places) this.join(live, el, r);
    return live;
  }

  /**
   * A marked element joins the stretch: its origin is the content's, in its own px (`r`: where it is,
   * read now if not given), on a layer of its own.
   */
  private join(live: Live, el: HTMLElement, r: DOMRect = el.getBoundingClientRect()) {
    if (live.els.has(el)) return;
    const origin = `${live.origin.x - r.left}px ${live.origin.y - r.top}px`;
    live.els.set(el, origin);
    el.style.transformOrigin = origin;
    el.style.willChange = "transform";
    if (this.cam.getState().laid) this.apply(live, [el]);
  }

  /**
   * The camera shown: painted by the view's picture, or one transform on each stretched element, from
   * the one laid out. False if the picture couldn't paint.
   */
  private apply(live: Live, els: Iterable<HTMLElement> = live.els.keys()): boolean {
    if (live.drawn) return showPicture(this.cam);
    const st = stretchOf(this.cam.getState());
    if (!st) return true;
    const tf = `translate(${st.shift.x}px, ${st.shift.y}px) scale(${st.scale.x}, ${st.scale.y})`;
    const still = markedOf(this.cam);
    for (const el of els) {
      // One the view let go of while it lasted: no longer its.
      if (!still.has(el)) live.els.delete(el);
      else el.style.transform = tf;
    }
    return true;
  }

  /**
   * Whether the camera `c` may still be shown by stretching what the view has drawn: it shows nothing
   * past the content as laid out, nor past any culling view's window on screen (VIEW-12); and each
   * such view's window asked for last still holds what it shows and the margin a window keeps. A zoom
   * out past that is laid out early: while what shows is still in the window on screen, so the view
   * draws its new window between frames, never all in one.
   */
  private covered(c: Camera, live: Live): boolean {
    // A drawn zoom shows what the view would draw at it, past what it has drawn too: never laid out early.
    if (live.drawn) return true;
    const s = this.cam.getState();
    const shows = cull(c, s, { x: { margin: 0 }, y: { margin: 0 } });
    const inside = (w: { from: number; to: number }, [from, to]: [number, number]) => w.from >= from && w.to <= to;
    if (!inside(shows.x, live.extent.x) || !inside(shows.y, live.extent.y)) return false;
    for (const d of drawnOf(this.cam)) {
      const w = d.on.current;
      const kept = holds(c, s, d.by, d.asked.current);
      if (d.ways.includes("x") && (!inside(shows.x, [w.x.from, w.x.to]) || !kept.x)) return false;
      if (d.ways.includes("y") && (!inside(shows.y, [w.y.from, w.y.to]) || !kept.y)) return false;
    }
    return true;
  }

  /**
   * A live zoom laid out (VIEW-5): the view is laid out at the camera shown, and the stretch goes in
   * the render that draws it (its layout effect), so no frame shows both. Returns whether one was.
   */
  private layOut(): boolean {
    const live = this.live;
    if (!live) return false;
    this.live = null;
    const s = this.cam.getState();
    if (s.zoom === s.laid?.zoom) {
      // The zoom drawn already: the box moves to what shows, and the stretch or the picture goes, now.
      this.cam.setState({ laid: null });
      this.release(live);
      this.write();
      return true;
    }
    // A zoom asks for room to look where it put the camera, as `set` does.
    this.cam.setState({ laid: null, asked: { w: s.x + s.view.w, h: s.y + s.view.h } });
    this.zoomed = true;
    this.dropStretch();
    this.gone = live;
    this.schedule();
    return true;
  }

  /** The stretch or the picture of a zoom laid out, taken off now that its render has drawn it. */
  private dropStretch() {
    const gone = this.gone;
    this.gone = null;
    if (gone) this.release(gone);
  }

  /** A zoom by the hand laid out: its picture hidden, or its stretch taken off. */
  private release(l: Live) {
    if (l.drawn) hidePicture(this.cam);
    else this.unstretchAll(l);
  }

  /** A live zoom's stretch off its elements, each keeping its layer `LAYERS_KEPT` ms more. */
  private unstretchAll(l: Live) {
    for (const el of l.els.keys()) {
      if (this.live?.els.has(el)) continue;
      unstretch(el, true);
      this.layers.add(el);
    }
    this.keepLayers();
  }

  /** The layers kept until the cameras have been still `LAYERS_KEPT` ms: letting them go lays them out again. */
  private keepLayers() {
    clearTimeout(this.layersTimer);
    this.layersTimer = window.setTimeout(() => (movingNow() ? this.keepLayers() : this.dropLayers()), LAYERS_KEPT);
  }

  /** The layers kept after a live zoom let go of, but those a live zoom stretches now. */
  private dropLayers() {
    clearTimeout(this.layersTimer);
    for (const el of this.layers) if (!this.live?.els.has(el)) unstretch(el);
    this.layers.clear();
  }

  // ---------------------------------------------------------------- near the view

  /**
   * Hear whether `el` is within a margin of the view (`margin` of the box's size each way): `near`;
   * and when it comes into the view itself: `inView` true.
   */
  watchNear(el: Element, margin: number, f: (near: boolean, inView: boolean) => void): () => void {
    const box = this.el;
    if (!box) return () => {};
    if (!this.near) {
      const seen = new Map<Element, (near: boolean, inView: boolean) => void>();
      const io = new IntersectionObserver((entries) => entries.forEach((e) => seen.get(e.target)?.(e.isIntersecting, false)), { root: box, rootMargin: `${margin * 100}%` });
      const inView = new IntersectionObserver((entries) => entries.forEach((e) => e.isIntersecting && seen.get(e.target)?.(true, true)), { root: box });
      this.near = { io, inView, seen };
    }
    const near = this.near;
    near.seen.set(el, f);
    near.io.observe(el);
    near.inView.observe(el);
    return () => {
      near.seen.delete(el);
      near.io.unobserve(el);
      near.inView.unobserve(el);
    };
  }
}

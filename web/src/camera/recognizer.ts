// The recognizer (the camera note's §5): what a view's wheel and gesture events mean. A wheel event
// has deltas, a time and the modifier keys, and nothing else: no phase. So a swipe is a run of events
// less than G ms apart, and whose it is is decided at its first event, for all of it (VIEW-2):
//   - ⌘ or Ctrl held: a zoom (VIEW-5);
//   - over a control marked `data-wheel` (a picture's dot): that control's;
//   - else the camera's: each event cancelled and stopped there, moving the view 1:1, both ways,
//     a nearly straight swipe on its rail until it goes far enough off it (VIEW-3), where the view
//     has rails. One WebKit marks not cancelable is the web view's: the view's box moves by it and
//     the camera takes that in.
// Where the platform gives the page no glide, the camera glides itself (VIEW-4).

import { GLIDE_FROM, GLIDE_RATE, GLIDE_STOP, LINE, RAIL_AFTER, RAIL_ANGLE, RAIL_BREAK, SWIPE_GAP, ZOOM_STEP, ZOOM_WHEEL } from "./numbers";
import { ownGlide, pointerHeld, swipesPaused } from "./state";

/** What a frame's events ask of the camera. */
export interface Asked {
  /** The move, in px, already on its rail. */
  dx: number;
  dy: number;
  /** A zoom: by a factor (⌘-scroll), or to a zoom (a pinch), around a point in the box. */
  zoom: { by: number; to: number | null; at: { x: number; y: number } } | null;
}

/** Whose a swipe is. */
type Owner = "camera" | "control" | "zoom" | "dropped";

/** The speed of a swipe's last events, in px a ms; `at` its last event's time. */
const speedOf = (samples: { t: number; dx: number; dy: number }[]) => {
  const last = samples.at(-1);
  if (!last) return null;
  const recent = samples.filter((s) => s.t >= last.t - GLIDE_FROM);
  if (recent.length < 2) return null;
  const span = last.t - recent[0].t;
  if (span <= 0) return null;
  // The first event's move happened before the span began.
  const sum = recent.slice(1).reduce((a, s) => ({ x: a.x + s.dx, y: a.y + s.dy }), { x: 0, y: 0 });
  return { x: sum.x / span, y: sum.y / span };
};

export class Recognizer {
  /** The last wheel event's time. */
  private last = -Infinity;
  private owner: Owner = "camera";
  /** After a click, a key or an app's move: wheel events are dropped until a gap of G (VIEW-4). */
  private dropping = false;
  /** The rail: decided once the swipe has gone `RAIL_AFTER` px; null until then. */
  private rail: "x" | "y" | "free" | null = null;
  /** How far the swipe has gone each way (its whole length), and what was held back off the way it mostly went before its rail was decided. */
  private went = { x: 0, y: 0 };
  private heldBack = { x: 0, y: 0 };
  /** The swipe's recent events, as moved: the glide starts at their speed. */
  private samples: { t: number; dx: number; dy: number }[] = [];
  /** Events heard since the last frame took them. */
  private pending = { dx: 0, dy: 0 };
  private zoom: Asked["zoom"] = null;
  /** The zoom a pinch started at. */
  private pinchFrom = 1;
  /** Frames since the last event of the camera's swipe; the glide starts at one. */
  private quiet = Infinity;
  private glide: { x: number; y: number } | null = null;
  private frameAt: number | null = null;
  /** Swipes whose events WebKit wouldn't let the page cancel (it took them over). */
  takenOver = 0;
  private overThisSwipe = false;

  constructor(
    private readonly box: () => HTMLElement | null,
    /** The camera's zoom now. */
    private readonly zoomNow: () => number,
    /** Whether the view has rails: a canvas (the Graph) pans freely, as Figma, Freeform and tldraw do. */
    private readonly rails: () => boolean = () => true,
  ) {}

  /** Whether frames are still wanted: events to sum, a glide to run, or one to start. */
  busy() {
    return this.pending.dx !== 0 || this.pending.dy !== 0 || this.zoom !== null || this.glide !== null || (this.owner === "camera" && this.quiet < 2 && this.samples.length > 1 && ownGlide());
  }

  /** A click or a key: stop the glide, and drop the swipe still arriving (VIEW-4). */
  stop() {
    this.glide = null;
    this.dropping = true;
  }

  /** The app moved the view: the same as a click. */
  appMoved() {
    this.stop();
  }

  /** A wheel event over the view's box, heard first (the capture phase). Returns whether it moves the camera. */
  wheel(e: WheelEvent): boolean {
    const t = e.timeStamp;
    const fresh = !(t - this.last >= 0 && t - this.last < SWIPE_GAP);
    this.last = t;
    const zooming = e.ctrlKey || e.metaKey;
    if (fresh) {
      // A gap of G: a new swipe, whatever was dropped before it.
      const control = e.target instanceof Element && !!e.target.closest("[data-wheel]") && !!this.box()?.contains(e.target);
      this.owner = zooming ? "zoom" : control ? "control" : "camera";
      this.dropping = false;
      this.rail = null;
      this.went = { x: 0, y: 0 };
      this.heldBack = { x: 0, y: 0 };
      this.samples = [];
      this.overThisSwipe = false;
    } else if (this.dropping) this.owner = "dropped";
    // Any new event stops the camera's glide: it takes over from it.
    this.glide = null;
    if (this.owner === "control") return false;
    if (zooming) {
      e.preventDefault();
      e.stopPropagation();
      if (pointerHeld()) return false;
      const box = this.box();
      if (!box) return false;
      const dy = Math.max(-ZOOM_WHEEL, Math.min(ZOOM_WHEEL, e.deltaY * this.unit(e, box)));
      const r = box.getBoundingClientRect();
      const by = ZOOM_STEP ** (-dy / ZOOM_WHEEL);
      this.zoom = { by: (this.zoom?.by ?? 1) * by, to: null, at: { x: e.clientX - r.left - box.clientLeft, y: e.clientY - r.top - box.clientTop } };
      return true;
    }
    if (this.owner === "zoom" || this.owner === "dropped") {
      // The rest of a zoom's swipe, or one still arriving after a click: the view stays.
      if (e.cancelable) e.preventDefault();
      e.stopPropagation();
      return false;
    }
    // The swipe probe has the swipes: the web view scrolls them, and the camera takes that in.
    if (swipesPaused()) return false;
    if (!e.cancelable) {
      // WebKit took the swipe over (VIEW-2): its move is the box's, taken in.
      if (!this.overThisSwipe) this.takenOver++;
      this.overThisSwipe = true;
      return false;
    }
    e.preventDefault();
    e.stopPropagation();
    // A drag holds the pointer: the view holds still (VIEW-8).
    if (pointerHeld()) return false;
    const box = this.box();
    if (!box) return false;
    const u = this.unit(e, box);
    this.take(t, e.deltaX * u, e.deltaY * u);
    return true;
  }

  /** Lines and pages as pixels: Chrome's, which only the suites send (VIEW-3). */
  private unit(e: WheelEvent, box: HTMLElement) {
    return e.deltaMode === 1 ? LINE : e.deltaMode === 2 ? box.clientHeight : 1;
  }

  /** One event's move, put on the swipe's rail (VIEW-3). */
  private take(t: number, dx: number, dy: number) {
    this.went.x += Math.abs(dx);
    this.went.y += Math.abs(dy);
    if (!this.rails()) this.rail = "free";
    // Gone far enough off its rail since it began: free from here, for the rest of the swipe. What
    // was held back off the rail stays back.
    else if ((this.rail === "x" && this.went.y > RAIL_BREAK) || (this.rail === "y" && this.went.x > RAIL_BREAK)) this.rail = "free";
    if (this.rail === null) {
      const way = this.went.x >= this.went.y ? "x" : "y";
      if (Math.hypot(this.went.x, this.went.y) >= RAIL_AFTER) {
        const angle = (Math.atan2(this.went.y, this.went.x) * 180) / Math.PI;
        this.rail = angle <= RAIL_ANGLE ? "x" : angle >= 90 - RAIL_ANGLE ? "y" : "free";
        // What was held back off the way it mostly went joins it now, where the rail lets it.
        const back = this.heldBack;
        this.heldBack = { x: 0, y: 0 };
        if (this.rail !== "y") dx += back.x;
        if (this.rail !== "x") dy += back.y;
      } else if (way === "x") {
        this.heldBack.y += dy;
        dy = 0;
      } else {
        this.heldBack.x += dx;
        dx = 0;
      }
    }
    if (this.rail === "x") dy = 0;
    else if (this.rail === "y") dx = 0;
    this.pending.dx += dx;
    this.pending.dy += dy;
    this.samples.push({ t, dx, dy });
    while (this.samples.length > 2 && this.samples[0].t < t - 4 * GLIDE_FROM) this.samples.shift();
    this.quiet = 0;
  }

  /** Safari's gesture events: a pinch's scale from its start, around its point (VIEW-5). */
  gesture(e: Event) {
    e.preventDefault();
    const g = e as Event & { scale?: number; clientX?: number; clientY?: number };
    if (e.type === "gesturestart") this.pinchFrom = this.zoomNow();
    if (e.type !== "gesturechange" || !g.scale) return false;
    const box = this.box();
    if (!box || pointerHeld()) return false;
    this.glide = null;
    const r = box.getBoundingClientRect();
    const x = (g.clientX ?? r.left + box.clientWidth / 2) - r.left - box.clientLeft;
    const y = (g.clientY ?? r.top + box.clientHeight / 2) - r.top - box.clientTop;
    this.zoom = { by: 1, to: this.pinchFrom * g.scale, at: { x, y } };
    return true;
  }

  /** What this frame asks: the events since the last, and the glide's step. `t` is the frame's time. */
  frame(t: number): Asked {
    const dt = this.frameAt === null ? 16 : Math.min(64, Math.max(0, t - this.frameAt));
    this.frameAt = t;
    const asked: Asked = { dx: this.pending.dx, dy: this.pending.dy, zoom: this.zoom };
    const heard = asked.dx !== 0 || asked.dy !== 0;
    this.pending = { dx: 0, dy: 0 };
    this.zoom = null;
    if (heard) return asked;
    this.quiet++;
    // One frame after the swipe's last event, where the platform gives none: the camera's own glide.
    if (this.quiet === 1 && this.owner === "camera" && !this.glide && ownGlide() && !pointerHeld()) {
      const v = speedOf(this.samples);
      if (v && Math.hypot(v.x, v.y) >= GLIDE_STOP) this.glide = v;
      this.samples = [];
    }
    const g = this.glide;
    if (g) {
      if (pointerHeld()) this.glide = null;
      else {
        // Slowing as an iPhone's scroll view does: GLIDE_RATE of its speed kept each ms.
        const keep = GLIDE_RATE ** dt;
        asked.dx += g.x * dt;
        asked.dy += g.y * dt;
        this.glide = Math.hypot(g.x * keep, g.y * keep) < GLIDE_STOP ? null : { x: g.x * keep, y: g.y * keep };
      }
    }
    if (!this.busy()) this.frameAt = null;
    return asked;
  }

  /** The glide met an edge: it stops (VIEW-6). */
  stopGlide() {
    this.glide = null;
  }

  /** Whether the camera glides now. */
  gliding() {
    return this.glide !== null;
  }
}

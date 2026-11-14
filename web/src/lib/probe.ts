// The swipe probe (taste.md VIEW-10, track 0 of docs/designs/camera/design.md): it asks about the
// Mac, then leads the musician through swipes, flicks and pinches over the timeline and the Graph,
// twice, and records what the page hears, so the camera's numbers are measured on the owner's Mac
// before it is built: the gap G that ends a swipe, the glide after a flick, the frame rate (D4) and
// the scrollbars (D5).
//
// Round A only listens. Round B cancels and stops each wheel event over the timeline's and the
// Graph's scroll boxes before any control sees it, so the web view doesn't scroll them, and moves the
// box itself, once a frame, as the camera does (the note's §4): so the musician can say whether the
// Mac shows its scrollbars for a move the page makes. Before each step it makes room for the view to
// move (zoom, and the Song's graph), and it puts back what it changed. It writes nothing to the song;
// `listen`'s stop removes every listener it added and stops its frame loop.
//
// Its last step, after round B, replays the owner's pinch (A13) over the timeline and the Graph by
// itself, and the zoom bench (`zoomBench.ts`, loaded then) measures each frame's split, so the Mac's
// numbers compare one for one with the bench's (the zoom note's §6 step 1).

import { create } from "zustand";
import { cameraNow, controlsOf, NO_EDGES, pauseSwipes, type Edges } from "../camera";
import { measureFrames } from "./frames";
import { native } from "./native";
import { canvas, useShell, type DockView } from "./shell";
import { useStore } from "./store";
import { ARRANGEMENT, setZoom, stepped, ZOOM, zoomNow } from "./zoom";
import type { Run } from "./zoomBench";

export type Round = "A" | "B";
type Box = "arrangement" | "graph";
export interface Step {
  box: Box;
  /** Where it is, in the steps' words. */
  view: "timeline" | "Graph";
  words: string;
  how: string;
  kind: "swipe" | "flick" | "hold" | "pinch";
  /** The swipes or flicks it asks for. */
  count?: number;
}

/** What each step asks, over the timeline and then over the Graph; the pinches last. */
const MOVES: Omit<Step, "box" | "view">[] = [
  { words: "three swipes across", how: "Swipe straight across three times (right, left, right), lifting your fingers between them.", kind: "swipe", count: 3 },
  { words: "three swipes down", how: "Swipe straight down three times (down, up, down), lifting your fingers between them.", kind: "swipe", count: 3 },
  { words: "three diagonal swipes", how: "Swipe diagonally three times, corner to corner and back, lifting your fingers between them.", kind: "swipe", count: 3 },
  { words: "two flicks across", how: "Flick across quickly and lift your fingers, so it glides on. Wait until it stops. Do it twice.", kind: "flick", count: 2 },
  { words: "two diagonal flicks", how: "Flick diagonally, corner to corner, and lift your fingers, so it glides on. Wait until it stops. Do it twice.", kind: "flick", count: 2 },
  { words: "two flicks held", how: "Flick across quickly, then put your fingers straight back down to stop the glide. Do it twice.", kind: "hold", count: 2 },
];
const PINCH = { words: "a pinch", how: "Pinch in and out a few times.", kind: "pinch" as const };
const TIMELINE = { box: "arrangement", view: "timeline" } as const;
const GRAPH = { box: "graph", view: "Graph" } as const;
export const STEPS: Step[] = [...MOVES.map((m) => ({ ...m, ...TIMELINE })), ...MOVES.map((m) => ({ ...m, ...GRAPH })), { ...PINCH, ...TIMELINE }, { ...PINCH, ...GRAPH }];
/** Whether a step asks about the scrollbar: every swipe and flick step, in both rounds. */
export const asks = (s: Step) => s.kind !== "pinch";

/** A step's name in the report: "A3 timeline, three diagonal swipes". */
const stepName = (at: string) => {
  const s = STEPS[Number(at.slice(1)) - 1];
  return s ? `${at} ${s.view}, ${s.words}` : at;
};
const stepOf = (at: string) => STEPS[Number(at.slice(1)) - 1];

/** What the musician says of their Mac: the page can't see it (its user agent always says macOS 10.15). */
export const MODELS = ["MacBook Pro 14 or 16 inch, 2021 or later", "MacBook Air", "iMac", "Mac mini or Mac Studio with a display", "Other"];
export const RATES = ["ProMotion", "120 Hz", "60 Hz", "I don't know"];
export interface Mac {
  model: string | null;
  more: string;
  macos: string;
  rate: string | null;
}

interface Probe {
  open: boolean;
  /** Its first page, the Mac's, the steps, the last step (a pinch replayed), or the end. */
  page: "intro" | "mac" | "steps" | "replay" | "done";
  round: Round;
  /** The step shown, 0 to 13. */
  step: number;
  /** The scrollbar answers, by step ("B1"). */
  answers: Record<string, "Yes" | "No">;
  mac: Mac;
  /** The step the probe has made room for, and its room: "A3 1200x340". */
  room: string;
  /** The last step: not run, running, or each view's median frame's work, in ms. */
  replay: "ready" | "running" | { timeline: number | null; graph: number | null };
}

export const useProbe = create<Probe>(() => ({ open: false, page: "intro", round: "A", step: 0, answers: {}, mac: { model: null, more: "", macos: "", rate: null }, room: "", replay: "ready" }));
export const openProbe = () => useProbe.setState({ open: true });
export const closeProbe = () => useProbe.setState({ open: false });

/** Go `by` steps on, from round A's last into round B's first, and past round B's last to the last step, Replay a pinch. */
export function go(by: 1 | -1, answer?: "Yes" | "No") {
  const { round, step, answers } = useProbe.getState();
  const next = answer ? { ...answers, [`${round}${step + 1}`]: answer } : answers;
  const at = step + by;
  if (at < 0) return useProbe.setState(round === "B" ? { round: "A", step: STEPS.length - 1, answers: next } : { page: "mac", answers: next });
  if (at < STEPS.length) return useProbe.setState({ step: at, answers: next });
  useProbe.setState(round === "A" ? { round: "B", step: 0, answers: next } : { page: "replay", answers: next });
}

// ---------------------------------------------------------------- what it hears

interface Wheel {
  /** When the page heard it, in ms from the probe's start (`performance.now()`). */
  t: number;
  /** The event's own time, in ms from the probe's start: the page's time where its own isn't the page's clock. */
  own: number;
  /** The round and step: "A1". */
  at: string;
  view: string;
  dx: number;
  dy: number;
  mode: number;
  cancelable: boolean;
  cancelled: boolean;
  /** How long after the event's own time the page heard it; null where its time isn't the page's clock. */
  late: number | null;
  keys: string;
  /** With ⌘ or Ctrl: a zoom, the app's, left out of the swipes. */
  zoom: boolean;
}
interface Gesture {
  t: number;
  at: string;
  type: string;
  scale: number;
  x: number;
}
interface Scroll {
  t: number;
  at: string;
  view: string;
  left: number;
  top: number;
  /** The probe's own write (round B), the probe making room, the app's zoom, or anything else. */
  by: "probe" | "setup" | "zoom" | "other";
}
/** Each step's own: when it began, the room it had, what the wheel asked and how far the view moved, its frames. */
interface StepLog {
  start: number;
  room: { x: number; y: number } | null;
  last: { left: number; top: number } | null;
  asked: { x: number; y: number };
  moved: { x: number; y: number };
  frames: number[];
}

const heard = {
  start: 0,
  wheels: [] as Wheel[],
  gestures: [] as Gesture[],
  scrolls: [] as Scroll[],
  frames: [] as number[],
  steps: new Map<string, StepLog>(),
  /** The last step's: what the zoom bench measured over each view. */
  replay: null as { timeline: Run; graph: Run } | null,
};
/** What the panel counts, kept as it hears: round B's scrolls in swipe and flick steps not the probe's are `theirs`. */
const tally = { wheels: 0, scrolls: 0, gestures: 0, cancelled: 0, theirs: 0 };
/** Wheel events whose dispatch hasn't finished: whether one was cancelled is known after. */
let unsettled: [Wheel, WheelEvent][] = [];
/** Round B's moves not yet written, and what the probe last wrote, by box. */
let pending = new Map<HTMLElement, { dx: number; dy: number }>();
let written = new WeakMap<HTMLElement, { x: number; y: number; fx: number; fy: number }>();
/** When the last ⌘ or Ctrl wheel event or gesture event came: a scroll soon after is the app's zoom. */
let zoomedAt = -Infinity;

/** Forget everything heard: Start again. */
export function startAgain() {
  heard.start = 0;
  heard.wheels = [];
  heard.gestures = [];
  heard.scrolls = [];
  heard.frames = [];
  heard.steps = new Map();
  heard.replay = null;
  unsettled = [];
  for (const k of Object.keys(tally) as (keyof typeof tally)[]) tally[k] = 0;
  useProbe.setState({ page: "intro", round: "A", step: 0, answers: {}, room: "", replay: "ready" });
}

const now = () => performance.now() - heard.start;
const here = () => {
  const { round, step } = useProbe.getState();
  return `${round}${step + 1}`;
};
const logOf = (at: string) => {
  let log = heard.steps.get(at);
  if (!log) heard.steps.set(at, (log = { start: now(), room: null, last: null, asked: { x: 0, y: 0 }, moved: { x: 0, y: 0 }, frames: [] }));
  return log;
};
/** The view box an event's target is in: `data-view-box` on the four views' scroll boxes. */
const boxOf = (t: EventTarget | null) => (t instanceof Element ? t.closest<HTMLElement>("[data-view-box]") : null);
const viewOf = (t: EventTarget | null) => boxOf(t)?.dataset.viewBox ?? "other";
const boxEl = (box: Box) => document.querySelector<HTMLElement>(`[data-view-box="${box}"]`);
/** Lines and pages as pixels, as the timeline's zoom takes them. */
const pixels = (e: WheelEvent, box: HTMLElement | null) => (e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? (box?.clientHeight ?? 800) : 1);

function settle() {
  for (const [w, e] of unsettled) {
    w.cancelled = e.defaultPrevented;
    tally.cancelled += +w.cancelled;
  }
  unsettled = [];
}

function onWheel(e: WheelEvent) {
  const heardAt = performance.now();
  const t = heardAt - heard.start;
  const at = here();
  // The event's own time, if it is the page's clock (WebKit's and Chrome's are).
  const late = Math.abs(heardAt - e.timeStamp) < 10_000 ? heardAt - e.timeStamp : null;
  const zoom = e.ctrlKey || e.metaKey;
  if (zoom) zoomedAt = heardAt;
  const w: Wheel = {
    t,
    own: late === null ? t : t - late,
    at,
    view: viewOf(e.target),
    dx: e.deltaX,
    dy: e.deltaY,
    mode: e.deltaMode,
    cancelable: e.cancelable,
    cancelled: false,
    late,
    keys: [e.shiftKey && "shift", e.ctrlKey && "ctrl", e.altKey && "alt", e.metaKey && "meta"].filter(Boolean).join("+") || "-",
    zoom,
  };
  heard.wheels.push(w);
  tally.wheels++;
  unsettled.push([w, e]);
  if (!zoom && w.view === stepOf(at)?.box) {
    const log = logOf(at), px = pixels(e, boxOf(e.target));
    log.asked.x += Math.abs(e.deltaX * px);
    log.asked.y += Math.abs(e.deltaY * px);
  }
}

function onGesture(e: Event) {
  const g = e as Event & { scale?: number; clientX?: number };
  zoomedAt = performance.now();
  heard.gestures.push({ t: now(), at: here(), type: e.type.replace("gesture", ""), scale: g.scale ?? NaN, x: g.clientX ?? NaN });
  tally.gestures++;
}

function onScroll(e: Event) {
  const el = e.target;
  if (!(el instanceof HTMLElement) || !el.dataset.viewBox) return;
  const at = here();
  const step = stepOf(at);
  const w = written.get(el);
  const by: Scroll["by"] =
    w && Math.abs(el.scrollLeft - w.x) <= 1 && Math.abs(el.scrollTop - w.y) <= 1
      ? "probe"
      : settingUp
        ? "setup"
        : step?.kind === "pinch" || performance.now() - zoomedAt < 150
          ? "zoom"
          : "other";
  heard.scrolls.push({ t: now(), at, view: el.dataset.viewBox, left: el.scrollLeft, top: el.scrollTop, by });
  tally.scrolls++;
  if (at.startsWith("B") && by === "other" && step && asks(step)) tally.theirs++;
  // How far the step's own view moved, the probe's room-making left out.
  if (step && el.dataset.viewBox === step.box) {
    const log = logOf(at);
    if (log.last && by !== "setup") {
      log.moved.x += Math.abs(el.scrollLeft - log.last.left);
      log.moved.y += Math.abs(el.scrollTop - log.last.top);
    }
    log.last = { left: el.scrollLeft, top: el.scrollTop };
  }
}

/** Round B: a wheel event over the timeline or the Graph is the probe's, before any control under the pointer sees it. */
function cancel(e: WheelEvent) {
  if (e.ctrlKey || e.metaKey) return;
  const box = boxOf(e.target);
  if (!box || (box.dataset.viewBox !== "arrangement" && box.dataset.viewBox !== "graph")) return;
  e.preventDefault();
  e.stopPropagation();
  const px = pixels(e, box);
  const p = pending.get(box) ?? { dx: 0, dy: 0 };
  p.dx += e.deltaX * px;
  p.dy += e.deltaY * px;
  pending.set(box, p);
}

/** Once a frame: round B's moves, each box written once, in whole pixels; a move it didn't make taken in. */
function move() {
  for (const [el, p] of pending) {
    if (!el.isConnected) continue;
    const w = written.get(el);
    const x0 = w && Math.abs(el.scrollLeft - w.x) <= 1 ? w.fx : el.scrollLeft;
    const y0 = w && Math.abs(el.scrollTop - w.y) <= 1 ? w.fy : el.scrollTop;
    const fx = Math.max(0, Math.min(x0 + p.dx, el.scrollWidth - el.clientWidth));
    const fy = Math.max(0, Math.min(y0 + p.dy, el.scrollHeight - el.clientHeight));
    const x = Math.round(fx), y = Math.round(fy);
    written.set(el, { x, y, fx, fy });
    el.scrollLeft = x;
    el.scrollTop = y;
  }
  pending.clear();
}

/** Listen for a round, until the function this returns is called; it removes every listener and stops its frame loop. */
export function listen(round: Round): () => void {
  if (!heard.start) heard.start = performance.now();
  const offs: (() => void)[] = [];
  const add = (target: Window | Document, type: string, f: (e: never) => void, options: AddEventListenerOptions) => {
    target.addEventListener(type, f as EventListener, options);
    offs.push(() => target.removeEventListener(type, f as EventListener, options));
  };
  // The web view scrolls plain swipes while the probe listens, as before the camera: the camera takes
  // them in, and still zooms (VIEW-10). The probe measures the platform, which the camera would change.
  offs.push(pauseSwipes());
  const quiet = { capture: true, passive: true };
  add(window, "wheel", onWheel, quiet);
  for (const type of ["gesturestart", "gesturechange", "gestureend"]) add(window, type, onGesture, quiet);
  // A scroll doesn't bubble; heard on its way down, from every box, whichever views show.
  add(document, "scroll", onScroll, quiet);
  pending = new Map();
  written = new WeakMap();
  // First of all, after the listener that hears it: no control under the pointer sees it.
  if (round === "B") add(window, "wheel", cancel, { capture: true, passive: false });
  offs.push(
    measureFrames((dt) => {
      heard.frames.push(dt);
      logOf(here()).frames.push(dt);
      settle();
      move();
    }),
  );
  return () => {
    offs.forEach((off) => off());
    settle();
    pending.clear();
  };
}

/** What the panel counts as it listens. */
export function counts() {
  settle();
  return { ...tally };
}

// ---------------------------------------------------------------- room to move

/** The room a view must have each way: a timeline as wide again as it shows; the Graph this much. */
const ROOM = 200;
/** What the window showed before the probe changed it, and the zooms it changed. */
let kept: { selected: string | null; module: string | null; auto: boolean; view: DockView; dock: boolean; zoom: number } | null = null;
let songZoom: { zoom: number; fitted: boolean } | null = null;
let pinched: { box: Box; zoom: number; fitted: boolean } | null = null;
/** Set while the probe makes room: the scrolls then are its own. */
let settingUp = false;
let entering = 0;

const frames = (n: number) => new Promise<void>((r) => {
  const one = (k: number) => (k ? requestAnimationFrame(() => one(k - 1)) : r());
  one(n);
});
const roomOf = (el: HTMLElement) => ({ x: el.scrollWidth - el.clientWidth, y: el.scrollHeight - el.clientHeight });
const zoomOf = (box: Box) => (box === "arrangement" ? { zoom: zoomNow(), fitted: false } : (canvas.graphZoom?.() ?? null));
function zoomTo(box: Box, z: { zoom: number; fitted: boolean }) {
  if (box === "arrangement") return void (zoomNow() !== z.zoom && setZoom(z.zoom));
  canvas.graphZoom?.(z.fitted ? null : z.zoom);
}
function putPinchBack() {
  if (pinched) zoomTo(pinched.box, pinched);
  pinched = null;
}

/** Make room for `box` to move both ways, if it has none: the timeline zoomed in; the Song's graph, zoomed in. */
async function makeRoom(box: Box, alive: () => boolean) {
  if (box === "arrangement") {
    for (let i = 0; i < 12 && alive(); i++) {
      const el = boxEl("arrangement");
      if (!el || roomOf(el).x >= el.clientWidth || zoomNow() >= ZOOM.max) return;
      setZoom(stepped(zoomNow(), 1));
      await frames(2);
    }
    return;
  }
  const enough = () => {
    const el = boxEl("graph");
    return !!el && roomOf(el).x >= ROOM && roomOf(el).y >= ROOM;
  };
  if (enough()) return;
  await songGraph(alive);
  let was = -1;
  for (let i = 0; i < 16 && alive() && !enough(); i++) {
    const z = canvas.graphZoom?.();
    if (!z || z.zoom <= was) break;
    was = z.zoom;
    canvas.graphZoom?.(z.zoom * 1.2);
    await frames(2);
  }
}

/** The Song's graph in the dock, and its zoom kept to put back. */
async function songGraph(alive: () => boolean) {
  const shell = useShell.getState();
  if (shell.module !== "song" || shell.view !== "graph" || !shell.dock) {
    useShell.setState({ view: "graph", dock: true });
    shell.select("song");
  }
  for (let i = 0; i < 30 && alive() && canvas.graphZoom?.().module !== "song"; i++) await frames(1);
  songZoom ??= canvas.graphZoom?.() ?? null;
}

/** What the window shows, kept once to put back when the probe stops. */
function keep() {
  if (kept) return;
  const sh = useShell.getState();
  kept = { selected: sh.selected, module: sh.module, auto: sh.auto, view: sh.view, dock: sh.dock, zoom: zoomNow() };
}

/** A step shows: put back the last pinch's zoom, make room for this one's view, and note its start. */
export async function enter(round: Round, step: number) {
  const me = ++entering;
  const alive = () => me === entering;
  settingUp = true;
  putPinchBack();
  const s = STEPS[step];
  const at = `${round}${step + 1}`;
  const log = logOf(at);
  keep();
  await makeRoom(s.box, alive);
  // What the room-making moved comes a frame or two later.
  await frames(2);
  await new Promise((r) => setTimeout(r, 150));
  if (!alive()) return;
  const el = boxEl(s.box);
  log.room ??= el ? roomOf(el) : { x: 0, y: 0 };
  log.last = el ? { left: el.scrollLeft, top: el.scrollTop } : null;
  if (s.kind === "pinch") {
    const z = zoomOf(s.box);
    if (z) pinched = { box: s.box, ...z };
  }
  settingUp = false;
  const r = el ? roomOf(el) : { x: 0, y: 0 };
  useProbe.setState({ room: `${at} ${Math.round(r.x)}x${Math.round(r.y)}` });
}

/** The probe stops: put back every zoom, the selection and the dock's view it changed. */
export function putBack() {
  entering++;
  settingUp = false;
  putPinchBack();
  if (!kept) return;
  const sh = useShell.getState();
  if (songZoom && canvas.graphZoom?.().module === "song") canvas.graphZoom(songZoom.fitted ? null : songZoom.zoom);
  songZoom = null;
  if (zoomNow() !== kept.zoom) setZoom(kept.zoom);
  if (sh.module !== kept.module) useShell.setState({ selected: kept.selected, module: kept.module, auto: kept.auto });
  if (sh.view !== kept.view || sh.dock !== kept.dock) useShell.setState({ view: kept.view, dock: kept.dock });
  kept = null;
  useProbe.setState({ room: "" });
}

// ---------------------------------------------------------------- the last step: a pinch replayed

/** A point in `el`'s clear part (inside `m`), as fractions of it across and down. */
function pointIn(el: HTMLElement, m: Edges, fx: number, fy: number) {
  const r = el.getBoundingClientRect();
  return [Math.round(r.left + el.clientLeft + m.left + (el.clientWidth - m.left - m.right) * fx), Math.round(r.top + el.clientTop + m.top + (el.clientHeight - m.top - m.bottom) * fy)] as const;
}
const zoomOfCamera = (name: string) => () => cameraNow(name)?.getState().zoom ?? null;
const SONG_GRAPH = "graph:song";
/** The zoom bench, once the last step has loaded it. */
let bench: typeof import("./zoomBench") | null = null;

/**
 * The last step: the owner's pinch of step A13 replayed by the zoom bench over the timeline, at 24 px
 * a beat from the song's start, then over the Song's graph at Fit (zoom 1), each frame measured.
 * None of the probe's listeners is on. Then every zoom, the selection and the dock's view put back.
 */
export async function replayPinch() {
  if (useProbe.getState().replay === "running") return;
  useProbe.setState({ replay: "running" });
  const me = ++entering;
  const alive = () => me === entering && useProbe.getState().open && useProbe.getState().page === "replay";
  const settle = async () => {
    await frames(3);
    await new Promise((r) => setTimeout(r, 800));
  };
  let done: { timeline: Run; graph: Run } | null = null;
  try {
    const b = (bench = await import("./zoomBench"));
    keep();
    controlsOf(ARRANGEMENT)?.to(0, 0, ZOOM.start);
    await settle();
    const tl = boxEl("arrangement");
    if (!tl || !alive()) return;
    const timeline = await b.replay(...pointIn(tl, cameraNow(ARRANGEMENT)?.getState().margins ?? NO_EDGES, 0.4, 0.3), { kind: "pinch", scales: b.A13 }, { zoom: zoomOfCamera(ARRANGEMENT) });
    if (!alive()) return;
    await songGraph(alive);
    canvas.graphZoom?.(null);
    await settle();
    const gr = boxEl("graph");
    if (!gr || !alive()) return;
    const graph = await b.replay(...pointIn(gr, NO_EDGES, 0.5, 0.5), { kind: "pinch", scales: b.A13 }, { zoom: zoomOfCamera(SONG_GRAPH) });
    done = heard.replay = { timeline, graph };
    const med = (r: Run) => b.split([r]).work?.med ?? null;
    useProbe.setState({ replay: { timeline: med(timeline), graph: med(graph) } });
  } finally {
    if (me === entering) putBack();
    if (!done) useProbe.setState({ replay: "ready" });
  }
}

// ---------------------------------------------------------------- the report

const num = (n: number, places = 3) => String(Math.round(n * 10 ** places) / 10 ** places);
const ms = (n: number) => `${num(n, 1)} ms`;
const pct = (a: number, b: number) => (b ? `${Math.round((100 * a) / b)}%` : "none");
const sorted = (xs: number[]) => [...xs].sort((a, b) => a - b);
const quantile = (s: number[], q: number) => (s.length ? s[Math.min(s.length - 1, Math.floor(q * s.length))] : NaN);
const VIEW_WORDS: Record<string, string> = { arrangement: "timeline", graph: "Graph", notes: "Notes", beat: "beat grid", other: "elsewhere" };
/** Where a swipe ends: these gaps, in ms, for the step table. */
const GS = [30, 50, 80, 120];
/** The gap the flicks and the take-overs split swipes at. */
const SPLIT = 120;
const TAKEN = 50;
const BUCKETS = [0, 4, 8, 12, 17, 25, 30, 40, 50, 65, 80, 100, 120, 150, 250, 500, 1000];
/** The raw events kept of each step. */
const RAW_WHEELS = 90, RAW_GESTURES = 40, RAW_SCROLLS = 40;

/** Runs of wheel events less than `g` ms apart, by their own times. */
function swipes(ws: Wheel[], g: number) {
  const out: Wheel[][] = [];
  ws.forEach((w, i) => {
    if (!i || w.own - ws[i - 1].own > g) out.push([]);
    out[out.length - 1].push(w);
  });
  return out;
}
const events = (n: number) => `${n} event${n === 1 ? "" : "s"}`;
const gapsOf = (ws: Wheel[]) => ws.slice(1).map((w, i) => w.own - ws[i].own);

const both = (w: Wheel) => w.dx !== 0 && w.dy !== 0;
const fraction = (w: Wheel) => !Number.isInteger(w.dx) || !Number.isInteger(w.dy);
const speed = (w: Wheel) => Math.hypot(w.dx, w.dy);
/** A run's direction: 0° straight across, 90° straight down. */
const angle = (ws: Wheel[]) => Math.round((Math.atan2(Math.abs(ws.reduce((a, w) => a + w.dy, 0)), Math.abs(ws.reduce((a, w) => a + w.dx, 0))) * 180) / Math.PI);

/** Whether scrollbars float over what they scroll (a Mac's "When scrolling"), or take room (its "Always"), in px. */
function scrollbarRoom() {
  const probe = document.createElement("div");
  probe.style.cssText = "position: absolute; top: -9999px; width: 100px; height: 100px; overflow: scroll";
  document.body.appendChild(probe);
  const w = probe.offsetWidth - probe.clientWidth;
  probe.remove();
  return w;
}

/** The report to paste back: the summary first, then each step's first raw events. */
export function report(): string {
  settle();
  const { wheels, gestures, scrolls, frames } = heard;
  const { answers, mac } = useProbe.getState();
  const graph = useStore.getState().graph;
  const out: string[] = [];
  const line = (s = "") => out.push(s);
  const rounds: Round[] = ["A", "B"];
  const ats = rounds.flatMap((r) => STEPS.map((_, i) => `${r}${i + 1}`));
  const inRound = <T extends { at: string }>(xs: T[], r: Round) => xs.filter((x) => x.at.startsWith(r));
  const swipeWheels = (at: string) => wheels.filter((w) => w.at === at && !w.zoom);

  line(`music2 swipe probe, ${new Date().toISOString()}`);
  line("Paste all of this back. The summary comes first, then the events it was made from.");
  line();
  line("Your Mac, as you said");
  line(`  Model: ${mac.model ?? "not said"}${mac.more.trim() ? ` (more: ${mac.more.trim()})` : ""}`);
  line(`  macOS: ${mac.macos.trim() || "not said"}`);
  line(`  Screen's refresh rate: ${mac.rate ?? "not said"}`);
  line();
  line("Where it ran");
  line(`  In the app: ${native ? "yes" : "no (a browser)"}`);
  line(`  Song: ${graph ? `${graph.name}, ${graph.timeline.tracks.length} tracks` : "none open"}`);
  line(`  userAgent: ${navigator.userAgent}`);
  line(`  platform: ${navigator.platform}`);
  line(`  devicePixelRatio: ${window.devicePixelRatio}`);
  line(`  screen: ${screen.width}×${screen.height} (the window ${window.innerWidth}×${window.innerHeight})`);
  const bar = scrollbarRoom();
  line(
    bar === 0
      ? `  Scroll bars: overlay: they take no room (the Mac's Show scroll bars: "When scrolling", or "Automatically" with a trackpad)`
      : `  Scroll bars: always shown, ${bar} px wide (the Mac's Show scroll bars: "Always", or "Automatically" with a mouse)`,
  );
  const f = sorted(frames);
  const median = quantile(f, 0.5);
  line(
    `  The page's frame rate (the web view may hold it at 60 even on a 120 Hz screen): ${
      f.length ? `median ${ms(median)} (${Math.round(1000 / median)} a second), 95th percentile ${ms(quantile(f, 0.95))}, slowest ${ms(f.at(-1)!)}, ${f.length} frames` : "median – ms, none measured"
    }`,
  );
  line();

  line("Swipes in each step: a swipe is a run of wheel events less than G ms apart (⌘ and Ctrl events left out)");
  line(`| Step | asked for | ${GS.map((g) => `G = ${g} ms`).join(" | ")} | wheel events | both deltas | fractions | ⌘ or Ctrl |`);
  line(`|---|---|${GS.map(() => "---").join("|")}|---|---|---|---|`);
  for (const at of ats) {
    const s = stepOf(at);
    if (!asks(s)) continue;
    const ws = swipeWheels(at);
    line(`| ${stepName(at)} | ${s.count} | ${GS.map((g) => (ws.length ? swipes(ws, g).length : 0)).join(" | ")} | ${ws.length} | ${pct(ws.filter(both).length, ws.length)} | ${pct(ws.filter(fraction).length, ws.length)} | ${wheels.filter((w) => w.at === at && w.zoom).length} |`);
  }
  line();

  const gaps = ats.flatMap((at) => gapsOf(swipeWheels(at)));
  line("Gaps between wheel events within a step (⌘ and Ctrl events left out)");
  const counted = BUCKETS.map((lo, i) => {
    const hi = BUCKETS[i + 1];
    return [hi === undefined ? `${lo}+ ms` : `${lo}–${hi} ms`, gaps.filter((g) => g >= lo && (hi === undefined || g < hi)).length] as const;
  });
  const most = Math.max(1, ...counted.map(([, n]) => n));
  for (const [range, n] of counted) line(`  ${range.padStart(11)} ${String(n).padStart(6)} ${"#".repeat(Math.ceil((40 * n) / most))}`);
  const sg = sorted(gaps);
  line(sg.length ? `  Median ${ms(quantile(sg, 0.5))}, 90th percentile ${ms(quantile(sg, 0.9))}, 99th percentile ${ms(quantile(sg, 0.99))}` : "  None yet.");
  line();

  line("Each step: the view's room when it began, what the wheel asked and how far the view moved (px, across × down), its frames, how late the page heard its events, and the scrollbar");
  line("| Step | room | the wheel asked | the view moved | frames | heard late: median, slowest | scrollbar showed |");
  line("|---|---|---|---|---|---|---|");
  const xy = (p: { x: number; y: number } | null) => (p ? `${Math.round(p.x)} × ${Math.round(p.y)}` : "–");
  for (const at of ats) {
    const log = heard.steps.get(at);
    if (!log) continue;
    const fs = sorted(log.frames);
    const fm = quantile(fs, 0.5);
    const frameWords = fs.length ? `${Math.round(1000 / fm)} a second, slowest ${ms(fs.at(-1)!)}, over 16.7 ms: ${fs.filter((d) => d > 16.7).length}, over 8.3 ms: ${fs.filter((d) => d > 8.3).length}` : "none";
    const lates = sorted(wheels.flatMap((w) => (w.at === at && w.late !== null ? [w.late] : [])));
    const lateWords = lates.length ? `${ms(quantile(lates, 0.5))}, ${ms(lates.at(-1)!)}` : "–";
    line(`| ${stepName(at)} | ${xy(log.room)} | ${xy(log.asked)} | ${xy(log.moved)} | ${frameWords} | ${lateWords} | ${answers[at] ?? (asks(stepOf(at)) ? "not answered" : "–")} |`);
  }
  line();

  line(`After each flick: the direction of the flick and of the glide after its fastest event (0° is straight across, 90° straight down; swipes split at gaps over ${SPLIT} ms)`);
  for (const at of ats) {
    const s = stepOf(at);
    if (s.kind !== "flick" && s.kind !== "hold") continue;
    const runs = swipes(swipeWheels(at), SPLIT).filter((run) => run.length >= 3);
    if (!runs.length) {
      line(`  ${stepName(at)}: none heard`);
      continue;
    }
    line(`  ${stepName(at)}:`);
    runs.slice(0, 4).forEach((run, i) => {
      const peak = run.reduce((a, w, j) => (speed(w) > speed(run[a]) ? j : a), 0);
      const push = run.slice(0, peak + 1), glide = run.slice(peak + 1);
      const ax = glide.reduce((a, w) => a + Math.abs(w.dx), 0), ay = glide.reduce((a, w) => a + Math.abs(w.dy), 0);
      line(
        `    ${i + 1}: the flick ${angle(push)}°, ${events(push.length)}; ${
          glide.length
            ? `the glide after it ${ms(glide.at(-1)!.own - run[peak].own)}, ${events(glide.length)}, ${angle(glide)}°, ${pct(ax, ax + ay)} across and ${pct(ay, ax + ay)} down, the last ${num(speed(glide.at(-1)!))} px`
            : "no glide after it"
        }`,
      );
    });
  }
  line();

  line(`Swipes WebKit took over, at G = ${TAKEN} ms`);
  line("  WebKit lets the web view keep a swipe the page didn't cancel at its first event: its later events can't be cancelled. In round A, where the app cancels nothing, that is its normal way. In round B, a swipe taken over is one the page was too slow to answer (the camera note's C1).");
  line("| Step | swipes | all cancelable | first cancelable, a later one not (taken over) | none cancelable |");
  line("|---|---|---|---|---|");
  for (const at of ats) {
    const runs = swipes(swipeWheels(at), TAKEN);
    if (!runs.length) continue;
    const taken = runs.filter((r) => r[0].cancelable && r.some((w) => !w.cancelable)).length;
    const none = runs.filter((r) => r.every((w) => !w.cancelable)).length;
    line(`| ${stepName(at)} | ${runs.length} | ${runs.length - taken - none} | ${taken} | ${none} |`);
  }
  line();

  const bs = inRound(scrolls, "B");
  const inSwipes = bs.filter((c) => asks(stepOf(c.at)));
  line("Round B's scrolls: the probe moves the view itself, so in swipe and flick steps every scroll should be its own");
  line(`  In swipe and flick steps, not the probe's: ${inSwipes.filter((c) => c.by === "other").length} (0 when cancelling works)`);
  for (const v of ["arrangement", "graph"]) {
    const of = inSwipes.filter((c) => c.view === v);
    if (of.length) line(`    ${VIEW_WORDS[v]}: ${of.filter((c) => c.by === "probe").length} the probe's, ${of.filter((c) => c.by === "setup").length} making room, ${of.filter((c) => c.by === "zoom").length} the app's zoom (⌘), ${of.filter((c) => c.by === "other").length} others`);
  }
  const pinches = bs.filter((c) => !asks(stepOf(c.at)) && c.by !== "setup");
  line(`  In the pinch steps, the app's zoom: ${["arrangement", "graph"].map((v) => `${VIEW_WORDS[v]} ${pinches.filter((c) => c.view === v).length}`).join(", ")}`);
  const as = inRound(scrolls, "A");
  line(`Round A's scrolls: ${["arrangement", "graph", "notes", "beat"].map((v) => `${VIEW_WORDS[v]} ${as.filter((c) => c.view === v && c.by !== "setup").length}`).join(", ")}; making room ${as.filter((c) => c.by === "setup").length}`);
  line();

  const scales = gestures.map((g) => g.scale).filter(Number.isFinite);
  const kind = (type: string) => gestures.filter((g) => g.type === type).length;
  line(
    gestures.length
      ? `Gesture events: ${gestures.length} (${kind("start")} start, ${kind("change")} change, ${kind("end")} end); scale from ${num(Math.min(...scales))} to ${num(Math.max(...scales))}`
      : "Gesture events: none (no gesturestart, gesturechange or gestureend)",
  );
  line("Wheel events");
  for (const r of rounds) {
    const ws = inRound(wheels, r);
    const by = (v: string) => ws.filter((w) => w.view === v).length;
    const views = Object.keys(VIEW_WORDS)
      .filter((v) => by(v))
      .map((v) => `${VIEW_WORDS[v]} ${by(v)}`)
      .join(", ");
    line(
      `  Round ${r}: ${ws.length} (${views || "none"}); in pixels ${ws.filter((w) => w.mode === 0).length}, lines ${ws.filter((w) => w.mode === 1).length}, pages ${ws.filter((w) => w.mode === 2).length}; cancelable: false ${ws.filter((w) => !w.cancelable).length}; cancelled ${ws.filter((w) => w.cancelled).length}; both deltas ${pct(ws.filter(both).length, ws.length)}; fractions ${pct(ws.filter(fraction).length, ws.length)}; ⌘ or Ctrl ${ws.filter((w) => w.zoom).length}`,
    );
  }
  line();

  const r = heard.replay;
  if (r && bench) {
    const z = (v: number | null) => (v === null ? "–" : num(v, 2));
    const way = (run: Run) => `from ${z(run.zoom.from)} to ${z(run.zoom.min)} and back to ${z(run.zoom.to)}`;
    line(
      `The replayed pinch: step A13's pinch (its scale from 1 to ${num(Math.min(...bench.A13))} and back, ${bench.A13.length} changes ${bench.GAP} ms apart), sent by the page over the timeline (24 px a beat, from the song's start) and over the Song's graph (Fit, zoom 1); your fingers did nothing`,
    );
    line(bench.table("View", [["timeline", [r.timeline]], ["Graph", [r.graph]]]));
    line(`  The zoom: timeline ${way(r.timeline)}; Graph ${way(r.graph)}`);
  } else line("The replayed pinch: not run (the last step's Replay wasn't pressed)");
  line();

  line(`Raw events: for each step, its first ${RAW_WHEELS} wheel events and ${RAW_GESTURES} gesture events, and in round B the scrolls that weren't the probe's (the first ${RAW_SCROLLS}); times in ms from the step's start`);
  line("  w time deltaX deltaY (c: could be cancelled, x: was) late [mode=1|2] [view=…] [keys=…]");
  line("  g time start|change|end scale clientX");
  line("  s time view scrollLeft scrollTop setup|zoom|other");
  for (const at of ats) {
    const log = heard.steps.get(at);
    const ws = wheels.filter((w) => w.at === at), gs = gestures.filter((g) => g.at === at);
    const ss = at.startsWith("B") ? scrolls.filter((c) => c.at === at && c.by !== "probe") : [];
    if (!log || (!ws.length && !gs.length && !ss.length)) continue;
    const cut = [ws.length > RAW_WHEELS && `${ws.length - RAW_WHEELS} wheel`, gs.length > RAW_GESTURES && `${gs.length - RAW_GESTURES} gesture`, ss.length > RAW_SCROLLS && `${ss.length - RAW_SCROLLS} scroll`].filter(Boolean);
    line(`${stepName(at)}: ${ws.length} wheel, ${gs.length} gesture${at.startsWith("B") ? `, ${ss.length} scrolls not the probe's` : ""}${cut.length ? `; cut: ${cut.join(", ")}` : ""}`);
    const step = stepOf(at);
    const rows = [
      ...ws.slice(0, RAW_WHEELS).map((w) => ({
        t: w.t,
        s: `w ${num(w.t - log.start, 1)} ${num(w.dx)} ${num(w.dy)} ${w.cancelable ? "c" : ""}${w.cancelled ? "x" : ""}${w.cancelable || w.cancelled ? "" : "-"} ${w.late === null ? "-" : num(w.late, 1)}${w.mode ? ` mode=${w.mode}` : ""}${w.view !== step.box ? ` view=${w.view}` : ""}${w.keys !== "-" ? ` keys=${w.keys}` : ""}`,
      })),
      ...gs.slice(0, RAW_GESTURES).map((g) => ({ t: g.t, s: `g ${num(g.t - log.start, 1)} ${g.type} ${num(g.scale)} ${num(g.x, 1)}` })),
      ...ss.slice(0, RAW_SCROLLS).map((c) => ({ t: c.t, s: `s ${num(c.t - log.start, 1)} ${c.view} ${num(c.left, 1)} ${num(c.top, 1)} ${c.by}` })),
    ].sort((a, b) => a.t - b.t);
    for (const r of rows) line(r.s);
  }
  return out.join("\n");
}

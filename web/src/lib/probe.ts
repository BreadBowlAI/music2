// The swipe probe (taste.md VIEW-10, track 0 of docs/designs/camera/design.md): it leads the
// musician through swipes, flicks and pinches over the timeline and the Graph, twice, and records
// what the page hears, so the camera's numbers are measured on the owner's Mac before it is built:
// the gap G that ends a swipe, the glide after a flick, the frame rate (D4) and the scrollbars (D5).
//
// Round A only listens. Round B cancels each wheel event over the timeline's and the Graph's scroll
// boxes, so the browser doesn't scroll them, and moves the box itself, once a frame, as the camera
// will (the note's §4): so the musician can say whether the Mac shows its scrollbars for a move the
// page makes. It writes nothing to the song; `listen`'s stop removes every listener it added.

import { create } from "zustand";
import { measureFrames } from "./frames";
import { useStore } from "./store";

export type Round = "A" | "B";

/** What each step asks, over the timeline and then over the Graph. */
const MOVES = [
  { words: "swipe straight across", how: "Swipe straight across with two fingers, left and right, a few times.", swipe: true },
  { words: "swipe straight down", how: "Swipe straight down and up, a few times.", swipe: true },
  { words: "swipe diagonally", how: "Swipe diagonally, from corner to corner, a few times.", swipe: true },
  { words: "flick and let go", how: "Flick across quickly and lift your fingers, so it glides on. Wait until it stops. Do it twice.", swipe: true },
  { words: "flick and hold", how: "Flick across quickly, then put your fingers straight back down to stop the glide. Do it twice.", swipe: true },
  { words: "pinch", how: "Pinch in and out, a few times.", swipe: false },
];
export const STEPS = [...MOVES.map((m) => ({ ...m, view: "timeline" as const })), ...MOVES.map((m) => ({ ...m, view: "Graph" as const }))];

/** A step's name in the report: "A3 timeline, swipe diagonally". */
const stepName = (at: string) => {
  const s = STEPS[Number(at.slice(1)) - 1];
  return s ? `${at} ${s.view}, ${s.words}` : at;
};

interface Probe {
  open: boolean;
  /** Its first page, the steps, or the end. */
  page: "intro" | "steps" | "done";
  round: Round;
  /** The step shown, 0 to 11. */
  step: number;
  /** Round B's scrollbar answers, by step ("B1"). */
  answers: Record<string, "Yes" | "No">;
}

export const useProbe = create<Probe>(() => ({ open: false, page: "intro", round: "A", step: 0, answers: {} }));
export const openProbe = () => useProbe.setState({ open: true });
export const closeProbe = () => useProbe.setState({ open: false });

/** Go `by` steps on, from round A's last into round B's first, and past round B's last to the end. */
export function go(by: 1 | -1, answer?: "Yes" | "No") {
  const { round, step, answers } = useProbe.getState();
  const next = answer ? { ...answers, [`${round}${step + 1}`]: answer } : answers;
  const at = step + by;
  if (at < 0) return useProbe.setState(round === "B" ? { round: "A", step: STEPS.length - 1, answers: next } : { page: "intro", answers: next });
  if (at < STEPS.length) return useProbe.setState({ step: at, answers: next });
  useProbe.setState(round === "A" ? { round: "B", step: 0, answers: next } : { page: "done", answers: next });
}

// ---------------------------------------------------------------- what it hears

interface Wheel {
  /** When the page heard it, in ms from the probe's start (`performance.now()`). */
  t: number;
  /** The round and step: "A1". */
  at: string;
  view: string;
  dx: number;
  dy: number;
  mode: number;
  cancelable: boolean;
  cancelled: boolean;
  /** The gap from the step's last wheel event, by the events' own times; null for its first. */
  gap: number | null;
  /** How long after the event's own time the page heard it; null where its time isn't the page's clock. */
  late: number | null;
  keys: string;
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
  /** The probe's own write (round B). */
  ours: boolean;
}

const heard = { start: 0, wheels: [] as Wheel[], gestures: [] as Gesture[], scrolls: [] as Scroll[], frames: [] as number[] };
/** What the panel counts, kept as it hears: round B's scrolls not the probe's are `theirs`. */
const tally = { wheels: 0, scrolls: 0, gestures: 0, cancelled: 0, theirs: 0 };
/** Wheel events whose dispatch hasn't finished: whether one was cancelled is known after. */
let unsettled: [Wheel, WheelEvent][] = [];
/** Round B's moves not yet written, and what the probe last wrote, by box. */
let pending = new Map<HTMLElement, { dx: number; dy: number }>();
let written = new WeakMap<HTMLElement, { x: number; y: number; fx: number; fy: number }>();

/** Forget everything heard: Start again. */
export function startAgain() {
  heard.start = 0;
  heard.wheels = [];
  heard.gestures = [];
  heard.scrolls = [];
  heard.frames = [];
  unsettled = [];
  for (const k of Object.keys(tally) as (keyof typeof tally)[]) tally[k] = 0;
  useProbe.setState({ page: "intro", round: "A", step: 0, answers: {} });
}

const now = () => performance.now() - heard.start;
const here = () => {
  const { round, step } = useProbe.getState();
  return `${round}${step + 1}`;
};
/** The view box an event's target is in: `data-view-box` on the four views' scroll boxes. */
const boxOf = (t: EventTarget | null) => (t instanceof Element ? t.closest<HTMLElement>("[data-view-box]") : null);
const viewOf = (t: EventTarget | null) => boxOf(t)?.dataset.viewBox ?? "other";

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
  const last = heard.wheels.at(-1);
  // The event's own time, if it is the page's clock (WebKit's and Chrome's are).
  const late = Math.abs(heardAt - e.timeStamp) < 10_000 ? heardAt - e.timeStamp : null;
  const own = late === null ? t : t - late;
  const w: Wheel = {
    t,
    at,
    view: viewOf(e.target),
    dx: e.deltaX,
    dy: e.deltaY,
    mode: e.deltaMode,
    cancelable: e.cancelable,
    cancelled: false,
    gap: last && last.at === at ? own - (last.late === null ? last.t : last.t - last.late) : null,
    late,
    keys: [e.shiftKey && "shift", e.ctrlKey && "ctrl", e.altKey && "alt", e.metaKey && "meta"].filter(Boolean).join("+") || "-",
  };
  heard.wheels.push(w);
  tally.wheels++;
  unsettled.push([w, e]);
}

function onGesture(e: Event) {
  const g = e as Event & { scale?: number; clientX?: number };
  heard.gestures.push({ t: now(), at: here(), type: e.type.replace("gesture", ""), scale: g.scale ?? NaN, x: g.clientX ?? NaN });
  tally.gestures++;
}

function onScroll(e: Event) {
  const el = e.target;
  if (!(el instanceof HTMLElement) || !el.dataset.viewBox) return;
  const w = written.get(el);
  const ours = !!w && Math.abs(el.scrollLeft - w.x) <= 1 && Math.abs(el.scrollTop - w.y) <= 1;
  const at = here();
  heard.scrolls.push({ t: now(), at, view: el.dataset.viewBox, left: el.scrollLeft, top: el.scrollTop, ours });
  tally.scrolls++;
  if (at.startsWith("B") && !ours) tally.theirs++;
}

/** Round B: a wheel event over the timeline or the Graph that no control took is the probe's to move by. */
function cancel(e: WheelEvent) {
  if (e.defaultPrevented || e.ctrlKey || e.metaKey) return;
  const box = boxOf(e.target);
  if (!box || (box.dataset.viewBox !== "arrangement" && box.dataset.viewBox !== "graph")) return;
  e.preventDefault();
  // Lines and pages as pixels, as the timeline's zoom takes them.
  const px = e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? box.clientHeight : 1;
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

/** Listen for a round, until the function this returns is called; it removes every listener. */
export function listen(round: Round): () => void {
  if (!heard.start) heard.start = performance.now();
  const offs: (() => void)[] = [];
  const add = (target: Window | Document, type: string, f: (e: never) => void, options: AddEventListenerOptions) => {
    target.addEventListener(type, f as EventListener, options);
    offs.push(() => target.removeEventListener(type, f as EventListener, options));
  };
  const quiet = { capture: true, passive: true };
  add(window, "wheel", onWheel, quiet);
  for (const type of ["gesturestart", "gesturechange", "gestureend"]) add(window, type, onGesture, quiet);
  // A scroll doesn't bubble; heard on its way down, from every box, whichever views show.
  add(document, "scroll", onScroll, quiet);
  pending = new Map();
  written = new WeakMap();
  if (round === "B") {
    // Last, after every control under the pointer has had its say.
    add(window, "wheel", cancel, { passive: false });
  }
  offs.push(
    measureFrames((dt) => {
      heard.frames.push(dt);
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

// ---------------------------------------------------------------- the report

const num = (n: number, places = 3) => String(Math.round(n * 10 ** places) / 10 ** places);
const ms = (n: number) => `${num(n, 1)} ms`;
const pct = (a: number, b: number) => (b ? `${Math.round((100 * a) / b)}%` : "none");
const sorted = (xs: number[]) => [...xs].sort((a, b) => a - b);
const quantile = (s: number[], q: number) => (s.length ? s[Math.min(s.length - 1, Math.floor(q * s.length))] : NaN);
const VIEW_WORDS: Record<string, string> = { arrangement: "timeline", graph: "Graph", notes: "Notes", beat: "beat grid", other: "elsewhere" };
/** Where a swipe ends: these gaps, in ms, for the step table. */
const GS = [25, 34, 50, 75, 100, 150];
/** The gap the flicks' swipes are split at. */
const SPLIT = 100;
const BUCKETS = [0, 4, 8, 12, 17, 25, 34, 50, 75, 100, 150, 250, 500, 1000];
const RAW = 4000;

/** A run of a step's wheel events closer than `g` ms. */
function swipes(ws: Wheel[], g: number) {
  const out: Wheel[][] = [];
  for (const w of ws) {
    if (!out.length || w.gap === null || w.gap > g) out.push([]);
    out[out.length - 1].push(w);
  }
  return out;
}

const both = (w: Wheel) => w.dx !== 0 && w.dy !== 0;
const fraction = (w: Wheel) => !Number.isInteger(w.dx) || !Number.isInteger(w.dy);
const speed = (w: Wheel) => Math.hypot(w.dx, w.dy);

/** The report to paste back: the summary first, then the last 4000 events it was made from. */
export function report(): string {
  settle();
  const { wheels, gestures, scrolls, frames } = heard;
  const { answers } = useProbe.getState();
  const graph = useStore.getState().graph;
  const out: string[] = [];
  const line = (s = "") => out.push(s);
  const rounds: Round[] = ["A", "B"];
  const inRound = <T extends { at: string }>(xs: T[], r: Round) => xs.filter((x) => x.at.startsWith(r));

  line(`music2 swipe probe, ${new Date().toISOString()}`);
  line("Paste all of this back. The summary comes first, then the events it was made from.");
  line(`Song: ${graph ? `${graph.name}, ${graph.timeline.tracks.length} tracks` : "none open"}`);
  line();
  line("Platform");
  line(`  userAgent: ${navigator.userAgent}`);
  line(`  platform: ${navigator.platform}`);
  line(`  devicePixelRatio: ${window.devicePixelRatio}`);
  line(`  screen: ${screen.width}×${screen.height} (the window ${window.innerWidth}×${window.innerHeight})`);
  const f = sorted(frames);
  const median = quantile(f, 0.5);
  line(
    f.length
      ? `  Frames: median ${ms(median)} (${Math.round(1000 / median)} a second), 95th percentile ${ms(quantile(f, 0.95))}, slowest ${ms(f.at(-1)!)}, ${f.length} frames, ${f.filter((d) => d > median * 1.5).length} over 1.5 times the median`
      : "  Frames: median – ms, none heard yet; 95th percentile – ms",
  );
  line();

  line("Wheel events");
  for (const r of rounds) {
    const ws = inRound(wheels, r);
    const by = (v: string) => ws.filter((w) => w.view === v).length;
    line(
      `  Round ${r}: ${ws.length} (${Object.keys(VIEW_WORDS)
        .filter((v) => by(v))
        .map((v) => `${VIEW_WORDS[v]} ${by(v)}`)
        .join(", ") || "none"}); in pixels ${ws.filter((w) => w.mode === 0).length}, lines ${ws.filter((w) => w.mode === 1).length}, pages ${ws.filter((w) => w.mode === 2).length}`,
    );
  }
  const late = sorted(wheels.flatMap((w) => (w.late === null ? [] : [w.late])));
  line(late.length ? `  Heard late by the page: median ${ms(quantile(late, 0.5))}, 95th percentile ${ms(quantile(late, 0.95))}, slowest ${ms(late.at(-1)!)}` : "  Heard late by the page: unknown (the events' times aren't the page's clock)");
  line(`  cancelable: false: ${rounds.map((r) => `round ${r} ${inRound(wheels, r).filter((w) => !w.cancelable).length} of ${inRound(wheels, r).length}`).join(", ")}`);
  line(`  Cancelled: ${rounds.map((r) => `round ${r} ${inRound(wheels, r).filter((w) => w.cancelled).length}`).join(", ")}`);
  line(`  With both deltas not 0: ${rounds.map((r) => `round ${r} ${pct(inRound(wheels, r).filter(both).length, inRound(wheels, r).length)}`).join(", ")}`);
  line(`  With fractions (a delta not a whole number): ${rounds.map((r) => `round ${r} ${pct(inRound(wheels, r).filter(fraction).length, inRound(wheels, r).length)}`).join(", ")}`);
  line(`  With Ctrl or ⌘ (Chrome's pinch, or ⌘-scroll): ${wheels.filter((w) => /ctrl|meta/.test(w.keys)).length}`);
  line();

  const gaps = wheels.flatMap((w) => (w.gap === null ? [] : [w.gap]));
  line("Gaps between wheel events, within a step");
  const counted = BUCKETS.map((lo, i) => {
    const hi = BUCKETS[i + 1];
    return [hi === undefined ? `${lo}+ ms` : `${lo}–${hi} ms`, gaps.filter((g) => g >= lo && (hi === undefined || g < hi)).length] as const;
  });
  const most = Math.max(1, ...counted.map(([, n]) => n));
  for (const [range, n] of counted) line(`  ${range.padStart(11)} ${String(n).padStart(6)} ${"#".repeat(Math.ceil((40 * n) / most))}`);
  const sg = sorted(gaps);
  line(sg.length ? `  Median ${ms(quantile(sg, 0.5))}, 90th percentile ${ms(quantile(sg, 0.9))}, 99th percentile ${ms(quantile(sg, 0.99))}` : "  None yet.");
  line();

  line("Swipes in each step: a swipe is a run of wheel events less than G ms apart");
  line(`| Step | events | both ways | fractions | ${GS.map((g) => `G = ${g} ms`).join(" | ")} |`);
  line(`|---|---|---|---|${GS.map(() => "---").join("|")}|`);
  for (const r of rounds)
    STEPS.forEach((_, i) => {
      const at = `${r}${i + 1}`;
      const ws = wheels.filter((w) => w.at === at);
      line(`| ${stepName(at)} | ${ws.length} | ${pct(ws.filter(both).length, ws.length)} | ${pct(ws.filter(fraction).length, ws.length)} | ${GS.map((g) => swipes(ws, g).length).join(" | ")} |`);
    });
  line();

  line(`After each flick: how long wheel events kept coming once the fastest had passed (swipes split at gaps over ${SPLIT} ms)`);
  for (const r of rounds)
    STEPS.forEach((s, i) => {
      if (!/^flick/.test(s.words)) return;
      const at = `${r}${i + 1}`;
      const runs = swipes(
        wheels.filter((w) => w.at === at),
        SPLIT,
      ).filter((run) => run.length >= 3);
      const said = runs.slice(0, 4).map((run) => {
        const peak = run.reduce((a, w, j) => (speed(w) > speed(run[a]) ? j : a), 0);
        const tail = run.slice(peak + 1);
        const when = (w: Wheel) => (w.late === null ? w.t : w.t - w.late);
        const last = run.at(-1)!;
        return `${ms(tail.length ? when(last) - when(run[peak]) : 0)} (${tail.length} events after the fastest, ${pct(tail.filter(both).length, tail.length)} both ways, the last ${num(speed(last))} px)`;
      });
      line(`  ${stepName(at)}: ${runs.length ? `${runs.length} swipe${runs.length === 1 ? "" : "s"}: ${said.join("; ")}` : "none heard"}`);
    });
  line();

  line("Round B's scrolls: the probe moves the view itself, so every scroll should be its own");
  const bs = inRound(scrolls, "B");
  for (const v of ["arrangement", "graph", "notes", "beat"]) {
    const of = bs.filter((s) => s.view === v);
    if (of.length) line(`  ${VIEW_WORDS[v]}: ${of.length}, of which not the probe's: ${of.filter((s) => !s.ours).length}`);
  }
  line(`  In all, not the probe's: ${bs.filter((s) => !s.ours).length}`);
  const as = inRound(scrolls, "A");
  line(`Round A's scrolls: ${["arrangement", "graph", "notes", "beat"].map((v) => `${VIEW_WORDS[v]} ${as.filter((s) => s.view === v).length}`).join(", ")}`);
  line();

  const scales = gestures.map((g) => g.scale).filter(Number.isFinite);
  const kind = (type: string) => gestures.filter((g) => g.type === type).length;
  line(
    gestures.length
      ? `Gesture events: ${gestures.length} (${kind("start")} start, ${kind("change")} change, ${kind("end")} end); scale from ${num(Math.min(...scales))} to ${num(Math.max(...scales))}`
      : "Gesture events: none (no gesturestart, gesturechange or gestureend)",
  );
  line();

  line("Scrollbar while the view moved (round B)");
  STEPS.forEach((s, i) => s.swipe && line(`  ${stepName(`B${i + 1}`)}: ${answers[`B${i + 1}`] ?? "not answered"}`));
  line();

  const raw = [
    ...wheels.map((w) => ({ t: w.t, s: `w ${num(w.t, 1)} ${w.at} ${w.view} ${num(w.dx)} ${num(w.dy)} ${w.mode} ${+w.cancelable} ${+w.cancelled} ${w.gap === null ? "-" : num(w.gap, 1)} ${w.late === null ? "-" : num(w.late, 1)} ${w.keys}` })),
    ...gestures.map((g) => ({ t: g.t, s: `g ${num(g.t, 1)} ${g.at} ${g.type} ${num(g.scale)} ${num(g.x, 1)}` })),
    ...scrolls.map((c) => ({ t: c.t, s: `s ${num(c.t, 1)} ${c.at} ${c.view} ${num(c.left, 1)} ${num(c.top, 1)} ${+c.ours}` })),
  ].sort((a, b) => a.t - b.t);
  line(`Raw events, the last ${Math.min(RAW, raw.length)} of ${raw.length}, oldest first; times in ms from the probe's start`);
  line("  w time step view deltaX deltaY deltaMode cancelable cancelled gap late keys");
  line("  g time step start|change|end scale clientX");
  line("  s time step view scrollLeft scrollTop the-probe's-own");
  line("  view: arrangement (the timeline), graph, notes, beat (the beat grid), other; 1 is yes, 0 no");
  for (const r of raw.slice(-RAW)) line(r.s);
  return out.join("\n");
}

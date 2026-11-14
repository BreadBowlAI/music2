// What the zoom bench's two drivers share (the zoom note's §6 step 1): `zoom-bench.mjs` in Chrome and
// the WebKit tool, `webkit/webkit.mjs`. The plan of runs, and the page's side of a run and of the
// tables, as functions that hold everything they use: the drivers send them to the page as text.
// The measuring itself is the app's zoom bench (`web/src/lib/zoomBench.ts`, `m2canvas.bench()`), the
// one the swipe probe's last step uses, so all of them measure the same way.

/** Each gesture's runs, one after another. */
export const REPS = 3;

/**
 * The runs on a song, in order: the timeline's pinch and ⌘-scroll in turn, then the Graph's pinch.
 * A run's `view` is where it happens, its `kind` the gesture: the owner's A13 pinch, or ⌘-scroll.
 */
export const PLAN = [
  ...Array.from({ length: REPS }, (_, r) => [
    { name: "timeline pinch", rep: r + 1, view: "arrangement", kind: "pinch" },
    { name: "timeline ⌘-scroll", rep: r + 1, view: "arrangement", kind: "scroll" },
  ]).flat(),
  ...Array.from({ length: REPS }, (_, r) => ({ name: "Graph pinch", rep: r + 1, view: "graph", kind: "pinch" })),
];

/** The gestures in the order of a table's rows. */
export const GESTURES = ["timeline pinch", "timeline ⌘-scroll", "Graph pinch"];

/**
 * In the page: one run of the plan. Where every run starts: the timeline at 24 px a beat from the
 * song's start, or the Song's graph at Fit (zoom 1); the point the bench replays at, the swipe
 * probe's (0.4 across and 0.3 down the timeline's clear part, the Graph's middle). Gives what the
 * bench measured. Uses only what the page gives suites: `m2canvas` and the DOM.
 */
export async function zoomRun(step) {
  const bench = await window.m2canvas.bench();
  const cam = window.m2canvas.camera;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frames = (n) =>
    new Promise((r) => {
      const one = (k) => (k ? requestAnimationFrame(() => one(k - 1)) : r());
      one(n);
    });
  const until = async (f, what) => {
    for (const t = performance.now(); !f(); await sleep(50)) if (performance.now() - t > 20000) throw new Error(`zoomRun: ${what} never came`);
  };
  // A click as a hand's: pointer down and up, then the click.
  const press = (el) => {
    for (const t of ["pointerdown", "pointerup"]) el.dispatchEvent(new PointerEvent(t, { bubbles: true, button: 0, isPrimary: true }));
    el.click();
  };
  const graph = step.view === "graph";
  const name = graph ? "graph:song" : "arrangement";
  const shows = () => document.querySelector("[data-testid=graph] [data-module]")?.dataset.module;
  if (graph) {
    const tab = document.querySelector('[data-view-tab="graph"]');
    if (tab && tab.getAttribute("aria-selected") !== "true") press(tab);
    if (shows() !== "song") press(document.querySelector('[data-module-tab="song"] [data-testid=track-label]') ?? document.querySelector('[data-module-tab="song"]'));
    await until(() => shows() === "song" && cam.now(name) && document.querySelector('[aria-label="Fit to panel"]'), "the Song's graph");
    press(document.querySelector('[aria-label="Fit to panel"]'));
  }
  const box = document.querySelector(`[data-view-box="${graph ? "graph" : "arrangement"}"]`);
  const r = box.getBoundingClientRect();
  if (!graph) {
    const z = cam.now(name).zoom;
    if (Math.abs(z - 24) > 1e-6) {
      // To 24 px a beat by a pinch: the camera's own way in.
      for (const [type, scale] of [["gesturestart", 1], ["gesturechange", 24 / z], ["gestureend", 24 / z]]) {
        const e = new UIEvent(type, { bubbles: true, cancelable: true });
        for (const [k, v] of Object.entries({ scale, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 })) Object.defineProperty(e, k, { value: v });
        box.dispatchEvent(e);
      }
      await frames(3);
    }
    box.scrollLeft = 0;
    box.scrollTop = 0;
  }
  await frames(3);
  await sleep(800);
  const m = cam.now(name)?.margins ?? { left: 0, top: 0, right: 0, bottom: 0 };
  const [fx, fy] = graph ? [0.5, 0.5] : [0.4, 0.3];
  const x = Math.round(r.left + box.clientLeft + m.left + (box.clientWidth - m.left - m.right) * fx);
  const y = Math.round(r.top + box.clientTop + m.top + (box.clientHeight - m.top - m.bottom) * fy);
  const gesture = step.kind === "scroll" ? { kind: "scroll", deltas: bench.CMD_SCROLL } : { kind: "pinch", scales: bench.A13 };
  return bench.replay(x, y, gesture, { zoom: () => cam.now(name)?.zoom ?? null });
}

/** In the page: the zoom bench's table of `rows` (each [name, runs]), headed `first`. */
export async function zoomTable(first, rows) {
  return (await window.m2canvas.bench()).table(first, rows);
}

/** Each gesture's runs on a song, as a table's rows: "timeline pinch, 3 runs". */
export const rowsOf = (results) => GESTURES.map((g) => [`${g}, ${results.filter((r) => r.name === g).length} runs`, results.filter((r) => r.name === g).map((r) => r.run)]);

/**
 * What proves a gesture's runs measured (no speed is judged): each run saw frames while its gesture
 * lasted, every part was timed in every one of them and the input was, and the zoom went out and came
 * back to where it started. Gives [frames-and-parts ok, zoom ok, what was seen].
 */
export function measured(runs) {
  const during = (run) => run.frames.filter((f) => f.at >= 0 && f.at <= run.sent + 34);
  const parts = ["work", "script", "forced", "layout", "paint", "input"];
  const seen = runs.map((run) => {
    const fs = during(run);
    return {
      frames: fs.length,
      untimed: fs.filter((f) => parts.some((k) => typeof f[k] !== "number" || !Number.isFinite(f[k]))).length,
      input: Math.round(fs.reduce((a, f) => a + f.input, 0) * 10) / 10,
      zoom: run.zoom,
    };
  });
  const framesOk = seen.length > 0 && seen.every((s) => s.frames >= 5 && s.untimed === 0 && s.input > 0);
  const zoomOk = seen.length > 0 && seen.every(({ zoom: z }) => z.from > 0 && z.min < z.from * 0.8 && Math.abs(z.to - z.from) <= z.from * 0.01);
  return [framesOk, zoomOk, seen];
}

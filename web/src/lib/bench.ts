// The pictures' latency in the window it runs in (CARD-75): opened with `?bench=pictures` (the
// desktop app under Xvfb, `MUSIC2_DEV_URL=http://127.0.0.1:4100/?bench=pictures`), it shows the
// first card with a picture, then moves the value its first dot moves, as a drag does (a preview of
// the gesture's lease), 200 times, a frame apart; each time it waits for the picture drawn again
// to be on screen (its `pictures` message, React's render, and the paint after it) and times it.
// What it measured goes to the shell's log (`bench …`), or the console in a browser. Nothing else
// in the app reads it.

import { on } from "./events";
import { graphApi, type ViewNode } from "./graph";
import { native, nativeAction } from "./native";
import { useStore } from "./store";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** The next frame: what React drew is on screen once it has painted. */
const frame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

async function until<T>(f: () => T | null | undefined, ms = 30000): Promise<T> {
  const t = performance.now();
  for (;;) {
    const x = f();
    if (x) return x;
    if (performance.now() - t > ms) throw new Error("the bench found nothing to time");
    await sleep(50);
  }
}

/** Run the bench, and say what it measured. */
export async function pictures() {
  const say = (s: string) => (native ? void nativeAction(`bench ${s}`) : console.log(`bench ${s}`));
  say(JSON.stringify({ started: location.href }));
  try {
    const graph = await until(() => useStore.getState().graph);
    const found = await until(() => {
      for (const m of useStore.getState().graph?.view.modules ?? []) {
        for (const n of m.nodes as ViewNode[]) {
          for (const v of n.views ?? []) {
            if (v.kind !== "picture") continue;
            const value = v.values.find((x) => x.knob && typeof x.value === "number");
            if (value) return { module: m.key, node: n, view: v.name, value };
          }
        }
      }
      return null;
    });
    // Show its card: its track, and Device.
    document.querySelector<HTMLElement>(`[data-module-tab="${CSS.escape(found.module)}"] [data-testid=track-label]`)?.click();
    document.querySelector<HTMLElement>('[data-view-tab="graph"]')?.click();
    const shown = await until(() => document.querySelector<HTMLElement>(`[data-picture="${CSS.escape(found.view)}"][data-drawn]`));
    say(JSON.stringify({ shown: true, visible: document.visibilityState }));
    const knob = found.value.knob!;
    const start = found.value.value as number;
    const gesture = `bench-${Date.now()}`;
    const took: number[] = [];
    const heard: number[] = [];
    // Each sample's own: since the message before, and the meters messages while it waited (review G6).
    const since: number[] = [];
    const meters: number[] = [];
    let last = performance.now();
    let metered = 0;
    const offMeters = on("meters", () => void (metered += 1));
    let lost = 0;
    for (let i = 1; i <= 200; i++) {
      // A new value each time, as a drag gives: a 1% step up, forty times, then back down at once.
      const value = Math.round(start * 1.01 ** ((i % 40) + 1) * 100) / 100;
      const drawn = Number(shown.dataset.drawn ?? 0);
      const t0 = performance.now();
      let at = 0;
      metered = 0;
      const came = new Promise<void>((r) => {
        const off = on<{ t: "pictures"; pictures: { node: string; view: string }[] }>("pictures", (msg) => {
          if (!msg.pictures.some((p) => p.node === found.node.id && p.view === found.view)) return;
          off();
          at = performance.now();
          r();
        });
      });
      await graphApi.preview(graph.session, graph.view.revision, knob, gesture, value);
      await Promise.race([came, sleep(1000)]);
      const drew = await until(() => Number(shown.dataset.drawn ?? 0) > drawn || null, 1000).catch(() => null);
      await frame();
      if (!at || !drew) {
        lost++;
        continue;
      }
      heard.push(at - t0);
      took.push(performance.now() - t0);
      since.push(t0 - last);
      meters.push(metered);
      last = at;
      await sleep(16);
    }
    offMeters();
    await graphApi.cancel(graph.session, knob, gesture).catch(() => {});
    const q = (xs: number[], p: number) => {
      const s = [...xs].sort((a, b) => a - b);
      return Math.round(s[Math.min(s.length - 1, Math.floor(s.length * p))] * 100) / 100;
    };
    const both = (xs: number[]) => ({ median_ms: q(xs, 0.5), p99_ms: q(xs, 0.99), max_ms: q(xs, 1) });
    const frames = (xs: number[]) => Math.ceil(q(xs, 0.99) / (1000 / 60));
    say(JSON.stringify({ n: took.length, lost, heard: both(heard), on_screen: both(took), frames_p99: frames(took), since_last_ms: both(since), meters_while: both(meters), agent: navigator.userAgent }));
  } catch (e) {
    say(JSON.stringify({ error: String(e) }));
  }
}

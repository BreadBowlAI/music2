// The web engine's probe in a page (docs/designs/live/web-engine.md): every case on the page's own
// thread, in a dedicated Worker, then in an AudioWorklet of an OfflineAudioContext, then of a
// real-time AudioContext.
import { flushes, runCase } from "./host.mjs";
import { check } from "./math.mjs";

// A result as the page reports it, its bytes hashed.
const report = async (data) =>
  data.error
    ? { id: data.id, error: data.error }
    : { id: data.id, out: await hex(data.out), in: await hex(data.ins), bytes: data.out.length, nonfinite: data.nonfinite, sounding: data.sounding, us_per_block: data.us, flushes: data.flushes };

async function inWorker(wasm, cases, blocks, waitMs) {
  const w = new Worker(new URL("worker.mjs", import.meta.url), { type: "module" });
  const got = [];
  const all = new Promise((resolve) => {
    w.onmessage = async ({ data }) => {
      got.push(await report(data));
      if (got.length === cases.length) resolve();
    };
  });
  w.postMessage({ wasm, cases, blocks });
  await Promise.race([all, new Promise((resolve) => setTimeout(resolve, waitMs))]);
  w.terminate();
  return { results: got };
}

const hex = async (bytes) => [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");

async function inWorklet(ctx, wasm, cases, blocks, waitMs) {
  await ctx.audioWorklet.addModule(new URL("worklet.mjs", import.meta.url));
  const node = new AudioWorkletNode(ctx, "probe", { processorOptions: { wasm, cases, blocks, tiny: 2 ** -1022 } });
  const got = [];
  let scope = null;
  const all = new Promise((resolve) => {
    node.port.onmessage = async ({ data }) => {
      if (data.scope) return void (scope = data.scope);
      got.push(await report(data));
      if (got.length === cases.length) resolve();
    };
  });
  node.connect(ctx.destination);
  const timeout = new Promise((resolve) => setTimeout(resolve, waitMs));
  if (ctx.startRendering) ctx.startRendering();
  else await ctx.resume();
  await Promise.race([all, timeout]);
  if (ctx.close) await ctx.close().catch(() => {});
  return { scope, state: ctx.state, results: got };
}

export async function run() {
  const { blocks, cases } = await (await fetch("/out/cases.json")).json();
  const names = [...new Set(cases.map((c) => c.wasm))];
  const wasm = {};
  for (const n of names) wasm[n] = await (await fetch(`/${n}`)).arrayBuffer();
  const modules = new Map(await Promise.all(names.map(async (n) => [n, await WebAssembly.compile(wasm[n])])));
  const main = [];
  for (const c of cases) {
    const id = `${c.package}:${c.kernel}`;
    try {
      const r = runCase(modules.get(c.wasm), c, blocks, () => performance.now());
      main.push({ id, out: await hex(r.out), in: await hex(r.ins), bytes: r.out.length, nonfinite: r.nonfinite, sounding: r.sounding, us_per_block: r.us });
    } catch (e) {
      main.push({ id, error: String(e) });
    }
  }
  const libm = await fetch("/out/libm.json");
  const math = libm.ok ? check(await libm.json(), Math) : null;
  const page = { isolated: crossOriginIsolated, flushes: flushes(2 ** -1022), agent: navigator.userAgent, math };
  const worker = await inWorker(wasm, cases, blocks, 120000);
  const offline = await inWorklet(new OfflineAudioContext({ numberOfChannels: 1, length: 128 * (cases.length + 8), sampleRate: 48000 }), wasm, cases, blocks, 120000);
  let realtime;
  try {
    realtime = await inWorklet(new AudioContext({ sampleRate: 48000 }), wasm, cases, blocks, 120000);
  } catch (e) {
    realtime = { error: String(e) };
  }
  return { page, main, worker, offline, realtime };
}

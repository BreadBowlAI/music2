// The web engine's probe in a dedicated Worker (docs/designs/live/web-engine.md): every case on a
// thread of the page's own, not the audio thread.
import { flushes, runCase } from "./host.mjs";

onmessage = ({ data: { wasm, cases, blocks } }) => {
  const modules = new Map(Object.entries(wasm).map(([k, bytes]) => [k, new WebAssembly.Module(bytes)]));
  for (const c of cases) {
    const id = `${c.package}:${c.kernel}`;
    try {
      const r = runCase(modules.get(c.wasm), c, blocks, () => performance.now());
      postMessage({ id, flushes: flushes(2 ** -1022), out: r.out.slice(), ins: r.ins.slice(), nonfinite: r.nonfinite, sounding: r.sounding, us: r.us });
    } catch (e) {
      postMessage({ id, error: String(e) });
    }
  }
};

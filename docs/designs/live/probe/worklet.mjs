// The web engine's probe in an AudioWorklet (docs/designs/live/web-engine.md): each process() call
// runs one kernel's case on the audio rendering thread, where a browser may set its own
// floating-point mode, and posts its bytes to the page.
import { flushes, runCase } from "./host.mjs";

class Probe extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const { wasm, cases, blocks, tiny } = options.processorOptions;
    this.modules = new Map(Object.entries(wasm).map(([k, bytes]) => [k, new WebAssembly.Module(bytes)]));
    Object.assign(this, { cases, blocks, tiny, next: 0 });
    // The clock a worklet has: `performance` is not in its scope in every browser.
    this.now = globalThis.performance ? () => globalThis.performance.now() : () => Date.now();
    this.port.postMessage({
      scope: { performance: !!globalThis.performance, sab: typeof SharedArrayBuffer === "function", isolated: !!globalThis.crossOriginIsolated },
    });
  }

  process() {
    if (this.next < this.cases.length) {
      const c = this.cases[this.next++];
      const id = `${c.package}:${c.kernel}`;
      try {
        const r = runCase(this.modules.get(c.wasm), c, this.blocks, this.now);
        this.port.postMessage({ id, flushes: flushes(this.tiny), out: r.out.slice(), ins: r.ins.slice(), nonfinite: r.nonfinite, sounding: r.sounding, us: r.us });
      } catch (e) {
        this.port.postMessage({ id, error: String(e) });
      }
    }
    return true;
  }
}

registerProcessor("probe", Probe);

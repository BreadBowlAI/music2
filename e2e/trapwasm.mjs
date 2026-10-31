// A wasm module with one exported function made to trap at once: `unreachable`, as a kernel with a
// bug does. The browser suites use it on a copy of std, so a sound really crashes while the song
// plays (sound-stops). Only what that needs: a new function of the export's type, the export pointed
// at it. Numeric value types only (what std's Rust build makes).
//
//   import { trapping } from "./trapwasm.mjs";
//   const bytes = trapping(fs.readFileSync("std/kernels/std.wasm"), "noise_process");
//
// `NOISE_HATS` is a `tracks/hats.ts` for a copy of first-song whose hats play on std's `noise`, as
// @m2/synths' simple hat did before wave 3: first-song's own hats play One drum, whose kernel is
// @m2/drums' and the kick's too, so a trap there would stop both.
export const NOISE_HATS = `import { hz, ms, poly, track } from "@m2/core";
import { adsr, noise, svf, vca } from "std/dsp";
import { scale } from "std/math";

export default track("hats", { name: "Hats" }, ({ lane }) =>
  poly(
    lane.notes("hits"),
    (n) => vca(svf.highpass(noise(7), hz(7500), 0.9), scale(adsr(n.gate, { attack: ms(1), decay: ms(45), sustain: 0, release: ms(30) }), scale(n.velocity, 0.5))),
    { voices: 4 },
  ),
);
`;

const uleb = (b, at) => {
  let n = 0, shift = 0, x;
  do {
    x = b[at++];
    n += (x & 0x7f) * 2 ** shift;
    shift += 7;
  } while (x & 0x80);
  return [n, at];
};
const enc = (n) => {
  const out = [];
  do {
    let x = n % 128;
    n = Math.floor(n / 128);
    if (n) x |= 0x80;
    out.push(x);
  } while (n);
  return out;
};
const name = (b, at) => {
  const [len, p] = uleb(b, at);
  return [Buffer.from(b.subarray(p, p + len)).toString("utf8"), p + len];
};

/** `wasm` with export `fn` (a function) calling a new function of its type that traps. */
export function trapping(wasm, fn) {
  const b = Uint8Array.from(wasm);
  if (b.length < 8 || b[0] !== 0 || b[1] !== 0x61 || b[2] !== 0x73 || b[3] !== 0x6d) throw new Error("not a wasm module");
  const sections = [];
  for (let at = 8; at < b.length; ) {
    const id = b[at];
    const [size, p] = uleb(b, at + 1);
    sections.push({ id, body: b.subarray(p, p + size) });
    at = p + size;
  }
  const sec = (id) => sections.find((s) => s.id === id) ?? { id, body: Uint8Array.of(0) };
  // Functions imported come first in the index space.
  let imported = 0;
  {
    const s = sec(2).body;
    let [n, p] = uleb(s, 0);
    for (let i = 0; i < n; i++) {
      [, p] = name(s, p);
      [, p] = name(s, p);
      const kind = s[p++];
      const limits = () => {
        const flag = s[p++];
        [, p] = uleb(s, p);
        if (flag & 1) [, p] = uleb(s, p);
      };
      if (kind === 0) {
        imported++;
        [, p] = uleb(s, p);
      } else if (kind === 1) {
        p++;
        limits();
      } else if (kind === 2) limits();
      else if (kind === 3) p += 2;
      else throw new Error(`import kind ${kind}`);
    }
  }
  // Each defined function's type.
  const funcs = sec(3).body;
  const [defined, fstart] = uleb(funcs, 0);
  const typeOf = [];
  for (let i = 0, p = fstart; i < defined; i++) {
    let t;
    [t, p] = uleb(funcs, p);
    typeOf.push(t);
  }
  // The export, and its function's type.
  const exps = sec(7).body;
  let [n, p] = uleb(exps, 0);
  const out = [...enc(n)];
  const at = imported + defined;
  let type = -1;
  for (let i = 0; i < n; i++) {
    const start = p;
    let e, idx;
    [e, p] = name(exps, p);
    const kind = exps[p++];
    const before = p;
    [idx, p] = uleb(exps, p);
    if (e === fn && kind === 0) {
      if (idx < imported) throw new Error(`${fn} is an import`);
      type = typeOf[idx - imported];
      out.push(...exps.subarray(start, before), ...enc(at));
    } else out.push(...exps.subarray(start, p));
  }
  if (type < 0) throw new Error(`no function export ${fn}`);
  // The new function: no locals, `unreachable`, `end`.
  const code = sec(10).body;
  const [bodies, cstart] = uleb(code, 0);
  const replace = {
    3: [...enc(defined + 1), ...funcs.subarray(fstart), ...enc(type)],
    7: out,
    10: [...enc(bodies + 1), ...code.subarray(cstart), 3, 0x00, 0x00, 0x0b],
  };
  const parts = [b.subarray(0, 8)];
  for (const s of sections) {
    const body = replace[s.id] ? Uint8Array.from(replace[s.id]) : s.body;
    parts.push(Uint8Array.of(s.id, ...enc(body.length)), body);
  }
  return Buffer.concat(parts);
}

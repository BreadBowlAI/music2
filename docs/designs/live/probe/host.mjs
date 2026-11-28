// The web engine's probe (docs/designs/live/web-engine.md): the kernel ABI's host, in JavaScript,
// for a browser's WebAssembly (and Node's). It makes each kernel's state as wasmtime.rs does, feeds
// it the same inputs, block by block, and gives back the same bytes: each output's channels, an
// events output's count and events, and k_active, after each block. No Node or DOM API here, so a
// page, a worker and an AudioWorklet can all import it.

const BLOCK = 128;
const PORT_BYTES = 2 * BLOCK * 4;
const NOTES_HEAD = 16;
const EVENT_BYTES = 24;
const SETTINGS_BYTES = 60;
const NOTE_CAP = 16;
const RATE = 48000;
const AUDIO = 2;
const NAN = 0x7fc00000;

const f32bits = (() => {
  const f = new Float32Array(1);
  const u = new Uint32Array(f.buffer);
  return (v) => ((f[0] = v), u[0]);
})();

// The notes every events input gets: [block, frame, kind, id, three values' bits].
const NOTES = [
  [10, 5, 0, 1, [f32bits(60), f32bits(0.75), NAN]],
  [100, 17, 1, 1, [f32bits(0.5), 0, 0]],
  [120, 0, 0, 2, [f32bits(67), f32bits(0.5), NAN]],
  [121, 64, 0, 3, [f32bits(71), f32bits(1), NAN]],
  [260, 127, 1, 2, [0, 0, 0]],
  [261, 0, 1, 3, [0, 0, 0]],
];

// One noise stream per port channel: the same LCG as wasmtime.rs's.
function lcg(port, ch) {
  let x = Math.imul(port * 16 + ch + 1, 2654435761 | 0) >>> 0;
  return () => (x = (Math.imul(x, 1664525) + 1013904223) >>> 0);
}

function noise(next, block, subnormals) {
  const x = next();
  if (block < 300) return f32bits(((x >>> 8) / 8388608 - 1) * 0.5);
  if (block < 550 || !subnormals) return 0;
  return ((x >>> 9) | ((x & 1) << 31)) >>> 0;
}

const gate = (block) => ((block >= 20 && block < 200) || (block >= 320 && block < 340) ? f32bits(1) : 0);
const sweep = (frame) => f32bits(50 + (frame % 48000) * 0.25);
const hits = (frame) => (frame % (50 * BLOCK) === 10 * BLOCK ? f32bits(1) : 0);
const beats = (frame) => f32bits(frame / 24000);

// A growing byte buffer.
class Bytes {
  constructor(n = 1 << 16) {
    this.buf = new Uint8Array(n);
    this.len = 0;
  }
  push(src) {
    if (this.len + src.length > this.buf.length) {
      const b = new Uint8Array(Math.max(this.buf.length * 2, this.len + src.length));
      b.set(this.buf.subarray(0, this.len));
      this.buf = b;
    }
    this.buf.set(src, this.len);
    this.len += src.length;
  }
  u32(v) {
    this.push(new Uint8Array(new Uint32Array([v >>> 0]).buffer));
  }
  bytes() {
    return this.buf.subarray(0, this.len);
  }
}

const roundUp = (n, m) => Math.ceil(n / m) * m;

// Make state `c` of `module`, as the host does (sandbox.rs), then reset it once (the prewarm).
function birth(module, c) {
  let memory;
  const imports = {
    m2: {
      asset_read(_h, _c, _f, dst, frames) {
        new Uint8Array(memory.buffer, dst, 4 * frames).fill(0);
        return 0;
      },
    },
  };
  const inst = new WebAssembly.Instance(module, WebAssembly.Module.imports(module).length ? imports : {});
  const x = inst.exports;
  memory = x.memory;
  const f = (n) => x[`${c.kernel}_${n}`];
  const inputs = c.ports.filter((p) => p.input).length;
  const outputs = c.ports.length - inputs;
  const declared = f("ports")() >>> 0;
  if (declared !== ((inputs << 8) | outputs)) throw new Error(`${c.kernel}: ports ${declared.toString(16)}`);
  const caps = c.ports.map((p) => (p.kind !== "events" ? 0 : p.input ? NOTE_CAP : NOTE_CAP * c.fan));
  const lens = caps.map((cap) => (cap === 0 ? PORT_BYTES : roundUp(NOTES_HEAD + cap * EVENT_BYTES, 16)));
  const n = c.ports.length;
  const settingsAt = lens.reduce((a, b) => a + b, 0);
  const listAt = settingsAt + SETTINGS_BYTES;
  const valuesAt = roundUp(listAt + 4 * n, 8);
  const tableAt = roundUp(valuesAt, 8);
  const channels = c.ports.map((p) => p.channels);
  const tableLen = channels.reduce((a, b) => a + b, 0);
  const bytes = tableAt + 4 * tableLen;
  const block = x.m2_alloc(bytes, 16) >>> 0;
  if (block === 0) throw new Error("no room");
  const bufs = [];
  let at = block;
  for (const len of lens) (bufs.push(at), (at += len));
  const io = [];
  channels.forEach((ch, p) => {
    for (let k = 0; k < ch; k++) io.push(bufs[p] + k * BLOCK * 4);
  });
  const words = [SETTINGS_BYTES, f32bits(RATE), BLOCK, 0, AUDIO, n === 0 ? 0 : block + listAt, n, 0, 0, 0, 0, 0, 0, 0, 0, ...channels];
  const dv = new DataView(memory.buffer);
  new Uint8Array(memory.buffer, block + settingsAt, valuesAt - settingsAt).fill(0);
  words.forEach((w, i) => dv.setUint32(block + settingsAt + 4 * i, w >>> 0, true));
  io.forEach((w, i) => dv.setUint32(block + tableAt + 4 * i, w, true));
  caps.forEach((cap, p) => {
    if (cap > 0) [0, cap, bufs[p] + NOTES_HEAD, 0].forEach((w, i) => dv.setUint32(bufs[p] + 4 * i, w, true));
  });
  const handle = f("new")(block + settingsAt) >>> 0;
  if (handle === 0) throw new Error(`${c.kernel}_new made no state`);
  f("reset")(handle);
  return { memory, process: f("process"), reset: f("reset"), active: f("active"), handle, io: block + tableAt, bufs, caps };
}

// Write block `b`'s inputs.
function feed(r, c, b, rngs, ins) {
  const dv = new DataView(r.memory.buffer);
  const u8 = new Uint8Array(r.memory.buffer);
  c.ports.forEach((port, p) => {
    const at = r.bufs[p];
    if (port.kind === "events") {
      if (port.input) {
        let count = 0;
        for (const [nb, frame, kind, id, v] of NOTES) {
          if (nb === b && count < r.caps[p]) {
            const e = at + NOTES_HEAD + count * EVENT_BYTES;
            [kind, frame, id, v[0], v[1], v[2]].forEach((w, i) => dv.setUint32(e + 4 * i, w >>> 0, true));
            ins?.push(u8.slice(e, e + EVENT_BYTES));
            count++;
          }
        }
        dv.setUint32(at, count, true);
      } else {
        dv.setUint32(at, 0, true);
      }
      return;
    }
    if (!port.input) return;
    for (let ch = 0; ch < port.channels; ch++) {
      const o = at + ch * BLOCK * 4;
      for (let i = 0; i < BLOCK; i++) {
        let bits;
        switch (port.gen) {
          case "noise":
            bits = noise(rngs[p][ch], b, c.subnormals);
            break;
          case "gate":
            bits = gate(b);
            break;
          case "sweep":
            bits = sweep(b * BLOCK + i);
            break;
          case "hits":
            bits = hits(b * BLOCK + i);
            break;
          case "beats":
            bits = beats(b * BLOCK + i);
            break;
          default:
            bits = port.bits;
        }
        dv.setUint32(o + 4 * i, bits, true);
      }
      ins?.push(u8.slice(o, o + BLOCK * 4));
    }
  });
}

// Append block `b`'s outputs, and count the samples that aren't finite, and those that aren't 0.
function take(r, c, out, counts) {
  const active = r.active(r.handle) >>> 0;
  const dv = new DataView(r.memory.buffer);
  const u8 = new Uint8Array(r.memory.buffer);
  c.ports.forEach((port, p) => {
    if (port.input) return;
    const at = r.bufs[p];
    if (port.kind === "events") {
      const count = Math.min(dv.getUint32(at, true), r.caps[p]);
      out.u32(count);
      out.push(u8.subarray(at + NOTES_HEAD, at + NOTES_HEAD + count * EVENT_BYTES));
      return;
    }
    const n = port.channels * BLOCK;
    for (let i = 0; i < n; i++) {
      const v = dv.getFloat32(at + 4 * i, true);
      if (!Number.isFinite(v)) counts.nonfinite++;
      else if (v !== 0) counts.sounding++;
    }
    out.push(u8.subarray(at, at + 4 * n));
  });
  out.u32(active);
}

const streams = (c) => c.ports.map((_, p) => [lcg(p, 0), lcg(p, 1)]);

// Run case `c` of `module` for `blocks` blocks; then time `blocks` calls with block 100's inputs
// held, the best of three, by `now()` (ms). Gives the output and input bytes and the time.
export function runCase(module, c, blocks, now) {
  const r = birth(module, c);
  const rngs = streams(c);
  const out = new Bytes();
  const ins = new Bytes();
  const counts = { nonfinite: 0, sounding: 0 };
  for (let b = 0; b < blocks; b++) {
    feed(r, c, b, rngs, ins);
    r.process(r.handle, r.io, 0, BLOCK);
    take(r, c, out, counts);
  }
  let best = Infinity;
  if (now) {
    feed(r, c, 100, streams(c), null);
    for (let k = 0; k < 3; k++) {
      r.reset(r.handle);
      const t = now();
      for (let b = 0; b < blocks; b++) r.process(r.handle, r.io, 0, BLOCK);
      best = Math.min(best, ((now() - t) * 1000) / blocks);
    }
  }
  return { out: out.bytes(), ins: ins.bytes(), nonfinite: counts.nonfinite, sounding: counts.sounding, us: Number.isFinite(best) ? Math.round(best * 100) / 100 : null };
}

// Whether this thread flushes subnormals to zero: `tiny` is 2^-1022, the least normal f64, passed
// in so nothing folds it. Halved, it is a subnormal, unless the thread flushes.
export const flushes = (tiny) => tiny / 2 === 0;

// Song code's Math in the app is the libm crate (PLAT-4): `check(table, Math)` holds a JavaScript
// engine's own Math against the rows wasmtime.rs --libm wrote, bit for bit. No Node or DOM API here.

const f64 = (hex) => new Float64Array(new BigUint64Array([BigInt(`0x${hex}`)]).buffer)[0];
const bits = (x) => new BigUint64Array(new Float64Array([x]).buffer)[0].toString(16).padStart(16, "0");

export function check(table, math) {
  return table.functions.map(({ name, rows }) => {
    let differ = 0;
    let ulps = 0n;
    for (const r of rows) {
      const want = r[r.length - 1];
      const got = bits(r.length === 3 ? math[name](f64(r[0]), f64(r[1])) : math[name](f64(r[0])));
      if (got !== want) {
        differ++;
        const d = BigInt(`0x${got}`) - BigInt(`0x${want}`);
        if ((d < 0n ? -d : d) > ulps) ulps = d < 0n ? -d : d;
      }
    }
    return { name, rows: rows.length, differ, most_ulps: Number(ulps) };
  });
}

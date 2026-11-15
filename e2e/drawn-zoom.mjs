// The drawn zoom's picture against the page (taste.md VIEW-13; docs/designs/camera/drawn-zoom.md):
// the last frame of a zoom by the hand, the fingers still on, and the frame after its end, laid out,
// MUST look the same within anti-aliasing. At a real 2 device px a px, as the owner's Mac draws.
//   the song   first-song, and every kind of thing the arrangement shows: a Vox track whose audio
//              clip plays the last of three takes (fades, its Takes rows open), a second audio clip
//              (its volume in its name), a lane of names (Kick's drum, a name it no longer has
//              struck through), Bass's automation open with one lane hidden (the line saying so),
//              a drum kit (its bus's words), Hats muted in the song (faded, hatched), a tempo and a
//              time signature change, markers with names cut short, the loop, selected bars, the
//              Vox clip picked (its fade handles) and a curve's point picked
//   the zooms  a pinch in, a pinch out, a pinch far out, and a run of ⌘-scroll: for each, the
//              screen's pixels over the arrangement's clear part, the last picture against the page
//              after it; no more than LIMIT of them over 32 of 255 apart in a colour (why: below)
//
//   node e2e/drawn-zoom.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import zlib from "node:zlib";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

/**
 * How many of the clear part's pixels may differ by more than 32 of 255 in a colour (of 3.95 million:
 * 1440 × 1200 at 2 device px a px, right of the headers), and how far an 8 × 8 device px cell may
 * differ on average: CELL, or TEXT_CELL where the picture lists a word. Measured on this song in Chrome
 * 133 at a real 2 device px a px, the picture and the page differ only by where text's glyphs fall, a
 * sub-pixel apart (a marker's name, a lane of names' steps), as Chrome places the page's glyphs on its
 * own grid: 0 to 91 pixels over 32 for the eight zooms below, cells at most 3.8 apart, 12.8 where a
 * word lies (drawn-zoom.md §1, "What VIEW-13's check sees"). A faint wash moved or a small shape left
 * out differs by under 32 a pixel but is dense in its cells. Review T's mutants each fail some of the
 * zooms: no strike-through (words' cells 28), no playhead cap (cells 115), the loop's shade a beat
 * right (cells 16), no curve points (44), no step ticks (36), no fade handles (83), no hatching (9.3
 * to 12, the closest), no marker ticks, no tempo words, no waves (19,975 px), 1 px across (232,355 px).
 * So: 200 pixels, above the glyphs' 91; 8 a cell, twice the song's own and under the hatching's 9.3;
 * 20 where a word lies, above the glyphs' 12.8 and under the strike's 25. They are this song's: on the
 * beat song Chrome rasters the page's bar numbers at a fraction of a device px half a px apart from
 * the picture's, 288 to 846 pixels (gaps.md, "What the zoom's picture may still miss").
 */
const LIMIT = 200;
/** The most an 8 × 8 device px cell may differ on average, of 255, and where a word lies (why: above). */
const CELL = 8, TEXT_CELL = 20;
const W = 1440, H = 1200;

const { page, errors, api, state, key, show } = await open(W, H, { dpr: 2 });
const frames = (n = 2) => page.evaluate((n) => new Promise((r) => { const one = (k) => (k ? requestAnimationFrame(() => one(k - 1)) : r()); one(n); }), n);
const has = async (sel) => !!(await page.$(sel));
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const tl = "[data-testid=timeline]";

// ---------------------------------------------------------------- the song: every kind of thing

const RATE = 48000;
const BEAT = 60 / 124;
/** A mono 16-bit WAV of `n` samples, each `f(i)` from -1 to 1. */
function wav(path, n, f) {
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(RATE, 24);
  b.writeUInt32LE(RATE * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, f(i))) * 32767), 44 + i * 2);
  fs.writeFileSync(path, b);
}
// Three takes of a 4-beat loop (comp.mjs's), each louder; and a pad that swells and fades.
const N = Math.round(4 * BEAT * RATE);
const TAKES = [0, N / RATE, (2 * N) / RATE];
fs.mkdirSync(`${PROJ}/assets`, { recursive: true });
wav(`${PROJ}/assets/vox take.wav`, 3 * N, (i) => 0.25 * (Math.floor(i / N) + 1) * Math.sin((2 * Math.PI * 220 * i) / RATE) * (0.5 + 0.5 * Math.sin((2 * Math.PI * i) / (RATE / 3))));
write("assets/vox take.wav.json", JSON.stringify({ takes: TAKES }) + "\n");
wav(`${PROJ}/assets/pad.wav`, Math.round(8 * BEAT * RATE), (i) => 0.8 * Math.sin((2 * Math.PI * 110 * i) / RATE) * Math.sin((Math.PI * i) / (8 * BEAT * RATE)));
// A file only a clip far on plays: the page never draws it until a zoom out brings it in.
wav(`${PROJ}/assets/far.wav`, Math.round(8 * BEAT * RATE), (i) => 0.9 * Math.sin((2 * Math.PI * 165 * i) / RATE) * Math.sin((Math.PI * i) / (8 * BEAT * RATE)));
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write(
  "data/vox/clips.json",
  JSON.stringify({
    clips: [
      { id: "voxclip1", start: 4, length: 4, asset: "vox take.wav", offset: TAKES[2], fade_in: 0.2, fade_out: 0.4 },
      { id: "padclip1", start: 12, length: 8, asset: "pad.wav", gain_db: -6 },
      { id: "farclip1", start: 146, length: 8, asset: "far.wav" },
    ],
  }),
);
// Kick's drum as a lane of names: kick, snare, and a name the choice no longer has.
write("tracks/kick.ts", read("tracks/kick.ts").replace('drums.drum(lane.notes("hits"))', 'drums.drum(lane.notes("hits"), { drum: lane.control("drum", "choice", "kick") })'));
write("data/kick/drum.json", JSON.stringify({ unit: "choice", clips: [{ id: "names1", start: 2, length: 10, points: [[0, "kick"], [3, "snare"], [6, "gone"]] }] }));
// Hats muted in the song: faded and hatched.
write("tracks/hats.ts", read("tracks/hats.ts").replace('{ name: "Hats" }', '{ name: "Hats", muted: true }'));
const song = read("song.ts");
write("song.ts", song.replace('import kick from "./tracks/kick";', 'import kick from "./tracks/kick";\nimport vox from "./tracks/vox";').replace("      kick: kick.out,", "      kick: kick.out,\n      vox: vox.out,"));
check(await until(() => has(`${tl} [data-lane="vox.clips"] [data-clip="voxclip1"]`), 20000), "the song has a Vox track of audio clips");
check(await until(async () => (await state()).overlays?.heard?.hats === "muted-in-song" || !!(await page.$('[data-track="hats"] [data-clip]')), 10000), "Hats is muted in the song");

// A drum kit: its bus's row says what it is (KIT-10).
await show("bass");
await key("Meta+t");
await page.waitForSelector("[data-testid=instrument-list]", { timeout: 3000 }).catch(() => null);
await page.keyboard.type("basic kit");
await sleep(150);
await page.keyboard.press("Enter");
check(await until(async () => ((await state()).view.drum_kits ?? []).length === 1 && (await has(`${tl} [data-testid=bus-lane]`)), 20000), "a drum kit, its bus's row saying so");
const { session } = await state();
check((await post("/tempo", { session, at: 16, bpm: 140, add: true })).status === 200 && (await post("/meter", { session, at: 32, beats: 3, unit: 4, add: true })).status === 200, "a tempo change at beat 16 and a time signature change at 32");
const markers = [[8, "Intro"], [16, "Verse with a long name"], [22, "Chorus"], [24, "Breakdown before the drop"], [40, "Outro"]].map(([beat, name]) => ({ beat, name }));
check((await post("/markers", { session: (await state()).session, markers })).status === 200, "markers, some cut short by the next");
await post("/loop", { start: 4, end: 12, on: true });
// Bass's automation open, its drop lane hidden: a line says so (D4).
await page.click(`${tl} [data-testid=fold-all]`);
await until(() => has(`${tl} [data-lane="bass.drop"]`), 5000);
const drop = await page.$(`${tl} [data-lane="bass.drop"] > div:first-child`);
if (drop) {
  const b = await drop.boundingBox();
  await page.mouse.click(b.x + 60, b.y + b.height / 2, { button: "right" });
  await until(() => has('[data-menu-item="hide-lane"]'), 3000);
  await page.click('[data-menu-item="hide-lane"]');
}
check(await until(() => has(`${tl} [data-testid=hidden-lanes][data-module="bass"]`), 5000), "Bass's automation open, one lane hidden, the line saying so");
// Vox's Takes rows open.
await page.click('[data-testid=takes][data-module="vox"]');
check(await until(() => has(`${tl} [data-testid=take-row][data-lane="vox.clips"]`), 5000), "Vox's Takes rows open");
await until(() => page.$$eval(`${tl} [data-wave]:not([data-wave=loading])`, (els) => els.length >= 2), 10000);
// The Vox clip picked: its fade handles show. (Bars selected come later: one selection at a time, ARR-9.)
const vox = await (await page.$(`${tl} [data-clip="voxclip1"]`)).boundingBox();
await page.mouse.click(vox.x + vox.width / 2, vox.y + vox.height - 6);
await page.mouse.move(5, 5);
// What the window says over the arrangement (a toast) goes before the pictures are taken.
await until(async () => !(await has("[data-testid=toast]")), 15000);
await sleep(300);
check(await has(`${tl} [data-clip="voxclip1"][data-picked]`), "the Vox clip picked");
check(await until(() => has(`${tl} [data-lane="kick.drum"] [data-step="gone"]`), 20000), "Kick has a lane of names, one name it no longer has");
await page.screenshot({ path: `${ROOT}/drawn-zoom-song.png` });

// ---------------------------------------------------------------- the pictures

/** The screen's pixels, decoded from a PNG screenshot (8-bit RGBA, not interlaced). */
function pixels(buf) {
  let p = 8, w = 0, h = 0;
  const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p), kind = buf.toString("ascii", p + 4, p + 8);
    const data = buf.subarray(p + 8, p + 8 + len);
    if (kind === "IHDR") (w = data.readUInt32BE(0)), (h = data.readUInt32BE(4));
    else if (kind === "IDAT") idat.push(data);
    p += 12 + len;
  }
  const bpp = buf[25] === 6 ? 4 : 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const out = new Uint8Array(w * h * 4);
  let prev = new Uint8Array(stride), cur = new Uint8Array(stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
      let v = raw[y * (stride + 1) + 1 + i];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const q = a + b - c, pa = Math.abs(q - a), pb = Math.abs(q - b), pc = Math.abs(q - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      cur[i] = v & 255;
    }
    for (let x = 0; x < w; x++) for (let k = 0; k < 4; k++) out[(y * w + x) * 4 + k] = k < bpp ? cur[x * bpp + k] : 255;
    [prev, cur] = [cur, prev];
  }
  return { w, h, data: out };
}
/**
 * How `a` and `b` differ over `region` (device px), leaving out the columns `mask` lists (device px
 * from the region's left, [from, to)): how many pixels by more than 32 in a colour and where most do
 * (8 css px cells from its corner); and the mean difference of each 8 × 8 device px cell, which sees
 * a faint wash moved or a small shape gone where no pixel differs by 32: `dense` cells over CELL, or
 * over TEXT_CELL where a word lies (`texts`: the picture's word boxes, css px from the region's
 * corner); `cell` the most, `where` some of them.
 */
function differ(a, b, region, dpr, mask = [], texts = []) {
  let over = 0, max = 0;
  const cells = new Map();
  const C = 8, cw = Math.ceil(region.w / C), sums = new Float64Array(cw * Math.ceil(region.h / C)), counts = new Uint16Array(sums.length);
  const masked = (x) => mask.some(([f, t]) => x >= f && x < t);
  for (let y = region.y; y < region.y + region.h; y++)
    for (let x = region.x; x < region.x + region.w; x++) {
      if (mask.length && masked(x - region.x)) continue;
      const o = (y * a.w + x) * 4;
      const d = Math.max(Math.abs(a.data[o] - b.data[o]), Math.abs(a.data[o + 1] - b.data[o + 1]), Math.abs(a.data[o + 2] - b.data[o + 2]));
      const q = Math.floor((y - region.y) / C) * cw + Math.floor((x - region.x) / C);
      sums[q] += d;
      counts[q]++;
      max = Math.max(max, d);
      if (d <= 32) continue;
      over++;
      const k = `${Math.floor((x - region.x) / (8 * dpr)) * 8},${Math.floor((y - region.y) / (8 * dpr)) * 8}`;
      cells.set(k, (cells.get(k) ?? 0) + 1);
    }
  const texty = new Uint8Array(sums.length), ch = Math.ceil(region.h / C);
  for (const [x, y, w, h] of texts) {
    const x0 = Math.max(0, Math.floor(((x - 1) * dpr) / C)), x1 = Math.min(cw - 1, Math.floor(((x + w + 1) * dpr) / C));
    const y0 = Math.max(0, Math.floor(((y - 1) * dpr) / C)), y1 = Math.min(ch - 1, Math.floor(((y + h + 1) * dpr) / C));
    for (let cy = y0; cy <= y1; cy++) for (let cx = x0; cx <= x1; cx++) texty[cy * cw + cx] = 1;
  }
  let cell = 0, word = 0, dense = 0;
  const where = [];
  for (let q = 0; q < sums.length; q++) {
    if (!counts[q]) continue;
    const m = sums[q] / counts[q];
    if (texty[q]) word = Math.max(word, m);
    else cell = Math.max(cell, m);
    if (m <= (texty[q] ? TEXT_CELL : CELL)) continue;
    dense++;
    if (where.length < 6) where.push(`${Math.round(((q % cw) * C) / dpr)},${Math.round((Math.floor(q / cw) * C) / dpr)}:${Math.round(m)}`);
  }
  return { over, max, cells: [...cells].sort((p, q) => q[1] - p[1]).slice(0, 8), cell: Math.round(cell * 10) / 10, word: Math.round(word * 10) / 10, dense, where };
}
const dpr = await page.evaluate(() => devicePixelRatio);
// The arrangement's clear part: its lanes and its ruler, right of the headers.
const region = await page.evaluate(() => {
  const el = document.querySelector("[data-testid=timeline]");
  const r = el.getBoundingClientRect();
  const m = window.m2canvas.camera.now("arrangement").margins;
  const d = window.devicePixelRatio;
  return { x: Math.round((r.left + el.clientLeft + m.left) * d), y: Math.round((r.top + el.clientTop) * d), w: Math.round((el.clientWidth - m.left) * d), h: Math.round(el.clientHeight * d) };
});
check(dpr === 2, `the page draws at 2 device px a px, as the owner's Mac (${dpr})`);
const pointIn = (fx, fy) =>
  page.evaluate((fx, fy) => {
    const el = document.querySelector("[data-testid=timeline]"), r = el.getBoundingClientRect();
    const m = window.m2canvas.camera.now("arrangement").margins;
    return { x: Math.round(r.left + m.left + (el.clientWidth - m.left - m.right) * fx), y: Math.round(r.top + m.top + (el.clientHeight - m.top - m.bottom) * fy) };
  }, fx, fy);
const gesture = (at, type, scale) =>
  page.evaluate((x, y, type, scale) => {
    const e = new UIEvent(type, { bubbles: true, cancelable: true });
    for (const [k, v] of Object.entries({ scale, clientX: x, clientY: y })) Object.defineProperty(e, k, { value: v });
    (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
  }, at.x, at.y, type, scale);
const shown = () => page.evaluate(() => ({ cam: window.m2canvas.camera.now("arrangement"), pic: window.m2canvas.camera.picture("arrangement") }));
const shot = async (file) => pixels(Buffer.from(await page.screenshot({ path: `${ROOT}/${file}` })));
const tag = process.env.DRAWN_ZOOM_TAG ?? "";

/**
 * The last picture of a zoom, against the page laid out after its end (and `later`, after that
 * long). `mask()`, after both shots, gives the columns left out, in device px from the region's left:
 * what moves on its own between the two shots (the playhead while the song plays).
 */
async function compare(name, last, before, { later = 0, mask = null } = {}) {
  const lastAt = await shown();
  const a = await shot(`${tag}${name}-last.png`);
  // Still the picture after the shot: the shot is of it, not of the page after an end.
  const still = await shown();
  await last();
  await page.waitForFunction(() => window.m2canvas.camera.now("arrangement").laid === null, { timeout: 3000 }).catch(() => null);
  await frames(3);
  const after = await shown();
  const b = await shot(`${tag}${name}-after.png`);
  const c = later ? (await sleep(later), await shot(`${tag}${name}-later.png`)) : null;
  const m = mask ? await mask(lastAt) : [];
  const texts = lastAt.pic?.drawn?.texts ?? [];
  const d = differ(a, b, region, dpr, m, texts);
  const e = c ? differ(a, c, region, dpr, m, texts) : null;
  const worst = e && (e.over > d.over || e.dense > d.dense) ? e : d;
  check(
    !!lastAt.cam.laid && lastAt.pic?.shown && lastAt.pic.drawn?.zoom === lastAt.cam.zoom && lastAt.cam.laid.zoom === before && !!still.cam.laid && still.pic?.shown && still.cam.zoom === lastAt.cam.zoom && after.cam.laid === null && !after.pic?.shown && after.cam.zoom === lastAt.cam.zoom && worst.over <= LIMIT && worst.dense === 0,
    `${name} (${before} → ${lastAt.cam.zoom.toFixed(2)} px a beat): its last picture (the fingers still on, the page laid out at ${lastAt.cam.laid?.zoom}, the picture ${still.pic?.shown ? "still shown" : "gone"} after the shot) and the page laid out after it${c ? ` (and ${later} ms after)` : ""}${m.length ? ` (but ${m.map(([f, t]) => `${Math.round(f / dpr)} to ${Math.round(t / dpr)} px across`).join(", ")})` : ""} differ by more than 32 of 255 in ${d.over}${e ? ` (${e.over})` : ""} of ${region.w * region.h} pixels (at most ${LIMIT}; the most ${d.max}; where: ${JSON.stringify(d.cells)}), and by more than ${CELL} on average (${TEXT_CELL} where a word lies) in ${d.dense}${e ? ` (${e.dense})` : ""} cells of 8 × 8 (none may; the most ${d.cell}${e ? ` (${e.cell})` : ""}, where a word lies ${d.word}${e ? ` (${e.word})` : ""}; where: ${JSON.stringify(worst.where)})`,
  );
  await sleep(400);
  return { d, e };
}
/** A pinch at `at` to `to` (and back to 1, with `back`), a frame each of 10 steps a way, compared at its end. */
async function pinch(name, at, to, back = false, opts = {}) {
  const before = (await shown()).cam.zoom;
  await gesture(at, "gesturestart", 1);
  for (let i = 1; i <= 10; i++) {
    await gesture(at, "gesturechange", 1 + ((to - 1) * i) / 10);
    await frames(1);
  }
  for (let i = 1; back && i <= 10; i++) {
    await gesture(at, "gesturechange", to + ((1 - to) * i) / 10);
    await frames(1);
  }
  await frames(3);
  return compare(name, () => gesture(at, "gestureend", back ? 1 : to), before, opts);
}

/** The page's playhead, in px from the lanes' left (as the picture lists it). */
const playheadAt = () =>
  page.evaluate(() => {
    const el = document.querySelector("[data-testid=timeline]"), r = el.getBoundingClientRect();
    const m = window.m2canvas.camera.now("arrangement").margins;
    return (document.querySelector("[data-testid=playhead]")?.getBoundingClientRect().left ?? NaN) - (r.left + el.clientLeft + m.left);
  });

await page.evaluate((tl) => document.querySelector(tl).scrollTo(0, 0), tl);
// The playhead in view, at beat 10, in every zoom below.
await post("/seek", { beat: 10 });
check(await until(async () => Math.abs((await playheadAt()) - 10 * (await shown()).cam.zoom) < 2, 5000), `the playhead in view at beat 10 (${await playheadAt()} px)`);
await frames(3);
await sleep(300);
// Out at the song's start and back in: the start stops the way out, so the way back in looks further
// on than the song's room at that zoom; the end keeps room where it looked.
await pinch("a pinch out and back in at the song's start", await pointIn(0.3, 0.4), 0.5, true);
await page.evaluate((tl) => document.querySelector(tl).scrollTo(0, 0), tl);
await frames(3);
await sleep(300);
// Vox's clips changed outside the app (AGENT-5): its lane glows, the picked clip too, until a press or a key.
{
  const file = `${PROJ}/data/vox/clips.json`;
  const j = JSON.parse(fs.readFileSync(file, "utf8"));
  j.clips.find((c) => c.id === "padclip1").gain_db = -7;
  fs.writeFileSync(file, JSON.stringify(j));
  check(await until(() => has(`${tl} [data-clip="voxclip1"][data-picked][data-glow]`), 10000), "Vox's clips changed outside the app: its lane glows, the picked clip too");
  await until(async () => !(await has("[data-testid=notice]")) && !(await has("[data-testid=toast]")), 15000);
  await sleep(300);
}
await pinch("a pinch in", await pointIn(0.3, 0.4), 1.6);
await pinch("a pinch out", await pointIn(0.3, 0.4), 0.65);
// The pointer resting on a clip, as a pinch on a Mac is where the pointer is: its hover outline.
// Headless Chrome has no pointer that hovers, so the app's own rules for one (`@media (hover: hover)`)
// are put on as the Mac has them.
{
  const hoverRules = (on) =>
    page.evaluate((on) => {
      window.__hoverRules ??= [];
      if (!on) {
        for (const r of window.__hoverRules) r.media.mediaText = "(hover: hover)";
        window.__hoverRules = [];
        return 0;
      }
      const walk = (list) => {
        for (const r of list) {
          if (r instanceof CSSMediaRule && r.media.mediaText.replace(/\s/g, "") === "(hover:hover)") {
            r.media.mediaText = "all";
            window.__hoverRules.push(r);
          }
          if (r.cssRules) walk(r.cssRules);
        }
      };
      for (const sheet of document.styleSheets) {
        try {
          walk(sheet.cssRules);
        } catch {}
      }
      return window.__hoverRules.length;
    }, on);
  const target = await page.evaluate((tl) => {
    const el = document.querySelector(tl), b = el.getBoundingClientRect();
    // A clip of Kick's or Bass's notes, its part in view clear of the headers and the edge.
    const lo = b.left + 300, hi = b.right - 100;
    const c = [...el.querySelectorAll('[data-lane="kick.hits"] [data-clip].cursor-grab, [data-lane="bass.notes"] [data-clip].cursor-grab')]
      .map((e) => ({ id: e.dataset.clip, r: e.getBoundingClientRect() }))
      .map(({ id, r }) => ({ id, r, l: Math.max(r.left, lo), h: Math.min(r.right, hi) }))
      .find(({ l, h, r }) => h - l > 60 && r.bottom < b.bottom - 60);
    return c ? { id: c.id, x: Math.round((c.l + c.h) / 2), y: Math.round(c.r.top + c.r.height / 2) } : null;
  }, tl);
  const border = () => page.$eval(`${tl} [data-clip="${target.id}"]`, (e) => getComputedStyle(e).borderTopColor);
  const n = await hoverRules(true);
  const plain = target && (await border());
  if (target) await page.mouse.move(target.x, target.y);
  await sleep(400);
  const hovered = target && (await border());
  check(n > 0 && !!target && plain !== hovered, `the Mac's hover rules on (${n}), the pointer resting on Kick's clip ${target?.id}: outlined as on a Mac (${plain} → ${hovered})`);
  if (target) {
    await pinch("the pointer resting on a clip, a pinch in", target, 1.25, false, { later: 600 });
    await pinch("the pointer resting on a clip, a pinch out", target, 0.8, false, { later: 600 });
  }
  await hoverRules(false);
  await page.mouse.move(5, 5);
  await sleep(300);
}
// Bars selected on the ruler's band (ARR-9), from the beat 34 to 39 at this zoom: clear of the markers.
{
  const band = await (await page.$(`${tl} [data-testid=bars-band]`)).boundingBox();
  const { zoom, x } = (await shown()).cam;
  await page.mouse.move(band.x + 34 * zoom - x, band.y + 5);
  await page.mouse.down();
  await page.mouse.move(band.x + 39 * zoom - x, band.y + 5, { steps: 4 });
  await page.mouse.up();
  await page.mouse.move(5, 5);
  await sleep(600);
  check(await has(`${tl} [data-testid=section]`), "bars selected");
}
// Vox's clip at beat 146 is far past what the page draws (VIEW-12): the page never asked for its
// file's waveform. The way far out brings it into view.
check(!(await has(`${tl} [data-clip="farclip1"]`)), "Vox's clip at beat 146 isn't on the page yet");
await pinch("a pinch far out", await pointIn(0.2, 0.5), 0.3);
{
  // A run of ⌘-scroll, an event a frame; compared at its last, before G passes. A shot takes longer
  // than G, so while it is taken the run goes on with events that zoom by nothing (a ⌘-scroll of 0,
  // as a trackpad's last events can be), then stops.
  const at = await pointIn(0.4, 0.4);
  const before = (await shown()).cam.zoom;
  for (let i = 0; i < 8; i++) {
    await page.evaluate((x, y) => (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new WheelEvent("wheel", { deltaY: -6, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true })), at.x, at.y);
    await frames(1);
  }
  await page.evaluate((x, y) => {
    window.__stillScroll = setInterval(() => (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(new WheelEvent("wheel", { deltaY: 0, clientX: x, clientY: y, metaKey: true, bubbles: true, cancelable: true })), 8);
  }, at.x, at.y);
  await compare("a run of ⌘-scroll", async () => {
    await page.evaluate(() => clearInterval(window.__stillScroll));
    await sleep(150);
  }, before);
}
// Recording a take on Vox while the song plays (lane B): the picture draws the take as it grows and
// the playhead as it moves, painting once a frame. Left out of the compare: the columns the playhead
// (and the take's growing end with it) crossed between the two shots.
{
  await page.click('[data-arm="vox"]');
  check(await until(async () => !!(await state()).arm?.armed?.vox, 3000), `Vox armed (${JSON.stringify((await state()).arm?.armed)})`);
  const { zoom, x, view, margins } = (await shown()).cam;
  await post("/seek", { beat: (Math.round(x) + 0.1 * (view.w - margins.left)) / zoom });
  await sleep(300);
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  await page.keyboard.press("r");
  check(await until(async () => (await state()).record?.phase === "take", 8000), `R records on Vox, the song playing (${JSON.stringify((await state()).record?.phase)})`);
  await api("/input/inject", { method: "POST", body: JSON.stringify({ tone: { hz: 330, seconds: 20, gain: 0.5 } }) });
  await page.mouse.move(5, 5);
  await sleep(2500);
  const at = await pointIn(0.6, 0.5);
  const before = (await shown()).cam.zoom;
  await gesture(at, "gesturestart", 1);
  await frames(2);
  // 20 changes, one a frame: the picture's paints in the frames they took.
  const count = await page.evaluate(
    (x, y) =>
      new Promise((done) => {
        const p0 = window.m2canvas.camera.picture("arrangement").paints;
        let k = 0;
        const one = () => {
          const e = new UIEvent("gesturechange", { bubbles: true, cancelable: true });
          for (const [key, v] of Object.entries({ scale: 1 + 0.0125 * (k + 1), clientX: x, clientY: y })) Object.defineProperty(e, key, { value: v });
          (document.elementFromPoint(x, y) ?? document.body).dispatchEvent(e);
          if (++k < 20) requestAnimationFrame(one);
          else requestAnimationFrame(() => done({ frames: k, paints: window.m2canvas.camera.picture("arrangement").paints - p0 }));
        };
        requestAnimationFrame(one);
      }),
    at.x,
    at.y,
  );
  check(count.paints <= count.frames + 1, `while the song plays and a take records, the picture paints at most once a frame: ${count.paints} paints in ${count.frames} frames`);
  await frames(3);
  const drawnTake = (await shown()).pic?.drawn;
  await compare("recording while the song plays, a pinch in", () => gesture(at, "gestureend", 1.25), before, {
    mask: async (lastAt) => {
      const from = lastAt.pic?.drawn?.playhead ?? NaN, to = await playheadAt();
      return [[Math.floor((Math.min(from, to) - 16) * dpr), Math.ceil((Math.max(from, to) + 16) * dpr)]];
    },
  });
  check(!!drawnTake && (await state()).record?.phase === "take", `the take still recording at the end (${JSON.stringify((await state()).record?.phase)})`);
  await page.keyboard.press("r");
  await until(async () => (await state()).record?.phase !== "take", 8000);
  await page.keyboard.press("Space");
  await until(async () => !(await api("/status")).body.playing, 5000);
}
done(errors);

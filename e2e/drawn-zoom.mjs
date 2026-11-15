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
 * 1440 × 1200 at 2 device px a px, right of the headers). Measured on this song in Chrome 133 at a
 * real 2 device px a px, the picture and the page differ only by where text's glyphs fall: a marker's
 * name, a lane of names' steps, a sub-pixel apart, as Chrome places the page's glyphs on its own grid.
 * The pixels over 32 apart were 0, 53, 112, 135 and 30 for the five zooms below, the same in each of
 * three runs (drawn-zoom.md §1, "Measured after the build"). A picture that leaves out the audio waves
 * differs by 1,536 to 17,800, and one moved 1 px across by 45,268 to 217,539: each fails all five.
 * 200 lies above the glyphs and 7 times under the smaller fault. It is this song's: on the beat song
 * Chrome rasters the page's bar numbers at a fraction of a device px half a px apart from the picture's,
 * 288 to 846 pixels (gaps.md, "What the zoom's picture may still miss").
 */
const LIMIT = 200;
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
write("tracks/vox.ts", 'import { track } from "@m2/core";\n\nexport default track("vox", { name: "Vox" }, ({ lane }) => lane.audio("clips"));\n');
fs.mkdirSync(`${PROJ}/data/vox`, { recursive: true });
write(
  "data/vox/clips.json",
  JSON.stringify({
    clips: [
      { id: "voxclip1", start: 4, length: 4, asset: "vox take.wav", offset: TAKES[2], fade_in: 0.2, fade_out: 0.4 },
      { id: "padclip1", start: 12, length: 8, asset: "pad.wav", gain_db: -6 },
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
check(await until(() => has(`${tl} [data-lane="vox.clips"] [data-clip="voxclip1"]`), 20000), "the song has a Vox track of two audio clips");
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
/** How many pixels of `region` differ by more than 32 in a colour, and where most do (8 px cells, in css px from its corner). */
function differ(a, b, region, dpr) {
  let over = 0, max = 0;
  const cells = new Map();
  for (let y = region.y; y < region.y + region.h; y++)
    for (let x = region.x; x < region.x + region.w; x++) {
      const o = (y * a.w + x) * 4;
      const d = Math.max(Math.abs(a.data[o] - b.data[o]), Math.abs(a.data[o + 1] - b.data[o + 1]), Math.abs(a.data[o + 2] - b.data[o + 2]));
      max = Math.max(max, d);
      if (d <= 32) continue;
      over++;
      const k = `${Math.floor((x - region.x) / (8 * dpr)) * 8},${Math.floor((y - region.y) / (8 * dpr)) * 8}`;
      cells.set(k, (cells.get(k) ?? 0) + 1);
    }
  return { over, max, cells: [...cells].sort((p, q) => q[1] - p[1]).slice(0, 8) };
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

/** The last picture of a zoom, against the page laid out after its end. */
async function compare(name, last, before) {
  const lastAt = await shown();
  const a = await shot(`${tag}${name}-last.png`);
  // Still the picture after the shot: the shot is of it, not of the page after an end.
  const still = await shown();
  await last();
  await page.waitForFunction(() => window.m2canvas.camera.now("arrangement").laid === null, { timeout: 3000 }).catch(() => null);
  await frames(3);
  const after = await shown();
  const b = await shot(`${tag}${name}-after.png`);
  const d = differ(a, b, region, dpr);
  check(
    !!lastAt.cam.laid && lastAt.pic?.shown && lastAt.pic.drawn?.zoom === lastAt.cam.zoom && lastAt.cam.laid.zoom === before && !!still.cam.laid && still.pic?.shown && still.cam.zoom === lastAt.cam.zoom && after.cam.laid === null && !after.pic?.shown && after.cam.zoom === lastAt.cam.zoom && d.over <= LIMIT,
    `${name} (${before} → ${lastAt.cam.zoom.toFixed(2)} px a beat): its last picture (the fingers still on, the page laid out at ${lastAt.cam.laid?.zoom}, the picture ${still.pic?.shown ? "still shown" : "gone"} after the shot) and the page laid out after it differ by more than 32 of 255 in ${d.over} of ${region.w * region.h} pixels (at most ${LIMIT}; the most ${d.max}; where: ${JSON.stringify(d.cells)})`,
  );
  await sleep(400);
}
/** A pinch at `at` to `to` (and back to 1, with `back`), a frame each of 10 steps a way, compared at its end. */
async function pinch(name, at, to, back = false) {
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
  await compare(name, () => gesture(at, "gestureend", back ? 1 : to), before);
}

await page.evaluate((tl) => document.querySelector(tl).scrollTo(0, 0), tl);
await frames(3);
await sleep(300);
// Out at the song's start and back in: the start stops the way out, so the way back in looks further
// on than the song's room at that zoom; the end keeps room where it looked.
await pinch("a pinch out and back in at the song's start", await pointIn(0.3, 0.4), 0.5, true);
await page.evaluate((tl) => document.querySelector(tl).scrollTo(0, 0), tl);
await frames(3);
await sleep(300);
await pinch("a pinch in", await pointIn(0.3, 0.4), 1.6);
await pinch("a pinch out", await pointIn(0.3, 0.4), 0.65);
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
done(errors);

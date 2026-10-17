// 8b.8 Meters in the app (design-8b §10.4, taste.md §2.8, §3.4; the 8b-ui plan's U10): a meter in
// every track header and on the Song row (the song's output), from the server's `meters` messages.
// The messages' rate and shape while playing and after a stop; meters that move, read through
// `role="meter"` and `aria-valuenow`; Kick's at the floor while Kick is muted and Bass's still moving;
// every meter at the floor a second after the song stops and when the socket closes; the fill's
// colours and the peak hold's tick, read from the canvas. It also prints the real load and late
// callbacks while playing, for the status pill's states (information only).
//
//   node e2e/s8-meters.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { check, done, open, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// The page's meters messages, as they arrive (CDP), and its sockets, so the suite can close one.
const heard = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") heard.push({ at: Date.now(), m });
  } catch {}
});
await page.evaluateOnNewDocument(() => {
  const Socket = window.WebSocket;
  window.WebSocket = class extends Socket {
    constructor(...args) {
      super(...args);
      (window.__sockets ??= []).push(this);
    }
  };
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid=timeline] [data-track="kick"]', { timeout: 10000 });

const TRACKS = ["kick", "hats", "bass", "keys"];
const FLOOR = -60;
/** Every one of the five meters (four tracks and the Song row) reads the floor. */
const allFloor = (l) => Object.keys(l).length === 5 && Object.values(l).every((v) => v === FLOOR);
/** Every meter's level as it reads now: `{kick: -12, …, song: -1}`. */
const levels = () =>
  page.evaluate(() => {
    const out = {};
    for (const e of document.querySelectorAll("[data-testid=timeline] [role=meter]")) {
      const row = e.closest("[data-song-row]") ? "song" : e.closest("[data-track]")?.dataset.track;
      out[row] = Number(e.getAttribute("aria-valuenow"));
    }
    return out;
  });
/** Sample the levels every 50 ms for `ms`. */
const sample = async (ms) => {
  const seen = [];
  const t = Date.now();
  while (Date.now() - t < ms) {
    seen.push(await levels());
    await sleep(50);
  }
  return seen;
};
const playing = async () => (await api("/status")).body.playing;
const playSong = async () => {
  if (!(await playing())) await page.keyboard.press("Space");
  return until(playing, 3000);
};
const stopSong = async () => {
  if (await playing()) await page.keyboard.press("Space");
  return until(async () => !(await playing()), 3000);
};
/** The colours drawn in meter `sel`'s canvas, by row from the top: one `r,g,b` per row (its middle column). */
const rows = (sel) =>
  page.$eval(sel, (c) => {
    const g = c.getContext("2d");
    const d = g.getImageData(Math.floor(c.width / 2), 0, 1, c.height).data;
    const out = [];
    for (let y = 0; y < c.height; y++) out.push(`${d[4 * y]},${d[4 * y + 1]},${d[4 * y + 2]}`);
    return out;
  }).catch(() => []);
const KICK = '[data-track="kick"] [role=meter]';
const SONG = "[data-song-row] [role=meter]";
const COLOURS = { trough: "220,223,227", fill: "101,105,111", warn: "176,122,32", danger: "175,61,54", tick: "32,35,39" };

// ---------------------------------------------------------------- where they sit

const meters = await page.$$eval("[data-testid=timeline] [role=meter]", (els) =>
  els.map((e) => {
    const head = e.closest("[data-track]") ?? e.closest("[data-song-row]");
    const h = head.getBoundingClientRect(), r = e.getBoundingClientRect();
    return { label: e.getAttribute("aria-label"), x: Math.round(r.left - h.left), w: r.width, h: r.height, min: e.getAttribute("aria-valuemin"), max: e.getAttribute("aria-valuemax") };
  }),
);
check(
  JSON.stringify(meters.map((m) => m.label)) === JSON.stringify(["Kick level", "Hats level", "Bass level", "Keys level", "Song level"]),
  `a meter on every track and on the Song row, each named (${meters.map((m) => m.label)})`,
);
check(meters.length === 5 && meters.every((m) => m.x === 231 && m.w === 3 && m.h === 28), `each 3 × 28 px at x = 231 in its 240 px header (${meters.map((m) => `${m.x}:${m.w}x${m.h}`).join(" ")})`);
check(meters.length === 5 && meters.every((m) => m.min === "-60" && m.max === "6"), "their scale runs from −60 to +6 dB");
// A window under 1280 px: 200 px headers, the meter 9 px in from their right edge still.
await page.setViewport({ width: 1100, height: 900 });
await sleep(300);
const small = await page.$$eval("[data-testid=timeline] [role=meter]", (els) =>
  els.map((e) => {
    const h = (e.closest("[data-track]") ?? e.closest("[data-song-row]")).getBoundingClientRect(), r = e.getBoundingClientRect();
    return `${Math.round(r.left - h.left)}:${r.width}x${r.height}`;
  }),
);
check(small.length === 5 && small.every((m) => m === "191:3x28"), `at 1100 px, in 200 px headers, at x = 191 (${small.join(" ")})`);
await page.setViewport({ width: 1440, height: 900 });
await sleep(300);
const rest = await levels();
check(allFloor(rest), `at rest every meter reads the floor (${JSON.stringify(rest)})`);

// ---------------------------------------------------------------- while playing

let from = heard.length;
check(await playSong(), "Space plays the song");
await sleep(2000);
const second = heard.slice(from).filter((h) => h.at >= Date.now() - 1000);
const last = heard.at(-1)?.m;
check(second.length >= 20 && second.length <= 40, `20 to 40 meters messages a second while playing (${second.length} in the last second)`);
check(!!last && TRACKS.every((k) => Array.isArray(last.tracks[k])) && last.out?.length === 2 && typeof last.dsp?.load === "number", `each with every track's peaks, the output's and the load (${JSON.stringify(last)})`);
const status = (await api("/status")).body;
console.log(`info: while playing, the load is ${(status.load * 100).toFixed(1)}% and late callbacks ${status.late} (xruns ${status.xruns}); the messages say ${JSON.stringify(last?.dsp)}`);

const play = await sample(1500);
const moved = (k) => new Set(play.map((s) => s[k])).size > 2 && Math.max(...play.map((s) => s[k])) > -30;
check(moved("kick") && moved("bass"), `Kick's and Bass's meters move (${[...new Set(play.map((s) => s.kick))].slice(0, 8)} · ${[...new Set(play.map((s) => s.bass))].slice(0, 8)})`);
check(moved("song"), `the Song row's meter moves with the output (${[...new Set(play.map((s) => s.song))].slice(0, 8)})`);

// The fill's zones: grey low, warn from −6 dB; the output sits near its −1 dB ceiling.
let zones = null;
await until(async () => {
  const r = await rows(SONG);
  zones = r;
  return r.includes(COLOURS.fill) && r.includes(COLOURS.warn);
}, 3000);
check(!!zones && zones.includes(COLOURS.fill) && zones.includes(COLOURS.warn), `the Song meter's fill is fg-muted below −6 dB and warn above (${[...new Set(zones ?? [])].join(" | ")})`);

// ---------------------------------------------------------------- a muted track

const session = (await state()).session;
await post("/overlay", { session, mute: { kick: true } });
await sleep(400);
const muted = await sample(1500);
check(muted.every((s) => s.kick === FLOOR), `Kick muted: its meter stays at the floor (${[...new Set(muted.map((s) => s.kick))]})`);
check(new Set(muted.map((s) => s.bass)).size > 2 && Math.max(...muted.map((s) => s.bass)) > -30, `while Bass's still moves (${[...new Set(muted.map((s) => s.bass))].slice(0, 8)})`);
await post("/overlay", { session, clear: true });
await sleep(400);

// ---------------------------------------------------------------- the song stops

await until(async () => (await levels()).kick > -20, 3000);
from = heard.length;
const stopped = Date.now();
await stopSong();
await sleep(1000);
const after = await levels();
check(allFloor(after), `a second after the song stops every meter reads the floor (${JSON.stringify(after)})`);
await sleep(1500);
const tail = heard.slice(from);
const quiet = heard.slice(from).filter((h) => h.at > stopped + 1500);
const end = tail.at(-1)?.m;
check(tail.length >= 1 && quiet.length === 0, `the server sends its last messages as the engine idles, then none (${tail.length} after the stop, ${quiet.length} after 1.5 s)`);
check(!!end && Math.max(...end.out) < 0.001, `the last one is the silence it ended in (out ${JSON.stringify(end?.out)})`);

// The peak hold: right after the stop the tick still shows where Kick last peaked, then falls.
const kickRows = async () => {
  const r = await rows(KICK);
  return { tick: r.indexOf(COLOURS.tick), fill: r.filter((c) => c === COLOURS.fill || c === COLOURS.warn || c === COLOURS.danger).length };
};
await playSong();
await until(async () => (await levels()).kick > -6, 4000);
await stopSong();
await sleep(500);
const held = await kickRows();
await sleep(300);
const still = await kickRows();
await sleep(2700);
const falling = await kickRows();
check(held.tick >= 0 && held.tick < 10 && held.fill === 0, `half a second after the stop the level is gone and the hold tick stays near the top (row ${held.tick} of 28, ${held.fill} rows filled)`);
check(held.tick >= 0 && still.tick === held.tick, `it holds for 1.5 s (row ${still.tick} a moment later)`);
check(held.tick >= 0 && (falling.tick > held.tick + 6 || falling.tick < 0), `then falls at 12 dB a second (row ${falling.tick} 3.5 s after the stop)`);

// ---------------------------------------------------------------- the socket closes

await playSong();
await until(async () => Object.values(await levels()).some((v) => v > -30), 3000);
await page.evaluate(() => (window.__sockets ?? []).filter((s) => s.readyState === 1).forEach((s) => s.close()));
await sleep(250);
const closed = await levels();
check(allFloor(closed), `the socket closing floors every meter at once (${JSON.stringify(closed)})`);
check(await until(async () => (await levels()).song > -30, 5000), "and once it is open again, the meters move again");
await stopSong();

done(errors);

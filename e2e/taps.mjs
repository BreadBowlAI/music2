// Taps in the app (TAP-5 to TAP-8, gaps #20): a fader dragged down while the song plays moves its
// strip's meter down by what it moved, as the server's channel meter does; Master's strip says the
// song's loudness in LUFS, as the meters' message does; an export's loudness shows in the Export
// dialog; and a scope's frames arrive over the socket while it is asked for, and stop when it stops.
//
//   node e2e/taps.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, key, box } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// The page's meters messages, as they arrive (CDP).
const heard = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") heard.push({ at: Date.now(), m });
  } catch {}
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid=timeline] [data-track="keys"]', { timeout: 10000 });

const FLOOR = -60;
const db = (x) => (x > 0 ? 20 * Math.log10(x) : -Infinity);
const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
const playing = async () => (await api("/status")).body.playing;
/** The loudest the server's meters said of channel `key` (after its fader) over `ms`, in dB. */
const channelOver = async (key, ms) => {
  const from = heard.length;
  await sleep(ms);
  return Math.max(...heard.slice(from).map((h) => db(h.m.channels?.[key]?.[0] ?? 0)));
};
/** The loudest meter `sel` read over `ms`, in whole dB. */
const meterOver = async (sel, ms) => {
  let top = FLOOR;
  const t = Date.now();
  while (Date.now() - t < ms) {
    top = Math.max(top, Number(await page.$eval(sel, (e) => e.getAttribute("aria-valuenow")).catch(() => FLOOR)));
    await sleep(40);
  }
  return top;
};
const stripOver = (id, ms) => meterOver(`${strip(id)} [role=meter]`, ms);

// The first bar, looped, so what plays repeats while the fader moves.
const session = (await state()).session;
await post("/loop", { start: 0, end: 4, on: true });
await key("Alt+3");
check(await until(async () => !!(await page.$(strip("master")))), "⌥3 shows the Mix view");
await page.keyboard.press("Space");
check(await until(playing, 3000), "Space plays the first bar, looped");
await sleep(2500);

// ---------------------------------------------------------------- a fader dragged down (TAP-5)

const keys0 = await channelOver("keys", 2500);
const strip0 = await stripOver("keys", 2500);
check(keys0 > -40 && strip0 > FLOOR, `the Keys channel sounds, and its strip meter shows it (${keys0.toFixed(1)} dB; the strip ${strip0} dB)`);
const was = read("song.ts").match(/keys: \{ in: keys\.out, db: db\((-?[\d.]+)\)/)?.[1];
const thumb = await box(`${strip("keys")} [data-fader] [data-testid=fader-thumb]`);
await page.mouse.move(thumb.x + thumb.width / 2, thumb.y + thumb.height / 2);
await page.mouse.down();
await page.mouse.move(thumb.x + thumb.width / 2, thumb.y + thumb.height / 2 + 60, { steps: 12 });
await page.mouse.up();
const moved = /keys: \{ in: keys\.out, db: db\((-?[\d.]+)\)/;
check(await until(() => read("song.ts").match(moved)?.[1] !== was), `let go, the Keys fader writes its level: ${read("song.ts").match(moved)?.[0]}`);
const drop = Number(read("song.ts").match(moved)?.[1]) - Number(was);
check(drop < -10, `60 px down is more than 10 dB down (${drop.toFixed(1)} dB)`);
await sleep(600);
const keys1 = await channelOver("keys", 2500);
const strip1 = await stripOver("keys", 2500);
check(Math.abs(keys1 - keys0 - drop) < 1, `the server's Keys meter fell by the fader's move: ${(keys1 - keys0).toFixed(1)} dB for ${drop.toFixed(1)}`);
check(strip1 <= strip0 + drop + 3 && strip1 < strip0, `and the strip's meter fell with it: ${strip0} to ${strip1} dB`);
const head = await meterOver('[data-testid=timeline] [data-track="keys"] [role=meter]', 2500);
check(head > FLOOR && Math.abs(head - strip1) <= 2, `as did the Keys header's meter: its channel is its own (${head} dB; the strip ${strip1})`);
const title = await page.$eval(`${strip("keys")} [role=meter]`, (e) => e.title).catch(() => "");
check(title === "Keys's level, after its fader.", `its tooltip says where it reads: "${title}"`);

// ---------------------------------------------------------------- the master's loudness (TAP-6)

const last = heard.at(-1)?.m;
const lufs = await page.$eval("[data-testid=master-lufs]", (e) => e.textContent).catch(() => "");
const peak = await page.$eval("[data-testid=master-peak]", (e) => e.textContent).catch(() => "");
check(typeof last?.loudness?.short === "number" && typeof last?.loudness?.true_peak === "number", `the meters carry the song's loudness (${JSON.stringify(last?.loudness)})`);
const said = Math.round(last?.loudness?.short ?? NaN);
check(/^−\d+ LUFS$/.test(lufs) && Math.abs(-Number(lufs.match(/\d+/)?.[0]) - said) <= 1, `Master's strip says it in words: "${lufs}" (${last?.loudness?.short})`);
check(/^Peak −?\d+(\.\d)? dB$/.test(peak), `and its true peak: "${peak}"`);
await page.screenshot({ path: `${ROOT}/taps-playing.png` });

// ---------------------------------------------------------------- a scope's stream (TAP-8)

const view = (await state()).view;
const bass = view.modules.find((m) => m.key === "bass")?.nodes.find((n) => n.label === "bass" && n.audio);
check(!!bass, `the Bass track's instrument is a box in the view (${bass?.id})`);
const asked = await post("/scope", { session, client: "taps-suite", module: "bass", node: bass?.id, on: true });
check(asked.status === 200 && asked.body.scope?.channels >= 1, `asked for, its scope answers with its channels (${JSON.stringify(asked.body)})`);
let from = heard.length;
await sleep(1500);
const shown = heard.slice(from).filter((h) => h.m.scopes?.some((s) => s.module === "bass" && s.node === bass?.id));
const frames = shown.at(-1)?.m.scopes.find((s) => s.node === bass?.id)?.frames;
check(shown.length >= 10, `its frames arrive with the meters while it is asked for (${shown.length} messages in 1.5 s)`);
check(frames?.length === asked.body.scope?.channels && frames.every((c) => c.length === 1024), `the last 1,024 frames, a list per channel (${frames?.map((c) => c.length)})`);
const refused = await post("/scope", { session, client: "taps-suite", module: "bass", node: view.modules.find((m) => m.key === "bass")?.nodes.find((n) => n.label === "lfo")?.id, on: true });
check(refused.status >= 400 && /a scope shows sound/.test(refused.body.error ?? ""), `a box that isn't sound is refused in words: "${refused.body.error}"`);
await post("/scope", { session, client: "taps-suite", module: "bass", node: bass?.id, on: false });
await sleep(300);
from = heard.length;
await sleep(800);
check(heard.length - from > 5 && heard.slice(from).every((h) => !h.m.scopes), "stopped, its frames stop; the meters go on");

// ---------------------------------------------------------------- the song's loudness at export (TAP-7)

await page.keyboard.press("Space");
await until(async () => !(await playing()), 3000);
const exported = await post("/export", { path: `${ROOT}/taps.wav` });
check(exported.status === 200, `an export starts (${JSON.stringify(exported.body)})`);
check(await until(async () => typeof (await state()).loudness?.lufs === "number", 30000), `done, the state carries its loudness (${JSON.stringify((await state()).loudness)})`);
const measured = (await state()).loudness.lufs;
await key("Shift+Meta+e");
await page.waitForSelector("[data-testid=export-dialog]", { timeout: 5000 }).catch(() => {});
const words = await page.$eval("[data-testid=export-loudness]", (e) => e.textContent).catch(() => "");
check(words === `The song's loudness is −${Math.abs(measured)} LUFS, measured when it was last exported.`, `the Export dialog says it: "${words}"`);
await page.keyboard.press("Escape");
await page.screenshot({ path: `${ROOT}/taps.png` });

done(errors);

// Taps in the app (TAP-5 to TAP-8, gaps #20): a fader dragged down while the song plays moves its
// strip's meter down by what it moved, as the server's channel meter does; Master's strip says the
// song's loudness in LUFS, which the Mix view asks for while it shows, as the meters' message does;
// an export's loudness shows in the Export dialog; and a scope's frames arrive over the socket of
// the window that asked while it asks, and stop when it stops.
//
//   node e2e/taps.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { ROOT, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, key, box } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// The page's meters and scopes messages, as they arrive (CDP), and the window's client id, from
// its socket's address.
const heard = [];
const scoped = [];
let client = null;
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketCreated", ({ url }) => {
  client = new URL(url).searchParams.get("client") ?? client;
});
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") heard.push({ at: Date.now(), m });
    if (m.t === "scopes") scoped.push({ at: Date.now(), m });
  } catch {}
});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-testid=timeline] [data-track="keys"]', { timeout: 10000 });

const FLOOR = -60;
const db = (x) => (x > 0 ? 20 * Math.log10(x) : -Infinity);
const strip = (id) => `[data-testid=mix] [data-strip="${id}"]`;
const playing = async () => (await api("/status")).body.playing;
/**
 * Over `ms`: the loudest the server's meters said of the Keys channel (after its fader), in dB;
 * the Keys strip's and header's meters read at the same moments, in whole dB (one loop draws
 * every meter, so two of one level agree); and the loudest the strip read.
 */
const listen = async (ms) => {
  const from = heard.length;
  const reads = [];
  const t = Date.now();
  while (Date.now() - t < ms) {
    reads.push(
      await page.evaluate(() => {
        const read = (sel) => Number(document.querySelector(sel)?.getAttribute("aria-valuenow") ?? -60);
        return { strip: read('[data-testid=mix] [data-strip="keys"] [role=meter]'), head: read('[data-testid=timeline] [data-track="keys"] [role=meter]') };
      }),
    );
    await sleep(40);
  }
  const server = Math.max(...heard.slice(from).map((h) => db(h.m.channels?.keys?.[0] ?? 0)));
  return { server, reads, strip: Math.max(...reads.map((r) => r.strip)) };
};
/** Whether the strip's and the header's meters read alike at every read. */
const alike = (l) => l.reads.every((r) => Math.abs(r.strip - r.head) <= 1);

// The first bar, looped, so what plays repeats while the fader moves.
const session = (await state()).session;
await post("/loop", { start: 0, end: 4, on: true });
await key("Alt+3");
check(await until(async () => !!(await page.$(strip("master")))), "⌥3 shows the Mix view");
await page.keyboard.press("Space");
check(await until(playing, 3000), "Space plays the first bar, looped");
await sleep(2500);

// ---------------------------------------------------------------- a fader dragged down (TAP-5)

const before = await listen(2500);
check(before.server > -40 && before.strip > FLOOR && alike(before), `the Keys channel sounds; its strip's and header's meters show it, alike at every read (${before.server.toFixed(1)} dB; the strip up to ${before.strip} dB)`);
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
const after = await listen(2500);
check(Math.abs(after.server - before.server - drop) < 1, `the server's Keys meter fell by the fader's move: ${(after.server - before.server).toFixed(1)} dB for ${drop.toFixed(1)}`);
check(
  after.strip > FLOOR && after.strip <= Math.round(after.server) + 1 && after.strip <= before.strip - 6,
  `and the strip's meter fell with it: up to ${before.strip} dB, then up to ${after.strip} (the server's ${after.server.toFixed(1)})`,
);
check(alike(after), `as did the Keys header's meter, alike at every read: its channel is its own (${after.reads.slice(0, 8).map((r) => `${r.strip}/${r.head}`).join(" ")})`);
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
const asked = await post("/scope", { session, client, module: "bass", node: bass?.id, on: true });
check(asked.status === 200 && asked.body.scope?.channels >= 1, `asked for by this window, its scope answers with its channels (${JSON.stringify(asked.body).slice(0, 120)})`);
let from = scoped.length;
let met = heard.length;
await sleep(1500);
const shown = scoped.slice(from).filter((h) => h.m.scopes?.some((s) => s.module === "bass" && s.node === bass?.id));
const frames = shown.at(-1)?.m.scopes.find((s) => s.node === bass?.id)?.frames;
check(shown.length >= 10 && heard.slice(met).every((h) => !h.m.scopes), `its frames come to this window's socket alone, beside the meters (${shown.length} messages in 1.5 s)`);
check(frames?.length === asked.body.scope?.channels && frames.every((c) => c.length === 1024), `the last 1,024 frames, a list per channel (${frames?.map((c) => c.length)})`);
const refused = await post("/scope", { session, client, module: "bass", node: view.modules.find((m) => m.key === "bass")?.nodes.find((n) => n.label === "lfo")?.id, on: true });
check(refused.status >= 400 && /a scope shows sound/.test(refused.body.error ?? ""), `a box that isn't sound is refused in words: "${refused.body.error}"`);
await post("/scope", { session, client, module: "bass", node: bass?.id, on: false });
await sleep(300);
from = scoped.length;
met = heard.length;
await sleep(800);
check(heard.length - met > 5 && scoped.length === from, "stopped, its frames stop; the meters go on");

// The loudness is measured only while the Mix view shows (TAP-4, TAP-6).
await key("Alt+1");
await sleep(600);
met = heard.length;
await sleep(600);
check(heard.length - met > 5 && heard.slice(met).every((h) => !h.m.loudness), "the Mix view gone, the meters carry no loudness: nothing measures it");

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

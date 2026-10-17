// 8b.6 Audition in Notes (design-8b §8.4, taste.md NT-2, NT-3; the 8b-ui plan's U9): a note sounds
// while it is pressed, a drag plays each pitch it crosses, the keyboard's keys are playable, the
// toggle, every way a held note is let go (pointer up, the toggle, ⌘., Space with no pointer down,
// the page closing), the header's hints (muted, muted in the song, silenced by a solo, naming only
// the solos still in the song, a lane nothing plays), a read-only roll that still sounds, a PC page's words, and a note heard while
// stopped on a server of the suite's own that plays into a file (M2_AUDIO_CAPTURE).
//
// M2_AUDIO=none plays in real time with no device, so the server's `live` (notes sounding, in
// /api/graph/status) says what sounds. What the page sends is read from its WebSocket frames (CDP).
//
//   node e2e/s8-audition.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import puppeteer from "puppeteer-core";
import { spawn } from "node:child_process";
import fs from "node:fs";
import { HEADLESS, REPO } from "./env.mjs";
import { exe } from "./chrome.mjs";
import { ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { page, errors, api, state, box, key } = await open(1440, 900);
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

// What the page sends: its audition messages, in order.
const sent = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameSent", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "audition") sent.push(m);
  } catch {}
});
// Reloaded, so the page's socket is made with the frames watched.
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });

const live = async () => (await api("/status")).body.live;
const liveIs = (n, ms = 3000) => until(async () => (await live()) === n, ms);
const ons = (msgs) => msgs.flatMap((m) => m.on ?? []);
const offs = (msgs) => msgs.flatMap((m) => m.off ?? []);
const PRESS = "rgb(220, 223, 227)";
const ACCENT_SOFT = "rgb(224, 234, 251)";

/** Open the first clip of lane `lane` (`bass.notes`) in Notes. */
async function openClip(p, lane) {
  const b = await p.$(`[data-lane="${lane}"] :nth-child(1 of [data-clip])`).then((e) => e?.boundingBox());
  if (!b) return false;
  await p.mouse.click(b.x + Math.min(40, b.width / 2), b.y + b.height / 2, { count: 2 });
  return until(async () => (await p.$$("[data-testid=notes] [data-note]")).length > 0, 5000);
}
/** The roll's notes wholly in view (below its ruler, right of its keys), with a place to press each. */
const visibleNotes = (p = page) =>
  p.evaluate(() => {
    const s = document.querySelector("[data-testid=roll-scroller]").getBoundingClientRect();
    return [...document.querySelectorAll("[data-testid=notes] [data-note]")]
      .map((e) => ({ e, r: e.getBoundingClientRect() }))
      .filter(({ r }) => r.top >= s.top + 34 && r.bottom <= s.bottom - 4 && r.left >= s.left + 60 && r.right <= s.right - 4 && r.width > 12)
      .map(({ e, r }) => ({ pitch: Number(e.dataset.pitch), velocity: Number(e.dataset.velocity), x: r.left + 5, y: r.top + r.height / 2, top: r.top, s: s.top }));
  });
/** Where to press key `pitch`: its white part, right of where a black key ends. */
const keyAt = (pitch, p = page) => p.$eval(`[data-key="${pitch}"]`, (e) => { const r = e.getBoundingClientRect(); return { x: r.left + 46, y: r.top + r.height / 2 }; });
const sounding = () => page.$$eval("[data-testid=notes] [data-key][data-sounding]", (els) => els.map((e) => Number(e.dataset.key)));
const keyFill = (pitch) => page.$eval(`[data-key="${pitch}"] > div`, (e) => getComputedStyle(e).backgroundColor);
const hint = () => page.$eval("[data-testid=audition-hint]", (e) => e.textContent).catch(() => null);
const toggle = () =>
  page.$eval("[data-testid=audition]", (e) => ({ on: e.getAttribute("aria-pressed") === "true", bg: getComputedStyle(e).backgroundColor, title: e.title })).catch(() => ({ on: null, bg: "", title: "" }));
/** The toggle clicked as a mouse would, or by a script while the mouse holds a key elsewhere. */
const clickToggle = () => page.click("[data-testid=audition]").catch(() => {});
const flipToggle = () => page.$eval("[data-testid=audition]", (b) => b.click()).catch(() => {});

// ---------------------------------------------------------------- the toggle (NT-2)

check(await openClip(page, "bass.notes"), "double-clicking Bass's notes clip opens it in Notes");
const t0 = await toggle();
check(t0.on && t0.bg === ACCENT_SOFT, `the Audition toggle is in the Notes header, on, in accent-soft (${JSON.stringify(t0)})`);
check(!!t0.title && t0.title.startsWith("Hear notes as you play them") && t0.title.includes("⌘. stops all sound"), `its tooltip says what it does and how to stop all sound: ${t0.title}`);
check(!(await hint()), "no hint while Bass is heard");

// ---------------------------------------------------------------- a note sounds while pressed

const notes0 = await visibleNotes();
const n0 = notes0[0];
let from = sent.length;
await page.mouse.move(n0.x, n0.y);
await page.mouse.down();
check(await liveIs(1), "a press on a note: one live note sounds (status live 1)");
const on0 = ons(sent.slice(from));
check(on0.length === 1 && on0[0].pitch === n0.pitch && Math.abs(on0[0].velocity - n0.velocity) < 1e-9 && sent[from]?.module === "bass" && sent[from]?.lane === "notes", `at its own pitch and velocity, on bass.notes (${JSON.stringify(sent.slice(from))})`);
check((await sounding()).join() === String(n0.pitch) && (await keyFill(n0.pitch)) === PRESS, `its key is drawn in press while it sounds (${await sounding()})`);
await page.mouse.up();
check(await liveIs(0), "letting go ends it (live 0)");
check(offs(sent.slice(from)).length === 1 && (await sounding()).length === 0, "with one Off, and the key unlit");
check((await keyFill(n0.pitch)) !== PRESS, `the selected note's key keeps the track's colour, not press (${await keyFill(n0.pitch)})`);

// ---------------------------------------------------------------- a drag across three pitches

// A note with two rows free to drag up into, in view.
const n1 = (await visibleNotes()).find((n) => n.top - 28 > n.s + 34) ?? n0;
from = sent.length;
await page.mouse.move(n1.x, n1.y);
await page.mouse.down();
await page.mouse.move(n1.x, n1.y - 28, { steps: 14 });
const during = await live();
await page.mouse.up();
const ended = await liveIs(0);
await sleep(150);
const drag = sent.slice(from);
check(
  ons(drag).map((s) => s.pitch).join() === [n1.pitch, n1.pitch + 1, n1.pitch + 2].join() && offs(drag).length === 3,
  `dragging a note up two rows plays each pitch it crosses: 3 Ons (${ons(drag).map((s) => s.pitch)}) and 3 Offs (${offs(drag).length})`,
);
check(during === 1 && ended, `one note sounds at a time during the drag (${during}), none after`);
// Back where it was: the drag wrote a move.
await key("Meta+z");
await sleep(400);

// ---------------------------------------------------------------- the keyboard is an instrument (NT-3)

const k = n0.pitch;
from = sent.length;
const at = await keyAt(k);
await page.mouse.move(at.x, at.y);
await page.mouse.down();
check(await liveIs(1), "a held key on the keyboard sounds");
const kOn = ons(sent.slice(from));
check(kOn.length === 1 && kOn[0].pitch === k && kOn[0].velocity === 0.8, `at its pitch, velocity 0.8 (${JSON.stringify(kOn)})`);
check((await sounding()).join() === String(k) && (await keyFill(k)) === PRESS, "and is drawn in press while held");
await page.mouse.move(at.x, at.y + 28, { steps: 8 });
const kDrag = ons(sent.slice(from)).map((s) => s.pitch);
check(kDrag.join() === [k, k - 1, k - 2].join() && (await live()) === 1, `a drag down the keys plays each key it reaches, one at a time (${kDrag})`);
await page.mouse.up();
check(await liveIs(0), "letting go of the keys ends it");

// ---------------------------------------------------------------- the toggle off

await clickToggle();
await page.mouse.move(10, 10);
await sleep(200);
const t1 = await toggle();
check(t1.on === false && t1.bg !== ACCENT_SOFT, `a click turns the toggle off, out of accent-soft (${JSON.stringify(t1)})`);
check(t1.on !== null && !(await page.evaluate(() => document.activeElement?.dataset?.testid === "audition")), "and it doesn't take focus (Space stays the transport)");
from = sent.length;
await page.mouse.move(n0.x, n0.y);
await page.mouse.down();
await sleep(300);
const offLive = await live();
await page.mouse.up();
check(sent.length === from && offLive === 0, `with it off, a press on a note sends nothing and nothing sounds (${sent.length - from} sent, live ${offLive})`);
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-lane="bass.notes"] [data-clip]', { timeout: 10000 });
await openClip(page, "bass.notes");
check((await toggle()).on === false, "a reload keeps it off (this browser's storage)");
await clickToggle();
check((await toggle()).on, "and a click turns it on again");

// Off while a key is held: everything held is let go (M4).
await page.mouse.move(at.x, at.y);
await page.mouse.down();
const held1 = await liveIs(1);
await flipToggle();
check(held1 && (await liveIs(0)) && (await sounding()).length === 0, "turning it off while a key is held lets go at once, and unlights the key");
await page.mouse.up();
await flipToggle();

// ---------------------------------------------------------------- ⌘. and Space (M4)

await page.mouse.move(at.x, at.y);
await page.mouse.down();
const held2 = await liveIs(1);
await key("Meta+.");
check(held2 && (await liveIs(0)) && (await until(async () => (await sounding()).length === 0, 2000)), "⌘. (Stop All Sound) ends the held key and unlights it");
from = sent.length;
await page.mouse.move(at.x, at.y + 28, { steps: 4 });
await sleep(200);
check(ons(sent.slice(from)).length === 0 && (await live()) === 0, "and the drag plays no more until the next press");
await page.mouse.up();

// A held key plays on through play and stop (a hand on a key, design-8b §8.1).
await page.evaluate(() => window.addEventListener("pointerdown", (e) => (window.__pointer = e.pointerId), { capture: true, once: true }));
await page.mouse.move(at.x, at.y);
await page.mouse.down();
await liveIs(1);
await page.keyboard.press("Space");
check(await until(async () => (await api("/status")).body.playing, 3000), "Space plays the song while a key is held");
await page.keyboard.press("Space");
await until(async () => !(await api("/status")).body.playing, 3000);
check((await live()) === 1, "and the key sounds on through play and stop (Space with a pointer down lets nothing go)");
// The pointer's up lost (it went to another window): Space with no pointer down lets go of what is stuck.
await page.evaluate(() => window.dispatchEvent(new PointerEvent("pointerup", { pointerId: window.__pointer })));
const stuck = await live();
await page.keyboard.press("Space");
check(stuck === 1 && (await liveIs(0)), "Space with no pointer down lets go of a stuck note");
await page.keyboard.press("Space");
await until(async () => !(await api("/status")).body.playing, 3000);
await page.mouse.up();

// ---------------------------------------------------------------- read only still sounds

const bass = read("tracks/bass.ts");
write("tracks/bass.ts", `${bass}\nconst broken = ;\n`);
check(!!(await page.waitForSelector("[data-testid=notes-locked]", { timeout: 8000 }).catch(() => null)), "with the code broken the roll is read only");
await page.mouse.move(n0.x, n0.y);
await page.mouse.down();
check(await liveIs(1), "and a press on a note still sounds (audition never needs the code to compile)");
await page.mouse.up();
check(await liveIs(0), "and ends on letting go");
write("tracks/bass.ts", bass);
await until(() => page.$("[data-testid=notes-locked]").then((e) => !e), 8000);

// ---------------------------------------------------------------- the hints (design-8b §6.7)

const session = (await state()).session;
await post("/overlay", { session, solo: { bass: true } });
check(await openClip(page, "keys.chords"), "the Keys clip opens in Notes");
check(await until(async () => (await hint()) === "Silenced by the Bass solo · audition is silent"), `with Bass soloed, the Keys roll says: ${await hint()}`);
const hintStyle = await page.$eval("[data-testid=audition-hint]", (e) => { const s = getComputedStyle(e); return `${s.fontSize} ${s.color}`; }).catch(() => "no hint");
check(hintStyle === "11px rgb(101, 105, 111)", `in text-xs fg-muted (${hintStyle})`);
// Hats soloed too, then taken out of the song: its solo stays in the file (MON-4) but no longer
// counts, so the hint names only Bass's, as the mutes pill counts one solo.
await post("/overlay", { session, solo: { hats: true } });
const songTs = read("song.ts");
const noHats = songTs.replace('import hats from "./tracks/hats";\n', "").replace("      hats: { in: hats.out, db: db(-6), pan: 0.3 },\n", "");
write("song.ts", noHats);
check(noHats !== songTs && (await until(async () => !(await state()).timeline.tracks.some((t) => t.module === "hats"), 10000)), "Hats, soloed, is taken out of song.ts");
check(
  (await until(async () => (await hint()) === "Silenced by the Bass solo · audition is silent", 3000)) && (await page.$eval("[data-testid=overlay-pill]", (e) => e.textContent).catch(() => null)) === "1 solo",
  `the Keys roll still names only Bass's solo, as the pill counts one: ${await hint()}`,
);
write("song.ts", songTs);
await until(async () => (await state()).timeline.tracks.some((t) => t.module === "hats"), 10000);
await post("/overlay", { session, solo: { hats: false } });
await post("/overlay", { session, mute: { keys: true } });
check(await until(async () => (await hint()) === "Muted · audition is silent"), `Keys muted: ${await hint()}`);
await post("/overlay", { session, clear: true });
const r = await post("/track/mute", { session, modules: ["keys"], muted: true });
check(r.status === 200 && (await until(async () => (await hint()) === "Muted in the song · audition is silent")), `Keys muted in the song: ${await hint()}`);
await post("/track/mute", { session, modules: ["keys"], muted: false });
check(await until(async () => (await hint()) === null), "heard again: no hint");
await flipToggle();
await post("/overlay", { session, mute: { keys: true } });
await sleep(500);
check((await hint()) === null, "with the toggle off, no hint: nothing would be heard anyway");
await flipToggle();
await post("/overlay", { session, clear: true });

// A lane nothing reads: Keys plays a lane of its own, so its chords are read by nothing.
const keysTs = read("tracks/keys.ts");
write("tracks/keys.ts", keysTs.replace('pluck(lane.notes("chords"))', '{\n  lane.notes("chords");\n  return pluck(lane.notes("spare"));\n}'));
await until(async () => (await state()).timeline.tracks.find((t) => t.module === "keys")?.lanes.some((l) => l.key === "spare"), 8000);
const kn = (await visibleNotes())[0];
await page.mouse.move(kn.x, kn.y);
await page.mouse.down();
check(await until(async () => (await hint()) === "Nothing plays this lane", 3000), `a press where nothing reads the lane: ${await hint()}`);
check((await live()) === 0, "and nothing sounds");
await page.mouse.up();
write("tracks/keys.ts", keysTs);
await until(async () => !(await state()).timeline.tracks.find((t) => t.module === "keys")?.lanes.some((l) => l.key === "spare"), 8000);
await page.mouse.move(kn.x, kn.y);
await page.mouse.down();
check(await liveIs(1), "read again, the lane sounds");
await page.mouse.up();
check(await until(async () => (await hint()) === null, 3000), "and the hint goes");

// ---------------------------------------------------------------- a PC page, and closing a page

const pc = await open(1440, 900, { pc: true });
check(await openClip(pc.page, "bass.notes"), "a PC page opens Bass's clip");
const pcTitle = await pc.page.$eval("[data-testid=audition]", (e) => e.title).catch(() => "");
check(pcTitle.includes("Ctrl+. stops all sound"), `its toggle's tooltip has the PC's words: ${pcTitle}`);
const pk = await keyAt(n0.pitch, pc.page);
await pc.page.mouse.move(pk.x, pk.y);
await pc.page.mouse.down();
const held3 = await liveIs(1);
check(held3, "a key held there sounds");
const closed = Date.now();
await pc.browser.close();
check(held3 && (await liveIs(0, 1000)), `closing that page lets go of its note within a second (${Date.now() - closed} ms)`);

// ---------------------------------------------------------------- heard, while stopped

await heardWhileStopped();

done(errors);

/** A server of the suite's own that plays into a file: a key held while stopped is heard there. */
async function heardWhileStopped() {
  const root = `${ROOT}/capture`;
  fs.rmSync(root, { recursive: true, force: true });
  fs.mkdirSync(`${root}/home/.music2`, { recursive: true });
  fs.cpSync(`${REPO}/examples/first-song`, `${root}/proj`, { recursive: true });
  const capture = `${root}/played.f32`;
  const sessFile = `${root}/home/.music2/session.json`;
  const env = { ...process.env, M2_AUDIO: "none", M2_MIDI: "none", M2_AUDIO_CAPTURE: capture, HOME: `${root}/home` };
  const server = spawn(HEADLESS, [`${root}/proj`, sessFile, `a${Date.now()}`], { env, stdio: ["ignore", "ignore", fs.openSync(`${root}/headless.log`, "w")] });
  const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
  try {
    if (!(await until(() => fs.existsSync(sessFile), 20_000))) return check(false, "the capture server started");
    await sleep(300);
    const s2 = JSON.parse(fs.readFileSync(sessFile, "utf8"));
    const p2 = await browser.newPage();
    await p2.setViewport({ width: 1440, height: 900 });
    await p2.evaluateOnNewDocument((token) => {
      window.__M2__ = { native: false, token, platform: "macos" };
    }, s2.token);
    await p2.goto(s2.url, { waitUntil: "domcontentloaded" });
    await p2.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
    await openClip(p2, "bass.notes");
    const frames = () => Math.floor(fs.statSync(capture).size / 8);
    const level = (a, b) => {
      const buf = fs.readFileSync(capture);
      const x = new Float32Array(buf.buffer, buf.byteOffset, Math.floor(buf.length / 4));
      let sum = 0, n = 0;
      for (let i = 2 * a; i < Math.min(2 * b, x.length); i++, n++) sum += x[i] * x[i];
      return 10 * Math.log10(sum / Math.max(n, 1) + 1e-24);
    };
    await until(() => fs.existsSync(capture), 5000);
    const f0 = frames();
    await sleep(500);
    const f1 = frames();
    const pitch = (await visibleNotes(p2))[0].pitch;
    const k2 = await keyAt(pitch, p2);
    await p2.mouse.move(k2.x, k2.y);
    await p2.mouse.down();
    await sleep(600);
    const f2 = frames();
    await p2.mouse.up();
    await sleep(800);
    const f3 = frames();
    const quiet = level(f0, f1), held = level(f1 + 4800, f2), after = level(f3 - 9600, f3);
    check(quiet < -100 && held > -40, `stopped, a held key is heard in what the player plays (${quiet.toFixed(0)} dB before, ${held.toFixed(0)} dB held)`);
    check(after < held - 30, `and falls silent once let go (${after.toFixed(0)} dB)`);
  } finally {
    await browser.close();
    server.kill();
  }
}

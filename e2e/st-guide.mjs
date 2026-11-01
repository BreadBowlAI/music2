// Make Your First Song (the studio plan's D3, Q47): Help ▸ Make Your First Song opens a card at the
// top of the side browser; its seven steps tick themselves as the song is made (drums, a beat, a
// bass, bass notes, a listen, a sound changed, an export: here as a MIDI file); kept in
// `.music2/view.json`; the done line says where the file is (in the app, the one its save panel
// picked: the review's F6); the fold, ×, and the start screen's link.
//
//   node e2e/st-guide.mjs <root> <home>     (the `none` kind: no song open; it makes an Empty one)
import fs from "node:fs";
import { HOME, check, done, open, sess, sleep, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";

const HDR = { "x-m2-token": sess.page_token, "content-type": "application/json" };
const made = await (await fetch(`${sess.url}/api/project/new`, { method: "POST", headers: HDR, body: JSON.stringify({ name: "First", template: "empty" }) })).json();
const DIR = made.dir;
check(!!DIR && made.timeline?.tracks?.every((t) => !t.track), `an Empty song opens: ${DIR}`);
const viewFile = () => {
  try {
    return JSON.parse(fs.readFileSync(`${DIR}/.music2/view.json`, "utf8"));
  } catch {
    return {};
  }
};

// ---------------------------------------------------------------- Help ▸ Make Your First Song

const { browser, page, errors, key, box, view } = await open(1440, 900, { browser: false });

/** A page as the desktop app's: its menu commands come with `window.__menu(id)`; `answers` are what its actions answer. */
async function appPage(answers = {}) {
  const context = await browser.createBrowserContext();
  const p = await context.newPage();
  await p.setViewport({ width: 1440, height: 900 });
  p.on("pageerror", (e) => errors.push(`app page: ${e}`));
  await p.evaluateOnNewDocument((token, answers) => {
    window.__M2__ = { native: true, token, platform: "macos" };
    try {
      localStorage.setItem("m2.browser.open", "false");
    } catch {}
    const callbacks = new Map();
    const menu = [];
    let next = 1;
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        if (cmd === "plugin:event|listen" && args?.event === "menu") menu.push(args.handler);
        if (cmd === "action" && args?.id in answers) return answers[args.id];
        return null;
      },
      transformCallback: (cb) => {
        callbacks.set(next, cb);
        return next++;
      },
      unregisterCallback: (id) => callbacks.delete(id),
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__menu = (id) => menu.forEach((h) => callbacks.get(h)?.({ event: "menu", id: 0, payload: id }));
  }, sess.page_token, answers);
  await p.goto(sess.url, { waitUntil: "domcontentloaded" });
  await p.waitForSelector("[data-testid=timeline]", { timeout: 10000 });
  return p;
}

const app = await appPage();
check(!(await app.$("[data-browser]")) && !(await app.$("[data-testid=guide]")), "the side browser is hidden, with no guide");
await app.evaluate(() => window.__menu("guide"));
const card = await app.waitForSelector("[data-browser] [data-testid=guide]", { timeout: 3000 }).catch(() => null);
check(!!card, "Help ▸ Make Your First Song shows the browser, the guide at its top");
const steps = (p = page) => p.$$eval("[data-testid=guide] [data-step]", (els) => els.map((e) => ({ id: e.dataset.step, done: e.dataset.done !== undefined, now: e.dataset.current !== undefined, text: e.innerText.replace(/\s+/g, " ").trim() })));
const s0 = await steps(app);
check(
  s0.length === 7 && s0.every((s) => !s.done) && s0[0].now && s0[0].text === "1 Add drums. Press ⌘T and pick a drum kit, such as Basic kit.",
  `seven steps, none ticked, the first the current one, with what to do: ${s0[0]?.text}`,
);
check(s0[1].text === "2 Make a beat." && !s0[1].now, `the others just their title: ${s0[1].text}`);
const first = await app.$eval("[data-testid=guide]", (e) => {
  const b = e.closest("[data-browser]").getBoundingClientRect(), c = e.getBoundingClientRect();
  return { top: Math.round(c.top - b.top), width: Math.round(c.width), browser: Math.round(b.width) };
});
check(first.top <= 8 && first.width >= first.browser - 17, `the card is first in the browser, its width: ${JSON.stringify(first)}`);
check(await until(() => viewFile().guide?.open === true && viewFile().guide.done.length === 0), `kept in .music2/view.json: ${JSON.stringify(viewFile().guide)}`);
await app.close();

// ---------------------------------------------------------------- the steps tick themselves

await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=timeline]");
await page.waitForSelector("[data-testid=guide]");
const ticked = async (id) => until(async () => (await steps()).find((s) => s.id === id)?.done, 10000);
const current = async () => (await steps()).find((s) => s.now)?.id ?? null;
const clean = async (sel, what) => {
  const bad = [...new Set([...(await page.evaluate(lint, sel)), ...(await page.evaluate(tips, sel))])];
  check(bad.length === 0, `${what}: the taste lint and the tooltip lint find nothing${bad.length ? ` (${bad.slice(0, 6).join("; ")})` : ""}`);
};
await clean("[data-testid=guide]", "the guide as it opens");
const overlap = async () => {
  const [c, t] = [await box("[data-testid=guide]"), await box("[data-testid=timeline]")];
  return !(c.x + c.width <= t.x || t.x + t.width <= c.x || c.y + c.height <= t.y || t.y + t.height <= c.y);
};
check(!(await overlap()), "the card never covers the timeline");

/** Add a track with the instrument `name` from + Track's list. */
const addTrack = async (name) => {
  await key("Meta+t");
  await page.waitForSelector("[data-testid=instrument-list]");
  await page.keyboard.type(name);
  await sleep(150);
  await page.keyboard.press("Enter");
};
/** Double-click `module`'s lane for a clip, then `n` notes in Notes, a beat apart. */
const notes = async (module, lane, n) => {
  await page.waitForSelector(`[data-lane="${module}.${lane}"]`, { timeout: 6000 });
  const l = await box(`[data-lane="${module}.${lane}"] > div:last-child`);
  await page.mouse.click(l.x + 30, l.y + l.height / 2, { count: 2 });
  await page.waitForSelector("[data-testid=roll-grid]");
  await sleep(300);
  const g = await box("[data-testid=roll-grid]");
  const s = await box("[data-testid=roll-scroller]");
  const y = Math.max(g.y, s.y) + Math.min(s.height, g.height) / 2;
  // A beat apart, inside the one-bar clip (its end is a quarter of the grid or more).
  const beat = Math.min(g.width / 8, 120);
  for (let i = 0; i < n; i++) {
    await page.mouse.click(g.x + 6 + i * beat, y, { count: 2 });
    await sleep(250);
  }
};

await addTrack("basic kit");
check(await ticked("drums"), "1. a Basic kit added: Add drums ticks");
check((await current()) === "beat", "and Make a beat is the current step");
await notes("kick", "hits", 4);
check(await ticked("beat"), "2. four drum notes: Make a beat ticks");
await clean("[data-testid=guide]", "the guide with two steps done");
await key("Escape");
await addTrack("synth bass");
check(await ticked("bass"), "3. a Synth bass added: Add a bass ticks");
await notes("synth-bass", "notes", 3);
check(await ticked("bassline"), "4. three bass notes: Play some bass notes ticks");
await key("Escape");
await page.click("[data-testid=timeline]");
await key("Escape");
await key("Space");
check(await ticked("listen"), "5. played: Listen ticks");
await key("Space");
// A knob in Device, dragged.
await view("graph");
const knob = await page.waitForSelector("[data-testid=graph] [data-knob-id] [role=slider]", { timeout: 6000 }).catch(() => null);
const kb = knob && (await knob.boundingBox());
if (kb) {
  await page.mouse.move(kb.x + kb.width / 2, kb.y + kb.height / 2);
  await page.mouse.down();
  await page.mouse.move(kb.x + kb.width / 2, kb.y + kb.height / 2 - 40, { steps: 8 });
  await page.mouse.up();
}
check(await ticked("sound"), "6. a knob turned: Change a sound ticks");
await key("Meta+Shift+e");
// The Export dialog's MIDI choice counts too.
await page.waitForSelector("[data-testid=export-dialog]", { timeout: 3000 });
await page.click("[data-export-choice=midi]");
await page.click("[data-testid=export-dialog-go]");
check(await ticked("export"), "7. exported as a MIDI file: Export it ticks");
const all = await steps();
check(all.every((s) => s.done) && !all.some((s) => s.now), "every step is done");
const doneLine = await page.$eval("[data-testid=guide-done]", (e) => e.innerText.trim()).catch(() => "");
check(doneLine.startsWith("You made a song. It is in your Exports folder."), `the done line: ${doneLine}`);
check(await until(() => viewFile().guide?.done?.length === 7), `view.json keeps the ticks: ${JSON.stringify(viewFile().guide)}`);
await clean("[data-testid=guide]", "the guide done");
// In the app the save panel can put the song anywhere: the done line names the file it saved.
fs.mkdirSync(`${HOME}/Desktop`, { recursive: true });
const app2 = await appPage({ export_path: `${HOME}/Desktop/First.wav` });
await app2.waitForSelector("[data-testid=guide-done]", { timeout: 5000 }).catch(() => null);
await app2.evaluate(() => window.__menu("export"));
await app2.waitForSelector("[data-testid=export-dialog]", { timeout: 3000 }).catch(() => null);
await app2.keyboard.press("Enter");
const appLine = async () => app2.$eval("[data-testid=guide-done]", (e) => e.innerText.replace(/\s+/g, " ").trim()).catch(() => "");
check(
  await until(async () => (await appLine()).startsWith("You made a song. It is saved as First.wav."), 15000) && fs.existsSync(`${HOME}/Desktop/First.wav`),
  `in the app, saved to the Desktop: the done line names the file (${await appLine()})`,
);
check((await appLine()).endsWith("Show in Finder"), `with Show in Finder: ${await appLine()}`);
await app2.close();

// ---------------------------------------------------------------- kept, folded, closed

await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=guide]");
check((await steps()).every((s) => s.done), "a reload keeps the ticks");
await page.click("[data-testid=guide-fold]");
check(await until(async () => (await page.$eval("[data-testid=guide-title]", (e) => e.innerText)) === "Your first song · 7 of 7" && !(await page.$("[data-testid=guide] [data-step]"))), "the fold makes it one line: Your first song · 7 of 7");
check(await until(() => viewFile().guide?.folded === true), "folded, kept");
await page.click("[data-testid=guide-fold]");
check(await until(async () => (await steps()).length === 7), "unfolded, the steps again");
await page.click("[data-testid=guide-close]");
check(await until(async () => !(await page.$("[data-testid=guide]"))), "× closes it");
check(await until(() => viewFile().guide?.open === false && viewFile().guide.done.length === 7), "closed, its ticks kept");

// ---------------------------------------------------------------- the start screen's link, and a PC

await fetch(`${sess.url}/api/graph/close`, { method: "POST", headers: HDR, body: "{}" });
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=start]");
const link = await page.waitForSelector("[data-testid=first-song]", { timeout: 4000 }).catch(() => null);
check((await link?.evaluate((e) => e.innerText.trim())) === "New to making music? Make your first song", "the start screen offers: New to making music? Make your first song");
await clean("[data-testid=start]", "the start screen");
await link?.click();
await page.waitForSelector("[data-testid=new-song-dialog]");
await page.waitForSelector("[data-testid=template-empty]");
check(await until(async () => (await page.$eval("[data-testid=template-empty]", (e) => e.getAttribute("aria-checked"))) === "true"), "it opens New Song with Empty chosen");
await page.click("[data-testid=create-song]");
const fresh = await page.waitForSelector("[data-testid=guide]", { timeout: 8000 }).catch(() => null);
check(!!fresh && (await steps()).every((s) => !s.done), "the new song opens with the guide, nothing ticked");
check(!(await overlap()), "beside the timeline, never over it");
// Once the guide is kept for the new song, a PC's page shows it too.
const local = async () => (await (await fetch(`${sess.url}/api/graph`, { headers: HDR })).json()).local?.guide;
check(await until(async () => (await local())?.open === true), "the new song keeps its guide open");
const pc = await open(1440, 900, { browser: true, pc: true });
await pc.page.waitForSelector("[data-testid=guide]");
const pcFirst = (await steps(pc.page))[0]?.text ?? "";
check(pcFirst === "1 Add drums. Press Ctrl+T and pick a drum kit, such as Basic kit.", `a PC's page says Ctrl+T: ${pcFirst}`);
await pc.browser.close();

await browser.close();
done(errors);

// A tooltip on every control (the studio plan's D1, Q43, D19): every place of the app, each of its
// screens, dialogs and popovers, walked with e2e/tiplib.mjs' `tips`, which lists each icon button,
// toggle, tab, slider, knob and fader with no tooltip, each tooltip that only repeats a control's
// words, and each control with a key whose tooltip doesn't name it. One check per place.
//
//   node e2e/st-tips.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";
import { tips } from "./tiplib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, state, show, view, key, box, status } = await open(1440, 900, { browser: true });
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });
const mid = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });

/** The controls with a key, and the key as a Mac page spells it: their tooltips name it. */
const MAC = {
  "[data-testid=play]": "(Space)",
  "[data-testid=loop-toggle]": "(C)",
  "[data-testid=metronome-toggle]": "(K)",
  "[data-testid=browser-toggle]": "(⌥⌘B)",
  "[data-testid=export]": "(⇧⌘E)",
  "button[aria-label=Undo]": "⌘Z)",
  "button[aria-label=Redo]": "⇧⌘Z)",
  "[data-testid=add-track]": "(⌘T)",
  "[data-testid=fold-all]": "(A)",
  "[data-testid=follow]": "(⇧⌘F)",
  "[data-testid=zoom-in]": "(⌘=)",
  "[data-testid=zoom-out]": "(⌘−)",
  "[data-view-tab=graph]": "(⌥1)",
  "[data-view-tab=notes]": "(⌥2)",
  "[data-view-tab=mix]": "(⌥3)",
  "[data-view-tab=code]": "(⌥4)",
  "[data-testid=piano-toggle]": "(⌘K)",
  "[data-tool=draw]": "(B)",
  'button[aria-label$="the dock"]': "(E)",
};

/** One place: every control under `sel` has its tooltip. */
const place = async (sel, what, keys = MAC, on = page) => {
  await sleep(200);
  const bad = [...new Set(await on.evaluate(tips, sel, keys))];
  const none = await on.$$eval(sel, (els) => els.length);
  check(none > 0 && bad.length === 0, `${what}: every control says what it does${none ? "" : " (nothing there)"}${bad.length ? ` (${bad.length}: ${bad.slice(0, 10).join("; ")})` : ""}`);
  return bad;
};

// First, that `tips` sees what it should: planted controls with and without tooltips.
const planted = await page.evaluate((tips) => {
  const box = document.createElement("div");
  box.id = "planted";
  box.innerHTML = `
    <button id="a"><svg width="10" height="10"></svg></button>
    <button id="b" title="Mute Kick (M)"><svg width="10" height="10"></svg></button>
    <button id="c">Done</button>
    <button id="d" title="Export">Export</button>
    <button id="e" aria-pressed="true">Snap</button>
    <button id="f" title="Loop">Loop</button>
    <div role="menu"><button id="g"><svg width="10" height="10"></svg></button></div>
    <div data-knob="Cutoff"><span>Cutoff</span><div role="slider" title="Cutoff: drag up or down">x</div></div>`;
  document.body.append(box);
  const bad = new Function(`return (${tips})`)()("#planted", { "#f": "(C)" });
  box.remove();
  return bad;
}, tips.toString());
check(
  planted.length === 4 &&
    planted.some((b) => b.startsWith("no tooltip") && b.includes("<button>") && !b.includes('"')) &&
    planted.some((b) => b.startsWith("a tooltip that repeats") && b.includes('"Export"')) &&
    planted.some((b) => b.startsWith("no tooltip") && b.includes('"Snap"')) &&
    planted.some((b) => b.startsWith("a tooltip without its key (C)")),
  `tips finds an icon with no tooltip, a tooltip that repeats its words, a toggle with none and a key not named, and passes the rest (${planted.join(" | ")})`,
);

// ---------------------------------------------------------------- the start screen and New Song

await post("/close", {});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=start]");
await page.waitForSelector("[data-testid=open-example]:not([disabled])");
await place("[data-testid=start]", "the start screen");
await page.click("[data-testid=new-song]");
await page.waitForSelector("[data-testid=new-song-dialog]");
await place("[data-testid=new-song-dialog]", "the New Song dialog");
await key("Escape");
await post("/open", { path: PROJ });
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]");

// ---------------------------------------------------------------- the song's window

await place("header", "the top bar");
await page.hover('[data-module-tab="kick"]');
await place("[data-module-tab]", "the track headers");
// Words a beginner reads: what M and S do, and a knob's tooltip leads with how to use it.
const ms = await page.$eval('[data-module-tab="hats"]', (e) => [e.querySelector("[data-mute]").title, e.querySelector("[data-solo]").title]);
check(ms[0] === "Mute while you listen (M)" && ms[1].startsWith("Solo: hear only the soloed tracks (S)"), `M and S say what they do: ${ms.join(" | ")}`);
await place("[data-testid=timeline]", "the timeline: its tools, grid chip, ruler and clips");
await place("[data-browser]", "the side browser");
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
await place("[data-testid=timeline]", "the timeline with Bass's automation shown");
await page.click('[data-fold="bass"]');
// A clip picked; Repeat… from its menu.
const picked = await box('[data-lane="bass.notes"] [data-clip]');
await page.mouse.click(picked.x + 20, picked.y + picked.height / 2);
await place("[data-testid=timeline]", "the timeline with a clip picked");
await page.mouse.click(picked.x + 20, picked.y + picked.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
await page.click('[data-testid=clip-menu] [data-menu-item="repeat"]');
await page.waitForSelector("[data-testid=repeat-popover]");
await place("[data-testid=repeat-popover]", "the Repeat popover");
await key("Escape");
// A marker, from the ruler's menu: its name field, then its flag.
const ruler = await box("[data-testid=ruler]");
await page.mouse.click(ruler.x + 300, ruler.y + ruler.height - 6, { button: "right" });
await page.waitForSelector("[data-testid=ruler-menu]");
await page.click("[data-testid=ruler-menu] [data-menu-item=add-marker]");
await page.waitForSelector("[data-testid=marker-field]");
await place("[data-testid=marker-field]", "a marker's name field");
await key("Enter");
await page.waitForSelector("[data-marker-name]");
await place("[data-testid=timeline]", "the timeline with a marker");
await place("[data-testid=dock-header]", "the dock's header");
const knobTip = await page.$eval("[data-testid=graph] [data-knob-id]", (e) => e.title).catch(() => "");
check(knobTip.startsWith("Drag (⇧ for fine)") && /\nIn your code: tracks\/\w+\.ts, line \d+ \(/.test(knobTip), `a knob's tooltip leads with how to use it, then where it is in the code: ${knobTip.replace(/\n/g, " / ")}`);
for (const m of ["kick", "hats", "keys", "song", "bass"]) {
  await show(m);
  await place("[data-testid=graph]", `the Device view of ${m}`);
}

// A knob's menu, the value typed in.
const drive = mid(await box('[data-knob="drive"]'));
await page.mouse.click(drive.x, drive.y, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu]");
await place("[data-testid=knob-menu]", "a knob's menu");
await key("Escape");

// The effect browser.
await show("keys");
await page.click('[aria-label^="Add an effect after"]');
await page.waitForSelector("[data-effect-browser]");
await place("[data-effect-browser]", "the effect browser");
await key("Escape");
await show("bass");

// Notes: a clip open, with its tools.
const clip = await box('[data-lane="bass.notes"] [data-clip]');
await page.mouse.click(clip.x + 20, clip.y + clip.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=roll-grid] [data-note]");
await place("#dock", "Notes, its tools and keys");
const note = await box("[data-testid=roll-grid] [data-note]");
await page.mouse.click(note.x + 3, note.y + note.height / 2, { button: "right" });
if (await page.waitForSelector("[data-testid=note-menu]", { timeout: 2000 }).catch(() => null)) await place("[data-testid=note-menu]", "a note's menu");
else check(false, "a note's menu opens");
await key("Escape");

// Mix, and a strip's menu.
await view("mix");
await page.waitForSelector("[data-testid=mix] [data-strip]");
await place("#dock", "the Mix view and its strips");
// Code and its file list.
await view("code");
await page.waitForSelector("[data-testid=code] .view-lines");
await place("#dock", "the Code view and its file list");
await page.click('[data-testid=code] [data-file="tracks/bass.ts"]', { button: "right" });
await page.waitForSelector("[data-testid=change-track-id]");
await page.click("[data-testid=change-track-id]");
await page.waitForSelector("[data-testid=track-id-dialog]");
await place("[data-testid=track-id-dialog]", "the Change track ID dialog");
await key("Escape");
await view("graph");

// ---------------------------------------------------------------- dialogs and lists

await key("Meta+Shift+KeyS");
await page.waitForSelector("[data-testid=save-as-dialog]");
await place("[data-testid=save-as-dialog]", "the Save As dialog");
await key("Escape");
await key("Meta+Comma");
await page.waitForSelector("[data-testid=settings-dialog]");
await place("[data-testid=settings-dialog]", "the Settings window");
await page.click("[data-testid=settings-done]");
await key("Alt+KeyK");
await page.waitForSelector("[data-testid=shortcuts]");
await place("[data-testid=shortcuts]", "the keyboard shortcuts sheet");
await key("Escape");
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list]");
await place("[data-testid=instrument-list]", "+ Track's list");
// Return adds the first: a toast with its Undo, which takes it back.
const before = (await state()).timeline.tracks.length;
await key("Enter");
await page.waitForSelector("[data-testid=toast]");
await place("[data-testid=toast]", "a toast");
await page.evaluate(() => [...document.querySelectorAll("[data-testid=toast] button")].find((b) => b.textContent === "Undo")?.click());
await until(async () => (await state()).timeline.tracks.length === before);
await page.hover('[data-module-tab="kick"]');
await page.click('[data-track-menu="kick"]');
await page.waitForSelector("[data-testid=track-menu]");
await page.evaluate(() => [...document.querySelectorAll("[data-testid=track-menu] [role=menuitem]")].find((e) => e.textContent.startsWith("Delete"))?.click());
await page.waitForSelector("[data-testid=delete-dialog]");
await place("[data-testid=delete-dialog]", "the Delete dialog");
await key("Escape");

// ---------------------------------------------------------------- popovers

await page.click("[data-testid=key-field]");
await page.waitForSelector("[data-testid=key-picker]");
await place("[data-testid=key-picker]", "the key's popover");
await key("Escape");
// A mute makes the overlay pill; its popover lists it.
await page.click('[data-module-tab="kick"] [data-mute]');
await page.waitForSelector("[data-testid=overlay-pill]");
await page.click("[data-testid=overlay-pill]");
await page.waitForSelector("[data-testid=overlay-popover]");
await place("[data-testid=overlay-popover]", "the mutes and solos popover");
await key("Escape");
await page.click('[data-module-tab="kick"] [data-mute]');
// Export with a track muted: the question.
await page.click('[data-module-tab="kick"] [data-mute]');
await page.click("[data-testid=export]");
// The Export dialog (lane C) comes first: Song is chosen and Export… has the focus.
if (await page.waitForSelector("[data-testid=export-dialog]", { timeout: 3000 }).catch(() => null)) await page.keyboard.press("Enter");
if (await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 }).catch(() => null)) await place("[data-testid=export-ask]", "the question before an export with a track muted");
else check(false, "the question before an export with a track muted shows");
await key("Escape");
await page.click('[data-module-tab="kick"] [data-mute]');
// Broken code: the status pill and its popover.
const keys = read("tracks/keys.ts");
write("tracks/keys.ts", `${keys}\nexport const broken = pluck(;\n`);
await until(async () => !!(await page.$("[data-status~=compile]")));
await status();
await page.waitForSelector("[data-testid=compile-banner]");
await place("[data-testid=status-popover]", "the status popover, the code broken");
write("tracks/keys.ts", keys);
await until(async () => !(await page.$("[data-status]")));
// The keyboard piano's card, and a MIDI keyboard's mark.
await key("Meta+KeyK");
await page.waitForSelector("[data-testid=keyboard-piano]");
await place("[data-testid=keyboard-piano]", "the keyboard piano's card");
await key("Meta+KeyK");
await page.evaluate(async (token) => {
  await fetch("/api/graph/midi/inject", { method: "POST", headers: { "x-m2-token": token, "content-type": "application/json" }, body: JSON.stringify({ device: "Test Keys", plugged: true }) });
}, sess.token);
if (await page.waitForSelector("[data-testid=midi-status]", { timeout: 4000 }).catch(() => null)) {
  const t = await page.$eval("[data-testid=midi-status]", (e) => e.closest("[title]")?.getAttribute("title") ?? "");
  check(t.length > 0, `a MIDI keyboard's mark says what it is: "${t}"`);
} else check(false, "a MIDI keyboard's mark shows (M2_MIDI=inject)");

// ---------------------------------------------------------------- Rename Song, from the app's menu

/** A page as the desktop app's: menu commands come with `window.__menu(id)`. */
async function appPage() {
  const context = await browser.createBrowserContext();
  const p = await context.newPage();
  await p.setViewport({ width: 1440, height: 900 });
  p.on("pageerror", (e) => errors.push(`app page: ${e}`));
  await p.evaluateOnNewDocument((token) => {
    window.__M2__ = { native: true, token, platform: "macos" };
    const callbacks = new Map();
    const menu = [];
    let next = 1;
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => {
        if (cmd === "plugin:event|listen" && args?.event === "menu") menu.push(args.handler);
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
  }, sess.token);
  await p.goto(sess.url, { waitUntil: "domcontentloaded" });
  await p.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
  return p;
}
const app = await appPage();
await app.evaluate(() => window.__menu("rename_song"));
await app.waitForSelector("[data-testid=rename-song-dialog]");
await place("[data-testid=rename-song-dialog]", "the Rename Song dialog", MAC, app);
await app.keyboard.press("Escape");
// Help ▸ Make Your First Song: the guide's card.
await app.evaluate(() => window.__menu("guide"));
await app.waitForSelector("[data-testid=guide]");
await place("[data-testid=guide]", "the first-song guide", MAC, app);
await app.close();

// ---------------------------------------------------------------- a PC's words, and the lint

const pc = await open(1440, 900, { browser: true, pc: true });
const PC = Object.fromEntries(
  Object.entries(MAC).map(([s, k]) => [
    s,
    k.replace("⌥⌘B", "Ctrl+Alt+B").replace("⇧⌘E", "Ctrl+Shift+E").replace("⇧⌘Z", "Ctrl+Shift+Z").replace("⇧⌘F", "Ctrl+Shift+F").replace(/⌘/g, "Ctrl+").replace(/⌥/g, "Alt+"),
  ]),
);
await place("header", "a PC's top bar names its keys as a PC does (Ctrl+Shift+E)", PC, pc.page);
await place("[data-testid=dock-header]", "a PC's dock header (Alt+1)", PC, pc.page);
await pc.browser.close();

const bad = await page.evaluate(lint, "body");
check(bad.length === 0, `the lint finds nothing outside the design system${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);

await browser.close();
done(errors);

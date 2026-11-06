// Bounce to audio (RENDER-40), as a musician does it: first-song's Keys track menu offers Bounce to
// audio, with its words, and so does its Mix strip's. A click renders Keys, adds Keys audio right
// under it playing the file, and mutes Keys in the song: one undo step, "bouncing Keys", the toast
// saying so with Undo, the new track selected. Played, the new track's meter moves. ⌘Z takes it all
// back: every file of the song as it was, the bounced file kept in assets/. A long bounce shows a box
// saying how far it is, and its Stop adds nothing and leaves no file. The lint, and a picture.
//
//   node e2e/bounce.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { check, done, open, PROJ, read, ROOT, until } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { page, errors, state, toasts, key, view } = await open(1440, 900);
const has = async (sel) => !!(await page.$(sel));
const WORDS = "Turns it into audio you can chop. The song sounds the same.";
const files = ["song.ts", "tracks/keys.ts", "layout.json"];
const before = Object.fromEntries(files.map((f) => [f, fs.existsSync(`${PROJ}/${f}`) ? read(f) : null]));

// ---------------------------------------------------------------- the menus offer it

await view("mix");
const strip = '[data-testid=mix] [data-strip="keys"] [data-testid=strip-name]';
if (await until(() => has(strip), 8000)) {
  await page.click(strip, { button: "right" });
  check(await until(() => has("[data-testid=strip-menu] [data-testid=bounce]")), "Keys's Mix strip menu has Bounce to audio");
  await page.keyboard.press("Escape");
} else {
  check(false, "Keys has a Mix strip");
}
await page.click('[data-track-menu="keys"]');
await page.waitForSelector("[data-testid=track-menu]");
const row = "[data-testid=track-menu] [data-testid=bounce]";
check(await has(row), "Keys's track menu has Bounce to audio");
check((await page.$eval(row, (e) => e.textContent)).includes(WORDS), "with its words");
await page.evaluate(lint, "[data-testid=track-menu]").then((bad) => check(bad.length === 0, `the lint finds nothing in the menu${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`));

// ---------------------------------------------------------------- bouncing it

await page.click(row);
const tracks = async () => (await state()).timeline.tracks.filter((t) => t.track).map((t) => t.module);
check(await until(async () => (await tracks()).includes("keys-audio"), 30000), "Keys audio is added");
const order = await tracks();
check(order[order.indexOf("keys") + 1] === "keys-audio", `right under Keys: ${order.join(", ")}`);
check(await until(async () => (await state()).overlays?.heard?.keys === "muted-in-song"), "Keys is muted in the song");
check(read("tracks/keys.ts").includes("muted: true"), "in its code");
check(read("tracks/keys-audio.ts").includes('lane.audio("clips", 1)'), "the new track plays its clip on a mono lane, as Keys is mono");
check(fs.existsSync(`${PROJ}/assets/keys-bounce.wav`) && fs.existsSync(`${PROJ}/assets/keys-bounce.wav.json`), "the file and its provenance are in assets/");
check(await until(async () => (await toasts()).some((t) => t.includes("Bounced Keys to audio: Keys audio plays it, and Keys is muted in the song"))), `the toast says what it did: ${(await toasts()).join(" | ")}`);
check(await until(async () => (await page.$$eval("button", (bs) => bs.some((b) => b.textContent === "Undo")))), "with Undo");
check((await state()).undo_name?.name === "bouncing Keys", `one undo step, "bouncing Keys": ${(await state()).undo_name?.name}`);
check(await until(() => has('[data-module-tab="keys-audio"][aria-selected=true]')), "the new track is selected");
check(!(await has("[data-testid=bouncing]")), "the progress is gone once it's done");
await page.screenshot({ path: `${ROOT}/bounce.png` });

// ---------------------------------------------------------------- the new track plays

// The server's meters messages, as the page gets them: each track's peaks (linear).
const meters = [];
const cdp = await page.createCDPSession();
await cdp.send("Network.enable");
cdp.on("Network.webSocketFrameReceived", ({ response }) => {
  try {
    const m = JSON.parse(response.payloadData);
    if (m.t === "meters") meters.push(m.tracks ?? {});
  } catch {}
});
await page.mouse.click(10, 10);
await page.keyboard.press("Space");
check(await until(() => meters.some((m) => Math.max(0, ...(m["keys-audio"] ?? [])) > 0), 8000), "played, Keys audio's meter moves");
await page.keyboard.press("Space");

// ---------------------------------------------------------------- one undo takes it all back

await page.mouse.click(10, 10);
await key("Meta+z");
check(await until(() => !fs.existsSync(`${PROJ}/tracks/keys-audio.ts`)), "⌘Z takes Keys audio away");
check(await until(() => files.every((f) => (fs.existsSync(`${PROJ}/${f}`) ? read(f) : null) === before[f])), "and puts song.ts, Keys and the layout back as they were");
check(await until(async () => (await state()).overlays?.heard?.keys !== "muted-in-song"), "Keys is heard again");
check(fs.existsSync(`${PROJ}/assets/keys-bounce.wav`), "the bounced file stays in assets/");

// ---------------------------------------------------------------- a long bounce shows, and Stop takes it back

// Keys's clip made 20000 beats long (2.7 hours): its render takes many seconds on any computer.
const chords = read("data/keys/chords.json");
const long = chords.replace('"start": 0, "length": 32, "notes"', '"start": 0, "length": 20000, "notes"');
check(long !== chords, "Keys's clip, made long by an outside edit");
const rev = (await state()).view.revision;
fs.writeFileSync(`${PROJ}/data/keys/chords.json`, long);
await until(async () => (await state()).view.revision > rev, 15000);
await page.click('[data-track-menu="keys"]');
await page.waitForSelector(row);
await page.click(row);
check(await until(() => has("[data-testid=bouncing]"), 8000), "a box says it is bouncing");
check((await page.$eval("[data-testid=bouncing]", (e) => e.textContent)).includes("Bouncing Keys to audio"), "in its words");
await page.click("[data-testid=bounce-stop]");
check(await until(async () => (await toasts()).some((t) => t.includes("Stopped bouncing Keys: nothing was added")), 15000), `Stop says what it did: ${(await toasts()).join(" | ")}`);
check(!(await has("[data-testid=bouncing]")) && !fs.existsSync(`${PROJ}/tracks/keys-audio.ts`) && !fs.existsSync(`${PROJ}/assets/keys-bounce-2.wav`), "nothing is added, and no file is left");

done(errors);

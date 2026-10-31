// Polish A3 and A4: change a track's instrument, and drum names in Notes (the polish plan's A3,
// A4, Q5 to Q7). Change instrument… in a track's header menu opens + Track's list titled for the
// track; typing and Return change Hats to the drum kit in its file, one undo step with a toast
// whose Undo puts the file back byte for byte; the list ticks what a track plays; Bass's cutoff,
// set by code, is refused in words with Show code; the strip menu's row does the same; while the
// code doesn't compile the row is off and says why; a PC page's keys; the lint with the list open.
// Then drum names: a Drum kit track's clip in Notes names its rows (Kick, Snare, Closed hat) in an
// 88 px key column, its notes say their drum, a key set for the song never shades its rows (X26),
// a short note keeps its name on one line inside it, and Keys' roll still says C3 in 56 px. A Lead's
// card in Device says it plays one note at a time (track pm).
//
//   node e2e/pl-sounds.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, ROOT, until, write } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const { browser, page, errors, api, state, toasts, key, view } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const orig = { hats: read("tracks/hats.ts"), bass: read("tracks/bass.ts"), keys: read("tracks/keys.ts"), song: read("song.ts") };
const LIST = "[data-testid=instrument-list]";

/** The lint over what is inside `sel`. */
const linted = async (sel, what) => {
  const bad = await page.evaluate(lint, sel);
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
const openMenu = async (m, p = page) => {
  await p.hover(`[data-module-tab="${m}"]`);
  await p.click(`[data-track-menu="${m}"]`);
  return !!(await p.waitForSelector("[data-testid=track-menu]", { timeout: 2000 }).catch(() => null));
};
const rows = (sel) => page.$$eval(`${sel} [role=menuitem]`, (els) => els.map((e) => e.textContent)).catch(() => []);
const listInfo = () =>
  page
    .$eval(LIST, (e) => ({
      title: e.getAttribute("aria-label"),
      shown: e.querySelector("[data-testid=instrument-list-title]")?.textContent ?? null,
      placeholder: e.querySelector("input")?.placeholder,
      focused: document.activeElement === e.querySelector("input"),
      ticked: [...e.querySelectorAll("[role=option][aria-checked=true]")].map((o) => o.dataset.instrument),
      active: e.querySelector("[data-active]")?.dataset.instrument ?? null,
      code: e.querySelector("[data-testid=instrument-code]")?.textContent ?? null,
    }))
    .catch(() => null);
const revision = async () => (await state()).view.revision;
const toasted = (words) => until(async () => (await toasts()).some((t) => t.includes(words)), 4000);
const clickToastAction = (label) =>
  page.evaluate((label) => {
    const b = [...document.querySelectorAll("[data-testid=toast] button")].find((x) => x.textContent === label);
    b?.click();
    return !!b;
  }, label);

// ---------------------------------------------------------------- the header menu (A3)

check(await openMenu("hats"), "Hats' ⋯ opens its menu");
const hatsRows = await rows("[data-testid=track-menu]");
const at = hatsRows.findIndex((r) => r.startsWith("Change instrument…"));
check(at === 1 && hatsRows[0].startsWith("Rename"), `the Hats menu has Change instrument…, after Rename (${hatsRows.join(" | ")})`);
await page.click("[data-testid=track-menu] [data-testid=change-instrument]");
check(!!(await page.waitForSelector(LIST, { timeout: 2000 }).catch(() => null)) && !(await page.$("[data-testid=track-menu]")), "it opens the instrument list in place of the menu");
const first = await listInfo();
check(
  first?.title === "Change Hats's instrument" && first.shown === "Change Hats's instrument" && first.placeholder === "Find an instrument…" && first.focused,
  `titled for the track, its search field focused (${JSON.stringify(first)})`,
);
check(same(first?.ticked, ["@m2/drums:drum"]), `Hats plays One drum, which the list ticks (${JSON.stringify(first?.ticked)})`);
await linted(LIST, "the list in change mode");
await page.screenshot({ path: `${ROOT}/pl-sounds-list.png` });
await page.keyboard.type("kit");
check(await until(async () => (await listInfo())?.active === "@m2/drums:drum-kit"), "typing kit picks the Drum kit");
check((await listInfo())?.code === 'tracks/hats.ts · drums.kit(lane.notes("hits"))', `its foot shows the code it writes: ${(await listInfo())?.code}`);
const rev0 = await revision();
await page.keyboard.press("Enter");
const kit = orig.hats.replace(/drums\.drum\(lane\.notes\("hits"\), \{[^}]*\}\)/, 'drums.kit(lane.notes("hits"))');
check(await until(() => read("tracks/hats.ts") === kit), `Return writes drums.kit(lane.notes("hits")) in tracks/hats.ts (${read("tracks/hats.ts").split("\n").find((l) => l.includes("track("))})`);
// The toast says the edit is done: since std 0.6 it writes music2.toml and music2.lock too (PACK-10),
// so the files may land a moment before the revision does.
check(await toasted("Hats plays Drum kit now"), `a toast says "Hats plays Drum kit now" (${await toasts()})`);
check((await revision()) === rev0 + 1 && read("song.ts") === orig.song, `one revision, and song.ts as it was (${rev0} → ${await revision()})`);
check(!(await page.$(LIST)), "the list closes");
// Opened again, the list ticks what it plays now.
await openMenu("hats");
await page.click("[data-testid=track-menu] [data-testid=change-instrument]");
await page.waitForSelector(LIST);
const again = await listInfo();
check(same(again?.ticked, ["@m2/drums:drum-kit"]) && again.active === "@m2/drums:drum-kit", `opened again, Drum kit is ticked and active (${JSON.stringify(again)})`);
await key("Escape");
check(await until(async () => !(await page.$(LIST))), "Esc closes it");
check(await clickToastAction("Undo").then(() => until(() => read("tracks/hats.ts") === orig.hats)), "the toast's Undo puts hats.ts back, byte for byte");

// ---------------------------------------------------------------- refused in words

await openMenu("bass");
await page.click("[data-testid=track-menu] [data-testid=change-instrument]");
await page.waitForSelector(LIST);
await page.keyboard.type("pad");
await page.keyboard.press("Enter");
check(await toasted("Bass's cutoff is set by code here. Change its instrument in the Code view."), `Bass is refused in words (${await toasts()})`);
check(read("tracks/bass.ts") === orig.bass, "and nothing is written");
check(await clickToastAction("Show code"), "the toast has Show code");
check(
  await until(async () => (await page.$eval("[data-testid=code] [data-file][aria-current=true]", (e) => e.dataset.file).catch(() => null)) === "tracks/bass.ts"),
  "which opens tracks/bass.ts in the Code view",
);
await key("Alt+3");
await until(async () => !!(await page.$('[data-testid=mix] [data-strip="keys"]')));

// ---------------------------------------------------------------- the strip menu

await page.click('[data-testid=mix] [data-strip="keys"] [data-testid=strip-name]', { button: "right" });
await page.waitForSelector("[data-testid=strip-menu]");
const stripRows = await rows("[data-testid=strip-menu]");
check(stripRows[1] === "Change instrument…", `the Keys strip's menu has Change instrument… (${stripRows.join(" | ")})`);
await page.click("[data-testid=strip-menu] [data-testid=change-instrument]");
await page.waitForSelector(LIST);
check((await listInfo())?.title === "Change Keys's instrument", "it opens the list for Keys");
await page.keyboard.type("fm bell");
await page.keyboard.press("Enter");
check(await until(() => read("tracks/keys.ts").includes('fm.bell(lane.notes("chords"))') && read("tracks/keys.ts").includes('import { fm } from "@m2/synths";')), `Keys plays an FM bell: ${read("tracks/keys.ts").split("\n").find((l) => l.includes("track("))}`);
check(await toasted("Keys plays FM bell now"), `with its toast (${await toasts()})`);
await key("Meta+z");
check(await until(() => read("tracks/keys.ts") === orig.keys), "⌘Z puts keys.ts back");

// ---------------------------------------------------------------- while the code doesn't compile

write("song.ts", `${orig.song}\nconst broken = ;\n`);
await until(async () => ((await state()).view.draft_errors ?? []).length > 0, 10000);
await openMenu("hats");
const off = await page.$eval("[data-testid=track-menu] [data-testid=change-instrument]", (e) => ({ disabled: e.disabled, title: e.title })).catch(() => null);
check(!!off?.disabled && off.title.length > 0, `while the code doesn't compile the row is off and says why (${JSON.stringify(off)})`);
await key("Escape");
write("song.ts", orig.song);
await until(async () => ((await state()).view.draft_errors ?? []).length === 0, 10000);

// ---------------------------------------------------------------- drum names in Notes (A4)

/** Open the first clip of lane `lane` (`bass.notes`) in Notes. */
async function openClip(lane) {
  const b = await page.$(`[data-lane="${lane}"] :nth-child(1 of [data-clip])`).then((e) => e?.boundingBox());
  if (!b) return false;
  await page.mouse.click(b.x + Math.min(40, b.width / 2), b.y + b.height / 2, { count: 2 });
  return until(async () => (await page.$$("[data-testid=notes] [data-note]")).length > 0, 5000);
}
const keyLabel = (pitch) => page.$eval(`[data-testid=roll-keys] [data-key="${pitch}"]`, (e) => e.querySelector("span")?.textContent ?? "").catch(() => null);
const keysWidth = () => page.$eval("[data-testid=roll-keys]", (e) => Math.round(e.getBoundingClientRect().width)).catch(() => null);
const s0 = await state();
const added = await post("/track/add", { session: s0.session, component: "@m2/drums:drum-kit" });
check(added.status === 200 && added.body.module === "drum-kit", `a Drum kit track is added (${JSON.stringify(added.body).slice(0, 80)})`);
await until(async () => (await state()).timeline.tracks.some((t) => t.module === "drum-kit"));
const s1 = await state();
const beat = [36, 42, 38, 42].map((pitch, i) => ({ pitch, start: i, length: 0.5, velocity: 0.8 }));
const wrote = await post("/lane", { session: s1.session, revision: s1.view.revision, module: "drum-kit", lane: "hits", clips: [{ id: "kitbeat1", start: 0, length: 4, notes: beat }] });
check(wrote.status === 200, "a beat is written in its lane");
const lanes = (await state()).timeline.tracks.find((t) => t.module === "drum-kit").lanes;
check(lanes[0].names?.["36"] === "Kick" && lanes[0].names?.["42"] === "Closed hat", "the timeline names its notes");
await until(async () => !!(await page.$('[data-lane="drum-kit.hits"] [data-clip]')), 5000);
check(await openClip("drum-kit.hits"), "its clip opens in Notes");
// Folded, the roll shows only the drums the beat uses (NOTE-8): Fold off shows every row's name.
await page.click("[data-testid=fold-rows]");
await until(async () => !!(await page.$('[data-testid=roll-keys] [data-key="60"]')));
check((await keyLabel(36)) === "Kick", `key 36 says Kick: ${await keyLabel(36)}`);
check((await keyLabel(38)) === "Snare" && (await keyLabel(42)) === "Closed hat" && (await keyLabel(46)) === "Open hat", "38 says Snare, 42 Closed hat, 46 Open hat");
check((await keyLabel(60)) === "" && (await keyLabel(40)) === "", "a row with no name has no label, not even a C");
check((await keysWidth()) === 88, `the key column is 88 px (${await keysWidth()})`);
const hat = await page.$eval('[data-testid=notes] [data-note][data-pitch="42"]', (e) => ({ title: e.title, label: e.querySelector("span")?.textContent })).catch(() => null);
check(hat?.title.startsWith("Closed hat · beat 2") && hat.label === "Closed hat", `a note on 42 says Closed hat (${JSON.stringify(hat)})`);
const bar = await page.$eval('[data-velocity="0"]', (e) => e.title).catch(() => "");
check(bar.startsWith("Kick: velocity 80%"), `its velocity bar says the drum too (${bar})`);
const misfits = await page.evaluate(() =>
  [...document.querySelectorAll("[data-testid=roll-keys] [data-name]")]
    .filter((e) => {
      const s = e.getBoundingClientRect(), k = e.parentElement.getBoundingClientRect();
      const black = e.parentElement.querySelector("[data-black]")?.getBoundingClientRect();
      return s.left < (black ? black.right + 2 : k.left) || s.right > k.right;
    })
    .map((e) => e.textContent),
);
check(misfits.length === 0, `every name fits on the white of its key (${misfits})`);
await linted("[data-testid=notes]", "Notes with drum names");
await page.screenshot({ path: `${ROOT}/pl-sounds-drums.png` });
// Zoomed out until a hat is too short for its name, each name stays on one line inside its note,
// cut short (seen in the End's pictures: "Closed hat" ran over two rows).
const hatWidth = () => page.$eval('[data-testid=notes] [data-note][data-pitch="42"]', (e) => e.getBoundingClientRect().width);
await page.focus("[data-testid=notes]");
let outs = 0;
while (outs < 5 && (await hatWidth()) > 60) {
  await key("Meta+Minus");
  outs++;
  await new Promise((r) => setTimeout(r, 300));
}
const spill = await page.$$eval("[data-testid=notes] [data-note]", (els) =>
  els.flatMap((e) => {
    const label = e.querySelector("span");
    if (!label) return [];
    const n = e.getBoundingClientRect(), l = label.getBoundingClientRect();
    return l.height > n.height || l.right > n.right + 0.5 ? [`${label.textContent} ${Math.round(l.width)}×${Math.round(l.height)} in ${Math.round(n.width)}×${Math.round(n.height)}`] : [];
  }),
);
const hatLabel = await page.$eval('[data-testid=notes] [data-note][data-pitch="42"] span', (e) => e.textContent).catch(() => null);
check(outs > 0 && hatLabel === "Closed hat" && spill.length === 0, `a hat ${Math.round(await hatWidth())} px long keeps its name on one line, inside it: ${spill.join(" | ") || "every name fits"}`);
for (let i = 0; i < outs; i++) await key("Meta+Equal");
// A key set for the song never shades a drum kit's rows: a kit has no key (X26, review PL-T3).
const shading = () => page.$eval("[data-testid=roll-grid]", (e) => e.dataset.shaded).catch(() => null);
let layoutWas = null;
try {
  layoutWas = read("layout.json");
} catch {}
write("layout.json", JSON.stringify({ ...(layoutWas ? JSON.parse(layoutWas) : { order: [], tracks: {} }), key: { root: "E", scale: "minor" } }, null, 2));
check(await until(async () => (await page.$eval("[data-testid=key-field]", (e) => e.textContent).catch(() => null)) === "E minor"), "E minor is set in layout.json");
await new Promise((r) => setTimeout(r, 300));
check((await shading()) === "", `the Drum kit's rows stay unshaded, not by E minor nor as black keys: a kit has rows, not keys (next D4c): ${await shading()}`);
check(await openClip("keys.chords"), "Keys' clip opens in Notes");
check(await until(async () => (await shading()) === "1,3,5,8,10"), `Keys' roll is shaded by E minor: ${await shading()}`);
if (layoutWas === null) fs.rmSync(`${PROJ}/layout.json`);
else write("layout.json", layoutWas);
check((await keyLabel(48)) === "C3" && (await keyLabel(36)) === "C2" && (await keysWidth()) === 56, `Keys' roll still says C3, in 56 px (${await keyLabel(48)}, ${await keysWidth()})`);

// ---------------------------------------------------------------- how an instrument plays (track pm)

// A Lead plays one note at a time: its card in Device says so under its name, all of it in the tooltip.
const s2 = await state();
const lead = await post("/track/add", { session: s2.session, component: "@m2/synths:synth-lead" });
await view("graph");
await until(async () => !!(await page.$(`[data-module-tab="${lead.body.module}"]`)));
await page.click(`[data-module-tab="${lead.body.module}"] [data-testid=track-label]`);
const plays = `[data-module="${lead.body.module}"] [data-testid=voices]`;
await until(async () => !!(await page.$(plays)));
const line = await page.$eval(plays, (e) => ({ text: e.textContent, title: e.getAttribute("title") })).catch(() => null);
check(
  lead.status === 200 && line?.text === "Plays one note at a time." && !!line?.title?.includes("slides to it over 60 ms, without starting again"),
  `Device shows "Plays one note at a time" on a Lead's card, its glide in the tooltip (${JSON.stringify(line)})`,
);

// ---------------------------------------------------------------- a PC page's keys

const pc = await open(1440, 900, { pc: true });
await openMenu("hats", pc.page);
await pc.page.click("[data-testid=track-menu] [data-testid=change-instrument]");
await pc.page.waitForSelector(LIST);
const startActive = await pc.page.$eval(`${LIST} [data-active]`, (e) => e.dataset.instrument);
await pc.page.keyboard.press("ArrowDown");
const moved = await pc.page.$eval(`${LIST} [data-active]`, (e) => e.dataset.instrument);
await pc.page.keyboard.press("Escape");
const closed = await until(async () => !(await pc.page.$(LIST)));
check(moved !== startActive && closed && read("tracks/hats.ts") === orig.hats, `on a PC, ↓ moves in the list and Esc closes it, writing nothing (${startActive} → ${moved})`);
await pc.browser.close();

await browser.close();
done(errors);

function same(a, b) {
  return JSON.stringify(a) === JSON.stringify(b);
}

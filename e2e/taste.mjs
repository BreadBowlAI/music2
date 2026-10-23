// The taste lint (taste.md §0): walks the DOM in each state of the app the UI audit shot and fails
// on anything outside the design system:
//   - a computed font-size outside {11, 12, 13, 15} px (text inside the code editor excepted);
//   - a border-radius outside {0, 2, 4, 8} px that isn't a full round (at least 9999 px);
//   - a background-image with a gradient;
//   - a backdrop-filter;
//   - a monospace font outside a `[data-code]` element (text that is literally code).
// One check per state; a failing one lists the first offenders.
//
//   node e2e/taste.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { spawn } from "node:child_process";
import fs from "node:fs";
import { PROJ, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

// With the side browser shown, as a wide window opens.
const { browser, page, errors, api, show, view, key, box, status } = await open(1440, 900, { browser: true });
const post = (path, body) => api(path, { method: "POST", body: JSON.stringify(body) });

/** Everything on the page outside the system, as `what: value at element`. Runs in the page. */
function lint() {
  const SIZES = new Set([11, 12, 13, 15]);
  const RADII = new Set([0, 2, 4, 8]);
  const bad = [];
  const name = (e) => {
    const id = e.dataset?.testid ? `[data-testid=${e.dataset.testid}]` : "";
    const cls = typeof e.className === "string" && e.className ? `.${e.className.trim().split(/\s+/).slice(0, 3).join(".")}` : "";
    const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join(" ").slice(0, 24);
    return `<${e.tagName.toLowerCase()}${id}${cls}>${text ? ` "${text}"` : ""}`;
  };
  const say = (e, what) => bad.length < 400 && bad.push(`${what} at ${name(e)}`);
  for (const e of document.body.querySelectorAll("*")) {
    if (!e.getClientRects().length) continue;
    const s = getComputedStyle(e);
    const inEditor = !!e.closest(".monaco-editor");
    const texty = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) || /^(INPUT|TEXTAREA|SELECT)$/.test(e.tagName);
    if (texty && !inEditor && e.namespaceURI === "http://www.w3.org/1999/xhtml") {
      const px = parseFloat(s.fontSize);
      if (!SIZES.has(px)) say(e, `font-size ${s.fontSize}`);
    }
    if (texty && /mono|menlo|courier|sf mono/i.test(s.fontFamily) && !e.closest("[data-code]")) say(e, `mono font ${s.fontFamily.split(",")[0]}`);
    for (const corner of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"]) {
      for (const v of s[corner].split(" ")) {
        const r = parseFloat(v);
        if (v.endsWith("%") ? r !== 0 && r < 50 : !RADII.has(r) && r < 9999) {
          say(e, `border-radius ${s[corner]}`);
          break;
        }
      }
    }
    for (const pseudo of [null, "::before", "::after"]) {
      const p = pseudo ? getComputedStyle(e, pseudo) : s;
      if (/gradient/.test(p.backgroundImage)) say(e, `gradient ${pseudo ?? ""}${p.backgroundImage.slice(0, 40)}`);
      if (p.backdropFilter && p.backdropFilter !== "none") say(e, `backdrop-filter ${pseudo ?? ""}${p.backdropFilter}`);
    }
  }
  return bad;
}

/** Lint the page as it is now, as one check. */
const state = async (what) => {
  await sleep(250);
  const bad = await page.evaluate(lint);
  const seen = [...new Set(bad)];
  check(seen.length === 0, `${what}: nothing outside the design system${seen.length ? ` (${seen.length}: ${seen.slice(0, 6).join("; ")})` : ""}`);
};
const mid = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });

// First, that the lint sees each kind of offence: one planted element with all five.
const planted = await page.evaluate((lint) => {
  const e = document.createElement("div");
  e.textContent = "planted";
  e.style.cssText = "font-size:14px;border-radius:6px;background-image:linear-gradient(red,blue);backdrop-filter:blur(2px);font-family:monospace";
  document.body.append(e);
  const bad = new Function(`return (${lint})()`)();
  e.remove();
  return bad;
}, lint.toString());
check(
  ["font-size 14px", "border-radius 6px", "gradient", "backdrop-filter", "mono font"].every((w) => planted.some((b) => b.startsWith(w) && b.includes('"planted"'))),
  `the lint finds a planted font size, radius, gradient, backdrop filter and mono font (${planted.filter((b) => b.includes("planted")).length})`,
);

// ---------------------------------------------------------------- the font

// The Mac keeps its own font; off the Mac the UI is in Inter, served from web/public/fonts
// (index.css, main.tsx's `data-os`). Every page here is given the Mac, so a second one is given
// Linux. What the CSS asks for isn't enough: Chrome's DevTools protocol says which font each text
// was drawn in, and the FontFace says whether Inter's file loaded.
/** The page's font: `data-os`, the body's font list, Inter's FontFace, and the fonts one line of its text was drawn in. */
async function font(p) {
  const seen = await p.evaluate(() => {
    const face = [...document.fonts].find((f) => f.family.replace(/"/g, "") === "Inter Variable");
    for (const e of document.querySelectorAll("[data-font-probe]")) delete e.dataset.fontProbe;
    const e = [...document.body.querySelectorAll("*")].find(
      (e) => e.getClientRects().length && !e.closest("[data-code]") && [...e.childNodes].some((n) => n.nodeType === 3 && /[A-Za-z]{3}/.test(n.textContent)),
    );
    if (e) e.dataset.fontProbe = "";
    return { os: document.documentElement.dataset.os ?? "", family: getComputedStyle(document.body).fontFamily, face: face?.status ?? "none", text: e?.textContent.trim().slice(0, 24) ?? "" };
  });
  const cdp = await p.createCDPSession();
  await cdp.send("DOM.enable");
  await cdp.send("CSS.enable");
  const { root } = await cdp.send("DOM.getDocument", { depth: 0 });
  const { nodeId } = await cdp.send("DOM.querySelector", { nodeId: root.nodeId, selector: "[data-font-probe]" });
  const { fonts } = nodeId ? await cdp.send("CSS.getPlatformFontsForNode", { nodeId }) : { fonts: [] };
  await cdp.detach();
  // The font that drew most of it (a symbol may come from another).
  const main = [...fonts].sort((a, b) => b.glyphCount - a.glyphCount)[0];
  return { ...seen, drawn: main ? `${main.familyName}${main.isCustomFont ? " (the page's own file)" : " (the system's)"}` : "nothing", own: !!main?.isCustomFont, name: main?.familyName ?? "" };
}
const mac = await font(page);
check(
  mac.os === "mac" && mac.family.startsWith("system-ui") && mac.face !== "loaded" && mac.name && !mac.own,
  `a Mac page keeps the system font: ${mac.family.split(",")[0]} first, Inter not loaded (${mac.face}), "${mac.text}" drawn in ${mac.drawn}`,
);
const linux = await browser.newPage();
linux.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(`linux page: ${m.text()}`));
linux.on("pageerror", (e) => errors.push(`linux page: ${e}`));
await linux.setViewport({ width: 1440, height: 900 });
await linux.evaluateOnNewDocument((token) => {
  window.__M2__ = { native: false, token, platform: "linux" };
}, sess.page_token);
await linux.goto(sess.url, { waitUntil: "domcontentloaded" });
await linux.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
// The text asks for Inter as it is laid out; wait until its file has loaded or failed.
await until(() => linux.evaluate(() => [...document.fonts].some((f) => f.family.replace(/"/g, "") === "Inter Variable" && /loaded|error/.test(f.status))), 5000);
const pc = await font(linux);
check(pc.face === "loaded", `a Linux page loads Inter: its FontFace "Inter Variable" is ${pc.face}`);
check(
  pc.os === "" && pc.family.startsWith('"Inter Variable"') && pc.own && /^Inter/.test(pc.name),
  `a Linux page draws its text in Inter: ${pc.family.split(",")[0]} first, "${pc.text}" drawn in ${pc.drawn}`,
);
// Inter's figures are tabular with taste's tabular-nums: "0000" and "1111" are as wide. Without it
// they aren't, so the check can tell.
const figures = await linux.evaluate(() => {
  const wide = (text, plain) => {
    const s = document.createElement("span");
    s.textContent = text;
    if (plain) s.style.fontVariantNumeric = "normal";
    document.body.append(s);
    const w = s.getBoundingClientRect().width;
    s.remove();
    return w;
  };
  return { tab: [wide("0000"), wide("1111")], plain: [wide("0000", true), wide("1111", true)] };
});
check(
  figures.tab[0] > 0 && figures.tab[0] === figures.tab[1] && figures.plain[0] !== figures.plain[1],
  `Inter's figures are tabular: "0000" and "1111" are ${figures.tab.map((w) => w.toFixed(2)).join(" and ")} px wide (${figures.plain.map((w) => w.toFixed(2)).join(" and ")} without tabular-nums)`,
);
await linux.close();
await page.bringToFront();

// ---------------------------------------------------------------- the start screen and New Song

await post("/close", {});
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=start]");
await page.waitForSelector("[data-testid=open-example]:not([disabled])");
await state("the start screen");
await page.click("[data-testid=open-example]");
await page.waitForSelector("[data-testid=examples]");
await state("its examples menu");
await key("Escape");
await page.click("[data-testid=new-song]");
await page.waitForSelector("[data-testid=template-starter]");
await state("the New Song dialog");
await key("Escape");
await post("/open", { path: PROJ });
await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector("[data-testid=graph] [data-module]");

// ---------------------------------------------------------------- the song

await state("first-song's timeline, the browser and the first track's device");
await page.click('[data-fold="bass"]');
await page.waitForSelector('[data-lane="bass.cutoff"]');
await state("the bass's automation unfolded");
await page.click('[data-fold="bass"]');
for (const m of ["song", "hats", "keys", "kick"]) {
  await show(m);
  await state(`the ${m} graph`);
}
await show("bass");
await key("Space");
await until(async () => (await api("/status")).body.playing === true);
await sleep(600);
await state("playing");
await key("Space");
await page.click("[data-testid=loop-toggle]");
await until(async () => !!(await page.$("[data-testid=loop-shade]")));
await state("looping");
await page.click("[data-testid=loop-toggle]");
const clip = await box('[data-lane="bass.notes"] [data-clip]');
await page.mouse.click(clip.x + 20, clip.y + clip.height / 2);
await state("a clip picked");
await page.mouse.click(clip.x + 20, clip.y + clip.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=roll-grid] [data-note]");
await page.click("[data-testid=notes]");
await key("Meta+a");
await state("the piano roll, its notes selected");
await key("Escape");
await page.click("[data-testid=timeline]");
await key("Escape");
await page.waitForSelector("[data-testid=notes][data-empty]");
await state("Notes with no clip selected");
await view("mix");
await page.waitForSelector("[data-testid=mix] [data-strip]");
await state("the Mix view");
await view("code");
await page.waitForSelector("[data-testid=code] .view-lines");
await sleep(800);
await state("the code view");
await view("graph");

// ---------------------------------------------------------------- knobs, menus, cables, toasts

// A level on its Mix strip (MX-6), dragged: the fader and the value bubble.
await show("song");
await view("mix");
const knob = mid(await box('[data-testid=mix] [data-knob="hats.db"] [role=slider]'));
await page.mouse.move(knob.x, knob.y);
await page.mouse.down();
await page.mouse.move(knob.x, knob.y - 20, { steps: 6 });
await state("a fader mid-drag, its value at the pointer");
await page.mouse.up();
// A knob makes no toast (taste.md §2.14); its Undo does.
await until(async () => !(await page.$('button[aria-label="Undo"][disabled]')));
await page.click('button[aria-label="Undo"]');
await until(async () => (await page.$$("[data-testid=toast]")).length > 0);
await state("a toast");
await view("graph");
await show("bass");
const drive = mid(await box('[data-knob="drive"]'));
await page.mouse.click(drive.x, drive.y, { button: "right" });
await page.waitForSelector("[data-testid=knob-menu]");
await state("the knob menu");
await key("Escape");
// A value typed into the knob (taste.md §2.14), one that doesn't read: the field and its message.
const driveKnob = mid(await box('[data-knob="drive"] svg'));
await page.mouse.click(driveKnob.x, driveKnob.y, { count: 2 });
await page.waitForSelector("[data-testid=knob-type-in]");
await page.keyboard.type("loud");
await key("Enter");
await page.waitForSelector('[data-knob="drive"] [role=alert]');
await state("a knob's value typed in, unreadable");
await key("Escape");
await show("keys");
await page.click('[aria-label^="Add an effect after"]');
await page.waitForSelector("[data-effect-browser]");
await state("the effect browser");
await key("Escape");
await show("bass");
const from = mid(await box('[data-outlet="wobble"]'));
await page.mouse.move(from.x, from.y);
await page.mouse.down();
await page.mouse.move(from.x + 40, from.y + 30, { steps: 5 });
await state("a cable being dragged");
await page.mouse.move(from.x + 30, from.y - 200, { steps: 4 });
await page.mouse.up();
await page.type("[data-browser] input", "verb");
await state("a search in the browser");
await page.$eval("[data-browser] input", (e) => e.blur());
await key("Alt+KeyK");
await page.waitForSelector("[data-testid=shortcuts]");
await state("the keyboard shortcuts sheet");
await key("Escape");
// The track builders' surfaces (design-8b §3.5): + Track's list, a header's menu and its colours, the delete dialog.
await page.click("[data-testid=add-track]");
await page.waitForSelector("[data-testid=instrument-list]");
await state("+ Track's instrument list");
await key("Escape");
await page.hover('[data-module-tab="kick"]');
await page.click('[data-track-menu="kick"]');
await page.waitForSelector("[data-testid=track-menu]");
await page.click("[data-testid=colour-row]");
await page.waitForSelector("[data-testid=colours]");
await state("a track's header menu, its colours open");
await key("Escape");
await key("Escape");
await page.hover('[data-module-tab="kick"]');
await page.click('[data-track-menu="kick"]');
await page.waitForSelector("[data-testid=track-menu]");
await page.evaluate(() => [...document.querySelectorAll("[data-testid=track-menu] [role=menuitem]")].find((e) => e.textContent.startsWith("Delete"))?.click());
await page.waitForSelector("[data-testid=delete-dialog]");
await state("the delete dialog");
await key("Escape");

// ---------------------------------------------------------------- problems

const keys = read("tracks/keys.ts");
write("tracks/keys.ts", `${keys}\nexport const broken = pluck(;\n`);
await until(async () => !!(await page.$("[data-status~=compile]")));
await status();
await page.waitForSelector("[data-testid=compile-banner]");
await state("the code broken: the status pill and its popover");
write("tracks/keys.ts", keys);
await until(async () => !(await page.$("[data-status]")));
const kick = read("tracks/kick.ts");
write("tracks/kick.ts", kick.replace('track("kick"', 'track("kicks"'));
await until(async () => !!(await page.$("[data-status~=problems]")), 10000);
await status();
await page.waitForSelector("[data-testid=attention]");
await state("Needs attention, in the status popover");
write("tracks/kick.ts", kick);
await until(async () => !(await page.$("[data-status]")), 10000);

await page.click("[data-testid=export]");
await until(async () => !!(await page.$("[data-testid=export-progress]")), 4000);
await state("an export under way");
await until(async () => !(await page.$("[data-testid=export-progress]")), 30000);

// Another program holds the project: read only.
await post("/close", {});
fs.mkdirSync(`${PROJ}/.music2`, { recursive: true });
const holder = spawn("python3", ["-c", `
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
os.ftruncate(fd, 0)
os.write(fd, sys.argv[2].replace("PID", str(os.getpid())).encode())
print("ready", flush=True)
sys.stdin.read()
`, `${PROJ}/.music2/lock`, JSON.stringify({ pid: "PID", program: "m2", host: "elsewhere", host_id: "x", token: "0", since: Math.floor(Date.now() / 1000) }).replace('"PID"', "PID")], { stdio: ["pipe", "pipe", "inherit"] });
await new Promise((r) => holder.stdout.once("data", r));
await post("/open", { path: PROJ });
await until(async () => !!(await page.$("[data-testid=read-only-banner]")), 8000);
await state("read only");
await status();
await page.waitForSelector("[data-testid=read-only-retry]");
await state("read only: its popover");
await key("Escape");
// A small window (taste.md §3.8): the headers narrow, the time goes; nothing else scrolls sideways.
await page.setViewport({ width: 1100, height: 700 });
await sleep(400);
await state("the smallest window, 1100×700");
holder.stdin.end();
holder.kill();

await browser.close();
done(errors);

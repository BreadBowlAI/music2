// 8b.4 Mute and solo in the app, and 8b.8's load (design-8b §6.6, §6.7, taste.md §2.9, §3.3, §3.4;
// the 8b-ui plan's U1, U2, U3 and U11): M and S in every track header, placed from its right edge; a click, ⌥-click on S, a
// drag across several, three quick clicks; the M and S keys; the mutes pill "2 muted · 1 solo" and
// its list, with each track's ×, "Still drives Duck" and Clear all; the pill clear of the song's
// name and the status pill at 1100 to 1280 px; nothing written but .music2/overlays.json; mute in
// the song from the header menu (the option written, struck through and hatched, one undo step,
// M opens the menu, the M key and drags skip it, S lights but mute wins); a reload keeps them; a
// read-only window keeps them in memory and can't mute in the song; a PC page's words. The export's
// question while mutes or solos change what you hear (U3): Mute Kick in the song, then export (its
// WAV equals `m2 render`'s), Export anyway (the toast names Kick), the keyboard in it (Tab goes
// round, Return and Space press the focused button, no key reaches the page), the solo's form, and
// only Cancel and Export anyway while the code doesn't compile. "Heavy load" and "Sound dropped out" in the
// status pill while playing (U11), through the page's own fetch wrapper.
//
//   node e2e/s8-mute.mjs <root> <home> <project> <m2>     (the `export` kind: examples/first-song open, and the m2 CLI)
import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { HOME, PROJ, ROOT, check, done, open, read, sess, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, api, state, toasts, key, box } = await open(1440, 900);
const post = (p, body) => api(p, { method: "POST", body: JSON.stringify(body) });
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const rgb = (hex) => `rgb(${[1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ")})`;
const GRAY_700 = rgb("#4c5056"), ACCENT = rgb("#3862a7"), FG_FAINT = rgb("#7c8186"), FG_MUTED = rgb("#65696f");

/** Every project file but .music2's, by path: what an edit would change. */
function files(dir = PROJ, rel = "") {
  const out = {};
  for (const e of fs.readdirSync(path.join(dir, rel), { withFileTypes: true })) {
    const r = path.join(rel, e.name);
    if (r === ".music2") continue;
    if (e.isDirectory()) Object.assign(out, files(dir, r));
    else out[r] = fs.readFileSync(path.join(dir, r), "utf8");
  }
  return out;
}
const overlaysFile = `${PROJ}/.music2/overlays.json`;
const saved = () => {
  try {
    return JSON.parse(fs.readFileSync(overlaysFile, "utf8"));
  } catch {
    return null;
  }
};
const overlays = async () => (await state()).overlays;
const pill = () => page.$eval("[data-testid=overlay-pill]", (e) => e.textContent).catch(() => null);
const pressed = (kind, m) => page.$eval(`[data-${kind}="${m}"]`, (e) => e.getAttribute("aria-pressed") === "true").catch(() => null);
const fill = (kind, m) => page.$eval(`[data-${kind}="${m}"]`, (e) => getComputedStyle(e).backgroundColor).catch(() => null);
const nameColour = (m) => page.$eval(`[data-track="${m}"] [data-testid=track-label]`, (e) => getComputedStyle(e).color).catch(() => null);
const title = (kind, m) => page.$eval(`[data-${kind}="${m}"]`, (e) => e.title).catch(() => "");
/** How faint a track's clips draw: the opacity of everything between its first clip and its lane. */
const clipOpacity = (m) =>
  page
    .$eval(`[data-track="${m}"] [data-clip]`, (e) => {
      let o = 1;
      for (let x = e; x && !x.dataset.lane; x = x.parentElement) o *= Number(getComputedStyle(x).opacity);
      return Math.round(o * 100) / 100;
    })
    .catch(() => null);
/** Wait for the server's state to be `mute` and `solo` (sorted), and the page to show it. */
const settled = (mute, solo) =>
  until(async () => {
    const o = await overlays();
    if (!same(o.mute, mute) || !same(o.solo, solo)) return false;
    for (const m of ["kick", "hats", "bass", "keys"]) {
      if ((await pressed("mute", m)) !== mute.includes(m) && (await page.$eval(`[data-mute="${m}"]`, (e) => !e.dataset.inSong))) return false;
      if ((await pressed("solo", m)) !== solo.includes(m)) return false;
    }
    return true;
  }, 5000);
const select = async (m) => {
  await page.click(`[data-module-tab="${m}"] [data-testid=track-label]`);
  await until(() => page.$eval(`[data-track="${m}"]`, (e) => e.dataset.selected === "true").catch(() => false));
};
const mid = (b) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 });

/** The taste lint (e2e/taste.mjs) over what is inside `sel`: font sizes, radii, gradients, backdrop filters, mono text. */
const lint = (sel) =>
  page.evaluate((sel) => {
    const bad = [];
    for (const root of document.querySelectorAll(sel))
      for (const e of [root, ...root.querySelectorAll("*")]) {
        if (!e.getClientRects().length) continue;
        const s = getComputedStyle(e);
        const texty = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
        if (texty && e.namespaceURI === "http://www.w3.org/1999/xhtml" && ![11, 12, 13, 15].includes(parseFloat(s.fontSize))) bad.push(`font-size ${s.fontSize}`);
        if (texty && /mono|menlo|courier/i.test(s.fontFamily) && !e.closest("[data-code]")) bad.push("mono font");
        for (const c of ["borderTopLeftRadius", "borderTopRightRadius", "borderBottomRightRadius", "borderBottomLeftRadius"]) {
          const r = parseFloat(s[c]);
          if (![0, 2, 4, 8].includes(r) && r < 9999) bad.push(`radius ${s[c]}`);
        }
        if (/gradient/.test(s.backgroundImage)) bad.push("gradient");
        if (s.backdropFilter && s.backdropFilter !== "none") bad.push("backdrop-filter");
      }
    return [...new Set(bad)];
  }, sel);

await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');
const orig = files();
const rev0 = (await state()).view.revision;

// ---------------------------------------------------------------- the buttons, placed from the header's right edge

const places = async () =>
  page.evaluate(() => {
    const out = {};
    for (const t of document.querySelectorAll("[data-testid=timeline] [data-module-tab]")) {
      const h = t.getBoundingClientRect();
      const at = (sel) => {
        const e = t.querySelector(sel);
        if (!e) return null;
        const b = e.getBoundingClientRect();
        return [Math.round(b.left - h.left), Math.round(b.width)];
      };
      const name = t.querySelector("[data-testid=track-label]").parentElement.getBoundingClientRect();
      out[t.dataset.moduleTab] = { w: Math.round(h.width), m: at("[data-mute]"), s: at("[data-solo]"), meter: at("[data-slot=meter]"), name: Math.round(name.right - h.left) };
    }
    return out;
  });
const p1440 = await places();
check(
  ["kick", "hats", "bass", "keys"].every((m) => p1440[m].w === 240 && same(p1440[m].m, [180, 20]) && same(p1440[m].s, [202, 20]) && same(p1440[m].meter, [231, 3])),
  `M at x = 180, S at 202, 20 px each, and the meter's spot at 231 in every 240 px header: ${JSON.stringify(p1440.kick)}`,
);
check(!p1440.song?.m && !p1440.song?.s, "the Song row has no M or S (MON-1)");
check(!(await page.$("[data-testid=overlay-pill]")), "with nothing muted or soloed there is no mutes pill");
check(
  (await fill("mute", "kick")) === "rgba(0, 0, 0, 0)" && (await page.$eval('[data-mute="kick"]', (e) => getComputedStyle(e).color)) === FG_MUTED,
  "M at rest: no fill, its letter fg-muted",
);

// ---------------------------------------------------------------- M on Kick

await page.click('[data-mute="kick"]');
check(await until(async () => (await pill()) === "1 muted", 3000), `M on Kick: the pill says “${await pill()}”`);
await page.mouse.move(700, 600);
check(await until(async () => (await pressed("mute", "kick")) && (await fill("mute", "kick")) === GRAY_700, 1000), "M lights gray-700");
check(await until(() => same(saved()?.mute, ["kick"]), 1500), `.music2/overlays.json says ["kick"] within a second: ${JSON.stringify(saved())}`);
check(same(files(), orig) && (await state()).view.revision === rev0, "no project file changed and no revision was made");
check(!(await state()).can_undo, "and it is no undo step");
check((await clipOpacity("kick")) === 0.4 && (await clipOpacity("bass")) === 1, `Kick's clips draw at 40% (${await clipOpacity("kick")}), Bass's as before`);
check((await nameColour("kick")) === FG_MUTED, "Kick's name is fg-muted");
check(await page.evaluate(() => document.activeElement === document.body), "the click left the focus where it was");
await key(" ");
check(await until(async () => (await api("/status")).body.playing, 3000), "so Space plays at once (M2)");
check(same((await overlays()).mute, ["kick"]), "and Kick stays muted");
await key(" ");
await until(async () => !(await api("/status")).body.playing);
const feeds = (await overlays()).feeds.kick ?? [];
check(feeds.some((f) => f.label === "Duck"), `the state says Kick feeds the Duck: ${JSON.stringify(feeds)}`);
check((await title("mute", "kick")).startsWith("Muted · still drives Duck"), `M's tooltip says so: “${await title("mute", "kick")}” (M3)`);

// ---------------------------------------------------------------- S on Bass, ⌥-click

await page.click('[data-solo="bass"]');
check(await settled(["kick"], ["bass"]), "S on Bass solos it");
const heard = (await overlays()).heard;
check(heard.hats === "silenced" && heard.keys === "silenced" && heard.kick === "muted" && heard.bass === "heard", `Hats and Keys are silenced, Kick still muted: ${JSON.stringify(heard)}`);
await page.mouse.move(700, 600);
check(await until(async () => (await fill("solo", "bass")) === ACCENT, 1000), "S lights accent");
check(await until(async () => (await nameColour("hats")) === FG_FAINT && (await nameColour("keys")) === FG_FAINT), "the silenced names are fg-faint");
check((await nameColour("kick")) === FG_MUTED && (await clipOpacity("hats")) === 0.4, "Kick still shows muted, and the silenced clips draw at 40%");
check((await pill()) === "1 muted · 1 solo", `the pill says “${await pill()}”`);
check((await title("solo", "keys")).includes("⌥-click: solo only this track"), `S's tooltip says ⌥-click: “${await title("solo", "keys")}”`);
await page.keyboard.down("Alt");
await page.click('[data-solo="keys"]');
await page.keyboard.up("Alt");
check(await settled(["kick"], ["keys"]), "⌥-click on S solos only Keys");
await page.click('[data-solo="keys"]');
check(await settled(["kick"], []), "a click on its S unsolos it");

// ---------------------------------------------------------------- a drag across M, quick clicks (M6)

const from = mid(await box('[data-mute="hats"]')), to = mid(await box('[data-mute="keys"]'));
await page.mouse.move(from.x, from.y);
await page.mouse.down();
await page.mouse.move(to.x, to.y, { steps: 16 });
await page.mouse.up();
check(await settled(["bass", "hats", "keys", "kick"], []), "a drag from Hats' M across Bass' to Keys' mutes all three");
check(same(files(), orig) && (await state()).view.revision === rev0, "still nothing written but overlays.json");
for (let i = 0; i < 3; i++) await page.click('[data-mute="hats"]');
check(await settled(["bass", "keys", "kick"], []), "three quick clicks on Hats' M end unmuted, on screen and on the server");
await page.click('[data-mute="kick"]');
await page.click('[data-mute="bass"]');
await page.click('[data-mute="keys"]');
await page.click('[data-solo="hats"]');
check(await settled([], ["hats"]), "quick clicks on three Ms and an S land as clicked");
check((await pill()) === "1 solo", `the pill says “${await pill()}”`);

// ---------------------------------------------------------------- the keys

await select("bass");
await key("m");
check(await settled(["bass"], ["hats"]), "M mutes the selected track");
await key("s");
check(await settled(["bass"], ["bass", "hats"]), "S solos it");
await key("Control+m");
check(await settled([], ["bass", "hats"]), "⌃M (the Mac's Track ▸ Mute) unmutes it");
await key("Control+s");
check(await settled([], ["hats"]), "and ⌃S unsolos it");
await page.click('[data-module-tab="song"] [data-testid=track-label]');
await until(() => page.$eval('[data-track="song"]', (e) => e.dataset.selected === "true").catch(() => false));
await key("m");
await key("s");
await sleep(400);
check(same((await overlays()).mute, []) && same((await overlays()).solo, ["hats"]), "on the Song row, M and S do nothing");

// ---------------------------------------------------------------- the pill's list, Clear all

await page.click('[data-mute="kick"]');
await page.click('[data-solo="bass"]');
check(await settled(["kick"], ["bass", "hats"]), "Kick muted, Bass and Hats soloed");
check((await pill()) === "1 muted · 2 solos", `the pill counts them: “${await pill()}”`);
const pillEl = await page.$("[data-testid=overlay-pill]");
check((await pillEl.$$("button, svg")).length === 0, "the pill has no × of its own");
check((await pillEl.evaluate((e) => e.title)).startsWith("Mutes and solos while you listen. The export has every track."), "its tooltip says what they are");
const pb = await box("[data-testid=overlay-pill]"), ub = await box("button[aria-label=Undo]");
check(pb.x + pb.width <= ub.x && pb.height <= 24, "it sits left of Undo, one line high");
await pillEl.click();
const pop = await page.waitForSelector("[data-testid=overlay-popover]", { timeout: 2000 }).catch(() => null);
const rows = await page.$$eval("[data-overlay-row]", (els) => els.map((e) => [e.dataset.overlayRow, e.innerText.replace(/\s+/g, " ").trim()]));
check(
  !!pop && same(rows.map((r) => r[0]), ["kick", "hats", "bass"]) && rows[0][1] === "Kick Muted Still drives Duck" && rows[1][1] === "Hats Soloed" && rows[2][1] === "Bass Soloed",
  `its list has each track in order, Kick's row saying it still drives the Duck: ${JSON.stringify(rows)}`,
);
check((await page.$$("[data-overlay-clear]")).length === 3 && !!(await page.$("[data-testid=overlay-clear-all]")), "each row has its ×, and Clear all sits at the foot");
const bad = await lint("[data-testid=overlay-popover], [data-testid=overlay-pill], [data-slot=mute-solo]");
check(bad.length === 0, `the pill, its list, M and S keep to the design system${bad.length ? `: ${bad.join("; ")}` : ""}`);
await sleep(300);
await page.screenshot({ path: `${ROOT}/s8-mute-pill.png` });
await page.click('[data-overlay-clear="hats"]');
check(await settled(["kick"], ["bass"]), "Hats' × unsolos it, and the list stays open");
check(!!(await page.$("[data-testid=overlay-popover]")) && (await page.$$("[data-overlay-row]")).length === 2, "with two rows now");
const focused = await page.evaluate(() => (document.activeElement?.closest("button, [data-testid=overlay-popover]") ? document.activeElement.outerHTML.slice(0, 120) : ""));
check(focused === "", `no button took the focus ${focused}`);
await page.click("[data-testid=overlay-clear-all]");
check(await settled([], []), "Clear all clears every mute and solo");
check(await until(async () => !(await page.$("[data-testid=overlay-pill]")) && !(await page.$("[data-testid=overlay-popover]")), 2000), "and the pill and its list go");
check(await until(() => same(saved(), { mute: [], solo: [] }), 1500), "overlays.json says so");
check(same(files(), orig) && (await state()).view.revision === rev0 && !(await state()).can_undo, "none of it wrote the project, made a revision or an undo step");

// ---------------------------------------------------------------- mute in the song (MON-5; U2)

const kickTs = orig["tracks/kick.ts"];
const openMenu = async (m) => {
  await page.hover(`[data-module-tab="${m}"]`);
  await page.click(`[data-track-menu="${m}"]`);
  return !!(await page.waitForSelector("[data-testid=track-menu]", { timeout: 2000 }).catch(() => null));
};
const menuRows = () => page.$$eval("[data-testid=track-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
const struck = (m) => page.$eval(`[data-track="${m}"] [data-testid=track-label]`, (e) => getComputedStyle(e).textDecorationLine).catch(() => null);
const hatched = (m) => page.$$eval(`[data-track="${m}"] [data-clip] pattern[patternTransform="rotate(45)"]`, (els) => els.length).catch(() => 0);
const toasted = (text) => until(async () => (await toasts()).some((t) => t.includes(text)), 5000);

check(await openMenu("kick"), "Kick's header menu opens");
const rows0 = await menuRows();
check(
  rows0.some((r) => r.startsWith("Mute in songSaved in its code: the export leaves it out too")) && !rows0[0].startsWith("Mute in song"),
  `it has Mute in song, saying what it does, below the first rows: ${JSON.stringify(rows0)}`,
);
await sleep(200);
await page.screenshot({ path: `${ROOT}/s8-mute-menu.png` });
await page.click("[data-testid=mute-in-song]");
check(await until(() => read("tracks/kick.ts") === kickTs.replace('{ name: "Kick" }', '{ name: "Kick", muted: true }')), "Mute in song writes { name: \"Kick\", muted: true } in tracks/kick.ts, and nothing else there");
const rev1 = (await state()).view.revision;
check(rev1 === rev0 + 1 && same(Object.keys(files()).filter((f) => files()[f] !== orig[f]), ["tracks/kick.ts"]), "one revision, and only that file changed");
check(await toasted("Kick muted in the song"), `the toast says “Kick muted in the song”: ${JSON.stringify(await toasts())}`);
check(
  await page.evaluate(() => [...document.querySelectorAll("[data-testid=toast]")].some((t) => t.textContent.includes("Kick muted in the song") && [...t.querySelectorAll("button")].some((b) => b.textContent === "Undo"))),
  "with Undo",
);
await page.mouse.move(700, 600);
check(await until(async () => (await overlays()).heard.kick === "muted-in-song" && same((await overlays()).muted_in_song, ["kick"])), "the state says Kick is muted in the song");
check(await until(async () => (await struck("kick")) === "line-through" && (await nameColour("kick")) === FG_MUTED), "its name is struck through, fg-muted");
check(await until(async () => (await pressed("mute", "kick")) && (await fill("mute", "kick")) === GRAY_700, 1000), `its M is lit gray-700 (M5): ${await fill("mute", "kick")}`);
check((await hatched("kick")) > 0 && (await clipOpacity("kick")) === 0.4 && (await hatched("hats")) === 0, "its clips draw at 40%, hatched; Hats' aren't");
check(!(await page.$("[data-testid=overlay-pill]")), "no mutes pill: that mute is the song's, not listening's");
await page.screenshot({ path: `${ROOT}/s8-mute-in-song.png` });

await page.click('[data-mute="kick"]');
const menu1 = await page.waitForSelector("[data-testid=track-menu]", { timeout: 2000 }).catch(() => null);
check(!!menu1 && (await menuRows())[0] === "Unmute in song", `a click on its M opens its menu with Unmute in song first: ${JSON.stringify(await menuRows())}`);
check(same((await overlays()).mute, []), "and mutes nothing while listening");
await page.keyboard.press("Escape");
await select("kick");
await key("m");
check(await toasted("Kick is muted in the song. Unmute in song is in its menu."), "the M key skips it, and says where its unmute is");
await sleep(300);
check(same((await overlays()).mute, []), "it muted nothing");
const hm = mid(await box('[data-mute="hats"]')), km = mid(await box('[data-mute="kick"]'));
await page.mouse.move(hm.x, hm.y);
await page.mouse.down();
await page.mouse.move(km.x, km.y, { steps: 10 });
await page.mouse.up();
check(await settled(["hats"], []), "a drag from Hats' M across Kick's mutes Hats only");
await page.click('[data-mute="hats"]');
await page.click('[data-solo="kick"]');
check(await settled([], ["kick"]), "S on Kick lights");
check((await overlays()).heard.kick === "muted-in-song", "but mute wins: Kick is still muted in the song");
check((await title("solo", "kick")) === "Muted in the song: unmute it to hear it", `S's tooltip says why: “${await title("solo", "kick")}”`);
await page.click('[data-solo="kick"]');
await settled([], []);

check((await page.$eval("button[aria-label=Undo]", (e) => e.title)) === "Undo mute in song (⌘Z)", `Undo names it: “${await page.$eval("button[aria-label=Undo]", (e) => e.title)}”`);
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === kickTs), "⌘Z puts tracks/kick.ts back byte for byte");
check(await toasted("Undid mute in song"), "and says “Undid mute in song”");
check(await until(async () => (await overlays()).heard.kick === "heard" && (await struck("kick")) === "none" && !(await pressed("mute", "kick"))), "Kick is heard again: no strike, M off");
await key("Meta+Shift+z");
check(await until(() => read("tracks/kick.ts").includes("muted: true")), "redo mutes it in the song again");
check(await openMenu("kick"), "its menu opens");
await page.click("[data-testid=mute-in-song]");
check(await until(() => read("tracks/kick.ts") === kickTs), "Unmute in song takes the option out");
check(await toasted("Kick unmuted in the song"), "the toast says “Kick unmuted in the song”");
check(same(files(), orig), "every project file is as it was");

// ---------------------------------------------------------------- the export's question (EX-3; U3)

const M2 = process.argv[5];
const exportsDir = `${HOME}/Music/music2/Exports`;
const wav = (name) => `${exportsDir}/${name}.wav`;
const ask = () => page.$("[data-testid=export-ask]");
const askText = () => page.$eval("[data-testid=export-ask]", (e) => [e.querySelector("h2").textContent, e.querySelector("p").textContent, [...e.querySelectorAll("button")].map((b) => b.textContent)]).catch(() => null);
const exported = (name) => until(async () => (await toasts()).some((t) => t.startsWith(`Exported ${name}.wav`)) && fs.existsSync(wav(name)), 60000);
const doneToast = async (name) => (await toasts()).find((t) => t.startsWith(`Exported ${name}.wav`)) ?? "";
/** `m2 render` of the project as it is now, to compare an export with. */
const rendered = (name) => {
  const out = `${ROOT}/${name}.wav`;
  execFileSync(M2, ["render", PROJ, out]);
  return fs.readFileSync(out);
};
const idle = () => until(async () => !(await page.$("[data-testid=export-progress]")), 60000);
// A button of the question, clicked as itself: the last export's long toast may lie over it.
const answer = (id) => page.$eval(`[data-testid=${id}]`, (b) => b.click());

await page.click('[data-mute="kick"]');
await settled(["kick"], []);
await key("Meta+Shift+e");
check(!!(await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 }).catch(() => null)), "⇧⌘E with Kick muted asks first");
check(
  same(await askText(), ["Kick is muted only while you listen", "It will be in the file.", ["Export anyway", "Mute Kick in the song, then export"]]),
  `“Kick is muted only while you listen”, “It will be in the file.”, [Export anyway] [Mute Kick in the song, then export]: ${JSON.stringify(await askText())}`,
);
check(await page.$eval("[data-testid=export-mute]", (e) => e === document.activeElement && e.className.includes("bg-accent")), "the second is the main button, and has the focus");
check(!fs.existsSync(exportsDir) || fs.readdirSync(exportsDir).length === 0, "nothing is exported yet");
const askBad = await lint("[data-testid=export-ask]");
check(askBad.length === 0, `the question keeps to the design system${askBad.length ? `: ${askBad.join("; ")}` : ""}`);
await page.screenshot({ path: `${ROOT}/s8-mute-export-ask.png` });
await page.keyboard.press("Escape");
check(await until(async () => !(await ask()), 2000), "Esc cancels it");
await sleep(500);
check(!fs.existsSync(exportsDir) || fs.readdirSync(exportsDir).length === 0, "and exports nothing");

await page.click("[data-testid=export]");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
const revM = (await state()).view.revision;
await answer("export-mute");
check(await exported("first-song"), "Mute Kick in the song, then export: the export runs");
check(read("tracks/kick.ts") === kickTs.replace('{ name: "Kick" }', '{ name: "Kick", muted: true }') && (await state()).view.revision === revM + 1, "Kick is muted in its code first, in one revision");
check(same((await overlays()).mute, []) && (await overlays()).heard.kick === "muted-in-song", "its listening mute is gone: the song mutes it now");
const t1 = await doneToast("first-song");
check(/^Exported first-song\.wav \(0:\d\d\) to .+\. Kick stays muted in the song\.$/.test(t1), `the done toast says Kick stays muted in the song: “${t1}”`);
check(Buffer.compare(fs.readFileSync(wav("first-song")), rendered("cli-muted")) === 0, "the file equals `m2 render`'s, byte for byte: Kick isn't in it");
await idle();
await key("Meta+z");
check(await until(() => read("tracks/kick.ts") === kickTs), "⌘Z puts tracks/kick.ts back");
await sleep(300);
check(same((await overlays()).mute, []), "and doesn't bring back the listening mute");

await page.click('[data-mute="kick"]');
await settled(["kick"], []);
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
await answer("export-anyway");
check(await exported("first-song 2"), "Export anyway exports");
check(Buffer.compare(fs.readFileSync(wav("first-song 2")), rendered("cli-plain")) === 0, "the file equals `m2 render`'s: every track is in it");
const t2 = await doneToast("first-song 2");
check(t2.endsWith("Kick was muted only while you listened, so it's in the file."), `its toast says so, by name: “${t2}”`);
check(read("tracks/kick.ts") === kickTs && same((await overlays()).mute, ["kick"]), "Kick stays muted only while listening");
await idle();

// From the keyboard: Return and Space press the button with the focus ring, Tab stays in the
// question, and no key reaches the page behind it (Space would play, M would unmute Kick, Return
// would send the start point home).
const beatNow = async () => (await api("/status")).body.beat;
await post("/seek", { beat: 8 });
await until(async () => Math.abs((await beatNow()) - 8) < 0.01);
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
await page.keyboard.press("m");
await sleep(400);
check(same((await overlays()).mute, ["kick"]) && !!(await ask()), "with the question open, M mutes nothing behind it");
const focusedId = () => page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.tagName);
const tabs = [];
for (const back of [true, true, false, false]) {
  if (back) await page.keyboard.down("Shift");
  await page.keyboard.press("Tab");
  if (back) await page.keyboard.up("Shift");
  tabs.push(await focusedId());
}
check(same(tabs, ["export-anyway", "export-mute", "export-anyway", "export-mute"]), `Tab and Shift+Tab go round the two buttons, never behind the question: ${tabs.join(" → ")}`);
await page.keyboard.down("Shift");
await page.keyboard.press("Tab");
await page.keyboard.up("Shift");
await page.keyboard.press("Enter");
check(await exported("first-song 3"), "Shift+Tab, then Return on Export anyway: it exports");
check(read("tracks/kick.ts") === kickTs && same((await overlays()).mute, ["kick"]), "Kick's code is untouched and it stays muted only while listening");
check(Math.abs((await beatNow()) - 8) < 0.01, `and the start point stays at beat 8: ${await beatNow()}`);
await idle();
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
await page.keyboard.down("Shift");
await page.keyboard.press("Tab");
await page.keyboard.up("Shift");
await page.keyboard.press("Space");
check(await exported("first-song 4"), "Shift+Tab, then Space on Export anyway: it exports too");
check(!(await api("/status")).body.playing && read("tracks/kick.ts") === kickTs, "and nothing plays; Kick's code is untouched");
await idle();
await post("/seek", { beat: 0 });

await page.click('[data-mute="kick"]');
await page.click('[data-solo="bass"]');
await settled([], ["bass"]);
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
check(
  same(await askText(), ["Only Bass is soloed", "The file will have every track.", ["Cancel", "Export anyway"]]),
  `with Bass soloed: “Only Bass is soloed”, [Cancel] [Export anyway]: ${JSON.stringify(await askText())}`,
);
await page.keyboard.press("Enter");
check(await exported("first-song 5"), "Return exports anyway");
const t3 = await doneToast("first-song 5");
check(t3.endsWith("Only Bass was soloed while you listened, so the file has every track."), `its toast says so: “${t3}”`);
await idle();
await page.click('[data-mute="kick"]');
await settled(["kick"], ["bass"]);
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
check(
  same(await askText(), ["Kick is muted and only Bass is soloed", "The file will have every track.", ["Cancel", "Export anyway"]]),
  `a mute and a solo: the solo's question, naming both: ${JSON.stringify(await askText())}`,
);
await answer("export-cancel");
check(await until(async () => !(await ask()), 2000), "Cancel closes it");
await page.click('[data-solo="bass"]');
await settled(["kick"], []);

// While the code doesn't compile, mute in the song can't run (M8): the question says so.
const songTs = read("song.ts");
write("song.ts", songTs.replace("export default", "export default )"));
check(await until(async () => (await state()).view.draft_errors.length > 0, 10000), "the code is broken");
await key("Meta+Shift+e");
await page.waitForSelector("[data-testid=export-ask]", { timeout: 3000 });
const broken = await askText();
check(
  same(broken, ["Kick is muted only while you listen", "It will be in the file.", ["Cancel", "Export anyway"]]) &&
    (await page.$eval("[data-testid=export-ask-why]", (e) => e.textContent).catch(() => "")) === "Can't mute in the song now: the code doesn't compile.",
  `then only [Cancel] [Export anyway], and why: ${JSON.stringify(broken)}`,
);
await page.keyboard.press("Escape");
write("song.ts", songTs);
check(await until(async () => (await state()).view.draft_errors.length === 0, 10000), "the code compiles again");
await page.click('[data-mute="kick"]');
await settled([], []);
check(same(files(), orig), "every project file is as it was");

// ---------------------------------------------------------------- a small window: 200 px headers (W10)

await page.click('[data-mute="kick"]');
await page.click('[data-solo="bass"]');
await page.click('[data-mute="hats"]');
await settled(["hats", "kick"], ["bass"]);
await page.setViewport({ width: 1100, height: 700 });
await sleep(300);
const p1100 = await places();
check(
  ["kick", "hats", "bass", "keys"].every((m) => p1100[m].w === 200 && same(p1100[m].m, [140, 20]) && same(p1100[m].s, [162, 20]) && same(p1100[m].meter, [191, 3]) && p1100[m].name <= 140),
  `at 1100 px the headers are 200 px: M at 140, S at 162, the meter at 191, the name ending before M: ${JSON.stringify(p1100.kick)}`,
);

// ---------------------------------------------------------------- the top bar: a long name, a loading state and the pill

const longName = "My very first summer song with friends and a much longer name than fits here";
check((await post("/title", { session: (await state()).session, name: longName })).status === 200, "the song gets a 77-character name");
await page.evaluate(() => {
  const real = window.fetch.bind(window);
  window.__status = { loading: ["Surge XT", "Airwindows Consolidated"] };
  window.fetch = async (url, init) => {
    const r = await real(url, init);
    if (!window.__status || !String(url).includes("/api/graph/status")) return r;
    return new Response(JSON.stringify({ ...(await r.json()), ...window.__status }), { status: r.status, headers: r.headers });
  };
});
const bar = () =>
  page.evaluate(() => {
    const rect = (el) => {
      const b = el.getBoundingClientRect();
      return { l: b.left, r: b.right, h: b.height };
    };
    const status = document.querySelector("[data-status]");
    const p = document.querySelector("[data-testid=overlay-pill]");
    const name = document.querySelector("[data-testid=bar-song-name]").parentElement;
    return status && p && { status: rect(status), statusText: status.textContent, statusTitle: status.title, pill: rect(p), pillText: p.textContent, name: rect(name), undo: rect(document.querySelector("button[aria-label=Undo]")) };
  });
for (const mac of [false, true]) {
  await page.evaluate((on) => (on ? (document.documentElement.dataset.app = "mac") : delete document.documentElement.dataset.app), mac);
  for (const w of [1100, 1180, 1280]) {
    await page.setViewport({ width: w, height: 800 });
    await until(async () => (await bar())?.statusTitle.startsWith("Loading"), 3000);
    await sleep(150);
    const b = await bar();
    const shows = b && b.name.r > b.name.l;
    const clear = b && b.status.h <= 24 && b.pill.h <= 24 && b.status.r + 4 <= b.pill.l && b.pill.r <= b.undo.l && (!shows || b.name.r + 4 <= b.status.l);
    check(
      clear && b.pillText === "2 muted · 1 solo" && b.statusTitle === "Loading Surge XT, Airwindows Consolidated…",
      `${w} px${mac ? " in the Mac app" : ""}: the name (…${b?.name.r.toFixed(0)}), “${b?.statusText}” (${b?.status.l.toFixed(0)}–${b?.status.r.toFixed(0)}) and “${b?.pillText}” (${b?.pill.l.toFixed(0)}–${b?.pill.r.toFixed(0)}) don't overlap, each on one line`,
    );
    await page.screenshot({ path: `${ROOT}/s8-mute-bar-${w}${mac ? "-mac" : ""}.png`, clip: { x: 0, y: 0, width: w, height: 44 } });
  }
}
await page.evaluate(() => {
  delete document.documentElement.dataset.app;
  window.__status = null;
});
await page.setViewport({ width: 1440, height: 900 });

// ---------------------------------------------------------------- load and dropouts in the status pill (8b.8; U11)

const pillState = () =>
  page.evaluate(() => {
    const p = document.querySelector("[data-status]");
    return p && { ids: p.dataset.status, text: p.textContent, title: p.title, bg: getComputedStyle(p).backgroundColor };
  });
const popoverText = async () => {
  if (!(await page.$("[data-testid=status-popover]"))) await page.click("[data-status]").catch(() => {});
  const t = await page.$eval("[data-testid=status-popover]", (e) => e.innerText.replace(/\s+/g, " ")).catch(() => "");
  await page.keyboard.press("Escape");
  return t;
};
const WARN_SOFT = rgb("#fbefd6");
await post("/transport", { action: "play" });
check(await until(async () => (await api("/status")).body.playing, 3000), "the song plays");
await sleep(1500);
const real = (await api("/status")).body;
console.log(`INFO  the server's own load while playing: ${real.load}, late ${real.late}, xruns ${real.xruns}`);
// The page's own fetch wrapper (above) says what the status would: a quiet engine first.
await page.evaluate(() => (window.__status = { load: 0.2, late: 0, xruns: 0 }));
await sleep(600);
check(!(await pillState()), "a load of 20% says nothing");
await page.evaluate(() => (window.__status = { load: 0.75, late: 0, xruns: 0 }));
check(await until(async () => (await pillState())?.ids === "load", 3000), "75% while playing: the status pill says “Heavy load”");
const heavy = await pillState();
check(heavy.text === "Heavy load" && heavy.bg === WARN_SOFT, `in warn: “${heavy.text}”`);
const heavyWords = await popoverText();
check(
  heavyWords.includes("Heavy load At its busiest moment in the last second, the sound took 75% of the time the computer has for it. At 100% it drops out. Sample rate 48 kHz"),
  `its popover says what it means, in plain words: “${heavyWords}”`,
);
await page.evaluate(() => (window.__status = { load: 0.75, late: 0, xruns: 1 }));
check(await until(async () => (await pillState())?.ids === "dropout load", 3000), `a dropout: “Sound dropped out” comes first: ${(await pillState())?.ids}`);
check((await pillState()).text === "Sound dropped out+1", `the pill says “${(await pillState()).text}”`);
const dropWords = await popoverText();
check(dropWords.includes("Sound dropped out The sound dropped out once since play started. Closing other apps or removing a plugin helps."), `its popover: “${dropWords}”`);
await page.evaluate(() => (window.__status = { load: 0.2, late: 2, xruns: 1 }));
check(await until(async () => (await pillState())?.ids === "dropout", 3000), "two late callbacks are dropouts too, and the load is back down");
check((await popoverText()).includes("dropped out 3 times since play started"), "three since play started");
const lastDrop = Date.now();
await sleep(8000);
check((await pillState())?.ids === "dropout", "it is still there 8 s after the last one");
check(await until(async () => !(await pillState()), 4000) && (await api("/status")).body.playing, `and gone 10 s after it, still playing (${((Date.now() - lastDrop) / 1000).toFixed(1)} s)`);
await page.evaluate(() => (window.__status = { load: 0.9, late: 3, xruns: 1 }));
await post("/transport", { action: "stop" });
check(await until(async () => !(await api("/status")).body.playing, 3000), "the song stops");
await sleep(600);
check(!(await pillState()), "stopped, neither shows, whatever the numbers say");
await post("/transport", { action: "play" });
await until(async () => (await api("/status")).body.playing, 3000);
// The page has seen play start (its counts from here), then one more late callback.
await sleep(800);
await page.evaluate(() => (window.__status = { load: 0.9, late: 4, xruns: 1 }));
const songTs4 = read("song.ts");
write("song.ts", songTs4.replace("export default", "export default )"));
check(await until(async () => (await pillState())?.ids === "compile dropout load", 10000), `a problem still ranks above both: ${(await pillState())?.ids}`);
write("song.ts", songTs4);
await until(async () => (await state()).view.draft_errors.length === 0, 10000);
await page.evaluate(() => (window.__status = null));
await post("/transport", { action: "stop" });
await until(async () => !(await api("/status")).body.playing, 3000);

// ---------------------------------------------------------------- a reload, and a read-only window (MON-4)

await page.reload({ waitUntil: "domcontentloaded" });
await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');
check(await until(async () => (await pill()) === "2 muted · 1 solo" && (await pressed("mute", "kick")) && (await pressed("solo", "bass"))), "a reload keeps the mutes and the solo");

check((await post("/close", {})).status === 200, "the song closes");
const holder = spawn(
  "python3",
  [
    "-c",
    `
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
os.ftruncate(fd, 0)
os.write(fd, sys.argv[2].replace("PID", str(os.getpid())).encode())
print("ready", flush=True)
sys.stdin.read()
`,
    `${PROJ}/.music2/lock`,
    JSON.stringify({ pid: 0, program: "agent", host: "here", host_id: "x", token: "0", since: Math.floor(Date.now() / 1000) }).replace('"pid":0', '"pid":PID'),
  ],
  { stdio: ["pipe", "pipe", "inherit"] },
);
await new Promise((r) => holder.stdout.once("data", r));
const opened = await post("/open", { path: PROJ });
check(opened.status === 200 && opened.body.read_only?.why === "held", "opened again while another program holds it: read-only");
await page.waitForSelector("[data-testid=read-only-banner]", { timeout: 6000 }).catch(() => {});
check(await until(async () => (await pill()) === "2 muted · 1 solo"), "the mutes come back from the file");
const before = fs.readFileSync(overlaysFile, "utf8");
await page.click('[data-mute="keys"]');
check(await settled(["hats", "keys", "kick"], ["bass"]), "M works in a read-only window");
await sleep(800);
check(fs.readFileSync(overlaysFile, "utf8") === before, "but nothing is written: overlays.json is as it was");
check((await page.$eval("[data-testid=overlay-pill]", (e) => e.title)).startsWith("Not saved: this song is open in another window."), "the pill's tooltip says they aren't saved");
await page.click("[data-testid=overlay-pill]");
await page.waitForSelector("[data-testid=overlay-popover]", { timeout: 2000 }).catch(() => {});
check((await page.$eval("[data-testid=overlay-popover]", (e) => e.innerText).catch(() => "")).includes("Not saved: this song is open in another window."), "and so does its list");
await page.keyboard.press("Escape");
check(await openMenu("kick"), "Kick's menu opens read-only");
check(await page.$eval("[data-testid=mute-in-song]", (e) => e.disabled && e.title.startsWith("Read only")), "Mute in song is off there, saying why");
await page.keyboard.press("Escape");
const kickRo = read("tracks/kick.ts");
const roMute = await post("/track/mute", { session: (await state()).session, modules: ["kick"], muted: true });
check(roMute.status === 423 && read("tracks/kick.ts") === kickRo, `the route refuses it, 423, and writes nothing: ${roMute.body.error?.slice(0, 60)}`);
holder.stdin.end();
await new Promise((r) => holder.once("exit", r));

// ---------------------------------------------------------------- a PC page

const pc = await open(1440, 900, { pc: true });
await pc.page.waitForSelector('[data-solo="keys"]');
const pcTitle = await pc.page.$eval('[data-solo="keys"]', (e) => e.title);
check(pcTitle.includes("Alt-click: solo only this track") && !pcTitle.includes("⌥"), `on a PC, S's tooltip says Alt-click: “${pcTitle}”`);
errors.push(...pc.errors);
await pc.browser.close();

await browser.close();
done(errors);

// The keys and labels on a PC (OS-54, OS-55, OS-75; plan A3): a page given a PC's user agent and
// `platform: "linux"`, on every OS, as Linux (and later Windows) users get it. Every other suite
// tests the Mac's keys. Checks:
//   labels   the tooltips, the dock's tabs, a knob's, the track menu and the ⌥K sheet say Ctrl,
//            Alt, Shift, Enter and Backspace; no Mac key symbol shows anywhere
//   keys     Ctrl+T opens + Track; Ctrl+R renames; Ctrl+Z and Ctrl+Shift+Z undo and redo;
//            Ctrl+Backspace deletes the selected track, but in a rename field the word before;
//            Alt+1 to Alt+4 switch the dock's views; Alt+K shows the sheet
//   code     in the code editor Ctrl+D adds the next match, Ctrl+Backspace deletes a word,
//            Ctrl+Z undoes typing and Ctrl+S saves
// The app on Linux takes ⌘ chords through GTK's menu first: the Xvfb checks in
// docs/designs/step8/build-notes-taste.md (A3) cover that path.
//
//   node e2e/pc-keys.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import fs from "node:fs";
import { PROJ, check, done, open, read, sleep, until } from "./fixlib.mjs";

const { page, errors, state, key, box } = await open(1440, 900, { pc: true });
const exists = (f) => fs.existsSync(`${PROJ}/${f}`);
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const title = (sel) => page.$eval(sel, (e) => e.title).catch(() => null);
const shown = () => page.$$eval("[data-testid=timeline] [data-track]", (els) => els.filter((e) => !e.closest("[data-song-row]")).map((e) => e.dataset.track));
const select = async (m) => {
  await page.click(`[data-module-tab="${m}"]`);
  await until(() => page.$eval(`[data-track="${m}"]`, (e) => e.dataset.selected === "true"));
};
const selectedView = () => page.$eval('[data-view-tab][aria-selected="true"]', (e) => e.dataset.viewTab).catch(() => null);

/** Every Mac key symbol the page shows: in text, titles, labels and placeholders, outside the code editor and code. */
const macSymbols = () =>
  page.evaluate(() => {
    const MAC = /[⌘⌥⇧⌃⌫]/;
    const found = [];
    for (const e of document.body.querySelectorAll("*")) {
      if (e.closest(".monaco-editor, [data-code]")) continue;
      const text = [...e.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join("");
      for (const v of [text, e.getAttribute("title"), e.getAttribute("aria-label"), e.getAttribute("placeholder")]) {
        if (v && MAC.test(v)) found.push(v.replace(/\s+/g, " ").slice(0, 60));
      }
    }
    return [...new Set(found)];
  });
const noMac = async (where) => {
  const seen = await macSymbols();
  check(seen.length === 0, `${where}: no Mac key symbol${seen.length ? ` (${seen.slice(0, 4).join(" | ")})` : ""}`);
};

await page.waitForSelector('[data-track="kick"] [data-testid=track-label]');
const orig = { song: read("song.ts"), kick: read("tracks/kick.ts"), hats: read("tracks/hats.ts") };

// ---------------------------------------------------------------- the page is a PC's

const agent = await page.evaluate(() => ({ ua: navigator.userAgent, platform: navigator.platform, mac: document.documentElement.classList.contains("mac"), os: document.documentElement.dataset.os ?? null }));
check(!/Mac/.test(agent.ua) && /Linux/.test(agent.platform) && !agent.mac && agent.os === null, `the page is a PC's, on any OS (${agent.platform}; mac class ${agent.mac})`);

// ---------------------------------------------------------------- labels

const bar = {
  undo: await title('button[aria-label="Undo"]'),
  redo: await title('button[aria-label="Redo"]'),
  export: await title("[data-testid=export]"),
  browser: await title("[data-testid=browser-toggle]"),
  start: await title('button[aria-label="Back to start"]'),
};
check(
  bar.undo?.endsWith("(Ctrl+Z)") && bar.redo?.endsWith("(Ctrl+Shift+Z)") && bar.export?.endsWith("(Ctrl+Shift+E)") && bar.browser === "Browser (Ctrl+Alt+B)" && bar.start === "Go to start (Enter)",
  `the top bar's tooltips say Ctrl+Z, Ctrl+Shift+Z, Ctrl+Shift+E, Ctrl+Alt+B and Enter: ${Object.values(bar).join(" | ")}`,
);
const tabs = await page.$$eval("[data-view-tab]", (els) => els.map((e) => e.title));
check(same(tabs, ["Device (Alt+1)", "Notes (Alt+2)", "Mix (Alt+3)", "Code (Alt+4)"]), `the dock's tabs say Alt+1 to Alt+4: ${tabs.join(" | ")}`);
// The song bar (8b.2): the tempo's drag and Play's continue spell Shift.
const song = { tempo: await title("[data-testid=tempo-field]"), play: await title("[data-testid=play]") };
check(
  song.tempo?.includes("Drag up or down (Shift for fine)") && song.play === "Play (Space) · continue from where it stopped (Shift+Space)",
  `the song bar's tooltips say Shift: ${song.tempo?.split("\n")[1]} | ${song.play}`,
);
const knob = await page.$eval("[data-testid=graph] [data-knob-id]", (e) => e.title).catch(() => "");
check(knob.includes("Drag (Shift for fine)") && knob.includes("Alt-click to reset"), `a knob's tooltip says Shift for fine and Alt-click: ${knob.split("\n").at(-1)}`);
await noMac("the window");

// ---------------------------------------------------------------- the sheet (Alt+K)

await key("Alt+k");
const sheet = await page.waitForSelector("[data-testid=shortcuts]", { timeout: 2000 }).catch(() => null);
check(!!sheet, "Alt+K shows the keyboard shortcuts");
const rows = sheet ? Object.fromEntries(await page.$$eval("[data-testid=shortcuts] dl > div", (els) => els.map((e) => [e.querySelector("dt").textContent, e.querySelector("dd").textContent]))) : {};
const want = {
  "Undo / redo": "Ctrl+Z / Ctrl+Shift+Z",
  "New track": "Ctrl+T",
  "Delete the selected track": "Ctrl+Backspace",
  "Rename the selected track": "Ctrl+R / Shift+Enter",
  "Device / Notes / Mix / Code": "Alt+1 / Alt+2 / Alt+3 / Alt+4",
  "Show / hide the browser": "Ctrl+Alt+B",
  "Keyboard shortcuts": "Alt+K",
  "Export audio": "Ctrl+Shift+E",
  "Go to start": "Enter",
  "Continue from where it stopped": "Shift+Space",
  "Loop the selected clips, or the bar": "Ctrl+L",
  "Stop all sound": "Ctrl+.",
  "Fine adjust": "Shift-drag",
  "Reset to its default": "Alt-click / Backspace",
  "Move notes by the grid / by a bar": "← → / Shift+← Shift+→",
};
const wrong = Object.entries(want).filter(([what, keys]) => rows[what] !== keys);
check(wrong.length === 0, `the sheet says Ctrl, Alt, Shift, Enter and Backspace${wrong.length ? `: ${wrong.map(([w]) => `${w} = ${rows[w]}`).join("; ")}` : ""}`);
const cut = await page.$$eval("[data-testid=shortcuts] dl > div > *", (els) => els.filter((e) => e.scrollWidth > e.clientWidth + 1).map((e) => e.textContent));
check(cut.length === 0, `no row of it is cut short, with a PC's longer keys${cut.length ? `: ${cut.join(" | ")}` : ""}`);
await noMac("the sheet");
await key("Escape");
check(await until(async () => !(await page.$("[data-testid=shortcuts]"))), "Escape closes it");

// ---------------------------------------------------------------- + Track and the track menu

await select("kick");
await key("Control+t");
check(!!(await page.waitForSelector("[data-testid=instrument-list]", { timeout: 2000 }).catch(() => null)), "Ctrl+T opens + Track's list");
await noMac("+ Track's list");
await key("Escape");
check(await until(async () => !(await page.$("[data-testid=instrument-list]"))) && read("song.ts") === orig.song, "Escape closes it, adding nothing");

await page.hover('[data-module-tab="kick"]');
await page.click('[data-track-menu="kick"]');
await page.waitForSelector("[data-testid=track-menu]", { timeout: 2000 }).catch(() => {});
const menu = await page.$$eval("[data-testid=track-menu] [role=menuitem]", (els) => els.map((e) => e.textContent));
const has = (start, keys) => menu.some((r) => r.startsWith(start) && r.endsWith(keys));
check(has("Rename", "Ctrl+R") && has("Duplicate", "Ctrl+D") && has("Delete", "Ctrl+Backspace") && menu.some((r) => r.startsWith("Edit in code") && r.includes("Alt+4")), `the track menu says Ctrl+R, Ctrl+D, Ctrl+Backspace and Alt+4: ${menu.join(" | ")}`);
await noMac("the track menu");
await key("Escape");
await until(async () => !(await page.$("[data-testid=track-menu]")));

// ---------------------------------------------------------------- rename, and Ctrl+Backspace in its field

await key("Control+r");
const field = await page.waitForSelector('[data-track="kick"] [data-testid=track-rename]', { timeout: 2000 }).catch(() => null);
check(!!field, "Ctrl+R turns the selected track's name into a field");
await page.keyboard.type("Kick big drum");
await key("Control+Backspace");
const value = () => page.$eval('[data-track="kick"] [data-testid=track-rename]', (e) => e.value).catch(() => null);
check((await value()) === "Kick big ", `Ctrl+Backspace in it deletes the word before: "${await value()}"`);
await key("Control+Backspace");
check((await value()) === "Kick ", `and again, the next: "${await value()}"`);
await sleep(300);
check(same(await shown(), ["kick", "hats", "bass", "keys"]) && exists("tracks/kick.ts") && read("song.ts") === orig.song, "and the track stays: Ctrl+Backspace was the field's");
await page.keyboard.type("drum");
await key("Enter");
check(await until(() => read("tracks/kick.ts").includes('{ name: "Kick drum" }')), "Enter writes the name");
const rev = (await state()).view.revision;
await key("Control+z");
check(await until(() => read("tracks/kick.ts") === orig.kick), "Ctrl+Z undoes it");
await key("Control+Shift+z");
check(await until(() => read("tracks/kick.ts").includes('{ name: "Kick drum" }')) && (await state()).view.revision > rev, "Ctrl+Shift+Z redoes it");

// ---------------------------------------------------------------- Ctrl+Backspace deletes the selected track

await select("hats");
await key("Control+Backspace");
check(await until(() => !exists("tracks/hats.ts")), "Ctrl+Backspace deletes the selected track, Hats");
await key("Control+z");
check(await until(() => exists("tracks/hats.ts") && read("tracks/hats.ts") === orig.hats), "and Ctrl+Z brings it back");
await until(async () => same(await shown(), ["kick", "hats", "bass", "keys"]));

// ---------------------------------------------------------------- the dock's views (Alt+1 to Alt+4)

await select("kick");
await key("Alt+3");
check(await until(async () => (await selectedView()) === "mix"), "Alt+3 shows Mix");
// Bass's clip, opened in Notes by a double-click; then Device and back.
const clip = await box('[data-lane="bass.notes"] :nth-child(1 of [data-clip])');
await page.mouse.click(clip.x + 60, clip.y + clip.height / 2, { count: 2 });
await until(() => page.$("[data-testid=roll-grid]"));
await key("Alt+1");
check(await until(async () => (await selectedView()) === "graph"), "Alt+1 shows Device");
await key("Alt+2");
check(await until(async () => (await selectedView()) === "notes" && !!(await page.$("[data-testid=roll-grid]"))), "Alt+2 shows Notes, on Bass's clip");
const roll = await title("[data-testid=roll-grid]");
check(!!roll && roll.includes("Backspace deletes · Ctrl+D duplicates"), `the roll's tooltip says Backspace and Ctrl+D: ${roll?.split(" · ").slice(-4, -2).join(" · ")}`);
await noMac("Notes");
await select("kick");

// ---------------------------------------------------------------- the code editor's own Ctrl chords

await key("Alt+4");
check(await until(async () => (await selectedView()) === "code" && !!(await page.$("[data-testid=code] .monaco-editor .view-lines"))), "Alt+4 shows Code");
await until(async () => (await page.$eval("[data-testid=code] .view-lines", (e) => e.innerText).catch(() => "")).includes('track("kick"'));
const lines = () => page.$eval("[data-testid=code] .view-lines", (e) => e.innerText.replace(/ /g, " "));
await page.click("[data-testid=code] .view-lines");
await key("Control+End");
await page.keyboard.press("Enter");
await page.keyboard.type("// abc abc end");
await key("Control+Backspace");
check(await until(async () => (await lines()).includes("// abc abc ") && !(await lines()).includes("abc end")), "Ctrl+Backspace deletes the word before");
await key("Control+z");
check(await until(async () => (await lines()).includes("// abc abc end")), "Ctrl+Z undoes it, in the editor");
// "// abc abc", the last abc selected: Ctrl+D adds the first.
await key("Control+Backspace");
await page.keyboard.press("Backspace");
await key("Control+Shift+ArrowLeft");
await key("Control+d");
await page.keyboard.type("xyz");
check(await until(async () => (await lines()).includes("// xyz xyz")), "Ctrl+D adds the next match: typing changes both");
await key("Control+s");
check(await until(() => read("tracks/kick.ts").includes("// xyz xyz")), "Ctrl+S saves the file");
await noMac("Code");

done(errors);

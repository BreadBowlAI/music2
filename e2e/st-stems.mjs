// Stems (studio lane C, Q39, Q40): each track of the song as its own WAV file, in a new folder in
// the songs folder's Exports, named "1 Kick.wav"… in the tracks' order, each the same file `m2
// bounce <song> x --track <key>` writes, byte for byte; a track muted in the song is left out; a
// song that plays plugins is refused in words; one export at a time.
// The Export dialog (Q42): ⇧⌘E opens it with Song chosen and Export… focused, its words, Tab
// round, Esc; Return exports the song as before; Stems from it, with "Exporting stems: Hats (2 of
// 4)" and the done toast; MIDI from it writes first-song.mid; Stems greyed with why for a song that
// plays plugins; a PC page's words; the taste lint.
// node st-stems.mjs <root> <home> <project> <m2 cli>
import fs from "node:fs";
import { execFileSync } from "node:child_process";
import { check, done, open, until as untilFix } from "./fixlib.mjs";
import { lint } from "./lintlib.mjs";

const [S, H, P, M2] = process.argv.slice(2);
const sess = JSON.parse(fs.readFileSync(`${H}/.music2/session.json`, "utf8"));
const HDR = { "x-m2-token": sess.page_token };
const api = async (path, body) => {
  const r = await fetch(`${sess.url}/api/${path}`, body === undefined ? { headers: HDR } : { method: "POST", headers: { ...HDR, "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: r.status, json: await r.json().catch(() => null) };
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const until = (f, ms = 30000) => untilFix(f, ms);
const exportsDir = `${H}/Music/music2/Exports`;
const session = async () => (await api("graph")).json.session;
const revision = async () => (await api("graph")).json.view?.revision ?? (await api("graph")).json.revision;

/** A WAV file's `fmt ` and `data` chunks, as bytes. */
function chunks(file) {
  const b = fs.readFileSync(file);
  const out = {};
  for (let off = 12; off + 8 <= b.length; ) {
    const id = b.toString("latin1", off, off + 4), size = b.readUInt32LE(off + 4);
    out[id] = b.subarray(off + 8, off + 8 + size);
    off += 8 + size + (size & 1);
  }
  return out;
}

/** The finished stems in `dir`: once its files stop changing and no part file is left. */
async function finished(dir, count) {
  let last = "";
  return until(() => {
    if (!fs.existsSync(dir)) return false;
    const files = fs.readdirSync(dir);
    const now = files.map((f) => `${f}:${fs.statSync(`${dir}/${f}`).size}`).join();
    const done = files.filter((f) => f.endsWith(".wav") && !f.startsWith(".")).length === count && !files.some((f) => f.endsWith(".part")) && now === last;
    last = now;
    return done;
  });
}

// ---------------------------------------------------------------- the song's stems
const token = await session();
const ok = await api("graph/stems/check", { session: token });
check(ok.status === 200 && ok.json.why === null, `first-song's stems can be made: ${JSON.stringify(ok.json)}`);
const started = await api("graph/stems", { session: token });
const folder = `${exportsDir}/first-song stems`;
check(started.status === 200 && started.json.path === folder, `they go in a new folder in Exports: ${JSON.stringify(started.json)}`);
// While they render, a second export of either kind is refused.
const again = await api("graph/export", {});
check(again.status === 400 && /already being exported/.test(again.json?.error ?? ""), `an export while stems render is refused: ${JSON.stringify(again.json)}`);
check(await finished(folder, 4), "four stems are written");
const files = fs.readdirSync(folder).sort();
check(files.join(" | ") === "1 Kick.wav | 2 Hats.wav | 3 Bass.wav | 4 Keys.wav", `named by number and track, in the tracks' order: ${files.join(" | ")}`);
// Each is `m2 bounce`'s file of its track, on a copy of the song.
const keys = { 1: "kick", 2: "hats", 3: "bass", 4: "keys" };
const named = { 1: "Kick", 2: "Hats", 3: "Bass", 4: "Keys" };
const copyOf = `${S}/bounce-song`;
fs.cpSync(P, copyOf, { recursive: true, filter: (f) => !f.includes("/.music2") });
for (const n of [1, 2, 3, 4]) {
  const stem = `${folder}/${n} ${named[n]}.wav`;
  check(fs.existsSync(stem), `${n} ${named[n]}.wav is there`);
  execFileSync(M2, ["bounce", copyOf, `stem-${keys[n]}`, "--track", keys[n]], { env: { ...process.env, M2_AUDIO: "none", M2_MIDI: "none" }, stdio: "pipe" });
  const bounced = `${copyOf}/assets/stem-${keys[n]}.wav`;
  const [a, b] = [chunks(stem), chunks(bounced)];
  check(a["fmt "].equals(b["fmt "]) && a.data.equals(b.data) && a.data.length > 1000, `${named[n]}'s stem is m2 bounce --track ${keys[n]}'s audio, bit for bit (${a.data.length} bytes of 32-bit float)`);
  check(fs.readFileSync(stem).equals(fs.readFileSync(bounced)), `and the same file, byte for byte`);
}

// ---------------------------------------------------------------- a track muted in the song is left out
const hats = fs.readFileSync(`${P}/tracks/hats.ts`, "utf8");
const before = await revision();
fs.writeFileSync(`${P}/tracks/hats.ts`, hats.replace('{ name: "Hats" }', '{ name: "Hats", muted: true }'));
check(await until(async () => (await revision()) !== before, 10000), "Hats muted in the song (written to its file)");
const second = await api("graph/stems", { session: await session() });
check(second.json?.path === `${exportsDir}/first-song stems 2`, `a second export takes a folder of its own: ${JSON.stringify(second.json)}`);
check(await finished(`${exportsDir}/first-song stems 2`, 3), "three stems");
check(fs.readdirSync(`${exportsDir}/first-song stems 2`).sort().join(" | ") === "1 Kick.wav | 2 Bass.wav | 3 Keys.wav", `Hats is left out, the rest numbered on: ${fs.readdirSync(`${exportsDir}/first-song stems 2`).sort().join(" | ")}`);
fs.writeFileSync(`${P}/tracks/hats.ts`, hats);

// ---------------------------------------------------------------- a song that plays plugins
const keysTs = fs.readFileSync(`${P}/tracks/keys.ts`, "utf8");
const before2 = await revision();
fs.writeFileSync(
  `${P}/tracks/keys.ts`,
  'import { plugin, track } from "@m2/core";\n\nexport default track("keys", { name: "Keys" }, ({ lane }) => plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("chords") }));\n',
);
check(await until(async () => (await revision()) !== before2, 10000), "Keys plays a plugin");
const why = await api("graph/stems/check", { session: await session() });
check(/^Not yet for a song that plays plugins \(.+\)\.$/.test(why.json?.why ?? ""), `the check says why in words: ${JSON.stringify(why.json)}`);
const refused = await api("graph/stems", { session: await session() });
check(refused.status === 400 && refused.json?.error === why.json?.why, `and the export is refused in those words: ${JSON.stringify(refused.json)}`);
check(!fs.existsSync(`${exportsDir}/first-song stems 3`), "with no folder made");
fs.writeFileSync(`${P}/tracks/keys.ts`, keysTs);
await until(async () => (await api("graph/stems/check", { session: await session() })).json?.why === null, 10000);

// ---------------------------------------------------------------- the Export dialog
const { browser, page, errors, toasts, key } = await open(1440, 900);
const dialog = "[data-testid=export-dialog]";
const opened = async () => !!(await page.$(dialog));
const focused = () => page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.getAttribute("value") ?? document.activeElement?.tagName);
await key("Meta+Shift+e");
check(await until(opened, 3000), "⇧⌘E opens the Export dialog");
const shown = await page.$eval(dialog, (d) => ({
  title: d.querySelector("h2").textContent,
  rows: [...d.querySelectorAll("[data-export-choice]")].map((r) => r.innerText.replace(/\s+/g, " ").trim()),
  checked: d.querySelector("input[type=radio]:checked")?.value,
  buttons: [...d.querySelectorAll("button")].map((b) => b.textContent.trim()),
}));
check(shown.title === "Export", `titled Export: ${shown.title}`);
check(
  shown.rows.join(" | ") === "Song The whole song as one WAV file | Stems Each track as its own WAV file, before its volume and pan | MIDI The notes, for other music apps",
  `three rows in plain words: ${shown.rows.join(" | ")}`,
);
check(shown.checked === "song" && shown.buttons.join(",") === "Cancel,Export…", `Song is chosen, then [Cancel] [Export…]: ${JSON.stringify(shown)}`);
check(await until(async () => (await focused()) === "export-dialog-go", 2000), `Export… has the focus: ${await focused()}`);
const bad = await page.evaluate(lint, dialog);
check(bad.length === 0, `the dialog passes the taste lint: ${bad.slice(0, 3).join("; ")}`);
await sleep(250);
await page.screenshot({ path: `${S}/st-stems-dialog.png` });
// Tab goes round its five controls and never behind it.
const round = [];
for (let i = 0; i < 5; i++) {
  await page.keyboard.press("Tab");
  round.push(await focused());
}
check(round.join(",") === "song,stems,midi,export-dialog-cancel,export-dialog-go", `Tab goes round: ${round.join(",")}`);
await page.keyboard.press("Escape");
check(await until(async () => !(await opened()), 2000), "Esc closes it");

// Return exports the song, as Export did.
await key("Meta+Shift+e");
await page.waitForSelector(dialog);
await page.keyboard.press("Enter");
check(await until(async () => (await toasts()).some((t) => t.startsWith("Exported first-song.wav")), 30000), `Return exports the song as before: ${(await toasts()).join(" | ")}`);
check(fs.existsSync(`${exportsDir}/first-song.wav`), "first-song.wav is in Exports");
await until(async () => !(await page.$("[data-testid=export-progress]")), 10000);

// Stems from the dialog: the button says which stem, then the toast says where.
await page.evaluate(() => {
  window.__titles = [];
  new MutationObserver(() => {
    const t = document.querySelector("[data-testid=export]")?.title;
    if (t && window.__titles.at(-1) !== t) window.__titles.push(t);
  }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["title"] });
});
await page.click("[data-testid=export]");
await page.waitForSelector(dialog);
await page.click('[data-export-choice="stems"]');
check(await page.$eval('input[value="stems"]', (e) => e.checked), "a click on Stems chooses it");
await page.keyboard.press("Enter");
const stemsToast = "Exported 4 stems to first-song stems 3, in ~/Music/music2/Exports.";
check(await until(async () => (await toasts()).includes(stemsToast), 60000), `the toast says how many and where: ${(await toasts()).join(" | ")}`);
const titles = await page.evaluate(() => window.__titles);
check(titles.some((t) => /^Exporting stems: (Kick|Hats|Bass|Keys) \([1-4] of 4\)$/.test(t)), `while they render, the button says which: ${titles.filter((t) => t.startsWith("Exporting")).slice(0, 4).join(" | ")}`);
check(fs.readdirSync(`${exportsDir}/first-song stems 3`).sort().join(" | ") === "1 Kick.wav | 2 Hats.wav | 3 Bass.wav | 4 Keys.wav", "the four stems are written");

// MIDI from the dialog: the song's notes, straight to Exports in a browser.
await key("Meta+Shift+e");
await page.waitForSelector(dialog);
await page.click('[data-export-choice="midi"]');
await page.keyboard.press("Enter");
const midiToast = "Saved the song as a MIDI file: first-song.mid, in ~/Music/music2/Exports.";
check(await until(async () => (await toasts()).includes(midiToast), 10000), `MIDI saves the song's notes: ${(await toasts()).join(" | ")}`);
const mid = fs.existsSync(`${exportsDir}/first-song.mid`) ? fs.readFileSync(`${exportsDir}/first-song.mid`) : Buffer.alloc(0);
check(mid.toString("latin1", 0, 4) === "MThd" && mid.readUInt16BE(10) === 5, `first-song.mid: a tempo track and one per notes lane (${mid.length ? mid.readUInt16BE(10) : 0} tracks)`);
check(!(await opened()), "and the dialog is gone");

// A song that plays plugins: Stems greyed, with why; the rest still go.
const before3 = await revision();
fs.writeFileSync(
  `${P}/tracks/keys.ts`,
  'import { plugin, track } from "@m2/core";\n\nexport default track("keys", { name: "Keys" }, ({ lane }) => plugin("clap:org.surge-synth-team.surge-xt", { notes: lane.notes("chords") }));\n',
);
await until(async () => (await revision()) !== before3, 10000);
await key("Meta+Shift+e");
await page.waitForSelector(dialog);
check(await until(async () => !!(await page.$("[data-testid=export-stems-why]")), 3000), "Stems is greyed for a song that plays plugins");
const greyed = await page.$eval('[data-export-choice="stems"]', (r) => ({ off: r.querySelector("input").disabled, why: r.querySelector("[data-testid=export-stems-why]")?.textContent }));
check(greyed.off && greyed.why === "Not yet for a song that plays plugins (org.surge-synth-team.surge-xt).", `with Q40's words: ${JSON.stringify(greyed)}`);
check(await page.$eval("[data-testid=export-dialog-go]", (b) => !b.disabled), "Export… still exports the song");
const badOff = await page.evaluate(lint, dialog);
check(badOff.length === 0, `the greyed dialog passes the lint: ${badOff.slice(0, 3).join("; ")}`);
await sleep(250);
await page.screenshot({ path: `${S}/st-stems-dialog-plugins.png` });
await page.keyboard.press("Escape");
fs.writeFileSync(`${P}/tracks/keys.ts`, keysTs);
await browser.close();

// A PC page: Ctrl+Shift+E.
const pc = await open(1440, 900, { pc: true });
const tip = await pc.page.$eval("[data-testid=export]", (e) => e.title);
check(tip === "Export the song, its stems or its notes (Ctrl+Shift+E)", `a PC page's tooltip: ${tip}`);
await pc.key("Control+Shift+e");
check(await until(async () => !!(await pc.page.$(dialog)), 3000), "Ctrl+Shift+E opens the dialog there");
await pc.browser.close();

done([...errors, ...pc.errors]);

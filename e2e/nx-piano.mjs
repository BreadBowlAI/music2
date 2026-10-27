// The keyboard piano (the next track's B2; Q7, Q8, Q9; X13, X15, X16, X31, X32): ⌘K, its toggle in the
// Notes tools, and its card at the timeline's lower right, which says what plays. A to ' and W to P play
// the selected track's notes from C4 (the page's WebSocket frames, as s8-audition reads them, and the
// server's `live`); Z and X move the octave; the shortcuts on those keys wait while it is on, the rest
// keep theirs; typing in a field or the code editor plays nothing; a held key repeats nothing; the
// window's blur and ⌘ let go; no track, nothing plays; a dialog takes the keys; off again, A shows
// automation again. The ⌥K sheet's rows, a PC page's Ctrl+K, and the lint of the card.
//
//   node e2e/nx-piano.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { lint } from "./lintlib.mjs";
import { ROOT, check, done, open, sleep, until } from "./fixlib.mjs";

const { page, errors, api, state, box, key } = await open(1440, 900);

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
const ons = (msgs) => msgs.flatMap((m) => (m.on ?? []).map((o) => ({ ...o, module: m.module, lane: m.lane })));
const offs = (msgs) => msgs.flatMap((m) => m.off ?? []);
const toggle = () => page.$eval("[data-testid=piano-toggle]", (e) => ({ on: e.getAttribute("aria-pressed") === "true", title: e.title })).catch(() => null);
const card = (p = page) =>
  p
    .$eval("[data-testid=keyboard-piano]", (e) => ({
      plays: e.querySelector("[data-testid=piano-plays]")?.textContent,
      octave: e.querySelector("[data-testid=piano-octave]")?.textContent,
      held: [...e.querySelectorAll("[data-held]")].map((k) => k.dataset.pianoKey),
      off: e.querySelector('button[aria-label="Keyboard piano off"]')?.title,
    }))
    .catch(() => null);
const cutoff = async () => !!(await page.$('[data-lane="bass.cutoff"]'));
const blur = () => page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
/** Press and let go of `k`, the page's key, with nothing focused. */
const tap = async (k) => {
  await blur();
  await page.keyboard.press(k);
};
/** The Ons `f` sends, after it ends and a beat for the frames to arrive. */
const sends = async (f) => {
  const from = sent.length;
  await f();
  await sleep(250);
  return sent.slice(from);
};

// ---------------------------------------------------------------- off at first

// Bass's clip in Notes: the toggle sits in Notes' tools.
const clip = await page.$('[data-lane="bass.notes"] :nth-child(1 of [data-clip])').then((e) => e?.boundingBox());
await page.mouse.click(clip.x + Math.min(40, clip.width / 2), clip.y + clip.height / 2, { count: 2 });
await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length > 0, 5000);
const t0 = await toggle();
check(!!t0 && !t0.on && !(await card()), `off at first: the toggle in Notes' tools is off and there is no card (${JSON.stringify(t0)})`);
check(t0?.title === "Keyboard piano: play notes with your letter keys (⌘K)", `its tooltip: ${t0?.title}`);
const lanes0 = await cutoff();
await tap("a");
const lanes1 = await until(async () => (await cutoff()) !== lanes0, 2000);
await tap("a");
await until(async () => (await cutoff()) === lanes0, 2000);
check(lanes1, "off, A shows Bass's automation as before");

// ---------------------------------------------------------------- ⌘K, the card

await key("Meta+k");
check(await until(async () => (await toggle())?.on === true && !!(await card()), 2000), "⌘K turns it on: the toggle is lit and the card shows");
const c1 = await card();
check(c1?.plays === "Plays Bass" && c1.octave === "ZC4–F5X" && c1.off === "Keyboard piano off (⌘K)", `the card says what plays and the octave: ${JSON.stringify(c1)}`);
// After its 140 ms rise.
await sleep(300);
const corner = await page.evaluate(() => {
  const c = document.querySelector("[data-testid=keyboard-piano]").getBoundingClientRect();
  const t = document.querySelector("[data-testid=timeline]").getBoundingClientRect();
  return { right: t.right - c.right, bottom: t.bottom - c.bottom };
});
check(Math.abs(corner.right - 16) < 1 && Math.abs(corner.bottom - 16) < 1, `at the timeline's lower right, 16 px in (${JSON.stringify(corner)})`);

// ---------------------------------------------------------------- the notes

await blur();
let from = sent.length;
await page.keyboard.down("a");
check(await liveIs(1), "A sounds a note (live 1)");
const aOn = ons(sent.slice(from));
check(aOn.length === 1 && aOn[0].pitch === 60 && aOn[0].velocity === 0.8 && aOn[0].module === "bass" && aOn[0].lane === "notes", `A is C4 (60) at 0.8 on bass.notes (${JSON.stringify(aOn)})`);
const lit = await page.$eval('[data-piano-key="KeyA"]', (e) => ({ held: e.hasAttribute("data-held"), bg: getComputedStyle(e).backgroundColor }));
check(lit.held && lit.bg !== "rgb(245, 247, 249)", `its cap lights in Bass's colour while held (${JSON.stringify(lit)})`);
await page.keyboard.up("a");
check((await liveIs(0)) && offs(sent.slice(from)).length === 1, "A up: one Off, nothing sounds");
const pitches = async (keys) => {
  const m = await sends(async () => {
    for (const k of keys) await page.keyboard.press(k);
  });
  return ons(m).map((o) => o.pitch);
};
await blur();
check((await pitches(["w"])).join() === "61" && (await pitches(["k"])).join() === "72", "W is 61 and K is 72");
check((await state()).local?.metronome !== true, "K played, and left the metronome off");
check((await pitches(["x", "a"])).join() === "72" && (await card())?.octave === "ZC5–F6X", "X, then A: 72, and the card says C5–F6");
check((await pitches(["z", "z", "a"])).join() === "48" && (await card())?.octave === "ZC3–F4X", "Z Z, then A: 48");
await pitches(["x"]);

// ---------------------------------------------------------------- the keys it takes, and those it leaves

const solo0 = JSON.stringify((await state()).overlays?.solo ?? []);
const zoom0 = (await state()).local?.zoom;
const dock = () => page.$eval("[data-testid=dock] button[aria-expanded]", (e) => e.getAttribute("aria-expanded"));
const dock0 = await dock();
await tap("s");
await tap("e");
await tap("z");
await tap("a");
await sleep(700);
check(JSON.stringify((await state()).overlays?.solo ?? []) === solo0, "while on, S solos nothing");
check((await dock()) === dock0, "E leaves the dock as it was");
check((await state()).local?.zoom === zoom0, "Z doesn't zoom the timeline");
check((await cutoff()) === lanes0, "A doesn't show automation");
await tap("x");
await tap("c");
check(await until(async () => (await api("/status")).body.loop?.on === true, 2000), "C still loops");
await tap("c");
await tap("m");
check(await until(async () => ((await state()).overlays?.mute ?? []).includes("bass"), 2000), "M still mutes Bass");
await tap("m");
await tap("Space");
check(await until(async () => (await api("/status")).body.playing === true, 2000), "Space still plays");
await tap("Space");
await until(async () => (await api("/status")).body.playing === false, 2000);

// ---------------------------------------------------------------- typing, dialogs, repeats, letting go

const name = await box("[data-testid=bar-song-name]");
await page.mouse.click(name.x + name.width / 2, name.y + name.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
const typed = await sends(() => page.keyboard.type("asdf"));
const value = await page.$eval("[data-testid=song-title-input]", (e) => e.value).catch(() => "");
check(value.endsWith("asdf") && ons(typed).length === 0, `"asdf" typed into the song's name types and plays nothing (${value})`);
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=song-title-input]")));

// The code editor gets its letters.
await key("Alt+4");
await page.waitForSelector(".monaco-editor .view-lines", { timeout: 20000 }).catch(() => null);
const lines = await box(".monaco-editor .view-lines");
check(!!lines, `the code view shows its editor (${JSON.stringify(lines)})`);
// An element as the messages name it, and whether it is in the code editor.
const named = (e) => `${e?.tagName.toLowerCase()}${e?.getAttribute("class")?.trim() ? `.${e.getAttribute("class").trim().split(/\s+/).join(".")}` : ""}${e?.closest(".monaco-editor") ? " in the editor" : ""}`;
const at = lines && { x: Math.round(lines.x + 40), y: Math.round(lines.y + 6) };
const hit = at ? await page.evaluate(`(${named})(document.elementFromPoint(${at.x}, ${at.y}))`) : null;
if (at) await page.mouse.click(at.x, at.y);
// The keys follow the click with nothing between, as before: where the focus is, is read after them.
const inCode = await sends(() => page.keyboard.type("asdf"));
const after = await page.evaluate(`(${named})(document.activeElement)`);
const code = await page.$eval(".monaco-editor .view-lines", (e) => e.textContent).catch(() => "");
check(
  code.includes("asdf"),
  `the code editor gets its letters (the click at ${JSON.stringify(at)} hit ${hit}; after the keys the focus was in ${after}; it shows ${JSON.stringify(code.slice(0, 60))}…)`,
);
check(ons(inCode).length === 0, `and nothing plays while typing in it (${JSON.stringify(ons(inCode))})`);
for (let i = 0; i < 4; i++) await key("Meta+z");
await blur();
await key("Alt+2");
await until(async () => (await page.$$("[data-testid=notes] [data-note]")).length > 0, 5000);

// A dialog takes the keys: the ⌥K sheet, open.
await blur();
await key("Alt+k");
await page.waitForSelector("[data-testid=shortcuts]", { timeout: 2000 }).catch(() => null);
const sheet = await page.$$eval("[data-testid=shortcuts] section", (ss) => ss.map((s) => [s.querySelector("h3")?.textContent, [...s.querySelectorAll("dl > div")].map((r) => `${r.querySelector("dt")?.textContent}: ${r.querySelector("dd")?.textContent}`)]));
// D: a note key with no shortcut of the page's, so nothing changes behind the sheet.
const inSheet = await sends(() => page.keyboard.press("d"));
check(ons(inSheet).length === 0, "with a dialog open (the ⌥K sheet), D plays nothing");
const view = sheet.find(([n]) => n === "View")?.[1] ?? [];
const group = sheet.at(-1);
check(
  view.includes("Keyboard piano on / off: ⌘K") && group?.[0] === "Keyboard piano" && group[1].join(" | ") === "Play notes: A to ' and W to P | Octave down / up: Z / X",
  `the ⌥K sheet lists it in View and in a last group of its own (${JSON.stringify(group)})`,
);
await page.keyboard.press("Escape");
await sleep(200);

// One On for a key held with repeats.
await blur();
from = sent.length;
await page.keyboard.down("a");
await page.keyboard.down("a");
await page.keyboard.down("a");
await sleep(200);
check(ons(sent.slice(from)).length === 1 && (await live()) === 1, `a key held with repeats sends one On (${ons(sent.slice(from)).length})`);
await page.evaluate(() => window.dispatchEvent(new Event("blur")));
check(await liveIs(0), "the window's blur lets go of it (live 0)");
await page.keyboard.up("a");
// ⌘ held while a letter comes up: a Mac sends that letter no key up.
await page.keyboard.down("a");
await liveIs(1);
await page.keyboard.down("Meta");
await page.keyboard.up("a");
await page.keyboard.up("Meta");
check(await liveIs(0), "A down, ⌘ down, A up, ⌘ up: nothing sounds");

// ---------------------------------------------------------------- what plays

await page.click('[data-module-tab="keys"] [data-testid=track-label]');
check(await until(async () => (await card())?.plays === "Plays Keys", 2000), "Keys selected: the card says it plays Keys");
const keysOn = ons(await sends(() => tap("a")));
check(keysOn.length === 1 && keysOn[0].module === "keys" && keysOn[0].lane === "chords", `A plays Keys' notes lane (${JSON.stringify(keysOn)})`);
await page.click('[data-module-tab="song"] [data-testid=track-label]');
check(await until(async () => (await card())?.plays === "Select a track with notes to play it", 2000), "the Song row: the card asks for a track with notes");
const none = await sends(() => tap("a"));
check(ons(none).length === 0, "and A plays nothing");

const bad = await page.evaluate(lint, "[data-testid=keyboard-piano]");
check(bad.length === 0, `the lint finds nothing in the card${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
await page.click('[data-module-tab="bass"] [data-testid=track-label]');
await blur();
await page.keyboard.down("d");
await sleep(150);
await page.screenshot({ path: `${ROOT}/nx-piano-card.png` });
await page.keyboard.up("d");

// ---------------------------------------------------------------- off again

// ⌘K from a text field: the piano's, not the field's.
await page.mouse.click(name.x + name.width / 2, name.y + name.height / 2, { count: 2 });
await page.waitForSelector("[data-testid=song-title-input]", { timeout: 2000 }).catch(() => null);
await key("Meta+k");
check(await until(async () => !(await card()) && (await toggle())?.on === false, 2000), "⌘K while typing in a field turns it off");
await page.keyboard.press("Escape");
await until(async () => !(await page.$("[data-testid=song-title-input]")));
await tap("a");
check(await until(async () => (await cutoff()) !== lanes0, 2000), "off again, A shows Bass's automation");
await tap("a");
await page.click("[data-testid=piano-toggle]");
check(await until(async () => (await toggle())?.on === true && !!(await card()), 2000), "its toggle in Notes turns it on");
await page.click('[data-testid=keyboard-piano] button[aria-label="Keyboard piano off"]');
check(await until(async () => !(await card()), 2000), "the card's × turns it off");

// ---------------------------------------------------------------- a PC page

const pc = await open(1440, 900, { pc: true });
await pc.key("Control+k");
const pcCard = await until(async () => !!(await card(pc.page)), 2000).then(() => card(pc.page));
check(pcCard?.off === "Keyboard piano off (Ctrl+K)" && pcCard.plays?.startsWith("Plays "), `a PC page: Ctrl+K shows the card, whose × says Ctrl+K (${JSON.stringify(pcCard)})`);
await pc.browser.close();
errors.push(...pc.errors);

done(errors);

// The Playground (VER-6): a view of one notes clip's versions, beside the roll.
//   - Open Playground in a notes clip's menu (⌥P) opens it on that clip, with versions or without:
//     the loop becomes the clip's bars, on, and nothing plays; a clip with none lists `original`
//     alone, with a line saying how versions get there; no edit, no undo step;
//   - the focus record names its clip and beats while it is open (AGENT-6);
//   - an agent's `add` shows in the list as it lands, with who added it; the keyboard's focus and
//     what plays stay as they were;
//   - ← and → (and a click) play the one before and after: the server's clip has its notes (VER-3);
//   - S and a row's star star; Duplicate copies what plays and plays the copy, one undo step;
//     Rename and Remove on a row;
//   - the clip on the timeline shows how many versions wait;
//   - Keep this one removes the others and closes it; Esc and its button close it too; closing
//     puts the loop back as it was;
//   - each state linted, every icon button with a tooltip.
//
//   node e2e/playground.mjs <root> <home> <project>     (the `fix` kind: examples/first-song open)
import { lint } from "./lintlib.mjs";
import { tips } from "./tiplib.mjs";
import { ROOT, check, done, open, sess, sleep, until } from "./fixlib.mjs";

const { page, errors, key, state, api } = await open(1440, 900);
const CLIP = "db9f9t5s";
const panel = "[data-testid=playground]";
const row = (name) => `${panel} [data-version="${name}"]`;

/** The bass clip as the server has it. */
const clip = async () => (await state()).timeline.tracks.find((t) => t.module === "bass").lanes.find((l) => l.key === "notes").clips.find((c) => c.id === CLIP);
/** Its versions in words: `name`, `*` when starred, `by who`, and `plays` or how many notes wait. */
const versions = async () => ((await clip()).versions ?? []).map((v) => `${v.name}${v.star ? "*" : ""}${v.by ? ` by ${v.by}` : ""} ${v.notes ? v.notes.length : "plays"}`).join(" | ");
/** The clip's notes that play: `pitch@start`. */
const plays = async () => (await clip()).notes.map((n) => `${n.pitch}@${n.start}`).join(" ");
/** The list as the panel shows it. */
const rows = () =>
  page.$$eval(`${panel} [data-version]`, (els) =>
    els.map((e) => `${e.dataset.version}${e.dataset.star ? "*" : ""}${e.querySelector("[data-by]") ? ` by ${e.querySelector("[data-by]").textContent}` : ""}${e.dataset.plays ? " plays" : ""}`).join(" | "),
  );
const loop = async () => {
  const l = (await api("/status")).body.loop;
  return l ? `${l.start}-${l.end} ${l.on ? "on" : "off"}` : "none";
};
const playing = async () => (await api("/status")).body.playing;
/** As an agent, with the tools key and its name. */
const agent = async (path, body) => {
  const r = await fetch(`${sess.url}${path}`, { method: body ? "POST" : "GET", headers: { "x-m2-token": sess.token, "x-m2-as": "Claude", "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined });
  return { status: r.status, body: await r.json().catch(() => ({})) };
};
const focus = async () => (await agent("/api/focus")).body;
const undoTitle = () => page.$eval('button[aria-label="Undo"]', (e) => e.title);
const revision = async () => (await state()).view.revision;
const linted = async (what) => {
  const bad = await page.evaluate(lint, "[data-testid=dock]");
  check(bad.length === 0, `the lint finds nothing in ${what}${bad.length ? `: ${bad.slice(0, 6).join(" | ")}` : ""}`);
};
/** What has the keyboard: the element's test id, or its tag. */
const focused = () => page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.tagName);
const shot = (name) => page.screenshot({ path: `${ROOT}/playground-${name}.png` });

// ---------------------------------------------------------------- open on a clip (VER-6)

// A loop of the musician's own, off: what closing must put back.
await api("/loop", { method: "POST", body: JSON.stringify({ start: 4, end: 8, on: false }) });
check((await loop()) === "4-8 off", `the musician's loop first: ${await loop()}`);
const original = await plays();
const rev0 = await revision();
const undo0 = await undoTitle();

const box = await (await page.waitForSelector(`[data-lane="bass.notes"] [data-clip="${CLIP}"]`)).boundingBox();
await page.mouse.click(box.x + 40, box.y + box.height / 2, { button: "right" });
await page.waitForSelector("[data-testid=clip-menu]");
const item = '[data-testid=clip-menu] [data-menu-item="playground"]';
check((await page.$eval(item, (e) => e.textContent).catch(() => "")).replace(/\s+/g, " ").includes("Open Playground"), "VER-6: a notes clip's menu has Open Playground");
check((await page.$eval(item, (e) => e.textContent).catch(() => "")).includes("⌥P"), "VER-6: with its key, ⌥P");
await page.click(item).catch(() => {});
check(await until(() => page.$(panel), 5000), "VER-6: it opens the Playground");
check(!!(await page.$("[data-testid=roll-grid]")) && (await page.$eval('[data-view-tab="notes"]', (e) => e.getAttribute("aria-selected"))) === "true", "VER-6: beside the roll, in Notes");
const pb = await (await page.$(panel))?.boundingBox();
const rb = await (await page.$("[data-testid=roll-scroller]"))?.boundingBox();
check(!!pb && !!rb && pb.x + pb.width <= rb.x + 0.5 && rb.width > 600, `VER-6: the list at the roll's left, the roll in sight (${pb?.width} px, then ${rb?.width} px)`);
check(await until(async () => (await loop()) === "0-32 on", 3000), `VER-6: the loop is the clip's bars, on: ${await loop()}`);
check(!(await playing()), "VER-6: opening doesn't press play");

// A clip with no versions: `original` alone, and how versions get there.
check((await rows()) === "original plays", `VER-6: a clip with no versions lists original alone: ${await rows()}`);
const says = await page.$eval(`${panel} [data-testid=playground-empty]`, (e) => e.textContent).catch(() => "");
check(/agent/i.test(says) && /Duplicate/.test(says) && !/Claude|AI\b/.test(says), `VER-6: a line says an agent or Duplicate adds versions, naming no product (AGENT-9): ${says}`);
check(await page.$eval("[data-testid=playground-keep]", (e) => e.disabled).catch(() => false), "VER-6: nothing to keep yet: Keep this one is off");
check((await revision()) === rev0 && (await undoTitle()) === undo0 && (await versions()) === "", "VER-6: opening is no edit and no undo step");
await linted("the Playground on a clip with no versions");
const noTips = await page.evaluate(tips, panel);
check(noTips.length === 0, `every control of the panel has a tooltip${noTips.length ? `: ${noTips.join(" | ")}` : ""}`);
await shot("empty");

// ---------------------------------------------------------------- the focus record (AGENT-6)

check(
  await until(async () => {
    const p = (await focus()).playground;
    return !!p && `${p.track} ${p.lane} ${p.id} ${p.from} ${p.to}` === `bass notes ${CLIP} 0 32`;
  }, 3000),
  `AGENT-6: the focus record names the Playground's clip and beats: ${JSON.stringify((await focus()).playground)}`,
);

// ---------------------------------------------------------------- an agent adds versions (VER-6)

await page.click("[data-testid=roll-grid]", { offset: { x: 300, y: 4 } });
const had = await focused();
const busy = [{ pitch: 45, start: 0, length: 0.25, velocity: 0.8 }, { pitch: 33, start: 4, length: 0.25, velocity: 0.8 }];
const sparse = [{ pitch: 33, start: 0, length: 4, velocity: 0.8 }];
const session = (await state()).session;
const added = await agent("/api/graph/clips", {
  session,
  gesture: { op: "versions", clip: { module: "bass", lane: "notes", id: CLIP }, act: "add", versions: [{ name: "busy", notes: busy }, { name: "sparse", notes: sparse }] },
});
check(added.status === 200, `an agent adds two versions: ${added.status} ${JSON.stringify(added.body).slice(0, 120)}`);
check(await until(async () => (await rows()) === "original plays | busy by Claude | sparse by Claude", 5000), `VER-6: the list grows as they land, each with who added it: ${await rows()}`);
check((await focused()) === had, `VER-6: the keyboard's focus stays where it was (${had}): ${await focused()}`);
check((await plays()) === original && (await versions()).startsWith("original plays"), "VER-6: what plays doesn't change");
check(!(await page.$(`${panel} [data-testid=playground-empty]`)), "the empty line is gone");
check(!(await page.$eval("[data-testid=playground-keep]", (e) => e.disabled)), "Keep this one is on");

// The versions that wait show faintly behind the notes, and take no click.
const ghosts = await page.$$eval("[data-testid=roll-grid] [data-waiting]", (els) => els.map((e) => `${e.dataset.waiting}:${e.dataset.pitch}@${e.dataset.start} ${getComputedStyle(e).pointerEvents}`));
check(ghosts.includes("sparse:33@0 none") && ghosts.includes("busy:45@0 none") && ghosts.length === 3, `VER-6: the waiting versions' notes are drawn behind, and take no click: ${ghosts.join(", ")}`);

// ---------------------------------------------------------------- the timeline's mark (VER-6)

const mark = `[data-lane="bass.notes"] [data-clip="${CLIP}"] [data-versions]`;
check(await until(async () => (await page.$eval(mark, (e) => e.textContent).catch(() => "")) === "2", 3000), `VER-6: the clip on the timeline shows how many versions wait: ${await page.$eval(mark, (e) => e.textContent).catch(() => "none")}`);
check(/2 versions wait/.test(await page.$eval(mark, (e) => e.title).catch(() => "")), "its tooltip says so in words");

// ---------------------------------------------------------------- ← and → flip (VER-3 play)

const rev1 = await revision();
await key("ArrowRight");
check(await until(async () => (await plays()) === "33@4 45@0" || (await plays()) === "45@0 33@4", 5000), `VER-3: → plays the next one, busy: the server's clip has its notes: ${await plays()}`);
check((await versions()) === "original 48 | busy by Claude plays | sparse by Claude 1", `VER-3: and the notes that played wait under their name: ${await versions()}`);
check(await until(async () => (await rows()) === "original | busy by Claude plays | sparse by Claude", 3000), `the list says busy plays: ${await rows()}`);
check((await revision()) === rev1 + 1, "VER-3: one step");
check(await until(async () => /^Undo/.test(await undoTitle()) && /busy/.test(await undoTitle()), 3000), `the step is named: ${await undoTitle()}`);
await key("ArrowRight");
check(await until(async () => (await plays()) === "33@0", 5000), `→ again plays sparse: ${await plays()}`);
await key("ArrowRight");
await sleep(300);
check((await plays()) === "33@0" && (await revision()) === rev1 + 2, "→ on the last one does nothing");
await key("ArrowLeft");
check(await until(async () => (await rows()) === "original | busy by Claude plays | sparse by Claude", 5000), `← plays the one before, busy: ${await rows()}`);
check(!(await playing()), "flipping never presses play");
await shot("open");

// ---------------------------------------------------------------- stars (VER-3 star)

await key("s");
check(await until(async () => (await versions()) === "original 48 | busy* by Claude plays | sparse by Claude 1", 5000), `VER-3: S stars the one that plays: ${await versions()}`);
check(await until(async () => (await rows()) === "original | busy* by Claude plays | sparse by Claude", 3000), `its row shows the star: ${await rows()}`);
await page.click(`${row("sparse")} [data-testid=version-star]`);
check(await until(async () => (await versions()) === "original 48 | busy* by Claude plays | sparse* by Claude 1", 5000), `VER-3: a row's star stars that one, and plays nothing else: ${await versions()}`);
await key("s");
check(await until(async () => (await versions()) === "original 48 | busy by Claude plays | sparse* by Claude 1", 5000), `S again takes the star off: ${await versions()}`);

// ---------------------------------------------------------------- a click plays (VER-3 play)

await page.click(row("original"));
check(await until(async () => (await plays()) === original, 5000), "VER-3: a click on original plays it: the first notes, as they were");

// ---------------------------------------------------------------- Duplicate (VER-3 add with play)

const rev2 = await revision();
await page.click("[data-testid=playground-duplicate]");
check(
  await until(async () => (await versions()) === "original 48 | busy by Claude 2 | sparse* by Claude 1 | original copy plays", 5000),
  `VER-6: Duplicate copies what plays under a new name and plays the copy, by no one: ${await versions()}`,
);
check((await plays()) === original && (await revision()) === rev2 + 1, "VER-3: with the same notes, in one step");
await key("Meta+z");
check(await until(async () => (await versions()) === "original plays | busy by Claude 2 | sparse* by Claude 1", 5000), `VER-3: one undo takes the copy and its playing back: ${await versions()}`);
check(!!(await page.$(panel)) && (await rows()) === "original plays | busy by Claude | sparse* by Claude", `the Playground stays open, and shows it: ${await rows()}`);
await page.click("[data-testid=playground-duplicate]");
check(await until(async () => (await rows()) === "original | busy by Claude | sparse* by Claude | original copy plays", 5000), `Duplicate again: ${await rows()}`);

// ---------------------------------------------------------------- Rename and Remove (VER-3)

await page.click(`${row("original copy")} [data-testid=version-more]`);
await page.waitForSelector("[data-testid=version-menu]");
await page.click('[data-testid=version-menu] [data-menu-item="rename"]');
await page.waitForSelector(`${panel} input[data-testid=version-name]`);
check((await focused()) === "version-name", "Rename turns the name into a field");
await page.keyboard.down("Meta");
await page.keyboard.press("a");
await page.keyboard.up("Meta");
await page.keyboard.type("mine");
await page.keyboard.press("Enter");
check(await until(async () => (await versions()) === "original 48 | busy by Claude 2 | sparse* by Claude 1 | mine plays", 5000), `VER-3: Rename names it: ${await versions()}`);
check(await until(async () => (await rows()) === "original | busy by Claude | sparse* by Claude | mine plays", 3000) && !!(await page.$(panel)), `and the Playground stays open on it: ${await rows()}`);
// A name another version has is refused in the server's words, and nothing changes.
await page.click(`${row("mine")} [data-testid=version-more]`);
await page.click('[data-testid=version-menu] [data-menu-item="rename"]');
await page.waitForSelector(`${panel} input[data-testid=version-name]`);
await page.keyboard.down("Meta");
await page.keyboard.press("a");
await page.keyboard.up("Meta");
await page.keyboard.type("busy");
await page.keyboard.press("Enter");
check(await until(async () => (await page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent).join(" "))).includes("has a version `busy` already"), 5000), "VER-3: a name another version has is refused in words");
check((await versions()).endsWith("mine plays"), "and nothing changed");
// Esc in the field leaves the field, the name and the Playground as they were.
await page.click(`${row("sparse")} [data-testid=version-more]`);
await page.click('[data-testid=version-menu] [data-menu-item="rename"]');
await page.waitForSelector(`${panel} input[data-testid=version-name]`);
await page.keyboard.type("other");
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$(`${panel} input[data-testid=version-name]`)), 3000) && !!(await page.$(panel)), "Esc in the field leaves the field, not the Playground");
await sleep(300);
check((await versions()) === "original 48 | busy by Claude 2 | sparse* by Claude 1 | mine plays", `and the name as it was: ${await versions()}`);

await page.click(`${row("mine")} [data-testid=version-more]`);
check(await page.$eval('[data-testid=version-menu] [data-menu-item="remove"]', (e) => e.disabled), "Remove is off on the one that plays");
await page.keyboard.press("Escape");
check(await until(async () => !(await page.$("[data-testid=version-menu]")), 3000) && !!(await page.$(panel)), "Esc closes the menu, not the Playground");
await page.click(`${row("busy")} [data-testid=version-more]`);
await page.click('[data-testid=version-menu] [data-menu-item="remove"]');
check(await until(async () => (await versions()) === "original 48 | sparse* by Claude 1 | mine plays", 5000), `VER-3: Remove removes one that waits: ${await versions()}`);
check(await until(async () => (await page.$eval(mark, (e) => e.textContent).catch(() => "")) === "2", 3000), "the timeline's mark follows");
await linted("the Playground with versions");
const someTips = await page.evaluate(tips, panel);
check(someTips.length === 0, `every control of the panel has a tooltip${someTips.length ? `: ${someTips.join(" | ")}` : ""}`);

// ---------------------------------------------------------------- Esc closes; the loop comes back (VER-6)

await key("Escape");
check(await until(async () => !(await page.$(panel)), 3000), "VER-6: Esc closes it");
check(await until(async () => (await loop()) === "4-8 off", 3000), `VER-6: and the loop is as it was: ${await loop()}`);
check(await until(async () => (await focus()).playground === null, 3000), `AGENT-6: the focus record's playground is null again: ${JSON.stringify((await focus()).playground)}`);
check(!!(await page.$("[data-testid=roll-grid]")) && (await versions()).endsWith("mine plays"), "the roll stays, on the clip, its versions as they were");
check(!(await page.$("[data-testid=roll-grid] [data-waiting]")), "closed, the roll draws no waiting notes");

// ---------------------------------------------------------------- ⌥P opens it; Keep this one (VER-3 keep)

await key("Alt+p");
check(await until(() => page.$(panel), 3000), "VER-6: ⌥P opens it on the selected clip");
check(await until(async () => (await loop()) === "0-32 on", 3000), `the loop is the clip's again: ${await loop()}`);
await page.click(row("sparse"));
check(await until(async () => (await plays()) === "33@0", 5000), "sparse plays");
const rev3 = await revision();
await page.click("[data-testid=playground-keep]");
check(await until(async () => (await versions()) === "" && (await plays()) === "33@0", 5000), `VER-3: Keep this one keeps what plays and removes the others: ${await versions()} / ${await plays()}`);
check((await revision()) === rev3 + 1, "in one step");
check(await until(async () => !(await page.$(panel)), 3000), "VER-6: and closes the Playground");
check(await until(async () => (await loop()) === "4-8 off", 3000), `the loop is as it was: ${await loop()}`);
check(await until(async () => !(await page.$(mark)), 3000), "the timeline's mark is gone");

// Its close button, and ⌥P again, close it too.
await key("Alt+p");
await page.waitForSelector(panel);
await page.click("[data-testid=playground-close]");
check(await until(async () => !(await page.$(panel)), 3000) && (await until(async () => (await loop()) === "4-8 off", 3000)), "its close button closes it, the loop back");
await key("Alt+p");
await page.waitForSelector(panel);
// Another clip selected: the Playground was this clip's.
await page.click('[data-lane="kick.hits"] [data-clip]', { offset: { x: 30, y: 10 } });
check(await until(async () => !(await page.$(panel)), 3000) && (await until(async () => (await loop()) === "4-8 off", 3000)), "VER-6: selecting another track closes it, the loop back");

done(errors);

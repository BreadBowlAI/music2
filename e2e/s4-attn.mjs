// The "Needs attention" strip in the diagnostic model, on a fresh s4 server with the converted
// first-song:  node s4-attn.mjs <root> <home> <project>
import fs from "node:fs";
import { PROJ, ROOT, check, done, open, read, sleep, until, write } from "./fixlib.mjs";

const { browser, page, errors, state, toasts, status } = await open();
const exists = (p) => fs.existsSync(`${PROJ}/${p}`);
// What needs attention is in the status pill's popover (taste.md ST-1).
const strip = async () => (await status(), page.$("[data-testid=attention]"));
check(!(await strip()), "nothing needs attention at first");

// ID-10: a label changed in code changes no address: the same tab, track and lane, no problem.
write("tracks/kick.ts", read("tracks/kick.ts").replace('{ name: "Kick" }', '{ name: "Big Kick" }'));
check(await until(async () => (await page.$eval('[data-module-tab="kick"] [data-testid=track-label]', (e) => e.textContent).catch(() => "")) === "Big Kick"), "a new label shows on the kick's tab");
check(!!(await page.$('[data-lane="kick.hits"] [data-clip]')) && !(await strip()), "its lane keeps its clips, and nothing needs attention");
const label = await page.$eval('[data-track="kick"] [data-testid=track-label]', (e) => e.textContent);
check(label === "Big Kick", `the timeline shows the label (${label})`);
const sub = await page.$eval('[data-track="kick"]', (e) => e.innerText);
// Keys show only in the code and in Details (taste.md principle 3): no caption beside the name.
check(!sub.includes("· kick") && !/\bkick\b/.test(sub), `and no key beside it (${sub.replace(/\n/g, " | ")})`);

// ID-17: a key renamed by hand leaves the data unclaimed, and Relink makes it play again.
write("tracks/kick.ts", read("tracks/kick.ts").replace('track("kick"', 'track("kicks"'));
check(await until(async () => !!(await strip())), "a renamed key: the strip says something needs attention");
const row = await page.$eval("[data-testid=attention] [data-problem]", (e) => ({ code: e.dataset.problem, severity: e.dataset.severity, src: e.dataset.src, text: e.innerText }));
check(row.code === "data.unclaimed" && row.severity === "warning" && row.src === "data/kick", `data.unclaimed on the track's folder (${JSON.stringify(row).slice(0, 200)})`);
const move = await page.$eval("[data-testid=attention] [data-fix][data-likely]", (e) => ({ code: e.dataset.fix, to: e.dataset.to, text: e.textContent }));
check(move.code === "relink" && move.to === "data/kicks" && move.text.includes("Big Kick (kicks)"), `one click moves it under the new key, named by label and key (${move.text})`);
await page.click("[data-testid=attention] [data-fix][data-likely]");
check(await until(() => exists("data/kicks/hits.json") && !exists("data/kick/hits.json")), "Move renames the folder");
check(await until(async () => !(await strip())), "and nothing needs attention");
check(await until(async () => (await page.$$('[data-lane="kicks.hits"] [data-clip]')).length === 1), "the track's lane shows its clip again");
check(await until(async () => (await toasts()).some((t) => t.includes("Moved data/kick to data/kicks."))), "a toast says what moved");
await page.screenshot({ path: `${ROOT}/s4-attn.png` });
await browser.close();
done(errors);

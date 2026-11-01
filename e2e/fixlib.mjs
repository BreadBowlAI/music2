// Shared setup for the fixes suites: node fixes-*.mjs <root> <home> <project> (as run.mjs passes them for the fix kind).
import puppeteer from "puppeteer-core";
import fs from "node:fs";

export const [ROOT, HOME, PROJ] = process.argv.slice(2);
export const sess = JSON.parse(fs.readFileSync(`${HOME}/.music2/session.json`, "utf8"));
import { exe, pc as pcAgent, real } from "./chrome.mjs";
import { following } from "./caughtup.mjs";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const until = async (f, ms = 8000) => {
  const t = Date.now();
  while (Date.now() - t < ms) {
    if (await f()) return true;
    await sleep(80);
  }
  return false;
};
export const read = (path) => fs.readFileSync(`${PROJ}/${path}`, "utf8");
export const write = (path, text) => fs.writeFileSync(`${PROJ}/${path}`, text);

let pass = 0, fail = 0;
export const check = (ok, what) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${what}`);
  ok ? pass++ : fail++;
};

/**
 * The app at `width`×`height`. The side browser starts hidden: these suites are about the timeline
 * and the dock, and place their clicks in the timeline's width (`browser: true` keeps it). The page
 * is a Mac's, unless `pc`: then a PC's (Linux), whatever the OS the suite runs on (pc-keys).
 *
 * `late` (ms; by default `E2E_LATE_READS`, else none): the page's own reads of the song's state
 * (`GET /api/graph`) are held back that long, as on a page that hears of each change late. The
 * suite's own reads (`api`, `state`) never wait.
 */
export async function open(width = 1440, height = 900, { browser: side = false, pc = false, late = Number(process.env.E2E_LATE_READS ?? 0) } = {}) {
  const browser = await puppeteer.launch({ executablePath: pc ? real : exe, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  if (pc) await page.setUserAgent(pcAgent);
  await page.setViewport({ width, height });
  // The page's requests, intercepted only while some are held back: its own reads (`late`), or
  // those a suite's `hold` names.
  const holders = new Set();
  let intercepting = false;
  const intercept = async (on) => {
    if (on !== intercepting) await page.setRequestInterception((intercepting = on));
  };
  page.on("request", async (r) => {
    if (!intercepting || r.isInterceptResolutionHandled()) return;
    const read = r.method() === "GET" && new URL(r.url()).pathname === "/api/graph" && !r.headers()["x-e2e"];
    const held = [...holders].map((h) => h(r)).find(Boolean);
    if (held && typeof held === "object") {
      await held.until;
      return void (held.answer ? r.respond(held.answer) : r.continue()).catch(() => {});
    }
    const ms = Math.max(read ? late : 0, held || 0);
    if (ms > 0) setTimeout(() => r.continue().catch(() => {}), ms);
    else r.continue().catch(() => {});
  });
  if (late > 0) await intercept(true);
  /**
   * Hold back each request of the page's that `f` names, until `off()`: `f` answers how many ms, or
   * `{ until, answer }` (until that promise settles, then answered with `answer` if given), or 0.
   */
  const hold = async (f) => {
    holders.add(f);
    await intercept(true);
    return async () => {
      holders.delete(f);
      if (!holders.size && !(late > 0)) await intercept(false);
    };
  };
  const errors = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  // The page's writes. Naming an undo step (`/api/graph/step`, gaps #29) writes nothing: not one of
  // them; nor what the musician looks at and an agent's pointer (`/api/focus`, `/api/point`, AGENT-6).
  const posts = [];
  const unwritten = /\/api\/(graph\/step|focus|point)$/;
  page.on("request", (r) => r.method() !== "GET" && !unwritten.test(r.url()) && posts.push({ url: r.url().replace(/^.*\/api/, "/api"), body: r.postData() ?? "" }));
  // Followed from the start, for `caughtUp` (caughtup.mjs).
  const caughtUp = following(page, () => state(), check);
  await page.evaluateOnNewDocument((token, side, platform) => {
    window.__M2__ = { native: false, token, platform };
    try {
      if (!localStorage.getItem("m2.browser.open")) localStorage.setItem("m2.browser.open", String(side));
    } catch {}
  }, sess.page_token, side, pc ? "linux" : "macos");
  await page.goto(sess.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
  const api = (path, opts = {}) =>
    page.evaluate(
      async (token, path, opts) => {
        // `x-e2e`: the suite's own read, never held back (`late`).
        const r = await fetch(`/api/graph${path}`, { ...opts, headers: { "x-m2-token": token, "content-type": "application/json", "x-e2e": "1" } });
        return { status: r.status, body: await r.json().catch(() => ({})) };
      },
      sess.page_token,
      path,
      opts,
    );
  const state = async () => (await api("")).body;
  const toasts = () => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
  const show = async (module) => {
    await page.click(`[data-module-tab="${module}"]`);
    await page.waitForSelector(`[data-module-tab="${module}"][aria-selected=true]`);
  };
  const view = async (id) => {
    await page.click(`[data-view-tab="${id}"]`);
    await until(() => page.$eval(`[data-view-tab="${id}"]`, (e) => e.getAttribute("aria-selected") === "true"));
  };
  const key = async (combo) => {
    const keys = combo.split("+");
    for (const k of keys.slice(0, -1)) await page.keyboard.down(k);
    await page.keyboard.press(keys.at(-1));
    for (const k of keys.slice(0, -1).reverse()) await page.keyboard.up(k);
  };
  const box = async (sel) => (await page.$(sel))?.boundingBox();
  /** Open the status pill's popover, if there is a pill and it isn't open: what needs attention is in it (taste.md ST-1). */
  const status = async () => {
    if ((await page.$("[data-status]")) && !(await page.$("[data-testid=status-popover]"))) await page.click("[data-status]").catch(() => {});
  };
  const drag = async (from, to, steps = 12) => {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps });
    await page.mouse.up();
  };
  return { browser, page, errors, posts, api, state, caughtUp, hold, toasts, show, view, key, box, drag, status };
}

export function done(errors) {
  check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
}

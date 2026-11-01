// Shared setup for the fixes suites: node fixes-*.mjs <root> <home> <project> (as run.mjs passes them for the fix kind).
import puppeteer from "puppeteer-core";
import fs from "node:fs";

export const [ROOT, HOME, PROJ] = process.argv.slice(2);
export const sess = JSON.parse(fs.readFileSync(`${HOME}/.music2/session.json`, "utf8"));
import { exe, pc as pcAgent, real } from "./chrome.mjs";

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
 */
export async function open(width = 1440, height = 900, { browser: side = false, pc = false } = {}) {
  const browser = await puppeteer.launch({ executablePath: pc ? real : exe, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  if (pc) await page.setUserAgent(pcAgent);
  await page.setViewport({ width, height });
  const errors = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  // The page's writes. Naming an undo step (`/api/graph/step`, gaps #29) writes nothing: not one of
  // them; nor what the musician looks at and an agent's pointer (`/api/focus`, `/api/point`, AGENT-6).
  const posts = [];
  const unwritten = /\/api\/(graph\/step|focus|point)$/;
  page.on("request", (r) => r.method() !== "GET" && !unwritten.test(r.url()) && posts.push({ url: r.url().replace(/^.*\/api/, "/api"), body: r.postData() ?? "" }));
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
        const r = await fetch(`/api/graph${path}`, { ...opts, headers: { "x-m2-token": token, "content-type": "application/json" } });
        return { status: r.status, body: await r.json().catch(() => ({})) };
      },
      sess.page_token,
      path,
      opts,
    );
  const state = async () => (await api("")).body;
  /**
   * Until the page shows the revision the server plays, and at least `at`. A gesture goes from the
   * revision the page shows (`[data-revision]`), and the server refuses one from an older revision
   * whose target has changed since ("the code changed around this control …", a clip's 409, an undo
   * step that isn't the newest). The server writes a change's files before it publishes its
   * revision, and the page reads it a moment later, so a file on disk doesn't mean the page has it.
   * A wait, not a check: only if they never meet, it fails, saying both.
   */
  const caughtUp = async (at = 0) => {
    let server = -1, shown = -1;
    const met = await until(async () => {
      server = (await state()).view?.revision ?? -1;
      shown = await page.$eval("[data-revision]", (e) => Number(e.dataset.revision)).catch(() => -1);
      return shown === server && shown >= at;
    });
    if (!met) check(false, `the page shows the revision the server plays${at ? `, ${at} or later` : ""}: it shows ${shown}, the server ${server}`);
    return met;
  };
  const toasts =() => page.$$eval("[data-testid=toast]", (els) => els.map((e) => e.textContent ?? ""));
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
  return { browser, page, errors, posts, api, state, caughtUp, toasts, show, view, key, box, drag, status };
}

export function done(errors) {
  check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
}

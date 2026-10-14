// Shared setup for the fixes suites: node fixes-*.mjs <root> <home> <project> (as run.mjs passes them for the fix kind).
import puppeteer from "puppeteer-core";
import fs from "node:fs";

export const [ROOT, HOME, PROJ] = process.argv.slice(2);
export const sess = JSON.parse(fs.readFileSync(`${HOME}/.music2/session.json`, "utf8"));
import { exe } from "./chrome.mjs";

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

export async function open(width = 1440, height = 900) {
  const browser = await puppeteer.launch({ executablePath: exe, args: ["--no-sandbox"] });
  const page = await browser.newPage();
  await page.setViewport({ width, height });
  const errors = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(String(e)));
  const posts = [];
  page.on("request", (r) => r.method() !== "GET" && posts.push({ url: r.url().replace(/^.*\/api/, "/api"), body: r.postData() ?? "" }));
  await page.evaluateOnNewDocument((token) => {
    window.__M2__ = { native: false, token, platform: "macos" };
  }, sess.token);
  await page.goto(sess.url, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-testid=graph] [data-module]", { timeout: 10000 });
  const api = (path, opts = {}) =>
    page.evaluate(
      async (token, path, opts) => {
        const r = await fetch(`/api/graph${path}`, { ...opts, headers: { "x-m2-token": token, "content-type": "application/json" } });
        return { status: r.status, body: await r.json().catch(() => ({})) };
      },
      sess.token,
      path,
      opts,
    );
  const state = async () => (await api("")).body;
  const toasts = () => page.$$eval(".glass", (els) => els.map((e) => e.textContent ?? ""));
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
  const drag = async (from, to, steps = 12) => {
    await page.mouse.move(from.x, from.y);
    await page.mouse.down();
    await page.mouse.move(to.x, to.y, { steps });
    await page.mouse.up();
  };
  return { browser, page, errors, posts, api, state, toasts, show, view, key, box, drag };
}

export function done(errors) {
  check(errors.length === 0, `no console errors ${errors.slice(0, 3).join(" | ")}`);
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
}

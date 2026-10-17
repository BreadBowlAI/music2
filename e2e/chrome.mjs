// The browser every suite drives: E2E_CHROME (run.mjs always sets it), else puppeteer's cached
// chrome-headless-shell 133.0.6943.141 for this OS (OS-73). Off the Mac, suites launch it through
// chrome-as-mac.sh, which gives pages a Mac user agent: the code editor (Monaco) takes its keys
// from it, and the suites test the Mac's keys (OS-75, Q2). A PC's page is opened with `pc`:
// pc-keys runs on one alone, and s8-mute, s8-audition, s8-clips and s8-automation each open one
// as well, to check their PC words. run.mjs takes `real` from here.
import os from "node:os";
import { fileURLToPath } from "node:url";

const mac = process.platform === "darwin";
const cache = `${os.homedir()}/.cache/puppeteer/chrome-headless-shell`;
export const real =
  process.env.E2E_CHROME ??
  (mac
    ? `${cache}/mac_arm-133.0.6943.141/chrome-headless-shell-mac-arm64/chrome-headless-shell`
    : `${cache}/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell`);
// puppeteer passes the environment on to the wrapper.
if (!mac) process.env.E2E_CHROME_BIN = real;
export const exe = mac ? real : fileURLToPath(new URL("./chrome-as-mac.sh", import.meta.url));
/**
 * A PC's user agent and platform, which pc-keys gives its page on every OS (OS-75, plan T8): the
 * code editor then takes a PC's keys (Ctrl) even on the Mac.
 */
export const pc = {
  userAgent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) HeadlessChrome/133.0.0.0 Safari/537.36",
  platform: "Linux x86_64",
};

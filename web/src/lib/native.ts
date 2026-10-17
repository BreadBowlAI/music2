// Bridges to the native shell (apps/music2-desktop): the menu bar and native actions.
// `native` is false only when the UI is opened outside the app (automated UI tests);
// keyboard shortcuts are then handled in the page instead of by the menu bar.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

declare global {
  interface Window {
    __M2__?: { native: boolean; token: string | null; platform: string };
  }
}

export const native = !!window.__M2__?.native;
export const token = window.__M2__?.token ?? null;
/** Whether to show the Mac's keys and labels: the platform the page is given (the app, a browser suite), else the browser's. */
export const isMac = window.__M2__?.platform ? window.__M2__.platform === "macos" : /Mac/.test(navigator.platform);

/*
 * Keys as this platform writes them (OS-54): the Mac's symbols on a Mac, the words on a PC's keys
 * elsewhere. Every label, tooltip and the ⌥K sheet spell keys through these, never by hand.
 */
/** A key's own name: ⌫ and Return on a Mac, Backspace and Enter on a PC. Other keys as given. */
export const keyName = (key: "Backspace" | "Enter" | (string & {})) =>
  key === "Backspace" ? (isMac ? "⌫" : "Backspace") : key === "Enter" ? (isMac ? "Return" : "Enter") : key;
/** A ⌘ chord: ⇧⌘E on a Mac, Ctrl+Shift+E elsewhere (⌥ first on a Mac, as Apple orders them). */
export const chord = (key: string, { shift = false, alt = false } = {}) =>
  isMac
    ? `${alt ? "⌥" : ""}${shift ? "⇧" : ""}⌘${keyName(key)}`
    : `Ctrl+${alt ? "Alt+" : ""}${shift ? "Shift+" : ""}${keyName(key)}`;
/** An ⌥ chord: ⌥3 on a Mac, Alt+3 elsewhere. */
export const opt = (key: string) => (isMac ? `⌥${keyName(key)}` : `Alt+${keyName(key)}`);
/** A ⇧ chord: ⇧Return on a Mac, Shift+Enter elsewhere. */
export const shifted = (key: string) => (isMac ? `⇧${keyName(key)}` : `Shift+${keyName(key)}`);
/** A modifier on its own, for a gesture ("⇧-drag", "Shift-drag") or a hint ("⌘ + scroll"). */
export const mod = isMac ? { cmd: "⌘", shift: "⇧", alt: "⌥" } : { cmd: "Ctrl", shift: "Shift", alt: "Alt" };
/** What shows a file or folder in the system's file manager. */
export const revealLabel = isMac ? "Show in Finder" : "Show in Folder";

/**
 * Ask the shell to do something with dialogs / the file system (see actions.rs). An action
 * that asks the user something resolves with the answer (`export_path`: the file picked, or
 * null if they cancelled); the others resolve with null at once.
 */
export function nativeAction(id: string, arg?: string) {
  return invoke<string | null>("action", { id, arg: arg ?? null });
}

export function onMenu(handler: (id: string) => void) {
  if (!native) return () => {};
  const p = listen<string>("menu", (e) => handler(e.payload));
  return () => void p.then((un) => un());
}

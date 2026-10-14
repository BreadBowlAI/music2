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

/** A shortcut as this platform writes it: ⌘D on a Mac, Ctrl+D elsewhere. */
export const kbd = (key: string, shift = false) => (isMac ? `${shift ? "⇧" : ""}⌘${key}` : `Ctrl+${shift ? "Shift+" : ""}${key}`);

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

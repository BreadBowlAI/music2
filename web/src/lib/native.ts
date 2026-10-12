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
export const isMac = native ? window.__M2__!.platform === "macos" : /Mac/.test(navigator.platform);

/** A shortcut as this platform writes it: ⌘D on a Mac, Ctrl+D elsewhere. */
export const kbd = (key: string, shift = false) => (isMac ? `${shift ? "⇧" : ""}⌘${key}` : `Ctrl+${shift ? "Shift+" : ""}${key}`);

/** Ask the shell to do something with dialogs / the file system (see actions.rs). */
export function nativeAction(id: string) {
  return invoke("action", { id });
}

export function onMenu(handler: (id: string) => void) {
  if (!native) return () => {};
  const p = listen<string>("menu", (e) => handler(e.payload));
  return () => void p.then((un) => un());
}

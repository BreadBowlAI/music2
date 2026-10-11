// Bridges to the native shell (apps/music2-desktop): menus, dialogs, drag-and-drop.
// `native` is false only when the UI is opened outside the app (automated UI tests);
// keyboard shortcuts are then handled in the page instead of by the menu bar.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { getCurrentWebview } from "@tauri-apps/api/webview";

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
export function nativeAction(id: string, args?: Record<string, unknown>) {
  return invoke("action", { id, args: args ?? null });
}

export function onMenu(handler: (id: string) => void) {
  if (!native) return () => {};
  const p = listen<string>("menu", (e) => handler(e.payload));
  return () => void p.then((un) => un());
}

// ---------------------------------------------------------------- context menus

export type MenuEntry =
  | "-"
  | {
      label: string;
      action?: () => void;
      /** Display only, e.g. "⌘D" — the shortcut itself is handled elsewhere. */
      shortcut?: string;
      checked?: boolean;
      disabled?: boolean;
      danger?: boolean;
      submenu?: MenuEntry[];
    };

/** Build a menu, dropping entries that are `false` (conditional items). */
export const menu = (...entries: (MenuEntry | false | null | undefined)[]) => entries.filter((x): x is MenuEntry => !!x);

/** Show a right-click menu (an OS menu). */
export function contextMenu(e: { preventDefault: () => void; stopPropagation: () => void }, entries: MenuEntry[]) {
  e.preventDefault();
  e.stopPropagation();
  void popupNative(entries);
}

// Accelerators shown in native menus are display hints; translate "⌘⇧D" → "CmdOrCtrl+Shift+D".
function accel(s?: string) {
  if (!s) return undefined;
  const mods: string[] = [];
  let key = s;
  for (const [sym, name] of [["⌘", "CmdOrCtrl"], ["⇧", "Shift"], ["⌥", "Alt"], ["⌃", "Ctrl"]] as const) {
    if (key.includes(sym)) {
      mods.push(name);
      key = key.replace(sym, "");
    }
  }
  key = { "⌫": "Backspace", "↵": "Enter", "␣": "Space" }[key] ?? key;
  return [...mods, key].join("+");
}

type NativeItem = Parameters<typeof Menu.new>[0] extends infer O ? (O extends { items?: (infer I)[] } ? I : never) : never;

function toNative(entries: MenuEntry[]): NativeItem[] {
  return entries.map((x): NativeItem => {
    if (x === "-") return { item: "Separator" };
    if (x.submenu) return { text: x.label, enabled: !x.disabled, items: toNative(x.submenu) } as NativeItem;
    const base = { text: x.label, enabled: !x.disabled && !!x.action, accelerator: accel(x.shortcut), action: () => x.action?.() };
    return (x.checked !== undefined ? { ...base, checked: x.checked } : base) as NativeItem;
  });
}

async function popupNative(entries: MenuEntry[]) {
  const menu = await Menu.new({ items: toNative(entries) });
  await menu.popup();
}

// ---------------------------------------------------------------- drag and drop

/** Files dragged in from Finder: songs open, WAVs land where they're dropped. */
export function onFileDrop(handler: (paths: string[], x: number, y: number) => void, hover: (over: boolean) => void) {
  if (!native) return () => {};
  const p = getCurrentWebview().onDragDropEvent((e) => {
    const t = e.payload.type;
    if (t === "enter" || t === "over") hover(true);
    else hover(false);
    if (t === "drop") {
      const scale = window.devicePixelRatio || 1;
      handler(e.payload.paths, e.payload.position.x / scale, e.payload.position.y / scale);
    }
  });
  return () => void p.then((un) => un());
}

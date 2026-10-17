import { create } from "zustand";
import type { GraphState } from "./graph";

export interface Toast { id: number; text: string; action?: { label: string; run: () => void }; server?: boolean }

interface Store {
  /** The open song project, or null when none is open. */
  graph: GraphState | null;
  /** The first check for an open project has finished (the loading screen shows until then). */
  ready: boolean;
  connected: boolean;
  /** The toast showing: one at a time, a new one replacing it (taste.md §2.14). */
  toasts: Toast[];
  set: (p: Partial<Store>) => void;
  /** `server`: the server's word to every window (a WebSocket toast), not this window's own. */
  toast: (text: string, action?: Toast["action"], server?: boolean) => void;
  dismiss: (id: number) => void;
}

let toastId = 0;
/** The action-less toasts shown in the last 5 s, replaced or not: what `toast` shows once. */
let lately: { text: string; server: boolean; at: number }[] = [];

export const useStore = create<Store>((set, get) => ({
  graph: null,
  ready: false,
  connected: false,
  toasts: [],
  set: (p) => set(p),
  // Its timer (4 s, or 6 s with an action, paused while hovered) is the toast's own: see Toasts.
  toast: (text, action, server = false) => {
    // Shown once: an edit's own error, and the server telling every window, in the same words, how
    // a heard edit ended (PLANE-5), even when another toast came between them. Two of this
    // window's own, or two of the server's, both show: the newer replaces the older.
    const now = Date.now();
    lately = lately.filter((t) => now - t.at < 5000);
    const same = (t: { text: string; server?: boolean; action?: unknown }) => !t.action && t.text === text && Boolean(t.server) !== server;
    if (!action && (get().toasts.some(same) || lately.some(same))) return;
    if (!action) lately.push({ text, server, at: now });
    set({ toasts: [{ id: ++toastId, text, action, server }] });
  },
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));

import { create } from "zustand";
import type { GraphState } from "./graph";

export interface Toast { id: number; text: string; action?: { label: string; run: () => void }; server?: boolean }

interface Store {
  /** The open song project, or null when none is open. */
  graph: GraphState | null;
  /** The first check for an open project has finished (the loading screen shows until then). */
  ready: boolean;
  connected: boolean;
  toasts: Toast[];
  set: (p: Partial<Store>) => void;
  /** `server`: the server's word to every window (a WebSocket toast), not this window's own. */
  toast: (text: string, action?: Toast["action"], server?: boolean) => void;
}

let toastId = 0;

export const useStore = create<Store>((set, get) => ({
  graph: null,
  ready: false,
  connected: false,
  toasts: [],
  set: (p) => set(p),
  toast: (text, action, server = false) => {
    // Shown once: an edit's own error, and the server telling every window, in the same words, how
    // a heard edit ended (PLANE-5). Two of this window's own, or two of the server's, both show.
    if (!action && get().toasts.some((t) => t.text === text && !t.action && Boolean(t.server) !== server)) return;
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, action, server }].slice(-4) });
    setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== id) }), 5000);
  },
}));

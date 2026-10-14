import { create } from "zustand";
import type { GraphState } from "./graph";

export interface Toast { id: number; text: string; action?: { label: string; run: () => void } }

interface Store {
  /** The open song project, or null when none is open. */
  graph: GraphState | null;
  /** The first check for an open project has finished (the loading screen shows until then). */
  ready: boolean;
  connected: boolean;
  /** The toast showing: one at a time, a new one replacing it (taste.md §2.14). */
  toasts: Toast[];
  set: (p: Partial<Store>) => void;
  toast: (text: string, action?: Toast["action"]) => void;
  dismiss: (id: number) => void;
}

let toastId = 0;

export const useStore = create<Store>((set, get) => ({
  graph: null,
  ready: false,
  connected: false,
  toasts: [],
  set: (p) => set(p),
  // Its timer (4 s, or 6 s with an action, paused while hovered) is the toast's own: see Toasts.
  toast: (text, action) => set({ toasts: [{ id: ++toastId, text, action }] }),
  dismiss: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),
}));

import { create } from "zustand";
import type { GraphState } from "./graph";

export interface Toast { id: number; text: string; action?: { label: string; run: () => void } }

interface Store {
  /** The open song project, or null when none is open. */
  graph: GraphState | null;
  /** The first check for an open project has finished (the loading screen shows until then). */
  ready: boolean;
  connected: boolean;
  toasts: Toast[];
  set: (p: Partial<Store>) => void;
  toast: (text: string, action?: Toast["action"]) => void;
}

let toastId = 0;

export const useStore = create<Store>((set, get) => ({
  graph: null,
  ready: false,
  connected: false,
  toasts: [],
  set: (p) => set(p),
  toast: (text, action) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, action }].slice(-4) });
    setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== id) }), 5000);
  },
}));

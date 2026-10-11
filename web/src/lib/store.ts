import { create } from "zustand";
import { api } from "./api";
import type { Id, ServerState, Tick } from "./types";

export type SideTab = "track" | "sounds";
export interface Toast { id: number; text: string; action?: { label: string; run: () => void } }

interface Store {
  server: ServerState | null;
  tick: Tick;
  connected: boolean;
  selectedTrack: Id | null;
  selectedClip: Id | null;
  sideTab: SideTab;
  ppb: number;
  modal: null | "settings" | "library";
  /** Files are being dragged over the window (desktop). */
  dropHover: boolean;
  /** Clip whose name is being edited inline. */
  renamingClip: Id | null;
  /** Open automation lanes: track id → lane key ("volume" | "pan" | "macro:<device>:<macro>"). */
  autoOpen: Record<number, string>;
  toasts: Toast[];
  set: (p: Partial<Store>) => void;
  toast: (text: string, action?: Toast["action"]) => void;
  run: (cmd: string, params?: Record<string, unknown>) => Promise<any>;
}

let toastId = 0;

export const useStore = create<Store>((set, get) => ({
  server: null,
  tick: { playhead: 0, playing: false, master: 0, peaks: {} },
  connected: false,
  selectedTrack: null,
  selectedClip: null,
  sideTab: "track",
  ppb: 28,
  modal: null,
  dropHover: false,
  renamingClip: null,
  autoOpen: {},
  toasts: [],
  set: (p) => set(p),
  toast: (text, action) => {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, text, action }].slice(-4) });
    setTimeout(() => set({ toasts: get().toasts.filter((t) => t.id !== id) }), 5000);
  },
  run: async (cmd, params = {}) => {
    try {
      return await api.exec(cmd, params);
    } catch (e) {
      get().toast((e as Error).message);
      return null;
    }
  },
}));

export const useSong = () => useStore((s) => s.server?.song ?? null);

// The sound output and this machine's settings (lane C of the next track): what the status says of
// the output (gaps #13), and the Settings window's calls and state (gaps #31). The settings are the
// server's (`~/.music2/settings.json`), never a song's, with no undo. Added to graph.ts's types from
// here, as graph8b.ts does.

import { create } from "zustand";
import { request } from "./projects";

declare module "./graph" {
  interface GraphStatus {
    /** The sound output's name; null with no player, or while none plays. */
    output?: string | null;
    /** No sound output started, so the song plays silently (gaps #13). */
    no_output?: boolean;
  }
}

/** A sound output there is: its own rate, and the buffer sizes it takes, when it says. */
export interface Output {
  name: string;
  default: boolean;
  rate: number;
  frames: [number, number] | null;
}

/** What plays now. */
export interface OutputNow {
  device: string;
  rate: number;
  frames: number | null;
  /** The output chosen is gone, so this is the default. */
  fell_back: boolean;
  /** No output started: the song plays silently. */
  no_output: boolean;
}

/** The settings as the server answers them (`GET /api/settings`). */
export interface Settings {
  /** What is chosen: null is the system's default. */
  output: { device: string | null; frames: number | null };
  outputs: Output[];
  /** Null while no song has started a player. */
  now: OutputNow | null;
  /** Where new songs go now, and the default for that. */
  songs: string | null;
  songs_chosen: boolean;
  songs_default: string | null;
  home: string | null;
  /** An export runs: the output can't change until it is done. */
  exporting: boolean;
  /** The change just made stopped playback. */
  stopped: boolean;
}

export interface SettingsChange {
  output?: { device: string | null; frames: number | null };
  songs?: string | null;
}

export const settingsApi = {
  get: () => request<Settings>("/api/settings"),
  set: (change: SettingsChange) => request<Settings>("/api/settings", change),
};

/** Whether the Settings window shows. */
export const useSettings = create<{ open: boolean }>(() => ({ open: false }));

/** Show the Settings window: ⌘, (Ctrl+, off the Mac), the menu's Settings…, the status pill's links. */
export const openSettings = () => useSettings.setState({ open: true });

/** The buffer sizes Settings offers, in samples, each where the device takes it. */
export const BUFFERS = [64, 128, 256, 512, 1024, 2048];

/** A buffer's length in time, to one decimal: "5.3 ms" for 256 samples at 48 kHz. */
export const bufferMs = (frames: number, rate: number) => `${(Math.round((frames / rate) * 10000) / 10).toLocaleString("en", { minimumFractionDigits: 1, maximumFractionDigits: 1 })} ms`;

/** A buffer as the window names it: "256 samples (5.3 ms)". */
export const bufferWords = (frames: number, rate: number) => `${frames} samples (${bufferMs(frames, rate)})`;

/** A rate as the app says it: "48 kHz", "44.1 kHz". */
export const rateWords = (rate: number) => `${(rate / 1000).toLocaleString("en", { maximumFractionDigits: 1 })} kHz`;

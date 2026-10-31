// What the 8b back ends add to the open project's state and status: mute and solo (8b.4), clips
// by id (8b.5), audition (8b.6), meters and the load (8b.8). Added to graph.ts's types from here,
// so changes to that file and these never touch the same lines.

import { useStore } from "./store";

/**
 * How a track is heard now (MON-3). A mute wins: "muted" while listening, "muted-in-song" when its
 * code says so. "silenced" when a solo leaves every place its sound goes off.
 */
export type Heard = "heard" | "muted" | "muted-in-song" | "silenced";

/** Something a track drives besides what it is heard through: a duck's sidechain, a control. */
export interface Feed {
  /** The module it sits in: `song` for one written in song.ts. */
  module: string;
  /** Its component's name: "Duck". */
  label: string;
}

/** Mutes and solos while listening (MON-4), by track key, and how each track is heard. */
export interface OverlayState {
  mute: string[];
  solo: string[];
  /** Tracks muted in the song: `muted: true` in their code (MON-5). */
  muted_in_song: string[];
  heard: Record<string, Heard>;
  /** What each track drives: a muted one still drives it. */
  feeds: Record<string, Feed[]>;
  /** False in a window that can't write the project: they live in memory only. */
  saved: boolean;
}

declare module "./graph" {
  interface GraphStatus {
    /** Live notes sounding: audition's (MON-7). */
    live?: number;
    /** The longest audio callback of the last second, as a share of the device's period (8b.8). */
    load?: number;
    /** Audio callbacks that took longer than their period since the player started. */
    late?: number;
    /** Dropouts the device reported since the player started. */
    xruns?: number;
    /** For 10 s after an audio lane missed its audio: what to say (RENDER-30). */
    disk?: string;
    /** The device's other errors since the player started. */
    device_errors?: number;
  }
  interface GraphState {
    overlays?: OverlayState;
  }
  // A clip's version (8b.5): a gesture says which one it saw, and is refused if it changed.
  interface NoteClip {
    sha?: string;
  }
  interface ControlClip {
    sha?: string;
  }
  interface AudioClip {
    sha?: string;
  }
}

/** A key struck on a lane: this window's handle for it, its pitch, its velocity (0.8 when left out). */
export interface Strike {
  h: number;
  pitch: number;
  velocity?: number;
}

/** What a window sends to play notes (MON-7): Offs first, then Ons. `keep` says handles are still held. */
export interface AuditionOut {
  t: "audition";
  module?: string;
  lane?: string;
  on?: Strike[];
  off?: number[];
  keep?: number[];
}

/** The server's answer, to this window only: why a lane is silent, or what went wrong, in words. */
export interface AuditionIn {
  t: "audition";
  module?: string;
  lane?: string;
  silent?: string;
  error?: string;
}

/** Peaks per channel, `[left, right]`, linear: 1.0 is 0 dBFS. */
export type Peaks = [number, number];

/** The meters (8b.8), about 30 times a second while the engine runs, and once more as it idles. */
export interface MetersIn {
  t: "meters";
  /**
   * Each track's loudest sample since the last message, after its mute and solo: after its fader
   * when its channel is its own, else where it enters the song (TAP-5).
   */
  tracks: Record<string, Peaks>;
  /** Each mixer channel's, after its fader (TAP-5). Absent from older servers. */
  channels?: Record<string, Peaks>;
  /** The song's output. */
  out: Peaks;
  /**
   * The output's loudness (TAP-6): the last 3 s in LUFS and the largest true peak of the last 3 s
   * in dBTP, each null until there is one. Absent from older servers.
   */
  loudness?: { short: number | null; true_peak: number | null };
  /** Each scope asked for (TAP-8): its box's last frames, a list per channel. */
  scopes?: { module: string; node: string; frames: number[][] }[];
  /** The sound input's loudest sample, while a take records on an audio lane (io). */
  input?: number[];
  dsp: { load: number; dropouts: number };
}

/** How `module` is heard now: "heard" until the state says otherwise. */
export const useHeard = (module: string): Heard => useStore((s) => s.graph?.overlays?.heard[module] ?? "heard");

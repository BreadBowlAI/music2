// Mirrors the Rust song model (m2-project) as serialized by serde.

export type Id = number;
export const PPQ = 960;

/** `id` is stable for the note's lifetime (0 = new, assigned by the engine). */
export interface Note { id: number; pitch: number; start: number; length: number; velocity: number }

export type ClipContent =
  | { type: "midi"; notes: Note[] }
  | { type: "audio"; asset: Id; offset: number; gain_db: number };

export interface Clip { id: Id; name: string; start: number; length: number; content: ClipContent }

export interface Device {
  id: Id; plugin: string; macros: Record<string, number>; assets: Record<string, Id>; bypass: boolean;
  /** Track feeding this device's sidechain input. */
  sidechain: Id | null;
}

export interface Send { to: Id; gain_db: number; pre_fader: boolean }
export type AutoTarget = { param: "volume" } | { param: "pan" } | { param: "macro"; device: Id; macro: string };
export interface AutoPoint { tick: number; value: number }
export interface AutomationLane { id: Id; target: AutoTarget; points: AutoPoint[] }

export interface Track {
  id: Id; name: string; color: [number, number, number]; kind: "instrument" | "audio" | "bus";
  instrument: Device | null; effects: Device[]; clips: Clip[];
  volume_db: number; pan: number; mute: boolean; solo: boolean;
  /** Bus this track feeds (null = master). */
  output: Id | null; sends: Send[]; automation: AutomationLane[];
}

export interface Asset { id: Id; name: string; file: string; prompt: string | null; root_pitch: number; duration: number }

export type ScaleName =
  | "major" | "minor" | "dorian" | "mixolydian" | "lydian" | "phrygian"
  | "harmonic_minor" | "pentatonic_major" | "pentatonic_minor" | "chromatic";

export interface Song {
  format: number; title: string; bpm: number; time_sig: { num: number; den: number };
  key: { root: number; scale: ScaleName }; tracks: Track[];
  master: { volume_db: number; effects: Device[] }; assets: Asset[]; next_id: Id;
}

export interface MacroDef { id: string; name: string; default: number; description: string }

export interface Plugin {
  id: string; name: string; kind: "instrument" | "effect" | "kit"; description: string; tags: string[];
  macros: MacroDef[]; asset_slots: string[]; pads: { pitch: number; name: string }[]; uses_sidechain: boolean;
}

export interface ServerState {
  revision: number; ui_rev: number; song: Song; end_tick: number;
  path: string | null; dirty: boolean; undo: string | null; redo: string | null;
  looping: boolean; plugins: Plugin[];
  audio: { ok: boolean; device: string | null; sample_rate: number | null; error: string | null };
  library: string;
}

export interface Tick { playhead: number; playing: boolean; master: number; peaks: Record<string, number> }


// An audio file's waveform peaks (lane A's A1, Q9): fetched once per file and kept while the page
// lives, however many clips play it and however often they draw. The server makes them from the
// file (`crates/server/src/peaks.rs`); this reads its bytes. Also the tempo map's seconds, which an
// audio clip is measured in (Q5).

import { useEffect } from "react";
import { create } from "zustand";
import type { AudioClip } from "./graph";
import { token } from "./native";
import { useStore } from "./store";
import { http } from "./http";

/** For each bucket of `bucket` frames, for each channel: its lowest, then its highest sample, × 127. */
export interface Peaks {
  channels: number;
  rate: number;
  frames: number;
  bucket: number;
  data: Int8Array;
}

/** The file isn't there, or doesn't read: the clip says its name, as before (FMT-15). */
export const MISSING = "missing";

const auth: Record<string, string> = token ? { "x-m2-token": token } : {};

/** `peaks.rs`' bytes, or null when they aren't peaks. */
export function readPeaks(buf: ArrayBuffer): Peaks | null {
  if (buf.byteLength < 24) return null;
  const v = new DataView(buf);
  const magic = String.fromCharCode(v.getUint8(0), v.getUint8(1), v.getUint8(2), v.getUint8(3));
  if (magic !== "M2PK" || v.getUint16(4, true) !== 1) return null;
  const channels = v.getUint16(6, true);
  const rate = v.getUint32(8, true);
  const frames = Number(v.getBigUint64(12, true));
  const bucket = v.getUint32(20, true);
  if (!channels || !bucket) return null;
  const data = new Int8Array(buf, 24);
  return data.length === Math.ceil(frames / bucket) * channels * 2 ? { channels, rate, frames, bucket, data } : null;
}

// What each file got, by session, name and pin; asked once each.
const got = new Map<string, Peaks | typeof MISSING>();
const asking = new Set<string>();
/** Bumped as answers come, so the clips that wait draw. */
const useArrived = create<{ n: number }>(() => ({ n: 0 }));

async function ask(session: number, asset: string, key: string) {
  if (asking.has(key) || (got.has(key) && got.get(key) !== MISSING)) return;
  asking.add(key);
  let peaks: Peaks | typeof MISSING = MISSING;
  try {
    const res = await http(`/api/graph/audio/peaks?session=${session}&asset=${encodeURIComponent(asset)}`, { headers: auth });
    if (res.ok) peaks = readPeaks(await res.arrayBuffer()) ?? MISSING;
  } catch {
    // Unread: drawn as a missing file until the song changes, then asked again.
  }
  asking.delete(key);
  got.set(key, peaks);
  useArrived.setState((s) => ({ n: s.n + 1 }));
}

/**
 * The peaks of the file clip `clip` plays: null until they come, `MISSING` when the file can't be
 * read. One request per file and pin; one that failed is asked again when the song changes.
 */
export function usePeaks(clip: Pick<AudioClip, "asset" | "hash">): Peaks | typeof MISSING | null {
  const session = useStore((s) => s.graph?.session);
  const revision = useStore((s) => s.graph?.view.revision);
  useArrived((s) => s.n);
  const key = `${session}\n${clip.asset}\n${clip.hash ?? ""}`;
  useEffect(() => {
    if (session != null) void ask(session, clip.asset, key);
  }, [session, clip.asset, key, revision]);
  return got.get(key) ?? null;
}

/** A tempo map as the timeline gives it: from `beat` on, `bpm` (FMT-13), the first at beat 0. */
export type Tempo = readonly { beat: number; bpm: number }[];

/** The seconds from the song's start to `beat`, as `TempoMap::seconds_at`. */
export function secondsAt(tempo: Tempo, beat: number): number {
  let at = 0;
  let prev = tempo[0] ?? { beat: 0, bpm: 120 };
  for (const t of tempo.slice(1)) {
    if (t.beat > beat) break;
    at += ((t.beat - prev.beat) * 60) / prev.bpm;
    prev = t;
  }
  return at + ((beat - prev.beat) * 60) / prev.bpm;
}

/** The beat `seconds` from the song's start, as `TempoMap::beat_at`. */
export function beatAt(tempo: Tempo, seconds: number): number {
  let at = 0;
  let prev = tempo[0] ?? { beat: 0, bpm: 120 };
  for (const t of tempo.slice(1)) {
    const next = at + ((t.beat - prev.beat) * 60) / prev.bpm;
    if (next > seconds) break;
    at = next;
    prev = t;
  }
  return prev.beat + ((seconds - at) * prev.bpm) / 60;
}

/** How long a clip plays, in seconds, under the tempo map. */
export const clipSeconds = (tempo: Tempo, c: { start: number; length: number }) => secondsAt(tempo, c.start + c.length) - secondsAt(tempo, c.start);

/** The length in seconds of the file clip `c` plays, once its peaks are here (null before, or missing). */
export function fileSeconds(session: number, c: Pick<AudioClip, "asset" | "hash">): number | null {
  const p = got.get(`${session}\n${c.asset}\n${c.hash ?? ""}`);
  return p && p !== MISSING ? p.frames / p.rate : null;
}

/**
 * How far audio clip `c`'s edges may go (Q13, D3), in beats: forwards, the left edge to the file's
 * start and the right edge to its end; backwards (a reversed clip plays its span from its end), the
 * left edge to the file's end and the right edge to its start. `start` and `end` say which end of
 * the file stops each edge. Null where nothing stops it (a pitched or warped clip, a file not read).
 */
export function edgeLimits(c: AudioClip, tempo: Tempo, file: number | null) {
  const free = { left: null as number | null, right: null as number | null, leftWord: "", rightWord: "" };
  const warped = ((c as AudioClip & { warp?: unknown[] }).warp?.length ?? 0) > 0;
  if ((c.pitch ?? 0) !== 0 || warped || file === null) return free;
  const offset = c.offset ?? 0;
  const t0 = secondsAt(tempo, c.start), t1 = secondsAt(tempo, c.start + c.length);
  const name = c.asset.split("/").pop() ?? c.asset;
  return c.reverse
    ? { left: beatAt(tempo, t1 - (file - offset)), right: beatAt(tempo, t1 + offset), leftWord: `End of ${name}`, rightWord: `Start of ${name}` }
    : { left: beatAt(tempo, t0 - offset), right: beatAt(tempo, t0 + file - offset), leftWord: `Start of ${name}`, rightWord: `End of ${name}` };
}

/** Audio clip `c` given a new span, what plays inside staying where it plays: its offset moves as the server moves it (A3). */
export function respanned(c: AudioClip, tempo: Tempo, start: number, length: number): AudioClip {
  const offset = c.offset ?? 0;
  const moved = c.reverse
    ? offset + secondsAt(tempo, c.start + c.length) - secondsAt(tempo, start + length)
    : offset + secondsAt(tempo, start) - secondsAt(tempo, c.start);
  return { ...c, start, length, offset: Math.max(0, moved) };
}

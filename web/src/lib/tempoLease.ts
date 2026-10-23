// The tempo field's drag, heard as it goes (RENDER-27a, 8b.7, design-8b §9.3): playing or not
// (the time readout follows), the song plays at the dragged tempo before it is let go. Letting go
// writes it once, one undo step ("Tempo"), as a typed tempo is. Esc puts it back.
//
// The server's tempo lease (`/api/graph/tempo/lease/…`) keeps the newest value and plays it when
// the plan it needs is ready: one at a time, the newest next. The page sends a value at most once
// an animation frame, numbered, and a keepalive every 2 s while the pointer holds still. In a song
// with an audio clip the server plays nothing new until it is let go (its renders are timed in the
// tempo they were made in).

import { clientId, readJson } from "./graph";
import { recorded, type Step } from "./history";
import { token } from "./native";

/** How often a still drag tells the server it lives (its lease ends after 10 s of nothing). */
const KEEPALIVE_MS = 2000;

/** What a tempo write answers, as `POST /api/graph/tempo` does. */
export interface TempoDone {
  file: string;
  before: string;
  after: string;
  revision: number;
  /** The new tempo made the loop too short, so it is off. */
  loop_off?: boolean;
}

export interface TempoLease {
  /** The drag's newest value: heard once its plan is ready. */
  update(bpm: number): void;
  /** Let go at `bpm`: written once, one undo step named `step`. Refused, it throws the server's words. */
  finish(bpm: number, step: Step): Promise<TempoDone>;
  /** Esc, a lost pointer: the tempo written plays again, and nothing is written. */
  abort(): void;
}

const post = async (path: string, body: unknown) => {
  const res = await fetch(`/api/graph/tempo/lease/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: JSON.stringify(body),
  });
  const json = await readJson(res);
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json;
};

/**
 * A lease for a drag of the tempo in effect at beat `at` of song `session`. `ended` hears why, if
 * the server ends it first: the drag is then over, and its caller puts the field back.
 */
export function begin(session: number, at: number, ended: (why: string) => void): TempoLease {
  let seq = 0;
  let next: number | null = null;
  let frame = 0;
  let over = false;
  // The lease, once the server answers: null when it couldn't begin (the write then goes as a typed tempo does).
  const lease: Promise<number | null> = post("begin", { session, at, client: clientId }).then(
    (b) => b.lease as number,
    () => null,
  );
  const stop = () => {
    over = true;
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
    clearInterval(alive);
  };
  const lost = (e: unknown) => {
    if (over) return;
    stop();
    ended(e instanceof Error ? e.message : String(e));
  };
  const alive = setInterval(() => void lease.then((id) => id !== null && !over && post("keepalive", { lease: id }).catch(lost)), KEEPALIVE_MS);
  const flush = () => {
    frame = 0;
    const bpm = next;
    next = null;
    if (bpm === null || over) return;
    seq += 1;
    const n = seq;
    void lease.then((id) => id !== null && !over && post("update", { lease: id, seq: n, bpm }).catch(lost));
  };
  return {
    update(bpm) {
      next = bpm;
      if (!frame && !over) frame = requestAnimationFrame(flush);
    },
    async finish(bpm, step) {
      stop();
      const id = await lease;
      const done: TempoDone =
        id === null
          ? await fetch("/api/graph/tempo", {
              method: "POST",
              headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
              body: JSON.stringify({ session, at, bpm }),
            }).then(async (res) => {
              const json = await readJson(res);
              if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
              return json;
            })
          : await post("commit", { lease: id, bpm });
      if (done.before !== done.after) recorded(session, done, step);
      return done;
    },
    abort() {
      if (over) return;
      stop();
      void lease.then((id) => id !== null && post("cancel", { lease: id }).catch(() => {}));
    },
  };
}

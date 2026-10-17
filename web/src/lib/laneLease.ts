// A drag heard while it lasts (WRITE-22a, 8b.7, design-8b §9.3): while the song plays, a clip, a
// clip's edge, an automation point or a note in Notes dragged is heard where it is, before it is
// let go. Letting go writes it: one gesture, one undo step, as without a lease. Esc puts it back.
// Stopped, there is nothing to hear: a drag writes on release as before, with no lease.
//
// The server's lane lease (`/api/graph/lane/lease/…`) plays each gesture straight into the
// renderer that plays. The page sends the newest at most once an animation frame, numbered, so an
// older one that arrives late is dropped, and a keepalive every 2 s while the pointer holds still.
// Nothing on screen is new: the sound is the feedback.

import { playingNow } from "./clock";
import { send, type Done, type Gesture } from "./clips";
import { clientId, readJson, type GraphState } from "./graph";
import type { Step } from "./history";
import { token } from "./native";

/** How often a still drag tells the server it lives (its lease ends after 10 s of nothing). */
const KEEPALIVE_MS = 2000;

export interface LaneLease {
  /** The drag's newest gesture: heard within a frame. */
  update(gesture: Gesture): void;
  /**
   * Let go: `gesture` written, one undo step named `step`, answered as `send` answers. A gesture the
   * server doesn't preview (an audio clip's) is written by `send`, as before.
   */
  finish(gesture: Gesture, step: Step): Promise<Done>;
  /** Esc, a lost pointer, the view gone: what was there plays again, and nothing is written. */
  abort(): void;
}

const post = async (path: string, body: unknown) => {
  const res = await fetch(`/api/graph/lane/lease/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...(token ? { "x-m2-token": token } : {}) },
    body: JSON.stringify(body),
  });
  const json = await readJson(res);
  if (!res.ok) throw new Error(json.error ?? `HTTP ${res.status}`);
  return json;
};

/**
 * A lease for a drag that has just begun as `gesture`, while the song plays; null when it is stopped
 * (the drag writes on release, as before). `ended` hears why, if the server ends it first (the clip
 * changed elsewhere): the drag is then over, and its caller puts it back.
 */
export function begin(graph: GraphState, gesture: Gesture, ended: (why: string) => void): LaneLease | null {
  if (!playingNow()) return null;
  let seq = 1;
  let next: Gesture | null = null;
  let frame = 0;
  let over = false;
  // The lease, once the server answers: null when it isn't previewed or wasn't begun.
  const lease: Promise<number | null> = post("begin", { session: graph.session, gesture, client: clientId }).then(
    (b) => (b.previewable ? (b.lease as number) : null),
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
    const g = next;
    next = null;
    if (!g || over) return;
    seq += 1;
    const n = seq;
    void lease.then((id) => id !== null && !over && post("update", { lease: id, seq: n, gesture: g }).catch(lost));
  };
  return {
    update(g) {
      next = g;
      if (!frame && !over) frame = requestAnimationFrame(flush);
    },
    async finish(g, step) {
      stop();
      const id = await lease;
      if (id === null) return send(graph, g, step);
      return send(graph, g, step, { path: "/api/graph/lane/lease/commit", body: { lease: id, gesture: g } });
    },
    abort() {
      if (over) return;
      stop();
      void lease.then((id) => id !== null && post("cancel", { lease: id }).catch(() => {}));
    },
  };
}

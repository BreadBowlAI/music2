// The transport as the page plays it (taste.md §5.1, design-8b §4.3): Space plays from the
// playhead and stops back where play started, as in Live; ⇧Space continues from where it stopped.
// A seek (a click in the ruler, Return) moves the start point. All of it is this window's: the
// server only plays, stops and seeks.

import { graphApi } from "./graph";

/** Where play last started, or where a seek moved the start since; null until this window plays. */
let start: number | null = null;
/** Where the last stop stopped, for ⇧Space. */
let stopped: number | null = null;

/** A song opened: nothing played here yet. */
export function resetTransport() {
  start = stopped = null;
}

/** A seek: play starts there now, and a stop comes back there. */
export function startAt(beat: number) {
  start = beat;
}

/** Space and the Play button: play from the playhead, or stop and go back to where play started. */
export async function playStop() {
  const st = await graphApi.status();
  if (st.playing) {
    // Sent with the stop, so nothing sees it stopped where it was first.
    const r = await graphApi.transport("stop", start ?? undefined);
    stopped = r.at ?? st.beat;
    return;
  }
  start = st.beat;
  await graphApi.transport("play");
}

/** ⇧Space: play on from where the last stop stopped. The start point stays where it was. */
export async function continuePlay() {
  const st = await graphApi.status();
  if (st.playing) return;
  if (stopped !== null && stopped !== st.beat) await graphApi.seek(stopped);
  start ??= stopped ?? st.beat;
  await graphApi.transport("play");
}

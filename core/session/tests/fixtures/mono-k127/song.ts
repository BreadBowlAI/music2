// One note at a time, played with velocities a keyboard sends (k/127): std's lead, 808 sub and bass,
// FM keys and the whole synth given `voices: 1`. Overlaps slide, touches start again, a trill
// glides over a held note, and a note let go brings back the one still held. Its render is
// recorded (`gate_tests.rs`): std's one-note sounds keep these bits on these velocities.

import { ms, song, track } from "@m2/core";
import { fm, synth } from "@m2/synths";
import { mixer } from "std/mix";

const lead = track("lead", { name: "Lead" }, ({ lane }) => synth.lead(lane.notes("notes"), { glide: ms(70) }));
const sub = track("sub", { name: "Sub" }, ({ lane }) => synth.sub(lane.notes("notes")));
const bass = track("bass", { name: "Bass" }, ({ lane }) => synth.bass(lane.notes("notes")));
const keys = track("keys", { name: "Keys" }, ({ lane }) => fm.keys(lane.notes("notes"), { voices: 1 }));
const solo = track("solo", { name: "Solo" }, ({ lane }) => synth.custom(lane.notes("notes"), { voices: 1, vibrato: 0.3, glide: ms(30) }));

export default song(() => mixer({ lead: lead.out, sub: sub.out, bass: bass.out, keys: keys.out, solo: solo.out }));

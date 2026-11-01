// The root module: whatever this returns is what you hear.
//
// Add a track with + Track, or write one in code: a track() line like the drum one below, and
// one line for it in the mixer. + Track's Drum kit adds a whole kit: a track for each drum and a
// Drums bus. To try one drum, delete the // in front of the two lines that end in "drums" and
// save: a Beat lane shows in the timeline, where a double-click adds a clip. Each track needs its
// own key: lowercase letters, digits and hyphens, such as "lead-vox".

import { song, track } from "@m2/core";
import { drums } from "@m2/drums";
import { master, mixer } from "std/mix";

// const beat = track("beat", { name: "Beat" }, ({ lane }) => drums.drum(lane.notes("hits"))); // drums

export default song(() =>
  master(
    mixer({
      // beat: beat.out, // drums
    }),
  ),
);

// The root module: whatever this returns is what you hear.
//
// Add a track with + Track, or write one in code: a track() line like the drums one below,
// and one line for it in the mixer. To try the drums, delete the // in front of the two lines
// that end in "drums" and save: a Drums lane shows in the timeline, where a double-click adds
// a clip. Each track needs its own key: lowercase letters, digits and hyphens, such as
// "lead-vox".

import { song, track } from "@m2/core";
import { kick } from "@m2/synths";
import { master, mixer } from "std/mix";

// const drums = track("drums", { name: "Drums" }, ({ lane }) => kick(lane.notes("hits"))); // drums

export default song(() =>
  master(
    mixer({
      // drums: drums.out, // drums
    }),
  ),
);

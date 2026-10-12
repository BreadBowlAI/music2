// The root module: whatever this returns is what you hear.
//
// This song has no tracks yet, so it plays silence. To add a drum track, right here:
//   1. Delete the // at the start of the two lines that end in "drums" below.
//   2. Save (⌘S). A Drums lane shows in the timeline: double-click it to add a clip,
//      then double-click the clip and draw some hits.
// Add more tracks the same way: a line like the drums one, each with its own key (lowercase
// letters, digits and hyphens, such as "lead-vox"), and one more line in the mixer for each.

import { song, track } from "@m2/core";
import { kick } from "std/instruments";
import { master, mixer } from "std/mix";

// const drums = track("drums", { name: "Drums" }, ({ lane }) => kick(lane.notes("hits"))); // drums

export default song(() =>
  master(
    mixer({
      // drums: drums.out, // drums
    }),
  ),
);

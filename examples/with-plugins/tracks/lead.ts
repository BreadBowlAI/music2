import { plugin, track } from "@m2/core";

export default track("Lead", ({ lane }) => {
  const synth = plugin("clap:org.surge-synth-team.surge-xt", {
    notes: lane.notes("notes"),
    state: "lead", // the patch, saved by the plugin into assets/plugin-state/lead.bin
    a_filter_1_type: 0.05, // "LP 24 dB"; the default patch has the filter off
    a_filter_1_cutoff: lane.control("cutoff", "scalar", 0.6), // automated from the timeline
  });
  const space = plugin("clap:com.airwindows.consolidated", { in: synth, dry_wet: 0.35 });
  return space;
});

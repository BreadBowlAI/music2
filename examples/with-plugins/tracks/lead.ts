import { plugin, track } from "@m2/core";

export default track("lead", { name: "Lead" }, ({ lane }) => {
  const synth = plugin("clap:org.surge-synth-team.surge-xt", {
    notes: lane.notes("notes"),
    key: "lead", // the patch, saved by the plugin into data/lead/lead.state
    a_filter_1_type_1537728286: 0.05, // "LP 24 dB"; the default patch has the filter off
    a_filter_1_cutoff_4092842705: lane.control("cutoff", "scalar", 0.6), // automated from the timeline
  });
  const space = plugin("clap:com.airwindows.consolidated", { in: synth, dry_wet_2944917344: 0.35 });
  return space;
});

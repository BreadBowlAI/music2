// The root module: whatever this returns is what you hear.
// Sixteen bars: an intro, an eight-bar drop, and a breakdown. Every track is one of music2's own
// instruments (@m2/synths, @m2/drums); the sub and the chords duck under the kick.

import { db, song } from "@m2/core";
import { gain } from "std/dsp";
import { compress, duck } from "std/fx";
import { master, mixer } from "std/mix";
import arp from "./tracks/arp";
import bass from "./tracks/bass";
import bell from "./tracks/bell";
import chords from "./tracks/chords";
import drums from "./tracks/drums";
import fx from "./tracks/fx";
import keys from "./tracks/keys";
import kick from "./tracks/kick";
import lead from "./tracks/lead";
import pad from "./tracks/pad";
import perc from "./tracks/perc";
import strings from "./tracks/strings";
import sub from "./tracks/sub";
import snare from "./tracks/snare";
import clap from "./tracks/clap";
import hats from "./tracks/hats";
import toms from "./tracks/toms";
import cymbal from "./tracks/cymbal";
import ride from "./tracks/ride";

export default song(() => {
  const mix = mixer({
    drums: { in: drums.out, db: db(-5) },
    perc: { in: perc.out, db: db(-6) },
    sub: { in: duck(sub.out, kick.env, { depth: db(-6) }), db: db(-5) },
    bass: { in: bass.out, db: db(-6) },
    chords: { in: duck(chords.out, kick.env, { depth: db(-4) }), db: db(-6) },
    lead: { in: lead.out, db: db(-2) },
    arp: { in: arp.out, db: db(-4), pan: -0.25 },
    pad: { in: pad.out, db: db(-6) },
    strings: { in: strings.out, db: db(-6) },
    keys: { in: keys.out, db: db(-6), pan: 0.2 },
    bell: { in: bell.out, db: db(-5), pan: 0.3 },
    fx: { in: fx.out, db: db(-6) },
    kick: { in: kick.out, to: "drums" },
    snare: { in: snare.out, to: "drums" },
    clap: { in: clap.out, to: "drums" },
    hats: { in: hats.out, db: 2, to: "drums" },
    toms: { in: toms.out, to: "drums" },
    cymbal: { in: cymbal.out, to: "drums" },
    ride: { in: ride.out, to: "drums" },
  });
  // Mastered about as loud as pop is (near -11.5 LUFS): a quick glue compressor holds the
  // kick's peaks in, then 7.5 dB more into the master's limiter, which takes up to 4 or 5 dB.
  const glued = compress(mix, { threshold: db(-14), ratio: 4, attack: 0.003, release: 0.1 });
  return master(gain(glued, db(7.5)));
});

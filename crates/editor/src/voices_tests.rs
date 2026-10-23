//! The Device card's line on how an instrument plays (track pm, slice 1): from the poly ops its
//! call makes, in plain words.

use m2_ir::{Alloc, Mode, Steal};
use m2_session::Session;

use super::*;
use crate::Gestures;
use crate::testing::{Fixture, MANIFEST};

fn alloc(mode: Mode, retrigger: bool, steal: Steal) -> Alloc {
    Alloc { mode, retrigger, steal }
}

#[test]
fn the_card_says_how_an_instrument_plays() {
    let poly = Alloc::default();
    for (voices, alloc, glide, says) in [
        (8, poly, Glide::None, "Plays up to 8 notes at once. One more stops the oldest."),
        (8, alloc(Mode::Poly, true, Steal::Quietest), Glide::None, "Plays up to 8 notes at once. One more stops the quietest."),
        (8, alloc(Mode::Poly, false, Steal::None), Glide::None, "Plays up to 8 notes at once. More are not played until one ends."),
        (1, poly, Glide::None, "Plays one note at a time. A new note stops the one sounding."),
        (
            1,
            alloc(Mode::Mono, true, Steal::Oldest),
            Glide::Secs(0.07),
            "Plays one note at a time. Each note starts again, sliding from the last over 70 ms.",
        ),
        (1, alloc(Mode::Mono, false, Steal::Oldest), Glide::None, "Plays one note at a time. Each note starts again."),
        (
            1,
            alloc(Mode::Legato, false, Steal::Oldest),
            Glide::Secs(0.07),
            "Plays one note at a time. A note played while another is held slides to it over 70 ms, without starting again.",
        ),
        (
            1,
            alloc(Mode::Legato, false, Steal::Oldest),
            Glide::Secs(1.5),
            "Plays one note at a time. A note played while another is held slides to it over 1.5 s, without starting again.",
        ),
        (
            1,
            alloc(Mode::Legato, true, Steal::Oldest),
            Glide::Code,
            "Plays one note at a time. A note played while another is held slides to it over the glide set in code, without starting again.",
        ),
        (
            1,
            alloc(Mode::Legato, false, Steal::Oldest),
            Glide::None,
            "Plays one note at a time. A note played while another is held carries on from it, without starting again.",
        ),
    ] {
        assert_eq!(words(voices, alloc, glide), says, "{voices} {alloc:?} {glide:?}");
    }

    // On the cards of a song's calls: std's instruments and polys written in tracks.
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "song.ts",
        "import { kernel, ms, poly, song, track, type Note } from \"@m2/core\";\n\
         import { drums } from \"std/drums\";\nimport { fm } from \"std/fm\";\nimport { mixer } from \"std/mix\";\nimport { synth } from \"std/synth\";\n\
         const tone = (n: Note) => kernel(\"std:sine\", { freq: kernel(\"std:mtof\", { pitch: n.pitch }) });\n\
         const lead = track(\"lead\", ({ lane }) => synth.lead(lane.notes(\"notes\")));\n\
         const pad = track(\"pad\", ({ lane }) => synth.pad(lane.notes(\"notes\")));\n\
         const kit = track(\"kit\", ({ lane }) => drums.kit(lane.notes(\"hits\")));\n\
         const keys = track(\"keys\", ({ lane }) => fm.keys(lane.notes(\"notes\"), { voices: 1 }));\n\
         const solo = track(\"solo\", ({ lane }) => poly(lane.notes(\"notes\"), tone, { mode: \"mono\", glide: ms(70) }));\n\
         const bend = track(\"bend\", ({ lane }) => poly(lane.notes(\"notes\"), tone, { mode: \"legato\", glide: kernel(\"std:scale\", { in: ms(50), by: 2 }) }));\n\
         export default song(() => mixer({ lead: lead.out, pad: pad.out, kit: kit.out, keys: keys.out, solo: solo.out, bend: bend.out }));\n",
    );
    let s: Session = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let shown = |module: &str| -> Vec<String> {
        s.view().modules.iter().filter(|m| m.key == module).flat_map(|m| &m.nodes).filter_map(|n| n.voices.clone()).collect()
    };
    let legato = "Plays one note at a time. A note played while another is held";
    assert_eq!(shown("lead"), [format!("{legato} slides to it over 60 ms, without starting again.")]);
    assert_eq!(shown("pad"), ["Plays up to 16 notes at once. One more stops the oldest."]);
    assert_eq!(shown("kit"), ["Plays up to 16 notes at once. One more stops the oldest."], "the kit's sounding poly, not its hits'");
    assert_eq!(shown("keys"), [format!("{legato} carries on from it, without starting again.")]);
    assert_eq!(shown("solo"), ["Plays one note at a time. Each note starts again, sliding from the last over 70 ms."]);
    assert_eq!(shown("bend"), [format!("{legato} slides to it over the glide set in code, without starting again.")]);
}

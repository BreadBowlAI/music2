//! Where a scope reads (TAP-8): a call's own output at the top of its track, found by the graph
//! view's id for the call, and refused in words for what isn't sound.

use m2_session::Session;

use super::*;
use crate::testing::{Fixture, MANIFEST};

const SONG: &str = "import { poly, song, track } from '@m2/core';\nimport { lfo } from 'std/control';\nimport { osc } from 'std/dsp';\n\
                    import { scale } from 'std/math';\n\
                    const a = track('a', () => scale(osc.sine(220), lfo(1)));\n\
                    const b = track('b', ({ lane }) => poly(lane.notes('n'), (v) => osc.saw(v.pitch)));\n\
                    const c = track('c', () => osc.square(110));\n\
                    export default song(() => scale(a.out, 0.5));\n";

fn session() -> (Fixture, Session) {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write("song.ts", SONG);
    let s = f.open();
    (f, s)
}

/// The view's id for the call of `callee` in `song.ts`.
fn id(s: &Session, callee: &str) -> String {
    let site = s.accepted().compiled.sites.iter().find(|x| x.file == "song.ts" && x.callee == callee).unwrap();
    format!("{}#{}", site.file, site.ordinal)
}

// TAP-8: a call's own output, found by its id in the view: the node the flat graph names for it,
// with its channels.
#[test]
fn a_scope_reads_a_calls_own_output() {
    let (_f, s) = session();
    let rev = s.accepted();
    let flat = &rev.compiled.flat;
    let (at, channels) = point(&rev, &s.packages, "a", &id(&s, "osc.sine")).unwrap();
    let TapAt::Node(i) = at else { panic!("{at:?}") };
    assert!(matches!(&flat.nodes[i].op, FOp::Kernel { kernel, .. } if kernel == "std:sine"), "{:?}", flat.nodes[i]);
    assert_eq!((flat.nodes[i].module.as_str(), channels), ("a", 1));
    let (at, _) = point(&rev, &s.packages, "a", &id(&s, "scale")).unwrap();
    assert!(matches!(at, TapAt::Node(j) if flat.nodes[j].module == "a" && j != i), "the call around it is another box: {at:?}");
    assert!(view_has(&s, "a", &id(&s, "osc.sine")), "the id is the view's");
    // TAP-9: from the node back to the id the view gives its call.
    assert_eq!(id_of(&rev, &s.packages, "a", i), Some(id(&s, "osc.sine")));
    assert_eq!(id_of(&rev, &s.packages, "c", i), None, "not c's");
}

/// Whether the graph view of module `module` has a box with id `node`.
fn view_has(s: &Session, module: &str, node: &str) -> bool {
    crate::view::view(s).modules.iter().any(|m| m.key == module && m.nodes.iter().any(|n| n.id == node))
}

// TAP-8: what isn't one sound at the top of its track is refused in words: a control, a call
// inside an instrument's voices, a box that isn't there, and one of another module.
#[test]
fn a_scope_is_refused_in_words_for_what_isnt_sound() {
    let (_f, s) = session();
    let rev = s.accepted();
    let no = |module: &str, node: &str| point(&rev, &s.packages, module, node).unwrap_err();
    assert_eq!(no("a", &id(&s, "lfo")), "lfo gives a control or notes, not sound: a scope shows sound.");
    assert_eq!(
        no("b", &id(&s, "osc.saw")),
        "osc.saw plays inside an instrument's voices, one for each note: put the scope on the instrument."
    );
    assert_eq!(no("a", "song.ts#999"), "There's no box song.ts#999 in a now, so there's nothing to show.");
    assert_eq!(no("a", "nothing"), "There's no box nothing in a now, so there's nothing to show.");
    let sine = id(&s, "osc.sine");
    assert_eq!(no("c", &sine), format!("There's no box {sine} in c now, so there's nothing to show."), "the sine is a's, not c's");
    assert!(point(&rev, &s.packages, "b", &id(&s, "poly")).is_ok(), "the instrument is one sound");
}

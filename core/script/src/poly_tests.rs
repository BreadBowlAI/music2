//! The `poly` op's options (RENDER-16): `poly(notes, voice, { voices, mode, retrigger, steal, glide })`
//! reach the IR, only where they aren't the default, and a wrong one is said in words (`@m2/core`
//! 1.4.0).

use std::time::Duration;

use m2_ir::{Alloc, Mode, Op, Steal};

use super::*;

fn build_of(song: &str) -> Result<Ir, Vec<Diagnostic>> {
    let files: BTreeMap<String, String> = [("song.ts".to_string(), song.to_string())].into();
    let mut o = Options::new(files, "song.ts");
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build(&o)
}

/// A song of one poly of sines over a notes lane, given `opts`.
fn one_poly(opts: &str) -> Result<Ir, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ song, track, poly, ms, hz, kernel }} from \"@m2/core\";\n\
         const t = track(\"t\", ({{ lane }}) => poly(lane.notes(\"notes\"), (n) => kernel(\"std:sine\", {{ freq: kernel(\"std:mtof\", {{ pitch: n.pitch }}) }}){opts}));\n\
         export default song(() => t.out);\n"
    ))
}

/// The track's poly node: its voices, options and glide, and the node its glide reads.
fn poly_of(ir: &Ir) -> (u32, Alloc, Option<Op>) {
    let m = ir.modules.iter().find(|m| m.key == "t").expect("the track");
    let (voices, alloc, glide) = m
        .nodes
        .iter()
        .find_map(|n| match &n.op {
            Op::Poly { voices, alloc, glide, .. } => Some((*voices, *alloc, glide.clone())),
            _ => None,
        })
        .expect("a poly");
    let glide = glide.map(|g| m.nodes.iter().find(|n| n.id == g).expect("the glide's node").op.clone());
    (voices, alloc, glide)
}

#[test]
fn poly_options_reach_the_ir() {
    let ir = one_poly(", { mode: \"legato\", retrigger: false, glide: ms(70) }").unwrap_or_else(|e| panic!("{e:?}"));
    let (voices, alloc, glide) = poly_of(&ir);
    assert_eq!((voices, alloc), (1, Alloc { mode: Mode::Legato, retrigger: false, steal: Steal::Oldest }));
    let Some(Op::Const { value, unit }) = glide else { panic!("the glide is a constant: {glide:?}") };
    assert!((value - 0.07).abs() < 1e-12 && unit == Some(m2_ir::Unit::Seconds), "{value} {unit:?}");

    let ir = one_poly(", { voices: 4, steal: \"quietest\", retrigger: false }").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(poly_of(&ir), (4, Alloc { mode: Mode::Poly, retrigger: false, steal: Steal::Quietest }, None));
    let ir = one_poly(", { mode: \"mono\", steal: \"none\", glide: 0.05 }").unwrap_or_else(|e| panic!("{e:?}"));
    let (voices, alloc, glide) = poly_of(&ir);
    assert_eq!((voices, alloc), (1, Alloc { mode: Mode::Mono, retrigger: true, steal: Steal::None }));
    assert!(matches!(glide, Some(Op::Const { value: 0.05, unit: None })), "{glide:?}");
    assert_eq!(SDK_VERSION, "1.4.0", "an addition to @m2/core (FMT-2): the poly op's options");
}

#[test]
fn default_options_leave_the_ir_as_today() {
    let today = serde_json::to_string(&one_poly("").unwrap_or_else(|e| panic!("{e:?}"))).unwrap();
    assert!(!today.contains("\"alloc\"") && !today.contains("\"glide\""), "{today}");
    for opts in [", {}", ", { voices: 16 }", ", { voices: 16, mode: \"poly\", retrigger: true, steal: \"oldest\", glide: 0 }"] {
        let ir = one_poly(opts).unwrap_or_else(|e| panic!("{opts}: {e:?}"));
        assert_eq!(serde_json::to_string(&ir).unwrap(), today, "{opts}");
    }
}

#[test]
fn a_bad_poly_option_is_said_in_words() {
    for (bad, says) in [
        (", { mode: \"solo\" }", "poly: mode must be \"poly\", \"mono\" or \"legato\", not \"solo\""),
        (", { steal: \"newest\" }", "poly: steal must be \"oldest\", \"quietest\" or \"none\", not \"newest\""),
        (", { retrigger: 1 }", "poly: retrigger must be true or false, not 1"),
        (", { voice: 4 }", "poly: no option \"voice\" (options: voices, mode, retrigger, steal, glide)"),
        (", { mode: \"mono\", voices: 1 }", "poly: \"mono\" plays one note at a time, so it takes no voices"),
        (", { mode: \"legato\", voices: 4 }", "poly: \"legato\" plays one note at a time, so it takes no voices"),
        (", { voices: 0 }", "poly: voices must be a whole number from 1 to 128, not 0"),
        (", { mode: \"mono\", glide: hz(3) }", "poly: glide is a time, like ms(70), not hz"),
        (", { mode: \"mono\", glide: -1 }", "poly: glide must be 0 or more seconds, not -1"),
    ] {
        let e = one_poly(bad).err().unwrap_or_else(|| panic!("{bad} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{bad}: {e:?}");
    }
}

//! Mute in song as a track option (MON-5): `track("kick", { muted: true }, fn)` reaches the IR as
//! `Module.muted`, only when true.

use std::time::Duration;

use super::*;

fn build_of(song: &str) -> Result<Ir, Vec<Diagnostic>> {
    let files: BTreeMap<String, String> = [("song.ts".to_string(), song.to_string())].into();
    let mut o = Options::new(files, "song.ts");
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build(&o)
}

/// A song of one `kick` track with `opts`.
fn kick(opts: &str) -> Result<Ir, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\n\
         const kick = track(\"kick\", {opts}() => osc.sine(55));\nexport default song(() => kick.out);\n"
    ))
}

#[test]
fn muted_is_a_track_option() {
    let muted = |ir: &Ir| ir.modules.iter().map(|m| (m.key.clone(), m.muted)).collect::<Vec<_>>();
    let on = kick("{ name: \"Kick\", muted: true }, ").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(muted(&on), [("song".into(), false), ("kick".into(), true)]);
    for opts in ["", "{ muted: false }, ", "{ name: \"Kick\" }, "] {
        let off = kick(opts).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(muted(&off).iter().all(|(_, m)| !m), "{opts}");
        // Only when true: an IR without it keeps its bytes (ID-11).
        assert!(!serde_json::to_string(&off).unwrap().contains("muted"), "{opts}");
    }
    assert_eq!(
        SDK_VERSION, "1.5.0",
        "additions to @m2/core (FMT-2): muted in 1.2.0, kernel settings in 1.3.0, poly options in 1.4.0, buses in 1.5.0"
    );
}

#[test]
fn an_unknown_option_is_still_refused() {
    let e = kick("{ mute: true }, ").unwrap_err();
    assert!(e.len() == 1 && e[0].message.contains("unknown option \"mute\" (options: name, muted)"), "{e:?}");
    let e = kick("{ muted: \"yes\" }, ").unwrap_err();
    assert!(e.len() == 1 && e[0].message.contains("option \"muted\" must be a boolean"), "{e:?}");
}

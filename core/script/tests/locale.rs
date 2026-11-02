//! Song code reads numbers as C does, whatever the process's locale (PLAT-4, RENDER-2; review det-1
//! of the platform step). The desktop app on Linux runs in the user's locale (GTK calls
//! `setlocale(LC_ALL, "")`), where German, French or Russian write a decimal comma, and the C
//! library's `strtod`, which QuickJS's `JSON.parse` uses, then read "0.5" as 0: a song's
//! `JSON.parse` built other numbers, and Add effect wrote a delay with zero mix into the song. A test
//! binary of its own, since it changes the whole process's locale.

use std::ffi::CString;
use std::path::{Path, PathBuf};

use m2_script::{Options, build, describe};

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

/// Put the process in a locale with a decimal comma: `de_DE.UTF-8` where the system has it (macOS),
/// else one `localedef` makes in a folder of this test's own (glibc reads it through `LOCPATH`).
fn decimal_comma() {
    let set = || {
        let name = CString::new("de_DE.UTF-8").unwrap();
        // SAFETY: setlocale with a NUL-terminated name, before this test starts any thread.
        !unsafe { libc::setlocale(libc::LC_ALL, name.as_ptr()) }.is_null()
    };
    if set() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("m2-locale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let made = std::process::Command::new("localedef").args(["-i", "de_DE", "-f", "UTF-8"]).arg(dir.join("de_DE.UTF-8")).output();
    // SAFETY: no other thread reads the environment yet.
    unsafe { std::env::set_var("LOCPATH", &dir) };
    let done = set();
    // setlocale has read every category in: the folder goes, so no run leaves one behind.
    let _ = std::fs::remove_dir_all(&dir);
    assert!(done, "no locale with a decimal comma (install glibc's locale sources for localedef): {made:?}");
}

// PLAT-4, RENDER-2: in a locale with a decimal comma, song code's `JSON.parse` and `toFixed` read
// and write numbers as everywhere else, and so do the catalog's descriptors (each parsed from
// JSON), whose defaults Add effect writes into a song.
#[test]
fn song_code_reads_numbers_as_c_does_in_any_locale() {
    decimal_comma();
    let half = CString::new("0.5").unwrap();
    // SAFETY: strtod of a NUL-terminated string.
    assert_eq!(unsafe { libc::strtod(half.as_ptr(), std::ptr::null_mut()) }, 0.0, "the C library now reads a decimal comma");
    let song = r#"import { song, kernel } from "@m2/core";
const parsed = JSON.parse('{"gain": 0.5, "hz": 261.63}');
if (parsed.gain !== 0.5 || parsed.hz !== 261.63) throw new Error(`JSON.parse gives gain ${parsed.gain} and hz ${parsed.hz}`);
const fixed = (0.5).toFixed(2) + " " + (1234.5).toPrecision(5) + " " + (0.25).toExponential(1);
if (fixed !== "0.50 1234.5 2.5e-1") throw new Error(`numbers written as ${fixed}`);
export default song(() => kernel("std:sine", { freq: parsed.hz }));
"#;
    let mut o = Options::new([("song.ts".to_string(), song.to_string())].into(), "song.ts");
    o.packages.push(("std".into(), std_dir()));
    o.dependencies.push("std".into());
    build(&o).unwrap_or_else(|e| panic!("{e:?}"));
    let effects = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/@m2/effects");
    let catalog = describe(&[("std".into(), std_dir()), ("@m2/effects".into(), effects)]);
    let delay = &catalog.components["@m2/effects:delay"];
    let mix = delay.controls.iter().find(|c| c.name == "mix").expect("@m2/effects:delay's mix");
    assert_eq!(mix.default.as_ref().map(|d| d.value), Some(0.3), "{mix:?}");
    assert_eq!(mix.range, Some((0.0, 1.0)));
}

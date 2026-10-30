//! Each rule of the plain words, on a real broken song: first-song with one file changed, opened
//! as the app opens it, so a change in core's or the compiler's words fails here.

use super::*;
use crate::testing::{Fixture, fixture};

/// first-song with each of `files` written, opened.
fn broken(files: &[(&str, &str)]) -> (Fixture, Session) {
    let f = fixture("examples/first-song");
    for (path, text) in files {
        f.write(path, text);
    }
    let s = f.open();
    (f, s)
}

/// `file:line:col`, or a file alone.
fn place(src: Option<&str>) -> (Option<String>, Option<u32>, Option<u32>) {
    let Some(src) = src else { return (None, None, None) };
    let mut parts = src.rsplitn(3, ':');
    match (parts.next().and_then(|c| c.parse().ok()), parts.next().and_then(|l| l.parse().ok()), parts.next()) {
        (Some(col), Some(line), Some(file)) => (Some(file.to_string()), Some(line), Some(col)),
        _ => (Some(src.to_string()), None, None),
    }
}

/// The first problem of `s` that keeps the code from compiling, in plain words, as the server
/// explains it: placed where its `src` says, with that line's text.
fn draft(f: &Fixture, s: &Session) -> (String, Plain) {
    let d = s.draft_errors.first().unwrap_or_else(|| panic!("the code compiles: {:?}", s.problems()));
    explained(f, s, d.code, &d.message, d.src.as_deref(), d.module.as_deref())
}

/// The revision's first problem with `code`, in plain words.
fn problem(f: &Fixture, s: &Session, code: &str) -> (String, Plain) {
    let d = s
        .problems()
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} in {:?} (draft: {:?})", s.problems(), s.draft_errors));
    explained(f, s, d.code, &d.message, d.src.as_deref(), d.module.as_deref())
}

fn explained(f: &Fixture, s: &Session, code: &str, message: &str, src: Option<&str>, module: Option<&str>) -> (String, Plain) {
    let (file, line, col) = place(src);
    let text = file.as_deref().zip(line).and_then(|(file, line)| {
        std::fs::read_to_string(f.0.join(file)).ok().and_then(|t| t.lines().nth(line as usize - 1).map(str::to_string))
    });
    let at = At { file: file.as_deref(), line, col, text: text.as_deref(), module };
    let plain = explain(code, message, &Song::of(s), &at).unwrap_or_else(|| panic!("no plain words for {code}: {message}"));
    (format!("{code} {message} at {src:?}"), plain)
}

fn fix(p: &Plain) -> &str {
    p.fix.as_deref().unwrap_or("")
}

const KEYS: &str = "import { track } from \"@m2/core\";\nimport { pluck } from \"std/instruments\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => pluck(lane.notes(\"notes\")));\n";

#[test]
fn a_unit_mistake_says_what_it_wants() {
    let bass = "import { db, track } from \"@m2/core\";\nimport { bass } from \"std/instruments\";\nimport { lowpass } from \"std/filter\";\n\nexport default track(\"bass\", { name: \"Bass\" }, ({ lane }) => {\n  return lowpass(bass(lane.notes(\"notes\")), db(-3));\n});\n";
    let (f, s) = broken(&[("tracks/bass.ts", bass)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("expects control<hz>, got control<db>") && raw.contains("tracks/bass.ts:6:10"), "{raw}");
    assert_eq!(p.words, "Cutoff wants a frequency (Hz), but gets a level (dB).", "{raw}");
    assert_eq!(fix(&p), "Write it in Hz, such as hz(800).");
}

#[test]
fn an_empty_input() {
    let keys = "import { track } from \"@m2/core\";\nimport { reverb } from \"std/fx\";\n\nexport default track(\"keys\", { name: \"Keys\" }, () => reverb());\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("reverb(): the input in is missing"), "{raw}");
    assert_eq!(p.words, "Reverb needs a sound to work on, but it gets none.", "{raw}");
    assert_eq!(fix(&p), "Give it a sound first, such as `reverb(bass.out)`.");
    // A kernel called without its sound: the graph check finds it, at the call.
    let keys = "import { kernel, track } from \"@m2/core\";\n\nexport default track(\"keys\", { name: \"Keys\" }, () => kernel(\"std:svf\", { cutoff: 100 }).port(\"lp\"));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("input `in` (audio<*>) is not connected"), "{raw}");
    assert_eq!(p.words, "Kernel needs a sound to work on, but it gets none.", "{raw}");
}

#[test]
fn mono_and_stereo() {
    let keys = "import { kernel, track } from \"@m2/core\";\nimport { pluck } from \"std/instruments\";\nimport { pan } from \"std/dsp\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => kernel(\"std:hitcount\", { hits: pan(pluck(lane.notes(\"notes\")), 0.5) }));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("is mono") && raw.contains("but a stereo signal is connected"), "{raw}");
    assert_eq!(p.words, "Kernel takes one channel, but gets two.", "{raw}");
    assert_eq!(fix(&p), "Make it mono first, or use the stereo version.");
}

#[test]
fn a_name_not_known_proposes_its_import() {
    // Misspelt: the nearest name std exports, and its import.
    let keys = "import { track } from \"@m2/core\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => pluk(lane.notes(\"notes\")));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("pluk is not defined"), "{raw}");
    assert_eq!(p.words, "`pluk` isn't known in tracks/keys.ts.");
    assert_eq!(fix(&p), "Did you mean pluck? Import it: `import { pluck } from \"std/instruments\"`.");
    // Spelt right, not imported: its import. A plain function std exports (not a component) too.
    for (name, module) in [("pluck", "std/instruments"), ("lfo", "std/control")] {
        let keys = format!(
            "import {{ track }} from \"@m2/core\";\n\nexport default track(\"keys\", {{ name: \"Keys\" }}, ({{ lane }}) => {name}(lane.notes(\"notes\")));\n"
        );
        let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
        let (raw, p) = draft(&f, &s);
        assert_eq!(fix(&p), format!("Import it: `import {{ {name} }} from \"{module}\"`."), "{raw}");
    }
    // Nothing near: its spelling.
    let keys = KEYS.replace("pluck(lane", "zzzzzz(lane");
    let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
    let (_, p) = draft(&f, &s);
    assert_eq!(fix(&p), "Check its spelling.");
}

#[test]
fn a_typo_names_its_line() {
    let keys = KEYS.replace("notes\")));", "notes\"));");
    let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("Expected"), "{raw}");
    assert_eq!(p.words, "There is a typo on line 4 of tracks/keys.ts.", "{raw}");
    assert_eq!(fix(&p), "Check the brackets, commas and quotes on that line and the one before.");
    // Another of the parser's: a comma left at the end of brackets.
    let keys = KEYS.replace("pluck(lane.notes(\"notes\"))", "(pluck(lane.notes(\"notes\")),)");
    let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("Parenthesized expressions may not have a trailing comma"), "{raw}");
    assert_eq!(p.words, "There is a typo on line 4 of tracks/keys.ts.", "{raw}");
}

#[test]
fn a_missing_file_lists_the_tracks() {
    let f = fixture("examples/first-song");
    f.write("song.ts", f.read("song.ts").replace("./tracks/bass", "./tracks/bas"));
    let s = f.open();
    let (raw, p) = draft(&f, &s);
    assert_eq!(p.words, "song.ts brings in `./tracks/bas`, which isn't there.", "{raw}");
    assert_eq!(fix(&p), "The track files are: bass, hats, keys, kick.");
    // A package's module that isn't there: the package's modules.
    let keys = KEYS.replace("std/instruments", "std/instrument");
    let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
    let (raw, p) = draft(&f, &s);
    assert_eq!(p.words, "tracks/keys.ts brings in `std/instrument`, which isn't there.", "{raw}");
    assert!(fix(&p).starts_with("Check its name: std has ") && fix(&p).contains("std/instruments"), "{}", fix(&p));
}

#[test]
fn not_a_function() {
    let keys = "import { track } from \"@m2/core\";\nconst pluck = 3;\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => pluck(lane.notes(\"notes\")));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.contains("not a function"), "{raw}");
    assert!(p.words.ends_with("on line 4 of tracks/keys.ts is used like an instrument or effect, but it isn't one."), "{}: {raw}", p.words);
    assert_eq!(fix(&p), "Check the name before the brackets: is it spelled right, and imported?");
}

#[test]
fn an_undeclared_package() {
    let keys = "import { track } from \"@m2/core\";\nimport { thing } from \"other/x\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => thing(lane.notes(\"notes\")));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = draft(&f, &s);
    assert_eq!(p.words, "tracks/keys.ts uses other, but music2.toml doesn't list it.", "{raw}");
    assert_eq!(fix(&p), "Add `other = \"…\"` under `[dependencies]` in music2.toml.");
}

#[test]
fn two_tracks_with_one_id() {
    let keys = KEYS.replace("track(\"keys\"", "track(\"kick\"");
    let (f, s) = broken(&[("tracks/keys.ts", &keys)]);
    let (raw, p) = draft(&f, &s);
    assert!(raw.starts_with("key.duplicate"), "{raw}");
    assert_eq!(p.words, "Two tracks have the ID kick.");
    assert_eq!(fix(&p), "Give one another ID: in Code, right-click its file, then Change track ID….");
}

#[test]
fn a_missing_plugin() {
    let keys = "import { plugin, track } from \"@m2/core\";\n\nexport default track(\"keys\", { name: \"Keys\" }, ({ lane }) => plugin(\"clap:org.example.not-installed\", { notes: lane.notes(\"notes\") }));\n";
    let (f, s) = broken(&[("tracks/keys.ts", keys)]);
    let (raw, p) = problem(&f, &s, "plugin.missing");
    assert_eq!(p.words, "Keys plays a plugin that isn't installed on this computer, so it is silent.", "{raw}");
    assert_eq!(fix(&p), "Install it, or pick another instrument: right-click Keys, then Change instrument….");
}

#[test]
fn a_missing_asset() {
    let song = "import { song, track } from \"@m2/core\";\nconst v = track(\"vox\", { name: \"Vox\" }, ({ lane }) => lane.audio(\"takes\"));\nexport default song(() => v.out);\n";
    let takes = r#"{"clips": [{"id": "a", "start": 0, "length": 4, "asset": "vox.wav"}]}"#;
    let f = Fixture::new();
    f.write("music2.toml", crate::testing::MANIFEST);
    f.write("song.ts", song);
    f.write("data/vox/takes.json", takes);
    let s = f.open();
    let (raw, p) = problem(&f, &s, "asset.missing");
    assert_eq!(p.words, "vox.wav isn't in the song's assets folder, so its clip plays nothing.", "{raw}");
    assert_eq!(fix(&p), "Put the file back, or delete the clip.");
}

#[test]
fn a_lane_file_that_doesnt_read() {
    let (f, s) = broken(&[("data/bass/notes.json", "{\"clips\": [")]);
    let (raw, p) = problem(&f, &s, "lane.invalid");
    assert_eq!(p.words, "data/bass/notes.json can't be read, so Bass plays nothing from it.", "{raw}");
    assert_eq!(fix(&p), "Undo the last change to it, or fix the file.");
}

#[test]
fn an_unknown_message_keeps_its_words() {
    let song = Song::default();
    for (code, message) in
        [("code.build", "something new went wrong"), ("code.check", "voices must be 1..128, not 0"), ("arrangement.invalid", "tempo 5 BPM")]
    {
        assert_eq!(explain(code, message, &song, &At::default()), None, "{code}: {message}");
    }
}

// PACK-3: a scoped package's missing module names the whole package, and lists only its modules.
#[test]
fn a_missing_scoped_module_names_its_whole_package() {
    let exports = [("feltPiano", "@alice/felt-piano"), ("feltKeys", "@alice/felt-piano/keys"), ("organ", "@alice/organ")];
    let song =
        Song { tracks: vec![], exports: exports.iter().map(|(n, m)| (n.to_string(), m.to_string())).collect(), names: BTreeMap::new() };
    let p = missing_import("@alice/felt-piano/nope", "tracks/keys.ts", &song).unwrap();
    assert_eq!(p.fix.as_deref(), Some("Check its name: @alice/felt-piano has @alice/felt-piano, @alice/felt-piano/keys."));
}

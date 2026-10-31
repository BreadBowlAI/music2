//! Presets (FMT-22): the file, its name, saving a std sound's settings, and loading them back as
//! one `Values` Tx.

use std::collections::BTreeMap;
use std::path::Path;

use m2_session::Session;

use super::*;
use crate::testing::{Fixture, fixture};

/// A copy of first-song whose Keys track plays `call` (on its `chords` lane), importing `imports`
/// from `@m2/core` and `synth` from `@m2/synths`.
fn keys(imports: &str, call: &str) -> Fixture {
    let f = fixture("examples/first-song");
    f.write(
        "tracks/keys.ts",
        format!(
            "import {{ {imports} }} from \"@m2/core\";\nimport {{ synth }} from \"@m2/synths\";\n\n\
             export default track(\"keys\", {{ name: \"Keys\" }}, ({{ lane }}) => {call});\n"
        ),
    );
    f
}

/// The id of the last call to `callee` in the accepted revision.
fn call(s: &Session, callee: &str) -> String {
    let site =
        s.accepted().compiled.sites.iter().rfind(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no call `{callee}`"));
    format!("{}#{}", site.file, site.ordinal)
}

fn render(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(3.0).unwrap()
}

fn preset(sound: &str, values: &[(&str, f64)]) -> Preset {
    let values = values.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    Preset { name: "Soft".into(), sound: sound.into(), values, plugin_version: None, state: None }
}

/// A folder for a test's presets, removed when dropped.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("m2-presets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Dir(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// FMT-22: a preset is one two-space JSON file, keys in the format's order, that reads back as it
// was written; it is stored by a rename and listed by its name; a newer format is refused in words.
#[test]
fn a_preset_file_round_trips() {
    let mut p = preset("@m2/synths:synth-pluck", &[("brightness", 1800.0), ("decay", 0.6)]);
    let text = p.text();
    assert_eq!(
        text,
        "{\n  \"m2preset\": 1,\n  \"name\": \"Soft\",\n  \"sound\": \"@m2/synths:synth-pluck\",\n  \"values\": {\n    \"brightness\": 1800,\n    \"decay\": 0.6\n  }\n}\n"
    );
    assert_eq!(Preset::parse(&text), Ok(p.clone()));
    p.plugin_version = Some("1.3.4".into());
    p.state = Some("AAEC".into());
    assert_eq!(Preset::parse(&p.text()), Ok(p.clone()), "a plugin's version and state too");

    let newer = text.replace("\"m2preset\": 1", "\"m2preset\": 2");
    assert_eq!(Preset::parse(&newer), Err("This preset was saved by a newer music2. Update music2 to load it.".into()));
    assert_eq!(Preset::parse("[1]"), Err("This file isn't a music2 preset.".into()));

    // Stored in its sound's folder, listed by name; another sound's file there is left out.
    assert_eq!(folder("@m2/synths:synth-pluck"), Path::new("_m2_synths/synth-pluck"));
    assert_eq!(folder("std:sampler"), Path::new("std/sampler"));
    assert_eq!(folder("clap:org.surge-synth-team.surge-xt"), Path::new("clap/org.surge-synth-team.surge-xt"));
    assert_eq!(folder("std:../x y"), Path::new("std/.._x_y"), "never a step up");
    let d = Dir::new("round-trip");
    let p = preset("@m2/synths:synth-pluck", &[("brightness", 1800.0)]);
    let file = store(&d.0, &p, false).unwrap();
    assert_eq!(file, d.0.join("Soft.json"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), p.text());
    std::fs::write(d.0.join("Pad.json"), preset("@m2/synths:synth-pad", &[]).text().replace("Soft", "Pad")).unwrap();
    std::fs::write(d.0.join("notes.json"), "not a preset").unwrap();
    assert_eq!(list(&d.0, "@m2/synths:synth-pluck"), vec![Listed { name: "Soft".into(), file: file.clone() }]);
    assert_eq!(read(&d.0, "@m2/synths:synth-pluck", "Soft"), Ok(p.clone()));

    // A name taken in other capitals is taken; replacing it takes the new capitals.
    let mut louder = p.clone();
    louder.name = "SOFT".into();
    assert_eq!(store(&d.0, &louder, false), Err(NotStored::Taken("Soft".into())));
    store(&d.0, &louder, true).unwrap();
    assert_eq!(list(&d.0, "@m2/synths:synth-pluck").iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["SOFT"]);
    let left: Vec<String> = std::fs::read_dir(&d.0).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n.ends_with(".tmp")), "no temporary file is left: {left:?}");
}

// FMT-22: a preset's name is one line of 1 to 40 characters that is a file name on every OS.
#[test]
fn a_preset_name_is_one_line_of_1_to_40() {
    for good in ["Soft", "My pluck 2", "Ünïcode ☺", &"x".repeat(40)] {
        assert_eq!(name_error(good), None, "{good}");
    }
    for bad in
        ["", &"x".repeat(41), "two\nlines", "a/b", "a\\b", "a:b", "a*", "a?", "a\"b", "<a>", "a|b", ".hidden", " Soft", "Soft ", "Soft."]
    {
        assert!(name_error(bad).is_some(), "{bad:?}");
    }
    let mut p = preset("@m2/synths:synth-pluck", &[]);
    p.name = "a/b".into();
    assert!(Preset::parse(&p.text()).is_err(), "a file with such a name isn't read");
    assert!(matches!(store(Path::new("/nowhere"), &p, false), Err(NotStored::Refused(_))), "nor written");
}

// FMT-22, PKG-3: saving takes every declared control of the call by its slot, in base units: the
// numbers written (a quantity by its value) and the ones it leaves out (their defaults). It writes
// nothing in the song.
#[test]
fn saving_a_std_preset_writes_every_control_of_the_call() {
    let f = keys("khz, track", "synth.pluck(lane.notes(\"chords\"), { brightness: khz(3), spread: 0.7 })");
    let s = f.open();
    let before = f.read("tracks/keys.ts");
    let saved = capture(&s, s.accepted().number, &call(&s, "synth.pluck"), "Mine").unwrap();
    let want: BTreeMap<String, f64> =
        [("brightness", 3000.0), ("decay", 0.45), ("spread", 0.7), ("velSens", 0.5)].map(|(k, v)| (k.to_string(), v)).into();
    assert_eq!(
        saved.preset,
        Preset { name: "Mine".into(), sound: "@m2/synths:synth-pluck".into(), values: want, plugin_version: None, state: None }
    );
    assert_eq!(saved.left, vec![]);
    assert_eq!(f.read("tracks/keys.ts"), before);
    assert!(capture(&s, s.accepted().number, &call(&s, "synth.pluck"), "a/b").is_err(), "a name that won't do");
}

// FMT-22, WRITE-10: loading is one `Values` Tx. A written number is rewritten in its units
// (`khz(0.9)`); left-out controls are written into the call's options, several as one insertion,
// or into new options; a value the call plays already writes nothing; a control the preset
// doesn't name keeps its value. One Undo gives the bytes back, and it sounds as the same values
// written by hand do.
#[test]
fn loading_a_std_preset_is_one_values_tx() {
    let f = keys("khz, track", "synth.pluck(lane.notes(\"chords\"), { brightness: khz(3) })");
    let mut s = f.open();
    let before = f.read("tracks/keys.ts");
    let at = s.accepted().number;
    let soft = preset("@m2/synths:synth-pluck", &[("brightness", 900.0), ("decay", 0.6), ("spread", 0.5)]);
    let node = call(&s, "synth.pluck");
    let loaded = load(&mut s, at, &node, &soft).unwrap();
    let want = before.replace("{ brightness: khz(3) }", "{ brightness: khz(0.9), decay: 0.6, spread: 0.5 }");
    assert_eq!(f.read("tracks/keys.ts"), want);
    assert_eq!(loaded.kept, vec![]);
    assert_eq!((loaded.edited.before.as_str(), loaded.edited.after.as_str()), ("Synth pluck settings", "Synth pluck preset Soft"));
    assert_eq!(loaded.edited.revision, at + 1, "one revision");
    let heard = render(&s);
    assert!(heard.iter().any(|x| x[0] != 0.0), "a render that says something");
    let by_hand = keys("khz, track", "synth.pluck(lane.notes(\"chords\"), { brightness: khz(0.9), decay: 0.6, spread: 0.5 })");
    assert!(heard == render(&by_hand.open()), "it sounds as the same values written by hand");
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read("tracks/keys.ts"), before, "one Undo gives the bytes back");

    // A call with no options gets them; spread is the pad's default already, so isn't written.
    let f = keys("track", "synth.pad(lane.notes(\"chords\"))");
    let mut s = f.open();
    let warm = preset("@m2/synths:synth-pad", &[("cutoff", 1200.0), ("attack", 0.9), ("spread", 0.9)]);
    let (at, node) = (s.accepted().number, call(&s, "synth.pad"));
    load(&mut s, at, &node, &warm).unwrap();
    assert!(
        f.read("tracks/keys.ts").contains("synth.pad(lane.notes(\"chords\"), { cutoff: 1200, attack: 0.9 })"),
        "{}",
        f.read("tracks/keys.ts")
    );
}

// FMT-22, HOME-2: a control a lane drives keeps its lane, and is named; the rest load.
#[test]
fn a_control_a_lane_drives_keeps_its_lane_and_is_named() {
    let f = keys("track", "synth.pluck(lane.notes(\"chords\"), { brightness: lane.control(\"bright\", \"hz\", 3000) })");
    f.write("data/keys/bright.json", m2_song::canonical(&m2_song::LaneData::Control { unit: Some("hz".into()), clips: vec![] }));
    let mut s = f.open();
    let soft = preset("@m2/synths:synth-pluck", &[("brightness", 900.0), ("decay", 0.6)]);
    let saved = capture(&s, s.accepted().number, &call(&s, "synth.pluck"), "Mine").unwrap();
    assert_eq!(saved.left, vec![("brightness".to_string(), Why::Lane)], "saving leaves it out, and names it");
    assert!(!saved.preset.values.contains_key("brightness"));
    let (at, node) = (s.accepted().number, call(&s, "synth.pluck"));
    let loaded = load(&mut s, at, &node, &soft).unwrap();
    assert_eq!(loaded.kept, vec![("brightness".to_string(), Why::Lane)]);
    assert!(
        f.read("tracks/keys.ts").contains("{ brightness: lane.control(\"bright\", \"hz\", 3000), decay: 0.6 }"),
        "{}",
        f.read("tracks/keys.ts")
    );
}

// FMT-22: a preset is for one sound; loading it into another is refused in words, writing nothing.
#[test]
fn a_preset_for_another_sound_is_refused() {
    let f = keys("track", "synth.pluck(lane.notes(\"chords\"))");
    let mut s = f.open();
    let before = f.read("tracks/keys.ts");
    let warm = preset("@m2/synths:synth-pad", &[("cutoff", 1200.0)]);
    let (at, node) = (s.accepted().number, call(&s, "synth.pluck"));
    let e = load(&mut s, at, &node, &warm).unwrap_err();
    assert_eq!(e.to_string(), "This preset is for Synth pad, not Synth pluck.");
    assert_eq!(f.read("tracks/keys.ts"), before);
}

// FMT-22, PACK-15: every factory preset of the packs music2 ships (`presets/<component>/`) is
// canonical, names the pack's sound by its folder, gives only that sound's declared controls, and
// loads into a song that calls the sound: afterwards it saves back as its values.
#[test]
fn every_factory_preset_loads() {
    let mut n = 0;
    let folders = m2_song::pack::test_packs()
        .into_iter()
        .flat_map(|(_, dir)| std::fs::read_dir(dir.join("presets")).into_iter().flatten().map(move |f| (dir.clone(), f.unwrap().path())));
    for (std, folder) in folders {
        for file in std::fs::read_dir(&folder).unwrap() {
            let file = file.unwrap().path();
            let text = std::fs::read_to_string(&file).unwrap();
            let p = Preset::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert_eq!(p.text(), text, "{} is written as music2 writes it", file.display());
            assert_eq!(file.file_stem().unwrap().to_str(), Some(p.name.as_str()), "{}", file.display());
            assert_eq!(factory(&std, &p.sound), Some(folder.clone()), "{} is in its sound's folder", file.display());

            let f = keys("track", "synth.pluck(lane.notes(\"chords\"))");
            // The song lists the pack each sound is from (PACK-10): first-song lists @m2/synths.
            let pack = p.sound.split(':').next().unwrap();
            if pack != "@m2/synths" {
                f.write("music2.toml", f.read("music2.toml") + &format!("\"{pack}\" = \"1\"\n"));
            }
            let s = f.open();
            let desc = s.catalog().components.get(&p.sound).unwrap_or_else(|| panic!("{}: no sound {}", file.display(), p.sound)).clone();
            for k in p.values.keys() {
                assert!(
                    desc.controls.iter().any(|c| &c.name == k && c.each.is_none()),
                    "{}: {} has no control {k}",
                    file.display(),
                    p.sound
                );
            }
            let export = s.catalog().exports[&p.sound][0].clone();
            drop(s);
            let import = export.name.split('.').next().unwrap();
            let played = match desc.inputs.first().map(|i| i.kind) {
                Some(m2_script::component::PortKind::Notes) => format!("{}(lane.notes(\"chords\"))", export.name),
                _ => format!("{}(synth.pluck(lane.notes(\"chords\")))", export.name),
            };
            let more = match export.module.as_str() {
                "@m2/synths" => String::new(),
                module => format!("import {{ {import} }} from \"{module}\";\n"),
            };
            f.write(
                "tracks/keys.ts",
                format!(
                    "import {{ track }} from \"@m2/core\";\nimport {{ synth }} from \"@m2/synths\";\n{more}\n\
                     export default track(\"keys\", {{ name: \"Keys\" }}, ({{ lane }}) => {played});\n"
                ),
            );
            let mut s = f.open();
            let (at, node) = (s.accepted().number, call(&s, &export.name));
            let loaded = load(&mut s, at, &node, &p).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            assert_eq!(loaded.kept, vec![], "{}", file.display());
            let saved = capture(&s, s.accepted().number, &node, "Again").unwrap();
            for (k, v) in &p.values {
                assert_eq!(saved.preset.values.get(k), Some(v), "{}: {k} loaded", file.display());
            }
            n += 1;
        }
    }
    assert_eq!(n, 20, "two each for Pluck, Pad, Bass, Reverb and Delay, and for each of @m2/mixing's five tools");
}

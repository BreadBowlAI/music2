//! Kernel settings (kernel ABI v1): `kernel(id, inputs, settings)` gives a kernel the assets it
//! reads and its values, and a component declares them with `setting.asset()` and
//! `setting.keymap()` (`@m2/core` 1.3.0).

use std::time::Duration;

use m2_ir::Op;

use super::*;

fn build_of(song: &str) -> Result<Build, Vec<Diagnostic>> {
    let files: BTreeMap<String, String> = [("song.ts".to_string(), song.to_string())].into();
    let mut o = Options::new(files, "song.ts");
    o.packages.push(("std".into(), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")));
    o.dependencies.push("std".into());
    o.time_limit = Duration::from_secs(2);
    build_full(&o)
}

/// The settings of the song's one `std:sine` kernel, as the IR holds them.
fn settings_of(b: &Build) -> (Vec<String>, Vec<f64>) {
    let n = b.ir.modules[0].nodes.iter().find_map(|n| match &n.op {
        Op::Kernel { kernel, assets, values, .. } if kernel == "std:sine" => Some((assets.clone(), values.clone())),
        _ => None,
    });
    n.expect("a std:sine kernel")
}

fn one_kernel(settings: &str) -> Result<Build, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ song, kernel }} from \"@m2/core\";\nexport default song(() => kernel(\"std:sine\", {{ freq: 440 }}{settings}));\n"
    ))
}

#[test]
fn a_kernel_takes_its_settings() {
    let b = one_kernel(", { assets: [\"kick.wav\", \"snare.wav\"], values: [60, 2.5] }").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(settings_of(&b), (vec!["kick.wav".to_string(), "snare.wav".to_string()], vec![60.0, 2.5]));
    // None given: the IR keeps its bytes (ID-11).
    for none in ["", ", {}", ", { assets: [], values: [] }"] {
        let b = one_kernel(none).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(settings_of(&b), (vec![], vec![]), "{none}");
        let json = serde_json::to_string(&b.ir).unwrap();
        assert!(!json.contains("\"assets\"") && !json.contains("\"values\""), "{none}");
    }
    for (bad, says) in [
        (", { values: [NaN] }", "value 0 must be a finite number"),
        (", { assets: [3] }", "asset 0 must be a file name in assets/"),
        (", { assets: [\"\"] }", "asset 0 must be a file name in assets/"),
        (", { sample: \"a.wav\" }", "unknown kernel setting \"sample\" (settings: assets, values)"),
    ] {
        let e = one_kernel(bad).err().unwrap_or_else(|| panic!("{bad} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{bad}: {e:?}");
    }
    assert_eq!(SDK_VERSION, "1.3.0", "an addition to @m2/core (FMT-2)");
}

/// A song whose component `player` declares a sample and a keymap, and gives its kernel every
/// asset they name, the keymap's notes as values; called with `call`'s options.
fn player(call: &str) -> Result<Build, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ song, kernel, component, setting, port, signature }} from \"@m2/core\";\n\
         const player = component({{ id: \"player\", signature: signature<() => any>(),\n\
           settings: {{ sample: setting.asset(\"hit.wav\"), zones: setting.keymap() }}, output: port.audio() }},\n\
           (p: any) => kernel(\"std:sine\", {{ freq: 440 }}, {{ assets: [p.sample, ...Object.values(p.zones) as string[]], values: Object.keys(p.zones).map(Number) }}));\n\
         export default song(() => player({call}));\n"
    ))
}

#[test]
fn asset_and_keymap_settings_are_checked() {
    let b = player("{ zones: { 36: \"kick.wav\", 38: \"snare.wav\" } }").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(settings_of(&b), (vec!["hit.wav".into(), "kick.wav".into(), "snare.wav".into()], vec![36.0, 38.0]));
    let d = b.components.iter().find(|d| d.id == "player").expect("its descriptor");
    let kinds: Vec<(&str, component::SettingKind)> = d.settings.iter().map(|s| (s.name.as_str(), s.kind)).collect();
    assert_eq!(kinds, [("sample", component::SettingKind::Asset), ("zones", component::SettingKind::Keymap)]);
    for (bad, says) in [
        ("{ zones: {}, sample: 7 }", "must be a file name in assets/, not 7"),
        ("{ zones: { 128: \"a.wav\" } }", "must map MIDI notes (0 to 127) to file names in assets/"),
        ("{ zones: { 60: 5 } }", "must map MIDI notes (0 to 127) to file names in assets/"),
        ("{ zones: [\"a.wav\"] }", "must map MIDI notes (0 to 127) to file names in assets/"),
    ] {
        let e = player(bad).err().unwrap_or_else(|| panic!("{bad} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{bad}: {e:?}");
    }
}

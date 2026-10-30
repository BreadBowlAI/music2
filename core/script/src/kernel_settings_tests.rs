//! Kernel settings (kernel ABI v1): `kernel(id, inputs, settings)` gives a kernel the assets it
//! reads and its values, and a component declares them with `setting.asset()`,
//! `setting.keymap()` (`@m2/core` 1.3.0) and `setting.regions()` (1.6.0).

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
    assert_eq!(
        SDK_VERSION, "1.6.0",
        "additions to @m2/core (FMT-2): kernel settings in 1.3.0, the poly op's options in 1.4.0, buses in 1.5.0, regions in 1.6.0"
    );
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

/// A song whose component `player` declares regions and gives its kernel each region's file and
/// root (or -1); called with `call`'s options.
fn regions_player(call: &str) -> Result<Build, Vec<Diagnostic>> {
    build_of(&format!(
        "import {{ song, kernel, component, setting, port, signature }} from \"@m2/core\";\n\
         const player = component({{ id: \"player\", signature: signature<() => any>(),\n\
           settings: {{ regions: setting.regions() }}, output: port.audio() }},\n\
           (p: any) => kernel(\"std:sine\", {{ freq: 440 }}, {{ assets: p.regions.map((r: any) => r.file), values: p.regions.map((r: any) => r.root ?? -1) }}));\n\
         export default song(() => player({call}));\n"
    ))
}

// PKG-15: `setting.regions()` takes a list of regions, each field checked, and refuses anything
// else in words that name the region by its place from 1.
#[test]
fn regions_settings_are_checked() {
    let full = "{ file: \"b.wav\", root: 36, keys: [30, 40], velocity: [1, 64], round_robin: [2, 2], trigger: \"release\", \
                loop: [10, 20], tune: -50, volume: -3.5, pan: 0.5, offset: 100 }";
    let b = regions_player(&format!("{{ regions: [{{ file: \"a.wav\" }}, {full}] }}")).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(settings_of(&b), (vec!["a.wav".into(), "b.wav".into()], vec![-1.0, 36.0]));
    let d = b.components.iter().find(|d| d.id == "player").expect("its descriptor");
    let s = &d.settings[0];
    assert_eq!((s.name.as_str(), s.kind, s.default.clone()), ("regions", component::SettingKind::Regions, Some(serde_json::json!([]))));
    // Left out, none; and none is a list too.
    for call in ["{}", "{ regions: [] }"] {
        let b = regions_player(call).unwrap_or_else(|e| panic!("{call}: {e:?}"));
        assert_eq!(settings_of(&b), (vec![], vec![]), "{call}");
    }
    let ok = "{ file: \"a.wav\" }, { file: \"b.wav\" }";
    for (region, says) in [
        ("7", "region 3: must be an object such as { file: \"C4.wav\", root: 60 }, not 7"),
        ("{ root: 60 }", "region 3: needs a file, as file: \"C4.wav\""),
        ("{ file: \"../x.wav\" }", "region 3: file must be a file name in assets/, not \"../x.wav\""),
        ("{ file: \"a.wav\", lokey: 3 }", "region 3: unknown field \"lokey\" (fields: file, root, keys, velocity, round_robin, trigger, loop, tune, volume, pan, offset)"),
        ("{ file: \"a.wav\", root: 128 }", "region 3: root must be a MIDI note, a whole number from 0 to 127, not 128"),
        ("{ file: \"a.wav\", root: 60.5 }", "region 3: root must be a MIDI note"),
        ("{ file: \"a.wav\", keys: [70, 60] }", "region 3: keys 70 to 60: the lowest is above the highest"),
        ("{ file: \"a.wav\", keys: [60] }", "region 3: keys must be [lowest, highest], two MIDI notes from 0 to 127, not [60]"),
        ("{ file: \"a.wav\", keys: [0, 128] }", "region 3: keys must be [lowest, highest]"),
        ("{ file: \"a.wav\", velocity: [0, 127] }", "region 3: velocity must be [lowest, highest], two whole numbers from 1 to 127, not [0, 127]"),
        ("{ file: \"a.wav\", velocity: [100, 20] }", "region 3: velocity 100 to 20: the lowest is above the highest"),
        ("{ file: \"a.wav\", round_robin: [3, 2] }", "region 3: round_robin [3, 2]: the position is above the number of takes"),
        ("{ file: \"a.wav\", round_robin: [0, 2] }", "region 3: round_robin must be [position, of], two whole numbers from 1 to 128 (take 2 of 3 is [2, 3])"),
        ("{ file: \"a.wav\", round_robin: 2 }", "region 3: round_robin must be [position, of]"),
        ("{ file: \"a.wav\", trigger: \"on\" }", "region 3: trigger must be \"attack\" or \"release\", not \"on\""),
        ("{ file: \"a.wav\", loop: [500, 100] }", "region 3: loop 500 to 100: the start must be before the end"),
        ("{ file: \"a.wav\", loop: [5, 5] }", "region 3: loop 5 to 5: the start must be before the end"),
        ("{ file: \"a.wav\", loop: [-1, 5] }", "region 3: loop must be [start, end], two frames of the file, whole numbers from 0, not [-1, 5]"),
        ("{ file: \"a.wav\", tune: 3000 }", "region 3: tune must be cents, a number from -2400 to 2400, not 3000"),
        ("{ file: \"a.wav\", volume: 30 }", "region 3: volume must be dB, a number from -120 to 24, not 30"),
        ("{ file: \"a.wav\", pan: -2 }", "region 3: pan must be a number from -1 (left) to 1 (right), not -2"),
        ("{ file: \"a.wav\", offset: 1.5 }", "region 3: offset must be a whole number of frames from 0, not 1.5"),
        ("{ file: \"a.wav\", offset: -1 }", "region 3: offset must be a whole number of frames from 0, not -1"),
    ] {
        let call = format!("{{ regions: [{ok}, {region}] }}");
        let e = regions_player(&call).err().unwrap_or_else(|| panic!("{call} builds"));
        let want = format!("player(): regions: {says}");
        assert!(e.iter().any(|d| d.message.contains(&want)), "{call}: wanted {want:?} in {e:?}");
    }
    for (call, says) in [
        ("{ regions: { file: \"a.wav\" } }", "player(): regions: must be a list of regions, as [{ file: \"C4.wav\", root: 60 }], not an object"),
        ("{ regions: \"a.wav\" }", "player(): regions: must be a list of regions"),
    ] {
        let e = regions_player(call).err().unwrap_or_else(|| panic!("{call} builds"));
        assert!(e.iter().any(|d| d.message.contains(says)), "{call}: {e:?}");
    }
}

/// A kernel's assets and values.
type Settings = (Vec<String>, Vec<f64>);

/// The `std:sampler` kernel `sampler(<notes><call>)` makes in its voices: its assets and values.
fn sampler_of(call: &str) -> Result<(Build, Settings), Vec<Diagnostic>> {
    let b = build_of(&format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ sampler }} from \"std/sampler\";\n\
         const s = track(\"s\", ({{ lane }}) => sampler(lane.notes(\"notes\"){call}));\nexport default song(() => s.out);\n"
    ))?;
    let voice = b.ir.modules.iter().flat_map(|m| &m.nodes).find_map(|n| match &n.op {
        Op::Poly { body, .. } => body.nodes.iter().find_map(|n| match &n.op {
            Op::Kernel { kernel, assets, values, .. } if kernel == "std:sampler" => Some((assets.clone(), values.clone())),
            _ => None,
        }),
        _ => None,
    });
    let got = voice.expect("a std:sampler kernel in a voice");
    Ok((b, got))
}

// std 0.2.1's sampler: its file and root, or its zones, reach its kernel as assets and values (a
// root, a lowest and a highest note each). With no file it builds, from its notes alone, so + Track
// can add it: `setting.asset().optional()`.
#[test]
fn the_sampler_gives_its_kernel_its_files() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    for (call, assets, values) in [
        ("", s(&[]), vec![]),
        (", { sample: \"kick.wav\" }", s(&["kick.wav"]), vec![60.0, 0.0, 127.0]),
        (", { sample: \"bass.wav\", root: 36 }", s(&["bass.wav"]), vec![36.0, 0.0, 127.0]),
        (
            ", { sample: \"x.wav\", zones: { 38: \"snare.wav\", 36: \"kick.wav\", 42: \"hat.wav\" } }",
            s(&["kick.wav", "snare.wav", "hat.wav"]),
            vec![36.0, 0.0, 37.0, 38.0, 38.0, 41.0, 42.0, 42.0, 127.0],
        ),
    ] {
        let (_, got) = sampler_of(call).unwrap_or_else(|e| panic!("{call}: {e:?}"));
        assert_eq!(got, (assets, values), "{call}");
    }
    let (b, _) = sampler_of("").unwrap();
    let d = b.components.iter().find(|d| d.id == "std:sampler").expect("its descriptor");
    assert!(d.is_instrument() && d.editor.category.as_deref() == Some("Sampler"), "+ Track offers it");
    let sample = d.settings.iter().find(|s| s.name == "sample").unwrap();
    assert!(sample.optional && sample.default.is_none() && sample.kind == component::SettingKind::Asset);
    let e = sampler_of(", { sample: \"../song.ts\" }").err().expect("a bad name is refused");
    assert!(e.iter().any(|d| d.message.contains("must be a file name in assets/")), "{e:?}");
}

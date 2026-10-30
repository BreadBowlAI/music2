use super::*;

const PIANO: &str = r#"# A felt piano.
label = "Felt Piano"
category = "Keys"
description = "Soft, close"
tags = ["piano", "soft"]
engine = "std:sampler"
release = 0.4
velocity = 0.7
mode = "gate"
voices = 16

[[region]]
file = "samples/C4.wav"
root = 60
keys = [58, 62]
velocity = [1, 64]

[[region]]
# the loud one
velocity = [65, 127]
file = "samples/C4-loud.wav"
root = 60
trigger = "attack"
loop = [100, 2000]
tune = -3.5
volume = -2
pan = 0.25
offset = 12
round_robin = [1, 1]
"#;

fn err(text: &str) -> Problem {
    DataInstrument::read(text).expect_err("refused")
}

#[test]
fn a_data_instrument_is_read_whole() {
    let i = DataInstrument::read(PIANO).unwrap();
    assert_eq!((i.label.as_str(), i.category.as_str(), i.description.as_str()), ("Felt Piano", "Keys", "Soft, close"));
    assert_eq!(i.tags, ["piano", "soft"]);
    assert_eq!(i.engine, "std:sampler");
    assert_eq!(i.controls, [("release", 0.4), ("velocity", 0.7)]);
    assert_eq!((i.mode.as_deref(), i.voices), (Some("gate"), Some(16.0)));
    assert_eq!(i.regions.len(), 2);
    assert_eq!(i.regions[0].line, 12, "a region is at its [[region]] line");
    assert_eq!(i.regions[1].line, 18);
    let names: Vec<&str> = i.regions[1].fields.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        names,
        ["file", "root", "velocity", "round_robin", "trigger", "loop", "tune", "volume", "pan", "offset"],
        "in PKG-15's order"
    );
    assert_eq!(i.regions[0].fields[0], ("file", Field::Text("samples/C4.wav".into())));
    assert_eq!(i.regions[0].fields[2], ("keys", Field::Numbers(vec![58.0, 62.0])));
}

#[test]
fn regions_on_one_line_each_are_read_too() {
    let text = "label = \"P\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\nregion = [\n  { file = \"a.wav\", root = 60 },\n  { file = \"b.wav\", root = 62 },\n]\n";
    let i = DataInstrument::read(text).unwrap();
    assert_eq!(i.regions.iter().map(|r| r.line).collect::<Vec<_>>(), [5, 6]);
}

#[test]
fn an_unknown_key_names_the_nearest_known_one_at_its_line() {
    let p = err("lable = \"P\"\ncategory = \"Keys\"\n");
    assert_eq!((p.line, p.col), (1, 1));
    assert_eq!(p.message, "`lable`: no such key; did you mean `label`?");
    let p = err(&PIANO.replace("root = 60\nkeys", "rot = 60\nkeys"));
    assert_eq!((p.line, p.col), (14, 1));
    assert_eq!(p.message, "`rot` in region 1: no such key; did you mean `root`?");
    let p = err(&PIANO.replace("release = 0.4", "relase = 0.4"));
    assert!(p.message.contains("did you mean `release`?"), "{}", p.message);
    // Nothing near: every key, so the right one can be found.
    let p = err(&PIANO.replace("tags =", "colour ="));
    assert!(p.message.starts_with("`colour`: no such key (keys: label, category, description, tags, engine, tune"), "{}", p.message);
}

#[test]
fn a_value_of_the_wrong_kind_is_named_at_its_line() {
    let p = err(&PIANO.replace("voices = 16", "voices = \"16\""));
    assert_eq!((p.line, p.col), (10, 10));
    assert_eq!(p.message, "`voices` must be a number, not the text \"16\"");
    let p = err(&PIANO.replace("keys = [58, 62]", "keys = 58"));
    assert_eq!(p.line, 15);
    assert_eq!(p.message, "region 1: `keys` must be two numbers, as [60, 64], not an integer");
    let p = err(&PIANO.replace("label = \"Felt Piano\"", "label = 3"));
    assert_eq!(p.message, "`label` must be text in quotes, not an integer");
    let p = err(&PIANO.replace("tags = [\"piano\", \"soft\"]", "tags = [1]"));
    assert!(p.message.starts_with("`tags` must be a list of words"), "{}", p.message);
    let p = err(&PIANO.replace("trigger = \"attack\"", "trigger = \"first\""));
    assert_eq!((p.line, p.message.as_str()), (23, "region 2: `trigger = \"first\"` must be \"attack\" or \"release\""));
    let p = err(&PIANO.replace("mode = \"gate\"", "mode = \"loop\""));
    assert_eq!((p.line, p.message.as_str()), (9, "`mode = \"loop\"` must be \"oneshot\" or \"gate\""));
}

#[test]
fn only_stds_sampler_plays_data() {
    let p = err(&PIANO.replace("std:sampler", "@alice/grains:grain"));
    assert_eq!(p.line, 6);
    assert!(p.message.contains("only with std's Sampler for now"), "{}", p.message);
}

#[test]
fn a_file_is_a_plain_path_in_the_pack() {
    for bad in ["../C4.wav", "/C4.wav", "@alice/other:C4.wav", "samples//C4.wav", ".hidden/C4.wav", "samples\\\\C4.wav", ""] {
        let p = err(&PIANO.replace("\"samples/C4.wav\"", &format!("\"{bad}\"")));
        assert_eq!(p.line, 13, "{bad}");
        assert!(p.message.contains("must be a path in this pack"), "{bad}: {}", p.message);
    }
    let p = err(&PIANO.replace("file = \"samples/C4.wav\"\n", ""));
    assert_eq!((p.line, p.message.as_str()), (12, "region 1: no `file`: name its recording, as `file = \"samples/C4.wav\"`"));
}

#[test]
fn what_must_be_there_is_asked_for() {
    assert_eq!(err(&PIANO.replace("label = \"Felt Piano\"\n", "")).message, "`label` is missing: add `label = \"Felt Piano\"` at the top");
    assert_eq!(err(&PIANO.replace("category = \"Keys\"\n", "")).message, "`category` is missing: add `category = \"Keys\"` at the top");
    assert_eq!(
        err(&PIANO.replace("engine = \"std:sampler\"\n", "")).message,
        "`engine` is missing: add `engine = \"std:sampler\"` at the top"
    );
    assert_eq!(err(&PIANO.replace("label = \"Felt Piano\"", "label = \" \"")).message, "`label` must not be empty");
    let none = err("label = \"P\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\n");
    assert!(none.message.starts_with("no `[[region]]`"), "{}", none.message);
}

#[test]
fn a_toml_mistake_is_at_its_line() {
    let p = err("label = \"P\"\ncategory = Keys\n");
    assert_eq!(p.line, 2);
    let p = err(&PIANO.replace("pan = 0.25", "pan = 0.25\npan = 0.5"));
    assert_eq!(p.line, 28, "a key twice: {}", p.message);
}

#[test]
fn the_code_is_laid_out_on_the_files_lines() {
    let i = DataInstrument::read(PIANO).unwrap();
    let code = i.module("@alice/felt-piano", "felt-piano");
    assert_eq!(code, i.module("@alice/felt-piano", "felt-piano"), "a function of the file alone");
    let lines: Vec<&str> = code.lines().collect();
    assert!(lines[0].starts_with("export const feltPiano = component({ id: \"felt-piano\", label: \"Felt Piano\","), "{}", lines[0]);
    assert!(lines[0].contains("controls: { tune: control.semis(0).range(-24, 24), level: control.db(0).range(-24, 12), attack: control.secs(0).range(0, 2), release: control.secs(0.4).range(0, 5), velocity: control.frac(0.7).range(0, 1) }"), "{}", lines[0]);
    assert!(
        lines[0].contains("settings: { mode: setting.choice([\"oneshot\", \"gate\"], \"gate\"), voices: setting.int(16) }"),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("editor: { category: \"Keys\", description: \"Soft, close\" }"), "{}", lines[0]);
    assert_eq!(lines[11], " { file: \"@alice/felt-piano:samples/C4.wav\", root: 60, keys: [58, 62], velocity: [1, 64] },");
    assert_eq!(
        lines[17],
        " { file: \"@alice/felt-piano:samples/C4-loud.wav\", root: 60, velocity: [65, 127], round_robin: [1, 1], trigger: \"attack\", loop: [100, 2000], tune: -3.5, volume: -2, pan: 0.25, offset: 12 },"
    );
    assert!(lines[1..11].iter().all(|l| l.is_empty()));
    assert!(lines.last().unwrap().starts_with("]; import {"));
    // Left out, the Sampler's own defaults.
    let plain =
        DataInstrument::read("label = \"P\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\n[[region]]\nfile = \"a.wav\"\n").unwrap();
    let code = plain.module("@a/b", "808-kit");
    assert!(code.starts_with("export const _808Kit = component("), "{code}");
    assert!(code.contains("release: control.secs(0.05)") && code.contains("\"oneshot\"), voices: setting.int(8)"), "{code}");
}

#[test]
fn bindings_are_camel_case() {
    assert_eq!(binding("grand-piano"), "grandPiano");
    assert_eq!(binding("piano"), "piano");
    assert_eq!(binding("808-kit"), "_808Kit");
    assert_eq!(binding("a-2b"), "a2b");
}

#[test]
fn the_nearest_word_is_a_slip_away() {
    assert_eq!(nearest("lable", ["label", "level"]), Some("label"));
    assert_eq!(nearest("relase", ["release", "region"]), Some("release"));
    assert_eq!(nearest("colour", ["label", "level"]), None);
    assert_eq!(nearest("fiel", ["file", "pan"]), Some("file"));
}

#[test]
fn code_editors_see_each_instrument_in_one_module() {
    let t = types(&[("felt-piano".into(), PIANO.into()), ("broken".into(), "label = 3".into()), ("808-kit".into(), PIANO.into())]);
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines.len(), 5, "a comment, two imports and the two that read: {t}");
    assert!(lines[3].starts_with("export const feltPiano = component({ id: \"felt-piano\""), "{t}");
    assert!(lines[3].ends_with("sampler(notes, { regions: [], tune: p.tune, level: p.level, attack: p.attack, release: p.release, velocity: p.velocity, mode: p.mode, voices: p.voices }));"), "{t}");
    assert!(lines[4].starts_with("export const _808Kit = component("), "{t}");
}

//! `m2 set` and `m2 graph` on a card's settings (CARD-1, CARD-4): a word is read as the setting's
//! kind needs it, a wrong id says so, and `m2 graph` says what the card says.
//!
//! Debug builds only: one song plays the probe plugin, which only a debug `m2` loads (`M2_PROBE`).
#![cfg(debug_assertions)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn put(dir: &Path, name: &str, text: &str) {
    let p = dir.join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// `m2 <args>`, the probe plugin playable, with no audio device and no MIDI one.
fn m2(args: &[&str]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_m2"));
    c.env("HOME", m2_song::pack::test_home());
    c.args(args).env("M2_PROBE", "1").env("M2_AUDIO", "none").env("M2_MIDI", "none");
    c.output().unwrap()
}

fn said(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

const SONG: &str = r#"import { component, plugin, port, setting, song } from "@m2/core";
import { lfo } from "std/control";
import { gain, osc, vca } from "std/dsp";
import { master, mixer } from "std/mix";

const box = component(
  { id: "box", label: "Box", inputs: { in: port.audio() }, settings: { steps: setting.choice(["1", "2"], "1"), fast: setting.bool(false) }, output: port.audio() },
  (x) => gain(x, 0.5),
);

export default song(({ time }) =>
  master(mixer({ a: box(vca(osc.saw(110), lfo(time.beats, { shape: `tri` }))), b: plugin("clap:m2.probe", { in: osc.saw(220), mode_3: 1 }) })),
);
"#;

// CARD-4: `m2 set` reads a word as the setting's kind needs it: a choice's option `2` is the word
// "2", not the number 2; a switch takes true or false, or its options' names (On), and refuses
// another word in words that say what it takes; an id no setting or knob has says so. `m2 graph` prints what the card shows: a channel's `to`
// left out as Master, the code that works a value out once in backticks, and a plugin's stepped
// parameter as the menu it is.
#[test]
fn m2_set_reads_a_word_as_its_setting_takes_it() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-settings-cli-{}", std::process::id())));
    let song = temp.0.join("song");
    put(
        &song,
        "music2.toml",
        "[project]\nname = \"s\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.8\"\n",
    );
    put(&song, "song.ts", SONG);
    let dir = song.to_str().unwrap();
    let graph = m2(&["graph", dir, "--json"]);
    assert!(graph.status.success(), "{}", said(&graph));
    let view: serde_json::Value = serde_json::from_slice(&graph.stdout).unwrap();
    let settings: Vec<&serde_json::Value> = view["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap())
        .flat_map(|n| n["settings"].as_array().into_iter().flatten())
        .collect();
    let id =
        |slot: &str| settings.iter().find(|x| x["slot"] == slot).unwrap_or_else(|| panic!("no {slot}"))["id"].as_str().unwrap().to_string();

    let o = m2(&["set", dir, &id("steps"), "2"]);
    assert!(o.status.success(), "{}", said(&o));
    assert!(
        std::fs::read_to_string(song.join("song.ts"))
            .unwrap()
            .contains(r#"box(vca(osc.saw(110), lfo(time.beats, { shape: `tri` })), { steps: "2" })"#),
        "{}",
        said(&o)
    );
    let o = m2(&["set", dir, &id("fast"), "yes"]);
    assert!(!o.status.success() && said(&o).contains(r#"fast is true or false, not "yes""#), "{}", said(&o));
    let o = m2(&["set", dir, &id("fast"), "on"]);
    assert!(o.status.success(), "a switch's On, by its name: {}", said(&o));
    assert!(std::fs::read_to_string(song.join("song.ts")).unwrap().contains(r#"{ steps: "2", fast: true }"#), "{}", said(&o));
    let o = m2(&["set", dir, "song.ts#99/@shape", "saw"]);
    assert!(!o.status.success() && said(&o).contains("no control or setting `song.ts#99/@shape`"), "{}", said(&o));

    let o = m2(&["graph", dir]);
    let text = said(&o);
    assert!(o.status.success(), "{text}");
    let line =
        |word: &str| text.lines().find(|l| l.trim_start().starts_with(word)).unwrap_or_else(|| panic!("no {word} in {text}")).to_string();
    assert!(line("a.to").contains("Master"), "{}", line("a.to"));
    assert!(line("shape").contains("`tri`") && !line("shape").contains("``"), "{}", line("shape"));
    assert!(line("mode_3").contains("choice: Soft | Warm | Hard") && line("mode_3").contains("Warm"), "{}", line("mode_3"));
}

const TONE: &str = r#"import { component, control, port, song, view } from "@m2/core";
import { osc } from "std/dsp";

const tone = component(
  {
    id: "tone",
    label: "Tone",
    controls: { shape: control.choice(["saw", "square", "sine"], "saw"), sub: control.bool(false), attack: control.secs(0.01), decay: control.secs(0.1), sustain: control.frac(0.5), release: control.secs(0.2) },
    output: port.audio(),
    editor: { views: { amp: view.envelope("attack", "decay", "sustain", "release") }, groups: { Sound: ["shape", "sub"], Amp: ["amp", "attack"] } },
  },
  () => osc.saw(110),
);

export default song(() => tone({ shape: "saw" }));
"#;

// CARD-21: `m2 set` writes a choice control by its name, and a switch by true or false or its
// option's name (On), as the card does; a name the choice doesn't have is refused in words, and a
// number too. CARD-20, CARD-25: `m2 graph` prints the knob's option by its name, and the card's
// groups as headings over their rows, with More last.
#[test]
fn m2_set_writes_a_choice_by_its_name() {
    let temp = Temp(std::env::temp_dir().join(format!("m2-choices-cli-{}", std::process::id())));
    let song = temp.0.join("song");
    put(
        &song,
        "music2.toml",
        "[project]\nname = \"s\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.7\"\n",
    );
    put(&song, "song.ts", TONE);
    let dir = song.to_str().unwrap();
    let graph = m2(&["graph", dir, "--json"]);
    assert!(graph.status.success(), "{}", said(&graph));
    let view: serde_json::Value = serde_json::from_slice(&graph.stdout).unwrap();
    let knobs: Vec<&serde_json::Value> = view["modules"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["nodes"].as_array().unwrap())
        .flat_map(|n| n["knobs"].as_array().unwrap())
        .collect();
    let id =
        |slot: &str| knobs.iter().find(|k| k["slot"] == slot).unwrap_or_else(|| panic!("no {slot}"))["id"].as_str().unwrap().to_string();
    let read = || std::fs::read_to_string(song.join("song.ts")).unwrap();

    let o = m2(&["set", dir, &id("shape"), "square"]);
    assert!(o.status.success(), "{}", said(&o));
    assert_eq!(read(), TONE.replace("tone({ shape: \"saw\" })", "tone({ shape: \"square\" })"), "{}", said(&o));
    let o = m2(&["set", dir, &id("sub"), "On"]);
    assert!(o.status.success(), "a switch's On, by its name: {}", said(&o));
    assert!(read().contains("tone({ shape: \"square\", sub: true })"), "{}", said(&o));
    let o = m2(&["set", dir, &id("shape"), "squar"]);
    assert!(!o.status.success() && said(&o).contains(r#"shape is one of "saw", "square" or "sine", not "squar""#), "{}", said(&o));
    let o = m2(&["set", dir, &id("shape"), "2"]);
    assert!(!o.status.success() && said(&o).contains("shape is one of \"saw\", \"square\" or \"sine\", not 2\n"), "{}", said(&o));
    assert!(read().contains("tone({ shape: \"square\", sub: true })"), "nothing more written");

    let o = m2(&["graph", dir]);
    let text = said(&o);
    assert!(o.status.success(), "{text}");
    let at = |word: &str| text.lines().position(|l| l.trim() == word).unwrap_or_else(|| panic!("no heading {word} in {text}"));
    let line = |word: &str| text.lines().position(|l| l.trim_start().starts_with(word)).unwrap_or_else(|| panic!("no {word} in {text}"));
    assert!(
        at("Sound") < line("shape") && line("shape") < at("Amp") && at("Amp") < line("attack") && line("attack") < at("More"),
        "{text}"
    );
    assert!(at("More") < line("decay") && at("More") < line("sustain"), "{text}");
    let shape = text.lines().nth(line("shape")).unwrap();
    assert!(shape.contains("choice: saw | square | sine (square)"), "{shape}");
}

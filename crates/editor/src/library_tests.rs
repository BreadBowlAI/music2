//! The library and its one search (LIB-1).

use std::path::Path;

use super::*;
use crate::presets::{Preset, store};

/// An item made by hand: the search's rules don't care where it came from.
fn item(kind: Kind, id: &str, label: &str, tags: &[&str], description: &str, package: &str) -> Item {
    Item {
        kind,
        id: id.into(),
        label: label.into(),
        category: "Instruments".into(),
        description: description.into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        package: package.into(),
        component: id.into(),
        instrument: kind == Kind::Instrument,
        preview: true,
        preset: None,
        sample: None,
    }
}

fn sample(id: &str, label: &str, kind: SampleType, bpm: Option<f64>, key: Option<&str>) -> Item {
    let mut i = item(Kind::Sample, id, label, &[], "", "@m2/vocals");
    i.sample = Some(Sample { kind, bpm, key: key.map(|k| Key::parse(k).unwrap()), seconds: Some(2.0) });
    i
}

/// The ids `text` finds in `items`, in order.
fn found(items: &[Item], text: &str) -> Vec<String> {
    search(items, &Query::parse(text)).into_iter().map(|i| i.id.clone()).collect()
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    crate::testing::files_under(dir)
}

fn std_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

// LIB-1: every word typed must match somewhere, in any capitals: a word of the label, inside the
// label, a tag, a word of the description. One that matches nothing leaves the item out.
#[test]
fn every_word_must_match_somewhere_in_any_capitals() {
    let items = [
        item(Kind::Instrument, "a", "Warm pad", &[], "", "std"),
        item(Kind::Instrument, "b", "Pad", &["warm"], "", "std"),
        item(Kind::Instrument, "c", "Pad", &[], "A warm, slow pad", "std"),
        item(Kind::Instrument, "d", "Bright pad", &[], "", "std"),
    ];
    assert_eq!(found(&items, "WARM pad"), ["a", "b", "c"]);
    assert_eq!(found(&items, "warm pad bright"), Vec::<String>::new());
    assert_eq!(found(&items, "ARM"), ["a"], "inside the label, but only the start of a tag's or the description's words");
    assert_eq!(found(&items, "   "), ["d", "b", "c", "a"], "no words: every item, by label");
}

// LIB-1: ranked by where the words match: the whole label, a label that starts with them, then by
// each item's weakest word: a word of the label, inside the label, a tag, the description.
#[test]
fn the_label_ranks_above_tags_and_tags_above_the_description() {
    let items = [
        item(Kind::Instrument, "desc", "Alpha", &[], "a reverb in the description", "std"),
        item(Kind::Instrument, "tag", "Beta", &["reverb"], "", "std"),
        item(Kind::Instrument, "inside", "Springreverb", &[], "", "std"),
        item(Kind::Instrument, "inner", "Gated shimmerreverb", &[], "", "std"),
        item(Kind::Instrument, "word", "Big reverb", &[], "", "std"),
        item(Kind::Instrument, "prefix", "Reverb hall", &[], "", "std"),
        item(Kind::Instrument, "exact", "Reverb", &[], "", "std"),
    ];
    assert_eq!(found(&items, "reverb"), ["exact", "prefix", "word", "inner", "inside", "tag", "desc"]);
    // Two words: the weakest decides. "hall" is only in Reverb hall's label.
    let two = [
        item(Kind::Instrument, "label-and-desc", "Reverb", &[], "a hall", "std"),
        item(Kind::Instrument, "both-label", "Big reverb hall", &[], "", "std"),
    ];
    assert_eq!(found(&two, "reverb hall"), ["both-label", "label-and-desc"]);
}

// LIB-1: then std's and music2's own before yours and the song's, before plugins and others; then
// by kind, label and id. The same list and words give the same order every time.
#[test]
fn std_and_ours_come_first_then_kind_label_and_id() {
    let items = [
        item(Kind::Plugin, "clap:x", "Keys", &[], "", "plugin"),
        item(Kind::Sound, "sound:yours/k", "Keys", &[], "", "yours"),
        item(Kind::Instrument, "song-keys", "Keys", &[], "", "song"),
        item(Kind::Instrument, "other:keys", "Keys", &[], "", "acme"),
        item(Kind::Sound, "sound:std/k", "Keys", &[], "", "std"),
        item(Kind::Instrument, "@m2/piano:keys", "Keys", &[], "", "@m2/piano"),
        item(Kind::Instrument, "std:keys", "Keys", &[], "", "std"),
        item(Kind::Instrument, "std:keys2", "keys", &[], "", "std"),
    ];
    let order = found(&items, "keys");
    assert_eq!(order, ["@m2/piano:keys", "std:keys", "std:keys2", "sound:std/k", "song-keys", "sound:yours/k", "other:keys", "clap:x"]);
    let mut reversed = items.clone();
    reversed.reverse();
    assert_eq!(found(&reversed, "keys"), order, "whatever order the items came in");
}

// LIB-1: a kind typed is a filter, not a word: `effects`, `fx`, `sound`, `presets`…
#[test]
fn a_kind_typed_filters_by_kind() {
    let items = [
        item(Kind::Instrument, "i", "Delay pluck", &[], "", "std"),
        item(Kind::Effect, "e", "Delay", &[], "", "std"),
        item(Kind::Sound, "s", "Long delay", &[], "", "std"),
    ];
    assert_eq!(found(&items, "delay effect"), ["e"]);
    assert_eq!(found(&items, "FX delay"), ["e"]);
    assert_eq!(found(&items, "delay presets"), ["s"]);
    assert_eq!(found(&items, "instruments sounds delay"), ["i", "s"], "two kinds: either");
    assert_eq!(found(&items, "effects"), ["e"], "a kind alone");
    let q = Query::parse("Sounds");
    assert_eq!((q.kinds.as_slice(), q.words.is_empty()), (&[Kind::Sound][..], true));
}

// LIB-1: `loop`, `one-shot`, a tempo and a key are filters only a sample can meet: they find only
// the samples that carry them.
#[test]
fn sample_filters_find_only_the_samples_that_carry_them() {
    let items = [
        sample("chop-am-90", "Vocal chop", SampleType::OneShot, Some(90.0), Some("Am")),
        sample("loop-am-90", "Vocal loop", SampleType::Loop, Some(90.0), Some("Am")),
        sample("loop-c-120", "Vocal loop", SampleType::Loop, Some(120.0), Some("Cmaj")),
        sample("loop-fs", "Vocal loop", SampleType::Loop, None, Some("Gb")),
        item(Kind::Instrument, "vocal-synth", "Vocal synth", &[], "", "std"),
    ];
    assert_eq!(found(&items, "vocal Am"), ["chop-am-90", "loop-am-90"]);
    assert_eq!(found(&items, "vocal loop"), ["loop-am-90", "loop-c-120", "loop-fs"]);
    assert_eq!(found(&items, "one-shot"), ["chop-am-90"]);
    assert_eq!(found(&items, "90 bpm"), ["chop-am-90", "loop-am-90"]);
    assert_eq!(found(&items, "120bpm loop"), ["loop-c-120"]);
    assert_eq!(found(&items, "C major"), ["loop-c-120"]);
    assert_eq!(found(&items, "F#"), ["loop-fs"], "F# is G♭");
    assert_eq!(found(&items, "vocal"), ["vocal-synth", "chop-am-90", "loop-am-90", "loop-c-120", "loop-fs"], "no filter: every kind");
}

// LIB-1: a key is written with a capital and a sign, or followed by major or minor; a bare letter
// or a lowercase `am` is a word, so typing "B" for Bass never filters by key.
#[test]
fn a_key_needs_a_capital_and_a_sign_or_a_mode() {
    let key = |t: &str| Query::parse(t).key.map(|k| k.words());
    assert_eq!(key("Am"), Some("A minor".into()));
    assert_eq!(key("F#"), Some("F# major".into()));
    assert_eq!(key("Bbm"), Some("Bb minor".into()));
    assert_eq!(key("C♯min"), Some("C# minor".into()));
    assert_eq!(key("E minor"), Some("E minor".into()));
    assert_eq!(key("D major"), Some("D major".into()));
    let q = Query::parse("F# minor pad");
    assert_eq!((q.key.map(|k| k.words()), q.words), (Some("F# minor".into()), vec!["pad".to_string()]));
    for word in ["B", "am", "bass", "Bass", "Amp", "f#"] {
        let q = Query::parse(word);
        assert_eq!((q.key, q.words), (None, vec![word.to_lowercase()]), "{word}");
    }
    let q = Query::parse("90 bpm piano");
    assert_eq!((q.bpm, q.words), (Some(90.0), vec!["piano".to_string()]));
    let q = Query::parse("90 drums");
    assert_eq!((q.bpm, q.words), (None, vec!["90".to_string(), "drums".to_string()]), "a number alone is a word");
}

// LIB-1: std's instruments and effects (those with a category), and each of std's presets as a
// sound with its sound's category; what code calls them, and a kit's drums, as tags.
#[test]
fn the_library_holds_stds_instruments_effects_and_presets() {
    let catalog = m2_script::describe(&[("std".to_string(), std_dir())]);
    let all = items(&Sources { catalog: &catalog, std: Some(&std_dir()), yours: None, plugins: &[] });
    let get = |id: &str| all.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id} in {:?}", all.iter().map(|i| &i.id).collect::<Vec<_>>()));
    let pluck = get("std:synth-pluck");
    assert_eq!((pluck.kind, pluck.label.as_str(), pluck.package.as_str(), pluck.instrument, pluck.preview), (Kind::Instrument, "Synth pluck", "std", true, true));
    assert!(pluck.tags.contains(&"pluck".to_string()), "{:?}", pluck.tags);
    let reverb = get("std:reverb");
    assert_eq!((reverb.kind, reverb.category.as_str(), reverb.instrument), (Kind::Effect, "Delay & Reverb", false));
    let glass = get("sound:std/std:synth-pluck/Glass");
    assert_eq!((glass.kind, glass.label.as_str(), glass.category.as_str(), glass.component.as_str()), (Kind::Sound, "Glass", "Instruments", "std:synth-pluck"));
    assert_eq!(glass.preset, Some(PresetOf { from: "std".into(), name: "Glass".into() }));
    assert!(get("sound:std/std:reverb/Big hall").kind == Kind::Sound);
    assert!(all.iter().all(|i| !i.category.is_empty()), "only what has a category (PKG-7)");
    assert!(!all.iter().any(|i| i.id == "std:pluck"), "std/instruments' pluck has none");
    let shipped = walk(&std_dir().join("presets")).into_iter().filter(|f| f.extension().is_some_and(|x| x == "json")).count();
    assert_eq!(all.iter().filter(|i| i.kind == Kind::Sound).count(), shipped, "each of std's presets");
    // "snare" finds the drum kit by what its notes play; "glass" finds the pluck's sound.
    assert!(found(&all, "snare").contains(&"std:drum-kit".to_string()));
    assert_eq!(found(&all, "glass pluck").first().map(String::as_str), Some("sound:std/std:synth-pluck/Glass"));
    assert_eq!(found(&all, "reverb")[0], "std:reverb");
}

// LIB-1: your presets are sounds too, found by the sound each file names in its own folder; one
// for a sound this computer hasn't got is left out, and a plugin's is one of yours with no preview.
#[test]
fn your_presets_are_sounds_and_a_missing_sounds_are_left_out() {
    let home = std::env::temp_dir().join(format!("m2-library-test-{}", std::process::id()));
    let yours = home.join("Presets");
    let put = |sound: &str, name: &str| {
        let p = Preset { name: name.into(), sound: sound.into(), values: [("decay".to_string(), 0.2)].into(), plugin_version: None, state: None };
        store(&yours.join(presets::folder(sound)), &p, false).unwrap();
    };
    put("std:synth-pluck", "Short");
    put("acme:gone", "Lost");
    put("clap:org.example.synth", "Init");
    // A file in another sound's folder isn't that sound's (P1).
    std::fs::create_dir_all(yours.join("std/synth-pad")).unwrap();
    std::fs::copy(yours.join("std/synth-pluck/Short.json"), yours.join("std/synth-pad/Short.json")).unwrap();
    let catalog = m2_script::describe(&[("std".to_string(), std_dir())]);
    let plugins = [Plugin { id: "clap:org.example.synth".into(), name: "Example Synth".into(), vendor: "Example".into(), instrument: true, effect: false }];
    let all = items(&Sources { catalog: &catalog, std: None, yours: Some(&yours), plugins: &plugins });
    let sounds: Vec<(&str, &str, &str, bool)> =
        all.iter().filter(|i| i.kind == Kind::Sound).map(|i| (i.id.as_str(), i.package.as_str(), i.category.as_str(), i.preview)).collect();
    assert_eq!(
        sounds,
        [("sound:yours/clap:org.example.synth/Init", "yours", "Plugins", false), ("sound:yours/std:synth-pluck/Short", "yours", "Instruments", true)]
    );
    let plugin = all.iter().find(|i| i.id == "clap:org.example.synth").unwrap();
    assert_eq!((plugin.kind, plugin.label.as_str(), plugin.instrument, plugin.preview), (Kind::Plugin, "Example Synth", true, false));
    assert_eq!(found(&all, "example")[0], "clap:org.example.synth");
    assert_eq!(found(&all, "init"), ["sound:yours/clap:org.example.synth/Init"]);
    let _ = std::fs::remove_dir_all(&home);
}

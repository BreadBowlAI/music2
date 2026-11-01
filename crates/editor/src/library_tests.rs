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
        own: false,
        component: id.into(),
        instrument: kind == Kind::Instrument,
        effect: kind == Kind::Effect,
        preview: true,
        no_preview: None,
        preset: None,
        sample: None,
    }
}

fn sample(id: &str, label: &str, kind: SampleType, bpm: Option<f64>, key: Option<&str>) -> Item {
    let mut i = item(Kind::Sample, id, label, &[], "", "@m2/vocals");
    i.sample = Some(Sample { kind, bpm, key: key.map(|k| Key::parse(k).unwrap()), seconds: Some(2.0), ..Sample::default() });
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

/// std alone, as the library's packages.
fn std_packages() -> Vec<(String, std::path::PathBuf)> {
    std::iter::once(("std".to_string(), std_dir())).chain(m2_song::pack::test_packs()).collect()
}

/// The folder of the package that makes `component`, among [`std_packages`].
fn package_dir(component: &str) -> std::path::PathBuf {
    let package = m2_song::pack::split_id(component).unwrap().0;
    std_packages().into_iter().find(|(n, _)| n == package).unwrap().1
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

// LIB-1: then std's and music2's own (`own`: a version music2 comes with, PACK-19) before yours and
// the song's, before plugins and others; then by kind, label and id. A pack named `@m2/` that isn't
// music2's own ranks with everyone else's. The same list and words give the same order every time.
#[test]
fn std_and_ours_come_first_then_kind_label_and_id() {
    let mut ours = item(Kind::Instrument, "@m2/piano:keys", "Keys", &[], "", "@m2/piano");
    ours.own = true;
    let items = [
        item(Kind::Plugin, "clap:x", "Keys", &[], "", "plugin"),
        item(Kind::Sound, "sound:yours/k", "Keys", &[], "", "yours"),
        item(Kind::Instrument, "song-keys", "Keys", &[], "", "song"),
        item(Kind::Instrument, "other:keys", "Keys", &[], "", "acme"),
        item(Kind::Instrument, "@m2/fake:keys", "Keys", &[], "", "@m2/fake"),
        item(Kind::Sound, "sound:std/k", "Keys", &[], "", "std"),
        ours,
        item(Kind::Instrument, "std:keys", "Keys", &[], "", "std"),
        item(Kind::Instrument, "std:keys2", "keys", &[], "", "std"),
    ];
    let order = found(&items, "keys");
    assert_eq!(
        order,
        ["@m2/piano:keys", "std:keys", "std:keys2", "sound:std/k", "song-keys", "sound:yours/k", "@m2/fake:keys", "other:keys", "clap:x"]
    );
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

// LIB-1: what is typed is split into words as an item's text is, at every character that isn't a
// letter or a digit: `hi-hat` finds the Drum's "hi-hats", `synth-pluck` and `synth.pluck` the Synth pluck,
// `delay & reverb` what its category says. An item's whole id finds it first.
#[test]
fn what_is_typed_is_split_into_words_as_an_items_text_is() {
    let catalog = m2_script::describe(&std_packages());
    let all = items(&Sources { catalog: &catalog, packages: &std_packages(), yours: None, plugins: &[], samples: &[] });
    let first = |text: &str| found(&all, text).first().cloned().unwrap_or_else(|| format!("nothing for {text:?}"));
    assert!(found(&all, "hi-hat").contains(&"@m2/drums:drum".to_string()), "{:?}", found(&all, "hi-hat"));
    assert_eq!(found(&all, "hi-hat"), found(&all, "hi hat"));
    for typed in ["synth-pluck", "synth.pluck", "Synth  pluck", "synth pluck!"] {
        assert_eq!(first(typed), "@m2/synths:synth-pluck", "{typed}");
    }
    assert_eq!(first("fm-metal"), "@m2/synths:fm-metal");
    assert_eq!(first("big-hall"), "sound:std/@m2/effects:reverb/Big hall");
    let both = found(&all, "delay & reverb");
    assert!(both.contains(&"@m2/effects:delay".to_string()) && both.contains(&"@m2/effects:reverb".to_string()), "{both:?}");
    assert_eq!(found(&all, "delay & reverb"), found(&all, "delay reverb"), "& is no word");
    assert_eq!(first("@m2/effects:reverb"), "@m2/effects:reverb", "an id typed");
    assert_eq!(first("sound:std/@m2/effects:reverb/Big hall"), "sound:std/@m2/effects:reverb/Big hall");
    let items = [item(Kind::Instrument, "hh", "Hi-hat", &[], "", "std"), item(Kind::Instrument, "hhs", "Hi-hat shaker", &[], "", "std")];
    assert_eq!(found(&items, "hi hat"), ["hh", "hhs"], "the label word for word, its dash aside");
    assert_eq!(found(&items, "-"), ["hh", "hhs"], "nothing but punctuation is no word");
}

// LIB-1: a kind word finds that kind (a plugin is an instrument or an effect too, by what it plays)
// and also the items whose label has the word: a name with a kind word in it is still found.
#[test]
fn a_kind_word_finds_that_kind_and_names_with_it() {
    let catalog = m2_script::describe(&std_packages());
    let plugins = [
        Plugin { id: "clap:aw".into(), name: "Consolidated".into(), vendor: "Airwindows".into(), instrument: false, effect: true },
        Plugin { id: "clap:sx".into(), name: "Surge XT".into(), vendor: "Surge".into(), instrument: true, effect: false },
        Plugin { id: "clap:sxfx".into(), name: "Surge XT Effects".into(), vendor: "Surge".into(), instrument: false, effect: true },
        Plugin { id: "clap:dly".into(), name: "Space Delay".into(), vendor: "Acme".into(), instrument: false, effect: true },
    ];
    let all = items(&Sources { catalog: &catalog, packages: &std_packages(), yours: None, plugins: &plugins, samples: &[] });
    assert_eq!(found(&all, "consolidated effect"), ["clap:aw"], "an effect plugin is an effect");
    let delays = found(&all, "delay effect");
    assert_eq!(delays.first().map(String::as_str), Some("@m2/effects:delay"));
    assert!(delays.contains(&"clap:dly".to_string()), "{delays:?}");
    let effects = found(&all, "effects");
    assert!(effects.contains(&"@m2/effects:reverb".to_string()) && effects.contains(&"clap:aw".to_string()), "{effects:?}");
    assert!(!effects.contains(&"clap:sx".to_string()) && !effects.iter().any(|i| i.starts_with("sound:")), "{effects:?}");
    let instruments = found(&all, "surge instrument");
    assert_eq!(instruments, ["clap:sx"], "an instrument plugin is an instrument");
    assert_eq!(found(&all, "surge xt effects")[0], "clap:sxfx", "a name with a kind word in it");
    let items = [
        item(Kind::Instrument, "loop-station", "Loop station", &[], "", "acme"),
        item(Kind::Instrument, "sound-engine", "Sound engine", &[], "", "acme"),
        sample("loop-90", "Drum loop", SampleType::Loop, Some(90.0), None),
        sample("hit", "Drum hit", SampleType::OneShot, None, None),
    ];
    assert_eq!(found(&items, "loop"), ["loop-station", "loop-90"], "a loop, and a name with loop in it");
    assert_eq!(found(&items, "loop station"), ["loop-station"]);
    assert_eq!(found(&items, "sound engine"), ["sound-engine"]);
    assert_eq!(found(&items, "drum one-shot"), ["hit"]);
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
    assert_eq!(found(&items, "vocal Am"), ["chop-am-90", "loop-am-90", "loop-c-120"], "A minor, and its relative, C major");
    assert_eq!(found(&items, "vocal loop"), ["loop-am-90", "loop-c-120", "loop-fs"]);
    assert_eq!(found(&items, "one-shot"), ["chop-am-90"]);
    assert_eq!(found(&items, "90 bpm"), ["chop-am-90", "loop-am-90"]);
    assert_eq!(found(&items, "120bpm loop"), ["loop-c-120"]);
    assert_eq!(found(&items, "C major"), ["chop-am-90", "loop-am-90", "loop-c-120"], "C major, and its relative, A minor");
    assert_eq!(found(&items, "F#"), ["loop-fs"], "F# is G♭");
    assert_eq!(found(&items, "vocal"), ["vocal-synth", "chop-am-90", "loop-am-90", "loop-c-120", "loop-fs"], "no filter: every kind");
}

// LIB-1: a tempo typed finds the samples within 2 bpm of it; a key, the samples in it or its
// relative, and no other.
#[test]
fn a_tempo_finds_within_two_bpm_and_a_key_its_relative() {
    let items = [
        sample("at-88", "Keys", SampleType::Loop, Some(88.0), None),
        sample("at-91", "Keys", SampleType::Loop, Some(91.5), None),
        sample("at-92", "Keys", SampleType::Loop, Some(92.0), None),
        sample("at-93", "Keys", SampleType::Loop, Some(92.5), None),
        sample("in-em", "Keys", SampleType::Loop, None, Some("Em")),
        sample("in-g", "Keys", SampleType::Loop, None, Some("Gmaj")),
        sample("in-e", "Keys", SampleType::Loop, None, Some("Emaj")),
        sample("in-bm", "Keys", SampleType::Loop, None, Some("Bm")),
    ];
    assert_eq!(found(&items, "90 bpm"), ["at-88", "at-91", "at-92"]);
    assert_eq!(found(&items, "Em"), ["in-em", "in-g"]);
    assert_eq!(found(&items, "G major"), ["in-em", "in-g"]);
    assert_eq!(found(&items, "E major"), ["in-e"], "E major's relative is C# minor, not E minor");
    assert_eq!(Key::parse("Am").unwrap().relative(), Key::parse("Cmaj").unwrap());
    assert_eq!(Key::parse("F#").unwrap().relative(), Key::parse("Ebm").unwrap(), "F# major's relative is D# (E♭) minor");
}

// LIB-1: Fits my song keeps the loops within 8% of the song's tempo, in its key or the key's
// relative when it has one (a loop with no key fits any); without a key, by tempo alone.
#[test]
fn fits_my_song_keeps_loops_near_the_songs_tempo_in_its_key() {
    let items = [
        sample("am-120", "Loop", SampleType::Loop, Some(120.0), Some("Am")),
        sample("c-111", "Loop", SampleType::Loop, Some(111.0), Some("Cmaj")),
        sample("am-110", "Loop", SampleType::Loop, Some(110.0), Some("Am")),
        sample("drums-128", "Loop", SampleType::Loop, Some(129.0), None),
        sample("drums-130", "Loop", SampleType::Loop, Some(130.0), None),
        sample("dm-120", "Loop", SampleType::Loop, Some(120.0), Some("Dm")),
        sample("hit-am", "Loop", SampleType::OneShot, Some(120.0), Some("Am")),
        sample("no-tempo", "Loop", SampleType::Loop, None, Some("Am")),
        item(Kind::Instrument, "std:loop", "Loop", &[], "", "std"),
    ];
    let fits = |bpm: f64, key: Option<&str>, text: &str| {
        let q = Query { fits: Some(Fits { bpm, key: key.map(|k| Key::parse(k).unwrap()) }), ..Query::parse(text) };
        search(&items, &q).into_iter().map(|i| i.id.clone()).collect::<Vec<_>>()
    };
    assert_eq!(fits(120.0, Some("Am"), ""), ["am-120", "c-111", "drums-128"], "110 is 8.3% under, 130 8.3% over");
    assert_eq!(fits(120.0, None, ""), ["am-120", "c-111", "dm-120", "drums-128"]);
    assert_eq!(fits(120.0, Some("Dm"), "loop"), ["dm-120", "drums-128"]);
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
    for (typed, word) in [("B", "b"), ("am", "am"), ("bass", "bass"), ("Bass", "bass"), ("Amp", "amp"), ("f#", "f")] {
        let q = Query::parse(typed);
        assert_eq!((q.key, q.words), (None, vec![word.to_string()]), "{typed}");
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
    let catalog = m2_script::describe(&std_packages());
    let all = items(&Sources { catalog: &catalog, packages: &std_packages(), yours: None, plugins: &[], samples: &[] });
    let get =
        |id: &str| all.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id} in {:?}", all.iter().map(|i| &i.id).collect::<Vec<_>>()));
    let pluck = get("@m2/synths:synth-pluck");
    assert_eq!(
        (pluck.kind, pluck.label.as_str(), pluck.package.as_str(), pluck.instrument, pluck.preview),
        (Kind::Instrument, "Synth pluck", "@m2/synths", true, true)
    );
    assert!(pluck.tags.contains(&"pluck".to_string()), "{:?}", pluck.tags);
    let reverb = get("@m2/effects:reverb");
    assert_eq!((reverb.kind, reverb.category.as_str(), reverb.instrument), (Kind::Effect, "Delay & Reverb", false));
    let glass = get("sound:std/@m2/synths:synth-pluck/Glass");
    assert_eq!(
        (glass.kind, glass.label.as_str(), glass.category.as_str(), glass.component.as_str()),
        (Kind::Sound, "Glass", "Instruments", "@m2/synths:synth-pluck")
    );
    assert_eq!(glass.preset, Some(PresetOf { from: "std".into(), name: "Glass".into() }));
    assert!(get("sound:std/@m2/effects:reverb/Big hall").kind == Kind::Sound);
    assert!(all.iter().all(|i| !i.category.is_empty()), "only what has a category (PKG-7)");
    assert!(!all.iter().any(|i| i.id == "@m2/synths:pluck"), "@m2/synths' simple pluck has none");
    let shipped = std_packages()
        .iter()
        .flat_map(|(_, dir)| walk(&dir.join("presets")))
        .filter(|f| f.extension().is_some_and(|x| x == "json"))
        .count();
    assert_eq!(all.iter().filter(|i| i.kind == Kind::Sound).count(), shipped, "each of the packages' presets");
    // "snare" finds the Drum by what its menu plays; "glass" finds the pluck's sound. The drum
    // machine is no item: no list offers it (KIT-2).
    assert!(found(&all, "snare").contains(&"@m2/drums:drum".to_string()));
    assert!(!all.iter().any(|i| i.id == "@m2/drums:drum-kit"), "the drum machine has no category");
    assert_eq!(found(&all, "glass pluck").first().map(String::as_str), Some("sound:std/@m2/synths:synth-pluck/Glass"));
    assert_eq!(found(&all, "reverb")[0], "@m2/effects:reverb");
}

// LIB-1: your presets are sounds too, found by the sound each file names in its own folder; one
// for a sound this computer hasn't got is left out, and a plugin's is one of yours with no preview.
#[test]
fn your_presets_are_sounds_and_a_missing_sounds_are_left_out() {
    let home = std::env::temp_dir().join(format!("m2-library-test-{}", std::process::id()));
    let yours = home.join("Presets");
    let put = |sound: &str, name: &str| {
        let p = Preset {
            name: name.into(),
            sound: sound.into(),
            values: [("decay".to_string(), 0.2)].into(),
            plugin_version: None,
            state: None,
        };
        store(&yours.join(presets::folder(sound)), &p, false).unwrap();
    };
    put("@m2/synths:synth-pluck", "Short");
    put("acme:gone", "Lost");
    put("clap:org.example.synth", "Init");
    // A file in another sound's folder isn't that sound's (P1).
    std::fs::create_dir_all(yours.join("_m2_synths/synth-pad")).unwrap();
    std::fs::copy(yours.join("_m2_synths/synth-pluck/Short.json"), yours.join("_m2_synths/synth-pad/Short.json")).unwrap();
    let catalog = m2_script::describe(&std_packages());
    let plugins = [Plugin {
        id: "clap:org.example.synth".into(),
        name: "Example Synth".into(),
        vendor: "Example".into(),
        instrument: true,
        effect: false,
    }];
    let all = items(&Sources { catalog: &catalog, packages: &[], yours: Some(&yours), plugins: &plugins, samples: &[] });
    let sounds: Vec<(&str, &str, &str, bool)> =
        all.iter().filter(|i| i.kind == Kind::Sound).map(|i| (i.id.as_str(), i.package.as_str(), i.category.as_str(), i.preview)).collect();
    assert_eq!(
        sounds,
        [
            ("sound:yours/clap:org.example.synth/Init", "yours", "Plugins", false),
            ("sound:yours/@m2/synths:synth-pluck/Short", "yours", "Instruments", true)
        ]
    );
    let plugin = all.iter().find(|i| i.id == "clap:org.example.synth").unwrap();
    assert_eq!((plugin.kind, plugin.label.as_str(), plugin.instrument, plugin.preview), (Kind::Plugin, "Example Synth", true, false));
    assert_eq!(found(&all, "example")[0], "clap:org.example.synth");
    assert_eq!(found(&all, "init"), ["sound:yours/clap:org.example.synth/Init"]);
    let _ = std::fs::remove_dir_all(&home);
}

mod previews {
    use std::collections::BTreeMap;

    use super::*;
    use crate::library::preview::{self, RATE, SECONDS};

    fn std_items() -> (m2_script::Catalog, Vec<Item>) {
        let catalog = m2_script::describe(&std_packages());
        let all = items(&Sources { catalog: &catalog, packages: &std_packages(), yours: None, plugins: &[], samples: &[] });
        (catalog, all)
    }

    fn get<'a>(all: &'a [Item], id: &str) -> &'a Item {
        all.iter().find(|i| i.id == id).unwrap_or_else(|| panic!("{id}"))
    }

    /// The pitches a lane file's notes play, in order.
    fn pitches(lane: &str) -> Vec<u64> {
        let v: serde_json::Value = serde_json::from_str(lane).unwrap();
        v["clips"][0]["notes"].as_array().unwrap().iter().map(|n| n["pitch"].as_u64().unwrap()).collect()
    }

    // LIB-3: the phrase follows the category: a beat (on the notes a drum names, else General MIDI's
    // kick, snare and closed hat, as for the Drum), a bass line, four chords; a sound plays its instrument with its preset's values; an effect plays the loop, the
    // first bar dry and the second through it.
    #[test]
    fn a_previews_song_plays_a_phrase_for_its_category() {
        let (catalog, all) = std_items();
        let none = BTreeMap::new();
        let drum = preview::song(get(&all, "@m2/drums:drum"), &catalog, &none).unwrap();
        let mut beat = pitches(&drum.files["data/play/notes.json"]);
        beat.sort();
        beat.dedup();
        assert_eq!(beat, [36, 38, 42], "kick, snare and closed hat, General MIDI's");
        let bass = preview::song(get(&all, "@m2/synths:synth-bass"), &catalog, &none).unwrap();
        assert!(pitches(&bass.files["data/play/notes.json"]).iter().all(|&p| p < 48), "a bass line, low");
        let pad = preview::song(get(&all, "@m2/synths:synth-pad"), &catalog, &none).unwrap();
        assert_eq!(pitches(&pad.files["data/play/notes.json"]).len(), 12, "four chords of three notes");
        assert!(pad.files["song.ts"].contains("item.pad(lane.notes(\"notes\"))"), "{}", pad.files["song.ts"]);

        let glass = get(&all, "sound:std/@m2/synths:synth-pluck/Glass");
        let values: BTreeMap<String, f64> = [("brightness".to_string(), 9000.0), ("nope".to_string(), 1.0)].into();
        let sound = preview::song(glass, &catalog, &values).unwrap();
        assert!(
            sound.files["song.ts"].contains(r#"item.pluck(lane.notes("notes"), { "brightness": 9000 })"#),
            "{}",
            sound.files["song.ts"]
        );

        let reverb = preview::song(get(&all, "@m2/effects:reverb"), &catalog, &none).unwrap();
        let text = &reverb.files["song.ts"];
        assert!(text.contains(r#"const wet = track("wet""#) && text.contains("item(mixer({"), "{text}");
        let starts = |f: &str| {
            let v: serde_json::Value = serde_json::from_str(&reverb.files[f]).unwrap();
            v["clips"][0]["notes"].as_array().unwrap().iter().map(|n| n["start"].as_f64().unwrap()).collect::<Vec<_>>()
        };
        assert!(starts("data/dry/keys.json").iter().chain(&starts("data/dry/hits.json")).all(|&s| s < 4.0), "bar 1 dry");
        assert!(starts("data/wet/keys.json").iter().chain(&starts("data/wet/hits.json")).all(|&s| s >= 4.0), "bar 2 wet");
        assert!(text.contains(r#""size": 0.6"#), "what adding it writes: {text}");
    }

    // LIB-3: every std instrument, effect and sound that says it has a preview renders one, and
    // it is heard: at −23 LUFS, or as near as a peak of −1 dBFS lets it, within 3 dB. Never silence.
    #[test]
    fn every_std_item_with_a_preview_renders_one() {
        let (catalog, all) = std_items();
        let dir = std::env::temp_dir().join(format!("m2-preview-all-{}", std::process::id()));
        let packages = std_packages();
        let mut failed = vec![];
        let mut under = vec![];
        for item in all.iter().filter(|i| i.preview) {
            let values = match &item.preset {
                Some(p) => {
                    crate::presets::read(
                        &crate::presets::factory(&package_dir(&item.component), &item.component).unwrap(),
                        &item.component,
                        &p.name,
                    )
                    .unwrap()
                    .values
                }
                None => BTreeMap::new(),
            };
            let made = preview::song(item, &catalog, &values)
                .map_err(anyhow::Error::msg)
                .and_then(|song| preview::render(&song, &packages, &dir.join("work"), &dir.join("out.wav")));
            if let Err(e) = made {
                failed.push(format!("{}: {e:#}", item.id));
                continue;
            }
            let (loudness, peak) = heard(&dir.join("out.wav"));
            let at_target = (loudness - preview::LUFS).abs() < 0.5;
            let peak_held = (peak - preview::PEAK_DB).abs() < 0.05 && loudness > preview::LUFS - 3.0;
            if !(at_target || peak_held) {
                failed.push(format!("{}: {loudness:.1} LUFS, peak {peak:.2} dBFS", item.id));
            } else if !at_target {
                under.push(format!("{} {:.1} dB under", item.label, preview::LUFS - loudness));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(failed.is_empty(), "{}", failed.join("\n"));
        // The guide names the ones a peak holds back: Hats and Rim.
        assert!(under.iter().all(|u| u.starts_with("Hats ") || u.starts_with("Rim ")), "{under:?}");
    }

    /// A preview's loudness (BS.1770, as `preview::loudness` measures it) and its peak in dBFS.
    fn heard(file: &std::path::Path) -> (f64, f64) {
        let mut wav = hound::WavReader::open(file).unwrap();
        let s: Vec<f32> = wav.samples::<i16>().map(|s| s.unwrap() as f32 / 32767.0).collect();
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs())) as f64;
        (preview::loudness(&s).unwrap_or(f64::NEG_INFINITY), 20.0 * peak.log10())
    }

    /// `seconds` of a 997 Hz sine of `amplitude` in both channels at 48 kHz, then `silent` seconds.
    fn sine(amplitude: f32, seconds: f64, silent: f64) -> Vec<f32> {
        let n = (seconds * RATE as f64) as usize;
        let mut v: Vec<f32> = (0..n)
            .flat_map(|i| {
                let x = amplitude * (std::f64::consts::TAU * 997.0 * i as f64 / RATE as f64).sin() as f32;
                [x, x]
            })
            .collect();
        v.extend(std::iter::repeat_n(0.0, (silent * RATE as f64) as usize * 2));
        v
    }

    // LIB-3: loudness as ITU-R BS.1770 measures it: a 997 Hz sine at −20 dBFS in both channels is
    // −20 LUFS; silence between sounds is gated out, so a short sound isn't judged by its gaps; all
    // silence has none.
    #[test]
    fn loudness_is_measured_as_bs_1770_does() {
        let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 0.1);
        let l = preview::loudness(&sine(0.1, 4.5, 0.0));
        assert!(near(l, -20.0), "{l:?}");
        // Ungated, 1.5 s of it in 4.5 would be 4.8 dB quieter; only the blocks across its end count.
        let l = preview::loudness(&sine(0.1, 1.5, 3.0));
        assert!(l.is_some_and(|l| (-20.6..=-20.0).contains(&l)), "the silence after it is gated out: {l:?}");
        assert_eq!(preview::loudness(&sine(0.0, 4.5, 0.0)), None);
        // Brought to the target, or to a peak of −1 dBFS where the target would pass it.
        let mut quiet = sine(0.01, 4.5, 0.0);
        preview::level(&mut quiet).unwrap();
        assert!(near(preview::loudness(&quiet[..quiet.len() - RATE as usize]), preview::LUFS), "{:?}", preview::loudness(&quiet));
        let mut spike = sine(0.01, 4.5, 0.0);
        spike[1000] = 1.0;
        preview::level(&mut spike).unwrap();
        let peak = spike.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((20.0 * (peak as f64).log10() - preview::PEAK_DB).abs() < 0.01, "{peak}");
        assert!(preview::level(&mut sine(0.0, 4.5, 0.0)).is_err(), "silence is no preview");
    }

    // LIB-3: a preview builds and renders on a thread of its own (at background priority, as an
    // export's workers do beside playback): the thread that asks spends next to no time on it.
    #[test]
    fn a_preview_renders_on_a_thread_of_its_own() {
        let (catalog, all) = std_items();
        let dir = std::env::temp_dir().join(format!("m2-preview-thread-{}", std::process::id()));
        let song = preview::song(get(&all, "@m2/synths:synth-strings"), &catalog, &BTreeMap::new()).unwrap();
        let (cpu, wall) = (m2_os::thread::cpu_time(), std::time::Instant::now());
        preview::render(&song, &std_packages(), &dir.join("work"), &dir.join("out.wav")).unwrap();
        let (cpu, wall) = (m2_os::thread::cpu_time() - cpu, wall.elapsed());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(cpu * 10 < wall, "the asking thread used {cpu:?} of the {wall:?} it took");
    }

    // LIB-3: plugins, a plugin's preset and the song's own code have no preview, said in words.
    #[test]
    fn a_plugin_or_the_songs_own_has_no_preview() {
        let (catalog, _) = std_items();
        let plugin = items(&Sources {
            catalog: &catalog,
            packages: &[],
            yours: None,
            plugins: &[Plugin { id: "clap:a.b".into(), name: "AB".into(), vendor: "A".into(), instrument: true, effect: false }],
            samples: &[],
        })
        .into_iter()
        .find(|i| i.kind == Kind::Plugin)
        .unwrap();
        assert_eq!(preview::song(&plugin, &catalog, &BTreeMap::new()), Err("Plugins have no preview yet.".to_string()));
        // An instrument that plays a file you give it has none: its preview would be silence.
        let sampler = items(&Sources { catalog: &catalog, packages: &[], yours: None, plugins: &[], samples: &[] })
            .into_iter()
            .find(|i| i.id == "std:sampler")
            .unwrap();
        assert!(!sampler.preview && sampler.no_preview.as_deref() == Some("Sampler plays a file you give it, so it has no preview."));
        assert_eq!(
            preview::song(&sampler, &catalog, &BTreeMap::new()),
            Err("Sampler plays a file you give it, so it has no preview.".to_string())
        );
        let mut own = item(Kind::Instrument, "my-synth", "My synth", &[], "", "song");
        own.preview = false;
        assert_eq!(
            preview::song(&own, &catalog, &BTreeMap::new()),
            Err("My synth is the song's own, so it has no preview here.".to_string())
        );
    }

    // LIB-3, RENDER-2: a preview is a pure render, so the same bits twice; 16-bit stereo at 48 kHz,
    // 4.5 s, at one loudness (−23 LUFS, or a peak of -1 dBFS), ending in silence.
    #[test]
    fn a_preview_is_the_same_bits_twice_at_one_loudness() {
        let (catalog, all) = std_items();
        let dir = std::env::temp_dir().join(format!("m2-preview-test-{}", std::process::id()));
        let packages = std_packages();
        for id in ["@m2/synths:synth-pad", "@m2/drums:drum", "@m2/effects:delay"] {
            let song = preview::song(get(&all, id), &catalog, &BTreeMap::new()).unwrap();
            let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
            preview::render(&song, &packages, &dir.join("work"), &a).unwrap();
            preview::render(&song, &packages, &dir.join("work"), &b).unwrap();
            assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap(), "{id}: the same bits twice");
            assert!(!dir.join("work").exists(), "the song it rendered is gone");
            let mut wav = hound::WavReader::open(&a).unwrap();
            let spec = wav.spec();
            assert_eq!((spec.channels, spec.sample_rate, spec.bits_per_sample), (2, RATE, 16), "{id}");
            assert_eq!(wav.duration() as f64, SECONDS * RATE as f64, "{id}");
            let s: Vec<f64> = wav.samples::<i16>().map(|s| s.unwrap() as f64 / 32767.0).collect();
            let db = |x: f64| 20.0 * x.log10();
            let (loudness, _) = heard(&a);
            let peak = s.iter().fold(0f64, |m, x| m.max(x.abs()));
            assert!((loudness - -23.0).abs() < 0.5 || (db(peak) - -1.0).abs() < 0.05, "{id}: {loudness} LUFS, peak {} dB", db(peak));
            assert!(db(peak) <= -0.99, "{id}: peak {}", db(peak));
            assert_eq!(s[s.len() - 2..], [0.0, 0.0], "{id}: it fades out to silence");
        }
        // The fingerprint follows what it renders: another song, another hash.
        let pad = preview::song(get(&all, "@m2/synths:synth-pad"), &catalog, &BTreeMap::new()).unwrap();
        let bright = preview::song(get(&all, "@m2/synths:synth-pad"), &catalog, &[("cutoff".to_string(), 3000.0)].into()).unwrap();
        assert_eq!(preview::fingerprint(&pad, &packages), preview::fingerprint(&pad, &packages));
        assert_ne!(preview::fingerprint(&pad, &packages), preview::fingerprint(&bright, &packages));
        // And music2's version and its SDK's: an update renders anew.
        let by = preview::made_by();
        assert!(by.contains(env!("CARGO_PKG_VERSION")) && by.contains(m2_script::SDK_VERSION), "{by}");
        assert_eq!(preview::fingerprint(&pad, &packages), preview::fingerprint_by(&pad, &packages, &by));
        assert_ne!(preview::fingerprint_by(&pad, &packages, "music2 0.1.0"), preview::fingerprint_by(&pad, &packages, "music2 0.1.1"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A pack in `dir`, `@alice/piano` 0.1.0, of one instrument written as data (PACK-13) that plays
/// one recording: what `m2 pack import-sfz` makes.
fn data_pack(dir: &Path) {
    std::fs::create_dir_all(dir.join("instruments")).unwrap();
    std::fs::write(
        dir.join("m2pkg.toml"),
        "[package]\nname = \"@alice/piano\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.7\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("instruments/piano.toml"),
        "label = \"Felt piano\"\ncategory = \"Keys\"\nengine = \"std:sampler\"\nmode = \"gate\"\n\n[[region]]\nfile = \"samples/C4.wav\"\nroot = 60\nkeys = [0, 127]\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(dir.join("samples/C4.wav"), spec).unwrap();
    for i in 0..24_000 {
        w.write_sample(((i as f32 * 261.6 * std::f32::consts::TAU / 48_000.0).sin() * 9_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

// LIB-1, LIB-2, REG-6 (gaps #47): with no song open the library lists a pack's instrument written
// as data, as the session's build and the registry's robot do, and its preview renders.
#[test]
fn a_data_instrument_is_in_the_library_with_no_song_and_previews() {
    let f = crate::testing::Fixture::new();
    let pack = f.0.join("piano");
    data_pack(&pack);
    let packages = vec![("std".to_string(), std_dir()), ("@alice/piano".to_string(), pack.clone())];
    let lib = without_song(&packages, None, &[], &[]);
    let piano = lib.get("@alice/piano:piano").expect("the data instrument is listed with no song open");
    assert_eq!(
        (piano.kind, piano.label.as_str(), piano.package.as_str(), piano.preview),
        (Kind::Instrument, "Felt piano", "@alice/piano", true)
    );
    let catalog = m2_session::describe(&packages);
    let (song, loads) = preview::of(piano, &catalog, &packages, None).unwrap();
    let wav = f.0.join("piano.wav");
    preview::render(&song, &loads, &f.0.join("work"), &wav).unwrap();
    assert!(std::fs::metadata(&wav).unwrap().len() > 44, "it renders");
}

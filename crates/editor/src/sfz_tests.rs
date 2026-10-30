use super::*;

/// A fresh folder for one test.
fn scratch(what: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("m2-sfz-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn parsed(text: &str) -> Sfz {
    let d = scratch(&format!("p{:x}", text.len() * 31 + text.bytes().map(usize::from).sum::<usize>()));
    std::fs::write(d.join("i.sfz"), text).unwrap();
    let s = parse(&d.join("i.sfz")).unwrap();
    std::fs::remove_dir_all(&d).ok();
    s
}

fn op<'a>(s: &'a Sfz, region: usize, k: &str) -> Option<&'a str> {
    s.regions[region].opcodes.get(k).map(String::as_str)
}

fn mapped(text: &str) -> Vec<Result<Mapped, String>> {
    let s = parsed(text);
    s.regions.iter().map(|r| map(r, &s.control)).collect()
}

fn one(text: &str) -> Mapped {
    mapped(text).remove(0).unwrap()
}

#[test]
fn a_region_takes_the_opcodes_of_every_header_above_it() {
    let s = parsed(
        "<global> volume=-3 pan=10\n<master> tune=5\n<group> lovel=1 hivel=64\n<region> sample=a.wav pan=-20\n<region> sample=b.wav\n\
         <group> lovel=65\n<region> sample=c.wav\n<master>\n<region> sample=d.wav\n<global>\n<region> sample=e.wav\n",
    );
    assert_eq!(s.regions.len(), 5);
    assert_eq!(
        (op(&s, 0, "volume"), op(&s, 0, "tune"), op(&s, 0, "hivel"), op(&s, 0, "pan")),
        (Some("-3"), Some("5"), Some("64"), Some("-20")),
        "its own wins"
    );
    assert_eq!(op(&s, 1, "pan"), Some("10"), "the next region starts again from the headers");
    assert_eq!((op(&s, 2, "lovel"), op(&s, 2, "hivel")), (Some("65"), None), "a new group starts the group afresh");
    assert_eq!(
        (op(&s, 3, "tune"), op(&s, 3, "lovel"), op(&s, 3, "volume")),
        (None, None, Some("-3")),
        "a new master clears master and group"
    );
    assert_eq!(op(&s, 4, "volume"), None, "a new global clears all");
    assert_eq!(s.regions[0].at, "i.sfz:4");
}

#[test]
fn defines_includes_and_comments() {
    let d = scratch("include");
    std::fs::create_dir_all(d.join("Data")).unwrap();
    std::fs::write(d.join("Data/notes.txt"), "<region> sample=$NAME.wav key=$KEY\n#include \"Data/more.txt\"\n").unwrap();
    std::fs::write(d.join("Data/more.txt"), "<region> sample=two.wav locc$PEDAL=64\n").unwrap();
    std::fs::write(
        d.join("main.sfz"),
        "// Recorded by Ada.\n/* by hand\n   in 2020 */\n// CC-BY 4.0\n#define $NAME one\n#define $KEY 60\n#define $PEDAL 64\n\
         <control> default_path=Samples/ // the folder\n<group> #include \"Data/notes.txt\" lovel=10\n<region> sample=Grand A0.wav lokey=21 hikey=22\n",
    )
    .unwrap();
    let s = parse(&d.join("main.sfz")).unwrap();
    assert_eq!(s.comments, ["Recorded by Ada.", "CC-BY 4.0"], "the comments before the first header");
    assert_eq!(s.regions.len(), 3);
    assert_eq!((op(&s, 0, "sample"), op(&s, 0, "key")), (Some("one.wav"), Some("60")), "a define in a value");
    assert_eq!(op(&s, 1, "locc64"), Some("64"), "a define in an opcode's name");
    assert_eq!(s.regions[1].at, "more.txt:1");
    assert_eq!(op(&s, 2, "sample"), Some("Grand A0.wav"), "a sample's name may hold spaces");
    assert_eq!(op(&s, 2, "lovel"), None, "an opcode after an include on the group's line is the group's, before the region");
    assert_eq!(s.regions[2].default_path, "Samples/");
    std::fs::write(d.join("loop.sfz"), "#include \"loop.sfz\"\n").unwrap();
    assert!(parse(&d.join("loop.sfz")).unwrap_err().contains("include itself"));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn notes_are_numbers_or_names() {
    for (text, n) in
        [("60", 60), ("c4", 60), ("C4", 60), ("c#4", 61), ("db4", 61), ("c-1", 0), ("g9", 127), ("a0", 21), ("F#3", 54), ("bb3", 58)]
    {
        assert_eq!(note(text), Some(n), "{text}");
    }
    assert_eq!(note("h4"), None);
    assert_eq!(note_name(61), "c#4");
}

#[test]
fn each_opcode_becomes_its_field() {
    let m = one("<region> sample=x.wav key=c4");
    assert_eq!((m.root, m.keys, m.velocity, m.round_robin, m.release, m.loop_), (60, (60, 60), (1, 127), None, false, None));
    let m = one("<region> sample=x.wav lokey=40 hikey=50 pitch_keycenter=45");
    assert_eq!((m.root, m.keys), (45, (40, 50)));
    let m = one("<region> sample=x.wav");
    assert_eq!((m.root, m.keys), (60, (0, 127)), "SFZ's defaults");
    let m = one("<region> sample=x.wav lovel=0 hivel=64 seq_length=3 seq_position=2 trigger=release");
    assert_eq!((m.velocity, m.round_robin, m.release), ((1, 64), Some((2, 3)), true));
    let m = one("<region> sample=x.wav loop_mode=loop_sustain loop_start=100 loop_end=900");
    assert_eq!((m.loop_, m.one_shot, m.notes.len()), (Some((100, 900)), false, 0));
    let m = one("<region> sample=x.wav loop_mode=loop_continuous loopstart=100 loopend=900");
    assert_eq!(m.loop_, Some((100, 900)));
    assert!(m.notes[0].contains("loop_continuous"), "{:?}", m.notes);
    assert!(one("<region> sample=x.wav loop_mode=loop_sustain").notes[0].contains("own loop points"));
    assert!(one("<region> sample=x.wav loop_mode=one_shot").one_shot);
    let m = one("<region> sample=x.wav tune=-12 transpose=2 volume=-6.5 pan=-50 offset=480");
    assert_eq!((m.tune, m.volume, m.pan, m.offset), (188.0, -6.5, -0.5, 480));
    let m = one("<region> sample=x.wav amp_veltrack=73 ampeg_attack=0.01 ampeg_release=0.8 off_mode=normal");
    assert_eq!((m.veltrack, m.attack, m.release_time), (Some(73.0), Some(0.01), Some(0.8)));
    assert_eq!(m.ignored, ["off_mode"], "what music2 doesn't play is named");
    let m = one("<control> default_path=Samples\\Piano\\\n<region> sample=Soft\\A0.wav");
    assert_eq!(m.source, "Samples/Piano/Soft/A0.wav", "Windows' \\ is /");
}

#[test]
fn regions_music2_cant_play_are_left_out_saying_why() {
    let got = mapped(
        "<control> set_cc20=100\n<global> sw_default=c0\n\
         <region> sample=a.wav sw_last=c0\n<region> sample=b.wav sw_last=c#0\n\
         <region> sample=c.wav locc64=64\n<region> sample=d.wav locc20=64\n<region> sample=e.wav on_locc64=64\n\
         <region> sample=f.wav trigger=first\n<region> sample=*sine\n<region> sample=g.wav hikey=-1\n<region> sample=h.wav lorand=0.5\n",
    );
    assert!(got[0].is_ok(), "the default key switch's");
    assert_eq!(got[1].as_ref().unwrap_err(), "belong to the key switch c#0, not the default");
    assert_eq!(got[2].as_ref().unwrap_err(), "play only with a controller moved (CC 64)");
    assert!(got[3].is_ok(), "CC 20 starts at 100 (set_cc20), which its condition takes");
    assert_eq!(got[4].as_ref().unwrap_err(), "play when a controller moves (CC 64)");
    assert_eq!(got[5].as_ref().unwrap_err(), "play on `trigger=first`, which music2 doesn't");
    assert!(got[6].as_ref().unwrap_err().contains("*sine"));
    assert_eq!(got[7].as_ref().unwrap_err(), "play on no key");
    assert!(got[8].as_ref().unwrap_err().contains("at random"));
}

#[test]
fn other_sections_are_counted() {
    let s = parsed("<curve> v000=0\n<region> sample=a.wav\n<effect> type=reverb\n<region> sample=b.wav\n");
    assert_eq!(s.regions.len(), 2);
    assert_eq!(s.other_headers.get("<curve>"), Some(&1));
    assert_eq!(op(&s, 1, "type"), None);
}

/// A WAV of `frames` frames of a quiet ramp, mono, 48 kHz.
fn wav(path: &Path, frames: u32) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        w.write_sample(((i % 100) as i16 - 50) * 100).unwrap();
    }
    w.finalize().unwrap();
}

fn std_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")
}

#[test]
fn an_import_writes_a_pack_and_says_what_it_left_out() {
    let d = scratch("import");
    std::fs::create_dir_all(d.join("in/samples")).unwrap();
    for f in ["c4-soft.wav", "c4-loud.wav", "rel.wav"] {
        wav(&d.join("in/samples").join(f), 4800);
    }
    std::fs::write(d.join("in/LICENSE"), "Creative Commons Attribution 4.0 International Public License\n").unwrap();
    std::fs::write(
        d.join("in/Tiny Keys.sfz"),
        "// Tiny Keys, recorded by Ada.\n<control> default_path=samples/\n<global> amp_veltrack=80\n\
         <group> lovel=1 hivel=64\n<region> sample=c4-soft.wav key=c4\n<group> lovel=65\n<region> sample=C4-LOUD.wav key=c4\n\
         <group> trigger=release volume=-20 amp_veltrack=90\n<region> sample=rel.wav key=60\n<region> sample=rel.wav key=61 locc64=64\n",
    )
    .unwrap();
    let name = PackName::new("@ada/tiny-keys").unwrap();
    let folder = d.join("pack");
    let i = Import {
        sfz: &d.join("in/Tiny Keys.sfz"),
        folder: &folder,
        name: &name,
        label: None,
        category: Some("Keys"),
        license: None,
        std: &std_dir(),
    };
    let done = import(&i).unwrap();
    assert_eq!((done.id.as_str(), done.regions, done.samples), ("tiny-keys", 3, 3));
    assert!(done.left_out.contains(&"left out: 1 region plays only with a controller moved (CC 64)".to_string()), "{:?}", done.left_out);
    assert!(done.left_out.iter().any(|l| l.contains("has its own amp_veltrack (90%)")), "{:?}", done.left_out);
    let t = std::fs::read_to_string(folder.join("instruments/tiny-keys.toml")).unwrap();
    let inst = m2_song::instrument::DataInstrument::read(&t).unwrap();
    assert_eq!((inst.label.as_str(), inst.category.as_str(), inst.mode.as_deref()), ("Tiny Keys", "Keys", Some("gate")));
    assert_eq!(inst.controls, [("velocity", 0.8)]);
    assert_eq!(inst.regions.len(), 3);
    assert!(t.contains("file = \"samples/C4-LOUD.wav\"") || t.contains("file = \"samples/c4-loud.wav\""), "{t}");
    assert!(folder.join("samples/c4-loud.wav").is_file() || folder.join("samples/C4-LOUD.wav").is_file());
    let m = std::fs::read_to_string(folder.join("m2pkg.toml")).unwrap();
    assert!(m.contains("name = \"@ada/tiny-keys\"") && m.contains("license = \"CC-BY-4.0\""), "{m}");
    let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
    assert!(readme.contains("Tiny Keys, recorded by Ada.") && readme.contains("controller moved"), "{readme}");
    assert!(folder.join("LICENSE").is_file());
    assert_eq!(import(&i).unwrap_err(), format!("{} exists already: name a new folder for the pack", folder.display()));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn a_flac_or_a_missing_sample_stops_the_import_before_anything_is_written() {
    let d = scratch("flac");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("i.sfz"), "<region> sample=a.flac\n").unwrap();
    let name = PackName::new("@ada/x").unwrap();
    let folder = d.join("pack");
    let i = Import {
        sfz: &d.join("i.sfz"),
        folder: &folder,
        name: &name,
        label: None,
        category: None,
        license: Some("CC0-1.0"),
        std: &std_dir(),
    };
    assert_eq!(import(&i).unwrap_err(), "a.flac is a FLAC file: convert it to WAV first; music2 reads only WAV for now");
    assert!(!folder.exists());
    std::fs::write(d.join("i.sfz"), "<region> sample=gone.wav\n").unwrap();
    assert!(import(&i).unwrap_err().starts_with("gone.wav, which the SFZ plays, isn't there"));
    wav(&d.join("gone.wav"), 10);
    let i = Import { license: None, ..i };
    assert!(import(&i).unwrap_err().contains("--license"), "no licence, none beside it");
    assert!(!folder.exists());
    std::fs::remove_dir_all(&d).ok();
}

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
    assert_eq!(
        (op(&s, 0, "sample"), op(&s, 0, "lokey")),
        (Some("one.wav"), Some("60")),
        "a define in a value (`key` is lokey, hikey and pitch_keycenter)"
    );
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
    std::fs::write(
        d.join("in/LICENSE"),
        "SPDX-License-Identifier: CC-BY-4.0\n\nCreative Commons Attribution 4.0 International Public License\n",
    )
    .unwrap();
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
        check: &|_| vec![],
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
        check: &|_| vec![],
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

/// `Import` of `sfz` into `folder` as `@ada/x`, licence CC0.
fn import_into(sfz: &Path, folder: &Path) -> Result<Imported, String> {
    let name = PackName::new("@ada/x").unwrap();
    import(&Import { sfz, folder, name: &name, label: None, category: None, license: Some("CC0-1.0"), std: &std_dir(), check: &|_| vec![] })
}

/// A link at `link` to `to`.
#[cfg(unix)]
fn link(to: &Path, link: &Path) {
    std::os::unix::fs::symlink(to, link).unwrap();
}

// PACK-14: a licence file that is a link (to a key, say) is refused, and nothing is written.
#[cfg(unix)]
#[test]
fn a_licence_link_is_refused() {
    let d = scratch("lic-link");
    std::fs::create_dir_all(d.join("in")).unwrap();
    std::fs::write(d.join("secret"), "-----BEGIN PRIVATE KEY-----\n").unwrap();
    wav(&d.join("in/a.wav"), 10);
    std::fs::write(d.join("in/i.sfz"), "<region> sample=a.wav\n").unwrap();
    link(&d.join("secret"), &d.join("in/LICENSE"));
    let e = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap_err();
    assert!(e.contains("LICENSE is a link"), "{e}");
    assert!(!d.join("pack").exists());
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: a sample that is a link, or reached through one, is refused, naming its opcode and line.
#[cfg(unix)]
#[test]
fn a_sample_link_is_refused() {
    let d = scratch("sample-link");
    std::fs::create_dir_all(d.join("in/samples")).unwrap();
    std::fs::create_dir_all(d.join("private")).unwrap();
    wav(&d.join("private/voice-memo.wav"), 10);
    link(&d.join("private/voice-memo.wav"), &d.join("in/samples/c4.wav"));
    link(&d.join("private"), &d.join("in/more"));
    for (sfz, said) in [
        ("<region> key=60\n<region> sample=samples/c4.wav key=61\n", "i.sfz:2: `sample=samples/c4.wav`: samples/c4.wav is a link"),
        ("<region> sample=more/voice-memo.wav\n", "i.sfz:1: `sample=more/voice-memo.wav`: more is a link"),
        ("<control> default_path=more/\n<region> sample=voice-memo.wav\n", "after `default_path=more/` (i.sfz:1)"),
    ] {
        std::fs::write(d.join("in/i.sfz"), sfz).unwrap();
        let e = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap_err();
        assert!(e.contains(said), "{sfz}: {e}");
        assert!(!d.join("pack").exists());
    }
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: `..` above the SFZ's folder is refused, from an absolute SFZ path and a relative one.
#[test]
fn dots_above_the_folder_are_refused() {
    let d = scratch("dots");
    std::fs::create_dir_all(d.join("in/samples")).unwrap();
    wav(&d.join("secret.wav"), 10);
    wav(&d.join("in/secret.wav"), 10);
    wav(&d.join("in/samples/a.wav"), 10);
    let relative = |p: &Path| -> PathBuf {
        let here = std::env::current_dir().unwrap();
        let ups = here.components().count() - 1;
        let mut r = PathBuf::new();
        for _ in 0..ups {
            r.push("..");
        }
        r.join(p.strip_prefix("/").unwrap())
    };
    for sfz in [d.join("in/i.sfz"), relative(&d.join("in/i.sfz"))] {
        for (text, said) in [
            ("<region> sample=../secret.wav\n", "i.sfz:1: `sample=../secret.wav` leaves the SFZ's folder"),
            ("<region> sample=samples/../../secret.wav\n", "leaves the SFZ's folder"),
            ("<control> default_path=../\n<region> sample=secret.wav\n", "after `default_path=../` (i.sfz:1)"),
            ("<region> sample=..\\secret.wav\n", "leaves the SFZ's folder"),
        ] {
            std::fs::write(d.join("in/i.sfz"), text).unwrap();
            let e = import_into(&sfz, &d.join("pack")).unwrap_err();
            assert!(e.contains(said), "{text}: {e}");
            assert!(!d.join("pack").exists());
        }
    }
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: an absolute path is refused, in a sample and in an include.
#[test]
fn an_absolute_path_is_refused() {
    let d = scratch("absolute");
    std::fs::create_dir_all(d.join("in")).unwrap();
    wav(&d.join("in/a.wav"), 10);
    let abs = d.join("in/a.wav");
    std::fs::write(d.join("in/i.sfz"), format!("<region> sample={}\n", abs.display())).unwrap();
    let e = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap_err();
    assert!(e.contains(&format!("i.sfz:1: `sample={}` is an absolute path", abs.display())), "{e}");
    std::fs::write(d.join("in/i.sfz"), "<region> sample=C:\\Samples\\a.wav\n").unwrap();
    let e = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap_err();
    assert!(e.contains("is an absolute path"), "{e}");
    std::fs::write(d.join("in/i.sfz"), format!("#include \"{}\"\n<region> sample=a.wav\n", d.join("in/x.sfz").display())).unwrap();
    std::fs::write(d.join("in/x.sfz"), "").unwrap();
    let e = parse(&d.join("in/i.sfz")).unwrap_err();
    assert!(e.contains("i.sfz:1: `#include") && e.contains("is an absolute path"), "{e}");
    assert!(!d.join("pack").exists());
    std::fs::remove_dir_all(&d).ok();
}

/// `parse` on its own thread, or a panic if it takes more than 10 s (a FIFO would hang it).
fn parse_soon(path: &Path) -> Result<Sfz, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let path = path.to_path_buf();
    std::thread::spawn(move || tx.send(parse(&path)).ok());
    rx.recv_timeout(std::time::Duration::from_secs(10)).expect("parse never hangs")
}

// PACK-14: an SFZ file over 4 MB is refused, unread.
#[test]
fn a_file_too_big_is_refused() {
    let d = scratch("big");
    let line = "// a comment line in a file far too big for an instrument's text, again and again\n";
    std::fs::write(d.join("big.txt"), line.repeat((4 << 20) / line.len() + 1)).unwrap();
    std::fs::write(d.join("i.sfz"), "<region> sample=a.wav\n#include \"big.txt\"\n").unwrap();
    let e = parse_soon(&d.join("i.sfz")).unwrap_err();
    assert!(e.contains("big.txt is over 4 MB"), "{e}");
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: #define names that grow the text past 8 MB are refused as they grow, not after.
#[test]
fn defines_that_grow_too_much_are_refused() {
    let d = scratch("laughs");
    let mut text = String::from("#define $A aaaaaaaaaa\n");
    for (n, prev) in ["B", "C", "D", "E", "F", "G", "H", "I", "J"].iter().zip(["A", "B", "C", "D", "E", "F", "G", "H", "I"]) {
        text.push_str(&format!("#define ${n} {}\n", format!("${prev}").repeat(10)));
    }
    text.push_str("<region> sample=$J.wav\n");
    std::fs::write(d.join("i.sfz"), text).unwrap();
    let e = parse_soon(&d.join("i.sfz")).unwrap_err();
    assert!(e.contains("8 MB"), "{e}");
    // Many uses of one name count as much.
    let text = format!("#define $A {}\n{}", "a".repeat(4000), "<region> sample=$A.wav\n".repeat(3000));
    std::fs::write(d.join("i.sfz"), text).unwrap();
    let e = parse_soon(&d.join("i.sfz")).unwrap_err();
    assert!(e.contains("8 MB"), "{e}");
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: #include more than 16 files deep, or of a file already being read, is refused.
#[test]
fn includes_too_deep_or_in_a_loop_are_refused() {
    let d = scratch("deep");
    for n in 0..20 {
        std::fs::write(d.join(format!("f{n}.sfz")), format!("<region> sample=a{n}.wav\n#include \"f{}.sfz\"\n", n + 1)).unwrap();
    }
    std::fs::write(d.join("f20.sfz"), "").unwrap();
    let e = parse_soon(&d.join("f0.sfz")).unwrap_err();
    assert!(e.contains("16 files deep"), "{e}");
    std::fs::write(d.join("a.sfz"), "#include \"b.sfz\"\n").unwrap();
    std::fs::write(d.join("b.sfz"), "<region> sample=x.wav\n#include \"a.sfz\"\n").unwrap();
    let e = parse_soon(&d.join("a.sfz")).unwrap_err();
    assert!(e.contains("b.sfz:2: `#include \"a.sfz\"`: a.sfz would include itself (a.sfz, b.sfz, a.sfz)"), "{e}");
    // Ten includes of the next, seventeen deep: each counts, so the text's bound stops it.
    for n in 0..16 {
        std::fs::write(d.join(format!("w{n}.sfz")), format!("#include \"w{}.sfz\"\n", n + 1).repeat(10)).unwrap();
    }
    std::fs::write(d.join("w16.sfz"), "<region> sample=a.wav\n").unwrap();
    let e = parse_soon(&d.join("w0.sfz")).unwrap_err();
    assert!(e.contains("8 MB") || e.contains("100 000 regions"), "{e}");
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: an include that isn't a plain file (a FIFO, a folder, a device) is refused, never read.
#[cfg(unix)]
#[test]
fn an_include_that_isnt_a_plain_file_is_refused() {
    let d = scratch("fifo");
    let made = std::process::Command::new("mkfifo").arg(d.join("pipe.sfz")).status().unwrap();
    assert!(made.success());
    std::fs::create_dir_all(d.join("folder.sfz")).unwrap();
    for (text, said) in [
        ("#include \"pipe.sfz\"\n", "pipe.sfz isn't a plain file"),
        ("#include \"folder.sfz\"\n", "folder.sfz isn't a plain file"),
        ("#include \"/dev/zero\"\n", "is an absolute path"),
        ("#include \"../../../../../../../../dev/zero\"\n", "leaves the SFZ's folder"),
    ] {
        std::fs::write(d.join("i.sfz"), text).unwrap();
        let e = parse_soon(&d.join("i.sfz")).unwrap_err();
        assert!(e.contains(said), "{text}: {e}");
    }
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: more opcodes in all the regions than music2 keeps (each with what it inherits) are refused.
#[test]
fn too_many_opcodes_are_refused() {
    let d = scratch("opcodes");
    let global: String = (0..1000).map(|n| format!("x{n}=1 ")).collect();
    std::fs::write(d.join("i.sfz"), format!("<global> {global}\n{}", "<region>\n".repeat(1001))).unwrap();
    let e = parse_soon(&d.join("i.sfz")).unwrap_err();
    assert!(e.contains("1 000 000 opcodes"), "{e}");
    std::fs::write(d.join("i.sfz"), "<region>".repeat(100_001)).unwrap();
    let e = parse_soon(&d.join("i.sfz")).unwrap_err();
    assert!(e.contains("100 000 regions"), "{e}");
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: one recording named two ways (capitals, `./`) is one file in the pack.
#[test]
fn one_recording_named_two_ways_is_one_file() {
    let d = scratch("two-ways");
    std::fs::create_dir_all(d.join("in/Samples")).unwrap();
    wav(&d.join("in/Samples/Piano_C4.wav"), 10);
    std::fs::write(
        d.join("in/i.sfz"),
        "<region> sample=Samples/Piano_C4.wav key=60\n<region> sample=samples/piano_c4.wav key=61\n<region> sample=./Samples/Piano_C4.wav key=62\n",
    )
    .unwrap();
    let done = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap();
    assert_eq!((done.regions, done.samples), (3, 1));
    let t = std::fs::read_to_string(d.join("pack/instruments/x.toml")).unwrap();
    assert_eq!(t.matches("file = \"samples/Piano_C4.wav\"").count(), 3, "{t}");
    std::fs::remove_dir_all(&d).ok();
}

/// `Import` of `sfz` into `folder` as `name`, licence CC0.
fn import_named(sfz: &Path, folder: &Path, name: &str) -> Result<Imported, String> {
    let name = PackName::new(name).unwrap();
    import(&Import { sfz, folder, name: &name, label: None, category: None, license: Some("CC0-1.0"), std: &std_dir(), check: &|_| vec![] })
}

// PACK-14: what check would refuse is refused before anything is written: an id that can't be one,
// and a round robin missing a take.
#[test]
fn what_check_would_refuse_is_refused_first() {
    let d = scratch("first");
    std::fs::create_dir_all(d.join("in")).unwrap();
    wav(&d.join("in/a.wav"), 10);
    std::fs::write(d.join("in/i.sfz"), "<region> sample=a.wav\n").unwrap();
    for name in ["@ada/new", "@ada/static", "@ada/abcdefghij-abcdefghij-abcdefghij-abc"] {
        let e = import_named(&d.join("in/i.sfz"), &d.join("pack"), name).unwrap_err();
        assert!(e.contains("can't be an instrument's id"), "{name}: {e}");
        assert!(!d.join("pack").exists(), "{name}");
    }
    std::fs::write(
        d.join("in/i.sfz"),
        "<group> seq_length=3\n<region> sample=a.wav key=60 seq_position=1\n<region> sample=a.wav key=60 seq_position=2\n",
    )
    .unwrap();
    let e = import_named(&d.join("in/i.sfz"), &d.join("pack"), "@ada/x").unwrap_err();
    assert!(e.contains("round robin") && e.contains("take 3") && e.contains("i.sfz:2"), "{e}");
    assert!(!d.join("pack").exists());
    std::fs::remove_dir_all(&d).ok();
}

/// The import of `sfz` (the text of `in/i.sfz`, playing `a.wav`) as `@ada/x`: its instrument's file
/// and what it said.
fn imported_text(what: &str, sfz: &str) -> (String, Vec<String>) {
    let d = scratch(what);
    std::fs::create_dir_all(d.join("in")).unwrap();
    wav(&d.join("in/a.wav"), 10);
    std::fs::write(d.join("in/i.sfz"), sfz).unwrap();
    let done = import_into(&d.join("in/i.sfz"), &d.join("pack")).unwrap();
    let t = std::fs::read_to_string(d.join("pack/instruments/x.toml")).unwrap();
    std::fs::remove_dir_all(&d).ok();
    (t, done.left_out)
}

// PACK-14: the instrument's velocity and envelope come from what 9 in 10 of the regions that play at
// the attack say, a region that doesn't set one counting as SFZ's default; release regions don't vote.
#[test]
fn instrument_values_come_from_what_nearly_every_attack_region_says() {
    // As Salamander's V3: a few top notes ring 4 s, release regions say 82 %.
    let sfz = format!(
        "{}{}{}",
        "<region> sample=a.wav key=60\n".repeat(8),
        "<region> sample=a.wav key=61 ampeg_release=4\n".repeat(2),
        "<region> sample=a.wav key=60 trigger=release amp_veltrack=82 ampeg_release=1\n".repeat(5)
    );
    let (t, said) = imported_text("vote", &sfz);
    assert!(!t.contains("release =") && !t.contains("velocity = 0"), "{t}");
    assert!(
        said.iter().any(|l| l.contains("don't agree on ampeg_release") && l.contains("8 don't set it") && l.contains("2 have 4 s")),
        "{said:?}"
    );
    assert!(said.iter().any(|l| l.contains("5 regions have their own amp_veltrack (82%)")), "{said:?}");
    // Nearly all agree: theirs, and the others said.
    let sfz =
        format!("{}{}", "<region> sample=a.wav key=60 ampeg_release=1\n".repeat(19), "<region> sample=a.wav key=61 ampeg_release=4\n");
    let (t, said) = imported_text("vote2", &sfz);
    assert!(t.contains("release = 1\n"), "{t}");
    assert!(
        said.iter().any(|l| l == "changed: 1 region has its own ampeg_release (4 s): the instrument's release is 1 s for all"),
        "{said:?}"
    );
    // Beyond the Sampler's range: at its end, said, never a file that doesn't read.
    let (t, said) = imported_text("vote4", "<global> ampeg_release=10 amp_veltrack=-50\n<region> sample=a.wav key=60\n");
    assert!(t.contains("release = 5\n") && t.contains("velocity = 0\n"), "{t}");
    assert!(said.iter().any(|l| l.contains("ampeg_release is 10 s, beyond the Sampler's 0 s to 5 s")), "{said:?}");
    assert!(said.iter().any(|l| l.contains("amp_veltrack is -50%, beyond the Sampler's 0% to 100%")), "{said:?}");
    let sfz = format!("<global> amp_veltrack=73\n{}", "<region> sample=a.wav key=60\n".repeat(3));
    let (t, said) = imported_text("vote3", &sfz);
    assert!(t.contains("velocity = 0.73\n"), "{t}");
    assert!(said.iter().any(|l| l.contains("a straight line") && l.contains("up to 3.3 dB louder")), "{said:?}");
}

// PACK-14: <control>'s octave_offset and note_offset move every note after them.
#[test]
fn octave_and_note_offsets_move_the_notes() {
    let m = one("<control> octave_offset=-1 note_offset=2\n<region> sample=x.wav lokey=60 hikey=62 pitch_keycenter=61\n");
    assert_eq!((m.keys, m.root), ((50, 52), 51));
    let m = one("<control> note_offset=1\n<region> sample=x.wav key=c4\n");
    assert_eq!((m.keys, m.root), ((61, 61), 61));
    let m = one("<region> sample=x.wav key=60\n<control> note_offset=-12\n<region> sample=x.wav key=60\n");
    assert_eq!(m.keys, (60, 60), "only after it");
}

// PACK-14: a value music2 can't read leaves its region out, saying so; never read as 0 or 60.
#[test]
fn a_value_that_doesnt_read_leaves_its_region_out() {
    let got = mapped(
        "<region> sample=a.wav pitch_keycenter=sample\n<region> sample=b.wav tune=+5c\n<region> sample=c.wav volume=-6dB\n\
         <region> sample=d.wav lokey=h4\n<region> sample=e.wav locc64=lots\n",
    );
    assert!(got[0].as_ref().unwrap_err().contains("`pitch_keycenter=sample`"), "{:?}", got[0]);
    assert_eq!(got[1].as_ref().unwrap_err(), "have `tune=+5c`, which isn't a number");
    assert_eq!(got[2].as_ref().unwrap_err(), "have `volume=-6dB`, which isn't a number");
    assert_eq!(got[3].as_ref().unwrap_err(), "have `lokey=h4`, which isn't a note");
    assert_eq!(got[4].as_ref().unwrap_err(), "have `locc64=lots`, which isn't a number");
}

// PACK-14: a `$NAME` no #define made stops the parse, naming it.
#[test]
fn an_undefined_name_is_an_error() {
    let d = scratch("undefined");
    std::fs::write(d.join("i.sfz"), "#define $HI 64\n<region> sample=a.wav lokey=$LO hikey=$HI\n").unwrap();
    assert_eq!(parse(&d.join("i.sfz")).unwrap_err(), "i.sfz:2: `$LO` isn't defined: add `#define $LO …` above it");
    std::fs::write(d.join("i.sfz"), "#define $B $A\n").unwrap();
    assert!(parse(&d.join("i.sfz")).unwrap_err().contains("`$A` isn't defined"));
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: <control> opcodes music2 doesn't read, and opcodes before the first header, are named.
#[test]
fn control_opcodes_and_loose_ones_are_named() {
    let (_, said) = imported_text("control", "volume=-3\n<control> hint_ram_based=1 set_cc7=100 label_cc7=Vol\n<region> sample=a.wav\n");
    let ignored = said.iter().find(|l| l.starts_with("ignored: <control>")).unwrap_or_else(|| panic!("{said:?}"));
    assert!(ignored.contains("hint_ram_based") && ignored.contains("label_cc7") && !ignored.contains("set_cc7"), "{ignored}");
    assert!(said.iter().any(|l| l.contains("before the first header") && l.contains("volume")), "{said:?}");
}

// PACK-14: a UTF-8 byte order mark at the start of a file is skipped; an opcode's name is letters,
// digits and `_`.
#[test]
fn a_byte_order_mark_is_skipped() {
    let s = parsed("\u{FEFF}<group> lovel=1 hivel=64\n<region> sample=a.wav\n<group> lovel=65\n<region> sample=b.wav\n");
    assert_eq!(s.regions.len(), 2);
    assert_eq!(op(&s, 0, "hivel"), Some("64"));
    let s = parsed("\u{FEFF}// Recorded by Ada.\n<region> sample=a.wav\n");
    assert_eq!((s.regions.len(), s.comments.clone()), (1, vec!["Recorded by Ada.".to_string()]));
    let d = scratch("names");
    std::fs::write(d.join("i.sfz"), "<region> sample=a.wav\nfoo bar=1\n").unwrap();
    assert_eq!(parse(&d.join("i.sfz")).unwrap_err(), "i.sfz:2: `foo bar` isn't an opcode's name: letters, digits and `_`");
    std::fs::remove_dir_all(&d).ok();
}

// PACK-14: `key=` sets lokey, hikey and pitch_keycenter where it is written, so a region's beats
// what it inherits; an alias beats an inherited name too.
#[test]
fn key_beats_what_a_region_inherits() {
    let m = one("<global> pitch_keycenter=60 lokey=0 hikey=127\n<region> sample=a.wav key=64\n");
    assert_eq!((m.keys, m.root), ((64, 64), 64));
    let m = one("<group> key=64\n<region> sample=a.wav pitch_keycenter=60\n");
    assert_eq!((m.keys, m.root), ((64, 64), 60), "a later opcode beats key");
    let m = one("<group> loop_mode=no_loop loop_start=5\n<region> sample=a.wav loopmode=loop_sustain loopstart=10 loopend=90\n");
    assert_eq!(m.loop_, Some((10, 90)));
}

// PACK-14: a licence is recognised only from a clear file: an SPDX line, or a whole known text.
#[test]
fn a_licence_is_recognised_only_when_clear() {
    let mixed = "The recordings are licensed under Creative Commons Attribution-NonCommercial 4.0 International.\n\
                 The SFZ mapping files are dedicated to the public domain (CC0 1.0).\n";
    assert_eq!(recognise(mixed), None);
    assert_eq!(recognise("CC0 1.0 Universal\n\nNo rights reserved.\n"), None, "a title is not the text");
    assert_eq!(recognise("Attribution 4.0 International\n"), None);
    assert_eq!(recognise("SPDX-License-Identifier: CC-BY-4.0\n\nRecorded by Ada.\n").map(|r| r.0), Some("CC-BY-4.0".to_string()));
    let cc0 = std::fs::read_to_string(sfz_fixture().join("LICENSE")).unwrap();
    assert_eq!(recognise(&cc0).map(|r| r.0), Some("CC0-1.0".to_string()), "the whole CC0 1.0 text");
    let wrapped = cc0.split_whitespace().collect::<Vec<_>>().join("\n");
    assert_eq!(recognise(&wrapped).map(|r| r.0), Some("CC0-1.0".to_string()), "spaces and line breaks aside");
    assert_eq!(recognise(&format!("{cc0}\nThe samples are NonCommercial.\n")), None, "the whole text and more is not it");
}

fn sfz_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures/sfz/tiny-keys")
}

// PACK-14: the pack is written beside the folder named and checked first: one check refuses
// leaves nothing, beside the folder or in it.
#[test]
fn a_pack_check_refuses_leaves_nothing() {
    let d = scratch("check-refuses");
    std::fs::create_dir_all(d.join("in")).unwrap();
    wav(&d.join("in/a.wav"), 10);
    std::fs::write(d.join("in/i.sfz"), "<region> sample=a.wav\n").unwrap();
    let name = PackName::new("@ada/x").unwrap();
    let seen = std::cell::RefCell::new(None);
    let check = |dir: &Path| {
        *seen.borrow_mut() = Some((dir.to_path_buf(), dir.join("instruments/x.toml").is_file()));
        vec!["instruments/x.toml:3:1: something check refuses".to_string()]
    };
    let folder = d.join("out/pack");
    let i = Import {
        sfz: &d.join("in/i.sfz"),
        folder: &folder,
        name: &name,
        label: None,
        category: None,
        license: Some("CC0-1.0"),
        std: &std_dir(),
        check: &check,
    };
    let e = import(&i).unwrap_err();
    assert!(e.contains("doesn't pass m2 pack check") && e.contains("something check refuses"), "{e}");
    let (checked, whole) = seen.borrow().clone().expect("checked");
    assert!(whole && checked.parent() == folder.parent() && checked != folder, "checked whole, beside it: {checked:?}");
    assert_eq!(std::fs::read_dir(d.join("out")).unwrap().count(), 0, "nothing is left");
    std::fs::remove_dir_all(&d).ok();
}

//! A WAV dropped on a sampler's card: the file comes in, then one Tx names it in the call; undo
//! takes the code back, and the file stays.

use m2_session::Session;

use super::*;
use crate::Gestures;
use crate::import::{self, Imported, Source};
use crate::{EditError, Edited};
use crate::testing::fixture;

/// A short WAV whose bytes differ by `seed`.
fn tone(seed: f32) -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut out = std::io::Cursor::new(vec![]);
    let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
    for i in 0..4_410 {
        w.write_sample(((i as f32 * 0.03 + seed).sin() * 12_000.0) as i16).unwrap();
    }
    w.finalize().unwrap();
    out.into_inner()
}

/// The id of the project's call to `callee`, and its site.
fn call(s: &Session, callee: &str) -> (String, u32) {
    let site = s.accepted().compiled.sites.iter().find(|x| x.in_project && x.callee == callee).unwrap_or_else(|| panic!("no call `{callee}`"));
    (format!("{}#{}", site.file, site.ordinal), site.id)
}

/// `file` dropped on the card of the project's call to `callee`.
fn drop_on(s: &mut Session, callee: &str, file: &Imported) -> Result<Edited, EditError> {
    let ((id, _), at) = (call(s, callee), s.accepted().number);
    set_sample(s, at, &id, file)
}

/// What the Device view shows on the sampler's card.
fn shown(s: &Session) -> Option<SamplerCard> {
    s.view().modules.iter().flat_map(|m| &m.nodes).find(|n| n.component.as_deref() == Some(SAMPLER)).and_then(|n| n.sampler.clone())
}

fn files(s: &Session) -> Vec<String> {
    let (_, site) = call(s, "sampler");
    card(&s.accepted().compiled, site).expect("a sampler's card").files
}

#[test]
fn dropping_a_wav_sets_the_sample_and_undo_takes_it_back() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let made = s.add_track(SAMPLER, None, None, None).unwrap();
    let code = made.edited.file.clone();
    let before = f.read(&code);
    assert!(before.contains("sampler(lane.notes(\"notes\"))"), "{before}");
    assert_eq!(shown(&s), Some(SamplerCard { files: vec![] }), "no file yet: its card says to drop one");

    // The drop: the file is imported (create-only), then one Tx writes its name into the call.
    let (kick, _) = import::audio(&f.0, Source::Bytes { name: "Kick 1.wav".into(), bytes: tone(0.0) }).unwrap();
    let name = kick.asset.as_str().to_string();
    let undo_before = s.accepted().number;
    let done = drop_on(&mut s, "sampler", &kick).unwrap();
    assert_eq!(f.read(&code), before.replace("sampler(lane.notes(\"notes\"))", &format!("sampler(lane.notes(\"notes\"), {{ sample: \"{name}\" }})")));
    assert_eq!((done.before.as_str(), done.after.as_str()), ("sampler(lane.notes(\"notes\"))", format!("sampler(lane.notes(\"notes\"), {{ sample: \"{name}\" }})").as_str()));
    // The graph: its voices read the file, which the revision knows by its hash, so it plays at once.
    assert_eq!(files(&s), std::slice::from_ref(&name));
    assert_eq!(shown(&s), Some(SamplerCard { files: vec![name.clone()] }), "its card names the file");
    assert!(s.accepted().compiled.data.kernel_assets().contains_key(&name), "the revision knows the file");
    assert!(s.problems().iter().all(|p| p.code != "asset.missing"), "{:?}", s.problems());

    // A second drop replaces the name written there.
    let (snare, _) = import::audio(&f.0, Source::Bytes { name: "snare.wav".into(), bytes: tone(1.0) }).unwrap();
    drop_on(&mut s, "sampler", &snare).unwrap();
    assert!(f.read(&code).contains("{ sample: \"snare.wav\" }"), "{}", f.read(&code));
    assert_eq!(files(&s), ["snare.wav".to_string()]);
    let again = drop_on(&mut s, "sampler", &snare).unwrap_err().to_string();
    assert!(again.contains("plays snare.wav already"), "{again}");

    // Undo, twice: the code's bytes are as they were; both files stay in assets/.
    crate::undo(&mut s).unwrap();
    crate::undo(&mut s).unwrap();
    assert_eq!(f.read(&code), before);
    assert_eq!(files(&s), Vec::<String>::new());
    assert!(s.accepted().number > undo_before);
    for file in [name.as_str(), "snare.wav"] {
        assert!(f.0.join("assets").join(file).is_file(), "{file} stays");
    }
}

#[test]
fn a_sampler_with_zones_or_another_call_is_refused_in_words() {
    let f = fixture("templates/empty");
    let mut s = f.open();
    let made = s.add_track(SAMPLER, None, None, None).unwrap();
    let code = made.edited.file.clone();
    let text = f.read(&code).replace("sampler(lane.notes(\"notes\"))", "sampler(lane.notes(\"notes\"), { zones: { 36: \"a.wav\" } })");
    f.write(&code, &text);
    assert!(s.refresh());
    let (kick, _) = import::audio(&f.0, Source::Bytes { name: "kick.wav".into(), bytes: tone(0.0) }).unwrap();
    let e = drop_on(&mut s, "sampler", &kick).unwrap_err().to_string();
    assert!(e.contains("plays zones"), "{e}");
    let e = drop_on(&mut s, "lane.notes", &kick).unwrap_err().to_string();
    assert!(e.contains("isn't a sampler"), "{e}");
    assert_eq!(f.read(&code), text, "nothing written");
}

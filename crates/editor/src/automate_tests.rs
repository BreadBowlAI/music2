//! Automating a knob: a literal, a quantity and an inherited control, in the song and in a track,
//! for each way a module's function takes its context; each sounds the same before and after, and
//! undo takes the lane away.

use m2_session::Session;

use super::*;
use crate::testing::{Fixture, MANIFEST, fixture};
use crate::{Gestures, TimelineLane};

fn render(s: &Session) -> Vec<[f32; 2]> {
    let rev = s.accepted();
    m2_runtime::Renderer::new(&rev.compiled.flat, &rev.compiled.data, &s.packages, 48_000).unwrap().render(3.0).unwrap()
}

/// The control lane `module › lane` as the timeline shows it: its unit, its fallback and its
/// clips.
fn lane(s: &Session, module: &str, key: &str) -> Option<(Option<&'static str>, Option<f64>, usize)> {
    let t = s.timeline();
    let track = t.tracks.into_iter().find(|t| t.module == module)?;
    track.lanes.into_iter().find_map(|l| match l {
        TimelineLane::Control { key: k, unit, default, clips, .. } if k == key => Some((unit, default, clips.len())),
        _ => None,
    })
}

/// Every file of the project but `.music2/`, by path, with its bytes.
fn tree(f: &Fixture) -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<_> = crate::testing::files_under(&f.0)
        .into_iter()
        .filter(|p| !p.strip_prefix(&f.0).unwrap().starts_with(".music2"))
        .map(|p| (p.strip_prefix(&f.0).unwrap().display().to_string(), std::fs::read(&p).unwrap()))
        .collect();
    all.sort();
    all
}

// HOME-2, WRITE-3, WRITE-10: a literal in the song becomes a control lane's fallback, read where
// it was, with an empty lane file; the song function takes its lanes for it. It sounds the same,
// the timeline shows the empty lane, and undo puts every file back as it was.
#[test]
fn a_literal_in_the_song_becomes_an_empty_lane_and_sounds_the_same() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (files, sound) = (tree(&f), render(&s));
    assert!(sound.iter().any(|x| x[0] != 0.0), "a render that says something");
    let song = f.read("song.ts");

    let a = s.automate(s.accepted().number, "song.ts#2/0.hats.pan").unwrap();
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("song/hats-pan".to_string(), "hats pan"));
    assert_eq!(
        (a.edited.file.as_str(), a.edited.before.as_str(), a.edited.after.as_str()),
        ("song.ts", "hats pan 0.3", "hats pan automated")
    );
    let want = song.replace("song(() =>", "song(({ lane }) =>").replace("pan: 0.3", "pan: lane.control(\"hats-pan\", \"bipolar\", 0.3)");
    assert_eq!(f.read("song.ts"), want);
    assert_eq!(f.read("data/song/hats-pan.json"), m2_song::canonical(&LaneData::Control { unit: Some("bipolar".into()), clips: vec![] }));
    assert_eq!(lane(&s, "song", "hats-pan"), Some((Some("bipolar"), Some(0.3), 0)), "an empty lane, drawing the fallback");
    assert!(!s.controls().iter().any(|c| c.id == "song.ts#2/0.hats.pan"), "the knob is a lane now");
    let knobs: Vec<crate::Knob> = s.view().modules.into_iter().flat_map(|m| m.nodes).flat_map(|n| n.knobs).collect();
    assert!(knobs.iter().all(|k| k.automate_refused.is_none()), "every knob of the example can be automated");
    assert_eq!(render(&s), sound, "the fallback is the value, bit for bit");
    assert!(!s.refresh(), "the session took in its own write");

    let undone = s.undo().unwrap();
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("hats pan automated", "hats pan 0.3"));
    assert_eq!(tree(&f), files, "the lane file is gone and the literal is back");
    assert_eq!(render(&s), sound);
    s.redo().unwrap();
    assert_eq!(f.read("song.ts"), want);
}

// HOME-2: a quantity is kept as written (`lane.control`'s fallback takes it); a track that reads
// its lanes already gives `lane` to it; a dictionary entry's control is named by its entry.
#[test]
fn a_quantity_is_the_fallback_as_written() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let sound = render(&s);
    let (song, bass) = (f.read("song.ts"), f.read("tracks/bass.ts"));

    let a = s.automate(s.accepted().number, "song.ts#3/0").unwrap();
    assert_eq!((a.lane.to_string(), a.edited.before.as_str()), ("song/hats-db".to_string(), "hats db db(-6)"));
    assert!(f.read("song.ts").contains("db: lane.control(\"hats-db\", \"db\", db(-6))"), "{}", f.read("song.ts"));
    assert_eq!(render(&s), sound);

    // `({ lane, time })` already: only the value changes in the track's file.
    let a = s.automate(s.accepted().number, "tracks/bass.ts#7/0").unwrap();
    let key = a.lane.lane.to_string();
    assert_eq!(a.lane.module.as_str(), "bass");
    assert_eq!(f.read("tracks/bass.ts"), bass.replacen("khz(2.5)", &format!("lane.control(\"{key}\", \"hz\", khz(2.5))"), 1));
    assert_eq!(lane(&s, "bass", &key), Some((Some("hz"), Some(2500.0), 0)));
    assert_eq!(render(&s), sound);

    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!((f.read("song.ts"), f.read("tracks/bass.ts")), (song, bass));
    assert!(!f.0.join("data/song").exists() && !f.0.join(format!("data/bass/{key}.json")).exists());
}

// HOME-2, PKG-3: a declared control the call leaves out is written into the call as the lane,
// its default the fallback as `disconnect` spells it; named by the call's binding.
#[test]
fn an_inherited_control_is_written_into_its_call_as_a_lane() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let sound = render(&s);
    let kick = f.read("tracks/kick.ts");
    let a = s.automate(s.accepted().number, "tracks/kick.ts#1/@decay").unwrap();
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("kick/out-decay".to_string(), "out decay"));
    let now = f.read("tracks/kick.ts");
    assert!(now.contains("kick(lane.notes(\"hits\"), { decay: lane.control(\"out-decay\", \"seconds\", "), "{now}");
    assert_eq!(lane(&s, "kick", "out-decay").map(|l| (l.0, l.2)), Some((Some("seconds"), 0)));
    assert_eq!(render(&s), sound, "the default, as spelled, is what played");
    s.undo().unwrap();
    assert_eq!(f.read("tracks/kick.ts"), kick);
}

/// A project of one track whose function is written `head`, around `body`, mixed by the song.
fn one_track(head: &str, body: &str) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = format!(
        "import {{ hz, song, track }} from \"@m2/core\";\nimport {{ osc }} from \"std/dsp\";\nimport {{ lowpass }} from \"std/filter\";\n\
         const lead = track(\"lead\", {head} => {body});\nexport default song(() => lead.out);\n"
    );
    f.write("song.ts", song);
    f
}

// HOME-2: each way a function takes its context: `(ctx)` reads `ctx.lane`, `({ time })` gains
// `lane`, `({ lane: l })` reads `l`; the key is the call's binding and the control.
#[test]
fn each_way_a_function_takes_its_context_reaches_the_lane() {
    let body = "{\n  const tone = lowpass(osc.saw(hz(110)), { cutoff: hz(900) });\n  return tone;\n}";
    for (head, reads, writes) in
        [("(ctx)", "ctx.lane", "(ctx)"), ("({ time })", "lane", "({ time, lane })"), ("({ lane: l })", "l", "({ lane: l })")]
    {
        let f = one_track(head, body);
        let mut s = f.open();
        assert!(s.draft_errors.is_empty(), "{head}: {:?}", s.draft_errors);
        let sound = render(&s);
        assert!(sound.iter().any(|x| x[0] != 0.0), "{head}: a render that says something");
        let id = s.controls().into_iter().find(|c| c.literal == "900").map(|c| c.id).unwrap();
        let a = s.automate(s.accepted().number, &id).unwrap();
        assert_eq!(a.lane.to_string(), "lead/tone-cutoff", "{head}");
        let text = f.read("song.ts");
        assert!(text.contains(&format!("{writes} => {{")), "{head}: {text}");
        assert!(text.contains(&format!("cutoff: {reads}.control(\"tone-cutoff\", \"hz\", hz(900))")), "{head}: {text}");
        assert_eq!(render(&s), sound, "{head}");
    }
}

// ID-2: the lane's key is never one the track has, in any capitals: a declared lane or a file.
#[test]
fn the_new_lanes_key_is_free() {
    let body = "{\n  const tone = lowpass(osc.saw(hz(110)), { cutoff: hz(900) });\n  lane.mask(\"tone-cutoff\");\n  return tone;\n}";
    let f = one_track("({ lane })", body);
    f.write("data/lead/Tone-Cutoff-2.json", "{\"unit\": \"hz\", \"clips\": []}\n");
    let mut s = f.open();
    let id = s.controls().into_iter().find(|c| c.literal == "900").map(|c| c.id).unwrap();
    let a = s.automate(s.accepted().number, &id).unwrap();
    assert_eq!(a.lane.to_string(), "lead/tone-cutoff-3");
}

// HOME-2: what can't be automated where it is says why, and nothing is written: a call shared by
// two tracks, a call outside its track's function, a function automate can't follow.
#[test]
fn a_knob_automate_cant_follow_is_refused() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { hz, song, track } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\nimport { lowpass } from \"std/filter\";\n\
                import { mixer } from \"std/mix\";\n\
                const tone = (f: number) => lowpass(osc.saw(hz(f)), { cutoff: hz(900) });\n\
                const outside = () => lowpass(osc.saw(hz(55)), { cutoff: hz(300) });\n\
                const a = track(\"a\", () => tone(110));\nconst b = track(\"b\", () => tone(220));\n\
                const c = track(\"c\", () => outside());\n\
                const d = track(\"d\", (...all) => lowpass(osc.saw(hz(70)), { cutoff: hz(400) }));\n\
                export default song(() => mixer({ a: a.out, b: b.out, c: c.out, d: d.out }));\n";
    f.write("song.ts", song);
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    // The canvas says why on each knob, without planning anything.
    let knobs: Vec<crate::Knob> = s.view().modules.into_iter().flat_map(|m| m.nodes).flat_map(|n| n.knobs).collect();
    let why = |literal: &str| knobs.iter().find(|k| k.literal == literal).and_then(|k| k.automate_refused.clone());
    assert!(why("900").is_some_and(|w| w.contains("shared by several tracks")), "{:?}", why("900"));
    assert!(why("300").is_some_and(|w| w.contains("isn't inside")) && why("400").is_some_and(|w| w.contains("rest")));
    for (literal, says) in [("900", "shared by several tracks"), ("300", "isn't inside"), ("400", "rest")] {
        let id = s.controls().into_iter().find(|c| c.literal == literal).map(|c| c.id).unwrap();
        let refused = s.automate(s.accepted().number, &id);
        assert!(matches!(&refused, Err(EditError::Rejected(why)) if why.contains(says)), "{literal}: {refused:?}");
    }
    assert_eq!(f.read("song.ts"), song);
    assert!(!f.0.join("data").exists() && !s.can_undo(), "nothing was written");
}

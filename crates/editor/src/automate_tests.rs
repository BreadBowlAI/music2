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
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("song/hats-pan".to_string(), "Hats pan"));
    assert_eq!(
        (a.edited.file.as_str(), a.edited.before.as_str(), a.edited.after.as_str()),
        ("song.ts", "Hats pan 0.3", "Hats pan automated")
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
    assert_eq!((undone.before.as_str(), undone.after.as_str()), ("Hats pan automated", "Hats pan 0.3"));
    assert_eq!(tree(&f), files, "the lane file is gone and the literal is back");
    assert_eq!(render(&s), sound);
    s.redo().unwrap();
    assert_eq!(f.read("song.ts"), want);
}

// HOME-2: a quantity is kept as written (`lane.control`'s fallback takes it); a track that reads
// its lanes already gives `lane` to it; a mixer channel's control is named by its track and the
// control's word, as the Mix view says it (`channels.hats.db` is Hats volume, never "hats db").
#[test]
fn a_quantity_is_the_fallback_as_written() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let sound = render(&s);
    let (song, bass) = (f.read("song.ts"), f.read("tracks/bass.ts"));

    let a = s.automate(s.accepted().number, "song.ts#3/0").unwrap();
    assert_eq!((a.lane.to_string(), a.edited.before.as_str()), ("song/hats-volume".to_string(), "Hats volume db(-6)"));
    assert!(f.read("song.ts").contains("db: lane.control(\"hats-volume\", \"db\", db(-6))"), "{}", f.read("song.ts"));
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
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("kick/decay".to_string(), "decay"));
    let now = f.read("tracks/kick.ts");
    assert!(now.contains("kick(lane.notes(\"hits\"), { decay: lane.control(\"decay\", \"seconds\", "), "{now}");
    assert_eq!(lane(&s, "kick", "decay").map(|l| (l.0, l.2)), Some((Some("seconds"), 0)));
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
        assert_eq!(a.lane.to_string(), "lead/cutoff", "{head}");
        let text = f.read("song.ts");
        assert!(text.contains(&format!("{writes} => {{")), "{head}: {text}");
        assert!(text.contains(&format!("cutoff: {reads}.control(\"cutoff\", \"hz\", hz(900))")), "{head}: {text}");
        assert_eq!(render(&s), sound, "{head}");
    }
}

// ID-2, D6: the lane's key is never one the track has, in any capitals: a declared lane or a
// file. On a clash the call's binding goes in front of the control's name, then a number.
#[test]
fn the_new_lanes_key_is_free() {
    let body = "{\n  const tone = lowpass(osc.saw(hz(110)), { cutoff: hz(900) });\n  lane.mask(\"cutoff\");\n  return tone;\n}";
    let f = one_track("({ lane })", body);
    f.write("data/lead/Tone-Cutoff.json", "{\"unit\": \"hz\", \"clips\": []}\n");
    let mut s = f.open();
    let id = s.controls().into_iter().find(|c| c.literal == "900").map(|c| c.id).unwrap();
    let a = s.automate(s.accepted().number, &id).unwrap();
    assert_eq!(a.lane.to_string(), "lead/tone-cutoff-2");
}

// D6, M7: a lane is named after its control: the control's label as a key, with the call's
// binding in front only on a clash. The code names the lane (`{ name }`) only when its key, read as
// words, isn't the label: "Detune (cents)" has to be written, "decay" never.
#[test]
fn the_lane_is_named_after_the_control() {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    let song = "import { secs, song, track } from \"@m2/core\";\nimport { fm } from \"@m2/synths\";\nimport { sum } from \"std/math\";\n\
                const keys = track(\"keys\", ({ lane }) => {\n  const lead = fm.custom(lane.notes(\"a\"), { detune: 5, decay: secs(2) });\n  \
                const pad = fm.custom(lane.notes(\"b\"), { decay: secs(4) });\n  return sum(lead, pad);\n});\n\
                export default song(() => keys.out);\n";
    f.write("song.ts", song);
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let id = |s: &Session, literal: &str| s.controls().into_iter().find(|c| c.literal == literal).map(|c| c.id).unwrap();
    let a = s.automate(s.accepted().number, &id(&s, "5")).unwrap();
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("keys/detune-cents".to_string(), "detune (cents)"));
    assert!(
        f.read("song.ts").contains("detune: lane.control(\"detune-cents\", \"scalar\", 5, { name: \"Detune (cents)\" })"),
        "{}",
        f.read("song.ts")
    );
    let shown = s.timeline().tracks.into_iter().flat_map(|t| t.lanes).find(|l| l.key() == "detune-cents").unwrap();
    assert!(matches!(shown, TimelineLane::Control { label, .. } if label == "Detune (cents)"));
    let a = s.automate(s.accepted().number, &id(&s, "2")).unwrap();
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("keys/decay".to_string(), "decay"));
    assert!(f.read("song.ts").contains("decay: lane.control(\"decay\", \"seconds\", secs(2)) }"), "{}", f.read("song.ts"));
    // The pad's decay: `decay` is the lead's lane now.
    let a = s.automate(s.accepted().number, &id(&s, "4")).unwrap();
    assert_eq!((a.lane.to_string(), a.name.as_str()), ("keys/pad-decay".to_string(), "pad decay"));
    assert!(f.read("song.ts").contains("decay: lane.control(\"pad-decay\", \"seconds\", secs(4)) }"), "{}", f.read("song.ts"));
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

// HOME-2, Q10: Remove automation writes what the lane plays, at the playhead or else its fallback,
// where the lane was read, in the fallback's spelling, and takes the lane file away: one edit, that
// undo takes back whole. The track function keeps its `lane`.
#[test]
fn remove_automation_puts_the_heard_value_back() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let (bass, cutoff) = (f.read("tracks/bass.ts"), f.read("data/bass/cutoff.json"));
    // The cutoff's clip rises from 300 to 2200 Hz over beats 0 to 16; the fallback is hz(500).
    for (beat, written) in [(Some(8.0), "hz(1250)"), (Some(40.0), "hz(500)"), (None, "hz(500)")] {
        let e = remove(&mut s, "bass", "cutoff", beat).unwrap();
        assert_eq!(
            (e.file.as_str(), e.before.as_str(), e.after.as_str()),
            ("tracks/bass.ts", "cutoff automated", &*format!("cutoff {written}"))
        );
        let now = f.read("tracks/bass.ts");
        assert_eq!(now, bass.replace("lane.control(\"cutoff\", \"hz\", hz(500))", written), "at {beat:?}");
        assert!(!f.0.join("data/bass/cutoff.json").exists(), "the lane file is gone");
        assert!(lane(&s, "bass", "cutoff").is_none(), "and the lane");
        s.undo().unwrap();
        assert_eq!((f.read("tracks/bass.ts"), f.read("data/bass/cutoff.json")), (bass.clone(), cutoff.clone()), "undo puts both back");
    }
    // Automate, then remove: the value is back as it was written.
    let song = f.read("song.ts");
    let a = s.automate(s.accepted().number, "song.ts#2/0.hats.pan").unwrap();
    remove(&mut s, "song", a.lane.lane.as_str(), None).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("song(() =>", "song(({ lane }) =>"), "the literal is back; the function keeps its lanes");
    assert!(!f.0.join("data/song/hats-pan.json").exists());
}

// What isn't an automation lane read in one place says why, and nothing is written.
#[test]
fn remove_automation_refuses_what_it_cant_take_out() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let why = remove(&mut s, "bass", "notes", None).unwrap_err().to_string();
    assert!(why.contains("isn't an automation lane"), "{why}");
    // Said whole, as the user reads it.
    for (module, key, says) in [("bass", "nope", "Bass has no lane `nope`."), ("nobody", "x", "There's no track `nobody`.")] {
        assert_eq!(remove(&mut s, module, key, None).unwrap_err().to_string(), says);
    }
    let why = remove(&mut s, "bass", "cutoff", Some(f64::NAN)).unwrap_err().to_string();
    assert!(why.contains("beat"), "{why}");
    assert!(!s.can_undo());
}

// A mask read into a `const` and used by name: the number written in its place is made where the
// name is read (`blend(…, drop)`), which the Tx declares, and the song plays what the lane played
// there.
#[test]
fn a_lane_read_by_name_is_taken_out() {
    let f = fixture("examples/first-song");
    let mut s = f.open();
    let bass = f.read("tracks/bass.ts");
    // The drop's clip is 1 from half a beat in until half a beat before its end (beats 16 to 32).
    remove(&mut s, "bass", "drop", Some(20.0)).unwrap();
    assert_eq!(f.read("tracks/bass.ts"), bass.replace("lane.mask(\"drop\")", "1"));
    assert!(!f.0.join("data/bass/drop.json").exists());
}

/// A project whose track `lead` plays `tone`, a sound of the project's own with a choice and a
/// switch, called as `call`.
fn with_choice(call: &str) -> Fixture {
    let f = Fixture::new();
    f.write("music2.toml", MANIFEST);
    f.write(
        "lib/tone.ts",
        "import { component, control, port } from \"@m2/core\";\nimport { osc } from \"std/dsp\";\n\
         export const tone = component(\n  { id: \"tone\", controls: { shape: control.choice([\"saw\", \"square\", \"sine\"], \"saw\"), sub: control.bool(false) }, output: port.audio() },\n  () => osc.saw(110),\n);\n",
    );
    let song = format!(
        "import {{ song, track }} from \"@m2/core\";\nimport {{ tone }} from \"./lib/tone\";\n\
         const lead = track(\"lead\", () => {call});\nexport default song(() => lead.out);\n"
    );
    f.write("song.ts", song);
    f
}

// HOME-2, CARD-14, CARD-15: Automate on a choice writes a lane of names, its fallback the name as
// written; on a switch left out of its call, its default as true or false. Each lane's file is
// a lane of names, empty; the IR's lane has the choice's unit and its place. Undo puts it back, and
// Remove automation writes the fallback back as it was.
#[test]
fn automate_writes_a_lane_of_names() {
    let f = with_choice("tone({ shape: \"square\" })");
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let song = f.read("song.ts");
    let shape = s.controls().into_iter().find(|c| c.literal == "\"square\"").expect("the choice, written by its name");
    assert_eq!((shape.value, shape.unit.map(|u| u.to_string())), (1.0, Some("choice<saw|square|sine>".into())));
    // What a menu offers (review K6), and why a number isn't written: m2 and the API can't write a name yet.
    assert_eq!(shape.options(), Some(vec!["saw", "square", "sine"]));
    let sub = s.controls().into_iter().find(|c| c.id.ends_with("/@sub")).expect("the switch left out");
    assert!(sub.is_switch() && sub.options().is_none() && !shape.is_switch());
    let e = s.set(s.accepted().number, &shape.id, 2.0, None).unwrap_err().to_string();
    assert!(
        e.contains("shape is a choice, written by its name (\"saw\", \"square\" or \"sine\"): change it in the code, as shape: \"square\""),
        "{e}"
    );
    let e = s.set(s.accepted().number, &sub.id, 1.0, None).unwrap_err().to_string();
    assert!(e.contains("sub is a switch, written true or false: change it in the code, as sub: true"), "{e}");
    let a = s.automate(s.accepted().number, &shape.id).unwrap();
    assert_eq!((a.lane.to_string(), a.edited.before.as_str()), ("lead/shape".to_string(), "shape \"square\""));
    let want =
        song.replace("() => tone({ shape: \"square\" })", "({ lane }) => tone({ shape: lane.control(\"shape\", \"choice\", \"square\") })");
    assert_eq!(f.read("song.ts"), want);
    assert_eq!(f.read("data/lead/shape.json"), "{\"unit\": \"choice\", \"clips\": []}\n");
    let decl = |s: &Session, key: &str| {
        let m = s.accepted().compiled.ir.modules.iter().find(|m| m.key == "lead").unwrap().clone();
        m.lanes.iter().find(|l| l.key == key).map(|l| (l.kind.to_string(), l.default))
    };
    assert_eq!(decl(&s, "shape"), Some(("control<choice<saw|square|sine>>".to_string(), Some(1.0))));

    // The switch the call leaves out: its default, false, written into the call as the lane.
    let sub = s.controls().into_iter().find(|c| c.id.ends_with("/@sub")).expect("the switch left out");
    s.automate(s.accepted().number, &sub.id).unwrap();
    assert!(
        f.read("song.ts")
            .contains("shape: lane.control(\"shape\", \"choice\", \"square\"), sub: lane.control(\"sub\", \"switch\", false) })"),
        "{}",
        f.read("song.ts")
    );
    assert_eq!(f.read("data/lead/sub.json"), "{\"unit\": \"switch\", \"clips\": []}\n");
    assert_eq!(decl(&s, "sub"), Some(("control<switch>".to_string(), Some(0.0))));

    // Undo, then take the choice's automation out: at the playhead, the name its lane plays there;
    // with none, its fallback as written.
    s.undo().unwrap();
    assert_eq!(f.read("song.ts"), want);
    f.write("data/lead/shape.json", "{\"unit\": \"choice\", \"clips\": [{\"start\": 0, \"length\": 4, \"points\": [[0, \"sine\"]]}]}\n");
    s.refresh();
    remove(&mut s, "lead", "shape", Some(1.0)).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("() => tone({ shape: \"square\" })", "({ lane }) => tone({ shape: \"sine\" })"));
    s.undo().unwrap();
    remove(&mut s, "lead", "shape", None).unwrap();
    assert_eq!(f.read("song.ts"), song.replace("() => tone(", "({ lane }) => tone("));
    assert!(!f.0.join("data/lead/shape.json").exists());
}

/// `with_choice`, the track's function taking its lanes and running `body`.
fn with_lanes(body: &str) -> Fixture {
    let f = with_choice("tone()");
    let song = f.read("song.ts").replace("track(\"lead\", () => tone())", &format!("track(\"lead\", ({{ lane }}) => {{\n  {body}\n}})"));
    f.write("song.ts", song);
    f
}

// CARD-14 (review K5): disconnecting a lane of names from a choice writes the choice's default as
// its name, and from a switch as true or false. The lane still feeds another choice, or else
// disconnecting it would leave a lane with no names: that is refused in words.
#[test]
fn disconnect_writes_a_choices_default_as_its_name() {
    let f = with_lanes(
        "const pick = lane.control(\"pick\", \"choice\", \"sine\");\n  const on = lane.control(\"on\", \"switch\", true);\n  tone({ shape: pick });\n  return tone({ shape: pick, sub: on });",
    );
    let mut s = f.open();
    assert!(s.draft_errors.is_empty(), "{:?}", s.draft_errors);
    let song = f.read("song.ts");
    let inlet = |s: &Session, name: &str| s.inlets().into_iter().rfind(|i| i.name == name).unwrap_or_else(|| panic!("{name} is a cable"));
    let on = inlet(&s, "on");
    let e = s.disconnect(on.revision, &on.id).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("on", "false"));
    let pick = inlet(&s, "pick");
    let e = s.disconnect(pick.revision, &pick.id).unwrap();
    assert_eq!((e.before.as_str(), e.after.as_str()), ("pick", "\"saw\""));
    assert_eq!(f.read("song.ts"), song.replace("tone({ shape: pick, sub: on })", "tone({ shape: \"saw\", sub: false })"));
    // The lane's last choice: without it, the lane would have no names.
    let pick = inlet(&s, "pick");
    let e = s.disconnect(pick.revision, &pick.id).unwrap_err();
    assert!(e.to_string().contains("pick is a lane of names, and this is the only choice it feeds"), "{e}");
}

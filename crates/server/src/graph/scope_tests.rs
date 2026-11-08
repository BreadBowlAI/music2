//! Scopes on the server (TAP-8, TAP-9): a box's last frames in every meters message while its
//! client asks, found again in each new plan, refused in words for what isn't sound, and ended by
//! a stop, a window that closes, or 10 s without an ask.

use m2_play::{Driver, Player};

use super::super::meters::MetersOut;
use super::super::tests::Fixture;
use super::*;

/// A saw bass, a sine kick under an LFO and a square pad, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { lfo } from 'std/control';\nimport { osc } from 'std/dsp';\n\
                    import { scale } from 'std/math';\nimport { mixer } from 'std/mix';\n\
                    const bass = track('bass', () => osc.saw(110));\n\
                    const kick = track('kick', () => scale(osc.sine(55), lfo(2)));\n\
                    const pad = track('pad', () => osc.square(880));\n\
                    export default song(() => mixer({ bass: bass.out, kick: kick.out, pad: pad.out }));\n";

/// How many of `x`'s frames go the same way as the one before, the most of up or down: nearly all
/// of a saw's.
fn one_way(x: &[f64]) -> usize {
    let up = x.windows(2).filter(|w| w[1] > w[0]).count();
    up.max(x.windows(2).filter(|w| w[1] < w[0]).count())
}

struct Rig {
    _fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

impl Rig {
    fn new(song: &str) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        graph.open(&fixture.0).unwrap();
        graph.watch_meters(true);
        graph.meters();
        let mut rig = Rig { _fixture: fixture, graph, driver };
        rig.fill(1);
        rig
    }

    fn fill(&mut self, blocks: usize) {
        for _ in 0..blocks {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    fn token(&self) -> u64 {
        self.graph.state().unwrap()["session"].as_u64().unwrap()
    }

    /// The view's id for the call of `callee` in `song.ts`.
    fn id(&self, callee: &str) -> String {
        let state = self.graph.state().unwrap();
        let nodes = state["view"]["modules"].as_array().unwrap().iter().flat_map(|m| m["nodes"].as_array().unwrap().clone());
        let ids: Vec<String> = nodes.filter(|n| n["label"] == callee).map(|n| n["id"].as_str().unwrap().to_string()).collect();
        assert_eq!(ids.len(), 1, "one {callee}: {ids:?}");
        ids[0].clone()
    }

    /// The next meters, after `blocks` blocks.
    fn read(&mut self, blocks: usize) -> MetersOut {
        self.fill(blocks);
        self.graph.meters().unwrap()
    }
}

/// What `m` sends window `client` of its scopes, as it is sent: each scope's entry.
fn sent_to(m: &MetersOut, client: &str) -> Vec<Value> {
    let mine: Vec<&Value> = m.looks.iter().filter(|(c, _)| c == client).map(|(_, v)| v).collect();
    assert!(mine.len() <= 1, "one message a window");
    assert!(m.all.get("looks").is_none(), "no look goes to every window (TAP-8)");
    mine.first().map_or(vec![], |v| {
        assert_eq!(v["t"], "looks");
        v["looks"].as_array().unwrap().clone()
    })
}

/// The scopes `m` sends w1 with frames: each one's module, node, and its frames per channel.
fn scopes(m: &MetersOut) -> Vec<(String, String, Vec<Vec<f64>>)> {
    scopes_of(m, "w1")
}

fn scopes_of(m: &MetersOut, client: &str) -> Vec<(String, String, Vec<Vec<f64>>)> {
    let list = sent_to(m, client);
    list.iter()
        .filter(|s| s.get("frames").is_some())
        .map(|s| {
            let frames = s["frames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect())
                .collect();
            (s["module"].as_str().unwrap().into(), s["node"].as_str().unwrap().into(), frames)
        })
        .collect()
}

// TAP-8: while a window asks, every meters message carries the box's last 1,024 frames, its
// channels: the bass's saw, as loud as its meter says, and nothing of another box. A stop ends it.
#[test]
fn a_scopes_frames_arrive_while_asked() {
    let mut rig = Rig::new(SONG);
    let saw = rig.id("osc.saw");
    let asked = rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    assert_eq!(asked, json!({"look": {"module": "bass", "node": saw, "kind": "scope", "at": "out", "channels": 1}}));
    rig.graph.transport("play").unwrap();
    rig.read(16);
    let m = rig.read(16);
    let got = scopes(&m);
    assert_eq!(got.len(), 1, "{:?}", m.looks);
    assert!(scopes_of(&m, "w2").is_empty(), "only to the window that asked");
    let (module, node, frames) = &got[0];
    assert_eq!((module.as_str(), node.as_str(), frames.len(), frames[0].len()), ("bass", saw.as_str(), 1, SCOPE));
    let top = frames[0].iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let ramp = one_way(&frames[0]);
    assert!(top > 0.5 && ramp > SCOPE * 9 / 10, "a saw, a ramp nearly every frame: top {top}, {ramp} one way");
    assert!((top - m.all["tracks"]["bass"][0].as_f64().unwrap()).abs() < 0.05, "as loud as its meter: {top} {}", m.all);
    // Asked again: the same scope, kept, and the answer has its last frames (an agent's way to see).
    let again = rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    assert_eq!((&again["look"]["node"], &again["look"]["channels"]), (&json!(saw), &json!(1)));
    let answered = again["look"]["frames"].as_array().map(|c| c.len());
    assert!(answered == Some(1) && again["look"]["frames"][0].as_array().unwrap().len() == SCOPE, "{answered:?}");
    assert_eq!(rig.graph.scope(rig.token(), "w1", "bass", &saw, false).unwrap(), json!({}));
    rig.read(4);
    assert!(scopes(&rig.read(4)).is_empty(), "stopped");
}

// TAP-9: a scope follows its call into each new plan: an edit that adds a track before it (a new
// plan, and a new id for its box in the view) keeps its frames coming, from the same call, under
// its new id; and an edit of the call keeps it on that call.
#[test]
fn a_scope_follows_its_box_into_each_new_plan() {
    let mut rig = Rig::new(SONG);
    let saw = rig.id("osc.saw");
    rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    rig.graph.transport("play").unwrap();
    assert_eq!(scopes(&rig.read(16)).len(), 1);
    let more = SONG.replace("const bass", "const hats = track('hats', () => osc.sine(880));\nconst bass").replace(
        "mixer({ bass: bass.out, kick: kick.out, pad: pad.out })",
        "mixer({ bass: bass.out, kick: kick.out, pad: pad.out, hats: hats.out })",
    );
    rig.graph.write_file(Some(rig.token()), "song.ts", SONG, &more).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert_eq!(got.len(), 1, "still shown");
    let moved = rig.id("osc.saw");
    assert_ne!(moved, saw, "the edit gave the saw's box another id");
    assert_eq!(got[0].1, moved, "the scope follows its call, under the id the view gives it now");
    assert_eq!(
        rig.graph.scope(rig.token(), "w1", "bass", &moved, true).unwrap()["look"]["channels"],
        1,
        "asked by it, it is the same scope"
    );
    let ramp = one_way(&got[0].2[0]);
    assert!(ramp > SCOPE * 9 / 10, "the saw still, not the sine added: {ramp} one way");
    // The saw made a sine: the scope's box is that call still.
    let sine = more.replace("osc.saw(110)", "osc.square(110)");
    rig.graph.write_file(Some(rig.token()), "song.ts", &more, &sine).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    let flat = got[0].2[0].windows(2).filter(|w| w[1] == w[0]).count();
    assert!(flat > SCOPE / 2, "the same call, now a square: {flat} flat");
}

// TAP-8: what isn't one sound at the top of its track is refused in words; a fifth box is shown
// beside four (TAP-10 gives a plan 31 rings for looks, `a_look_past_the_rings_is_refused_in_words`);
// a window that closes ends its scopes, and so do 10 s without an ask.
#[test]
fn a_scope_is_refused_in_words_and_ends_with_its_client() {
    let mut rig = Rig::new(SONG);
    let token = rig.token();
    let lfo = rig.id("lfo");
    let no = rig.graph.scope(token, "w1", "kick", &lfo, true).unwrap_err();
    assert_eq!(format!("{no:#}"), "lfo gives a control or notes, not sound: a scope shows sound.");
    let boxes = [("bass", rig.id("osc.saw")), ("kick", rig.id("osc.sine")), ("kick", rig.id("scale")), ("song", rig.id("mixer"))];
    for (module, node) in &boxes {
        rig.graph.scope(token, "w1", module, node, true).unwrap();
    }
    rig.graph.transport("play").unwrap();
    rig.read(16);
    assert_eq!(scopes(&rig.read(16)).len(), 4, "four shown");
    rig.graph.scope(token, "w2", "pad", &rig.id("osc.square"), true).unwrap();
    assert_eq!(scopes_of(&rig.read(16), "w2").len(), 1, "a fifth, shown to the window that asked");
    rig.graph.scope(token, "w2", "pad", &rig.id("osc.square"), false).unwrap();
    // Another window asks for one shown: it shares it, and keeps it when the first closes.
    rig.graph.scope(token, "w2", "bass", &boxes[0].1, true).unwrap();
    rig.graph.client_gone("w1");
    let out = rig.read(16);
    let left = scopes_of(&out, "w2");
    assert_eq!(left.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), [boxes[0].1.as_str()], "only w2's");
    assert!(scopes(&out).is_empty(), "w1 is gone");
    // 10 s on, w2 never asked again: it ends.
    {
        let mut cp = rig.graph.inner.control.lock();
        let taps = cp.open.as_ref().unwrap().engine.plays.as_ref().unwrap().info.taps.clone();
        cp.asks.expire(std::time::Instant::now() + KEEP, taps.as_deref());
    }
    assert!(scopes_of(&rig.read(16), "w2").is_empty(), "ended");
    assert!(rig.graph.scope(token + 1, "w2", "bass", &boxes[0].1, true).is_err(), "another session's");
}

// TAP-9 (the review's P2): a scope on a call that an edit deletes sends nothing, though the next
// call in the file now has its old id; an undo that brings the call back brings its scope back.
#[test]
fn a_scope_on_a_deleted_call_sends_nothing() {
    let two = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { sum } from 'std/math';\n\
               import { mixer } from 'std/mix';\n\
               const bass = track('bass', () => sum(osc.saw(110), osc.square(55)));\n\
               export default song(() => mixer({ bass: bass.out }));\n";
    let mut rig = Rig::new(two);
    let saw = rig.id("osc.saw");
    rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    rig.graph.transport("play").unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert_eq!(got.len(), 1);
    assert!(one_way(&got[0].2[0]) > SCOPE * 9 / 10, "a saw");
    let gone = two.replace("osc.saw(110), ", "");
    rig.graph.write_file(Some(rig.token()), "song.ts", two, &gone).unwrap();
    assert_eq!(rig.id("osc.square"), saw, "the square has the saw's old id now");
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert!(got.is_empty(), "the saw is gone, so its scope sends nothing, not the square: {:?}", got.first().map(|g| &g.1));
    // Put back as it was: the same call, so its scope shows it again.
    rig.graph.write_file(Some(rig.token()), "song.ts", &gone, two).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert_eq!(got.len(), 1, "the saw is back");
    assert!(got[0].1 == saw && one_way(&got[0].2[0]) > SCOPE * 9 / 10, "and its scope shows it");
}

// TAP-9 (the review's P3): a scope's id is the shown view's. While what plays isn't the song as the
// view shows it (here, a change that couldn't get ready to play, which also renumbered the calls),
// the scope sends no frames, says why, and keeps the view's id for its call; it shows again once
// the song plays as shown.
#[test]
fn while_what_plays_isnt_the_view_a_scope_waits_and_says_why() {
    let mut rig = Rig::new(SONG);
    let saw = rig.id("osc.saw");
    rig.graph.scope(rig.token(), "w1", "bass", &saw, true).unwrap();
    rig.graph.transport("play").unwrap();
    rig.read(16);
    assert_eq!(scopes(&rig.read(16)).len(), 1);
    rig.graph.inspect(|a| a.fail_builds = true);
    let more = SONG.replace("const bass", "const hats = track('hats', () => osc.sine(880));\nconst bass");
    rig.graph.write_file(Some(rig.token()), "song.ts", SONG, &more).unwrap();
    let moved = rig.id("osc.saw");
    assert_ne!(moved, saw, "the view renumbered the saw");
    rig.read(16);
    let list = sent_to(&rig.read(16), "w1");
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!((&list[0]["node"], list[0].get("frames")), (&json!(moved), None), "the view's id, and no frames: {}", list[0]);
    assert!(list[0]["why"].as_str().is_some_and(|w| w.contains("isn't ready to play")), "{}", list[0]);
    let asked = rig.graph.scope(rig.token(), "w1", "bass", &moved, true).unwrap();
    assert_eq!(asked["look"]["channels"], Value::Null, "asked again by the view's id, it waits: {asked}");
    rig.graph.inspect(|a| a.fail_builds = false);
    rig.graph.write_file(Some(rig.token()), "song.ts", &more, &more.replace("osc.sine(880)", "osc.sine(440)")).unwrap();
    rig.read(16);
    let got = scopes(&rig.read(16));
    assert_eq!(got.len(), 1, "it plays as shown again");
    assert!(got[0].1 == moved && one_way(&got[0].2[0]) > SCOPE * 9 / 10, "the saw, by the view's id");
}

/// A saw through a compressor that turns it down a lot, and a sine at 1 kHz, half as loud as full
/// scale, into std's mixer.
const LOOKS: &str = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { compress } from 'std/fx';\n\
                     import { scale } from 'std/math';\nimport { mixer } from 'std/mix';\n\
                     const bass = track('bass', () => compress(osc.saw(110), { threshold: -30, ratio: 8 }));\n\
                     const pad = track('pad', () => scale(osc.sine(1000), 0.5));\n\
                     export default song(() => mixer({ bass: bass.out, pad: pad.out }));\n";

/// What `m` sends w1 of the look of `kind` at `at` of box `node`.
fn entry(m: &MetersOut, node: &str, kind: &str, at: &str) -> Option<Value> {
    sent_to(m, "w1").into_iter().find(|s| s["node"] == node && s["kind"] == kind && s["at"] == at)
}

fn numbers(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// A look by its kind and point, as the route takes it.
fn look(module: &str, node: &str, kind: &str, at: &str) -> Look {
    Look { module: module.into(), node: node.into(), kind: kind.parse().unwrap(), at: m2_editor::scope::At::parse(at) }
}

// TAP-12 to TAP-15: each kind of look arrives while asked, in the window's message: the sine's
// spectrum, loudest in the band of 1 kHz, at its level; the level of what goes into the
// compressor, the saw at full scale, and of what comes out, lower; and the compressor's meter, how
// far it turns down. Looks at one point share its ring, and a meter takes a slot.
#[test]
fn looks_of_each_kind_arrive_while_asked() {
    let mut rig = Rig::new(LOOKS);
    let token = rig.token();
    let (comp, sine) = (rig.id("compress"), rig.id("scale"));
    let spectrum = rig.graph.look(token, "w1", &look("pad", &sine, "spectrum", "out"), true).unwrap();
    let hz = numbers(&spectrum["look"]["hz"]);
    assert_eq!(hz.len(), 120, "a band each twelfth of an octave, from 20 Hz to under 20 kHz");
    assert!((hz[0] - 20.0).abs() < 1e-9 && (hz[12] - 40.0).abs() < 1e-9 && hz[119] < 20_000.0, "{:?}", &hz[..13]);
    for (kind, at) in [("scope", "out"), ("level", "in"), ("level", "out")] {
        rig.graph.look(token, "w1", &look("bass", &comp, kind, at), true).unwrap();
    }
    let meter = rig.graph.look(token, "w1", &look("bass", &comp, "meter", "reduction"), true).unwrap();
    assert_eq!(
        (&meter["look"]["kind"], &meter["look"]["at"], &meter["look"]["unit"]),
        (&json!("meter"), &json!("reduction"), &json!("db"))
    );
    rig.graph.transport("play").unwrap();
    rig.read(64);
    let m = rig.read(64);
    let bands = numbers(&entry(&m, &sine, "spectrum", "out").expect("the spectrum")["bins"]);
    let loudest = (0..bands.len()).max_by(|&a, &b| bands[a].total_cmp(&bands[b])).unwrap();
    assert!((hz[loudest] / 1000.0).log2().abs() < 1.0 / 24.0, "loudest at 1 kHz: {} Hz", hz[loudest]);
    assert!((bands[loudest] + 6.02).abs() < 1.0, "half of full scale is −6 dB: {}", bands[loudest]);
    assert!(bands[loudest - 24] < -60.0 && bands[loudest + 24] < -60.0, "an octave either side, nearly nothing: {bands:?}");
    let level = |at: &str| entry(&m, &comp, "level", at).unwrap_or_else(|| panic!("the level at {at}"));
    let (went, came) = (level("in"), level("out"));
    let (peak, rms) = (numbers(&went["peak"])[0], numbers(&went["rms"])[0]);
    assert!(peak > 0.95 && (rms - 1.0 / 3f64.sqrt()).abs() < 0.05, "a saw at full scale goes in: peak {peak}, rms {rms}");
    assert!(numbers(&came["peak"])[0] < peak / 2.0, "and comes out turned down: {came}");
    let read = entry(&m, &comp, "meter", "reduction").expect("the meter");
    let (now, most) = (read["now"].as_f64().unwrap(), read["most"].as_f64().unwrap());
    assert!(now > 10.0 && most >= now, "turned down more than 10 dB: now {now}, most {most}");
    assert_eq!(entry(&m, &comp, "scope", "out").unwrap()["frames"][0].as_array().unwrap().len(), SCOPE, "and its scope");
    let cp = rig.graph.inner.control.lock();
    assert_eq!(cp.asks.rings(), 3, "the sine's, and the compressor's input and output: its scope and level share one");
}

// TAP-10, TAP-12: a 32nd point is refused in words while 31 are shown, as is a 257th meter; a
// kind or a point an ask can't have is refused too.
#[test]
fn a_look_past_the_rings_is_refused_in_words() {
    let tracks: String = (0..32).map(|k| format!("const t{k} = track('t{k}', () => osc.sine({}));\n", 100 + k)).collect();
    let mix: Vec<String> = (0..32).map(|k| format!("t{k}: t{k}.out")).collect();
    let song = format!(
        "import {{ song, track }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\nimport {{ mixer }} from 'std/mix';\n{tracks}\
         export default song(() => mixer({{ {} }}));\n",
        mix.join(", ")
    );
    let rig = Rig::new(&song);
    let token = rig.token();
    let sines: Vec<(String, String)> = {
        let state = rig.graph.state().unwrap();
        let mods = state["view"]["modules"].as_array().unwrap().clone();
        let mut v = vec![];
        for m in &mods {
            for n in m["nodes"].as_array().unwrap() {
                if n["label"] == "osc.sine" {
                    v.push((m["key"].as_str().unwrap().to_string(), n["id"].as_str().unwrap().to_string()));
                }
            }
        }
        v
    };
    assert_eq!(sines.len(), 32);
    rig.graph.loudness(token, "w1", true).unwrap();
    for (module, node) in &sines[..31] {
        rig.graph.look(token, "w1", &look(module, node, "spectrum", "out"), true).unwrap();
    }
    let (module, node) = &sines[31];
    let no = rig.graph.look(token, "w2", &look(module, node, "level", "out"), true).unwrap_err();
    assert_eq!(format!("{no:#}"), "No room to show this: 31 sounds are shown already. Fold a card or scroll one away, then try again.");
    rig.graph.look(token, "w1", &look(&sines[0].0, &sines[0].1, "scope", "out"), true).expect("a point shown already has room");
    // A meter's slots, as many as a plan has, and one more.
    let mut cp = rig.graph.inner.control.lock();
    let now = Instant::now();
    for k in 0..m2_runtime::METERS {
        cp.asks.add("w1", What::Box(look("t0", &format!("x#{k}"), "meter", "reduction")), now).unwrap();
    }
    let no = cp.asks.add("w1", What::Box(look("t0", "x#x", "meter", "reduction")), now).err().unwrap();
    assert_eq!(format!("{no:#}"), "No room to show this: 256 meters are shown already. Fold a card or scroll one away, then try again.");
    drop(cp);
    for (kind, at, says) in [
        ("meter", "out", "A meter look names the meter it shows, such as at: \"reduction\"."),
        ("scope", "reduction", "A scope shows sound at a call's out or in; a meter is a look of its own, kind: \"meter\"."),
        ("meter", "", "A meter look names the meter it shows, such as at: \"reduction\"."),
    ] {
        let e = rig.graph.look(token, "w1", &look(&sines[0].0, &sines[0].1, kind, at), true).unwrap_err();
        assert_eq!(format!("{e:#}"), says);
    }
    assert_eq!("picture".parse::<Kind>().unwrap_err(), "A look is a scope, a spectrum, a level or a meter, not picture.");
}

/// A saw whose loudness an LFO swings, through a compressor, so how far it turns down rises and
/// falls.
const SWING: &str = "import { song, track } from '@m2/core';\nimport { lfo } from 'std/control';\nimport { osc } from 'std/dsp';\n\
                     import { compress } from 'std/fx';\nimport { scale } from 'std/math';\nimport { mixer } from 'std/mix';\n\
                     const bass = track('bass', ({ time }) => compress(scale(osc.saw(110), lfo(time.beats, { period: 0.5 })), { threshold: -30, ratio: 8 }));\n\
                     export default song(() => mixer({ bass: bass.out }));\n";

// TAP-14, TAP-15 (review T2): each client keeps its own most and its own level, an agent's too,
// with no window listening: one asking at every read sees its most rise and fall back, never under
// its now; one asking once more at the end gets the most and the loudest of the whole while.
#[test]
fn each_client_keeps_its_own_most_and_level() {
    let mut rig = Rig::new(SWING);
    rig.graph.watch_meters(false);
    let (token, comp) = (rig.token(), rig.id("compress"));
    let (meter, level) = (look("bass", &comp, "meter", "reduction"), look("bass", &comp, "level", "in"));
    for who in ["agent:a", "agent:b"] {
        rig.graph.look(token, who, &meter, true).unwrap();
        rig.graph.look(token, who, &level, true).unwrap();
    }
    rig.graph.transport("play").unwrap();
    rig.fill(8);
    rig.graph.meters();
    rig.graph.look(token, "agent:b", &meter, true).unwrap();
    rig.graph.look(token, "agent:b", &level, true).unwrap();
    let (mut mosts, mut peaks) = (vec![], vec![]);
    for _ in 0..60 {
        rig.fill(4);
        rig.graph.meters();
        let m = &rig.graph.look(token, "agent:a", &meter, true).unwrap()["look"];
        let (now, most) = (m["now"].as_f64().unwrap(), m["most"].as_f64().unwrap());
        assert!(most >= now, "most {most} under now {now}");
        mosts.push(most);
        peaks.push(numbers(&rig.graph.look(token, "agent:a", &level, true).unwrap()["look"]["peak"])[0]);
    }
    let (lo, hi) = (mosts.iter().copied().fold(f64::MAX, f64::min), mosts.iter().copied().fold(0.0, f64::max));
    assert!(hi > lo + 3.0, "a's most falls back between its asks: from {hi} to {lo}");
    let b = &rig.graph.look(token, "agent:b", &meter, true).unwrap()["look"];
    assert!((b["most"].as_f64().unwrap() - hi).abs() < 0.01, "b's most is of the whole while: {} against {hi}", b["most"]);
    let b = numbers(&rig.graph.look(token, "agent:b", &level, true).unwrap()["look"]["peak"])[0];
    let top = peaks.iter().copied().fold(0.0, f64::max);
    assert!(b >= top - 0.02, "b's level is of all the ring holds since its last ask: {b} against {top}");
}

// TAP-14, TAP-15: a window's last read is its message before, so its ask again (every 4 s, TAP-16)
// takes nothing from its next message: the meter's most and the level's frames since that message
// are all in the next one, as an agent asking at the same moments gets them. An ask again that took
// them lost a gate's deepest moment from the window's history (dress2a's closings, GitHub's Mac).
#[test]
fn a_windows_ask_again_takes_nothing_from_its_next_message() {
    let mut rig = Rig::new(SWING);
    let (token, comp) = (rig.token(), rig.id("compress"));
    let (meter, level) = (look("bass", &comp, "meter", "reduction"), look("bass", &comp, "level", "in"));
    for who in ["w1", "agent:t"] {
        rig.graph.look(token, who, &meter, true).unwrap();
        rig.graph.look(token, who, &level, true).unwrap();
    }
    rig.graph.transport("play").unwrap();
    rig.read(64);
    // The agent reads where the window's message did, at each of them.
    let agent = |rig: &Rig| {
        let m = rig.graph.look(token, "agent:t", &meter, true).unwrap()["look"]["most"].as_f64().unwrap();
        let l = numbers(&rig.graph.look(token, "agent:t", &level, true).unwrap()["look"]["peak"])[0];
        (m, l)
    };
    agent(&rig);
    for k in 0..6 {
        // A while of the LFO's swing, then the window's ask again, then its next message.
        rig.fill(40);
        rig.graph.look(token, "w1", &meter, true).unwrap();
        rig.graph.look(token, "w1", &level, true).unwrap();
        let m = rig.read(2);
        let (most, peak) = agent(&rig);
        let w = entry(&m, &comp, "meter", "reduction").expect("the meter")["most"].as_f64().unwrap();
        assert!((w - most).abs() < 0.01, "read {k}: the window's most {w} is the agent's {most}, of all since its message before");
        let w = numbers(&entry(&m, &comp, "level", "in").expect("the level")["peak"])[0];
        assert!((w - peak).abs() < 1e-6, "read {k}: the window's level {w} is the agent's {peak}, of all since its message before");
    }
}

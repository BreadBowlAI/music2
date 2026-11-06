//! Pictures on the server (CARD-63, CARD-65, CARD-67, CARD-68): a card's picture drawn on request,
//! with each dot's knobs; drawn again while asked for, when a value it reads changes, for the windows
//! that asked; said in words when it can't be drawn; ended by a stop or a window that closes; asked
//! by an id that holds only while the code does; drawn from what the pack's code is now; and every
//! picture's words in the view.

use m2_play::Player;
use serde_json::{Value, json};

use super::super::tests::Fixture;
use super::*;

/// A song's own sound whose picture is a line at its boost and a dot at its cutoff and boost.
const TONE: &str = "import { axis, component, control, port, view } from '@m2/core';
import { lowpass } from 'std/filter';
import { osc } from 'std/dsp';
export const tone = component(
  {
    id: 'tone', label: 'Tone',
    controls: { cutoff: control.hz(1000).range(20, 20000).log(), boost: control.db(0).range(-24, 24), q: control.scalar(0.7) },
    output: port.audio(),
    editor: {
      views: {
        curve: view.picture({ x: axis.log('hz', 20, 20000), y: axis.lin('db', -24, 24) }),
        broken: view.picture({ x: axis.log('hz', 20, 20000), y: axis.lin('db', -24, 24) }),
      },
    },
  },
  ({ cutoff, q }) => lowpass(osc.saw(110), { cutoff, q }),
  {
    curve: {
      draw: (v, p) => [p.line([20, v.boost ?? 0, v.cutoff ?? 1000, v.boost ?? 0, 20000, -24]), p.handle(v.cutoff, v.boost, { x: 'cutoff', y: 'boost', wheel: 'q' })],
      words: (v) => `cut at ${v.cutoff} Hz, ${v.boost} dB`,
    },
    broken: { draw: (v, p) => [p.handle(1, 1, { x: `cut${9}` as any })], words: () => 'x' },
  },
);
";

const SONG: &str = "import { song, track } from '@m2/core';\nimport { tone } from './lib/tone';\n\
                    const lead = track('lead', () => tone({ cutoff: 2400 }));\nexport default song(() => lead.out);\n";

struct Rig {
    fixture: Fixture,
    graph: Graph,
}

impl Rig {
    fn new() -> Rig {
        let fixture = Fixture::new();
        fixture.put("lib/tone.ts", TONE);
        fixture.put("song.ts", SONG);
        let (p, _driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        graph.open(&fixture.0).unwrap();
        Rig { fixture, graph }
    }

    fn state(&self) -> Value {
        self.graph.state().unwrap()
    }

    fn token(&self) -> u64 {
        self.state()["session"].as_u64().unwrap()
    }

    /// The card called `title` in the view.
    fn card_of(&self, title: &str) -> Value {
        let state = self.state();
        let lead = state["view"]["modules"].as_array().unwrap().iter().find(|m| m["key"] == "lead").unwrap().clone();
        lead["nodes"].as_array().unwrap().iter().find(|n| n["title"] == title).unwrap().clone()
    }

    fn card(&self) -> Value {
        self.card_of("Tone")
    }

    /// The id of Tone's knob for `slot`.
    fn knob(&self, slot: &str) -> String {
        let card = self.card();
        card["knobs"].as_array().unwrap().iter().find(|k| k["slot"] == slot).unwrap()["id"].as_str().unwrap().to_string()
    }

    fn revision(&self) -> u64 {
        self.state()["view"]["revision"].as_u64().unwrap()
    }

    fn ask_of(&self, client: &str, node: &str, view: &str, on: bool) -> Result<Value> {
        self.graph.picture(self.token(), self.revision(), client, ("lead", node, view), on)
    }

    fn ask(&self, client: &str, view: &str, on: bool) -> Value {
        let node = self.card()["id"].as_str().unwrap().to_string();
        self.ask_of(client, &node, view, on).unwrap()
    }
}

/// The pictures a frame sends window `client`, newer than `since` (each by its version): what its
/// socket would send.
fn sent(out: &Option<PicturesOut>, client: &str, since: u64) -> Vec<Value> {
    let Some(out) = out else { return vec![] };
    out.clients
        .iter()
        .filter(|(c, _)| c == client)
        .flat_map(|(_, list)| list.iter().filter(|p| p.version > since).map(|p| (*p.json).clone()))
        .collect()
}

// CARD-67: a picture is drawn on request from what plays: its shapes, its words, the component and
// the revision, and each dot's values with the knobs that move them. An agent asking gets the same.
#[test]
fn a_picture_is_drawn_on_request_with_its_knobs() {
    let rig = Rig::new();
    let answer = rig.ask("w1", "curve", true);
    let node = rig.card()["id"].clone();
    let p = &answer["picture"];
    assert_eq!((&p["module"], &p["node"], &p["view"], &p["component"]), (&json!("lead"), &node, &json!("curve"), &json!("tone")));
    assert_eq!(p["revision"], json!(rig.revision()));
    assert_eq!(p["words"], "cut at 2400 Hz, 0 dB");
    assert_eq!(p["shapes"][0], json!({"kind": "line", "points": [20.0, 0.0, 2400.0, 0.0, 20000.0, -24.0]}));
    assert_eq!(
        p["shapes"][1],
        json!({"kind": "handle", "x": 2400.0, "y": 0.0, "off": false, "moves": {
            "x": {"name": "cutoff", "knob": rig.knob("cutoff")},
            "y": {"name": "boost", "knob": rig.knob("boost")},
            "wheel": {"name": "q", "knob": rig.knob("q")},
        }})
    );
    assert_eq!(rig.ask("agent:a", "curve", true), answer, "an agent reads the same shapes");
    // A picture that can't be drawn says why, named by its card.
    assert_eq!(rig.ask("w1", "broken", true)["picture"]["why"], "Tone's picture names cut9, which this sound doesn't have.");
    assert!(rig.ask_of("w1", "nowhere", "curve", true).is_err());
}

// CARD-68: while asked for, a picture is drawn again when a value it reads changes, a lease's
// preview included, once for all its leases, and sent to each window that asked: never to one that
// didn't, and nothing when nothing changed. A renew with nothing changed answers what was drawn.
#[test]
fn a_drag_draws_it_again_for_the_windows_that_asked() {
    let rig = Rig::new();
    let first = rig.ask("w1", "curve", true);
    assert!(rig.graph.pictures_frame().is_none(), "nothing changed since the answer");
    assert_eq!(rig.ask("w1", "curve", true), first, "a renew answers the picture drawn");
    let (token, rev) = (rig.token(), rig.revision());
    // One gesture moving two values: one draw, from both.
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("cutoff"), "g1", 3000.0).unwrap();
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("boost"), "g1", -6.0).unwrap();
    let out = rig.graph.pictures_frame();
    let mine = sent(&out, "w1", 0);
    assert_eq!(mine.len(), 1, "{mine:?}");
    assert_eq!(mine[0]["words"], "cut at 3000 Hz, -6 dB");
    assert_eq!((&mine[0]["shapes"][1]["x"], &mine[0]["shapes"][1]["y"]), (&json!(3000.0), &json!(-6.0)));
    assert!(sent(&out, "w2", 0).is_empty(), "a window that didn't ask gets nothing");
    let version = out.as_ref().unwrap().clients[0].1[0].version;
    assert!(rig.graph.pictures_frame().is_none(), "nothing more to draw");
    // A second window asking gets it too, and the first stops.
    rig.ask("w2", "curve", true);
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("cutoff"), "g1", 3500.0).unwrap();
    let out = rig.graph.pictures_frame();
    assert_eq!(sent(&out, "w2", 0)[0]["words"], "cut at 3500 Hz, -6 dB");
    assert_eq!(sent(&out, "w1", version)[0]["words"], "cut at 3500 Hz, -6 dB");
    assert_eq!(rig.ask("w1", "curve", false), json!({}));
    rig.graph.cancel_preview(token, &rig.knob("boost"), "g1").unwrap();
    rig.graph.preview_for(Some("w2"), token, rev, &rig.knob("boost"), "g2", 3.0).unwrap();
    let out = rig.graph.pictures_frame();
    assert!(sent(&out, "w1", 0).is_empty(), "w1 stopped");
    assert_eq!(sent(&out, "w2", 0)[0]["words"], "cut at 3500 Hz, 3 dB");
    // Its window gone, an ask ends.
    rig.graph.client_gone("w2");
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("cutoff"), "g1", 900.0).unwrap();
    assert!(rig.graph.pictures_frame().is_none(), "no one asks");
}

// CARD-67: an ask names its card by its id in a revision's view. Once the code changed, that id may
// name another call (a second Tone written before the first takes `#1`): the ask is refused, never
// answered with the other card's picture; and a stop by the old id ends no ask by the new one
// (review G11).
#[test]
fn an_ask_by_an_id_of_older_code_is_refused() {
    let rig = Rig::new();
    let (old_rev, old_id) = (rig.revision(), rig.card()["id"].as_str().unwrap().to_string());
    rig.ask("w1", "curve", true);
    rig.fixture.put(
        "song.ts",
        &SONG
            .replace("tone({ cutoff: 2400 })", "add(tone({ cutoff: 600, boost: 6 }), tone({ cutoff: 2400 }))")
            .replace("import { tone }", "import { add } from 'std/math';\nimport { tone }"),
    );
    assert!(rig.graph.poll());
    let e = rig.graph.picture(rig.token(), old_rev, "agent:a", ("lead", &old_id, "curve"), true).unwrap_err();
    assert!(e.to_string().contains("the song's code changed since revision"), "{e}");
    // By the ids now, each card's own picture.
    let ids: Vec<String> = rig.state()["view"]["modules"].as_array().unwrap().iter().find(|m| m["key"] == "lead").unwrap()["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["title"] == "Tone")
        .map(|n| n["id"].as_str().unwrap().to_string())
        .collect();
    let words: Vec<Value> = ids.iter().map(|id| rig.ask_of("w1", id, "curve", true).unwrap()["picture"]["words"].clone()).collect();
    assert!(words.contains(&json!("cut at 600 Hz, 6 dB")) && words.contains(&json!("cut at 2400 Hz, 0 dB")), "{words:?}");
    // A stop by the old id, from the old revision, ends nothing asked now.
    rig.graph.picture(rig.token(), old_rev, "w1", ("lead", &old_id, "curve"), false).unwrap();
    let (token, rev) = (rig.token(), rig.revision());
    let knob = rig.card()["knobs"].as_array().unwrap().iter().find(|k| k["slot"] == "boost").unwrap()["id"].as_str().unwrap().to_string();
    rig.graph.preview_for(Some("w1"), token, rev, &knob, "g1", -3.0).unwrap();
    assert!(!sent(&rig.graph.pictures_frame(), "w1", 0).is_empty(), "the asks by the ids now go on");
}

// CARD-64: a folder pack edited while the song is open draws its new code: a runtime is keyed by
// what the code is, not by its name and folder (review G3).
#[test]
fn an_edited_folder_pack_draws_its_new_code() {
    let fixture = Fixture::new();
    let pack = fixture.0.with_extension("curvepack");
    let _ = std::fs::remove_dir_all(&pack);
    std::fs::create_dir_all(pack.join("src")).unwrap();
    let index = TONE.to_string();
    std::fs::write(pack.join("src/index.ts"), &index).unwrap();
    std::fs::write(pack.join("m2pkg.toml"), "[package]\nname = \"@test/curve\"\nversion = \"0.1.0\"\nlicense = \"MIT\"\n\n[dependencies]\n\"@m2/core\" = \"2.4\"\nstd = \"0.9\"\n").unwrap();
    let manifest = std::fs::read_to_string(fixture.0.join("music2.toml")).unwrap();
    let rel = format!("../{}", pack.file_name().unwrap().to_string_lossy());
    fixture
        .put("music2.toml", &manifest.replace("[dependencies]\n", &format!("[dependencies]\n\"@test/curve\" = {{ path = \"{rel}\" }}\n")));
    fixture.put("song.ts", &SONG.replace("'./lib/tone'", "'@test/curve'"));
    let (p, _driver) = Player::driven(48_000);
    let graph = Graph::with_player(p).stored();
    graph.open(&fixture.0).unwrap();
    let rig = Rig { fixture, graph };
    assert_eq!(rig.ask("w1", "curve", true)["picture"]["words"], "cut at 2400 Hz, 0 dB");
    std::fs::write(pack.join("src/index.ts"), index.replace("words: (v) => `cut at", "words: (v) => `EDITED cut at")).unwrap();
    rig.fixture.put("song.ts", &SONG.replace("'./lib/tone'", "'@test/curve'").replace("const lead", "// again\nconst lead"));
    assert!(rig.graph.poll());
    assert_eq!(rig.ask("w1", "curve", true)["picture"]["words"], "EDITED cut at 2400 Hz, 0 dB");
    let _ = std::fs::remove_dir_all(&pack);
}

// CARD-63: every picture's words are in the view, worked out by the runner after the view is made:
// null until then, its words, or why it can't be drawn; again for a new revision's values.
#[test]
fn a_pictures_words_are_in_the_view() {
    let rig = Rig::new();
    let views = |rig: &Rig| rig.card()["views"].clone();
    assert_eq!(views(&rig)[0]["words"], json!(null), "not by the settle");
    assert!(rig.graph.pictures_words());
    let v = views(&rig);
    assert_eq!((&v[0]["name"], &v[0]["words"]), (&json!("curve"), &json!("cut at 2400 Hz, 0 dB")));
    assert_eq!((&v[1]["words"], &v[1]["why"]), (&json!(null), &json!("Tone's picture names cut9, which this sound doesn't have.")));
    assert!(!rig.graph.pictures_words(), "nothing new");
    rig.graph.set(rig.token(), rig.revision(), &rig.knob("boost"), "set", 4.0, None).unwrap();
    assert_eq!(views(&rig)[0]["words"], json!(null), "a new view: not yet");
    assert!(rig.graph.pictures_words());
    assert_eq!(views(&rig)[0]["words"], "cut at 2400 Hz, 4 dB");
}

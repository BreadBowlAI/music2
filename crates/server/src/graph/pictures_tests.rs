//! Pictures on the server (CARD-65, CARD-67, CARD-68): a card's picture drawn on request, with
//! each dot's knobs; drawn again at most once a frame while asked for, when a value it reads
//! changes, for the windows that asked; said in words when it can't be drawn; and ended by a stop
//! or a window that closes.

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
        curve: view.picture({
          x: axis.log('hz', 20, 20000), y: axis.lin('db', -24, 24),
          draw: (v, p) => [p.line([20, v.boost, v.cutoff, v.boost, 20000, -24]), p.handle(v.cutoff, v.boost, { x: 'cutoff', y: 'boost', wheel: 'q' })],
          words: (v) => `cut at ${v.cutoff} Hz, ${v.boost} dB`,
        }),
        broken: view.picture({ x: axis.log('hz', 20, 20000), y: axis.lin('db', -24, 24), draw: (v, p) => [p.handle(1, 1, { x: `cut${9}` as any })], words: () => 'x' }),
      },
    },
  },
  ({ cutoff, q }) => lowpass(osc.saw(110), { cutoff, q }),
);
";

struct Rig {
    _fixture: Fixture,
    graph: Graph,
}

impl Rig {
    fn new() -> Rig {
        let fixture = Fixture::new();
        fixture.put("lib/tone.ts", TONE);
        fixture.put(
            "song.ts",
            "import { song, track } from '@m2/core';\nimport { tone } from './lib/tone';\n\
             const lead = track('lead', () => tone({ cutoff: 2400 }));\nexport default song(() => lead.out);\n",
        );
        let (p, _driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        graph.open(&fixture.0).unwrap();
        Rig { _fixture: fixture, graph }
    }

    fn state(&self) -> Value {
        self.graph.state().unwrap()
    }

    fn token(&self) -> u64 {
        self.state()["session"].as_u64().unwrap()
    }

    /// Tone's card in the view.
    fn card(&self) -> Value {
        let state = self.state();
        let lead = state["view"]["modules"].as_array().unwrap().iter().find(|m| m["key"] == "lead").unwrap().clone();
        lead["nodes"].as_array().unwrap().iter().find(|n| n["title"] == "Tone").unwrap().clone()
    }

    /// The id of Tone's knob for `slot`.
    fn knob(&self, slot: &str) -> String {
        let card = self.card();
        card["knobs"].as_array().unwrap().iter().find(|k| k["slot"] == slot).unwrap()["id"].as_str().unwrap().to_string()
    }

    fn ask(&self, client: &str, view: &str, on: bool) -> Value {
        let node = self.card()["id"].as_str().unwrap().to_string();
        self.graph.picture(self.token(), client, "lead", &node, view, on).unwrap()
    }

    fn revision(&self) -> u64 {
        self.state()["view"]["revision"].as_u64().unwrap()
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

// CARD-67: a picture is drawn on request from what plays: its shapes, its words, and each dot's
// values with the knobs that move them. An agent asking gets the same.
#[test]
fn a_picture_is_drawn_on_request_with_its_knobs() {
    let rig = Rig::new();
    let answer = rig.ask("w1", "curve", true);
    let node = rig.card()["id"].clone();
    let p = &answer["picture"];
    assert_eq!((&p["module"], &p["node"], &p["view"]), (&json!("lead"), &node, &json!("curve")));
    assert_eq!(p["words"], "cut at 2400 Hz, 0 dB");
    assert_eq!(p["shapes"][0], json!({"kind": "line", "points": [20, 0, 2400, 0, 20000, -24]}));
    assert_eq!(
        p["shapes"][1],
        json!({"kind": "handle", "x": 2400, "y": 0, "off": false, "moves": {
            "x": {"name": "cutoff", "knob": rig.knob("cutoff")},
            "y": {"name": "boost", "knob": rig.knob("boost")},
            "wheel": {"name": "q", "knob": rig.knob("q")},
        }})
    );
    assert_eq!(rig.ask("agent:a", "curve", true), answer, "an agent reads the same shapes");
    // A picture that can't be drawn says why, named by its card.
    assert_eq!(rig.ask("w1", "broken", true)["picture"]["why"], "Tone's picture names cut9, which this sound doesn't have.");
    assert!(rig.graph.picture(rig.token(), "w1", "lead", "nowhere", "curve", true).is_err());
}

// CARD-68: while asked for, a picture is drawn again when a value it reads changes, a lease's
// preview included, at most once a frame and once for all its leases, and sent to each window that
// asked: never to one that didn't, and nothing when nothing changed.
#[test]
fn a_drag_draws_it_again_once_a_frame_for_the_windows_that_asked() {
    let rig = Rig::new();
    let first = rig.ask("w1", "curve", true);
    assert!(rig.graph.pictures_frame().is_none(), "nothing changed since the answer");
    let (token, rev) = (rig.token(), rig.revision());
    // One gesture moving two values: one draw, from both.
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("cutoff"), "g1", 3000.0).unwrap();
    rig.graph.preview_for(Some("w1"), token, rev, &rig.knob("boost"), "g1", -6.0).unwrap();
    let out = rig.graph.pictures_frame();
    let mine = sent(&out, "w1", 0);
    assert_eq!(mine.len(), 1, "{mine:?}");
    assert_eq!(mine[0]["words"], "cut at 3000 Hz, -6 dB");
    assert_eq!((&mine[0]["shapes"][1]["x"], &mine[0]["shapes"][1]["y"]), (&json!(3000), &json!(-6)));
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
    assert_ne!(first["picture"]["words"], json!(null));
}

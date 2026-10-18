//! What the session tells of a fault found while sound is made (RENDER-25, DIAG-4): the test
//! reports into the app's ring as the audio thread would, for a node of the plan sent last.

use m2_ir::FOp;
use m2_play::{Driver, Player};
use m2_runtime::fault::{Code, Fault, Subject};
use serde_json::Value;

use super::super::Graph;
use super::super::tests::Fixture;

/// A bass through a delay and a hat, each a track with its own name, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { delay } from 'std/fx';\n\
                    import { hat } from 'std/instruments';\nimport { mixer } from 'std/mix';\n\
                    const bass = track('bass', { name: 'Bass' }, () => delay(osc.saw(110)));\n\
                    const hats = track('hats', { name: 'Hats' }, ({ lane }) => hat(lane.notes('hits')));\n\
                    export default song(() => mixer({ bass: bass.out, hats: hats.out }));\n";

/// The song open with a driven player, playing.
struct Rig {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

impl Rig {
    fn new() -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG);
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p);
        graph.open(&fixture.0).unwrap();
        let mut rig = Rig { fixture, graph, driver };
        rig.graph.transport("play").unwrap();
        rig.fill(4);
        rig
    }

    /// `blocks` callbacks of a block each.
    fn fill(&mut self, blocks: usize) {
        for _ in 0..blocks {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
        }
    }

    /// The plan sent last's id, and its node in `module` that `is`.
    fn node(&self, module: &'static str, is: fn(&FOp) -> bool) -> (u64, u32) {
        self.graph.inspect(move |a| {
            let p = a.open.as_ref().unwrap().plays.as_ref().unwrap();
            let at = p.revision.compiled.flat.nodes.iter().position(|n| n.module == module && is(&n.op));
            (p.info.id, at.unwrap_or_else(|| panic!("no such node in {module}")) as u32)
        })
    }

    /// `code` happened to `node` of plan `plan`, reported as the audio thread reports it.
    fn report(&self, code: Code, (plan, node): (u64, u32)) {
        self.graph.inner.faults.report(Fault { code, plan, subject: Subject::Node(node) });
    }

    /// What the notices thread toasts at its next read of the ring.
    fn told(&self) -> Vec<String> {
        self.graph.hear_faults().0
    }

    /// The problems the news lists that faults made, as (code, module, message).
    fn problems(&self) -> Vec<(String, Option<String>, String)> {
        let news = self.graph.news();
        let state = self.graph.state().unwrap();
        assert_eq!(news["problems"], state["problems"], "the news and the state list the same problems");
        listed(&news["problems"])
    }
}

/// The problems in `problems` that faults made, as (code, module, message).
pub(in crate::graph) fn listed(problems: &Value) -> Vec<(String, Option<String>, String)> {
    let text = |v: &Value| v.as_str().map(str::to_string);
    let codes = ["sound.stopped", "plugin.failed", "plugin.crashed"];
    let faults = problems.as_array().unwrap().iter().filter(|p| codes.contains(&p["code"].as_str().unwrap()));
    faults.map(|p| (text(&p["code"]).unwrap(), text(&p["module"]), text(&p["message"]).unwrap())).collect()
}

fn delay(op: &FOp) -> bool {
    matches!(op, FOp::Kernel { kernel, .. } if kernel == "std:delay")
}

/// What starts it again, in words every platform shares: the key is the menu's to show, ⌘. on a Mac
/// and Ctrl+. elsewhere (PLAT-2).
const AGAIN: &str = "The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound.";

/// What is said when `what` stopped.
fn stopped(what: &str) -> String {
    format!("{what} stopped working, so music2 made it silent. {AGAIN}")
}

// RENDER-25, DIAG-4: a sound that stops while the song plays is a problem, `sound.stopped`, named
// by what it is and its track, with the track's key and what starts it again: a kernel by its name,
// an instrument as such, one of the song's own "in the song". It is toasted once, and listed until
// it plays again: a new plan plays it. Stopped again there, it is listed again, with no toast.
#[test]
fn a_stopped_sound_is_a_problem_naming_it_and_its_track() {
    let mut rig = Rig::new();
    let d = rig.node("bass", delay);
    assert!(rig.told().is_empty() && rig.problems().is_empty(), "nothing stopped");
    rig.report(Code::Stopped, d);
    rig.fill(2);
    let said = stopped("The delay on Bass");
    assert_eq!(rig.told(), std::slice::from_ref(&said));
    assert_eq!(rig.problems(), [("sound.stopped".into(), Some("bass".into()), said.clone())]);
    assert!(rig.told().is_empty(), "toasted once");
    rig.report(Code::Stopped, d);
    assert!(rig.told().is_empty() && rig.problems().len() == 1, "listed once");
    let poly = rig.node("hats", |op| matches!(op, FOp::Poly { .. }));
    let gain = rig.node("song", |op| matches!(op, FOp::Kernel { kernel, .. } if kernel == "std:gain"));
    rig.report(Code::Stopped, poly);
    rig.report(Code::Stopped, gain);
    assert_eq!(rig.told(), [stopped("The instrument on Hats"), stopped("The gain in the song")]);
    assert_eq!(rig.problems().iter().map(|p| p.1.as_deref()).collect::<Vec<_>>(), [Some("bass"), Some("hats"), Some("song")]);
    // An edit that makes a new plan (another oscillator; a value would be a cell write): each part
    // plays again in it.
    rig.fixture.put("song.ts", &SONG.replace("osc.saw(110)", "osc.square(110)"));
    assert!(rig.graph.poll());
    assert!(rig.told().is_empty() && rig.problems().is_empty(), "they play again");
    let next = rig.node("bass", delay);
    assert_ne!(next.0, d.0, "a new plan");
    rig.report(Code::Stopped, next);
    assert!(rig.told().is_empty(), "toasted once in the open song");
    assert_eq!(rig.problems(), [("sound.stopped".into(), Some("bass".into()), said)]);
    assert!(rig.graph.status().playing, "the song plays on");
}

// RENDER-32, RENDER-25: a sound over its memory is told as running out of memory, with the cap.
#[test]
fn out_of_memory_is_told_as_memory() {
    let rig = Rig::new();
    rig.report(Code::OutOfMemory, rig.node("bass", delay));
    assert_eq!(rig.told(), [format!("The delay on Bass ran out of memory (a sound may use up to 256 MB), so music2 made it silent. {AGAIN}")]);
}

// RENDER-25, RENDER-10, D3: a problem goes when its part plays again: Stop All Sound resets every
// sound, so a sound that stopped isn't listed after it, and the clients are told the list changed.
#[test]
fn a_problem_goes_when_its_part_plays_again() {
    let mut rig = Rig::new();
    rig.report(Code::Stopped, rig.node("bass", delay));
    assert_eq!((rig.told().len(), rig.problems().len()), (1, 1));
    rig.graph.panic().unwrap();
    rig.fill(2);
    assert_eq!(rig.problems(), [], "it plays again");
    assert_eq!(rig.graph.hear_faults(), (vec![], true), "the list changed");
}

// D2: a fault is named by the plan it came from, never by the plan that plays now, whose node of
// that number may be another sound. One from the plan before the one sent last is told, not listed
// (its part plays again in the new plan); one from an older plan is dropped.
#[test]
fn a_fault_from_a_plan_gone_is_never_named_by_the_new_one() {
    let rig = Rig::new();
    let first = rig.node("bass", delay);
    // A track before the bass: the delay's number is another node's in the plans after.
    let more = |wave: &str| {
        let lead = format!("const lead = track('lead', {{ name: 'Lead' }}, () => osc.{wave}(220));\n");
        SONG.replace("const bass", &format!("{lead}const bass")).replace("mixer({ bass", "mixer({ lead: lead.out, bass")
    };
    rig.fixture.put("song.ts", &more("sine"));
    assert!(rig.graph.poll());
    let second = rig.node("bass", delay);
    assert_ne!(second.1, first.1, "the delay is another number");
    // Another oscillator: a new plan again.
    rig.fixture.put("song.ts", &more("square"));
    assert!(rig.graph.poll());
    rig.report(Code::Stopped, first);
    assert!(rig.told().is_empty() && rig.problems().is_empty(), "an older plan's fault is dropped");
    rig.report(Code::Stopped, second);
    assert_eq!(rig.told(), [stopped("The delay on Bass")], "named by its own plan");
    assert_eq!(rig.problems(), [], "it plays again in the new plan");
}

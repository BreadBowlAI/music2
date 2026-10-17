//! What the session tells when a sound stops while the song plays (RENDER-25): the test marks a
//! node of the plan sent last, as the audio thread would.

use std::sync::Arc;

use m2_ir::FOp;
use m2_play::{Driver, Player};
use m2_runtime::{Stopped, Why};

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

    /// The plan sent last's marks, and its node in `module` that `is`.
    fn node(&self, module: &'static str, is: fn(&FOp) -> bool) -> (Arc<Stopped>, usize) {
        self.graph.inspect(move |a| {
            let p = a.open.as_ref().unwrap().plays.as_ref().unwrap();
            let at = p.revision.compiled.flat.nodes.iter().position(|n| n.module == module && is(&n.op));
            (p.info.stopped.clone(), at.unwrap_or_else(|| panic!("no such node in {module}")))
        })
    }

    /// What the user was told about stopped sounds, after the actor's next job.
    fn told(&self) -> Vec<String> {
        self.graph.inspect(|_| ());
        self.graph.expire().into_iter().filter(|t| t.contains("music2 made it silent")).collect()
    }
}

fn delay(op: &FOp) -> bool {
    matches!(op, FOp::Kernel { kernel, .. } if kernel == "std:delay")
}

/// What starts it again, in words every platform shares: the key is the menu's to show, ⌘. on a Mac
/// and Ctrl+. elsewhere (PLAT-2).
const AGAIN: &str = "To start it again, choose Transport ▸ Stop All Sound.";

// RENDER-25: a sound that stops while the song plays is told once, by what it is and its track, with
// what starts it again: a kernel by its name, an instrument as such, one of the song's own "in the
// song". A second look tells nothing new; a new plan can tell the same sound again.
#[test]
fn a_kernel_that_stops_is_told_once_by_sound_and_track() {
    let mut rig = Rig::new();
    let (stopped, d) = rig.node("bass", delay);
    assert!(rig.told().is_empty(), "nothing stopped");
    stopped.stop(d, Why::Crashed);
    rig.fill(2);
    let said = format!("The delay on Bass stopped working, so music2 made it silent. The rest of the song plays on. {AGAIN}");
    assert_eq!(rig.told(), std::slice::from_ref(&said));
    assert!(rig.told().is_empty(), "once");
    let (_, poly) = rig.node("hats", |op| matches!(op, FOp::Poly { .. }));
    let (_, gain) = rig.node("song", |op| matches!(op, FOp::Kernel { kernel, .. } if kernel == "std:gain"));
    stopped.stop(poly, Why::Crashed);
    stopped.stop(gain, Why::Crashed);
    // In node order: the song's own come first here.
    assert_eq!(
        rig.told(),
        [
            format!("The gain in the song stopped working, so music2 made it silent. The rest of the song plays on. {AGAIN}"),
            format!("The instrument on Hats stopped working, so music2 made it silent. The rest of the song plays on. {AGAIN}"),
        ]
    );
    // An edit that makes a new plan (another oscillator; a value would be a cell write): its marks
    // are its own.
    rig.fixture.put("song.ts", &SONG.replace("osc.saw(110)", "osc.square(110)"));
    assert!(rig.graph.poll());
    let (next, d) = rig.node("bass", delay);
    assert!(!Arc::ptr_eq(&stopped, &next), "a new plan");
    next.stop(d, Why::Crashed);
    assert_eq!(rig.told(), [said]);
    assert!(rig.graph.status().playing, "the song plays on");
}

// RENDER-32, RENDER-25: a sound over its 32 MB is told as running out of memory, with the cap.
#[test]
fn out_of_memory_is_told_as_memory() {
    let rig = Rig::new();
    let (stopped, d) = rig.node("bass", delay);
    stopped.stop(d, Why::OutOfMemory);
    assert_eq!(
        rig.told(),
        [format!(
            "The delay on Bass ran out of memory (a sound may use up to 32 MB), so music2 made it silent. The rest of the song plays on. {AGAIN}"
        )]
    );
}

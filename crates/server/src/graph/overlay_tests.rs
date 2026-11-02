//! Monitoring mute and solo on the server (MON-1 to MON-5): the path rule, the file, read-only
//! windows, new plans, mute in song as a cell write, a renamed track, and exports.

use m2_editor::render::Length;
use m2_play::{Driver, Player};
use m2_song::tx::{Confined, Declared, Edit, Expect, ModuleChange, Op, Pos, Range, TxPath};

use super::super::tests::Fixture;
use super::*;

fn site(m: &str, c: &str) -> Site {
    (m.to_string(), c.to_string())
}

fn set(mute: &[&str], solo: &[&str]) -> Overlays {
    Overlays { mute: mute.iter().map(|s| s.to_string()).collect(), solo: solo.iter().map(|s| s.to_string()).collect() }
}

/// A vocal and a backing vocal into a bus, the bus and a lead into the song, and the vocal into
/// the song directly too.
fn bus() -> Vec<Site> {
    vec![site("vox", "bus"), site("backing", "bus"), site("bus", "song"), site("lead", "song"), site("vox", "song")]
}

/// The tracks of [`bus`].
fn four() -> Vec<String> {
    ["vox", "backing", "bus", "lead"].map(String::from).to_vec()
}

// MON-3: soloing a vocal keeps its path to the song (into the bus, the bus out) and mutes the
// backing vocal's way into the same bus; soloing the bus keeps what feeds it and mutes the vocal's
// direct way past it, which one value per track couldn't tell apart.
#[test]
fn solo_keeps_a_bus_fed_and_its_sources() {
    assert_eq!(values(&bus(), &[], &set(&[], &[]), &[], &four()), [1.0; 5]);
    assert_eq!(values(&bus(), &[], &set(&[], &["vox"]), &[], &four()), [1.0, 0.0, 1.0, 0.0, 1.0]);
    assert_eq!(values(&bus(), &[], &set(&[], &["bus"]), &[], &four()), [1.0, 1.0, 1.0, 0.0, 0.0]);
    assert_eq!(values(&bus(), &[], &set(&[], &["vox", "lead"]), &[], &four()), [1.0, 0.0, 1.0, 1.0, 1.0]);
}

/// Buses the song feeds (RENDER-34): kick and hats go to the drums group, vox sends to verb, which
/// sends on to echo, and bass is dry. Each track enters the song at one site; the routes say which
/// bus each one's way into the song reaches.
fn routed() -> (Vec<Site>, Vec<Site>) {
    let sites = ["kick", "hats", "drums", "vox", "bass", "verb", "echo"].map(|m| site(m, "song")).to_vec();
    (sites, vec![site("hats", "drums"), site("kick", "drums"), site("verb", "echo"), site("vox", "verb")])
}

/// The values of [`routed`]'s sites (kick, hats, drums, vox, bass, verb, echo) with `mute` and `solo`.
fn heard(mute: &[&str], solo: &[&str]) -> Vec<f32> {
    let (sites, routes) = routed();
    let tracks = ["kick", "hats", "drums", "vox", "bass", "verb", "echo"].map(String::from).to_vec();
    values(&sites, &routes, &set(mute, solo), &[], &tracks)
}

// MON-3, RENDER-34: soloing a track keeps it and every bus it feeds, through sends or `to`, and the
// buses those feed; the other tracks into the same buses are silenced.
#[test]
fn solo_keeps_the_buses_a_track_feeds() {
    assert_eq!(heard(&[], &[]), [1.0; 7]);
    assert_eq!(heard(&[], &["vox"]), [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    assert_eq!(heard(&[], &["kick"]), [1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(heard(&[], &["kick", "bass"]), [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]);
}

// MON-3: soloing a bus keeps it, every track that feeds it (their dry sound too: one cell serves
// both ways) and the buses it feeds.
#[test]
fn soloing_a_bus_keeps_what_feeds_it() {
    assert_eq!(heard(&[], &["drums"]), [1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(heard(&[], &["verb"]), [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
    assert_eq!(heard(&[], &["echo"]), [0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]);
}

// MON-3: muting a track silences all of it, dry, sends and group, as all start after its one cell;
// muting a group silences its tracks with it, and muting a bus leaves what feeds it heard dry.
#[test]
fn muting_a_track_silences_its_sends() {
    assert_eq!(heard(&["vox"], &[]), [1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(heard(&["drums"], &[]), [1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert_eq!(heard(&["vox"], &["vox"]), [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
}

// MON-4: a solo of a track that is gone is ignored, alone or beside one that is there.
#[test]
fn a_solo_of_a_track_that_is_gone_is_ignored() {
    assert_eq!(values(&bus(), &[], &set(&[], &["gone"]), &[], &four()), [1.0; 5]);
    assert_eq!(values(&bus(), &[], &set(&[], &["gone", "vox"]), &[], &four()), values(&bus(), &[], &set(&[], &["vox"]), &[], &four()));
}

// MON-3: mute wins over solo on the same track, and a mute upstream silences only that track.
#[test]
fn mute_wins_over_solo() {
    assert_eq!(values(&bus(), &[], &set(&["vox"], &["vox"]), &[], &four()), [0.0, 0.0, 1.0, 0.0, 0.0]);
    assert_eq!(values(&bus(), &[], &set(&["backing"], &["bus"]), &[], &four()), [1.0, 0.0, 1.0, 0.0, 0.0]);
}

// MON-3, MON-5: a track muted in the song stays silent when soloed, and says so.
#[test]
fn a_track_muted_in_song_stays_silent_when_soloed() {
    let muted = ["vox".to_string()];
    assert_eq!(values(&bus(), &[], &set(&[], &["vox"]), &muted, &four()), [0.0, 0.0, 1.0, 0.0, 0.0]);
    let song = Song {
        tracks: ["vox", "backing", "bus", "lead"].map(String::from).to_vec(),
        sites: bus(),
        muted: muted.to_vec(),
        ..Song::default()
    };
    let m = Monitor { set: set(&["lead"], &["vox"]), version: 0, written: 0, dir: PathBuf::new(), writer: true, song: Arc::new(song) };
    let s = m.state();
    assert_eq!(s["heard"], json!({"vox": "muted-in-song", "backing": "silenced", "bus": "heard", "lead": "muted"}), "{s:#}");
    assert_eq!(
        (&s["mute"], &s["solo"], &s["muted_in_song"], &s["saved"]),
        (&json!(["lead"]), &json!(["vox"]), &json!(["vox"]), &json!(true))
    );
}

/// Kick and bass tracks into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { mixer } from 'std/mix';\n\
                    const kick = track('kick', { name: 'Kick' }, () => osc.sine(55));\n\
                    const bass = track('bass', () => osc.saw(110));\n\
                    export default song(() => mixer({ kick: kick.out, bass: bass.out }));\n";

/// A project of [`SONG`] open with a driven player.
struct Rig {
    fixture: Fixture,
    graph: Graph,
    driver: Driver,
}

impl Rig {
    fn new() -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", SONG);
        Rig::of(fixture)
    }

    fn of(fixture: Fixture) -> Rig {
        let (p, driver) = Player::driven(48_000);
        let graph = Graph::with_player(p).stored();
        graph.open(&fixture.0).unwrap();
        let mut rig = Rig { fixture, graph, driver };
        // The engine takes what was loaded at its first callback.
        rig.fill(1);
        rig
    }

    fn token(&self) -> u64 {
        self.graph.state().unwrap()["session"].as_u64().unwrap()
    }

    /// `blocks` blocks of 128 frames, left channel.
    fn fill(&mut self, blocks: usize) -> Vec<f32> {
        let mut out = vec![];
        for _ in 0..blocks {
            let mut buf = [0.0f32; 256];
            self.driver.fill(&mut buf, 2);
            drop(self.driver.drain_retired());
            out.extend(buf.iter().step_by(2));
        }
        out
    }

    fn renderer(&mut self) -> &m2_runtime::Renderer {
        self.driver.engine().unwrap().current().unwrap()
    }

    /// What the renderer that plays holds in each overlay cell of `track`'s sites.
    fn cells(&mut self, track: &str) -> Vec<f32> {
        let r = self.renderer();
        r.info().overlays.iter().filter(|o| o.0 == track).map(|o| r.cell_value(o.2).unwrap()).collect()
    }

    fn mute(&self, track: &str, on: bool) -> Result<Value> {
        self.graph.overlay(self.token(), Change { mute: [(track.to_string(), on)].into(), ..Change::default() })
    }
}

/// A Tx replacing `old` with `new` in `song.ts`, confined to it, declaring `modules`.
fn song_edit(rig: &Rig, old: &str, new: &str, modules: Vec<ModuleChange>) -> m2_song::Tx {
    let text = rig.fixture.read("song.ts");
    let at = text.find(old).unwrap();
    let file = TxPath::new("song.ts").unwrap();
    let r = Range::bytes(at as u32, (at + old.len()) as u32);
    let edit = Edit { start: Pos::Byte(at as u32), end: Pos::Byte((at + old.len()) as u32), expected: old.into(), text: new.into() };
    let mut tx = m2_song::Tx::new(
        "mute",
        ("Kick".into(), "Kick, muted in song".into()),
        vec![Op::TextEdit { file: file.clone(), edits: vec![edit] }],
        Expect::Confined(Confined { ranges: [(file.clone(), Declared::Ranges(vec![r]))].into(), modules, ..Confined::default() }),
    );
    tx.base.insert(file, m2_song::tx::Base::text(&text));
    tx
}

// MON-4, A6, M2: mutes and solos are saved in `.music2/overlays.json` (by the server's thread, not
// the request), changing no project file; a reopened project plays them from its first block.
#[test]
fn overlays_persist_in_music2() {
    let mut rig = Rig::new();
    let song = rig.fixture.read("song.ts");
    let state = rig.mute("kick", true).unwrap();
    assert_eq!((&state["mute"], &state["heard"]["kick"], &state["heard"]["bass"]), (&json!(["kick"]), &json!("muted"), &json!("heard")));
    assert!(!rig.fixture.0.join(".music2/overlays.json").exists(), "written later, off the request");
    rig.graph.save_overlays();
    let saved: Value = serde_json::from_str(&rig.fixture.read(".music2/overlays.json")).unwrap();
    assert_eq!(saved, json!({"mute": ["kick"], "solo": []}));
    assert_eq!(rig.fixture.read("song.ts"), song);
    assert_eq!(rig.graph.state().unwrap()["overlays"]["mute"], json!(["kick"]));
    rig.fill(8);
    assert_eq!(rig.cells("kick"), [0.0], "the cell, at once");
    // Reopened: the first renderer has them before it plays.
    let fixture = std::mem::replace(&mut rig.fixture, Fixture::new());
    drop(rig);
    let mut again = Rig::of(fixture);
    assert_eq!(again.graph.state().unwrap()["overlays"]["mute"], json!(["kick"]));
    assert_eq!((again.cells("kick"), again.cells("bass")), (vec![0.0], vec![1.0]));
}

// MON-4: a mute made just before the project closes, or opens again, is saved all the same: the
// file is written before the project goes, not only by the server's thread.
#[test]
fn a_mute_just_before_a_close_or_a_reopen_is_kept() {
    let mut rig = Rig::new();
    rig.mute("kick", true).unwrap();
    rig.graph.close();
    let saved: Value =
        serde_json::from_str(&std::fs::read_to_string(rig.fixture.0.join(".music2/overlays.json")).unwrap_or_default()).unwrap_or_default();
    assert_eq!(saved, json!({"mute": ["kick"], "solo": []}), "written at the close");
    rig.graph.open(&rig.fixture.0).unwrap();
    rig.fill(1);
    assert_eq!(rig.graph.state().unwrap()["overlays"]["mute"], json!(["kick"]));
    rig.mute("bass", true).unwrap();
    rig.graph.open(&rig.fixture.0).unwrap();
    rig.fill(1);
    assert_eq!(rig.graph.state().unwrap()["overlays"]["mute"], json!(["bass", "kick"]), "opened again at once");
    assert_eq!((rig.cells("kick"), rig.cells("bass")), (vec![0.0], vec![0.0]));
}

// MON-4, WRITE-17: in a window open read-only, mute and solo still work, in memory: nothing is
// written, and the state says they aren't saved.
#[test]
fn a_read_only_window_keeps_overlays_in_memory() {
    let fixture = Fixture::new();
    fixture.put("song.ts", SONG);
    let held = m2_session::lock::acquire(&fixture.0, "m2", m2_session::lock::RETRY).unwrap();
    let mut rig = Rig::of(fixture);
    assert!(rig.graph.read_only().is_some());
    let state = rig.mute("kick", true).unwrap();
    assert_eq!((&state["mute"], &state["saved"]), (&json!(["kick"]), &json!(false)));
    rig.graph.save_overlays();
    assert!(!rig.fixture.0.join(".music2/overlays.json").exists(), "nothing written");
    rig.fill(8);
    assert_eq!(rig.cells("kick"), [0.0], "it plays muted all the same");
    drop(held);
}

// MON-1: the song module can't be muted or soloed, and a track that isn't there can't be either;
// the refusal says so in words, and changes nothing.
#[test]
fn the_song_module_cant_be_muted() {
    let rig = Rig::new();
    let e = rig.mute("song", true).unwrap_err().to_string();
    assert!(e.contains("song can't be muted or soloed"), "{e}");
    let e = rig.graph.overlay(rig.token(), Change { exclusive: Some("hats".into()), ..Change::default() }).unwrap_err().to_string();
    assert!(e.contains("no track \"hats\""), "{e}");
    assert_eq!(rig.graph.state().unwrap()["overlays"]["mute"], json!([]));
    // Solo one exclusively, then clear all.
    let s = rig.graph.overlay(rig.token(), Change { exclusive: Some("bass".into()), ..Change::default() }).unwrap();
    assert_eq!((&s["solo"], &s["heard"]["kick"]), (&json!(["bass"]), &json!("silenced")));
    let s = rig.graph.overlay(rig.token(), Change { clear: true, ..Change::default() }).unwrap();
    assert_eq!((&s["solo"], &s["mute"]), (&json!([]), &json!([])));
}

// MON-1, A7: a structural edit while the kick is muted prepares a new plan that has the kick's
// overlay at 0 before it is sent: the kick never sounds.
#[test]
fn a_new_plan_takes_the_overlays_before_it_is_sent() {
    let mut rig = Rig::new();
    rig.graph.transport("play").unwrap();
    rig.mute("kick", true).unwrap();
    rig.fill(16);
    let first = rig.renderer().generation();
    // Another bass: a new plan.
    let song = rig.fixture.read("song.ts").replace("bass: bass.out", "bass: osc.saw(220)");
    let token = rig.token();
    rig.graph.write_file(Some(token), "song.ts", &rig.fixture.read("song.ts"), &song).unwrap();
    rig.fill(16);
    assert_ne!(rig.renderer().generation(), first, "an activation");
    assert_eq!(rig.cells("kick"), [0.0], "muted from its first block");
}

// MON-5, WRITE-8, A1: a mute-in-song toggle reaches what plays as a cell write: no new plan, and
// the kick glides out; undo glides it back.
#[test]
fn a_mute_in_song_toggle_is_a_cell_write() {
    let mut rig = Rig::new();
    rig.graph.transport("play").unwrap();
    rig.fill(16);
    let first = rig.renderer().generation();
    let kick = m2_song::Key::new("kick").unwrap();
    let muted = vec![ModuleChange::Muted { module: kick.clone(), muted: true }];
    let tx = song_edit(&rig, "{ name: 'Kick' }", "{ name: 'Kick', muted: true }", muted);
    let (status, done) = rig.graph.tx_commit(Some(rig.token()), &tx.to_json(), None, &[]).unwrap();
    assert_eq!(status, 200, "{done:#}");
    rig.fill(16);
    assert_eq!(rig.renderer().generation(), first, "no activation");
    assert_eq!(rig.cells("kick"), [0.0]);
    assert_eq!(rig.graph.state().unwrap()["overlays"]["heard"]["kick"], json!("muted-in-song"));
    rig.graph.undo(rig.token()).unwrap();
    rig.fill(16);
    assert_eq!((rig.renderer().generation(), rig.cells("kick")), (first, vec![1.0]));
}

// MON-4: a soloed track taken out of the code leaves the rest heard, with no export warning. Its
// solo stays in the file, so the track comes back soloed.
#[test]
fn a_soloed_track_taken_out_of_the_code_silences_nothing() {
    let mut rig = Rig::new();
    rig.graph.overlay(rig.token(), Change { solo: [("kick".to_string(), true)].into(), ..Change::default() }).unwrap();
    rig.fill(8);
    assert_eq!(rig.cells("bass"), [0.0], "soloed away");
    let text = rig.fixture.read("song.ts");
    let without = text.replace("const kick = track('kick', { name: 'Kick' }, () => osc.sine(55));\n", "").replace("kick: kick.out, ", "");
    rig.graph.write_file(Some(rig.token()), "song.ts", &text, &without).unwrap();
    rig.fill(8);
    assert_eq!(rig.cells("bass"), [1.0], "heard again");
    let s = rig.graph.state().unwrap();
    assert_eq!((&s["overlays"]["solo"], &s["overlays"]["heard"]), (&json!(["kick"]), &json!({"bass": "heard"})), "{s:#}");
    let e = rig.graph.export(Length::Seconds(0.1)).unwrap();
    assert!(!e.problems.iter().any(|d| d.code == "overlays.active"), "{:?}", e.problems);
    rig.graph.save_overlays();
    rig.graph.write_file(Some(rig.token()), "song.ts", &without, &text).unwrap();
    rig.fill(8);
    assert_eq!((rig.cells("kick"), rig.cells("bass")), (vec![1.0], vec![0.0]), "back, soloed");
}

// MON-4: a track renamed in code keeps its mute.
#[test]
fn a_rekeyed_track_keeps_its_mute() {
    let mut rig = Rig::new();
    rig.mute("kick", true).unwrap();
    let text = rig.fixture.read("song.ts");
    let renamed = text.replace("track('kick'", "track('drum'");
    rig.graph.write_file(Some(rig.token()), "song.ts", &text, &renamed).unwrap();
    rig.fill(8);
    let s = rig.graph.state().unwrap();
    assert_eq!((&s["overlays"]["mute"], &s["overlays"]["heard"]["drum"]), (&json!(["drum"]), &json!("muted")));
    assert_eq!(rig.cells("drum"), [0.0]);
}

// Gate 8, MON-4: an export is byte-identical with and without overlays active, and only the one
// made while they are carries `overlays.active`.
#[test]
fn an_export_is_byte_identical_with_overlays_active() {
    let rig = Rig::new();
    let export = |name: &str| {
        let e = rig.graph.export(Length::Seconds(0.5)).unwrap();
        let path = rig.fixture.0.join(name);
        let problems: Vec<&'static str> = e.problems.iter().map(|d| d.code).collect();
        e.render.write(&path, |_| {}).unwrap();
        (std::fs::read(&path).unwrap(), problems)
    };
    let (plain, none) = export("a.wav");
    rig.mute("kick", true).unwrap();
    rig.graph.overlay(rig.token(), Change { solo: [("bass".to_string(), true)].into(), ..Change::default() }).unwrap();
    let (muted, warned) = export("b.wav");
    assert!(plain == muted, "the same bytes");
    assert!(!none.contains(&"overlays.active") && warned.contains(&"overlays.active"), "{none:?} {warned:?}");
}

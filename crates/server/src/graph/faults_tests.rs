//! What the session tells of a fault found while sound is made (RENDER-25, DIAG-4): the test
//! reports into the app's ring as the audio thread would, for a node of the plan sent last.

use m2_ir::FOp;
use m2_play::{Driver, Player};
use m2_runtime::fault::{Code, Fault, Subject};
use serde_json::Value;

use super::super::Graph;
use super::super::tests::Fixture;

/// A bass through a delay and a hat, each a track with its own name, into std's mixer.
const SONG: &str = "import { song, track } from '@m2/core';\nimport { osc } from 'std/dsp';\nimport { delay } from '@m2/effects';\n\
                    import { hat } from '@m2/synths';\nimport { mixer } from 'std/mix';\n\
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
        Rig::with(SONG, |_| {})
    }

    /// `song` open and playing, the project's other files made by `files` first.
    fn with(song: &str, files: impl FnOnce(&Fixture)) -> Rig {
        let fixture = Fixture::new();
        fixture.put("song.ts", song);
        files(&fixture);
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

    /// What it toasts within 10 s, while the song plays: the disk and stretch threads report in
    /// their own time.
    fn told_soon(&mut self) -> Vec<String> {
        let t = std::time::Instant::now();
        loop {
            self.fill(8);
            let told = self.told();
            if !told.is_empty() || t.elapsed() > std::time::Duration::from_secs(10) {
                return told;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
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
    let codes = [
        "sound.stopped",
        "plugin.failed",
        "plugin.crashed",
        "asset.unreadable",
        "stretch.failed",
        "input.missing",
        "input.lost",
        "input.refused",
    ];
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
    assert_eq!(
        rig.told(),
        [format!("The delay on Bass ran out of memory (a sound may use up to 256 MB), so music2 made it silent. {AGAIN}")]
    );
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

/// Reads of a file on a thread that fail, as from a drive unplugged while it plays: the file, by
/// what it is, and the thread's name (`watch_reads`). Tests that read other files run beside these.
static UNPLUGGED: std::sync::Mutex<Vec<(m2_os::fs::FileId, &'static str)>> = std::sync::Mutex::new(vec![]);

fn unplugged(f: &std::fs::File, _: usize) -> std::io::Result<()> {
    let gone = UNPLUGGED.lock().unwrap_or_else(|e| e.into_inner());
    let thread = std::thread::current();
    if !gone.is_empty() && gone.iter().any(|&(id, t)| thread.name() == Some(t) && m2_os::fs::file_id_of(f).ok() == Some(id)) {
        return Err(std::io::Error::other("the drive was unplugged"));
    }
    Ok(())
}

/// From now on, reads of `path` on the thread named `thread` fail.
fn unplug(path: &std::path::Path, thread: &'static str) {
    m2_os::fs::watch_reads(unplugged);
    UNPLUGGED.lock().unwrap_or_else(|e| e.into_inner()).push((m2_os::fs::file_id(path).unwrap(), thread));
}

/// A track, Drums, playing its audio lane `loops`, which holds `clip`.
fn drums(clip: &str) -> (&'static str, String) {
    let song = "import { song, track } from '@m2/core';\n\
                const d = track('drums', { name: 'Drums' }, ({ lane }) => lane.audio('loops'));\n\
                export default song(() => d.out);\n";
    (song, format!(r#"{{"clips": [{clip}]}}"#))
}

/// A WAV in the project's `assets/`: a stereo sine of `seconds`.
fn wav(f: &Fixture, name: &str, seconds: f64) -> std::path::PathBuf {
    let path = f.0.join("assets").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let sine: Vec<f32> = (0..(seconds * 48_000.0) as usize).map(|i| (i as f32 * 0.03).sin() * 0.3).collect();
    m2_runtime::audio::write_wav(&path, &[sine.clone(), sine], 48_000).unwrap();
    path
}

// io r8, RENDER-25, D3: a streamed file whose reads fail while it plays (a drive unplugged) is one
// problem, `asset.unreadable`, naming the file and its track, with the track's key; toasted once,
// however many of its chunks fail. The song plays on. Before, each failed chunk was counted, and
// nothing read the count.
#[test]
fn a_streamed_file_that_cant_be_read_is_told_once() {
    let (song, clips) = drums(r#"{"id": "a", "start": 0, "length": 16, "asset": "loop.wav"}"#);
    let mut rig = Rig::with(song, |f| {
        // Eight seconds: more than a lane's slots hold, so the disk thread reads it as it plays.
        let path = wav(f, "loop.wav", 8.0);
        f.put("data/drums/loops.json", &clips);
        unplug(&path, "m2-disk");
    });
    let said = "loop.wav on Drums can't be read, so it plays silent. It may be damaged, or on a drive that isn't plugged in. The rest of \
                the song plays on.";
    assert_eq!(rig.told_soon(), [said]);
    assert_eq!(rig.problems(), [("asset.unreadable".into(), Some("drums".into()), said.into())]);
    rig.graph.seek(12.0).unwrap();
    rig.fill(40);
    assert!(rig.told().is_empty() && rig.problems().len() == 1, "told once");
    assert!(rig.graph.status().playing, "the song plays on");
}

// FMT-24: a clip whose file in assets/ is a FLAC (put there by hand: an import makes a WAV of it)
// plays silent, and says so in words that name it and say how to bring it in. Never noise.
#[test]
fn a_clip_on_a_flac_is_silent_and_says_why() {
    let (song, clips) = drums(r#"{"id": "a", "start": 0, "length": 4, "asset": "loop.flac"}"#);
    let mut rig = Rig::with(song, |f| {
        let flac = include_bytes!("../../../../core/runtime/tests/fixtures/flac/16bit-stereo.flac");
        std::fs::create_dir_all(f.0.join("assets")).unwrap();
        std::fs::write(f.0.join("assets/loop.flac"), flac).unwrap();
        f.put("data/drums/loops.json", &clips);
    });
    let said = "loop.flac on Drums isn't a WAV file, so it plays silent: a clip plays WAV files only. To use a FLAC, import it \
                (File ▸ Import Audio…): music2 brings it in as a WAV. The rest of the song plays on.";
    assert_eq!(rig.told_soon(), [said]);
    assert_eq!(rig.problems(), [("asset.unreadable".into(), Some("drums".into()), said.into())]);
}

// ab r4, RENDER-25, D3: a Sampler file that is in assets/ but won't decode is one problem,
// `asset.unreadable`, naming the file, what plays it and its track, toasted once. Its build found
// it, before the plan was sent. A new plan finds it again: listed again, with no toast. Before,
// nothing was said.
#[test]
fn a_sampler_file_that_wont_decode_is_told_naming_it_and_its_track() {
    let song = |zone: &str| {
        format!(
            "import {{ song, track }} from '@m2/core';\nimport {{ sampler }} from 'std/sampler';\n\
             const d = track('drums', {{ name: 'Drums' }}, ({{ lane }}) => sampler(lane.notes('hits'), {{ zones: {{ {zone}: 'kick.wav' }} }}));\n\
             export default song(() => d.out);\n"
        )
    };
    let mut rig = Rig::with(&song("36"), |f| {
        f.put(
            "data/drums/hits.json",
            r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C2", "start": 0, "length": 1}]}]}"#,
        );
        f.put("assets/kick.wav", "RIFF\u{0}\u{0}\u{0}\u{0}WAVEnot a sound at all");
    });
    let said = "kick.wav, which the instrument on Drums plays, can't be read, so it plays silent. It may be damaged. The rest of the song \
                plays on.";
    assert_eq!(rig.told_soon(), [said]);
    let listed = [("asset.unreadable".into(), Some("drums".into()), said.to_string())];
    assert_eq!(rig.problems(), listed);
    rig.fixture.put("song.ts", &song("38"));
    assert!(rig.graph.poll());
    rig.fill(2);
    assert!(rig.told().is_empty(), "toasted once in the open song");
    assert_eq!(rig.problems(), listed, "listed again: the new plan plays it silent too");
}

// FMT-24 (review F1): a Sampler file whose header says more samples than one whole decode may make
// (a FLAC of silence, 29 KB, that would decode to 512 MiB) is refused before it is decoded: one
// problem, `asset.unreadable`, in words that say why, not that it may be damaged.
#[test]
fn a_sampler_file_too_long_to_hold_is_told_so_and_never_decoded() {
    let song = "import { song, track } from '@m2/core';\nimport { sampler } from 'std/sampler';\n\
                const d = track('drums', { name: 'Drums' }, ({ lane }) => sampler(lane.notes('hits'), { zones: { 36: 'kick.flac' } }));\n\
                export default song(() => d.out);\n";
    let mut rig = Rig::with(song, |f| {
        f.put(
            "data/drums/hits.json",
            r#"{"clips": [{"id": "a", "start": 0, "length": 4, "notes": [{"pitch": "C2", "start": 0, "length": 1}]}]}"#,
        );
        std::fs::create_dir_all(f.0.join("assets")).unwrap();
        std::fs::write(f.0.join("assets/kick.flac"), include_bytes!("../../../../core/runtime/tests/fixtures/flac/bomb.flac")).unwrap();
    });
    let said = "kick.flac, which the instrument on Drums plays, is too long to hold whole (over 134,217,728 samples, about 23 minutes \
                of stereo at 48 kHz), so it plays silent. The rest of the song plays on.";
    assert_eq!(rig.told_soon(), [said]);
    assert_eq!(rig.problems(), [("asset.unreadable".into(), Some("drums".into()), said.to_string())]);
}

// sx, RENDER-25, RENDER-30: a warped clip whose stretch fails while the song plays (here its
// source's reads fail on the stretch thread) is one problem, `stretch.failed`, naming the clip's
// file and its track, toasted once. Before, a line in the log said so, and nothing else.
#[test]
fn a_stretch_that_fails_live_is_told_naming_the_clip() {
    let (song, clips) = drums(r#"{"id": "w", "start": 0, "length": 6, "asset": "a.wav", "warp": [[0, 0], [3, 6]]}"#);
    let mut rig = Rig::with(song, |f| {
        let path = wav(f, "a.wav", 3.0);
        f.put("data/drums/loops.json", &clips);
        unplug(&path, "m2-stretch");
    });
    let said = "The warped clip of a.wav on Drums can't be stretched, so it plays silent. The rest of the song plays on.";
    assert_eq!(rig.told_soon(), [said]);
    assert_eq!(rig.problems(), [("stretch.failed".into(), Some("drums".into()), said.into())]);
    rig.fill(40);
    assert!(rig.told().is_empty() && rig.problems().len() == 1, "told once");
}

// D2 (as built in slice 2): a plan's build reports what it can't read before the plan is sent, and
// the notices thread may read the ring in between. A fault of a plan newer than the one sent last
// waits for it, and is named once it is sent; one of a plan made but never sent is dropped once a
// newer plan is.
#[test]
fn a_fault_from_a_plan_not_sent_yet_waits_for_it() {
    let rig = Rig::new();
    let (plan, node) = rig.node("bass", delay);
    let (token, namers) = rig.graph.inspect(move |a| {
        let o = a.open.as_ref().unwrap();
        let rev = o.plays.as_ref().unwrap().revision.clone();
        let namer = |id| m2_editor::faults::Namer::new(id, rev.clone(), &o.session.packages);
        (o.token, [namer(plan), namer(plan + 5)])
    });
    let [first, next] = namers;
    let (mut told, faults) = (super::Told::default(), m2_runtime::fault::Faults::default());
    told.sent(token, first);
    let fault = |plan| Fault { code: Code::Stopped, plan, subject: Subject::Node(node) };
    faults.report(fault(plan + 3));
    faults.report(fault(plan + 5));
    assert_eq!(told.hear(&faults).0, Vec::<String>::new(), "neither plan is sent yet");
    told.sent(token, next);
    assert_eq!(told.hear(&faults).0, [stopped("The delay on Bass")], "named once its plan is sent; the plan never sent is dropped");
    assert!(told.hear(&faults).0.is_empty());
}

// The ex review's r2: another song opened without closing this one (File ▸ Open) lists none of
// this song's problems, an input's neither: they are of the song that was open.
#[test]
fn another_song_lists_none_of_the_last_songs_problems() {
    let rig = Rig::new();
    let (plan, _) = rig.node("bass", delay);
    let (token, namers) = rig.graph.inspect(move |a| {
        let o = a.open.as_ref().unwrap();
        let rev = o.plays.as_ref().unwrap().revision.clone();
        let namer = |id| m2_editor::faults::Namer::new(id, rev.clone(), &o.session.packages);
        (o.token, [namer(plan), namer(plan + 1)])
    });
    let [first, next] = namers;
    let (mut told, faults) = (super::Told::default(), m2_runtime::fault::Faults::default());
    told.sent(token, first);
    told.opened(token, super::InputNamer { open: 1, used: "Mic A".into(), missing: None });
    faults.report(Fault { code: Code::InputLost, plan: 0, subject: Subject::Input(1) });
    told.hear(&faults);
    assert_eq!(told.problems(token).len(), 1, "listed in its song");
    told.sent(token + 1, next);
    assert!(told.problems(token + 1).is_empty(), "none in the next song");
}

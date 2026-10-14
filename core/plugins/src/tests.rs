//! The host through [`PluginHost`]: main-thread calls on the test's dedicated plugin main thread,
//! `process` on the test's own thread. The tests with real plugins need Surge XT or Airwindows
//! Consolidated on `CLAP_PATH`, and skip with a message without them; the probe's run everywhere.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::*;

const SR: u32 = 48_000;
const BLOCK: u32 = 128;

fn setup() -> Arc<dyn PluginHost> {
    test_main_thread();
    probe::install();
    host()
}

fn plugin(id: &str) -> Option<Found> {
    match find(id) {
        Ok(f) => Some(f),
        Err(_) => {
            eprintln!("skipping: {id} is not installed");
            None
        }
    }
}

/// An instance created and activated, destroyed when dropped.
struct Inst {
    host: Arc<dyn PluginHost>,
    h: Handle,
    info: Info,
}

impl Inst {
    fn new(f: &Found, state: Option<Vec<u8>>, owner: &Owner) -> Inst {
        let host = setup();
        let (h, info) = host.create(f, state.map(|b| Arc::new(Blob::new(b))), owner).unwrap();
        host.activate(h, SR, BLOCK).unwrap();
        Inst { host, h, info }
    }

    fn value(&self, id: u32) -> Option<f64> {
        match self.host.main_call(self.h, MainCall::Value(id)).unwrap() {
            Reply::Value(v) => v,
            r => panic!("{r:?}"),
        }
    }
}

impl Drop for Inst {
    fn drop(&mut self) {
        self.host.destroy(self.h);
    }
}

/// Render `seconds` in blocks on this thread, with `events(block_start)` giving each block's events.
fn render(inst: &Inst, seconds: f64, input: Option<&dyn Fn(u64) -> f32>, events: &dyn Fn(u64) -> Vec<Event>) -> Vec<f32> {
    let mut out = vec![];
    let (mut l, mut r) = (vec![0.0; BLOCK as usize], vec![0.0; BLOCK as usize]);
    let (mut ev, mut back) = (Events::with_capacity(256), Events::with_capacity(256));
    let mut t = 0u64;
    while (t as f64) < seconds * SR as f64 {
        let x: Vec<f32> = (0..BLOCK as u64).map(|i| input.map_or(0.0, |f| f(t + i))).collect();
        ev.clear();
        for e in events(t) {
            ev.push(e);
        }
        let mut io = SharedBuffers { inputs: [&x, &x], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: BLOCK };
        let transport = Transport { seconds: t as f64 / SR as f64, bpm: 120.0, playing: true, steady: t as i64, ..Default::default() };
        assert_ne!(inst.host.process(inst.h, &mut io, &ev, &mut back, &transport), Status::Error);
        out.extend_from_slice(&l);
        t += BLOCK as u64;
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// Wait until `f` holds, for a second at most.
fn eventually(mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(2) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

fn n(a: &std::sync::atomic::AtomicU64) -> u64 {
    a.load(Ordering::Relaxed)
}

#[test]
fn finds_installed_plugins() {
    setup();
    let (found, errors) = scan();
    assert!(errors.is_empty(), "{errors:?}");
    for f in &found {
        assert!(!f.id.is_empty() && !f.name.is_empty());
    }
    assert!(found.iter().any(|f| f.id == probe::ID), "the probe, once installed");
}

// Notes reach the plugin on their sample. Surge XT works in blocks of 32 samples of its own and
// applies each event at the start of the block it falls in (`clap_direct_process`, Surge XT 1.3.4),
// so a note at sample 5 can sound from sample 0; its oscillator starts at a random phase, so the
// first sound comes up to a cycle later (a note at 5: first non-zero sample at 0 to 108, before 5
// in 10 of 200 runs). So the note sits on one of Surge's block lines inside the host's block, with
// the oscillator's retrigger on: it then sounds from exactly that sample (100 of 100 runs at ten
// offsets), and a note passed a sample early, at the block's start or a block late does not.
#[test]
fn surge_plays_notes() {
    setup();
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let s = Inst::new(&f, None, &Owner::discard());
    assert!(s.info.note_in && s.info.audio_out == Some(2));
    let retrigger = s.info.params.iter().find(|p| p.name == "A Osc 1 Retrigger").expect("Osc 1's retrigger").id;
    const AT: u32 = 96;
    let off = (0.3 * SR as f64) as u64;
    let out = render(&s, 1.5, None, &|t| {
        let mut e = vec![];
        if t == 0 {
            e.push(Event::Param { time: 0, id: retrigger, value: 1.0 });
            e.push(Event::NoteOn { time: AT, id: 1, key: 57, velocity: 0.9 });
        }
        if (t..t + BLOCK as u64).contains(&off) {
            e.push(Event::NoteOff { time: (off - t) as u32, id: 1, key: 57 });
        }
        e
    });
    let sr = SR as f64;
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak(&out[..(0.3 * sr) as usize]) > 0.01, "the note is audible");
    assert!(peak(&out[(1.3 * sr) as usize..]) < 1e-3, "and released after note-off");
    assert_eq!(out.iter().position(|x| *x != 0.0), Some(AT as usize), "the note sounds from its sample, and nothing before it");
}

#[test]
fn state_round_trips() {
    setup();
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let a = Inst::new(&f, None, &Owner::discard());
    let p = a.info.params.iter().find(|p| !p.stepped && p.max > p.min && p.name.contains("Volume")).expect("a volume parameter").clone();
    let target = p.min + (p.max - p.min) * 0.237;
    render(&a, 0.01, None, &|t| if t == 0 { vec![Event::Param { time: 0, id: p.id, value: target }] } else { vec![] });
    assert!((a.value(p.id).unwrap() - target).abs() < 1e-6, "{} is set", p.name);

    let blob = a.host.save_state(a.h).recv().unwrap().unwrap();
    assert!(!blob.bytes.is_empty());
    let b = Inst::new(&f, None, &Owner::discard());
    assert!((b.value(p.id).unwrap() - target).abs() > 1e-3, "a fresh instance starts elsewhere");
    b.host.load_state(b.h, Arc::new(blob.clone())).unwrap();
    assert!((b.value(p.id).unwrap() - target).abs() < 1e-6, "{} survives save and load", p.name);
    // Created with the state: loaded before it is activated.
    let c = Inst::new(&f, Some(blob.bytes), &Owner::discard());
    assert!((c.value(p.id).unwrap() - target).abs() < 1e-6);
}

#[test]
fn airwindows_processes_audio() {
    setup();
    let Some(f) = plugin("com.airwindows.consolidated") else { return };
    let fx = Inst::new(&f, None, &Owner::discard());
    assert_eq!((fx.info.audio_in, fx.info.audio_out), (Some(2), Some(2)));
    let sine = |t: u64| (t as f32 / SR as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5;
    let out = render(&fx, 0.5, Some(&sine), &|_| vec![]);
    assert!(out.iter().all(|x| x.is_finite()));
    assert!(peak(&out[(0.25 * SR as f64) as usize..]) > 0.05, "audio comes through");
    fx.host.reset(fx.h);
}

// FMT-18: every parameter is addressed `<name>_<id>`, so two with one name never collide
// and the reserved ports (`in`, `notes`, `out`, `key`) are never taken.
#[test]
fn param_addresses_are_a_name_then_the_id() {
    let p = |id, name: &str| Param { id, name: name.into(), module: String::new(), min: 0.0, max: 1.0, default: 0.0, stepped: false };
    let names =
        param_port_names(&[p(1, "Dry/Wet"), p(2, "-"), p(3, "-"), p(4, "Filter 1 Cutoff"), p(5, "in"), p(6, "2nd"), p(7, "Key")]);
    assert_eq!(names, ["dry_wet_1", "param_2", "param_3", "filter_1_cutoff_4", "in_5", "2nd_6", "key_7"]);
    assert_eq!(param_address("dry_wet_12"), Some(("dry_wet", 12)));
    assert_eq!(param_address("osc_2_4294967295"), Some(("osc_2", u32::MAX)));
    for unaddressed in ["dry_wet", "in", "notes", "mix_", "mix_+1", "mix_99999999999"] {
        assert_eq!(param_address(unaddressed), None, "{unaddressed}");
    }
}

#[test]
fn parameter_values_have_display_text() {
    setup();
    let Some(f) = plugin("org.surge-synth-team.surge-xt") else { return };
    let s = Inst::new(&f, None, &Owner::discard());
    let p = s.info.params.iter().find(|p| p.name == "A Filter 1 Type").expect("a filter type parameter");
    let text = |v: f64| match s.host.main_call(s.h, MainCall::Text(p.id, v)).unwrap() {
        Reply::Text(t) => t.unwrap_or_default(),
        r => panic!("{r:?}"),
    };
    let names: Vec<String> = (0..=20).map(|i| text(i as f64 / 20.0)).collect();
    println!("A Filter 1 Type: {names:?}");
    assert!(names.iter().any(|n| !n.is_empty()));
}

// HOST-2: every main-thread call runs on the plugin main thread, whose thread checks say "main"
// and not "audio"; `process` runs on the calling thread, which they call "audio" and not "main";
// a callback requested from `process` runs on the main thread, never inside `process`; and
// `stop_processing` runs on the main thread once it holds the audio role, when destroyed.
#[test]
fn the_thread_checks_answer_truthfully() {
    let host = setup();
    let f = find(probe::ID).unwrap();
    let inst = Inst::new(&f, Some(probe::state("thread-checks", 1.0)), &Owner::discard());
    let stats = probe::stats(inst.h).unwrap();
    assert_eq!(*stats.tag.lock().unwrap(), "thread-checks");
    let out = render(&inst, 0.2, None, &|t| if t == 0 { vec![Event::NoteOn { time: 3, id: 1, key: 69, velocity: 1.0 }] } else { vec![] });
    assert!(peak(&out) > 0.05 && peak(&out[..3]) == 0.0, "the probe plays its note from its frame");
    // Requests made between two sweeps run once (CLAP allows it).
    assert!(eventually(|| n(&stats.callbacks) >= 1), "callbacks requested from process ran");
    assert!(n(&stats.requested) >= n(&stats.callbacks) && n(&stats.requested) > 1);
    let h = inst.h;
    drop(inst);
    assert!(eventually(|| probe::stats(h).is_none()), "destroyed, on the main thread");
    let main = main_thread().unwrap();
    let calls = stats.main_calls();
    let what: Vec<&str> = calls.iter().map(|c| c.what).collect();
    assert_eq!(what.iter().filter(|w| **w == "stop_processing").count(), 1, "{what:?}");
    for want in ["init", "load", "activate", "on_main_thread", "stop_processing", "deactivate", "destroy"] {
        assert!(what.contains(&want), "{want} in {what:?}");
    }
    for c in &calls {
        assert!(c.says_main, "{} ran where the host doesn't say main: {c:?}", c.what);
        assert_eq!(c.says_audio, c.what == "stop_processing", "{c:?}: the audio role only to stop processing");
        assert!(!c.process_main, "tests serve plugins from a dedicated thread: {c:?}");
    }
    assert!(calls.iter().all(|c| c.thread == calls[0].thread), "one main thread");
    assert!(!main.is_current());
    assert!(n(&stats.process) > 0);
    assert_eq!(n(&stats.process_says_main), 0, "process isn't the main thread");
    assert_eq!(n(&stats.process_not_audio), 0, "process holds the audio role");
    assert_eq!((n(&stats.start_on_audio), n(&stats.start_elsewhere)), (1, 0), "started on the audio thread");
    assert_eq!(n(&stats.callbacks_in_process), 0, "on_main_thread never runs inside process");
    drop(host);
}

// HOST-1, C3: unsolicited events go to the instance's own owner, never another's.
#[test]
fn events_go_to_their_owner() {
    setup();
    let f = find(probe::ID).unwrap();
    let ((a, a_rx), (b, b_rx)) = (Owner::new(), Owner::new());
    let x = Inst::new(&f, None, &a);
    let y = Inst::new(&f, None, &b);
    assert_eq!((a.live(), b.live()), (1, 1));
    probe::stats(x.h).unwrap().restart.store(true, Ordering::Relaxed);
    render(&x, 0.01, None, &|_| vec![]);
    render(&y, 0.01, None, &|_| vec![]);
    assert_eq!(a_rx.recv_timeout(Duration::from_secs(2)).unwrap(), HostEvent::Restart(x.h));
    std::thread::sleep(Duration::from_millis(30));
    assert!(b_rx.try_recv().is_err(), "the other owner saw nothing");
    let h = x.h;
    drop(x);
    assert!(eventually(|| a.live() == 0), "the owner counts its instances");
    assert_eq!(b.live(), 1);
    // A stale handle does nothing, and says so.
    assert!(x_is_gone(&*host(), h));
    drop(y);
}

fn x_is_gone(host: &dyn PluginHost, h: Handle) -> bool {
    let (mut l, mut r) = ([1.0f32; 16], [1.0f32; 16]);
    let mut io = SharedBuffers { inputs: [&[0.0; 16], &[0.0; 16]], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: 16 };
    let (ev, mut back) = (Events::with_capacity(1), Events::with_capacity(1));
    let status = host.process(h, &mut io, &ev, &mut back, &Transport::default());
    status == Status::Error && l == [0.0; 16] && host.main_call(h, MainCall::Value(1)).is_err() && !host.take_process_request(h)
}

// C3: each `save_state` answers on its own reply, while other threads save at once.
#[test]
fn saves_answer_their_own_callers() {
    setup();
    let f = find(probe::ID).unwrap();
    let owner = Owner::discard();
    let insts: Vec<Inst> = (0..6).map(|i| Inst::new(&f, Some(probe::state(&format!("save-{i}"), 1.0)), &owner)).collect();
    std::thread::scope(|s| {
        for (i, inst) in insts.iter().enumerate() {
            s.spawn(move || {
                for _ in 0..20 {
                    let blob = inst.host.save_state(inst.h).recv().unwrap().unwrap();
                    assert!(blob.bytes.ends_with(format!("save-{i}").as_bytes()));
                }
            });
        }
    });
    let lat = insts[0].host.latency(insts[0].h).recv().unwrap();
    assert_eq!(lat, Ok(0));
}

// HOME-6: a blob equal to the state the instance holds isn't loaded again: the one it was created
// with, or the one it last saved; another is, and is then the state it holds.
#[test]
fn a_load_of_the_held_state_is_skipped() {
    setup();
    let f = find(probe::ID).unwrap();
    let created = Blob::new(probe::state("created", 1.0));
    let inst = Inst::new(&f, Some(created.bytes.clone()), &Owner::discard());
    let stats = probe::stats(inst.h).unwrap();
    let loads = || stats.main_calls().iter().filter(|c| c.what == "load").count();
    assert_eq!(loads(), 1);
    assert!(!inst.host.load_state(inst.h, Arc::new(created)).unwrap(), "the blob it was created with, skipped");
    let saved = inst.host.save_state(inst.h).recv().unwrap().unwrap();
    assert!(!inst.host.load_state(inst.h, Arc::new(saved)).unwrap(), "its own save, skipped");
    assert_eq!(loads(), 1);
    let other = Arc::new(Blob::new(probe::state("other", 0.5)));
    assert!(inst.host.load_state(inst.h, other.clone()).unwrap());
    assert_eq!((loads(), inst.value(probe::GAIN)), (2, Some(0.5)));
    assert_eq!(*stats.tag.lock().unwrap(), "other");
    assert!(!inst.host.load_state(inst.h, other).unwrap(), "the blob it just loaded, skipped");
}

// HOME-6: the skip compares with what the instance holds now, not with its last save: save S, load
// C (an undo), load S (a redo), and S loads.
#[test]
fn a_reload_after_another_load_is_never_skipped() {
    setup();
    let f = find(probe::ID).unwrap();
    let inst = Inst::new(&f, Some(probe::state("s", 1.0)), &Owner::discard());
    let s = Arc::new(inst.host.save_state(inst.h).recv().unwrap().unwrap());
    assert!(inst.host.load_state(inst.h, Arc::new(Blob::new(probe::state("c", 0.5)))).unwrap());
    assert!(inst.host.load_state(inst.h, s).unwrap(), "the redo loads");
    assert_eq!((probe::stats(inst.h).unwrap().tag.lock().unwrap().as_str(), inst.value(probe::GAIN)), ("s", Some(1.0)));
}

// RENDER-25: while a plugin is created its owner says so, by name, and no longer once it is.
#[test]
fn an_owner_shows_what_it_is_loading() {
    let host = setup();
    let f = find(probe::ID).unwrap();
    let owner = Owner::discard();
    let creating = {
        let (host, f, owner) = (host.clone(), f.clone(), owner.clone());
        std::thread::spawn(move || host.create(&f, Some(Arc::new(Blob::new(b"slow patch".to_vec()))), &owner).unwrap().0)
    };
    let t = Instant::now();
    while owner.loading().is_empty() && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(owner.loading(), ["m2 probe"]);
    let h = creating.join().unwrap();
    assert!(owner.loading().is_empty());
    host.destroy(h);
}

// A state the plugin refuses fails `create`, and leaves nothing behind.
#[test]
fn a_refused_state_fails_create() {
    let host = setup();
    let f = find(probe::FAILS_STATE).unwrap();
    let owner = Owner::discard();
    let e = host.create(&f, Some(Arc::new(Blob::new(b"failing".to_vec()))), &owner).unwrap_err();
    assert!(format!("{e:#}").contains("refused the saved state"), "{e:#}");
    assert_eq!(owner.live(), 0, "no instance is left for it");
    let ok = Inst::new(&f, Some(b"fine".to_vec()), &owner);
    assert_eq!(*probe::stats(ok.h).unwrap().tag.lock().unwrap(), "fine");
}

// RENDER-6a, §5.6: a plugin that slept asks to be processed from a thread of its own; the
// engine reads the request with no sweep.
#[test]
fn request_process_is_seen_once() {
    setup();
    let f = find(probe::ID).unwrap();
    let inst = Inst::new(&f, None, &Owner::discard());
    let stats = probe::stats(inst.h).unwrap();
    stats.sleep.store(true, Ordering::Relaxed);
    let (mut l, mut r) = (vec![0.0; BLOCK as usize], vec![0.0; BLOCK as usize]);
    let silence = vec![0.0f32; BLOCK as usize];
    let mut io = SharedBuffers { inputs: [&silence, &silence], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: BLOCK };
    let (ev, mut back) = (Events::with_capacity(4), Events::with_capacity(4));
    assert_eq!(inst.host.process(inst.h, &mut io, &ev, &mut back, &Transport::default()), Status::Sleep);
    assert!(!inst.host.take_process_request(inst.h));
    let h = inst.h;
    std::thread::spawn(move || assert!(probe::request_process(h))).join().unwrap();
    assert!(inst.host.take_process_request(inst.h));
    assert!(!inst.host.take_process_request(inst.h), "taken once");
}

// RENDER-28: an event list never grows; what doesn't fit is counted. Events read back as they
// were pushed.
#[test]
fn event_lists_keep_their_room() {
    let mut ev = Events::with_capacity(2);
    let on = Event::NoteOn { time: 1, id: 7, key: 60, velocity: 0.5 };
    let tune = Event::Tuning { time: 1, id: 7, key: 60, semitones: 0.25 };
    assert!(ev.push(on) && ev.push(tune));
    assert!(!ev.push(Event::Param { time: 2, id: 1, value: 0.5 }));
    assert_eq!((ev.len(), ev.capacity(), ev.dropped()), (2, 2, 1));
    assert_eq!(ev.iter().collect::<Vec<_>>(), [on, tune]);
    assert!(ev.sorted());
}

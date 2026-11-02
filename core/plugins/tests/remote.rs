//! HOST-3: the host process. The same suite of calls on the in-process host and on [`Remote`], the
//! probe's output bit for bit the same; a crash or a hang there marks its instances and tells their
//! owners, and the next create starts a new process.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use m2_plugins::{
    Blob, Event, Events, Found, HostEvent, InProcess, MainCall, Offered, Owner, PluginHost, PresetAt, Remote, RenderMode, Reply,
    SharedBuffers, Status, Transport, probe,
};

const SR: u32 = 48_000;
const N: usize = 128;

fn remote() -> Remote {
    Remote::new(PathBuf::from(env!("CARGO_BIN_EXE_m2-plugin-host")), vec![])
}

fn setup() {
    m2_plugins::test_main_thread();
    probe::install();
}

fn the_probe(host: &dyn PluginHost) -> Found {
    let (found, errors) = host.scan();
    found.into_iter().find(|f| f.id == probe::ID).unwrap_or_else(|| panic!("the probe isn't found: {errors:?}"))
}

/// `calls` blocks of the probe: input a ramp, a note at the start, its gain moved halfway. Its
/// output's bits.
fn play(host: &dyn PluginHost, h: m2_plugins::Handle, calls: usize) -> (Vec<u32>, Vec<Status>) {
    let (mut ev, mut back) = (Events::with_capacity(64), Events::with_capacity(64));
    let input: Vec<f32> = (0..N).map(|i| i as f32 / N as f32 - 0.5).collect();
    let (mut l, mut r) = (vec![0.0f32; N], vec![0.0f32; N]);
    let (mut bits, mut status) = (vec![], vec![]);
    for c in 0..calls {
        ev.clear();
        if c == 0 {
            ev.push(Event::NoteOn { time: 3, id: 1, key: 60, velocity: 0.8 });
        }
        if c == calls / 2 {
            ev.push(Event::Param { time: 5, id: probe::GAIN, value: 0.75 });
        }
        let t = Transport { bpm: 120.0, playing: true, steady: (c * N) as i64, tsig: (4, 4), ..Transport::default() };
        let mut io = SharedBuffers { inputs: [&input, &input], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: N as u32 };
        status.push(host.process(h, &mut io, &ev, &mut back, &t));
        bits.extend(l.iter().chain(&r).map(|x| x.to_bits()));
    }
    (bits, status)
}

/// Every call of the trait once, as the engine and the session make them. What it heard back.
fn suite(host: &dyn PluginHost) -> Vec<String> {
    let mut heard = vec![];
    let found = the_probe(host);
    let (owner, _events) = Owner::new();
    let blob = Arc::new(Blob::new(probe::state("suite", 0.5)));
    let (h, info) = host.create(&found, Some(blob.clone()), &owner).unwrap();
    heard.push(format!(
        "info {:?} {:?} {} {:?}",
        info.audio_in,
        info.audio_out,
        info.note_in,
        info.params.iter().map(|p| p.id).collect::<Vec<_>>()
    ));
    assert_eq!((owner.live(), owner.made()), (1, 1));
    heard.push(format!("activate {:?}", host.activate(h, SR, N as u32).unwrap()));
    heard.push(format!("same blob loads {}", host.load_state(h, blob.clone()).unwrap()));
    let saved = host.save_state(h).recv().unwrap().unwrap();
    heard.push(format!("saved loads {}", host.load_state(h, Arc::new(saved.clone())).unwrap()));
    heard.push(format!("another loads {}", host.load_state(h, Arc::new(Blob::new(probe::state("suite", 0.25)))).unwrap()));
    heard.push(format!("latency {:?}", host.latency(h).recv().unwrap()));
    let value = |id| match host.main_call(h, MainCall::Value(id)).unwrap() {
        Reply::Value(v) => v,
        r => panic!("{r:?}"),
    };
    heard.push(format!("gain {:?}", value(probe::GAIN)));
    heard.push(format!("text {:?}", host.main_call(h, MainCall::Text(probe::GAIN, 0.5)).map(|r| format!("{r:?}")).unwrap()));
    heard.push(format!("info again {}", matches!(host.main_call(h, MainCall::Info).unwrap(), Reply::Info(i) if i.params.len() == 4)));
    heard.push(format!("log {:?}", host.main_call(h, MainCall::TakeLog).map(|r| format!("{r:?}")).unwrap()));
    let (bits, status) = play(host, h, 12);
    heard.push(format!("played {} samples, {:?}, a sum of {}", bits.len(), status[0], bits.iter().map(|&b| b as u64).sum::<u64>()));
    heard.push(format!("gain after {:?}", value(probe::GAIN)));
    host.reset(h);
    let (after, _) = play(host, h, 2);
    heard.push(format!("after a reset, a sum of {}", after.iter().map(|&b| b as u64).sum::<u64>()));
    // Offline, as an export takes it (RENDER-1).
    host.deactivate(h);
    heard.push(format!("offline {:?}", host.set_render_mode(h, RenderMode::Offline).unwrap()));
    heard.push(format!("activate again {:?}", host.activate(h, SR, 64).unwrap()));
    heard.push(format!("a call longer than activated for: {:?}", play(host, h, 1).1));
    host.destroy(h);
    let t = Instant::now();
    while owner.live() > 0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(owner.live(), 0, "destroyed");
    heard.push(format!("a stale handle: {:?} {}", play(host, h, 1).1, host.load_state(h, blob).is_err()));
    heard
}

#[test]
fn the_host_suite_passes_on_both_hosts() {
    setup();
    let here = suite(&InProcess);
    let r = remote();
    let there = suite(&r);
    assert_eq!(here, there, "the host process answers as the in-process host does, the probe's bits too");
    assert!(r.process_id().is_some(), "a host process ran it");
    assert_eq!(r.counts(), (1, 1));
}

// HOST-8: a plugin's own presets are found where it runs, through CLAP's preset discovery: the
// probe's Bright and Dark, inside it. The same on both hosts.
#[test]
fn the_probe_offers_its_presets_on_both_hosts() {
    setup();
    let r = remote();
    for host in [&InProcess as &dyn PluginHost, &r] {
        let offered = host.presets(&the_probe(host)).unwrap();
        let at = |name: &str, key: &str| Offered { name: name.into(), location: PresetAt::Plugin, key: Some(key.into()) };
        assert_eq!(offered, [at("Bright", "bright"), at("Dark", "dark")]);
    }
    assert!(r.process_id().is_some(), "the host process found them");
}

// HOST-8: the search runs on a thread of its own, never on the plugin main thread, which serves
// every playing plugin's main-thread calls and windows: a slow search must not hold them up.
#[test]
fn a_preset_search_never_runs_on_the_main_thread() {
    setup();
    let offered = InProcess.presets(&the_probe(&InProcess)).unwrap();
    assert_eq!(offered.len(), 2);
    let (searches, on_main) = probe::searches();
    assert!(searches > 0, "the probe was searched");
    assert_eq!(on_main, 0, "{on_main} of {searches} searches ran on the plugin main thread");
}

// HOST-8: one of them loads on an instance made for it (CLAP's preset-load), whose saved state then
// holds it; the instance is destroyed after, on both hosts.
#[test]
fn a_plugin_preset_loads_on_a_scratch_instance() {
    setup();
    let r = remote();
    for host in [&InProcess as &dyn PluginHost, &r] {
        let found = the_probe(host);
        let dark = host.presets(&found).unwrap().into_iter().find(|o| o.name == "Dark").expect("Dark");
        let (owner, _events) = Owner::new();
        let (h, _) = host.create(&found, None, &owner).unwrap();
        let loaded = host.main_call(h, MainCall::LoadPreset { location: dark.location, key: dark.key }).unwrap();
        assert!(matches!(loaded, Reply::Loaded), "{loaded:?}");
        let blob = host.save_state(h).recv().unwrap().unwrap();
        assert_eq!(blob.bytes, probe::state_with("dark", 0.5, 0.05), "its state holds Dark");
        let missing = host.main_call(h, MainCall::LoadPreset { location: PresetAt::Plugin, key: Some("none".into()) });
        assert!(missing.is_err(), "a preset it doesn't have is refused: {missing:?}");
        host.destroy(h);
        let t = Instant::now();
        while owner.live() > 0 && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!((owner.made(), owner.live()), (1, 0), "as many instances after as before");
    }
}

/// Wait for `owner` to hear `n` events that `want` takes.
fn heard(events: &m2_plugins::HostEvents, n: usize, want: impl Fn(&HostEvent) -> bool) -> Vec<HostEvent> {
    let t = Instant::now();
    let mut got = vec![];
    while got.len() < n && t.elapsed() < Duration::from_secs(5) {
        if let Ok(e) = events.recv_timeout(Duration::from_millis(50))
            && want(&e)
        {
            got.push(e);
        }
    }
    got
}

fn crashed_by(tag: &str) -> Duration {
    setup();
    let r = remote();
    let found = the_probe(&r);
    let (owner, events) = Owner::new();
    let make = |tag: &str| {
        let (h, _) = r.create(&found, Some(Arc::new(Blob::new(probe::state(tag, 1.0)))), &owner).unwrap();
        r.activate(h, SR, N as u32).unwrap();
        h
    };
    let (bad, good) = (make(tag), make("fine"));
    let first = r.process_id().unwrap();
    assert!(play(&r, good, 3).1.iter().all(|s| *s == Status::Continue));
    let t = Instant::now();
    let (_, status) = play(&r, bad, 3);
    let took = t.elapsed();
    assert_eq!(status, [Status::Continue, Status::Continue, Status::Error], "its third call ends the process");
    let hung = tag.starts_with("hang");
    let crashed = heard(&events, 2, |e| matches!((e, hung), (HostEvent::Crashed(_), false) | (HostEvent::Hung(_), true)));
    assert_eq!(crashed.len(), 2, "both instances of the process are said to have crashed, or hung: {crashed:?}");
    assert_eq!(play(&r, good, 1).1, [Status::Error], "the other instance's calls fail too, at once");
    assert!(r.activate(good, SR, N as u32).is_err(), "and its main-thread calls");
    r.destroy(bad);
    r.destroy(good);
    assert_eq!(owner.live(), 0, "a crashed instance is destroyed at once");
    // The next create starts a new process.
    let again = make("fine");
    assert_ne!(r.process_id(), Some(first));
    assert!(play(&r, again, 2).1.iter().all(|s| *s == Status::Continue));
    took
}

// HOST-3: a crash ends every call of that process's instances and tells their owners.
#[test]
fn a_crash_marks_every_instance_of_its_process() {
    let took = crashed_by("crash-at-3");
    assert!(took < Duration::from_millis(400), "a crash is seen at once: {took:?}");
}

// HOST-3, D5: a live call that waits 500 ms ends the process, the same way, said as a hang.
#[test]
fn a_hung_call_ends_its_process() {
    let took = crashed_by("hang-at-3");
    assert!(took >= Duration::from_millis(480) && took < Duration::from_secs(3), "ended after 500 ms: {took:?}");
}

/// `f`'s answer on a thread of its own, or None when it hasn't come in `most`.
fn within<T: Send + 'static>(most: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(f()));
    rx.recv_timeout(most).ok()
}

// HOST-3, D5: a main-thread call that waits the limit ends the process, said as a hang; its caller
// hears why, and an answer the other instance awaits fails too.
#[test]
fn a_main_thread_call_that_hangs_ends_its_process() {
    setup();
    let r = remote();
    r.set_limit(Duration::from_secs(1));
    let found = the_probe(&r);
    let (owner, events) = Owner::new();
    let make = |tag: &str| r.create(&found, Some(Arc::new(Blob::new(probe::state(tag, 1.0)))), &owner).unwrap().0;
    let (bad, good) = (make("hangs-activate"), make("fine"));
    r.activate(good, SR, N as u32).unwrap();
    let t = Instant::now();
    let rr = r.clone();
    let got = within(Duration::from_secs(10), move || rr.activate(bad, SR, N as u32).map_err(|e| format!("{e:#}")));
    let took = t.elapsed();
    let err = got.expect("the call never returned").expect_err("its activate never returns");
    assert!(err.contains("stopped answering"), "{err}");
    assert!(took >= Duration::from_millis(950) && took < Duration::from_secs(5), "ended after the limit: {took:?}");
    let hung = heard(&events, 2, |e| matches!(e, HostEvent::Hung(_)));
    assert_eq!(hung.len(), 2, "both instances of the process are said to have hung: {hung:?}");
    assert!(r.latency(good).recv_timeout(Duration::from_secs(1)).expect("an answer").is_err());
    r.destroy(bad);
    r.destroy(good);
}

// HOST-3, D5: an offline call (an export) that waits the limit ends the process too.
#[test]
fn an_offline_call_that_hangs_ends_its_process() {
    setup();
    let r = remote();
    r.set_limit(Duration::from_secs(1));
    let found = the_probe(&r);
    let (owner, events) = Owner::new();
    let (h, _) = r.create(&found, Some(Arc::new(Blob::new(probe::state("hang-at-3", 1.0)))), &owner).unwrap();
    r.activate(h, SR, N as u32).unwrap();
    r.set_render_mode(h, RenderMode::Offline).unwrap();
    let t = Instant::now();
    let rr = r.clone();
    let got = within(Duration::from_secs(10), move || play(&rr, h, 3).1);
    let took = t.elapsed();
    assert_eq!(got.expect("the call never returned"), [Status::Continue, Status::Continue, Status::Error]);
    assert!(took >= Duration::from_millis(950) && took < Duration::from_secs(5), "ended after the limit: {took:?}");
    assert_eq!(heard(&events, 1, |e| matches!(e, HostEvent::Hung(_))).len(), 1, "said as a hang");
    r.destroy(h);
}

// HOST-3 (PLAT-1, information only): a call's round trip through the host process, against the
// same call in this process. Run it in release: cargo test --release -p m2-plugins --test remote
// -- --ignored --nocapture round_trip
#[test]
#[ignore]
fn the_round_trip_is_measured() {
    setup();
    let r = remote();
    let found = the_probe(&r);
    let (owner, _events) = Owner::new();
    let mut times = vec![];
    for host in [&InProcess as &dyn PluginHost, &r] {
        let (h, _) = host.create(&found, None, &owner).unwrap();
        host.activate(h, SR, N as u32).unwrap();
        play(host, h, 200);
        let mut each: Vec<Duration> = (0..2000)
            .map(|_| {
                let t = Instant::now();
                play(host, h, 1);
                t.elapsed()
            })
            .collect();
        each.sort();
        times.push((each[each.len() / 2], each[each.len() * 99 / 100], each[each.len() - 1]));
        host.destroy(h);
    }
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    println!(
        "a probe call of {N} frames: in this process {:.1} us (median), {:.1} us (99%); through the host process {:.1} us, {:.1} us, at most {:.1} us",
        us(times[0].0),
        us(times[0].1),
        us(times[1].0),
        us(times[1].1),
        us(times[1].2)
    );
}

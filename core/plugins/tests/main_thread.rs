//! HOST-2 on the process's real main thread: `run_main` runs the program on a worker while the main
//! thread runs the OS's main loop (a CFRunLoop serving the main dispatch queue on macOS, a job queue
//! on Linux), and every main-thread call reaches it there. libtest owns the main thread, so this
//! test has no harness (`harness = false`).
//!
//! It runs the in-process host itself: that is what the host process runs under `run_main`
//! (HOST-3), where the probe's records can't be read from here.
//!
//! It creates the probe from the worker, checks where each call ran, leaves one instance alive when
//! the worker returns, and checks that `run_main` destroyed it before returning, and that the
//! executor refuses work after. With `CLAP_PATH` naming a folder that holds Surge XT, it does the
//! same with Surge.

use std::sync::Arc;

use m2_plugins::{Blob, Event, Events, Found, Handle, InProcess, Owner, PluginHost, SharedBuffers, Status, Transport, probe};

const RATE: u32 = 48_000;
const BLOCK: u32 = 128;

/// Process `blocks` blocks of `h` on this thread (the worker takes the audio role), with `events`
/// in the first. Returns the peak output.
fn play(host: &dyn PluginHost, h: Handle, blocks: usize, events: &[Event]) -> f32 {
    let (mut l, mut r) = (vec![0.0f32; BLOCK as usize], vec![0.0f32; BLOCK as usize]);
    let silence = vec![0.0f32; BLOCK as usize];
    let mut ev = Events::with_capacity(64);
    let mut out = Events::with_capacity(64);
    let mut peak = 0.0f32;
    for b in 0..blocks {
        ev.clear();
        if b == 0 {
            for e in events {
                ev.push(*e);
            }
        }
        let mut io = SharedBuffers { inputs: [&silence, &silence], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: BLOCK };
        let t = Transport { bpm: 120.0, playing: true, steady: (b as u32 * BLOCK) as i64, ..Default::default() };
        let s = host.process(h, &mut io, &ev, &mut out, &t);
        assert_ne!(s, Status::Error, "process failed");
        peak = l.iter().fold(peak, |m, x| m.max(x.abs()));
    }
    peak
}

fn main() {
    probe::install();
    let (left, surge) = m2_plugins::run_main(|| {
        let host: Arc<dyn PluginHost> = Arc::new(InProcess);
        let (owner, _events) = Owner::new();
        let found = find(probe::ID).expect("the probe");
        let (h, info) = host.create(&found, Some(Arc::new(Blob::new(probe::state("real-main", 1.0)))), &owner).expect("created");
        assert!(info.note_in && info.audio_out == Some(2));
        host.activate(h, RATE, BLOCK).expect("activated");
        assert!(play(&*host, h, 40, &[Event::NoteOn { time: 0, id: 1, key: 69, velocity: 1.0 }]) > 0.05, "the probe sounds");
        let stats = probe::stats(h).expect("its record");
        let saved = host.save_state(h).recv().expect("a reply").expect("saved");
        assert!(saved.bytes.ends_with(b"real-main"));
        // Its callbacks were requested from `process` (every 10 calls): the sweep runs them on the
        // main thread, never inside `process`.
        let t = std::time::Instant::now();
        while stats.callbacks.load(std::sync::atomic::Ordering::Relaxed) == 0 && t.elapsed().as_secs() < 2 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        host.destroy(h);
        let surge = surge(&*host, &owner);
        // One more, dropped as the program returns, while the main thread is busy: run_main must
        // destroy it on its way out.
        let (h2, _) = host.create(&found, None, &owner).expect("created");
        host.activate(h2, RATE, BLOCK).expect("activated");
        play(&*host, h2, 2, &[]);
        let left = probe::stats(h2).expect("its record");
        m2_plugins::main_thread().unwrap().post(|| std::thread::sleep(std::time::Duration::from_millis(100)));
        host.destroy(h2);
        assert!(host.live() > 0, "still alive when the program returns");
        (vec![stats, left], surge)
    });
    // HOST-2: every main-thread call ran on the process's main thread, which the host's thread
    // checks call main and not audio; process ran elsewhere, with the audio role.
    for s in &left {
        let calls = s.main_calls();
        for c in calls.iter().filter(|c| c.what != "stop_processing") {
            assert!(c.process_main && c.says_main && !c.says_audio, "{} ran off the main thread: {c:?}", c.what);
        }
        let stop: Vec<_> = calls.iter().filter(|c| c.what == "stop_processing").collect();
        assert_eq!(stop.len(), 1, "{calls:?}");
        assert!(stop[0].process_main && stop[0].says_audio, "the main thread takes the audio role to stop processing");
        assert!(calls.iter().any(|c| c.what == "destroy"), "destroyed: {calls:?}");
        let n = |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed);
        assert!(n(&s.process) > 0 && n(&s.process_on_main) == 0 && n(&s.process_says_main) == 0 && n(&s.process_not_audio) == 0);
        assert_eq!(n(&s.callbacks_in_process), 0, "on_main_thread never runs inside process");
    }
    assert!(left[0].callbacks.load(std::sync::atomic::Ordering::Relaxed) > 0, "requested callbacks ran");
    assert_eq!(InProcess::counts().0, InProcess::counts().1, "run_main destroyed what was left before returning");
    let refused = m2_plugins::main_thread().expect("installed").run(|| ());
    assert!(refused.is_err(), "the executor refuses work once it has shut down");
    println!("main_thread: ok ({})", surge.unwrap_or("Surge XT not on CLAP_PATH: skipped"));
}

/// A plugin installed, found in this process.
fn find(id: &str) -> Option<Found> {
    InProcess.scan().0.into_iter().find(|f| f.id == id)
}

/// Surge XT on the real main thread, played from the worker, destroyed. None when it isn't on
/// `CLAP_PATH` (an explicit folder: tests never read the user's own).
fn surge(host: &dyn PluginHost, owner: &Owner) -> Option<&'static str> {
    std::env::var_os("CLAP_PATH")?;
    let found = find("org.surge-synth-team.surge-xt")?;
    let (h, info) = host.create(&found, None, owner).expect("Surge XT is created on the main thread");
    assert!(info.note_in);
    host.activate(h, RATE, BLOCK).expect("activated");
    let peak = play(host, h, 200, &[Event::NoteOn { time: 0, id: 1, key: 57, velocity: 0.9 }]);
    assert!(peak > 0.01, "Surge XT plays: peak {peak}");
    host.destroy(h);
    Some("Surge XT too")
}

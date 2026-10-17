//! HOST-2 in the app, without launching it: the app's own hook ([`plugin_host`], the same file
//! `main` uses) installed on the process's main thread. On macOS the main thread then runs AppKit's
//! event loop, `[NSApp run]`, as Tauri does (with no window, no Dock icon and no menu bar), and the
//! plugins' main thread is AppKit's, reached through the main dispatch queue. On Linux it is the
//! app's loop thread of its own, and the main thread, which GTK would own, only waits. Meanwhile
//! the server, on other threads as in the app, opens a song with the probe plugin off the main
//! thread, plays it, rebuilds it (the instance carried, HOST-5), changes its state (loaded into
//! the same instance, RENDER-26b), and closes it the way quit does.
//!
//! Every main-thread call the probe recorded must have run on the plugins' main thread, which is
//! the process's main thread exactly when `APP_ON_PROCESS_MAIN`, with the host's thread checks
//! answering truthfully; `on_main_thread` must run there and never inside `process`; and quit's
//! close must leave no instance alive. libtest owns the main thread, so this test has no harness.

// What the test doesn't call (`leave_now`) is the app's.
#[allow(dead_code)]
#[path = "../src/plugin_host.rs"]
mod plugin_host;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use m2_os::main_loop::APP_ON_PROCESS_MAIN;
use m2_plugins::probe;
use m2_server::ServeOptions;

/// The longest the whole test may take before it is called hung: the app's main loop never served
/// the plugins, or quit never returned.
const HUNG: Duration = Duration::from_secs(120);

#[cfg(target_os = "macos")]
mod appkit {
    //! The few AppKit calls the test makes, through the Objective-C runtime (no new dependency).
    use std::ffi::{CStr, c_char, c_void};
    use std::ptr::null_mut;

    pub type Id = *mut c_void;

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {}

    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Id;
        fn objc_msgSend();
    }

    fn class(name: &CStr) -> Id {
        // SAFETY: a NUL-terminated class name.
        unsafe { objc_getClass(name.as_ptr()) }
    }

    fn sel(name: &CStr) -> Id {
        // SAFETY: a NUL-terminated selector name.
        unsafe { sel_registerName(name.as_ptr()) }
    }

    /// `objc_msgSend` as the function type of the method it calls (arm64 needs the exact one).
    macro_rules! send {
        ($ty:ty) => {
            // SAFETY: the caller names the method's real signature.
            unsafe { std::mem::transmute::<*const (), $ty>(objc_msgSend as *const ()) }
        };
    }

    /// `[NSApplication sharedApplication]`, prohibited from becoming a regular app: no Dock icon,
    /// no menu bar, never active. On the main thread.
    pub fn app() -> Id {
        let shared = send!(extern "C" fn(Id, Id) -> Id);
        shared(class(c"NSApplication"), sel(c"sharedApplication"))
    }

    pub fn prohibit(app: Id) {
        let set = send!(extern "C" fn(Id, Id, isize) -> bool);
        // NSApplicationActivationPolicyProhibited
        set(app, sel(c"setActivationPolicy:"), 2);
    }

    /// `[NSApp run]`: AppKit's event loop, as Tauri (tao) runs it, until [`stop`].
    pub fn run(app: Id) {
        let run = send!(extern "C" fn(Id, Id));
        run(app, sel(c"run"));
    }

    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }

    /// `[NSApp stop:nil]`, then an event, so the loop wakes and sees it. On the main thread.
    pub fn stop() {
        let app = app();
        let stop = send!(extern "C" fn(Id, Id, Id));
        stop(app, sel(c"stop:"), null_mut());
        let make = send!(extern "C" fn(Id, Id, usize, Point, usize, f64, isize, Id, i16, isize, isize) -> Id);
        let event = make(
            class(c"NSEvent"),
            sel(c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:"),
            15, // NSEventTypeApplicationDefined
            Point { x: 0.0, y: 0.0 },
            0,
            0.0,
            0,
            null_mut(),
            0,
            0,
            0,
        );
        let post = send!(extern "C" fn(Id, Id, Id, bool));
        post(app, sel(c"postEvent:atStart:"), event, true);
    }
}

/// Where the plugins' main thread is, for messages.
const WHERE: &str = if APP_ON_PROCESS_MAIN { "AppKit's main thread" } else { "the app's loop thread" };

/// A song with the probe on a notes lane, keyed `p` so it loads the state `data/p/p.state` (tagged
/// `desktop`), beside a sine in `tracks/a.ts`.
fn project(dir: &Path) {
    let put = |path: &str, text: &[u8]| {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put(
        "music2.toml",
        b"[project]\nname = \"app\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n",
    );
    put(
        "song.ts",
        b"import { plugin, song, track } from '@m2/core';\nimport { sum } from 'std/math';\nimport { a } from './tracks/a';\n\
          const p = track('p', ({ lane }) => plugin('clap:m2.probe', { notes: lane.notes('n'), key: 'p' }));\n\
          export default song(() => sum(a.out, p.out));\n",
    );
    put("tracks/a.ts", A.as_bytes());
    put("data/p/n.json", br#"{"clips": [{"id": "a", "start": 0, "length": 64, "notes": [{"pitch": "C4", "start": 0, "length": 60}]}]}"#);
    put("data/p/p.state", &probe::state("desktop", 0.5));
    put("data/p/p.state.json", br#"{"plugin": "clap:m2.probe", "plugin_version": "1.0.0"}"#);
}

const A: &str = "import { hz, track } from '@m2/core';\nimport { osc, vca } from 'std/dsp';\n\
                 export const a = track('a', () => vca(osc.sine(hz(220)), 0.25));\n";

fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let t = Instant::now();
    while !ok() {
        assert!(t.elapsed() < Duration::from_secs(10), "{what}: not within 10 s");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The app's server and project work, off the main thread as in the app. Returns what it saw.
fn work(dir: PathBuf) -> String {
    probe::install();
    let n = |a: &AtomicU64| a.load(Ordering::Relaxed);
    assert!(!m2_plugins::on_main_thread(), "the worker isn't the plugins' main thread");
    // The executor is the app's main loop: a job posted there runs on the plugins' main thread,
    // which is the process's main thread exactly when the OS says so.
    let (on, process_main, main) = m2_plugins::main_thread()
        .unwrap()
        .run(|| (m2_plugins::on_main_thread(), m2_os::thread::is_process_main(), m2_os::thread::token()))
        .unwrap();
    assert!(on && process_main == APP_ON_PROCESS_MAIN, "a job ran on the plugins' main thread: {on}; on the process's: {process_main}");
    let there = |c: &probe::Call| c.thread == main && c.process_main == APP_ON_PROCESS_MAIN;

    let running = m2_server::start(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "app".into() }).unwrap();
    let server = running.state.clone();
    // The launch project, as `setup` opens it: off the main thread.
    plugin_host::open_off_main(server.clone(), dir.clone(), |_, dir, e| panic!("couldn't open {}: {e:#}", dir.display())).join().unwrap();
    server.graph.transport("play").unwrap();
    let live = probe::tagged("desktop");
    assert_eq!(live.len(), 1, "one probe instance plays the song");
    let first = live[0].clone();
    eventually("the probe plays", || n(&first.process) > 50);

    // HOST-5: an outside save that rebuilds the song carries the instance: no second one is made.
    let revision = |s: &m2_server::AppState| s.graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let before = revision(&server);
    std::fs::write(dir.join("tracks/a.ts"), format!("// a comment\n{A}")).unwrap();
    assert!(server.graph.poll());
    assert!(revision(&server) > before, "the save was taken in");
    let played = n(&first.process);
    eventually("the carried instance plays on", || n(&first.process) > played + 50);
    assert_eq!(probe::tagged("desktop").len(), 1, "carried, not created again");
    assert_eq!(first.main_calls().iter().filter(|c| c.what == "init").count(), 1, "initialised once");

    // RENDER-26b: a changed state blob loads into the instance that plays, on the plugins' main
    // thread (checked with every call below): none is made.
    let loads = |s: &probe::Stats| s.main_calls().iter().filter(|c| c.what == "load").count();
    let loaded = loads(&first);
    std::fs::write(dir.join("data/p/p.state"), probe::state("desktop-2", 0.5)).unwrap();
    assert!(server.graph.poll());
    assert_eq!((first.tag.lock().unwrap().clone(), loads(&first)), ("desktop-2".to_string(), loaded + 1), "one load, into it");
    assert!(probe::tagged("desktop").is_empty() && probe::tagged("desktop-2").len() == 1, "no instance made for the new state");
    let played = n(&first.process);
    eventually("it plays on", || n(&first.process) > played + 50);
    eventually("the plugin's requested callbacks ran", || n(&first.callbacks) > 0);

    // Quit, from a thread of its own while the app's main loop serves the plugins.
    let quit = std::thread::spawn({
        let server = server.clone();
        move || plugin_host::close_for_quit(&server)
    });
    let teardown = quit.join().unwrap();
    assert!(teardown.left == 0 && teardown.destroyed >= 1, "quit left plugin instances alive: {teardown:?}");
    assert!(first.main_calls().iter().any(|c| c.what == "destroy"), "quit destroyed the playing instance");

    // HOST-2: every main-thread call, the describes made while compiling included, ran on the
    // plugins' main thread, where the host's checks said main and not audio; `stop_processing`
    // ran there with the audio role taken. `process` ran elsewhere, with the audio role, and
    // `on_main_thread` never inside it.
    let mut all = probe::tagged("desktop");
    all.extend(probe::tagged("desktop-2"));
    all.extend(probe::tagged(""));
    let mut calls = 0;
    for s in &all {
        for c in s.main_calls() {
            calls += 1;
            if c.what == "stop_processing" {
                assert!(there(&c) && c.says_audio, "stop_processing: {c:?}");
            } else {
                assert!(there(&c) && c.says_main && !c.says_audio, "{} ran off the main thread: {c:?}", c.what);
            }
        }
        assert_eq!((n(&s.process_on_main), n(&s.process_says_main), n(&s.process_not_audio)), (0, 0, 0), "process's thread");
        assert_eq!(n(&s.callbacks_in_process), 0, "on_main_thread ran inside process");
        assert_eq!(n(&s.doubled), 0, "an instance was processed twice for one block");
    }
    let surge = with_plugins(&server, &dir);
    format!(
        "{} probe instances, {calls} main-thread calls on {WHERE}, {} callbacks; quit destroyed {} and left {}; {surge}",
        all.len(),
        n(&first.callbacks),
        teardown.destroyed,
        teardown.left
    )
}

/// With `CLAP_PATH` naming a folder with Surge XT and Airwindows (tests never read the user's own):
/// `examples/with-plugins` opened in the same server, played, rebuilt by an outside save that
/// carries both instances (HOST-5), and closed the way quit does, all while the app's main loop
/// serves the plugins.
fn with_plugins(server: &Arc<m2_server::AppState>, scratch: &Path) -> String {
    let installed = ["org.surge-synth-team.surge-xt", "com.airwindows.consolidated"].iter().all(|id| m2_plugins::find(id).is_ok());
    if std::env::var_os("CLAP_PATH").is_none() || !installed {
        return "with-plugins skipped (CLAP_PATH has no Surge XT and Airwindows)".into();
    }
    let dir = scratch.with_file_name(format!("{}-with-plugins", scratch.file_name().unwrap().to_string_lossy()));
    copy(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/with-plugins")), &dir);
    let created = || m2_plugins::InProcess::counts().0;
    plugin_host::open_off_main(server.clone(), dir.clone(), |_, dir, e| panic!("couldn't open {}: {e:#}", dir.display())).join().unwrap();
    assert!(server.graph.news()["problems"].as_array().unwrap().iter().all(|p| p["code"] != "plugin.missing"));
    server.graph.transport("play").unwrap();
    std::thread::sleep(Duration::from_millis(500));
    let before = created();
    let kick = std::fs::read_to_string(dir.join("tracks/kick.ts")).unwrap();
    std::fs::write(dir.join("tracks/kick.ts"), kick.replace("ms(150)", "ms(160)")).unwrap();
    assert!(server.graph.poll());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(created(), before, "the rebuild carried Surge XT and Airwindows");
    let t = plugin_host::close_for_quit(server);
    assert!(t.left == 0 && t.destroyed == 2, "quit left plugin instances alive: {t:?}");
    let _ = std::fs::remove_dir_all(&dir);
    format!("with-plugins played, carried across a rebuild and closed on {WHERE}")
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

fn main() {
    let dir = std::env::temp_dir().join(format!("m2-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Before any thread starts: the silent player, and a HOME of its own (never the user's).
    // SAFETY: nothing else runs yet.
    unsafe {
        std::env::set_var("M2_AUDIO", "none");
        std::env::set_var("HOME", dir.join("home"));
    }
    project(&dir.join("song"));
    // As `main` does, first, on the main thread: the plugins' main thread starts.
    assert!(m2_os::thread::is_process_main());
    plugin_host::install().expect("the app's hook installs on the main thread");
    std::thread::spawn(|| {
        std::thread::sleep(HUNG);
        eprintln!("app_main_thread: hung for {} s (the app's main loop never served the plugins?)", HUNG.as_secs());
        std::process::exit(1);
    });
    #[cfg(target_os = "macos")]
    let app = {
        let app = appkit::app();
        appkit::prohibit(app);
        app
    };
    let result = Arc::new(std::sync::Mutex::new(None));
    let worker = std::thread::spawn({
        let (result, song) = (result.clone(), dir.join("song"));
        move || {
            let r = std::panic::catch_unwind(move || work(song));
            *result.lock().unwrap() = Some(r);
            #[cfg(target_os = "macos")]
            m2_plugins::main_thread().unwrap().post(appkit::stop);
        }
    });
    // macOS: AppKit's loop, which serves the plugins. Linux: GTK would run here, but plugins never
    // use it, so the main thread only waits.
    #[cfg(target_os = "macos")]
    appkit::run(app);
    worker.join().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    match result.lock().unwrap().take() {
        Some(Ok(said)) => println!("app_main_thread: ok ({said})"),
        _ => {
            eprintln!("app_main_thread: FAILED (the panic above says why)");
            std::process::exit(1);
        }
    }
}

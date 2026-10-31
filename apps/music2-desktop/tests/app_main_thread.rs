//! HOST-3 and HOST-2 in the app, without launching it: the app's own hook ([`plugin_host`], the
//! same file `main` uses) installed on the process's main thread. Plugins run in the host
//! process, this program run again with `--m2-plugin-host`, which `main` serves first thing as the
//! app does; the probe's records stay there (its HOST-2 rule inside it is tested by
//! `core/plugins/tests/main_thread.rs`). On macOS the main thread then runs AppKit's
//! event loop, `[NSApp run]`, as Tauri does (with no window, no Dock icon and no menu bar), and the
//! plugins' main thread is AppKit's, reached through the main dispatch queue. On Linux it is the
//! app's loop thread of its own, and the main thread, which GTK would own, only waits. Meanwhile
//! the server, on other threads as in the app, opens a song with the probe plugin off the main
//! thread, plays it, rebuilds it (the instance carried, HOST-5), changes its state (loaded into
//! the same instance, RENDER-26b), and closes it the way quit does.
//!
//! The probe must be made in the host process, not here; carried across the rebuild and the state
//! load; and quit's close must leave no instance alive and end the host process. libtest owns the
//! main thread, so this test has no harness.

// What the test doesn't call (`leave_now`) is the app's.
#[allow(dead_code)]
#[path = "../src/plugin_host.rs"]
mod plugin_host;

use std::path::{Path, PathBuf};
use std::sync::Arc;
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
        b"[project]\nname = \"app\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.6\"\n",
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
    assert!(!m2_plugins::on_main_thread(), "the worker isn't the plugins' main thread");
    // The app's main loop still runs jobs: on the process's main thread exactly when the OS says so.
    let (on, process_main) =
        m2_plugins::main_thread().unwrap().run(|| (m2_plugins::on_main_thread(), m2_os::thread::is_process_main())).unwrap();
    assert!(on && process_main == APP_ON_PROCESS_MAIN, "a job ran on the app's main loop: {on}; on the process's: {process_main}");
    assert_eq!(m2_plugins::host_process(), None, "no host process before a plugin is needed");

    let running =
        m2_server::start(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "app-tools".into(), page_token: "app".into() })
            .unwrap();
    let server = running.state.clone();
    let created = || m2_plugins::host().counts().0;
    // The launch project, as `setup` opens it: off the main thread.
    plugin_host::open_off_main(server.clone(), dir.clone(), |_, dir, e| panic!("couldn't open {}: {e:#}", dir.display())).join().unwrap();
    server.graph.transport("play").unwrap();
    let host = m2_plugins::host_process().expect("the app's hook started the host process (HOST-3)");
    assert_ne!(host, std::process::id());
    assert!(m2_plugins::probe::tagged("desktop").is_empty(), "the probe runs in the host process, not here");
    let made = created();
    assert!(made >= 1 && m2_plugins::host().live() >= 1, "the probe is made there");
    eventually("the song plays", || server.graph.status().playing);

    // HOST-5: an outside save that rebuilds the song carries the instance: no second one is made.
    let revision = |s: &m2_server::AppState| s.graph.state().unwrap()["view"]["revision"].as_u64().unwrap();
    let before = revision(&server);
    std::fs::write(dir.join("tracks/a.ts"), format!("// a comment\n{A}")).unwrap();
    assert!(server.graph.poll());
    assert!(revision(&server) > before, "the save was taken in");
    // RENDER-26b: a changed state blob loads into the instance that plays: none is made.
    std::fs::write(dir.join("data/p/p.state"), probe::state("desktop-2", 0.5)).unwrap();
    assert!(server.graph.poll());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(created(), made, "carried across the rebuild and the load, not created again");
    assert!(server.graph.news()["problems"].as_array().unwrap().is_empty(), "{}", server.graph.news());

    // Quit, from a thread of its own while the app's main loop runs: the host process ends too.
    let quit = std::thread::spawn({
        let server = server.clone();
        move || plugin_host::close_for_quit(&server)
    });
    let teardown = quit.join().unwrap();
    assert!(teardown.left == 0 && teardown.destroyed >= 1, "quit left plugin instances alive: {teardown:?}");
    eventually("quit ended the host process", || m2_plugins::host_process().is_none());
    let surge = with_plugins(&server, &dir);
    format!(
        "{made} probe instances made in host process {host} from the app's hook; quit destroyed {} and left {}; {surge}",
        teardown.destroyed, teardown.left
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
    let created = || m2_plugins::host().counts().0;
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
    // Run again as the host process (HOST-3): the app's hook serves, as the app's `main` does.
    if std::env::args().nth(1).as_deref() == Some(m2_plugins::HOST_FLAG) {
        plugin_host::install().expect("serves");
    }
    let dir = std::env::temp_dir().join(format!("m2-app-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Before any thread starts: the silent player, no MIDI keyboard, and a HOME of its own (never
    // the user's).
    // SAFETY: nothing else runs yet.
    unsafe {
        std::env::set_var("M2_AUDIO", "none");
        std::env::set_var("M2_MIDI", "none");
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

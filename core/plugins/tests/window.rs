//! HOST-7 and HOST-3: a plugin's window opens on the host process's main loop. The probe's
//! `gui.set_parent` is given a window that exists and is shown, on the plugin main thread; Close
//! takes it away; the person's close (the window manager's message) does too, and its owner hears
//! `WindowClosed`. Needs a screen: on Linux, `DISPLAY` naming one (Xvfb `:96` here). Without one
//! it says why and checks nothing.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use m2_os::window::Native;
use m2_plugins::{HostEvent, MainCall, Owner, PluginHost, Remote, Reply, WindowCall, probe};

/// The window the probe was last given, and whether it was told so on the main thread, from what
/// it logged.
fn parent(host: &Remote, h: m2_plugins::Handle) -> (Native, bool) {
    let Ok(Reply::Log(lines)) = host.main_call(h, MainCall::TakeLog) else { panic!("no log") };
    let line = lines.iter().rev().find(|l| l.starts_with("probe gui: set_parent")).unwrap_or_else(|| panic!("no set_parent in {lines:?}"));
    let words: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(words[3], m2_os::window::API, "{line}");
    (Native(words[4].parse().unwrap()), words[5] == "main=true")
}

/// Wait up to 5 s for `w` to be as `want` says (None: gone).
fn until(w: Native, want: Option<bool>) -> Option<bool> {
    let t = Instant::now();
    loop {
        let now = m2_os::window::shown(w);
        if now == want || t.elapsed() > Duration::from_secs(5) {
            return now;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_plugin_window_opens_on_the_host_process_main_loop() {
    if let Err(why) = m2_os::window::available() {
        eprintln!("a_plugin_window_opens_on_the_host_process_main_loop: nothing checked, as {why} (Xvfb :96 gives one)");
        return;
    }
    probe::install();
    let host = Remote::new(PathBuf::from(env!("CARGO_BIN_EXE_m2-plugin-host")), vec![]);
    let (found, errors) = host.scan();
    let found = found.into_iter().find(|f| f.id == probe::ID).unwrap_or_else(|| panic!("no probe: {errors:?}"));
    let (owner, events) = Owner::new();
    let (h, info) = host.create(&found, None, &owner).unwrap();
    assert!(info.gui, "the probe offers a window");
    host.activate(h, 48_000, 128).unwrap();

    let open = || host.main_call(h, MainCall::Window(WindowCall::Open { title: "Probe (a test)".into() })).unwrap();
    assert!(matches!(open(), Reply::Window(true)));
    let (w, on_main) = parent(&host, h);
    assert!(on_main, "set_parent ran on the host process's plugin main thread");
    assert_eq!(until(w, Some(true)), Some(true), "the window exists and is shown");
    assert!(matches!(open(), Reply::Window(true)), "an open window stays open");

    assert!(matches!(host.main_call(h, MainCall::Window(WindowCall::Close)).unwrap(), Reply::Window(false)));
    assert_eq!(until(w, None), None, "Close takes the window away");

    assert!(matches!(open(), Reply::Window(true)));
    let (w, _) = parent(&host, h);
    assert_eq!(until(w, Some(true)), Some(true));
    m2_os::window::ask_to_close(w);
    assert_eq!(events.recv_timeout(Duration::from_secs(5)), Ok(HostEvent::WindowClosed(h)), "the person's close is heard");
    assert_eq!(until(w, None), None, "and the window is gone");
    host.destroy(h);
}

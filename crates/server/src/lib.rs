//! The app's local server. It serves the React UI to the desktop app's web view, and the
//! open song project over HTTP (`/api/graph…`) and a WebSocket of events.
//!
//! `music2-desktop` hosts it in-process. Local scripts and agents use the same API with the tools
//! key that `~/.music2/session.json` publishes; the page has a key of its own (AGENT-10).

pub mod caller;
pub mod export;
mod files;
pub mod graph;
mod peaks;
pub mod project;
pub mod recent;
pub mod resources;
mod routes;
mod routes_8b;
mod routes_audio;
mod routes_exchange;
mod routes_fx;
mod routes_leases;
mod routes_mix;
mod routes_notes;
mod routes_plugins;
mod routes_presets;
mod routes_project;
mod routes_record;
mod routes_see;
mod routes_settings;
mod routes_view;
pub mod settings;
pub mod socket;
pub mod stems;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::{broadcast, watch};

/// The largest request `POST /api/tx/commit`, `/api/tx/check` and `/api/graph/fix` take (AGENT-1):
/// a Tx carries each file it changes whole, as it was and as it will be, so this is room for both
/// at what undo keeps (64 MiB each), and the JSON around them. `m2` checks it before it sends.
pub const TX_BODY: usize = 144 << 20;

/// Startup can't produce more than a few; the newest are kept.
const MAX_UNHEARD: usize = 8;

/// The toasts no page has heard, and how many pages' sockets listen now.
#[derive(Default)]
struct Unheard {
    pages: usize,
    kept: Vec<String>,
}

/// A page's socket, while it listens: a toast sent meanwhile is its to hear, so none is kept.
pub(crate) struct PageListens(Arc<Mutex<Unheard>>);

impl Drop for PageListens {
    fn drop(&mut self) {
        self.0.lock().pages -= 1;
    }
}

pub struct AppState {
    /// One-shot events for every connected client: toasts, and `graph` when the open
    /// project changed.
    pub events: broadcast::Sender<String>,
    /// The tools key (AGENT-10): every `/api` request must carry it or the page's key
    /// (`x-m2-token` header or `?token=`); one with it is an agent's. The desktop app makes a
    /// fresh one per launch and publishes it in `~/.music2/session.json`, so other local software
    /// and web pages can't drive the app.
    pub token: String,
    /// The page's key, handed only to its window: a request with it is the musician's.
    pub page_token: String,
    /// The open song project, if any.
    pub graph: graph::Graph,
    /// Export Audio, which renders the song on a thread of its own.
    pub exports: export::Exports,
    /// Toasts sent while no page's socket was connected, such as why the folder given at launch
    /// didn't open. The next page to connect gets them: an agent's socket (`m2 watch`) hears them
    /// live but never takes one (AGENT-6).
    unheard: Arc<Mutex<Unheard>>,
    /// The latest meters message (8b.8), for every window: only the latest is kept, so a window
    /// that falls behind skips to it (empty until the first).
    pub meters: watch::Sender<String>,
    /// The recent songs list (polish C2), when this server keeps one ([`AppState::keep_recent`]):
    /// the app and the headless server do; tests don't, so none writes the real `~/.music2`.
    recent: OnceLock<recent::Recent>,
    /// This machine's settings (gaps #31), when this server keeps them ([`AppState::keep_settings`]):
    /// the app and the headless server do; tests don't.
    settings: OnceLock<settings::Settings>,
    /// Listening to MIDI keyboards (8b.10), unless `M2_MIDI` says not to ([`spawn_midi`]).
    midi: OnceLock<m2_os::midi::Listening>,
    /// The installed plugins as the app lists them (gaps #32), once a list was asked for: kept for
    /// the app run, until Look again. Held through a scan, so two asks scan once.
    plugins: Mutex<Option<serde_json::Value>>,
}

impl AppState {
    /// The app's state around `graph`, with no server: tests and the gates.
    pub fn new(token: String, graph: graph::Graph) -> AppState {
        let events = broadcast::channel(256).0;
        // Every change that is an undo step is told as it lands (AGENT-5).
        graph.tell_steps(events.clone());
        AppState {
            events,
            token,
            page_token: String::new(),
            graph,
            exports: Default::default(),
            unheard: Arc::default(),
            meters: watch::channel(String::new()).0,
            recent: OnceLock::new(),
            settings: OnceLock::new(),
            midi: OnceLock::new(),
            plugins: Mutex::default(),
        }
    }

    /// The installed plugins, as the side browser and + Track list them: the host process's scan
    /// (HOST-3), made on the first ask, or now when `again` (Look again), and kept. It waits for a
    /// scan, up to 2 minutes: call it off the async threads.
    pub fn plugins(&self, again: bool) -> serde_json::Value {
        let mut kept = self.plugins.lock();
        if again || kept.is_none() {
            *kept = Some(routes_plugins::listed(&*self.graph.host()));
        }
        kept.clone().unwrap_or_default()
    }

    /// Keep this machine's settings in `file` (`~/.music2/settings.json`) from now on, and apply
    /// them: the sound output a player opens, and where new songs go. Once. A songs folder that
    /// is no longer one (inside a song now) is the default.
    pub fn keep_settings(&self, file: PathBuf) {
        let s = settings::Settings::new(file);
        let kept = s.read();
        if let Err(e) = self.graph.set_output(kept.output.choice()) {
            eprintln!("music2: couldn't use the sound output chosen: {e:#}");
        }
        project::set_songs_dir(kept.songs.as_deref().and_then(|d| project::songs_folder(d).ok()));
        // Lane B: the MIDI keyboards turned off play nothing.
        self.graph.set_midi_off(&kept.midi.off);
        let _ = self.settings.set(s);
    }

    /// This machine's settings, if this server keeps them.
    pub fn settings(&self) -> Option<&settings::Settings> {
        self.settings.get()
    }

    /// Keep the recent songs list in `file` (`~/.music2/recent.json`) from now on. Once.
    pub fn keep_recent(&self, file: PathBuf) {
        let _ = self.recent.set(recent::Recent::new(file));
    }

    /// The recent songs list, if this server keeps one.
    pub fn recent(&self) -> Option<&recent::Recent> {
        self.recent.get()
    }

    /// Open the song in `dir` and put it first in the recent list. Every way a song opens comes
    /// here, or to [`AppState::opened`] after it (plan X29): Open, New Song, an example, Open
    /// Recent, the start screen, Save As and Rename.
    pub fn open_song(&self, dir: &Path) -> Result<serde_json::Value> {
        let state = self.graph.open(dir)?;
        self.opened();
        Ok(state)
    }

    /// The open song was just opened: it goes first in the recent list.
    pub fn opened(&self) {
        if let (Some(r), Some(dir)) = (self.recent(), self.graph.dir())
            && let Err(e) = r.opened(&dir)
        {
            eprintln!("music2: couldn't keep the recent songs list: {e:#}");
        }
    }

    /// The song was closed (Close Song): the next launch doesn't open it again.
    pub fn closed(&self) {
        if let Some(Err(e)) = self.recent().map(recent::Recent::closed) {
            eprintln!("music2: couldn't keep the recent songs list: {e:#}");
        }
    }

    pub fn toast(&self, msg: impl Into<String>) {
        self.announce(json!({"t": "toast", "text": msg.into()}).to_string());
    }

    /// Send an event every window should hear, even one that connects later: kept until a page's
    /// socket hears it. An agent's socket hears it live, but it is kept for the page all the same.
    fn announce(&self, ev: String) {
        // Held across the send so a page subscribing right now can't miss it.
        let mut unheard = self.unheard.lock();
        let _ = self.events.send(ev.clone());
        if unheard.pages == 0 {
            unheard.kept.push(ev);
            let extra = unheard.kept.len().saturating_sub(MAX_UNHEARD);
            unheard.kept.drain(..extra);
        }
    }

    /// Listen for events: a page's socket (`page`) starts with the toasts no page has heard yet,
    /// and while its [`PageListens`] lives none is kept; an agent's starts with none (AGENT-6).
    fn subscribe(&self, page: bool) -> (Vec<String>, broadcast::Receiver<String>, Option<PageListens>) {
        let mut unheard = self.unheard.lock();
        if !page {
            return (vec![], self.events.subscribe(), None);
        }
        unheard.pages += 1;
        (std::mem::take(&mut unheard.kept), self.events.subscribe(), Some(PageListens(self.unheard.clone())))
    }

    /// Tell every client to fetch the project again: it was opened, closed or edited. The
    /// event carries the revision and what needs attention in it (see [`graph::Graph::news`]),
    /// so a client that only listens hears every warning too.
    pub fn graph_changed(&self) {
        let mut event = match self.graph.news() {
            serde_json::Value::Object(news) => news,
            _ => serde_json::Map::new(),
        };
        event.insert("t".into(), json!("graph"));
        let _ = self.events.send(serde_json::Value::Object(event).to_string());
    }
}

/// MIDI keyboards (8b.10): what a listener heard goes to the open song, a keyboard that came or
/// went is said in a toast, and every window hears the `midi` event.
impl AppState {
    pub fn midi(&self, ev: m2_os::midi::Event<'_>) {
        let plugging = !matches!(ev, m2_os::midi::Event::Bytes(..));
        if let Some(said) = self.graph.midi(ev) {
            self.toast(said);
        }
        if plugging {
            self.midi_changed();
        }
    }

    /// Tell every window the keyboards plugged in and what they play: `{"t": "midi", "midi": …}`.
    pub fn midi_changed(&self) {
        let _ = self.events.send(json!({"t": "midi", "midi": self.graph.midi_state()}).to_string());
    }
}

pub struct ServeOptions {
    /// 0 picks a free port.
    pub port: u16,
    /// Built frontend (`web/dist`). Missing → API only (use the Vite dev server).
    pub web_dir: PathBuf,
    /// The tools key: what agents send (see [`AppState::token`]).
    pub token: String,
    /// The page's key, handed only to its window (AGENT-10).
    pub page_token: String,
}

/// A running server.
pub struct Running {
    pub url: String,
    pub state: Arc<AppState>,
}

/// Start the HTTP server on a background thread and return at once. No project is open
/// yet, and nothing opens the audio device until one is.
pub fn start(opts: ServeOptions) -> Result<Running> {
    start_with(opts, graph::Graph::default())
}

/// [`start`] serving `graph`, such as one whose player plays into a file for a test
/// ([`graph::Graph::with_player`]).
pub fn start_with(opts: ServeOptions, graph: graph::Graph) -> Result<Running> {
    resources::keep_hashes();
    let mut state = AppState::new(opts.token, graph);
    state.page_token = opts.page_token;
    let state = Arc::new(state);
    spawn_graph_watch(&state);
    spawn_meters(&state);
    spawn_midi(&state);
    spawn_takes(&state);

    let listener = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], opts.port)))?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}", listener.local_addr()?);
    let app = routes::router(state.clone(), &opts.web_dir);
    std::thread::Builder::new().name("music2-http".into()).spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("listener");
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("music2: server stopped: {e}");
            }
        });
    })?;
    Ok(Running { url, state })
}

/// How often the server ends idle gestures and says what the user must hear (see
/// [`spawn_graph_watch`]).
const TELL_EVERY: Duration = Duration::from_millis(300);

/// Take in edits to the open project made outside the app once they settle, and tell clients, on
/// two threads, so what the user must hear never waits for the session's actor:
/// - `music2-graph` ticks the watcher every 50 ms, sooner when an event names a change while none
///   is settling or a batch's quiet ends (WRITE-19), and every 300 ms brings an engine that fell
///   behind the accepted revision up to it. Taking in a batch waits for the actor, which may be
///   busy for seconds: a compile, or a commit the disk is slow to make durable.
/// - `music2-tell` every 300 ms ends the knob drags nobody moves any more (C6) and says what
///   ended or was saved on its own, and a commit heard as its preview that isn't saved yet
///   (PLANE-5); and lets go of the notes of a window that went quiet (MON-7). Every 2 s, while
///   no sound output plays, it looks for one (gaps #13). It never waits for the actor.
///
/// Both end once the state is dropped.
fn spawn_graph_watch(state: &Arc<AppState>) {
    let weak = Arc::downgrade(state);
    std::thread::Builder::new()
        .name("music2-graph".into())
        .spawn(move || {
            let mut last = std::time::Instant::now();
            while let Some(state) = weak.upgrade() {
                state.graph.watch_wait(Duration::from_millis(50));
                let mut changed = state.graph.watch();
                if last.elapsed() >= Duration::from_millis(300) {
                    last = std::time::Instant::now();
                    changed |= state.graph.catch_up();
                    changed |= state.graph.plugins_changed();
                }
                if changed {
                    state.graph_changed();
                }
            }
        })
        .expect("spawn graph watcher");
    let weak = Arc::downgrade(state);
    std::thread::Builder::new()
        .name("music2-tell".into())
        .spawn(move || {
            let mut looked = std::time::Instant::now();
            loop {
                std::thread::sleep(TELL_EVERY);
                let Some(state) = weak.upgrade() else { return };
                // While no sound output plays, look for one (gaps #13).
                if looked.elapsed() >= graph::output::LOOK_EVERY {
                    looked = std::time::Instant::now();
                    state.graph.look_for_output();
                }
                let said = state.graph.expire();
                state.graph.save_overlays();
                state.graph.expire_auditions();
                // Faults found while sound is made: each a problem until its part plays again.
                let (faults, listed) = state.graph.hear_faults();
                for s in said.iter().chain(&faults) {
                    state.toast(s.clone());
                }
                // What ended changes the gestures clients show.
                if !said.is_empty() || listed {
                    state.graph_changed();
                }
            }
        })
        .expect("spawn the server's notices");
}

/// How often the meters are read and sent: about 30 times a second.
const METERS_EVERY: Duration = Duration::from_millis(33);

/// `music2-meters`: every 33 ms, the meters of what plays go to every window (8b.8,
/// [`graph::Graph::meters`]), on their own channel, never through the session. The load is read
/// even with no window, so the status's `load` stays current. Ends once the state is dropped.
fn spawn_meters(state: &Arc<AppState>) {
    let weak = Arc::downgrade(state);
    std::thread::Builder::new()
        .name("music2-meters".into())
        .spawn(move || {
            loop {
                std::thread::sleep(METERS_EVERY);
                let Some(state) = weak.upgrade() else { return };
                if let Some(m) = state.graph.meters()
                    && state.meters.receiver_count() > 0
                {
                    state.meters.send_replace(m.to_string());
                }
            }
        })
        .expect("spawn the meters");
}

/// How often a take is looked at (lane B): its count, its loop's end, and a take to write.
const TAKES_EVERY: Duration = Duration::from_millis(100);

/// `music2-takes` (lane B): every 100 ms, what a take does that no note or key says (a count that
/// turned into playing, a loop's end, a player gone), and each take that ended, written; every
/// window hears it (`{"t": "take", …}`). Writing waits for the session, on this thread only: the
/// transport never does (D7). Ends once the state is dropped.
fn spawn_takes(state: &Arc<AppState>) {
    let weak = Arc::downgrade(state);
    std::thread::Builder::new()
        .name("music2-takes".into())
        .spawn(move || {
            loop {
                std::thread::sleep(TAKES_EVERY);
                let Some(state) = weak.upgrade() else { return };
                for said in state.graph.record_tick() {
                    let written = said.get("written").is_some();
                    let _ = state.events.send(said.to_string());
                    if written {
                        state.graph_changed();
                    }
                }
            }
        })
        .expect("spawn the takes");
}

/// Listen to every MIDI keyboard (`m2_os::midi`), unless [`listens_to_midi`] says not to. Stops
/// once the state is dropped.
fn spawn_midi(state: &Arc<AppState>) {
    if !listens_to_midi(std::env::var("M2_MIDI").ok().as_deref()) {
        return;
    }
    let weak = Arc::downgrade(state);
    let listening = m2_os::midi::listen(Arc::new(move |ev| {
        if let Some(state) = weak.upgrade() {
            state.midi(ev);
        }
    }));
    let _ = state.midi.set(listening);
}

/// Whether a server listens to MIDI keyboards, as `M2_MIDI` (`midi`) says: not with `none` (a
/// test or a check that must never open MIDI) or `inject` (the suites: they post what a keyboard
/// would send to `POST /api/graph/midi/inject` instead), and never in this crate's own tests,
/// which start servers of their own (R3).
fn listens_to_midi(midi: Option<&str>) -> bool {
    !cfg!(test) && !matches!(midi, Some("none" | "inject"))
}

/// Whether `POST /api/graph/midi/inject` answers: only with `M2_MIDI=inject`.
pub(crate) fn midi_injects() -> bool {
    std::env::var("M2_MIDI").as_deref() == Ok("inject")
}

/// Whether `POST /api/graph/input/inject` answers: only with `M2_AUDIO_INPUT=inject`, and in this
/// crate's own tests (the io plan's D24).
pub(crate) fn input_injects() -> bool {
    cfg!(test) || std::env::var("M2_AUDIO_INPUT").as_deref() == Ok("inject")
}

#[cfg(test)]
mod tests {
    use super::*;

    // gaps #13, R6: with no sound device, one plugged in is used within a few seconds with no
    // action at all: the server looks for it every 2 s, and its catch-up moves the song there,
    // stopped, and says so. (The popover says just this.)
    #[test]
    fn a_device_plugged_in_is_used_with_no_action() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let there = Arc::new(AtomicBool::new(false));
        let plugged = there.clone();
        let listed = there.clone();
        let graph = graph::Graph::starting(
            Box::new(move |_| match plugged.load(Ordering::Relaxed) {
                true => m2_play::Player::silent(),
                false => Err(anyhow::anyhow!("no audio output device")),
            }),
            Box::new(move || listed.load(Ordering::Relaxed).then(|| "Headphones".to_string())),
        );
        let dir = std::env::temp_dir().join(format!("m2-plug-{}", std::process::id()));
        let song = dir.join("song");
        std::fs::create_dir_all(&song).unwrap();
        std::fs::write(
            song.join("music2.toml"),
            "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.4\"\n",
        )
        .unwrap();
        std::fs::write(
            song.join("song.ts"),
            "import { hz, song } from '@m2/core';\nimport { osc } from 'std/dsp';\nexport default song(() => osc.sine(hz(800)));\n",
        )
        .unwrap();
        let running =
            start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t-tools".into(), page_token: "t".into() }, graph)
                .unwrap();
        let state = running.state.clone();
        state.graph.open(&song).unwrap();
        assert!(state.graph.no_output());
        let mut news = state.events.subscribe();
        there.store(true, Ordering::Relaxed);
        let t = std::time::Instant::now();
        while state.graph.no_output() {
            assert!(t.elapsed() < Duration::from_secs(6), "not used within 6 s");
            std::thread::sleep(Duration::from_millis(20));
        }
        let took = t.elapsed();
        assert!(took < Duration::from_secs(4), "used in {took:?}: within a few seconds");
        eprintln!("a device plugged in was used in {took:?}");
        // The window is told, as a toast.
        let mut told = false;
        while let Ok(m) = news.try_recv() {
            told |= m.contains("Sound output is back");
        }
        let t = std::time::Instant::now();
        while !told && t.elapsed() < Duration::from_secs(2) {
            match news.try_recv() {
                Ok(m) => told |= m.contains("Sound output is back"),
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        assert!(told, "the window is told");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // R3: a server this crate's tests start never opens a MIDI device, whatever `M2_MIDI` says,
    // so a keyboard plugged in never plays into a test.
    #[test]
    fn a_test_server_never_listens_to_midi() {
        for midi in [None, Some(""), Some("none"), Some("inject")] {
            assert!(!listens_to_midi(midi), "M2_MIDI {midi:?}");
        }
    }

    fn state() -> AppState {
        AppState::new(String::new(), graph::Graph::default())
    }

    /// The desktop app reports a bad project folder before its window connects.
    #[test]
    fn a_toast_no_client_heard_goes_to_the_next_one() {
        let s = state();
        s.toast("Couldn't open my-song");
        let (unheard, mut events, _page) = s.subscribe(true);
        assert_eq!(unheard.len(), 1);
        assert!(unheard[0].contains("Couldn't open my-song"));
        // Heard once is enough: the next client doesn't get it again.
        assert!(s.subscribe(true).0.is_empty());
        // While a client listens, toasts go out live and aren't kept.
        s.toast("live");
        assert!(events.try_recv().unwrap().contains("live"));
        drop(events);
        assert!(s.subscribe(true).0.is_empty());
    }

    /// AGENT-6: `m2 watch`'s socket (the tools key) hears a toast live but never takes it from the
    /// page: one sent while no page listens waits for the page's socket.
    #[test]
    fn a_toast_a_tools_socket_heard_still_waits_for_the_page() {
        let s = state();
        let (unheard, mut tools, _) = s.subscribe(false);
        assert!(unheard.is_empty());
        s.toast("Couldn't open my-song");
        assert!(tools.try_recv().unwrap().contains("Couldn't open my-song"), "the agent hears it live");
        let (unheard, _, page) = s.subscribe(true);
        assert_eq!(unheard.len(), 1, "it still waits for the page");
        assert!(unheard[0].contains("Couldn't open my-song"));
        // While the page listens, nothing is kept; once it went, a toast waits for the next page,
        // and a tools socket that connects meanwhile takes none.
        s.toast("live");
        drop(page);
        s.toast("again");
        assert!(s.subscribe(false).0.is_empty(), "a tools socket takes no kept toast");
        let (unheard, _, _page) = s.subscribe(true);
        assert_eq!(unheard.len(), 1, "{unheard:?}");
        assert!(unheard[0].contains("again"));
    }

    /// Every change is announced with what needs attention in the revision it leaves.
    #[test]
    fn a_graph_event_carries_the_revision_and_its_problems() {
        let dir = std::env::temp_dir().join(format!("m2-server-news-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/kick")).unwrap();
        let song = "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                    const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n";
        let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.4\"\n";
        std::fs::write(dir.join("music2.toml"), manifest).unwrap();
        std::fs::write(dir.join("song.ts"), song).unwrap();
        std::fs::write(dir.join("data/kick/old.json"), r#"{"clips": []}"#).unwrap();
        let s = AppState::new(String::new(), graph::Graph::silent());
        let (_, mut events, _page) = s.subscribe(true);
        s.graph_changed();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&events.try_recv().unwrap()).unwrap(), json!({"t": "graph"}));
        s.graph.open(&dir).unwrap();
        s.graph_changed();
        let event: serde_json::Value = serde_json::from_str(&events.try_recv().unwrap()).unwrap();
        assert_eq!((&event["t"], &event["revision"], &event["diagnostics"]), (&json!("graph"), &json!(1), &json!([])));
        // DIAG-4: the revision's problems, each with its severity, code and file.
        let problem = &event["problems"][0];
        assert_eq!(
            (&problem["severity"], &problem["code"], &problem["src"]),
            (&json!("warning"), &json!("data.unclaimed"), &json!("data/kick/old.json"))
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn only_the_newest_unheard_toasts_are_kept() {
        let s = state();
        for i in 0..20 {
            s.toast(format!("toast {i}"));
        }
        let (unheard, _, _) = s.subscribe(true);
        assert_eq!(unheard.len(), MAX_UNHEARD);
        assert!(unheard.last().unwrap().contains("toast 19"));
    }
}

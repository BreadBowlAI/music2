//! The app's local server. It serves the React UI to the desktop app's web view, and the
//! open song project over HTTP (`/api/graph…`) and a WebSocket of events.
//!
//! `music2-desktop` hosts it in-process. Local scripts use the same API with the session token.

pub mod export;
mod files;
pub mod graph;
pub mod project;
pub mod resources;
mod routes;
mod routes_8b;
mod routes_view;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::{broadcast, watch};

/// Startup can't produce more than a few; the newest are kept.
const MAX_UNHEARD: usize = 8;

pub struct AppState {
    /// One-shot events for every connected client: toasts, and `graph` when the open
    /// project changed.
    pub events: broadcast::Sender<String>,
    /// Every `/api` request must carry it (`x-m2-token` header or `?token=`).
    /// The desktop app sets a fresh one per launch so other local software and web pages
    /// can't drive the app.
    pub token: String,
    /// The open song project, if any.
    pub graph: graph::Graph,
    /// Export Audio, which renders the song on a thread of its own.
    pub exports: export::Exports,
    /// Toasts sent while no window was connected, such as why the folder given at launch
    /// didn't open. The next client to connect gets them.
    unheard: Mutex<Vec<String>>,
    /// The latest meters message (8b.8), for every window: only the latest is kept, so a window
    /// that falls behind skips to it (empty until the first).
    pub meters: watch::Sender<String>,
}

impl AppState {
    /// The app's state around `graph`, with no server: tests and the gates.
    pub fn new(token: String, graph: graph::Graph) -> AppState {
        AppState {
            events: broadcast::channel(256).0,
            token,
            graph,
            exports: Default::default(),
            unheard: Mutex::default(),
            meters: watch::channel(String::new()).0,
        }
    }

    pub fn toast(&self, msg: impl Into<String>) {
        self.announce(json!({"t": "toast", "text": msg.into()}).to_string());
    }

    /// Send an event every client should hear, even one that connects later.
    fn announce(&self, ev: String) {
        // Held across the send so a client subscribing right now can't miss it.
        let mut unheard = self.unheard.lock();
        if self.events.send(ev.clone()).is_err() {
            unheard.push(ev);
            let extra = unheard.len().saturating_sub(MAX_UNHEARD);
            unheard.drain(..extra);
        }
    }

    /// Listen for events, starting with the toasts no client has heard yet.
    fn subscribe(&self) -> (Vec<String>, broadcast::Receiver<String>) {
        let mut unheard = self.unheard.lock();
        (std::mem::take(&mut *unheard), self.events.subscribe())
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

pub struct ServeOptions {
    /// 0 picks a free port.
    pub port: u16,
    /// Built frontend (`web/dist`). Missing → API only (use the Vite dev server).
    pub web_dir: PathBuf,
    /// Require this token on every API call (see [`AppState::token`]).
    pub token: String,
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
    let state = Arc::new(AppState::new(opts.token, graph));
    spawn_graph_watch(&state);
    spawn_meters(&state);

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
///   (PLANE-5); and lets go of the notes of a window that went quiet (MON-7). It never waits for
///   the actor.
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
            loop {
                std::thread::sleep(TELL_EVERY);
                let Some(state) = weak.upgrade() else { return };
                let said = state.graph.expire();
                state.graph.save_overlays();
                state.graph.expire_auditions();
                for s in &said {
                    state.toast(s.clone());
                }
                // What ended changes the gestures clients show.
                if !said.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::new(String::new(), graph::Graph::default())
    }

    /// The desktop app reports a bad project folder before its window connects.
    #[test]
    fn a_toast_no_client_heard_goes_to_the_next_one() {
        let s = state();
        s.toast("Couldn't open my-song");
        let (unheard, mut events) = s.subscribe();
        assert_eq!(unheard.len(), 1);
        assert!(unheard[0].contains("Couldn't open my-song"));
        // Heard once is enough: the next client doesn't get it again.
        assert!(s.subscribe().0.is_empty());
        // While a client listens, toasts go out live and aren't kept.
        s.toast("live");
        assert!(events.try_recv().unwrap().contains("live"));
        drop(events);
        assert!(s.subscribe().0.is_empty());
    }

    /// Every change is announced with what needs attention in the revision it leaves.
    #[test]
    fn a_graph_event_carries_the_revision_and_its_problems() {
        let dir = std::env::temp_dir().join(format!("m2-server-news-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/kick")).unwrap();
        let song = "import { song, track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
                    const k = track('kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n";
        let manifest = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.2\"\n";
        std::fs::write(dir.join("music2.toml"), manifest).unwrap();
        std::fs::write(dir.join("song.ts"), song).unwrap();
        std::fs::write(dir.join("data/kick/old.json"), r#"{"clips": []}"#).unwrap();
        let s = AppState::new(String::new(), graph::Graph::silent());
        let (_, mut events) = s.subscribe();
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
        let (unheard, _) = s.subscribe();
        assert_eq!(unheard.len(), MAX_UNHEARD);
        assert!(unheard.last().unwrap().contains("toast 19"));
    }
}

//! L5 — the UI adapter. Serves the React UI to the desktop app's web view and exposes
//! the engine over HTTP (commands, files, transport) and a WebSocket (meters, playhead,
//! song updates). Audio stays native: the UI is a remote control for the Rust engine.
//!
//! `music2-desktop` hosts it in-process, and also calls the file operations on
//! [`AppState`] directly. Local scripts reach the same API with the session token.

pub mod graph;
mod patch;
mod routes;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use m2_audio_io::{AudioHandle, Msg};
use m2_engine::Session;
use m2_project::Song;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::broadcast;

pub struct AppState {
    pub session: Arc<Mutex<Session>>,
    pub audio: Option<AudioHandle>,
    pub audio_error: Option<String>,
    pub looping: AtomicBool,
    /// Bumped on changes that aren't song revisions (e.g. the loop switch).
    pub ui_rev: AtomicU64,
    /// One-shot events for every connected client (toasts).
    pub events: broadcast::Sender<String>,
    /// Every `/api` request must carry it (`x-m2-token` header or `?token=`).
    /// The desktop app sets a fresh one per launch so other local software and web pages
    /// can't drive the engine.
    pub token: String,
    /// A song-as-program project (the new core), shown as a graph.
    pub graph: graph::Graph,
}

impl AppState {
    pub fn send(&self, msg: Msg) {
        if let Some(a) = &self.audio {
            a.send(msg);
        }
    }

    pub fn toast(&self, msg: impl Into<String>) {
        let _ = self.events.send(json!({"t": "toast", "text": msg.into()}).to_string());
    }

    pub fn bump(&self) {
        self.ui_rev.fetch_add(1, Ordering::Relaxed);
    }

    pub fn sync_loop(&self) {
        let end = self.session.lock().song().end_tick();
        self.send(Msg::Loop(self.looping.load(Ordering::Relaxed).then_some((0, end))));
    }

    pub fn is_dirty(&self) -> bool {
        self.session.lock().is_dirty()
    }

    /// Bar (1-based, fractional) under the playhead.
    pub fn playhead_bar(&self) -> f64 {
        let tick = self.audio.as_ref().map(|a| a.playhead()).unwrap_or(0).max(0);
        tick as f64 / self.session.lock().song().time_sig.ticks_per_bar() as f64 + 1.0
    }

    // ------------------------------------------------------------ files

    pub fn new_song(&self) -> Result<()> {
        self.send(Msg::Stop);
        self.send(Msg::Seek(0));
        self.session.lock().new_song(Song::default());
        self.sync_loop();
        Ok(())
    }

    pub fn open_song(&self, path: &Path) -> Result<()> {
        self.send(Msg::Stop);
        self.session.lock().open(path)?;
        self.send(Msg::Seek(0));
        self.sync_loop();
        Ok(())
    }

    /// Save to `path`, or to the song's own file, or (first save / `copy_title`) to a
    /// fresh file in the library. Saving to a new path names the song after the file.
    pub fn save(&self, path: Option<&Path>, copy_title: Option<&str>) -> Result<PathBuf> {
        let mut sess = self.session.lock();
        if let Some(t) = copy_title {
            sess.execute("song.set", json!({"title": t}))?;
        }
        let path = match (path, sess.path(), copy_title) {
            (Some(p), _, _) => {
                let stem = p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
                if !stem.is_empty() && stem != sess.song().title {
                    sess.execute("song.set", json!({"title": stem}))?;
                }
                p.to_path_buf()
            }
            (None, Some(p), None) => p.to_path_buf(),
            _ => {
                let dir = library_dir();
                std::fs::create_dir_all(&dir)?;
                let base = slug(&sess.song().title);
                let mut p = dir.join(format!("{base}.m2song"));
                let mut n = 2;
                while p.exists() {
                    p = dir.join(format!("{base} {n}.m2song"));
                    n += 1;
                }
                p
            }
        };
        sess.save(&path)?;
        Ok(path)
    }

    /// Render the song to a WAV (blocking; seconds of work for a long song).
    pub fn export_wav(&self, path: Option<PathBuf>) -> Result<PathBuf> {
        let (song, reg, bank) = {
            let sess = self.session.lock();
            (sess.song().clone(), sess.registry.clone(), sess.bank().clone())
        };
        let path = match path {
            Some(p) => p,
            None => {
                let dir = library_dir().join("exports");
                std::fs::create_dir_all(&dir)?;
                dir.join(format!("{}.wav", slug(&song.title)))
            }
        };
        let frames = m2_render::render_song(song, &reg, &bank, 48_000, 2.0);
        m2_render::write_wav(&path, &frames, 48_000)?;
        Ok(path)
    }

    /// Add a sound file as an audio clip at `bar` (or as a sampler instrument).
    pub fn import_audio(&self, name: &str, bytes: &[u8], as_instrument: bool, bar: f64) -> Result<Value> {
        let mut sess = self.session.lock();
        let stem = Path::new(name).file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Sound".into());
        let asset = sess.import_wav_bytes(&stem, bytes, None)?;
        if as_instrument {
            sess.execute("sampler.load", json!({"asset": asset}))
        } else {
            sess.execute("audio.place", json!({"asset": asset, "start_bar": bar.max(1.0)}))
        }
    }

    pub fn import_audio_file(&self, path: &Path, as_instrument: bool, bar: f64) -> Result<Value> {
        let bytes = std::fs::read(path)?;
        self.import_audio(&path.display().to_string(), &bytes, as_instrument, bar)
    }
}

/// Show a file in Finder / Explorer (selected), or open a folder.
pub fn reveal(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    if path.is_file() {
        std::process::Command::new("open").arg("-R").arg(path).spawn()?;
        return Ok(());
    }
    let target = if path.is_file() { path.parent().map(Path::to_path_buf).unwrap_or(path.to_path_buf()) } else { path.to_path_buf() };
    open::that(target)?;
    Ok(())
}

fn slug(s: &str) -> String {
    let out: String = s.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    let out = out.trim().to_string();
    if out.is_empty() { "Untitled".into() } else { out }
}

/// Where songs live by default.
pub fn library_dir() -> PathBuf {
    dirs::audio_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Music")).join("music2")
}

pub struct ServeOptions {
    /// 0 picks a free port.
    pub port: u16,
    /// Built frontend (`web/dist`). Missing → API only (use the Vite dev server).
    pub web_dir: PathBuf,
    pub song: Option<PathBuf>,
    /// Require this token on every API call (see [`AppState::token`]).
    pub token: String,
}

/// A running engine + HTTP server.
pub struct Running {
    pub url: String,
    pub state: Arc<AppState>,
}

/// Start the engine, audio and HTTP server on background threads and return at once.
pub fn start(opts: ServeOptions) -> Result<Running> {
    let (reg, plugin_errors) = Session::default_registry();
    let mut session = Session::new(Arc::new(reg));
    if let Some(p) = &opts.song {
        session.open(p)?;
    }
    let (audio, audio_error) = match AudioHandle::spawn() {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(format!("No audio output: {e:#}"))),
    };
    let (events, _) = broadcast::channel(256);
    let state = Arc::new(AppState {
        session: Arc::new(Mutex::new(session)),
        audio,
        audio_error,
        looping: AtomicBool::new(true),
        ui_rev: AtomicU64::new(1),
        events,
        token: opts.token,
        graph: graph::Graph::default(),
    });
    for e in plugin_errors {
        state.toast(e);
    }
    spawn_audio_sync(state.clone());
    spawn_graph_watch(state.clone());

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

/// Pick up edits to the open graph project made outside the app, and tell clients.
fn spawn_graph_watch(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("music2-graph".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(300));
                if state.graph.poll() {
                    let _ = state.events.send(json!({"t": "graph"}).to_string());
                }
            }
        })
        .expect("spawn graph watcher");
}

/// Keep the audio thread's renderer in step with the session, and
/// forward engine notices and renderer warnings as toasts.
fn spawn_audio_sync(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("music2-sync".into())
        .spawn(move || {
            let mut synced = 0u64;
            let mut warned: HashSet<String> = HashSet::new();
            loop {
                std::thread::sleep(Duration::from_millis(8));
                let notices = state.session.lock().take_notices();
                for n in notices {
                    state.toast(n);
                }
                let Some(audio) = &state.audio else { continue };
                let s = state.session.lock();
                if s.revision() == synced {
                    continue;
                }
                synced = s.revision();
                let song = s.song().clone();
                let renderer = m2_render::Renderer::new(song.clone(), &s.registry, s.bank(), audio.sample_rate as f32);
                drop(s);
                for w in &renderer.warnings {
                    if warned.insert(w.clone()) {
                        state.toast(w.clone());
                    }
                }
                audio.send(Msg::Renderer(Box::new(renderer)));
                if state.looping.load(Ordering::Relaxed) {
                    audio.send(Msg::Loop(Some((0, song.end_tick()))));
                }
            }
        })
        .expect("spawn sync thread");
}

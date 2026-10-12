//! The new core in the app: a song-as-program project, shown as a graph.
//!
//! One editing session ([`m2_session::Session`]) and one player ([`m2_play::Player`]).
//! Knob drags preview straight into the playing renderer; releasing commits the value
//! to code through the session. A value commit doesn't rebuild the renderer: the
//! committed graph equals the playing one with that value, which the preview already
//! set. Edits made elsewhere (a text editor, an agent) are picked up by [`Graph::poll`],
//! which rebuilds the renderer at the current position when the accepted revision
//! changes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow};
use m2_ir::Origin;
use m2_play::Player;
use m2_runtime::{PackageSet, Renderer};
use m2_session::{EditError, Edited, NoteClip, Session};
use m2_song::TempoMap;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};

use crate::files;

/// How far past the song's last clip, or past where playback is, a seek may go: it
/// replays the song from the start while holding the project.
const SEEK_ROOM: f64 = 1024.0;

struct Open {
    token: u64,
    session: Session,
    /// The accepted revision the player is playing.
    playing: u64,
    previews: Vec<Preview>,
    /// The playing revision's tempo map, shared with [`Graph::status`].
    tempo: Arc<Mutex<Option<TempoMap>>>,
}

struct Preview {
    /// The control's id as the gesture named it when it started.
    id: String,
    gesture: String,
    /// Baselines belong to the renderer revision, never a newly adopted source.
    revision: u64,
    origin: Origin,
    baseline: f64,
}

/// Whether the player is playing, and where: in seconds, and in beats of the accepted
/// revision's tempo map (0 with no project open).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Status {
    pub playing: bool,
    pub seconds: f64,
    pub beat: f64,
}

#[derive(Default)]
pub struct Graph {
    open: Mutex<Option<Open>>,
    player: Mutex<Option<Player>>,
    /// Kept apart from `open`, which seeks and edits hold while they replay the song,
    /// so status (the playhead) never waits for them. None with no project open.
    tempo: Arc<Mutex<Option<TempoMap>>>,
    next_session: AtomicU64,
    /// The code editor's files with edits not yet saved, as it last said, and the project
    /// (its token) it said so for: the app asks before closing or replacing that project.
    unsaved: Mutex<(u64, Vec<String>)>,
}

/// The std package: `$M2_STD`, or the one in this source tree.
fn std_dir() -> PathBuf {
    std::env::var_os("M2_STD").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../std")))
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&std_dir()).context("loading the std package")?;
    Ok(p)
}

/// A renderer for the accepted revision, positioned at `frame`. It replays in live mode,
/// as the player plays: a NaN on the way is silenced, not an error.
fn renderer(session: &Session, sample_rate: u32, frame: u64) -> Result<Renderer> {
    let rev = session.accepted();
    let mut r = Renderer::for_project(&rev.compiled.flat, &rev.project, &session.packages, sample_rate)?;
    r.set_strict(false);
    r.seek(frame)?;
    Ok(r)
}

impl Open {
    fn check_token(&self, token: u64) -> Result<()> {
        if token != self.token {
            return Err(anyhow!("the open project changed; refresh the graph before editing"));
        }
        Ok(())
    }

    fn restore_previews(&mut self, player: &Player) -> Result<()> {
        for preview in self.previews.drain(..) {
            if preview.revision == self.playing {
                player.commit_const(preview.origin, preview.baseline)?;
            }
        }
        Ok(())
    }

    /// Gestures name controls by the revision they started in, so ownership is
    /// decided by the value in the playing graph, not by the name.
    fn check_owner(&self, origin: &Origin, gesture: &str) -> Result<()> {
        let accepted = self.session.accepted().number;
        if self.previews.iter().any(|p| p.revision == accepted && p.origin.same_value(origin) && p.gesture != gesture) {
            return Err(anyhow!("this control is being edited by another gesture"));
        }
        Ok(())
    }

    fn cancel(&mut self, player: &Player, id: &str, gesture: &str) -> Result<()> {
        if let Some(index) = self.previews.iter().position(|p| p.id == id && p.gesture == gesture) {
            let preview = self.previews.remove(index);
            if preview.revision == self.playing {
                player.commit_const(preview.origin, preview.baseline)?;
            }
        }
        Ok(())
    }

    /// Play `r`, a renderer for the accepted revision, playing or stopped as before.
    /// Previews belonged to the old one.
    fn play(&mut self, player: &Player, r: Renderer) {
        let rev = self.session.accepted();
        // Status reads the position under this lock too, so it never pairs one renderer's
        // position with the other's tempo.
        let mut tempo = self.tempo.lock();
        player.load(r);
        *tempo = Some(rev.tempo());
        self.playing = rev.number;
        self.previews.clear();
    }

    /// Source refresh can happen inside an edit, independently of the file watcher.
    fn sync(&mut self, player: &Player) -> Result<bool> {
        if self.session.accepted().number == self.playing {
            return Ok(false);
        }
        let r = renderer(&self.session, player.sample_rate, player.position())?;
        self.play(player, r);
        Ok(true)
    }

    fn restore(&mut self, player: &Player) -> Result<()> {
        self.restore_previews(player)?;
        self.sync(player)?;
        Ok(())
    }

    fn reject(&mut self, player: &Player, id: &str, gesture: &str) -> Result<()> {
        if self.playing != self.session.accepted().number || !self.session.draft_errors.is_empty() {
            self.restore(player)
        } else {
            self.cancel(player, id, gesture)
        }
    }
}

impl Graph {
    fn player(&self) -> Result<Player> {
        let mut p = self.player.lock();
        if p.is_none() {
            *p = Some(Player::start()?);
        }
        Ok(p.clone().expect("started"))
    }

    pub fn open(&self, dir: &Path) -> Result<Value> {
        let session =
            Session::open(dir, packages()?).map_err(|d| anyhow!(d.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")))?;
        let mut open = self.open.lock();
        let player = self.player()?;
        let new_renderer = renderer(&session, player.sample_rate, 0)?;
        let token = self.next_session.fetch_add(1, Ordering::Relaxed) + 1;
        let mut o = Open { token, session, playing: 0, previews: vec![], tempo: self.tempo.clone() };
        player.stop();
        o.play(&player, new_renderer);
        *open = Some(o);
        drop(open);
        self.state().context("no project")
    }

    pub fn close(&self) {
        let mut open = self.open.lock();
        if let Some(p) = self.player.lock().as_ref() {
            p.stop();
        }
        *open = None;
        *self.tempo.lock() = None;
    }

    /// The view, the timeline, the project's name and folder, and the transport.
    pub fn state(&self) -> Option<Value> {
        let open = self.open.lock();
        let o = open.as_ref()?;
        let rev = o.session.accepted();
        let status = self.status();
        Some(json!({
            "session": o.token,
            "name": rev.project.manifest.project.name,
            "dir": rev.project.dir,
            "view": o.session.view(),
            "timeline": o.session.timeline(),
            "can_undo": o.session.can_undo(),
            "can_redo": o.session.can_redo(),
            "effects": o.session.effects(),
            "diagnostics": files::diagnostics(&o.session),
            "playing": status.playing,
            "seconds": status.seconds,
            "beat": status.beat,
        }))
    }

    /// Where playback is. It doesn't wait for the project, which seeks and edits hold
    /// while they replay the song: meanwhile the old renderer plays on, and so does this.
    pub fn status(&self) -> Status {
        let Some(p) = self.player.lock().clone() else { return Status::default() };
        let tempo = self.tempo.lock();
        let seconds = p.position() as f64 / p.sample_rate as f64;
        Status { playing: p.is_playing(), seconds, beat: tempo.as_ref().map_or(0.0, |t| t.beat_at(seconds)) }
    }

    /// Move playback to `beat` (at least 0) of the accepted revision, playing or not.
    /// The renderer is rebuilt there, so previews of the old one close.
    pub fn seek(&self, beat: f64) -> Result<()> {
        if !beat.is_finite() {
            return Err(anyhow!("can't seek to beat {beat}"));
        }
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        let player = self.player()?;
        let rev = o.session.accepted();
        let tempo = rev.tempo();
        let now = tempo.beat_at(player.position() as f64 / player.sample_rate as f64);
        let limit = rev.project.timeline.end().max(now) + SEEK_ROOM;
        if beat > limit {
            return Err(anyhow!("can't seek to beat {beat}: seeking replays the song from its start, up to beat {limit:.0} at most"));
        }
        let frame = (tempo.seconds_at(beat.max(0.0)) * player.sample_rate as f64).round() as u64;
        let r = renderer(&o.session, player.sample_rate, frame)?;
        o.play(&player, r);
        Ok(())
    }

    /// Change a value in the playing audio only (while a knob is dragged).
    pub fn preview(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64) -> Result<()> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let player = self.player()?;
        // The session carries the control forward across saves that didn't touch it.
        match o.session.preview(revision, id, value) {
            Ok(audition) => {
                o.check_owner(&audition.origin, gesture)?;
                o.sync(&player)?;
                if !o.previews.iter().any(|p| p.origin.same_value(&audition.origin)) {
                    o.previews.push(Preview {
                        id: id.into(),
                        gesture: gesture.into(),
                        revision: o.playing,
                        origin: audition.origin.clone(),
                        baseline: audition.current,
                    });
                }
                player.set_const(audition.origin, audition.value);
                Ok(())
            }
            Err(error) => {
                o.reject(&player, id, gesture)?;
                Err(anyhow!("{error}"))
            }
        }
    }

    /// Close only the preview owned by this gesture. Old callbacks are harmless:
    /// a preview from an older renderer was already discarded with it.
    pub fn cancel_preview(&self, token: u64, id: &str, gesture: &str) -> Result<()> {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return Ok(()) };
        if token != o.token {
            return Ok(());
        }
        o.cancel(&self.player()?, id, gesture)
    }

    /// Commit a value to code. Errors carry what went wrong (stale, conflict, …).
    pub fn set(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64, expect: Option<f64>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let player = self.player()?;
        o.session.refresh();
        if let Ok(target) = o.session.preview(revision, id, value) {
            o.check_owner(&target.origin, gesture)?;
        }
        let before = o.session.accepted().number;
        // The session carries the edit forward from `revision` if the code changed
        // without touching the control, and refuses it as stale otherwise.
        match o.session.set(revision, id, value, expect) {
            Ok(c) => {
                // Only a verified single value edit can update the existing renderer.
                // set() may refresh again and adopt unrelated source changes first.
                let single_edit = c.before != c.after && c.revision == before + 1;
                if o.playing == before && (single_edit || c.revision == before) {
                    o.previews.retain(|p| p.gesture != gesture);
                    o.restore_previews(&player)?;
                    player.commit_const(c.origin.clone(), c.value)?;
                    o.playing = c.revision;
                } else {
                    o.restore(&player)?;
                }
                Ok(json!({"file": c.file, "before": c.before, "after": c.after, "revision": c.revision}))
            }
            Err(e) => {
                o.reject(&player, id, gesture)?;
                Err(anyhow!("{e}"))
            }
        }
    }

    /// Connect a named signal to a control's input, in code. The graph changes shape,
    /// so the player is rebuilt (at the current position) rather than patched.
    pub fn connect(&self, token: u64, revision: u64, from: &str, to: &str) -> Result<Value> {
        self.restructure(token, |session| session.connect(revision, from, to))
    }

    /// Disconnect a cable, writing the input's declared default in code.
    pub fn disconnect(&self, token: u64, revision: u64, id: &str) -> Result<Value> {
        self.restructure(token, |session| session.disconnect(revision, id))
    }

    /// Put one of the packages' effects after a call's audio, in code.
    pub fn add_effect(&self, token: u64, revision: u64, call: &str, module: &str, name: &str) -> Result<Value> {
        self.restructure(token, |session| session.add_effect(revision, call, module, name))
    }

    /// Take an effect out, leaving what it was applied to.
    pub fn remove_effect(&self, token: u64, revision: u64, call: &str) -> Result<Value> {
        self.restructure(token, |session| session.remove_effect(revision, call))
    }

    /// Rewrite a notes lane's clips (the piano roll, or clips made, moved or deleted in the
    /// timeline). The notes the renderer plays change, so it is rebuilt like a graph edit.
    pub fn write_notes(&self, token: u64, revision: u64, module: &str, lane: &str, clips: &[NoteClip]) -> Result<Value> {
        self.restructure(token, |session| session.write_notes(revision, module, lane, clips))
    }

    /// Undo the last edit made through this session (knob or cable).
    pub fn undo(&self, token: u64) -> Result<Value> {
        self.restructure(token, Session::undo)
    }

    pub fn redo(&self, token: u64) -> Result<Value> {
        self.restructure(token, Session::redo)
    }

    fn restructure(&self, token: u64, edit: impl FnOnce(&mut Session) -> Result<Edited, EditError>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        let player = self.player()?;
        let result = edit(&mut o.session);
        // Whatever happened, play the accepted revision; previews belong to the old one.
        o.restore(&player)?;
        let e = result.map_err(|e| anyhow!("{e}"))?;
        Ok(json!({"file": e.file, "before": e.before, "after": e.after, "revision": e.revision}))
    }

    pub fn transport(&self, action: &str) -> Result<()> {
        let player = self.player()?;
        match action {
            "play" => player.play(),
            "stop" => player.stop(),
            "toggle" if player.is_playing() => player.stop(),
            "toggle" => player.play(),
            "restart" => self.seek(0.0)?,
            other => return Err(anyhow!("unknown transport action {other}")),
        }
        Ok(())
    }

    /// Pick up edits made outside the app. Returns whether clients should refresh.
    pub fn poll(&self) -> bool {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return false };
        let changed = o.session.refresh();
        let mut synced = false;
        // A clone: syncing replays the song, and status waits for the player's lock.
        if let Some(p) = self.player.lock().clone() {
            if !o.session.draft_errors.is_empty()
                && let Err(e) = o.restore_previews(&p)
            {
                eprintln!("music2: couldn't restore the preview: {e:#}");
            }
            match o.sync(&p) {
                Ok(s) => synced = s,
                Err(e) => eprintln!("music2: couldn't rebuild the graph: {e:#}"),
            }
        }
        changed || synced
    }

    /// The project's TypeScript files, for the code editor (see [`files::list`]).
    pub fn files(&self) -> Result<Value> {
        let open = self.open.lock();
        files::list(&open.as_ref().context("no project is open")?.session)
    }

    /// The SDK and package sources the code editor checks imports against.
    pub fn types(&self) -> Result<Value> {
        let open = self.open.lock();
        Ok(files::types(&open.as_ref().context("no project is open")?.session.packages))
    }

    /// Save a file from the code editor if it still says `base` (else a [`files::Conflict`]),
    /// and adopt it at once, so the state returned shows it. Code that doesn't compile is
    /// saved too: the last revision that did plays on, and the diagnostics say why. A save
    /// that compiles is an edit made here, which undo takes back like any other.
    pub fn write_file(&self, token: Option<u64>, path: &str, base: &str, text: &str) -> Result<Value> {
        {
            let mut open = self.open.lock();
            let o = open.as_mut().context("no project is open")?;
            if let Some(token) = token {
                o.check_token(token)?;
            }
            let before = o.session.accepted().snapshot.files.get(Path::new(path)).cloned();
            files::write(&o.session.accepted().project.dir, path, base, text)?;
            o.session.saved(Path::new(path), before);
        }
        self.poll();
        self.state().context("no project is open")
    }

    /// The code editor of project `token` says which of its files have unsaved edits.
    pub fn set_unsaved(&self, token: u64, files: Vec<String>) -> Result<()> {
        self.open.lock().as_ref().context("no project is open")?.check_token(token)?;
        *self.unsaved.lock() = (token, files);
        Ok(())
    }

    /// The open project's files with edits in the code editor not yet saved, as it last said.
    pub fn unsaved(&self) -> Vec<String> {
        let Some(token) = self.open.lock().as_ref().map(|o| o.token) else { return vec![] };
        let unsaved = self.unsaved.lock();
        if unsaved.0 == token { unsaved.1.clone() } else { vec![] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("m2-graph-server-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            let fixture = Self(dir);
            fixture.write("export default song(() => osc.sine(hz(800)));\n");
            fixture
        }

        fn write(&self, body: &str) {
            std::fs::write(
                self.0.join("song.ts"),
                format!("import {{ hz, song }} from '@m2/core';\nimport {{ osc }} from 'std/dsp';\n{body}"),
            )
            .unwrap();
        }

        fn open(&self) -> Graph {
            let graph = Graph { player: Mutex::new(Some(Player::silent().unwrap())), ..Graph::default() };
            graph.open(&self.0).unwrap();
            graph
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn control(graph: &Graph) -> (u64, u64, String) {
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        let control = o.session.controls().into_iter().find(|c| c.value == 800.0).unwrap();
        (o.token, o.session.accepted().number, control.id)
    }

    #[test]
    fn rejected_edits_and_previews_close_their_preview() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        assert_eq!(graph.open.lock().as_ref().unwrap().previews[0].baseline, 800.0);
        assert!(graph.set(token, revision, &id, "drag", 950.0, Some(100.0)).is_err());
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
        assert_eq!(control(&graph), (token, revision, id.clone()));

        graph.preview(token, revision, &id, "drag-2", 900.0).unwrap();
        assert!(graph.preview(token, revision, &id, "drag-2", f64::NAN).is_err());
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
    }

    #[test]
    fn cancellation_and_conflicts_respect_gesture_ownership() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "owner", 900.0).unwrap();
        graph.cancel_preview(token, &id, "old-gesture").unwrap();
        assert!(graph.preview(token, revision, &id, "other", 1000.0).is_err());
        assert!(graph.set(token, revision, &id, "other", 1000.0, Some(800.0)).is_err());
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(graph.open.lock().as_ref().unwrap().previews[0].gesture, "owner");
        graph.cancel_preview(token, &id, "owner").unwrap();
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
        assert_eq!(control(&graph), (token, revision, id));
    }

    #[test]
    fn old_project_requests_do_not_modify_the_new_project_or_its_preview() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (old, revision, id) = control(&graph);
        graph.open(&fixture.0).unwrap();
        let (current, _, _) = control(&graph);
        assert!(current > old);
        graph.preview(current, revision, &id, "new", 900.0).unwrap();
        assert!(graph.preview(old, revision, &id, "old", 1000.0).is_err());
        assert!(graph.set(old, revision, &id, "old", 1000.0, Some(800.0)).is_err());
        graph.cancel_preview(old, &id, "new").unwrap();
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(control(&graph), (current, revision, id));
    }

    #[test]
    fn a_commit_carried_past_inserted_code_rebuilds_instead_of_patching_an_old_origin() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        let old_origin = graph.open.lock().as_ref().unwrap().previews[0].origin.clone();
        fixture.write("const first = hz(123);\nconst second = hz(456);\nexport default song(() => osc.sine(hz(800)));\n");
        // The inserted calls shift the control's position in the graph. The commit is
        // carried to it, and the player is rebuilt rather than patched at the old one.
        graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.session.accepted().number > revision + 1);
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(o.previews.is_empty());
        let current = o.session.controls().into_iter().find(|c| c.value == 950.0).unwrap();
        assert_ne!(current.origin, old_origin);
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        assert!(source.contains("hz(123)") && source.contains("hz(456)") && source.contains("hz(950)"));
    }

    #[test]
    fn an_unrelated_save_during_a_drag_keeps_the_drag_and_commits_it() {
        let fixture = Fixture::new();
        fixture.write("// notes\nexport default song(() => osc.sine(hz(800)));\n");
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("// notes, edited elsewhere\nexport default song(() => osc.sine(hz(800)));\n");
        assert!(graph.poll());
        // The drag goes on, still named by the revision it started in.
        graph.preview(token, revision, &id, "drag", 920.0).unwrap();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            assert_eq!((o.previews.len(), o.previews[0].baseline), (1, 800.0));
        }
        // Another gesture can't take the carried control.
        assert!(graph.preview(token, revision + 1, &id, "other", 1000.0).is_err());
        let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        assert_eq!(result["after"], "950");
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        assert!(source.contains("// notes, edited elsewhere") && source.contains("hz(950)"));
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, o.session.accepted().number);
    }

    #[test]
    fn a_save_that_touches_the_dragged_value_ends_the_drag() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("export default song(() => osc.sine(hz(801)));\n");
        assert!(graph.poll());
        assert!(graph.preview(token, revision, &id, "drag", 920.0).is_err());
        assert!(graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).is_err());
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("hz(801)"));
        assert!(graph.open.lock().as_ref().unwrap().previews.is_empty());
    }

    #[test]
    fn a_cable_edit_closes_previews_and_plays_the_new_graph() {
        let fixture = Fixture::new();
        fixture.write("export default song(() => {\n  const tone = osc.saw(hz(110));\n  return osc.sine(hz(800));\n});\n");
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        // An audio signal can't drive a frequency: refused, and the preview is closed.
        let from = "song.ts#0";
        assert!(graph.connect(token, revision, from, &id).is_err());
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!((o.playing, o.session.accepted().number), (revision, revision));
        drop(open);
        assert!(graph.connect(token + 1, revision, from, &id).is_err(), "another session's request is refused");
    }

    #[test]
    fn connecting_and_disconnecting_play_each_new_revision() {
        let fixture = Fixture::new();
        fixture.write(
            "import { lfo } from 'std/control';\nimport { xrange } from 'std/math';\nexport default song(({ time }) => {\n  const sweep = xrange(lfo(time.beats), hz(100), hz(200));\n  return osc.sine(hz(800));\n});\n",
        );
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        let outlet = graph.open.lock().as_ref().unwrap().session.outlets()[0].id.clone();
        let connected = graph.connect(token, revision, &outlet, &id).unwrap();
        assert_eq!((connected["before"].as_str(), connected["after"].as_str()), (Some("hz(800)"), Some("sweep")));
        let inlet = {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            o.session.inlets()[0].id.clone()
        };
        let revision = connected["revision"].as_u64().unwrap();
        let disconnected = graph.disconnect(token, revision, &inlet).unwrap();
        // std:sine declares `freq = 440`.
        assert_eq!(disconnected["after"], "hz(440)");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("osc.sine(hz(440))"));
    }

    #[test]
    fn undo_and_redo_play_each_revision_they_make() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        assert_eq!(graph.state().unwrap()["can_undo"], false);
        graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        assert_eq!(graph.state().unwrap()["can_undo"], true);
        let undone = graph.undo(token).unwrap();
        assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("950"), Some("800")));
        let state = graph.state().unwrap();
        assert_eq!((&state["can_undo"], &state["can_redo"]), (&json!(false), &json!(true)));
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
            assert_eq!(o.session.controls()[0].value, 800.0);
        }
        graph.redo(token).unwrap();
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("hz(950)"));
        assert!(graph.undo(token + 1).is_err(), "another session's request is refused");
    }

    #[test]
    fn effects_are_listed_added_and_removed_and_the_player_follows() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let state = graph.state().unwrap();
        let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
        assert!(state["effects"].as_array().unwrap().iter().any(|e| e["name"] == "delay" && e["module"] == "std/fx"));
        let node = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["audio"] == true).unwrap().clone();
        let call = node["id"].as_str().unwrap();
        let added = graph.add_effect(token, revision, call, "std/fx", "delay").unwrap();
        assert_eq!(added["after"], "delay(osc.sine(hz(800)), { time: 0.25, feedback: 0.3, mix: 0.3 })");
        let state = graph.state().unwrap();
        let delay = state["view"]["modules"][0]["nodes"].as_array().unwrap().iter().find(|n| n["effect"] == "delay").unwrap().clone();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert_eq!(o.playing, o.session.accepted().number);
        }
        let revision = state["view"]["revision"].as_u64().unwrap();
        graph.remove_effect(token, revision, delay["id"].as_str().unwrap()).unwrap();
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("song(() => osc.sine(hz(800)))"));
    }

    #[test]
    fn a_lane_write_plays_the_new_notes_and_undo_takes_them_back() {
        let fixture = Fixture::new();
        fixture.write(
            "import { track } from '@m2/core';\nimport { kick } from 'std/instruments';\n\
             const k = track('Kick', ({ lane }) => kick(lane.notes('hits')));\nexport default song(() => k.out);\n",
        );
        let graph = fixture.open();
        let state = graph.state().unwrap();
        let (token, revision) = (state["session"].as_u64().unwrap(), state["view"]["revision"].as_u64().unwrap());
        let synced = |graph: &Graph| graph.open.lock().as_ref().map(|o| o.playing == o.session.accepted().number).unwrap();
        let clips: Vec<NoteClip> =
            serde_json::from_value(json!([{"start": 0, "length": 4, "notes": [{"pitch": "C1", "start": 0, "length": 0.25}]}])).unwrap();
        let written = graph.write_notes(token, revision, "Kick", "hits", &clips).unwrap();
        assert_eq!((written["file"].as_str(), written["after"].as_str()), (Some("timeline/kick.hits.json"), Some("clip at bar 1 (1 bar)")));
        let state = graph.state().unwrap();
        assert_eq!(state["timeline"]["tracks"][0]["lanes"][0]["clips"][0]["notes"][0]["pitch"], 24.0);
        assert!(synced(&graph), "the player plays the new notes");

        assert!(graph.write_notes(token, revision + 1, "Kick", "nope", &clips).is_err());
        assert!(graph.write_notes(token + 1, revision + 1, "Kick", "hits", &[]).is_err(), "another session's request is refused");
        graph.undo(token).unwrap();
        assert!(!fixture.0.join("timeline/kick.hits.json").exists(), "undo removes the file the write created");
        assert!(synced(&graph));
    }

    #[test]
    fn a_saved_file_is_undone_like_an_edit_made_here() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.set(token, revision, &id, "drag", 900.0, Some(800.0)).unwrap();
        let base = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        let saved = base.replace("// ", "").replacen('\n', "\n// a comment\n", 1);
        let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
        assert_eq!(state["can_undo"], true);
        let undone = graph.undo(token).unwrap();
        assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("// a comment"), Some("nothing")));
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), base);
        // Then the knob, underneath it.
        let undone = graph.undo(token).unwrap();
        assert_eq!((undone["before"].as_str(), undone["after"].as_str()), (Some("900"), Some("800")));
        let state = graph.state().unwrap();
        assert_eq!((&state["can_undo"], &state["can_redo"]), (&json!(false), &json!(true)));
    }

    #[test]
    fn the_code_editors_unsaved_files_belong_to_the_project_it_said_them_for() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, ..) = control(&graph);
        assert!(graph.unsaved().is_empty());
        graph.set_unsaved(token, vec!["song.ts".into()]).unwrap();
        assert_eq!(graph.unsaved(), ["song.ts"]);
        assert!(graph.set_unsaved(token + 1, vec![]).is_err(), "another project's editor is refused");
        // Opened again, it is a new project: its editor starts with nothing unsaved.
        graph.open(&fixture.0).unwrap();
        assert!(graph.unsaved().is_empty());
        graph.close();
        assert!(graph.unsaved().is_empty());
    }

    #[test]
    fn poll_restores_invalid_drafts_and_syncs_already_refreshed_sources() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("(");
        assert!(graph.poll());
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert!(!o.session.draft_errors.is_empty());
            assert!(o.previews.is_empty());
            assert_eq!(o.playing, revision);
        }
        assert!(graph.preview(token, revision, &id, "drag", 900.0).is_err());
        fixture.write("export default song(() => osc.sine(hz(900)));\n");
        {
            let mut open = graph.open.lock();
            let o = open.as_mut().unwrap();
            assert!(o.session.refresh());
            assert!(!o.session.refresh());
            assert_ne!(o.playing, o.session.accepted().number);
        }
        assert!(graph.poll(), "a renderer behind the accepted revision still needs synchronization");
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert_eq!(o.playing, o.session.accepted().number);
    }

    #[test]
    fn commit_and_restart_close_previews_and_track_the_accepted_revision() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        let result = graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).unwrap();
        {
            let open = graph.open.lock();
            let o = open.as_ref().unwrap();
            assert!(o.previews.is_empty());
            assert_eq!(o.playing, result["revision"].as_u64().unwrap());
            assert_eq!(o.session.controls()[0].value, 950.0);
        }
        let revision = result["revision"].as_u64().unwrap();
        graph.preview(token, revision, &id, "next", 1000.0).unwrap();
        graph.transport("restart").unwrap();
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, revision);
        assert_eq!(o.session.controls()[0].value, 950.0);
    }

    /// Wait for the player's thread to catch up.
    fn eventually(what: &str, done: impl Fn() -> bool) {
        let start = std::time::Instant::now();
        while !done() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "timed out waiting until {what}");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn seeking_keeps_playing_or_stopped_and_status_counts_beats_in_the_tempo() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.0.join("timeline")).unwrap();
        std::fs::write(fixture.0.join("timeline/song.json"), r#"{"tempo": [{"beat": 0, "bpm": 120}, {"beat": 4, "bpm": 60}]}"#).unwrap();
        let graph = fixture.open();
        let state = graph.state().unwrap();
        assert_eq!(state["timeline"]["tempo"], json!([{"beat": 0.0, "bpm": 120.0}, {"beat": 4.0, "bpm": 60.0}]));
        assert_eq!((&state["timeline"]["end"], &state["beat"]), (&json!(4.0), &json!(0.0)));

        // Stopped, the position moves at once and stays there.
        graph.seek(2.0).unwrap();
        assert_eq!(graph.status(), Status { playing: false, seconds: 1.0, beat: 2.0 }, "at 120 bpm, 1 second is 2 beats");
        graph.seek(8.0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(graph.status(), Status { playing: false, seconds: 6.0, beat: 8.0 }, "then 4 beats at 60 bpm");
        graph.seek(-3.0).unwrap();
        assert_eq!(graph.status().beat, 0.0);
        assert!(graph.seek(f64::NAN).is_err());

        // Playing, it goes on playing from there.
        graph.transport("play").unwrap();
        eventually("it plays", || graph.status().playing);
        graph.seek(4.0).unwrap();
        eventually("it plays on from beat 4", || graph.status().beat > 4.0);
        assert!(graph.status().playing);
        graph.transport("stop").unwrap();
        eventually("it stops", || !graph.status().playing);
        graph.seek(1.0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert_eq!(graph.status(), Status { playing: false, seconds: 0.5, beat: 1.0 });

        // Seeking replays from the start, so it stays within reach of the song.
        assert!(graph.seek(4.0 + SEEK_ROOM + 1.0).is_err());
        assert_eq!(graph.status().beat, 1.0);

        let closed = Graph { player: Mutex::new(Some(Player::silent().unwrap())), ..Graph::default() };
        assert!(closed.seek(1.0).is_err(), "no project is open");
        assert_eq!(closed.status().beat, 0.0);
        graph.close();
        assert_eq!(graph.status().beat, 0.0);
    }

    #[test]
    fn seeking_replays_past_a_nan_the_way_playback_does() {
        let fixture = Fixture::new();
        std::fs::create_dir_all(fixture.0.join("timeline")).unwrap();
        // 1e39 overflows to infinity: a kernel produces non-finite samples from beat 8.
        std::fs::write(fixture.0.join("timeline/song.g.json"), r#"{"clips": [{"start": 8, "length": 8, "points": [[0, 1e39]]}]}"#).unwrap();
        fixture.write("import { vca } from 'std/dsp';\nexport default song(({ lane }) => vca(osc.sine(hz(440)), lane.control(\"g\", \"gain\", 1)));\n");
        let graph = fixture.open();
        graph.seek(12.0).unwrap();
        assert_eq!(graph.status().beat, 12.0);
    }

    #[test]
    fn status_does_not_wait_for_the_project() {
        let fixture = Fixture::new();
        let graph = &fixture.open();
        graph.seek(2.0).unwrap();
        // As a seek or an edit does while it replays the song.
        let held = graph.open.lock();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(move || tx.send(graph.status()).unwrap());
            let status = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("status answers while the project is held");
            assert_eq!(status.beat, 2.0);
            drop(held);
        });
    }

    #[test]
    fn seeking_closes_previews_and_plays_the_accepted_revision() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        fixture.write("export default song(() => osc.sine(hz(700)));\n");
        {
            let mut open = graph.open.lock();
            let o = open.as_mut().unwrap();
            assert!(o.session.refresh());
            assert_ne!(o.playing, o.session.accepted().number);
        }
        graph.seek(4.0).unwrap();
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, o.session.accepted().number);
    }

    #[test]
    fn returning_a_drag_to_its_start_closes_the_preview_without_rewriting_source() {
        let fixture = Fixture::new();
        fixture.write("export default song(() => osc.sine(hz(8_00)));\n");
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        let source = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        graph.preview(token, revision, &id, "drag", 950.0).unwrap();
        let result = graph.set(token, revision, &id, "drag", 800.0, Some(800.0)).unwrap();
        assert_eq!(result["revision"].as_u64().unwrap(), revision);
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), source);
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.previews.is_empty());
        assert_eq!(o.playing, revision);
        assert_eq!(o.session.controls()[0].value, 800.0);
    }

    #[test]
    fn a_saved_file_plays_at_once_and_a_save_over_a_change_is_refused() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, _) = control(&graph);
        let playing = |graph: &Graph| graph.open.lock().as_ref().map(|o| (o.playing, o.session.accepted().number)).unwrap();
        let base = std::fs::read_to_string(fixture.0.join("song.ts")).unwrap();
        let saved = base.replace("800", "900");
        let state = graph.write_file(Some(token), "song.ts", &base, &saved).unwrap();
        assert_eq!((&state["view"]["revision"], &state["diagnostics"]), (&json!(revision + 1), &json!([])));
        assert_eq!(playing(&graph), (revision + 1, revision + 1), "adopted and playing without waiting for the watcher");
        assert!(!graph.poll());

        // Saved from the old text: refused with what the file says now, and nothing written.
        let e = graph.write_file(Some(token), "song.ts", &base, "export default 1;\n").unwrap_err();
        assert_eq!(e.downcast_ref::<files::Conflict>().unwrap().current, saved);
        assert_eq!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap(), saved);

        // Code that doesn't compile is saved; the last revision that did plays on.
        let state = graph.write_file(Some(token), "song.ts", &saved, "export default song(() => (\n").unwrap();
        assert_eq!(state["view"]["revision"], revision + 1);
        assert_eq!((&state["diagnostics"][0]["file"], &state["diagnostics"][0]["line"]), (&json!("song.ts"), &json!(2)));
        assert_eq!(playing(&graph), (revision + 1, revision + 1));
        assert!(std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().ends_with("song(() => (\n"));

        assert!(graph.write_file(Some(token + 1), "song.ts", "", "").is_err(), "another session's editor is refused");
        assert!(graph.write_file(Some(token), "../song.ts", "", "").is_err());
        graph.close();
        assert!(graph.files().is_err() && graph.write_file(None, "song.ts", "", "").is_err());
    }
}

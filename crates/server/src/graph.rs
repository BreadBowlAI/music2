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
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow};
use m2_ir::Origin;
use m2_play::Player;
use m2_runtime::{PackageSet, Renderer};
use m2_session::Session;
use parking_lot::Mutex;
use serde_json::{Value, json};

struct Open {
    token: u64,
    session: Session,
    /// The accepted revision the player is playing.
    playing: u64,
    previews: Vec<Preview>,
}

struct Preview {
    id: String,
    gesture: String,
    /// Baselines belong to the renderer revision, never a newly adopted source.
    revision: u64,
    origin: Origin,
    baseline: f64,
}

#[derive(Default)]
pub struct Graph {
    open: Mutex<Option<Open>>,
    player: Mutex<Option<Player>>,
    next_session: AtomicU64,
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

/// A renderer for the accepted revision, positioned at `frame`.
fn renderer(session: &Session, sample_rate: u32, frame: u64) -> Result<Renderer> {
    let rev = session.accepted();
    let mut r = Renderer::for_project(&rev.compiled.flat, &rev.project, &session.packages, sample_rate)?;
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

    fn check_owner(&self, id: &str, gesture: &str) -> Result<()> {
        if self.previews.iter().any(|p| p.revision == self.session.accepted().number && p.id == id && p.gesture != gesture) {
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

    /// Source refresh can happen inside an edit, independently of the file watcher.
    fn sync(&mut self, player: &Player) -> Result<bool> {
        let accepted = self.session.accepted().number;
        if accepted == self.playing {
            return Ok(false);
        }
        player.load(renderer(&self.session, player.sample_rate, player.position())?);
        self.playing = accepted;
        self.previews.clear();
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
        player.stop();
        player.load(new_renderer);
        let playing = session.accepted().number;
        let token = self.next_session.fetch_add(1, Ordering::Relaxed) + 1;
        *open = Some(Open { token, session, playing, previews: vec![] });
        drop(open);
        self.state().context("no project")
    }

    pub fn close(&self) {
        let mut open = self.open.lock();
        if let Some(p) = self.player.lock().as_ref() {
            p.stop();
        }
        *open = None;
    }

    /// The view, the project's name and folder, and the transport.
    pub fn state(&self) -> Option<Value> {
        let open = self.open.lock();
        let o = open.as_ref()?;
        let rev = o.session.accepted();
        let (playing, seconds) = self.transport_state();
        Some(json!({
            "session": o.token,
            "name": rev.project.manifest.project.name,
            "dir": rev.project.dir,
            "view": o.session.view(),
            "playing": playing,
            "seconds": seconds,
        }))
    }

    pub fn transport_state(&self) -> (bool, f64) {
        match self.player.lock().as_ref() {
            Some(p) => (p.is_playing(), p.position() as f64 / p.sample_rate as f64),
            None => (false, 0.0),
        }
    }

    /// Change a value in the playing audio only (while a knob is dragged).
    pub fn preview(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64) -> Result<()> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        o.check_owner(id, gesture)?;
        let player = self.player()?;
        match o.session.preview(revision, id, value) {
            Ok((origin, value)) => {
                o.sync(&player)?;
                if !o.previews.iter().any(|p| p.origin.same_value(&origin)) {
                    let control = o.session.controls().into_iter().find(|c| c.id == id).context("the control disappeared")?;
                    o.previews.push(Preview {
                        id: id.into(),
                        gesture: gesture.into(),
                        revision: o.playing,
                        origin: origin.clone(),
                        baseline: control.value,
                    });
                }
                player.set_const(origin, value);
                Ok(())
            }
            Err(error) => {
                o.reject(&player, id, gesture)?;
                Err(anyhow!("{error}"))
            }
        }
    }

    /// Close only the preview owned by this gesture. Old callbacks are harmless.
    pub fn cancel_preview(&self, token: u64, revision: u64, id: &str, gesture: &str) -> Result<()> {
        let mut open = self.open.lock();
        let Some(o) = open.as_mut() else { return Ok(()) };
        if token != o.token || revision != o.session.accepted().number || revision != o.playing {
            return Ok(());
        }
        o.cancel(&self.player()?, id, gesture)
    }

    /// Commit a value to code. Errors carry what went wrong (stale, conflict, …).
    pub fn set(&self, token: u64, revision: u64, id: &str, gesture: &str, value: f64, expect: Option<f64>) -> Result<Value> {
        let mut open = self.open.lock();
        let o = open.as_mut().context("no project is open")?;
        o.check_token(token)?;
        o.check_owner(id, gesture)?;
        let player = self.player()?;
        o.session.refresh();
        let before = o.session.accepted().number;
        if revision != before {
            o.reject(&player, id, gesture)?;
            return Err(anyhow!("the code changed; it is revision {before} now, refresh the graph before editing"));
        }
        match o.session.set(revision, id, value, expect) {
            Ok(c) => {
                // Only a verified single value edit can update the existing renderer.
                // set() may refresh again and adopt unrelated source changes first.
                let single_edit = c.before != c.after && c.revision == before + 1;
                if o.playing == before && (single_edit || c.revision == before) {
                    o.previews.retain(|p| p.id != id);
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

    pub fn transport(&self, action: &str) -> Result<()> {
        let player = self.player()?;
        match action {
            "play" => player.play(),
            "stop" => player.stop(),
            "toggle" if player.is_playing() => player.stop(),
            "toggle" => player.play(),
            "restart" => {
                let mut open = self.open.lock();
                let o = open.as_mut().context("no project is open")?;
                player.load(renderer(&o.session, player.sample_rate, 0)?);
                o.playing = o.session.accepted().number;
                o.previews.clear();
            }
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
        if let Some(p) = self.player.lock().as_ref() {
            if !o.session.draft_errors.is_empty()
                && let Err(e) = o.restore_previews(p)
            {
                eprintln!("music2: couldn't restore the preview: {e:#}");
            }
            match o.sync(p) {
                Ok(s) => synced = s,
                Err(e) => eprintln!("music2: couldn't rebuild the graph: {e:#}"),
            }
        }
        changed || synced
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
        graph.cancel_preview(token, revision, &id, "old-gesture").unwrap();
        assert!(graph.preview(token, revision, &id, "other", 1000.0).is_err());
        assert!(graph.set(token, revision, &id, "other", 1000.0, Some(800.0)).is_err());
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(graph.open.lock().as_ref().unwrap().previews[0].gesture, "owner");
        graph.cancel_preview(token, revision, &id, "owner").unwrap();
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
        graph.cancel_preview(old, revision, &id, "new").unwrap();
        assert_eq!(graph.open.lock().as_ref().unwrap().previews.len(), 1);
        assert_eq!(control(&graph), (current, revision, id));
    }

    #[test]
    fn stale_commit_after_source_insertion_rebuilds_instead_of_patching_an_old_origin() {
        let fixture = Fixture::new();
        let graph = fixture.open();
        let (token, revision, id) = control(&graph);
        graph.preview(token, revision, &id, "drag", 900.0).unwrap();
        let old_origin = graph.open.lock().as_ref().unwrap().previews[0].origin.clone();
        fixture.write("const first = hz(123);\nconst second = hz(456);\nexport default song(() => osc.sine(hz(800)));\n");
        assert!(graph.set(token, revision, &id, "drag", 950.0, Some(800.0)).is_err());
        let open = graph.open.lock();
        let o = open.as_ref().unwrap();
        assert!(o.session.accepted().number > revision);
        assert_eq!(o.playing, o.session.accepted().number);
        assert!(o.previews.is_empty());
        let current = o.session.controls().into_iter().find(|c| c.value == 800.0).unwrap();
        assert_ne!(current.origin, old_origin);
        assert!(!std::fs::read_to_string(fixture.0.join("song.ts")).unwrap().contains("950"));
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
}

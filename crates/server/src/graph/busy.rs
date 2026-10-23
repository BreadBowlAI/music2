//! What the musician holds (AGENT-3): an agent's write that touches it is refused, busy. The app
//! checks when the write arrives, and again at the start of the settle that follows, under the
//! control plane's lock, before anything of the change is carried, sent to the engine or shown;
//! a change the musician began to hold meanwhile is taken back there.

use m2_ir::FOp;
use serde_json::{Value, json};

use super::actor::{Open, plugin_named};
use super::{ControlPlane, Inner};

/// An agent's write refused because the musician holds what it touches (AGENT-3): status 423.
#[derive(Clone, Debug, PartialEq)]
pub struct Busy {
    /// `knob`, `lane`, `tempo`, `plugin`, `code` or `take`.
    pub what: &'static str,
    /// The file held, if one is (a take holds everything).
    pub file: Option<String>,
    pub words: String,
    /// The change landed, and was taken back at its settle.
    pub taken_back: bool,
}

impl Busy {
    /// The answer's body: `{"busy": {what, file, words, taken_back}}`.
    pub fn json(&self) -> Value {
        json!({"busy": {"what": self.what, "file": self.file, "words": self.words, "taken_back": self.taken_back}})
    }
}

impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.words)
    }
}

impl std::error::Error for Busy {}

/// Agents don't hold leases (AGENT-3): a drag is the musician's. An agent's change is one edit.
pub(super) fn no_lease() -> anyhow::Result<()> {
    if crate::caller::now().is_agent() {
        anyhow::bail!("agents don't drag: an agent's change is one edit (m2 set, m2 clips, m2 tempo), never a lease");
    }
    Ok(())
}

/// Something the musician holds: a file (or everything, `None`), and the words that say it.
#[derive(Clone, Debug)]
pub(super) struct Hold {
    what: &'static str,
    file: Option<String>,
    words: String,
}

/// What the musician holds in project `o` now (AGENT-3): their knob, clip and tempo drags, a
/// plugin window's changes not yet saved, the code editor's unsaved text, and a take. Read under
/// the control plane's lock, `cp`, where drags, unsaved text and takes begin.
pub(super) fn held(cp: &ControlPlane, o: &Open, inner: &Inner) -> Vec<Hold> {
    let token = o.token;
    let view = cp.open.as_ref().filter(|p| p.engine.token == token).map(|p| &p.shown.state["view"]["modules"]);
    let label = |module: &str| super::record::label_of(cp, module);
    let mut out = vec![];
    // A take holds everything: its notes follow the tempo map, and the musician may hear
    // themselves through the track's effects.
    if let Some(module) = super::record::recording_on(cp, token) {
        out.push(Hold { what: "take", file: None, words: format!("you're recording on {}", label(&module)) });
    }
    for l in cp.leases.live.iter().filter(|l| l.token == token) {
        let knob = view.and_then(|v| v.as_array()).into_iter().flatten().find_map(|m| {
            let nodes = m["nodes"].as_array()?;
            let k = nodes.iter().flat_map(|n| n["knobs"].as_array().into_iter().flatten()).find(|k| k["id"] == l.control.id.as_str())?;
            Some(format!("{} {}", m["label"].as_str()?, k["label"].as_str()?))
        });
        let knob = knob.unwrap_or_else(|| format!("a knob in {}", l.control.file));
        out.push(Hold { what: "knob", file: Some(l.control.file.clone()), words: format!("you're dragging {knob} in the app") });
    }
    for lane in super::lanes::held(cp, token) {
        let file = m2_song::DataPath::lane(&lane).to_string();
        out.push(Hold { what: "lane", file: Some(file), words: format!("you're dragging a clip on {}", label(lane.module.as_str())) });
    }
    if super::tempo_lease::held(cp, token) {
        out.push(Hold { what: "tempo", file: Some("data/arrangement.json".into()), words: "you're dragging the tempo".into() });
    }
    if let Some(p) = &o.plays {
        for h in o.windows.unsaved() {
            let Some(at) = p.info.plugins.iter().find(|at| at.handle == h) else { continue };
            let n = &p.revision.compiled.flat.nodes[at.node];
            let FOp::Plugin { state: Some(keys), .. } = &n.op else { continue };
            let file = format!("data/{}/{}.state", n.module, keys.join("/"));
            let who = plugin_named(&*p.revision, &o.session.packages, at.node);
            out.push(Hold { what: "plugin", file: Some(file), words: format!("you're changing {who} in its window") });
        }
    }
    let unsaved = inner.unsaved.lock();
    if unsaved.0 == token {
        for f in &unsaved.1 {
            out.push(Hold { what: "code", file: Some(f.clone()), words: format!("{f} has unsaved changes in the app's code editor") });
        }
    }
    out
}

/// The first of `holds` an agent's write touching `files` meets: every hold, when `files` is
/// None (a write whose files aren't known before it is planned meets only a take). A file is
/// held by its own path, a folder holding it, or (a plugin's state) its sidecar.
pub(super) fn check(holds: &[Hold], files: Option<&[String]>) -> Option<Busy> {
    let meets = |h: &Hold| match (&h.file, files) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(f), Some(files)) => {
            files.iter().any(|t| t == f || *t == format!("{f}.json") || f.starts_with(&format!("{t}/")) || t.starts_with(&format!("{f}/")))
        }
    };
    holds.iter().find(|h| meets(h)).map(|h| Busy { what: h.what, file: h.file.clone(), words: h.words.clone(), taken_back: false })
}

#[cfg(test)]
#[path = "busy_tests.rs"]
mod tests;

//! Pictures asked for (CARD-67, CARD-68): a card's picture, drawn by the picture runner
//! (`m2_script::pictures`, CARD-64) from what plays, while a window or an agent asks for it.
//!
//! - **On request.** `POST /api/graph/picture` names a card by the shown view's id and a picture by
//!   its name; the answer is drawn from what plays then: the accepted view's values, and every live
//!   lease's on the card. An ask lasts as a scope's does ([`super::scope::KEEP`]): while its client
//!   asks again, until it stops, or its window's socket closes.
//! - **Drawn again once a frame.** The server's `music2-pictures` thread calls
//!   [`Graph::pictures_frame`] as soon as a lease's value moves, or every [`LOOK`], and at most once
//!   every [`FRAME`]: each picture asked for whose values or code changed since it was drawn is drawn
//!   again, once for all its card's leases, and each window that asked gets it in its own
//!   `pictures` message. Nothing changed, nothing is drawn.
//! - **Each dot's knobs.** The runner names the values a dot moves; the answer adds the knob that
//!   moves each, from the view, or why the card can't (CARD-66), so an agent writes it as a knob.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_script::pictures::{Ask, Drawn, Song};
use serde_json::{Map, Value, json};

use super::{ControlPlane, Graph};

/// How often a picture is drawn again at most: one screen frame.
pub const FRAME: Duration = Duration::from_millis(16);
/// How long the pictures' thread waits for a lease to move before it looks anyway: for a write, an
/// agent's change, a new code revision and asks that end.
pub const LOOK: Duration = Duration::from_millis(100);

/// What wakes the pictures' thread: a lease's value moved, so a picture may draw again now.
#[derive(Default)]
pub(super) struct Wake {
    due: parking_lot::Mutex<bool>,
    rung: parking_lot::Condvar,
}

impl Wake {
    pub(super) fn ring(&self) {
        *self.due.lock() = true;
        self.rung.notify_one();
    }
}

/// The pictures windows and agents ask for, on the control plane.
#[derive(Default)]
pub(super) struct Pictures {
    asked: Vec<Asked>,
    /// What each draw is numbered by, so a socket sends only what is newer than what it sent.
    version: u64,
    /// The song's code at its last code revision, for its own sounds' pictures (D16).
    song: Option<Arc<Song>>,
}

/// One picture asked for: by its module, the shown view's id of its card, and its name.
struct Asked {
    module: String,
    node: String,
    view: String,
    clients: Vec<(String, Instant)>,
    /// What it was last drawn from, and the picture that drew, numbered.
    from: Option<String>,
    drawn: Option<Arc<Value>>,
    version: u64,
}

/// One picture as a frame sends it: its draw's number and its JSON.
#[derive(Debug)]
pub struct SentPicture {
    pub version: u64,
    pub json: Arc<Value>,
}

/// What a frame sends (CARD-68): each window's pictures, by its client id.
#[derive(Debug)]
pub struct PicturesOut {
    pub session: u64,
    pub clients: Vec<(String, Vec<SentPicture>)>,
}

/// A picture to draw, as found on the control plane: what the runner is asked, and what the answer
/// is made with.
struct Found {
    module: String,
    node: String,
    view: String,
    /// The card's label, which names a picture that can't be drawn.
    label: String,
    ask: Ask,
    /// What it is drawn from: its code and its values.
    from: String,
    /// Each value's knob, or why the card can't move it, by name.
    knobs: Map<String, Value>,
}

impl Pictures {
    fn find(&mut self, module: &str, node: &str, view: &str) -> Option<&mut Asked> {
        self.asked.iter_mut().find(|a| a.module == module && a.node == node && a.view == view)
    }

    /// Keep `client`'s ask for the picture, from now.
    fn renew(&mut self, client: &str, module: &str, node: &str, view: &str, now: Instant) {
        let a = match self.find(module, node, view) {
            Some(a) => a,
            None => {
                let a = Asked {
                    module: module.into(),
                    node: node.into(),
                    view: view.into(),
                    clients: vec![],
                    from: None,
                    drawn: None,
                    version: 0,
                };
                self.asked.push(a);
                self.asked.last_mut().expect("just pushed")
            }
        };
        match a.clients.iter_mut().find(|(c, _)| c == client) {
            Some(c) => c.1 = now,
            None => a.clients.push((client.to_string(), now)),
        }
    }

    /// `client` stops asking for the picture, or for every one (`None`, its window closed); asks no
    /// one keeps, or no one renewed in [`super::scope::KEEP`], end.
    fn stop(&mut self, client: &str, which: Option<(&str, &str, &str)>, now: Instant) {
        for a in &mut self.asked {
            if which.is_none_or(|(m, n, v)| a.module == m && a.node == n && a.view == v) {
                a.clients.retain(|(c, _)| c != client);
            }
        }
        self.expire(now);
    }

    /// Asks no one renewed in [`super::scope::KEEP`] end.
    fn expire(&mut self, now: Instant) {
        for a in &mut self.asked {
            a.clients.retain(|(_, t)| now.duration_since(*t) < super::scope::KEEP);
        }
        self.asked.retain(|a| !a.clients.is_empty());
    }

    /// Store what `found` drew; the picture as its answer says it.
    fn drew(&mut self, found: &Found, picture: Value) -> Arc<Value> {
        self.version += 1;
        let version = self.version;
        let picture = Arc::new(picture);
        if let Some(a) = self.find(&found.module, &found.node, &found.view) {
            (a.from, a.drawn, a.version) = (Some(found.from.clone()), Some(picture.clone()), version);
        }
        picture
    }
}

/// The picture `view` of card `node` of module `module` in what the control plane holds: what to
/// ask the runner, from the accepted view's values and each live lease's.
fn found(cp: &mut ControlPlane, module: &str, node: &str, view: &str) -> Result<Found> {
    let open = cp.open.as_ref().context("no project is open")?;
    let (shown, accepted, token) = (open.shown.clone(), open.engine.accepted.clone(), open.engine.token);
    let state = &shown.state;
    let modules = state["view"]["modules"].as_array().map(Vec::as_slice).unwrap_or_default();
    let m = modules.iter().find(|m| m["key"] == module).with_context(|| format!("there is no track {module}"))?;
    let nodes = m["nodes"].as_array().map(Vec::as_slice).unwrap_or_default();
    let card = nodes.iter().find(|n| n["id"] == node).with_context(|| format!("{module} has no card {node}: refresh the graph"))?;
    let views = card["views"].as_array().map(Vec::as_slice).unwrap_or_default();
    let Some(part) = views.iter().find(|v| v["name"] == view && v["kind"] == "picture") else {
        bail!("{} has no picture named {view}", card["title"].as_str().unwrap_or(node));
    };
    let cid = card["component"].as_str().context("the card is no sound's")?.to_string();
    let code = accepted.compiled.code();
    let desc = code.components.get(&cid).with_context(|| format!("no sound {cid} was built"))?;
    let packages: Vec<(String, std::path::PathBuf)> = code.decls.packages.iter().map(|(n, d)| (n.clone(), d.clone())).collect();
    // A sound of the song's own draws from the song's code at its code revision (D16).
    let song = match cid.contains(':') {
        true => None,
        false => {
            let kept = cp.pictures.song.as_ref().filter(|s| s.key == accepted.code_rev);
            Some(match kept {
                Some(s) => s.clone(),
                None => {
                    let project = &accepted.project;
                    let s = Arc::new(Song {
                        key: accepted.code_rev,
                        code: project.code.iter().map(|(p, t)| (p.clone(), t.to_string())).collect(),
                        dependencies: project.manifest.dependencies.keys().filter(|d| *d != m2_song::SDK).cloned().collect(),
                    });
                    cp.pictures.song = Some(s.clone());
                    s
                }
            })
        }
    };
    // What plays: the view's values, and each live lease's on them (CARD-68).
    let mut values = Map::new();
    let mut knobs = Map::new();
    for v in part["values"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let name = v["name"].as_str().unwrap_or_default().to_string();
        let mut value = v["value"].clone();
        if let Some(id) = v["knob"].as_str()
            && let Some(l) = cp.leases.live.iter().find(|l| l.token == token && l.control.id == id)
        {
            value = json!(l.playing);
        }
        let mut k = Map::new();
        for f in ["knob", "why"] {
            if let Some(x) = v.get(f).filter(|x| !x.is_null()) {
                k.insert(f.into(), x.clone());
            }
        }
        knobs.insert(name.clone(), Value::Object(k));
        values.insert(name, value);
    }
    let values = Value::Object(values).to_string();
    let code_key = match &song {
        Some(s) => format!("song {}", s.key),
        None => format!("{packages:?}"),
    };
    let from = format!("{code_key}\n{cid}\n{view}\n{values}");
    let ask = Ask {
        packages,
        song,
        module: desc.module.clone(),
        component: cid,
        view: view.to_string(),
        values,
        memo: format!("{module}\u{0}{node}\u{0}{view}"),
    };
    let label = card["title"].as_str().or(card["label"].as_str()).unwrap_or(node).to_string();
    Ok(Found { module: module.into(), node: node.into(), view: view.into(), label, ask, from, knobs })
}

/// The picture as an answer says it (CARD-67): its shapes, each dot's values with their knobs, and
/// its words; or why it can't be drawn, named by its card.
fn said(found: &Found, drawn: Drawn) -> Value {
    let mut out = json!({"module": found.module, "node": found.node, "view": found.view});
    match drawn {
        Drawn::Cant(why) => {
            let why = format!("{}'s picture {}", found.label, why.trim_end_matches('.'));
            out["why"] = json!(format!("{why}."));
        }
        Drawn::Shapes(json) => {
            let mut drew: Value = serde_json::from_str(&json).unwrap_or_default();
            if let Some(shapes) = drew["shapes"].as_array_mut() {
                knobbed(shapes, &found.knobs);
            }
            out["words"] = drew["words"].take();
            out["shapes"] = drew["shapes"].take();
        }
    }
    out
}

/// Each dot of `shapes`, in a part's too, its values given their knobs (CARD-66).
fn knobbed(shapes: &mut [Value], knobs: &Map<String, Value>) {
    for s in shapes {
        if let Some(inner) = s.get_mut("shapes").and_then(Value::as_array_mut) {
            knobbed(inner, knobs);
        }
        let Some(moves) = s.get_mut("moves").and_then(Value::as_object_mut) else { continue };
        for m in moves.values_mut() {
            let Some(k) = m["name"].as_str().and_then(|n| knobs.get(n)).and_then(Value::as_object) else { continue };
            for (f, x) in k {
                m[f] = x.clone();
            }
        }
    }
}

impl Graph {
    /// Start, keep or stop picture `view` of card `node` of module `module`, by the shown view's id,
    /// for `client` (`POST /api/graph/picture`, CARD-67). Not a write: it works read-only too.
    /// Answers `{picture: {module, node, view, words, shapes}}` drawn from what plays now, or
    /// `{picture: {…, why}}` when it can't be drawn; `{}` for a stop.
    pub fn picture(&self, token: u64, client: &str, module: &str, node: &str, view: &str, on: bool) -> Result<Value> {
        let found = {
            let mut cp = self.inner.control.lock();
            cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
            let now = Instant::now();
            if !on {
                cp.pictures.stop(client, Some((module, node, view)), now);
                return Ok(json!({}));
            }
            let found = found(&mut cp, module, node, view)?;
            cp.pictures.renew(client, module, node, view, now);
            found
        };
        let drawn = self.inner.runner().draw(found.ask.clone());
        let picture = said(&found, drawn);
        let picture = self.inner.control.lock().pictures.drew(&found, picture);
        Ok(json!({"picture": *picture}))
    }

    /// One frame of pictures (CARD-68): each picture asked for whose values or code changed since it
    /// was drawn, drawn again; and each window's pictures, when any was.
    pub fn pictures_frame(&self) -> Option<PicturesOut> {
        let (token, todo) = {
            let mut cp = self.inner.control.lock();
            cp.pictures.expire(Instant::now());
            if cp.pictures.asked.is_empty() {
                return None;
            }
            let token = cp.open.as_ref()?.engine.token;
            let asked: Vec<(String, String, String, Option<String>)> =
                cp.pictures.asked.iter().map(|a| (a.module.clone(), a.node.clone(), a.view.clone(), a.from.clone())).collect();
            // A card gone from the view waits: its window asks for it by its new id.
            let todo: Vec<Found> = asked
                .into_iter()
                .filter_map(|(m, n, v, from)| found(&mut cp, &m, &n, &v).ok().filter(|f| from.as_ref() != Some(&f.from)))
                .collect();
            (token, todo)
        };
        if todo.is_empty() {
            return None;
        }
        for f in &todo {
            let picture = said(f, self.inner.runner().draw(f.ask.clone()));
            self.inner.control.lock().pictures.drew(f, picture);
        }
        let cp = self.inner.control.lock();
        let mut clients: Vec<(String, Vec<SentPicture>)> = vec![];
        for a in &cp.pictures.asked {
            let Some(json) = &a.drawn else { continue };
            for (c, _) in &a.clients {
                let sent = SentPicture { version: a.version, json: json.clone() };
                match clients.iter_mut().find(|(w, _)| w == c) {
                    Some(w) => w.1.push(sent),
                    None => clients.push((c.clone(), vec![sent])),
                }
            }
        }
        Some(PicturesOut { session: token, clients })
    }

    /// Wait until a lease's value moves, or `most` (the pictures' thread, CARD-68).
    pub fn pictures_wait(&self, most: Duration) {
        let w = &self.inner.picture_wake;
        let mut due = w.due.lock();
        if !*due {
            w.rung.wait_for(&mut due, most);
        }
        *due = false;
    }

    /// `client`'s window closed: the pictures it asked for end (CARD-67).
    pub(super) fn pictures_gone(&self, client: &str) {
        self.inner.control.lock().pictures.stop(client, None, Instant::now());
    }
}

#[cfg(test)]
#[path = "pictures_tests.rs"]
mod tests;

//! Pictures (CARD-63, CARD-67, CARD-68): a card's picture, drawn by the picture runner
//! (`m2_script::pictures`, CARD-64) from what plays, while a window or an agent asks for it; and
//! every picture's words, in the view.
//!
//! - **On request.** `POST /api/graph/picture` names a card by its id in the view of a revision and
//!   a picture by its name. An id is good only while the song's code is as it was then: past that
//!   it may name another call, so the ask is refused, and asked again by the card's id now. The
//!   answer is drawn from what plays then: the view's values, and every live lease's on the card.
//!   An ask lasts as a scope's does ([`super::scope::KEEP`]); asked again with nothing changed, it
//!   is answered with the picture already drawn.
//! - **Drawn again at once.** The `music2-pictures` thread waits for a lease to move (or [`LOOK`]),
//!   then draws each picture asked for whose values or code changed since it was drawn, once for
//!   all its card's leases; moves that come while it draws are drawn together after it. Each window
//!   that asked gets it in its own `pictures` message.
//! - **Words in the view.** After each accepted revision the same thread works out every picture's
//!   words at the view's values, and [`Graph::state`] puts them in the view (CARD-63).
//! - **Each dot's knobs.** The runner checked the names a dot moves against the descriptor; the
//!   answer gives each the card's own knob, or why the card can't move it (CARD-66).
//! - **Measured layers** (CARD-80 to CARD-85). A draw's `p.response()` and `p.hit()` come with no
//!   points: [`fill`] measures each from an offline copy of the call (`m2_editor::measure`), at the
//!   values the picture was drawn at, after the draw, on the thread that drew it and outside the
//!   control plane's lock, and keeps each measure by what it was made from ([`Measures`]).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use m2_editor::measure::{self, Layer, Span};
use m2_editor::views::{Drawn as Part, PictureCall};
use m2_script::pictures::Drawn;
use serde_json::{Map, Value, json};

use super::{ControlPlane, Graph};

/// How long the pictures' thread waits for a lease to move before it looks anyway: for a write, an
/// agent's change, a new revision's words and asks that end.
pub const LOOK: Duration = Duration::from_millis(100);
/// How long an ask waits for its draw (CARD-64).
const WAIT: Duration = Duration::from_secs(2);

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

/// The pictures windows and agents ask for, on the control plane, and the words of every picture.
#[derive(Default)]
pub(super) struct Pictures {
    asked: Vec<Asked>,
    /// What each draw is numbered by, so a socket sends only what is newer than what it sent.
    version: u64,
    /// Every picture's words in the view (CARD-63), by module, card and picture, at the revision
    /// they were worked out for: its words, or why it can't be drawn.
    words: (u64, HashMap<(String, String, String), Value>),
    /// What each picture's words were worked out from, and what they were.
    said: HashMap<String, Value>,
}

/// One picture asked for: by its module, its card's id at a code revision, and its name.
struct Asked {
    module: String,
    node: String,
    view: String,
    /// The code revision its id names its card at.
    code: u64,
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

/// The measures kept (CARD-84) at most: the one used longest ago goes first.
const MEASURES: usize = 64;

/// The measures kept by what each was made from (CARD-84), and how many were rendered.
#[derive(Default)]
pub(super) struct Measures {
    kept: parking_lot::Mutex<std::collections::VecDeque<(String, Arc<Vec<f64>>)>>,
    rendered: std::sync::atomic::AtomicU64,
}

impl Measures {
    fn get(&self, key: &str) -> Option<Arc<Vec<f64>>> {
        let mut kept = self.kept.lock();
        let i = kept.iter().position(|(k, _)| k == key)?;
        let hit = kept.remove(i)?;
        let points = hit.1.clone();
        kept.push_back(hit);
        Some(points)
    }

    fn put(&self, key: String, points: Arc<Vec<f64>>) {
        let mut kept = self.kept.lock();
        kept.retain(|(k, _)| *k != key);
        if kept.len() >= MEASURES {
            kept.pop_front();
        }
        kept.push_back((key, points));
    }
}

/// A picture to draw, as found on the control plane.
struct Found {
    module: String,
    node: String,
    view: String,
    code: u64,
    revision: u64,
    call: PictureCall,
    /// The runner's ask, the leases' values on the view's.
    ask: m2_script::pictures::Ask,
    /// What it is drawn from: its code and its values.
    from: String,
    /// The revision it is drawn from, which a measured layer copies its call from (CARD-81).
    rev: Arc<m2_session::Revision>,
}

impl Pictures {
    fn find(&mut self, module: &str, node: &str, view: &str, code: u64) -> Option<&mut Asked> {
        self.asked.iter_mut().find(|a| a.module == module && a.node == node && a.view == view && a.code == code)
    }

    /// Keep `client`'s ask for the picture, from now.
    fn renew(&mut self, client: &str, f: &Found, now: Instant) {
        let a = match self.find(&f.module, &f.node, &f.view, f.code) {
            Some(a) => a,
            None => {
                let a = Asked {
                    module: f.module.clone(),
                    node: f.node.clone(),
                    view: f.view.clone(),
                    code: f.code,
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

    /// `client` stops asking for the picture at code revision `code` (any, when `None`), or for every
    /// one (`which` none: its window closed); asks no one keeps end.
    fn stop(&mut self, client: &str, which: Option<(&str, &str, &str, Option<u64>)>, now: Instant) {
        for a in &mut self.asked {
            if which.is_none_or(|(m, n, v, c)| a.module == m && a.node == n && a.view == v && c.is_none_or(|c| c == a.code)) {
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

    /// Store what `f` drew; the picture as its answer says it.
    fn drew(&mut self, f: &Found, picture: Value) -> Arc<Value> {
        self.version += 1;
        let version = self.version;
        let picture = Arc::new(picture);
        if let Some(a) = self.find(&f.module, &f.node, &f.view, f.code) {
            (a.from, a.drawn, a.version) = (Some(f.from.clone()), Some(picture.clone()), version);
        }
        picture
    }
}

/// The card `node` of module `module` in the view the control plane holds, and its picture `view`.
fn card<'a>(view: &'a m2_editor::GraphView, module: &str, node: &str) -> Result<&'a m2_editor::ViewNode> {
    let m = view.modules.iter().find(|m| m.key == module).with_context(|| format!("there is no track {module}"))?;
    m.nodes.iter().find(|n| n.id == node).with_context(|| format!("{module} has no card {node}: refresh the graph"))
}

/// The picture `view` of card `node` of module `module`, from what the control plane holds: the
/// accepted view's values, each live lease's on them (CARD-68).
fn found(cp: &ControlPlane, module: &str, node: &str, view: &str) -> Result<Found> {
    let open = cp.open.as_ref().context("no project is open")?;
    let (shown, accepted, token, code) = (&open.shown, &open.engine.accepted, open.engine.token, open.engine.code_rev);
    let n = card(&shown.view, module, node)?;
    let call = m2_editor::views::picture_call(accepted, n, view)
        .with_context(|| format!("{} has no picture named {view}", n.title.as_deref().unwrap_or(node)))?;
    let playing = |id: &str| cp.leases.live.iter().find(|l| l.token == token && l.control.id == id).map(|l| l.playing);
    let ask = call.at(playing);
    let from = format!("{}\n{}\n{view}\n{}", ask.key, ask.descriptor.id, ask.values);
    let rev = accepted.clone();
    Ok(Found { module: module.into(), node: node.into(), view: view.into(), code, revision: shown.revision, call, ask, from, rev })
}

/// Each measured layer of `shapes`, a part's too, filled with its points (CARD-80): measured from
/// an offline copy of `f`'s call at the values it was drawn at, or kept from a measure made from the
/// same (CARD-84). Refused in words that follow "can't measure its sound: ".
fn fill(f: &Found, measures: &Measures, shapes: &mut [Value]) -> Result<(), String> {
    let mut packages: Option<m2_runtime::PackageSet> = None;
    fill_in(f, measures, shapes, &mut packages)
}

fn fill_in(f: &Found, measures: &Measures, shapes: &mut [Value], packages: &mut Option<m2_runtime::PackageSet>) -> Result<(), String> {
    for s in shapes {
        if let Some(inner) = s.get_mut("shapes").and_then(Value::as_array_mut) {
            fill_in(f, measures, inner, packages)?;
            continue;
        }
        let layer = match s["kind"].as_str() {
            Some("response") => Layer::Response,
            Some("hit") => Layer::Hit { note: s["note"].as_u64().unwrap_or(60) as u8, seconds: s["seconds"].as_f64().unwrap_or(1.0) },
            _ => continue,
        };
        let Some(m2_script::component::ViewOf::Picture { x, .. }) =
            f.ask.descriptor.editor.views.iter().find(|v| v.name == f.view).map(|v| &v.of)
        else {
            continue;
        };
        let values: Map<String, Value> = serde_json::from_str(&f.ask.values).unwrap_or_default();
        // The revision's own kernels (PACK-9), whatever the session shows now.
        let packages = packages.get_or_insert_with(|| m2_runtime::PackageSet::default().viewing(&f.rev.compiled.view).into_owned());
        let copy = measure::copy(&f.rev, packages, &f.module, &f.node, &values, layer)?;
        let points = match measures.get(&copy.key) {
            Some(p) => p,
            None => {
                let sound = copy.render(packages)?;
                measures.rendered.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let points = Arc::new(match layer {
                    Layer::Response => measure::response(&sound, copy.rate(), Span { log: x.scale == "log", lo: x.lo, hi: x.hi }),
                    Layer::Hit { seconds, .. } => measure::hit(&sound, seconds),
                });
                measures.put(copy.key.clone(), points.clone());
                points
            }
        };
        s["points"] = json!(*points);
    }
    Ok(())
}

/// The picture as an answer says it (CARD-67): its shapes, each dot's values with the card's own
/// knobs, and its measured layers' points (CARD-80); its words; or why it can't be drawn, named by
/// its card.
fn said(f: &Found, drawn: Drawn, measures: &Measures) -> Value {
    let mut out = json!({"module": f.module, "node": f.node, "view": f.view, "component": f.ask.descriptor.id, "revision": f.revision});
    match drawn {
        Drawn::Cant(why) => out["why"] = json!(format!("{}'s picture {}.", f.call.label, why.trim_end_matches('.'))),
        Drawn::Busy => out["why"] = json!(format!("{}'s picture is still being drawn: ask again in a moment.", f.call.label)),
        Drawn::Shapes(json) => {
            let mut drew: Value = serde_json::from_str(&json).unwrap_or_default();
            let knobs: Map<String, Value> = f
                .call
                .values
                .iter()
                .map(|v| {
                    let mut k = Map::new();
                    if let Some(id) = &v.knob {
                        k.insert("knob".into(), json!(id));
                    }
                    if let Some(why) = &v.why {
                        k.insert("why".into(), json!(why));
                    }
                    (v.name.clone(), Value::Object(k))
                })
                .collect();
            if let Some(shapes) = drew["shapes"].as_array_mut() {
                knobbed(shapes, &knobs);
                if let Err(why) = fill(f, measures, shapes) {
                    out["why"] = json!(format!("{}'s picture can't measure its sound: {}.", f.call.label, why.trim_end_matches('.')));
                    return out;
                }
            }
            out["words"] = drew["words"].take();
            out["shapes"] = drew["shapes"].take();
        }
    }
    out
}

/// Each dot of `shapes`, in a part's too, its values given the card's own knobs (CARD-66). The
/// runner checked each name is one of the card's controls.
fn knobbed(shapes: &mut [Value], knobs: &Map<String, Value>) {
    for s in shapes {
        if let Some(inner) = s.get_mut("shapes").and_then(Value::as_array_mut) {
            knobbed(inner, knobs);
        }
        let Some(moves) = s.get_mut("moves").and_then(Value::as_object_mut) else { continue };
        for m in moves.values_mut() {
            let k = m["name"].as_str().and_then(|n| knobs.get(n)).and_then(Value::as_object).cloned().unwrap_or_default();
            for (f, x) in k {
                m[f] = x;
            }
        }
    }
}

impl Graph {
    /// Start, keep or stop picture `view` of card `node` of module `module` (`(module, node, view)`), by its id in the view of
    /// `revision`, for `client` (`POST /api/graph/picture`, CARD-67). Not a write: it works read-only
    /// too. Refused when the song's code changed since `revision` (the id may name another call
    /// now). Answers `{picture: {module, node, view, component, revision, words, shapes}}` drawn
    /// from what plays now, or with `why` when it can't be drawn; `{}` for a stop.
    pub fn picture(&self, token: u64, revision: u64, client: &str, (module, node, view): (&str, &str, &str), on: bool) -> Result<Value> {
        let found = {
            let mut cp = self.inner.control.lock();
            let open = cp.open.as_ref().filter(|o| o.engine.token == token).context("the open song changed; refresh it first")?;
            let code = open.revs.code_rev(revision);
            let now = Instant::now();
            if !on {
                cp.pictures.stop(client, Some((module, node, view, code)), now);
                return Ok(json!({}));
            }
            if code != Some(open.engine.code_rev) {
                bail!(
                    "the song's code changed since revision {revision}, so {node} may name another call now: ask again by the card's id in the view now"
                );
            }
            let found = found(&cp, module, node, view)?;
            cp.pictures.renew(client, &found, now);
            // Nothing changed since it was drawn: the picture drawn (review G1).
            if let Some(a) = cp.pictures.find(module, node, view, found.code)
                && a.from.as_ref() == Some(&found.from)
                && let Some(d) = &a.drawn
            {
                return Ok(json!({"picture": **d}));
            }
            found
        };
        let drawn = self.inner.runner().draw_within(found.ask.clone(), Some(WAIT));
        let busy = drawn == Drawn::Busy;
        let picture = said(&found, drawn, &self.inner.measures);
        let mut cp = self.inner.control.lock();
        let picture = match busy {
            true => Arc::new(picture),
            false => cp.pictures.drew(&found, picture),
        };
        Ok(json!({"picture": *picture}))
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

    /// One look (CARD-68): each picture asked for whose values or code changed since it was drawn,
    /// drawn again; and each window's pictures, when any was.
    pub fn pictures_frame(&self) -> Option<PicturesOut> {
        let (token, todo) = {
            let mut cp = self.inner.control.lock();
            cp.pictures.expire(Instant::now());
            if cp.pictures.asked.is_empty() {
                return None;
            }
            let open = cp.open.as_ref()?;
            let (token, code) = (open.engine.token, open.engine.code_rev);
            // An ask by an id of older code waits: its window asks again by the card's id now.
            let todo: Vec<Found> = cp
                .pictures
                .asked
                .iter()
                .filter(|a| a.code == code)
                .filter_map(|a| found(&cp, &a.module, &a.node, &a.view).ok().filter(|f| a.from.as_ref() != Some(&f.from)))
                .collect();
            (token, todo)
        };
        if todo.is_empty() {
            return None;
        }
        for f in &todo {
            let drawn = self.inner.runner().draw_within(f.ask.clone(), Some(WAIT));
            if drawn == Drawn::Busy {
                continue;
            }
            let picture = said(f, drawn, &self.inner.measures);
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

    /// Every picture's words in the accepted view (CARD-63), worked out by the runner when the view
    /// is new: whether any changed, so windows read the view again.
    pub fn pictures_words(&self) -> bool {
        let (revision, todo) = {
            let mut cp = self.inner.control.lock();
            let Some(open) = cp.open.as_ref() else { return false };
            let revision = open.shown.revision;
            if cp.pictures.words.0 == revision || revision != open.engine.revision {
                return false;
            }
            let mut todo = vec![];
            for m in &open.shown.view.modules {
                for n in &m.nodes {
                    for v in n.views.iter().filter(|v| matches!(v.of, Part::Picture { .. })) {
                        if let Some(call) = m2_editor::views::picture_call(&open.engine.accepted, n, &v.name) {
                            let ask = call.at(|_| None);
                            let from = format!("{}\n{}\n{}\n{}", ask.key, ask.descriptor.id, ask.view, ask.values);
                            todo.push(((m.key.clone(), n.id.clone(), v.name.clone()), call, ask, from));
                        }
                    }
                }
            }
            if todo.is_empty() {
                let had = !cp.pictures.words.1.is_empty();
                cp.pictures.words = (revision, HashMap::new());
                return had;
            }
            (revision, todo)
        };
        let mut words = HashMap::new();
        let mut whole = true;
        for (at, call, ask, from) in todo {
            let known = self.inner.control.lock().pictures.said.get(&from).cloned();
            let said = match known {
                Some(s) => s,
                None => {
                    let s = match self.inner.runner().draw_within(ask, Some(WAIT)) {
                        Drawn::Shapes(json) => json!({"words": serde_json::from_str::<Value>(&json).ok().map(|d| d["words"].clone())}),
                        Drawn::Cant(why) => {
                            json!({"words": null, "why": format!("{}'s picture {}.", call.label, why.trim_end_matches('.'))})
                        }
                        // Still being drawn: its words come at a later look.
                        Drawn::Busy => {
                            whole = false;
                            continue;
                        }
                    };
                    let mut cp = self.inner.control.lock();
                    if cp.pictures.said.len() > 4096 {
                        cp.pictures.said.clear();
                    }
                    cp.pictures.said.insert(from, s.clone());
                    s
                }
            };
            words.insert(at, said);
        }
        let mut cp = self.inner.control.lock();
        let changed = cp.pictures.words.1 != words;
        cp.pictures.words = (if whole { revision } else { 0 }, words);
        changed
    }

    /// How many measures were rendered, not kept (CARD-84): what a test reads.
    #[doc(hidden)]
    pub fn measures_rendered(&self) -> u64 {
        self.inner.measures.rendered.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `client`'s window closed: the pictures it asked for end (CARD-67).
    pub(super) fn pictures_gone(&self, client: &str) {
        self.inner.control.lock().pictures.stop(client, None, Instant::now());
    }
}

/// Put each picture's words in the state's view (CARD-63), when they were worked out for the
/// revision it shows: `words` stays null until then.
pub(super) fn put_words(cp: &Pictures, revision: u64, view: &mut Value) {
    if cp.words.0 != revision {
        return;
    }
    for m in view["modules"].as_array_mut().into_iter().flatten() {
        let key = m["key"].as_str().unwrap_or_default().to_string();
        for n in m["nodes"].as_array_mut().into_iter().flatten() {
            let id = n["id"].as_str().unwrap_or_default().to_string();
            for v in n["views"].as_array_mut().into_iter().flatten() {
                if v["kind"] != "picture" {
                    continue;
                }
                let at = (key.clone(), id.clone(), v["name"].as_str().unwrap_or_default().to_string());
                if let Some(said) = cp.words.1.get(&at).and_then(Value::as_object) {
                    for (k, x) in said {
                        v[k] = x.clone();
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pictures_tests.rs"]
mod tests;

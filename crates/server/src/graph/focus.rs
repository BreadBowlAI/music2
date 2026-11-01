//! What the musician looks at, and what an agent points at (AGENT-6): attention, not song.
//!
//! The page posts its focus record 200 ms after the last change to it (`POST /api/focus`, the
//! page key only); `m2 point` posts a pointer (`POST /api/point`). Both live here in memory, beside
//! the code editor's unsaved list: never in a file, a revision or an undo step. The newest post
//! wins, there is one pointer at a time, and both go when the song closes or another opens. Every
//! window and agent hears each as an event (`focus`, `point`). Neither is a write, so both work in
//! a read-only window.

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Graph, Inner, Published};

/// The dock's view.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum View {
    Device,
    Notes,
    Mix,
    Code,
    /// The beat grid (KIT-10).
    Beat,
}

/// A clip, by its track's key, its lane's key and its id: as `m2 clips` names it, `bass/notes:c3`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ClipAt {
    pub track: String,
    pub lane: String,
    pub id: String,
}

/// A note, by what it is (notes have no ids): its pitch (a MIDI note number), and where it starts
/// and how long it is in its clip, in beats.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Note {
    pub pitch: u8,
    pub start: f64,
    pub length: f64,
}

/// The roll's selected notes, and the clip they are in.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Notes {
    pub clip: ClipAt,
    pub notes: Vec<Note>,
}

/// The code view's file and its cursor's line (from 1).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Code {
    pub file: String,
    pub line: u32,
}

/// The focus record, as the page posts it (the design's 5.9): what the musician looks at, read
/// against the revision the page drew. Fields the page leaves out are null.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Focus {
    /// The open song's token: a focus for another song is refused.
    pub session: u64,
    /// The revision the page drew: ids and clips are read against it.
    #[serde(default)]
    pub revision: Option<u64>,
    #[serde(default)]
    pub view: Option<View>,
    /// The selected track, by key.
    #[serde(default)]
    pub track: Option<String>,
    /// The timeline's picked clips.
    #[serde(default)]
    pub clips: Vec<ClipAt>,
    /// A marquee's bars, `[start, end)` in beats.
    #[serde(default)]
    pub range: Option<[f64; 2]>,
    #[serde(default)]
    pub notes: Option<Notes>,
    /// The knob last touched, by the view's id.
    #[serde(default)]
    pub knob: Option<String>,
    /// The beats the timeline shows, `[start, end)`.
    #[serde(default)]
    pub seen: Option<[f64; 2]>,
    #[serde(default)]
    pub code: Option<Code>,
}

/// What is kept: the page's focus record and one pointer, each for the song its token names.
#[derive(Default)]
pub(super) struct Attention {
    focus: Option<Focus>,
    /// The pointer as it is shown, with the song it is for.
    point: Option<(u64, Value)>,
}

impl Inner {
    /// The pointer shown in song `token`, or null.
    pub(super) fn pointer(&self, token: u64) -> Value {
        self.attention.lock().point.as_ref().filter(|(t, _)| *t == token).map_or(Value::Null, |(_, p)| p.clone())
    }

    /// The song closed, or another opened: what was looked at and pointed at goes with it.
    pub(super) fn forget_attention(&self) {
        *self.attention.lock() = Attention::default();
    }
}

impl Graph {
    /// The page's focus record for the open song (AGENT-6): kept in memory, the newest post
    /// winning, and told to every window and agent. Answers it as [`Graph::focus`] serves it.
    pub fn post_focus(&self, focus: Focus) -> Result<Value> {
        let session = focus.session;
        {
            // Under the control plane's lock, so a song closing or opening meanwhile never keeps it.
            let cp = self.inner.control.lock();
            let token = cp.open.as_ref().map(|p| p.engine.token).context("no project is open")?;
            if focus.session != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            self.inner.attention.lock().focus = Some(focus);
        }
        let served = self.focus();
        self.inner.tell(json!({"t": "focus", "session": session, "focus": served}));
        Ok(served)
    }

    /// What the musician looks at, as `GET /api/focus` and `m2 focus` say it: the page's last
    /// record (its fields null until it posts one), with what the server knows: `playing`, `beat`
    /// and `loop`, the code editor's `unsaved` files, what Compare compares (`comparing`), and the
    /// MIDI keyboard's target (`midi`). Null with no song open.
    pub fn focus(&self) -> Value {
        let Some(token) = self.inner.control.lock().open.as_ref().map(|p| p.engine.token) else { return Value::Null };
        let posted = self.inner.attention.lock().focus.clone().filter(|f| f.session == token);
        let mut v = json!(posted.unwrap_or(Focus { session: token, ..Focus::default() }));
        let status = self.status();
        v["playing"] = json!(status.playing);
        v["beat"] = json!(status.beat);
        v["loop"] = json!(status.looping);
        v["unsaved"] = json!(self.unsaved());
        v["comparing"] = self.inner.compared(token);
        v["midi"] = self.midi_state()["target"].clone();
        v
    }

    /// Show a pointer in song `token`: `at` is its target and the words said there, as plain text
    /// of at most 80 characters (AGENT-10); None clears it (the page's OK, `m2 point --clear`).
    /// The target is checked against the accepted revision. One at a time; every window and agent
    /// hears it. Not a write: it changes no file, revision or step, and never the musician's view
    /// or selection. Answers the pointer shown (null once cleared).
    pub fn point(&self, token: u64, at: Option<(&str, &str)>) -> Result<Value> {
        let by = match crate::caller::now() {
            crate::caller::Caller::Agent { name, .. } => json!(name),
            crate::caller::Caller::Page => Value::Null,
        };
        let shown = {
            let cp = self.inner.control.lock();
            let p = cp.open.as_ref().context("no project is open")?;
            if p.engine.token != token {
                bail!("the open project changed; refresh the graph before editing");
            }
            let shown = match at {
                None => Value::Null,
                Some((target, words)) => {
                    let mut v = target_of(target, p)?;
                    v["session"] = json!(token);
                    v["target"] = json!(target);
                    v["words"] = json!(crate::caller::words(words));
                    v["by"] = by;
                    v
                }
            };
            self.inner.attention.lock().point = (!shown.is_null()).then(|| (token, shown.clone()));
            shown
        };
        self.inner.tell(json!({"t": "point", "session": token, "point": shown}));
        Ok(shown)
    }

    /// The pointer shown in the open song, or null.
    pub fn pointer(&self) -> Value {
        match self.inner.control.lock().open.as_ref().map(|p| p.engine.token) {
            Some(token) => self.inner.pointer(token),
            None => Value::Null,
        }
    }
}

/// What a pointer's target names in the accepted revision `p` publishes: its `kind` and its parts.
/// `track:bass`, `clip:bass/notes:c3`, `lane:bass/cutoff`, `knob:<id>` (the view's id, with its
/// track), `bars:9-16` or `bars:9` (bars counted from 1, with the beats they span). One the song
/// hasn't is refused, saying what it is.
fn target_of(target: &str, p: &Published) -> Result<Value> {
    let (kind, what) = target.split_once(':').ok_or_else(|| {
        anyhow!("a pointer's target is track:<track>, clip:<track>/<lane>:<id>, lane:<track>/<lane>, knob:<id> or bars:<from>-<to>, not {target}")
    })?;
    let timeline = &p.shown.timeline;
    let track = |key: &str| -> Result<()> {
        let known = timeline.tracks.iter().any(|t| t.module == key)
            || p.shown.state["view"]["modules"].as_array().into_iter().flatten().any(|m| m["key"] == key);
        if !known {
            bail!("the song has no track {key}");
        }
        Ok(())
    };
    let lane = |at: &str| -> Result<(String, String)> {
        let (t, l) = at.split_once('/').ok_or_else(|| anyhow!("a lane is <track>/<lane>, not {at}"))?;
        track(t)?;
        if timeline.lane(t, l).is_none() {
            bail!("track {t} has no lane {l}");
        }
        Ok((t.to_string(), l.to_string()))
    };
    Ok(match kind {
        "track" => {
            track(what)?;
            json!({"kind": "track", "track": what})
        }
        "lane" => {
            let (t, l) = lane(what)?;
            json!({"kind": "lane", "track": t, "lane": l})
        }
        "clip" => {
            let (at, id) = what.rsplit_once(':').ok_or_else(|| anyhow!("a clip is <track>/<lane>:<id>, not {what}"))?;
            let (t, l) = lane(at)?;
            let json = &timeline.lane(&t, &l).expect("checked").json;
            if !json["clips"].as_array().into_iter().flatten().any(|c| c["id"] == id) {
                bail!("{t}/{l} has no clip {id}");
            }
            json!({"kind": "clip", "track": t, "lane": l, "clip": id})
        }
        "knob" => {
            if !p.engine.controls.contains_key(what) {
                bail!("the song has no knob {what} now (m2 controls lists them)");
            }
            // The track whose device shows it, and its name there, "Bass Cutoff": while the dock
            // doesn't show it, the note waits beside that track's header, naming it.
            let found = p.shown.state["view"]["modules"].as_array().into_iter().flatten().find_map(|m| {
                let nodes = m["nodes"].as_array()?;
                let k = nodes.iter().flat_map(|n| n["knobs"].as_array().into_iter().flatten()).find(|k| k["id"] == what)?;
                let label = [m["label"].as_str(), k["label"].as_str()].into_iter().flatten().collect::<Vec<_>>().join(" ");
                Some((m["key"].clone(), label))
            });
            let (module, label) = found.unzip();
            json!({"kind": "knob", "knob": what, "track": module, "label": label})
        }
        "bars" => {
            let bar = |s: &str| s.trim().parse::<u64>().ok().filter(|n| *n >= 1);
            let (from, to) = match what.split_once('-') {
                Some((a, b)) => (bar(a), bar(b)),
                None => (bar(what), bar(what)),
            };
            let (Some(from), Some(to)) = (from, to) else { bail!("bars are counted from 1: bars:9-16 or bars:9, not bars:{what}") };
            if to < from {
                bail!("bars:{what} ends before it starts");
            }
            let arrangement = &p.engine.accepted.compiled.data.arrangement;
            // Bars past the song's end aren't the song's: as many as start before it.
            let end = timeline.head.get("end").and_then(Value::as_f64).unwrap_or(0.0);
            let mut bars = 0;
            while bars < 100_000 && m2_editor::clips::bar_start(arrangement, bars) < end - 1e-9 {
                bars += 1;
            }
            if to > bars {
                let one = if bars == 1 { "bar" } else { "bars" };
                bail!("the song has {bars} {one}, so bars:{what} is past its end");
            }
            let beats = [m2_editor::clips::bar_start(arrangement, from - 1), m2_editor::clips::bar_start(arrangement, to)];
            json!({"kind": "bars", "bars": [from, to], "beats": beats})
        }
        _ => bail!("a pointer's target is track:, clip:, lane:, knob: or bars:, not {kind}:"),
    })
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;

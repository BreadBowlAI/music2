//! The Compare key, V (D2; AGENT-3): hear before and after the newest change an agent or another
//! program made. Compare only plays: it never writes the song.
//!
//! V names the newest step (`POST /api/graph/compare` `{session, step, before: true}`): the server
//! builds Before, the song with that step taken back, in memory ([`m2_session::Session::undone`]:
//! its undo, checked and built, nothing written), and each settle plays it in place of the accepted
//! revision while the pill says Before and the step is still undo's newest ([`hearing`]). V again
//! plays After, the song as it is. Its files, undo and the view stay at After all the while, so a
//! quit or a crash on Before leaves the song as it was. While the pill is open every agent's write
//! is busy (`busy::held`), so no agent's step changes what is compared. A V naming another step
//! than the one compared, or than the newest when it opens, is refused ([`Stale`], `NotNewest`).
//!
//! It closes (each change told as a `compare` event, numbered so a window keeps the newest):
//! - Esc keeps what plays (`closed`): on Before, the step is undone then, as the page's Undo does;
//! - at After, writing nothing: any other action of the musician's (`closed`); undo's newest step
//!   no longer the one compared, after a write with the page key, an undo, a redo, a plugin
//!   window's save (`changed`); the socket of the window that opened it closing (`page`); the song
//!   closing or another opening (`song`); a batch the watcher takes in (`outside`); Before that
//!   couldn't be got ready to play (`failed`).
//!
//! Everything here runs on the session's actor, so its events follow the steps before them.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result, anyhow, bail};
use m2_session::{Revision, Source};
use serde_json::{Value, json};

use super::actor::{Actor, Open};
use super::{Graph, Inner, steps};

/// What the pill compares: the step, by its id, in song `token`, and whether it plays After (the
/// song as it is) or Before (`before`, the song with the step taken back, built in memory); `name`
/// as Undo says it, "Claude: brighter hats". `n` numbers the last change told of it; `client` is the
/// window that opened it.
#[derive(Clone)]
pub(crate) struct Comparing {
    pub(crate) token: u64,
    pub(crate) step: u64,
    pub(crate) after: bool,
    pub(crate) name: String,
    pub(crate) n: u64,
    client: Option<String>,
    before: Arc<Revision>,
}

impl Inner {
    /// What the pill compares in song `token`: `{step, after, name, n}`, or null while it is closed.
    pub(super) fn compared(&self, token: u64) -> Value {
        match self.comparing.lock().as_ref().filter(|c| c.token == token) {
            Some(c) => json!({"step": c.step, "after": c.after, "name": c.name, "n": c.n}),
            None => Value::Null,
        }
    }
}

/// What the engine plays in place of song `o`'s accepted revision: Compare's Before, while the pill
/// says Before and its step is still undo's newest. None: the accepted revision.
pub(super) fn hearing(inner: &Inner, o: &Open) -> Option<Arc<Revision>> {
    let c = inner.comparing.lock().as_ref().filter(|c| c.token == o.token && !c.after)?.clone();
    (o.session.step_names().0?.id == c.step).then_some(c.before)
}

impl Actor {
    /// Tell every window and agent the pill as `c` has it (`why` None: open), numbered after every
    /// change told before it. The number.
    fn told_compare(&mut self, c: &Comparing, after: bool, why: Option<&str>) -> u64 {
        let n = self.inner.compare_n.fetch_add(1, Ordering::Relaxed) + 1;
        self.tell_in_order(json!({
            "t": "compare",
            "session": c.token,
            "n": n,
            "step": c.step,
            "open": why.is_none(),
            "after": after,
            "name": c.name,
            "why": why,
        }));
        n
    }

    /// Open the pill on `c`, or flip it: told, then played. Before that can't be got ready to play
    /// closes it again (`failed`), and the answer says why.
    fn compare_now(&mut self, mut c: Comparing) -> Result<()> {
        c.n = self.told_compare(&c, c.after, None);
        let before = (!c.after).then(|| c.before.clone());
        *self.inner.comparing.lock() = Some(c);
        let settled = self.settle_as(None, false);
        let Some(before) = before else { return Ok(()) };
        let plays = self.open.as_ref().and_then(|o| o.plays.as_ref()).is_some_and(|p| Arc::ptr_eq(&p.revision, &before));
        if !plays {
            self.end_compare("failed");
            self.settle_as(None, false);
            let why = settled.failed.map(|e| format!(" (In detail: {e})")).unwrap_or_default();
            bail!("Before couldn't be got ready to play, so Compare closed.{why}");
        }
        Ok(())
    }

    /// The pill closes, for `why`, if it is open, at After: nothing was written, so the song is as
    /// it is. Whether Before played: then the caller settles, so the song as it is plays again.
    pub(super) fn end_compare(&mut self, why: &str) -> bool {
        let Some(c) = self.inner.comparing.lock().take() else { return false };
        self.told_compare(&c, true, Some(why));
        !c.after
    }

    /// After every job: the pill stays open only while what it compares is undo's newest step.
    /// Anything else changed the history, and it closes at After.
    pub(super) fn kept_compare(&mut self) {
        let Some(c) = self.inner.comparing.lock().clone() else { return };
        let newest = |o: &Open| o.session.step_names().0.is_some_and(|t| t.id == c.step);
        let kept = self.open.as_ref().filter(|o| o.token == c.token).is_some_and(newest);
        if !kept && self.end_compare("changed") {
            self.settle_as(None, false);
        }
    }
}

impl Graph {
    /// V (D2): play step `step` of song `token` Before (`before`) or After. With the pill closed, it
    /// opens on Before, only while `step` is undo's newest and an agent or another program made it
    /// (`NotNewest`, [`steps::NotElsewhere`]); Before is built then, and nothing is written. While
    /// it is open, only for the step it compares ([`Stale`]); the side that plays already changes
    /// nothing. `client` is the window asking: its socket closing ends what it opened. Answers
    /// `{comparing}`.
    pub fn compare(&self, token: u64, step: u64, before: bool, client: Option<String>) -> Result<Value> {
        self.call(move |a| {
            if a.caller.is_agent() {
                bail!("agents can't do this: Compare is the musician's (V in the app)");
            }
            let o = a.open.as_mut().context("no project is open")?;
            o.check_token(token)?;
            let now = a.inner.comparing.lock().clone().filter(|c| c.token == token);
            match now {
                Some(c) if c.step != step => return Err(Stale { comparing: a.inner.compared(token) }.into()),
                Some(c) if c.after != before => {}
                Some(c) => a.compare_now(Comparing { after: !before, ..c })?,
                None if !before => return Err(Stale { comparing: Value::Null }.into()),
                None => {
                    let newest = o.session.step_names().0;
                    let took = steps::named(newest.as_ref(), &o.timeline);
                    match newest {
                        Some(t) if t.id == step && t.source != Source::Page => {}
                        Some(t) if t.id == step => return Err(steps::NotElsewhere { newest: took }.into()),
                        _ => return Err(steps::NotNewest { redo: false, newest: took }.into()),
                    }
                    let name = steps::said(&took).unwrap_or_default();
                    let before = o.session.undone().map_err(|e| anyhow!("V can't compare {name} now: {e}"))?;
                    a.compare_now(Comparing { token, step, after: false, name, n: 0, client, before })?;
                }
            }
            Ok(json!({"comparing": a.inner.compared(token)}))
        })
    }

    /// Close the pill. With `keep` (Esc) it keeps what plays: on Before, the step it compares is
    /// undone now, as the page's Undo does, and the answer is the undo's. Else, and on After, at
    /// After: nothing is written.
    pub fn close_compare(&self, token: u64, keep: bool) -> Result<Value> {
        self.call(move |a| {
            a.open.as_ref().context("no project is open")?.check_token(token)?;
            let now = a.inner.comparing.lock().clone().filter(|c| c.token == token);
            let Some(c) = now else { return Ok(json!({"comparing": null})) };
            if keep && !c.after {
                // The musician chose Before: one undo step of theirs, which Redo brings back.
                *a.inner.comparing.lock() = None;
                return match steps::stepped(a, token, false, Some(c.step)) {
                    Ok(mut v) => {
                        a.told_compare(&c, false, Some("closed"));
                        v["comparing"] = Value::Null;
                        Ok(v)
                    }
                    Err(e) => {
                        a.told_compare(&c, true, Some("closed"));
                        a.settle_as(None, false);
                        Err(e)
                    }
                };
            }
            if a.end_compare("closed") {
                a.settle_as(None, false);
            }
            Ok(json!({"comparing": null}))
        })
    }

    /// A page's socket closed: `client` is its window, and `others` whether another page's socket
    /// still listens. The pill closes, at After, when it was that window's, or when no page is left.
    pub fn page_gone(&self, client: Option<&str>, others: bool) {
        let client = client.map(str::to_string);
        let _ = self.call(move |a| {
            let its = a.inner.comparing.lock().as_ref().is_some_and(|c| !others || (c.client.is_some() && c.client == client));
            if its && a.end_compare("page") {
                a.settle_as(None, false);
            }
            Ok(())
        });
    }

    /// AGENT-3: whether the caller may close the open song or open another (a new one too). An
    /// agent may not while a take records or the Compare pill is open: busy, as its writes are.
    pub fn may_end_song(&self) -> Result<()> {
        self.call(|a| a.arrive(None))
    }

    /// What the pill compares in the open song, or null.
    pub fn comparing(&self) -> Value {
        match self.inner.control.lock().open.as_ref().map(|p| p.engine.token) {
            Some(token) => self.inner.compared(token),
            None => Value::Null,
        }
    }
}

/// A V that names another step than the one the pill compares, or asks for After while it is
/// closed (D2): nothing changed. `comparing` is what it compares, as the state says it (null: it
/// is closed).
#[derive(Debug)]
pub struct Stale {
    pub comparing: Value,
}

impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.comparing["name"].as_str() {
            Some(name) => write!(f, "Nothing changed: Compare compares {name} now."),
            None => write!(f, "Nothing changed: Compare isn't open now. V opens it on the newest change an agent or another program made."),
        }
    }
}

impl std::error::Error for Stale {}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;

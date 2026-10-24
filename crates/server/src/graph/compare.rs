//! The Compare key, V (D2; AGENT-3): hear before and after the newest change an agent or another
//! program made.
//!
//! V sends an undo of that step with `compare: true`: the server undoes it and keeps what the pill
//! compares, on Before. V again sends a redo of the same step: After. Each flip is an undo or a
//! redo with the page key, so it is heard as any undo is. While the pill is open every agent's
//! write is busy (`busy::held`), so no agent's step can clear redo and take After away. It closes
//! (each end told as a `compare` event, with why) when:
//! - the page closes it: Esc, or any other action of the musician's (`closed`);
//! - undo's newest step (on After) or redo's (on Before) is no longer the one compared: any other
//!   write with the page key, an undo or a redo that isn't V's, a plugin window's save (`changed`);
//! - the page's socket closes (`page`), or the song closes or another opens (`song`);
//! - the watcher takes in a batch: a file written directly can land while the pill is open, and
//!   the pill says a file changed outside the app (`outside`).
//!
//! It keeps what plays: on Before, the change stays undone, and Redo has it until the next change.
//! Everything here runs on the session's actor, so its events follow the steps before them.

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::actor::Actor;
use super::{Graph, Inner, steps};

/// What the pill compares: the step, by its id, in song `token`, and whether it shows After (the
/// step redone) or Before (undone); `name` as Undo says it, "Claude: brighter hats".
#[derive(Clone, Debug)]
pub(crate) struct Comparing {
    pub(crate) token: u64,
    pub(crate) step: u64,
    pub(crate) after: bool,
    pub(crate) name: String,
}

impl Inner {
    /// What the pill compares in song `token`: `{step, after, name}`, or null while it is closed.
    pub(super) fn compared(&self, token: u64) -> Value {
        match self.comparing.lock().as_ref().filter(|c| c.token == token) {
            Some(c) => json!({"step": c.step, "after": c.after, "name": c.name}),
            None => Value::Null,
        }
    }
}

impl Actor {
    /// The pill closes, for `why`, if it is open: every window and agent hears it.
    pub(super) fn end_compare(&mut self, why: &str) {
        let Some(c) = self.inner.comparing.lock().take() else { return };
        self.tell_in_order(json!({"t": "compare", "session": c.token, "step": c.step, "open": false, "why": why}));
    }

    /// After every job: the pill stays open only while what it compares is where V left it, undo's
    /// newest step on After, redo's on Before. Anything else changed the history.
    pub(super) fn kept_compare(&mut self) {
        let Some(c) = self.inner.comparing.lock().clone() else { return };
        let kept = self.open.as_ref().filter(|o| o.token == c.token).is_some_and(|o| {
            let (undo, redo) = o.session.step_names();
            let top = if c.after { undo } else { redo };
            top.is_some_and(|t| t.id == c.step)
        });
        if !kept {
            self.end_compare("changed");
        }
    }
}

impl Graph {
    /// V (D2): undo step `step` of song `token`, or with `redo` redo it, as Undo and Redo do, only
    /// while it is the newest (for redo, the newest undone); the pill then compares it, Before or
    /// After. The answer is the undo's or the redo's.
    pub fn compare(&self, token: u64, step: u64, redo: bool) -> Result<Value> {
        self.call(move |a| {
            let o = a.open.as_ref().context("no project is open")?;
            o.check_token(token)?;
            let (undo, again) = o.session.step_names();
            let top = if redo { again } else { undo };
            let name = steps::said(&steps::named(top.as_ref(), &o.timeline)).unwrap_or_default();
            let v = steps::stepped(a, token, redo, Some(step), true)?;
            *a.inner.comparing.lock() = Some(Comparing { token, step, after: redo, name });
            Ok(v)
        })
    }

    /// Close the pill (Esc, or any other action of the musician's), keeping what plays.
    pub fn close_compare(&self, token: u64) -> Result<Value> {
        self.call(move |a| {
            a.open.as_ref().context("no project is open")?.check_token(token)?;
            a.end_compare("closed");
            Ok(Value::Null)
        })
    }

    /// The page's socket closed: the pill closes with it.
    pub fn page_gone(&self) {
        let _ = self.call(|a| {
            a.end_compare("page");
            Ok(())
        });
    }

    /// What the pill compares in the open song, or null.
    pub fn comparing(&self) -> Value {
        match self.inner.control.lock().open.as_ref().map(|p| p.engine.token) {
            Some(token) => self.inner.compared(token),
            None => Value::Null,
        }
    }
}

#[cfg(test)]
#[path = "compare_tests.rs"]
mod tests;

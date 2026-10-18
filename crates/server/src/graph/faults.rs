//! Faults found while sound is made, told (RENDER-25, DIAG-4). Every part reports into the app's
//! one ring ([`m2_runtime::fault::Faults`], in [`Inner`]): the audio thread a sound that stopped
//! (`sound.stopped`) or a plugin that failed (`plugin.failed`), the actor a plugin host that ended
//! (`plugin.crashed`). Every 300 ms the server's notices thread reads it here ([`Told::hear`]).
//! Each fault is named by the [`Namer`] of the plan it came from (the ex plan's D2), listed among
//! the open song's problems until its part plays again, and toasted once (D3).

use std::collections::HashSet;
use std::sync::Arc;

use m2_editor::faults::Namer;
use m2_runtime::fault::{Code, Fault, Faults, Subject};
use m2_song::Diagnostic;
use serde_json::Value;

use super::Inner;
use super::actor::{Open, Plays};

/// What the session told of the ring: the namers it names with, and the problems it lists.
#[derive(Default)]
pub(super) struct Told {
    /// The session these are for.
    token: u64,
    /// The namers of the plan sent last and the one before, newest first. A fault from an older
    /// plan is dropped: its part plays again in the plan that plays now.
    namers: [Option<Arc<Namer>>; 2],
    /// What is listed now, oldest first.
    live: Vec<Live>,
    /// Each part told by a toast, by code, in the open song: once each (D3).
    toasted: HashSet<(&'static str, String)>,
    /// The ring's lost faults when it was last read.
    lost: u64,
    /// The list changed since the notices thread last read the ring.
    changed: bool,
}

/// A problem listed: the plan, the code and the parts it is about, and how it is said.
struct Live {
    plan: u64,
    code: Code,
    parts: Vec<Subject>,
    problem: Diagnostic,
    /// Faults the ring lost after it, told as more (D3).
    more: u64,
}

impl Told {
    /// `plays` was sent to the engine in session `token`: a new plan, whose parts all play.
    /// What was listed goes; a fault of the plan before it is still named.
    pub(super) fn sent(&mut self, token: u64, namer: Namer) {
        if token != self.token {
            (self.token, self.toasted, self.namers) = (token, HashSet::new(), [None, None]);
        }
        self.namers = [Some(Arc::new(namer)), self.namers[0].take()];
        self.changed |= !self.live.is_empty();
        self.live.clear();
    }

    /// The song closed: nothing is listed or named.
    pub(super) fn closed(&mut self) {
        self.changed |= !self.live.is_empty();
        (self.namers, self.live) = ([None, None], vec![]);
    }

    /// Stop All Sound: each sound that stopped plays again (RENDER-10), so it isn't listed. A
    /// plugin that failed isn't started again by it.
    pub(super) fn panic(&mut self) {
        let before = self.live.len();
        self.live.retain(|l| !matches!(l.code, Code::Stopped | Code::OutOfMemory));
        self.changed |= self.live.len() != before;
    }

    /// Read every fault in `faults` and name each. One of the plan sent last is listed; one of
    /// the plan before is only told, as its part plays again in the plan sent last (an activation
    /// resets a sound that stopped, and makes a plugin that failed again). Returns what to toast,
    /// a part's first fault of a code in the open song, and whether the list changed.
    pub(super) fn hear(&mut self, faults: &Faults) -> (Vec<String>, bool) {
        let mut before: Vec<Live> = vec![];
        let mut toast: Vec<(bool, usize)> = vec![];
        for f in std::iter::from_fn(|| faults.take()) {
            let mine = |n: &Option<Arc<Namer>>| n.as_ref().is_some_and(|n| n.plan() == f.plan);
            let Some(newest) = self.namers.iter().position(mine).map(|i| i == 0) else { continue };
            let namer = self.namers[usize::from(!newest)].clone().expect("found above");
            let list = if newest { &mut self.live } else { &mut before };
            let Some(at) = add(list, &namer, f) else { continue };
            self.changed |= newest;
            if let Some(part) = namer.part(f.subject)
                && self.toasted.insert((f.code.diag(), part))
                && !toast.contains(&(newest, at))
            {
                toast.push((newest, at));
            }
        }
        let lost = faults.lost();
        if lost > self.lost
            && let Some(last) = self.live.last_mut()
        {
            last.more += lost - self.lost;
            self.changed = true;
        }
        self.lost = lost;
        let said = toast.into_iter().map(|(newest, i)| said(if newest { &self.live[i] } else { &before[i] })).collect();
        (said, std::mem::take(&mut self.changed))
    }

    /// The problems listed now for session `token`, oldest first, as the news carries them.
    pub(super) fn problems(&self, token: u64) -> Vec<Value> {
        match token == self.token {
            true => self.live.iter().map(|l| serde_json::to_value(Diagnostic { message: said(l), ..l.problem.clone() }).expect("JSON")).collect(),
            false => vec![],
        }
    }
}

/// Add fault `f` to `list`, named by `namer`: a new problem, or one more plugin of a plugin host
/// that ended, as the plugins of one host are one problem. Where it is now; None when it is listed
/// already (a part reports once while it is silent) or isn't of this plan's graph.
fn add(list: &mut Vec<Live>, namer: &Namer, f: Fault) -> Option<usize> {
    if list.iter().any(|l| l.plan == f.plan && l.code == f.code && l.parts.contains(&f.subject)) {
        return None;
    }
    let host = matches!(f.code, Code::PluginCrashed | Code::PluginHung);
    match list.iter().position(|l| host && l.plan == f.plan && l.code == f.code) {
        Some(i) => {
            let l = &mut list[i];
            let parts: Vec<Subject> = l.parts.iter().copied().chain([f.subject]).collect();
            l.problem = namer.problem(f.code, &parts)?;
            l.parts = parts;
            Some(i)
        }
        None => {
            let problem = namer.problem(f.code, &[f.subject])?;
            list.push(Live { plan: f.plan, code: f.code, parts: vec![f.subject], problem, more: 0 });
            Some(list.len() - 1)
        }
    }
}

/// How `l` is said: its words, and the faults lost after it.
fn said(l: &Live) -> String {
    match l.more {
        0 => l.problem.message.clone(),
        1 => format!("{} And 1 more problem.", l.problem.message),
        n => format!("{} And {n} more problems.", l.problem.message),
    }
}

/// `plays` was sent to the engine: the session names its faults from now on. A plugin left silent
/// (it failed again soon after it was made again, HOST-5) plays on silent in it: it is listed
/// again, as this plan's.
pub(super) fn sent(inner: &Inner, o: &Open, plays: &Plays) {
    let namer = Namer::new(plays.info.id, plays.revision.clone(), &o.session.packages);
    inner.told.lock().sent(o.token, namer);
    let nodes = &plays.revision.compiled.flat.nodes;
    for at in &plays.info.plugins {
        let silent = nodes.get(at.node).is_some_and(|n| o.revived.get(&n.name) == Some(&None));
        if silent && at.inst.upgrade().is_some_and(|i| i.dead()) {
            inner.faults.report(Fault { code: Code::PluginFailed, plan: plays.info.id, subject: Subject::Node(at.node as u32) });
        }
    }
}

/// Put the problems listed for session `token` after the revision's own in `at`, a news's or a
/// state's `problems`.
pub(super) fn listed(inner: &Inner, token: u64, at: &mut Value) {
    let live = inner.told.lock().problems(token);
    if let (false, Some(list)) = (live.is_empty(), at.as_array_mut()) {
        list.extend(live);
    }
}

#[cfg(test)]
#[path = "faults_tests.rs"]
pub(super) mod tests;

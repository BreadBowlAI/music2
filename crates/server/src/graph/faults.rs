//! Faults found while sound is made, told (RENDER-25, DIAG-4). Every part reports into the app's
//! one ring ([`m2_runtime::fault::Faults`], in [`Inner`]): the audio thread a sound that stopped
//! (`sound.stopped`) or a plugin that failed (`plugin.failed`), the actor a plugin host that ended
//! (`plugin.crashed`), a plan's build and the disk thread a file that can't be read
//! (`asset.unreadable`), a lane a warped clip whose stretch failed (`stretch.failed`), the player
//! an input chosen that isn't there (`input.missing`), the take thread a take's input that went
//! away (`input.lost`) or that the OS refuses (`input.refused`). Every 300 ms the server's notices
//! thread reads it here ([`Told::hear`]).
//! Each fault is named by the [`Namer`] of the plan it came from (the ex plan's D2), or an input's
//! by the [`InputNamer`] of its opening, listed among the open song's problems until its part plays
//! again (an input's: until the next take), and toasted once (D3).

use std::collections::HashSet;
use std::sync::Arc;

use m2_editor::faults::Namer;
use m2_play::{Listen, NO_MICROPHONE};
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
    /// The namers of the input a take opened last and the one before, newest first.
    inputs: [Option<Arc<InputNamer>>; 2],
    /// What is listed now, oldest first.
    live: Vec<Live>,
    /// Each part told by a toast, by code, in the open song: once each (D3).
    toasted: HashSet<(&'static str, String)>,
    /// Faults of a plan prepared but not sent yet, which its build reported (a file it can't
    /// read): named once it is sent, dropped once a newer one is. And an input's, reported as it
    /// opened, before its take named it.
    pending: Vec<Fault>,
    /// The ring's lost faults when it was last read.
    lost: u64,
    /// The list changed since the notices thread last read the ring.
    changed: bool,
}

/// Names the faults of one opening of the sound input ([`Listen::open`]): what it records from,
/// and the input chosen when that isn't there. The session's, as a plan's [`Namer`] is: the player
/// reports numbers only.
pub(super) struct InputNamer {
    open: u32,
    used: String,
    missing: Option<String>,
}

impl InputNamer {
    pub(super) fn of(l: &Listen) -> InputNamer {
        InputNamer { open: l.open, used: l.name.clone(), missing: l.missing.clone() }
    }

    /// The input a fault of `code` is about: what a toast is once for.
    fn part(&self, code: Code) -> String {
        match (code, &self.missing) {
            (Code::InputMissing, Some(chosen)) => chosen.clone(),
            _ => self.used.clone(),
        }
    }

    /// The problem `code` is, in plain words. None for a code not the input's.
    fn problem(&self, code: Code) -> Option<Diagnostic> {
        let used = &self.used;
        Some(match code {
            Code::InputMissing => Diagnostic::warning(
                code.diag(),
                format!(
                    "{} isn't plugged in, so this take records from {used}. To record from another input, choose it in Settings ▸ Sound input.",
                    self.missing.as_deref()?
                ),
            ),
            Code::InputLost => Diagnostic::warning(
                code.diag(),
                format!("{used} was unplugged, so the take ended there. It was kept up to then. The song plays on."),
            ),
            Code::InputRefused => Diagnostic::error(code.diag(), format!("{NO_MICROPHONE} The take was only silence, so it wasn't kept.")),
            _ => return None,
        })
    }
}

/// What names a fault: the namer of its plan, or of its input's opening.
#[derive(Clone)]
enum Who {
    Plan(Arc<Namer>),
    Input(Arc<InputNamer>),
}

impl Who {
    fn part(&self, f: Fault) -> Option<String> {
        match self {
            Who::Plan(n) => n.part(f.subject),
            Who::Input(i) => Some(i.part(f.code)),
        }
    }

    fn problem(&self, code: Code, parts: &[Subject]) -> Option<Diagnostic> {
        match self {
            Who::Plan(n) => n.problem(code, parts),
            Who::Input(i) => i.problem(code),
        }
    }
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
    /// What was listed goes, but the input's (it goes at the next take); a fault of the plan before
    /// it is still named.
    pub(super) fn sent(&mut self, token: u64, namer: Namer) {
        if token != self.token {
            (self.token, self.toasted, self.namers, self.inputs) = (token, HashSet::new(), [None, None], [None, None]);
        }
        self.namers = [Some(Arc::new(namer)), self.namers[0].take()];
        self.unlist(|code| !code.of_input());
    }

    /// A take of session `token` starts on the input opened as `namer` says: the input's problems
    /// listed go, as the next take is the input playing again (D3). A fault of the input a take
    /// opened before is still told.
    pub(super) fn opened(&mut self, token: u64, namer: InputNamer) {
        if token != self.token {
            return;
        }
        if self.inputs[0].as_ref().is_none_or(|i| i.open != namer.open) {
            self.inputs = [Some(Arc::new(namer)), self.inputs[0].take()];
        }
        self.unlist(Code::of_input);
    }

    /// The problems listed whose code is `gone`'s go.
    fn unlist(&mut self, gone: impl Fn(Code) -> bool) {
        let before = self.live.len();
        self.live.retain(|l| !gone(l.code));
        self.changed |= self.live.len() != before;
    }

    /// The song closed: nothing is listed or named.
    pub(super) fn closed(&mut self) {
        self.changed |= !self.live.is_empty();
        (self.namers, self.inputs, self.live, self.pending) = ([None, None], [None, None], vec![], vec![]);
    }

    /// What names `f`, and whether it is the newest of its kind (a plan sent last, an input a take
    /// opened last). None when it is older than the one before, or not named yet.
    fn who(&self, f: Fault) -> Option<(bool, Who)> {
        match f.subject {
            Subject::Input(n) => {
                let at = self.inputs.iter().position(|i| i.as_ref().is_some_and(|i| i.open == n))?;
                Some((at == 0, Who::Input(self.inputs[at].clone()?)))
            }
            _ => {
                let at = self.namers.iter().position(|x| x.as_ref().is_some_and(|x| x.plan() == f.plan))?;
                Some((at == 0, Who::Plan(self.namers[at].clone()?)))
            }
        }
    }

    /// Whether `f`, which nothing names, will be: plans and inputs are numbered as they are made,
    /// so one newer than the newest named isn't sent (or opened for a take) yet.
    fn waits(&self, f: Fault) -> bool {
        match f.subject {
            Subject::Input(n) => self.inputs[0].as_ref().is_none_or(|i| n > i.open),
            _ => self.namers[0].as_ref().is_none_or(|x| f.plan > x.plan()),
        }
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
        let pending = std::mem::take(&mut self.pending);
        for f in pending.into_iter().chain(std::iter::from_fn(|| faults.take())) {
            let Some((newest, who)) = self.who(f) else {
                if self.waits(f) && self.pending.len() < m2_runtime::fault::SLOTS {
                    self.pending.push(f);
                }
                continue;
            };
            let list = if newest { &mut self.live } else { &mut before };
            let Some(at) = add(list, &who, f) else { continue };
            self.changed |= newest;
            if let Some(part) = who.part(f)
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
fn add(list: &mut Vec<Live>, namer: &Who, f: Fault) -> Option<usize> {
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

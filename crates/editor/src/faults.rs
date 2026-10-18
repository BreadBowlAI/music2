//! Faults found while sound is made, in words (RENDER-25, DIAG-4). A part reports a fault into the
//! app's ring as numbers only ([`m2_runtime::fault`]). The session names it with the [`Namer`] of
//! the plan it came from: what the part is and where ("The delay on Bass"), as a problem with its
//! track's key.

use std::collections::HashMap;
use std::sync::Arc;

use m2_ir::FOp;
use m2_runtime::PackageSet;
use m2_runtime::fault::{Code, Subject};
use m2_session::Revision;
use m2_song::Diagnostic;

/// Names the faults of one plan, made when the plan is prepared from the revision it plays. It
/// holds that revision, so it names nothing until asked.
pub struct Namer {
    plan: u64,
    rev: Arc<Revision>,
    /// Each plugin node, as the app shows it ([`plugin_named`]).
    plugins: HashMap<usize, String>,
}

impl Namer {
    /// The namer of plan `plan` ([`m2_runtime::PlanInfo::id`]), built from `rev`, whose plugins
    /// are described in `packages`.
    pub fn new(plan: u64, rev: Arc<Revision>, packages: &PackageSet) -> Namer {
        let nodes = rev.compiled.flat.nodes.iter().enumerate();
        let plugins = nodes.filter(|(_, n)| matches!(n.op, FOp::Plugin { .. })).map(|(i, _)| (i, plugin_named(&rev, packages, i))).collect();
        Namer { plan, rev, plugins }
    }

    /// The plan it names faults for.
    pub fn plan(&self) -> u64 {
        self.plan
    }

    /// The part `s` names, as the user knows it, the same in every plan: "The delay on Bass",
    /// "Surge XT (lead)". A node's number and name change with an edit; these words don't. None
    /// for one past the revision's graph (a node leaving with an activation's window).
    pub fn part(&self, s: Subject) -> Option<String> {
        match s {
            Subject::Node(n) => {
                let n = n as usize;
                match &self.rev.compiled.flat.nodes.get(n)?.op {
                    FOp::Plugin { .. } => Some(self.plugin_named(n)),
                    _ => Some(self.sound_named(n)),
                }
            }
        }
    }

    /// The problem that `code` happened to `parts`, in plain words: one part, or each plugin of a
    /// plugin host that ended. Its module is their track's key, when they share one. None when
    /// this plan has none of them.
    pub fn problem(&self, code: Code, parts: &[Subject]) -> Option<Diagnostic> {
        let nodes: Vec<usize> = parts
            .iter()
            .map(|&s| match s {
                Subject::Node(n) => n as usize,
            })
            .filter(|&n| n < self.rev.compiled.flat.nodes.len())
            .collect();
        let first = *nodes.first()?;
        let again = "The rest of the song plays on. To start it again, choose Transport ▸ Stop All Sound.";
        let message = match code {
            Code::Stopped => format!("{} stopped working, so music2 made it silent. {again}", self.sound_named(first)),
            Code::OutOfMemory => format!(
                "{} ran out of memory (a sound may use up to {} MB), so music2 made it silent. {again}",
                self.sound_named(first),
                m2_runtime::SOUND_MEMORY_MB
            ),
            Code::PluginFailed => format!(
                "{} stopped working, so music2 made it silent. The rest of the song plays on. music2 starts it again from its saved \
                 settings. If it stops again soon after, it stays silent until you open the song again.",
                self.plugin_named(first)
            ),
            Code::PluginCrashed | Code::PluginHung => {
                let names: Vec<String> = nodes.iter().map(|&n| self.plugin_named(n)).collect();
                let (it, its, stops, stays) = match names.len() {
                    1 => ("it", "its", "it stops", "it stays"),
                    _ => ("them", "their", "they stop", "they stay"),
                };
                let ended = match code {
                    Code::PluginHung => "The plugin host stopped answering, so music2 ended it.",
                    _ => "The plugin host crashed.",
                };
                format!(
                    "{ended} {} went silent. The rest of the song plays on. music2 starts {it} again from {its} saved settings. If {stops} \
                     again soon after, {stays} silent until you open the song again.",
                    list(&names)
                )
            }
        };
        let module = &self.rev.compiled.flat.nodes[first].module;
        let shared = nodes.iter().all(|&n| self.rev.compiled.flat.nodes[n].module == *module);
        Some(Diagnostic { module: shared.then(|| module.clone()), ..Diagnostic::error(code.diag(), message) })
    }

    /// How the user knows node `node`: what it is, and where it is ("The delay on Bass"). A kernel
    /// by its name, a `poly` as the instrument, anything else by its id; on a track by its label, in
    /// the song's own module as "in the song".
    pub fn sound_named(&self, node: usize) -> String {
        let Some(n) = self.rev.compiled.flat.nodes.get(node) else { return "A sound".into() };
        let what = match &n.op {
            FOp::Poly { .. } => "instrument",
            FOp::Kernel { kernel, .. } => kernel.split_once(':').map_or(kernel.as_str(), |(_, name)| name),
            _ => n.name.rsplit_once('/').map_or(n.name.as_str(), |(_, id)| id),
        };
        let ir = &self.rev.compiled.ir;
        let place = match ir.modules.iter().find(|m| m.key == n.module) {
            Some(m) if m.track => format!("on {}", m.label()),
            _ if n.module == ir.root.module => "in the song".to_string(),
            Some(m) => format!("in {}", m.label()),
            None => format!("in {}", n.module),
        };
        format!("The {what} {place}")
    }

    /// How the user knows plugin node `node` ([`plugin_named`]).
    pub fn plugin_named(&self, node: usize) -> String {
        self.plugins.get(&node).cloned().unwrap_or_else(|| "A plugin".into())
    }
}

/// How the user knows plugin node `node` of `rev`'s graph: its plugin's name and its track's, as
/// the app shows them ("Surge XT (lead)").
pub fn plugin_named(rev: &Revision, packages: &PackageSet, node: usize) -> String {
    let Some(n) = rev.compiled.flat.nodes.get(node) else { return "A plugin".into() };
    let plugin = match &n.op {
        FOp::Plugin { plugin, .. } => packages.plugin_info(plugin).map_or_else(|| plugin.clone(), |i| i.found.name.clone()),
        _ => n.name.clone(),
    };
    let track = rev.compiled.ir.modules.iter().find(|m| m.key == n.module).map_or(n.module.as_str(), |m| m.label());
    format!("{plugin} ({track})")
}

/// `a`, `a and b`, `a, b and c`.
fn list(names: &[String]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

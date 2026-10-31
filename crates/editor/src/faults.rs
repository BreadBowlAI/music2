//! Faults found while sound is made, in words (RENDER-25, DIAG-4, RENDER-1). A part reports a fault
//! into a ring as numbers only ([`m2_runtime::fault`]): the app's live, an offline render's own.
//! The session names it with the [`Namer`] of the plan it came from: what the part is and where
//! ("The delay on Bass", "loop.wav on Drums"), as a problem with its track's key. An offline render
//! that a fault stopped says so with the namer's words ([`Faulted`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use m2_ir::{FOp, Flat, Ir, Op};
use m2_runtime::fault::{Code, Failed, Fault, Subject};
use m2_runtime::{PackageSet, TooSlow};
use m2_script::Descriptor;
use m2_session::{Compiled, Revision};
use m2_song::{DataSet, Diagnostic};

/// The song a namer names from: what a plan or a render plays.
pub trait Song: Send + Sync {
    fn ir(&self) -> &Ir;
    fn flat(&self) -> &Flat;
    fn data(&self) -> &DataSet;
    /// The packages it was built with (PACK-9): a render of it runs their kernels, whatever the
    /// set shows now.
    fn view(&self) -> &Arc<m2_runtime::View>;
    /// The descriptor of each component its build declared, by ComponentId: what names a sound by
    /// its label (ABI-7). None when it doesn't say.
    fn components(&self) -> Option<&BTreeMap<String, Arc<Descriptor>>> {
        None
    }
}

impl Song for Revision {
    fn ir(&self) -> &Ir {
        &self.compiled.ir
    }
    fn flat(&self) -> &Flat {
        &self.compiled.flat
    }
    fn data(&self) -> &DataSet {
        &self.compiled.data
    }
    fn view(&self) -> &Arc<m2_runtime::View> {
        &self.compiled.view
    }
    fn components(&self) -> Option<&BTreeMap<String, Arc<Descriptor>>> {
        Some(&self.compiled.components)
    }
}

impl Song for Compiled {
    fn ir(&self) -> &Ir {
        &self.ir
    }
    fn flat(&self) -> &Flat {
        &self.flat
    }
    fn data(&self) -> &DataSet {
        &self.data
    }
    fn view(&self) -> &Arc<m2_runtime::View> {
        &self.view
    }
    fn components(&self) -> Option<&BTreeMap<String, Arc<Descriptor>>> {
        Some(&self.components)
    }
}

/// A song's parts, each its own: one rooted at a track (a stem, `m2 bounce --track`), whose graph
/// is checked from its own IR, with the packages of the song it was made from.
pub struct Parts {
    pub ir: Ir,
    pub flat: Flat,
    pub data: DataSet,
    view: Arc<m2_runtime::View>,
    components: Option<BTreeMap<String, Arc<Descriptor>>>,
}

impl Parts {
    /// The parts of `ir`, made from `song` (rooted at one of its tracks, [`crate::render::rooted`]):
    /// its graph checked against the packages `song` was built with (PACK-9), which it keeps, so it
    /// renders with them whatever the session shows by then; and `song`'s data.
    pub fn of(song: &dyn Song, ir: Ir, packages: &PackageSet) -> Result<Parts, Vec<m2_ir::Diagnostic>> {
        let flat = m2_ir::check(&ir, &*packages.viewing(song.view()))?;
        Ok(Parts { ir, flat, data: song.data().clone(), view: song.view().clone(), components: song.components().cloned() })
    }
}

impl Song for Parts {
    fn ir(&self) -> &Ir {
        &self.ir
    }
    fn flat(&self) -> &Flat {
        &self.flat
    }
    fn data(&self) -> &DataSet {
        &self.data
    }
    fn view(&self) -> &Arc<m2_runtime::View> {
        &self.view
    }
    fn components(&self) -> Option<&BTreeMap<String, Arc<Descriptor>>> {
        self.components.as_ref()
    }
}

/// An offline render that a fault stopped (RENDER-1): the fault's code (DIAG-2) and what happened,
/// in plain words that name the part. What `m2 render` prints, and an export's `error` event.
#[derive(Debug)]
pub struct Faulted {
    pub code: &'static str,
    pub words: String,
}

impl std::fmt::Display for Faulted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.words)
    }
}

impl std::error::Error for Faulted {}

impl Faulted {
    /// The [`Faulted`] in `e`'s chain, if a fault stopped it.
    pub fn of(e: &anyhow::Error) -> Option<&Faulted> {
        e.chain().find_map(|c| c.downcast_ref::<Faulted>())
    }
}

/// How a FLAC comes into a song as a clip (FMT-24): as the WAV of its samples.
const NOT_WAV: &str = "To use a FLAC, import it (File ▸ Import Audio…): music2 brings it in as a WAV.";

/// How long a file one whole decode may make is (FMT-24's budget), in words.
const TOO_LONG: &str = "over 134,217,728 samples, about 23 minutes of stereo at 48 kHz";

/// Names the faults of one plan, made when the plan is prepared from the song it plays, or for an
/// offline render. It holds that song, so it names nothing until asked.
pub struct Namer {
    plan: u64,
    song: Arc<dyn Song>,
    /// Each plugin node, as the app shows it ([`plugin_named`]).
    plugins: HashMap<usize, String>,
}

impl Namer {
    /// The namer of plan `plan` ([`m2_runtime::PlanInfo::id`]), built from `song`, whose plugins
    /// are described in `packages`.
    pub fn new(plan: u64, song: Arc<dyn Song>, packages: &PackageSet) -> Namer {
        let nodes = song.flat().nodes.iter().enumerate();
        let plugins =
            nodes.filter(|(_, n)| matches!(n.op, FOp::Plugin { .. })).map(|(i, _)| (i, plugin_named(&*song, packages, i))).collect();
        Namer { plan, song, plugins }
    }

    /// The plan it names faults for.
    pub fn plan(&self) -> u64 {
        self.plan
    }

    /// The part `s` names, as the user knows it, the same in every plan: "The delay on Bass",
    /// "Surge XT (lead)", "loop.wav on Drums". A node's number and name change with an edit; these
    /// words don't. None for one past the song's graph (a node leaving with an activation's
    /// window).
    pub fn part(&self, s: Subject) -> Option<String> {
        match s {
            Subject::Node(n) => {
                let n = n as usize;
                match &self.song.flat().nodes.get(n)?.op {
                    FOp::Plugin { .. } => Some(self.plugin_named(n)),
                    _ => Some(self.sound_named(n)),
                }
            }
            Subject::Clip { .. } => self.clip(s).map(|(file, place)| format!("{file} {place}")),
            Subject::Asset { node, .. } => self.asset(s).map(|file| format!("{file} {}", self.place(node as usize))),
            // The session names the sound input: it is of no plan.
            Subject::Input(_) => None,
        }
    }

    /// The problem that `code` happened to `parts`, in plain words: one part, or each plugin of a
    /// plugin host that ended. Its module is their track's key, when they share one. None when
    /// this plan has none of them.
    pub fn problem(&self, code: Code, parts: &[Subject]) -> Option<Diagnostic> {
        let on = "The rest of the song plays on.";
        let (message, module) = match code {
            Code::Unreadable | Code::NotWav | Code::TooLong | Code::StretchFailed => {
                let s = *parts.first()?;
                let message = match (code, s) {
                    (Code::NotWav, _) => {
                        let (file, place) = self.clip(s)?;
                        format!("{file} {place} isn't a WAV file, so it plays silent: a clip plays WAV files only. {NOT_WAV} {on}")
                    }
                    (Code::StretchFailed, _) => {
                        let (file, place) = self.clip(s)?;
                        format!("The warped clip of {file} {place} can't be stretched, so it plays silent. {on}")
                    }
                    (_, Subject::Clip { .. }) => {
                        let (file, place) = self.clip(s)?;
                        format!(
                            "{file} {place} can't be read, so it plays silent. It may be damaged, or on a drive that isn't plugged in. {on}"
                        )
                    }
                    (Code::TooLong, Subject::Asset { node, .. }) => {
                        let (file, who) = (self.asset(s)?, lower(&self.sound_named(node as usize)));
                        format!("{file}, which {who} plays, is too long to hold whole ({TOO_LONG}), so it plays silent. {on}")
                    }
                    (_, Subject::Asset { node, .. }) => {
                        let (file, who) = (self.asset(s)?, lower(&self.sound_named(node as usize)));
                        format!("{file}, which {who} plays, can't be read, so it plays silent. It may be damaged. {on}")
                    }
                    _ => return None,
                };
                (message, self.module(s))
            }
            _ => self.nodes_problem(code, parts)?,
        };
        Some(Diagnostic { module, ..Diagnostic::error(code.diag(), message) })
    }

    /// [`Self::problem`] for a fault of nodes: a sound that stopped, plugins that failed.
    fn nodes_problem(&self, code: Code, parts: &[Subject]) -> Option<(String, Option<String>)> {
        let flat = self.song.flat();
        let nodes: Vec<usize> = parts
            .iter()
            .filter_map(|&s| match s {
                Subject::Node(n) => Some(n as usize),
                _ => None,
            })
            .filter(|&n| n < flat.nodes.len())
            .collect();
        let first = *nodes.first()?;
        let on = "The rest of the song plays on.";
        let again = "To start it again, choose Transport ▸ Stop All Sound.";
        let message = match code {
            Code::OutOfMemory => format!(
                "{} ran out of memory (a sound may use up to {} MB), so music2 made it silent. {on} {again}",
                self.sound_named(first),
                m2_runtime::SOUND_MEMORY_MB
            ),
            Code::PluginFailed => format!(
                "{} stopped working, so music2 made it silent. {on} music2 starts it again from its saved settings. If it stops again \
                 soon after, it stays silent until you open the song again.",
                self.plugin_named(first)
            ),
            Code::TooSlow => format!(
                "{} took too long and was stopped. It stays silent until the song is built again. {on}",
                self.slow_sound(&flat.nodes[first].name)
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
                    "{ended} {} went silent. {on} music2 starts {it} again from {its} saved settings. If {stops} again soon after, {stays} \
                     silent until you open the song again.",
                    list(&names)
                )
            }
            _ => format!("{} stopped working, so music2 made it silent. {on} {again}", self.sound_named(first)),
        };
        let module = &flat.nodes[first].module;
        let shared = nodes.iter().all(|&n| flat.nodes[n].module == *module);
        Some((message, shared.then(|| module.clone())))
    }

    /// What stopped an offline render (RENDER-1), in plain words that name the part, `why` (the
    /// error that said so, when one did) after as the detail.
    pub fn failed(&self, f: Fault, why: Option<&anyhow::Error>) -> String {
        let not = "so the file wasn't made.";
        let detail = why.map(|e| format!(" (In detail: {e:#})")).unwrap_or_default();
        let part = self.part(f.subject).unwrap_or_else(|| "A part of the song".into());
        match (f.code, f.subject) {
            (Code::OutOfMemory, _) => format!("{part} ran out of memory (a sound may use up to {} MB), {not}", m2_runtime::SOUND_MEMORY_MB),
            (Code::PluginFailed, _) => format!("{part} stopped working, {not} Try again. If it stops again, the plugin may be broken."),
            (Code::PluginCrashed | Code::PluginHung, _) => format!("The plugin host crashed while {part} played, {not} Try again."),
            (Code::StretchFailed, s) => match self.clip(s) {
                Some((file, place)) => format!("The warped clip of {file} {place} can't be stretched, {not}{detail}"),
                None => format!("A warped clip can't be stretched, {not}{detail}"),
            },
            (Code::Unreadable, Subject::Asset { node, .. }) => match self.asset(f.subject) {
                Some(file) => {
                    format!("{file}, which {} plays, can't be read, {not} It may be damaged.", lower(&self.sound_named(node as usize)))
                }
                None => format!("A file the song plays can't be read, {not}"),
            },
            (Code::Unreadable, _) => format!("{part} can't be read, {not} It may be damaged, or on a drive that isn't plugged in."),
            (Code::NotWav, _) => format!("{part} isn't a WAV file, {not} A clip plays WAV files only. {NOT_WAV}"),
            (Code::TooLong, Subject::Asset { node, .. }) => match self.asset(f.subject) {
                Some(file) => format!(
                    "{file}, which {} plays, is too long to hold whole ({TOO_LONG}), {not}",
                    lower(&self.sound_named(node as usize))
                ),
                None => format!("A file the song plays is too long to hold whole ({TOO_LONG}), {not}"),
            },
            (Code::TooLong, _) => format!("{part} is too long to hold whole ({TOO_LONG}), {not}"),
            (Code::Stopped, _) => format!("{part} stopped working, {not}{detail}"),
            (Code::TooSlow, s) => {
                let sound = match s {
                    Subject::Node(n) => self.song.flat().nodes.get(n as usize).map(|n| self.slow_sound(&n.name)),
                    _ => None,
                };
                format!("{} took too long and was stopped, {not}", sound.unwrap_or(part))
            }
            // No offline render reads the sound input.
            (Code::InputMissing | Code::InputLost | Code::InputRefused, _) => format!("The sound input stopped, {not}"),
        }
    }

    /// `e`, from an offline render: the fault that stopped it, named, when one did ([`Faulted`]);
    /// or a kernel too slow to be made (ABI-7).
    pub fn faulted(&self, e: anyhow::Error) -> anyhow::Error {
        if let Some(f) = e.downcast_ref::<Failed>() {
            return Faulted { code: f.fault.code.diag(), words: self.failed(f.fault, f.why()) }.into();
        }
        match e.downcast_ref::<TooSlow>() {
            Some(slow) => {
                Faulted { code: Code::TooSlow.diag(), words: format!("{}, so the file wasn't made.", self.too_slow(slow)) }.into()
            }
            None => e,
        }
    }

    /// A kernel too slow to be made, as its plan was ([`TooSlow`] at a sandbox's birth, ABI-7):
    /// "Growl on Bass, from @alice/growl, took too long to get ready and was stopped".
    pub fn too_slow(&self, slow: &TooSlow) -> String {
        format!("{} took too long to get ready and was stopped", self.slow_sound(&slow.node))
    }

    /// The sound node `name` (`<module>/<id>`, a voice's node too) is part of, as the song's code
    /// wrote it, with its place and whose code it runs (ABI-7), as a sentence's subject:
    /// - made inside a package's component: the outermost such call, by the component's label, and
    ///   its package ("Growl on Bass, from @alice/growl,"); and when the time-checked code it runs
    ///   (a pack's kernels, [`m2_runtime::packages::checked`]) is another package's, that one too
    ///   ("…, from @bob/synth, with sound code from @alice/growl,");
    /// - a kernel the song's own code calls: the kernel by its name, and its package ("The growl on
    ///   Bass, from @alice/growl,");
    /// - an instrument the song's own code writes: as the song's own, with the packages whose
    ///   time-checked kernels its voices run ("The instrument on Keys, from the song's own code,
    ///   with sound code from @alice/growl,").
    ///
    /// A node not in the song is named as it is.
    pub fn slow_sound(&self, name: &str) -> String {
        let ir = self.song.ir();
        let found = ir.modules.iter().find_map(|m| {
            let id = name.strip_prefix(m.key.as_str())?.strip_prefix('/')?;
            let mut at = None;
            crate::controls::walk(&m.nodes, &mut |n| {
                if at.is_none() && n.id == id {
                    // The kernels it runs: its own, or its voices'.
                    let mut kernels = vec![];
                    let mut kernel_of = |n: &m2_ir::Node| {
                        if let Op::Kernel { kernel, .. } = &n.op {
                            kernels.push(kernel.clone());
                        }
                    };
                    kernel_of(n);
                    let what = match &n.op {
                        Op::Kernel { kernel, .. } => m2_song::pack::split_id(kernel).map_or(kernel.as_str(), |(_, k)| k).to_string(),
                        Op::Poly { body, .. } => {
                            crate::controls::walk(&body.nodes, &mut kernel_of);
                            "instrument".to_string()
                        }
                        _ => id.to_string(),
                    };
                    at = Some((n.frame, kernels, what, matches!(n.op, Op::Kernel { .. })));
                }
            });
            at.map(|(frame, kernels, what, kernel)| (m, frame, kernels, what, kernel))
        });
        let Some((m, frame, kernels, what, kernel)) = found else { return name.to_string() };
        let place = self.place_of(&m.key);
        // The packages whose time-checked code it runs, in order, once each: as the build loaded
        // them, so never std's, nor a pack music2 ships (ABI-11).
        let view = self.song.view();
        let mut code: Vec<&str> = kernels
            .iter()
            .filter_map(|k| m2_song::pack::split_id(k).map(|(p, _)| p))
            .filter(|p| view.get(p).is_none_or(|p| p.checked))
            .collect();
        code.sort();
        code.dedup();
        let label = |id: &str| self.song.components().and_then(|c| c.get(id)).map(|d| d.label.clone());
        let component = m.chain(frame).into_iter().filter_map(|f| f.component.as_deref()).find_map(|id| {
            let (package, bare) = m2_song::pack::split_id(id)?;
            Some((label(id).unwrap_or_else(|| bare.to_string()), package))
        });
        let with = |skip: &str| match code.iter().filter(|p| **p != skip).copied().collect::<Vec<_>>() {
            others if others.is_empty() => String::new(),
            others => format!(" with sound code from {},", or(&others)),
        };
        match (component, kernel) {
            (Some((label, package)), _) => format!("{} {place}, from {package},{}", capital(&label), with(package)),
            // A kernel the song's own code calls: its own package's.
            (None, true) => match kernels.first().and_then(|k| m2_song::pack::split_id(k)) {
                Some((package, _)) => format!("The {what} {place}, from {package},"),
                None => format!("The {what} {place}"),
            },
            (None, false) => format!("The {what} {place}, from the song's own code,{}", with("")),
        }
    }

    /// The track's key of what `s` is about.
    fn module(&self, s: Subject) -> Option<String> {
        match s {
            Subject::Node(n) | Subject::Asset { node: n, .. } => Some(self.song.flat().nodes.get(n as usize)?.module.clone()),
            Subject::Clip { lane, .. } => Some(m2_runtime::lane_slots(self.song.flat()).get(lane as usize)?.module.clone()),
            Subject::Input(_) => None,
        }
    }

    /// Clip `s`: its file, and where it is ("on Drums").
    fn clip(&self, s: Subject) -> Option<(String, String)> {
        let Subject::Clip { lane, clip } = s else { return None };
        let slots = m2_runtime::lane_slots(self.song.flat());
        let r = slots.get(lane as usize)?;
        let file = self.song.data().audio(&r.module, &r.lane).get(clip as usize)?.asset.clone();
        Some((file, self.place_of(&r.module)))
    }

    /// The file `s`, one a kernel reads, names.
    fn asset(&self, s: Subject) -> Option<String> {
        let Subject::Asset { node, slot } = s else { return None };
        let n = self.song.flat().nodes.get(node as usize)?;
        m2_runtime::audio::assets_of(n).get(slot as usize).map(|a| a.to_string())
    }

    /// Where node `node` is: "on Bass", "in the song".
    fn place(&self, node: usize) -> String {
        self.song.flat().nodes.get(node).map_or_else(|| "in the song".into(), |n| self.place_of(&n.module))
    }

    /// Where module `module` is: on a track by its label, the song's own as "in the song".
    fn place_of(&self, module: &str) -> String {
        let ir = self.song.ir();
        match ir.modules.iter().find(|m| m.key == module) {
            Some(m) if m.track => format!("on {}", m.label()),
            _ if module == ir.root.module => "in the song".to_string(),
            Some(m) => format!("in {}", m.label()),
            None => format!("in {module}"),
        }
    }

    /// How the user knows node `node`: what it is, and where it is ("The delay on Bass"). A kernel
    /// by its name, a `poly` as the instrument, anything else by its id; on a track by its label, in
    /// the song's own module as "in the song".
    pub fn sound_named(&self, node: usize) -> String {
        let Some(n) = self.song.flat().nodes.get(node) else { return "A sound".into() };
        let what = match &n.op {
            FOp::Poly { .. } => "instrument",
            FOp::Kernel { kernel, .. } => m2_song::pack::split_id(kernel).map_or(kernel.as_str(), |(_, name)| name),
            _ => n.name.rsplit_once('/').map_or(n.name.as_str(), |(_, id)| id),
        };
        format!("The {what} {}", self.place_of(&n.module))
    }

    /// How the user knows plugin node `node` ([`plugin_named`]).
    pub fn plugin_named(&self, node: usize) -> String {
        self.plugins.get(&node).cloned().unwrap_or_else(|| "A plugin".into())
    }
}

/// How the user knows plugin node `node` of `song`'s graph: its plugin's name and its track's, as
/// the app shows them ("Surge XT (lead)").
pub fn plugin_named(song: &dyn Song, packages: &PackageSet, node: usize) -> String {
    let Some(n) = song.flat().nodes.get(node) else { return "A plugin".into() };
    let plugin = match &n.op {
        FOp::Plugin { plugin, .. } => packages.plugin_info(plugin).map_or_else(|| plugin.clone(), |i| i.found.name.clone()),
        _ => n.name.clone(),
    };
    let track = song.ir().modules.iter().find(|m| m.key == n.module).map_or(n.module.as_str(), |m| m.label());
    format!("{plugin} ({track})")
}

/// `words` with a capital first letter, to start a sentence.
fn capital(words: &str) -> String {
    let mut c = words.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// `The x` as `the x`, inside a sentence.
fn lower(words: &str) -> String {
    match words.strip_prefix("The ") {
        Some(rest) => format!("the {rest}"),
        None => words.to_string(),
    }
}

/// `a`, `a or b`, `a, b or c`.
fn or(names: &[&str]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

/// `a`, `a and b`, `a, b and c`.
fn list(names: &[String]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.concat(),
    }
}

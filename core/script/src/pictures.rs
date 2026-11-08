//! The picture runner (CARD-64, CARD-65): a sound's pictures, drawn by its own TypeScript on the
//! server, in the sandbox a build runs in, never in the window, and checked here in Rust after.
//!
//! - **A thread for each runtime.** One runtime for each package by what its code is, and one for
//!   the song's code at each code revision, each on a thread of its own: a pack that hangs, or is
//!   slow to load, holds only its own pictures. [`KEPT`] at most, the one used longest ago let go.
//! - **Limits.** Loading the module that declares a sound may work [`LOAD`]; each draw [`DRAW`],
//!   counted from its start, of its own thread's CPU time, and [`DRAW_CLOCK`] by the clock. A load that fails or is stopped, and a draw stopped for its time or its
//!   memory, is remembered: that runtime's pictures (or that picture) are refused at once, in words,
//!   until the code changes, which makes a new runtime.
//! - **Pure draws.** Each draw runs twice; one that gives anything else the second time, at the same
//!   values, is refused.
//! - **Checked in Rust.** What a draw gives is read into this module's own shapes and checked again
//!   (sizes, counts, text, each dot's names against the descriptor), whatever the pack's code did to
//!   its sandbox, and written out again from them: only that goes on.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use m2_ir::Unit;
use rquickjs::{CatchResultExt, Context, Function, Module, Persistent, Runtime};
use serde::{Deserialize, Serialize};

use super::{CORE, JS_STACK, Limit, PROJECT, STACK, State, caught, host};
use crate::component::{Descriptor, ViewOf};

/// The runtimes kept at most.
const KEPT: usize = 16;
/// The memory each runtime may use.
const MEMORY: usize = 64 << 20;
/// How long loading a picture's module may work: its build already ran it once.
const LOAD: Duration = Duration::from_secs(1);
/// How long one draw may work (CARD-64): its own thread's CPU time, so a draw that waits for a CPU
/// on a busy machine is never stopped for the wait.
const DRAW: Duration = Duration::from_millis(20);
/// How long one draw may take by the clock: as long as an ask waits for it (the server's 2 s), so a
/// draw that gets almost no CPU still ends.
const DRAW_CLOCK: Duration = Duration::from_secs(2);
/// The most a draw may give, as JSON (CARD-65).
pub const BYTES: usize = 256 << 10;
/// The most numbers a draw may give in all (CARD-65).
pub const NUMBERS: usize = 20_000;

/// A song's own code (D16), at one code revision: `key` names that revision.
pub struct Song {
    pub key: u64,
    /// Every code file of the snapshot, by its path in the project.
    pub code: BTreeMap<String, String>,
    /// The packages its manifest lists besides `@m2/core`.
    pub dependencies: Vec<String>,
}

/// One draw to make.
#[derive(Clone)]
pub struct Ask {
    /// What the code it runs is: the runtime's key. A new key is a new runtime, and forgets what
    /// the old one refused (`m2_editor::views::picture_call` makes it from the hashes of the files
    /// the build read, and the song's code revision for its own sounds).
    pub key: String,
    /// The packages the song's build resolved, by name: what the module imports from.
    pub packages: Vec<(String, PathBuf)>,
    /// The song's code, when the sound is the song's own.
    pub song: Option<Arc<Song>>,
    /// The module that declares the sound, as its descriptor names it (`@m2/mixing`, `lib/eq.ts`).
    pub module: String,
    /// Its descriptor, which a draw's dots are checked against.
    pub descriptor: Arc<Descriptor>,
    /// The picture's name among its views.
    pub view: String,
    /// What plays, as a JSON object by value name (CARD-63).
    pub values: String,
    /// The picture asked for, which its parts are kept by (CARD-64).
    pub memo: String,
    /// The song's sample rate, which a draw reads as `p.rate` (CARD-87): [`RATE`] where no song plays.
    pub rate: u32,
}

/// The rate a picture is drawn at where no song plays (`m2 pack check`, CARD-87).
pub const RATE: u32 = 48_000;

/// What a draw gave.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    /// `{"shapes": […], "words": "…"}`, as checked here and written again.
    Shapes(String),
    /// Why it can't be drawn, in words that follow "Its picture" ("names freq9, which this sound
    /// doesn't have").
    Cant(String),
    /// Not drawn within the time asked: it is still being drawn.
    Busy,
}

struct Job {
    ask: Ask,
    reply: mpsc::SyncSender<Drawn>,
}

/// A runtime's thread, and when it was last asked.
struct Worker {
    key: String,
    jobs: mpsc::Sender<Job>,
    used: u64,
}

/// The picture runner: a thread for each runtime, each ending once the runner lets it go.
#[derive(Default)]
pub struct Runner {
    workers: Mutex<(Vec<Worker>, u64)>,
}

impl Runner {
    pub fn new() -> Runner {
        Runner::default()
    }

    /// Draw `ask` on its runtime's thread, and wait for it.
    pub fn draw(&self, ask: Ask) -> Drawn {
        self.draw_within(ask, None)
    }

    /// Draw `ask`, waiting `most` at most: past it, [`Drawn::Busy`] (the draw goes on, and a next
    /// ask finds the runtime free again, or still refusing).
    pub fn draw_within(&self, ask: Ask, most: Option<Duration>) -> Drawn {
        let gone = || Drawn::Cant("can't be drawn: the picture runner stopped".into());
        let (reply, answer) = mpsc::sync_channel(1);
        let mut job = Job { ask, reply };
        for _ in 0..2 {
            let jobs = self.worker(&job.ask.key);
            match jobs.send(job) {
                Ok(()) => {
                    return match most {
                        Some(d) => answer.recv_timeout(d).unwrap_or_else(|e| match e {
                            mpsc::RecvTimeoutError::Timeout => Drawn::Busy,
                            mpsc::RecvTimeoutError::Disconnected => gone(),
                        }),
                        None => answer.recv().unwrap_or_else(|_| gone()),
                    };
                }
                // Its thread ended (a panic): let it go, and start it again once.
                Err(mpsc::SendError(j)) => {
                    job = j;
                    self.workers.lock().expect("the workers").0.retain(|w| w.key != job.ask.key);
                }
            }
        }
        gone()
    }

    /// The thread of the runtime `key`, started if none is.
    fn worker(&self, key: &str) -> mpsc::Sender<Job> {
        let mut w = self.workers.lock().expect("the workers");
        let (workers, clock) = &mut *w;
        *clock += 1;
        if let Some(x) = workers.iter_mut().find(|x| x.key == key) {
            x.used = *clock;
            return x.jobs.clone();
        }
        if workers.len() >= KEPT
            && let Some(old) = workers.iter().enumerate().min_by_key(|(_, x)| x.used).map(|(i, _)| i)
        {
            workers.swap_remove(old);
        }
        let (jobs, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("m2-picture".into())
            .stack_size(STACK)
            .spawn(move || {
                m2_os::thread::c_numbers();
                let mut kept = Kept::default();
                for job in rx {
                    let _ = job.reply.send(kept.draw(&job.ask));
                }
            })
            .expect("a thread for pictures");
        workers.push(Worker { key: key.to_string(), jobs: jobs.clone(), used: *clock });
        jobs
    }
}

/// How long a picture stopped for its time or memory is refused at once before it is tried again:
/// a stop on a busy machine may be a blip (CARD-64).
#[cfg(not(test))]
const BACKOFF: Duration = Duration::from_secs(10);
#[cfg(test)]
const BACKOFF: Duration = Duration::from_millis(300);

/// What a picture's draws were stopped at: when last, and the values stopped once and twice.
#[derive(Default)]
struct Stops {
    last: Option<(std::time::Instant, String)>,
    once: HashSet<String>,
    twice: HashSet<String>,
}

/// One runtime's thread's own: its runtime, and what it refuses until its code changes.
#[derive(Default)]
struct Kept {
    one: Option<One>,
    /// Why every picture here is refused: a load that failed or was stopped.
    broken: Option<String>,
    /// Each picture's stops, by component and view (CARD-64).
    stopped: HashMap<(String, String), Stops>,
    /// The pictures shown pure here: each runtime's first draw of each runs twice (CARD-64).
    proven: HashSet<(String, String)>,
    /// The pictures that drew differently for the same values: refused until the code changes.
    impure: HashSet<(String, String)>,
}

/// What a picture whose draw keeps something between draws is refused with.
const IMPURE: &str = "draws differently each time for the same values: a picture keeps nothing between its draws";

impl Kept {
    fn draw(&mut self, ask: &Ask) -> Drawn {
        if let Some(why) = &self.broken {
            return Drawn::Cant(why.clone());
        }
        let at = (ask.descriptor.id.clone(), ask.view.clone());
        if self.impure.contains(&at) {
            return Drawn::Cant(IMPURE.into());
        }
        if let Some(st) = self.stopped.get(&at) {
            if st.twice.contains(&ask.values) {
                return Drawn::Cant("was stopped twice at these values, and is drawn at them again once its code changes".into());
            }
            if let Some((when, why)) = &st.last
                && when.elapsed() < BACKOFF
            {
                return Drawn::Cant(format!("{why}; it is tried again in {} s", BACKOFF.as_secs().max(1)));
            }
        }
        if self.one.is_none() {
            match made(ask) {
                Ok(one) => self.one = Some(one),
                Err(why) => {
                    self.broken = Some(why.clone());
                    return Drawn::Cant(why);
                }
            }
        }
        let one = self.one.as_mut().expect("made");
        if let Err(why) = one.load(ask) {
            self.one = None;
            self.broken = Some(why.clone());
            return Drawn::Cant(why);
        }
        // A picture's first draw here runs twice, at the same values: a pure draw gives the same
        // both times (CARD-64). After that, once.
        let first = one.call(ask);
        let twice = !self.proven.contains(&at);
        let second = match &first {
            Ok(_) if twice => one.call(ask),
            _ => first.clone(),
        };
        match (first, second) {
            (Ok(a), Ok(b)) if a == b => {
                self.proven.insert(at);
                match a.strip_prefix("{\"why\":") {
                    Some(_) => Drawn::Cant(
                        serde_json::from_str::<serde_json::Value>(&a).ok().and_then(|v| v["why"].as_str().map(String::from)).unwrap_or(a),
                    ),
                    None => match checked(&a, &ask.descriptor, &ask.view) {
                        Ok(json) => Drawn::Shapes(json),
                        Err(why) => Drawn::Cant(why),
                    },
                }
            }
            (Ok(_), Ok(_)) => {
                self.impure.insert(at);
                Drawn::Cant(IMPURE.into())
            }
            (Err(Stop::Hard(why)), _) | (_, Err(Stop::Hard(why))) => {
                // Stopped for its time or its memory: its runtime is made again; the picture is
                // refused for a while, and at these values for good once they were stopped twice.
                self.one = None;
                let st = self.stopped.entry(at).or_default();
                st.last = Some((std::time::Instant::now(), why.clone()));
                if !st.once.insert(ask.values.clone()) {
                    st.twice.insert(ask.values.clone());
                }
                Drawn::Cant(format!("{why}; it is tried again in {} s", BACKOFF.as_secs().max(1)))
            }
            (Err(Stop::Plain(m)), _) | (_, Err(Stop::Plain(m))) => Drawn::Cant(format!("couldn't be drawn: {m}")),
        }
    }
}

/// What a picture of `desc` is drawn from at its defaults (`m2 pack check`, CARD-64): each control
/// but a dictionary at its default (a choice's name, a switch's true or false), each setting of a
/// number, a name or true or false at its default, the rest null.
pub fn defaults(desc: &Descriptor) -> String {
    let mut v = serde_json::Map::new();
    for c in desc.controls.iter().filter(|c| c.each.is_none()) {
        let x = match (&c.default, &c.unit) {
            (None, _) => serde_json::Value::Null,
            (Some(d), Some(Unit::Switch)) => serde_json::json!(d.value != 0.0),
            (Some(d), Some(Unit::Choice(_))) => c
                .options()
                .and_then(|o| o.names().get(d.value as usize).map(|n| serde_json::json!(&**n)))
                .unwrap_or(serde_json::Value::Null),
            (Some(d), _) => serde_json::json!(d.value),
        };
        v.insert(c.name.clone(), x);
    }
    for st in &desc.settings {
        let plain = st.default.as_ref().filter(|d| d.is_number() || d.is_string() || d.is_boolean()).cloned();
        v.insert(st.name.clone(), plain.unwrap_or(serde_json::Value::Null));
    }
    serde_json::Value::Object(v).to_string()
}

/// Why a call into the sandbox gave nothing.
#[derive(Clone, Debug)]
enum Stop {
    /// It threw: its message.
    Plain(String),
    /// It was stopped for its time or its memory, in words.
    Hard(String),
}

/// A runtime and the modules it loaded.
struct One {
    // The function kept goes before its context, and that before its runtime.
    call: Persistent<Function<'static>>,
    ctx: Context,
    _rt: Runtime,
    state: Rc<RefCell<State>>,
    limit: Arc<Mutex<Limit>>,
    loaded: HashSet<String>,
}

/// A new runtime for `ask`'s key: the build's sandbox (its prelude, `Math` on libm, the loader over
/// the packages and the song's code), with `@m2/core`'s `__picture` kept as it was before any
/// pack's code ran.
fn made(ask: &Ask) -> Result<One, String> {
    let packages: Vec<(String, PathBuf)> = ask.packages.iter().filter_map(|(n, d)| Some((n.clone(), d.canonicalize().ok()?))).collect();
    let (code, dependencies) = match &ask.song {
        Some(s) => (s.code.clone(), s.dependencies.clone()),
        None => (BTreeMap::new(), packages.iter().map(|(n, _)| n.clone()).collect()),
    };
    let state = Rc::new(RefCell::new(State { code, packages: packages.clone(), dependencies, ..Default::default() }));
    let broke = |e: rquickjs::Error| format!("can't be drawn: the picture runner couldn't start ({e})");
    let rt = Runtime::new().map_err(broke)?;
    rt.set_memory_limit(MEMORY);
    rt.set_max_stack_size(JS_STACK);
    let limit = Arc::new(Mutex::new(Limit::new(LOAD)));
    let watched = limit.clone();
    rt.set_interrupt_handler(Some(Box::new(move || watched.lock().map(|l| l.passed()).unwrap_or(true))));
    rt.set_loader(super::Resolve(state.clone()), super::Load(state.clone()));
    let ctx = Context::full(&rt).map_err(broke)?;
    let call = ctx.with(|ctx| {
        let run = || -> rquickjs::Result<Persistent<Function<'static>>> {
            host(&ctx, &state, &packages)?;
            let main = format!("import {{ __picture }} from \"{CORE}\";\nglobalThis.__m2_picture = __picture;\n");
            Module::evaluate(ctx.clone(), "<picture>", main)?.finish::<()>()?;
            let f: Function = ctx.globals().get("__m2_picture")?;
            // Kept here: nothing a pack's code does to a global reaches it.
            ctx.globals().remove("__m2_picture")?;
            Ok(Persistent::save(&ctx, f))
        };
        run().catch(&ctx).map_err(|e| format!("can't be drawn: {}", caught(&mut state.borrow_mut(), e, false).message))
    })?;
    Ok(One { call, ctx, _rt: rt, state, limit, loaded: HashSet::new() })
}

impl One {
    /// Load the module that declares `ask`'s sound, once: or why it can't be.
    fn load(&mut self, ask: &Ask) -> Result<(), String> {
        let spec = match ask.song {
            Some(_) => format!("{PROJECT}{}", ask.module),
            None => ask.module.clone(),
        };
        if self.loaded.contains(&spec) {
            return Ok(());
        }
        *self.limit.lock().expect("the limit") = Limit::new(LOAD);
        // A song's module is imported as the project's own code imports it: from the project.
        let name = match ask.song {
            Some(_) => format!("{PROJECT}<picture {}>", self.loaded.len()),
            None => format!("<picture {}>", self.loaded.len()),
        };
        let state = self.state.clone();
        let loaded = self.ctx.with(|ctx| {
            let main = format!("import * as m from {};\n", serde_json::to_string(&spec).expect("a string"));
            let run = || Module::evaluate(ctx.clone(), name, main)?.finish::<()>();
            run().catch(&ctx).map_err(|e| caught(&mut state.borrow_mut(), e, false).message)
        });
        let stopped = self.limit.lock().expect("the limit").passed();
        match loaded {
            _ if stopped => Err(format!("can't be drawn: its module {} took too long to load and was stopped", ask.module)),
            Err(why) => Err(format!("can't be drawn: its module {} doesn't load: {why}", ask.module)),
            Ok(()) => {
                self.loaded.insert(spec);
                Ok(())
            }
        }
    }

    /// One call of `__picture` for `ask`, with a draw's limit: its JSON, or why it gave none.
    fn call(&mut self, ask: &Ask) -> Result<String, Stop> {
        *self.limit.lock().expect("the limit") = Limit::clocked(DRAW, DRAW_CLOCK);
        let state = self.state.clone();
        let call = self.call.clone();
        let out = self.ctx.with(|ctx| {
            let run = || -> rquickjs::Result<String> {
                let f = call.restore(&ctx)?;
                f.call((ask.descriptor.id.as_str(), ask.view.as_str(), ask.values.as_str(), ask.memo.as_str(), ask.rate))
            };
            run().catch(&ctx).map_err(|e| caught(&mut state.borrow_mut(), e, false).message)
        });
        // Past its limit it was stopped, whatever it gave: a built-in that ran long isn't interrupted.
        if self.limit.lock().expect("the limit").passed() {
            return Err(Stop::Hard("took too long and was stopped".into()));
        }
        match out {
            Ok(json) => Ok(json),
            // QuickJS says running out of memory as an exception with no message, or as `null`.
            Err(m) if m.contains("out of memory") || m.is_empty() || m == "null" || m == "error" => {
                Err(Stop::Hard("used too much memory and was stopped".into()))
            }
            Err(m) => Err(Stop::Plain(m)),
        }
    }
}

// ---------------------------------------------------------------- what a draw gives, checked here

/// A tone (CARD-61).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Main,
    Soft,
    Faint,
}

/// Which axis a mark is across.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisName {
    X,
    Y,
}

/// A value a dot moves, by its name; `by: "knob"` when it moves as its knob's drag does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<By>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    Knob,
}

/// What a dot moves (CARD-61).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Moves {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Move>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Move>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheel: Option<Move>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggles: Option<Move>,
}

/// A shape of a picture (CARD-61), as this module reads it and writes it again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Shape {
    Line {
        points: Vec<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    Fill {
        points: Vec<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
    },
    Bars {
        points: Vec<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
    },
    Handle {
        x: Option<f64>,
        y: Option<f64>,
        moves: Moves,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opens: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mark: Option<String>,
        off: bool,
    },
    Mark {
        axis: AxisName,
        at: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        words: Option<String>,
    },
    Label {
        x: f64,
        y: f64,
        words: String,
    },
    Play {
        #[serde(rename = "box")]
        place: [f64; 4],
        note: u8,
    },
    Part {
        name: String,
        shapes: Vec<Shape>,
    },
    /// The curve music2 measures from the call's own sound (CARD-80): its points are the server's
    /// to fill, never the draw's.
    Response {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// Its click's level in dB, within [`CLICK`]; the loudest when left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        level: Option<f64>,
    },
    /// One note's sound, as a copy of the call plays it (CARD-80): its points are the server's.
    Hit {
        note: u8,
        seconds: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<Tone>,
    },
}

/// The points a measured layer is filled with (CARD-80): a response's, and a hit's columns.
pub const MEASURED: (usize, usize) = (256, 256);
/// The quietest and the loudest a response's click plays, in dB (CARD-80): a draw that gives no
/// level plays the loudest, −20.
pub const CLICK: (f64, f64) = (-80.0, -20.0);

/// A draw, as checked (CARD-65): its shapes, its words, and what of `@m2/core` 2.5 it used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drawing {
    pub shapes: Vec<Shape>,
    pub words: String,
    /// What it used that came in 2.5 (CARD-87), for checks only (PACK-23): none, mostly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<Use>,
}

/// What a draw used that came in `@m2/core` 2.5 (CARD-87).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Use {
    /// The song's rate: the draw, or its words, read `p.rate`.
    Rate,
    /// A response's click at a level other than −20 dB (CARD-80).
    Level,
    /// A bare axis: the picture's grid and numbers left off (CARD-60).
    Bare,
}

/// Whether `t` is text of `lo` to `hi` characters (code points), with no control character, nor a
/// line or paragraph break (CARD-61).
pub fn fits(t: &str, lo: usize, hi: usize) -> bool {
    let n = t.chars().count();
    n >= lo && n <= hi && !t.chars().any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
}

/// A plain name, as a line's: a letter, `_` or `$`, then letters, digits, `_` or `$`.
fn plain(t: &str) -> bool {
    let mut c = t.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_' || f == '$')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_' || x == '$')
}

/// `json`, what a draw of picture `view` of `desc` gave, checked in Rust (CARD-65) and written again
/// from this module's own types; or why it can't be drawn, in the words the SDK uses.
pub fn checked(json: &str, desc: &Descriptor, view: &str) -> Result<String, String> {
    if json.len() > BYTES {
        return Err(format!("draws {} KB, more than {} KB", json.len() >> 10, BYTES >> 10));
    }
    let d: Drawing = serde_json::from_str(json).map_err(|e| format!("gives what music2 can't read as shapes ({e})"))?;
    let Some(ViewOf::Picture { x, y, .. }) = desc.editor.views.iter().find(|v| v.name == view).map(|v| &v.of) else {
        return Err(format!("is no picture named {view}"));
    };
    let mut numbers = 0;
    check(&d.shapes, desc, (x.unit, y.unit, x.hi), &mut numbers, false)?;
    measured_once(&d.shapes, &mut vec![])?;
    if !fits(&d.words, 1, 400) || d.words.trim().is_empty() {
        return Err(format!("its words must be one line of 1 to 400 characters, not {:?}", cut(&d.words)));
    }
    serde_json::to_string(&d).map_err(|e| format!("can't be written: {e}"))
}

/// `t` cut short for a refusal's words.
fn cut(t: &str) -> String {
    let s: String = t.chars().take(40).collect();
    if s.len() < t.len() { format!("{s}…") } else { s }
}

/// `axes`: the x and y axes' units, and where the x axis ends.
fn check(shapes: &[Shape], desc: &Descriptor, axes: (Unit, Unit, f64), numbers: &mut usize, inside: bool) -> Result<(), String> {
    let count = |numbers: &mut usize, n: usize| {
        *numbers += n;
        if *numbers > NUMBERS { Err("draws more than 20,000 numbers: draw fewer points".to_string()) } else { Ok(()) }
    };
    let pairs = |word: &str, pts: &[f64]| match pts.len() % 2 {
        0 => Ok(()),
        _ => Err(format!("gives {word} an odd count of numbers ({}): points are x, y pairs", pts.len())),
    };
    for s in shapes {
        match s {
            Shape::Line { points, name, .. } => {
                pairs("p.line", points)?;
                count(numbers, points.len())?;
                if let Some(n) = name
                    && !(fits(n, 1, 32) && plain(n))
                {
                    return Err(format!("gives p.line the name {:?}: a line's name is a plain name of up to 32 characters", cut(n)));
                }
            }
            Shape::Fill { points, .. } => {
                pairs("p.fill", points)?;
                count(numbers, points.len())?;
            }
            Shape::Bars { points, .. } => {
                pairs("p.bars", points)?;
                count(numbers, points.len())?;
            }
            Shape::Handle { moves, opens, mark, .. } => {
                count(numbers, 2)?;
                moved(desc, moves.x.as_ref(), "x", &axes.0)?;
                moved(desc, moves.y.as_ref(), "y", &axes.1)?;
                moved(desc, moves.wheel.as_ref(), "wheel", &axes.0)?;
                moved(desc, moves.toggles.as_ref(), "toggles", &axes.0)?;
                if let Some(o) = opens
                    && !desc.editor.groups.iter().any(|g| &g.name == o)
                {
                    let groups: Vec<&str> = desc.editor.groups.iter().map(|g| g.name.as_str()).collect();
                    let groups = if groups.is_empty() { "none".to_string() } else { groups.join(", ") };
                    return Err(format!("a dot opens {:?}, which isn't a group of this card (groups: {groups})", cut(o)));
                }
                if let Some(m) = mark
                    && !fits(m, 1, 3)
                {
                    return Err(format!("a dot's mark is 1 to 3 characters, not {:?}", cut(m)));
                }
            }
            Shape::Mark { words, .. } => {
                if let Some(w) = words
                    && !fits(w, 1, 80)
                {
                    return Err(format!("gives p.mark one line of 1 to 80 characters, not {:?}", cut(w)));
                }
            }
            Shape::Label { words, .. } => {
                if !fits(words, 1, 40) {
                    return Err(format!("gives p.label one line of 1 to 40 characters, not {:?}", cut(words)));
                }
            }
            Shape::Play { note, .. } => {
                count(numbers, 4)?;
                if *note > 127 {
                    return Err(format!("gives p.play the note {note}: a note is a whole number 0 to 127"));
                }
            }
            Shape::Response { name, level, .. } => {
                if (axes.0, axes.1) != (Unit::Hz, Unit::Db) {
                    return Err(format!(
                        "gives p.response an x axis in {} and a y axis in {}: a response is drawn across hz and up db",
                        axes.0.name(),
                        axes.1.name()
                    ));
                }
                if let Some(n) = name
                    && !(fits(n, 1, 32) && plain(n))
                {
                    return Err(format!("gives p.response the name {:?}: a line's name is a plain name of up to 32 characters", cut(n)));
                }
                if let Some(l) = level
                    && !(CLICK.0..=CLICK.1).contains(l)
                {
                    return Err(format!("gives p.response the level {l}: its click plays at −80 to −20 dB"));
                }
                count(numbers, 2 * MEASURED.0)?;
            }
            Shape::Hit { note, seconds, .. } => {
                if (axes.0, axes.1) != (Unit::Seconds, Unit::Bipolar) {
                    return Err(format!(
                        "gives p.hit an x axis in {} and a y axis in {}: a hit is drawn across seconds and up bipolar",
                        axes.0.name(),
                        axes.1.name()
                    ));
                }
                if *note > 127 {
                    return Err(format!("gives p.hit the note {note}: a note is a whole number 0 to 127"));
                }
                if !(*seconds > 0.0 && *seconds <= 1.0) {
                    return Err(format!("gives p.hit {seconds} seconds: a hit lasts more than 0 and at most 1 second"));
                }
                if *seconds > axes.2 {
                    return Err(format!("gives p.hit {seconds} seconds, past its x axis's end at {} s", axes.2));
                }
                count(numbers, 4 * MEASURED.1)?;
            }
            Shape::Part { name, shapes } => {
                if inside {
                    return Err("makes a part inside a part: parts don't nest".into());
                }
                if !fits(name, 1, 32) {
                    return Err(format!("gives p.part a name of 1 to 32 characters, not {:?}", cut(name)));
                }
                check(shapes, desc, axes, numbers, true)?;
            }
        }
    }
    Ok(())
}

/// Each measured layer of `shapes` (a part's too) at most once (CARD-80).
fn measured_once(shapes: &[Shape], seen: &mut Vec<&'static str>) -> Result<(), String> {
    for s in shapes {
        let word = match s {
            Shape::Response { .. } => "p.response",
            Shape::Hit { .. } => "p.hit",
            Shape::Part { shapes, .. } => {
                measured_once(shapes, seen)?;
                continue;
            }
            _ => continue,
        };
        if seen.contains(&word) {
            return Err(format!("gives {word} twice: a picture holds one at most"));
        }
        seen.push(word);
    }
    Ok(())
}

/// A dot's value `m`, moved as `how` (`x`, `y`, `wheel`, `toggles`), checked against `desc`
/// (CARD-62): a control of this sound, of the kind its move needs.
fn moved(desc: &Descriptor, m: Option<&Move>, how: &str, axis: &Unit) -> Result<(), String> {
    let Some(m) = m else { return Ok(()) };
    let n = cut(&m.name);
    let Some(c) = desc.controls.iter().find(|c| c.name == m.name && c.each.is_none()) else {
        return Err(format!("names {n}, which this sound doesn't have"));
    };
    let kind = match &c.unit {
        Some(Unit::Switch) => "a switch".to_string(),
        Some(Unit::Choice(_)) => "a choice".to_string(),
        Some(u) => format!("a control in {}", u.name()),
        None => "a control with no unit".to_string(),
    };
    let number = !matches!(c.unit, Some(Unit::Switch) | Some(Unit::Choice(_)));
    if m.by.is_some() && how != "x" && how != "y" {
        return Err(format!("moves {n} with {how} as its knob: only x and y move as a knob does"));
    }
    let ok = match how {
        "toggles" => c.unit == Some(Unit::Switch),
        _ if m.by.is_some() || how == "wheel" => number,
        _ => number && c.unit.as_ref() == Some(axis),
    };
    if ok {
        return Ok(());
    }
    Err(match how {
        "toggles" => format!("flips {n}, {kind}, but only a switch flips"),
        _ if m.by.is_some() => format!("moves {n} as its knob, {kind}, but a knob's drag moves a number"),
        "wheel" => format!("moves {n} with the wheel, {kind}, but the wheel moves a number"),
        _ => format!("moves {n} {}, {kind}, but its {how} axis is in {}", if how == "x" { "across" } else { "up and down" }, axis.name()),
    })
}

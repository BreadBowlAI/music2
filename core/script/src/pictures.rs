//! The picture runner (CARD-64): a sound's pictures, drawn by its own TypeScript on the server, in
//! the sandbox a build runs in, never in the window.
//!
//! - **One thread.** Every draw runs on the runner's own thread, apart from the session's builds,
//!   so a compile never freezes a curve mid-drag and a curve never waits for a compile.
//! - **Kept runtimes.** One for each package folder a song's build resolved (a locked version is a
//!   folder of its own) and one for the song's own code at each code revision, [`KEPT`] at most,
//!   the one used longest ago let go first. Each loads the module that declares the sound once,
//!   with the build's own limit, and keeps it.
//! - **A limit for each draw.** Counted from the draw's start ([`DRAW`]), never from when the
//!   runtime was made; one past it is stopped, said in words, and its runtime made again.
//! - **Frozen inputs.** `@m2/core`'s `__picture` gives `draw` and `words` the values frozen, checks
//!   what they give back, and says each mistake in words (CARD-65).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use rquickjs::{CatchResultExt, Context, Function, Module, Runtime};

use super::{CORE, JS_STACK, Limit, PROJECT, STACK, State, caught, host, package_of};

/// The runtimes kept at most.
const KEPT: usize = 16;
/// The memory each runtime may use.
const MEMORY: usize = 64 << 20;
/// How long loading a picture's module may work: a build's limit.
const LOAD: Duration = Duration::from_secs(10);
/// How long one draw may work (CARD-64), and by the clock ten times that.
const DRAW: Duration = Duration::from_millis(20);

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
    /// The packages the song's build resolved, by name: what the module imports from.
    pub packages: Vec<(String, PathBuf)>,
    /// The song's code, when the sound is the song's own.
    pub song: Option<Arc<Song>>,
    /// The module that declares the sound, as its descriptor names it (`@m2/mixing`, `lib/eq.ts`).
    pub module: String,
    /// The ComponentId (`@m2/mixing:eq`, or a song's own bare id).
    pub component: String,
    /// The picture's name among its views.
    pub view: String,
    /// What plays, as a JSON object by value name (CARD-63).
    pub values: String,
    /// The picture asked for, which its parts are kept by (CARD-64).
    pub memo: String,
}

/// What a draw gave.
#[derive(Clone, Debug, PartialEq)]
pub enum Drawn {
    /// `{"shapes": […], "words": "…"}`.
    Shapes(String),
    /// Why it can't be drawn, in words that follow "Its picture" ("names freq9, which this sound
    /// doesn't have").
    Cant(String),
}

struct Job {
    ask: Ask,
    reply: mpsc::SyncSender<Drawn>,
}

/// The picture runner: draws on a thread of its own, which ends once the runner is dropped.
pub struct Runner {
    jobs: mpsc::Sender<Job>,
}

impl Default for Runner {
    fn default() -> Runner {
        Runner::new()
    }
}

impl Runner {
    pub fn new() -> Runner {
        let (jobs, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("m2-pictures".into())
            .stack_size(STACK)
            .spawn(move || {
                m2_os::thread::c_numbers();
                let mut kept = Kept::default();
                for job in rx {
                    let _ = job.reply.send(kept.draw(&job.ask));
                }
            })
            .expect("a thread for pictures");
        Runner { jobs }
    }

    /// Draw `ask` on the runner's thread, and wait for it.
    pub fn draw(&self, ask: Ask) -> Drawn {
        let gone = || Drawn::Cant("can't be drawn: the picture runner stopped".into());
        let (reply, answer) = mpsc::sync_channel(1);
        if self.jobs.send(Job { ask, reply }).is_err() {
            return gone();
        }
        answer.recv().unwrap_or_else(|_| gone())
    }
}

/// Which runtime a draw runs in.
#[derive(Clone, PartialEq, Eq)]
enum Key {
    /// A package's, by its name, with the packages it was resolved among.
    Package(String, Vec<(String, PathBuf)>),
    /// The song's own code at a code revision.
    Song(u64, Vec<(String, PathBuf)>),
}

/// A runtime kept for its key: what it loaded, and when it was last used.
struct One {
    key: Key,
    // The context goes before its runtime.
    ctx: Context,
    _rt: Runtime,
    state: Rc<RefCell<State>>,
    limit: Arc<Mutex<Limit>>,
    loaded: HashSet<String>,
    used: u64,
}

#[derive(Default)]
struct Kept {
    ones: Vec<One>,
    clock: u64,
}

impl Kept {
    fn draw(&mut self, ask: &Ask) -> Drawn {
        let key = match &ask.song {
            Some(s) => Key::Song(s.key, ask.packages.clone()),
            None => Key::Package(package_of(&ask.module).0.to_string(), ask.packages.clone()),
        };
        self.clock += 1;
        let at = match self.ones.iter().position(|o| o.key == key) {
            Some(i) => i,
            None => match made(key, ask) {
                Ok(one) => {
                    if self.ones.len() >= KEPT
                        && let Some(old) = self.ones.iter().enumerate().min_by_key(|(_, o)| o.used).map(|(i, _)| i)
                    {
                        self.ones.swap_remove(old);
                    }
                    self.ones.push(one);
                    self.ones.len() - 1
                }
                Err(why) => return Drawn::Cant(why),
            },
        };
        self.ones[at].used = self.clock;
        let (drawn, broken) = self.ones[at].draw(ask);
        if broken {
            self.ones.swap_remove(at);
        }
        drawn
    }
}

/// A new runtime for `key`: the build's sandbox (its prelude, `Math` on libm, the loader over the
/// packages and the song's code), with `@m2/core`'s `__picture` at hand.
fn made(key: Key, ask: &Ask) -> Result<One, String> {
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
    ctx.with(|ctx| {
        let run = || -> rquickjs::Result<()> {
            host(&ctx, &state, &packages)?;
            let main = format!("import {{ __picture }} from \"{CORE}\";\nglobalThis.__m2_picture = __picture;\n");
            Module::evaluate(ctx.clone(), "<picture>", main)?.finish::<()>()
        };
        run().catch(&ctx).map_err(|e| format!("can't be drawn: {}", caught(&mut state.borrow_mut(), e, false).message))
    })?;
    Ok(One { key, ctx, _rt: rt, state, limit, loaded: HashSet::new(), used: 0 })
}

impl One {
    /// Draw `ask` here; and whether this runtime is to be made again (it was stopped, or ran out of memory).
    fn draw(&mut self, ask: &Ask) -> (Drawn, bool) {
        let spec = match ask.song {
            Some(_) => format!("{PROJECT}{}", ask.module),
            None => ask.module.clone(),
        };
        if !self.loaded.contains(&spec) {
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
            if let Err(why) = loaded {
                return (Drawn::Cant(format!("can't be drawn: its module {} doesn't load: {why}", ask.module)), true);
            }
            self.loaded.insert(spec);
        }
        *self.limit.lock().expect("the limit") = Limit::new(DRAW);
        let state = self.state.clone();
        let out = self.ctx.with(|ctx| {
            let run = || -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__m2_picture")?;
                f.call((ask.component.as_str(), ask.view.as_str(), ask.values.as_str(), ask.memo.as_str()))
            };
            run().catch(&ctx).map_err(|e| caught(&mut state.borrow_mut(), e, false).message)
        });
        let stopped = self.limit.lock().expect("the limit").passed();
        match out {
            Err(_) if stopped => (Drawn::Cant("took too long and was stopped".into()), true),
            Err(m) if m.contains("out of memory") => (Drawn::Cant("used too much memory and was stopped".into()), true),
            Err(m) => (Drawn::Cant(format!("couldn't be drawn: {m}")), true),
            Ok(json) if json.starts_with("{\"why\":") => {
                let why = serde_json::from_str::<serde_json::Value>(&json).ok().and_then(|v| v["why"].as_str().map(String::from));
                (Drawn::Cant(why.unwrap_or(json)), false)
            }
            Ok(json) => (Drawn::Shapes(json), false),
        }
    }
}

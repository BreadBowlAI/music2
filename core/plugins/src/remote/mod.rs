//! The host process (HOST-3): [`Remote`] runs plugins in a program of their own, so a plugin that
//! crashes or hangs takes only that process down, and the app plays on.
//!
//! In short:
//! - The process is this program run again with `--m2-plugin-host` ([`crate::run_main`] and
//!   [`crate::use_app_main_thread`] serve when they see it); tests run the small `m2-plugin-host`
//!   program. It starts at the first call that needs it, and again after it ended. Inside it,
//!   [`crate::InProcess`] runs every plugin under `run_main`: its main-thread calls on the
//!   process's own main loop (HOST-2).
//! - Main-thread calls are messages on its standard input and output (`wire.rs`). One reader
//!   thread here routes each reply to its caller, and each plugin's own events to its [`Owner`].
//! - `process` and `reset` cross in one shared-memory slot per active instance (`slot.rs`): copy
//!   in, ring, spin, then sleep; one audio thread per instance in the host process answers. Nothing
//!   on this side allocates, locks or waits on anything but the call (RENDER-28): instances are in
//!   a fixed slab, as `InProcess`'s are.
//! - When the process ends (a crash: its output closes), every one of its instances is marked
//!   crashed: its calls fail (`Status::Error`, so the engine's fail-live path runs) and its owner
//!   hears [`HostEvent::Crashed`] ([`HostEvent::Hung`] after the watchdog).
//! - Every wait has a limit (D5). A live call that waits [`HANG`] makes the watchdog end the
//!   process, the same way. So does an offline call, a main-thread call or an answer awaited that
//!   waits [`LIMIT`] (a scan, which loads every library: four times as long). Then a plugin
//!   stuck on the process's main loop can't hold up the session (or an export) for good.

pub(crate) mod child;
pub(crate) mod slot;
pub(crate) mod wire;

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use crossbeam_channel::bounded;
use m2_os::ipc::Shm;

use crate::host::*;
use crate::{Found, Info, MAX_INSTANCES, Transport};
use wire::{R, Req, W};

/// A live call that waits this long is a hang: the process is ended (RENDER-28's stall time).
pub const HANG: Duration = Duration::from_millis(500);
/// Any other call that waits this long is a hang too: offline, on the main thread, or an answer
/// awaited ([`Remote::set_limit`] changes it).
pub const LIMIT: Duration = Duration::from_secs(30);

/// The flag that makes a program serve as the host process.
pub const FLAG: &str = "--m2-plugin-host";
/// Its second flag: install the probe there too (tests).
pub(crate) const PROBE_FLAG: &str = "--m2-probe";

/// Plugins in a host process of their own (HOST-3). Cheap to clone: one set of instances.
#[derive(Clone)]
pub struct Remote {
    s: Arc<Shared>,
}

struct Shared {
    program: PathBuf,
    args: Vec<String>,
    /// The process now, while it runs.
    current: Mutex<Option<Arc<Proc>>>,
    entries: Box<[Entry]>,
    /// Slots freed, and the first never used.
    free: Mutex<(Vec<u32>, u32)>,
    /// Slots ever used: the watchdog looks at these.
    used: AtomicU32,
    created: AtomicU64,
    destroyed: AtomicU64,
    epoch: Instant,
    procs: AtomicU64,
    watchdog: Once,
    /// [`LIMIT`] in ns, or what [`Remote::set_limit`] set.
    limit: AtomicU64,
}

/// An instance, as this side knows it. The audio thread reads only its atomics.
struct Entry {
    generation: AtomicU32,
    /// Its mapped slot, while active; null otherwise.
    mem: AtomicPtr<u8>,
    max_frames: AtomicU32,
    /// Its process ended.
    dead: AtomicBool,
    offline: AtomicBool,
    /// The call in flight on its slot, for the watchdog ([`slot::since`]); 0: none.
    since: AtomicU64,
    /// Its process's number (0: none).
    proc_id: AtomicU64,
    main: Mutex<Main>,
}

#[derive(Default)]
struct Main {
    /// Its handle in the host process.
    child: u64,
    proc_: Option<Arc<Proc>>,
    owner: Option<Owner>,
    shm: Option<Shm>,
}

type Then = Box<dyn FnOnce(Result<Vec<u8>, String>) + Send>;

/// One run of the host process.
struct Proc {
    id: u64,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    /// Replies awaited, by request id: when each is late (ns since the epoch), and who hears it.
    pending: Mutex<HashMap<u64, (u64, Then)>>,
    next: AtomicU64,
    dead: AtomicBool,
    /// Ended on purpose: its instances aren't said to have crashed.
    closing: AtomicBool,
    /// The watchdog ended it: it stopped answering.
    hung: AtomicBool,
    /// Its instances' handles there, to ours.
    handles: Mutex<HashMap<u64, Handle>>,
}

impl Drop for Shared {
    /// The last handle on these instances went: so does their process.
    fn drop(&mut self) {
        if let Some(p) = locked(&self.current).take() {
            p.closing.store(true, Ordering::Release);
            let _ = locked(&p.child).kill();
        }
    }
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Remote {
    /// The host process is `program` run with `args`.
    pub fn new(program: PathBuf, args: Vec<String>) -> Remote {
        Remote {
            s: Arc::new(Shared {
                program,
                args,
                current: Mutex::new(None),
                entries: (0..MAX_INSTANCES)
                    .map(|_| Entry {
                        generation: AtomicU32::new(1),
                        mem: AtomicPtr::new(null_mut()),
                        max_frames: AtomicU32::new(0),
                        dead: AtomicBool::new(false),
                        offline: AtomicBool::new(false),
                        since: AtomicU64::new(0),
                        proc_id: AtomicU64::new(0),
                        main: Mutex::default(),
                    })
                    .collect(),
                free: Mutex::new((vec![], 0)),
                used: AtomicU32::new(0),
                created: AtomicU64::new(0),
                destroyed: AtomicU64::new(0),
                epoch: Instant::now(),
                procs: AtomicU64::new(0),
                watchdog: Once::new(),
                limit: AtomicU64::new(LIMIT.as_nanos() as u64),
            }),
        }
    }

    /// The host process is this program, run again with [`FLAG`] (D2: the two sides never differ).
    pub fn this_program() -> Result<Remote> {
        let me = std::env::current_exe().context("this program's path, to run it again as the plugin host")?;
        Ok(Remote::new(me, vec![FLAG.into()]))
    }

    /// Tests: `$M2_PLUGIN_HOST`, else the `m2-plugin-host` program cargo built beside the test
    /// (`target/<profile>/`). Build it first: `cargo build -p m2-plugins --bin m2-plugin-host`.
    pub fn for_tests() -> Remote {
        let program = std::env::var_os("M2_PLUGIN_HOST").map(PathBuf::from).unwrap_or_else(|| {
            let exe = std::env::current_exe().expect("the test's path");
            let dir = exe.parent().expect("a folder");
            let dir = if dir.ends_with("deps") { dir.parent().expect("the profile's folder") } else { dir };
            dir.join(format!("m2-plugin-host{}", std::env::consts::EXE_SUFFIX))
        });
        assert!(program.exists(), "{} isn't built: cargo build -p m2-plugins --bin m2-plugin-host", program.display());
        Remote::new(program, vec![])
    }

    /// The host process's id while it runs (tests end it to play a crash).
    pub fn process_id(&self) -> Option<u32> {
        locked(&self.s.current).as_ref().filter(|p| !p.dead.load(Ordering::Acquire)).map(|p| locked(&p.child).id())
    }

    /// How long a call that isn't live may wait before its process is ended ([`LIMIT`]; a scan
    /// four times as long). Tests make it short.
    pub fn set_limit(&self, limit: Duration) {
        self.s.limit.store(limit.as_nanos() as u64, Ordering::Relaxed);
    }

    /// End the host process: quit. Its instances aren't said to have crashed; their calls fail.
    pub fn shutdown(&self) {
        if let Some(p) = locked(&self.s.current).take() {
            p.closing.store(true, Ordering::Release);
            let _ = locked(&p.child).kill();
        }
    }

    fn entry(&self, h: Handle) -> Option<&Entry> {
        let e = self.s.entries.get((h.0 >> 32) as usize)?;
        (e.generation.load(Ordering::Acquire) == h.0 as u32).then_some(e)
    }

    /// The running process, started when there is none.
    fn running(&self) -> Result<Arc<Proc>> {
        let mut cur = locked(&self.s.current);
        if let Some(p) = cur.as_ref().filter(|p| !p.dead.load(Ordering::Acquire)) {
            return Ok(p.clone());
        }
        let p = self.spawn()?;
        *cur = Some(p.clone());
        Ok(p)
    }

    fn spawn(&self) -> Result<Arc<Proc>> {
        let mut cmd = Command::new(&self.s.program);
        cmd.args(&self.s.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        if crate::probe::installed() {
            cmd.arg(PROBE_FLAG);
        }
        let mut child = cmd.spawn().with_context(|| format!("starting the plugin host {}", self.s.program.display()))?;
        let (stdin, stdout) = (child.stdin.take().context("its input")?, child.stdout.take().context("its output")?);
        let p = Arc::new(Proc {
            id: self.s.procs.fetch_add(1, Ordering::AcqRel) + 1,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Mutex::default(),
            next: AtomicU64::new(1),
            dead: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            hung: AtomicBool::new(false),
            handles: Mutex::default(),
        });
        let (s, rp) = (Arc::downgrade(&self.s), p.clone());
        std::thread::Builder::new()
            .name("m2-plugin-host-reader".into())
            .spawn(move || read(s, rp, stdout))
            .context("a thread to read the plugin host")?;
        let s = Arc::downgrade(&self.s);
        self.s.watchdog.call_once(|| {
            let _ = std::thread::Builder::new().name("m2-plugin-host-watchdog".into()).spawn(move || watchdog(s));
        });
        let hello = self.call(&p, &Req::Hello).context("the plugin host didn't start")?;
        if R::new(&hello).u64()? != wire::STAMP {
            p.closing.store(true, Ordering::Release);
            let _ = locked(&p.child).kill();
            bail!(
                "the plugin host {} is from another build of music2: build it again (cargo build -p m2-plugins --bin m2-plugin-host)",
                self.s.program.display()
            );
        }
        Ok(p)
    }

    /// Send `req`; `then` hears its reply, or why none came: exactly once. A reply that doesn't
    /// come in time ends the process (the watchdog), and so fails.
    fn send(&self, p: &Proc, req: &Req, then: Then) {
        let id = p.next.fetch_add(1, Ordering::AcqRel);
        // A scan loads every plugin's library, which takes a while the first time (macOS checks each).
        let limit = self.s.limit.load(Ordering::Relaxed) * if matches!(req, Req::Scan) { 4 } else { 1 };
        let late = self.s.epoch.elapsed().as_nanos() as u64 + limit;
        {
            let mut pending = locked(&p.pending);
            if p.dead.load(Ordering::Acquire) {
                drop(pending);
                return then(Err(ended(p)));
            }
            pending.insert(id, (late, then));
        }
        let mut w = W::frame();
        w.req(id, req);
        // A write that fails means the process is gone: the reader then fails what waits.
        let _ = locked(&p.stdin).write_all(&w.done());
    }

    /// Send `req` and wait for its reply.
    fn call(&self, p: &Proc, req: &Req) -> Result<Vec<u8>> {
        let (tx, rx) = bounded(1);
        self.send(
            p,
            req,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        rx.recv().map_err(|_| anyhow!("the plugin host didn't answer"))?.map_err(|e| anyhow!(e))
    }

    /// `h`'s process and its handle there; an error when it ended or `h` is gone.
    fn target(&self, h: Handle) -> Result<(Arc<Proc>, u64)> {
        let e = self.entry(h).ok_or_else(|| anyhow!("plugin instance {h:?} no longer exists"))?;
        let m = locked(&e.main);
        let p = m.proc_.clone().ok_or_else(|| anyhow!("plugin instance {h:?} no longer exists"))?;
        if e.dead.load(Ordering::Acquire) {
            bail!("{}", ended(&p));
        }
        Ok((p, m.child))
    }

    fn call_on(&self, h: Handle, req: impl FnOnce(u64) -> Req) -> Result<Vec<u8>> {
        let (p, child) = self.target(h)?;
        self.call(&p, &req(child))
    }

    fn queued<T: Send + 'static>(
        &self,
        h: Handle,
        req: impl FnOnce(u64) -> Req,
        read: impl FnOnce(&mut R) -> Result<T> + Send + 'static,
    ) -> Pending<Result<T, String>> {
        let (tx, rx) = bounded(1);
        match self.target(h) {
            Ok((p, child)) => self.send(
                &p,
                &req(child),
                Box::new(move |r| {
                    let _ = tx.send(r.and_then(|b| read(&mut R::new(&b)).map_err(|e| format!("{e:#}"))));
                }),
            ),
            Err(e) => drop(tx.send(Err(format!("{e:#}")))),
        }
        rx
    }

    fn alloc(&self) -> Result<u32> {
        let mut f = locked(&self.s.free);
        if let Some(i) = f.0.pop() {
            return Ok(i);
        }
        if f.1 as usize == MAX_INSTANCES {
            bail!("more than {MAX_INSTANCES} plugin instances at once");
        }
        f.1 += 1;
        self.s.used.store(f.1, Ordering::Release);
        Ok(f.1 - 1)
    }

    fn waiter<'a>(&'a self, e: &'a Entry) -> slot::Waiter<'a> {
        slot::Waiter { dead: &e.dead, since: &e.since, live: !e.offline.load(Ordering::Relaxed), epoch: self.s.epoch }
    }
}

/// Why a process's calls fail once it ended.
fn ended(p: &Proc) -> String {
    match (p.closing.load(Ordering::Acquire), p.hung.load(Ordering::Acquire)) {
        (true, _) => "the plugin host was closed".into(),
        (_, true) => "the plugin host stopped answering, so music2 ended it".into(),
        _ => "the plugin host crashed".into(),
    }
}

/// The reader thread: each reply to its caller, each event to its instance's owner; at the end of
/// the output, every instance of the process is marked crashed.
fn read(s: Weak<Shared>, p: Arc<Proc>, out: std::process::ChildStdout) {
    let mut out = std::io::BufReader::new(out);
    while let Ok(Some(frame)) = wire::read_frame(&mut out) {
        let mut r = R::new(&frame);
        match r.u8() {
            Ok(wire::REPLY) => {
                let Ok(id) = r.u64() else { break };
                let Some((_, then)) = locked(&p.pending).remove(&id) else { continue };
                then(match r.u8() {
                    Ok(1) => Ok(frame[r.at()..].to_vec()),
                    _ => Err(r.str().unwrap_or_else(|e| format!("{e:#}"))),
                });
            }
            Ok(wire::EVENT) => {
                let (Ok(e), Some(s)) = (r.event(), s.upgrade()) else { continue };
                let Some(&h) = locked(&p.handles).get(&event_handle(&e).0) else { continue };
                let Some(entry) = Remote { s }.entry(h).map(|e| locked(&e.main).owner.clone()) else { continue };
                if let Some(owner) = entry {
                    owner.send(with_handle(e, h));
                }
            }
            _ => break,
        }
    }
    // Ended: what waits fails, then every instance is marked and its owner told.
    let waiting: Vec<Then> = {
        let mut pending = locked(&p.pending);
        p.dead.store(true, Ordering::Release);
        pending.drain().map(|(_, (_, t))| t).collect()
    };
    for then in waiting {
        then(Err(ended(&p)));
    }
    let _ = locked(&p.child).wait();
    let Some(s) = s.upgrade() else { return };
    let said = !p.closing.load(Ordering::Acquire);
    let hung = p.hung.load(Ordering::Acquire);
    let used = s.used.load(Ordering::Acquire) as usize;
    for (i, e) in s.entries[..used].iter().enumerate() {
        if e.proc_id.load(Ordering::Acquire) != p.id {
            continue;
        }
        e.dead.store(true, Ordering::Release);
        let mem = e.mem.load(Ordering::Acquire);
        if !mem.is_null() {
            // SAFETY: the slot stays mapped while its entry holds it; the wake only reads its word.
            m2_os::ipc::wake(&unsafe { slot::View::new(mem, 0) }.header().done);
        }
        let owner = locked(&e.main).owner.clone();
        if let (true, Some(owner)) = (said, owner) {
            let h = Handle((i as u64) << 32 | e.generation.load(Ordering::Acquire) as u64);
            owner.send(if hung { HostEvent::Hung(h) } else { HostEvent::Crashed(h) });
        }
    }
    let mut cur = locked(&s.current);
    if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, &p)) {
        *cur = None;
    }
}

fn event_handle(e: &HostEvent) -> Handle {
    match *e {
        HostEvent::Restart(h)
        | HostEvent::Dirty(h)
        | HostEvent::Latency(h, _)
        | HostEvent::Crashed(h)
        | HostEvent::Hung(h)
        | HostEvent::WindowClosed(h) => h,
    }
}

fn with_handle(e: HostEvent, h: Handle) -> HostEvent {
    match e {
        HostEvent::Restart(_) => HostEvent::Restart(h),
        HostEvent::Dirty(_) => HostEvent::Dirty(h),
        HostEvent::Latency(_, f) => HostEvent::Latency(h, f),
        HostEvent::Crashed(_) => HostEvent::Crashed(h),
        HostEvent::Hung(_) => HostEvent::Hung(h),
        HostEvent::WindowClosed(_) => HostEvent::WindowClosed(h),
    }
}

/// Every 20 ms: a live call in flight for [`HANG`] or more ends its process (D5); so does an
/// offline call in flight for the limit ([`LIMIT`]), or a reply that is late.
fn watchdog(s: Weak<Shared>) {
    loop {
        std::thread::sleep(Duration::from_millis(20));
        let Some(s) = s.upgrade() else { return };
        let ended = |p: &Arc<Proc>| p.dead.load(Ordering::Acquire) || p.hung.load(Ordering::Acquire);
        let Some(p) = locked(&s.current).clone().filter(|p| !ended(p)) else { continue };
        let now = s.epoch.elapsed().as_nanos() as u64;
        let limit = s.limit.load(Ordering::Relaxed);
        let used = s.used.load(Ordering::Acquire) as usize;
        let call = s.entries[..used].iter().find_map(|e| {
            let (at, live) = slot::began(e.since.load(Ordering::Relaxed));
            let most = if live { HANG.as_nanos() as u64 } else { limit };
            (at != 0 && now.saturating_sub(at) >= most && e.proc_id.load(Ordering::Acquire) == p.id).then_some(live)
        });
        let late = locked(&p.pending).values().any(|(late, _)| now >= *late);
        let why = match (call, late) {
            (Some(true), _) => format!("a live call waited {} ms", HANG.as_millis()),
            (Some(false), _) => format!("an offline call waited {} s", limit / 1_000_000_000),
            (None, true) => "it didn't answer a call in time".to_string(),
            (None, false) => continue,
        };
        p.hung.store(true, Ordering::Release);
        eprintln!("music2: the plugin host stopped answering ({why}), so it was ended");
        let _ = locked(&p.child).kill();
    }
}

impl PluginHost for Remote {
    fn create(&self, plugin: &Found, state: Option<Arc<Blob>>, owner: &Owner) -> Result<(Handle, Info)> {
        let _shown = owner.creating(&plugin.name);
        let p = self.running()?;
        let b = self.call(&p, &Req::Create { found: plugin.clone(), state: state.map(|b| (*b).clone()) })?;
        let mut r = R::new(&b);
        let (child, info) = (r.u64()?, r.info()?);
        let index = match self.alloc() {
            Ok(i) => i,
            Err(e) => {
                self.send(&p, &Req::Destroy { h: child }, Box::new(|_| {}));
                return Err(e);
            }
        };
        let e = &self.s.entries[index as usize];
        let h = Handle((index as u64) << 32 | e.generation.load(Ordering::Acquire) as u64);
        *locked(&e.main) = Main { child, proc_: Some(p.clone()), owner: Some(owner.clone()), shm: None };
        e.mem.store(null_mut(), Ordering::Release);
        e.offline.store(false, Ordering::Relaxed);
        e.since.store(0, Ordering::Relaxed);
        e.dead.store(false, Ordering::Release);
        e.proc_id.store(p.id, Ordering::Release);
        locked(&p.handles).insert(child, h);
        // It ended between its reply and now: the reader may have missed it.
        if p.dead.load(Ordering::Acquire) {
            e.dead.store(true, Ordering::Release);
        }
        self.s.created.fetch_add(1, Ordering::AcqRel);
        owner.created();
        Ok((h, info))
    }

    fn activate(&self, h: Handle, sample_rate: u32, max_frames: u32) -> Result<u32> {
        let e = self.entry(h).ok_or_else(|| anyhow!("plugin instance {h:?} no longer exists"))?;
        let (p, child) = self.target(h)?;
        let mut m = locked(&e.main);
        // A slot of the size asked for: the one it has, or a new one mapped there first.
        let fresh = match &m.shm {
            Some(_) if e.max_frames.load(Ordering::Relaxed) == max_frames => None,
            _ => {
                static SEQ: AtomicU64 = AtomicU64::new(0);
                let name = format!("/m2-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
                let shm = Shm::create(&name, slot::size(max_frames)).context("shared memory for a plugin's audio")?;
                // SAFETY: just made, of that size.
                unsafe { slot::View::new(shm.ptr(), max_frames) }.header().max_frames.store(max_frames, Ordering::Relaxed);
                Some((name, shm))
            }
        };
        let r = self.call(&p, &Req::Activate { h: child, rate: sample_rate, max_frames, shm: fresh.as_ref().map(|f| f.0.clone()) });
        // Whether the host process mapped the new slot: then it serves that one alone, whether or
        // not the plugin activated.
        let mut b = r.as_deref().map(R::new).ok();
        let mapped = match &mut b {
            Some(r) => r.bool()?,
            None => false,
        };
        if let Some((name, shm)) = fresh {
            // Both sides have it now, or the host never will (D10).
            let _ = Shm::remove(&name);
            e.max_frames.store(max_frames, Ordering::Relaxed);
            e.mem.store(if mapped { shm.ptr() } else { null_mut() }, Ordering::Release);
            m.shm = mapped.then_some(shm);
        }
        let Some(mut b) = b else { return r.map(|_| 0) };
        match b.u8()? {
            1 => Ok(b.u32()?),
            _ => bail!("{}", b.str()?),
        }
    }

    fn deactivate(&self, h: Handle) {
        let _ = self.call_on(h, |h| Req::Deactivate { h });
    }

    fn load_state(&self, h: Handle, blob: Arc<Blob>) -> Result<bool> {
        R::new(&self.call_on(h, |h| Req::LoadState { h, blob: (*blob).clone() })?).bool()
    }

    fn set_render_mode(&self, h: Handle, mode: RenderMode) -> Result<bool> {
        let r = R::new(&self.call_on(h, |h| Req::SetRenderMode { h, mode })?).bool()?;
        if let Some(e) = self.entry(h) {
            e.offline.store(mode == RenderMode::Offline, Ordering::Relaxed);
        }
        Ok(r)
    }

    fn main_call(&self, h: Handle, call: MainCall) -> Result<Reply> {
        R::new(&self.call_on(h, |h| Req::MainCall { h, call })?).reply()
    }

    fn destroy(&self, h: Handle) {
        let Some(e) = self.entry(h) else { return };
        let index = (h.0 >> 32) as u32;
        let m = std::mem::take(&mut *locked(&e.main));
        e.mem.store(null_mut(), Ordering::Release);
        e.proc_id.store(0, Ordering::Release);
        e.generation.store(e.generation.load(Ordering::Acquire).wrapping_add(1).max(1), Ordering::Release);
        locked(&self.s.free).0.push(index);
        drop(m.shm);
        let s = self.s.clone();
        let done = move || {
            s.destroyed.fetch_add(1, Ordering::AcqRel);
            if let Some(o) = m.owner {
                o.destroyed();
            }
        };
        match m.proc_ {
            Some(p) => {
                locked(&p.handles).remove(&m.child);
                self.send(&p, &Req::Destroy { h: m.child }, Box::new(move |_| done()));
            }
            None => done(),
        }
    }

    fn latency(&self, h: Handle) -> Pending<Result<u32, String>> {
        self.queued(h, |h| Req::Latency { h }, |r| r.u32())
    }

    fn save_state(&self, h: Handle) -> Pending<Result<Blob, String>> {
        self.queued(h, |h| Req::SaveState { h }, |r| r.blob())
    }

    fn process(&self, h: Handle, io: &mut SharedBuffers, ev_in: &Events, ev_out: &mut Events, t: &Transport) -> Status {
        let Some(e) = self.entry(h) else {
            slot::silence(io);
            return Status::Error;
        };
        let mem = e.mem.load(Ordering::Acquire);
        let max = e.max_frames.load(Ordering::Relaxed);
        if mem.is_null() || e.dead.load(Ordering::Acquire) || io.frames > max || ev_in.len() > MAX_CALL_EVENTS {
            slot::silence(io);
            return Status::Error;
        }
        // SAFETY: the slot stays mapped while the entry holds it, and only an activation or a
        // destroy, never during a call, changes that.
        let v = unsafe { slot::View::new(mem, max) };
        slot::process(&v, &self.waiter(e), io, ev_in, ev_out, t)
    }

    fn reset(&self, h: Handle) {
        let Some(e) = self.entry(h) else { return };
        let mem = e.mem.load(Ordering::Acquire);
        if !mem.is_null() && !e.dead.load(Ordering::Acquire) {
            // SAFETY: as in `process`.
            slot::reset(&unsafe { slot::View::new(mem, e.max_frames.load(Ordering::Relaxed)) }, &self.waiter(e));
        }
    }

    fn take_process_request(&self, h: Handle) -> bool {
        let Some(e) = self.entry(h) else { return false };
        let mem = e.mem.load(Ordering::Acquire);
        // SAFETY: as in `process`.
        !mem.is_null() && unsafe { slot::View::new(mem, 0) }.header().requested.swap(0, Ordering::Relaxed) != 0
    }

    fn scan(&self) -> (Vec<Found>, Vec<String>) {
        let got = self.running().and_then(|p| self.call(&p, &Req::Scan)).and_then(|b| {
            let mut r = R::new(&b);
            let found = (0..r.u32()?).map(|_| r.found()).collect::<Result<Vec<_>>>()?;
            Ok((found, r.strs()?))
        });
        got.unwrap_or_else(|e| (vec![], vec![format!("{e:#}")]))
    }

    fn live(&self) -> u64 {
        self.s.created.load(Ordering::Acquire) - self.s.destroyed.load(Ordering::Acquire)
    }

    fn counts(&self) -> (u64, u64) {
        (self.s.created.load(Ordering::Acquire), self.s.destroyed.load(Ordering::Acquire))
    }
}

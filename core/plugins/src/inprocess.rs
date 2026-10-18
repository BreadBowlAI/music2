//! The in-process [`PluginHost`] (HOST-2): plugins in this process, every main-thread call on the
//! plugin main thread ([`crate::main_thread`]), `process` on the caller's thread.
//!
//! Instances live in a fixed slab, so the audio thread finds one from its [`Handle`] with two
//! atomic loads and no lock. Only the main thread writes a slot: at create and at destroy. A handle
//! reaches the audio thread inside a renderer, after its create, and is destroyed only once no
//! renderer holds it, so a slot never changes under a `process` call.

use std::cell::UnsafeCell;
use std::ffi::{CString, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::gui::*;
use clap_sys::ext::posix_fd_support::*;
use clap_sys::ext::timer_support::*;
use clap_sys::id::clap_id;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::render::*;
use clap_sys::ext::state::*;
use clap_sys::ext::thread_check::*;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use crossbeam_channel::bounded;

use crate::{audio, gui};
use crate::host::*;
use crate::main_thread::{main_thread, token};
use crate::{Found, Info, Library, Param, Transport, cstr, fixed, library};

/// Instances alive at once, across the process: 50 tracks with a few plugins each, twice over
/// while an activation crossfades.
pub const MAX_INSTANCES: usize = 1024;

/// What a plugin's `clap_host` points to. Boxed, so its address never changes.
pub(crate) struct HostData {
    pub clap: clap_host,
    /// The thread holding the instance's audio role (`pthread_self`), 0 for none (§3.5).
    pub audio_owner: AtomicUsize,
    pub callback: AtomicBool,
    pub restart: AtomicBool,
    pub latency_changed: AtomicBool,
    pub dirty: AtomicBool,
    pub rescan: AtomicBool,
    pub process_requested: AtomicBool,
    /// Messages logged while it held the audio role: counted, never kept (no lock, no allocation).
    pub logs_dropped: AtomicU32,
    log: Mutex<Vec<String>>,
    /// The plugin, once the factory made it: what its timers and fds call.
    pub(crate) plugin: AtomicPtr<clap_plugin>,
    /// Its timers and watched fds on the main loop (HOST-3), by CLAP's ids and fds.
    pub(crate) next_timer: AtomicU32,
    pub(crate) timers: Mutex<Vec<(clap_id, m2_os::main_loop::Timer)>>,
    pub(crate) fds: Mutex<Vec<(i32, m2_os::main_loop::Watch)>>,
    /// A size it asked its window to be (width << 32 | height), 0 for none.
    pub(crate) resize: AtomicU64,
    /// Its window, while open (HOST-7): the main thread's.
    pub(crate) window: Mutex<Option<m2_os::window::Window>>,
}

/// Raised by any instance's flag; the tick thread then has the main thread sweep them.
pub(crate) static PENDING: AtomicBool = AtomicBool::new(false);

impl HostData {
    fn boxed() -> Box<HostData> {
        let mut h = Box::new(HostData {
            clap: clap_host {
                clap_version: CLAP_VERSION,
                host_data: null_mut(),
                name: c"music2".as_ptr(),
                vendor: c"music2".as_ptr(),
                url: c"https://github.com/".as_ptr(),
                version: c"0.1.0".as_ptr(),
                get_extension: Some(host_get_extension),
                request_restart: Some(audio::request_restart),
                request_process: Some(audio::request_process),
                request_callback: Some(audio::request_callback),
            },
            audio_owner: AtomicUsize::new(0),
            callback: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            latency_changed: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
            rescan: AtomicBool::new(false),
            process_requested: AtomicBool::new(false),
            logs_dropped: AtomicU32::new(0),
            log: Mutex::new(vec![]),
            plugin: AtomicPtr::new(null_mut()),
            next_timer: AtomicU32::new(1),
            timers: Mutex::new(vec![]),
            fds: Mutex::new(vec![]),
            resize: AtomicU64::new(0),
            window: Mutex::new(None),
        });
        h.clap.host_data = &*h as *const HostData as *mut c_void;
        h
    }
}

/// The host data behind a `clap_host` this host made.
///
/// # Safety
/// `h` is the `clap` field of a live [`HostData`].
pub(crate) unsafe fn host_data<'a>(h: *const clap_host) -> &'a HostData {
    unsafe { &*((*h).host_data as *const HostData) }
}

/// Keep a warning or an error a plugin logged off the audio role.
pub(crate) fn keep_log(hd: &HostData, msg: *const c_char) {
    if let Ok(mut l) = hd.log.lock()
        && l.len() < 256
    {
        l.push(cstr(msg));
    }
}

unsafe extern "C" fn host_get_extension(_: *const clap_host, id: *const c_char) -> *const c_void {
    let id = unsafe { std::ffi::CStr::from_ptr(id) };
    if id == CLAP_EXT_THREAD_CHECK {
        &THREAD_CHECK as *const _ as *const c_void
    } else if id == CLAP_EXT_LOG {
        &LOG as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &HOST_STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &HOST_PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &HOST_LATENCY as *const _ as *const c_void
    } else if id == CLAP_EXT_TIMER_SUPPORT {
        &HOST_TIMERS as *const _ as *const c_void
    } else if id == CLAP_EXT_POSIX_FD_SUPPORT && m2_os::main_loop::WATCHES_FDS {
        &HOST_FDS as *const _ as *const c_void
    } else if id == CLAP_EXT_GUI {
        &HOST_GUI as *const _ as *const c_void
    } else {
        null()
    }
}

static THREAD_CHECK: clap_host_thread_check =
    clap_host_thread_check { is_main_thread: Some(audio::is_main_thread), is_audio_thread: Some(audio::is_audio_thread) };
static LOG: clap_host_log = clap_host_log { log: Some(audio::host_log) };
static HOST_STATE: clap_host_state = clap_host_state { mark_dirty: Some(audio::mark_dirty) };
static HOST_PARAMS: clap_host_params =
    clap_host_params { rescan: Some(audio::params_rescan), clear: Some(audio::params_clear), request_flush: Some(audio::request_process) };
static HOST_LATENCY: clap_host_latency = clap_host_latency { changed: Some(audio::latency_changed) };
static HOST_TIMERS: clap_host_timer_support =
    clap_host_timer_support { register_timer: Some(gui::register_timer), unregister_timer: Some(gui::unregister_timer) };
static HOST_FDS: clap_host_posix_fd_support = clap_host_posix_fd_support {
    register_fd: Some(gui::register_fd),
    modify_fd: Some(gui::modify_fd),
    unregister_fd: Some(gui::unregister_fd),
};
static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(gui::resize_hints_changed),
    request_resize: Some(gui::request_resize),
    request_show: Some(gui::request_show),
    request_hide: Some(gui::request_hide),
    closed: Some(gui::gui_closed),
};

/// An instance. Its main side is touched only on the main thread; its audio side only by the
/// thread holding its audio role (and by the main thread while inactive or once no renderer
/// holds it).
pub(crate) struct Core {
    pub plugin: *const clap_plugin,
    pub host: Box<HostData>,
    _lib: Arc<Library>,
    pub(crate) owner: Owner,
    main: UnsafeCell<MainSide>,
    pub audio: UnsafeCell<AudioSide>,
}

// SAFETY: see the struct's comment; CLAP's thread model is what makes it sound.
unsafe impl Send for Core {}
unsafe impl Sync for Core {}

struct MainSide {
    info: Info,
    active: bool,
    /// The hash of the state it holds: the one it last saved or last loaded, at creation included
    /// (HOME-6). None while it plays its defaults.
    holds: Option<[u8; 32]>,
}

pub(crate) struct AudioSide {
    pub active: bool,
    /// `start_processing` was called: it is, lazily, at the first `process` after activating.
    pub processing: bool,
    pub max_frames: u32,
    /// Reserved when activated, so `process` never allocates (RENDER-28).
    pub ins: Vec<Vec<f32>>,
    pub outs: Vec<Vec<f32>>,
    pub in_ptrs: Vec<*mut f32>,
    pub out_ptrs: Vec<*mut f32>,
}

impl Core {
    #[allow(clippy::mut_from_ref)]
    fn main(&self) -> &mut MainSide {
        debug_assert!(crate::on_main_thread(), "an instance's main side, off the main thread");
        // SAFETY: only the main thread reaches it, one call at a time.
        unsafe { &mut *self.main.get() }
    }

    pub(crate) fn ext<T>(&self, id: &std::ffi::CStr) -> Option<&T> {
        ext(self.plugin, id)
    }

    /// Its name, as people know it (main thread).
    pub(crate) fn name(&self) -> String {
        self.main().info.found.name.clone()
    }
}

pub(crate) fn ext<'a, T>(plugin: *const clap_plugin, id: &std::ffi::CStr) -> Option<&'a T> {
    let get = unsafe { (*plugin).get_extension }?;
    let p = unsafe { get(plugin, id.as_ptr()) } as *const T;
    (!p.is_null()).then(|| unsafe { &*p })
}

struct Slot {
    generation: AtomicU32,
    core: AtomicPtr<Core>,
}

struct State {
    slots: Box<[Slot]>,
    /// Slots freed, and the first never used: the main thread's alone.
    free: Mutex<(Vec<u32>, u32)>,
    created: AtomicU64,
    destroyed: AtomicU64,
    tick: Once,
}

fn state() -> &'static State {
    static S: OnceLock<State> = OnceLock::new();
    S.get_or_init(|| State {
        slots: (0..MAX_INSTANCES).map(|_| Slot { generation: AtomicU32::new(1), core: AtomicPtr::new(null_mut()) }).collect(),
        free: Mutex::new((vec![], 0)),
        created: AtomicU64::new(0),
        destroyed: AtomicU64::new(0),
        tick: Once::new(),
    })
}

/// The instance `h` names, while it lives. Two atomic loads: the audio thread calls it.
pub(crate) fn core(h: Handle) -> Option<&'static Core> {
    let slot = state().slots.get((h.0 >> 32) as usize)?;
    if slot.generation.load(Ordering::Acquire) != h.0 as u32 {
        return None;
    }
    let p = slot.core.load(Ordering::Acquire);
    // SAFETY: a core is freed only by destroy, once no renderer holds its handle.
    (!p.is_null()).then(|| unsafe { &*p })
}

/// Instances created and not yet destroyed, in this process.
pub(crate) fn live() -> u64 {
    let s = state();
    s.created.load(Ordering::Acquire) - s.destroyed.load(Ordering::Acquire)
}

/// The in-process host: a handle to the process's one set of instances.
#[derive(Clone, Copy, Debug, Default)]
pub struct InProcess;

impl InProcess {
    /// Instances created and destroyed so far, in this process (gates and tests).
    pub fn counts() -> (u64, u64) {
        let s = state();
        (s.created.load(Ordering::Acquire), s.destroyed.load(Ordering::Acquire))
    }

    /// The `clap_plugin` behind `h`, for the probe's records.
    pub(crate) fn plugin_ptr(h: Handle) -> Option<usize> {
        core(h).map(|c| c.plugin as usize)
    }
}

/// `f` on the main thread with the instance `h`, or an error when it is gone.
fn on_main<R: Send + 'static>(h: Handle, f: impl FnOnce(&'static Core) -> Result<R> + Send + 'static) -> Result<R> {
    main_thread()?.run(move || f(core(h).ok_or_else(|| anyhow!("plugin instance {h:?} no longer exists"))?))?
}

impl PluginHost for InProcess {
    fn create(&self, plugin: &Found, state: Option<Arc<Blob>>, owner: &Owner) -> Result<(Handle, Info)> {
        let exec = main_thread()?;
        // Loaded here, never on the main thread: a bundle whose load waits on the main queue
        // would deadlock it (§3.3).
        let lib = library(&plugin.bundle)?;
        let _shown = owner.creating(&plugin.name);
        let (found, owner) = (plugin.clone(), owner.clone());
        exec.run(move || create_main(lib, found, state, owner))?
    }

    fn activate(&self, h: Handle, sample_rate: u32, max_frames: u32) -> Result<u32> {
        on_main(h, move |c| activate_main(c, sample_rate, max_frames))
    }

    fn deactivate(&self, h: Handle) {
        let _ = on_main(h, |c| {
            stop(c);
            Ok(())
        });
    }

    fn load_state(&self, h: Handle, blob: Arc<Blob>) -> Result<bool> {
        on_main(h, move |c| {
            if c.main().holds == Some(blob.sha) {
                return Ok(false);
            }
            // A load that fails may have landed in part: the plugin then holds nothing known, and
            // the next load of any blob, the one it held before included, isn't skipped.
            c.main().holds = None;
            load_state(c.plugin, &blob.bytes).with_context(|| c.main().info.found.id.clone())?;
            c.main().holds = Some(blob.sha);
            Ok(true)
        })
    }

    fn set_render_mode(&self, h: Handle, mode: RenderMode) -> Result<bool> {
        on_main(h, move |c| {
            let Some(set) = c.ext::<clap_plugin_render>(CLAP_EXT_RENDER).and_then(|r| r.set) else { return Ok(false) };
            let m = if mode == RenderMode::Offline { CLAP_RENDER_OFFLINE } else { CLAP_RENDER_REALTIME };
            Ok(unsafe { set(c.plugin, m) })
        })
    }

    fn main_call(&self, h: Handle, call: MainCall) -> Result<Reply> {
        on_main(h, move |c| {
            Ok(match call {
                MainCall::Window(WindowCall::Open { title }) => {
                    gui::open(c, h, &title)?;
                    Reply::Window(true)
                }
                MainCall::Window(WindowCall::Close) => {
                    gui::close(c);
                    Reply::Window(false)
                }
                MainCall::Value(id) => Reply::Value(param_value(c.plugin, id)),
                MainCall::Text(id, v) => Reply::Text(value_text(c.plugin, id, v)),
                MainCall::Info => {
                    let m = c.main();
                    let latency = m.info.latency;
                    m.info = read_info(c.plugin, m.info.found.clone());
                    m.info.latency = latency;
                    Reply::Info(Box::new(m.info.clone()))
                }
                MainCall::LoadPreset { location, key } => {
                    // What it holds is known no more: the next load of any blob isn't skipped.
                    c.main().holds = None;
                    crate::presets::load(c.plugin, &location, key.as_deref()).with_context(|| c.main().info.found.name.clone())?;
                    Reply::Loaded
                }
                MainCall::TakeLog => {
                    let mut log = std::mem::take(&mut *c.host.log.lock().unwrap_or_else(|e| e.into_inner()));
                    let quiet = c.host.logs_dropped.swap(0, Ordering::Relaxed);
                    if quiet > 0 {
                        log.push(format!("({quiet} more logged while processing, not kept)"));
                    }
                    Reply::Log(log)
                }
            })
        })
    }

    fn destroy(&self, h: Handle) {
        // Posting allocates, so a drop on the live audio thread shows up in its allocation trap.
        match main_thread() {
            Ok(m) => m.post(move || destroy_main(h)),
            // Nothing could have created it.
            Err(_) => debug_assert!(false, "destroyed with no plugin main thread"),
        }
    }

    fn latency(&self, h: Handle) -> Pending<Result<u32, String>> {
        let (tx, rx) = bounded(1);
        reply_on_main(h, tx, |c| {
            if !c.main().active {
                bail!("it isn't active");
            }
            Ok(c.ext::<clap_plugin_latency>(CLAP_EXT_LATENCY).and_then(|l| l.get).map_or(0, |g| unsafe { g(c.plugin) }))
        });
        rx
    }

    fn save_state(&self, h: Handle) -> Pending<Result<Blob, String>> {
        let (tx, rx) = bounded(1);
        reply_on_main(h, tx, |c| {
            let blob = Blob::new(save_state(c.plugin).with_context(|| c.main().info.found.id.clone())?);
            c.main().holds = Some(blob.sha);
            Ok(blob)
        });
        rx
    }

    fn process(&self, h: Handle, io: &mut SharedBuffers, ev_in: &Events, ev_out: &mut Events, t: &Transport) -> Status {
        match core(h) {
            Some(c) => audio::process(c, io, ev_in, ev_out, t),
            None => {
                audio::silence(io);
                Status::Error
            }
        }
    }

    fn reset(&self, h: Handle) {
        if let Some(c) = core(h) {
            audio::reset(c);
        }
    }

    fn take_process_request(&self, h: Handle) -> bool {
        core(h).is_some_and(|c| c.host.process_requested.swap(false, Ordering::Relaxed))
    }

    fn scan(&self) -> (Vec<Found>, Vec<String>) {
        crate::scan_here()
    }

    fn presets(&self, plugin: &Found) -> Result<Vec<Offered>> {
        // Loaded here, never on the main thread (§3.3), as `create` does. The search runs here
        // too, on the calling thread (HOST-8): CLAP's preset-discovery factory is thread-safe, and
        // a search that reads many files must not hold up the main thread, which serves every
        // playing plugin's main-thread calls and windows.
        let lib = library(&plugin.bundle)?;
        crate::presets::discover(&lib, &plugin.id)
    }

    fn live(&self) -> u64 {
        live()
    }

    fn counts(&self) -> (u64, u64) {
        InProcess::counts()
    }
}

/// Run `f` on the main thread and send its result, as text, on `tx`: always exactly once.
fn reply_on_main<R: Send + 'static>(h: Handle, tx: crossbeam_channel::Sender<Result<R, String>>, f: impl FnOnce(&Core) -> Result<R> + Send + 'static) {
    let m = match main_thread() {
        Ok(m) if !m.closed() => m,
        Ok(_) => return drop(tx.send(Err("the plugin main thread has shut down".into()))),
        Err(e) => return drop(tx.send(Err(format!("{e:#}")))),
    };
    m.post(move || {
        let r = match core(h) {
            Some(c) => f(c).map_err(|e| format!("{e:#}")),
            None => Err(format!("plugin instance {h:?} no longer exists")),
        };
        let _ = tx.send(r);
    });
}

macro_rules! call {
    ($p:ident . $f:ident ( $($a:expr),* )) => {
        unsafe { ((*$p).$f.context(concat!("plugin has no ", stringify!($f)))?)($p $(, $a)*) }
    };
}

fn create_main(lib: Arc<Library>, found: Found, state: Option<Arc<Blob>>, owner: Owner) -> Result<(Handle, Info)> {
    let id = CString::new(found.id.as_str())?;
    let host = HostData::boxed();
    let f = unsafe { &*lib.factory };
    let plugin = unsafe { (f.create_plugin.context("no create_plugin")?)(lib.factory, &host.clap, id.as_ptr()) };
    if plugin.is_null() {
        bail!("`{}` could not be created", found.id);
    }
    host.plugin.store(plugin as *mut clap_plugin, Ordering::Release);
    let made = (|| -> Result<(Info, u32)> {
        if !call!(plugin.init()) {
            bail!("`{}` failed to initialise", found.id);
        }
        let info = read_info(plugin, found.clone());
        if let Some(b) = &state {
            load_state(plugin, &b.bytes).with_context(|| format!("`{}`", found.id))?;
        }
        Ok((info, alloc()?))
    })();
    let (info, index) = match made {
        Ok(m) => m,
        Err(e) => {
            if let Some(d) = unsafe { (*plugin).destroy } {
                unsafe { d(plugin) };
            }
            return Err(e);
        }
    };
    let st = self::state();
    let slot = &st.slots[index as usize];
    let h = Handle((index as u64) << 32 | slot.generation.load(Ordering::Acquire) as u64);
    let core = Box::new(Core {
        plugin,
        host,
        _lib: lib,
        owner: owner.clone(),
        main: UnsafeCell::new(MainSide { info: info.clone(), active: false, holds: state.as_ref().map(|b| b.sha) }),
        audio: UnsafeCell::new(AudioSide {
            active: false,
            processing: false,
            max_frames: 0,
            ins: vec![],
            outs: vec![],
            in_ptrs: vec![],
            out_ptrs: vec![],
        }),
    });
    slot.core.store(Box::into_raw(core), Ordering::Release);
    st.created.fetch_add(1, Ordering::AcqRel);
    owner.created();
    // What it asked for while it was made, before it had a slot, is swept now.
    PENDING.store(true, Ordering::Release);
    st.tick.call_once(start_tick);
    Ok((h, info))
}

fn alloc() -> Result<u32> {
    let mut f = state().free.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = f.0.pop() {
        return Ok(i);
    }
    if f.1 as usize == MAX_INSTANCES {
        bail!("more than {MAX_INSTANCES} plugin instances at once");
    }
    f.1 += 1;
    Ok(f.1 - 1)
}

fn activate_main(c: &Core, sample_rate: u32, max_frames: u32) -> Result<u32> {
    let m = c.main();
    if m.active {
        stop(c);
    }
    // SAFETY: inactive, so no thread processes it.
    let a = unsafe { &mut *c.audio.get() };
    let ch = |x: Option<u32>| x.unwrap_or(0) as usize;
    a.ins = vec![vec![0.0; max_frames as usize]; ch(m.info.audio_in)];
    a.outs = vec![vec![0.0; max_frames as usize]; ch(m.info.audio_out)];
    a.in_ptrs = Vec::with_capacity(a.ins.len());
    a.out_ptrs = Vec::with_capacity(a.outs.len());
    a.max_frames = max_frames;
    let plugin = c.plugin;
    if !call!(plugin.activate(sample_rate as f64, 1, max_frames)) {
        bail!("`{}` failed to activate", m.info.found.id);
    }
    let latency = c.ext::<clap_plugin_latency>(CLAP_EXT_LATENCY).and_then(|l| l.get).map_or(0, |g| unsafe { g(plugin) });
    (m.active, m.info.latency, a.active, a.processing) = (true, latency, true, false);
    Ok(latency)
}

/// Stop processing and deactivate, on the main thread, which takes the instance's audio role for
/// `stop_processing` (CLAP lets any one thread hold it) and gives it up after. Only once no
/// renderer holds the instance.
fn stop(c: &Core) {
    c.host.audio_owner.store(token(), Ordering::Relaxed);
    // SAFETY: no renderer holds it, so no other thread processes it.
    let a = unsafe { &mut *c.audio.get() };
    if a.processing {
        if let Some(f) = unsafe { (*c.plugin).stop_processing } {
            unsafe { f(c.plugin) };
        }
        a.processing = false;
    }
    // Held only to stop processing: the next renderer's thread takes it at its first call.
    c.host.audio_owner.store(0, Ordering::Relaxed);
    let m = c.main();
    if m.active {
        if let Some(f) = unsafe { (*c.plugin).deactivate } {
            unsafe { f(c.plugin) };
        }
        (m.active, a.active) = (false, false);
    }
}

fn destroy_main(h: Handle) {
    let Some(c) = core(h) else { return };
    gui::close(c);
    stop(c);
    if let Some(f) = unsafe { (*c.plugin).destroy } {
        unsafe { f(c.plugin) };
    }
    // What it left registered on the main loop calls it no more.
    gui::forget(&c.host);
    let st = state();
    let index = (h.0 >> 32) as usize;
    let slot = &st.slots[index];
    let p = slot.core.swap(null_mut(), Ordering::AcqRel);
    // Never 0: a handle's generation is never 0.
    let next = slot.generation.load(Ordering::Acquire).wrapping_add(1).max(1);
    slot.generation.store(next, Ordering::Release);
    st.free.lock().unwrap_or_else(|e| e.into_inner()).0.push(index as u32);
    // SAFETY: the slot no longer reaches it, and only this thread frees.
    let core = unsafe { Box::from_raw(p) };
    let owner = core.owner.clone();
    drop(core);
    st.destroyed.fetch_add(1, Ordering::AcqRel);
    owner.destroyed();
}

fn read_info(plugin: *const clap_plugin, found: Found) -> Info {
    let api = CString::new(m2_os::window::API).unwrap_or_default();
    let gui = ext::<clap_plugin_gui>(plugin, CLAP_EXT_GUI).and_then(|g| g.is_api_supported).is_some_and(|f| unsafe { f(plugin, api.as_ptr(), false) });
    let mut info = Info { found, audio_in: None, audio_out: None, note_in: false, params: vec![], latency: 0, gui };
    if let Some(ap) = ext::<clap_plugin_audio_ports>(plugin, CLAP_EXT_AUDIO_PORTS)
        && let (Some(count), Some(get)) = (ap.count, ap.get)
    {
        for is_input in [true, false] {
            // The first port flagged main (or the first port) is the one we connect.
            let n = unsafe { count(plugin, is_input) };
            let mut chosen = None;
            for i in 0..n {
                let mut port: clap_audio_port_info = unsafe { std::mem::zeroed() };
                if unsafe { get(plugin, i, is_input, &mut port) } && (chosen.is_none() || port.flags & CLAP_AUDIO_PORT_IS_MAIN != 0) {
                    chosen = Some(port.channel_count);
                    if port.flags & CLAP_AUDIO_PORT_IS_MAIN != 0 {
                        break;
                    }
                }
            }
            if is_input {
                info.audio_in = chosen;
            } else {
                info.audio_out = chosen;
            }
        }
    }
    if let Some(np) = ext::<clap_plugin_note_ports>(plugin, CLAP_EXT_NOTE_PORTS)
        && let Some(count) = np.count
    {
        info.note_in = unsafe { count(plugin, true) } > 0;
    }
    if let Some(pp) = ext::<clap_plugin_params>(plugin, CLAP_EXT_PARAMS)
        && let (Some(count), Some(get)) = (pp.count, pp.get_info)
    {
        for i in 0..unsafe { count(plugin) } {
            let mut p: clap_param_info = unsafe { std::mem::zeroed() };
            if unsafe { get(plugin, i, &mut p) } && p.flags & CLAP_PARAM_IS_HIDDEN == 0 && p.flags & CLAP_PARAM_IS_READONLY == 0 {
                info.params.push(Param {
                    id: p.id,
                    name: fixed(&p.name),
                    module: fixed(&p.module),
                    min: p.min_value,
                    max: p.max_value,
                    default: p.default_value,
                    stepped: p.flags & CLAP_PARAM_IS_STEPPED != 0,
                });
            }
        }
    }
    info
}

fn param_value(plugin: *const clap_plugin, id: u32) -> Option<f64> {
    let get = ext::<clap_plugin_params>(plugin, CLAP_EXT_PARAMS)?.get_value?;
    let mut v = 0.0;
    unsafe { get(plugin, id, &mut v) }.then_some(v)
}

fn value_text(plugin: *const clap_plugin, id: u32, value: f64) -> Option<String> {
    let f = ext::<clap_plugin_params>(plugin, CLAP_EXT_PARAMS)?.value_to_text?;
    let mut buf = [0 as c_char; 256];
    unsafe { f(plugin, id, value, buf.as_mut_ptr(), buf.len() as u32) }.then(|| fixed(&buf))
}

fn save_state(plugin: *const clap_plugin) -> Result<Vec<u8>> {
    let st = ext::<clap_plugin_state>(plugin, CLAP_EXT_STATE).context("this plugin can't save its state")?;
    let mut buf: Vec<u8> = vec![];
    unsafe extern "C" fn write(s: *const clap_ostream, data: *const c_void, size: u64) -> i64 {
        let buf = unsafe { &mut *((*s).ctx as *mut Vec<u8>) };
        buf.extend_from_slice(unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) });
        size as i64
    }
    let stream = clap_ostream { ctx: &mut buf as *mut Vec<u8> as *mut c_void, write: Some(write) };
    if !unsafe { (st.save.context("no save")?)(plugin, &stream) } {
        bail!("it failed to save its state");
    }
    Ok(buf)
}

fn load_state(plugin: *const clap_plugin, bytes: &[u8]) -> Result<()> {
    let st = ext::<clap_plugin_state>(plugin, CLAP_EXT_STATE).context("this plugin can't load state")?;
    struct Reader<'a> {
        data: &'a [u8],
        at: usize,
    }
    unsafe extern "C" fn read(s: *const clap_istream, out: *mut c_void, size: u64) -> i64 {
        let r = unsafe { &mut *((*s).ctx as *mut Reader) };
        let n = (size as usize).min(r.data.len() - r.at);
        unsafe { std::ptr::copy_nonoverlapping(r.data[r.at..].as_ptr(), out as *mut u8, n) };
        r.at += n;
        n as i64
    }
    let mut r = Reader { data: bytes, at: 0 };
    let stream = clap_istream { ctx: &mut r as *mut Reader as *mut c_void, read: Some(read) };
    if !unsafe { (st.load.context("no load")?)(plugin, &stream) } {
        bail!("it refused the saved state");
    }
    Ok(())
}

/// Every 5 ms, have the main thread sweep the flags plugins raised, when one did (§3.5).
fn start_tick() {
    std::thread::Builder::new()
        .name("m2-plugin-tick".into())
        .spawn(|| {
            loop {
                std::thread::sleep(Duration::from_millis(5));
                let Ok(m) = main_thread() else { return };
                if m.closed() {
                    return;
                }
                if PENDING.swap(false, Ordering::Acquire) {
                    m.post(sweep);
                }
            }
        })
        .expect("a thread for plugin requests");
}

/// On the main thread: what each instance asked for since the last sweep. `on_main_thread` runs
/// here, never inside `process`; the rest goes to each instance's owner.
fn sweep() {
    let st = state();
    let used = st.free.lock().unwrap_or_else(|e| e.into_inner()).1 as usize;
    for (i, slot) in st.slots[..used].iter().enumerate() {
        let p = slot.core.load(Ordering::Acquire);
        if p.is_null() {
            continue;
        }
        // SAFETY: only this thread frees a core, and it isn't doing so now.
        let c = unsafe { &*p };
        let h = Handle((i as u64) << 32 | slot.generation.load(Ordering::Acquire) as u64);
        let hd = &c.host;
        gui::resize_asked(hd);
        if hd.callback.swap(false, Ordering::Relaxed)
            && let Some(f) = unsafe { (*c.plugin).on_main_thread }
        {
            unsafe { f(c.plugin) };
        }
        if hd.latency_changed.swap(false, Ordering::Relaxed) && c.main().active {
            let l = c.ext::<clap_plugin_latency>(CLAP_EXT_LATENCY).and_then(|l| l.get).map_or(0, |g| unsafe { g(c.plugin) });
            c.main().info.latency = l;
            c.owner.send(HostEvent::Latency(h, l));
        }
        if hd.restart.swap(false, Ordering::Relaxed) {
            c.owner.send(HostEvent::Restart(h));
        }
        if hd.dirty.swap(false, Ordering::Relaxed) {
            c.owner.send(HostEvent::Dirty(h));
        }
        if hd.rescan.swap(false, Ordering::Relaxed) {
            // A parameter change while it runs is reported, never acted on (HOST-4 comes later).
            let m = c.main();
            let latency = m.info.latency;
            m.info = read_info(c.plugin, m.info.found.clone());
            m.info.latency = latency;
            keep_log(hd, c"the plugin's parameters changed".as_ptr());
        }
    }
}

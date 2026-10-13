//! Hosts CLAP plugins.
//!
//! The plugins producers already own become graph nodes: audio in and out, notes in
//! for instruments, and every automatable parameter as a control input. A plugin's
//! state is saved and loaded as bytes, which the project stores as an asset.
//!
//! Plugins are trusted native code running in this process (out-of-process hosting
//! is planned). They are impure: the runtime never caches their output. This host
//! calls everything from one thread and answers "yes" to both "is this the main
//! thread" and "is this the audio thread", which suits offline rendering; the
//! realtime engine will split them.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, anyhow, bail};
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::log::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::ext::thread_check::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::*;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::{CLAP_VERSION, clap_version_is_compatible};

/// Where plugins are installed on this machine.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("CLAP_PATH") {
        v.extend(std::env::split_paths(&p));
    }
    if let Some(home) = std::env::var_os("HOME") {
        v.push(PathBuf::from(home).join("Library/Audio/Plug-Ins/CLAP"));
    }
    v.push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    v
}

/// A plugin found on disk.
#[derive(Clone, Debug)]
pub struct Found {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub features: Vec<String>,
    pub bundle: PathBuf,
}

/// What a plugin instance offers, read after `init`.
#[derive(Clone, Debug)]
pub struct Info {
    pub found: Found,
    /// Channel count of the main audio input, if any.
    pub audio_in: Option<u32>,
    /// Channel count of the main audio output, if any.
    pub audio_out: Option<u32>,
    pub note_in: bool,
    pub params: Vec<Param>,
    pub latency: u32,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub id: u32,
    pub name: String,
    pub module: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub stepped: bool,
}

// ---------------------------------------------------------------- libraries

struct Library {
    _lib: libloading::Library,
    _entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
    bundle: PathBuf,
}

// SAFETY: the entry and factory are process-wide tables that CLAP requires to be
// thread-safe; the library stays loaded for the life of the process.
unsafe impl Send for Library {}
unsafe impl Sync for Library {}

fn cstr(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
}

fn fixed(s: &[c_char]) -> String {
    let bytes: Vec<u8> = s.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Loaded libraries, kept for the life of the process (CLAP's `deinit` is never
/// called, which is allowed when the process exits).
fn libraries() -> &'static Mutex<HashMap<PathBuf, Arc<Library>>> {
    static L: OnceLock<Mutex<HashMap<PathBuf, Arc<Library>>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

fn binary_in(bundle: &Path) -> Result<PathBuf> {
    if bundle.is_file() {
        return Ok(bundle.to_path_buf());
    }
    let dir = bundle.join("Contents/MacOS");
    let mut files = std::fs::read_dir(&dir).with_context(|| format!("{} is not a CLAP bundle", bundle.display()))?.flatten().map(|e| e.path());
    files.next().ok_or_else(|| anyhow!("{} has no binary", dir.display()))
}

fn library(bundle: &Path) -> Result<Arc<Library>> {
    let mut libs = libraries().lock().expect("plugin library lock");
    if let Some(l) = libs.get(bundle) {
        return Ok(l.clone());
    }
    let bin = binary_in(bundle)?;
    // SAFETY: loading a plugin runs its native code; plugins are trusted like in any DAW.
    let lib = unsafe { libloading::Library::new(&bin) }.with_context(|| format!("loading {}", bin.display()))?;
    let entry = unsafe { *lib.get::<*const clap_plugin_entry>(b"clap_entry\0").with_context(|| format!("{} has no clap_entry", bin.display()))? };
    let e = unsafe { &*entry };
    if !clap_version_is_compatible(e.clap_version) {
        bail!("{} uses an incompatible CLAP version", bundle.display());
    }
    let path = CString::new(bundle.to_string_lossy().as_bytes())?;
    if !unsafe { (e.init.context("no init")?)(path.as_ptr()) } {
        bail!("{} failed to initialise", bundle.display());
    }
    let factory = unsafe { (e.get_factory.context("no get_factory")?)(CLAP_PLUGIN_FACTORY_ID.as_ptr()) } as *const clap_plugin_factory;
    if factory.is_null() {
        bail!("{} has no plugin factory", bundle.display());
    }
    let l = Arc::new(Library { _lib: lib, _entry: entry, factory, bundle: bundle.to_path_buf() });
    libs.insert(bundle.to_path_buf(), l.clone());
    Ok(l)
}

impl Library {
    fn plugins(&self) -> Vec<Found> {
        let f = unsafe { &*self.factory };
        let Some(count) = f.get_plugin_count else { return vec![] };
        let Some(desc) = f.get_plugin_descriptor else { return vec![] };
        (0..unsafe { count(self.factory) })
            .filter_map(|i| {
                let d = unsafe { desc(self.factory, i) };
                if d.is_null() {
                    return None;
                }
                let d = unsafe { &*d };
                let mut features = vec![];
                if !d.features.is_null() {
                    let mut p = d.features;
                    while !unsafe { *p }.is_null() {
                        features.push(cstr(unsafe { *p }));
                        p = unsafe { p.add(1) };
                    }
                }
                Some(Found {
                    id: cstr(d.id),
                    name: cstr(d.name),
                    vendor: cstr(d.vendor),
                    version: cstr(d.version),
                    features,
                    bundle: self.bundle.clone(),
                })
            })
            .collect()
    }
}

/// Every plugin in the search paths. Bundles that fail to load are reported, not fatal.
pub fn scan() -> (Vec<Found>, Vec<String>) {
    let (mut found, mut errors) = (vec![], vec![]);
    for dir in search_paths() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut bundles: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "clap")).collect();
        bundles.sort();
        for b in bundles {
            match library(&b) {
                Ok(l) => found.extend(l.plugins()),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
    }
    (found, errors)
}

/// Find a plugin by id.
pub fn find(id: &str) -> Result<Found> {
    let (found, _) = scan();
    found.into_iter().find(|f| f.id == id).ok_or_else(|| anyhow!("no CLAP plugin `{id}` is installed (looked in {:?})", search_paths()))
}

// ---------------------------------------------------------------- host callbacks

#[derive(Default)]
struct HostState {
    log: Vec<String>,
    callback_requested: bool,
}

/// The `clap_host` a plugin talks to. Boxed so its address never changes.
struct Host {
    clap: clap_host,
    state: Mutex<HostState>,
    _strings: [CString; 4],
}

unsafe fn host_state<'a>(h: *const clap_host) -> &'a Mutex<HostState> {
    unsafe { &(*((*h).host_data as *const Host)).state }
}

unsafe extern "C" fn host_get_extension(_: *const clap_host, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
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
    } else {
        null()
    }
}

unsafe extern "C" fn host_nothing(_: *const clap_host) {}

unsafe extern "C" fn host_request_callback(h: *const clap_host) {
    if let Ok(mut s) = unsafe { host_state(h) }.lock() {
        s.callback_requested = true;
    }
}

unsafe extern "C" fn host_yes(_: *const clap_host) -> bool {
    true
}

unsafe extern "C" fn host_log(h: *const clap_host, severity: clap_log_severity, msg: *const c_char) {
    if severity >= CLAP_LOG_WARNING
        && let Ok(mut s) = unsafe { host_state(h) }.lock()
    {
        s.log.push(cstr(msg));
    }
}

unsafe extern "C" fn host_params_rescan(_: *const clap_host, _: clap_param_rescan_flags) {}
unsafe extern "C" fn host_params_clear(_: *const clap_host, _: clap_id, _: clap_param_clear_flags) {}

static THREAD_CHECK: clap_host_thread_check = clap_host_thread_check { is_main_thread: Some(host_yes), is_audio_thread: Some(host_yes) };
static LOG: clap_host_log = clap_host_log { log: Some(host_log) };
static HOST_STATE: clap_host_state = clap_host_state { mark_dirty: Some(host_nothing) };
static HOST_PARAMS: clap_host_params = clap_host_params {
    rescan: Some(host_params_rescan),
    clear: Some(host_params_clear),
    request_flush: Some(host_nothing),
};
static HOST_LATENCY: clap_host_latency = clap_host_latency { changed: Some(host_nothing) };

fn new_host() -> Box<Host> {
    let strings = [c"music2".to_owned(), c"music2".to_owned(), c"https://github.com/".to_owned(), c"0.1.0".to_owned()];
    let mut h = Box::new(Host {
        clap: clap_host {
            clap_version: CLAP_VERSION,
            host_data: null_mut(),
            name: strings[0].as_ptr(),
            vendor: strings[1].as_ptr(),
            url: strings[2].as_ptr(),
            version: strings[3].as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_nothing),
            request_process: Some(host_nothing),
            request_callback: Some(host_request_callback),
        },
        state: Mutex::new(HostState::default()),
        _strings: strings,
    });
    h.clap.host_data = &*h as *const Host as *mut c_void;
    h
}

// ---------------------------------------------------------------- events

/// Something that happens at a sample offset within a `process` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn { time: u32, id: i32, key: i16, velocity: f64 },
    NoteOff { time: u32, id: i32, key: i16 },
    /// End a note at once, with no release (a panic).
    NoteChoke { time: u32, id: i32, key: i16 },
    /// Tuning in semitones for one note (for pitches between keys).
    Tuning { time: u32, id: i32, key: i16, semitones: f64 },
    Param { time: u32, id: u32, value: f64 },
}

impl Event {
    fn time(&self) -> u32 {
        match *self {
            Event::NoteOn { time, .. }
            | Event::NoteOff { time, .. }
            | Event::NoteChoke { time, .. }
            | Event::Tuning { time, .. }
            | Event::Param { time, .. } => time,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
union Raw {
    header: clap_event_header,
    note: clap_event_note,
    expr: clap_event_note_expression,
    param: clap_event_param_value,
    transport: clap_event_transport,
}

fn header(size: usize, time: u32, type_: u16) -> clap_event_header {
    clap_event_header { size: size as u32, time, space_id: CLAP_CORE_EVENT_SPACE_ID, type_, flags: 0 }
}

fn raw(e: &Event) -> Raw {
    use std::mem::size_of;
    match *e {
        Event::NoteOn { time, id, key, velocity } => Raw {
            note: clap_event_note { header: header(size_of::<clap_event_note>(), time, CLAP_EVENT_NOTE_ON), note_id: id, port_index: 0, channel: 0, key, velocity },
        },
        Event::NoteOff { time, id, key } => Raw {
            note: clap_event_note { header: header(size_of::<clap_event_note>(), time, CLAP_EVENT_NOTE_OFF), note_id: id, port_index: 0, channel: 0, key, velocity: 0.0 },
        },
        Event::NoteChoke { time, id, key } => Raw {
            note: clap_event_note { header: header(size_of::<clap_event_note>(), time, CLAP_EVENT_NOTE_CHOKE), note_id: id, port_index: 0, channel: 0, key, velocity: 0.0 },
        },
        Event::Tuning { time, id, key, semitones } => Raw {
            expr: clap_event_note_expression {
                header: header(size_of::<clap_event_note_expression>(), time, CLAP_EVENT_NOTE_EXPRESSION),
                expression_id: CLAP_NOTE_EXPRESSION_TUNING,
                note_id: id,
                port_index: 0,
                channel: 0,
                key,
                value: semitones,
            },
        },
        Event::Param { time, id, value } => Raw {
            param: clap_event_param_value {
                header: header(size_of::<clap_event_param_value>(), time, CLAP_EVENT_PARAM_VALUE),
                param_id: id,
                cookie: null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value,
            },
        },
    }
}

struct InList {
    events: Vec<Raw>,
}

/// Events an instance has room for in one call before it is told otherwise.
const EVENTS: usize = 256;

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    unsafe { (*((*list).ctx as *const InList)).events.len() as u32 }
}

unsafe extern "C" fn in_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    let l = unsafe { &*((*list).ctx as *const InList) };
    l.events.get(i as usize).map_or(null(), |r| r as *const Raw as *const clap_event_header)
}

unsafe extern "C" fn out_push(_: *const clap_output_events, _: *const clap_event_header) -> bool {
    true
}

/// The transport plugins get from the engine (HOST-6), for each `process` call: the song
/// position at its first frame, whether the song plays and loops, the meter, and the engine
/// clock as `steady_time`. A call never spans a jump: the engine splits the block there.
#[derive(Clone, Copy, Debug, Default)]
pub struct Transport {
    pub beats: f64,
    pub seconds: f64,
    pub bpm: f64,
    pub playing: bool,
    /// The loop, when one is on: in beats and in seconds.
    pub looping: Option<((f64, f64), (f64, f64))>,
    /// Time signature: beats per bar and the note value of a beat.
    pub tsig: (u16, u16),
    /// The beat the current bar started on, and its number (0 is the first).
    pub bar_start: f64,
    pub bar: i32,
    /// Samples since the engine started: never jumps, never resets (RENDER-6).
    pub steady: i64,
}

// ---------------------------------------------------------------- instances

/// One plugin instance, activated and processing.
pub struct Instance {
    plugin: *const clap_plugin,
    host: Box<Host>,
    _lib: Arc<Library>,
    pub info: Info,
    max_frames: u32,
    ins: Vec<Vec<f32>>,
    outs: Vec<Vec<f32>>,
    /// Reserved when the instance is made, so `process` never allocates (RENDER-28).
    in_ptrs: Vec<*mut f32>,
    out_ptrs: Vec<*mut f32>,
    list: InList,
}

// SAFETY: an instance is used from one thread at a time.
unsafe impl Send for Instance {}

macro_rules! call {
    ($self:ident . $f:ident ( $($a:expr),* )) => {
        unsafe { ((*$self.plugin).$f.context(concat!("plugin has no ", stringify!($f)))?)($self.plugin $(, $a)*) }
    };
}

impl Instance {
    /// Create, initialise and activate plugin `id` from `bundle`.
    pub fn new(found: &Found, sample_rate: f64, max_frames: u32) -> Result<Instance> {
        let lib = library(&found.bundle)?;
        let host = new_host();
        let id = CString::new(found.id.as_str())?;
        let f = unsafe { &*lib.factory };
        let plugin = unsafe { (f.create_plugin.context("no create_plugin")?)(lib.factory, &host.clap, id.as_ptr()) };
        if plugin.is_null() {
            bail!("`{}` could not be created", found.id);
        }
        let mut inst = Instance {
            plugin,
            host,
            _lib: lib,
            info: Info { found: found.clone(), audio_in: None, audio_out: None, note_in: false, params: vec![], latency: 0 },
            max_frames,
            ins: vec![],
            outs: vec![],
            in_ptrs: vec![],
            out_ptrs: vec![],
            list: InList { events: Vec::with_capacity(EVENTS) },
        };
        if !call!(inst.init()) {
            bail!("`{}` failed to initialise", found.id);
        }
        inst.read_info();
        if !call!(inst.activate(sample_rate, 1, max_frames)) {
            bail!("`{}` failed to activate", found.id);
        }
        inst.info.latency = inst.ext::<clap_plugin_latency>(CLAP_EXT_LATENCY).and_then(|l| l.get).map_or(0, |g| unsafe { g(inst.plugin) });
        if !call!(inst.start_processing()) {
            bail!("`{}` failed to start processing", found.id);
        }
        let ch = |c: Option<u32>| c.unwrap_or(0) as usize;
        inst.ins = vec![vec![0.0; max_frames as usize]; ch(inst.info.audio_in)];
        inst.outs = vec![vec![0.0; max_frames as usize]; ch(inst.info.audio_out)];
        inst.in_ptrs = Vec::with_capacity(inst.ins.len());
        inst.out_ptrs = Vec::with_capacity(inst.outs.len());
        Ok(inst)
    }

    /// Make room for `n` events in one `process` call, so it never allocates for them.
    pub fn reserve_events(&mut self, n: usize) {
        self.list.events.clear();
        self.list.events.reserve(n);
    }

    fn ext<T>(&self, id: &CStr) -> Option<&T> {
        let get = unsafe { (*self.plugin).get_extension }?;
        let p = unsafe { get(self.plugin, id.as_ptr()) } as *const T;
        (!p.is_null()).then(|| unsafe { &*p })
    }

    fn read_info(&mut self) {
        if let Some(ap) = self.ext::<clap_plugin_audio_ports>(CLAP_EXT_AUDIO_PORTS)
            && let (Some(count), Some(get)) = (ap.count, ap.get)
        {
            for is_input in [true, false] {
                // The first port flagged main (or the first port) is the one we connect.
                let n = unsafe { count(self.plugin, is_input) };
                let mut chosen = None;
                for i in 0..n {
                    let mut info: clap_audio_port_info = unsafe { std::mem::zeroed() };
                    if unsafe { get(self.plugin, i, is_input, &mut info) } && (chosen.is_none() || info.flags & CLAP_AUDIO_PORT_IS_MAIN != 0) {
                        let main = info.flags & CLAP_AUDIO_PORT_IS_MAIN != 0;
                        chosen = Some(info.channel_count);
                        if main {
                            break;
                        }
                    }
                }
                if is_input {
                    self.info.audio_in = chosen;
                } else {
                    self.info.audio_out = chosen;
                }
            }
        }
        if let Some(np) = self.ext::<clap_plugin_note_ports>(CLAP_EXT_NOTE_PORTS)
            && let Some(count) = np.count
        {
            self.info.note_in = unsafe { count(self.plugin, true) } > 0;
        }
        if let Some(pp) = self.ext::<clap_plugin_params>(CLAP_EXT_PARAMS)
            && let (Some(count), Some(get)) = (pp.count, pp.get_info)
        {
            for i in 0..unsafe { count(self.plugin) } {
                let mut info: clap_param_info = unsafe { std::mem::zeroed() };
                if unsafe { get(self.plugin, i, &mut info) } && info.flags & CLAP_PARAM_IS_HIDDEN == 0 && info.flags & CLAP_PARAM_IS_READONLY == 0 {
                    self.info.params.push(Param {
                        id: info.id,
                        name: fixed(&info.name),
                        module: fixed(&info.module),
                        min: info.min_value,
                        max: info.max_value,
                        default: info.default_value,
                        stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0,
                    });
                }
            }
        }
    }

    /// Current value of a parameter.
    pub fn param(&self, id: u32) -> Option<f64> {
        let get = self.ext::<clap_plugin_params>(CLAP_EXT_PARAMS)?.get_value?;
        let mut v = 0.0;
        unsafe { get(self.plugin, id, &mut v) }.then_some(v)
    }

    /// How the plugin displays a parameter value, e.g. `"Lowpass 24 dB"` or `"440.0 Hz"`.
    pub fn value_text(&self, id: u32, value: f64) -> Option<String> {
        let f = self.ext::<clap_plugin_params>(CLAP_EXT_PARAMS)?.value_to_text?;
        let mut buf = [0 as c_char; 256];
        unsafe { f(self.plugin, id, value, buf.as_mut_ptr(), buf.len() as u32) }.then(|| fixed(&buf))
    }

    /// Process `frames` samples. `inputs` has one slice per input channel (missing
    /// channels read silence); `outputs` receives one slice per output channel. `events` must
    /// be in time order. Allocates nothing while `events` fits what [`Self::reserve_events`]
    /// made room for (RENDER-28); the host's lock below is the main-thread callback's (HOST-2,
    /// step 3b).
    pub fn process(&mut self, frames: u32, inputs: &[&[f32]], outputs: &mut [&mut [f32]], events: &[Event], transport: Transport) -> Result<()> {
        if frames > self.max_frames {
            bail!("asked for {frames} frames, activated for {}", self.max_frames);
        }
        debug_assert!(events.windows(2).all(|w| w[0].time() <= w[1].time()), "events must come in time order");
        let n = frames as usize;
        for (c, buf) in self.ins.iter_mut().enumerate() {
            match inputs.get(c).or(inputs.first()) {
                Some(src) => buf[..n].copy_from_slice(&src[..n]),
                None => buf[..n].fill(0.0),
            }
        }
        self.in_ptrs.clear();
        self.in_ptrs.extend(self.ins.iter_mut().map(|b| b.as_mut_ptr()));
        self.out_ptrs.clear();
        self.out_ptrs.extend(self.outs.iter_mut().map(|b| b.as_mut_ptr()));
        let audio_in =
            clap_audio_buffer { data32: self.in_ptrs.as_mut_ptr(), data64: null_mut(), channel_count: self.in_ptrs.len() as u32, latency: 0, constant_mask: 0 };
        let mut audio_out =
            clap_audio_buffer { data32: self.out_ptrs.as_mut_ptr(), data64: null_mut(), channel_count: self.out_ptrs.len() as u32, latency: 0, constant_mask: 0 };

        self.list.events.clear();
        self.list.events.extend(events.iter().map(raw));
        let beat_time = |b: f64| (b * CLAP_BEATTIME_FACTOR as f64) as i64;
        let sec_time = |s: f64| (s * CLAP_SECTIME_FACTOR as f64) as i64;
        let mut flags = CLAP_TRANSPORT_HAS_TEMPO | CLAP_TRANSPORT_HAS_BEATS_TIMELINE | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE | CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
        if transport.playing {
            flags |= CLAP_TRANSPORT_IS_PLAYING;
        }
        if transport.looping.is_some() {
            flags |= CLAP_TRANSPORT_IS_LOOP_ACTIVE;
        }
        let ((loop_start, loop_end), (loop_start_s, loop_end_s)) = transport.looping.unwrap_or_default();
        let (tsig_num, tsig_denom) = if transport.tsig.0 == 0 { (4, 4) } else { transport.tsig };
        let transport_event = clap_event_transport {
            header: header(std::mem::size_of::<clap_event_transport>(), 0, CLAP_EVENT_TRANSPORT),
            flags,
            song_pos_beats: beat_time(transport.beats),
            song_pos_seconds: sec_time(transport.seconds),
            tempo: transport.bpm,
            tempo_inc: 0.0,
            loop_start_beats: beat_time(loop_start),
            loop_end_beats: beat_time(loop_end),
            loop_start_seconds: sec_time(loop_start_s),
            loop_end_seconds: sec_time(loop_end_s),
            bar_start: beat_time(transport.bar_start),
            bar_number: transport.bar,
            tsig_num,
            tsig_denom,
        };
        let in_events = clap_input_events { ctx: &mut self.list as *mut InList as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let out_events = clap_output_events { ctx: null_mut(), try_push: Some(out_push) };
        let process = clap_process {
            steady_time: transport.steady,
            frames_count: frames,
            transport: &transport_event,
            audio_inputs: if self.ins.is_empty() { null() } else { &audio_in },
            audio_outputs: if self.outs.is_empty() { null_mut() } else { &mut audio_out },
            audio_inputs_count: (!self.ins.is_empty()) as u32,
            audio_outputs_count: (!self.outs.is_empty()) as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        let status = call!(self.process(&process));
        if status == CLAP_PROCESS_ERROR {
            bail!("`{}` reported a processing error", self.info.found.id);
        }
        for (c, out) in outputs.iter_mut().enumerate() {
            match self.outs.get(c) {
                Some(src) => out[..n].copy_from_slice(&src[..n]),
                None => out[..n].fill(0.0),
            }
        }
        self.main_thread_callback();
        Ok(())
    }

    fn main_thread_callback(&mut self) {
        let wanted = self.host.state.lock().map(|mut s| std::mem::take(&mut s.callback_requested)).unwrap_or(false);
        if wanted && let Some(f) = unsafe { (*self.plugin).on_main_thread } {
            unsafe { f(self.plugin) };
        }
    }

    /// Back to silence: stop all notes, clear tails.
    pub fn reset(&mut self) -> Result<()> {
        call!(self.reset());
        Ok(())
    }

    /// The plugin's complete state as bytes.
    pub fn save_state(&self) -> Result<Vec<u8>> {
        let st = self.ext::<clap_plugin_state>(CLAP_EXT_STATE).context("this plugin can't save its state")?;
        let mut buf: Vec<u8> = vec![];
        unsafe extern "C" fn write(s: *const clap_ostream, data: *const c_void, size: u64) -> i64 {
            let buf = unsafe { &mut *((*s).ctx as *mut Vec<u8>) };
            buf.extend_from_slice(unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) });
            size as i64
        }
        let stream = clap_ostream { ctx: &mut buf as *mut Vec<u8> as *mut c_void, write: Some(write) };
        if !unsafe { (st.save.context("no save")?)(self.plugin, &stream) } {
            bail!("`{}` failed to save its state", self.info.found.id);
        }
        Ok(buf)
    }

    /// Restore state saved by [`Self::save_state`].
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<()> {
        let st = self.ext::<clap_plugin_state>(CLAP_EXT_STATE).context("this plugin can't load state")?;
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
        if !unsafe { (st.load.context("no load")?)(self.plugin, &stream) } {
            bail!("`{}` refused the saved state", self.info.found.id);
        }
        Ok(())
    }

    /// Warnings and errors the plugin logged since the last call.
    pub fn take_log(&self) -> Vec<String> {
        self.host.state.lock().map(|mut s| std::mem::take(&mut s.log)).unwrap_or_default()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            if let Some(f) = (*self.plugin).stop_processing {
                f(self.plugin);
            }
            if let Some(f) = (*self.plugin).deactivate {
                f(self.plugin);
            }
            if let Some(f) = (*self.plugin).destroy {
                f(self.plugin);
            }
        }
    }
}

/// The address of each of a plugin's parameters in code (FMT-18): `<name>_<id>`, its display
/// name in lowercase words joined by `_`, then its CLAP parameter id (`dry_wet_12`). The id
/// makes it unique and is all that binds it; the name part is a label that a plugin update may
/// change. A parameter without a name is `param_<id>`.
pub fn param_port_names(params: &[Param]) -> Vec<String> {
    params.iter().map(|p| format!("{}_{}", param_label(&p.name), p.id)).collect()
}

/// The name part of a parameter address: lowercase ASCII letters and digits, every run of
/// anything else one `_`, trimmed; `param` if nothing is left.
pub fn param_label(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('_') {
            s.push('_');
        }
    }
    match s.trim_matches('_') {
        "" => "param".into(),
        t => t.into(),
    }
}

/// A parameter address split into its name part and its CLAP id: `dry_wet_12` → (`dry_wet`, 12).
/// `None` for a port written without an id (`dry_wet`), or one that isn't a parameter (`in`).
pub fn param_address(port: &str) -> Option<(&str, u32)> {
    let (name, id) = port.rsplit_once('_')?;
    // `u32::from_str` takes a leading `+`; an address is digits only.
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(())?;
    Some((name, id.parse().ok()?))
}

#[cfg(test)]
mod tests;

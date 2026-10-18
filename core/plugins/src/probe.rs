//! The probe: a CLAP plugin built into this crate, for tests that need a deterministic plugin and a
//! record of how the host treats it (§6.4 of the step 3b design). Inert until [`install`]
//! registers it; then [`crate::find`] finds `m2.probe` and `m2.probe.fails-state`, and everything
//! but `dlopen` is the real host code.
//!
//! It is an instrument and an effect at once: stereo in and out, notes in. Its output is its input
//! times `Gain`, plus a sine at each sounding note's key, times `Level`. It records:
//! - every main-thread call, with the thread it ran on and what the host's thread checks said;
//! - every `process` call's thread, each call of either kind stamped from one process-wide
//!   counter, so a test can tell which came first;
//! - in atomics, from `process`: calls, wrong thread-check answers, blocks whose steady time was
//!   already processed (an instance processed twice), `on_main_thread` entered during a `process`
//!   call, every note event with its steady time, and the transport the last calls were told;
//! - and it calls `request_callback` from `process` every 10 calls.
//!
//! Its state is `m2probe\0`, the two parameters, then a tag; any other bytes load as the tag
//! alone. `m2.probe.fails-state` refuses a state that starts with `fail`, and any probe takes
//! 300 ms to load one that starts with `slow`, as a big patch does. One whose tag starts with
//! `fails-process` reports an error from every `process` call, as a broken plugin does; one whose
//! tag starts with `sets-fz` leaves its thread flushing subnormals to zero after every `process`
//! call, as some plugins do (RENDER-2a). One whose tag is `crash-at-<n>` aborts its process at its
//! n-th `process` call, and `hang-at-<n>` never returns from it, as broken plugins do (HOST-3: run
//! it in the host process); `crash-at-<n>:<file>` and `hang-at-<n>:<file>` do so only when they
//! make `<file>`, so once. With
//! [`Stats::defer_load`] set,
//! a probe applies the parameters of a state it loads at the end of its next `process` call, after
//! that call's events, ending every voice, as Surge XT does. With [`Stats::reset_keeps_voices`]
//! set, `reset` ends no voice, as Surge XT's doesn't.

use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::*;
use clap_sys::ext::latency::*;
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::*;
use clap_sys::ext::render::*;
use clap_sys::ext::state::*;
use clap_sys::ext::thread_check::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::host::clap_host;
use clap_sys::id::{CLAP_INVALID_ID, clap_id};
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::*;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;

use crate::{Handle, InProcess, Library};

pub const ID: &str = "m2.probe";
/// The probe, refusing any state that starts with `fail`.
pub const FAILS_STATE: &str = "m2.probe.fails-state";
/// Its parameters' CLAP ids: addressed `gain_1` and `level_2` in code.
pub const GAIN: u32 = 1;
pub const LEVEL: u32 = 2;

const BUNDLE: &str = "<built-in m2 probe>";
const MAGIC: &[u8] = b"m2probe\0";
/// Notes it plays at once; more are counted and ignored.
const VOICES: usize = 512;
/// Note events it remembers.
const EVENTS: usize = 1 << 16;

static INSTALLED: AtomicBool = AtomicBool::new(false);

/// What orders every probe's main-thread and `process` calls, across threads.
static TICK: AtomicU64 = AtomicU64::new(1);
/// `process` calls one instance remembers.
const PROCESS_LOG: usize = 1 << 14;
/// The last `process` calls whose transport one instance remembers ([`Stats::told`]).
const TOLD: usize = 32;
/// What one call's transport is kept as: CLAP's event, packed into this many words.
const TOLD_WORDS: usize = 11;

/// Make the probe findable (tests only). Idempotent.
pub fn install() {
    INSTALLED.store(true, Ordering::Release);
}

pub(crate) fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

pub(crate) fn is_bundle(p: &Path) -> bool {
    p == Path::new(BUNDLE)
}

pub(crate) fn library() -> Arc<Library> {
    static L: OnceLock<Arc<Library>> = OnceLock::new();
    L.get_or_init(|| {
        Arc::new(Library { _lib: None, _entry: &ENTRY, factory: &FACTORY as *const clap_plugin_factory, bundle: PathBuf::from(BUNDLE) })
    })
    .clone()
}

// ---------------------------------------------------------------- what it records

/// A main-thread call, and where it ran.
#[derive(Clone, Debug)]
pub struct Call {
    pub what: &'static str,
    /// Whether it ran on the process's real main thread (`m2_os::thread::is_process_main`).
    pub process_main: bool,
    /// What the host's thread checks answered.
    pub says_main: bool,
    pub says_audio: bool,
    /// The thread's token (`m2_os::thread::token`).
    pub thread: usize,
    /// When it was made, on the counter `process` calls are stamped from ([`Stats::process_calls`]).
    pub tick: u64,
}

/// What one `process` call was told of the transport (HOST-6): CLAP's transport event, read back
/// from its fixed-point form, with the call's `steady_time` and length.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Told {
    pub steady: i64,
    pub frames: u32,
    pub playing: bool,
    pub looping: bool,
    pub tempo: f64,
    pub beats: f64,
    pub seconds: f64,
    pub loop_beats: (f64, f64),
    pub loop_seconds: (f64, f64),
    pub bar_start: f64,
    pub bar: i32,
    pub tsig: (u16, u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    On,
    Off,
    Choke,
}

/// One instance's record. Kept after it is destroyed.
pub struct Stats {
    pub plugin: &'static str,
    /// The tag of the last state it loaded.
    pub tag: Mutex<String>,
    pub calls: Mutex<Vec<Call>>,
    pub process: AtomicU64,
    /// `process` calls where the host said "main thread", or "not the audio thread".
    pub process_says_main: AtomicU64,
    pub process_not_audio: AtomicU64,
    /// `process` calls on the process's main thread.
    pub process_on_main: AtomicU64,
    /// Calls whose block was already processed: the instance ran twice for one engine block.
    pub doubled: AtomicU64,
    /// Times the thread calling `process` changed: an export borrows an instance from the audio
    /// thread and gives it back (RENDER-1), two.
    pub threads: AtomicU64,
    last_thread: AtomicUsize,
    pub start_on_audio: AtomicU64,
    pub start_elsewhere: AtomicU64,
    /// `on_main_thread` calls, and those made inside a `process` call, on its thread (the main
    /// thread running one while another thread processes is CLAP's normal case).
    pub callbacks: AtomicU64,
    pub callbacks_in_process: AtomicU64,
    pub requested: AtomicU64,
    /// Return `Sleep` while no note sounds.
    pub sleep: AtomicBool,
    /// Call `request_restart` from the next `process` call.
    pub restart: AtomicBool,
    /// Refuse the next `activate`, as a plugin that can't start does.
    pub refuse_activate: AtomicBool,
    /// Apply a loaded state at the end of the next `process` call, ending every voice (Surge XT).
    pub defer_load: AtomicBool,
    /// Keep every voice through `reset`, as Surge XT does: only a NoteOff or a choke ends one.
    pub reset_keeps_voices: AtomicBool,
    /// A loaded state's gain and level (as bits), waiting for that call.
    deferred: (AtomicBool, AtomicU64, AtomicU64),
    /// Allocate and free once in the next `process` call: what the audio thread's allocation
    /// counter must see (RENDER-28).
    pub allocate: AtomicBool,
    pub slept: AtomicU64,
    pub resets: AtomicU64,
    /// The last `reset`: its tick and its thread's token.
    pub reset_at: (AtomicU64, AtomicUsize),
    /// Notes sounding, Ons whose id wasn't above every id before, Offs and chokes for a note it
    /// doesn't hold, Ons it had no voice for.
    pub sounding: AtomicU32,
    pub repeated_ids: AtomicU64,
    pub unknown_offs: AtomicU64,
    pub full: AtomicU64,
    max_id: AtomicI64,
    /// The thread inside `process` now, 0 for none.
    in_process: AtomicUsize,
    events: Box<[(AtomicU64, AtomicU64)]>,
    next: AtomicUsize,
    /// Each `process` call: its tick and its thread's token.
    processes: Box<[(AtomicU64, AtomicUsize)]>,
    next_process: AtomicUsize,
    /// The transport each of the last [`TOLD`] calls was told, packed, in a ring.
    told: Box<[[AtomicU64; TOLD_WORDS]]>,
    next_told: AtomicUsize,
}

impl Stats {
    fn new(plugin: &'static str) -> Stats {
        Stats {
            plugin,
            tag: Mutex::new(String::new()),
            calls: Mutex::new(vec![]),
            process: AtomicU64::new(0),
            process_says_main: AtomicU64::new(0),
            process_not_audio: AtomicU64::new(0),
            process_on_main: AtomicU64::new(0),
            doubled: AtomicU64::new(0),
            threads: AtomicU64::new(0),
            last_thread: AtomicUsize::new(0),
            start_on_audio: AtomicU64::new(0),
            start_elsewhere: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            callbacks_in_process: AtomicU64::new(0),
            requested: AtomicU64::new(0),
            sleep: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            refuse_activate: AtomicBool::new(false),
            defer_load: AtomicBool::new(false),
            reset_keeps_voices: AtomicBool::new(false),
            deferred: (AtomicBool::new(false), AtomicU64::new(0), AtomicU64::new(0)),
            allocate: AtomicBool::new(false),
            slept: AtomicU64::new(0),
            resets: AtomicU64::new(0),
            reset_at: (AtomicU64::new(0), AtomicUsize::new(0)),
            sounding: AtomicU32::new(0),
            repeated_ids: AtomicU64::new(0),
            unknown_offs: AtomicU64::new(0),
            full: AtomicU64::new(0),
            max_id: AtomicI64::new(i64::MIN),
            in_process: AtomicUsize::new(0),
            events: (0..EVENTS).map(|_| (AtomicU64::new(0), AtomicU64::new(0))).collect(),
            next: AtomicUsize::new(0),
            processes: (0..PROCESS_LOG).map(|_| (AtomicU64::new(0), AtomicUsize::new(0))).collect(),
            next_process: AtomicUsize::new(0),
            told: (0..TOLD).map(|_| std::array::from_fn(|_| AtomicU64::new(0))).collect(),
            next_told: AtomicUsize::new(0),
        }
    }

    /// What the last calls (up to 32) were told of the transport, oldest first (HOST-6).
    pub fn told(&self) -> Vec<Told> {
        let n = self.next_told.load(Ordering::Acquire);
        let beats = |w: u64| w as i64 as f64 / CLAP_BEATTIME_FACTOR as f64;
        let secs = |w: u64| w as i64 as f64 / CLAP_SECTIME_FACTOR as f64;
        (n.saturating_sub(TOLD)..n)
            .map(|i| {
                let w: [u64; TOLD_WORDS] = std::array::from_fn(|k| self.told[i % TOLD][k].load(Ordering::Relaxed));
                let flags = w[0] as u32;
                Told {
                    steady: w[10] as i64,
                    frames: (w[0] >> 32) as u32,
                    playing: flags & CLAP_TRANSPORT_IS_PLAYING != 0,
                    looping: flags & CLAP_TRANSPORT_IS_LOOP_ACTIVE != 0,
                    tempo: f64::from_bits(w[3]),
                    beats: beats(w[1]),
                    seconds: secs(w[2]),
                    loop_beats: (beats(w[4]), beats(w[5])),
                    loop_seconds: (secs(w[6]), secs(w[7])),
                    bar_start: beats(w[8]),
                    bar: w[9] as u32 as i32,
                    tsig: ((w[9] >> 32) as u16, (w[9] >> 48) as u16),
                }
            })
            .collect()
    }

    /// Keep what a call was told of the transport. Atomic stores only: it runs in `process`.
    fn tell(&self, t: &clap_event_transport, steady: i64, frames: u32) {
        let i = self.next_told.load(Ordering::Relaxed);
        let bar = t.bar_number as u32 as u64 | (t.tsig_num as u64) << 32 | (t.tsig_denom as u64) << 48;
        let w = [
            t.flags as u64 | (frames as u64) << 32,
            t.song_pos_beats as u64,
            t.song_pos_seconds as u64,
            t.tempo.to_bits(),
            t.loop_start_beats as u64,
            t.loop_end_beats as u64,
            t.loop_start_seconds as u64,
            t.loop_end_seconds as u64,
            t.bar_start as u64,
            bar,
            steady as u64,
        ];
        for (slot, v) in self.told[i % TOLD].iter().zip(w) {
            slot.store(v, Ordering::Relaxed);
        }
        self.next_told.store(i + 1, Ordering::Release);
    }

    /// Every `process` call so far, the first [`PROCESS_LOG`] of them: its tick (the counter
    /// [`Call::tick`] reads) and its thread's token, in order.
    pub fn process_calls(&self) -> Vec<(u64, usize)> {
        let n = self.next_process.load(Ordering::Acquire).min(PROCESS_LOG);
        self.processes[..n].iter().map(|(t, th)| (t.load(Ordering::Relaxed), th.load(Ordering::Relaxed))).collect()
    }

    /// Every note event it was sent: `(steady time of its frame, kind, note id)`, in order.
    pub fn notes(&self) -> Vec<(i64, Note, i32)> {
        let n = self.next.load(Ordering::Acquire).min(EVENTS);
        (0..n)
            .map(|i| {
                let (t, k) = (&self.events[i].0, &self.events[i].1);
                let k = k.load(Ordering::Relaxed);
                let kind = match k >> 32 {
                    1 => Note::On,
                    2 => Note::Off,
                    _ => Note::Choke,
                };
                (t.load(Ordering::Relaxed) as i64, kind, k as u32 as i32)
            })
            .collect()
    }

    /// The main-thread calls so far.
    pub fn main_calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn note(&self, at: i64, kind: Note, id: i32) {
        let i = self.next.fetch_add(1, Ordering::AcqRel);
        if let Some((t, k)) = self.events.get(i) {
            let code = match kind {
                Note::On => 1u64,
                Note::Off => 2,
                Note::Choke => 3,
            };
            t.store(at as u64, Ordering::Relaxed);
            k.store(code << 32 | id as u32 as u64, Ordering::Relaxed);
        }
    }
}

struct Registered {
    plugin: usize,
    host: usize,
    stats: Arc<Stats>,
}

fn registry() -> &'static Mutex<Vec<Registered>> {
    static R: OnceLock<Mutex<Vec<Registered>>> = OnceLock::new();
    R.get_or_init(Default::default)
}

/// The record of the probe instance `h` names, while it lives.
pub fn stats(h: Handle) -> Option<Arc<Stats>> {
    let p = InProcess::plugin_ptr(h)?;
    registry().lock().unwrap_or_else(|e| e.into_inner()).iter().rev().find(|r| r.plugin == p).map(|r| r.stats.clone())
}

/// The records of every probe instance whose last loaded state carried `tag`, oldest first.
pub fn tagged(tag: &str) -> Vec<Arc<Stats>> {
    let r = registry().lock().unwrap_or_else(|e| e.into_inner());
    r.iter().filter(|r| *r.stats.tag.lock().unwrap_or_else(|e| e.into_inner()) == tag).map(|r| r.stats.clone()).collect()
}

/// Call `request_process` for the probe instance `h`, from the calling thread, as a plugin's own
/// thread would.
pub fn request_process(h: Handle) -> bool {
    let Some(p) = InProcess::plugin_ptr(h) else { return false };
    let host = registry().lock().unwrap_or_else(|e| e.into_inner()).iter().rev().find(|r| r.plugin == p).map(|r| r.host);
    match host {
        Some(host) => {
            let host = host as *const clap_host;
            if let Some(f) = unsafe { (*host).request_process } {
                unsafe { f(host) };
            }
            true
        }
        None => false,
    }
}

/// A state blob the probe loads as carrying `tag`, with its `Level` at its default.
pub fn state(tag: &str, gain: f64) -> Vec<u8> {
    state_with(tag, gain, 0.1)
}

/// A state blob the probe loads as carrying `tag`, `gain` and `level`.
pub fn state_with(tag: &str, gain: f64, level: f64) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend_from_slice(&gain.to_le_bytes());
    b.extend_from_slice(&level.to_le_bytes());
    b.extend_from_slice(tag.as_bytes());
    b
}

// ---------------------------------------------------------------- the plugin

struct Features([*const c_char; 3]);
// SAFETY: pointers to static strings.
unsafe impl Sync for Features {}

static FEATURES: Features = Features([c"instrument".as_ptr(), c"audio-effect".as_ptr(), null()]);

const fn descriptor(id: &'static CStr, name: &'static CStr, features: *const *const c_char) -> clap_plugin_descriptor {
    clap_plugin_descriptor {
        clap_version: CLAP_VERSION,
        id: id.as_ptr(),
        name: name.as_ptr(),
        vendor: c"music2".as_ptr(),
        url: c"".as_ptr(),
        manual_url: c"".as_ptr(),
        support_url: c"".as_ptr(),
        version: c"1.0.0".as_ptr(),
        description: c"the host's test plugin".as_ptr(),
        features,
    }
}

static PROBE: clap_plugin_descriptor = descriptor(c"m2.probe", c"m2 probe", FEATURES.0.as_ptr());
static FAILING: clap_plugin_descriptor = descriptor(c"m2.probe.fails-state", c"m2 probe (fails state)", FEATURES.0.as_ptr());

static FACTORY: clap_plugin_factory =
    clap_plugin_factory { get_plugin_count: Some(plugin_count), get_plugin_descriptor: Some(plugin_descriptor), create_plugin: Some(create) };

static ENTRY: clap_plugin_entry = clap_plugin_entry { clap_version: CLAP_VERSION, init: Some(entry_init), deinit: Some(entry_deinit), get_factory: Some(get_factory) };

unsafe extern "C" fn entry_init(_: *const c_char) -> bool {
    true
}

unsafe extern "C" fn entry_deinit() {}

unsafe extern "C" fn get_factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID { &FACTORY as *const _ as *const c_void } else { null() }
}

unsafe extern "C" fn plugin_count(_: *const clap_plugin_factory) -> u32 {
    2
}

unsafe extern "C" fn plugin_descriptor(_: *const clap_plugin_factory, i: u32) -> *const clap_plugin_descriptor {
    match i {
        0 => &PROBE,
        1 => &FAILING,
        _ => null(),
    }
}

#[derive(Clone, Copy, Default)]
struct Voice {
    id: i32,
    key: i16,
    phase: f64,
    on: bool,
}

struct Probe {
    clap: clap_plugin,
    host: *const clap_host,
    checks: Option<&'static clap_host_thread_check>,
    fails_state: bool,
    /// Its state's tag starts with `fails-process`: every `process` call reports an error.
    fails_process: bool,
    /// Its state's tag starts with `sets-fz`: every `process` call ends by setting flush-to-zero.
    sets_fz: bool,
    /// Its state's tag is `crash-at-<n>` or `hang-at-<n>`: the call, whether it hangs, and the
    /// file that makes it happen once.
    stops_at: Option<(u64, bool, Option<PathBuf>)>,
    stats: Arc<Stats>,
    rate: f64,
    gain: f64,
    level: f64,
    voices: [Voice; VOICES],
    /// The steady time after the last block processed.
    done_to: i64,
    calls: u64,
}

unsafe fn probe<'a>(p: *const clap_plugin) -> &'a mut Probe {
    unsafe { &mut *((*p).plugin_data as *mut Probe) }
}

impl Probe {
    fn thread(&self) -> (bool, bool) {
        match self.checks {
            Some(c) => (
                c.is_main_thread.is_some_and(|f| unsafe { f(self.host) }),
                c.is_audio_thread.is_some_and(|f| unsafe { f(self.host) }),
            ),
            None => (false, false),
        }
    }

    /// Record a main-thread call (off the audio thread: it locks and allocates).
    fn record(&self, what: &'static str) {
        let (says_main, says_audio) = self.thread();
        let call = Call {
            what,
            process_main: crate::main_thread::is_process_main(),
            says_main,
            says_audio,
            thread: crate::main_thread::token(),
            tick: TICK.fetch_add(1, Ordering::AcqRel),
        };
        self.stats.calls.lock().unwrap_or_else(|e| e.into_inner()).push(call);
    }

    fn sounding(&self) -> u32 {
        self.voices.iter().filter(|v| v.on).count() as u32
    }

    fn apply(&mut self, h: *const clap_event_header, steady: i64) {
        let e = unsafe { &*h };
        if e.space_id != CLAP_CORE_EVENT_SPACE_ID {
            return;
        }
        let at = steady + e.time as i64;
        match e.type_ {
            CLAP_EVENT_NOTE_ON => {
                let n = unsafe { &*(h as *const clap_event_note) };
                self.stats.note(at, Note::On, n.note_id);
                if (n.note_id as i64) <= self.stats.max_id.load(Ordering::Relaxed) {
                    self.stats.repeated_ids.fetch_add(1, Ordering::Relaxed);
                }
                self.stats.max_id.fetch_max(n.note_id as i64, Ordering::Relaxed);
                match self.voices.iter_mut().find(|v| !v.on) {
                    Some(v) => *v = Voice { id: n.note_id, key: n.key, phase: 0.0, on: true },
                    None => {
                        self.stats.full.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE => {
                let n = unsafe { &*(h as *const clap_event_note) };
                let kind = if e.type_ == CLAP_EVENT_NOTE_OFF { Note::Off } else { Note::Choke };
                self.stats.note(at, kind, n.note_id);
                let held = self.voices.iter_mut().find(|v| v.on && (v.id == n.note_id || (n.note_id < 0 && v.key == n.key)));
                match held {
                    Some(v) => v.on = false,
                    None if kind == Note::Off => {
                        self.stats.unknown_offs.fetch_add(1, Ordering::Relaxed);
                    }
                    // A choke follows its Off after a reset: nothing left to end.
                    None => {}
                }
            }
            CLAP_EVENT_PARAM_VALUE => {
                let p = unsafe { &*(h as *const clap_event_param_value) };
                match p.param_id {
                    GAIN => self.gain = p.value,
                    LEVEL => self.level = p.value,
                    _ => {}
                }
            }
            _ => {}
        }
        self.stats.sounding.store(self.sounding(), Ordering::Relaxed);
    }
}

unsafe extern "C" fn create(_: *const clap_plugin_factory, host: *const clap_host, id: *const c_char) -> *const clap_plugin {
    let id = unsafe { CStr::from_ptr(id) };
    let (desc, fails_state, name): (&'static clap_plugin_descriptor, bool, &'static str) = if id == c"m2.probe" {
        (&PROBE, false, ID)
    } else if id == c"m2.probe.fails-state" {
        (&FAILING, true, FAILS_STATE)
    } else {
        return null();
    };
    let stats = Arc::new(Stats::new(name));
    let p = Box::new(Probe {
        clap: clap_plugin {
            desc,
            plugin_data: std::ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start_processing),
            stop_processing: Some(stop_processing),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(on_main_thread),
        },
        host,
        checks: None,
        fails_state,
        fails_process: false,
        sets_fz: false,
        stops_at: None,
        stats: stats.clone(),
        rate: 48_000.0,
        gain: 1.0,
        level: 0.1,
        voices: [Voice::default(); VOICES],
        done_to: i64::MIN,
        calls: 0,
    });
    let raw = Box::into_raw(p);
    unsafe { (*raw).clap.plugin_data = raw as *mut c_void };
    let plugin = unsafe { &(*raw).clap as *const clap_plugin };
    registry().lock().unwrap_or_else(|e| e.into_inner()).push(Registered { plugin: plugin as usize, host: host as usize, stats });
    plugin
}

unsafe extern "C" fn init(p: *const clap_plugin) -> bool {
    let me = unsafe { probe(p) };
    let get = unsafe { (*me.host).get_extension };
    let ext = get.map_or(null(), |g| unsafe { g(me.host, CLAP_EXT_THREAD_CHECK.as_ptr()) }) as *const clap_host_thread_check;
    me.checks = (!ext.is_null()).then(|| unsafe { &*ext });
    me.record("init");
    true
}

unsafe extern "C" fn destroy(p: *const clap_plugin) {
    let me = unsafe { probe(p) };
    me.record("destroy");
    drop(unsafe { Box::from_raw(me as *mut Probe) });
}

unsafe extern "C" fn activate(p: *const clap_plugin, rate: f64, _: u32, _: u32) -> bool {
    let me = unsafe { probe(p) };
    me.record("activate");
    if me.stats.refuse_activate.swap(false, Ordering::Relaxed) {
        return false;
    }
    me.rate = rate;
    // A new activation is a new steady timeline: an export's starts at 0 (RENDER-1).
    me.done_to = i64::MIN;
    true
}

unsafe extern "C" fn deactivate(p: *const clap_plugin) {
    unsafe { probe(p) }.record("deactivate");
}

unsafe extern "C" fn start_processing(p: *const clap_plugin) -> bool {
    let me = unsafe { probe(p) };
    match me.thread() {
        (false, true) => me.stats.start_on_audio.fetch_add(1, Ordering::Relaxed),
        _ => me.stats.start_elsewhere.fetch_add(1, Ordering::Relaxed),
    };
    true
}

unsafe extern "C" fn stop_processing(p: *const clap_plugin) {
    unsafe { probe(p) }.record("stop_processing");
}

unsafe extern "C" fn reset(p: *const clap_plugin) {
    let me = unsafe { probe(p) };
    me.stats.resets.fetch_add(1, Ordering::Relaxed);
    me.stats.reset_at.0.store(TICK.fetch_add(1, Ordering::AcqRel), Ordering::Relaxed);
    me.stats.reset_at.1.store(crate::main_thread::token(), Ordering::Relaxed);
    if me.stats.reset_keeps_voices.load(Ordering::Relaxed) {
        return;
    }
    for v in &mut me.voices {
        v.on = false;
    }
    me.stats.sounding.store(0, Ordering::Relaxed);
}

unsafe extern "C" fn on_main_thread(p: *const clap_plugin) {
    let me = unsafe { probe(p) };
    me.stats.callbacks.fetch_add(1, Ordering::Relaxed);
    if me.stats.in_process.load(Ordering::Acquire) == crate::main_thread::token() {
        me.stats.callbacks_in_process.fetch_add(1, Ordering::Relaxed);
    }
    me.record("on_main_thread");
}

/// Never allocates or locks (unless asked to, `allocate`): it runs under the audio thread's
/// allocation trap.
unsafe extern "C" fn process(p: *const clap_plugin, proc_: *const clap_process) -> clap_process_status {
    let me = unsafe { probe(p) };
    let pr = unsafe { &*proc_ };
    let st = me.stats.clone();
    st.in_process.store(crate::main_thread::token(), Ordering::Release);
    st.process.fetch_add(1, Ordering::Relaxed);
    let (says_main, says_audio) = me.thread();
    if says_main {
        st.process_says_main.fetch_add(1, Ordering::Relaxed);
    }
    if !says_audio {
        st.process_not_audio.fetch_add(1, Ordering::Relaxed);
    }
    if crate::main_thread::is_process_main() {
        st.process_on_main.fetch_add(1, Ordering::Relaxed);
    }
    if pr.steady_time < me.done_to {
        st.doubled.fetch_add(1, Ordering::Relaxed);
    }
    let thread = crate::main_thread::token();
    let i = st.next_process.fetch_add(1, Ordering::AcqRel);
    if let Some((t, th)) = st.processes.get(i) {
        t.store(TICK.fetch_add(1, Ordering::AcqRel), Ordering::Relaxed);
        th.store(thread, Ordering::Relaxed);
    }
    if !pr.transport.is_null() {
        st.tell(unsafe { &*pr.transport }, pr.steady_time, pr.frames_count);
    }
    let last = st.last_thread.swap(thread, Ordering::Relaxed);
    if last != 0 && last != thread {
        st.threads.fetch_add(1, Ordering::Relaxed);
    }
    me.done_to = pr.steady_time + pr.frames_count as i64;
    if let Some((n, hang, once)) = &me.stops_at
        && me.calls + 1 == *n
        && once.as_ref().is_none_or(|f| std::fs::OpenOptions::new().write(true).create_new(true).open(f).is_ok())
    {
        match hang {
            true => loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            },
            false => std::process::abort(),
        }
    }
    if me.fails_process {
        st.in_process.store(0, Ordering::Release);
        return CLAP_PROCESS_ERROR;
    }
    let n = pr.frames_count as usize;
    let (ins, outs) = unsafe {
        let i = (pr.audio_inputs_count > 0 && !pr.audio_inputs.is_null()).then(|| &*pr.audio_inputs);
        let o = (pr.audio_outputs_count > 0 && !pr.audio_outputs.is_null()).then(|| &mut *pr.audio_outputs);
        (i, o)
    };
    let events = unsafe { &*pr.in_events };
    let count = events.size.map_or(0, |f| unsafe { f(pr.in_events) });
    let mut next = 0;
    let step = std::f64::consts::TAU / me.rate;
    for f in 0..n {
        while next < count {
            let h = events.get.map_or(null(), |g| unsafe { g(pr.in_events, next) });
            if h.is_null() || unsafe { (*h).time } as usize > f {
                break;
            }
            me.apply(h, pr.steady_time);
            next += 1;
        }
        let mut s = 0.0f64;
        for v in me.voices.iter_mut().filter(|v| v.on) {
            s += v.phase.sin() * me.level;
            v.phase += step * 440.0 * 2f64.powf((v.key as f64 - 69.0) / 12.0);
        }
        if let Some(o) = &outs {
            for c in 0..o.channel_count as usize {
                let x = ins.filter(|i| i.channel_count > 0).map_or(0.0, |i| {
                    let ch = c.min(i.channel_count as usize - 1);
                    unsafe { *(*i.data32.add(ch)).add(f) }
                });
                unsafe { *(*o.data32.add(c)).add(f) = (x as f64 * me.gain + s) as f32 };
            }
        }
    }
    while next < count {
        let h = events.get.map_or(null(), |g| unsafe { g(pr.in_events, next) });
        if !h.is_null() {
            me.apply(h, pr.steady_time);
        }
        next += 1;
    }
    me.calls += 1;
    if me.calls.is_multiple_of(10)
        && let Some(f) = unsafe { (*me.host).request_callback }
    {
        st.requested.fetch_add(1, Ordering::Relaxed);
        unsafe { f(me.host) };
    }
    // A deferred state: its values over what this call's events set, and every voice ended.
    if st.deferred.0.swap(false, Ordering::AcqRel) {
        me.gain = f64::from_bits(st.deferred.1.load(Ordering::Relaxed));
        me.level = f64::from_bits(st.deferred.2.load(Ordering::Relaxed));
        for v in &mut me.voices {
            v.on = false;
        }
        st.sounding.store(0, Ordering::Relaxed);
    }
    if st.allocate.swap(false, Ordering::Relaxed) {
        drop(std::hint::black_box(Box::new(0u64)));
    }
    if st.restart.swap(false, Ordering::Relaxed)
        && let Some(f) = unsafe { (*me.host).request_restart }
    {
        unsafe { f(me.host) };
    }
    st.in_process.store(0, Ordering::Release);
    if me.sets_fz {
        flush_to_zero();
    }
    if st.sleep.load(Ordering::Relaxed) && me.sounding() == 0 {
        st.slept.fetch_add(1, Ordering::Relaxed);
        return CLAP_PROCESS_SLEEP;
    }
    CLAP_PROCESS_CONTINUE
}

/// Leave the calling thread flushing subnormals to zero, as some plugins do: MXCSR's FZ and DAZ on
/// x86-64, FPCR's FZ on arm64 (RENDER-2a).
fn flush_to_zero() {
    #[cfg(target_arch = "x86_64")]
    {
        let mut csr: u32 = 0;
        // SAFETY: reads MXCSR into a local, then loads it back with FZ (bit 15) and DAZ (bit 6) set.
        unsafe {
            std::arch::asm!("stmxcsr [{}]", in(reg) &mut csr, options(nostack, preserves_flags));
            csr |= 0x8040;
            std::arch::asm!("ldmxcsr [{}]", in(reg) &csr, options(nostack, readonly, preserves_flags));
        }
    }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: reads FPCR, then writes it back with FZ (bit 24) set.
    unsafe {
        let fpcr: u64;
        std::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags));
        std::arch::asm!("msr fpcr, {}", in(reg) fpcr | 1 << 24, options(nomem, nostack, preserves_flags));
    }
}

unsafe extern "C" fn get_extension(_: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        &AUDIO_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTE_PORTS as *const _ as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as *const c_void
    } else if id == CLAP_EXT_LATENCY {
        &LATENCY as *const _ as *const c_void
    } else if id == CLAP_EXT_RENDER {
        &RENDER as *const _ as *const c_void
    } else {
        null()
    }
}

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports { count: Some(audio_count), get: Some(audio_get) };
static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports { count: Some(note_count), get: Some(note_get) };
static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(param_count),
    get_info: Some(param_info),
    get_value: Some(param_value),
    value_to_text: Some(value_to_text),
    text_to_value: None,
    flush: Some(flush),
};
static STATE: clap_plugin_state = clap_plugin_state { save: Some(save), load: Some(load) };
static LATENCY: clap_plugin_latency = clap_plugin_latency { get: Some(latency) };
static RENDER: clap_plugin_render = clap_plugin_render { has_hard_realtime_requirement: Some(no), set: Some(render_set) };

fn name<const N: usize>(s: &str) -> [c_char; N] {
    let mut out = [0 as c_char; N];
    for (o, b) in out.iter_mut().zip(s.bytes().take(N - 1)) {
        *o = b as c_char;
    }
    out
}

unsafe extern "C" fn audio_count(_: *const clap_plugin, _: bool) -> u32 {
    1
}

unsafe extern "C" fn audio_get(_: *const clap_plugin, i: u32, _: bool, info: *mut clap_audio_port_info) -> bool {
    if i != 0 {
        return false;
    }
    unsafe {
        *info = clap_audio_port_info {
            id: 0,
            name: name("main"),
            flags: CLAP_AUDIO_PORT_IS_MAIN,
            channel_count: 2,
            port_type: CLAP_PORT_STEREO.as_ptr(),
            in_place_pair: CLAP_INVALID_ID,
        }
    };
    true
}

unsafe extern "C" fn note_count(_: *const clap_plugin, is_input: bool) -> u32 {
    is_input as u32
}

unsafe extern "C" fn note_get(_: *const clap_plugin, i: u32, is_input: bool, info: *mut clap_note_port_info) -> bool {
    if i != 0 || !is_input {
        return false;
    }
    unsafe {
        *info = clap_note_port_info { id: 0, supported_dialects: CLAP_NOTE_DIALECT_CLAP, preferred_dialect: CLAP_NOTE_DIALECT_CLAP, name: name("notes") }
    };
    true
}

unsafe extern "C" fn param_count(_: *const clap_plugin) -> u32 {
    2
}

unsafe extern "C" fn param_info(_: *const clap_plugin, i: u32, info: *mut clap_param_info) -> bool {
    let (id, n, max, default) = match i {
        0 => (GAIN, "Gain", 2.0, 1.0),
        1 => (LEVEL, "Level", 1.0, 0.1),
        _ => return false,
    };
    unsafe {
        *info = clap_param_info {
            id,
            flags: CLAP_PARAM_IS_AUTOMATABLE,
            cookie: std::ptr::null_mut(),
            name: name(n),
            module: name(""),
            min_value: 0.0,
            max_value: max,
            default_value: default,
        }
    };
    true
}

unsafe extern "C" fn param_value(p: *const clap_plugin, id: clap_id, out: *mut f64) -> bool {
    let me = unsafe { probe(p) };
    let v = match id {
        GAIN => me.gain,
        LEVEL => me.level,
        _ => return false,
    };
    unsafe { *out = v };
    true
}

unsafe extern "C" fn value_to_text(_: *const clap_plugin, id: clap_id, v: f64, out: *mut c_char, cap: u32) -> bool {
    if !matches!(id, GAIN | LEVEL) || cap == 0 {
        return false;
    }
    let text = format!("{v:.2}");
    let n = text.len().min(cap as usize - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(text.as_ptr() as *const c_char, out, n);
        *out.add(n) = 0;
    }
    true
}

unsafe extern "C" fn flush(_: *const clap_plugin, _: *const clap_input_events, _: *const clap_output_events) {}

unsafe extern "C" fn save(p: *const clap_plugin, s: *const clap_ostream) -> bool {
    let me = unsafe { probe(p) };
    me.record("save");
    let tag = me.stats.tag.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut bytes = state(&tag, me.gain);
    bytes[MAGIC.len() + 8..MAGIC.len() + 16].copy_from_slice(&me.level.to_le_bytes());
    let Some(write) = (unsafe { (*s).write }) else { return false };
    let mut at = 0;
    while at < bytes.len() {
        let n = unsafe { write(s, bytes[at..].as_ptr() as *const c_void, (bytes.len() - at) as u64) };
        if n <= 0 {
            return false;
        }
        at += n as usize;
    }
    true
}

unsafe extern "C" fn load(p: *const clap_plugin, s: *const clap_istream) -> bool {
    let me = unsafe { probe(p) };
    me.record("load");
    let Some(read) = (unsafe { (*s).read }) else { return false };
    let mut bytes = vec![];
    let mut buf = [0u8; 4096];
    loop {
        let n = unsafe { read(s, buf.as_mut_ptr() as *mut c_void, buf.len() as u64) };
        if n <= 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n as usize]);
    }
    if me.fails_state && bytes.starts_with(b"fail") {
        return false;
    }
    if bytes.starts_with(b"slow") {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let tag = if bytes.starts_with(MAGIC) && bytes.len() >= MAGIC.len() + 16 {
        let f = |at: usize| f64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"));
        let (gain, level) = (f(MAGIC.len()), f(MAGIC.len() + 8));
        if me.stats.defer_load.load(Ordering::Relaxed) {
            let d = &me.stats.deferred;
            d.1.store(gain.to_bits(), Ordering::Relaxed);
            d.2.store(level.to_bits(), Ordering::Relaxed);
            d.0.store(true, Ordering::Release);
        } else {
            (me.gain, me.level) = (gain, level);
        }
        String::from_utf8_lossy(&bytes[MAGIC.len() + 16..]).into_owned()
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    me.fails_process = tag.starts_with("fails-process");
    me.sets_fz = tag.starts_with("sets-fz");
    me.stops_at = stops_at(&tag);
    *me.stats.tag.lock().unwrap_or_else(|e| e.into_inner()) = tag;
    true
}

/// `crash-at-<n>[:<file>]` or `hang-at-<n>[:<file>]`.
fn stops_at(tag: &str) -> Option<(u64, bool, Option<PathBuf>)> {
    let (hang, rest) = match (tag.strip_prefix("crash-at-"), tag.strip_prefix("hang-at-")) {
        (Some(r), _) => (false, r),
        (_, Some(r)) => (true, r),
        _ => return None,
    };
    let (n, file) = match rest.split_once(':') {
        Some((n, f)) => (n, Some(PathBuf::from(f))),
        None => (rest, None),
    };
    Some((n.parse().ok()?, hang, file))
}

unsafe extern "C" fn latency(_: *const clap_plugin) -> u32 {
    0
}

unsafe extern "C" fn no(_: *const clap_plugin) -> bool {
    false
}

unsafe extern "C" fn render_set(p: *const clap_plugin, mode: clap_plugin_render_mode) -> bool {
    unsafe { probe(p) }.record(if mode == CLAP_RENDER_OFFLINE { "render_offline" } else { "render_realtime" });
    true
}

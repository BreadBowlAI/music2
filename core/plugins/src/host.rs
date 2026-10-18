//! `PluginHost` (P11, HOST-1): the one way the engine reaches a plugin, and the types its calls
//! carry. Nothing here assumes the plugin shares this process or its threads: the in-process host
//! ([`crate::InProcess`]) is one implementation, and the host process (HOST-3) will be another.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use clap_sys::events::*;
use crossbeam_channel::{Receiver, Sender, unbounded};
use sha2::{Digest, Sha256};

use crate::{Event, Found, Info, Transport};

/// How the engine reaches a plugin (P11, HOST-1). Each method runs on the thread its group names;
/// the host marshals main-thread calls itself, so any thread but the audio thread may call those.
pub trait PluginHost: Send + Sync {
    // The host's main thread. The caller waits; on the main thread itself they run inline.
    /// Create and init `plugin`, loading `state` when given (CLAP: init, then `state.load`), and
    /// read its ports and parameters. Not active. Its unsolicited events go to `owner`, which also
    /// counts it.
    fn create(&self, plugin: &Found, state: Option<Arc<Blob>>, owner: &Owner) -> Result<(Handle, Info)>;
    /// Activate at `sample_rate` for calls of at most `max_frames`, and measure the latency then
    /// (RENDER-17: after the state `create` loaded). CLAP reads latency only while activating or
    /// active.
    fn activate(&self, h: Handle, sample_rate: u32, max_frames: u32) -> Result<u32>;
    /// Only while no renderer calls `h`: none holds it, or every one that does is held (an export,
    /// RENDER-1). The main thread then takes the instance's audio role to stop processing first,
    /// which CLAP's thread model allows.
    fn deactivate(&self, h: Handle);
    /// Load `blob`, active or not (CLAP's `state.load` is a main-thread call). Skipped when the
    /// blob's hash equals the state the instance holds: the one it last saved or last loaded, at
    /// creation included (HOME-6). True when it loaded. After a load that fails it holds nothing
    /// known, so the next load isn't skipped.
    fn load_state(&self, h: Handle, blob: Arc<Blob>) -> Result<bool>;
    /// Realtime or offline (CLAP's render extension); false when the plugin has no such mode.
    /// CLAP's `render.set` is a main-thread call; music2 makes it while the instance is inactive.
    fn set_render_mode(&self, h: Handle, mode: RenderMode) -> Result<bool>;
    fn main_call(&self, h: Handle, call: MainCall) -> Result<Reply>;
    /// Queued: returns at once, so a renderer dropped on any thread never waits. Stops processing
    /// and deactivates first. Only once no renderer holds `h`.
    fn destroy(&self, h: Handle);
    // Queued to the main thread: the result comes back as one message on the reply returned.
    fn latency(&self, h: Handle) -> Pending<Result<u32, String>>;
    fn save_state(&self, h: Handle) -> Pending<Result<Blob, String>>;
    // The audio thread (or an offline render's): never allocates, locks or waits (RENDER-28).
    fn process(&self, h: Handle, io: &mut SharedBuffers, ev_in: &Events, ev_out: &mut Events, t: &Transport) -> Status;
    /// Panic, and before an offline render: voices and tails cleared.
    fn reset(&self, h: Handle);
    /// Whether the plugin asked to be processed since the last call: wakes an idle engine.
    fn take_process_request(&self, h: Handle) -> bool;
    /// Whether the process `h` ran in ended (HOST-3): its calls fail, and its owner hears
    /// [`HostEvent::Crashed`] or [`HostEvent::Hung`]. An atomic load: the audio thread asks, to
    /// tell such a failure from the plugin's own. Never, for plugins in this process.
    fn gone(&self, h: Handle) -> bool {
        let _ = h;
        false
    }
    // Any thread but the audio thread.
    /// Every plugin installed, and why each bundle that failed to load did: the host loads them,
    /// so a program that hosts plugins in another process never loads plugin code (HOST-3).
    fn scan(&self) -> (Vec<Found>, Vec<String>);
    /// The presets `plugin` offers itself, through CLAP's preset discovery (HOST-8): each
    /// provider's locations inside the plugin, and its files (at most [`MAX_PRESET_FILES`] read).
    /// Found where the plugin runs, on the host's main thread. None for a plugin with no such
    /// factory; an error, in words, when its library doesn't load.
    fn presets(&self, plugin: &Found) -> Result<Vec<Offered>>;
    // Any thread.
    /// Instances created and not yet destroyed, across every owner.
    fn live(&self) -> u64;
    /// Instances created and destroyed so far, across every owner (gates and tests).
    fn counts(&self) -> (u64, u64);
}

/// An instance, as the host names it. Copied freely; meaningless once destroyed (a stale handle
/// does nothing).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Handle(pub u64);

/// A plugin's state as bytes, with their SHA-256.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub sha: [u8; 32],
}

impl Blob {
    pub fn new(bytes: Vec<u8>) -> Blob {
        let sha = Sha256::digest(&bytes).into();
        Blob { bytes, sha }
    }
}

/// A result queued to the main thread: exactly one message arrives (the host never drops the
/// sender unsent).
pub type Pending<T> = Receiver<T>;

/// What the plugin asked for on its own, outside any call: sent to its creator's [`Owner`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// `request_restart`: deactivate and activate again. Such an instance isn't carried again.
    Restart(Handle),
    /// `state.mark_dirty`: its state changed with no gesture.
    Dirty(Handle),
    /// `latency.changed`, measured again.
    Latency(Handle, u32),
    /// The host process it ran in crashed (HOST-3): it plays silence, and its calls fail, until
    /// the next activation makes it again.
    Crashed(Handle),
    /// The same, when the host process stopped answering a live call and was ended (D5).
    Hung(Handle),
    /// The person closed its window (HOST-7): the plugin's view is gone, and the window with it.
    WindowClosed(Handle),
}

/// Where an [`Owner`]'s events arrive.
pub type HostEvents = Receiver<HostEvent>;

/// Who created an instance: where its unsolicited events go, and how many of its instances are
/// alive. Each session, test and rig has its own, so none sees another's events.
#[derive(Clone, Debug)]
pub struct Owner {
    events: Sender<HostEvent>,
    live: Arc<AtomicU64>,
    /// Its instances created so far, destroyed or not.
    made: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
    /// The plugins being created for it now, by name.
    loading: Arc<Mutex<Vec<String>>>,
}

impl Owner {
    /// An owner and the receiver of its events.
    pub fn new() -> (Owner, HostEvents) {
        let (events, rx) = unbounded();
        (Owner { events, live: Arc::default(), made: Arc::default(), dropped: Arc::default(), loading: Arc::default() }, rx)
    }

    /// The plugins being created for it now, by name: what "loading <plugin>" shows (RENDER-25).
    pub fn loading(&self) -> Vec<String> {
        self.loading.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// `name` shows as loading until the guard drops.
    pub(crate) fn creating(&self, name: &str) -> Creating<'_> {
        self.loading.lock().unwrap_or_else(|e| e.into_inner()).push(name.to_string());
        Creating { owner: self, name: name.to_string() }
    }

    /// An owner whose events nobody reads (describing a plugin, an export): they are counted only.
    pub fn discard() -> Owner {
        Owner::new().0
    }

    /// Its instances created and not yet destroyed.
    pub fn live(&self) -> u64 {
        self.live.load(Ordering::Acquire)
    }

    /// Its instances created so far, destroyed or not: what a test or a gate counts, which other
    /// owners' instances, made at the same time, never add to.
    pub fn made(&self) -> u64 {
        self.made.load(Ordering::Acquire)
    }

    /// Events nobody received.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub(crate) fn send(&self, e: HostEvent) {
        if self.events.try_send(e).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn created(&self) {
        self.made.fetch_add(1, Ordering::AcqRel);
        self.live.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn destroyed(&self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A plugin shown as loading while it is created ([`Owner::loading`]).
pub(crate) struct Creating<'a> {
    owner: &'a Owner,
    name: String,
}

impl Drop for Creating<'_> {
    fn drop(&mut self) {
        let mut l = self.owner.loading.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = l.iter().position(|n| *n == self.name) {
            l.remove(i);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Realtime,
    Offline,
}

/// Other main-thread calls.
#[derive(Clone, Debug, PartialEq)]
pub enum MainCall {
    /// Its own window (HOST-7): CLAP's `gui`, embedded in a window of the host's.
    Window(WindowCall),
    /// A parameter's current value (`params.get_value`).
    Value(u32),
    /// How the plugin displays `value` of a parameter (`params.value_to_text`).
    Text(u32, f64),
    /// Its ports and parameters, read again.
    Info,
    /// Warnings and errors it logged since the last call.
    TakeLog,
    /// Load one of its own presets ([`PluginHost::presets`], CLAP's `preset-load`): only on an
    /// instance made for it (HOST-8), whose state is then saved. Answers [`Reply::Loaded`].
    LoadPreset { location: PresetAt, key: Option<String> },
}

/// Where a plugin's own preset is (CLAP's preset discovery).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetAt {
    /// Inside the plugin.
    Plugin,
    /// In this file.
    File(std::path::PathBuf),
}

/// A preset a plugin offers itself (HOST-8): its name, and where `preset-load` finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub name: String,
    pub location: PresetAt,
    /// Which of the location's presets, when it holds several (CLAP's `load_key`).
    pub key: Option<String>,
}

/// The most files [`PluginHost::presets`] reads for one plugin.
pub const MAX_PRESET_FILES: usize = 2_000;

/// What to do with a plugin's window. The window belongs to the process that hosts the plugin:
/// what tests ask of it is asked there too ([`WindowCall::Shown`], [`WindowCall::AskToClose`]),
/// never of its handle in another process (on macOS a handle is a pointer into the host process).
#[derive(Clone, Debug, PartialEq)]
pub enum WindowCall {
    /// Open it, titled `title`, or bring it to the front when it is open. An error, in plain
    /// words, when the plugin has no window this OS can show, or there is no screen.
    Open { title: String },
    Close,
    /// Tests (debug builds): its window as the process hosting it sees it ([`Reply::Shown`]).
    #[cfg(debug_assertions)]
    Shown,
    /// Tests (debug builds): the person's close of its window, as the title bar's close button
    /// does it, made by the process hosting it. Answers whether one was open; its owner hears
    /// [`HostEvent::WindowClosed`] after.
    #[cfg(debug_assertions)]
    AskToClose,
}

#[derive(Clone, Debug)]
pub enum Reply {
    /// Whether its window is open now.
    Window(bool),
    Value(Option<f64>),
    Text(Option<String>),
    Info(Box<Info>),
    Log(Vec<String>),
    /// A preset loaded ([`MainCall::LoadPreset`]).
    Loaded,
    /// Tests (debug builds): its window's handle, as the plugin was given it (to compare only),
    /// and whether the window is shown; None when it has no window.
    #[cfg(debug_assertions)]
    Shown(Option<(m2_os::window::Native, bool)>),
}

/// What a `process` call reported (CLAP's process status). No error value: the audio thread can't
/// build one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Continue,
    ContinueIfNotQuiet,
    Tail,
    /// Nothing to do until its input changes or it asks to be processed.
    Sleep,
    Error,
}

/// One call's audio: up to two channels in and out, `frames` long. A missing input channel reads
/// the first; a channel the plugin doesn't output is written with silence.
pub struct SharedBuffers<'a> {
    pub inputs: [&'a [f32]; 2],
    pub n_in: usize,
    pub outputs: [&'a mut [f32]; 2],
    pub n_out: usize,
    pub frames: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union Raw {
    pub header: clap_event_header,
    note: clap_event_note,
    expr: clap_event_note_expression,
    param: clap_event_param_value,
    midi: clap_event_midi,
    gesture: clap_event_param_gesture,
}

fn header(size: usize, time: u32, type_: u16) -> clap_event_header {
    clap_event_header { size: size as u32, time, space_id: CLAP_CORE_EVENT_SPACE_ID, type_, flags: 0 }
}

fn raw(e: &Event) -> Raw {
    use std::mem::size_of;
    let note = |time, type_, id, key, velocity| Raw {
        note: clap_event_note { header: header(size_of::<clap_event_note>(), time, type_), note_id: id, port_index: 0, channel: 0, key, velocity },
    };
    match *e {
        Event::NoteOn { time, id, key, velocity } => note(time, CLAP_EVENT_NOTE_ON, id, key, velocity),
        Event::NoteOff { time, id, key } => note(time, CLAP_EVENT_NOTE_OFF, id, key, 0.0),
        Event::NoteChoke { time, id, key } => note(time, CLAP_EVENT_NOTE_CHOKE, id, key, 0.0),
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
        Event::Gesture { time, id, begin } => Raw {
            gesture: clap_event_param_gesture {
                header: header(
                    size_of::<clap_event_param_gesture>(),
                    time,
                    if begin { CLAP_EVENT_PARAM_GESTURE_BEGIN } else { CLAP_EVENT_PARAM_GESTURE_END },
                ),
                param_id: id,
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
        Event::Midi { time, bytes } => {
            Raw { midi: clap_event_midi { header: header(size_of::<clap_event_midi>(), time, CLAP_EVENT_MIDI), port_index: 0, data: bytes } }
        }
    }
}

/// The event a CLAP header holds, when it is one of ours.
///
/// # Safety
/// `h` points to a whole event of the size its header gives.
pub(crate) unsafe fn decode(h: *const clap_event_header) -> Option<Event> {
    let hd = unsafe { &*h };
    if hd.space_id != CLAP_CORE_EVENT_SPACE_ID {
        return None;
    }
    let time = hd.time;
    match hd.type_ {
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF | CLAP_EVENT_NOTE_CHOKE => {
            let n = unsafe { &*(h as *const clap_event_note) };
            let (id, key) = (n.note_id, n.key);
            Some(match hd.type_ {
                CLAP_EVENT_NOTE_ON => Event::NoteOn { time, id, key, velocity: n.velocity },
                CLAP_EVENT_NOTE_OFF => Event::NoteOff { time, id, key },
                _ => Event::NoteChoke { time, id, key },
            })
        }
        CLAP_EVENT_NOTE_EXPRESSION => {
            let x = unsafe { &*(h as *const clap_event_note_expression) };
            (x.expression_id == CLAP_NOTE_EXPRESSION_TUNING).then_some(Event::Tuning { time, id: x.note_id, key: x.key, semitones: x.value })
        }
        CLAP_EVENT_PARAM_VALUE => {
            let p = unsafe { &*(h as *const clap_event_param_value) };
            Some(Event::Param { time, id: p.param_id, value: p.value })
        }
        CLAP_EVENT_MIDI => {
            let m = unsafe { &*(h as *const clap_event_midi) };
            Some(Event::Midi { time, bytes: m.data })
        }
        CLAP_EVENT_PARAM_GESTURE_BEGIN | CLAP_EVENT_PARAM_GESTURE_END => {
            let g = unsafe { &*(h as *const clap_event_param_gesture) };
            Some(Event::Gesture { time, id: g.param_id, begin: hd.type_ == CLAP_EVENT_PARAM_GESTURE_BEGIN })
        }
        _ => None,
    }
}

/// The most events one `process` call carries in: a call crosses to the host process in shared
/// memory, which can't grow on the audio thread (HOST-3). A plan whose plugin node needs more is
/// refused when it is built, naming the node.
pub const MAX_CALL_EVENTS: usize = 16_384;

/// One call's events, in CLAP's own layout, so the host hands them to the plugin as they are.
/// Sized when built: [`Events::push`] never reallocates (RENDER-28); what doesn't fit is counted.
pub struct Events {
    raw: Vec<Raw>,
    dropped: u32,
}

// SAFETY: the raw events hold no pointer the list owns (a parameter's cookie is always null).
unsafe impl Send for Events {}

impl Events {
    pub fn with_capacity(n: usize) -> Events {
        Events { raw: Vec::with_capacity(n), dropped: 0 }
    }

    pub fn clear(&mut self) {
        self.raw.clear();
    }

    /// Add `e`, or count it when the list is full: false then. Never allocates.
    pub fn push(&mut self, e: Event) -> bool {
        if self.raw.len() == self.raw.capacity() {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.raw.push(raw(&e));
        true
    }

    /// Add an event a plugin output, when it is one of ours; others are counted.
    ///
    /// # Safety
    /// As [`decode`].
    pub(crate) unsafe fn push_raw(&mut self, h: *const clap_event_header) -> bool {
        match unsafe { decode(h) } {
            Some(e) => self.push(e),
            None => {
                self.dropped = self.dropped.saturating_add(1);
                false
            }
        }
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.raw.capacity()
    }

    /// Events that didn't fit, or that weren't ones the host reads, since it was built.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn get(&self, i: usize) -> Option<Event> {
        // SAFETY: every entry was written whole by `push`.
        self.raw.get(i).and_then(|r| unsafe { decode(&r.header) })
    }

    pub fn iter(&self) -> impl Iterator<Item = Event> + '_ {
        (0..self.len()).filter_map(|i| self.get(i))
    }

    /// The time of each event, in order: whether they come sorted (CLAP requires it).
    pub(crate) fn sorted(&self) -> bool {
        // SAFETY: every entry starts with its header.
        self.raw.windows(2).all(|w| unsafe { w[0].header.time <= w[1].header.time })
    }

    /// Its events as they are, for a copy into shared memory (HOST-3).
    pub(crate) fn raw(&self) -> &[Raw] {
        &self.raw
    }

    /// Add events as they are, within its room; what doesn't fit is counted, with `dropped` more.
    /// Never allocates.
    pub(crate) fn append_raw(&mut self, src: &[Raw], dropped: u32) {
        let fit = src.len().min(self.raw.capacity() - self.raw.len());
        self.raw.extend_from_slice(&src[..fit]);
        self.dropped = self.dropped.saturating_add((src.len() - fit) as u32).saturating_add(dropped);
    }

    /// The count of [`Events::dropped`], set back to 0.
    pub(crate) fn take_dropped(&mut self) -> u32 {
        std::mem::take(&mut self.dropped)
    }

    pub(crate) fn header(&self, i: usize) -> *const clap_event_header {
        self.raw.get(i).map_or(std::ptr::null(), |r| r as *const Raw as *const clap_event_header)
    }

    pub(crate) fn as_ctx(&self) -> *mut c_void {
        self as *const Events as *mut c_void
    }
}

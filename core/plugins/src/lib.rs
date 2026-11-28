//! Hosts CLAP plugins.
//!
//! The plugins producers already own become graph nodes: audio in and out, notes in for
//! instruments, and every automatable parameter as a control input. A plugin's state is saved and
//! loaded as bytes, which the project stores as data.
//!
//! The engine reaches plugins only through [`PluginHost`] (P11, HOST-1). [`InProcess`], today's
//! host, runs them in this process, which trusts their native code like any DAW (the host process
//! of HOST-3 comes later). Its threads follow CLAP's model, and its thread checks answer truthfully
//! (HOST-2):
//! - every main-thread call (create, activate, state, parameters, `on_main_thread`, deactivate,
//!   destroy) runs on the plugin main thread, which the program installs: see [`main_thread`];
//! - `process` and `reset` run on whichever thread renders (the live engine's audio thread, or an
//!   offline render's), which holds the instance's audio role while it does; they never allocate,
//!   lock or wait (`src/audio.rs`, RENDER-28);
//! - a plugin's requests from any thread (`request_callback`, `request_restart`, `latency.changed`
//!   and others) only raise flags, which the main thread sweeps every 5 ms: `on_main_thread` never
//!   runs inside `process`;
//! - libraries load on the thread that asks for them, never the main thread, once per bundle.
//!
//! Plugins are impure: the runtime never caches their output.
//!
//! All of that is the `clap` feature, on by default (HOST-1). Without it, as on the web, the crate
//! is [`PluginHost`] and the types the engine names ([`Handle`], [`Owner`], [`Event`], [`Events`],
//! [`Blob`], [`Found`], [`RenderMode`] and the rest): nothing loads or runs plugin code, and there
//! is no process host.

#[cfg(feature = "clap")]
mod audio;
#[cfg(feature = "clap")]
mod gui;
mod host;
#[cfg(feature = "clap")]
mod hosting;
#[cfg(feature = "clap")]
mod inprocess;
#[cfg(feature = "clap")]
mod main_thread;
#[cfg(feature = "clap")]
mod presets;
#[cfg(feature = "clap")]
pub mod probe;
#[cfg(feature = "clap")]
mod remote;

use std::path::PathBuf;

pub use host::{
    Blob, Events, Handle, HostEvent, HostEvents, MAX_CALL_EVENTS, MAX_PRESET_FILES, MainCall, Offered, Owner, Pending, PluginHost,
    PresetAt, RenderMode, Reply, SharedBuffers, Status, WindowCall,
};
#[cfg(feature = "clap")]
pub(crate) use hosting::{Library, cstr, fixed, library, remote_live, scan_here, use_remote};
#[cfg(feature = "clap")]
pub use hosting::{background_host, end_host_process, find, host, host_process, scan, search_paths};
#[cfg(feature = "clap")]
pub use inprocess::{InProcess, MAX_INSTANCES, sweep_now};
#[cfg(feature = "clap")]
pub use main_thread::{DRAIN, MainThread, main_thread, on_main_thread, run_main, test_main_thread, use_app_main_thread};
#[cfg(feature = "clap")]
pub use remote::child::serve as serve_host;
#[cfg(feature = "clap")]
pub use remote::{FLAG as HOST_FLAG, HANG, Remote};

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

impl Found {
    /// It plays notes: CLAP's `instrument` feature. What + Track lists (gaps #32).
    pub fn is_instrument(&self) -> bool {
        self.features.iter().any(|f| f == "instrument")
    }

    /// It goes after a sound: CLAP's `audio-effect` feature. What the effect lists list.
    pub fn is_effect(&self) -> bool {
        self.features.iter().any(|f| f == "audio-effect")
    }
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
    /// Measured when it was last activated (0 before).
    pub latency: u32,
    /// It has a window of its own this OS can show (CLAP's `gui`, embedded; HOST-7).
    pub gui: bool,
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

// ---------------------------------------------------------------- events

/// Something that happens at a sample offset within a `process` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    NoteOn {
        time: u32,
        id: i32,
        key: i16,
        velocity: f64,
    },
    NoteOff {
        time: u32,
        id: i32,
        key: i16,
    },
    /// End a note at once, with no release (a panic).
    NoteChoke {
        time: u32,
        id: i32,
        key: i16,
    },
    /// Tuning in semitones for one note (for pitches between keys).
    Tuning {
        time: u32,
        id: i32,
        key: i16,
        semitones: f64,
    },
    Param {
        time: u32,
        id: u32,
        value: f64,
    },
    /// A MIDI message, sent as `CLAP_EVENT_MIDI` on the note port: the lane's pitch wheel and mod
    /// wheel (RENDER-14's `Midi`).
    Midi {
        time: u32,
        bytes: [u8; 3],
    },
    /// The person began (or ended) changing a parameter in the plugin's window: what a plugin
    /// outputs around the values it sets (CLAP's gesture events, HOST-4).
    Gesture {
        time: u32,
        id: u32,
        begin: bool,
    },
}

impl Event {
    pub fn time(&self) -> u32 {
        match *self {
            Event::NoteOn { time, .. }
            | Event::NoteOff { time, .. }
            | Event::NoteChoke { time, .. }
            | Event::Tuning { time, .. }
            | Event::Param { time, .. }
            | Event::Midi { time, .. }
            | Event::Gesture { time, .. } => time,
        }
    }
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

#[cfg(all(test, feature = "clap"))]
mod tests;

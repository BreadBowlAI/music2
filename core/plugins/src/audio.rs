//! The host's audio side: `process`, `reset`, the thread checks, and the callbacks a plugin may
//! make from any thread, its audio thread included. Nothing here allocates, locks or waits
//! (RENDER-28), and no thread-local is touched (a thread's first one allocates on macOS);
//! `tests/no_blocking.rs` greps this file to keep it so.

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::Ordering;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::*;
use clap_sys::ext::log::*;
use clap_sys::ext::params::{clap_param_clear_flags, clap_param_rescan_flags};
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::host::clap_host;
use clap_sys::id::clap_id;
use clap_sys::process::*;

use crate::Transport;
use crate::host::{Events, SharedBuffers, Status};
use crate::inprocess::{Core, PENDING, host_data, keep_log};
use crate::main_thread::token;

/// Silence every output of `io`.
pub(crate) fn silence(io: &mut SharedBuffers) {
    let n = io.frames as usize;
    for o in io.outputs.iter_mut().take(io.n_out) {
        let len = n.min(o.len());
        o[..len].fill(0.0);
    }
}

/// One `process` call on the calling thread, which takes the instance's audio role. Starts
/// processing at the first call after an activation (CLAP: `start_processing` on the audio thread).
pub(crate) fn process(c: &Core, io: &mut SharedBuffers, ev_in: &Events, ev_out: &mut Events, t: &Transport) -> Status {
    c.host.audio_owner.store(token(), Ordering::Relaxed);
    // SAFETY: the caller holds the audio role; the main thread touches this side only while no
    // renderer holds the instance.
    let a = unsafe { &mut *c.audio.get() };
    let n = io.frames as usize;
    if !a.active || io.frames > a.max_frames {
        silence(io);
        return Status::Error;
    }
    if !a.processing {
        let started = unsafe { (*c.plugin).start_processing }.is_none_or(|f| unsafe { f(c.plugin) });
        if !started {
            silence(io);
            return Status::Error;
        }
        a.processing = true;
    }
    debug_assert!(ev_in.sorted(), "events must come in time order");
    for (ch, buf) in a.ins.iter_mut().enumerate() {
        let src = if ch < io.n_in {
            Some(io.inputs[ch])
        } else if io.n_in > 0 {
            Some(io.inputs[0])
        } else {
            None
        };
        match src {
            Some(s) => buf[..n].copy_from_slice(&s[..n]),
            None => buf[..n].fill(0.0),
        }
    }
    a.in_ptrs.clear();
    for b in a.ins.iter_mut() {
        a.in_ptrs.push(b.as_mut_ptr());
    }
    a.out_ptrs.clear();
    for b in a.outs.iter_mut() {
        a.out_ptrs.push(b.as_mut_ptr());
    }
    let audio_in = clap_audio_buffer {
        data32: a.in_ptrs.as_mut_ptr(),
        data64: null_mut(),
        channel_count: a.in_ptrs.len() as u32,
        latency: 0,
        constant_mask: 0,
    };
    let mut audio_out = clap_audio_buffer {
        data32: a.out_ptrs.as_mut_ptr(),
        data64: null_mut(),
        channel_count: a.out_ptrs.len() as u32,
        latency: 0,
        constant_mask: 0,
    };
    let transport = transport_event(t);
    let in_events = clap_input_events { ctx: ev_in.as_ctx(), size: Some(in_size), get: Some(in_get) };
    let out_events = clap_output_events { ctx: ev_out as *mut Events as *mut c_void, try_push: Some(out_push) };
    let p = clap_process {
        steady_time: t.steady,
        frames_count: io.frames,
        transport: &transport,
        audio_inputs: if a.ins.is_empty() { null() } else { &audio_in },
        audio_outputs: if a.outs.is_empty() { null_mut() } else { &mut audio_out },
        audio_inputs_count: (!a.ins.is_empty()) as u32,
        audio_outputs_count: (!a.outs.is_empty()) as u32,
        in_events: &in_events,
        out_events: &out_events,
    };
    let status = match unsafe { (*c.plugin).process } {
        Some(f) => unsafe { f(c.plugin, &p) },
        None => CLAP_PROCESS_ERROR,
    };
    for (ch, out) in io.outputs.iter_mut().enumerate().take(io.n_out) {
        let len = n.min(out.len());
        match a.outs.get(ch) {
            Some(src) => out[..len].copy_from_slice(&src[..len]),
            None => out[..len].fill(0.0),
        }
    }
    match status {
        CLAP_PROCESS_CONTINUE => Status::Continue,
        CLAP_PROCESS_CONTINUE_IF_NOT_QUIET => Status::ContinueIfNotQuiet,
        CLAP_PROCESS_TAIL => Status::Tail,
        CLAP_PROCESS_SLEEP => Status::Sleep,
        _ => Status::Error,
    }
}

/// Back to silence: every voice and tail cleared. Only while active (CLAP).
pub(crate) fn reset(c: &Core) {
    c.host.audio_owner.store(token(), Ordering::Relaxed);
    // SAFETY: as in `process`.
    let a = unsafe { &*c.audio.get() };
    if a.active
        && let Some(f) = unsafe { (*c.plugin).reset }
    {
        unsafe { f(c.plugin) };
    }
}

fn transport_event(t: &Transport) -> clap_event_transport {
    let beat_time = |b: f64| (b * CLAP_BEATTIME_FACTOR as f64) as i64;
    let sec_time = |s: f64| (s * CLAP_SECTIME_FACTOR as f64) as i64;
    let mut flags = CLAP_TRANSPORT_HAS_TEMPO
        | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
        | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE
        | CLAP_TRANSPORT_HAS_TIME_SIGNATURE;
    if t.playing {
        flags |= CLAP_TRANSPORT_IS_PLAYING;
    }
    if t.looping.is_some() {
        flags |= CLAP_TRANSPORT_IS_LOOP_ACTIVE;
    }
    let ((loop_start, loop_end), (loop_start_s, loop_end_s)) = t.looping.unwrap_or_default();
    let (tsig_num, tsig_denom) = if t.tsig.0 == 0 { (4, 4) } else { t.tsig };
    clap_event_transport {
        header: clap_event_header {
            size: std::mem::size_of::<clap_event_transport>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_TRANSPORT,
            flags: 0,
        },
        flags,
        song_pos_beats: beat_time(t.beats),
        song_pos_seconds: sec_time(t.seconds),
        tempo: t.bpm,
        tempo_inc: 0.0,
        loop_start_beats: beat_time(loop_start),
        loop_end_beats: beat_time(loop_end),
        loop_start_seconds: sec_time(loop_start_s),
        loop_end_seconds: sec_time(loop_end_s),
        bar_start: beat_time(t.bar_start),
        bar_number: t.bar,
        tsig_num,
        tsig_denom,
    }
}

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    unsafe { (*((*list).ctx as *const Events)).len() as u32 }
}

unsafe extern "C" fn in_get(list: *const clap_input_events, i: u32) -> *const clap_event_header {
    unsafe { (*((*list).ctx as *const Events)).header(i as usize) }
}

/// What a plugin outputs (parameter gestures and values, HOST-4) is kept, within the list's room.
unsafe extern "C" fn out_push(list: *const clap_output_events, e: *const clap_event_header) -> bool {
    if e.is_null() {
        return false;
    }
    unsafe { (*((*list).ctx as *mut Events)).push_raw(e) }
}

// ---------------------------------------------------------------- the plugin's calls to the host

/// CLAP's `is_main_thread`: whether the caller is the plugin main thread.
pub(crate) unsafe extern "C" fn is_main_thread(_: *const clap_host) -> bool {
    crate::main_thread::on_main_thread()
}

/// CLAP's `is_audio_thread`: whether the caller holds this instance's audio role. Asked every
/// block: an atomic load and a register read.
pub(crate) unsafe extern "C" fn is_audio_thread(h: *const clap_host) -> bool {
    unsafe { host_data(h) }.audio_owner.load(Ordering::Relaxed) == token()
}

/// A request: raise the flag, then the host's, which the sweep reads with Acquire.
fn raise(flag: &std::sync::atomic::AtomicBool) {
    flag.store(true, Ordering::Relaxed);
    PENDING.store(true, Ordering::Release);
}

/// `request_callback`, from any thread: `on_main_thread` runs at the next sweep.
pub(crate) unsafe extern "C" fn request_callback(h: *const clap_host) {
    raise(&unsafe { host_data(h) }.callback);
}

pub(crate) unsafe extern "C" fn request_restart(h: *const clap_host) {
    raise(&unsafe { host_data(h) }.restart);
}

/// `request_process` (and `params.request_flush`): a sleeping plugin wants to run. Read by the
/// engine at its next block line, with no sweep.
pub(crate) unsafe extern "C" fn request_process(h: *const clap_host) {
    unsafe { host_data(h) }.process_requested.store(true, Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn mark_dirty(h: *const clap_host) {
    raise(&unsafe { host_data(h) }.dirty);
}

pub(crate) unsafe extern "C" fn latency_changed(h: *const clap_host) {
    raise(&unsafe { host_data(h) }.latency_changed);
}

pub(crate) unsafe extern "C" fn params_rescan(h: *const clap_host, _: clap_param_rescan_flags) {
    raise(&unsafe { host_data(h) }.rescan);
}

pub(crate) unsafe extern "C" fn params_clear(_: *const clap_host, _: clap_id, _: clap_param_clear_flags) {}

/// A warning or an error. Kept off the audio role; while a renderer's thread holds it, only
/// counted (the main thread holds it only while stopping the instance, and may keep a message).
pub(crate) unsafe extern "C" fn host_log(h: *const clap_host, severity: clap_log_severity, msg: *const c_char) {
    if severity < CLAP_LOG_WARNING || msg.is_null() {
        return;
    }
    let hd = unsafe { host_data(h) };
    if hd.audio_owner.load(Ordering::Relaxed) == token() && !crate::main_thread::on_main_thread() {
        hd.logs_dropped.fetch_add(1, Ordering::Relaxed);
    } else {
        keep_log(hd, msg);
    }
}

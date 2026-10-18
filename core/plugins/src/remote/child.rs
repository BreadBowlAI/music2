//! The host process's side (HOST-3): read each request from standard input, make it with
//! [`InProcess`], answer on (the real) standard output. Every main-thread call runs on this
//! process's own main loop, in the order it came (HOST-2); a create and a scan load their libraries
//! on a thread of their own first, as `InProcess` does. Each active instance has an audio thread,
//! asleep on its shared-memory slot (`slot.rs`). At the end of its input (the app quit or died),
//! the process ends at once.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use m2_os::ipc::Shm;

use super::slot::{self, OUT_EVENTS, View};
use super::wire::{self, R, Req, W};
use crate::host::*;
use crate::{InProcess, main_thread};

/// The real standard output: every reply and event goes here, a frame at a time.
static OUT: OnceLock<Mutex<File>> = OnceLock::new();

/// Each active instance's slot and the thread serving it.
struct Audio {
    shm: Shm,
    max_frames: u32,
    thread: std::thread::JoinHandle<()>,
}

fn audio() -> &'static Mutex<HashMap<u64, Audio>> {
    static A: OnceLock<Mutex<HashMap<u64, Audio>>> = OnceLock::new();
    A.get_or_init(Default::default)
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn write(frame: Vec<u8>) {
    if let Some(out) = OUT.get() {
        let mut o = locked(out);
        if o.write_all(&frame).is_err() {
            // The app is gone.
            m2_os::process::exit_now(0);
        }
    }
}

/// Answer request `id`: its value, written by `put`, or why it failed.
fn reply(id: u64, r: Result<impl FnOnce(&mut W)>) {
    let mut w = W::frame();
    w.u8(wire::REPLY).u64(id);
    match r {
        Ok(put) => {
            w.u8(1);
            put(&mut w);
        }
        Err(e) => {
            w.u8(0).str(&format!("{e:#}"));
        }
    }
    write(w.done());
}

/// Serve as the host process, for good. `run_main` and `use_app_main_thread` call it when the
/// program was run with [`super::FLAG`]; so does `m2-plugin-host`.
pub fn serve() -> ! {
    // A crash is seen by the app when this process has ended: no core dump holds it up.
    m2_os::process::no_core_dump();
    if std::env::args().any(|a| a == super::PROBE_FLAG) {
        crate::probe::install();
    }
    // A plugin that prints must not break a message: standard output goes to standard error, and
    // the messages to what it was.
    let out = match m2_os::process::take_stdout() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("music2's plugin host: no output to answer on: {e}");
            m2_os::process::exit_now(3)
        }
    };
    let _ = OUT.set(Mutex::new(out));
    main_thread::serve_main(run);
    m2_os::process::exit_now(0)
}

/// On a worker, while the main thread runs the loop: read requests until the input ends.
fn run() {
    let (owner, events) = Owner::new();
    std::thread::Builder::new()
        .name("m2-host-events".into())
        .spawn(move || {
            for e in events {
                let mut w = W::frame();
                w.u8(wire::EVENT).event(&e);
                write(w.done());
            }
        })
        .expect("a thread for plugin events");
    std::thread::Builder::new().name("m2-host-requests".into()).spawn(requests).expect("a thread for plugin requests");
    let mut input = std::io::BufReader::new(std::io::stdin().lock());
    while let Ok(Some(frame)) = wire::read_frame(&mut input) {
        match R::new(&frame).req() {
            Ok((id, req)) => handle(id, req, &owner),
            Err(e) => eprintln!("music2's plugin host: a message it can't read: {e:#}"),
        }
    }
    m2_os::process::exit_now(0)
}

/// Every 5 ms, what each instance asked for (CLAP's `request_process`) goes to its slot, where the
/// app's engine reads it at its next block line.
fn requests() {
    loop {
        std::thread::sleep(Duration::from_millis(5));
        for (&h, a) in locked(audio()).iter() {
            if InProcess.take_process_request(Handle(h)) {
                // SAFETY: the slot is mapped while its entry is in the map.
                unsafe { View::new(a.shm.ptr(), a.max_frames) }.header().requested.store(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

/// Run `f` on the main loop, in order, and answer with what it returns.
fn on_main<P: FnOnce(&mut W) + 'static>(id: u64, f: impl FnOnce() -> Result<P> + Send + 'static) {
    match main_thread::main_thread() {
        Ok(m) => m.post(move || reply(id, f())),
        Err(e) => reply(id, Err::<fn(&mut W), _>(e)),
    }
}

fn handle(id: u64, req: Req, owner: &Owner) {
    let h = |h: u64| Handle(h);
    match req {
        Req::Hello => reply(
            id,
            Ok(|w: &mut W| {
                w.u64(wire::STAMP);
            }),
        ),
        Req::Scan => drop(std::thread::spawn(move || {
            let (found, errors) = crate::scan_here();
            reply(
                id,
                Ok(move |w: &mut W| {
                    w.u32(found.len() as u32);
                    for f in &found {
                        w.found(f);
                    }
                    w.strs(&errors);
                }),
            );
        })),
        Req::Presets { found } => drop(std::thread::spawn(move || {
            let r = InProcess.presets(&found);
            reply(
                id,
                r.map(|offered| {
                    move |w: &mut W| {
                        w.offered(&offered);
                    }
                }),
            );
        })),
        Req::Create { found, state } => {
            let owner = owner.clone();
            drop(std::thread::spawn(move || {
                let made = InProcess.create(&found, state.map(Arc::new), &owner);
                reply(
                    id,
                    made.map(|(h, info)| {
                        move |w: &mut W| {
                            w.u64(h.0).info(&info);
                        }
                    }),
                );
            }))
        }
        Req::Activate { h: hh, rate, max_frames, shm } => on_main(id, move || {
            let mapped = match shm {
                Some(name) => {
                    stop_audio(hh);
                    start_audio(hh, &name, rate, max_frames).is_ok()
                }
                None => locked(audio()).contains_key(&hh),
            };
            let r = InProcess.activate(h(hh), rate, max_frames);
            Ok(move |w: &mut W| {
                w.bool(mapped);
                match r {
                    Ok(l) => {
                        w.u8(1).u32(l);
                    }
                    Err(e) => {
                        w.u8(0).str(&format!("{e:#}"));
                    }
                }
            })
        }),
        Req::Deactivate { h: hh } => on_main(id, move || {
            InProcess.deactivate(h(hh));
            Ok(|_: &mut W| {})
        }),
        Req::LoadState { h: hh, blob } => on_main(id, move || {
            let r = InProcess.load_state(h(hh), Arc::new(blob))?;
            Ok(move |w: &mut W| {
                w.bool(r);
            })
        }),
        Req::SetRenderMode { h: hh, mode } => on_main(id, move || {
            let r = InProcess.set_render_mode(h(hh), mode)?;
            Ok(move |w: &mut W| {
                w.bool(r);
            })
        }),
        Req::MainCall { h: hh, call } => on_main(id, move || {
            let r = InProcess.main_call(h(hh), call)?;
            Ok(move |w: &mut W| {
                w.reply(&r);
            })
        }),
        Req::Destroy { h: hh } => match main_thread::main_thread() {
            Ok(m) => m.post(move || {
                stop_audio(hh);
                // Queued after this job, and the answer after it: it comes once it is destroyed.
                InProcess.destroy(h(hh));
                if let Ok(m) = main_thread::main_thread() {
                    m.post(move || reply(id, Ok(|_: &mut W| {})));
                }
            }),
            Err(e) => reply(id, Err::<fn(&mut W), _>(e)),
        },
        Req::Latency { h: hh } => {
            let rx = InProcess.latency(h(hh));
            answer_later(id, rx, |w, l| {
                w.u32(l);
            });
        }
        Req::SaveState { h: hh } => {
            let rx = InProcess.save_state(h(hh));
            answer_later(id, rx, |w, b| {
                w.blob(&b);
            });
        }
    }
}

/// Answer `id` with what `rx` gets, from a thread of its own (the main thread makes it).
fn answer_later<T: Send + 'static>(id: u64, rx: Pending<Result<T, String>>, put: fn(&mut W, T)) {
    std::thread::spawn(move || {
        let r = rx.recv().map_err(|_| anyhow!("no answer")).and_then(|r| r.map_err(|e| anyhow!(e)));
        reply(id, r.map(|t| move |w: &mut W| put(w, t)));
    });
}

/// Map `h`'s slot `name` and start its audio thread.
fn start_audio(h: u64, name: &str, rate: u32, max_frames: u32) -> Result<()> {
    let shm = Shm::open(name, slot::size(max_frames)).context("the plugin's shared memory")?;
    // SAFETY: mapped, of that size, for as long as the thread runs: the entry keeps it until the
    // thread was joined.
    let v = unsafe { View::new(shm.ptr(), max_frames) };
    let thread = std::thread::Builder::new()
        .name("m2-plugin-audio".into())
        .spawn(move || {
            // Real time, as the engine's threads ask for, at a call's period (D4); never fatal.
            let g = m2_os::rt::Group::real_time(Duration::from_secs_f64(f64::from(max_frames) / f64::from(rate.max(1))));
            m2_os::rt::engine_thread(&g);
            m2_os::fp::canon();
            let (mut ev_in, mut ev_out) = (Events::with_capacity(MAX_CALL_EVENTS), Events::with_capacity(OUT_EVENTS));
            slot::serve(
                &v,
                &mut ev_in,
                &mut ev_out,
                |io, ev_in, ev_out, t| InProcess.process(Handle(h), io, ev_in, ev_out, t),
                || InProcess.reset(Handle(h)),
            );
            m2_os::rt::end_engine_thread(&g);
        })
        .context("a thread for the plugin's audio")?;
    locked(audio()).insert(h, Audio { shm, max_frames, thread });
    Ok(())
}

/// End `h`'s audio thread, and unmap its slot.
fn stop_audio(h: u64) {
    let Some(a) = locked(audio()).remove(&h) else { return };
    // SAFETY: mapped until `a` drops, after the join.
    slot::quit(&unsafe { View::new(a.shm.ptr(), a.max_frames) });
    let _ = a.thread.join();
}

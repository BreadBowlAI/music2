//! Linux: X11 windows, through `libX11.so.6` opened at run time, so a program without a screen
//! never needs it. One connection per process, opened by the first window on the main loop's
//! thread; its fd is watched there, and the person's close (`WM_DELETE_WINDOW`, from the window
//! manager) calls the window's `closed`. Wayland is reached only through XWayland.

// An X id is a C `unsigned long`: 64 bits here, not on every target.
#![allow(clippy::unnecessary_cast)]

use std::ffi::{CString, c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::io;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::main_loop::{Loop, Ready};
use crate::window::{Native, OnClose};

pub(crate) const API: &str = "x11";

type Dpy = *mut c_void;
type Xid = c_ulong;

const CLIENT_MESSAGE: c_int = 33;
const IS_VIEWABLE: c_int = 2;

#[repr(C)]
#[derive(Clone, Copy)]
struct ClientMessage {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: Dpy,
    window: Xid,
    message_type: Xid,
    format: c_int,
    data: [c_long; 5],
}

#[repr(C)]
union XEvent {
    type_: c_int,
    client: ClientMessage,
    pad: [c_long; 24],
}

#[repr(C)]
struct Attributes {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    depth: c_int,
    visual: *mut c_void,
    root: Xid,
    class: c_int,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    colormap: Xid,
    map_installed: c_int,
    map_state: c_int,
    all_event_masks: c_long,
    your_event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    screen: *mut c_void,
}

type OnError = unsafe extern "C" fn(Dpy, *mut c_void) -> c_int;

/// The calls music2 makes, looked up once.
struct X {
    open_display: unsafe extern "C" fn(*const c_char) -> Dpy,
    default_screen: unsafe extern "C" fn(Dpy) -> c_int,
    root_window: unsafe extern "C" fn(Dpy, c_int) -> Xid,
    black_pixel: unsafe extern "C" fn(Dpy, c_int) -> c_ulong,
    create_simple_window: unsafe extern "C" fn(Dpy, Xid, c_int, c_int, c_uint, c_uint, c_uint, c_ulong, c_ulong) -> Xid,
    store_name: unsafe extern "C" fn(Dpy, Xid, *const c_char) -> c_int,
    intern_atom: unsafe extern "C" fn(Dpy, *const c_char, c_int) -> Xid,
    set_wm_protocols: unsafe extern "C" fn(Dpy, Xid, *mut Xid, c_int) -> c_int,
    map_window: unsafe extern "C" fn(Dpy, Xid) -> c_int,
    destroy_window: unsafe extern "C" fn(Dpy, Xid) -> c_int,
    resize_window: unsafe extern "C" fn(Dpy, Xid, c_uint, c_uint) -> c_int,
    flush: unsafe extern "C" fn(Dpy) -> c_int,
    pending: unsafe extern "C" fn(Dpy) -> c_int,
    next_event: unsafe extern "C" fn(Dpy, *mut XEvent) -> c_int,
    connection_number: unsafe extern "C" fn(Dpy) -> c_int,
    set_error_handler: unsafe extern "C" fn(Option<OnError>) -> Option<OnError>,
    get_window_attributes: unsafe extern "C" fn(Dpy, Xid, *mut Attributes) -> c_int,
    send_event: unsafe extern "C" fn(Dpy, Xid, c_int, c_long, *mut XEvent) -> c_int,
}

/// The last X error's code: errors are said by what the calls return, never by ending the
/// process (Xlib's own handler exits).
static LAST_ERROR: AtomicU8 = AtomicU8::new(0);

unsafe extern "C" fn on_error(_: Dpy, e: *mut c_void) -> c_int {
    // XErrorEvent: type (int), display, resource id, serial, then the error code (a byte).
    // SAFETY: Xlib passes a whole XErrorEvent.
    let code = unsafe { *(e as *const u8).add(4 * std::mem::size_of::<usize>()) };
    LAST_ERROR.store(code, Ordering::Relaxed);
    0
}

// Each symbol's type is the field it goes into.
#[allow(clippy::missing_transmute_annotations)]
fn x() -> Result<&'static X, String> {
    static LIB: OnceLock<Result<X, String>> = OnceLock::new();
    LIB.get_or_init(|| {
        // SAFETY: dlopen with a constant name; the library stays loaded for the process's life.
        let lib = unsafe { libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if lib.is_null() {
            return Err("libX11 isn't installed, so music2 can't open windows (install libx11-6)".into());
        }
        macro_rules! sym {
            ($name:literal) => {{
                // SAFETY: a symbol of the library just opened, given the type the X11 headers give it.
                let p = unsafe { libc::dlsym(lib, concat!($name, "\0").as_ptr() as *const c_char) };
                if p.is_null() {
                    return Err(format!("libX11 has no {}", $name));
                }
                // SAFETY: as above.
                unsafe { std::mem::transmute::<*mut c_void, _>(p) }
            }};
        }
        let init_threads: unsafe extern "C" fn() -> c_int = sym!("XInitThreads");
        let x = X {
            open_display: sym!("XOpenDisplay"),
            default_screen: sym!("XDefaultScreen"),
            root_window: sym!("XRootWindow"),
            black_pixel: sym!("XBlackPixel"),
            create_simple_window: sym!("XCreateSimpleWindow"),
            store_name: sym!("XStoreName"),
            intern_atom: sym!("XInternAtom"),
            set_wm_protocols: sym!("XSetWMProtocols"),
            map_window: sym!("XMapWindow"),
            destroy_window: sym!("XDestroyWindow"),
            resize_window: sym!("XResizeWindow"),
            flush: sym!("XFlush"),
            pending: sym!("XPending"),
            next_event: sym!("XNextEvent"),
            connection_number: sym!("XConnectionNumber"),
            set_error_handler: sym!("XSetErrorHandler"),
            get_window_attributes: sym!("XGetWindowAttributes"),
            send_event: sym!("XSendEvent"),
        };
        // SAFETY: before any other Xlib call of this process's: plugins use Xlib on threads of
        // their own. Then errors are counted, never fatal.
        unsafe {
            init_threads();
            (x.set_error_handler)(Some(on_error));
        }
        Ok(x)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// The connection windows open on: the main loop's thread's alone.
struct Conn {
    dpy: Dpy,
    protocols: Xid,
    delete: Xid,
    /// Each window, and what its close calls (taken while it runs).
    windows: Vec<(Xid, Option<OnClose>)>,
}

// SAFETY: used only on the main loop's thread, which opened it (Xlib is initialised for threads).
unsafe impl Send for Conn {}

static MAIN: Mutex<Option<Conn>> = Mutex::new(None);

fn locked<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn connect(x: &X) -> io::Result<(Dpy, Xid, Xid)> {
    // SAFETY: XOpenDisplay with the default ($DISPLAY); the atoms on that connection.
    unsafe {
        let dpy = (x.open_display)(std::ptr::null());
        if dpy.is_null() {
            return Err(io::Error::other(no_screen()));
        }
        let protocols = (x.intern_atom)(dpy, c"WM_PROTOCOLS".as_ptr(), 0);
        let delete = (x.intern_atom)(dpy, c"WM_DELETE_WINDOW".as_ptr(), 0);
        Ok((dpy, protocols, delete))
    }
}

fn no_screen() -> String {
    match std::env::var("DISPLAY") {
        Ok(d) if !d.is_empty() => format!("music2 can't reach the screen {d} to open a window on"),
        _ => "there's no screen to open a window on (DISPLAY isn't set)".into(),
    }
}

/// The person closed windows: read what the X connection says, and call each closed window's
/// `closed`, with no lock held.
fn drain() {
    let Ok(x) = x() else { return };
    let mut closed = vec![];
    {
        let m = locked(&MAIN);
        let Some(c) = m.as_ref() else { return };
        // SAFETY: the main connection, on its thread; each event read whole.
        unsafe {
            while (x.pending)(c.dpy) > 0 {
                let mut e: XEvent = std::mem::zeroed();
                (x.next_event)(c.dpy, &mut e);
                if e.type_ == CLIENT_MESSAGE && e.client.message_type == c.protocols && e.client.data[0] as Xid == c.delete {
                    closed.push(e.client.window);
                }
            }
        }
    }
    for w in closed {
        let f = locked(&MAIN).as_mut().and_then(|c| c.windows.iter_mut().find(|x| x.0 == w)).and_then(|x| x.1.take());
        let Some(mut f) = f else { continue };
        f();
        if let Some(slot) = locked(&MAIN).as_mut().and_then(|c| c.windows.iter_mut().find(|x| x.0 == w)) {
            slot.1 = Some(f);
        }
    }
}

pub(crate) struct Window {
    id: Xid,
}

impl Window {
    pub(crate) fn open(lp: &Loop, title: &str, width: u32, height: u32, closed: OnClose) -> io::Result<Window> {
        let x = x().map_err(io::Error::other)?;
        let mut m = locked(&MAIN);
        if m.is_none() {
            let (dpy, protocols, delete) = connect(x)?;
            // SAFETY: the connection just opened.
            let fd = unsafe { (x.connection_number)(dpy) };
            lp.watch(fd, Ready { read: true, ..Ready::default() }, Box::new(|_| drain()))?;
            *m = Some(Conn { dpy, protocols, delete, windows: vec![] });
        }
        let c = m.as_mut().expect("connected");
        let title = CString::new(title.replace('\0', " ")).unwrap_or_default();
        // SAFETY: the main connection, on its thread: a top-level window, named, closable by the
        // window manager, shown.
        let id = unsafe {
            let screen = (x.default_screen)(c.dpy);
            let black = (x.black_pixel)(c.dpy, screen);
            let id = (x.create_simple_window)(c.dpy, (x.root_window)(c.dpy, screen), 0, 0, width, height, 0, black, black);
            if id == 0 {
                return Err(io::Error::other("the screen refused a new window"));
            }
            (x.store_name)(c.dpy, id, title.as_ptr());
            let mut delete = c.delete;
            (x.set_wm_protocols)(c.dpy, id, &mut delete, 1);
            (x.map_window)(c.dpy, id);
            (x.flush)(c.dpy);
            id
        };
        c.windows.push((id, Some(closed)));
        Ok(Window { id })
    }

    pub(crate) fn native(&self) -> Native {
        Native(self.id as u64)
    }

    pub(crate) fn resize(&self, width: u32, height: u32) {
        let (Ok(x), Some(c)) = (x(), locked(&MAIN).as_ref().map(|c| c.dpy)) else { return };
        // SAFETY: a window of the main connection, on its thread.
        unsafe {
            (x.resize_window)(c, self.id, width, height);
            (x.flush)(c);
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        let Ok(x) = x() else { return };
        let mut m = locked(&MAIN);
        let Some(c) = m.as_mut() else { return };
        c.windows.retain(|w| w.0 != self.id);
        // SAFETY: a window of the main connection, destroyed once, on its thread.
        unsafe {
            (x.destroy_window)(c.dpy, self.id);
            (x.flush)(c.dpy);
        }
    }
}

pub(crate) fn available() -> Result<(), String> {
    let x = x()?;
    if locked(&MAIN).is_some() {
        return Ok(());
    }
    let (dpy, ..) = connect(x).map_err(|e| e.to_string())?;
    // A connection of its own for asking, kept: tests ask again and again.
    let _ = PROBE.set(Probe(dpy));
    Ok(())
}

/// The connection the tests' questions about this process's windows go on, apart from the main one.
struct Probe(Dpy);
// SAFETY: Xlib is initialised for threads; the questions are single requests.
unsafe impl Send for Probe {}
unsafe impl Sync for Probe {}
static PROBE: OnceLock<Probe> = OnceLock::new();

fn probe() -> Option<(&'static X, Dpy)> {
    let x = x().ok()?;
    if PROBE.get().is_none() {
        let (dpy, ..) = connect(x).ok()?;
        let _ = PROBE.set(Probe(dpy));
    }
    Some((x, PROBE.get()?.0))
}

impl Window {
    /// Asked on the probe connection: a round trip on the main one could read the person's close
    /// into Xlib's queue, where its fd's watch never sees it.
    pub(crate) fn shown(&self) -> bool {
        let Some((x, dpy)) = probe() else { return false };
        // SAFETY: a zeroed struct for Xlib to fill, about a window of this process's that exists.
        unsafe {
            let mut a: Attributes = std::mem::zeroed();
            (x.get_window_attributes)(dpy, self.id, &mut a) != 0 && a.map_state == IS_VIEWABLE
        }
    }

    /// The window manager's message, sent on the probe connection; the main one reads it.
    pub(crate) fn ask_to_close(&self) {
        let Some((x, dpy)) = probe() else { return };
        // SAFETY: the window manager's message, as ICCCM spells it, sent to the window itself.
        unsafe {
            let mut e: XEvent = std::mem::zeroed();
            e.client = ClientMessage {
                type_: CLIENT_MESSAGE,
                serial: 0,
                send_event: 1,
                display: dpy,
                window: self.id,
                message_type: (x.intern_atom)(dpy, c"WM_PROTOCOLS".as_ptr(), 0),
                format: 32,
                data: [(x.intern_atom)(dpy, c"WM_DELETE_WINDOW".as_ptr(), 0) as c_long, 0, 0, 0, 0],
            };
            (x.send_event)(dpy, self.id, 0, 0, &mut e);
            (x.flush)(dpy);
        }
    }
}

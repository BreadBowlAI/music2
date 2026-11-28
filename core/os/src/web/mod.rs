//! The web: `wasm32-unknown-unknown` in a browser, the web engine's platform (PLAT-1; the web
//! engine note, `docs/designs/live/web-engine.md`, decision 3). Only the engine runs here, in a
//! dedicated Worker, so this folder holds what the engine's crates call, and nothing of the
//! desktop's (`lib.rs` leaves those modules out).
//!
//! - **No files.** Every file call returns `Unsupported` (PLAT-3): the page fetches what the engine
//!   needs and gives it the bytes.
//! - **No folders.** There is no home folder, so no songs, presets, packs or cache folder (every
//!   cache is in memory, as PLAT-7 allows), and no CLAP folder: plugins never play here.
//! - **One thread** (decision 5), at the page's priority: no real time, no workgroup, no thread
//!   std didn't start, no process to ask about.
//! - **The float mode is WebAssembly's own**, which no code can set: `fp.rs` measures it.
//!
//! Its tests run on this machine (`web_on_this_machine` in `lib.rs`): a test can't run on
//! `wasm32-unknown-unknown`.

pub(crate) mod fp;
pub(crate) mod rt;
pub(crate) mod wake;

use std::ffi::OsString;
use std::fs::{File, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fs::{FileId, Rename};
use crate::process::{Cores, Memory};

/// What every file call says here.
const NO_FILES: &str = "the web has no files: the page gives the engine their bytes";
/// What a call about the one thread's scheduling says.
const NO_SCHEDULING: &str = "the web sets no thread's priority or CPU: the engine has the one thread of a Worker";

// The folders.

/// None: the web has no folders.
pub(crate) fn home() -> Option<PathBuf> {
    None
}

/// A path only, as Linux names it: nothing asks for it here, since [`home`] is none.
pub(crate) fn cache(home: &Path) -> PathBuf {
    home.join(".cache/music2")
}

/// None: plugins never play on the web.
pub(crate) fn clap(_home: Option<&Path>) -> Vec<PathBuf> {
    Vec::new()
}

// The disk.

pub(crate) fn push(_f: &File) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn barrier(_f: &File) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn read_at(_f: &File, _buf: &mut [u8], _offset: u64) -> io::Result<usize> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn rename(_from: &Path, _to: &Path, _how: Rename) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn clone_file(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn file_id(_p: &Path) -> io::Result<FileId> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn file_id_of(_f: &File) -> io::Result<FileId> {
    Err(crate::unsupported(NO_FILES))
}

pub(crate) fn make_writable(_p: &Path) -> io::Result<()> {
    Err(crate::unsupported(NO_FILES))
}

/// No metadata exists here (std's `fs` gives none on this target): nothing to read.
pub(crate) fn change_and_inode(_m: &Metadata) -> (i128, u64) {
    (0, 0)
}

/// No error here has an OS's number.
pub(crate) fn rename_unsupported(_code: i32) -> bool {
    false
}

pub(crate) fn lock_unsupported(_code: i32) -> bool {
    false
}

/// True, as where the OS can't tell.
pub(crate) fn is_local(_dir: &Path) -> bool {
    true
}

pub(crate) fn stored_name(_p: &Path) -> Option<OsString> {
    None
}

// This machine and this process: a page says nothing of them.

pub(crate) fn machine_id() -> Option<String> {
    None
}

pub(crate) fn host_name() -> Option<String> {
    None
}

/// None: the module has no time zone.
pub(crate) fn local_time(_unix_secs: u64) -> Option<(u8, u8)> {
    None
}

/// False: the page asks the browser for the microphone, not the engine.
pub(crate) fn microphone_refused() -> bool {
    false
}

pub(crate) fn memory() -> Option<Memory> {
    None
}

pub(crate) fn machine() -> (String, String, String) {
    ("unknown model".into(), "the web".into(), "unknown CPU".into())
}

/// One core: the engine has one thread here (decision 5).
pub(crate) fn cores() -> Cores {
    Cores { fast: 1, slow: 0, per_cache: 1, per_last_cache: 1 }
}

pub(crate) fn load_average() -> Option<[f64; 3]> {
    None
}

pub(crate) fn waited() -> Option<Duration> {
    None
}

pub(crate) fn take_stdout() -> io::Result<File> {
    Err(crate::unsupported("the web has no standard output to take"))
}

pub(crate) fn background_process() -> io::Result<()> {
    Err(crate::unsupported("the web has no process to put in the background"))
}

pub(crate) fn in_background(_pid: u32) -> Option<bool> {
    None
}

/// False: the web has no processes (and std's process id traps here).
pub(crate) fn alive(_pid: u32) -> bool {
    false
}

/// 0, the one module's: std's traps here.
pub(crate) fn pid() -> u32 {
    0
}

/// There is no process to end: the module traps at once (`unreachable`), as an abort does.
pub(crate) fn exit_now(_code: i32) -> ! {
    std::process::abort()
}

pub(crate) fn no_core_dump() {}

// The one thread.

/// The one thread's: there is no other.
#[inline]
pub(crate) fn token() -> usize {
    1
}

/// False: a Worker's thread is no process's main thread, and nothing here needs one.
#[inline]
pub(crate) fn is_process_main() -> bool {
    false
}

/// Zero: the module can't read a thread's CPU time.
pub(crate) fn cpu_time() -> Duration {
    Duration::ZERO
}

pub(crate) fn one_cpu() -> io::Result<()> {
    Err(crate::unsupported(NO_SCHEDULING))
}

pub(crate) fn lowest() -> io::Result<()> {
    Err(crate::unsupported(NO_SCHEDULING))
}

pub(crate) fn interactive() -> io::Result<()> {
    Err(crate::unsupported(NO_SCHEDULING))
}

pub(crate) fn background() -> io::Result<()> {
    Err(crate::unsupported(NO_SCHEDULING))
}

pub(crate) fn foreign(_f: &mut (dyn FnMut() + Send)) -> io::Result<()> {
    Err(crate::unsupported("the web has one thread: none that std didn't start"))
}

/// Nothing to do: the module has no C locale, so numbers are read as C reads them.
pub(crate) fn c_numbers() {}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::Path;

    use super::*;

    fn says_web(r: io::Result<impl std::fmt::Debug>, what: &str) {
        let e = r.expect_err(what);
        assert_eq!(e.kind(), ErrorKind::Unsupported, "{what}: {e}");
        assert!(e.to_string().starts_with("the web "), "{what} says why in words: {e}");
    }

    // PLAT-3: every file call says the web has none, in words; the questions with no answer get
    // none, and nothing panics.
    #[test]
    fn the_web_has_no_files_and_says_so() {
        // A real file of this machine's, since a test can't make one on the web.
        let f = File::open(std::env::current_exe().unwrap()).unwrap();
        let p = Path::new("song.ts");
        says_web(push(&f), "push");
        says_web(barrier(&f), "barrier");
        says_web(sync_dir(p), "sync_dir");
        says_web(read_at(&f, &mut [0; 4], 0), "read_at");
        says_web(rename(p, p, Rename::Swap), "rename");
        says_web(clone_file(p, p), "clone_file");
        says_web(file_id(p), "file_id");
        says_web(file_id_of(&f), "file_id_of");
        says_web(make_writable(p), "make_writable");
        assert!(is_local(p) && stored_name(p).is_none());
        assert!(!rename_unsupported(95) && !lock_unsupported(95), "no OS numbers its errors here");
    }

    // PLAT-7: no folders, so nothing to write outside a project, and no plugins to find.
    #[test]
    fn the_web_has_no_folders_and_no_plugins() {
        assert_eq!(home(), None);
        assert!(clap(Some(Path::new("/h"))).is_empty() && clap(None).is_empty());
        assert!(cache(Path::new("/h")).starts_with("/h"), "a name under the home it is given, were there one");
    }

    // The web engine note's decision 5: one thread, one core, at the page's priority; and nothing
    // to say of the machine or the process.
    #[test]
    fn the_web_has_one_thread_and_one_core() {
        assert_eq!(cores(), Cores { fast: 1, slow: 0, per_cache: 1, per_last_cache: 1 });
        assert_eq!(token(), token());
        assert!(!is_process_main());
        assert_eq!(cpu_time(), Duration::ZERO);
        says_web(foreign(&mut || {}), "foreign");
        for (what, r) in [("one_cpu", one_cpu()), ("lowest", lowest()), ("interactive", interactive()), ("background", background())] {
            says_web(r, what);
        }
        let w = rt::output_workgroup(None).unwrap();
        assert_eq!(w.recommends(), None);
        assert_eq!(rt::engine_thread(Some(Duration::from_micros(2667)), &w), crate::rt::Joined::default(), "no real time");
        assert_eq!(rt::on_device_thread(), crate::rt::Joined::default());
        says_web(take_stdout(), "take_stdout");
        says_web(background_process(), "background_process");
        assert_eq!((memory(), load_average(), waited(), in_background(1)), (None, None, None, None));
        assert!(!alive(std::process::id()), "no processes");
        assert_eq!((machine_id(), host_name(), local_time(0)), (None, None, None));
        assert!(!microphone_refused());
        let (model, system, cpu) = machine();
        assert!(![&model, &system, &cpu].iter().any(|p| p.is_empty() || p.contains(", ")), "{model}, {system}, {cpu}");
    }
}

//! Writing a commit to disk (WRITE-15): the journal, staging in `.music2/tx/<jid>/`, one
//! barrier, then checked renames into place, and the previous commit's tidy.
//!
//! The order, so a crash at any point leaves either nothing changed or a journal that says
//! exactly what to finish or undo (the step 6 design's 6a.2):
//!
//! 1. every target is compared with its base, before anything is written;
//! 2. the journal is written under a temporary name, synced, and renamed into place;
//! 3. every new content is staged and synced, and every binary target's before-image is cloned
//!    into `blobs/` (a copy that shares no inode, so a writer that rewrites the target in place
//!    can't change it) and synced;
//! 4. the staging folder, `blobs/` and `.music2/tx/` are synced, for their new entries;
//! 5. one barrier (`F_FULLFSYNC` on macOS) makes all of that durable before the project changes;
//! 6. the previous commit's journal and staging go: its renames were drained by this barrier;
//! 7. each target is renamed into place, checked as it goes: a replaced file is swapped
//!    (`RENAME_SWAP`) and what it held compared with its base, a created one is renamed only
//!    where nothing is (`RENAME_EXCL`), a removed one is moved aside and compared;
//! 8. each target's folder is synced, so the renames reach the disk by the next barrier.
//!
//! A commit's journal stays until a later barrier covers its renames: the next commit's step 6,
//! or the session's close. A failure before the first rename removes what was written and
//! changes nothing; one after rolls back what was renamed, in this process, which still holds
//! the lock (6a.6).
//!
//! Every disk call goes through [`Disk`], so a test can count them, fail any of them, and kill
//! the process at any of them.

use std::collections::BTreeSet;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use m2_song::{DataPath, Sha};

use super::journal::{self, JTarget, Journal};
use crate::watch::Stamp;

/// How a rename treats its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// Exchange the two: both must exist.
    Swap,
    /// Only where nothing is.
    Excl,
    /// Replace whatever is there.
    Plain,
}

/// What a path is on disk, links not followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Link,
}

/// Every disk call a commit makes. One implementation outside tests, [`Real`]; tests record the
/// calls, fail any of them and abort at any of them.
pub trait Disk: Send {
    fn read(&mut self, p: &Path) -> io::Result<Vec<u8>>;
    /// None when nothing is there.
    fn kind(&mut self, p: &Path) -> io::Result<Option<Kind>>;
    /// A new file with `bytes`, synced. Never opens a name that is there already.
    fn create(&mut self, p: &Path, bytes: &[u8]) -> io::Result<()>;
    fn mkdirs(&mut self, p: &Path) -> io::Result<()>;
    fn sync_dir(&mut self, p: &Path) -> io::Result<()>;
    /// Make everything written so far durable: `F_FULLFSYNC` on `file` on macOS, else its sync.
    fn barrier(&mut self, file: &Path) -> io::Result<()>;
    /// `Unsupported` when the volume can't rename that way.
    fn rename(&mut self, from: &Path, to: &Path, how: How) -> io::Result<()>;
    fn remove_file(&mut self, p: &Path) -> io::Result<()>;
    /// An empty folder.
    fn remove_dir(&mut self, p: &Path) -> io::Result<()>;
    fn remove_tree(&mut self, p: &Path) -> io::Result<()>;
    /// A copy of `from` at `to` that shares no inode with it (a clone where the volume can),
    /// synced. `to` must not exist.
    fn clone_file(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// The names in a folder.
    fn list(&mut self, dir: &Path) -> io::Result<Vec<String>>;
}

/// The disk as it is.
#[derive(Default)]
pub struct Real;

/// Push a file's bytes, or a folder's entries, to the drive without flushing the drive's cache:
/// `fsync(2)`. On macOS that alone isn't durable; the commit's one barrier (`F_FULLFSYNC`) makes
/// everything pushed before it durable, so a commit flushes the drive once (the step 6 design's
/// 6a.2, R1). `File::sync_all` is `F_FULLFSYNC` there: a drive-cache flush each time.
fn push(f: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: an fsync on a descriptor the caller owns.
        if unsafe { libc::fsync(f.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }
    #[cfg(not(unix))]
    f.sync_all()
}

#[cfg(unix)]
fn cpath(p: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::new(ErrorKind::InvalidInput, "a NUL in a path"))
}

impl Disk for Real {
    fn read(&mut self, p: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(p)
    }

    fn kind(&mut self, p: &Path) -> io::Result<Option<Kind>> {
        match std::fs::symlink_metadata(p) {
            Ok(m) if m.file_type().is_symlink() => Ok(Some(Kind::Link)),
            Ok(m) if m.is_dir() => Ok(Some(Kind::Dir)),
            Ok(_) => Ok(Some(Kind::File)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn create(&mut self, p: &Path, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(p)?;
        f.write_all(bytes)?;
        push(&f)
    }

    fn mkdirs(&mut self, p: &Path) -> io::Result<()> {
        std::fs::create_dir_all(p)
    }

    fn sync_dir(&mut self, p: &Path) -> io::Result<()> {
        push(&std::fs::File::open(p)?)
    }

    fn barrier(&mut self, file: &Path) -> io::Result<()> {
        let f = std::fs::File::open(file)?;
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd;
            // SAFETY: an fcntl on a descriptor this function owns.
            if unsafe { libc::fcntl(f.as_raw_fd(), libc::F_FULLFSYNC) } == 0 {
                return Ok(());
            }
        }
        // A volume without F_FULLFSYNC (some network and FAT volumes): its own sync.
        f.sync_all()
    }

    fn rename(&mut self, from: &Path, to: &Path, how: How) -> io::Result<()> {
        if how == How::Plain {
            return std::fs::rename(from, to);
        }
        #[cfg(target_os = "macos")]
        {
            let flags = if how == How::Swap { libc::RENAME_SWAP } else { libc::RENAME_EXCL };
            let (a, b) = (cpath(from)?, cpath(to)?);
            // SAFETY: two NUL-terminated paths that outlive the call.
            if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), flags) } == 0 {
                return Ok(());
            }
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(c) if c == libc::ENOTSUP || c == libc::EINVAL => Err(io::Error::new(ErrorKind::Unsupported, e)),
                _ => Err(e),
            };
        }
        #[allow(unreachable_code)]
        Err(io::Error::new(ErrorKind::Unsupported, "this volume can't swap or rename exclusively"))
    }

    fn remove_file(&mut self, p: &Path) -> io::Result<()> {
        std::fs::remove_file(p)
    }

    fn remove_dir(&mut self, p: &Path) -> io::Result<()> {
        std::fs::remove_dir(p)
    }

    fn remove_tree(&mut self, p: &Path) -> io::Result<()> {
        match std::fs::remove_dir_all(p) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    fn clone_file(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        {
            let (a, b) = (cpath(from)?, cpath(to)?);
            // SAFETY: two NUL-terminated paths that outlive the call.
            if unsafe { libc::clonefile(a.as_ptr(), b.as_ptr(), 0) } == 0 {
                return push(&std::fs::File::open(to)?);
            }
            let e = io::Error::last_os_error();
            if e.kind() == ErrorKind::AlreadyExists {
                return Err(e);
            }
        }
        // No clones here (another file system): a copy.
        let bytes = std::fs::read(from)?;
        self.create(to, &bytes)
    }

    fn list(&mut self, dir: &Path) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = std::fs::read_dir(dir)?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        Ok(names)
    }
}

/// `.music2/tx/` of the project at `dir`.
pub fn tx_dir(dir: &Path) -> PathBuf {
    dir.join(m2_song::path::LOCAL).join("tx")
}

/// A file's content before a commit replaces or removes it.
#[derive(Clone, Debug)]
pub struct Before {
    pub sha: Sha,
    pub bytes: Arc<[u8]>,
}

/// One change a commit makes, in op order.
#[derive(Clone, Debug)]
pub enum Target {
    /// Write `bytes` at `path`: over a file whose content is `before`, or creating it (`None`).
    Write { path: String, before: Option<Before>, bytes: Arc<[u8]>, after: Sha },
    /// Remove the file at `path`, whose content is `before`.
    Remove { path: String, before: Before },
    /// Rename a file, or a folder `data/<module>/` with everything in it, whose hash (a folder's
    /// directory hash, WRITE-12) is `hash`.
    Rename { from: String, to: String, hash: Sha, dir: bool },
}

impl Target {
    /// The paths it changes.
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Target::Write { path, .. } | Target::Remove { path, .. } => vec![path],
            Target::Rename { from, to, .. } => vec![from, to],
        }
    }
}

/// A file a commit found other than its base, with what it is now.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub file: String,
    pub current: Option<Sha>,
}

/// Why a commit didn't happen.
#[derive(Debug)]
pub enum Failed {
    /// A target isn't what its base says: nothing changed, or it was all rolled back.
    Conflict(Vec<Changed>),
    /// The disk failed: nothing changed, or it was all rolled back.
    Io(String),
    /// A rollback failed too: the journal stays, and says what is half done.
    Interrupted { journal: String, why: String },
}

/// What a commit wrote: its journal, which stays until the next barrier, and each target path
/// with the stamp of the session's own file there after its rename (None: gone, or not known to
/// be ours), for the watcher (WRITE-19, R12).
pub struct Written {
    pub journal: String,
    pub stamps: Vec<(String, Option<Stamp>)>,
}

/// The stamp of the file renamed from `staged` to `target`, when it is still the one renamed: the
/// same inode, size and modification time as before the rename (which changes only its ctime).
/// A writer that replaced it since is another inode; one that wrote into it changed its mtime.
fn own_stamp(before: Option<Stamp>, target: &Path) -> Option<Stamp> {
    let (b, a) = (before?, Stamp::at(target)?);
    (b.inode == a.inode && b.size == a.size && b.mtime == a.mtime).then_some(a)
}

/// Who commits, for the journal.
pub struct Writer<'a> {
    pub host: &'a str,
    pub host_id: &'a str,
    pub program: &'a str,
}

/// A file's hash, or None when nothing is there.
fn file_sha(disk: &mut dyn Disk, p: &Path) -> io::Result<Option<Sha>> {
    match disk.kind(p)? {
        None => Ok(None),
        Some(Kind::File) => Ok(Some(Sha::of(&disk.read(p)?))),
        Some(Kind::Dir) => Err(io::Error::other(format!("{} is a folder", p.display()))),
        Some(Kind::Link) => Err(io::Error::other(format!("{} is a link", p.display()))),
    }
}

/// The directory hash of folder `rel` of the project at `dir` (WRITE-12), or None when it isn't
/// there. Dot files aren't hashed and move with it; any other file that isn't a lane or a
/// plugin state is an error naming it.
pub fn folder_sha(disk: &mut dyn Disk, dir: &Path, rel: &str) -> io::Result<Option<Sha>> {
    if disk.kind(&dir.join(rel))? != Some(Kind::Dir) {
        return Ok(None);
    }
    let mut files = vec![];
    let mut stack = vec![rel.to_string()];
    while let Some(d) = stack.pop() {
        for name in disk.list(&dir.join(&d))? {
            let p = format!("{d}/{name}");
            match disk.kind(&dir.join(&p))? {
                Some(Kind::Dir) => stack.push(p),
                _ if name.starts_with('.') => {}
                Some(Kind::File) if matches!(DataPath::parse(Path::new(&p)), DataPath::Lane { .. } | DataPath::PluginState { .. }) => {
                    let sha = Sha::of(&disk.read(&dir.join(&p))?);
                    files.push((p[rel.len() + 1..].to_string(), sha));
                }
                _ => {
                    let why = "so the folder isn't moved: move that file out of it, or delete it";
                    return Err(io::Error::other(format!("{p} isn't a lane or a plugin state, {why}")));
                }
            }
        }
    }
    Ok(Some(m2_song::dir_hash(&files)))
}

/// The hash at `path` of a target's kind: a file's, or a folder's directory hash.
fn sha_at(disk: &mut dyn Disk, dir: &Path, path: &str, folder: bool) -> io::Result<Option<Sha>> {
    if folder { folder_sha(disk, dir, path) } else { file_sha(disk, &dir.join(path)) }
}

/// What a commit did so far, to undo it newest first.
enum Done {
    /// `staged` now holds what `target` held.
    Swapped {
        staged: PathBuf,
        target: PathBuf,
    },
    /// Without `RENAME_SWAP`: `target` was moved to `aside`, then `staged` to `target`.
    Replaced {
        staged: PathBuf,
        target: PathBuf,
        aside: PathBuf,
    },
    Created {
        target: PathBuf,
        after: Sha,
    },
    /// Moved aside into the staging folder.
    Removed {
        target: PathBuf,
        aside: PathBuf,
    },
    Renamed {
        from: PathBuf,
        to: PathBuf,
    },
}

/// A journal's name: `<unix ms>-<pid>-<n>`, so names sort oldest first and never repeat.
pub fn journal_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    format!("{ms}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// Remove the folders `path` leaves empty, up to `data/` (or the project's top).
fn prune(disk: &mut dyn Disk, dir: &Path, path: &str) {
    let mut at = Path::new(path).parent();
    while let Some(p) = at.filter(|p| p.components().count() > 1) {
        if disk.remove_dir(&dir.join(p)).is_err() {
            break;
        }
        at = p.parent();
    }
}

/// Every target compared with its base, before anything is written (step 1): each that differs.
fn precheck(disk: &mut dyn Disk, dir: &Path, targets: &[Target]) -> Result<Vec<Changed>, Failed> {
    let io = |e: io::Error| Failed::Io(e.to_string());
    let mut changed = vec![];
    for t in targets {
        match t {
            Target::Write { path, before, .. } => {
                let now = file_sha(disk, &dir.join(path)).map_err(io)?;
                if now != before.as_ref().map(|b| b.sha) {
                    changed.push(Changed { file: path.clone(), current: now });
                }
            }
            Target::Remove { path, before } => {
                let now = file_sha(disk, &dir.join(path)).map_err(io)?;
                if now != Some(before.sha) {
                    changed.push(Changed { file: path.clone(), current: now });
                }
            }
            Target::Rename { from, to, hash, dir: folder } => {
                let now = sha_at(disk, dir, from, *folder).map_err(io)?;
                if now != Some(*hash) {
                    changed.push(Changed { file: from.clone(), current: now });
                }
                if disk.kind(&dir.join(to)).map_err(io)?.is_some() {
                    let now = sha_at(disk, dir, to, *folder).ok().flatten();
                    changed.push(Changed { file: to.clone(), current: now });
                }
            }
        }
    }
    Ok(changed)
}

/// Commit `targets` in the project at `dir` (steps 1 to 8 of the module). `previous` holds the
/// journals awaiting their tidy, removed at step 6; `finished`, the journals of interrupted commits
/// this one recovers, removed once its renames are done; `keep` the before-images undo still
/// needs, which the tidy leaves in `blobs/`.
#[allow(clippy::too_many_arguments)]
pub fn commit(
    disk: &mut dyn Disk,
    dir: &Path,
    tx: &str,
    label: &(String, String),
    targets: &[Target],
    writer: &Writer,
    previous: &mut Vec<String>,
    finished: &[String],
    keep: &BTreeSet<Sha>,
) -> Result<Written, Failed> {
    let changed = precheck(disk, dir, targets)?;
    if !changed.is_empty() {
        return Err(Failed::Conflict(changed));
    }
    let txd = tx_dir(dir);
    let jid = journal_id();
    let staging = txd.join(&jid);
    let blobs = txd.join("blobs");
    let jfile = txd.join(format!("{jid}.json"));
    let part = txd.join(format!(".{jid}.json.part"));

    // Steps 2 to 5: nothing in the project changes; a failure removes what was written.
    let mut made_blobs: Vec<PathBuf> = vec![];
    let prepared = (|| -> Result<(), Failed> {
        let io = |e: io::Error| Failed::Io(e.to_string());
        disk.mkdirs(&txd).map_err(io)?;
        let mut jtargets = vec![];
        let mut binaries = vec![];
        for (k, t) in targets.iter().enumerate() {
            jtargets.push(match t {
                Target::Write { path, before, after, .. } => {
                    let (text, blob) = journal::before_image(path, before.as_ref());
                    if blob {
                        binaries.push((path.clone(), before.clone().expect("a blob is a before")));
                    }
                    JTarget::File {
                        path: path.clone(),
                        before: before.as_ref().map(|b| b.sha),
                        after: Some(*after),
                        text,
                        blob,
                        staged: Some(k),
                    }
                }
                Target::Remove { path, before } => {
                    let (text, blob) = journal::before_image(path, Some(before));
                    if blob {
                        binaries.push((path.clone(), before.clone()));
                    }
                    JTarget::File { path: path.clone(), before: Some(before.sha), after: None, text, blob, staged: None }
                }
                Target::Rename { from, to, hash, dir: folder } => {
                    JTarget::Rename { from: from.clone(), to: to.clone(), hash: *hash, dir: *folder }
                }
            });
        }
        let journal = Journal {
            id: jid.clone(),
            tx: tx.to_string(),
            host: writer.host.to_string(),
            host_id: writer.host_id.to_string(),
            pid: std::process::id(),
            program: writer.program.to_string(),
            label: label.clone(),
            targets: jtargets,
        };
        disk.create(&part, journal.to_json().as_bytes()).map_err(io)?;
        disk.rename(&part, &jfile, How::Plain).map_err(io)?;
        disk.mkdirs(&staging).map_err(io)?;
        for (k, t) in targets.iter().enumerate() {
            if let Target::Write { bytes, .. } = t {
                disk.create(&staging.join(k.to_string()), bytes).map_err(io)?;
            }
        }
        if !binaries.is_empty() {
            disk.mkdirs(&blobs).map_err(io)?;
        }
        for (path, before) in &binaries {
            let blob = blobs.join(before.sha.hex());
            // An existing blob is relied on only once its bytes are checked (R10).
            if disk.kind(&blob).map_err(io)?.is_some() {
                if disk.read(&blob).map(|b| Sha::of(&b)).ok() == Some(before.sha) {
                    continue;
                }
                disk.remove_file(&blob).map_err(io)?;
            }
            let part = blobs.join(format!(".{}.{jid}.part", before.sha.hex()));
            disk.clone_file(&dir.join(path), &part).map_err(io)?;
            made_blobs.push(part.clone());
            // The clone is what was compared: a target changed since the pre-check is a conflict.
            let cloned = Sha::of(&disk.read(&part).map_err(io)?);
            if cloned != before.sha {
                return Err(Failed::Conflict(vec![Changed { file: path.clone(), current: Some(cloned) }]));
            }
            disk.rename(&part, &blob, How::Plain).map_err(io)?;
            made_blobs.pop();
            made_blobs.push(blob);
        }
        disk.sync_dir(&staging).map_err(io)?;
        if !binaries.is_empty() {
            disk.sync_dir(&blobs).map_err(io)?;
        }
        disk.sync_dir(&txd).map_err(io)?;
        disk.barrier(&jfile).map_err(io)?;
        Ok(())
    })();
    if let Err(e) = prepared {
        // Nothing in the project changed: what this commit wrote goes, and with it the need to
        // recover. If even that fails, the journal stays with every target at its before hash,
        // and the next lock reports it.
        let _ = disk.remove_file(&part);
        let _ = disk.remove_tree(&staging);
        let _ = disk.remove_file(&jfile);
        for b in made_blobs {
            let _ = disk.remove_file(&b);
        }
        return Err(e);
    }

    // Step 6: the previous commits' journals, whose renames this barrier drained.
    for p in previous.drain(..) {
        tidy(disk, &txd, &p);
    }
    gc_blobs(disk, &blobs, keep, targets);

    // Step 7: checked renames, in op order.
    let mut done: Vec<Done> = vec![];
    let mut stamps = vec![];
    let renamed = rename_all(disk, dir, &staging, targets, &mut done, &mut stamps);
    match renamed {
        Ok(()) => {
            // Step 8: push the renames, for the next barrier to make durable.
            push_folders(disk, dir, targets);
            // A recovered commit's journal goes once what recovers it is in place.
            for f in finished {
                tidy(disk, &txd, f);
            }
            previous.push(jid.clone());
            Ok(Written { journal: jid, stamps })
        }
        Err(e) => match rollback(disk, &mut done) {
            Ok(()) => {
                // Every target is at its before hash again: the folders the rollback renamed
                // into are pushed as step 8 pushes them, and made durable, before the journal
                // can go (6a.6).
                push_folders(disk, dir, targets);
                let _ = disk.barrier(&jfile);
                tidy(disk, &txd, &jid);
                Err(e)
            }
            Err(why) => Err(Failed::Interrupted { journal: jid, why }),
        },
    }
}

/// Push the folders `targets` renamed into or out of, for the next barrier to make durable: each
/// target's folder, and for a target that makes or removes a file or moves a folder, each folder
/// above it up to the project, where a folder made for it, pruned after it, or made again by a
/// rollback changed an entry.
fn push_folders(disk: &mut dyn Disk, dir: &Path, targets: &[Target]) {
    let mut parents: BTreeSet<PathBuf> = BTreeSet::new();
    for t in targets {
        let deep = !matches!(t, Target::Write { before: Some(_), .. });
        for p in t.paths() {
            let mut at = dir.join(p);
            while let Some(parent) = at.parent().filter(|x| x.starts_with(dir)) {
                parents.insert(parent.to_path_buf());
                if !deep {
                    break;
                }
                at = parent.to_path_buf();
            }
        }
    }
    for p in parents {
        if disk.kind(&p).ok().flatten() == Some(Kind::Dir) {
            let _ = disk.sync_dir(&p);
        }
    }
}

/// Remove journal `id` and its staging folder, the staging first, so a crash between the two
/// never leaves a staging folder that no journal names (6a.3).
fn tidy(disk: &mut dyn Disk, txd: &Path, id: &str) {
    let _ = disk.remove_tree(&txd.join(id));
    let _ = disk.remove_file(&txd.join(format!("{id}.json")));
}

/// Remove the before-images no journal and no undo step needs any more.
fn gc_blobs(disk: &mut dyn Disk, blobs: &Path, keep: &BTreeSet<Sha>, targets: &[Target]) {
    let Ok(names) = disk.list(blobs) else { return };
    let ours: BTreeSet<Sha> = targets
        .iter()
        .filter_map(|t| match t {
            Target::Write { before: Some(b), .. } | Target::Remove { before: b, .. } => Some(b.sha),
            _ => None,
        })
        .collect();
    for name in names {
        match Sha::parse(&name) {
            Some(sha) if keep.contains(&sha) || ours.contains(&sha) => {}
            Some(_) => {
                let _ = disk.remove_file(&blobs.join(&name));
            }
            // A clone a killed commit never renamed into place: every clone of this commit
            // already was.
            None if name.starts_with('.') && name.ends_with(".part") => {
                let _ = disk.remove_file(&blobs.join(&name));
            }
            None => {}
        }
    }
}

/// Step 7 for every target, recording what was done.
fn rename_all(
    disk: &mut dyn Disk,
    dir: &Path,
    staging: &Path,
    targets: &[Target],
    done: &mut Vec<Done>,
    stamps: &mut Vec<(String, Option<Stamp>)>,
) -> Result<(), Failed> {
    let io = |e: io::Error| Failed::Io(e.to_string());
    let conflict = |file: &str, current: Option<Sha>| Failed::Conflict(vec![Changed { file: file.to_string(), current }]);
    for (k, t) in targets.iter().enumerate() {
        match t {
            Target::Write { path, before: Some(before), .. } => {
                let (staged, target) = (staging.join(k.to_string()), dir.join(path));
                // Not a disk call of the commit's: only what the watcher is told (R12).
                let ours = Stamp::at(&staged);
                match disk.rename(&staged, &target, How::Swap) {
                    Ok(()) => {
                        done.push(Done::Swapped { staged: staged.clone(), target: target.clone() });
                        // What the target held, compared with its base: an outside writer that
                        // replaced it since the pre-check is caught here (R9).
                        let was = Sha::of(&disk.read(&staged).map_err(io)?);
                        if was != before.sha {
                            return Err(conflict(path, Some(was)));
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::Unsupported => {
                        let aside = staging.join(format!("old-{k}"));
                        disk.rename(&target, &aside, How::Plain).map_err(io)?;
                        done.push(Done::Removed { target: target.clone(), aside: aside.clone() });
                        let was = Sha::of(&disk.read(&aside).map_err(io)?);
                        if was != before.sha {
                            return Err(conflict(path, Some(was)));
                        }
                        disk.rename(&staged, &target, How::Plain).map_err(io)?;
                        done.pop();
                        done.push(Done::Replaced { staged, target: target.clone(), aside });
                    }
                    Err(e) => return Err(io(e)),
                }
                stamps.push((path.clone(), own_stamp(ours, &target)));
            }
            Target::Write { path, before: None, after, .. } => {
                let (staged, target) = (staging.join(k.to_string()), dir.join(path));
                if let Some(parent) = target.parent() {
                    disk.mkdirs(parent).map_err(io)?;
                }
                let ours = Stamp::at(&staged);
                match disk.rename(&staged, &target, How::Excl) {
                    Ok(()) => done.push(Done::Created { target: target.clone(), after: *after }),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(conflict(path, file_sha(disk, &target).ok().flatten())),
                    Err(e) if e.kind() == ErrorKind::Unsupported => {
                        if disk.kind(&target).map_err(io)?.is_some() {
                            return Err(conflict(path, file_sha(disk, &target).ok().flatten()));
                        }
                        disk.rename(&staged, &target, How::Plain).map_err(io)?;
                        done.push(Done::Created { target: target.clone(), after: *after });
                    }
                    Err(e) => return Err(io(e)),
                }
                stamps.push((path.clone(), own_stamp(ours, &target)));
            }
            Target::Remove { path, before } => {
                let (target, aside) = (dir.join(path), staging.join(format!("removed-{k}")));
                disk.rename(&target, &aside, How::Plain).map_err(io)?;
                // Recorded first: whatever fails next, the rollback moves it back.
                done.push(Done::Removed { target, aside: aside.clone() });
                let was = Sha::of(&disk.read(&aside).map_err(io)?);
                if was != before.sha {
                    return Err(conflict(path, Some(was)));
                }
                prune(disk, dir, path);
                stamps.push((path.clone(), None));
            }
            Target::Rename { from, to, hash, dir: folder } => {
                let now = sha_at(disk, dir, from, *folder).map_err(io)?;
                if now != Some(*hash) {
                    return Err(conflict(from, now));
                }
                let (src, dst) = (dir.join(from), dir.join(to));
                if let Some(parent) = dst.parent() {
                    disk.mkdirs(parent).map_err(io)?;
                }
                match disk.rename(&src, &dst, How::Excl) {
                    Ok(()) => {}
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        return Err(conflict(to, sha_at(disk, dir, to, *folder).ok().flatten()));
                    }
                    Err(e) if e.kind() == ErrorKind::Unsupported => {
                        if disk.kind(&dst).map_err(io)?.is_some() {
                            return Err(conflict(to, sha_at(disk, dir, to, *folder).ok().flatten()));
                        }
                        disk.rename(&src, &dst, How::Plain).map_err(io)?;
                    }
                    Err(e) => return Err(io(e)),
                }
                done.push(Done::Renamed { from: src.clone(), to: dst.clone() });
                // A file written into it during the rename is caught here.
                let moved = sha_at(disk, dir, to, *folder).map_err(io)?;
                if moved != Some(*hash) {
                    return Err(conflict(to, moved));
                }
                prune(disk, dir, from);
                // The watcher takes a move in as a batch of its own (its take-in changes nothing).
                stamps.push((from.clone(), None));
            }
        }
    }
    Ok(())
}

/// Move `aside` back to `target`, never over something written there meanwhile.
fn put_back(disk: &mut dyn Disk, aside: &Path, target: &Path) -> io::Result<()> {
    match disk.rename(aside, target, How::Excl) {
        Err(e) if e.kind() == ErrorKind::Unsupported => {
            if disk.kind(target)?.is_some() {
                return Err(io::Error::other(format!("{} was written meanwhile", target.display())));
            }
            disk.rename(aside, target, How::Plain)
        }
        other => other,
    }
}

/// Undo what [`rename_all`] did, newest first. Each step is checked as the forward one was: no
/// file anyone else wrote is touched.
fn rollback(disk: &mut dyn Disk, done: &mut Vec<Done>) -> Result<(), String> {
    while let Some(d) = done.pop() {
        let undone = match &d {
            Done::Swapped { staged, target } => disk.rename(staged, target, How::Swap),
            Done::Replaced { staged, target, aside } => {
                disk.rename(target, staged, How::Plain).and_then(|_| disk.rename(aside, target, How::Plain))
            }
            Done::Created { target, after } => match file_sha(disk, target) {
                Ok(Some(sha)) if sha == *after => disk.remove_file(target),
                Ok(_) => Err(io::Error::other(format!("{} was written over meanwhile", target.display()))),
                Err(e) => Err(e),
            },
            Done::Removed { target, aside } => {
                if let Some(parent) = target.parent() {
                    let _ = disk.mkdirs(parent);
                }
                put_back(disk, aside, target)
            }
            Done::Renamed { from, to } => {
                if let Some(parent) = from.parent() {
                    let _ = disk.mkdirs(parent);
                }
                put_back(disk, to, from)
            }
        };
        if let Err(e) = undone {
            done.push(d);
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// The last barrier and tidy, at close: the journals awaiting their tidy go once their renames
/// are durable, and with them the before-images only undo needed (`keep`: those interrupted
/// journals still need).
pub fn close(disk: &mut dyn Disk, dir: &Path, previous: &mut Vec<String>, keep: &BTreeSet<Sha>) {
    let txd = tx_dir(dir);
    if let Some(last) = previous.last() {
        let _ = disk.barrier(&txd.join(format!("{last}.json")));
    }
    for p in previous.drain(..) {
        tidy(disk, &txd, &p);
    }
    gc_blobs(disk, &txd.join("blobs"), keep, &[]);
}

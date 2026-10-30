//! The fault ring: one path for every part that finds a fault while sound is made (RENDER-25,
//! DIAG-4). A part reports a [`Fault`], numbers only, into the app's one [`Faults`]. The session
//! reads it, names each fault with the plan it came from, and tells it as a problem.
//!
//! [`Faults::report`] never allocates, locks or waits, so the audio thread reports too
//! (RENDER-28). It is a bounded ring of [`SLOTS`] slots, each with an atomic turn number (Vyukov's
//! design). A writer claims a slot with a compare-and-swap. A full ring counts what it lost. Many
//! write, one reads.

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::OutOfMemory;

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Code {
    /// A node stopped live: a kernel or a voice trapped (`sound.stopped`).
    Stopped,
    /// A node stopped live: a kernel asked for more memory than a sound may use (`sound.stopped`).
    OutOfMemory,
    /// A plugin reported an error live, or didn't come back after an export (`plugin.failed`).
    PluginFailed,
    /// The plugin host process crashed (`plugin.crashed`).
    PluginCrashed,
    /// The plugin host process stopped answering, and was ended (`plugin.crashed`).
    PluginHung,
    /// A file that is there can't be read or decoded while it plays: a clip's, or one a kernel
    /// reads (`asset.unreadable`).
    Unreadable,
    /// A clip's file is there but isn't a WAV (a FLAC put there by hand: FMT-24), so it can't be
    /// streamed (`asset.unreadable`).
    NotWav,
    /// A file a kernel reads holds more samples than one whole decode may make (FMT-24), so it
    /// isn't decoded (`asset.unreadable`).
    TooLong,
    /// A warped clip's stretch failed (`stretch.failed`).
    StretchFailed,
    /// The sound input chosen in Settings isn't there, so a take records from another
    /// (`input.missing`).
    InputMissing,
    /// The sound input went away during a take, which ended with what it had (`input.lost`).
    InputLost,
    /// The OS refuses music2 the microphone during a take, which recorded silence (`input.refused`).
    InputRefused,
}

impl Code {
    /// Why `e`, a node's error, stopped it.
    pub fn of(e: &anyhow::Error) -> Code {
        if e.downcast_ref::<OutOfMemory>().is_some() {
            return Code::OutOfMemory;
        }
        match e.downcast_ref::<PluginStopped>() {
            Some(p) => p.code,
            None => Code::Stopped,
        }
    }

    /// Its code in the contract's table (DIAG-2).
    pub fn diag(self) -> &'static str {
        match self {
            Code::Stopped | Code::OutOfMemory => "sound.stopped",
            Code::PluginFailed => "plugin.failed",
            Code::PluginCrashed | Code::PluginHung => "plugin.crashed",
            Code::Unreadable | Code::NotWav | Code::TooLong => "asset.unreadable",
            Code::StretchFailed => "stretch.failed",
            Code::InputMissing => "input.missing",
            Code::InputLost => "input.lost",
            Code::InputRefused => "input.refused",
        }
    }

    /// Whether it is the sound input's: of no plan ([`Subject::Input`]).
    pub fn of_input(self) -> bool {
        matches!(self, Code::InputMissing | Code::InputLost | Code::InputRefused)
    }
}

/// What a fault is about, by its number in the plan it came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Subject {
    /// A top-level node: a kernel, a `poly`, a plugin.
    Node(u32),
    /// A clip: the plan's lane `lane` (its place in [`crate::lane_slots`]), and the clip's place
    /// among that lane's clips in the song's data.
    Clip { lane: u32, clip: u32 },
    /// A file a kernel reads: top-level node `node`, and the file's place among the names its
    /// kernels read ([`crate::audio::assets_of`]), its voice's for a `poly`.
    Asset { node: u32, slot: u32 },
    /// The sound input, by its opening (`m2_play::Listen::open`, counted up for the whole app). It
    /// is of no plan: its fault's `plan` is 0.
    Input(u32),
}

/// A plugin that stopped working while an offline render played it: why the render fails
/// (`plugin.failed`, or `plugin.crashed` when its host process ended).
#[derive(Debug)]
pub struct PluginStopped {
    pub code: Code,
    /// The node's name.
    pub name: String,
}

impl std::fmt::Display for PluginStopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            Code::PluginCrashed | Code::PluginHung => write!(f, "{}: its plugin host ended", self.name),
            _ => write!(f, "{} reported a processing error", self.name),
        }
    }
}

impl std::error::Error for PluginStopped {}

/// What stopped an offline render (RENDER-1): a fault that changed its sound, as numbers, and the
/// error that said so, when one did (a node's). Its words are the error's, or the code's; the
/// session names its part ([`Fault::subject`]) for the person.
#[derive(Debug)]
pub struct Failed {
    pub fault: Fault,
    why: Option<anyhow::Error>,
}

impl Failed {
    pub fn new(fault: Fault, why: Option<anyhow::Error>) -> Failed {
        Failed { fault, why }
    }

    /// The error that said so, when one did: the detail.
    pub fn why(&self) -> Option<&anyhow::Error> {
        self.why.as_ref()
    }
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The node's words as they were, its causes after them: what `{:#}` showed before.
        match &self.why {
            Some(e) => write!(f, "{e:#}"),
            None => write!(f, "{:?} faulted ({})", self.fault.subject, self.fault.code.diag()),
        }
    }
}

impl std::error::Error for Failed {}

/// One fault: what went wrong, in which plan ([`crate::PlanInfo::id`]), and what it is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault {
    pub code: Code,
    pub plan: u64,
    pub subject: Subject,
}

// A fault is copied into its slot whole: small, and never more than this.
const _: () = assert!(size_of::<Fault>() <= 24);

/// The ring's slots.
pub const SLOTS: usize = 256;

/// The fault ring (D1). One live ring per app: every live plan reports into it, and the session
/// reads it. An offline render has its own.
pub struct Faults {
    slots: Box<[Slot]>,
    /// The position the next writer claims, counting up for ever.
    head: AtomicUsize,
    /// The position the reader takes next.
    tail: AtomicUsize,
    /// Faults a full ring couldn't take.
    lost: AtomicU64,
    /// Faults reported, lost ones too.
    reported: AtomicU64,
}

struct Slot {
    /// `pos` while it waits for the writer of position `pos`; `pos + 1` once that writer filled it.
    /// The reader frees it for the next lap, `pos + SLOTS`.
    turn: AtomicUsize,
    fault: UnsafeCell<MaybeUninit<Fault>>,
}

// SAFETY: a slot's fault is written only by the one writer whose compare-and-swap claimed its
// position, and read only by the one reader whose compare-and-swap took it, after its turn said it
// was filled. The turn's Release and Acquire order each write before the read, and each read before
// the next lap's write.
unsafe impl Sync for Faults {}

impl Default for Faults {
    fn default() -> Faults {
        Faults {
            slots: (0..SLOTS).map(|i| Slot { turn: AtomicUsize::new(i), fault: UnsafeCell::new(MaybeUninit::uninit()) }).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            lost: AtomicU64::new(0),
            reported: AtomicU64::new(0),
        }
    }
}

impl std::fmt::Debug for Faults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Faults").field("reported", &self.count()).field("lost", &self.lost()).finish()
    }
}

impl Faults {
    /// Report `f`. Never allocates, locks or waits: any thread may, the audio thread too. A full
    /// ring drops it and counts it in [`Self::lost`].
    pub fn report(&self, f: Fault) {
        self.reported.fetch_add(1, Ordering::Relaxed);
        let mut pos = self.head.load(Ordering::Relaxed);
        loop {
            let slot = &self.slots[pos % SLOTS];
            let turn = slot.turn.load(Ordering::Acquire);
            match turn.wrapping_sub(pos) as isize {
                0 => match self.head.compare_exchange_weak(pos, pos.wrapping_add(1), Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => {
                        // SAFETY: the compare-and-swap made position `pos` this writer's alone.
                        unsafe { (*slot.fault.get()).write(f) };
                        slot.turn.store(pos.wrapping_add(1), Ordering::Release);
                        return;
                    }
                    Err(now) => pos = now,
                },
                // Still holds the fault of the lap before: full.
                d if d < 0 => {
                    self.lost.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                // Another writer claimed it first.
                _ => pos = self.head.load(Ordering::Relaxed),
            }
        }
    }

    /// The oldest fault not read yet, if any: the session's.
    pub fn take(&self) -> Option<Fault> {
        let mut pos = self.tail.load(Ordering::Relaxed);
        loop {
            let slot = &self.slots[pos % SLOTS];
            let turn = slot.turn.load(Ordering::Acquire);
            match turn.wrapping_sub(pos.wrapping_add(1)) as isize {
                0 => match self.tail.compare_exchange_weak(pos, pos.wrapping_add(1), Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => {
                        // SAFETY: its turn says its writer filled it, and the compare-and-swap made
                        // position `pos` this reader's alone.
                        let f = unsafe { (*slot.fault.get()).assume_init_read() };
                        slot.turn.store(pos.wrapping_add(SLOTS), Ordering::Release);
                        return Some(f);
                    }
                    Err(now) => pos = now,
                },
                // Not filled yet: empty.
                d if d < 0 => return None,
                _ => pos = self.tail.load(Ordering::Relaxed),
            }
        }
    }

    /// Every fault not read yet, oldest first.
    pub fn drain(&self) -> Vec<Fault> {
        std::iter::from_fn(|| self.take()).collect()
    }

    /// Faults a full ring couldn't take, so far.
    pub fn lost(&self) -> u64 {
        self.lost.load(Ordering::Relaxed)
    }

    /// Faults reported so far, lost ones too: G12 counts each as a crash.
    pub fn count(&self) -> u64 {
        self.reported.load(Ordering::Relaxed)
    }
}

/// A plan's way to its ring: what its parts report through, as that plan.
#[derive(Clone, Debug)]
pub struct Reporter {
    faults: Arc<Faults>,
    plan: u64,
}

impl Reporter {
    pub fn new(faults: Arc<Faults>, plan: u64) -> Reporter {
        Reporter { faults, plan }
    }

    /// Report `code` about top-level node `node`. Never allocates, locks or waits.
    pub fn node(&self, code: Code, node: usize) {
        self.report(code, Subject::Node(node as u32));
    }

    /// Report `code` about `subject`. Never allocates, locks or waits.
    pub fn report(&self, code: Code, subject: Subject) {
        self.faults.report(self.fault(code, subject));
    }

    /// The fault `code` about `subject`, as this plan's.
    pub fn fault(&self, code: Code, subject: Subject) -> Fault {
        Fault { code, plan: self.plan, subject }
    }

    /// The ring it reports to.
    pub fn faults(&self) -> &Arc<Faults> {
        &self.faults
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopped(plan: u64, node: u32) -> Fault {
        Fault { code: Code::Stopped, plan, subject: Subject::Node(node) }
    }

    // D1: a full ring takes no more; it counts what it lost, and what it holds comes out oldest
    // first. Read, it takes more again.
    #[test]
    fn a_full_ring_counts_what_it_lost() {
        let faults = Faults::default();
        for n in 0..SLOTS as u32 + 10 {
            faults.report(stopped(1, n));
        }
        assert_eq!((faults.lost(), faults.count()), (10, SLOTS as u64 + 10));
        let read = faults.drain();
        assert_eq!(read, (0..SLOTS as u32).map(|n| stopped(1, n)).collect::<Vec<_>>());
        assert_eq!(faults.take(), None, "empty");
        // Laps on: each fault in, each out.
        for n in 0..3 * SLOTS as u32 {
            faults.report(stopped(2, n));
            assert_eq!(faults.take(), Some(stopped(2, n)));
        }
        assert_eq!(faults.lost(), 10, "none lost since");
    }

    // D1: many writers at once, one reader: every fault comes out once.
    #[test]
    fn many_writers_lose_nothing_the_ring_has_room_for() {
        let faults = std::sync::Arc::new(Faults::default());
        let threads: Vec<_> = (0..4u64)
            .map(|t| {
                let faults = faults.clone();
                std::thread::spawn(move || (0..50).for_each(|n| faults.report(stopped(t, n))))
            })
            .collect();
        threads.into_iter().for_each(|t| t.join().unwrap());
        let mut read = faults.drain();
        read.sort_by_key(|f| (f.plan, f.subject.node()));
        let all: Vec<Fault> = (0..4).flat_map(|t| (0..50).map(move |n| stopped(t, n))).collect();
        assert_eq!((read, faults.lost()), (all, 0));
    }

    impl Subject {
        fn node(self) -> u32 {
            match self {
                Subject::Node(n) | Subject::Clip { clip: n, .. } | Subject::Asset { slot: n, .. } | Subject::Input(n) => n,
            }
        }
    }
}

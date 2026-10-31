//! Where a voice's tables are kept, and how a table is laid out.
//!
//! **A table** is frames of [`FRAME`] samples, each one cycle of a wave. Each frame is kept as
//! [`LEVELS`] copies, one a mip level: level `k` holds the frame's harmonics up to [`TOP`]`[k]`
//! (1023, then 512, 256, … 2: an octave fewer each level), in [`LEN`]`[k]` samples (256 for each
//! harmonic it holds, a frame's at most), with its first sample again at its end, so a read
//! between its last sample and its first needs no wrap. A frame's levels lie one after another,
//! [`PER_FRAME`] floats in all, and a table's frames one after another.
//!
//! **The shelf.** Every voice of a sound plays the same tables, so they are made once for a
//! sandbox, at the birth of its first voice that plays them, and kept in the module's static memory:
//! the shelf, room for [`ROOM`] frames (two tables of the most frames one may have). Each table on it
//! is kept by what it was made from ([`Source`]): a built-in table's place, or a file's handle,
//! which the host gives every state of a sandbox that reads the same audio. A table is made in
//! full before any voice reads it and never changed after, and none is made while a voice plays.
//! So kernel ABI v1's fifth rule holds (ABI-5): no state's own data is in a static (a table is
//! the same for every state that plays it, a pure function of its source), and no static points
//! at memory a state took (the shelf is the module's own). A table the shelf has no room for is
//! made in the voice's own memory, as its birth takes it, and plays the same bits.

use std::cell::UnsafeCell;

/// Samples in a frame: one cycle.
pub const FRAME: usize = 2048;

/// The most frames a table holds.
pub const MAX_FRAMES: usize = 64;

/// Mip levels: an octave each.
pub const LEVELS: usize = 10;

/// The highest harmonic each level holds: all a frame has below its Nyquist at level 0, then an
/// octave fewer each level.
pub const TOP: [usize; LEVELS] = {
    let mut t = [0; LEVELS];
    let mut k = 0;
    while k < LEVELS {
        t[k] = if k == 0 { FRAME / 2 - 1 } else { (FRAME / 2) >> k };
        k += 1;
    }
    t
};

/// Samples of each level's copy of a frame: 256 for each harmonic it holds, a frame's at most. A
/// read draws a straight line between two samples, which makes an image of each harmonic `h`
/// about `(h / LEN)²` of it, so 96 dB under the top harmonic, and further under the rest.
pub const LEN: [usize; LEVELS] = {
    let mut l = [0; LEVELS];
    let mut k = 0;
    while k < LEVELS {
        let want = 256 * ((FRAME / 2) >> k);
        l[k] = if want < FRAME { want } else { FRAME };
        k += 1;
    }
    l
};

/// Where each level's copy starts in a frame's floats.
pub const OFFSET: [usize; LEVELS] = {
    let mut o = [0; LEVELS];
    let mut k = 1;
    while k < LEVELS {
        o[k] = o[k - 1] + LEN[k - 1] + 1;
        k += 1;
    }
    o
};

/// Floats a frame takes: each level's copy and its one sample again.
pub const PER_FRAME: usize = OFFSET[LEVELS - 1] + LEN[LEVELS - 1] + 1;

/// Frames the shelf has room for.
pub const ROOM: usize = 2 * MAX_FRAMES;

/// Tables the shelf keeps track of: a voice plays two, and a sandbox holds one sound's voices.
const KEPT: usize = 4;

/// What a table is made from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    /// A built-in table, by its place in the menu.
    Builtin(usize),
    /// A file, by the handle the host gave it.
    File(u32),
}

/// A table, where it lies: on the shelf, or in the voice's own memory.
pub struct Table {
    at: *const f32,
    frames: usize,
    /// The table, when the voice made it in memory of its own.
    _own: Option<Box<[f32]>>,
}

impl Table {
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Where its first float lies.
    pub fn at(&self) -> *const f32 {
        self.at
    }

    /// Level `k`'s copy of frame `frame` (the last frame past the end), with its first sample again
    /// at its end: [`LEN`]`[k] + 1` floats.
    pub fn level(&self, frame: usize, k: usize) -> &[f32] {
        let at = frame.min(self.frames - 1) * PER_FRAME + OFFSET[k];
        // SAFETY: a table holds `frames` frames of PER_FRAME floats, made in full before this
        // voice was born and never changed after: on the shelf, which lives as long as the module,
        // or in `_own`, which lives as long as `self`.
        unsafe { std::slice::from_raw_parts(self.at.add(at), LEN[k] + 1) }
    }
}

/// What the shelf holds: each table's source, where it starts and its frames.
struct Kept {
    tables: [(Source, usize, usize); KEPT],
    n: usize,
    /// Frames taken.
    used: usize,
}

impl Kept {
    const fn new() -> Self {
        Kept { tables: [(Source::Builtin(0), 0, 0); KEPT], n: 0, used: 0 }
    }
}

/// The table made from `source`: from the shelf when it is there; else made by `make` (into the
/// `frames * PER_FRAME` floats it is given), on the shelf when it has room, else in memory of the
/// voice's own. Only a birth calls it.
pub fn table(source: Source, frames: usize, make: impl FnOnce(&mut [f32])) -> Table {
    let frames = frames.clamp(1, MAX_FRAMES);
    store::with(|kept, base| {
        if let Some(&(_, start, frames)) = kept.tables[..kept.n].iter().find(|t| t.0 == source) {
            // SAFETY: `start` frames in, inside the shelf.
            return Table { at: unsafe { base.add(start * PER_FRAME) }, frames, _own: None };
        }
        if kept.n < KEPT && kept.used + frames <= ROOM {
            let start = kept.used;
            // SAFETY: frames `start..start + frames` of the shelf, which no table holds yet and no
            // voice reads.
            let room = unsafe { std::slice::from_raw_parts_mut(base.add(start * PER_FRAME), frames * PER_FRAME) };
            make(room);
            kept.tables[kept.n] = (source, start, frames);
            kept.n += 1;
            kept.used += frames;
            return Table { at: room.as_ptr(), frames, _own: None };
        }
        let mut own = vec![0.0; frames * PER_FRAME].into_boxed_slice();
        make(&mut own);
        Table { at: own.as_ptr(), frames, _own: Some(own) }
    })
}

/// The shelf's memory: the module's static memory in a sandbox. A sandbox runs one call at a time,
/// on one thread, so nothing else touches it while a call does.
#[cfg(target_arch = "wasm32")]
mod store {
    use super::*;

    struct Shelf {
        kept: UnsafeCell<Kept>,
        floats: UnsafeCell<[f32; ROOM * PER_FRAME]>,
    }

    // SAFETY: a sandbox's module runs on one thread at a time (it has no threads of its own).
    unsafe impl Sync for Shelf {}

    static SHELF: Shelf = Shelf { kept: UnsafeCell::new(Kept::new()), floats: UnsafeCell::new([0.0; ROOM * PER_FRAME]) };

    pub(super) fn with<R>(f: impl FnOnce(&mut Kept, *mut f32) -> R) -> R {
        // SAFETY: one call at a time, and no other reference to `kept` lives past this one.
        f(unsafe { &mut *SHELF.kept.get() }, SHELF.floats.get().cast())
    }
}

/// Off wasm32 (the native tests), a shelf for each thread, as each test is a sandbox of its own.
#[cfg(not(target_arch = "wasm32"))]
mod store {
    use super::*;

    thread_local! {
        static KEPT: UnsafeCell<Kept> = const { UnsafeCell::new(Kept::new()) };
        static FLOATS: *mut f32 = Box::leak(vec![0.0f32; ROOM * PER_FRAME].into_boxed_slice()).as_mut_ptr();
    }

    pub(super) fn with<R>(f: impl FnOnce(&mut Kept, *mut f32) -> R) -> R {
        let base = FLOATS.with(|b| *b);
        // SAFETY: this thread's own, and no other reference to it lives past this one.
        KEPT.with(|k| f(unsafe { &mut *k.get() }, base))
    }
}

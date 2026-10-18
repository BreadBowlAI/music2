//! The disk thread (RENDER-30, D6): one per player, `m2-disk`. It keeps each audio lane's ring
//! filled around the playhead: the chunk before it and 2 s ahead, along the transport's path (through
//! a loop's wrap), and while a loop is on, the loop start's first half second. Stopped, the same
//! window, so Play starts at once. A jump fades out where it left for 5 ms (RENDER-9), so it also
//! keeps where playback is now while a seek's target is read, and what follows a loop's end. The
//! sound thread only copies what it filled.
//!
//! It runs at normal priority, not macOS's background class: that class throttles disk reads.
//!
//! A seek asks for its target first and waits for it up to 30 ms (D8); an activation waits up to
//! 500 ms for each new lane's next half second, in the new tempo map (D9). Both are caps: what is
//! ready already costs nothing.
//!
//! A warped clip's render may still be being made, by its live cache's stretch thread (the sx
//! plan's D10): a chunk that reads frames not made yet is left empty, so it plays silence and
//! counts a miss, and the thread fills it once they are made. Each render wakes it as it goes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Wake, Waker};
use std::time::{Duration, Instant};

use m2_runtime::audio::AudioLane;

/// How far ahead the disk thread reads, in seconds.
const AHEAD: f64 = 2.0;
/// How much of the loop start it keeps while a loop is on, and how much of a new lane an
/// activation waits for, in seconds.
const HALF: f64 = 0.5;
/// How much of a seek's target it waits for, in seconds.
const TARGET: f64 = 0.1;
/// How long a seek waits for its target at most (gate 3 allows 50 ms), and an activation for its
/// new lanes.
pub(crate) const SEEK_WAIT: Duration = Duration::from_millis(30);
pub(crate) const PREFILL_WAIT: Duration = Duration::from_millis(500);
/// How long a lane a newer plan dropped is kept filled: its plan may still play it through a fade.
const KEPT: Duration = Duration::from_secs(2);
/// How long a seek's start is kept: its fade out is 5 ms, after a callback or two.
const FADING: Duration = Duration::from_millis(200);
/// How often the thread looks again with nothing to do.
const IDLE: Duration = Duration::from_millis(10);

struct Entry {
    module: String,
    lane: Weak<AudioLane>,
    /// When a newer plan left it out; None while the plan sent last plays it.
    left: Option<Instant>,
}

#[derive(Default)]
struct State {
    lanes: Vec<Entry>,
    looping: Option<(f64, f64)>,
    /// A seek's target, read first.
    hint: Option<f64>,
    /// Where recent seeks left from, which their fades read for 5 ms, by when they were sent.
    left: Vec<(Instant, f64)>,
}

/// What a player and its disk thread share.
pub(crate) struct Disk {
    state: Mutex<State>,
    wake: Condvar,
    /// Where playback is about to be, in beats (a load or a seek not yet taken counts), and where
    /// the sound thread last said it is.
    heading: Box<dyn Fn() -> (f64, f64) + Send + Sync>,
    /// A test seam: the thread fills nothing while it is set.
    paused: AtomicBool,
    /// Wakes the thread: given to the renders its lanes play, which wake it as they are made.
    waker: Waker,
}

/// What wakes a disk thread, while its disk is there.
struct Nudge(Weak<Disk>);

impl Wake for Nudge {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(d) = self.0.upgrade() {
            d.wake.notify_all();
        }
    }
}

impl Disk {
    /// A disk for a player whose playhead `heading` reads, and its thread, which ends once `alive`
    /// is gone.
    pub(crate) fn start(heading: Box<dyn Fn() -> (f64, f64) + Send + Sync>, alive: Weak<()>) -> Arc<Disk> {
        let disk = Arc::new_cyclic(|d| Disk {
            state: Mutex::default(),
            wake: Condvar::new(),
            heading,
            paused: AtomicBool::new(false),
            waker: Waker::from(Arc::new(Nudge(d.clone()))),
        });
        let d = disk.clone();
        let spawned = std::thread::Builder::new().name("m2-disk".into()).spawn(move || d.run(alive));
        if let Err(e) = spawned {
            eprintln!("music2: no disk thread ({e}): long audio clips will be silent");
        }
        disk
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The thread: fill the most urgent chunk any lane lacks, again and again; with none, wait a
    /// little or until asked.
    fn run(&self, alive: Weak<()>) {
        while alive.strong_count() > 0 {
            let (lanes, at) = self.snapshot();
            let urgent = (!self.paused.load(Ordering::Relaxed)).then(|| most_urgent(&lanes, &at)).flatten();
            match urgent {
                Some((lane, k, wanted)) => {
                    lane.fill_chunk(k, &|x| wanted.contains(&x));
                }
                None => {
                    drop(lanes);
                    let st = self.state();
                    drop(self.wake.wait_timeout(st, IDLE));
                }
            }
        }
    }

    /// Every lane alive, those of the plan sent last first; and where playback goes.
    fn snapshot(&self) -> (Vec<Arc<AudioLane>>, At) {
        let mut st = self.state();
        st.lanes.retain(|e| e.lane.strong_count() > 0 && e.left.is_none_or(|t| t.elapsed() < KEPT));
        let mut lanes: Vec<(bool, Arc<AudioLane>)> = st.lanes.iter().filter_map(|e| Some((e.left.is_some(), e.lane.upgrade()?))).collect();
        lanes.sort_by_key(|l| l.0);
        let (heading, now) = (self.heading)();
        st.left.retain(|l| l.0.elapsed() < FADING);
        let left = st.left.iter().map(|l| l.1).collect();
        let at = At { beat: st.hint.unwrap_or(heading), now, looping: st.looping, left };
        (lanes.into_iter().map(|l| l.1).collect(), at)
    }

    /// The lanes of the plan about to be sent, each with the module it plays in: these are kept
    /// filled from now on, and the plan sent before keeps its lanes for a while. Returns the lanes
    /// that are new.
    pub(crate) fn register(&self, lanes: Vec<(String, Arc<AudioLane>)>) -> Vec<Arc<AudioLane>> {
        let mut st = self.state();
        let now = Instant::now();
        for e in &mut st.lanes {
            e.left.get_or_insert(now);
        }
        let mut new = vec![];
        for (module, lane) in lanes {
            match st.lanes.iter_mut().find(|e| e.lane.as_ptr() == Arc::as_ptr(&lane)) {
                Some(e) => {
                    e.left = None;
                    e.module = module;
                }
                None => {
                    lane.watch(&self.waker);
                    st.lanes.push(Entry { module, lane: Arc::downgrade(&lane), left: None });
                    new.push(lane);
                }
            }
        }
        drop(st);
        self.wake.notify_all();
        new
    }

    /// The loop, in beats, or none: its start is kept filled while it is on.
    pub(crate) fn set_loop(&self, beats: Option<(f64, f64)>) {
        self.state().looping = beats;
        self.wake.notify_all();
    }

    /// Wait until each of `lanes` holds the next `seconds` from where playback is (or `at`), for
    /// `cap` at most. True when they do.
    fn wait_for(&self, lanes: &[Arc<AudioLane>], at: Option<f64>, seconds: f64, cap: Duration) -> bool {
        let t = Instant::now();
        loop {
            let beat = at.unwrap_or_else(|| (self.heading)().0);
            if lanes.iter().all(|l| l.ready_for(l.frame_of(beat), (seconds * l.rate()) as u64)) {
                return true;
            }
            if t.elapsed() >= cap {
                return false;
            }
            self.wake.notify_all();
            std::thread::sleep(Duration::from_micros(250));
        }
    }

    /// Before a plan is sent: its new lanes hold the next half second from `at` (else where
    /// playback is), or 500 ms passed (D9). A given `at` is read first until [`Disk::sought`].
    pub(crate) fn prefill(&self, new: &[Arc<AudioLane>], at: Option<f64>) -> bool {
        if at.is_some() {
            self.state().hint = at;
        }
        self.wait_for(new, at, HALF, PREFILL_WAIT)
    }

    /// Before a seek to `beat` is sent: read its target first, and wait for it up to 30 ms (D8).
    pub(crate) fn seek(&self, beat: f64) -> bool {
        let lanes = self.current();
        let (heading, now) = (self.heading)();
        let mut st = self.state();
        // A seek fades out where playback is when it is taken: where it is, or where a seek sent
        // before goes.
        let t = Instant::now();
        let going = st.hint.unwrap_or(heading);
        st.left.extend([(t, now), (t, going)]);
        st.hint = Some(beat);
        drop(st);
        self.wait_for(&lanes, Some(beat), TARGET, SEEK_WAIT)
    }

    /// The seek or load was sent: the playhead says where it goes from now on.
    pub(crate) fn sought(&self) {
        self.state().hint = None;
    }

    /// The lanes of the plan sent last.
    fn current(&self) -> Vec<Arc<AudioLane>> {
        self.state().lanes.iter().filter(|e| e.left.is_none()).filter_map(|e| e.lane.upgrade()).collect()
    }

    /// The blocks each module's lanes missed, in all, for the plan sent last (D7).
    pub(crate) fn misses(&self) -> Vec<(String, u64)> {
        let mut out: Vec<(String, u64)> = vec![];
        let st = self.state();
        for e in st.lanes.iter().filter(|e| e.left.is_none()) {
            let Some(l) = e.lane.upgrade() else { continue };
            match out.iter_mut().find(|o| o.0 == e.module) {
                Some(o) => o.1 += l.misses(),
                None => out.push((e.module.clone(), l.misses())),
            }
        }
        out
    }

    /// Wait until every lane holds all the disk thread keeps for where playback is now, for `cap`
    /// at most: a driven player's callbacks then never outrun the disk, as a device's real time
    /// never does (tests). Never on a callback.
    pub(crate) fn settle(&self, cap: Duration) {
        let t = Instant::now();
        loop {
            let (lanes, at) = self.snapshot();
            if self.paused.load(Ordering::Relaxed) || most_urgent(&lanes, &at).is_none() || t.elapsed() >= cap {
                return;
            }
            self.wake.notify_all();
            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Stop filling, or fill again (a test seam: a disk that never answers).
    #[doc(hidden)]
    pub(crate) fn pause(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
        self.wake.notify_all();
    }
}

/// Where playback goes: the beat to read around (a seek's target first), where it is now, and the
/// loop.
pub(crate) struct At {
    pub(crate) beat: f64,
    pub(crate) now: f64,
    pub(crate) looping: Option<(f64, f64)>,
    /// Where recent seeks left from.
    pub(crate) left: Vec<f64>,
}

/// The chunks `lane` should hold for playback `at`, most urgent first: along the transport's path
/// from the playhead, 2 s ahead (through a loop's wrap); the loop start's first half second; what
/// follows the loop's end, where playback is now and where recent seeks left from, which a jump
/// fades out from; the chunk before the playhead. Only chunks with audio, and no more than its
/// slots.
pub(crate) fn wanted(lane: &AudioLane, at: &At) -> Vec<u64> {
    let (c, rate) = (lane.chunk(), lane.rate());
    let p = lane.frame_of(at.beat);
    let slots = lane.slots();
    let mut out = Vec::with_capacity(slots);
    let push = |out: &mut Vec<u64>, k: u64| {
        if out.len() < slots && lane.has_audio(k) && !out.contains(&k) {
            out.push(k);
        }
    };
    let ahead = (AHEAD * rate) as u64;
    let looped = at.looping.map(|(a, b)| (lane.frame_of(a), lane.frame_of(b))).filter(|&(s, e)| e > s);
    match looped {
        Some((s, e)) if p < e => {
            let (mut at, mut left) = (p, ahead);
            while left > 0 && out.len() < slots {
                let to = e.min(at + left);
                (at / c..to.div_ceil(c)).for_each(|k| push(&mut out, k));
                left -= to - at;
                at = s;
            }
        }
        _ => (p / c..(p + ahead).div_ceil(c)).for_each(|k| push(&mut out, k)),
    }
    if let Some((s, e)) = looped {
        (s / c..e.min(s + (HALF * rate) as u64).div_ceil(c)).for_each(|k| push(&mut out, k));
        push(&mut out, e / c);
    }
    for b in std::iter::once(at.now).chain(at.left.iter().copied()) {
        let k = lane.frame_of(b) / c;
        push(&mut out, k);
        push(&mut out, k + 1);
    }
    if p / c > 0 {
        push(&mut out, p / c - 1);
    }
    out
}

/// The most urgent chunk a lane lacks and can fill now (a render being made may not have made its
/// frames yet): the lowest rank in its wanted list, lanes of the plan sent last first. With the
/// lane and its whole wanted list, which its fill must not evict.
fn most_urgent(lanes: &[Arc<AudioLane>], at: &At) -> Option<(Arc<AudioLane>, u64, Vec<u64>)> {
    let wants: Vec<Vec<u64>> = lanes.iter().map(|l| wanted(l, at)).collect();
    let most = wants.iter().map(Vec::len).max().unwrap_or(0);
    for rank in 0..most {
        for (l, w) in lanes.iter().zip(&wants) {
            if let Some(&k) = w.get(rank)
                && !l.ready(k)
                && l.can_fill(k)
            {
                return Some((l.clone(), k, w.clone()));
            }
        }
    }
    None
}

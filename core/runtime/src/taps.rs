//! Render taps (8b.8): what the meters show. A live plan (one [`crate::Renderer::prepare`] makes)
//! taps the song's output, and each track where its audio enters another module: the overlay
//! nodes with a cell (MON-1), so a track's meter reads after its mute and before what it feeds.
//! A track read in two places is one meter, the larger of the two.
//!
//! After each block, each channel's largest `|x|` over the block is raised into its slot with
//! `fetch_max`: a non-negative `f32` orders as its bits do, so no lock and no allocation is needed
//! (RENDER-28). The server reads and resets them ([`TapSlots::take`]). Taps read buffers only, so
//! no render changes. Offline renders have none.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::BLOCK;

/// A plan's peaks since they were last taken: the song's output (two channels), then each track's.
#[derive(Debug, Default)]
pub struct TapSlots {
    peaks: Box<[AtomicU32]>,
    /// Each track tapped, by key, in the order its first site comes: its first slot and channels.
    tracks: Vec<(String, usize, usize)>,
}

/// What [`TapSlots::take`] read: each channel's largest `|x|`, linear (1.0 is 0 dBFS). A track's
/// NaN is passed over and an infinity reads as `f32::MAX`, so every peak is a number JSON can
/// carry. `out` reads the block as played: its samples that weren't finite are 0 already.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peaks {
    pub out: [f32; 2],
    pub tracks: Vec<(String, Vec<f32>)>,
}

impl Peaks {
    /// Track `key`'s channels, if it is tapped.
    pub fn track(&self, key: &str) -> Option<&[f32]> {
        self.tracks.iter().find(|t| t.0 == key).map(|t| t.1.as_slice())
    }
}

impl TapSlots {
    /// The tracks tapped, by key.
    pub fn tracks(&self) -> impl Iterator<Item = &str> {
        self.tracks.iter().map(|t| t.0.as_str())
    }

    /// The peaks since the last take, each slot set back to 0.
    pub fn take(&self) -> Peaks {
        let take = |i: usize| f32::from_bits(self.peaks[i].swap(0, Ordering::Relaxed));
        Peaks { out: [take(0), take(1)], tracks: self.tracks.iter().map(|(k, at, n)| (k.clone(), (*at..at + n).map(take).collect())).collect() }
    }

    fn raise(&self, slot: usize, peak: f32) {
        self.peaks[slot].fetch_max(peak.to_bits(), Ordering::Relaxed);
    }
}

/// The audio thread's side of a plan's taps: which exec node raises which slots.
pub(crate) struct Taps {
    slots: Arc<TapSlots>,
    /// Each tapped node: its exec node, its track's first slot, and its track's channels.
    nodes: Vec<(usize, usize, usize)>,
}

impl Taps {
    /// Taps on the output and on `sites`: each overlay node with a cell, as (exec node, track,
    /// channels), in site order. A track's channels are its widest site's.
    pub(crate) fn new(sites: &[(usize, &str, u8)]) -> Taps {
        let mut tracks: Vec<(String, usize, usize)> = vec![];
        for &(_, key, channels) in sites {
            match tracks.iter_mut().find(|t| t.0 == key) {
                Some(t) => t.2 = t.2.max(channels as usize),
                None => tracks.push((key.to_string(), 0, channels as usize)),
            }
        }
        let mut next = 2;
        for t in &mut tracks {
            t.1 = next;
            next += t.2;
        }
        let nodes = sites
            .iter()
            .map(|&(node, key, _)| {
                let t = tracks.iter().find(|t| t.0 == key).expect("each site's track");
                (node, t.1, t.2)
            })
            .collect();
        let peaks = (0..next).map(|_| AtomicU32::new(0)).collect();
        Taps { slots: Arc::new(TapSlots { peaks, tracks }), nodes }
    }

    pub(crate) fn slots(&self) -> &Arc<TapSlots> {
        &self.slots
    }

    /// After a block: raise each slot to its block's peak. `bufs` are the graph's, by exec node
    /// and port; `out` is the block the song gave. A node narrower than its track gives each
    /// channel it lacks its last one, as a mono signal is heard on both sides. Never allocates.
    pub(crate) fn write(&self, bufs: &[Vec<Vec<f32>>], out: &[[f32; 2]; BLOCK]) {
        for c in 0..2 {
            self.slots.raise(c, peak(out.iter().map(|f| f[c])));
        }
        for &(node, at, channels) in &self.nodes {
            let b = &bufs[node][0];
            let have = (b.len() / BLOCK).max(1);
            for c in 0..channels {
                let s = c.min(have - 1) * BLOCK;
                self.slots.raise(at + c, peak(b[s..s + BLOCK].iter().copied()));
            }
        }
    }
}

/// The largest `|x|`: a NaN is passed over, and an infinity reads as `f32::MAX`.
fn peak(x: impl Iterator<Item = f32>) -> f32 {
    x.fold(0.0f32, |m, v| m.max(v.abs())).min(f32::MAX)
}

#[cfg(test)]
mod tests {
    // `Peaks` says it: a NaN is passed over, and an infinity reads as `f32::MAX`.
    #[test]
    fn a_nan_is_passed_over_and_an_infinity_reads_as_max() {
        assert_eq!(super::peak([f32::NAN, f32::NAN].into_iter()), 0.0);
        assert_eq!(super::peak([0.5, f32::NAN, -0.25].into_iter()), 0.5);
        assert_eq!(super::peak([0.5, f32::NEG_INFINITY, f32::NAN].into_iter()), f32::MAX);
    }
}

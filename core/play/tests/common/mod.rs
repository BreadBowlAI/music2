//! What the player's integration tests share: projects in temp folders, songs, and a device
//! driven by hand.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use m2_ir::{Flat, Ir};
use m2_play::{Driver, Player};
use m2_runtime::{PackageSet, Renderer};
use m2_song::{Arrangement, DataSet, LaneAddr, LaneData, LaneKind, Project};
use serde_json::{Value, json};

pub const SR: u32 = 48_000;

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&repo().join("std")).expect("std loads");
    p
}

/// A fresh, empty temp folder.
pub fn temp(what: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("m2-play-{what}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().unwrap().is_dir() { copy(&src, &dst) } else { std::fs::copy(&src, &dst).map(drop).unwrap() }
    }
}

/// A compiled project: its graph, data, packages and rate.
pub struct Song {
    pub flat: Flat,
    pub data: DataSet,
    pub packages: PackageSet,
    pub rate: u32,
    pub dir: PathBuf,
}

impl Song {
    pub fn renderer(&self) -> Renderer {
        let mut r = Renderer::new(&self.flat, &self.data, &self.packages, self.rate).unwrap();
        r.set_strict(false);
        r
    }

    /// The whole song and a second past its last clip, offline, as it plays live.
    pub fn seconds(&self) -> f64 {
        self.data.tempo().seconds_at(self.data.end()) + 1.0
    }
}

impl Drop for Song {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Compile the project in `dir`, which the song then owns.
pub fn compile(dir: PathBuf) -> Song {
    let project = Project::open(&dir).unwrap_or_else(|d| panic!("{d:?}"));
    let mut packages = std_packages();
    let c = m2_session::compile(&project, &mut packages).unwrap_or_else(|d| panic!("{d:?}"));
    Song { flat: c.flat, data: c.data, packages, rate: project.sample_rate(), dir }
}

/// A copy of the repo's fixture `rel`, compiled.
pub fn fixture(rel: &str) -> Song {
    let dir = temp("fixture");
    copy(&repo().join(rel), &dir);
    compile(dir)
}

/// A song held in memory: one module's nodes and lanes, and its lanes' data.
pub fn song(nodes: Value, out: &str, lanes: Value, data: DataSet) -> Song {
    let ir: Ir = serde_json::from_value(json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
        "modules": [{"key": "song", "lanes": lanes, "nodes": nodes, "exports": {"out": out}}]}))
    .unwrap();
    let packages = std_packages();
    let flat = m2_ir::check(&ir, &packages).unwrap_or_else(|e| panic!("{e:?}"));
    Song { flat, data, packages, rate: SR, dir: temp("song") }
}

pub fn notes(v: Value) -> LaneData {
    LaneData::read(&v.to_string(), &LaneKind::Notes).unwrap()
}

pub fn at_bpm(bpm: f64) -> Arrangement {
    serde_json::from_value(json!({"tempo": [{"beat": 0, "bpm": bpm}]})).unwrap()
}

pub fn addr(lane: &str) -> LaneAddr {
    LaneAddr::new("song", lane).unwrap()
}

/// Sine voices at their notes' pitches, through an envelope releasing over `release` seconds,
/// then `after` (reading the poly as `v`), out of `out`.
pub fn synth(release: f64, after: Value, out: &str, notes_json: Value, arr: Arrangement) -> Song {
    let body = json!([
        {"id": "p", "op": "lane", "lane": "pitch"}, {"id": "g", "op": "lane", "lane": "gate"},
        {"id": "hz", "op": "kernel", "kernel": "std:mtof", "inputs": {"pitch": "p"}},
        {"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {"freq": "hz"}},
        {"id": "e", "op": "kernel", "kernel": "std:adsr", "inputs": {"gate": "g", "sustain": "s", "release": "r"}},
        {"id": "s", "op": "const", "value": 1},
        {"id": "r", "op": "const", "value": release, "unit": "seconds"},
        {"id": "amp", "op": "kernel", "kernel": "std:scale", "inputs": {"in": "o", "by": "e"}}]);
    let mut nodes = vec![
        json!({"id": "n", "op": "lane", "lane": "notes"}),
        json!({"id": "v", "op": "poly", "events": "n", "voices": 8, "body": {"out": "amp", "nodes": body}}),
    ];
    nodes.extend(after.as_array().unwrap().iter().cloned());
    let data = DataSet::with_lanes(arr, [(addr("notes"), notes(notes_json))]);
    song(Value::Array(nodes), out, json!([{"key": "notes", "kind": "events<note>"}]), data)
}

/// Device buffer sizes, in turn: odd ones, and ones bigger than a block.
pub const SIZES: [usize; 7] = [1, 7, 128, 300, 64, 1000, 4096];

/// Fill `frames` frames through the driver in buffers of [`SIZES`], stereo, calling `between`
/// before each buffer with the frames done so far.
pub fn hear(d: &mut Driver, frames: usize, between: &mut dyn FnMut(&mut Driver, usize)) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(frames);
    let mut data = vec![0.0f32; 2 * 4096];
    for n in SIZES.iter().cycle() {
        let n = (*n).min(frames - out.len());
        if n == 0 {
            break;
        }
        between(d, out.len());
        d.fill(&mut data[..2 * n], 2);
        out.extend(data[..2 * n].chunks(2).map(|f| [f[0], f[1]]));
        drop(d.drain_retired());
    }
    out
}

/// A player driven by hand, playing `r` from 0: opened and started before the first fill.
pub fn playing(r: Renderer, device_rate: u32) -> (Player, Driver) {
    let (player, d) = Player::driven(device_rate);
    player.open(r).unwrap();
    player.play();
    (player, d)
}

pub fn peak(x: &[[f32; 2]]) -> f32 {
    x.iter().fold(0.0, |m, f| m.max(f[0].abs()).max(f[1].abs()))
}

pub fn rms(x: &[[f32; 2]]) -> f32 {
    (x.iter().map(|f| 0.5 * (f[0] * f[0] + f[1] * f[1])).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The largest step from one sample to the next.
pub fn max_step(x: &[[f32; 2]]) -> f32 {
    x.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f32::max)
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The first frame where `a` and `b` differ, comparing bits.
pub fn first_diff(a: &[[f32; 2]], b: &[[f32; 2]]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x[0].to_bits() != y[0].to_bits() || x[1].to_bits() != y[1].to_bits())
}

/// A small deterministic generator, for "random" choices that reproduce.
pub fn lcg(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed >> 33
}

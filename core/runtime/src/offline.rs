//! Offline kernels (kernel ABI v1, `flags` bit 0, `docs/kernel-abi.md`): kernels the host runs off
//! the audio thread to make something from a file, never in a song's graph (`kernel.offline`).
//! `std:stretch` makes every warped clip's render ([`crate::audio::Cache`]).
//!
//! One run is one sandbox, born, sealed and prewarmed as any (RENDER-32): its one state is made
//! from its settings, the time map among them, with its source as asset 0, read from the disk
//! ([`Held::Disk`]). Then `k_process` is called block by block, each call the next [`BLOCK`] frames
//! of the output, and each block goes to a sink as soon as it is made. Neither the source nor the
//! output is ever held whole.

use anyhow::{Context, Result};
use m2_song::Sha;
use wasmtime::{Engine, Module};

use crate::limit::Deadline;
use crate::packages::PackageSet;
use crate::sandbox::{Held, Sandbox, Spec};
use crate::{BLOCK, WasmContext, fpenv};

/// The kernel that stretches warped clips: always the song's std's (the sx plan's D9).
pub(crate) const STRETCH: &str = "std:stretch";

/// An offline kernel, ready to run: its module, its export prefix, and its module's hash, which
/// names what it makes (the cache's renders, D8). Cheap to clone.
#[derive(Clone)]
pub(crate) struct Kernel {
    engine: Engine,
    module: Module,
    prefix: String,
    pub(crate) sha: Sha,
    /// How long each call may run: the offline limit, live too, off the audio thread (ABI-6).
    deadline: Deadline,
}

impl Kernel {
    /// Offline kernel `id` of `packages`, when it has one.
    pub(crate) fn of(packages: &PackageSet, id: &str) -> Option<Kernel> {
        if !m2_ir::Kernels::offline(packages, id) {
            return None;
        }
        let (module, prefix, _) = packages.module_for(id)?;
        let (engine, module, prefix) = (packages.engine.clone(), module.clone(), prefix.to_string());
        Some(Kernel { engine, module, prefix, sha: packages.module_sha(id)?, deadline: packages.limits.of(false) })
    }
}

/// One run of an offline kernel: its source, its values and time map, and the output it makes:
/// `frames` frames of `channels` (1 or 2) at `rate`.
pub(crate) struct Run<'a> {
    pub(crate) source: Held,
    pub(crate) values: &'a [f64],
    pub(crate) map: &'a [[f64; 2]],
    pub(crate) rate: f64,
    pub(crate) channels: usize,
    pub(crate) frames: usize,
}

/// Run `k` as `r` says, each block of its output in turn into `sink`, a slice a channel, each of the
/// block's frames (at most [`BLOCK`]). A kernel that traps or runs past its deadline (ABI-6), a
/// file that can't be read and a sink that fails each stop it, with why.
pub(crate) fn run(k: &Kernel, r: &Run, sink: &mut dyn FnMut(&[&[f32]]) -> Result<()>) -> Result<()> {
    let _mode = fpenv::Canon::enter();
    let ch = r.channels.clamp(1, 2);
    let name = &k.prefix;
    let mut sb = Sandbox::new(&k.engine, &k.module, k.deadline).with_context(|| format!("`{name}`"))?;
    let assets = [Some(r.source.clone())];
    let spec = Spec {
        inputs: 0,
        outputs: 1,
        channels: &[ch as u32],
        audio: ch as u8,
        sample_rate: r.rate,
        values: r.values,
        assets: &assets,
        offline: Some(r.map),
    };
    let state = match sb.state(&k.prefix, &spec) {
        Ok(s) => s,
        Err(e) => return Err(sb.ran_out(name, e).context(format!("`{name}`"))),
    };
    sb.seal().with_context(|| format!("`{name}`"))?;
    // The prewarm: a reset that would take memory fails here.
    state.reset(&mut sb).map_err(|e| sb.ran_out(name, e)).with_context(|| format!("`{name}`'s reset"))?;
    let out = state.outs[0];
    let mut block = [[0.0f32; BLOCK]; 2];
    let mut done = 0;
    while done < r.frames {
        let n = (r.frames - done).min(BLOCK);
        let made = state.process(&mut sb, n).wctx(|| format!("`{name}` crashed at frame {done}"));
        made.map_err(|e| sb.ran_out(name, e))?;
        let data = sb.data();
        for (c, b) in block[..ch].iter_mut().enumerate() {
            let at = out + 4 * c * BLOCK;
            b[..n].copy_from_slice(bytemuck::cast_slice(&data[at..at + 4 * n]));
        }
        let [a, b] = &block;
        sink(&[&a[..n], &b[..n]][..ch])?;
        done += n;
    }
    Ok(())
}

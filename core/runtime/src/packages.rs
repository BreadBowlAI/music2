//! Packages: an `m2pkg.toml` manifest, TypeScript modules, and compiled kernels.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use m2_ir::{Diagnostic, Ir, KernelSig, Kernels, Node, Op, PortKind, PortSig, Severity};
use m2_plugins::PluginHost;
use serde::Deserialize;
use wasmtime::{Engine, ExternType, Module, ValType};

use crate::WasmContext;

#[derive(Deserialize)]
struct Manifest {
    package: Info,
    #[serde(default)]
    kernel: Vec<KernelDecl>,
    /// Kept only to refuse it: effects are declared in TypeScript now (PKG-2), and without the
    /// field an old manifest's `[[effect]]` would be skipped silently.
    #[serde(default)]
    effect: Vec<toml::Value>,
}

#[derive(Deserialize)]
struct Info {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    /// The compiled kernels, relative to the package folder.
    wasm: Option<String>,
}

#[derive(Deserialize)]
struct KernelDecl {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    inputs: Vec<String>,
    outputs: Vec<String>,
    #[serde(default)]
    latency: u32,
    /// An offline kernel (kernel ABI v1, flags bit 0): run by the host to make a clip's render, off
    /// the audio thread, never by song code. No inputs; one `audio` output; its source is asset 0.
    #[serde(default)]
    offline: bool,
}

pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    /// The files the runtime read to load it (`std/m2pkg.toml`, `std/kernels/std.wasm`), by the
    /// SHA-256 of their bytes: with the TypeScript the build read, what a render's provenance
    /// names as the package contents that ran (RENDER-2).
    pub files: BTreeMap<String, String>,
    module: Option<Module>,
    /// The SHA-256 of its wasm module: part of each of its kernels' shapes, so a module rebuilt
    /// with the same kernel ids starts every instance from init (RENDER-25).
    module_sha: Option<m2_song::Sha>,
}

impl Package {
    /// The TypeScript entry for `name/<sub>` imports.
    pub fn source(&self, sub: &str) -> PathBuf {
        self.dir.join("src").join(format!("{sub}.ts"))
    }
}

/// Every package a song can use, the kernels they declare, and the hosted plugins
/// a song has asked about.
pub struct PackageSet {
    /// Always [`engine`]'s, whose memories never move: each sandbox keeps its memory's address from
    /// its seal (RENDER-32), so no other engine may be put here.
    pub(crate) engine: Engine,
    packages: BTreeMap<String, Package>,
    kernels: HashMap<String, (KernelSig, String)>,
    /// The offline kernels, by id, and the package of each: kept apart from `kernels`, so no
    /// song's graph types or runs one (`kernel.offline`).
    offline: HashMap<String, String>,
    plugins: HashMap<String, Hosted>,
    /// How plugins are reached (P11): the process's in-process host unless set.
    host: Arc<dyn PluginHost>,
}

/// A hosted plugin as the last build found it.
enum Hosted {
    /// Instantiated once to read its ports and parameters.
    Installed(KernelSig, m2_plugins::Info),
    /// Not installed, or not loadable (FMT-18a): typed from the code as written, played as
    /// silence or as its input passed through. Its ports are those of the last build.
    Missing(KernelSig),
}

/// The engine every kernel runs in.
///
/// WebAssembly floating point is bit-exact on every machine except for the bits
/// inside NaNs. Canonicalising those costs about 4.6× in the M1 benchmark, so instead
/// the renderer silences non-finite output (a NaN means a kernel is broken anyway).
/// Relaxed SIMD is off because its results may differ between CPUs.
///
/// Memories take wasmtime's default reservation, 4 GiB with guards, so the hardware catches a
/// stray access: no access is bounds-checked, and Cranelift's Spectre guard stays on, at no cost
/// (RENDER-32). A sandbox has one memory, and a box one sandbox (one a package for a voice pool),
/// so two renderers of p50 hold about 1.2% of the address space. A sealed memory never grows, so it
/// never moves, and a sandbox keeps its address.
pub fn engine() -> Engine {
    let mut config = wasmtime::Config::new();
    config.cranelift_nan_canonicalization(false);
    config.wasm_relaxed_simd(false);
    config.memory_may_move(false);
    Engine::new(&config).expect("wasmtime engine")
}

impl Default for PackageSet {
    fn default() -> Self {
        let (packages, kernels, plugins) = (BTreeMap::new(), HashMap::new(), HashMap::new());
        PackageSet { engine: engine(), packages, kernels, offline: HashMap::new(), plugins, host: m2_plugins::host() }
    }
}

impl PackageSet {
    /// The host its plugins are described and built through.
    pub fn host(&self) -> Arc<dyn PluginHost> {
        self.host.clone()
    }

    /// Describe and build plugins through `host` instead of the process's.
    pub fn with_host(mut self, host: Arc<dyn PluginHost>) -> PackageSet {
        self.host = host;
        self
    }

    /// Load the package in `dir` (which holds `m2pkg.toml`).
    pub fn load(&mut self, dir: &Path) -> Result<&Package> {
        let path = dir.join("m2pkg.toml");
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let m: Manifest = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        let name = m.package.name.clone();
        if self.packages.contains_key(&name) {
            bail!("package `{name}` is loaded twice");
        }
        if !m.effect.is_empty() {
            bail!(
                "package `{name}`: `[[effect]]` is gone from m2pkg.toml (PKG-2). Effects are declared in TypeScript, \
                 `component({{ …, editor: {{ category: \"Filter\", insert: {{ … }} }} }}, …)`"
            );
        }
        let mut files = BTreeMap::from([(format!("{name}/m2pkg.toml"), m2_song::Sha::of(text.as_bytes()).hex())]);
        let (module, module_sha) = match &m.package.wasm {
            Some(w) => {
                let wasm_path = dir.join(w);
                let bytes = std::fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
                let sha = m2_song::Sha::of(&bytes);
                files.insert(format!("{name}/{w}"), sha.hex());
                check_abi(&bytes).with_context(|| format!("package `{name}`: {}", wasm_path.display()))?;
                let module = Module::new(&self.engine, &bytes).wctx(|| format!("compiling {}", wasm_path.display()))?;
                verify_module(&self.engine, &module, &m.kernel).with_context(|| format!("package `{name}`: {}", wasm_path.display()))?;
                (Some(module), Some(sha))
            }
            None if !m.kernel.is_empty() => bail!("package `{name}` declares kernels but no `wasm` file"),
            None => (None, None),
        };
        for k in &m.kernel {
            let id = format!("{name}:{}", k.name);
            if k.offline {
                check_offline(&id, k)?;
                self.offline.insert(id, name.clone());
                continue;
            }
            let sig = KernelSig::parse(&id, &k.description, &k.inputs, &k.outputs, k.latency).map_err(anyhow::Error::msg)?;
            self.kernels.insert(id, (sig, name.clone()));
        }
        let pkg = Package {
            name: name.clone(),
            version: m.package.version,
            description: m.package.description,
            dir: dir.to_path_buf(),
            files,
            module,
            module_sha,
        };
        Ok(self.packages.entry(name).or_insert(pkg))
    }

    pub fn get(&self, name: &str) -> Option<&Package> {
        self.packages.get(name)
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.values()
    }

    pub fn kernel_sigs(&self) -> impl Iterator<Item = &KernelSig> {
        self.kernels.values().map(|(s, _)| s)
    }

    /// Look up hosted plugins by graph id (`clap:<plugin id>`), so the checker can type
    /// them. Each is instantiated once to read its ports and parameters. Returns why each one
    /// that isn't installed can't be used.
    pub fn describe_plugins<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut errors = vec![];
        for id in ids {
            if matches!(self.plugins.get(id), Some(Hosted::Installed(..))) {
                continue;
            }
            match describe_plugin(&*self.host, id) {
                Ok((sig, info)) => {
                    self.plugins.insert(id.to_string(), Hosted::Installed(sig, info));
                }
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        errors
    }

    /// An installed plugin's ports and parameters, once described.
    pub fn plugin_info(&self, id: &str) -> Option<&m2_plugins::Info> {
        match self.plugins.get(id)? {
            Hosted::Installed(_, info) => Some(info),
            Hosted::Missing(_) => None,
        }
    }

    /// Whether the last build found `id` missing, so its node plays silence or its input (FMT-18a).
    pub fn plugin_missing(&self, id: &str) -> bool {
        matches!(self.plugins.get(id), Some(Hosted::Missing(_)))
    }

    /// Get the hosted plugins of `ir` ready for the checker, and bind their parameters by id.
    ///
    /// - A plugin that isn't installed (or doesn't load) is `plugin.missing`, which doesn't
    ///   block: its node is typed from the code as written, stereo audio in and out and each
    ///   `<name>_<id>` port a plain control (FMT-18a).
    /// - A parameter is written `<name>_<id>` and bound by the id alone (FMT-18): each port is
    ///   renamed to the plugin's current address for that id, and one whose name part differs
    ///   is the Notice `plugin.param-renamed`. One written without an id is the error
    ///   `plugin.param-unaddressed`, naming the full address.
    ///
    /// Only Errors other than `plugin.missing` block the build.
    pub fn bind_plugins(&mut self, ir: &mut Ir) -> Vec<Diagnostic> {
        let mut diags = vec![];
        // Every port written for each plugin, for the plugins that turn out to be missing.
        let mut written: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut users: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();
        for m in &ir.modules {
            walk(&m.nodes, &mut |n| {
                if let Op::Plugin { plugin, inputs, .. } = &n.op {
                    written.entry(plugin.clone()).or_default().extend(inputs.keys().cloned());
                    users.entry(plugin.clone()).or_default().push((m.key.clone(), n.src.clone()));
                }
            });
        }
        for (id, ports) in &written {
            if !id.starts_with("clap:") {
                // The checker says so: no plugin is ever found by another id.
                continue;
            }
            if let Some(why) = self.describe_plugins([id.as_str()]).pop() {
                self.plugins.insert(id.clone(), Hosted::Missing(missing_sig(id, ports)));
                let (module, src) = users[id][0].clone();
                let nodes = if users[id].len() == 1 { "its node".to_string() } else { format!("each of its {} nodes", users[id].len()) };
                // An effect (its `in` written) passes its input through; an instrument is silent (FMT-18a).
                let plays = if ports.contains("in") { "passes its audio through unchanged" } else { "plays silence" };
                let message = format!("{why}. Until it is installed, {nodes} {plays}; its code and saved state are kept as they are");
                diags.push(Diagnostic::error("plugin.missing", message).module(module).at(src));
            }
        }
        for m in &mut ir.modules {
            let key = m.key.clone();
            walk_mut(&mut m.nodes, &mut |n| {
                let Op::Plugin { plugin, inputs, .. } = &mut n.op else { return };
                let at = |d: Diagnostic| d.module(key.clone()).at(n.src.clone());
                match self.plugins.get(plugin.as_str()) {
                    Some(Hosted::Installed(sig, _)) => {
                        let (bound, found) = bind(plugin, sig, std::mem::take(inputs));
                        *inputs = bound;
                        diags.extend(found.into_iter().map(at));
                    }
                    Some(Hosted::Missing(_)) => {
                        let unaddressed = inputs.keys().filter(|p| !matches!(p.as_str(), "in" | "notes") && address(p).is_none());
                        for p in unaddressed {
                            let message = format!(
                                "`{p}` names a parameter of {plugin} without its id. Write it as `<name>_<id>` (such as `mix_12`); \
                                 `m2 plugins {plugin}` lists the addresses once the plugin is installed"
                            );
                            diags.push(at(Diagnostic::error("plugin.param-unaddressed", message)));
                        }
                    }
                    None => {}
                }
            });
        }
        diags
    }

    /// The SHA-256 of the wasm module holding kernel `id` (an offline one too).
    pub fn module_sha(&self, id: &str) -> Option<m2_song::Sha> {
        let pkg = self.kernels.get(id).map(|(_, p)| p).or_else(|| self.offline.get(id))?;
        self.packages.get(pkg)?.module_sha
    }

    /// The compiled module holding kernel `id` (an offline one too), the kernel's export prefix,
    /// and its package's name.
    pub(crate) fn module_for<'a>(&'a self, id: &'a str) -> Option<(&'a Module, &'a str, &'a str)> {
        let pkg = self.kernels.get(id).map(|(_, p)| p).or_else(|| self.offline.get(id))?;
        let module = self.packages.get(pkg)?.module.as_ref()?;
        Some((module, id.split_once(':').map_or(id, |(_, k)| k), pkg))
    }
}

impl Kernels for PackageSet {
    fn kernel(&self, id: &str) -> Option<&KernelSig> {
        self.kernels.get(id).map(|(s, _)| s)
    }

    fn offline(&self, id: &str) -> bool {
        self.offline.contains_key(id)
    }

    fn plugin(&self, id: &str) -> Option<&KernelSig> {
        match self.plugins.get(id)? {
            Hosted::Installed(sig, _) | Hosted::Missing(sig) => Some(sig),
        }
    }
}

fn walk(ns: &[Node], f: &mut impl FnMut(&Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &n.op {
            walk(&body.nodes, f);
        }
    }
}

fn walk_mut(ns: &mut [Node], f: &mut impl FnMut(&mut Node)) {
    for n in ns {
        f(n);
        if let Op::Poly { body, .. } = &mut n.op {
            walk_mut(&mut body.nodes, f);
        }
    }
}

/// A parameter address `<name>_<id>` (FMT-18), as the plugin host spells it.
pub fn address(port: &str) -> Option<(&str, u32)> {
    m2_plugins::param_address(port)
}

/// The ports written for `id`, by the plugin's current addresses: each `<name>_<id>` renamed to
/// the address of its id. Returns the inputs and what was found on the way.
fn bind(id: &str, sig: &KernelSig, inputs: BTreeMap<String, String>) -> (BTreeMap<String, String>, Vec<Diagnostic>) {
    let by_id: HashMap<u32, &str> = sig.inputs.iter().filter_map(|p| Some((address(&p.name)?.1, p.name.as_str()))).collect();
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut diags = vec![];
    let mut from: HashMap<String, String> = HashMap::new();
    for (port, r) in inputs {
        let current = match address(&port) {
            Some((name, pid)) => match by_id.get(&pid) {
                Some(&now) => {
                    let label = address(now).map_or(now, |(n, _)| n);
                    if label != name {
                        let message = format!(
                            "`{port}` drives {id}'s parameter {pid}, which the plugin now calls `{now}`. It still drives parameter \
                             {pid}; rename it to `{now}` to match"
                        );
                        diags.push(Diagnostic { severity: Severity::Notice, ..Diagnostic::error("plugin.param-renamed", message) });
                    }
                    now.to_string()
                }
                // No such id: the checker names the inputs there are.
                None => port.clone(),
            },
            None => {
                let full: Vec<&str> =
                    sig.inputs.iter().map(|p| p.name.as_str()).filter(|n| address(n).is_some_and(|(l, _)| l == port)).collect();
                if !full.is_empty() {
                    let list = full.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(" or ");
                    let message = format!("`{port}` names a parameter of {id} without its id: write {list}");
                    diags.push(Diagnostic::error("plugin.param-unaddressed", message));
                }
                port.clone()
            }
        };
        if let Some(other) = from.insert(current.clone(), port.clone()) {
            let message = format!("`{other}` and `{port}` both drive {id}'s parameter `{current}`; keep one");
            diags.push(Diagnostic::error("code.check", message));
            continue;
        }
        out.insert(current, r);
    }
    (out, diags)
}

/// The ports a plugin that isn't installed is typed with (FMT-18a): stereo audio in and out,
/// notes, and each parameter address written for it, a plain control.
fn missing_sig(id: &str, written: &BTreeSet<String>) -> KernelSig {
    let port = |name: &str, kind| PortSig { name: name.into(), kind, default: None, range: None };
    let mut inputs = vec![port("in", PortKind::Audio(Some(2))), port("notes", PortKind::Events(m2_ir::EventType::Note))];
    inputs.extend(written.iter().filter(|p| address(p).is_some()).map(|p| port(p, PortKind::Control(Some(m2_ir::Unit::Scalar)))));
    KernelSig {
        id: id.to_string(),
        description: "not installed".into(),
        inputs,
        outputs: vec![port("out", PortKind::Audio(Some(2)))],
        latency: 0,
    }
}

/// A plugin as typed ports: `in: audio<n>`, `notes: events<note>`, one control per
/// parameter (in the plugin's own units, as `scalar`, optional), and `out: audio<n>`. Read from an
/// instance created and activated through the host (on the plugin main thread), then destroyed:
/// activating is what measures its latency (RENDER-21's check).
fn describe_plugin(host: &dyn PluginHost, id: &str) -> Result<(KernelSig, m2_plugins::Info)> {
    let Some(clap_id) = id.strip_prefix("clap:") else { bail!("plugin ids start with `clap:`, not `{id}`") };
    let found = m2_plugins::find(clap_id)?;
    let (h, mut info) = host.create(&found, None, &m2_plugins::Owner::discard())?;
    let latency = host.activate(h, 48_000, 128);
    host.destroy(h);
    info.latency = latency?;
    Ok((plugin_sig(id, &info)?, info))
}

/// The ports of an installed plugin: `in`, `notes`, each parameter by its address (FMT-18), `out`.
fn plugin_sig(id: &str, info: &m2_plugins::Info) -> Result<KernelSig> {
    let mut inputs = vec![];
    if let Some(c) = info.audio_in {
        let kind = m2_ir::PortKind::Audio(Some(c.clamp(1, 2) as u8));
        inputs.push(m2_ir::PortSig { name: "in".into(), kind, default: None, range: None });
    }
    if info.note_in {
        let kind = m2_ir::PortKind::Events(m2_ir::EventType::Note);
        inputs.push(m2_ir::PortSig { name: "notes".into(), kind, default: None, range: None });
    }
    for name in m2_plugins::param_port_names(&info.params) {
        // No default: an unconnected parameter keeps the plugin's own value.
        inputs.push(m2_ir::PortSig { name, kind: m2_ir::PortKind::Control(Some(m2_ir::Unit::Scalar)), default: None, range: None });
    }
    let out = info.audio_out.context("plugins without an audio output aren't supported")?.clamp(1, 2) as u8;
    let sig = KernelSig {
        id: id.to_string(),
        description: format!("{} by {}", info.found.name, info.found.vendor),
        inputs,
        outputs: vec![m2_ir::PortSig { name: "out".into(), kind: m2_ir::PortKind::Audio(Some(out)), default: None, range: None }],
        latency: info.latency,
    };
    Ok(sig)
}

/// An offline kernel's ports, as kernel ABI v1 has them (`docs/kernel-abi.md`, Offline kernels):
/// no inputs, since its source is its asset 0, and one output of audio, each port written as any
/// port is.
fn check_offline(id: &str, k: &KernelDecl) -> Result<()> {
    let outputs = k.outputs.iter().map(|o| m2_ir::PortSig::parse(o).map_err(|e| anyhow::anyhow!("kernel `{id}`: {e}")));
    let outputs = outputs.collect::<Result<Vec<_>>>()?;
    if !k.inputs.is_empty() || k.latency > 0 || !matches!(outputs[..], [PortSig { kind: PortKind::Audio(_), .. }]) {
        bail!("kernel `{id}` is offline: it takes no inputs (its source is its asset 0) and has one audio output");
    }
    Ok(())
}

/// The kernel ABI version this runtime speaks (`docs/kernel-abi.md`). Frozen: v1 only ever gains
/// optional exports and fields at the end of the settings block.
pub const KERNEL_ABI: u32 = 1;

/// The kernel ABI a module was built for: its custom section `m2_abi`, exactly 4 bytes, the
/// version as a little-endian u32 (`docs/kernel-abi.md`, The version rule). Read from the bytes
/// alone, before the module is compiled, so a module built for a later ABI is refused by its
/// version whatever else it holds. `None` when it has no such section: built for v0, or not a
/// music2 kernel. Every later ABI keeps this section.
pub(crate) fn abi_version(bytes: &[u8]) -> Result<Option<u32>> {
    /// A u32 in LEB128, as the binary format writes sizes.
    fn leb(b: &mut &[u8]) -> Result<u32> {
        let mut v = 0u32;
        for shift in (0..35).step_by(7) {
            let Some((&byte, rest)) = b.split_first() else { bail!("it ends inside a section's size") };
            *b = rest;
            v |= ((byte & 0x7f) as u32).checked_shl(shift).unwrap_or(0);
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        bail!("a section's size is longer than 5 bytes")
    }
    // The magic number and the format's version, then sections: an id, a size, the body.
    let Some(mut rest) = bytes.strip_prefix(b"\0asm").and_then(|b| b.get(4..)) else { bail!("it isn't a WebAssembly module") };
    while let Some((&id, after)) = rest.split_first() {
        rest = after;
        let len = leb(&mut rest)? as usize;
        let Some((mut body, after)) = rest.split_at_checked(len) else { bail!("a section runs past the module's end") };
        rest = after;
        if id != 0 {
            continue;
        }
        // A custom section: its name, then its bytes.
        let n = leb(&mut body)? as usize;
        let Some((name, data)) = body.split_at_checked(n) else { bail!("a custom section's name runs past its end") };
        if name == b"m2_abi" {
            let Ok(v) = <[u8; 4]>::try_from(data) else { bail!("its `m2_abi` section holds {} bytes, not 4", data.len()) };
            return Ok(Some(u32::from_le_bytes(v)));
        }
    }
    Ok(None)
}

/// A module that speaks this ABI, by its `m2_abi` section ([`abi_version`]); else why not, and
/// what to do.
fn check_abi(bytes: &[u8]) -> Result<()> {
    match abi_version(bytes)? {
        Some(KERNEL_ABI) => Ok(()),
        Some(v) => bail!(
            "built for kernel ABI v{v}, but this music2 speaks v{KERNEL_ABI} only. Rebuild the package with the v{KERNEL_ABI} kit \
             (docs/kernel-abi.md)"
        ),
        None => bail!(
            "built for kernel ABI v0, or isn't a music2 kernel: it has no `m2_abi` section. Rebuild it with the v{KERNEL_ABI} kit \
             (docs/kernel-abi.md)"
        ),
    }
}

/// A package's module, built for this ABI ([`check_abi`], before it was compiled), checked before
/// any kernel of it is made (ABI-3): its one import, if any, is `m2.asset_read`; its memory is a
/// defined 32-bit memory of 64 KiB pages, not shared, which the hardware checks can guard
/// (RENDER-32); and it has the ABI's exports, with their signatures.
fn verify_module(engine: &Engine, module: &Module, kernels: &[KernelDecl]) -> Result<()> {
    let exports: HashMap<&str, ExternType> = module.exports().map(|e| (e.name(), e.ty())).collect();
    let func = |name: &str, params: &[ValType], results: &[ValType]| -> Result<()> {
        match exports.get(name) {
            Some(ExternType::Func(f)) => {
                let same = |a: &mut dyn ExactSizeIterator<Item = ValType>, b: &[ValType]| {
                    a.len() == b.len() && a.zip(b).all(|(x, y)| ValType::eq(&x, y))
                };
                if same(&mut f.params(), params) && same(&mut f.results(), results) {
                    Ok(())
                } else {
                    bail!("export `{name}` has the wrong signature")
                }
            }
            _ => bail!("missing export `{name}`"),
        }
    };
    match exports.get("memory") {
        Some(ExternType::Memory(m)) if m.is_64() || m.is_shared() || m.page_size() != 1 << 16 => {
            bail!("its memory must be a 32-bit memory of 64 KiB pages, not shared (docs/kernel-abi.md)")
        }
        Some(ExternType::Memory(_)) => {}
        _ => bail!("missing export `memory`"),
    }
    crate::sandbox::instantiate(engine, module)?;
    func("m2_alloc", &[ValType::I32, ValType::I32], &[ValType::I32])?;
    for k in kernels {
        let n = &k.name;
        func(&format!("{n}_ports"), &[], &[ValType::I32])?;
        func(&format!("{n}_new"), &[ValType::I32], &[ValType::I32])?;
        func(&format!("{n}_reset"), &[ValType::I32], &[])?;
        func(&format!("{n}_process"), &[ValType::I32, ValType::I32, ValType::I32, ValType::I32], &[])?;
        func(&format!("{n}_active"), &[ValType::I32], &[ValType::I32])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(effect: &str) -> Result<PackageSet> {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("m2-packages-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = format!("[package]\nname = \"p\"\nversion = \"0.1.0\"\n\n[[effect]]\nmodule = \"p/fx\"\n{effect}");
        std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
        let mut set = PackageSet::default();
        let loaded = set.load(&dir).map(drop);
        std::fs::remove_dir_all(&dir).ok();
        loaded.map(|()| set)
    }

    /// An effect plugin with `params` (id, display name), as if installed.
    fn installed(set: &mut PackageSet, id: &str, params: &[(u32, &str)]) {
        let param = |&(id, name): &(u32, &str)| m2_plugins::Param {
            id,
            name: name.into(),
            module: String::new(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            stepped: false,
        };
        let found = m2_plugins::Found {
            id: id.trim_start_matches("clap:").into(),
            name: "Fx".into(),
            vendor: "Test".into(),
            version: "1.0.0".into(),
            features: vec![],
            bundle: PathBuf::new(),
        };
        let info = m2_plugins::Info {
            found,
            audio_in: Some(2),
            audio_out: Some(2),
            note_in: false,
            params: params.iter().map(param).collect(),
            latency: 0,
        };
        set.plugins.insert(id.into(), Hosted::Installed(plugin_sig(id, &info).unwrap(), info));
    }

    /// One plugin after a sine, with `ports` written as `port: constant`.
    fn with_plugin(id: &str, ports: &[(&str, f64)]) -> Ir {
        let mut nodes = vec![serde_json::json!({"id": "o", "op": "kernel", "kernel": "std:sine", "inputs": {}})];
        let mut inputs = serde_json::json!({"in": "o"});
        for (i, (port, v)) in ports.iter().enumerate() {
            nodes.push(serde_json::json!({"id": format!("c{i}"), "op": "const", "value": v, "origin": {"site": i}}));
            inputs[*port] = serde_json::json!(format!("c{i}"));
        }
        nodes.push(serde_json::json!({"id": "p", "op": "plugin", "plugin": id, "inputs": inputs, "src": "song.ts:3:5"}));
        serde_json::from_value(serde_json::json!({"ir": m2_ir::IR_VERSION, "root": {"module": "song", "export": "out"},
            "modules": [{"key": "song", "nodes": nodes, "exports": {"out": "p"}}]}))
        .unwrap()
    }

    fn std_set() -> PackageSet {
        let mut set = PackageSet::default();
        set.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
        set
    }

    /// The constant feeding each parameter id of the checked plugin node.
    fn bound(set: &PackageSet, ir: &Ir) -> BTreeMap<u32, f64> {
        let flat = m2_ir::check(ir, set).unwrap_or_else(|e| panic!("{e:?}"));
        let node = flat.nodes.iter().find(|n| matches!(n.op, m2_ir::FOp::Plugin { .. })).unwrap();
        let m2_ir::FOp::Plugin { plugin, inputs, .. } = &node.op else { unreachable!() };
        let sig = set.plugin(plugin).unwrap();
        let value = |i: &m2_ir::Input| match i {
            m2_ir::Input::Port { src, .. } => match flat.nodes[src.node].op {
                m2_ir::FOp::Const(v) => v,
                _ => panic!("not a constant"),
            },
            m2_ir::Input::Const(v) => *v,
            m2_ir::Input::Unset => f64::NAN,
        };
        sig.inputs.iter().zip(inputs).filter_map(|(p, i)| Some((address(&p.name)?.1, value(i)))).filter(|(_, v)| !v.is_nan()).collect()
    }

    // FMT-18: a parameter is addressed `<name>_<id>` and bound by the id alone. A plugin build
    // that renames, swaps or adds clashing names leaves every connection on the same id, and
    // says so with a Notice.
    #[test]
    fn parameters_are_bound_by_their_id_whatever_the_plugin_calls_them() {
        let id = "clap:org.example.fx";
        let code = with_plugin(id, &[("mix_12", 0.3), ("drive_13", 0.7)]);
        let builds: [&[(u32, &str)]; 3] =
            [&[(12, "Mix"), (13, "Drive")], &[(13, "Mix"), (12, "Drive")], &[(11, "Mix"), (12, "Drive"), (13, "Mix")]];
        for (n, params) in builds.iter().enumerate() {
            let mut set = std_set();
            installed(&mut set, id, params);
            let mut ir = code.clone();
            let diags = set.bind_plugins(&mut ir);
            assert_eq!(bound(&set, &ir), BTreeMap::from([(12, 0.3), (13, 0.7)]), "build {n}");
            let renamed: Vec<&str> = diags.iter().filter(|d| d.code == "plugin.param-renamed").map(|d| d.message.as_str()).collect();
            assert!(diags.iter().all(|d| d.severity == Severity::Notice && d.src.as_deref() == Some("song.ts:3:5")), "{diags:?}");
            match n {
                0 => assert!(renamed.is_empty(), "{renamed:?}"),
                _ => assert_eq!(renamed.len(), 2, "{renamed:?}"),
            }
        }
        let mut set = std_set();
        installed(&mut set, id, builds[1]);
        let mut ir = code.clone();
        let renamed = set.bind_plugins(&mut ir);
        assert_eq!(
            renamed[0].message,
            "`drive_13` drives clap:org.example.fx's parameter 13, which the plugin now calls `mix_13`. It still drives parameter 13; \
             rename it to `mix_13` to match"
        );
    }

    // FMT-18: a parameter written without its id is `plugin.param-unaddressed`, naming the full address.
    #[test]
    fn a_parameter_without_its_id_names_the_full_address() {
        let id = "clap:org.example.fx";
        let mut set = std_set();
        installed(&mut set, id, &[(12, "Mix"), (13, "Mix"), (14, "Drive")]);
        let mut ir = with_plugin(id, &[("drive", 0.7), ("mix", 0.3)]);
        let diags = set.bind_plugins(&mut ir);
        let found: Vec<(&str, &str)> = diags.iter().map(|d| (d.code, d.message.as_str())).collect();
        assert_eq!(
            found,
            [
                ("plugin.param-unaddressed", "`drive` names a parameter of clap:org.example.fx without its id: write `drive_14`"),
                ("plugin.param-unaddressed", "`mix` names a parameter of clap:org.example.fx without its id: write `mix_12` or `mix_13`"),
            ]
        );
        // Two addresses of one id: one must go.
        let mut ir = with_plugin(id, &[("mix_12", 0.3), ("wet_12", 0.5)]);
        let codes: Vec<&str> = set.bind_plugins(&mut ir).iter().map(|d| d.code).collect();
        assert!(codes.contains(&"code.check"), "{codes:?}");
    }

    // FMT-18a: a plugin that isn't installed doesn't stop the song. It is typed from the code
    // as written, raises `plugin.missing`, and is found missing by the runtime.
    #[test]
    fn a_plugin_that_isnt_installed_is_typed_from_the_code() {
        let mut set = std_set();
        let mut ir = with_plugin("clap:org.example.not-installed", &[("mix_3", 0.5)]);
        let diags = set.bind_plugins(&mut ir);
        assert_eq!(diags.iter().map(|d| (d.code, d.severity)).collect::<Vec<_>>(), [("plugin.missing", Severity::Error)]);
        assert!(
            diags[0].message.ends_with(
                "Until it is installed, its node passes its audio through unchanged; its code and saved state are kept as they are"
            ),
            "{}",
            diags[0].message
        );
        assert!(set.plugin_missing("clap:org.example.not-installed"));
        let flat = m2_ir::check(&ir, &set).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(flat.out_kind, m2_ir::Kind::Audio(2));
        // A port written without an id still can't be typed.
        let mut ir = with_plugin("clap:org.example.not-installed", &[("mix", 0.5)]);
        let codes: Vec<&str> = set.bind_plugins(&mut ir).iter().map(|d| d.code).collect();
        assert_eq!(codes, ["plugin.missing", "plugin.param-unaddressed"]);
    }

    // PKG-2: `m2pkg.toml` describes no TypeScript. A manifest that still has `[[effect]]` fails
    // to load, saying where effects are declared now, rather than losing them silently.
    #[test]
    fn an_effect_entry_fails_to_load() {
        let err = manifest("name = \"wobble\"\ncategory = \"Modulation\"\n").err().unwrap();
        assert!(format!("{err:#}").contains("`[[effect]]` is gone from m2pkg.toml (PKG-2)"), "{err:#}");
    }
}

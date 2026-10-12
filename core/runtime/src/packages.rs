//! Packages: an `m2pkg.toml` manifest, TypeScript modules, and compiled kernels.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use m2_ir::{KernelSig, Kernels};
use serde::Deserialize;
use wasmtime::{Engine, ExternType, Module, ValType};

use crate::WasmContext;

#[derive(Deserialize)]
struct Manifest {
    package: Info,
    #[serde(default)]
    kernel: Vec<KernelDecl>,
    #[serde(default)]
    effect: Vec<Effect>,
}

/// An effect a package offers to put after any audio: a function exported by one of its
/// modules that takes the audio as its first argument and returns audio.
#[derive(Clone, Debug, Deserialize, serde::Serialize, PartialEq)]
pub struct Effect {
    /// The module to import it from, e.g. `std/fx`.
    pub module: String,
    /// The exported function, e.g. `delay`.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Where the editor lists it: one of [`EFFECT_CATEGORIES`].
    #[serde(default = "other")]
    pub category: String,
    /// TypeScript written after the input when the effect is added: `"1000"` gives
    /// `lowpass(x, 1000)`. Each number in it should be a declared control of the effect,
    /// so it shows up as a knob.
    #[serde(default)]
    pub args: Option<String>,
}

/// The categories effects are listed under, in the editor's order.
pub const EFFECT_CATEGORIES: [&str; 8] = ["Filter", "EQ", "Dynamics", "Distortion", "Delay & Reverb", "Modulation", "Utility", "Other"];

fn other() -> String {
    "Other".into()
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
}

pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
    pub effects: Vec<Effect>,
    module: Option<Module>,
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
    pub engine: Engine,
    packages: BTreeMap<String, Package>,
    kernels: HashMap<String, (KernelSig, String)>,
    plugins: HashMap<String, (KernelSig, m2_plugins::Info)>,
}

/// The engine every kernel runs in.
///
/// WebAssembly floating point is bit-exact on every machine except for the bits
/// inside NaNs. Canonicalising those costs about 4.6× in the M1 benchmark, so instead
/// the renderer silences non-finite output (a NaN means a kernel is broken anyway).
/// Relaxed SIMD is off because its results may differ between CPUs.
pub fn engine() -> Engine {
    let mut config = wasmtime::Config::new();
    config.cranelift_nan_canonicalization(false);
    config.wasm_relaxed_simd(false);
    Engine::new(&config).expect("wasmtime engine")
}

impl Default for PackageSet {
    fn default() -> Self {
        PackageSet { engine: engine(), packages: BTreeMap::new(), kernels: HashMap::new(), plugins: HashMap::new() }
    }
}

impl PackageSet {
    /// Load the package in `dir` (which holds `m2pkg.toml`).
    pub fn load(&mut self, dir: &Path) -> Result<&Package> {
        let path = dir.join("m2pkg.toml");
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let m: Manifest = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        let name = m.package.name.clone();
        if self.packages.contains_key(&name) {
            bail!("package `{name}` is loaded twice");
        }
        let module = match &m.package.wasm {
            Some(w) => {
                let wasm_path = dir.join(w);
                let bytes = std::fs::read(&wasm_path).with_context(|| format!("reading {}", wasm_path.display()))?;
                let module = Module::new(&self.engine, &bytes).wctx(|| format!("compiling {}", wasm_path.display()))?;
                verify_module(&self.engine, &module, &m.kernel).with_context(|| format!("package `{name}`: {}", wasm_path.display()))?;
                Some(module)
            }
            None if !m.kernel.is_empty() => bail!("package `{name}` declares kernels but no `wasm` file"),
            None => None,
        };
        for k in &m.kernel {
            let id = format!("{name}:{}", k.name);
            let sig = KernelSig::parse(&id, &k.description, &k.inputs, &k.outputs, k.latency).map_err(anyhow::Error::msg)?;
            self.kernels.insert(id, (sig, name.clone()));
        }
        for e in &m.effect {
            if !e.module.strip_prefix(&name).is_some_and(|sub| sub.starts_with('/')) {
                bail!("package `{name}`: effect `{}` must come from one of its own modules, not `{}`", e.name, e.module);
            }
            if !EFFECT_CATEGORIES.contains(&e.category.as_str()) {
                bail!("package `{name}`: effect `{}` has category `{}`; use one of {}", e.name, e.category, EFFECT_CATEGORIES.join(", "));
            }
        }
        let pkg = Package {
            name: name.clone(),
            version: m.package.version,
            description: m.package.description,
            dir: dir.to_path_buf(),
            effects: m.effect,
            module,
        };
        Ok(self.packages.entry(name).or_insert(pkg))
    }

    pub fn get(&self, name: &str) -> Option<&Package> {
        self.packages.get(name)
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.values()
    }

    /// Every effect the loaded packages offer.
    pub fn effects(&self) -> impl Iterator<Item = &Effect> {
        self.packages.values().flat_map(|p| &p.effects)
    }

    pub fn kernel_sigs(&self) -> impl Iterator<Item = &KernelSig> {
        self.kernels.values().map(|(s, _)| s)
    }

    /// Look up hosted plugins by graph id (`clap:<plugin id>`), so the checker can type
    /// them. Each is instantiated once to read its ports and parameters.
    pub fn describe_plugins<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let mut errors = vec![];
        for id in ids {
            if self.plugins.contains_key(id) {
                continue;
            }
            match describe_plugin(id) {
                Ok(d) => {
                    self.plugins.insert(id.to_string(), d);
                }
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        errors
    }

    pub fn plugin_info(&self, id: &str) -> Option<&m2_plugins::Info> {
        self.plugins.get(id).map(|(_, i)| i)
    }

    /// The compiled module holding kernel `id`, and the kernel's export prefix.
    pub(crate) fn module_for<'a>(&'a self, id: &'a str) -> Option<(&'a Module, &'a str)> {
        let (_, pkg) = self.kernels.get(id)?;
        let module = self.packages.get(pkg)?.module.as_ref()?;
        Some((module, id.split_once(':').map_or(id, |(_, k)| k)))
    }
}

impl Kernels for PackageSet {
    fn kernel(&self, id: &str) -> Option<&KernelSig> {
        self.kernels.get(id).map(|(s, _)| s)
    }

    fn plugin(&self, id: &str) -> Option<&KernelSig> {
        self.plugins.get(id).map(|(s, _)| s)
    }
}

/// A plugin as typed ports: `in: audio<n>`, `notes: events<note>`, one control per
/// parameter (in the plugin's own units, as `scalar`, optional), and `out: audio<n>`.
fn describe_plugin(id: &str) -> Result<(KernelSig, m2_plugins::Info)> {
    let Some(clap_id) = id.strip_prefix("clap:") else { bail!("plugin ids start with `clap:`, not `{id}`") };
    let found = m2_plugins::find(clap_id)?;
    let info = m2_plugins::Instance::new(&found, 48_000.0, 128)?.info.clone();
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
    Ok((sig, info))
}

/// The kernel ABI version this runtime speaks (see `m2-kernel`).
pub const KERNEL_ABI: u32 = 0;

/// Kernels are sandboxed: no imports at all, the ABI exports, and the right ABI version.
fn verify_module(engine: &Engine, module: &Module, kernels: &[KernelDecl]) -> Result<()> {
    if let Some(i) = module.imports().next() {
        bail!("kernels can't import anything, but this module imports `{}::{}`", i.module(), i.name());
    }
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
    func("m2_abi", &[], &[ValType::I32])?;
    if !matches!(exports.get("memory"), Some(ExternType::Memory(_))) {
        bail!("missing export `memory`");
    }
    let mut store = wasmtime::Store::new(engine, ());
    let instance = wasmtime::Instance::new(&mut store, module, &[]).wctx(|| "instantiating")?;
    let abi = instance.get_typed_func::<(), u32>(&mut store, "m2_abi")?.call(&mut store, ())?;
    if abi != KERNEL_ABI {
        bail!("built for kernel ABI v{abi}, but this runtime speaks v{KERNEL_ABI}");
    }
    for k in kernels {
        let n = &k.name;
        func(&format!("{n}_init"), &[ValType::F32, ValType::I32, ValType::I32], &[])?;
        func(&format!("{n}_ports"), &[], &[ValType::I32])?;
        func(&format!("{n}_port"), &[ValType::I32], &[ValType::I32])?;
        func(&format!("{n}_reset"), &[], &[])?;
        func(&format!("{n}_process"), &[ValType::I32], &[])?;
        func(&format!("{n}_active"), &[], &[ValType::I32])?;
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

    #[test]
    fn effects_have_a_known_category_and_optional_args() {
        let set = manifest("name = \"wobble\"\n").unwrap();
        let e = set.effects().next().unwrap();
        assert_eq!((e.category.as_str(), e.args.as_deref()), ("Other", None));
        let set = manifest("name = \"wobble\"\ncategory = \"Modulation\"\nargs = \"{ rate: 2 }\"\n").unwrap();
        let e = set.effects().next().unwrap();
        assert_eq!((e.category.as_str(), e.args.as_deref()), ("Modulation", Some("{ rate: 2 }")));
        let err = manifest("name = \"wobble\"\ncategory = \"Wobbles\"\n").err().unwrap();
        assert!(format!("{err:#}").contains("has category `Wobbles`; use one of Filter, EQ"), "{err:#}");
    }
}

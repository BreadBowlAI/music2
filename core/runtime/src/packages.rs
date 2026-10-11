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
    module: Option<Module>,
}

impl Package {
    /// The TypeScript entry for `name/<sub>` imports.
    pub fn source(&self, sub: &str) -> PathBuf {
        self.dir.join("src").join(format!("{sub}.ts"))
    }
}

/// Every package a song can use, and the kernels they declare.
pub struct PackageSet {
    pub engine: Engine,
    packages: BTreeMap<String, Package>,
    kernels: HashMap<String, (KernelSig, String)>,
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
        PackageSet { engine: engine(), packages: BTreeMap::new(), kernels: HashMap::new() }
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
                verify_module(&module, &m.kernel).with_context(|| format!("package `{name}`: {}", wasm_path.display()))?;
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
        let pkg = Package { name: name.clone(), version: m.package.version, description: m.package.description, dir: dir.to_path_buf(), module };
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
}

/// Kernels are sandboxed: no imports at all, and exactly the ABI exports.
fn verify_module(module: &Module, kernels: &[KernelDecl]) -> Result<()> {
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
    for k in kernels {
        let n = &k.name;
        func(&format!("{n}_init"), &[ValType::F32, ValType::I32, ValType::I32], &[])?;
        func(&format!("{n}_ports"), &[], &[ValType::I32])?;
        func(&format!("{n}_port"), &[ValType::I32], &[ValType::I32])?;
        func(&format!("{n}_reset"), &[], &[])?;
        func(&format!("{n}_process"), &[ValType::I32], &[])?;
    }
    Ok(())
}

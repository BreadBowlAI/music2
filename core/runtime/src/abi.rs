//! What a kernel module declares, read from its bytes (ABI-2, ABI-3): its imports, its memories
//! and its exports, with their signatures. The checks of ABI-3 read these, not a compiled module,
//! so every kernel host ([`crate::host`]) refuses a module in the same words.

use anyhow::{Result, bail};

use crate::sandbox::ASSET_READ;

/// The calls of kernel ABI v1, by their shape: every argument and every result an `i32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    /// `m2_alloc(size, align) -> address`.
    Alloc,
    /// `k_ports() -> inputs << 8 | outputs`.
    Ports,
    /// `k_new(settings) -> state`.
    New,
    /// `k_reset(state)`.
    Reset,
    /// `k_process(state, io, from, to)`.
    Process,
    /// `k_active(state) -> 0 or 1`.
    Active,
}

impl Call {
    /// Its `i32` arguments and results.
    pub(crate) fn arity(self) -> (usize, usize) {
        match self {
            Call::Alloc => (2, 1),
            Call::Ports => (0, 1),
            Call::New | Call::Active => (1, 1),
            Call::Reset => (1, 0),
            Call::Process => (4, 0),
        }
    }
}

/// The value type `i32`, as the binary format writes it: the only one the ABI's calls use.
const I32: u8 = 0x7f;

/// A function's signature: its parameters' and results' value types, one byte each (a reference
/// type of the GC proposal as its first byte only: it is never an `i32`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Sig {
    params: Vec<u8>,
    results: Vec<u8>,
}

impl Sig {
    /// Whether it takes `params` `i32`s and gives `results`.
    fn is(&self, (params, results): (usize, usize)) -> bool {
        let i32s = |v: &[u8], n: usize| v.len() == n && v.iter().all(|&t| t == I32);
        i32s(&self.params, params) && i32s(&self.results, results)
    }
}

/// A memory's type, as much of it as the checks ask.
#[derive(Clone, Copy, Debug)]
struct Mem {
    is_64: bool,
    shared: bool,
    /// Its page size, as a power of two: 16 unless the custom page sizes proposal says.
    page_log2: u32,
}

/// What an import or an export is: a function or a memory, by its index (imports first), or
/// something else.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Func(u32),
    Memory(u32),
    Other,
}

/// What a module declares, read from its bytes.
pub(crate) struct Declared {
    /// Its imports, in order: module, name and kind.
    imports: Vec<(String, String, Kind)>,
    /// Every function's signature, the imported first (the function index space).
    funcs: Vec<Option<Sig>>,
    /// Every memory's type, the imported first (the memory index space).
    memories: Vec<Mem>,
    /// Its exports, by name.
    exports: std::collections::HashMap<String, Kind>,
}

impl Declared {
    /// Read `bytes`, a module the host compiled (so a valid one): its type, import, function,
    /// memory and export sections. A module this can't read is refused, saying why.
    pub(crate) fn read(bytes: &[u8]) -> Result<Declared> {
        let Some(mut rest) = bytes.strip_prefix(b"\0asm").and_then(|b| b.get(4..)) else { bail!("it isn't a WebAssembly module") };
        let mut types: Vec<Option<Sig>> = vec![];
        let mut d = Declared { imports: vec![], funcs: vec![], memories: vec![], exports: Default::default() };
        while let Some((&id, after)) = rest.split_first() {
            let mut r = Reader(after);
            let len = r.u32()? as usize;
            let Some((body, after)) = r.0.split_at_checked(len) else { bail!("a section runs past the module's end") };
            rest = after;
            let mut r = Reader(body);
            match id {
                1 => {
                    for _ in 0..r.u32()? {
                        r.rec(&mut types)?;
                    }
                }
                2 => {
                    for _ in 0..r.u32()? {
                        let (module, name) = (r.name()?, r.name()?);
                        let kind = match r.byte()? {
                            0x00 => {
                                let t = r.u32()?;
                                d.funcs.push(types.get(t as usize).cloned().flatten());
                                Kind::Func(d.funcs.len() as u32 - 1)
                            }
                            0x01 => {
                                r.val()?;
                                r.limits()?;
                                Kind::Other
                            }
                            0x02 => {
                                d.memories.push(r.memory()?);
                                Kind::Memory(d.memories.len() as u32 - 1)
                            }
                            0x03 => {
                                r.val()?;
                                r.byte()?;
                                Kind::Other
                            }
                            0x04 => {
                                r.byte()?;
                                r.u32()?;
                                Kind::Other
                            }
                            k => bail!("it imports a kind of thing music2 can't read ({k:#04x})"),
                        };
                        d.imports.push((module, name, kind));
                    }
                }
                3 => {
                    for _ in 0..r.u32()? {
                        let t = r.u32()?;
                        d.funcs.push(types.get(t as usize).cloned().flatten());
                    }
                }
                5 => {
                    for _ in 0..r.u32()? {
                        d.memories.push(r.memory()?);
                    }
                }
                7 => {
                    for _ in 0..r.u32()? {
                        let name = r.name()?;
                        let kind = match (r.byte()?, r.u32()?) {
                            (0x00, i) => Kind::Func(i),
                            (0x02, i) => Kind::Memory(i),
                            _ => Kind::Other,
                        };
                        d.exports.insert(name, kind);
                    }
                }
                _ => {}
            }
        }
        Ok(d)
    }

    /// Its memory, the export `memory`: a 32-bit memory of 64 KiB pages, not shared, which the
    /// hardware checks can guard (RENDER-32).
    pub(crate) fn memory(&self) -> Result<()> {
        match self.exports.get("memory") {
            Some(&Kind::Memory(i)) => match self.memories.get(i as usize) {
                Some(m) if m.is_64 || m.shared || m.page_log2 != 16 => {
                    bail!("its memory must be a 32-bit memory of 64 KiB pages, not shared (docs/kernel-abi.md)")
                }
                Some(_) => Ok(()),
                None => bail!("missing export `memory`"),
            },
            _ => bail!("missing export `memory`"),
        }
    }

    /// Its imports: only [`ASSET_READ`], a function of its signature (ABI-2). Anything else is
    /// refused, naming it.
    pub(crate) fn imports(&self) -> Result<()> {
        for (module, name, kind) in &self.imports {
            if (module.as_str(), name.as_str()) != ASSET_READ {
                bail!("a kernel may import only `m2.asset_read`, but this module imports `{module}.{name}`");
            }
            let &Kind::Func(f) = kind else { bail!("`m2.asset_read` must be imported as a function") };
            if !self.funcs.get(f as usize).and_then(Option::as_ref).is_some_and(|s| s.is((5, 1))) {
                bail!("`m2.asset_read` has the wrong signature: it is (i32, i32, i32, i32, i32) -> i32 (docs/kernel-abi.md)");
            }
        }
        Ok(())
    }

    /// Its exports: `m2_alloc`, and each kernel's calls, named `<kernel>_<call>`, each a function
    /// of its shape ([`Call`]).
    pub(crate) fn exports<'a>(&self, kernels: impl IntoIterator<Item = &'a str>) -> Result<()> {
        self.func("m2_alloc", Call::Alloc)?;
        for k in kernels {
            for (call, shape) in
                [("ports", Call::Ports), ("new", Call::New), ("reset", Call::Reset), ("process", Call::Process), ("active", Call::Active)]
            {
                self.func(&format!("{k}_{call}"), shape)?;
            }
        }
        Ok(())
    }

    /// Export `name`: a function of `shape`.
    fn func(&self, name: &str, shape: Call) -> Result<()> {
        match self.exports.get(name) {
            Some(&Kind::Func(i)) => match self.funcs.get(i as usize) {
                Some(Some(sig)) if sig.is(shape.arity()) => Ok(()),
                Some(_) => bail!("export `{name}` has the wrong signature"),
                None => bail!("missing export `{name}`"),
            },
            _ => bail!("missing export `{name}`"),
        }
    }
}

/// The bytes of a section, read from the front.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8> {
        let Some((&b, rest)) = self.0.split_first() else { bail!("it ends inside a section") };
        self.0 = rest;
        Ok(b)
    }

    /// An unsigned LEB128 number of up to `bits` bits.
    fn leb(&mut self, bits: u32) -> Result<u64> {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64).checked_shl(shift).unwrap_or(0);
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
            if shift >= bits.next_multiple_of(7) {
                bail!("a number in it is longer than {bits} bits");
            }
        }
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(self.leb(32)? as u32)
    }

    fn name(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        let Some((name, rest)) = self.0.split_at_checked(n) else { bail!("a name runs past its section's end") };
        self.0 = rest;
        Ok(String::from_utf8_lossy(name).into_owned())
    }

    /// A value type: its first byte. A reference type of the GC proposal (`ref`, `ref null`)
    /// is followed by its heap type, a signed number.
    /// Proposals this build's engines leave off (shared-everything's 0x65, exact heap types'
    /// 0x62) are refused, never misread.
    fn val(&mut self) -> Result<u8> {
        let t = self.byte()?;
        if matches!(t, 0x63 | 0x64) {
            if let Some(&h @ (0x62 | 0x65)) = self.0.first() {
                bail!("its types use a heap type music2 can't read ({h:#04x})");
            }
            self.leb(33)?;
        }
        Ok(t)
    }

    /// A list of value types.
    fn vals(&mut self) -> Result<Vec<u8>> {
        (0..self.u32()?).map(|_| self.val()).collect()
    }

    /// A limits' flags, its minimum and its maximum when it has one. Each number is read as
    /// wasmtime reads it, up to 64 bits in a padded form: wasmtime has checked its range.
    fn limits(&mut self) -> Result<u8> {
        let flags = self.byte()?;
        self.leb(64)?;
        if flags & 0x01 != 0 {
            self.leb(64)?;
        }
        Ok(flags)
    }

    /// A memory's type: its limits, and its page size when the flags give one.
    fn memory(&mut self) -> Result<Mem> {
        let flags = self.limits()?;
        let page_log2 = if flags & 0x08 != 0 { self.u32()? } else { 16 };
        Ok(Mem { is_64: flags & 0x04 != 0, shared: flags & 0x02 != 0, page_log2 })
    }

    /// One entry of the type section: a function type, or a recursive group or subtype of the GC
    /// proposal, each type in it given its index. A struct's or an array's type is no function's.
    fn rec(&mut self, types: &mut Vec<Option<Sig>>) -> Result<()> {
        match self.byte()? {
            0x4e => {
                for _ in 0..self.u32()? {
                    let form = self.byte()?;
                    self.sub(form, types)?;
                }
                Ok(())
            }
            form => self.sub(form, types),
        }
    }

    /// A subtype whose first byte is `form`.
    fn sub(&mut self, form: u8, types: &mut Vec<Option<Sig>>) -> Result<()> {
        let form = match form {
            0x50 | 0x4f => {
                for _ in 0..self.u32()? {
                    self.u32()?;
                }
                self.byte()?
            }
            f => f,
        };
        let sig = match form {
            0x60 => Some(Sig { params: self.vals()?, results: self.vals()? }),
            0x5f => {
                for _ in 0..self.u32()? {
                    self.field()?;
                }
                None
            }
            0x5e => {
                self.field()?;
                None
            }
            f => bail!("its type section holds a type music2 can't read ({f:#04x})"),
        };
        types.push(sig);
        Ok(())
    }

    /// A struct's or an array's field: its storage type (a value type, or packed `i8` or `i16`)
    /// and whether it may change.
    fn field(&mut self) -> Result<()> {
        if !matches!(self.0.first(), Some(0x78 | 0x77)) {
            self.val()?;
        } else {
            self.byte()?;
        }
        self.byte()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{kernel_v1, module_v1, out_v1, wasm};

    // ABI-3, review of track kernel-host: a limit's numbers are read as wasmtime reads them, up
    // to 64 bits in a padded form (10 bytes), as the old check through wasmtime took them; and
    // encodings of proposals this build's engines leave off (shared-everything's 0x65, exact heap
    // types' 0x62) are refused in words, never misread.
    #[test]
    fn a_limit_reads_as_wasmtime_reads_it_and_off_proposals_are_refused() {
        // A 32-bit memory's minimum of 1, padded to 6 bytes (0x81 0x80 0x80 0x80 0x80 0x00).
        let mut r = Reader(&[0x00, 0x81, 0x80, 0x80, 0x80, 0x80, 0x00]);
        assert!(!r.memory().unwrap().is_64);
        assert!(r.0.is_empty(), "the whole limit is read");
        for heap in [0x62u8, 0x65] {
            let mut r = Reader(&[0x63, heap, 0x70]);
            let why = format!("{:#}", r.val().expect_err("refused"));
            assert!(why.contains("music2 can't read"), "{heap:#04x}: {why}");
        }
    }

    /// ABI-3's checks of `wat`'s module, from its bytes alone, in the loader's order.
    fn check(wat: &str) -> Result<()> {
        let d = Declared::read(&wasm(wat))?;
        d.memory()?;
        d.imports()?;
        d.exports(["k"])
    }

    /// Why `wat`'s module is refused.
    fn refused(wat: &str) -> String {
        format!("{:#}", check(wat).expect_err(wat))
    }

    // ABI-2, ABI-3: the checks of a module's imports, memory and exports read its bytes alone: no
    // kernel host compiles anything here. They refuse what the loader refused through wasmtime's
    // compiled module, in the same words: another import, `m2.asset_read` as anything but a
    // function of its signature, a memory a sandbox can't hold, a missing export, and an export
    // with the wrong signature, counting functions and memories a module imports first.
    #[test]
    fn abi3s_checks_read_the_modules_bytes_alone() {
        let copy = kernel_v1("k", &out_v1("(local.get $x)"));
        let ok = module_v1(&copy);
        check(&ok).unwrap();
        let import = |i: &str| ok.replacen("(module", &format!("(module {i}"), 1);
        check(&import(r#"(import "m2" "asset_read" (func (param i32 i32 i32 i32 i32) (result i32)))"#)).unwrap();
        let only = "a kernel may import only `m2.asset_read`, but this module imports `env.clock`";
        assert_eq!(refused(&import(r#"(import "env" "clock" (func (result f64)))"#)), only);
        assert_eq!(refused(&import(r#"(import "m2" "asset_read" (global i32))"#)), "`m2.asset_read` must be imported as a function");
        let wrong = "`m2.asset_read` has the wrong signature: it is (i32, i32, i32, i32, i32) -> i32 (docs/kernel-abi.md)";
        assert_eq!(refused(&import(r#"(import "m2" "asset_read" (func (param i32) (result i32)))"#)), wrong);
        assert_eq!(refused(&import(r#"(import "m2" "asset_read" (func (param i32 i32 i32 i32 i32)))"#)), wrong);

        let memory = "its memory must be a 32-bit memory of 64 KiB pages, not shared (docs/kernel-abi.md)";
        let with = |m: &str| ok.replace(r#"(memory (export "memory") 1)"#, m);
        for m in [
            r#"(memory (export "memory") i64 1)"#,
            r#"(memory (export "memory") 1 2 shared)"#,
            r#"(memory (export "memory") 1 (pagesize 1))"#,
        ] {
            assert_eq!(refused(&with(m)), memory, "{m}");
        }
        check(&with(r#"(memory (export "memory") 1 (pagesize 65536))"#)).unwrap();
        assert_eq!(refused(&with("(memory 1)")), "missing export `memory`");
        assert_eq!(refused(&with(r#"(memory 1) (func (export "memory"))"#)), "missing export `memory`");
        // An imported memory, exported: its type is the import's. (It is refused as an import next.)
        let imported = with(r#"(import "m2" "memory" (memory i64 1)) (export "memory" (memory 0))"#);
        assert_eq!(refused(&imported), memory);
        let imported = with(r#"(import "m2" "memory" (memory 1)) (export "memory" (memory 0))"#);
        assert_eq!(refused(&imported), "a kernel may import only `m2.asset_read`, but this module imports `m2.memory`");

        let missing = module_v1(&copy.replace(r#"(export "k_ports")"#, ""));
        assert_eq!(refused(&missing), "missing export `k_ports`");
        let global = module_v1(&copy.replace(r#"(func (export "k_new")"#, r#"(global (export "k_new") i32 (i32.const 0)) (func"#));
        assert_eq!(refused(&global), "missing export `k_new`");
        let process = module_v1(&copy.replace(r#"(export "k_process")"#, "")).replacen(
            "(module",
            r#"(module (func (export "k_process") (param i32))"#,
            1,
        );
        assert_eq!(refused(&process), "export `k_process` has the wrong signature");
        let reset = module_v1(&copy.replace(
            r#"(func (export "k_reset") (param i32) (f32.store (i32.const 4096) (f32.const 0)))"#,
            r#"(func (export "k_reset") (param i32) (result i32) (i32.const 0))"#,
        ));
        assert_eq!(refused(&reset), "export `k_reset` has the wrong signature");
        // A function index counts the imported functions first: here `m2_alloc` is the import.
        let alloc = import(r#"(import "m2" "asset_read" (func $read (param i32 i32 i32 i32 i32) (result i32)))"#)
            .replace(r#"(func (export "m2_alloc")"#, r#"(export "m2_alloc" (func $read)) (func"#);
        assert_eq!(refused(&alloc), "export `m2_alloc` has the wrong signature");
    }

    // ABI-3: every module music2 ships passes the checks from its bytes: std's and each pack's.
    #[test]
    fn every_module_music2_ships_passes_abi3_from_its_bytes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut dirs = vec![root.join("std")];
        dirs.extend(m2_song::pack::bundled(&root.join("packs")));
        let mut checked = 0;
        for dir in dirs {
            let m = m2_song::pack::PkgManifest::at(&dir).unwrap();
            let Some(w) = &m.package.wasm else { continue };
            let bytes = std::fs::read(dir.join(w)).unwrap();
            let d = Declared::read(&bytes).unwrap();
            let names: Vec<String> = m.kernel.iter().map(|k| k["name"].as_str().unwrap().to_string()).collect();
            let named = || names.iter().map(String::as_str);
            d.memory().and_then(|()| d.imports()).and_then(|()| d.exports(named())).unwrap_or_else(|e| panic!("{}: {e:#}", dir.display()));
            // And a kernel it hasn't is missing.
            let e = d.exports(named().chain(["nothing"])).unwrap_err();
            assert_eq!(format!("{e:#}"), "missing export `nothing_ports`", "{}", dir.display());
            checked += 1;
        }
        assert_eq!(checked, 8, "std's module and the 7 packs'");
    }
}

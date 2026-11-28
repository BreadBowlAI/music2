//! The web engine's probe (`docs/designs/live/web-engine.md`): every live kernel music2 ships,
//! run under wasmtime on fixed inputs, as `host.mjs` runs them under a browser's WebAssembly.
//!
//! `cargo run --locked --release -p m2-runtime --example web-probe -- <out dir> [--flush] [--normal]`
//! (`-- <out dir> --libm` writes `libm.json` instead: what song code's `Math` gives in the app;
//! `M2_PROBE_DUMP=std:svf` also writes that kernel's output bytes, `std-svf.bin`)
//!
//! It writes `cases.json` (each kernel's ports and what each input gets, for `host.mjs`) and
//! `wasmtime.json` (or `wasmtime-flush.json`): each kernel's sha256 of its outputs, block by block,
//! the sha256 of its inputs, and its time per 128-frame block. The engine is the runtime's own,
//! `m2_runtime::packages::engine(false)`: std's, with no time checks. `--flush` runs with
//! flush-to-zero on, as a thread whose owner set it would (RENDER-2a says the app never does).
//! `--normal` feeds no subnormal inputs: any subnormal then is one a kernel made itself.
//!
//! A state's block is laid out as `core/runtime/src/sandbox.rs` lays it out, with no values, no
//! assets and no time map. `m2.asset_read` reads silence. Nothing here is the app's code path: it
//! is the kernel ABI alone (`docs/kernel-abi.md`), the same bytes in both hosts.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use wasmtime::{Caller, Func, Instance, Memory, Module, Store, TypedFunc};

const BLOCK: usize = 128;
/// 2 s at 48 kHz.
const BLOCKS: usize = 750;
const PORT_BYTES: usize = 2 * BLOCK * 4;
const NOTES_HEAD: usize = 16;
const EVENT_BYTES: usize = 24;
const SETTINGS_BYTES: usize = 60;
/// Room for the notes of one block, in an events input.
const NOTE_CAP: usize = 16;
const RATE: f32 = 48_000.0;
/// What `audio<*>` resolves to: stereo.
const AUDIO: u32 = 2;

/// The packages whose modules run: std, then each of music2's own packs with kernels.
const PACKAGES: [&str; 8] = [
    "std",
    "packs/@m2/drums",
    "packs/@m2/effects",
    "packs/@m2/mixing",
    "packs/@m2/notes",
    "packs/@m2/synths",
    "packs/@m2/vocals",
    "packs/@m2/wavetable",
];

/// One port: what it is, its channels, and for an input what it is fed (`fed`, `bits`).
#[derive(Clone)]
struct Port {
    name: String,
    input: bool,
    kind: &'static str,
    channels: u32,
    fed: &'static str,
    bits: u32,
}

/// `name: kind = default in lo..hi`, as an `m2pkg.toml` writes a port.
fn port(text: &str, input: bool) -> Result<Port> {
    let (name, rest) = text.split_once(": ").with_context(|| format!("port `{text}`"))?;
    let kind = rest.split(" = ").next().unwrap_or(rest).trim();
    let default = rest.rsplit_once('>').map_or(rest.split_once(" = ").map(|x| x.1), |(_, after)| after.strip_prefix(" = "));
    let default: Option<f32> = default.and_then(|d| d.split(" in ").next()).and_then(|d| d.trim().parse::<f64>().ok()).map(|d| d as f32);
    let (kind, channels) = if kind.starts_with("events") {
        ("events", 1)
    } else if kind.starts_with("control") {
        ("control", 1)
    } else if kind == "audio<1>" {
        ("audio", 1)
    } else if kind == "audio<2>" {
        ("audio", 2)
    } else if kind == "audio<*>" || kind == "*" {
        ("audio", AUDIO)
    } else {
        bail!("port `{text}`: kind `{kind}`")
    };
    let unit = rest.split('<').nth(1).and_then(|u| u.split('>').next()).unwrap_or("");
    let (fed, value) = match (kind, input) {
        (_, false) => ("", 0.0),
        ("events", _) => ("notes", 0.0),
        ("audio", _) if name == "hits" => ("hits", 0.0),
        ("audio", _) => ("noise", 0.0),
        _ if name == "gate" || name.starts_with("trig") => ("gate", 0.0),
        _ => match (default, unit) {
            (Some(d), _) => ("const", d),
            (None, "hz") => ("sweep", 0.0),
            (None, "beats") => ("beats", 0.0),
            (None, "db") => ("const", -6.0),
            (None, "unit01" | "*") => ("const", 0.5),
            (None, "semitones") => ("const", 60.0),
            (None, "seconds") => ("const", 0.1),
            (None, "bpm") => ("const", 120.0),
            (None, "bipolar") => ("const", 0.25),
            (None, _) => ("const", 1.0),
        },
    };
    Ok(Port { name: name.into(), input, kind, channels, fed, bits: value.to_bits() })
}

/// One kernel to run.
struct Case {
    package: String,
    wasm: PathBuf,
    kernel: String,
    ports: Vec<Port>,
    fan: usize,
    /// Whether noise inputs end in subnormals.
    subnormals: bool,
}

fn cases(root: &Path, subnormals: bool) -> Result<Vec<Case>> {
    let mut out = vec![];
    for p in PACKAGES {
        let text = std::fs::read_to_string(root.join(p).join("m2pkg.toml"))?;
        let m: toml::Table = toml::from_str(&text)?;
        let pkg = m.get("package").and_then(|v| v.as_table()).context("[package]")?;
        let wasm = root.join(p).join(pkg.get("wasm").and_then(|v| v.as_str()).context("wasm")?);
        let package = pkg.get("name").and_then(|v| v.as_str()).unwrap_or(p).to_string();
        for k in m.get("kernel").and_then(|k| k.as_array()).into_iter().flatten() {
            if k.get("offline").and_then(|o| o.as_bool()) == Some(true) {
                continue;
            }
            let list = |key: &str| -> Vec<String> {
                k.get(key).and_then(|v| v.as_array()).map_or(vec![], |a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            };
            let mut ports = vec![];
            for i in list("inputs") {
                ports.push(port(&i, true)?);
            }
            for o in list("outputs").into_iter().chain(list("meters")) {
                ports.push(port(&o, false)?);
            }
            let fan = k.get("fan").and_then(|f| f.as_integer()).unwrap_or(1) as usize;
            out.push(Case {
                package: package.clone(),
                wasm: wasm.clone(),
                kernel: k["name"].as_str().unwrap().into(),
                ports,
                fan,
                subnormals,
            });
        }
    }
    Ok(out)
}

/// The stream every noise input reads: one per port channel.
struct Lcg(u32);

impl Lcg {
    fn new(port: usize, ch: usize) -> Lcg {
        Lcg(((port * 16 + ch + 1) as u32).wrapping_mul(2_654_435_761))
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// Frame `f`'s sample of a noise input: half-scale noise, then silence (a tail), then subnormals
/// (or more silence, with `--normal`).
fn noise(rng: &mut Lcg, block: usize, subnormals: bool) -> u32 {
    let x = rng.next();
    if block < 300 {
        (((x >> 8) as f32 / 8_388_608.0 - 1.0) * 0.5).to_bits()
    } else if block < 550 || !subnormals {
        0
    } else {
        (x >> 9) | ((x & 1) << 31)
    }
}

fn gate(block: usize) -> u32 {
    if (20..200).contains(&block) || (320..340).contains(&block) { 1.0f32.to_bits() } else { 0 }
}

/// A drum lane's hits: 1 on the first frame of every 50th block.
fn hits(frame: usize) -> u32 {
    if frame % (50 * BLOCK) == 10 * BLOCK { 1.0f32.to_bits() } else { 0 }
}

/// The song's beat at 120 bpm.
fn beats(frame: usize) -> u32 {
    ((frame as f64 / 24_000.0) as f32).to_bits()
}

fn sweep(frame: usize) -> u32 {
    (50.0 + (frame % 48_000) as f32 * 0.25).to_bits()
}

/// The notes every events input gets: (block, frame, kind, id, three values' bits).
const NAN: u32 = 0x7fc0_0000;
fn notes() -> [(usize, u32, u32, u32, [u32; 3]); 6] {
    let f = |v: f32| v.to_bits();
    [
        (10, 5, 0, 1, [f(60.0), f(0.75), NAN]),
        (100, 17, 1, 1, [f(0.5), 0, 0]),
        (120, 0, 0, 2, [f(67.0), f(0.5), NAN]),
        (121, 64, 0, 3, [f(71.0), f(1.0), NAN]),
        (260, 127, 1, 2, [0, 0, 0]),
        (261, 0, 1, 3, [0, 0, 0]),
    ]
}

/// A kernel's state, made as the host makes one.
struct Run {
    store: Store<()>,
    memory: Memory,
    process: TypedFunc<(u32, u32, u32, u32), ()>,
    reset: TypedFunc<u32, ()>,
    active: TypedFunc<u32, u32>,
    handle: u32,
    io: u32,
    /// Each port's buffer, and an events port's room.
    bufs: Vec<usize>,
    caps: Vec<usize>,
}

fn birth(module: &Module, c: &Case) -> Result<Run> {
    let mut store = Store::new(module.engine(), ());
    let mut imports = vec![];
    for i in module.imports() {
        if (i.module(), i.name()) != ("m2", "asset_read") {
            bail!("imports {}.{}", i.module(), i.name());
        }
        let read = |mut caller: Caller<'_, ()>, _h: u32, _c: u32, _f: u32, dst: u32, frames: u32| -> u32 {
            if let Some(wasmtime::Extern::Memory(m)) = caller.get_export("memory") {
                let d = m.data_mut(&mut caller);
                d[dst as usize..dst as usize + 4 * frames as usize].fill(0);
            }
            0
        };
        imports.push(Func::wrap(&mut store, read).into());
    }
    let instance = Instance::new(&mut store, module, &imports)?;
    let memory = instance.get_memory(&mut store, "memory").context("memory")?;
    let f = |n: &str| format!("{}_{n}", c.kernel);
    let ports = instance.get_typed_func::<(), u32>(&mut store, &f("ports"))?;
    let new = instance.get_typed_func::<u32, u32>(&mut store, &f("new"))?;
    let alloc = instance.get_typed_func::<(u32, u32), u32>(&mut store, "m2_alloc")?;
    let (inputs, outputs) = (c.ports.iter().filter(|p| p.input).count(), c.ports.iter().filter(|p| !p.input).count());
    let declared = ports.call(&mut store, ())?;
    if declared != ((inputs as u32) << 8 | outputs as u32) {
        bail!("{}: ports {declared:#x}, manifest {inputs} in {outputs} out", c.kernel);
    }
    let caps: Vec<usize> = c
        .ports
        .iter()
        .map(|p| {
            if p.kind != "events" {
                0
            } else if p.input {
                NOTE_CAP
            } else {
                NOTE_CAP * c.fan
            }
        })
        .collect();
    let lens: Vec<usize> =
        caps.iter().map(|&cap| if cap == 0 { PORT_BYTES } else { (NOTES_HEAD + cap * EVENT_BYTES).next_multiple_of(16) }).collect();
    let n = c.ports.len();
    let settings_at: usize = lens.iter().sum();
    let list_at = settings_at + SETTINGS_BYTES;
    let values_at = (list_at + 4 * n).next_multiple_of(8);
    let table_at = values_at.next_multiple_of(8);
    let channels: Vec<u32> = c.ports.iter().map(|p| p.channels).collect();
    let table_len: usize = channels.iter().map(|&c| c as usize).sum();
    let bytes = table_at + 4 * table_len;
    let block = alloc.call(&mut store, (bytes as u32, 16))? as usize;
    if block == 0 {
        bail!("no room");
    }
    let bufs: Vec<usize> = lens.iter().scan(block, |at, len| Some(std::mem::replace(at, *at + len))).collect();
    let mut io: Vec<u32> = vec![];
    for (p, &ch) in channels.iter().enumerate() {
        io.extend((0..ch as usize).map(|k| (bufs[p] + k * BLOCK * 4) as u32));
    }
    let at = |off: usize, len: usize| if len == 0 { 0 } else { (block + off) as u32 };
    let mut words: Vec<u32> =
        vec![SETTINGS_BYTES as u32, RATE.to_bits(), BLOCK as u32, 0, AUDIO, at(list_at, n), n as u32, 0, 0, 0, 0, 0, 0, 0, 0];
    words.extend(&channels);
    let data = memory.data_mut(&mut store);
    data[block + settings_at..block + values_at].fill(0);
    for (i, w) in words.iter().enumerate() {
        data[block + settings_at + 4 * i..][..4].copy_from_slice(&w.to_le_bytes());
    }
    for (i, w) in io.iter().enumerate() {
        data[block + table_at + 4 * i..][..4].copy_from_slice(&w.to_le_bytes());
    }
    for (p, &cap) in caps.iter().enumerate() {
        if cap > 0 {
            for (i, w) in [0, cap as u32, (bufs[p] + NOTES_HEAD) as u32, 0].iter().enumerate() {
                data[bufs[p] + 4 * i..][..4].copy_from_slice(&w.to_le_bytes());
            }
        }
    }
    let handle = new.call(&mut store, (block + settings_at) as u32)?;
    if handle == 0 {
        bail!("{}_new made no state", c.kernel);
    }
    let process = instance.get_typed_func(&mut store, &f("process"))?;
    let reset = instance.get_typed_func(&mut store, &f("reset"))?;
    let active = instance.get_typed_func(&mut store, &f("active"))?;
    // The prewarm: the host resets each state once after the seal.
    reset.call(&mut store, handle)?;
    Ok(Run { store, memory, process, reset, active, handle, io: (block + table_at) as u32, bufs, caps })
}

/// Write block `b`'s inputs, the noise streams moved on by a block.
fn feed(r: &mut Run, c: &Case, b: usize, rngs: &mut [Vec<Lcg>], ins: &mut Vec<u8>) {
    let data = r.memory.data_mut(&mut r.store);
    for (p, port) in c.ports.iter().enumerate() {
        let at = r.bufs[p];
        if port.kind == "events" {
            if port.input {
                let mut count = 0usize;
                for (nb, frame, kind, id, v) in notes() {
                    if nb == b && count < r.caps[p] {
                        let e = at + NOTES_HEAD + count * EVENT_BYTES;
                        for (i, w) in [kind, frame, id, v[0], v[1], v[2]].iter().enumerate() {
                            data[e + 4 * i..][..4].copy_from_slice(&w.to_le_bytes());
                        }
                        ins.extend(&data[e..e + EVENT_BYTES]);
                        count += 1;
                    }
                }
                data[at..at + 4].copy_from_slice(&(count as u32).to_le_bytes());
            } else {
                data[at..at + 4].fill(0);
            }
            continue;
        }
        if !port.input {
            continue;
        }
        for (ch, rng) in rngs[p].iter_mut().enumerate().take(port.channels as usize) {
            for i in 0..BLOCK {
                let frame = b * BLOCK + i;
                let bits = match port.fed {
                    "noise" => noise(rng, b, c.subnormals),
                    "gate" => gate(b),
                    "sweep" => sweep(frame),
                    "hits" => hits(frame),
                    "beats" => beats(frame),
                    _ => port.bits,
                };
                let o = at + ch * BLOCK * 4 + 4 * i;
                data[o..o + 4].copy_from_slice(&bits.to_le_bytes());
                ins.extend(bits.to_le_bytes());
            }
        }
    }
}

/// Append block `b`'s outputs: each output's channels, an events output's count and events, and
/// `k_active`. Counts the samples that aren't finite, and those that aren't 0.
fn take(r: &mut Run, c: &Case, out: &mut Vec<u8>, counts: &mut (usize, usize)) -> Result<()> {
    let active = r.active.call(&mut r.store, r.handle)?;
    let data = r.memory.data(&r.store);
    for (p, port) in c.ports.iter().enumerate().filter(|(_, p)| !p.input) {
        let at = r.bufs[p];
        if port.kind == "events" {
            let count = (u32::from_le_bytes(data[at..at + 4].try_into()?) as usize).min(r.caps[p]);
            out.extend((count as u32).to_le_bytes());
            out.extend(&data[at + NOTES_HEAD..at + NOTES_HEAD + count * EVENT_BYTES]);
            continue;
        }
        let bytes = &data[at..at + port.channels as usize * BLOCK * 4];
        for w in bytes.as_chunks::<4>().0 {
            let v = f32::from_le_bytes(*w);
            counts.0 += usize::from(!v.is_finite());
            counts.1 += usize::from(v.is_finite() && v != 0.0);
        }
        out.extend(bytes);
    }
    out.extend(active.to_le_bytes());
    Ok(())
}

/// A function of one number, its name and the range of its inputs.
type One = (&'static str, fn(f64) -> f64, f64, f64);
/// A function of two numbers and its name.
type Two = (&'static str, fn(f64, f64) -> f64);

/// What song code's `Math` gives in the app: the `libm` crate (PLAT-4), for `libm.mjs` to hold a
/// JavaScript engine's own `Math` against. Each function on 4,096 inputs a range, as f64 bits.
fn libm_table(dir: &Path) -> Result<()> {
    let one: [One; 15] = [
        ("sin", libm::sin, -10.0, 10.0),
        ("cos", libm::cos, -10.0, 10.0),
        ("tan", libm::tan, -1.5, 1.5),
        ("exp", libm::exp, -20.0, 20.0),
        ("log", libm::log, 1e-6, 1e6),
        ("log2", libm::log2, 1e-6, 1e6),
        ("log10", libm::log10, 1e-6, 1e6),
        ("atan", libm::atan, -10.0, 10.0),
        ("asin", libm::asin, -1.0, 1.0),
        ("acos", libm::acos, -1.0, 1.0),
        ("tanh", libm::tanh, -5.0, 5.0),
        ("sinh", libm::sinh, -5.0, 5.0),
        ("expm1", libm::expm1, -2.0, 2.0),
        ("log1p", libm::log1p, -0.9, 10.0),
        ("cbrt", libm::cbrt, -100.0, 100.0),
    ];
    let mut fns = vec![];
    let n = 4096;
    let at = |lo: f64, hi: f64, i: usize| lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
    for (name, f, lo, hi) in one {
        let rows: Vec<[String; 2]> =
            (0..n).map(|i| at(lo, hi, i)).map(|x| [format!("{:016x}", x.to_bits()), format!("{:016x}", f(x).to_bits())]).collect();
        fns.push(json!({"name": name, "rows": rows}));
    }
    // pow as song code uses it most: 10^(dB/20) and 2^(semitones/12), and atan2.
    let two: [Two; 2] = [("pow", libm::pow), ("atan2", libm::atan2)];
    for (name, f) in two {
        let rows: Vec<[String; 3]> = (0..n)
            .map(|i| match name {
                "pow" if i % 2 == 0 => (10.0, at(-120.0, 24.0, i) / 20.0),
                "pow" => (2.0, at(-48.0, 48.0, i) / 12.0),
                _ => (at(-3.0, 3.0, i), at(-2.0, 2.5, (i * 7) % n)),
            })
            .map(|(x, y)| [format!("{:016x}", x.to_bits()), format!("{:016x}", y.to_bits()), format!("{:016x}", f(x, y).to_bits())])
            .collect();
        fns.push(json!({"name": name, "rows": rows}));
    }
    std::fs::write(dir.join("libm.json"), serde_json::to_string(&json!({"functions": fns}))?)?;
    println!("libm: {}", dir.join("libm.json").display());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(args.first().context("an output folder")?);
    if args.iter().any(|a| a == "--libm") {
        std::fs::create_dir_all(&dir)?;
        return libm_table(&dir);
    }
    let flush = args.iter().any(|a| a == "--flush");
    let subnormals = !args.iter().any(|a| a == "--normal");
    std::fs::create_dir_all(&dir)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let engine = m2_runtime::packages::engine(false);
    m2_runtime::fpenv::canon();
    m2_runtime::fpenv::flush_for_tests(flush);
    let cases = cases(&root, subnormals)?;
    let mut modules = std::collections::HashMap::new();
    let (mut listed, mut results) = (vec![], vec![]);
    for c in &cases {
        let rel = c.wasm.strip_prefix(&root)?.to_string_lossy().to_string();
        let ports: Vec<Value> = c
            .ports
            .iter()
            .map(|p| json!({"name": p.name, "input": p.input, "kind": p.kind, "channels": p.channels, "gen": p.fed, "bits": p.bits}))
            .collect();
        listed
            .push(json!({"package": c.package, "wasm": rel, "kernel": c.kernel, "fan": c.fan, "subnormals": c.subnormals, "ports": ports}));
        if !modules.contains_key(&rel) {
            let bytes = std::fs::read(&c.wasm)?;
            modules.insert(rel.clone(), Module::new(&engine, &bytes)?);
        }
        let id = format!("{}:{}", c.package, c.kernel);
        let mut r = match birth(&modules[&rel], c) {
            Ok(r) => r,
            Err(e) => {
                results.push(json!({"id": id, "error": format!("{e:#}")}));
                continue;
            }
        };
        let mut rngs: Vec<Vec<Lcg>> = (0..c.ports.len()).map(|p| (0..2).map(|ch| Lcg::new(p, ch)).collect()).collect();
        let (mut ins, mut out, mut counts) = (vec![], vec![], (0, 0));
        for b in 0..BLOCKS {
            feed(&mut r, c, b, &mut rngs, &mut ins);
            r.process.call(&mut r.store, (r.handle, r.io, 0, BLOCK as u32))?;
            take(&mut r, c, &mut out, &mut counts)?;
        }
        // The time: block 100's inputs, held, for BLOCKS calls, the best of three.
        let mut rngs: Vec<Vec<Lcg>> = (0..c.ports.len()).map(|p| (0..2).map(|ch| Lcg::new(p, ch)).collect()).collect();
        feed(&mut r, c, 100, &mut rngs, &mut vec![]);
        let mut best = f64::MAX;
        for _ in 0..3 {
            r.reset.call(&mut r.store, r.handle)?;
            let t = Instant::now();
            for _ in 0..BLOCKS {
                r.process.call(&mut r.store, (r.handle, r.io, 0, BLOCK as u32))?;
            }
            best = best.min(t.elapsed().as_secs_f64() * 1e6 / BLOCKS as f64);
        }
        if std::env::var("M2_PROBE_DUMP").is_ok_and(|d| d == id) {
            std::fs::write(dir.join(format!("{}{}.bin", id.replace([':', '/'], "-"), if flush { "-flush" } else { "" })), &out)?;
        }
        results.push(json!({
            "id": id,
            "out": m2_song::Sha::of(&out).hex(),
            "in": m2_song::Sha::of(&ins).hex(),
            "bytes": out.len(),
            "nonfinite": counts.0,
            "sounding": counts.1,
            "us_per_block": (best * 100.0).round() / 100.0,
        }));
    }
    // Which modules need SIMD at all: each compiled again by an engine without it. The runtime's
    // engine already refuses relaxed SIMD (`packages::engine`), so every module here is free of it.
    let mut plain = wasmtime::Config::new();
    plain.wasm_simd(false).wasm_relaxed_simd(false);
    let plain = wasmtime::Engine::new(&plain)?;
    let mut simd: Vec<Value> = vec![];
    for rel in modules.keys() {
        let without = Module::new(&plain, std::fs::read(root.join(rel))?).err().map(|e| format!("{e:#}"));
        simd.push(json!({"wasm": rel, "compiles without simd": without.is_none(), "why not": without}));
    }
    simd.sort_by_key(|m| m["wasm"].as_str().unwrap_or("").to_string());
    std::fs::write(dir.join("cases.json"), serde_json::to_string_pretty(&json!({"blocks": BLOCKS, "cases": listed}))?)?;
    let name = if flush { "wasmtime-flush.json" } else { "wasmtime.json" };
    let host = json!({"host": if flush { "wasmtime 50, flush-to-zero on" } else { "wasmtime 50" }, "modules": simd, "results": results});
    std::fs::write(dir.join(name), serde_json::to_string_pretty(&host)?)?;
    println!("{} kernels: {}", cases.len(), dir.join(name).display());
    Ok(())
}

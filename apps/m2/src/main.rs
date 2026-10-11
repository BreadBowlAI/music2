//! `m2`: music2 song-as-program projects from the terminal.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Diagnostic, FOp, Flat, Ir};
use m2_runtime::{PackageSet, Renderer};
use m2_song::Project;

const USAGE: &str = "m2: build, check and render music2 projects

USAGE:
  m2 build <project> [--json]            run the TypeScript, check the graph, print a summary (or the IR)
  m2 render <project> <out.wav> [--sr 48000] [--seconds N]
  m2 kernels [--json]                    list every kernel and its typed ports

The std package is found at $M2_STD, or next to this source tree.
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn std_dir() -> PathBuf {
    std::env::var_os("M2_STD").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std"))
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    p.load(&std_dir()).context("loading the std package")?;
    Ok(p)
}

/// Build and check a project, printing diagnostics on failure.
fn compile(dir: &Path, packages: &PackageSet) -> Result<(Project, Ir, Flat)> {
    let project = Project::open(dir)?;
    let mut opts = m2_script::Options::new(&project.dir, &project.entry());
    opts.packages = packages.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
    let ir = m2_script::build(&opts).map_err(report)?;
    let flat = m2_ir::check(&ir, packages).map_err(report)?;
    Ok((project, ir, flat))
}

fn report(diags: Vec<Diagnostic>) -> anyhow::Error {
    for d in &diags {
        eprintln!("{d}");
    }
    anyhow!("{} problem{} found", diags.len(), if diags.len() == 1 { "" } else { "s" })
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("missing argument\n\n{USAGE}"));
    match args.first().map(String::as_str) {
        Some("build") => {
            let pk = packages()?;
            let (project, ir, flat) = compile(Path::new(&arg(1)?), &pk)?;
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&ir)?);
                return Ok(());
            }
            summary(&project, &ir, &flat);
        }
        Some("render") => {
            let pk = packages()?;
            let (project, _, flat) = compile(Path::new(&arg(1)?), &pk)?;
            let out = PathBuf::from(arg(2)?);
            let sr: u32 = flag(&args, "--sr").map(|v| v.parse()).transpose().context("--sr")?.unwrap_or(48_000);
            let tempo = project.timeline.tempo()?;
            let seconds = match flag(&args, "--seconds") {
                Some(s) => s.parse().context("--seconds")?,
                None => tempo.seconds_at(project.timeline.end()) + 2.0,
            };
            if seconds <= 0.0 {
                bail!("nothing to render: the timeline is empty (pass --seconds)");
            }
            let t = Instant::now();
            let mut r = Renderer::new(&flat, &project.timeline, &pk, sr)?;
            let frames = r.render(seconds)?;
            write_wav(&out, &frames, sr)?;
            let peak = frames.iter().fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
            let took = t.elapsed().as_secs_f64();
            println!(
                "rendered {seconds:.1}s to {} in {took:.2}s ({:.0}× real time), peak {:.1} dBFS",
                out.display(),
                seconds / took,
                20.0 * peak.max(1e-9).log10()
            );
            if r.non_finite() > 0 {
                eprintln!("warning: {} samples were NaN or infinite and were silenced; a kernel is misbehaving", r.non_finite());
            }
        }
        Some("kernels") => {
            let pk = packages()?;
            let mut sigs: Vec<_> = pk.kernel_sigs().collect();
            sigs.sort_by(|a, b| a.id.cmp(&b.id));
            for s in sigs {
                let ports = |v: &[m2_ir::PortSig]| {
                    v.iter()
                        .map(|p| match p.default {
                            Some(d) => format!("{}: {} = {d}", p.name, p.kind),
                            None => format!("{}: {}", p.name, p.kind),
                        })
                        .collect::<Vec<_>>()
                };
                if args.iter().any(|a| a == "--json") {
                    println!("{}", serde_json::json!({"id": s.id, "description": s.description, "inputs": ports(&s.inputs), "outputs": ports(&s.outputs)}));
                } else {
                    println!("{:12} ({}) -> ({})\n             {}", s.id, ports(&s.inputs).join(", "), ports(&s.outputs).join(", "), s.description);
                }
            }
        }
        _ => print!("{USAGE}"),
    }
    Ok(())
}

fn summary(project: &Project, ir: &Ir, flat: &Flat) {
    println!("{} ({})", project.manifest.project.name, project.dir.display());
    for m in &ir.modules {
        let kind = if m.track { "track" } else { "module" };
        let lanes: Vec<String> = m.lanes.iter().map(|l| format!("{}: {}", l.id, l.kind)).collect();
        let exports: Vec<&str> = m.exports.keys().map(String::as_str).collect();
        println!("  {kind} {:10} lanes [{}]  exports [{}]", m.name, lanes.join(", "), exports.join(", "));
    }
    let mut kernels = 0;
    let mut voices = 0;
    for &i in &flat.order {
        match &flat.nodes[i].op {
            FOp::Kernel { .. } => kernels += 1,
            FOp::Poly { voice, voices: n, .. } => {
                kernels += voice.nodes.iter().filter(|v| matches!(v.op, FOp::Kernel { .. })).count() * *n as usize;
                voices += n;
            }
            _ => {}
        }
    }
    println!("  output {}; {} nodes to run, {kernels} kernel instances, {voices} voices", flat.out_kind, flat.order.len());
    let used: Vec<(String, String)> = ir.modules.iter().flat_map(|m| m.lanes.iter().map(|l| (m.name.clone(), l.id.clone()))).collect();
    for f in project.timeline.unused(&used) {
        println!("  warning: {} is not used by any lane", f.display());
    }
}

fn write_wav(path: &Path, frames: &[[f32; 2]], sr: u32) -> Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate: sr, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    for f in frames {
        for s in f {
            w.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0) as i32)?;
        }
    }
    w.finalize()?;
    Ok(())
}

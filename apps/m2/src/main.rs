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
  m2 check <project> [--json]            run the TypeScript, check the graph and timeline, print a summary (or the IR)
  m2 render <project> <out.wav> [--sr 48000] [--from S] [--seconds N]
                                         render to WAV; --from starts S seconds in
  m2 kernels [--json]                    list every kernel and its typed ports
  m2 bounce <project> <asset-name> [--track NAME] [--export out] [--from S] [--seconds N]
            [--place Track.lane@beat] [--force]
                                         render a track (or the whole song) to assets/<asset-name>.wav,
                                         optionally placing it as a clip on an audio lane
  m2 controls <project>                  every number written in the project that the graph can edit
  m2 set <project> <control> <value> [--expect V]
                                         change one, verified by the compiler (values in the control's units)
  m2 plugins [clap:<id>]                 list installed CLAP plugins, or one plugin's ports
  m2 plugin-state <project> <name> clap:<id> [--force]
                                         save a plugin's default state to assets/plugin-state/<name>.bin

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

/// Compile a project, printing diagnostics on failure and warnings on success.
fn compile(dir: &Path, packages: &mut PackageSet) -> Result<(Project, Ir, Flat)> {
    let project = Project::open(dir)?;
    let c = m2_session::compile(&project, packages).map_err(report)?;
    for w in &c.warnings {
        eprintln!("warning: {w}");
    }
    Ok((project, c.ir, c.flat))
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
        Some("check" | "build") => {
            let mut pk = packages()?;
            let (project, ir, flat) = compile(Path::new(&arg(1)?), &mut pk)?;
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&ir)?);
                return Ok(());
            }
            summary(&project, &ir, &flat);
        }
        Some("render") => {
            let mut pk = packages()?;
            let (project, _, flat) = compile(Path::new(&arg(1)?), &mut pk)?;
            let out = PathBuf::from(arg(2)?);
            let sr: u32 = flag(&args, "--sr").map(|v| v.parse()).transpose().context("--sr")?.unwrap_or(48_000);
            let tempo = project.timeline.tempo()?;
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let seconds = match flag(&args, "--seconds") {
                Some(s) => s.parse().context("--seconds")?,
                None => tempo.seconds_at(project.timeline.end()) + 2.0 - from,
            };
            if seconds <= 0.0 || from < 0.0 {
                bail!("nothing to render (check --from and --seconds; the timeline may be empty)");
            }
            let t = Instant::now();
            let mut r = Renderer::for_project(&flat, &project, &pk, sr)?;
            r.seek((from * sr as f64).round() as u64)?;
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
        Some("bounce") => {
            let mut pk = packages()?;
            let (project, mut ir, _) = compile(Path::new(&arg(1)?), &mut pk)?;
            let name = arg(2)?;
            if name.contains(['/', '\\']) || name.starts_with('.') {
                bail!("the asset name is a file name in assets/, like `keys-bounce`");
            }
            let file = if name.ends_with(".wav") { name.clone() } else { format!("{name}.wav") };
            if let Some(track) = flag(&args, "--track") {
                let export = flag(&args, "--export").unwrap_or_else(|| "out".into());
                ir.root = m2_ir::Root { module: track, export };
            }
            let flat = m2_ir::check(&ir, &pk).map_err(report)?;
            let impure = flat.order.iter().any(|&i| matches!(flat.nodes[i].op, FOp::Plugin { .. }));
            let sr: u32 = flag(&args, "--sr").map(|v| v.parse()).transpose().context("--sr")?.unwrap_or(48_000);
            let tempo = project.timeline.tempo()?;
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let seconds: f64 = match flag(&args, "--seconds") {
                Some(v) => v.parse().context("--seconds")?,
                None => tempo.seconds_at(project.timeline.end()) + 2.0 - from,
            };
            let assets = project.dir.join("assets");
            let path = assets.join(&file);
            if path.exists() && !args.iter().any(|a| a == "--force") {
                bail!("{} exists (pass --force to replace it)", path.display());
            }
            let mut r = Renderer::for_project(&flat, &project, &pk, sr)?;
            r.seek((from * sr as f64).round() as u64)?;
            let frames = r.render(seconds)?;
            std::fs::create_dir_all(&assets)?;
            let channels = vec![frames.iter().map(|f| f[0]).collect::<Vec<f32>>(), frames.iter().map(|f| f[1]).collect()];
            m2_runtime::audio::write_wav(&path, &channels, sr)?;
            // Where it came from, so a bounce of open kernels can always be reproduced.
            let provenance = serde_json::json!({
                "bounced_from": {"module": ir.root.module, "export": ir.root.export},
                "from_seconds": from, "seconds": seconds, "sample_rate": sr,
                "reproducible": !impure,
            });
            std::fs::write(assets.join(format!("{file}.json")), serde_json::to_string_pretty(&provenance)? + "\n")?;
            println!("bounced {}/{} ({seconds:.2}s) to {}{}", ir.root.module, ir.root.export, path.display(), if impure { " (includes plugins: not reproducible)" } else { "" });
            if let Some(place) = flag(&args, "--place") {
                let (target, beat) = place.split_once('@').ok_or_else(|| anyhow!("--place takes Track.lane@beat"))?;
                let (track, lane) = target.split_once('.').ok_or_else(|| anyhow!("--place takes Track.lane@beat"))?;
                let beat: f64 = beat.parse().context("--place beat")?;
                let length = tempo.beat_at(tempo.seconds_at(beat) + seconds) - beat;
                let lane_file = project.dir.join("timeline").join(format!("{}.{lane}.json", m2_song::slug(track)));
                let mut data: serde_json::Value = match std::fs::read_to_string(&lane_file) {
                    Ok(t) => serde_json::from_str(&t).with_context(|| format!("reading {}", lane_file.display()))?,
                    Err(_) => serde_json::json!({"clips": []}),
                };
                let clips = data["clips"].as_array_mut().ok_or_else(|| anyhow!("{} has no clips list", lane_file.display()))?;
                clips.push(serde_json::json!({"start": beat, "length": (length * 1e6).round() / 1e6, "asset": file}));
                std::fs::create_dir_all(lane_file.parent().expect("has a parent"))?;
                std::fs::write(&lane_file, serde_json::to_string_pretty(&data)? + "\n")?;
                println!("placed it on {track}.{lane} at beat {beat} ({})", lane_file.display());
            }
        }
        Some("controls") => {
            let s = m2_session::Session::open(Path::new(&arg(1)?), packages()?).map_err(report)?;
            println!("revision {}", s.accepted().number);
            for c in s.controls() {
                let unit = c.unit.map_or("number".to_string(), |u| u.name().to_string());
                let uses = if c.uses > 1 { format!("  ({} uses)", c.uses) } else { String::new() };
                println!("{:34} {:>10} {:10} {}:{} `{}`{uses}", c.id, c.value, unit, c.file, c.line, c.literal);
            }
        }
        Some("set") => {
            let mut s = m2_session::Session::open(Path::new(&arg(1)?), packages()?).map_err(report)?;
            let id = arg(2)?;
            let value: f64 = arg(3)?.parse().context("value")?;
            let expect: Option<f64> = flag(&args, "--expect").map(|v| v.parse()).transpose().context("--expect")?;
            let t = Instant::now();
            let at = s.accepted().number;
            let c = s.set(at, &id, value, expect).map_err(|e| anyhow!("{e}"))?;
            println!("{}: `{}` → `{}` (revision {}, verified in {:.0} ms)", c.file, c.before, c.after, c.revision, t.elapsed().as_secs_f64() * 1000.0);
        }
        Some("plugins") => match args.get(1) {
            None => {
                let (found, errors) = m2_plugins::scan();
                for f in found {
                    println!("clap:{:40} {} {} ({})", f.id, f.name, f.version, f.features.join(", "));
                }
                for e in errors {
                    eprintln!("warning: {e}");
                }
            }
            Some(id) => {
                let mut pk = PackageSet::default();
                if let Some(e) = pk.describe_plugins([id.as_str()]).into_iter().next() {
                    bail!(e);
                }
                let sig = m2_ir::Kernels::plugin(&pk, id).expect("described");
                let info = pk.plugin_info(id).expect("described");
                println!("{} ({}), latency {} samples", sig.id, sig.description, sig.latency);
                let params: std::collections::HashMap<&str, &m2_plugins::Param> =
                    m2_plugins::param_port_names(&info.params).into_iter().zip(&info.params).map(|(n, p)| (sig.inputs.iter().find(|i| i.name == n).map_or("", |i| i.name.as_str()), p)).collect();
                for p in &sig.inputs {
                    match params.get(p.name.as_str()) {
                        Some(q) => println!("  in  {:28} {}  {}..{}, default {}  ({})", p.name, p.kind, q.min, q.max, q.default, q.name),
                        None => println!("  in  {:28} {}", p.name, p.kind),
                    }
                }
                for p in &sig.outputs {
                    println!("  out {:28} {}", p.name, p.kind);
                }
            }
        },
        Some("plugin-state") => {
            let project = Project::open(Path::new(&arg(1)?))?;
            let name = arg(2)?;
            let id = arg(3)?;
            let clap_id = id.strip_prefix("clap:").ok_or_else(|| anyhow!("plugin ids start with `clap:`"))?;
            let path = m2_runtime::plugin_state_path(&project.dir.join("assets"), &name);
            if path.exists() && !args.iter().any(|a| a == "--force") {
                bail!("{} exists (pass --force to replace it)", path.display());
            }
            let inst = m2_plugins::Instance::new(&m2_plugins::find(clap_id)?, 48_000.0, 128)?;
            let bytes = inst.save_state()?;
            std::fs::create_dir_all(path.parent().expect("has a parent"))?;
            std::fs::write(&path, &bytes)?;
            println!("saved {} bytes of {} state to {}", bytes.len(), inst.info.found.name, path.display());
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

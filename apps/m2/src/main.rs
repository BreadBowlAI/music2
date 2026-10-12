//! `m2`: music2 song-as-program projects from the terminal.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Diagnostic, FOp, Flat, Ir};
use m2_runtime::{PackageSet, Renderer};
use m2_session::{Fix, Session, Warning, render};
use m2_song::Project;

const USAGE: &str = "m2: build, check and render music2 projects

USAGE:
  m2 new <folder> [--template ID | --example ID]
                                         make a new song in <folder> from a template (starter, empty;
                                         starter if none is given) or a copy of an example
  m2 check <project> [--json]            run the TypeScript, check the graph and timeline, print a summary (or the IR)
  m2 render <project> <out.wav> [--sr 48000] [--from S] [--seconds N]
                                         render to a 24-bit WAV, as the app's Export Audio does: to the
                                         last clip's end and on until it falls silent (10 s more at most);
                                         --from starts S seconds in, --seconds renders exactly N seconds
  m2 kernels [--json]                    list every kernel and its typed ports
  m2 bounce <project> <asset-name> [--track NAME] [--export out] [--from S] [--seconds N]
            [--place Track.lane@beat] [--force]
                                         render a track (or the whole song) to assets/<asset-name>.wav,
                                         optionally placing it as a clip on an audio lane
  m2 graph <project> [--json]            the graph the canvas shows: calls, their knobs, connections
  m2 controls <project>                  every number written in the project that the graph can edit
  m2 set <project> <control> <value> [--expect V]
                                         change one, verified by the compiler (values in the control's units)
  m2 cables <project>                    named signals a cable can start from, and the inputs cables reach
  m2 connect <project> <signal> <control>
                                         write a signal's name in place of a control's value, verified
  m2 disconnect <project> <input>        write the input's declared default in place of the name, verified
  m2 effects <project>                   the effects packages offer, and the calls they can go after
  m2 add-effect <project> <call> <module>.<effect>
                                         wrap a call's audio in an effect (e.g. std/fx.delay), verified
  m2 remove-effect <project> <call>      take an effect out, leaving what it was applied to, verified
  m2 relink <project> <from-file> <track> <lane>
                                         move lane data no lane reads (left behind when a track or lane was
                                         renamed in code) to the lane it belongs to now, verified
  m2 delete-data <project> <file>        delete lane data no lane reads
  m2 plugins [clap:<id>]                 list installed CLAP plugins, or one plugin's ports
  m2 plugin-state <project> <name> clap:<id> [--force]
                                         save a plugin's default state to assets/plugin-state/<name>.bin

Every command that reads a project prints what needs attention in it, such as lane data no lane
reads, with the commands that fix it.

std, the templates and the examples are found at $M2_STD, $M2_TEMPLATES and $M2_EXAMPLES, inside
music2.app when m2 runs from there, or else in this source tree.
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

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    // Found the way the app finds it, so both load the same std.
    p.load(&m2_server::resources::std_dir()).context("loading the std package")?;
    Ok(p)
}

/// Compile a project, printing diagnostics on failure and warnings on success.
fn compile(dir: &Path, packages: &mut PackageSet) -> Result<(Project, Ir, Flat)> {
    let project = Project::open(dir)?;
    let c = m2_session::compile(&project, packages).map_err(report)?;
    attention(dir, &c.warnings);
    Ok((project, c.ir, c.flat))
}

/// Open an editing session on a project, printing diagnostics on failure and warnings on success.
fn session(dir: &Path) -> Result<Session> {
    let s = Session::open(dir, packages()?).map_err(report)?;
    attention(dir, s.warnings());
    Ok(s)
}

/// Print what needs attention in the project at `dir` (as it was named), and the commands that fix it.
fn attention(dir: &Path, warnings: &[Warning]) {
    let dir = quote(&dir.display().to_string());
    for w in warnings {
        eprintln!("warning: {}: {w}", w.file);
        for fix in &w.fixes {
            match fix {
                Fix::Relink { from, module, lane, .. } => {
                    eprintln!("  move it:   m2 relink {dir} {} {} {}", quote(from), quote(module), quote(lane))
                }
                Fix::DeleteData { file } => eprintln!("  delete it: m2 delete-data {dir} {}", quote(file)),
            }
        }
    }
}

/// Print a project's warnings if it compiles, for commands that don't need it to.
fn attention_if_it_compiles(dir: &Path) {
    let compiled =
        packages().and_then(|mut pk| m2_session::compile(&Project::open(dir)?, &mut pk).map_err(|d| anyhow!("{} problems", d.len())));
    if let Ok(c) = compiled {
        attention(dir, &c.warnings);
    }
}

/// `s` as one word in a shell.
fn quote(s: &str) -> String {
    let plain = !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./+@:,=%".contains(c));
    if plain { s.to_string() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

/// A file of the project at `dir`, as the project spells it: given that way, or by a path
/// from here that leads into the project.
fn in_project(dir: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if !(path.is_relative() && dir.join(path).exists())
        && let (Ok(dir), Ok(found)) = (dir.canonicalize(), path.canonicalize())
        && let Ok(rel) = found.strip_prefix(&dir)
    {
        return rel.to_path_buf();
    }
    path.to_path_buf()
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
        Some("new") => {
            use m2_server::project::{self, Source};
            let dir = arg(1)?;
            if dir.starts_with('-') {
                bail!("name the new song's folder first: m2 new <folder> [--template ID | --example ID]");
            }
            let (source, what, ids) = match flag(&args, "--example") {
                Some(id) => (Source::Example(id), "example", project::examples()),
                None => {
                    let id = flag(&args, "--template").unwrap_or_else(|| project::DEFAULT_TEMPLATE.into());
                    (Source::Template(id), "template", project::templates())
                }
            };
            let from =
                source.dir().map_err(|e| anyhow!("{e} (try {})", ids.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")))?;
            let made = project::create_at(Path::new(&dir), &from)?;
            let id = from.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            println!("made {} from the {id} {what}", made.display());
            attention_if_it_compiles(&made);
        }
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
            let sr: u32 = flag(&args, "--sr").map(|v| v.parse()).transpose().context("--sr")?.unwrap_or(render::SAMPLE_RATE);
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let length = match flag(&args, "--seconds") {
                Some(s) => render::Length::Seconds(s.parse().context("--seconds")?),
                None => render::Length::Song,
            };
            let t = Instant::now();
            // The same render as the app's Export Audio, so both write the same file.
            let r = render::Render::new(&flat, &project, &pk, sr, from, length)?.write(&out, |_| {})?;
            let took = t.elapsed().as_secs_f64();
            if r.non_finite > 0 {
                eprintln!("warning: {} samples were not numbers (NaN or infinite) and were silenced, as the app plays them", r.non_finite);
            }
            println!(
                "rendered {:.1}s to {} in {took:.2}s ({:.0}× real time), peak {:.1} dBFS",
                r.seconds(),
                out.display(),
                r.seconds() / took,
                r.peak_db
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
        Some("graph") => {
            let s = session(Path::new(&arg(1)?))?;
            let v = s.view();
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            println!("revision {}", v.revision);
            for m in &v.modules {
                println!("\n{}{}", m.name, if m.track { " (track)" } else { "" });
                for n in &m.nodes {
                    let at = n.file.as_ref().map(|f| format!("  {f}:{}", n.line.unwrap_or(0))).unwrap_or_default();
                    let to: Vec<&str> = m.edges.iter().filter(|(a, _)| *a == n.id).filter_map(|(_, b)| m.nodes.iter().find(|x| x.id == *b)).map(|x| x.label.as_str()).collect();
                    println!("  {:22} {:7}{at}{}", n.label, n.kind, if to.is_empty() { String::new() } else { format!("  → {}", to.join(", ")) });
                    for k in &n.knobs {
                        let shared = if k.uses > 1 { format!("  shared ×{}", k.uses) } else { String::new() };
                        println!("      {:14} {:>9} {:9} [{} … {}]{}  `{}`{shared}", k.label, k.value, k.unit.unwrap_or("number"), k.min, k.max, if k.log { " log" } else { "" }, k.literal);
                    }
                    for r in &n.readonly {
                        println!("      {:14} `{}`  (edit in code: {})", r.label, r.expression, r.reason);
                    }
                }
            }
        }
        Some("controls") => {
            let s = session(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for c in s.controls() {
                let unit = c.unit.map_or("number".to_string(), |u| u.name().to_string());
                let uses = if c.uses > 1 { format!("  ({} uses)", c.uses) } else { String::new() };
                println!("{:34} {:>10} {:10} {}:{} `{}`{uses}", c.id, c.value, unit, c.file, c.line, c.literal);
            }
        }
        Some("set") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let id = arg(2)?;
            let value: f64 = arg(3)?.parse().context("value")?;
            let expect: Option<f64> = flag(&args, "--expect").map(|v| v.parse()).transpose().context("--expect")?;
            let t = Instant::now();
            let at = s.accepted().number;
            let c = s.set(at, &id, value, expect).map_err(|e| anyhow!("{e}"))?;
            println!("{}: `{}` → `{}` (revision {}, verified in {:.0} ms)", c.file, c.before, c.after, c.revision, t.elapsed().as_secs_f64() * 1000.0);
        }
        Some("cables") => {
            let s = session(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for o in s.outlets() {
                println!("signal {:30} {:16} {}  {}:{}", o.id, o.name, o.module, o.file, o.line);
            }
            for i in s.inlets() {
                println!("input  {:30} ← {:14} from {}  {}:{}", i.id, i.name, i.from, i.file, i.line);
            }
        }
        Some("effects") => {
            let s = session(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for e in s.effects() {
                println!("effect {:24} {}", format!("{}.{}", e.module, e.name), e.description);
            }
            for m in s.view().modules {
                for n in m.nodes.iter().filter(|n| n.audio || n.effect.is_some()) {
                    let removable = n.effect.as_ref().map_or(String::new(), |e| format!("  (removable {e})"));
                    println!("audio  {:30} {:16} {}{removable}", n.id, n.label, m.name);
                }
            }
        }
        Some(command @ ("add-effect" | "remove-effect")) => {
            let mut s = session(Path::new(&arg(1)?))?;
            let t = Instant::now();
            let at = s.accepted().number;
            let e = match command {
                "add-effect" => {
                    let effect = arg(3)?;
                    let (module, name) = effect.rsplit_once('.').context("name the effect as <module>.<name>, e.g. std/fx.delay")?;
                    s.add_effect(at, &arg(2)?, module, name)
                }
                _ => s.remove_effect(at, &arg(2)?),
            }
            .map_err(|e| anyhow!("{e}"))?;
            println!("{}: `{}` → `{}` (revision {}, verified in {:.0} ms)", e.file, e.before, e.after, e.revision, t.elapsed().as_secs_f64() * 1000.0);
        }
        Some(command @ ("connect" | "disconnect")) => {
            let mut s = session(Path::new(&arg(1)?))?;
            let t = Instant::now();
            let at = s.accepted().number;
            let e = match command {
                "connect" => s.connect(at, &arg(2)?, &arg(3)?),
                _ => s.disconnect(at, &arg(2)?),
            }
            .map_err(|e| anyhow!("{e}"))?;
            println!("{}: `{}` → `{}` (revision {}, verified in {:.0} ms)", e.file, e.before, e.after, e.revision, t.elapsed().as_secs_f64() * 1000.0);
        }
        Some(command @ ("relink" | "delete-data")) => {
            // What needs attention is printed after: the fix may have settled it.
            let dir = PathBuf::from(arg(1)?);
            let mut s = Session::open(&dir, packages()?).map_err(report)?;
            let t = Instant::now();
            let (at, file) = (s.accepted().number, in_project(&dir, &arg(2)?));
            let e = match command {
                "relink" => s.relink(at, &file, &arg(3)?, &arg(4)?),
                _ => s.delete_data(at, &file),
            };
            let e = e.map_err(|e| {
                attention(&dir, s.warnings());
                anyhow!("{e}")
            })?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("{}: `{}` → `{}` (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision);
            attention(&dir, s.warnings());
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
            attention_if_it_compiles(&project.dir);
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

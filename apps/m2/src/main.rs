//! `m2`: music2 song-as-program projects from the terminal.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Diagnostic, FOp, Flat, Ir};
use m2_runtime::{PackageSet, Renderer};
use m2_session::{Compiled, Session, render};
use m2_song::diag::Op;
use m2_song::{AudioClip, DataPath, Key, LaneAddr, LaneData, LaneKind, Project};

const USAGE: &str = "m2: build, check and render music2 projects

USAGE:
  m2 new <folder> [--template ID | --example ID]
                                         make a new song in <folder> from a template (starter, empty;
                                         starter if none is given) or a copy of an example
  m2 check <project> [--json]            run the TypeScript, check the graph and the data, print a summary (or the IR)
  m2 render <project> <out.wav> [--from S] [--seconds N]
                                         render to a 24-bit WAV, as the app's Export Audio does: to the
                                         last clip's end and on until it falls silent (10 s more at most),
                                         at the project's sample_rate; --from starts S seconds in,
                                         --seconds renders exactly N seconds
  m2 kernels [--json]                    list every kernel and its typed ports
  m2 bounce <project> <asset-name> [--track KEY] [--export out] [--from S] [--seconds N]
            [--place track.lane@beat]
                                         render a track, named by its key (or the whole song), to a new
                                         assets/<asset-name>.wav (never over one that exists), optionally
                                         placing it as a clip on an audio lane (both by key)
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
  m2 relink <project> <from> <to>        move data no lane or plugin reads (left behind when a key was renamed
                                         in code) to where it is read now, verified: a lane file
                                         (data/kick/hits.json data/kicks/hits.json), a track's folder
                                         (data/kick data/kicks) or a plugin state
  m2 delete-data <project> <path>        delete data no lane or plugin reads: a lane file, a folder, a state
  m2 fix <project> <path> <fix>          apply a fix a problem offers (assign-clip-ids, repin-asset, …)
  m2 plugins [clap:<id>]                 list installed CLAP plugins, or one plugin's ports
  m2 plugin-state <project> <track>/<key> clap:<id> [--force]
                                         save a plugin's default state to data/<track>/<key>.state, with the
                                         .state.json sidecar that says which plugin wrote it

Every command that reads a project prints what needs attention in it, such as data no lane reads,
with the commands that fix it.

std, the templates and the examples are found at $M2_STD, $M2_TEMPLATES and $M2_EXAMPLES, inside
music2.app when m2 runs from there, or else in this source tree.
";

fn main() -> ExitCode {
    m2_server::resources::keep_hashes();
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

/// The rate an offline render runs at: the project's, always (RENDER-31a). Running the DSP and
/// the plugins at another rate would sound different, and the converter for exporting at
/// another rate isn't built, so `--sr` is refused rather than honoured.
fn render_rate(args: &[String], project: &Project) -> Result<u32> {
    if args.iter().any(|a| a == "--sr") {
        bail!("--sr is gone: a song renders at its own sample_rate ({} Hz, in music2.toml)", project.sample_rate());
    }
    Ok(project.sample_rate())
}

fn packages() -> Result<PackageSet> {
    let mut p = PackageSet::default();
    // Found the way the app finds it, so both load the same std.
    p.load(&m2_server::resources::std_dir()).context("loading the std package")?;
    Ok(p)
}

/// Read a project, printing why it can't be (DIAG-4).
fn open(dir: &Path) -> Result<Project> {
    Project::open(dir).map_err(|d| report(d.into_iter().map(m2_session::diagnostic).collect()))
}

/// Compile a project, printing diagnostics on failure, and what needs attention on success.
fn compile(dir: &Path, packages: &mut PackageSet) -> Result<(Project, Compiled)> {
    let project = open(dir)?;
    let c = m2_session::compile(&project, packages).map_err(report)?;
    attention(dir, &c.problems);
    Ok((project, c))
}

/// Open an editing session on a project, printing diagnostics on failure and what needs
/// attention on success.
fn session(dir: &Path) -> Result<Session> {
    let s = Session::open(dir, packages()?).map_err(report)?;
    attention(dir, s.problems());
    Ok(s)
}

/// Print what needs attention in the project at `dir` (as it was named) though it plays (DIAG-4),
/// each with its severity and code, and the commands that apply its fixes.
fn attention(dir: &Path, problems: &[m2_song::Diagnostic]) {
    let dir = quote(&dir.display().to_string());
    for d in problems {
        let severity = match d.severity {
            m2_song::Severity::Error => "error",
            m2_song::Severity::Warning => "warning",
            m2_song::Severity::Notice => "notice",
        };
        eprintln!("{severity}: {d}");
        let src = quote(d.src.as_deref().unwrap_or_default());
        for fix in &d.fixes {
            match (fix.code, fix.edit.ops.first()) {
                ("relink", Some(Op::Rename { from, to })) => eprintln!("  move it:   m2 relink {dir} {} {}", quote(from), quote(to)),
                ("delete-data", _) => eprintln!("  delete it: m2 delete-data {dir} {src}"),
                (code, _) => eprintln!("  fix it:    m2 fix {dir} {src} {code}   ({})", fix.title),
            }
        }
    }
}

/// Print what needs attention in a project if it compiles, for commands that don't need it to.
fn attention_if_it_compiles(dir: &Path) {
    let project = Project::open(dir).map_err(|d| anyhow!("{} problems", d.len()));
    let compiled = packages().and_then(|mut pk| m2_session::compile(&project?, &mut pk).map_err(|d| anyhow!("{} problems", d.len())));
    if let Ok(c) = compiled {
        attention(dir, &c.problems);
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

/// Print why a project can't be opened or built, each problem with its severity and code, as
/// [`attention`] prints the rest (DIAG-1, DIAG-4): agents act on the code, not the prose.
fn report(diags: Vec<Diagnostic>) -> anyhow::Error {
    for d in &diags {
        let severity = match d.severity {
            m2_ir::Severity::Error => "error",
            m2_ir::Severity::Warning => "warning",
            m2_ir::Severity::Notice => "notice",
        };
        eprintln!("{severity}: {d} [{}]", d.code);
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
            let (project, c) = compile(Path::new(&arg(1)?), &mut pk)?;
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&c.ir)?);
                return Ok(());
            }
            summary(&project, &c.ir, &c.flat);
        }
        Some("render") => {
            let mut pk = packages()?;
            let (project, c) = compile(Path::new(&arg(1)?), &mut pk)?;
            let out = PathBuf::from(arg(2)?);
            let sr = render_rate(&args, &project)?;
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let length = match flag(&args, "--seconds") {
                Some(s) => render::Length::Seconds(s.parse().context("--seconds")?),
                None => render::Length::Song,
            };
            let t = Instant::now();
            // The same render as the app's Export Audio, so both write the same file.
            let r = render::Render::new(&c.flat, &c.data, &pk, sr, from, length)?.write(&out, |_| {})?;
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
            let (project, c) = compile(Path::new(&arg(1)?), &mut pk)?;
            let (mut ir, data) = (c.ir.clone(), c.data.clone());
            let name = arg(2)?;
            let file = if name.ends_with(".wav") { name.clone() } else { format!("{name}.wav") };
            let asset = m2_song::AssetName::new(&file).map_err(|e| anyhow!("{e}: name it like `keys-bounce`"))?;
            // Its provenance is an asset too, beside it (FMT-14).
            let record = m2_song::AssetName::new(&format!("{file}.json")).map_err(|e| anyhow!("{e}"))?;
            // Everything --place needs is checked before anything is written, so a mistake there
            // leaves no asset behind to block the retry (FMT-14: assets are never replaced).
            let place = flag(&args, "--place").map(|p| Place::parse(&p, &ir, &project)).transpose()?;
            let mut bounced = ir.root.clone();
            if let Some(track) = flag(&args, "--track") {
                // A track is named by its key, never its label (ID-11).
                if !ir.modules.iter().any(|m| m.key == track) {
                    bail!("--track takes a track's key, and there's no track `{track}`: the tracks are {}", tracks(&ir));
                }
                let export = flag(&args, "--export").unwrap_or_else(|| "out".into());
                bounced = m2_ir::Root { module: track.clone(), export: export.clone() };
                // Played through an export of the root module, which stays `song` (ID-3).
                let root = ir.modules.iter_mut().find(|m| m.key == m2_song::ROOT).context("the song has no root module")?;
                // A reference is `node.port`, so the node's id has no dot.
                let id = "m2-bounce".to_string();
                let op = m2_ir::Op::Import { module: track, export };
                root.nodes.push(m2_ir::Node { id: id.clone(), op, src: None, origin: None, within: vec![] });
                root.exports.insert(id.clone(), id.clone());
                ir.root.export = id;
            }
            let flat = m2_ir::check(&ir, &pk).map_err(report)?;
            let impure = flat.order.iter().any(|&i| matches!(flat.nodes[i].op, FOp::Plugin { .. }));
            let sr = render_rate(&args, &project)?;
            let tempo = data.tempo().clone();
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let seconds: f64 = match flag(&args, "--seconds") {
                Some(v) => v.parse().context("--seconds")?,
                None => tempo.seconds_at(data.end()) + 2.0 - from,
            };
            let path = project.dir.join(DataPath::Asset(asset.clone()).path());
            let record = project.dir.join(DataPath::Asset(record).path());
            // An asset is never overwritten (FMT-14): a new version gets a new name. Nor is a
            // provenance, even one whose asset is gone.
            for p in [&path, &record] {
                if std::fs::symlink_metadata(p).is_ok() {
                    bail!("{} exists, and assets are never replaced: bounce to a new name", p.display());
                }
            }
            let mut r = Renderer::new(&flat, &data, &pk, sr)?;
            r.seek((from * sr as f64).round() as u64)?;
            let frames = r.render(seconds)?;
            std::fs::create_dir_all(path.parent().expect("inside assets/"))?;
            let channels = vec![frames.iter().map(|f| f[0]).collect::<Vec<f32>>(), frames.iter().map(|f| f[1]).collect()];
            m2_runtime::audio::write_new_wav(&path, &channels, sr)?;
            // Where it came from, so a bounce of open kernels can always be reproduced: the module
            // by its key (ID-11), and every package file the build and the runtime read, by
            // content (RENDER-2).
            let packages: std::collections::BTreeMap<&str, serde_json::Value> = pk
                .packages()
                .map(|p| {
                    let files: std::collections::BTreeMap<&str, &str> = c
                        .package_files
                        .iter()
                        .filter(|f| f.path.split('/').next() == Some(p.name.as_str()))
                        .map(|f| (f.path.as_str(), f.sha256.as_str()))
                        .chain(p.files.iter().map(|(path, sha)| (path.as_str(), sha.as_str())))
                        .collect();
                    (p.name.as_str(), serde_json::json!({"version": p.version, "files": files}))
                })
                .collect();
            let provenance = serde_json::json!({
                "bounced_from": {"module": bounced.module, "export": bounced.export},
                "from_seconds": from, "seconds": seconds, "sample_rate": sr,
                "reproducible": !impure,
                "packages": packages,
            });
            // Created only if it isn't there, as the asset is. An asset without its provenance
            // isn't kept, so the name stays free for a retry.
            let written = serde_json::to_string_pretty(&provenance).map_err(anyhow::Error::from).and_then(|text| {
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&record)?;
                std::io::Write::write_all(&mut f, (text + "\n").as_bytes())?;
                Ok(())
            });
            if let Err(e) = written {
                let _ = std::fs::remove_file(&path);
                return Err(e.context(format!("writing {}", record.display())));
            }
            let impure = if impure { " (includes plugins: not reproducible)" } else { "" };
            println!("bounced {}/{} ({seconds:.2}s) to {}{impure}", bounced.module, bounced.export, path.display());
            if let Some(place) = place {
                let length = tempo.beat_at(tempo.seconds_at(place.beat) + seconds) - place.beat;
                let hash = m2_song::Sha::of_file(&path)?.pin();
                place.put(&project, &asset, hash, length)?;
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
                let shown = if m.label == m.key { m.key.clone() } else { format!("{} ({})", m.label, m.key) };
                println!("\n{shown}{}", if m.track { " (track)" } else { "" });
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
                    println!("audio  {:30} {:16} {}{removable}", n.id, n.label, m.key);
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
        Some(command @ ("relink" | "delete-data" | "fix")) => {
            // What needs attention is printed after: the fix may have settled it.
            let dir = PathBuf::from(arg(1)?);
            let mut s = Session::open(&dir, packages()?).map_err(report)?;
            let t = Instant::now();
            let (at, file) = (s.accepted().number, in_project(&dir, &arg(2)?));
            let e = match command {
                "relink" => s.relink(at, &file, &in_project(&dir, &arg(3)?)),
                "delete-data" => s.delete_data(at, &file),
                _ => {
                    let code = arg(3)?;
                    let src = m2_song::path::slashed(&file);
                    let offered = s.problems().iter().filter(|d| d.src.as_deref() == Some(src.as_str())).flat_map(|d| &d.fixes);
                    match offered.into_iter().find(|f| f.code == code) {
                        Some(f) => s.fix(at, f.code, &serde_json::to_value(&f.edit)?),
                        None => Err(m2_session::EditError::NotFound(format!("a `{code}` fix for {src}"))),
                    }
                }
            };
            let e = e.map_err(|e| {
                attention(&dir, s.problems());
                anyhow!("{e}")
            })?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("{}: `{}` → `{}` (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision);
            attention(&dir, s.problems());
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
            let project = open(Path::new(&arg(1)?))?;
            let keys: Vec<Key> = arg(2)?.split('/').map(Key::new).collect::<Result<_, _>>().map_err(|e| anyhow!("{e}"))?;
            let [module, path @ ..] = &keys[..] else { unreachable!("split gives one at least") };
            if path.is_empty() {
                bail!("name the state as <track>/<plugin key>, with any key() frames between, such as lead/lead");
            }
            let id = arg(3)?;
            let clap_id = id.strip_prefix("clap:").ok_or_else(|| anyhow!("plugin ids start with `clap:`"))?;
            let at = |sidecar| project.dir.join(DataPath::PluginState { module: module.clone(), path: path.to_vec(), sidecar }.path());
            let (blob, sidecar) = (at(false), at(true));
            if blob.exists() && !args.iter().any(|a| a == "--force") {
                bail!("{} exists (pass --force to replace it)", blob.display());
            }
            let inst = m2_plugins::Instance::new(&m2_plugins::find(clap_id)?, project.sample_rate() as f64, 128)?;
            let bytes = inst.save_state()?;
            // The blob and the sidecar that says which plugin wrote it, always together (FMT-17).
            let side = m2_song::Sidecar { plugin: id.clone(), plugin_version: inst.info.found.version.clone() };
            std::fs::create_dir_all(blob.parent().expect("has a parent"))?;
            std::fs::write(&blob, &bytes)?;
            std::fs::write(&sidecar, side.canonical())?;
            println!("saved {} bytes of {} state to {}", bytes.len(), inst.info.found.name, blob.display());
            attention_if_it_compiles(&project.dir);
        }
        _ => print!("{USAGE}"),
    }
    Ok(())
}

/// The tracks, by key, with their labels where they differ: `kick, chops (Chops)`.
fn tracks(ir: &Ir) -> String {
    let named = |m: &m2_ir::Module| if m.label() == m.key { m.key.clone() } else { format!("{} ({})", m.key, m.label()) };
    ir.modules.iter().filter(|m| m.track).map(named).collect::<Vec<_>>().join(", ")
}

/// Where `m2 bounce --place track.lane@beat` puts the bounce: an audio lane, by keys (ID-11),
/// checked whole before anything is rendered or written.
struct Place {
    addr: LaneAddr,
    beat: f64,
    /// The lane file, relative to the project, and what it held when checked.
    rel: PathBuf,
    was: Option<String>,
}

impl Place {
    fn parse(place: &str, ir: &Ir, project: &Project) -> Result<Place> {
        let shape = || anyhow!("--place takes track.lane@beat, by keys, such as chops.clips@16");
        let (target, beat) = place.split_once('@').ok_or_else(shape)?;
        let (track, lane) = target.split_once('.').ok_or_else(shape)?;
        let beat: f64 = beat.parse().context("--place beat")?;
        if !(beat.is_finite() && beat >= 0.0) {
            bail!("--place puts the clip at a beat of 0 or later, not {beat}");
        }
        // A label is never an address (ID-10): say which key it is the label of.
        let Some(module) = ir.modules.iter().find(|m| m.key == track) else {
            let hint = ir
                .modules
                .iter()
                .find(|m| m.label() == track)
                .map_or(String::new(), |m| format!("; `{track}` is the label of `{}`", m.key));
            bail!("--place takes a track's key, and there's no track `{track}`{hint}: the tracks are {}", tracks(ir));
        };
        let addr = LaneAddr::new(track, lane).map_err(|e| anyhow!("--place: {e}"))?;
        let audio: Vec<&str> = module.lanes.iter().filter(|l| matches!(l.kind, m2_ir::Kind::Audio(_))).map(|l| l.key.as_str()).collect();
        if !audio.contains(&lane) {
            let have = if audio.is_empty() { "none".to_string() } else { audio.join(", ") };
            bail!("--place puts the bounce on an audio lane, and `{track}` has no audio lane `{lane}`: its audio lanes are {have}");
        }
        let rel = DataPath::lane(&addr).path();
        let was = match std::fs::read_to_string(project.dir.join(&rel)) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => bail!("{} can't be read ({e}); place the clip by hand", rel.display()),
        };
        if let Some(t) = &was
            && !matches!(LaneData::read(t, &LaneKind::Audio), Ok(LaneData::Audio(_)))
        {
            bail!("{} isn't an audio lane's data; place the clip by hand", rel.display());
        }
        Ok(Place { addr, beat, rel, was })
    }

    /// Add a clip of `asset` to the lane file, if it still holds what it did when checked.
    fn put(&self, project: &Project, asset: &m2_song::AssetName, hash: String, length: f64) -> Result<()> {
        let lane_file = project.dir.join(&self.rel);
        let now = std::fs::read_to_string(&lane_file).ok();
        if now != self.was {
            bail!("{} changed while bouncing; place the clip on it by hand", self.rel.display());
        }
        let mut clips = match &now {
            Some(t) => match LaneData::read(t, &LaneKind::Audio) {
                Ok(LaneData::Audio(c)) => c,
                _ => bail!("{} isn't an audio lane's data; place the clip by hand", self.rel.display()),
            },
            None => vec![],
        };
        // A clip id as the CLI derives it (FMT-11), and the asset's pin (FMT-15).
        let tx = format!("bounce-{}", m2_song::Sha::of(asset.as_str().as_bytes()).pin());
        let id = (0..)
            .map(|n| m2_song::ClipId::derive(&tx, &self.addr, n))
            .find(|id| !clips.iter().any(|c| c.id.as_deref() == Some(id)))
            .expect("an id is free");
        clips.push(AudioClip {
            id: Some(id),
            other_id: None,
            start: self.beat,
            length: (length * 1e6).round() / 1e6,
            asset: asset.to_string(),
            hash: Some(hash),
            offset: 0.0,
            gain_db: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
            reverse: false,
            pitch: 0.0,
            warp: vec![],
        });
        std::fs::create_dir_all(lane_file.parent().expect("has a parent"))?;
        std::fs::write(&lane_file, m2_song::canonical(&LaneData::Audio(clips)))?;
        println!("placed it on {}.{} at beat {} ({})", self.addr.module, self.addr.lane, self.beat, self.rel.display());
        Ok(())
    }
}

fn summary(project: &Project, ir: &Ir, flat: &Flat) {
    println!("{} ({})", project.manifest.project.name, project.dir.display());
    for m in &ir.modules {
        let kind = if m.track { "track" } else { "module" };
        let lanes: Vec<String> = m.lanes.iter().map(|l| format!("{}: {}", l.key, l.kind)).collect();
        let exports: Vec<&str> = m.exports.keys().map(String::as_str).collect();
        let label = if m.label() == m.key { String::new() } else { format!(" ({})", m.label()) };
        println!("  {kind} {:10} lanes [{}]  exports [{}]", format!("{}{label}", m.key), lanes.join(", "), exports.join(", "));
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

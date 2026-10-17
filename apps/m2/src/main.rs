//! `m2`: music2 song-as-program projects from the terminal.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_ir::{Diagnostic, FOp, Flat, Ir};
use m2_runtime::{PackageSet, Renderer};
use m2_editor::{Gestures, render};
use m2_session::{Compiled, Session};
use m2_song::{AudioClip, DataPath, Key, LaneAddr, LaneData, LaneKind, Project, TxOp};

mod edits;

const USAGE: &str = "m2: build, check and render music2 projects

USAGE:
  m2 new <folder> [--template ID | --example ID]
                                         make a new song in <folder> from a template (starter, empty;
                                         starter if none is given) or a copy of an example
  m2 check <project> [--json]            run the TypeScript, check the graph and the data, print a summary (or the IR)
  m2 render <project> <out.wav> [--from S | --from-bar N] [--seconds N] [--preroll P] [--threads N]
                                         render to a 24-bit WAV, as the app's Export Audio does: to the
                                         last clip's end and on until it falls silent (10 s more at most),
                                         at the project's sample_rate; --from starts S seconds in, or
                                         --from-bar at bar N (counted from 1), replaying the song up to
                                         there; --seconds renders exactly N seconds. --preroll starts only
                                         P seconds earlier instead of replaying: fast for listening late in
                                         a long song, but approximate (notes held from before then, and
                                         the tails of what played earlier, are missing), so it is never
                                         written into assets/. --threads renders on N threads (1 to 64,
                                         or M2_THREADS), the same file on any number
  m2 kernels [--json]                    list every kernel and its typed ports
  m2 bounce <project> <asset-name> [--track KEY] [--export out] [--from S] [--seconds N]
            [--place track.lane@beat] [--threads N]
                                         render a track, named by its key (or the whole song), to a new
                                         assets/<asset-name>.wav (never over one that exists), optionally
                                         placing it as a clip on an audio lane (both by key)
  m2 graph <project> [--json]            the graph the canvas shows: calls, their knobs, connections
  m2 controls <project>                  every number written in the project that the graph can edit, and
                                         every declared control a call leaves out (inherited: its default)
  m2 set <project> <control> <value> [--expect V]
                                         change one, verified by the compiler (values in the control's units)
  m2 automate <project> <control>        make a control's value the fallback of a new, empty control lane,
                                         read where the value was, verified to sound the same
  m2 label <project> <track> <name>      show a track, named by its key, as <name>: its track() call's
                                         name, verified to change nothing that plays (its key as <name>
                                         takes the name out)
  m2 add-track <project> <instrument> [--name N] [--key ID] [--after ID]
                                         add a track playing an instrument (std:synth-pluck, or synth.pluck): its file in
                                         tracks/, its import and mixer channel in song.ts, an empty notes lane,
                                         and its place and colour in layout.json, verified
  m2 import <project> <file.wav>… [--track ID] [--at BAR]
                                         bring WAV files into assets/ (a copy, or the same bytes already
                                         there; a taken name numbered) and place their clips from bar BAR
                                         (1 if not given): on a track's audio lane one after another, or
                                         without --track each on a new audio track named after it, at the
                                         end; one edit that undo takes back
  m2 duplicate-track <project> <track>   copy a track, named by its ID: its code, its clips (with new ids), its
                                         plugin states and its mixer channel, verified
  m2 delete-track <project> <track> [--cascade]
                                         take a track out: its file, its imports, its channel, its data and its
                                         layout; --cascade also takes it out of effects that read it (a duck's
                                         sidechain), which are otherwise listed and nothing is changed
  m2 layout <project> order <IDs…> | colour <ID> <colour> | reset
                                         reorder or recolour tracks in layout.json (no revision), or write the
                                         default layout over one that doesn't read; the colours are rose, teal,
                                         amber, indigo, green, plum, olive, blue, coral, cyan, violet and lime
  m2 tempo <project> <bpm> [--at <bar>]  set the song's tempo (20 to 999 BPM) in data/arrangement.json, or with
                                         --at a tempo change from bar N (counted from 1), verified
  m2 meter <project> <n/d> [--at <bar>]  set the song's time signature (7/8: 1 to 64 beats of a 1, 2, 4, 8, 16
                                         or 32 note), or with --at a change from bar N, verified
  m2 title <project> <name>              call the song <name>: its name in music2.toml (the folder keeps its own)
  m2 cables <project>                    named signals a cable can start from, and the inputs cables reach
  m2 connect <project> <signal> <control>
                                         write a signal's name in place of a control's value, verified
  m2 disconnect <project> <input>        write the input's declared default in place of the name, verified
  m2 effects <project>                   the effects packages offer, and the calls they can go after
  m2 add-effect <project> <call> <effect>
                                         wrap a call's audio in an effect, named by its component id
                                         (std:delay) or its export (std/fx.delay), verified
  m2 remove-effect <project> <call>      take an effect out, leaving what it was applied to, verified
  m2 relink <project> <from> <to>        move data no lane or plugin reads (left behind when a key was renamed
                                         in code) to where it is read now, verified: a lane file
                                         (data/kick/hits.json data/kicks/hits.json), a track's folder
                                         (data/kick data/kicks) or a plugin state
  m2 delete-data <project> <path>        delete data no lane or plugin reads: a lane file, a folder, a state
  m2 fix <project> <path> <fix>          apply a fix a problem offers (assign-clip-ids, repin-asset, …)
  m2 tx <project> <tx.json | -> [--check] [--seen HASH]
                                         a verified edit as Tx JSON: --check prints its delta, the delta's
                                         hash and the diagnostics, writing nothing; without it, it is
                                         committed (with --seen, only while its delta has that hash) and the
                                         outcome printed as JSON; exit 1 when it isn't committed
  m2 plugins [clap:<id>]                 list installed CLAP plugins, or one plugin's ports
  m2 plugin-state <project> <track>/<key> clap:<id>
                                         save a plugin's default state to data/<track>/<key>.state, with the
                                         .state.json sidecar that says which plugin wrote it, as one edit
                                         (replacing a state that is there, which undo puts back)
  m2 mute-in-song <project> <track>… on|off
                                         mute tracks, named by their keys, in the song (every render and
                                         export), or unmute them: their track() calls' `muted` option, as
                                         one edit that undo takes back
  m2 clips <project> <gesture> <clip>… [--beats]
                                         one clip gesture, as one edit that undo takes back. A clip is
                                         track/lane:id (bass/notes:k3f9q2zt), or track/lane@place for the
                                         clip that starts there. Places are bar.beat.sixteenth (9.3.1), or
                                         beats with --beats; --by, --length and --grid are beats. Gestures:
                                           create <track>/<lane> --at <place> [--length B]
                                           move <clip>… --by B [--to <track>/<lane>] [--copy]
                                           resize <clip> --start <place> --length B
                                           split <clip>… --at <place>
                                           join <clip>…
                                           duplicate <clip>… [--grid B]
                                           quantize <clip>… --grid B [--ends]
                                           delete <clip>…
                                           json <gesture.json | ->   (any gesture, as the app sends it)
  m2 unautomate <project> <track>/<lane> [--at <place>] [--beats]
                                         take a knob's automation out: what the lane plays at the place
                                         (else its fallback) is written back where the lane is read, and
                                         the lane file goes, as one edit that undo takes back

Every command that reads a project prints what needs attention in it, such as data no lane reads,
with the commands that fix it.

One program writes a project at a time (its lock is .music2/lock): while the app has it open, a
command that writes it stops before changing anything and names the app. Reading, rendering and
bounce without --place work all the same.

std, the templates and the examples are found at $M2_STD, $M2_TEMPLATES and $M2_EXAMPLES, inside
music2.app when m2 runs from there, or else in this source tree.
";

fn main() -> ExitCode {
    // HOST-2: the command runs on a worker while the main thread serves plugins as their main
    // thread, with its run loop; plugins it created are destroyed there before it exits.
    m2_plugins::run_main(|| {
        m2_server::resources::keep_hashes();
        match run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::FAILURE
            }
        }
    })
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// How many threads `m2 render` and `m2 bounce` use on the project in `dir` (8a): `--threads`, else
/// `M2_THREADS`; else the slow cores for a range render (an agent's listen, maybe while the app
/// plays) or while the app holds the project, and one cache's fast cores when nothing else plays.
fn threads(args: &[String], dir: &Path, range: bool) -> Result<render::Threads> {
    if let Some(n) = flag(args, "--threads") {
        return render::Threads::parse(&n).context("--threads");
    }
    Ok(match render::Threads::from_env()? {
        Some(t) => t,
        None if range || m2_session::lock::taken(dir) => render::Threads::Beside,
        None => render::Threads::Alone,
    })
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

/// The beat bar `n` starts at (the first is 0), counted by the arrangement's time signatures.
fn bar_start(a: &m2_song::Arrangement, n: u64) -> f64 {
    let meters = if a.meter.is_empty() { vec![m2_song::Meter::COMMON] } else { a.meter.clone() };
    let mut before = 0;
    for (i, m) in meters.iter().enumerate() {
        let k = n - before;
        match meters.get(i + 1).map(|next| ((next.beat - m.beat) / m.bar()).round() as u64) {
            Some(bars) if k >= bars => before += bars,
            _ => return m.beat + k as f64 * m.bar(),
        }
    }
    unreachable!("the last time signature runs forever")
}

/// `2`, `2.5`: a number as short as it reads.
fn number(x: f64) -> String {
    let s = format!("{x:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
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

/// Compile a project, printing diagnostics on failure, and what needs attention on success. An
/// interrupted commit is said either way, with its fixes, when the project's lock is free: it may
/// be why the code doesn't compile (WRITE-16, the step 6 design's 6a.4).
fn compile(dir: &Path, packages: &mut PackageSet) -> Result<(Project, Compiled)> {
    let project = open(dir)?;
    let journals = m2_session::journal_problems(dir);
    let c = m2_session::compile(&project, packages).map_err(|d| {
        attention(dir, &journals);
        report(d)
    })?;
    attention(dir, &[journals, package_problems(packages), c.problems.clone()].concat());
    Ok((project, c))
}

/// The packages' own problems (`pkg.invalid`: a module that doesn't load, an invalid descriptor,
/// one component id twice), which a session holds and a plain compile doesn't (DIAG-4).
fn package_problems(packages: &PackageSet) -> Vec<m2_song::Diagnostic> {
    m2_session::catalog(packages).problems.into_iter().map(m2_session::problem).collect()
}

/// Open an editing session on a project, printing diagnostics on failure and what needs
/// attention on success.
fn session(dir: &Path) -> Result<Session> {
    let s = writer(dir)?;
    attention(dir, s.problems());
    Ok(s)
}

/// Open a project for writing (WRITE-17, WRITE-18): a command that can't take its lock fails
/// before changing anything, naming the holder, and one whose code doesn't compile can't be
/// edited from here.
fn writer(dir: &Path) -> Result<Session> {
    writer_saying(dir, "")
}

/// [`writer`], whose refusal adds `also` to WRITE-18's message.
fn writer_saying(dir: &Path, also: &str) -> Result<Session> {
    let s = locked(dir, also)?;
    if !s.accepted().has_code() {
        return Err(broken(dir, &s));
    }
    Ok(s)
}

/// Open a project for writing, whether its code compiles or not: what recovery needs, which
/// puts back recorded bytes whatever they compile to (P3, the step 6 design's 6.5).
fn locked(dir: &Path, also: &str) -> Result<Session> {
    let s = Session::open_as(dir, packages()?, "m2").map_err(report)?;
    if let Some(r) = s.access().refusal() {
        bail!("{}{also}", r.message);
    }
    Ok(s)
}

/// Why a project whose code doesn't compile can't be read or edited from here: what needs
/// attention first (an interrupted commit, with the commands that finish or discard it, may be
/// why), then the code's errors.
fn broken(dir: &Path, s: &Session) -> anyhow::Error {
    attention(dir, s.problems());
    report(s.draft_errors.clone())
}

/// Open a project to read it: a reading command never takes the lock (WRITE-18), but reports an
/// interrupted commit when the lock is free.
fn reader(dir: &Path) -> Result<Session> {
    let s = Session::open_read_only(dir, packages()?).map_err(report)?;
    if !s.accepted().has_code() {
        return Err(broken(dir, &s));
    }
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
            // Each fix is a Tx (DIAG-1); its first op says which command applies it.
            match (fix.code, fix.tx.ops.first()) {
                ("relink", Some(TxOp::Rename { from, to })) => {
                    eprintln!("  move it:   m2 relink {dir} {} {}", quote(from.as_str()), quote(to.as_str()))
                }
                ("delete-data", _) => eprintln!("  delete it: m2 delete-data {dir} {src}"),
                (code, _) => eprintln!("  fix it:    m2 fix {dir} {src} {code}   ({})", fix.title),
            }
        }
    }
}

/// Print what needs attention in a project if it compiles, for commands that don't need it to.
fn attention_if_it_compiles(dir: &Path) {
    let project = Project::open(dir).map_err(|d| anyhow!("{} problems", d.len()));
    let compiled = packages().and_then(|mut pk| {
        let c = m2_session::compile(&project?, &mut pk).map_err(|d| anyhow!("{} problems", d.len()))?;
        Ok((package_problems(&pk), c))
    });
    if let Ok((packages, c)) = compiled {
        attention(dir, &[packages, c.problems].concat());
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
            let dir = PathBuf::from(arg(1)?);
            let (project, c) = compile(&dir, &mut pk)?;
            let out = PathBuf::from(arg(2)?);
            let sr = render_rate(&args, &project)?;
            let from = match (flag(&args, "--from"), flag(&args, "--from-bar")) {
                (Some(_), Some(_)) => bail!("say where to start once: --from (seconds) or --from-bar (a bar, from 1)"),
                (None, Some(bar)) => {
                    let bar: u64 = bar.parse().ok().filter(|&b| b >= 1).with_context(|| format!("--from-bar {bar}: bars count from 1"))?;
                    c.data.tempo().seconds_at(bar_start(&project.arrangement, bar - 1))
                }
                (from, None) => from.map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0),
            };
            let length = match flag(&args, "--seconds") {
                Some(s) => render::Length::Seconds(s.parse().context("--seconds")?),
                None => render::Length::Song,
            };
            let preroll: Option<f64> = flag(&args, "--preroll").map(|v| v.parse()).transpose().context("--preroll")?;
            let threads = threads(&args, &dir, preroll.is_some())?;
            let t = Instant::now();
            let r = match preroll {
                // The range render agents listen with (RENDER-4).
                Some(p) => render::Render::range(&c.flat, &c.data, &pk, sr, from, p, length, threads)?,
                // The same render as the app's Export Audio, so both write the same file.
                None => render::Render::new(&c.flat, &c.data, &pk, sr, from, length, threads)?,
            };
            // Into the project's assets/ only exactly, and only as a new file (RENDER-5, WRITE-1).
            let r = r.write_for(&dir, &out, |_| {})?;
            let took = t.elapsed().as_secs_f64();
            if r.non_finite > 0 {
                eprintln!("warning: {} samples were not numbers (NaN or infinite) and were silenced, as the app plays them", r.non_finite);
            }
            println!(
                "rendered {:.1}s to {} in {took:.2}s ({:.0}× real time) on {} thread{}, peak {:.1} dBFS",
                r.seconds(),
                out.display(),
                r.seconds() / took,
                r.threads,
                if r.threads == 1 { "" } else { "s" },
                r.peak_db
            );
            if r.hosts_plugins {
                println!("on 1 thread: songs with plugins render on one for now");
            }
            // RENDER-5: say when it isn't what the export holds, and when it won't be the same again.
            if !r.exact {
                println!(
                    "approximate: it started {} s in with nothing playing, so notes held from before then and the tails of what played \
                     earlier are missing; render without --preroll for the exact render",
                    number((from - preroll.unwrap_or(0.0)).max(0.0))
                );
            }
            if !r.reproducible {
                println!("not reproducible: it hosts plugins, which may render other bits next time");
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
            // Its threads are chosen before --place takes the lock: after, `lock::taken` would see
            // this bounce's own lock as the app's, and render beside it.
            let threads = threads(&args, &project.dir, false)?;
            // Placing it is a Tx on a lane file, so the lock is taken (a writer session) before
            // anything is rendered (WRITE-18); a bounce alone only creates new assets, and needs
            // none.
            let mut placer = match &place {
                Some(p) => Some(writer_saying(&project.dir, &format!(
                    "\nThe clip goes on {}: add it by writing that file directly, or bounce without --place.",
                    m2_song::path::slashed(&p.rel)
                ))?),
                None => None,
            };
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
                root.nodes.push(m2_ir::Node { id: id.clone(), op, src: None, origin: None, frame: None });
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
            let (mut runner, _pool) = render::threads_for(&r, threads);
            r.seek_on((from * sr as f64).round() as u64, &mut runner)?;
            let frames = r.render_on(seconds, &mut runner)?;
            let folder = path.parent().expect("inside assets/");
            // `assets/`, and `vox/` in it for `vox/keys`, each synced in the one above as it is made.
            m2_os::fs::make_dirs(folder)?;
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
            // isn't kept, nor is a provenance this run made but couldn't finish, so the name
            // stays free for a retry. Its bytes are pushed, and its folder synced, before --place
            // names the asset (WRITE-1).
            let written = serde_json::to_string_pretty(&provenance).map_err(anyhow::Error::from).and_then(|text| {
                let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&record)?;
                std::io::Write::write_all(&mut f, (text + "\n").as_bytes())
                    .and_then(|()| m2_os::fs::push(&f))
                    .and_then(|()| m2_os::fs::sync_dir(folder))
                    .inspect_err(|_| drop(std::fs::remove_file(&record)))?;
                Ok(())
            });
            if let Err(e) = written {
                let _ = std::fs::remove_file(&path);
                return Err(e.context(format!("writing {}", record.display())));
            }
            let impure = if impure { " (includes plugins: not reproducible)" } else { "" };
            let n = runner.threads();
            println!(
                "bounced {}/{} ({seconds:.2}s) to {} on {n} thread{}{impure}",
                bounced.module,
                bounced.export,
                path.display(),
                if n == 1 { "" } else { "s" }
            );
            if let (Some(place), Some(session)) = (place, placer.as_mut()) {
                let length = tempo.beat_at(tempo.seconds_at(place.beat) + seconds) - place.beat;
                let hash = m2_song::Sha::of_file(&path)?.pin();
                place.put(session, &asset, hash, length)?;
            }
        }
        Some("graph") => {
            let s = reader(Path::new(&arg(1)?))?;
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
                        // A range only where one is declared (PKG-8); the code's text, or the default it plays.
                        let range = k.range.map_or("—".to_string(), |[lo, hi]| format!("[{lo} … {hi}]{}", if k.log { " log" } else { "" }));
                        let written = if k.written { format!("`{}`", k.literal) } else { "(default)".to_string() };
                        println!("      {:14} {:>9} {:9} {range}  {written}{shared}", k.label, k.value, k.unit.unwrap_or("number"));
                    }
                    for r in &n.readonly {
                        println!("      {:14} `{}`  (edit in code: {})", r.label, r.expression, r.reason);
                    }
                }
            }
        }
        Some("controls") => {
            let s = reader(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for c in s.controls() {
                let unit = c.unit.map_or("number".to_string(), |u| u.name().to_string());
                let uses = if c.uses > 1 { format!("  ({} uses)", c.uses) } else { String::new() };
                // A control the call leaves out plays its declared default; setting it writes it in.
                let written = if c.written { format!("`{}`", c.literal) } else { "inherited (the default)".to_string() };
                println!("{:34} {:>10} {:10} {}:{} {written}{uses}", c.id, c.value, unit, c.file, c.line);
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
        Some("automate") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let t = Instant::now();
            let at = s.accepted().number;
            let a = s.automate(at, &arg(2)?).map_err(|e| anyhow!("{e}"))?;
            let (e, ms) = (&a.edited, t.elapsed().as_secs_f64() * 1000.0);
            println!(
                "{}: `{}` → `{}`, in the new lane {} (revision {}, verified in {ms:.0} ms)",
                e.file, e.before, e.after, a.lane, e.revision
            );
        }
        Some("label") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let t = Instant::now();
            let e = s.rename_label(&arg(2)?, &arg(3)?).map_err(|e| anyhow!("{e}"))?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            match e.before == e.after {
                true => println!("{}: already shown as {}; nothing to write", arg(2)?, e.after),
                false => println!("{}: {} → {} (revision {}, verified in {ms:.0} ms)", e.file, e.before, e.after, e.revision),
            }
        }
        Some("add-track") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let wanted = arg(2)?;
            // By ComponentId (`std:synth-pluck`) or by its export (`synth.pluck`, `std/synth.synth.pluck`).
            let list = s.instruments();
            let inst = list
                .iter()
                .find(|i| i.component == wanted || i.name == wanted || format!("{}.{}", i.module, i.name) == wanted)
                .ok_or_else(|| {
                    let names: Vec<String> = list.iter().map(|i| format!("{} ({})", i.component, i.label)).collect();
                    anyhow!("no instrument `{wanted}`: the instruments are {}", names.join(", "))
                })?;
            let (name, key, after) = (flag(&args, "--name"), flag(&args, "--key"), flag(&args, "--after"));
            let made = s.add_track(&inst.component, name.as_deref(), key.as_deref(), after.as_deref()).map_err(|e| anyhow!("{e}"))?;
            println!("added {} ({}), ID {}, in {} (revision {})", made.label, inst.label, made.module, made.edited.file, made.edited.revision);
        }
        Some("import") => {
            // Studio lane A's A4: what the app's File ▸ Import Audio… does, from the terminal.
            let dir = PathBuf::from(arg(1)?);
            let mut files = vec![];
            let mut i = 2;
            while i < args.len() {
                if args[i].starts_with("--") {
                    i += 2;
                    continue;
                }
                files.push(PathBuf::from(&args[i]));
                i += 1;
            }
            if files.is_empty() {
                bail!("m2 import names the WAV files to bring in\n\n{USAGE}");
            }
            let mut s = session(&dir)?;
            let beat = match flag(&args, "--at") {
                Some(p) => m2_editor::clips::position(&s.accepted().project.arrangement, &p).map_err(|e| anyhow!("--at: {e}"))?,
                None => 0.0,
            };
            let target = match flag(&args, "--track") {
                Some(key) => {
                    let ir = &s.accepted().compiled.ir;
                    let m = ir.modules.iter().find(|m| m.key == key).ok_or_else(|| anyhow!("there's no track `{key}`: the tracks are {}", tracks(ir)))?;
                    let lane = m.lanes.iter().find(|l| m2_runtime::lane_kind(l.kind) == LaneKind::Audio);
                    let lane = lane.ok_or_else(|| anyhow!("`{key}` has no audio lane: bring audio onto a track that plays one (lane.audio), or leave out --track for new ones"))?;
                    m2_editor::import::Target::Lane { module: key, lane: lane.key.clone(), beat }
                }
                None => m2_editor::import::Target::Below { beat },
            };
            let words = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            let mut imported = vec![];
            for f in files {
                let (i, _) = m2_editor::import::audio(&dir, m2_editor::import::Source::Path(f)).map_err(words)?;
                if i.dropped_channels > 0 {
                    eprintln!("{} has {} channels: music2 plays the first two.", i.name, i.dropped_channels + 2);
                }
                imported.push(i);
            }
            let placed = m2_editor::import::place(&mut s, &imported, &target).map_err(words)?;
            for i in &imported {
                println!("imported {} as assets/{}{}", i.name, i.asset, if i.reused { " (the same bytes were there)" } else { "" });
            }
            let onto: Vec<String> = match &target {
                m2_editor::import::Target::Lane { module, .. } => vec![module.clone()],
                _ => placed.tracks.iter().map(|(k, l)| format!("a new track {l} (ID {k})")).collect(),
            };
            println!("onto {} from beat {} (revision {})", onto.join(", "), m2_song::lane::number(beat), placed.edited.revision);
        }
        Some("duplicate-track") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let made = s.duplicate_track(&arg(2)?).map_err(|e| anyhow!("{e}"))?;
            println!("duplicated {} as {}, ID {}, in {} (revision {})", arg(2)?, made.label, made.module, made.edited.file, made.edited.revision);
        }
        Some("delete-track") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let track = arg(2)?;
            let cascade = args.iter().any(|a| a == "--cascade");
            let shown = |d: &m2_editor::Dependent| format!("  {} in {} ({}, line {})", d.component, d.label, d.file, d.line);
            match s.delete_track(&track, cascade).map_err(|e| anyhow!("{e}"))? {
                m2_editor::Deleted::Done { edited, excised } => {
                    println!("deleted {} (revision {})", edited.before, edited.revision);
                    for d in &excised {
                        println!("  and took {track} out of {}'s {} ({}, line {})", d.label, d.component, d.file, d.line);
                    }
                }
                m2_editor::Deleted::Asks(deps) => {
                    let list: Vec<String> = deps.iter().map(shown).collect();
                    bail!("other tracks read {track}:
{}
run again with --cascade to take {track} out of them too", list.join("
"));
                }
                m2_editor::Deleted::Blocked(deps) => {
                    let list: Vec<String> = deps.iter().map(shown).collect();
                    bail!("{track} can't be deleted from here, since other code reads it:
{}
change that code first", list.join("
"));
                }
            }
        }
        Some("layout") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let e = match arg(2)?.as_str() {
                "order" => s.reorder(&args[3..]),
                "colour" | "color" => s.recolour(&arg(3)?, &arg(4)?),
                "reset" => m2_editor::layout::reset(&mut s),
                other => bail!("m2 layout <project> order <IDs…> | colour <ID> <colour> | reset, not `{other}`"),
            }
            .map_err(|e| anyhow!("{e}"))?;
            println!("layout.json: {} → {} (no revision: the layout isn't part of the song)", e.before, e.after);
        }
        Some(command @ ("tempo" | "meter")) => {
            let mut s = session(Path::new(&arg(1)?))?;
            let value = arg(2)?;
            // From bar N (counted from 1): a change there; else the song's own.
            let bar = match flag(&args, "--at") {
                Some(n) => Some(n.parse::<u64>().ok().filter(|n| *n >= 1).ok_or_else(|| anyhow!("--at takes a bar, counted from 1, not `{n}`"))?),
                None => None,
            };
            let at = bar.map(|n| bar_start(&s.accepted().project.arrangement, n - 1));
            let e = match command {
                "tempo" => {
                    let bpm: f64 = value.trim_end_matches("bpm").trim().parse().map_err(|_| anyhow!("a tempo is a number of BPM, such as 128, not `{value}`"))?;
                    match at {
                        Some(at) => m2_editor::song::tempo_change(&mut s, at, bpm),
                        None => m2_editor::song::tempo(&mut s, 0.0, bpm),
                    }
                }
                _ => {
                    let (beats, unit) = value
                        .split_once('/')
                        .and_then(|(b, u)| Some((b.trim().parse::<u32>().ok()?, u.trim().parse::<u32>().ok()?)))
                        .ok_or_else(|| anyhow!("a time signature is written n/d, such as 7/8, not `{value}`"))?;
                    match at {
                        Some(at) => m2_editor::song::meter_change(&mut s, at, beats, unit),
                        None => m2_editor::song::meter(&mut s, 0.0, beats, unit),
                    }
                }
            }
            .map_err(|e| anyhow!("{e}"))?;
            match e.before == e.after {
                true => println!("{}: already {}; nothing to write", e.file, e.after),
                false => println!("{}: {} → {} (revision {})", e.file, e.before, e.after, e.revision),
            }
        }
        Some("title") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let e = m2_editor::song::title(&mut s, &arg(2)?).map_err(|e| anyhow!("{e}"))?;
            match e.before == e.after {
                true => println!("{}: already called {}; nothing to write", e.file, e.after),
                false => println!("{}: {} → {} (revision {})", e.file, e.before, e.after, e.revision),
            }
        }
        Some("cables") => {
            let s = reader(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for o in s.outlets() {
                println!("signal {:30} {:16} {}  {}:{}", o.id, o.name, o.module, o.file, o.line);
            }
            for i in s.inlets() {
                println!("input  {:30} ← {:14} from {}  {}:{}", i.id, i.name, i.from, i.file, i.line);
            }
        }
        Some("effects") => {
            let s = reader(Path::new(&arg(1)?))?;
            println!("revision {}", s.accepted().number);
            for e in s.effects() {
                println!("effect {:18} {:14} {:16} {}", e.component, e.label, e.category, e.description);
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
                    // By its ComponentId (`std:delay`), or by where it is exported (`std/fx.delay`).
                    let effect = arg(3)?;
                    let by_export = || {
                        let (module, name) = effect.rsplit_once('.')?;
                        s.effects().into_iter().find(|e| e.module == module && e.name == name).map(|e| e.component)
                    };
                    let component = if effect.contains(':') { effect.clone() } else { by_export().context("name the effect by its id (std:delay) or its export (std/fx.delay); `m2 effects` lists them")? };
                    s.add_effect(at, &arg(2)?, &component)
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
            // A fix opens whatever the code says: finishing or discarding an interrupted commit
            // needs none that compiles, and may be what makes it compile again (P3).
            let mut s = if command == "fix" { locked(&dir, "")? } else { writer(&dir)? };
            let t = Instant::now();
            let (at, file) = (s.accepted().number, in_project(&dir, &arg(2)?));
            let e = match command {
                "relink" => s.relink(at, &file, &in_project(&dir, &arg(3)?)),
                "delete-data" => s.delete_data(at, &file),
                _ => {
                    // Applying a fix is committing its Tx (DIAG-1).
                    let code = arg(3)?;
                    let src = m2_song::path::slashed(&file);
                    let offered = s.problems().iter().filter(|d| d.src.as_deref() == Some(src.as_str())).flat_map(|d| &d.fixes);
                    let recovery = |f: &m2_song::Fix| matches!(f.tx.expect, m2_song::Expect::Journal(_));
                    match offered.into_iter().find(|f| f.code == code).cloned() {
                        Some(f) if s.accepted().has_code() || recovery(&f) => s.apply_fix(&f),
                        _ if !s.accepted().has_code() => return Err(broken(&dir, &s)),
                        _ => Err(m2_editor::EditError::NotFound(format!("a `{code}` fix for {src}"))),
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
            if args.iter().any(|a| a == "--force") {
                bail!("--force is gone: saving a plugin's state replaces what is there as one edit, which undo puts back (WRITE-1)");
            }
            let dir = PathBuf::from(arg(1)?);
            let mut s = writer(&dir)?;
            let keys: Vec<Key> = arg(2)?.split('/').map(Key::new).collect::<Result<_, _>>().map_err(|e| anyhow!("{e}"))?;
            let [module, path @ ..] = &keys[..] else { unreachable!("split gives one at least") };
            if path.is_empty() {
                bail!("name the state as <track>/<plugin key>, with any key() frames between, such as lead/lead");
            }
            let id = arg(3)?;
            let clap_id = id.strip_prefix("clap:").ok_or_else(|| anyhow!("plugin ids start with `clap:`"))?;
            // Through the host (P11): created and activated on the plugin main thread, saved there,
            // and destroyed.
            let host = m2_plugins::host();
            let (h, info) = host.create(&m2_plugins::find(clap_id)?, None, &m2_plugins::Owner::discard())?;
            let rate = s.accepted().project.sample_rate();
            let saved = host.activate(h, rate, 128).and_then(|_| host.save_state(h).recv()?.map_err(|e| anyhow!("{e}")));
            host.destroy(h);
            let bytes = saved?.bytes;
            let n = bytes.len();
            // The blob and the sidecar that says which plugin wrote it, together, as one Tx (FMT-17).
            let side = m2_song::Sidecar { plugin: id.clone(), plugin_version: info.found.version.clone() };
            let e = m2_editor::data::plugin_state(&mut s, module, path, bytes, &side).map_err(|e| anyhow!("{e}"))?;
            println!("saved {n} bytes of {} state to {} (revision {})", info.found.name, e.file, e.revision);
            attention(&dir, s.problems());
        }
        Some("tx") => tx(&args)?,
        Some(cmd) if edits::handles(cmd) => edits::run(cmd, &args)?,
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

    /// Add a clip of `asset` to the lane, as one Tx on the lane file as it was when checked: a
    /// lane changed while bouncing is a conflict, and the asset stays (WRITE-1).
    fn put(&self, s: &mut Session, asset: &m2_song::AssetName, hash: String, length: f64) -> Result<()> {
        let clip = AudioClip {
            id: None,
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
        };
        m2_editor::data::place_clip(s, &self.addr, self.was.as_deref(), clip).map_err(|e| {
            anyhow!("{e}; {asset} is bounced, place it on {} by hand", self.rel.display())
        })?;
        println!("placed it on {}.{} at beat {} ({})", self.addr.module, self.addr.lane, self.beat, self.rel.display());
        Ok(())
    }
}

/// `m2 tx <project> <tx.json | -> [--check] [--seen HASH]` (WRITE-11): a Tx as agents write it.
/// A check opens the project read-only and writes nothing; a commit takes the lock (WRITE-18).
/// Bases given by hash alone carry their text when this session or the disk has it, so the Tx
/// has the same outcome in every process (WRITE-12).
fn tx(args: &[String]) -> Result<()> {
    let dir = PathBuf::from(args.get(1).context("m2 tx <project> <tx.json | -> [--check] [--seen HASH]")?);
    let from = args.get(2).context("m2 tx <project> <tx.json | -> [--check] [--seen HASH]")?;
    let text = match from.as_str() {
        "-" => std::io::read_to_string(std::io::stdin())?,
        file => std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?,
    };
    // A Tx that doesn't read is rejected as any other refusal is, in JSON on stdout (the step 6
    // design's 1.2: `tx.invalid`, or `tx.path` for a path WRITE-3a refuses).
    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|e| vec![m2_song::Diagnostic::error("tx.invalid", format!("the Tx isn't JSON: {e}"))])
        .and_then(|json| m2_song::Tx::from_json(&json));
    let mut tx = match parsed {
        Ok(tx) => tx,
        Err(diagnostics) => {
            let r = m2_session::Refusal::Rejected { diagnostics, unexpected: None };
            println!("{}", serde_json::to_string_pretty(&m2_editor::json::refusal(&r))?);
            bail!("nothing was changed: {r}")
        }
    };
    if let Some(h) = flag(args, "--seen") {
        let hash = m2_song::Sha::parse_prefixed(&h).with_context(|| format!("--seen {h}: a hash is `sha256:` and 64 hex digits"))?;
        tx.expect = m2_song::Expect::Seen(hash);
    }
    let check = args.iter().any(|a| a == "--check");
    // Whatever the code says: the session refuses what needs it to compile, in JSON, and a
    // recovery fix's Tx needs it not (P3).
    let mut s = match check {
        true => Session::open_read_only(&dir, packages()?).map_err(report)?,
        false => locked(&dir, "")?,
    };
    for (p, base) in tx.base.iter_mut() {
        if let m2_song::Base::Sha(sha) = base
            && p.is_text()
        {
            let text = s.text(sha).map(|t| t.to_string()).or_else(|| {
                std::fs::read_to_string(s.accepted().project.dir.join(p.path())).ok().filter(|t| m2_song::Sha::of(t.as_bytes()) == *sha)
            });
            if let Some(text) = text {
                *base = m2_song::Base::text(&text);
            }
        }
    }
    let out = match check {
        true => s.check(&tx).map(|c| m2_editor::json::checked(&c)),
        false => s.commit(tx).map(|c| m2_editor::json::committed(&c)),
    };
    attention(&dir, s.problems());
    match out {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v)?);
            Ok(())
        }
        Err(r) => {
            println!("{}", serde_json::to_string_pretty(&m2_editor::json::refusal(&r))?);
            bail!("nothing was changed: {r}")
        }
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

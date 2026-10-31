//! `m2`: music2 song-as-program projects from the terminal.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use m2_editor::{Gestures, render};
use m2_ir::{Diagnostic, FOp, Flat, Ir};
use m2_runtime::{PackageSet, Renderer};
use m2_session::{Compiled, Session};
use m2_song::{AudioClip, DataPath, Key, LaneAddr, LaneData, LaneKind, Project, TxOp};

mod app;
mod call;
mod commands;
mod docs;
mod edits;
mod get;
mod listen;
mod search;
mod see;
mod text;
mod words;

fn main() -> ExitCode {
    // Answered before anything starts: the usage, the guide (AGENT-12), a command m2 doesn't
    // know, which is refused with 2, and a word the command doesn't take (AGENT-13).
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => return usage(),
        Some(word) if HELP.contains(&word) => return usage(),
        Some(word) if VERSION.contains(&word) => {
            println!("m2 {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        // m2 run again as the plugin host for a song it plays (`m2_plugins::run_main` serves it).
        Some(word) if word == m2_plugins::HOST_FLAG => {}
        // --as or --why put first isn't a command m2 doesn't know: app::take_who says where they go.
        Some(word) if commands::find(word).is_none() && !["--as", "--why"].contains(&word) => {
            eprintln!("error: m2 has no command `{word}`: m2 alone lists them, and m2 docs commands says what each does");
            return ExitCode::from(2);
        }
        // The guide needs nothing started (AGENT-12).
        Some("docs") => {
            return match command_words(args) {
                Ok(words) => docs::docs(&words),
                Err(code) => code,
            };
        }
        Some(_) => match command_words(args) {
            Ok(words) => args = words,
            Err(code) => return code,
        },
    }
    // Tests: `M2_PROBE` set, songs may play the probe plugin (`clap:m2.probe`), in the host process
    // as any plugin, so a test can make a plugin fail or its host crash while `m2 render` plays it.
    // Debug builds only: a release `m2` never loads it.
    #[cfg(debug_assertions)]
    if std::env::var_os("M2_PROBE").is_some() {
        m2_plugins::probe::install();
    }
    // PLAT-7: a program keeps the cache folder (hashes, renders, samples' measures); a test doesn't.
    m2_os::dirs::keep_cache();
    // HOST-2: the command runs on a worker while the main thread serves plugins as their main
    // thread, with its run loop; plugins it created are destroyed there before it exits.
    m2_plugins::run_main(move || {
        m2_server::resources::keep_hashes();
        let ran = run(args);
        // What the command hashed since the project was read (an installed pack's files, checked
        // as the song resolves them: PACK-6) is a stat for the next one (FMT-16).
        m2_song::HashCache::global().save();
        match ran {
            Ok(()) => {
                // A write the open app took: that it is one undo step there (AGENT-1).
                for line in app::after() {
                    println!("{line}");
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                // A fault that stopped a render says its code too (RENDER-1, DIAG-2).
                match m2_editor::faults::Faulted::of(&e) {
                    Some(f) => eprintln!("error: {e:#} ({})", f.code),
                    None => eprintln!("error: {e:#}"),
                }
                // Busy is 3: nothing changed, try later (AGENT-3).
                app::exit_code().map_or(ExitCode::FAILURE, ExitCode::from)
            }
        }
    })
}

/// The words after `m2`, for the command they name: `--as` and `--why` taken off the end (AGENT-10),
/// then the rest read against the command's usage (AGENT-13). The words to run it with; or the
/// exit status to end with: 0 when they ask for its usage (printed), 2 for a word it doesn't
/// take, 1 for `--as` or `--why` out of place (each said).
fn command_words(mut args: Vec<String>) -> Result<Vec<String>, ExitCode> {
    if let Err(e) = app::take_who(&mut args) {
        eprintln!("error: {e:#}");
        return Err(ExitCode::FAILURE);
    }
    if args.first().and_then(|w| commands::find(w)).is_none() {
        eprintln!("error: name a command first: m2 <command> … --as <name> --why \"<words>\"");
        return Err(ExitCode::FAILURE);
    }
    match words::read(&args) {
        words::Read::Run(words) => Ok(words),
        words::Read::Help(c) => {
            print!("{}", commands::entry(c));
            Err(ExitCode::SUCCESS)
        }
        words::Read::Refused(why) => {
            eprintln!("error: {why}");
            Err(ExitCode::from(2))
        }
    }
}

/// The words that ask for the usage, as `m2` alone does. Not commands: the table hasn't got them.
const HELP: &[&str] = &["help", "--help", "-h"];

/// The words that ask which m2 this is (PLAT-8): it prints `m2 <version>`.
const VERSION: &[&str] = &["--version", "-V"];

/// `m2`'s usage, from the one command table.
fn usage() -> ExitCode {
    print!("{}", commands::usage());
    ExitCode::SUCCESS
}

/// How `word` is called, its forms one a line, for a message.
fn usage_of(word: &str) -> String {
    commands::find(word).map_or_else(String::new, |c| c.usage.replace('\n', "\n  "))
}

/// The value of flag `name`, in words `words::read` handed on (AGENT-13).
fn flag(args: &[String], name: &str) -> Option<String> {
    words::value(args, name)
}

/// Whether flag `name` was given, in words `words::read` handed on (AGENT-13).
fn on(args: &[String], name: &str) -> bool {
    words::on(args, name)
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
    if on(args, "--sr") {
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
    // This computer's packs (PACK-6), under `$HOME` (PLAT-7).
    if let Some(store) = m2_os::dirs::packs() {
        p = p.with_store(store);
    }
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
/// puts back recorded bytes whatever they compile to (P3, the step 6 design's 6.5). While the app
/// holds its lock, a read-only session whose commits go to the app (AGENT-1), at background
/// priority, so its builds never compete with playback; no wait for the lock.
fn locked(dir: &Path, also: &str) -> Result<Session> {
    if let Some(holder) = app::holder(dir) {
        let to = app::reach(dir, &holder, also)?;
        let mut s = Session::open_read_only(dir, packages()?).map_err(report)?;
        if let Err(e) = m2_os::thread::background() {
            eprintln!("m2: couldn't lower its priority beside the app ({e})");
        }
        s.forward_to(Box::new(to));
        return Ok(s);
    }
    let mut s = Session::open_as(dir, packages()?, "m2").map_err(report)?;
    if let Some(r) = s.access().refusal() {
        bail!("{}{also}", r.message);
    }
    // With the app closed, its step goes in the song's one history, which the app reads back when
    // it opens the song: an agent's, by the name it gives (WRITE-23, AGENT-10).
    let who = app::who();
    let caller = m2_server::caller::Caller::agent(who.by.as_deref(), who.why.as_deref());
    s.act_as(caller.source(), caller.why());
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

/// Run the command `args` names, its words as [`words::read`] hands them on: its place words in
/// order, then its flags.
fn run(args: Vec<String>) -> Result<()> {
    let cmd = args.first().map_or("", String::as_str);
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("missing argument: {}", usage_of(cmd)));
    match args.first().map(String::as_str) {
        Some("new") => {
            use m2_server::project::{self, Source};
            let dir = arg(1)?;
            if dir.starts_with('-') {
                bail!("a new song's folder can't start with `-`, which reads as a flag or a number: name it another way");
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
            if on(&args, "--json") {
                println!("{}", serde_json::to_string_pretty(&c.ir)?);
                return Ok(());
            }
            summary(&project, &c.ir, &c.flat);
        }
        Some("render") => {
            let mut pk = packages()?;
            let dir = PathBuf::from(arg(1)?);
            let (project, c) = compile(&dir, &mut pk)?;
            let c = std::sync::Arc::new(c);
            let out = PathBuf::from(arg(2)?);
            let sr = render_rate(&args, &project)?;
            let from = match (flag(&args, "--from"), flag(&args, "--from-bar")) {
                (Some(_), Some(_)) => unreachable!("its usage takes one of them at most"),
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
                Some(p) => render::Render::range(c.clone(), &pk, sr, from, p, length, threads)?,
                // The same render as the app's Export Audio, so both write the same file.
                None => render::Render::new(c.clone(), &pk, sr, from, length, threads)?,
            };
            // Into the project's assets/ only exactly, and only as a new file (RENDER-5, WRITE-1).
            let r = r.write_for(&dir, &out, |_| {})?;
            let took = t.elapsed().as_secs_f64();
            if r.non_finite > 0 {
                eprintln!("warning: {} samples were not numbers (NaN or infinite) and were silenced, as the app plays them", r.non_finite);
            }
            if r.latency_changed > 0 {
                eprintln!("warning: {}", m2_editor::render::LATENCY_CHANGED);
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
                if on(&args, "--json") {
                    println!(
                        "{}",
                        serde_json::json!({"id": s.id, "description": s.description, "inputs": ports(&s.inputs), "outputs": ports(&s.outputs)})
                    );
                } else {
                    println!(
                        "{:12} ({}) -> ({})\n             {}",
                        s.id,
                        ports(&s.inputs).join(", "),
                        ports(&s.outputs).join(", "),
                        s.description
                    );
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
            // none. While the app has the song open, the song is read after the render and the
            // clip placed through the app (AGENT-1): a long render never plans on old reads.
            let also = |p: &Place| {
                format!("\nThe clip goes on {}: bounce without --place, then place it with m2 clips.", m2_song::path::slashed(&p.rel))
            };
            let holder = app::holder(&project.dir);
            let mut placer = match (&place, &holder) {
                (Some(p), None) => Some(writer_saying(&project.dir, &also(p))?),
                // An app m2 can't reach fails before anything is rendered.
                (Some(p), Some(h)) => {
                    app::reach(&project.dir, h, &also(p))?;
                    None
                }
                (None, _) => None,
            };
            let mut bounced = ir.root.clone();
            if let Some(track) = flag(&args, "--track") {
                // A track is named by its key, never its label (ID-11).
                if !ir.modules.iter().any(|m| m.key == track) {
                    bail!("--track takes a track's key, and there's no track `{track}`: the tracks are {}", tracks(&ir));
                }
                let export = flag(&args, "--export").unwrap_or_else(|| "out".into());
                bounced = m2_ir::Root { module: track.clone(), export: export.clone() };
                // Played through an export of the root module, which stays `song` (ID-3): as a stem is.
                ir = render::rooted(&ir, &track, &export)?;
            }
            let stem = std::sync::Arc::new(render::Parts::of(&c, ir, &pk).map_err(report)?);
            let flat = &stem.flat;
            let impure = flat.order.iter().any(|&i| matches!(flat.nodes[i].op, FOp::Plugin { .. }));
            let sr = render_rate(&args, &project)?;
            let tempo = data.tempo().clone();
            let from: f64 = flag(&args, "--from").map(|v| v.parse()).transpose().context("--from")?.unwrap_or(0.0);
            let seconds: f64 = match flag(&args, "--seconds") {
                Some(v) => v.parse().context("--seconds")?,
                None => render::bounce_seconds(&data) - from,
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
            // A fault that changes its sound stops it, named (RENDER-1).
            // With the packages it was built with (PACK-9), as every render of a song is.
            let seen = render::seen(&*stem, &pk);
            let namer = m2_editor::faults::Namer::new(0, stem.clone(), &seen);
            let mut r = Renderer::new(flat, &data, &seen, sr).map_err(|e| namer.faulted(e))?;
            let (mut runner, _pool) = render::threads_for(&r, threads);
            r.seek_on((from * sr as f64).round() as u64, &mut runner).map_err(|e| namer.faulted(e))?;
            let frames = r.render_on(seconds, &mut runner).map_err(|e| namer.faulted(e))?;
            let folder = path.parent().expect("inside assets/");
            // `assets/`, and `vox/` in it for `vox/keys`, each synced in the one above as it is made.
            m2_os::fs::make_dirs(folder)?;
            let channels = vec![frames.iter().map(|f| f[0]).collect::<Vec<f32>>(), frames.iter().map(|f| f[1]).collect()];
            m2_runtime::audio::write_new_wav(&path, &channels, sr)?;
            // Where it came from, so a bounce of open kernels can always be reproduced: the module
            // by its key (ID-11), and every package file the build and the runtime read, by
            // content (RENDER-2): its code, its manifest and wasm, the files its kernels read (a
            // pack's recordings, PACK-5), and a pack's version hash (PACK-6).
            let read: Vec<(String, String)> = c
                .data
                .kernel_assets()
                .iter()
                .filter_map(|(name, sha)| m2_song::pack::split_id(name).map(|(pkg, path)| (format!("{pkg}/{path}"), sha.hex())))
                .collect();
            let packages: std::collections::BTreeMap<&str, serde_json::Value> = pk
                .packages()
                .map(|p| {
                    // A scoped name has a `/` of its own (PACK-1): the file is the package's when its path starts with it.
                    let ours = |path: &str| path.strip_prefix(p.name.as_str()).is_some_and(|rest| rest.starts_with('/'));
                    let files: std::collections::BTreeMap<&str, &str> = c
                        .package_files
                        .iter()
                        .filter(|f| ours(&f.path))
                        .map(|f| (f.path.as_str(), f.sha256.as_str()))
                        .chain(p.files.iter().map(|(path, sha)| (path.as_str(), sha.as_str())))
                        .chain(read.iter().filter(|(path, _)| ours(path)).map(|(path, sha)| (path.as_str(), sha.as_str())))
                        .collect();
                    let mut entry = serde_json::json!({"version": p.version, "files": files});
                    if let Some(hash) = c.packs.iter().find(|r| r.name.as_str() == p.name).and_then(|r| r.hash) {
                        entry["hash"] = serde_json::json!(hash.prefixed());
                    }
                    (p.name.as_str(), entry)
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
            if let Some(place) = place {
                let (place, mut session) = match placer.take() {
                    Some(s) => (place, s),
                    None => {
                        let s = writer_saying(&project.dir, &also(&place))?;
                        let raw = flag(&args, "--place").expect("placing");
                        let again = Place::parse(&raw, &s.accepted().compiled.ir, &s.accepted().project)?;
                        (again, s)
                    }
                };
                let length = tempo.beat_at(tempo.seconds_at(place.beat) + seconds) - place.beat;
                let hash = m2_song::Sha::of_file(&path)?.pin();
                place.put(&mut session, &asset, hash, length)?;
            }
        }
        Some("graph") => {
            let s = reader(Path::new(&arg(1)?))?;
            let v = s.view();
            if on(&args, "--json") {
                println!("{}", serde_json::to_string_pretty(&v)?);
                return Ok(());
            }
            println!("revision {}", v.revision);
            for m in &v.modules {
                let shown = if m.label == m.key { m.key.clone() } else { format!("{} ({})", m.label, m.key) };
                println!("\n{shown}{}", if m.track { " (track)" } else { "" });
                for n in &m.nodes {
                    let at = n.file.as_ref().map(|f| format!("  {f}:{}", n.line.unwrap_or(0))).unwrap_or_default();
                    let to: Vec<&str> = m
                        .edges
                        .iter()
                        .filter(|(a, _)| *a == n.id)
                        .filter_map(|(_, b)| m.nodes.iter().find(|x| x.id == *b))
                        .map(|x| x.label.as_str())
                        .collect();
                    println!(
                        "  {:22} {:7}{at}{}",
                        n.label,
                        n.kind,
                        if to.is_empty() { String::new() } else { format!("  → {}", to.join(", ")) }
                    );
                    for k in &n.knobs {
                        let shared = if k.uses > 1 { format!("  shared ×{}", k.uses) } else { String::new() };
                        // A range only where one is declared (PKG-8); the code's text, or the default it plays.
                        let range =
                            k.range.map_or("—".to_string(), |[lo, hi]| format!("[{lo} … {hi}]{}", if k.log { " log" } else { "" }));
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
            println!(
                "{}: `{}` → `{}` (revision {}, verified in {:.0} ms)",
                c.file,
                c.before,
                c.after,
                c.revision,
                t.elapsed().as_secs_f64() * 1000.0
            );
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
            println!(
                "added {} ({}), ID {}, in {} (revision {})",
                made.label, inst.label, made.module, made.edited.file, made.edited.revision
            );
        }
        Some("import") => {
            // Studio lane A's A4: what the app's File ▸ Import Audio… does, from the terminal.
            let dir = PathBuf::from(arg(1)?);
            let files: Vec<PathBuf> = words::places(&args)[1..].iter().map(PathBuf::from).collect();
            // While the app has the song open, the app's own import brings the files in, so they
            // come into the song only inside the change that lands (AGENT-1, AGENT-3).
            if let Some(holder) = app::holder(&dir) {
                return import_in_app(&dir, &holder, &files, &args);
            }
            let mut s = session(&dir)?;
            let target = import_target(&s, &args)?;
            let beat = target.beat();
            let words = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            let mut imported = vec![];
            for f in files {
                let i = m2_editor::import::audio(&dir, m2_editor::import::Source::Path(f), &mut |_| {}).map_err(words)?;
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
        Some("add-sample") => {
            // A sample from the library (LIB-7): copied with its note, then its clip placed.
            let dir = PathBuf::from(arg(1)?);
            let id = arg(2)?;
            if let Some(holder) = app::holder(&dir) {
                let mut app = app::reach(&dir, &holder, "")?;
                let target = import_target(&reader(&dir)?, &args)?;
                let v = app.add_sample(serde_json::json!({"id": id, "to": target}))?;
                said_added(&v, &target);
                return Ok(());
            }
            let mut s = session(&dir)?;
            let target = import_target(&s, &args)?;
            let words = |e: m2_editor::EditError| anyhow!("{}", e.to_string().trim_start_matches("edit rejected: "));
            let sample = search::song_sample(&s, &id)?;
            m2_editor::library::samples::still_plain(&sample).map_err(|e| anyhow!("{id} can't be added: {e}."))?;
            let staged =
                m2_editor::import::stage(&dir, m2_editor::import::Source::Path(sample.file.clone()), &mut |_| {}).map_err(words)?;
            let (file, noted) = staged.name_noted(Some(&sample.from)).map_err(words)?;
            let song_bpm = s.accepted().compiled.data.tempo().bpm_at(target.beat());
            let follows = m2_editor::library::samples::follows(&sample);
            let placed = m2_editor::import::place_with(&mut s, std::slice::from_ref(&file), &target, &[follows]).map_err(words)?;
            let tracks: Vec<serde_json::Value> = placed.tracks.iter().map(|(m, l)| serde_json::json!({"module": m, "label": l})).collect();
            let v = serde_json::json!({
                "asset": file.asset.as_str(), "reused": file.reused, "noted": noted, "follows": follows,
                "tracks": tracks, "revision": placed.edited.revision, "song_bpm": song_bpm,
            });
            said_added(&v, &target);
        }
        Some("duplicate-track") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let made = s.duplicate_track(&arg(2)?).map_err(|e| anyhow!("{e}"))?;
            println!(
                "duplicated {} as {}, ID {}, in {} (revision {})",
                arg(2)?,
                made.label,
                made.module,
                made.edited.file,
                made.edited.revision
            );
        }
        Some("delete-track") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let track = arg(2)?;
            let cascade = on(&args, "--cascade");
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
                    bail!(
                        "other tracks read {track}:
{}
run again with --cascade to take {track} out of them too",
                        list.join(
                            "
"
                        )
                    );
                }
                m2_editor::Deleted::Blocked(deps) => {
                    let list: Vec<String> = deps.iter().map(shown).collect();
                    bail!(
                        "{track} can't be deleted from here, since other code reads it:
{}
change that code first",
                        list.join(
                            "
"
                        )
                    );
                }
            }
        }
        Some("layout") => {
            let mut s = session(Path::new(&arg(1)?))?;
            let e = match arg(2)?.as_str() {
                "order" => s.reorder(&args[3..]),
                "colour" | "color" => s.recolour(&arg(3)?, &arg(4)?),
                "reset" => m2_editor::layout::reset(&mut s),
                _ => unreachable!("its usage takes only these"),
            }
            .map_err(|e| anyhow!("{e}"))?;
            println!("layout.json: {} → {} (no revision: the layout isn't part of the song)", e.before, e.after);
        }
        Some(command @ ("tempo" | "meter")) => {
            let mut s = session(Path::new(&arg(1)?))?;
            let value = arg(2)?;
            // From bar N (counted from 1): a change there; else the song's own.
            let bar = match flag(&args, "--at") {
                Some(n) => {
                    Some(n.parse::<u64>().ok().filter(|n| *n >= 1).ok_or_else(|| anyhow!("--at takes a bar, counted from 1, not `{n}`"))?)
                }
                None => None,
            };
            let at = bar.map(|n| bar_start(&s.accepted().project.arrangement, n - 1));
            let e = match command {
                "tempo" => {
                    let bpm: f64 = value
                        .trim_end_matches("bpm")
                        .trim()
                        .parse()
                        .map_err(|_| anyhow!("a tempo is a number of BPM, such as 128, not `{value}`"))?;
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
                    let component = if effect.contains(':') {
                        effect.clone()
                    } else {
                        by_export()
                            .context("name the effect by its id (std:delay) or its export (std/fx.delay); `m2 effects` lists them")?
                    };
                    s.add_effect(at, &arg(2)?, &component)
                }
                _ => s.remove_effect(at, &arg(2)?),
            }
            .map_err(|e| anyhow!("{e}"))?;
            println!(
                "{}: `{}` → `{}` (revision {}, verified in {:.0} ms)",
                e.file,
                e.before,
                e.after,
                e.revision,
                t.elapsed().as_secs_f64() * 1000.0
            );
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
            println!(
                "{}: `{}` → `{}` (revision {}, verified in {:.0} ms)",
                e.file,
                e.before,
                e.after,
                e.revision,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        Some("fix") if app::holder(Path::new(&arg(1)?)).is_some() => {
            // The app knows which journals are still being written: its fix applies (the review's m1).
            let dir = PathBuf::from(arg(1)?);
            let holder = app::holder(&dir).context("the app let go of the song")?;
            let src = m2_song::path::slashed(&in_project(&dir, &arg(2)?));
            let e = app::reach(&dir, &holder, "")?.fix(&src, &arg(3)?)?;
            let text = |k: &str| e[k].as_str().unwrap_or_default().to_string();
            println!("{}: `{}` → `{}` (revision {})", text("file"), text("before"), text("after"), e["revision"]);
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
        Some("pack") => pack(&args)?,
        Some("registry") => registry(&args)?,
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
                let params: std::collections::HashMap<&str, &m2_plugins::Param> = m2_plugins::param_port_names(&info.params)
                    .into_iter()
                    .zip(&info.params)
                    .map(|(n, p)| (sig.inputs.iter().find(|i| i.name == n).map_or("", |i| i.name.as_str()), p))
                    .collect();
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
            if on(&args, "--force") {
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
        Some("edit") => text::edit(&args)?,
        Some("write") => text::write(&args)?,
        Some("undo") => {
            // Undo lives in the open app (AGENT-4): history is in its memory.
            let dir = PathBuf::from(arg(1)?);
            let Some(holder) = app::holder(&dir) else {
                bail!("nothing to undo: undo lives in the open app, and music2 doesn't have this song open");
            };
            let v = app::reach(&dir, &holder, "")?.undo()?;
            println!("undid {} in the open app", v["name"].as_str().unwrap_or("the newest change"));
        }
        Some("listen") => listen::listen(&args)?,
        Some("focus") => see::focus(&args)?,
        Some("watch") => see::watch(&args)?,
        Some("point") => see::point(&args)?,
        Some("routes") => call::routes(&args)?,
        Some("call") => call::call(&args)?,
        Some("search") => search::search(&args)?,
        Some(cmd) if edits::handles(cmd) => edits::run(cmd, &args)?,
        // main refused every word the table doesn't hold, and a test holds the table to these arms.
        _ => bail!("m2 has no command `{cmd}`"),
    }
    Ok(())
}

/// Where `m2 import` puts its files in the song `s`: on `--track`'s audio lane, else on new tracks
/// below, from `--at` (the start when it is left out).
/// `m2 pack …` (PACK-6, PACK-8, PACK-12, REG-17): make, check, build and install packs; get them
/// from the library; list and remove what is installed; lock a song's.
fn pack(args: &[String]) -> Result<()> {
    use m2_editor::packs;
    use m2_song::pack::{PackName, Store};
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("missing argument: {}", usage_of("pack")));
    let std = m2_server::resources::std_dir();
    let store = || Store::here().ok_or_else(|| anyhow!("can't find your home folder ($HOME isn't set), where your packs are"));
    let failed = |problems: Vec<String>| {
        for p in &problems {
            eprintln!("problem: {p}");
        }
        anyhow!("{} problem{} to fix first", problems.len(), if problems.len() == 1 { "" } else { "s" })
    };
    // Not `Some("…")` arms: those are m2's own commands, which the one table lists (AGENT-12).
    match args.get(1).map_or("", String::as_str) {
        "new" => {
            let folder = PathBuf::from(arg(2)?);
            let name = flag(args, "--name").ok_or_else(|| anyhow!("name it: m2 pack new <folder> --name @owner/name"))?;
            let name = PackName::new(&name).map_err(|e| anyhow!(e))?;
            packs::new_pack(&folder, &name, &std).map_err(|e| anyhow!(e))?;
            println!("made {name} in {}: check it with m2 pack check {}", folder.display(), folder.display());
        }
        "import-sfz" => {
            let (sfz, folder) = (PathBuf::from(arg(2)?), PathBuf::from(arg(3)?));
            let name = flag(args, "--name").ok_or_else(|| anyhow!("name it: m2 pack import-sfz <file.sfz> <folder> --name @owner/name"))?;
            let name = PackName::new(&name).map_err(|e| anyhow!(e))?;
            let (label, category, license) = (flag(args, "--label"), flag(args, "--category"), flag(args, "--license"));
            let import = m2_editor::sfz::Import {
                sfz: &sfz,
                folder: &folder,
                name: &name,
                label: label.as_deref(),
                category: category.as_deref(),
                license: license.as_deref(),
                std: &std,
                check: &|dir: &Path| packs::check(dir, &std),
            };
            let done = m2_editor::sfz::import(&import).map_err(|e| anyhow!(e))?;
            println!(
                "made {name} in {}: {} plays {} region{} from {} recording{}",
                folder.display(),
                done.id,
                done.regions,
                if done.regions == 1 { "" } else { "s" },
                done.samples,
                if done.samples == 1 { "" } else { "s" }
            );
            for l in done.licence.iter().chain(&done.left_out) {
                println!("{l}");
            }
            // Checked before it was kept (PACK-14).
            println!("{} is good to install: m2 pack install {}", folder.display(), folder.display());
        }
        "check" => {
            let dir = PathBuf::from(arg(2)?);
            let problems = packs::check(&dir, &std);
            if !problems.is_empty() {
                return Err(failed(problems));
            }
            println!("{} is good to install", dir.display());
        }
        "build" => {
            let dir = PathBuf::from(arg(2)?);
            let (listing, hash) = packs::build(&dir).with_context(|| format!("listing {}", dir.display()))?;
            println!("wrote {} ({} files): {hash}", dir.join(m2_song::pack::LISTING).display(), listing.files.len());
        }
        "install" => {
            let dir = PathBuf::from(arg(2)?);
            match packs::install(&dir, &store()?, &std).map_err(failed)? {
                packs::Install::Installed { name, version, dir, hash } => {
                    println!("installed {name} {version} in {} ({hash})", dir.display())
                }
                packs::Install::Already { name, version, .. } => println!("{name} {version} is already installed, with these very files"),
                packs::Install::Repaired { name, version, why } => {
                    println!("{name} {version} was changed after it was installed ({why}): put back as it was installed")
                }
            }
        }
        "get" => get::pack_get(args.get(2).map(String::as_str))?,
        "list" => {
            let all = packs::installed(&store()?);
            if all.is_empty() {
                println!("no packs installed: install one from its folder with m2 pack install <folder>");
            }
            for p in all {
                let licence = p.license.map(|l| format!(" ({l})")).unwrap_or_default();
                println!("{} {}{licence}  {}", p.name, p.versions.join(", "), p.description);
            }
        }
        "remove" => {
            let name = PackName::new(&arg(2)?).map_err(|e| anyhow!(e))?;
            let v = arg(3)?;
            let version = m2_song::Version::parse(&v).ok_or_else(|| anyhow!("`{v}` isn't a version such as 1.2.0"))?;
            packs::remove(&store()?, &name, &version)?;
            println!("removed {name} {version}. A song that locks this version says so (pkg.missing) until it is installed again");
        }
        // Both work while the song doesn't build: that is when a song moved to another version
        // needs them (PACK-8).
        command @ ("lock" | "update") => {
            let dir = PathBuf::from(arg(2)?);
            let only = args.get(3).map(|n| PackName::new(n).map_err(|e| anyhow!(e))).transpose()?;
            let moving = match (command, &only) {
                ("lock", None) => m2_song::pack::Move::Needed,
                ("lock", Some(_)) => unreachable!("its usage takes one word"),
                (_, None) => m2_song::pack::Move::All,
                (_, Some(name)) => m2_song::pack::Move::Only(name),
            };
            let mut s = locked(&dir, "")?;
            match packs::write_lock(&mut s, moving).map_err(|e| anyhow!("{e}"))? {
                Some(e) => println!("{}: {}", e.file, e.after),
                None => println!("{} says that already", m2_song::path::LOCK),
            }
        }
        _ => unreachable!("its usage takes only these"),
    }
    Ok(())
}

/// `m2 registry …` (REG-2, REG-5 to REG-9): the registry's robot, run by the registry repo's CI,
/// and `add` for a pack's author. Each problem is printed in words; any is exit 1.
fn registry(args: &[String]) -> Result<()> {
    use m2_editor::registry::{self as robot, Bucket};
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("missing argument: {}", usage_of("registry")));
    let path_flag = |name: &str| flag(args, name).map(PathBuf::from).ok_or_else(|| anyhow!("give {name}: {}", usage_of("registry")));
    let from = || flag(args, "--from").map(|u| Bucket::parse(&u).map_err(|e| anyhow!("--from: {e}"))).transpose();
    let failed = |problems: Vec<String>| {
        for p in &problems {
            eprintln!("problem: {p}");
        }
        anyhow!("{} problem{} to fix first", problems.len(), if problems.len() == 1 { "" } else { "s" })
    };
    let std = m2_server::resources::std_dir();
    // Not `Some("…")` arms: those are m2's own commands, which the one table lists (AGENT-12).
    match args.get(1).map_or("", String::as_str) {
        "check" => {
            let dir = PathBuf::from(arg(2)?);
            let base = flag(args, "--base").map(PathBuf::from);
            let author = flag(args, "--author");
            let r = robot::check(&robot::Check { registry: &dir, base: base.as_deref(), author: author.as_deref() }).map_err(failed)?;
            let versions: usize = r.packs.iter().map(|p| p.versions.len()).sum();
            println!("{} is good: {} pack{}, {versions} version{}", dir.display(), r.packs.len(), plural(r.packs.len()), plural(versions));
        }
        "add" => {
            let (dir, pack) = (PathBuf::from(arg(2)?), PathBuf::from(arg(3)?));
            let source = flag(args, "--source").ok_or_else(|| anyhow!("say where its .tar.gz is: {}", usage_of("registry")))?;
            match robot::add(&robot::Add { registry: &dir, pack: &pack, source: &source, std: &std }).map_err(failed)? {
                robot::Added::Listed { name, version, hash, file, new } => println!(
                    "listed {name} {version} ({hash}) in {}{}: open a pull request with it to the registry",
                    file.display(),
                    if new { ", a new file" } else { "" }
                ),
                robot::Added::Already { name, version } => println!("{name} {version} is listed already, with these very files"),
            }
        }
        "pending" => {
            let dir = PathBuf::from(arg(2)?);
            let r = m2_song::registry::Registry::read(&dir).map_err(failed)?;
            // The hash's hex first: the folder the CI unpacks that version's source into (REG-9).
            for (name, v) in robot::pending(&r, &path_flag("--out")?, from()?.as_ref()) {
                println!("{} {name} {} {}", v.hash.hex(), v.version, v.source);
            }
        }
        "build" => {
            let dir = PathBuf::from(arg(2)?);
            let (sources, out) = (path_flag("--sources")?, path_flag("--out")?);
            let made = flag(args, "--made").unwrap_or_else(|| {
                let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
                m2_song::registry::utc(secs)
            });
            let from = from()?;
            let work = m2_os::dirs::cache().unwrap_or_else(std::env::temp_dir).join(format!("registry-build-{}", std::process::id()));
            let to_delete = flag(args, "--to-delete").map(PathBuf::from);
            let b = robot::Build {
                registry: &dir,
                sources: &sources,
                out: &out,
                from: from.as_ref(),
                made: &made,
                std: &std,
                work: &work,
                to_delete: to_delete.as_deref(),
                limit: robot::LONGEST,
            };
            let built = robot::build(&b).map_err(failed)?;
            for f in &built.built {
                let previews = f.items.iter().filter(|i| i.preview.is_some()).count();
                println!(
                    "built {} {}: {} item{} ({previews} with a preview), {} file{}, {:.1} MB",
                    f.name,
                    f.version,
                    f.items.len(),
                    plural(f.items.len()),
                    f.files,
                    plural(f.files as usize),
                    f.size as f64 / 1e6
                );
                for i in f.items.iter().filter_map(|i| i.no_preview.as_ref().map(|why| (&i.id, why))) {
                    println!("  {}: {}", i.0, i.1);
                }
            }
            let items: usize = built.catalog.packs.iter().map(|p| p.items.len()).sum();
            println!(
                "catalog {} ({} bytes, made {}): {} pack{}, {items} item{}",
                built.pointer.catalog,
                built.pointer.size,
                built.pointer.made,
                built.catalog.packs.len(),
                plural(built.catalog.packs.len()),
                plural(items)
            );
            if to_delete.is_none() && !built.to_delete.is_empty() {
                println!("{} blobs of removed versions to delete (--to-delete <file> lists them)", built.to_delete.len());
            }
            for u in &built.unknown {
                eprintln!("warning: {u} is taken down, but its facts aren't there, so its blobs aren't known: delete them by hand");
            }
            // Each failed alone: the rest are built and in the catalog (REG-5).
            for f in &built.failed {
                for p in &f.problems {
                    eprintln!("problem: {p}");
                }
            }
            if let n @ 1.. = built.failed.len() {
                let (s, it) = if n == 1 { ("", "it stays") } else { ("s", "each stays") };
                bail!("{n} version{s} not built: {it} pending, and the rest are in the catalog");
            }
        }
        _ => unreachable!("its usage takes only these"),
    }
    Ok(())
}

/// `s` when `n` isn't one.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn import_target(s: &Session, args: &[String]) -> Result<m2_editor::import::Target> {
    let beat = match flag(args, "--at") {
        Some(p) => m2_editor::clips::position(&s.accepted().project.arrangement, &p).map_err(|e| anyhow!("--at: {e}"))?,
        None => 0.0,
    };
    Ok(match flag(args, "--track") {
        Some(key) => {
            let ir = &s.accepted().compiled.ir;
            let m = ir
                .modules
                .iter()
                .find(|m| m.key == key)
                .ok_or_else(|| anyhow!("there's no track `{key}`: the tracks are {}", tracks(ir)))?;
            let lane = m.lanes.iter().find(|l| m2_runtime::lane_kind(l.kind) == LaneKind::Audio);
            let lane = lane.ok_or_else(|| {
                anyhow!(
                    "`{key}` has no audio lane: bring audio onto a track that plays one (lane.audio), or leave out --track for new ones"
                )
            })?;
            m2_editor::import::Target::Lane { module: key, lane: lane.key.clone(), beat }
        }
        None => m2_editor::import::Target::Below { beat },
    })
}

/// `m2 import` while the app `holder` has the song at `dir` open (AGENT-1): the target read from
/// the song as a local import reads it, then the files' paths sent to the app's own import, which
/// copies them into the song only inside the change that lands (AGENT-3). It prints what a local
/// import prints.
fn import_in_app(dir: &Path, holder: &m2_session::lock::Holder, files: &[PathBuf], args: &[String]) -> Result<()> {
    let mut app = app::reach(dir, holder, "")?;
    let target = import_target(&reader(dir)?, args)?;
    let beat = target.beat();
    // The app reads each file where it is: by its whole path.
    let paths: Vec<PathBuf> = files.iter().map(std::path::absolute).collect::<std::io::Result<_>>()?;
    let v = app.import(serde_json::json!({"paths": paths, "to": target}))?;
    for note in v["notes"].as_array().into_iter().flatten().filter_map(|n| n.as_str()) {
        eprintln!("{note}");
    }
    let assets = v["assets"].as_array().cloned().unwrap_or_default();
    for (i, f) in files.iter().enumerate() {
        let name = f.file_name().map_or_else(|| f.display().to_string(), |n| n.to_string_lossy().into_owned());
        let again = if v["reused"][i] == serde_json::json!(true) { " (the same bytes were there)" } else { "" };
        println!("imported {name} as assets/{}{again}", assets.get(i).and_then(|a| a.as_str()).unwrap_or("?"));
    }
    let onto: Vec<String> = match &target {
        m2_editor::import::Target::Lane { module, .. } => vec![module.clone()],
        _ => v["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| format!("a new track {} (ID {})", t["label"].as_str().unwrap_or("?"), t["module"].as_str().unwrap_or("?")))
            .collect(),
    };
    println!("onto {} from beat {} (revision {})", onto.join(", "), m2_song::lane::number(beat), v["revision"].as_u64().unwrap_or(0));
    Ok(())
}

/// What `m2 add-sample` did, as it prints it: the file, its note, where its clip went, and the
/// tempo a loop follows.
fn said_added(v: &serde_json::Value, target: &m2_editor::import::Target) {
    let asset = v["asset"].as_str().unwrap_or("?");
    let again = if v["reused"] == serde_json::json!(true) { " (the same bytes were there)" } else { "" };
    let note = if v["noted"] == serde_json::json!(true) { format!(", its note in assets/{asset}.json") } else { String::new() };
    println!("added assets/{asset}{again}{note}");
    let onto: Vec<String> = match target {
        m2_editor::import::Target::Lane { module, .. } => vec![module.clone()],
        _ => v["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| format!("a new track {} (ID {})", t["label"].as_str().unwrap_or("?"), t["module"].as_str().unwrap_or("?")))
            .collect(),
    };
    println!(
        "onto {} from beat {} (revision {})",
        onto.join(", "),
        m2_song::lane::number(target.beat()),
        v["revision"].as_u64().unwrap_or(0)
    );
    if let Some(bpm) = v["follows"].as_f64() {
        println!("a loop at {} bpm: it follows the song's tempo (Follow tempo)", m2_song::lane::number(bpm));
    }
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
        let put = serde_json::to_value(&clip)?;
        m2_editor::data::place_clip(s, &self.addr, self.was.as_deref(), clip).map_err(|e| {
            // Assets are never replaced: the bounce stays, and this command places it.
            let gesture = m2_editor::clips::Gesture::Set { lane: self.addr.clone(), put: vec![put], delete: vec![] };
            let json = serde_json::to_string(&gesture).unwrap_or_default();
            let dir = quote(&s.accepted().project.dir.display().to_string());
            anyhow!("{e}; {asset} is bounced: place it with\n  echo {} | m2 clips {dir} json -", quote(&json))
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
    // A Tx that doesn't read is rejected as any other refusal is, in JSON on stdout (the step 6
    // design's 1.2: `tx.invalid`, or `tx.path` for a path WRITE-3a refuses).
    let mut tx = match read_tx(from)? {
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
    let check = on(args, "--check");
    // Whatever the code says: the session refuses what needs it to compile, in JSON, and a
    // recovery fix's Tx needs it not (P3).
    let mut s = match check {
        true => Session::open_read_only(&dir, packages()?).map_err(report)?,
        false => locked(&dir, "")?,
    };
    text_bases(&s, &mut tx);
    let out = match check {
        true => s.check(&tx).map(|c| m2_editor::json::checked(&c)),
        false => s.commit(tx).map(|c| m2_editor::json::committed(&c)),
    };
    attention(&dir, s.problems());
    match out {
        Ok(v) => {
            println!("{}", serde_json::to_string_pretty(&v)?);
            // Its answer is JSON alone: that it is one undo step in the open app goes to stderr.
            for line in app::after() {
                eprintln!("{line}");
            }
            Ok(())
        }
        Err(r) => {
            println!("{}", serde_json::to_string_pretty(&m2_editor::json::refusal(&r))?);
            bail!("nothing was changed: {r}")
        }
    }
}

/// The Tx in `from`, a file or `-` for standard input; what is wrong with it when it doesn't read.
fn read_tx(from: &str) -> Result<Result<m2_song::Tx, Vec<m2_song::Diagnostic>>> {
    let text = match from {
        "-" => std::io::read_to_string(std::io::stdin())?,
        file => std::fs::read_to_string(file).with_context(|| format!("reading {file}"))?,
    };
    Ok(serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|e| vec![m2_song::Diagnostic::error("tx.invalid", format!("the Tx isn't JSON: {e}"))])
        .and_then(|json| m2_song::Tx::from_json(&json)))
}

/// Each text file's base a Tx names by its hash, as its text, when the session or the file on
/// disk has that text: its edits then apply as a base's do.
fn text_bases(s: &Session, tx: &mut m2_song::Tx) {
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

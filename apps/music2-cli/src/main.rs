//! L6 — headless music2. Same engine and commands as the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use m2_engine::{COMMANDS, Session};
use m2_project::Song;
use serde_json::Value;

const USAGE: &str = "music2-cli — the music2 engine from the terminal

USAGE:
  music2-cli new <song.m2song>                    create an empty song
  music2-cli describe <song.m2song> [--clip ID]   print the song summary as JSON
  music2-cli commands [--json]                    list every editing command
  music2-cli exec <song.m2song> <command> [json]  run one command and save
  music2-cli import <song.m2song> <file.wav> [--as clip|instrument] [--at BAR]
  music2-cli render <song.m2song> <out.wav> [--sr 48000]
  music2-cli plugin add github:owner/repo[/path.json][@ref]
  music2-cli plugin list
  music2-cli plugin check <file.m2plug.json>      validate a plugin before publishing
";

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn session_for(path: &Path) -> Result<Session> {
    let (reg, errors) = Session::default_registry();
    for e in errors {
        eprintln!("warning: {e}");
    }
    let mut s = Session::new(Arc::new(reg));
    s.open(path)?;
    for n in s.take_notices() {
        eprintln!("note: {n}");
    }
    Ok(s)
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let arg = |i: usize| args.get(i).cloned().ok_or_else(|| anyhow!("missing argument\n\n{USAGE}"));
    match cmd {
        "new" => {
            let path = PathBuf::from(arg(1)?);
            let (reg, _) = Session::default_registry();
            let mut s = Session::new(Arc::new(reg));
            s.new_song(Song::default());
            s.save(&path)?;
            println!("created {}", path.display());
        }
        "describe" => {
            let s = session_for(Path::new(&arg(1)?))?;
            let clip = flag(&args, "--clip");
            let v = m2_engine::commands::describe(s.song(), &s.registry, clip.and_then(|c| c.parse().ok()));
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        "commands" => {
            for c in COMMANDS {
                if args.iter().any(|a| a == "--json") {
                    println!(
                        "{}",
                        serde_json::json!({"id": c.id, "description": c.description, "mutates": c.mutates, "schema": (c.schema)()})
                    );
                } else {
                    println!("{:22} {}", c.id, c.description);
                }
            }
        }
        "exec" => {
            let path = PathBuf::from(arg(1)?);
            let mut s = session_for(&path)?;
            let params: Value = match args.get(3) {
                Some(j) => serde_json::from_str(j).context("params must be JSON")?,
                None => Value::Object(Default::default()),
            };
            let out = s.execute(&arg(2)?, params)?;
            if s.is_dirty() {
                s.save(&path)?;
            }
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        "import" => {
            let path = PathBuf::from(arg(1)?);
            let wav = PathBuf::from(arg(2)?);
            let mut s = session_for(&path)?;
            let name = wav.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Sound".into());
            let asset = s.import_wav_bytes(&name, &std::fs::read(&wav).with_context(|| format!("reading {}", wav.display()))?, None)?;
            let out = if flag(&args, "--as").as_deref() == Some("instrument") {
                s.execute("sampler.load", serde_json::json!({"asset": asset, "name": name}))?
            } else {
                let bar: f64 = flag(&args, "--at").and_then(|v| v.parse().ok()).unwrap_or(1.0);
                s.execute("audio.place", serde_json::json!({"asset": asset, "start_bar": bar}))?
            };
            s.save(&path)?;
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        "render" => {
            let s = session_for(Path::new(&arg(1)?))?;
            let out = PathBuf::from(arg(2)?);
            let sr: u32 = flag(&args, "--sr").and_then(|v| v.parse().ok()).unwrap_or(48_000);
            let t = std::time::Instant::now();
            let frames = m2_render::render_song(s.song().clone(), &s.registry, s.bank(), sr, 2.0);
            m2_render::write_wav(&out, &frames, sr)?;
            let peak = frames.iter().fold(0.0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
            println!(
                "rendered {:.1}s to {} in {:.2}s (peak {:.1} dBFS)",
                frames.len() as f32 / sr as f32,
                out.display(),
                t.elapsed().as_secs_f32(),
                20.0 * peak.max(1e-9).log10()
            );
        }
        "plugin" => match args.get(1).map(String::as_str) {
            Some("add") => {
                let (mut reg, _) = Session::default_registry();
                let id = m2_engine::install_github_plugin(&mut reg, &arg(2)?)?;
                println!("installed {id}");
            }
            Some("list") => {
                let (reg, errors) = Session::default_registry();
                for p in reg.list() {
                    println!("{:28} {:10} {}", p.id, format!("{:?}", p.kind).to_lowercase(), p.description);
                }
                for e in errors {
                    eprintln!("warning: {e}");
                }
            }
            Some("check") => {
                let text = std::fs::read_to_string(arg(2)?)?;
                let def = m2_graph::parse(&text)?;
                m2_graph::validate(&def)?;
                println!("ok: {} ({:?}, {} macros)", def.id, def.kind, def.macros.len());
            }
            _ => bail!("{USAGE}"),
        },
        _ => print!("{USAGE}"),
    }
    Ok(())
}

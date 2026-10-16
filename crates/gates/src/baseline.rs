//! S, HEAD `8aa301e`, the commit before step 7: G7f's baseline (RENDER-32), measured fresh in every
//! run. S's runner (`crates/gates/s/`, built from a clean copy of S by its `build.sh`) plays it in a
//! process of its own and takes turns with the players here over a pipe ([`wire`]), so the app and
//! S are timed over the same music in the same minutes. Only the reference Mac judges the timings
//! (PLAT-1).
//! `$M2_GATES_S` names the runner; ci/check.sh builds it and sets that.

use std::io::BufReader;
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};

use crate::paired::Side;

#[path = "../s/wire.rs"]
pub mod wire;

/// The commit S is, which `crates/gates/s/build.sh` builds the runner at.
pub const S: &str = "8aa301e";

/// The variable that names S's runner.
pub const RUNNER: &str = "M2_GATES_S";

/// The code of the runner this tree holds: a runner built from other code is refused.
pub fn code() -> u64 {
    wire::code(include_bytes!("../s/main.rs"), include_bytes!("../s/wire.rs"))
}

/// S's runner, started.
pub struct Runner {
    child: Child,
    client: wire::Client<BufReader<ChildStdout>, ChildStdin>,
    path: String,
}

/// What S's renderer build said.
#[derive(Clone, Copy, Debug)]
pub struct Built {
    pub build_ms: f64,
    pub count: usize,
}

impl Runner {
    /// Start the runner `$M2_GATES_S` names, its scratch folder under `work`.
    pub fn start(work: &Path) -> Result<Runner> {
        Runner::start_at(std::env::var_os(RUNNER), work)
    }

    /// Start the runner at `path`. It plays S's own std, whatever `$M2_STD` says.
    fn start_at(path: Option<std::ffi::OsString>, work: &Path) -> Result<Runner> {
        let path = path.filter(|p| !p.is_empty()).with_context(|| {
            format!(
                "S's runner isn't named: crates/gates/s/build.sh builds it and prints its path, and {RUNNER} names that path (ci/check.sh does both)"
            )
        })?;
        let path = Path::new(&path).display().to_string();
        let mut child = Command::new(&path)
            .arg(work.join("s"))
            .env_remove("M2_STD")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| format!("starting S's runner {path}"))?;
        let (input, output) = (child.stdout.take().context("its stdout")?, child.stdin.take().context("its stdin")?);
        let (client, code, s) = match wire::Client::new(BufReader::new(input), output) {
            Ok(c) => c,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("S's runner {path}: {e}");
            }
        };
        let runner = Runner { child, client, path };
        if code != self::code() {
            bail!(
                "S's runner {} was built from other code than this tree's crates/gates/s: build it again (crates/gates/s/build.sh)",
                runner.path
            );
        }
        if s != S {
            bail!("S's runner {} plays {s}, not S ({S}): build it with crates/gates/s/build.sh", runner.path);
        }
        Ok(runner)
    }

    /// Where it runs from, and its process.
    pub fn about(&self) -> String {
        format!("HEAD {S}, played by its runner {} (process {})", self.path, self.child.id())
    }

    fn ask(&mut self, cmd: wire::Cmd) -> Result<wire::Reply> {
        self.client.ask(&cmd).map_err(|e| anyhow!("{e}"))
    }

    fn ok(&mut self, cmd: wire::Cmd) -> Result<Vec<(String, f64)>> {
        match self.ask(cmd)? {
            wire::Reply::Ok(values) => Ok(values),
            other => bail!("S's runner said {:?}, not ok", other.line()),
        }
    }

    /// S's player, playing the project in `dir` from `from` seconds, `frames` frames a callback.
    pub fn player(&mut self, dir: &Path, from: f64, frames: usize) -> Result<()> {
        self.ok(wire::Cmd::Player { from, frames, dir: dir.to_path_buf() }).map(drop)
    }

    /// S's renderer of the project in `dir`, its transport at `from` seconds.
    pub fn renderer(&mut self, dir: &Path, from: f64) -> Result<Built> {
        let values = self.ok(wire::Cmd::Renderer { from, dir: dir.to_path_buf() })?;
        let get = |k| crate::paired::value(&values, k).with_context(|| format!("S's renderer didn't say its {k}"));
        Ok(Built { build_ms: get("build_ms")?, count: get("count")? as usize })
    }

    /// Drop S's player or renderer: how long that took, in ms.
    pub fn drop_it(&mut self) -> Result<f64> {
        let values = self.ok(wire::Cmd::Drop)?;
        crate::paired::value(&values, "drop_ms").context("S's runner didn't say how long its drop took")
    }
}

impl Side for Runner {
    fn warm(&mut self, n: usize) -> Result<()> {
        self.ok(wire::Cmd::Warm(n)).map(drop)
    }

    fn turn(&mut self, lead: usize, n: usize) -> Result<Vec<f64>> {
        match self.ask(wire::Cmd::Turn { lead, n })? {
            wire::Reply::Times(t) => Ok(t),
            other => bail!("S's runner said {:?}, not its times", other.line()),
        }
    }

    fn heard(&mut self) -> Result<wire::Heard> {
        match self.ask(wire::Cmd::Hash)? {
            wire::Reply::Heard(h) => Ok(h),
            other => bail!("S's runner said {:?}, not what it heard", other.line()),
        }
    }
}

impl Drop for Runner {
    /// Asked to stop, it gets 10 s; then it is stopped.
    fn drop(&mut self) {
        self.client.quit();
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RENDER-32: the runner is built at S, the commit before step 7, by the script in its folder.
    #[test]
    fn the_runner_is_built_at_s() {
        let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("s/build.sh")).unwrap();
        assert!(script.lines().any(|l| l == format!("S={S}")), "build.sh builds S at {S}");
        assert!(script.contains("--locked"), "with S's own Cargo.lock");
    }

    // A missing runner is named in plain words, never measured around.
    #[test]
    fn without_a_runner_it_says_how_to_build_one() {
        let e = Runner::start_at(None, &std::env::temp_dir()).err().expect("no runner");
        assert!(e.to_string().contains("crates/gates/s/build.sh"), "{e}");
        let e = Runner::start_at(Some("/no/such/s_runner".into()), &std::env::temp_dir()).err().expect("no runner");
        assert!(format!("{e:#}").starts_with("starting S's runner /no/such/s_runner: "), "{e:#}");
    }

    /// A runner made of `script`, a shell script, in a new folder of its own.
    #[cfg(unix)]
    fn script(name: &str, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("m2-gates-runner-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s_runner");
        std::fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    // RENDER-32: a runner built from other code than this tree's, or at another commit than S, is
    // refused before anything is measured; the right one is asked and answers over its pipe, and is
    // told to stop when let go.
    #[cfg(unix)]
    #[test]
    fn only_this_trees_runner_at_s_is_used() {
        let work = std::env::temp_dir();
        let old = script("old", "echo 'ready 0000000000000001 8aa301e'\nread l\n");
        let e = Runner::start_at(Some(old.clone().into()), &work).err().expect("other code");
        assert!(e.to_string().contains("was built from other code than this tree's crates/gates/s"), "{e}");
        let other = script("other", &format!("echo 'ready {:016x} 0ebc6e6'\nread l\n", code()));
        let e = Runner::start_at(Some(other.clone().into()), &work).err().expect("another commit");
        assert!(e.to_string().contains("plays 0ebc6e6, not S (8aa301e)"), "{e}");
        let quiet = script("quiet", "exit 3\n");
        let e = Runner::start_at(Some(quiet.clone().into()), &work).err().expect("silent");
        assert!(e.to_string().contains("the runner stopped without answering"), "{e}");
        // Answers as S's runner does, and leaves a mark when told to stop.
        let done = work.join(format!("m2-gates-runner-done-{}", std::process::id()));
        let good = script(
            "good",
            &format!(
                "echo 'ready {:016x} 8aa301e'\nwhile read c a; do case $c in\n\
                 quit) touch '{}'; exit 0;;\n\
                 turn) echo 'times 1.5 2.5';;\n\
                 hash) echo 'heard 00000000000000ff 4';;\n\
                 renderer) echo 'ok build_ms=103.5 count=6058';;\n\
                 drop) echo 'ok drop_ms=2';;\n\
                 warm) echo ok;;\n\
                 *) echo \"error no $c\";;\n\
                 esac; done\n",
                code(),
                done.display()
            ),
        );
        let mut r = Runner::start_at(Some(good.clone().into()), &work).unwrap();
        let b = r.renderer(Path::new("/a lab/p50-0"), 180.0).unwrap();
        assert_eq!((b.build_ms, b.count), (103.5, 6058));
        r.warm(375).unwrap();
        assert_eq!(r.turn(1, 1).unwrap(), [1.5, 2.5]);
        assert_eq!(r.heard().unwrap(), wire::Heard { hash: 255, samples: 4 });
        assert_eq!(r.drop_it().unwrap(), 2.0);
        assert_eq!(r.player(Path::new("/a"), 180.0, 512).unwrap_err().to_string(), "S's runner: no player");
        drop(r);
        assert!(done.exists(), "told to stop");
        let _ = std::fs::remove_file(&done);
        for p in [old, other, quiet, good] {
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }
}

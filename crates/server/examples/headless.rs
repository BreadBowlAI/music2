//! The engine and UI server with no window, for automated UI tests.
//!
//!   M2_AUDIO=none cargo run -p m2-server --example headless -- <project-folder> <session.json> <token>
//!
//! Opens the project as a graph and writes `{url, token, pid}` to the session file.

use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [project, session, token] = &args[..] else { anyhow::bail!("usage: headless <project-folder> <session.json> <token>") };
    let running = m2_server::start(m2_server::ServeOptions {
        port: 0,
        web_dir: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/dist")),
        song: None,
        token: token.clone(),
    })?;
    running.state.graph.open(std::path::Path::new(project))?;
    let body = serde_json::json!({"url": running.url, "token": token, "pid": std::process::id()});
    std::fs::write(session, serde_json::to_string_pretty(&body)?)?;
    println!("{}", running.url);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

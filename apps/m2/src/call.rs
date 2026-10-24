//! The app's one route table from the command line (AGENT-8, AGENT-11): `m2 routes` prints it, with
//! no app running, and `m2 call` sends one route's request to the app that has the song open, as an
//! agent. So every song change the page can make is reachable from `m2`, those with no command of
//! their own too (an effect's bypass, markers, the song's key), and so is every read. The
//! musician's controls are refused before anything is sent, with the words the app would answer.

use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use m2_server::api::{self, Route};
use serde_json::{Value, json};

use crate::app;

/// `m2 routes [--json]`: every route of the app, the ones for agents first, each with its method,
/// its path, whether it writes, its words and an example body; or the table as JSON.
pub fn routes(args: &[String]) -> Result<()> {
    let all = api::all();
    if args.iter().skip(1).any(|a| a == "--json") {
        let table: Vec<Value> = all.iter().map(|r| r.json()).collect();
        return say(&format!("{}\n", serde_json::to_string_pretty(&table)?).into_bytes());
    }
    let mut text = String::from(
        "The open app's routes, from its one route table (AGENT-8). Send one to the app that has a song open\n\
         with m2 call <song> <path> [<json> | -]: the song's session is filled in when the body leaves it\n\
         out, and a GET's keys go in its query. A route that writes makes one undo step, the agent's,\n\
         named by --as and --why, and waits while the musician holds what it changes.\n\nFor agents:\n",
    );
    for r in all.iter().filter(|r| r.agents) {
        let writes = if r.writes { "  (writes)" } else { "" };
        text += &format!("  {} {}{writes}\n      {}\n      {}\n", r.method, r.path, r.words, r.example);
    }
    text += "\nThe musician's (an agent gets 403, AGENT-11):\n";
    for r in all.iter().filter(|r| !r.agents) {
        text += &format!("  {} {}\n      {}\n", r.method, r.path, r.words);
    }
    say(text.as_bytes())
}

/// `m2 call <song> <path> [<json> | -] [--method M]`: the route's request, sent to the app that has
/// the song open, as an agent (`--as`, `--why`). It prints the app's answer: JSON, or the bytes
/// it sent (a waveform's peaks). A status of 400 or more exits 1, busy 3.
pub fn call(args: &[String]) -> Result<()> {
    const USE: &str = "m2 call <song> <path> [<json> | -] [--method GET|POST|PUT]";
    let (mut method, mut given) = (None, vec![]);
    let mut words = args.iter().skip(1);
    while let Some(a) = words.next() {
        match a.as_str() {
            "--method" => {
                method = Some(words.next().with_context(|| format!("--method needs GET, POST or PUT: {USE}"))?.to_ascii_uppercase())
            }
            _ => given.push(a.as_str()),
        }
    }
    let [dir, path, body @ ..] = given.as_slice() else { bail!("{USE}") };
    if body.len() > 1 {
        bail!("one body, as JSON, in one argument: {USE}");
    }
    let (path, query) = path.split_once('?').map_or((*path, None), |(p, q)| (p, Some(q)));
    let route = route(path, method.as_deref())?;
    // The musician's: refused before anything is sent, as the app would refuse it (AGENT-11).
    if !route.agents {
        bail!("{}", route.refusal());
    }
    let mut body: Value = match body.first().copied() {
        None => json!({}),
        Some("-") => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).context("reading the body from standard input")?;
            serde_json::from_str(&text).context("the body on standard input isn't JSON")?
        }
        Some(text) => serde_json::from_str(text).with_context(|| format!("the body isn't JSON: {text}"))?,
    };
    let fields = body.as_object_mut().ok_or_else(|| anyhow!("the body is a JSON object, {{\"key\": value, …}}, as m2 routes shows"))?;
    let mut app = app::looking(Path::new(dir))?;
    // The song's session, when the route takes one and the body leaves it out.
    if route.example().get("session").is_some() && !fields.contains_key("session") {
        fields.insert("session".into(), json!(app.session()));
    }
    let (status, answer) = if route.method == "GET" {
        let keys = fields.iter().map(|(k, v)| {
            let v = v.as_str().map_or_else(|| v.to_string(), str::to_string);
            format!("{}={}", m2_server::caller::encoded(k), m2_server::caller::encoded(&v))
        });
        let query: Vec<String> = query.map(str::to_string).into_iter().chain(keys).collect();
        let at = if query.is_empty() { path.to_string() } else { format!("{path}?{}", query.join("&")) };
        app.call("GET", &at, None, false)?
    } else {
        let at = query.map_or_else(|| path.to_string(), |q| format!("{path}?{q}"));
        app.call(route.method, &at, Some(&body), route.writes)?
    };
    let json = serde_json::from_slice::<Value>(&answer).ok();
    match &json {
        Some(v) => say(&format!("{}\n", serde_json::to_string_pretty(v)?).into_bytes())?,
        None => say(&answer)?,
    }
    if status >= 400 {
        let said = json
            .as_ref()
            .map(app::words)
            .unwrap_or_else(|| format!("the app answered {status}: {}", String::from_utf8_lossy(&answer).trim()));
        bail!("{said}");
    }
    // A step: said on stderr, so what is printed stays the app's answer alone.
    if let Some(made) = json.as_ref().and_then(|v| v.get("undo_name")).filter(|s| route.writes && s.is_object()) {
        eprintln!("in the open app: one undo step, {}", app::step_said(made));
    }
    Ok(())
}

/// The route `path` names, by `method` when it is given: a path with two methods needs it.
fn route(path: &str, method: Option<&str>) -> Result<&'static Route> {
    let all: Vec<&Route> = api::all().into_iter().filter(|r| r.path == path).collect();
    let methods = || all.iter().map(|r| r.method).collect::<Vec<_>>().join(" or ");
    match (method, all.as_slice()) {
        (_, []) => bail!("the app has no route {path}: m2 routes lists them"),
        (Some(m), _) => all.iter().copied().find(|r| r.method == m).ok_or_else(|| anyhow!("{path} takes {}, not {m}", methods())),
        (None, [one]) => Ok(one),
        (None, _) => bail!("{path} takes {}: say which with --method", methods()),
    }
}

/// Write `bytes` to standard output; whoever reads it going away ends it quietly.
fn say(bytes: &[u8]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e.into()),
        _ => Ok(()),
    }
}

//! The plugin list through a running server (gaps #32, HOST-3): the host process's scan, kept for
//! the app run, scanned again on Look again. In a process of its own: it checks that this process
//! mapped no plugin library, which the unit tests' in-process plugins would.

use std::io::{Read, Write};
use std::sync::Arc;

use serde_json::Value;

use m2_server::graph::Graph;
use m2_server::{ServeOptions, start_with};

/// `method path` with the token: the status and the JSON answer.
fn ask(url: &str, method: &str, path: &str) -> (u16, Value) {
    let addr = url.trim_start_matches("http://");
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let req = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nx-m2-token: t0k\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut answer = String::new();
    s.read_to_string(&mut answer).unwrap();
    let status = answer[9..12].parse().unwrap();
    (status, answer.split_once("\r\n\r\n").map_or(Value::Null, |(_, b)| serde_json::from_str(b).unwrap_or(Value::Null)))
}

// HOST-3, gaps #32: `GET /api/plugins` lists what the host process's scan found: the probe, as an
// instrument and an effect, by its `clap:` id, name, vendor and version. The scan ran in the host
// process: it is running, and this process mapped no plugin library. Look again
// (`POST /api/plugins/scan`) scans again and answers the same.
#[test]
fn the_plugin_list_comes_from_the_host_scan() {
    m2_plugins::probe::install();
    let remote = m2_plugins::Remote::for_tests();
    let host: Arc<dyn m2_plugins::PluginHost> = Arc::new(remote.clone());
    let player = m2_play::Player::silent().expect("a silent player");
    let dir = std::env::temp_dir().join(format!("m2-routes-plugins-{}", std::process::id()));
    let running =
        start_with(ServeOptions { port: 0, web_dir: dir.join("no-web"), token: "t0k".into() }, Graph::with_player_on(player, host))
            .unwrap();
    assert!(remote.process_id().is_none(), "no host process before the list is asked for");

    let (status, list) = ask(&running.url, "GET", "/api/plugins");
    assert_eq!(status, 200, "{list}");
    let probe =
        list["plugins"].as_array().unwrap().iter().find(|p| p["id"] == "clap:m2.probe").unwrap_or_else(|| panic!("the probe: {list:#}"));
    assert_eq!(
        (&probe["name"], &probe["vendor"], &probe["version"], &probe["instrument"], &probe["effect"]),
        (&Value::from("m2 probe"), &Value::from("music2"), &Value::from("1.0.0"), &Value::from(true), &Value::from(true)),
        "{probe:#}"
    );
    assert!(list["folder"].as_str().is_some_and(|f| !f.is_empty()), "where plugins are installed: {list:#}");
    assert!(remote.process_id().is_some(), "the host process scanned");

    let (status, again) = ask(&running.url, "POST", "/api/plugins/scan");
    assert_eq!((status, &again["plugins"]), (200, &list["plugins"]), "{again:#}");

    // The app never loads plugin code (Linux: what this process mapped).
    if let Ok(maps) = std::fs::read_to_string("/proc/self/maps") {
        assert!(!maps.lines().any(|l| l.contains(".clap")), "a plugin library is mapped in the app's process");
    }
    remote.shutdown();
}

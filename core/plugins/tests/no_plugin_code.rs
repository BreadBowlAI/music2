//! HOST-3: a program that hosts plugins in the host process never loads plugin code. A binary of
//! its own: no other test here may load a `.clap` in this process.

use std::path::PathBuf;
use std::sync::Arc;

use m2_plugins::{Events, Found, Owner, PluginHost, Remote, SharedBuffers, Transport};

fn play(host: &dyn PluginHost, h: m2_plugins::Handle) {
    let (ev, mut back) = (Events::with_capacity(4), Events::with_capacity(4));
    let input = [0.25f32; 128];
    let (mut l, mut r) = ([0.0f32; 128], [0.0f32; 128]);
    for c in 0..4 {
        let t = Transport { bpm: 120.0, playing: true, steady: c * 128, ..Transport::default() };
        let mut io = SharedBuffers { inputs: [&input, &input], n_in: 2, outputs: [&mut l, &mut r], n_out: 2, frames: 128 };
        host.process(h, &mut io, &ev, &mut back, &t);
    }
}

// HOST-3: the app's side never loads plugin code: no `.clap` mapped here after the host process
// made, played and destroyed Surge XT and Airwindows (Linux: /proc/self/maps). Red with
// `InProcess` in `Remote`'s place.
#[test]
fn the_app_never_loads_plugin_code() {
    m2_plugins::test_main_thread();
    let r: Arc<dyn PluginHost> = Arc::new(Remote::new(PathBuf::from(env!("CARGO_BIN_EXE_m2-plugin-host")), vec![]));
    let (found, _) = r.scan();
    let real: Vec<&Found> =
        found.iter().filter(|f| f.id == "org.surge-synth-team.surge-xt" || f.id == "com.airwindows.consolidated").collect();
    if real.len() < 2 {
        eprintln!("the_app_never_loads_plugin_code: needs Surge XT and Airwindows in CLAP_PATH; only the scan was checked");
    }
    let (owner, _events) = Owner::new();
    for f in &real {
        let (h, _) = r.create(f, None, &owner).unwrap();
        r.activate(h, 48_000, 128).unwrap();
        play(&*r, h);
        r.destroy(h);
    }
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let loaded: Vec<&str> = maps.lines().filter(|l| l.contains(".clap")).collect();
    assert!(loaded.is_empty(), "plugin code is mapped in the app's process: {loaded:#?}");
}

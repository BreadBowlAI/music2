//! FMT-23: music2.app carries m2 beside its own program, `Contents/MacOS/m2`, built by the same
//! `tauri build` that makes the app, where a new song's AGENTS.md finds it
//! (`m2_server::resources::m2_program`, whose tests fake such a bundle). The bundle itself is made
//! only on a Mac, so this pins the config to the script that builds m2, on every OS.

use std::path::{Path, PathBuf};

fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn config() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(here().join("tauri.conf.json")).unwrap()).unwrap()
}

/// `p` relative to this folder, where tauri.conf.json is, with `..` taken out.
fn from_here(p: &str) -> PathBuf {
    let mut out = here();
    for part in Path::new(p).components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

#[test]
fn the_app_bundle_carries_the_m2_its_build_makes() {
    let c = config();
    // Bundled beside the app's program, `Contents/MacOS/music2`, from the release m2 in the
    // source tree's target/.
    let source = c["bundle"]["macOS"]["files"]["MacOS/m2"].as_str().expect("bundle.macOS.files names MacOS/m2");
    let root = here().join("../..").canonicalize().unwrap();
    assert_eq!(from_here(source), root.join("target/release/m2"));
    // Made by the script tauri build runs before it bundles, from the repo's top folder.
    let hook = &c["build"]["beforeBundleCommand"];
    assert_eq!(hook["script"], "bash apps/music2-desktop/bundle-m2.sh");
    assert_eq!(from_here(hook["cwd"].as_str().unwrap()).canonicalize().unwrap(), root);
    let script = std::fs::read_to_string(root.join("apps/music2-desktop/bundle-m2.sh")).unwrap();
    assert!(script.contains("cargo build --locked --release -p m2 --target-dir target"), "the script builds m2 where the config looks");
    assert!(script.contains("codesign --force --sign - target/release/m2"), "and signs it as the app is signed");
    // Not an external binary: Tauri's build script copies those at every cargo build of the app,
    // which would then fail until m2 is built.
    assert!(c["bundle"]["externalBin"].is_null());
}

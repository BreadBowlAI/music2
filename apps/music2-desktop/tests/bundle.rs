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
    // Not an external binary: Tauri's build script copies those at every cargo build of the app,
    // which would then fail until m2 is built.
    assert!(c["bundle"]["externalBin"].is_null());
}

/// What `bundle-m2.sh --sign-args` says it signs m2 with, one argument a line, for the config
/// `conf` and the environment `env` (Tauri's own signing variables cleared first); or its refusal.
fn sign_args(conf: &Path, env: &[(&str, &str)]) -> Result<Vec<String>, String> {
    let mut c = std::process::Command::new("bash");
    c.arg(here().join("bundle-m2.sh")).arg("--sign-args").env("M2_TAURI_CONF", conf);
    for v in ["APPLE_SIGNING_IDENTITY", "APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD"] {
        c.env_remove(v);
    }
    let out = c.envs(env.iter().copied()).output().unwrap();
    if out.status.success() {
        Ok(String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// A tauri.conf.json holding only `mac` as bundle.macOS, in a folder of its own.
fn conf_with(tag: &str, mac: serde_json::Value) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m2-bundle-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tauri.conf.json");
    std::fs::write(&path, serde_json::json!({"bundle": {"macOS": mac}}).to_string()).unwrap();
    path
}

// FMT-23, the review's R4: m2 inside the app is signed as the app is, before Tauri seals the
// bundle (Tauri signs only its own programs, not bundle.macOS.files): the same identity
// (APPLE_SIGNING_IDENTITY over signingIdentity), the hardened runtime when it is on, with the
// app's entitlements (m2 compiles std's kernels too), and a secure timestamp for a real identity.
#[test]
fn m2_is_signed_with_the_apps_own_signing() {
    let ours = here().join("tauri.conf.json");
    assert_eq!(sign_args(&ours, &[]).unwrap(), ["--force", "--sign", "-"], "ad hoc, as the app is today");
    assert_eq!(
        sign_args(&ours, &[("APPLE_SIGNING_IDENTITY", "Developer ID Application: Me (AB12)")]).unwrap(),
        ["--force", "--sign", "Developer ID Application: Me (AB12)", "--timestamp"]
    );
    let hardened = conf_with("hardened", serde_json::json!({"signingIdentity": "Dev Me", "entitlements": "e.plist"}));
    assert_eq!(
        sign_args(&hardened, &[]).unwrap(),
        [
            "--force",
            "--sign",
            "Dev Me",
            "--timestamp",
            "--options",
            "runtime",
            "--entitlements",
            hardened.with_file_name("e.plist").to_str().unwrap()
        ],
        "Tauri's hardened runtime is on unless the config says false"
    );
    let unsigned = conf_with("unsigned", serde_json::json!({"hardenedRuntime": false}));
    assert_eq!(sign_args(&unsigned, &[]).unwrap(), Vec::<String>::new(), "no identity: the app isn't signed, nor is m2");
    // A certificate Tauri imports into a keychain of its own, as it bundles, can't sign m2 first.
    let refused = sign_args(&ours, &[("APPLE_CERTIFICATE", "base64"), ("APPLE_SIGNING_IDENTITY", "Me")]).unwrap_err();
    assert!(refused.contains("APPLE_CERTIFICATE"), "{refused}");
    for c in [hardened, unsigned] {
        let _ = std::fs::remove_dir_all(c.parent().unwrap());
    }
}

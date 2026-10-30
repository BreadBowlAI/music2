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

// PLAT-8, FMT-23: the Linux packages carry the same m2 at /usr/bin/m2, beside the app's own
// program, /usr/bin/music2; their resources land in /usr/lib/music2, where
// `m2_server::resources` looks (`<program's folder>/../lib/music2`). A plain `tauri build` makes
// the app on a Mac and the .deb and the AppImage on Linux.
#[test]
fn the_linux_packages_carry_the_m2_its_build_makes() {
    let c = config();
    let root = here().join("../..").canonicalize().unwrap();
    for package in ["deb", "appimage"] {
        let files = &c["bundle"]["linux"][package]["files"];
        let source = files["/usr/bin/m2"].as_str().unwrap_or_else(|| panic!("bundle.linux.{package}.files names /usr/bin/m2"));
        assert_eq!(from_here(source), root.join("target/release/m2"), "{package}");
    }
    let targets: Vec<&str> = c["bundle"]["targets"].as_array().unwrap().iter().map(|t| t.as_str().unwrap()).collect();
    for t in ["app", "deb", "appimage"] {
        assert!(targets.contains(&t), "bundle.targets has {t}: {targets:?}");
    }
    // Tauri names the resource folder after the product: /usr/lib/<productName>. The program is
    // /usr/bin/music2 (music2.app/Contents/MacOS/music2), so `music2` starts it from a shell.
    assert_eq!(c["productName"], "music2");
    assert_eq!(c["mainBinaryName"], "music2");
    // What the programs link beyond what Tauri adds itself (WebKitGTK and GTK): ALSA and D-Bus;
    // and the glibc they need, the newest version among their symbols that aren't weak
    // (ci/release.sh check computes it from the built programs and refuses a .deb that differs),
    // so an older system is told by apt, not by a crash (the review's L1).
    let depends: Vec<&str> = c["bundle"]["linux"]["deb"]["depends"].as_array().unwrap().iter().map(|d| d.as_str().unwrap()).collect();
    assert_eq!(depends, ["libc6 (>= 2.35)", "libasound2", "libdbus-1-3"]);
    // The .deb's metadata (L9): a maintainer with an address, words beyond the one line, and no
    // icon in a size folder icon themes don't know (an `@2x` file lands in `256x256@2`).
    assert!(c["bundle"]["publisher"].as_str().is_some_and(|p| p.contains('<') && p.ends_with('>')), "bundle.publisher is a name <address>");
    assert!(c["bundle"]["longDescription"].as_str().is_some_and(|d| d.len() > 80));
    for icon in c["bundle"]["icon"].as_array().unwrap() {
        assert!(!icon.as_str().unwrap().contains('@'), "{icon}: Tauri files it in a size folder themes ignore");
    }
}

// PLAT-8, the review's L3: every download carries music2's own licences and the notices of the
// code inside it that others wrote, which `bundle-m2.sh` makes as it bundles (ci/notices.mjs, from
// Cargo.lock's crates and the UI's npm packages, so it can't drift): music2.app in
// Contents/Resources, the .deb in /usr/share/doc/music2 with its copyright file, the AppImage in
// the same folder inside it (Tauri copies nothing to the AppImage's top folder).
#[test]
fn every_bundle_carries_the_licences_and_notices() {
    let c = config();
    let root = here().join("../..").canonicalize().unwrap();
    let want = |files: &serde_json::Value, at: &str, what: &str| {
        let name = format!("{at}{}", what.rsplit('/').next().unwrap());
        let source = files[name.as_str()].as_str().unwrap_or_else(|| panic!("no {name}"));
        assert_eq!(from_here(source), root.join(what), "{name}");
    };
    let all = ["LICENSE-MIT", "LICENSE-APACHE", "target/notices/THIRD-PARTY-NOTICES.txt"];
    for (files, at) in [
        (&c["bundle"]["macOS"]["files"], "Resources/"),
        (&c["bundle"]["linux"]["deb"]["files"], "/usr/share/doc/music2/"),
        (&c["bundle"]["linux"]["appimage"]["files"], "/usr/share/doc/music2/"),
    ] {
        for what in all {
            want(files, at, what);
        }
    }
    want(&c["bundle"]["linux"]["deb"]["files"], "/usr/share/doc/music2/", "apps/music2-desktop/copyright");
    let script = std::fs::read_to_string(here().join("bundle-m2.sh")).unwrap();
    assert!(script.contains("node ci/notices.mjs"), "the hook makes the notices");
}

// PLAT-8, LIB-8: the bundle carries std's kit and the samples.toml that lists it, beside std's
// code and presets, and every file of the kit is one the config's pattern takes.
#[test]
fn the_bundle_carries_stds_kit() {
    let c = config();
    let resources = c["bundle"]["resources"].as_object().unwrap();
    assert_eq!(resources["../../std/samples.toml"], "std/samples.toml");
    assert_eq!(resources["../../std/samples/kit/*.wav"], "std/samples/kit/");
    let kit = from_here("../../std/samples/kit");
    let files: Vec<String> = std::fs::read_dir(&kit).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(files.len() >= 12 && files.iter().all(|f| f.ends_with(".wav")), "{files:?}");
}

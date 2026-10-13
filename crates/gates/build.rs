//! Records which build the gates were measured with, so every run can print it: timings from
//! a dev build and a release build differ several times over.

use std::path::Path;

fn main() {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let root = Path::new(&var("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let manifest = std::fs::read_to_string(&root).unwrap_or_default();
    // The crates the audio thread runs, raised to opt-level 3 in dev builds by step 3a's design (§11).
    let raised = ["m2-runtime", "m2-play", "m2-plugins", "m2-song"]
        .iter()
        .filter(|p| {
            manifest.contains(&format!("[profile.dev.package.{p}]")) || manifest.contains(&format!("[profile.dev.package.\"{p}\"]"))
        })
        .count();
    println!("cargo:rustc-env=M2_GATES_PROFILE={}", var("PROFILE"));
    println!("cargo:rustc-env=M2_GATES_OPT_LEVEL={}", var("OPT_LEVEL"));
    println!("cargo:rustc-env=M2_GATES_RAISED={raised}");
    println!("cargo:rerun-if-changed={}", root.display());
}

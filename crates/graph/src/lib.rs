//! L2 — the `m2plug` plugin standard: manifest types, built-in nodes, graph compiler,
//! device runtimes, and the plugin registry.
//!
//! A plugin is one JSON file describing a graph of built-in nodes plus a few macro
//! knobs. Plugins are data, so loading one can never run code.

pub mod def;
pub mod nodes;
pub mod runtime;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};

pub use def::*;
pub use nodes::{Ctx, NODE_TYPES, SampleData};
pub use runtime::{BLOCK, DeviceRt, MacroValues, resolve_macros};

/// Built-in plugins, embedded at compile time from `/plugins`.
const BUILTINS: &[&str] = &[
    include_str!("../../../plugins/pluck.m2plug.json"),
    include_str!("../../../plugins/lead.m2plug.json"),
    include_str!("../../../plugins/pad.m2plug.json"),
    include_str!("../../../plugins/bass.m2plug.json"),
    include_str!("../../../plugins/keys.m2plug.json"),
    include_str!("../../../plugins/strings.m2plug.json"),
    include_str!("../../../plugins/sampler.m2plug.json"),
    include_str!("../../../plugins/drums.m2plug.json"),
    include_str!("../../../plugins/space.m2plug.json"),
    include_str!("../../../plugins/echo.m2plug.json"),
    include_str!("../../../plugins/pump.m2plug.json"),
    include_str!("../../../plugins/drive.m2plug.json"),
    include_str!("../../../plugins/shimmer.m2plug.json"),
    include_str!("../../../plugins/eq.m2plug.json"),
    include_str!("../../../plugins/glue.m2plug.json"),
    include_str!("../../../plugins/duck.m2plug.json"),
    include_str!("../../../plugins/filter.m2plug.json"),
    include_str!("../../../plugins/master.m2plug.json"),
];

#[derive(Clone, Default)]
pub struct Registry {
    plugins: BTreeMap<String, Arc<PluginDef>>,
}

impl Registry {
    pub fn with_builtins() -> Registry {
        let mut r = Registry::default();
        for src in BUILTINS {
            let def = parse(src).expect("built-in plugin parses");
            r.add(def).expect("built-in plugin validates");
        }
        r
    }

    /// Validate (by compiling once) and register. Replaces any plugin with the same id.
    pub fn add(&mut self, def: PluginDef) -> Result<()> {
        validate(&def).with_context(|| format!("plugin `{}`", def.id))?;
        self.plugins.insert(def.id.clone(), Arc::new(def));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<Arc<PluginDef>> {
        self.plugins.get(id).cloned()
    }

    pub fn list(&self) -> impl Iterator<Item = &Arc<PluginDef>> {
        self.plugins.values()
    }

    /// Load every `*.m2plug.json` / `m2plug.json` in `dir`. Returns per-file errors.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else { return errors };
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !name.ends_with(".json") {
                continue;
            }
            let res = std::fs::read_to_string(&path).map_err(anyhow::Error::from).and_then(|s| parse(&s)).and_then(|d| self.add(d));
            if let Err(err) = res {
                errors.push(format!("{}: {err:#}", path.display()));
            }
        }
        errors
    }
}

pub fn parse(json: &str) -> Result<PluginDef> {
    let def: PluginDef = serde_json::from_str(json).context("invalid m2plug JSON")?;
    if def.id.trim().is_empty() {
        bail!("plugin id is empty");
    }
    Ok(def)
}

/// Compile the plugin once with default macros and placeholder samples.
pub fn validate(def: &PluginDef) -> Result<()> {
    let major = def.m2plug.split('.').next().unwrap_or("");
    if major != SPEC_VERSION.split('.').next().unwrap_or("") {
        return Err(anyhow!("unsupported m2plug version {} (this build supports {SPEC_VERSION})", def.m2plug));
    }
    let mut seen = std::collections::HashSet::new();
    for m in &def.macros {
        if !seen.insert(&m.id) {
            bail!("duplicate macro `{}`", m.id);
        }
    }
    let macros = resolve_macros(def, &BTreeMap::new());
    let samples = BTreeMap::new();
    let mut rt = DeviceRt::new(def, &macros, &samples, 48_000.0)?;
    // Smoke-run a block so obviously broken graphs fail at install time, not playback.
    let ctx = Ctx { sr: 48_000.0, bpm: 120.0, beat_pos: 0.0, note_freq: 261.6, velocity: 0.8, gate: true };
    let mut buf = vec![[0.0f32; 2]; BLOCK];
    rt.note_on(60, 100);
    rt.process(&ctx, &mut buf, BLOCK);
    if buf.iter().any(|f| !f[0].is_finite() || !f[1].is_finite()) {
        bail!("produces non-finite samples");
    }
    Ok(())
}

/// Turn `github:owner/repo[/path/to/file.json][@ref]` into a raw download URL and the
/// plugin id it will be registered under.
pub fn github_source(spec: &str) -> Result<(String, String)> {
    let rest = spec.strip_prefix("github:").ok_or_else(|| anyhow!("expected github:owner/repo"))?;
    let (path, git_ref) = match rest.split_once('@') {
        Some((p, r)) => (p, r),
        None => (rest, "HEAD"),
    };
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() < 2 {
        bail!("expected github:owner/repo");
    }
    let file = if parts.len() > 2 { parts[2..].join("/") } else { "m2plug.json".into() };
    let url = format!("https://raw.githubusercontent.com/{}/{}/{git_ref}/{file}", parts[0], parts[1]);
    let id_suffix = if parts.len() > 2 {
        format!("/{}", parts.last().unwrap().trim_end_matches(".json").trim_end_matches(".m2plug"))
    } else {
        String::new()
    };
    Ok((url, format!("github:{}/{}{id_suffix}", parts[0], parts[1])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_load_and_make_sound() {
        let reg = Registry::with_builtins();
        assert!(reg.get("m2/pluck").is_some());
        for def in reg.list() {
            if def.kind == PluginKind::Effect || !def.asset_slots.is_empty() {
                continue;
            }
            let macros = resolve_macros(def, &BTreeMap::new());
            let mut rt = DeviceRt::new(def, &macros, &BTreeMap::new(), 48_000.0).unwrap();
            rt.note_on(if def.kind == PluginKind::Kit { 36 } else { 60 }, 110);
            let ctx = Ctx { sr: 48_000.0, bpm: 120.0, beat_pos: 0.0, note_freq: 0.0, velocity: 0.0, gate: false };
            let mut peak = 0.0f32;
            let mut buf = vec![[0.0f32; 2]; BLOCK];
            for _ in 0..200 {
                rt.process(&ctx, &mut buf, BLOCK);
                for f in &buf {
                    peak = peak.max(f[0].abs()).max(f[1].abs());
                }
            }
            assert!(peak > 0.02, "{} is silent ({peak})", def.id);
            assert!(peak < 4.0, "{} is too loud ({peak})", def.id);
        }
    }

    #[test]
    fn rejects_cycles_and_unknowns() {
        let bad = r#"{"m2plug":"0.1","id":"x","name":"x","kind":"effect",
            "graph":{"nodes":[{"id":"a","type":"gain","inputs":{"in":"b"}},{"id":"b","type":"gain","inputs":{"in":"a"}}],"out":"a"}}"#;
        assert!(validate(&parse(bad).unwrap()).unwrap_err().to_string().contains("cycle"));
        let bad = r#"{"m2plug":"0.1","id":"x","name":"x","kind":"effect",
            "graph":{"nodes":[{"id":"a","type":"gain","params":{"loud":3}}],"out":"a"}}"#;
        assert!(format!("{:#}", validate(&parse(bad).unwrap()).unwrap_err()).contains("unknown param"));
    }

    #[test]
    fn github_specs() {
        let (url, id) = github_source("github:me/glitter-pad").unwrap();
        assert_eq!(url, "https://raw.githubusercontent.com/me/glitter-pad/HEAD/m2plug.json");
        assert_eq!(id, "github:me/glitter-pad");
        let (url, id) = github_source("github:me/pack/plugins/sparkle.m2plug.json@v1").unwrap();
        assert_eq!(url, "https://raw.githubusercontent.com/me/pack/v1/plugins/sparkle.m2plug.json");
        assert_eq!(id, "github:me/pack/sparkle");
    }
}

//! Audio files in the open song (lane A): where an asset's file is, for its waveform's peaks.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use m2_song::AssetName;

use super::Graph;

impl Graph {
    /// The file asset `name` of the open song (session `token`) is: under its `assets/`. Reads the
    /// control plane only, never the session, so it never waits for a compile.
    pub fn asset_file(&self, token: u64, name: &str) -> Result<PathBuf> {
        let name = AssetName::new(name).map_err(anyhow::Error::msg)?;
        let cp = self.inner.control.lock();
        let p = cp.open.as_ref().context("no project is open")?;
        if p.engine.token != token {
            bail!("the open project changed; refresh the graph");
        }
        Ok(p.shown.dir.join(m2_song::path::ASSETS).join(name.as_str()))
    }
}

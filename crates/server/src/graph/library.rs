//! What the library (LIB-1, LIB-3) needs of the open song: the components its build offers, and the
//! packages it loaded.

use std::path::PathBuf;

use anyhow::Result;
use m2_script::Catalog;

use super::Graph;

/// A build's packages, by name and folder.
pub type Packages = Vec<(String, PathBuf)>;

impl Graph {
    /// The components the open song's build offers (its packages' and its own, PKG-11), and the
    /// packages it loaded; None with no song open.
    pub fn library(&self) -> Result<Option<(Catalog, Packages)>> {
        self.call(|a| {
            Ok(a.open.as_ref().map(|o| {
                let packages = o.session.packages.packages().map(|p| (p.name.clone(), p.dir.clone())).collect();
                (o.session.catalog().clone(), packages)
            }))
        })
    }
}

//! What the library (LIB-1) needs of the open song: the components its build offers.

use anyhow::Result;
use m2_script::Catalog;

use super::Graph;

impl Graph {
    /// The components the open song's build offers (its packages' and its own, PKG-11); None with
    /// no song open.
    pub fn catalog(&self) -> Result<Option<Catalog>> {
        self.call(|a| Ok(a.open.as_ref().map(|o| o.session.catalog().clone())))
    }
}

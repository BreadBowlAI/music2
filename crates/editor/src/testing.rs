//! What the editor's tests share: the test manifest, std, and copies of the fixtures.

use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_session::Session;

pub(crate) const MANIFEST: &str =
    "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"1\"\nstd = \"0.5\"\n";

pub(crate) fn std_packages() -> PackageSet {
    let mut p = PackageSet::default();
    p.load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std")).unwrap();
    p
}

/// A project folder, removed when dropped.
pub(crate) struct Fixture(pub(crate) PathBuf);

impl Fixture {
    pub(crate) fn new() -> Fixture {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Fixture(std::env::temp_dir().join(format!("m2-editor-test-{}-{n}", std::process::id())))
    }

    pub(crate) fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap()
    }

    pub(crate) fn write(&self, name: &str, text: impl AsRef<[u8]>) {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub(crate) fn open(&self) -> Session {
        Session::open(&self.0, std_packages()).unwrap_or_else(|d| panic!("{d:?}"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every file under `dir`, recursively.
pub(crate) fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) { out.extend(files_under(&e.path())) } else { out.push(e.path()) }
    }
    out
}

/// A copy of one of the repository's projects, `rel` from its root (`examples/first-song`).
pub(crate) fn fixture(rel: &str) -> Fixture {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    let f = Fixture::new();
    for file in files_under(&from) {
        f.write(file.strip_prefix(&from).unwrap().to_str().unwrap(), std::fs::read(&file).unwrap());
    }
    f
}

/// The session's test fixtures (`core/session/tests/fixtures`): `first-song`, `every-component`.
pub(crate) fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/session/tests/fixtures")
}

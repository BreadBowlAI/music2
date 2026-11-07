//! What the editor's tests share: the test manifest, std, and copies of the fixtures.

use std::path::{Path, PathBuf};

use m2_runtime::PackageSet;
use m2_session::Session;

pub(crate) const MANIFEST: &str = "[project]\nname = \"t\"\nformat = 1\nsample_rate = 48000\n\n[dependencies]\n\"@m2/core\" = \"2\"\nstd = \"0.10\"\n\"@m2/drums\" = \"2\"\n\"@m2/effects\" = \"1\"\n\"@m2/synths\" = \"2\"\n";

pub(crate) fn std_packages() -> PackageSet {
    let mut p = PackageSet::default().with_store(m2_song::pack::test_store());
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
        // Read-only files too: a pack installed in a store here (PACK-6).
        let _ = m2_song::pack::remove_tree(&self.0);
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

/// `@test/notes` in `dir`: two note effects, Arpeggiator and Chord, each over a kernel of the pack
/// that gives notes (`kernels`), or giving their notes on as they come (not).
pub(crate) fn notes_pack(dir: &Path, kernels: bool) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let mut manifest = "[package]\nname = \"@test/notes\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\n\
                        description = \"Note effects for music2's tests\"\nauthors = [\"music2\"]\ntags = [\"test\"]\n"
        .to_string();
    if kernels {
        // std's module, whose `sine` exports a kernel's functions; this manifest says it gives notes.
        manifest += "wasm = \"k.wasm\"\n";
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../std/kernels/std.wasm"), dir.join("k.wasm")).unwrap();
    }
    manifest += "\n[dependencies]\n\"@m2/core\" = \"2.5\"\nstd = \"0.10\"\n";
    if kernels {
        manifest += "\n[[kernel]]\nname = \"sine\"\ndescription = \"Gives notes\"\n\
                     inputs = [\"notes: events<note>\", \"tempo: control<bpm>\", \"gate: control<scalar> = 0.8\"]\n\
                     outputs = [\"out: events<note>\"]\n";
    }
    std::fs::write(dir.join("m2pkg.toml"), manifest).unwrap();
    let body = |gate: &str| match kernels {
        true => format!("kernel<EventsKind>(\"@test/notes:sine\", {{ notes, tempo: time.tempo, gate: {gate} }})"),
        false => "notes".to_string(),
    };
    let index = format!(
        "import {{ component, control, kernel, port, time, type Events, type EventsKind, type Params }} from \"@m2/core\";\n\n\
         /** Its notes, played one at a time. */\n\
         export const arp = component(\n  {{ id: \"arp\", label: \"Arpeggiator\", inputs: {{ notes: port.notes() }}, controls: {{ gate: control.scalar(0.8) }}, \
         output: port.notes(), editor: {{ category: \"Arpeggiator\" }} }},\n  (notes: Events, {{ gate }}: Params) => {},\n);\n\n\
         /** A chord for each note. */\n\
         export const chord = component(\n  {{ id: \"chord\", label: \"Chord\", inputs: {{ notes: port.notes() }}, output: port.notes(), \
         editor: {{ category: \"Chords\" }} }},\n  (notes: Events) => {},\n);\n",
        body("gate"),
        body("0.8")
    );
    std::fs::write(dir.join("src/index.ts"), index).unwrap();
}

//! RENDER-28 (8a): what the executor runs on the audio thread for every node, the view of the graph
//! (`src/view.rs`), and the floating-point mode (`src/fpenv.rs`), never allocate, lock, use a
//! channel, a thread-local or I/O. This reads both files and fails on any word that would do one
//! of those. Comments don't count. The pool's own block path has its test in `core/pool`.

#[rustfmt::skip]
const BANNED: [&str; 34] = [
    // Allocation.
    "Box::new", "Box<[", "vec!", "Vec<", "Vec::", ".to_vec(", "String", "format!", ".collect", "Arc::new", ".clone()",
    "to_string", "to_owned", "with_capacity", ".push(", ".extend(",
    // Locks.
    "Mutex", "RwLock", ".lock(", "Condvar", "Barrier",
    // Channels.
    "channel", "mpsc", "crossbeam",
    // Thread-locals.
    "thread_local!", "thread::current", "LocalKey",
    // I/O.
    "print!", "println!", "eprint", "dbg!", "std::io", "File", "stdout",
];

/// Words that look banned but aren't: the graph's own vectors, named as types, never made here.
const ALLOWED: [&str; 1] = ["Vec<Buf>"];

#[test]
fn the_executor_view_and_fpenv_never_block() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = vec![];
    for file in ["view.rs", "fpenv.rs"] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for (i, line) in text.lines().enumerate() {
            let mut code = line.split("//").next().unwrap_or("").to_string();
            for a in ALLOWED {
                code = code.replace(a, "");
            }
            for word in BANNED {
                if code.contains(word) {
                    found.push(format!("{file}:{}: {word}: {}", i + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the executor's view or the floating-point mode allocates, locks or waits (RENDER-28):\n{}",
        found.join("\n")
    );
}

//! RENDER-28: a wake never allocates, locks or does I/O, so the audio thread may ring and wait on
//! it. Each OS's wake is a file of its own (`mac/wake.rs`, `linux/wake.rs`); this reads both whole
//! and fails on any word that would do one of those. Comments don't count.

const BANNED: [&str; 30] = [
    // Allocation.
    "Box",
    "vec!",
    "Vec",
    "String",
    "format!",
    ".collect",
    "Arc",
    ".clone()",
    "to_string",
    "to_owned",
    "with_capacity",
    ".push(",
    "CString",
    // Locks.
    "Mutex",
    "RwLock",
    ".lock(",
    "Condvar",
    "Barrier",
    "OnceLock",
    // Channels.
    "channel",
    "mpsc",
    // Thread-locals, and std's park, which makes a thread handle on a thread std didn't start.
    "thread_local!",
    "thread::current",
    "park(",
    // I/O.
    "print!",
    "println!",
    "eprint",
    "dbg!",
    "std::io",
    "File",
];

#[test]
fn the_wake_never_blocks() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = vec![];
    for file in ["mac/wake.rs", "linux/wake.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        for (i, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for word in BANNED {
                if code.contains(word) {
                    found.push(format!("{file}:{}: {word}: {}", i + 1, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "a wake allocates, locks or does I/O (RENDER-28):\n{}", found.join("\n"));
}

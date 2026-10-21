//! RENDER-28: the pool's block path never allocates, locks, uses a channel, a thread-local or I/O,
//! so the audio thread may run it. All of it is in `src/run.rs`: waking, a run, the workers'
//! loop, sleeping, and the run state's steps. This reads that file and fails on any word that
//! would do one of those. Comments don't count. Workers park with std's `park` through the sync
//! shim; the caller of a run never does (it sleeps in steps only when a task is stuck).

const BANNED: [&str; 34] = [
    // Allocation.
    "Box::new",
    "Box<[",
    "vec!",
    "Vec<",
    "Vec::",
    ".to_vec(",
    "String",
    "format!",
    ".collect",
    "Arc::new",
    ".clone()",
    "to_string",
    "to_owned",
    "with_capacity",
    ".push(",
    ".extend(",
    // Locks.
    "Mutex",
    "RwLock",
    ".lock(",
    "Condvar",
    "Barrier",
    // Channels.
    "channel",
    "mpsc",
    "crossbeam",
    // Thread-locals.
    "thread_local!",
    "thread::current",
    "LocalKey",
    // I/O.
    "print!",
    "println!",
    "eprint",
    "dbg!",
    "std::io",
    "File",
    "stdout",
];

/// Words that look banned but aren't: the workers' table is made in lib.rs and only handed in.
const ALLOWED: [&str; 1] = ["workers: Box<[Worker]>"];

#[test]
fn the_block_path_never_blocks() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/run.rs");
    let text = std::fs::read_to_string(&file).unwrap();
    let mut found = vec![];
    for (i, line) in text.lines().enumerate() {
        let code = line.split("//").next().unwrap_or("");
        if ALLOWED.iter().any(|a| code.contains(a)) {
            continue;
        }
        for word in BANNED {
            if code.contains(word) {
                found.push(format!("run.rs:{}: {word}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "the pool's block path allocates, locks or waits (RENDER-28):\n{}", found.join("\n"));
}

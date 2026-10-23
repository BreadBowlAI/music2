//! RENDER-28: the host's audio side (`src/audio.rs`: `process`, `reset`, the thread checks and the
//! callbacks a plugin makes from its audio thread; `src/remote/slot.rs`: a call through the host
//! process's shared memory, HOST-3) never locks, waits, allocates or touches a thread-local (a
//! thread's first one allocates on macOS). The allocation trap in `m2-play` runs both; this keeps
//! the obvious ways out of the files. A file's tests, from `#[cfg(test)]` on, don't count.

#[test]
fn the_audio_side_never_blocks_or_allocates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let forbidden = [
        ".lock(",
        "Mutex",
        "RwLock",
        "Condvar",
        ".send(",
        ".recv(",
        ".join(",
        "sleep(",
        "String",
        "format!",
        "to_string",
        "to_owned",
        "vec!",
        "Vec::",
        "Box::new",
        "Arc::new",
        "thread_local!",
        "println!",
        "eprintln!",
        "collect(",
    ];
    let mut found = vec![];
    for file in ["src/audio.rs", "src/remote/slot.rs"] {
        let text = std::fs::read_to_string(root.join(file)).unwrap();
        for (i, line) in text.lines().enumerate().take_while(|(_, l)| !l.starts_with("#[cfg(test)]")) {
            let code = line.trim_start();
            if !code.starts_with("//") && forbidden.iter().any(|f| code.contains(f)) {
                found.push(format!("{file}:{}: {code}", i + 1));
            }
        }
    }
    assert!(found.is_empty(), "the audio side may block or allocate: {found:#?}");
}

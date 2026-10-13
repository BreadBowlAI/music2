//! RENDER-28: the host's audio side (`src/audio.rs`: `process`, `reset`, the thread checks and the
//! callbacks a plugin makes from its audio thread) never locks, waits, allocates or touches a
//! thread-local (a thread's first one allocates on macOS). The allocation trap in `m2-play` runs it;
//! this keeps the obvious ways out of the file.

#[test]
fn the_audio_side_never_blocks_or_allocates() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/audio.rs");
    let forbidden = [
        ".lock(", "Mutex", "RwLock", "Condvar", ".send(", ".recv(", ".join(", "sleep(", "String", "format!", "to_string", "to_owned",
        "vec!", "Vec::", "Box::new", "Arc::new", "thread_local!", "println!", "eprintln!", "collect(",
    ];
    let text = std::fs::read_to_string(path).unwrap();
    let mut found = vec![];
    for (i, line) in text.lines().enumerate() {
        let code = line.trim_start();
        if !code.starts_with("//") && forbidden.iter().any(|f| code.contains(f)) {
            found.push(format!("audio.rs:{}: {code}", i + 1));
        }
    }
    assert!(found.is_empty(), "the audio side may block or allocate: {found:#?}");
}

//! RENDER-28: no thread ever blocks on the player's channels. crossbeam's channels lock a mutex
//! to wake a blocked thread, so a blocked receiver (or sender) would make the audio thread's
//! `try_send` (or `try_recv`) take a lock. The player only `try_send`s and `try_recv`s; this
//! keeps it that way. The one blocking pair is the start-up handshake, on a channel of its own
//! that exists before the audio thread does; `self.send` is the player's bounded retry. The one
//! lock is the cell writers' (`cells.rs`), which only control threads take: the audio thread reads
//! cells from atomics.

#[test]
fn the_player_never_blocks_on_its_channels() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let blocking = [".send(", ".recv(", "recv_timeout(", "send_timeout(", ".lock(", "recv_deadline(", "send_deadline("];
    let allowed = ["ready_tx.send(", "ready.recv(", "self.send(", "self.state.lock("];
    let mut found = vec![];
    for file in ["lib.rs", "resample.rs", "cells.rs"] {
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        for (i, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if !code.starts_with("//") && blocking.iter().any(|b| code.contains(b)) && !allowed.iter().any(|a| code.contains(a)) {
                found.push(format!("{file}:{}: {code}", i + 1));
            }
        }
    }
    assert!(found.is_empty(), "blocking channel calls in core/play: {found:#?}");
}

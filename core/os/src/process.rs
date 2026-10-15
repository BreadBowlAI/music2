//! What this process holds, and which machine it runs on: for the gates (step 7's G7f) and their
//! records, which name the machine that made them (PLAT-1).

use crate::os;

/// The process's memory, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Memory {
    /// The physical memory the process is charged for: macOS's `phys_footprint`; on Linux its
    /// anonymous resident memory and its swap (`RssAnon` + `VmSwap`).
    pub footprint: u64,
    /// The address space it has mapped, reservations no page backs included: macOS's
    /// `virtual_size`; Linux's `VmSize`.
    pub mapped: u64,
    /// The most address space it can map: macOS's span from `task_info`; on Linux the user half
    /// of the CPU's virtual addresses, or `RLIMIT_AS` when that is lower.
    pub address_space: u64,
}

/// The process's memory now; None when the OS doesn't say. A gate that needs it fails then, never
/// passes (step 7's G7f).
pub fn memory() -> Option<Memory> {
    os::memory()
}

/// This machine, for a gate record: "<model>, <OS>, <CPU>, <architecture>, <n> threads", for
/// example "MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads". No part holds ", ",
/// so records compare part by part. The threads are those this process may use.
pub fn machine() -> String {
    let (model, system, cpu) = os::machine();
    let threads = std::thread::available_parallelism().map_or(0, |n| n.get());
    [model, system, cpu, std::env::consts::ARCH.to_string(), format!("{threads} threads")].map(|part| part.replace(", ", " ")).join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // What memory() says is tested in unix.rs, beside the reservation it measures.

    // OS-84, PLAT-1: a record names its machine in five parts, the same each time.
    #[test]
    fn the_machine_is_named_in_five_parts() {
        let m = machine();
        let parts: Vec<&str> = m.split(", ").collect();
        assert_eq!(parts.len(), 5, "{m}");
        assert!(parts.iter().all(|p| !p.trim().is_empty() && p.trim() == *p), "{m}");
        assert_eq!(parts[3], std::env::consts::ARCH);
        assert!(parts[4].strip_suffix(" threads").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n > 0), "{m}");
        assert_eq!(machine(), m);
    }
}

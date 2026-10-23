//! What this process holds, and which machine it runs on: for the gates (step 7's G7f) and their
//! records, which name the machine that made them (PLAT-1); and how many cores a render may use
//! (8a).

use std::time::Duration;

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

/// The cores this process may use, by kind (8a). A render's thread count is chosen from them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cores {
    /// Physical performance cores: on a Mac its P-cores, elsewhere every physical core inside the
    /// process's affinity mask and CPU quota. One at least.
    pub fast: usize,
    /// Efficiency cores (a Mac's E-cores); 0 where every core is alike.
    pub slow: usize,
    /// The most fast cores that share one cache: an L2 cluster on a Mac, a last-level cache
    /// elsewhere (one die of a two-die CPU). One at least, `fast` at most.
    pub per_cache: usize,
    /// The most fast cores that share the last cache before memory: every fast core on a Mac (its
    /// L2 clusters share one system-level cache), `per_cache` elsewhere. `per_cache` at least,
    /// `fast` at most.
    pub per_last_cache: usize,
}

/// The cores this process may use. macOS: `hw.perflevel0` and `hw.perflevel1` (or
/// `hw.physicalcpu` on a Mac with one kind of core). Linux: physical cores from sysfs inside
/// `sched_getaffinity`, cut to the cgroup v2 `cpu.max` quota, and the most sharing one
/// last-level cache.
pub fn cores() -> Cores {
    os::cores()
}

/// The 1, 5 and 15 minute load averages (`getloadavg`): what the gates print before they time.
pub fn load_average() -> Option<[f64; 3]> {
    os::load_average()
}

/// How long this process's threads have waited for a CPU, summed over the threads alive now:
/// Linux's `/proc/self/task/*/schedstat`. macOS doesn't say: None.
pub fn waited() -> Option<Duration> {
    os::waited()
}

/// Standard output, for this process's own use alone: what it was is returned, and standard output
/// now goes to standard error. The plugin host process (HOST-3) speaks to the app on it, so a
/// plugin that prints can't break a message. Windows (designed): `GetStdHandle` and
/// `SetStdHandle`.
pub fn take_stdout() -> std::io::Result<std::fs::File> {
    os::take_stdout()
}

/// Whether process `pid` lives: `m2` waits for the app while it does (AGENT-1). Windows
/// (designed): `OpenProcess` and `GetExitCodeProcess`.
pub fn alive(pid: u32) -> bool {
    os::alive(pid)
}

/// End the process at once with `code`, running no exit handler: a process whose plugin code may
/// still run on other threads must not unload it under them.
pub fn exit_now(code: i32) -> ! {
    os::exit_now(code)
}

/// This process writes no core dump when it crashes, so it ends at once and its pipes close then:
/// the plugin host process (HOST-3), whose crash the app must see in time. Linux: not dumpable
/// (a crash handler such as apport would hold it until the dump is read). macOS: no core file
/// (its crash report is written by another process). Never fatal.
pub fn no_core_dump() {
    os::no_core_dump()
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

    // 8a: the load average is three numbers, none negative.
    #[test]
    fn the_load_average_is_three_numbers() {
        let l = load_average().expect("both OSes say");
        assert!(l.iter().all(|x| x.is_finite() && *x >= 0.0), "{l:?}");
    }
}

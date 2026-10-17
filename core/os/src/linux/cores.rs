//! Linux: how many cores this process may use, from sysfs, its affinity mask and its cgroup's CPU
//! quota.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::process::Cores;

/// This process's cores: the physical ones inside its affinity mask, cut to its cgroup's quota.
pub(crate) fn cores() -> Cores {
    let mask = affinity().unwrap_or_else(|| (0..std::thread::available_parallelism().map_or(1, |n| n.get())).collect());
    cores_in(Path::new("/sys/devices/system/cpu"), &mask, quota())
}

/// The cores of CPUs `mask` in the sysfs tree at `cpu` (`/sys/devices/system/cpu`), cut to `quota`
/// CPUs: physical cores are unique (package, core) pairs; `per_cache` is the most of them sharing
/// one last-level cache, the cache of the highest level each CPU lists. A CPU sysfs doesn't
/// describe counts as a core of its own. Never less than one core.
pub(crate) fn cores_in(cpu: &Path, mask: &[usize], quota: Option<f64>) -> Cores {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok().map(|t| t.trim().to_string());
    let mut physical = BTreeSet::new();
    // Each last-level cache, by the CPUs it serves, and the cores in it.
    let mut caches: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    for &c in mask {
        let dir = cpu.join(format!("cpu{c}"));
        let package = read(dir.join("topology/physical_package_id")).unwrap_or_else(|| format!("cpu{c}"));
        let core = read(dir.join("topology/core_id")).unwrap_or_else(|| format!("cpu{c}"));
        let id = (package, core);
        physical.insert(id.clone());
        let mut last: Option<(u32, String)> = None;
        for k in 0.. {
            let index = dir.join(format!("cache/index{k}"));
            let Some(level) = read(index.join("level")) else { break };
            let (Ok(level), Some(shared)) = (level.parse::<u32>(), read(index.join("shared_cpu_list"))) else { continue };
            if last.as_ref().is_none_or(|(l, _)| level > *l) {
                last = Some((level, shared));
            }
        }
        let cache = last.map_or_else(|| format!("cpu{c}"), |(level, shared)| format!("L{level} {shared}"));
        caches.entry(cache).or_default().insert(id);
    }
    let mut fast = physical.len().max(1);
    if let Some(q) = quota {
        fast = fast.min((q.floor() as usize).max(1));
    }
    let per_cache = caches.values().map(BTreeSet::len).max().unwrap_or(1).clamp(1, fast);
    Cores { fast, slow: 0, per_cache, per_last_cache: per_cache }
}

/// The CPUs this process may run on (`sched_getaffinity`).
fn affinity() -> Option<Vec<usize>> {
    // SAFETY: a zeroed cpu_set_t is an empty set; sched_getaffinity fills it.
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) } != 0 {
        return None;
    }
    // SAFETY: CPU_ISSET reads the set filled above.
    let cpus: Vec<usize> = (0..libc::CPU_SETSIZE as usize).filter(|&c| unsafe { libc::CPU_ISSET(c, &set) }).collect();
    (!cpus.is_empty()).then_some(cpus)
}

/// The tightest CPU quota on this process's cgroup v2 path, in CPUs: `cpu.max`'s quota over its
/// period, in its own cgroup and each above it. None without one.
fn quota() -> Option<f64> {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    let mut dir = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    let mut tightest: Option<f64> = None;
    loop {
        if let Some(q) = std::fs::read_to_string(dir.join("cpu.max")).ok().and_then(|t| cpu_max(&t)) {
            tightest = Some(tightest.map_or(q, |t| t.min(q)));
        }
        if dir == Path::new("/sys/fs/cgroup") || !dir.pop() {
            return tightest;
        }
    }
}

/// `cpu.max`: "max 100000" is no quota; "250000 100000" is 2.5 CPUs.
pub(crate) fn cpu_max(text: &str) -> Option<f64> {
    let mut parts = text.split_whitespace();
    let quota: f64 = parts.next()?.parse().ok()?;
    let period: f64 = parts.next()?.parse().ok()?;
    (quota > 0.0 && period > 0.0).then(|| quota / period)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sysfs tree shaped like this server's CPU: 2 caches of 8 cores, 2 threads each; CPU n and
    /// n + 16 share a core, and caches 0-7,16-23 and 8-15,24-31. Each CPU lists L1, L2 and L3.
    fn fake_sysfs() -> PathBuf {
        let root = std::env::temp_dir().join(format!("m2-os-cores-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for c in 0..32usize {
            let core = c % 16;
            let dir = root.join(format!("cpu{c}"));
            std::fs::create_dir_all(dir.join("topology")).unwrap();
            std::fs::write(dir.join("topology/physical_package_id"), "0\n").unwrap();
            std::fs::write(dir.join("topology/core_id"), format!("{core}\n")).unwrap();
            let l3 = if core < 8 { "0-7,16-23" } else { "8-15,24-31" };
            let siblings = format!("{core},{}", core + 16);
            for (k, (level, shared)) in [("1", siblings.as_str()), ("2", siblings.as_str()), ("3", l3)].iter().enumerate() {
                let index = dir.join(format!("cache/index{k}"));
                std::fs::create_dir_all(&index).unwrap();
                std::fs::write(index.join("level"), format!("{level}\n")).unwrap();
                std::fs::write(index.join("shared_cpu_list"), format!("{shared}\n")).unwrap();
            }
        }
        root
    }

    // 8a (§5.2): physical cores are counted once however many threads each runs, and a cache's
    // cores are the most that share the last level.
    #[test]
    fn cores_are_read_from_sysfs() {
        let root = fake_sysfs();
        let all: Vec<usize> = (0..32).collect();
        assert_eq!(cores_in(&root, &all, None), Cores { fast: 16, slow: 0, per_cache: 8, per_last_cache: 8 });
        let _ = std::fs::remove_dir_all(&root);
    }

    // 8a (§5.2): only the CPUs this process may use count, and a cgroup's quota cuts them, rounded
    // down, to one at least.
    #[test]
    fn the_affinity_mask_and_quota_cut_them() {
        let root = fake_sysfs();
        let one_die: Vec<usize> = (8..16).collect();
        assert_eq!(cores_in(&root, &one_die, None), Cores { fast: 8, slow: 0, per_cache: 8, per_last_cache: 8 });
        assert_eq!(cores_in(&root, &[8, 24], None), Cores { fast: 1, slow: 0, per_cache: 1, per_last_cache: 1 }, "two threads of one core");
        let all: Vec<usize> = (0..32).collect();
        assert_eq!(cores_in(&root, &all, Some(2.5)), Cores { fast: 2, slow: 0, per_cache: 2, per_last_cache: 2 });
        assert_eq!(cores_in(&root, &all, Some(0.5)), Cores { fast: 1, slow: 0, per_cache: 1, per_last_cache: 1 });
        assert_eq!(cpu_max("max 100000\n"), None);
        assert_eq!(cpu_max("250000 100000\n"), Some(2.5));
        let _ = std::fs::remove_dir_all(&root);
    }

    // 8a (§5.2): this machine has at least one core, no slow ones, and a cache holds at most all.
    #[test]
    fn cores_here_are_sane() {
        let c = crate::process::cores();
        assert!(c.fast >= 1 && c.slow == 0 && (1..=c.fast).contains(&c.per_cache), "{c:?}");
        assert!(c.fast <= std::thread::available_parallelism().unwrap().get(), "{c:?}");
    }
}

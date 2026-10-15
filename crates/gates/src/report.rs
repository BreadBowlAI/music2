//! What a gate run says: each check, passed or failed with its numbers, or skipped with why.

use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Not measured on this rig or build, with why.
    Skip,
    /// Measured for the record, with no bound of its own.
    Info,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub verdict: Verdict,
    pub detail: String,
}

/// One gate on one rig.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub gate: &'static str,
    /// The contract's rule ids the gate checks.
    pub rules: &'static str,
    pub rig: String,
    pub checks: Vec<Check>,
}

impl Outcome {
    pub fn new(gate: &'static str, rules: &'static str, rig: &str) -> Outcome {
        Outcome { gate, rules, rig: rig.to_string(), checks: vec![] }
    }

    pub fn check(&mut self, name: impl Into<String>, pass: bool, detail: impl fmt::Display) -> bool {
        let verdict = if pass { Verdict::Pass } else { Verdict::Fail };
        self.checks.push(Check { name: name.into(), verdict, detail: detail.to_string() });
        pass
    }

    pub fn skip(&mut self, name: impl Into<String>, why: impl fmt::Display) {
        self.checks.push(Check { name: name.into(), verdict: Verdict::Skip, detail: why.to_string() });
    }

    /// Something measured that has no bound of its own, such as a timing printed for the record.
    pub fn info(&mut self, name: impl Into<String>, detail: impl fmt::Display) {
        self.checks.push(Check { name: name.into(), verdict: Verdict::Info, detail: detail.to_string() });
    }

    /// Pass when every bounded check passed, Skip when none was measured here, else Fail.
    pub fn verdict(&self) -> Verdict {
        if self.checks.iter().any(|c| c.verdict == Verdict::Fail) {
            Verdict::Fail
        } else if self.checks.iter().any(|c| c.verdict == Verdict::Pass) {
            Verdict::Pass
        } else {
            Verdict::Skip
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let all = match self.verdict() {
            Verdict::Pass => "PASS",
            Verdict::Fail => "FAIL",
            _ => "SKIP",
        };
        writeln!(f, "{all}  {} ({}) on {}", self.gate, self.rules, self.rig)?;
        for c in &self.checks {
            let v = match c.verdict {
                Verdict::Pass => "ok  ",
                Verdict::Fail => "FAIL",
                Verdict::Skip => "skip",
                Verdict::Info => "info",
            };
            writeln!(f, "      {v}  {}: {}", c.name, c.detail)?;
        }
        Ok(())
    }
}

/// Whether a record made on machine `then` judges a run on machine `now` (PLAT-1): the same
/// machine, whatever version of its OS it runs. Both are `m2_os::process::machine()`, whose second
/// part is the OS.
pub fn same_machine(then: &str, now: &str) -> bool {
    let hardware = |m: &str| m.split(", ").enumerate().filter(|(i, _)| *i != 1).map(|(_, part)| part.to_string()).collect::<Vec<_>>();
    hardware(then) == hardware(now)
}

/// Which build measured: timings from a dev build and a release build differ several times over,
/// and the gates must pass on both (step 3a's design, §15).
pub fn profile() -> String {
    let profile = env!("M2_GATES_PROFILE");
    let raised: u32 = env!("M2_GATES_RAISED").parse().unwrap_or(0);
    let checks = if cfg!(debug_assertions) { "debug assertions and overflow checks on" } else { "no debug assertions" };
    match profile {
        "release" => format!("release (opt-level {}), {checks}", env!("M2_GATES_OPT_LEVEL")),
        other => format!(
            "{other} (opt-level {} here; {raised} of the 4 audio-path crates raised to opt-level 3 by [profile.dev.package.*]), {checks}",
            env!("M2_GATES_OPT_LEVEL")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // PLAT-1: a record judges runs on its own machine only, through an OS update too.
    #[test]
    fn a_record_judges_its_own_machine_only() {
        let mac = "MacBookPro18,1, macOS 26.6.2, Apple M1 Pro, aarch64, 10 threads";
        assert!(same_machine(mac, mac));
        assert!(same_machine(mac, "MacBookPro18,1, macOS 26.7, Apple M1 Pro, aarch64, 10 threads"), "an OS update");
        assert!(!same_machine(mac, "Mac14,2, macOS 26.6.2, Apple M2, aarch64, 8 threads"));
        let linux = "ASRockRack B665D4U-1L, Ubuntu 24.04.3 LTS, AMD Ryzen 9 7950X3D 16-Core Processor, x86_64, 32 threads";
        assert!(!same_machine(mac, linux) && !same_machine(linux, mac));
        assert!(!same_machine(linux, &linux.replace("32 threads", "8 threads")), "fewer threads are another machine");
        assert!(same_machine(&m2_os::process::machine(), &m2_os::process::machine()));
    }
}

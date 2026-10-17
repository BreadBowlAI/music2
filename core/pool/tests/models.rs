//! The pool's models: its protocol run many times over, built with `--features models`, so a
//! chaos point (a random yield or short spin) comes before every atomic step and every park
//! (`src/sync.rs`). Each model runs 4,000 to 10,000 small cases on 2 to 4 threads, 34,000 in all. A hang is caught
//! by a watchdog: no case finishing for 60 s stops the binary with "<model>: hung in case N".
//!
//! loom would try every interleaving; it isn't in the offline registry, so these try many at
//! random instead. Each model was shown red against a mutant of the pool (the build notes for
//! 8a, slice 1). x86 keeps stores in order (TSO), so a too-weak ordering may pass here and only
//! fail on an arm64 Mac, which runs the same row of ci/check.sh.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use m2_pool::{Outcome, Pool, Runner, Setup, Tasks, models};

/// One model at a time: their workers spin.
static ONE: Mutex<()> = Mutex::new(());

/// Run a model on its own thread. `progress` counts its finished cases: if none finishes for 60 s
/// the model hung, and the binary stops with a line saying so (a hung pool can't be cleaned up).
fn watched(name: &'static str, body: impl FnOnce(&AtomicU64) + Send + 'static) {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let progress = Arc::new(AtomicU64::new(0));
    let handle = {
        let progress = progress.clone();
        std::thread::spawn(move || body(&progress))
    };
    let (mut seen, mut since) = (0, Instant::now());
    while !handle.is_finished() {
        std::thread::sleep(Duration::from_millis(10));
        let now = progress.load(SeqCst);
        if now != seen {
            (seen, since) = (now, Instant::now());
        } else if since.elapsed() > Duration::from_secs(60) {
            // Straight to stderr: the test harness keeps what `eprintln!` prints, and exit drops it.
            let _ = writeln!(std::io::stderr(), "{name}: hung in case {now}");
            std::process::exit(101);
        }
    }
    if let Err(payload) = handle.join() {
        std::panic::resume_unwind(payload);
    }
}

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// A random graph of 1 to 16 tasks, and which of them fail when `failing` (each one time in six).
struct Case {
    costs: Vec<u32>,
    edges: Vec<(u32, u32)>,
    from: Vec<Vec<u32>>,
    fails: Vec<bool>,
}

impl Case {
    fn new(rng: &mut Rng, failing: bool) -> Case {
        let n = 1 + rng.below(16) as usize;
        let mut order: Vec<u32> = (0..n as u32).collect();
        for i in (1..n).rev() {
            order.swap(i, rng.below(i as u64 + 1) as usize);
        }
        let percent = 10 + rng.below(50);
        let mut edges = vec![];
        for i in 0..n {
            for j in i + 1..n {
                if rng.chance(percent) {
                    edges.push((order[i], order[j]));
                }
            }
        }
        let mut from = vec![vec![]; n];
        for &(a, b) in &edges {
            from[b as usize].push(a);
        }
        let costs = (0..n).map(|_| 1 + rng.below(4) as u32).collect();
        let fails = (0..n).map(|_| failing && rng.chance(16)).collect();
        Case { costs, edges, from, fails }
    }

    /// What each task's outcome must be: skipped when anything it reads failed or was skipped.
    fn want(&self) -> Vec<Outcome> {
        let n = self.costs.len();
        let mut want: Vec<Option<Outcome>> = vec![None; n];
        while want.iter().any(Option::is_none) {
            for t in 0..n {
                if want[t].is_some() || self.from[t].iter().any(|&p| want[p as usize].is_none()) {
                    continue;
                }
                want[t] = Some(if self.from[t].iter().any(|&p| want[p as usize] != Some(Outcome::Ran)) {
                    Outcome::Skipped
                } else if self.fails[t] {
                    Outcome::Failed
                } else {
                    Outcome::Ran
                });
            }
        }
        want.into_iter().map(Option::unwrap).collect()
    }

    /// Run it, and check every task that ran was called once, after its inputs, and every outcome.
    /// `sealed` is set once the run returned: a task called then is a breach.
    fn run(&self, runner: &mut Runner, sealed: &AtomicBool) -> Result<Tasks, String> {
        let n = self.costs.len();
        let mut tasks = Tasks::new(&self.costs, &self.edges).unwrap();
        let done: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
        let calls: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
        let early = AtomicU32::new(0);
        let late = AtomicU32::new(0);
        runner
            .run(&mut tasks, &|t| {
                if sealed.load(SeqCst) {
                    late.fetch_add(1, SeqCst);
                }
                if self.from[t as usize].iter().any(|&p| !done[p as usize].load(SeqCst)) {
                    early.fetch_add(1, SeqCst);
                }
                calls[t as usize].fetch_add(1, SeqCst);
                done[t as usize].store(true, SeqCst);
                !self.fails[t as usize]
            })
            .map_err(|_| "a task panicked".to_string())?;
        sealed.store(true, SeqCst);
        let want = self.want();
        for t in 0..n {
            let got = tasks.outcome(t as u32);
            if got != want[t] {
                return Err(format!("task {t}: {got:?}, not {:?}", want[t]));
            }
            let c = calls[t].load(SeqCst);
            if c != u32::from(want[t] != Outcome::Skipped) {
                return Err(format!("task {t} was called {c} times"));
            }
        }
        if early.load(SeqCst) > 0 {
            return Err(format!("{} tasks ran before their inputs", early.load(SeqCst)));
        }
        if late.load(SeqCst) > 0 {
            return Err("a task ran after the run returned".into());
        }
        Ok(tasks)
    }
}

/// Pools of 2, 3 and 4 threads.
fn pools() -> Vec<(Pool, Runner)> {
    (2..=4).map(|n| Pool::new(n, Setup::plain()).unwrap()).collect()
}

#[test]
fn each_task_once_after_its_inputs() {
    watched("each_task_once_after_its_inputs", |progress| {
        models::seed(1);
        let mut rng = Rng(1);
        let mut pools = pools();
        let mut on_workers = 0;
        for case in 0..10_000 {
            if case % 500 == 499 {
                pools = self::pools();
            }
            let (_, runner) = &mut pools[rng.below(3) as usize];
            runner.spread(rng.chance(50));
            let before = runner.spread_report().tasks_on_workers;
            let c = Case::new(&mut rng, false);
            if let Err(e) = c.run(runner, &AtomicBool::new(false)) {
                panic!("case {case}: {e}");
            }
            on_workers += runner.spread_report().tasks_on_workers - before;
            progress.fetch_add(1, SeqCst);
        }
        assert!(on_workers > 0, "no task ran on a worker");
    });
}

#[test]
fn nothing_touches_a_job_after_run_returns() {
    watched("nothing_touches_a_job_after_run_returns", |progress| {
        models::seed(2);
        let mut rng = Rng(2);
        let mut pools = pools();
        let breaches = models::breaches();
        for case in 0..5000 {
            let (_, runner) = &mut pools[rng.below(3) as usize];
            runner.spread(rng.chance(50));
            let sealed = AtomicBool::new(false);
            let failing = rng.chance(30);
            let c = Case::new(&mut rng, failing);
            let tasks = match c.run(runner, &sealed) {
                Ok(tasks) => tasks,
                Err(e) => panic!("case {case}: {e}"),
            };
            // A worker still in the run would touch it now.
            tasks.seal(true);
            for _ in 0..1 + rng.below(20) {
                std::thread::yield_now();
            }
            tasks.seal(false);
            progress.fetch_add(1, SeqCst);
        }
        assert_eq!(models::breaches() - breaches, 0, "the pool touched a run after it returned");
    });
}

#[test]
fn back_to_back_runs_with_rest_between() {
    watched("back_to_back_runs_with_rest_between", |progress| {
        models::seed(3);
        let mut rng = Rng(3);
        for round in 0..20 {
            let (pool, mut runner) = Pool::new(2 + rng.below(3) as usize, Setup::plain()).unwrap();
            runner.spread(round % 2 == 0);
            for case in 0..250 {
                let failing = rng.chance(20);
                let c = Case::new(&mut rng, failing);
                if rng.chance(50) {
                    runner.wake(Tasks::new(&c.costs, &c.edges).unwrap().width());
                }
                if let Err(e) = c.run(&mut runner, &AtomicBool::new(false)) {
                    panic!("round {round}, case {case}: {e}");
                }
                if rng.chance(30) {
                    runner.rest();
                }
                progress.fetch_add(1, SeqCst);
            }
            if round % 2 == 0 {
                assert!(runner.spread_report().tasks_on_workers > 0, "round {round}: no task ran on a worker");
            }
            drop(pool);
        }
    });
}

#[test]
fn no_lost_wake() {
    watched("no_lost_wake", |progress| {
        models::seed(4);
        let mut rng = Rng(4);
        // Mostly one worker: then a lost wake is a hang, as a spread run waits for a worker to
        // take every root.
        for threads in [2, 2, 2, 3] {
            let (_pool, mut runner) = Pool::new(threads, Setup::plain()).unwrap();
            runner.spread(true);
            for case in 0..1000 {
                runner.rest();
                // Let the workers be anywhere on their way to sleep.
                for _ in 0..rng.below(4) {
                    std::thread::yield_now();
                }
                let c = Case::new(&mut rng, false);
                runner.wake(Tasks::new(&c.costs, &c.edges).unwrap().width());
                if let Err(e) = c.run(&mut runner, &AtomicBool::new(false)) {
                    panic!("{threads} threads, case {case}: {e}");
                }
                progress.fetch_add(1, SeqCst);
            }
        }
    });
}

#[test]
fn skips_are_exactly_the_descendants() {
    watched("skips_are_exactly_the_descendants", |progress| {
        models::seed(5);
        let mut rng = Rng(5);
        let mut pools = pools();
        for case in 0..10_000 {
            let (_, runner) = &mut pools[rng.below(3) as usize];
            runner.spread(rng.chance(50));
            let c = Case::new(&mut rng, true);
            if let Err(e) = c.run(runner, &AtomicBool::new(false)) {
                panic!("case {case}: {e}");
            }
            progress.fetch_add(1, SeqCst);
        }
    });
}

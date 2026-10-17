//! The pool's unit tests. No test has a time bound: each waits on counters and events, under a
//! 60 s watchdog that only catches a hang. One pool spins at a time in this binary.

use std::collections::BTreeSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::*};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

use crate::{Cycle, Outcome, Pool, Runner, Setup, Tasks, Trip};

/// Makes `complete` panic: set only in a child process (`a_panic_in_pool_code_aborts`).
pub(crate) static PANIC_IN_COMPLETE: AtomicBool = AtomicBool::new(false);

/// One pool at a time in this binary, so spinning workers never crowd another test's.
fn one_pool() -> MutexGuard<'static, ()> {
    static ONE: Mutex<()> = Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run `f` on its own thread, and fail if it hasn't finished in 60 s: a hang, never slowness.
fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)));
    });
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(v)) => v,
        Ok(Err(payload)) => std::panic::resume_unwind(payload),
        Err(_) => panic!("hung: not done in 60 s"),
    }
}

/// Wait (a 60 s watchdog) until `ok` holds.
fn until(what: &str, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < Duration::from_secs(60), "hung: {what}");
        std::thread::sleep(Duration::from_millis(1));
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

/// A random graph of `n` tasks: a random order, and each pair in it joined with a random chance,
/// so it has no cycle. Costs 1 to 10.
fn dag(rng: &mut Rng, n: usize) -> (Vec<u32>, Vec<(u32, u32)>) {
    let mut order: Vec<u32> = (0..n as u32).collect();
    for i in (1..n).rev() {
        order.swap(i, rng.below(i as u64 + 1) as usize);
    }
    let percent = 5 + rng.below(40);
    let mut edges = vec![];
    for i in 0..n {
        for j in i + 1..n {
            if rng.chance(percent) {
                edges.push((order[i], order[j]));
            }
        }
    }
    let costs = (0..n).map(|_| 1 + rng.below(10) as u32).collect();
    (costs, edges)
}

fn producers(n: usize, edges: &[(u32, u32)]) -> Vec<Vec<u32>> {
    let mut p = vec![vec![]; n];
    for &(a, b) in edges {
        p[b as usize].push(a);
    }
    p
}

/// A runner for `threads` threads; 0 means `Runner::one()`.
fn runner(threads: usize) -> (Option<Pool>, Runner) {
    match threads {
        0 => (None, Runner::one()),
        n => {
            let (pool, runner) = Pool::new(n, Setup::plain()).unwrap();
            (Some(pool), runner)
        }
    }
}

/// Every worker of `pool` other than `except` is asleep.
fn asleep(pool: &Pool, except: Option<usize>) -> bool {
    pool.shared.workers.iter().enumerate().all(|(i, w)| Some(i + 1) == except || w.parked.load(SeqCst))
}

/// The worker number of the thread this runs on: 0 off the pool.
fn me() -> usize {
    std::thread::current().name().and_then(|n| n.strip_prefix("m2-pool-")).and_then(|k| k.parse().ok()).unwrap_or(0)
}

#[test]
fn every_task_runs_once_after_its_inputs() {
    let _one = one_pool();
    within(|| {
        let mut rng = Rng(1);
        let dags: Vec<_> = (0..500)
            .map(|_| {
                let n = 2 + rng.below(39) as usize;
                dag(&mut rng, n)
            })
            .collect();
        for threads in 0..=8 {
            let (_pool, mut runner) = runner(threads);
            for spread in [false, true] {
                runner.spread(spread);
                let before = runner.spread_report().tasks_on_workers;
                for (costs, edges) in &dags {
                    let n = costs.len();
                    let mut tasks = Tasks::new(costs, edges).unwrap();
                    let from = producers(n, edges);
                    let done: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
                    let runs: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
                    let early = AtomicU32::new(0);
                    runner
                        .run(&mut tasks, &|t| {
                            if from[t as usize].iter().any(|&p| !done[p as usize].load(Acquire)) {
                                early.fetch_add(1, SeqCst);
                            }
                            runs[t as usize].fetch_add(1, SeqCst);
                            done[t as usize].store(true, Release);
                            true
                        })
                        .unwrap();
                    assert_eq!(early.load(SeqCst), 0, "a task ran before its inputs ({threads} threads, spread {spread})");
                    assert!(runs.iter().all(|r| r.load(SeqCst) == 1), "a task didn't run exactly once ({threads} threads)");
                    assert!((0..n as u32).all(|t| tasks.outcome(t) == Outcome::Ran));
                }
                if spread && threads >= 2 {
                    assert!(runner.spread_report().tasks_on_workers > before, "no task ran on a worker ({threads} threads)");
                }
            }
        }
    });
}

#[test]
fn a_failed_task_skips_what_reads_it_and_the_rest_run() {
    let _one = one_pool();
    within(|| {
        let mut rng = Rng(2);
        for threads in [0, 1, 2, 3, 4, 7] {
            let (_pool, mut runner) = runner(threads);
            runner.spread(true);
            for _ in 0..200 {
                let n = 2 + rng.below(39) as usize;
                let (costs, edges) = dag(&mut rng, n);
                let fails: Vec<bool> = (0..n).map(|_| rng.chance(15)).collect();
                let mut tasks = Tasks::new(&costs, &edges).unwrap();
                let called: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
                runner
                    .run(&mut tasks, &|t| {
                        called[t as usize].fetch_add(1, SeqCst);
                        !fails[t as usize]
                    })
                    .unwrap();
                // Serially: skipped when anything it reads failed or was skipped.
                let from = producers(n, &edges);
                let mut want: Vec<Option<Outcome>> = vec![None; n];
                while want.iter().any(Option::is_none) {
                    for t in 0..n {
                        if want[t].is_some() || from[t].iter().any(|&p| want[p as usize].is_none()) {
                            continue;
                        }
                        want[t] = Some(if from[t].iter().any(|&p| want[p as usize] != Some(Outcome::Ran)) {
                            Outcome::Skipped
                        } else if fails[t] {
                            Outcome::Failed
                        } else {
                            Outcome::Ran
                        });
                    }
                }
                for t in 0..n {
                    let want = want[t].unwrap();
                    assert_eq!(tasks.outcome(t as u32), want, "task {t} ({threads} threads)");
                    assert_eq!(called[t].load(SeqCst), u32::from(want != Outcome::Skipped), "task {t} called ({threads} threads)");
                }
            }
        }
    });
}

#[test]
fn a_panicking_task_finishes_the_run_then_is_raised() {
    let _one = one_pool();
    within(|| {
        // 0 to 3 are roots; 4 reads 0 and 1; 5 reads 4; 6 reads 2; 7 reads 3; 8 reads 6 and 7.
        let edges = [(0, 4), (1, 4), (4, 5), (2, 6), (3, 7), (6, 8), (7, 8)];
        let (pool, mut runner) = Pool::new(4, Setup::plain()).unwrap();
        runner.spread(true);
        let mut tasks = Tasks::new(&[1; 9], &edges).unwrap();
        for _ in 0..50 {
            let on_worker = AtomicBool::new(false);
            let got = runner.run(&mut tasks, &|t| {
                if t == 3 || t == 6 {
                    on_worker.fetch_or(me() != 0, SeqCst);
                    // A panic without the panic hook's message.
                    std::panic::resume_unwind(Box::new(format!("task {t}")));
                }
                true
            });
            let payload = got.expect_err("the panic is raised again");
            assert_eq!(payload.downcast_ref::<String>().map(String::as_str), Some("task 3"));
            assert!(on_worker.load(SeqCst), "task 3 is a root, so a worker ran it");
            let outcomes: Vec<Outcome> = (0..9).map(|t| tasks.outcome(t)).collect();
            use Outcome::*;
            assert_eq!(outcomes, [Ran, Ran, Ran, Failed, Ran, Ran, Failed, Skipped, Skipped]);
        }
        // The workers lived through it.
        runner.run(&mut tasks, &|_| true).unwrap();
        assert!((0..9).all(|t| tasks.outcome(t) == Outcome::Ran));
        drop(pool);
    });
}

/// Run this test binary again, as a child running only `test`, with `M2_POOL_CHILD` set. Its exit
/// status and what it printed; a failure if it runs for over 60 s.
fn child(test: &str) -> (std::process::ExitStatus, String) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env("M2_POOL_CHILD", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            panic!("hung: the child running {test} didn't end in 60 s");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut said = String::new();
    child.stdout.take().unwrap().read_to_string(&mut said).unwrap();
    child.stderr.take().unwrap().read_to_string(&mut said).unwrap();
    (status, said)
}

#[test]
fn a_panic_in_pool_code_aborts() {
    if std::env::var_os("M2_POOL_CHILD").is_some() {
        PANIC_IN_COMPLETE.store(true, SeqCst);
        let (_pool, mut runner) = Pool::new(3, Setup::plain()).unwrap();
        runner.spread(true);
        let mut tasks = Tasks::new(&[1; 4], &[(0, 3), (1, 3), (2, 3)]).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.run(&mut tasks, &|_| true)));
        println!("the run came back");
        return;
    }
    let (status, said) = child("tests::a_panic_in_pool_code_aborts");
    assert!(said.contains("a panic in the pool's own code, for a test"), "the child didn't reach the hook: {said}");
    assert!(!said.contains("the run came back"), "the panic unwound out of run: {said}");
    // Killed by a signal (SIGABRT), so no exit code.
    assert!(status.code().is_none(), "the child wasn't aborted: {status}");
}

#[test]
fn on_one_thread_a_panic_in_pool_code_unwinds() {
    if std::env::var_os("M2_POOL_CHILD").is_some() {
        PANIC_IN_COMPLETE.store(true, SeqCst);
        let mut runner = Runner::one();
        let mut tasks = Tasks::new(&[1; 2], &[(0, 1)]).unwrap();
        let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.run(&mut tasks, &|_| true)));
        assert!(got.is_err(), "the panic should unwind out of run");
        println!("unwound");
        return;
    }
    let (status, said) = child("tests::on_one_thread_a_panic_in_pool_code_unwinds");
    assert!(status.success() && said.contains("unwound"), "{status}: {said}");
}

#[test]
fn idle_workers_sleep_while_a_task_blocks() {
    let _one = one_pool();
    within(|| {
        let (pool, mut runner) = Pool::new(4, Setup::plain()).unwrap();
        let open = Arc::new(AtomicBool::new(false));
        let blocked_on = Arc::new(AtomicUsize::new(usize::MAX));
        let run = {
            let (open, blocked_on) = (open.clone(), blocked_on.clone());
            std::thread::spawn(move || {
                runner.spread(true);
                // One task that blocks, and one that reads it, which nobody can take meanwhile.
                let mut tasks = Tasks::new(&[1, 1], &[(0, 1)]).unwrap();
                runner.run(&mut tasks, &|t| {
                    if t == 0 {
                        blocked_on.store(me(), SeqCst);
                        while !open.load(SeqCst) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    true
                })
            })
        };
        until("the task blocks", || blocked_on.load(SeqCst) != usize::MAX);
        let k = blocked_on.load(SeqCst);
        assert_ne!(k, 0, "with spread, a worker takes the root");
        until("every other worker sleeps", || asleep(&pool, Some(k)) && pool.shared.inside.load(SeqCst) == 1);
        open.store(true, SeqCst);
        run.join().unwrap().unwrap();
    });
}

#[test]
fn rest_sends_workers_to_sleep_at_once() {
    let _one = one_pool();
    within(|| {
        let setup = Setup { patience: Duration::from_secs(3600), ..Setup::plain() };
        let (pool, mut runner) = Pool::new(4, setup).unwrap();
        runner.spread(true);
        let mut tasks = Tasks::new(&[1; 6], &[]).unwrap();
        runner.run(&mut tasks, &|_| true).unwrap();
        assert!(runner.spread_report().tasks_on_workers > 0);
        runner.rest();
        until("every worker sleeps", || asleep(&pool, None));
    });
}

#[test]
fn a_worker_held_off_wakes_once() {
    let _one = one_pool();
    within(|| {
        let (pool, mut runner) = Pool::new(3, Setup::plain()).unwrap();
        let open = Arc::new(AtomicBool::new(false));
        let started = Arc::new(AtomicBool::new(false));
        let run = {
            let (open, started) = (open.clone(), started.clone());
            std::thread::spawn(move || {
                runner.spread(true);
                let mut tasks = Tasks::new(&[1], &[]).unwrap();
                runner.run(&mut tasks, &|_| {
                    started.store(true, SeqCst);
                    while !open.load(SeqCst) {
                        std::hint::spin_loop();
                    }
                    true
                })
            })
        };
        until("the long task starts", || started.load(SeqCst));
        // What `wake` does, 1,000 times, while the run holds the runner.
        let threads: Vec<_> = pool.handles.iter().map(|h| h.thread().clone()).collect();
        for _ in 0..1000 {
            crate::run::signal(&pool.shared, &threads, threads.len());
        }
        open.store(true, SeqCst);
        run.join().unwrap().unwrap();
        for (k, c) in pool.counters().iter().enumerate().skip(1) {
            assert!(c.signals <= c.parks + 1, "worker {k}: {} signals for {} parks", c.signals, c.parks);
        }
    });
}

#[test]
fn a_chain_wakes_nobody() {
    let _one = one_pool();
    within(|| {
        let (pool, mut runner) = Pool::new(4, Setup::plain()).unwrap();
        until("the workers sleep", || asleep(&pool, None));
        let before = pool.counters();
        let edges: Vec<(u32, u32)> = (0..9).map(|t| (t, t + 1)).collect();
        let mut tasks = Tasks::new(&[1; 10], &edges).unwrap();
        assert_eq!(tasks.width(), 1);
        for _ in 0..100 {
            runner.wake(tasks.width());
            runner.run(&mut tasks, &|_| true).unwrap();
        }
        let after = pool.counters();
        assert_eq!(after[0].tasks - before[0].tasks, 1000, "the caller ran every task");
        for (k, (a, b)) in after.iter().zip(&before).enumerate().skip(1) {
            assert_eq!((a.signals, a.tasks), (b.signals, b.tasks), "worker {k} was woken, or worked");
        }
    });
}

#[test]
fn a_runner_outlives_its_pool() {
    let _one = one_pool();
    within(|| {
        let (pool, mut runner) = Pool::new(4, Setup::plain()).unwrap();
        assert_eq!(runner.threads(), 4);
        drop(pool);
        assert_eq!(runner.threads(), 1);
        runner.spread(true);
        let mut rng = Rng(3);
        let (costs, edges) = dag(&mut rng, 30);
        let mut tasks = Tasks::new(&costs, &edges).unwrap();
        runner.wake(tasks.width());
        runner.run(&mut tasks, &|_| true).unwrap();
        assert!((0..30).all(|t| tasks.outcome(t) == Outcome::Ran));
        assert_eq!(runner.counters()[0].tasks, 30);
        assert_eq!(runner.spread_report().tasks_on_workers, 0);
    });
}

#[test]
fn dropping_a_pool_joins_every_worker() {
    let _one = one_pool();
    within(|| {
        struct Gone(Arc<AtomicUsize>);
        impl Drop for Gone {
            fn drop(&mut self) {
                self.0.fetch_sub(1, SeqCst);
            }
        }
        thread_local! { static GONE: std::cell::RefCell<Option<Gone>> = const { std::cell::RefCell::new(None) } }
        let alive = Arc::new(AtomicUsize::new(0));
        let hook = alive.clone();
        let setup = Setup {
            start: Some(Arc::new(move |_| {
                hook.fetch_add(1, SeqCst);
                GONE.with(|g| *g.borrow_mut() = Some(Gone(hook.clone())));
            })),
            ..Setup::plain()
        };
        let (pool, _runner) = Pool::new(6, setup).unwrap();
        until("every worker started", || alive.load(SeqCst) == 5);
        drop(pool);
        assert_eq!(alive.load(SeqCst), 0, "a worker outlived its pool");
    });
}

#[test]
fn spread_puts_every_root_on_a_worker() {
    let _one = one_pool();
    within(|| {
        let mut rng = Rng(4);
        for threads in [2, 3, 5] {
            let (_pool, mut runner) = Pool::new(threads, Setup::plain()).unwrap();
            runner.spread(true);
            for _ in 0..100 {
                let n = 2 + rng.below(30) as usize;
                let (costs, edges) = dag(&mut rng, n);
                let mut tasks = Tasks::new(&costs, &edges).unwrap();
                runner.run(&mut tasks, &|_| true).unwrap();
                for &r in tasks.roots() {
                    assert_ne!(tasks.state[r as usize].ran_on.load(SeqCst), 0, "root {r} ran on the caller ({threads} threads)");
                }
            }
        }
    });
}

#[test]
fn the_same_graph_builds_the_same_tasks() {
    let mut rng = Rng(5);
    for _ in 0..200 {
        let n = 1 + rng.below(40) as usize;
        let (costs, edges) = dag(&mut rng, n);
        let a = Tasks::new(&costs, &edges).unwrap();
        // The same edges shuffled, some twice.
        let mut more = edges.clone();
        for &e in &edges {
            if rng.chance(30) {
                more.push(e);
            }
        }
        for i in (1..more.len()).rev() {
            more.swap(i, rng.below(i as u64 + 1) as usize);
        }
        let b = Tasks::new(&costs, &more).unwrap();
        assert_eq!(a.roots(), b.roots());
        assert_eq!(a.width(), b.width());
        for t in 0..n as u32 {
            assert_eq!(a.consumers(t), b.consumers(t));
            let set: BTreeSet<u32> = edges.iter().filter(|e| e.0 == t).map(|e| e.1).collect();
            assert_eq!(a.consumers(t).len(), set.len(), "consumers are each listed once");
        }
    }
}

#[test]
fn the_most_critical_go_first_and_width_is_the_widest_layer() {
    // 0 → 2 (cost 10), 0 → 3, 1 → 3: bottom levels 0: 11, 1: 2, 2: 10, 3: 1.
    let tasks = Tasks::new(&[1, 1, 10, 1], &[(1, 3), (0, 3), (0, 2)]).unwrap();
    assert_eq!(tasks.roots(), [0, 1]);
    assert_eq!(tasks.consumers(0), [2, 3]);
    assert_eq!(tasks.width(), 2);
    // Ties go to the lower number.
    let tasks = Tasks::new(&[5, 5, 5], &[]).unwrap();
    assert_eq!(tasks.roots(), [0, 1, 2]);
    assert_eq!(tasks.width(), 3);
    // Fifty tracks into one song: width 50. A chain: width 1. Nothing: width 0.
    let edges: Vec<(u32, u32)> = (0..50).map(|t| (t, 50)).collect();
    assert_eq!(Tasks::new(&[1; 51], &edges).unwrap().width(), 50);
    assert_eq!(Tasks::new(&[1; 3], &[(0, 1), (1, 2)]).unwrap().width(), 1);
    assert_eq!(Tasks::new(&[], &[]).unwrap().width(), 0);
    // The longest path sets a task's layer: 0 → 1 → 2 and 0 → 2 put 2 in layer 2, alone.
    assert_eq!(Tasks::new(&[1; 4], &[(0, 1), (1, 2), (0, 2), (3, 2)]).unwrap().width(), 2);
}

#[test]
fn a_cycle_is_refused_and_named() {
    let err = Tasks::new(&[1; 5], &[(4, 0), (0, 1), (1, 2), (2, 1), (2, 3)]).err().unwrap();
    assert_eq!(err, Cycle(vec![1, 2]));
    assert_eq!(err.to_string(), "tasks read each other in a circle: 1 → 2 → 1");
    assert_eq!(Tasks::new(&[1; 3], &[(0, 1), (2, 2)]).err(), Some(Cycle(vec![2])));
    assert_eq!(Tasks::new(&[1; 4], &[(3, 1), (1, 0), (0, 3)]).err(), Some(Cycle(vec![0, 3, 1])));
}

#[test]
fn a_pool_has_1_to_64_threads() {
    assert!(Pool::new(0, Setup::plain()).is_err());
    assert!(Pool::new(65, Setup::plain()).is_err());
    let _one = one_pool();
    let (_pool, runner) = Pool::new(1, Setup::plain()).unwrap();
    assert_eq!(runner.threads(), 1);
    assert_eq!(Runner::one().threads(), 1);
}

#[test]
fn an_empty_graph_runs_at_once() {
    let mut tasks = Tasks::new(&[], &[]).unwrap();
    Runner::one().run(&mut tasks, &|_| unreachable!()).unwrap();
    assert!(tasks.is_empty());
}

// 8a's live half: a caller whose task is stuck sleeps on its wake (no CPU), and the task's end
// rings it. Task 0 holds on the one worker; when the caller dozes, 0 is let go and task 1 (which
// reads it) holds next, on whichever thread takes it: only the ring at task 0's end can wake the
// caller then, since the worker doesn't leave the run.
#[test]
fn a_stuck_caller_dozes_until_the_task_ends() {
    let _one = one_pool();
    within(|| {
        let setup = Setup { stuck: Duration::from_micros(1), doze: Duration::from_secs(3600), ..Setup::plain() };
        let (pool, mut runner) = Pool::new(2, setup).unwrap();
        let open: [Arc<AtomicBool>; 2] = Default::default();
        let run = {
            let open = open.clone();
            std::thread::spawn(move || {
                runner.spread(true);
                let mut tasks = Tasks::new(&[1, 1], &[(0, 1)]).unwrap();
                runner.run(&mut tasks, &|t| {
                    while !open[t as usize].load(SeqCst) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    true
                })
            })
        };
        until("the caller dozes", || pool.counters()[0].dozes >= 1);
        assert_eq!(pool.counters()[0].rung, 0, "nothing ended yet");
        open[0].store(true, SeqCst);
        until("task 0's end rings the caller", || pool.counters()[0].rung >= 1);
        open[1].store(true, SeqCst);
        run.join().unwrap().unwrap();
        let c = pool.counters()[0];
        assert_eq!((c.timeouts, c.rung + c.timeouts), (0, c.dozes), "{c:?}");
    });
}

// 8a's live half: a worker that never comes to a run (not scheduled, its start still running)
// costs only speed: the caller and the other workers take every task, and nobody waits for it.
#[test]
fn a_worker_that_never_comes_is_not_waited_for() {
    let _one = one_pool();
    within(|| {
        let latch = Arc::new(AtomicBool::new(false));
        let hold = latch.clone();
        let setup = Setup {
            start: Some(Arc::new(move |k| {
                while k == 3 && !hold.load(SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            })),
            ..Setup::plain()
        };
        let (pool, mut runner) = Pool::new(4, setup).unwrap();
        runner.spread(true);
        let mut rng = Rng(6);
        for _ in 0..1000 {
            let n = 1 + rng.below(30) as usize;
            let (costs, edges) = dag(&mut rng, n);
            let mut tasks = Tasks::new(&costs, &edges).unwrap();
            let from = producers(n, &edges);
            let done: Vec<AtomicBool> = (0..n).map(|_| AtomicBool::new(false)).collect();
            let runs: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(0)).collect();
            runner
                .run(&mut tasks, &|t| {
                    assert!(from[t as usize].iter().all(|&p| done[p as usize].load(Acquire)), "task {t} ran before its inputs");
                    runs[t as usize].fetch_add(1, SeqCst);
                    done[t as usize].store(true, Release);
                    true
                })
                .unwrap();
            assert!(runs.iter().all(|r| r.load(SeqCst) == 1), "each task once");
        }
        assert_eq!(pool.counters()[3].tasks, 0, "worker 3 never came");
        latch.store(true, SeqCst);
        drop(pool);
    });
}

// 8a's live half: each worker runs its stop hook before it exits (macOS leaves the workgroup
// there), so after the drop every start has its stop.
#[test]
fn stop_runs_on_each_worker_before_it_exits() {
    let _one = one_pool();
    within(|| {
        let alive = Arc::new(AtomicUsize::new(0));
        let (up, down) = (alive.clone(), alive.clone());
        let setup = Setup {
            start: Some(Arc::new(move |_| {
                up.fetch_add(1, SeqCst);
            })),
            stop: Some(Arc::new(move |_| {
                down.fetch_sub(1, SeqCst);
            })),
            ..Setup::plain()
        };
        let (pool, mut runner) = Pool::new(5, setup).unwrap();
        until("every worker started", || alive.load(SeqCst) == 4);
        runner.spread(true);
        let mut tasks = Tasks::new(&[1; 8], &[]).unwrap();
        runner.run(&mut tasks, &|_| true).unwrap();
        drop(pool);
        assert_eq!(alive.load(SeqCst), 0, "a worker exited without its stop");
    });
}

// 8a's live half: a tripped task panics on a worker as a task's own panic does: the run finishes,
// `run` returns its payload, naming the worker, and the pool runs on.
#[test]
fn a_tripped_task_panics_on_a_worker_and_is_raised() {
    let _one = one_pool();
    within(|| {
        let (_pool, mut runner) = Pool::new(4, Setup::plain()).unwrap();
        runner.spread(true);
        let mut tasks = Tasks::new(&[1; 6], &[(0, 3), (1, 4), (2, 5)]).unwrap();
        runner.trip(runner.runs() + 3, Trip::Panic);
        for k in 1..=5 {
            let got = runner.run(&mut tasks, &|_| true);
            match k {
                3 => {
                    let payload = got.expect_err("the trip's panic is raised");
                    let said = payload.downcast_ref::<String>().cloned().unwrap_or_default();
                    let on = runner.tripped_on().expect("a task tripped");
                    assert!(on != 0 && said == format!("a tripped task, on thread {on}"), "{said:?} on {on}");
                    let failed = (0..6).filter(|&t| tasks.outcome(t) != Outcome::Ran).count();
                    assert_eq!(failed, 2, "the root that tripped and what reads it");
                }
                _ => {
                    got.unwrap();
                    assert!((0..6).all(|t| tasks.outcome(t) == Outcome::Ran));
                }
            }
        }
    });
}

// 8a's live half (G12): a worker's entry is timed from its run's start, so a worker that comes late
// shows. Worker 1 can't start until the caller's task, 20 ms into the run, lets it; the task then
// waits until it entered: its one entry is in the last bucket (16 ms or more). The meter reads the
// pool's own counters.
#[test]
fn a_workers_entry_is_timed_from_its_runs_start() {
    let _one = one_pool();
    within(|| {
        let latch = Arc::new(AtomicBool::new(false));
        let hold = latch.clone();
        let setup = Setup {
            start: Some(Arc::new(move |_| {
                while !hold.load(SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            })),
            ..Setup::plain()
        };
        let (pool, mut runner) = Pool::new(2, setup).unwrap();
        let meter = pool.meter();
        let mut tasks = Tasks::new(&[1, 1], &[]).unwrap();
        runner
            .run(&mut tasks, &|_| {
                if !latch.load(SeqCst) {
                    std::thread::sleep(Duration::from_millis(20));
                    latch.store(true, SeqCst);
                }
                while meter.counters()[1].entry.iter().sum::<u64>() == 0 {
                    std::hint::spin_loop();
                }
                true
            })
            .unwrap();
        let entry = pool.meter().counters()[1].entry;
        assert_eq!((entry.iter().sum::<u64>(), entry[crate::ENTRY_BUCKETS - 1]), (1, 1), "{entry:?}");
        assert_eq!(pool.counters(), pool.meter().counters());
        assert_eq!(
            [0, 999, 1_000, 1_999, 2_000, 8_191_999, 8_192_000, 16_384_000, u64::MAX].map(crate::entry_bucket),
            [0, 0, 1, 1, 2, 13, 14, 15, 15]
        );
    });
}

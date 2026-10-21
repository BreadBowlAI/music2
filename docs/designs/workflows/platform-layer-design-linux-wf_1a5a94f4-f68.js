export const meta = {
  name: 'platform-layer-design-linux',
  description: 'Design the platform layer on the Linux server: ground truth here, two designs, synthesis, five attacks, revision, a completeness critic. Writes only docs/designs/platform/ and $SCRATCH/.',
  phases: [
    { title: 'Ground', detail: 'root causes of every Linux failure; the surveys\' (verify) claims checked on this machine; branch overlap; Mac CI facts' },
    { title: 'Design', detail: 'two independent designs: smallest, and shaped for 8a/9/Windows' },
    { title: 'Synthesize', detail: 'one design from the two' },
    { title: 'Attack', detail: 'five lenses' },
    { title: 'Revise', detail: 'fold in the real problems' },
    { title: 'Critic', detail: 'completeness against all 85 OS pieces, every failure, plain language' },
  ],
}

const REPO = '<repo>'
const P = `${REPO}/docs/designs/platform`
const G = `${P}/ground`
const BASE = '$SCRATCH/base-f126d03'
const RULES = `
CONTEXT
- Repository ${REPO}, branch main at f126d03 (= 8aa301e plus docs). music2 is a DAW where TypeScript defines the music. Rust core core/{ir,kernel,play,plugins,runtime,script,session,song}, std/ (wasm kernels, committed std/kernels/std.wasm), crates/{server,gates,editor}, web/ (React UI), apps/{music2-desktop (Tauri), m2 (CLI)}, e2e/ (browser suites).
- Read ${REPO}/HANDOFF.md first (short). The contract ${REPO}/docs/architecture.md is normative. The two surveys are the input: ${P}/survey-code.md (85 OS-specific pieces, ids OS-01..OS-85, what Linux and Windows need for each) and ${P}/survey-later.md (what steps 8a, 8b and 9 add per OS; its §5 sketches one trait). Later-step designs: docs/designs/step7/design-7.md, docs/designs/step8/design-8a.md, design-8b.md, taste.md.
- THE MACHINE: the owner's Linux server. Ubuntu 24.04, x86_64 Ryzen 9 7950X3D (16 cores, 32 threads), 124 GB RAM, root filesystem ext4 on software RAID (/dev/md2); /tmp is on that ext4, not tmpfs. Rust 1.99.0 with wasm32-unknown-unknown, Node 24, chrome-headless-shell linux64 133.0.6943.141 at ~/.cache/puppeteer/chrome-headless-shell/linux-133.0.6943.141/chrome-headless-shell-linux64/chrome-headless-shell. Passwordless sudo exists but do NOT install system packages or change system settings; report what would be needed instead.
- WHAT CHANGED SINCE THE SURVEYS: they assumed Linux CI in Docker on the owner's Mac. Development has now MOVED to this Linux server. Linux must be fully green here, natively, first. No Docker. The Mac stays the reference platform (timing gates are judged on Apple silicon; its look and native menus stay). The owner CHOSE GitHub Actions macOS runners to check the Mac side, started by hand once per step (not on every push); the repo is the private BreadBowlAI/music2 (git remote over ssh works; the gh CLI is not installed here yet). Windows: designed for, built later.
- THIS STEP: the platform layer, built on main BEFORE finishing step 7 (branch wip/step7), then merging the branches sounds and taste. Its aim: Linux green (every Rust test, clippy -D warnings, the desktop app builds and its tests pass, every browser suite ok, offline renders deterministic), the Mac still green (checked on a GitHub macOS runner), and one place per OS for what differs, so steps 8a and 9 write each OS piece once.
- ALREADY DECIDED: use deterministic math (the pure-Rust libm crate, the same code that is inside std.wasm) for every transcendental on the render path, so a song exports the same bits on every OS. This changes the Mac's recorded render hashes once; that is accepted and is recorded as a deliberate change.
- THE OWNER wants: "simple yet elegant", a tasteful professional DAW; don't be afraid to change earlier work when it makes things simpler; no backwards compatibility; no built-in AI; quality over speed (full checks: all tests, clippy, every browser suite, timing gates, offline byte-identity, multi-lens review, skeptic confirmation). The owner is dyslexic and new to music: anything they read needs "In short" first, short sentences, plain words, tables.
- LINUX BASELINE at f126d03, run by the lead with $SCRATCH/check.sh. Logs in ${BASE}/: build.log, test.log (full cargo test output) and test.sum, clippy.log, desktop.log, web.log, e2e.log (one line per browser suite) and e2e/<suite>-<kind>.log (each suite's own log; failed runs keep their scratch folder there), baseline.log and renders/SHA256SUMS (8 offline renders on Linux). Results: build ok; cargo test 686 pass, 10 fail; clippy -D warnings fails on Linux at its first crate (dead code: core/plugins/src/main_thread.rs:217 install), so the other crates' Linux lints are unknown; cargo build -p music2-desktop now SUCCEEDS on Linux; web build ok. The Mac's render hashes at 8aa301e are in HANDOFF.md (compare with renders/SHA256SUMS).
- OTHER BRANCHES, merged after this step (see git diff main...origin/<branch>): wip/step7 (step 7 engine, unfinished; touches core/runtime/src/audio.rs, crates/gates/* including a new store.rs with Mach memory calls, core/plugins/{host,inprocess,probe}.rs, core/play tests, crates/server/src/graph/actor.rs); taste (the new UI look; rewrites apps/music2-desktop/src/menu.rs, touches crates/server/src/project.rs, e2e/run.mjs and many e2e suites); sounds (20 new std sounds, a rebuilt std.wasm).
RULES FOR WORKING
- The working tree is READ-ONLY for you: no edits, commits, stashes, branch or ref changes, except the files your task says to write. Never touch other branches' worktrees.
- Cargo: use your OWN target dir, CARGO_TARGET_DIR=$SCRATCH/target-<your label>, and delete it when you finish (they are 20-60 GB each).
- Any job that takes more than about 20 seconds (cargo build/test/clippy, browser suites, renders, std rebuilds) runs through the helper: $SCRATCH/tw <short-name> '<shell command>'. It opens a window in the tmux session the owner watches, logs to $SCRATCH/logs/, waits up to ~9.5 minutes and prints the exit status and the log's tail. If it prints "still running", wait with the tw-wait command it prints. Short commands (git, grep, cat, small probes) run directly.
- Anything that runs music2 (m2, the headless server, tests that spawn it) gets a scratch HOME (never the real ~/.music2 or ~/Music) and M2_AUDIO=none. Never launch music2-desktop.
- Scratch files go in $SCRATCH/scratch-<your label>/; delete them when done unless your task says to keep them.
- Facts: mark anything you did not check on this machine or in source as (unverified). Prefer measuring here over memory.
`
const NOTES = { type: 'object', properties: { file: { type: 'string' }, summary: { type: 'string' } }, required: ['file', 'summary'] }

phase('Ground')
const grounds = await parallel([
  () => agent(`${RULES}
YOUR TASK (label ground-tests): find the ROOT CAUSE of every Rust failure on Linux, precisely.
1. The 10 failing cargo tests in ${BASE}/test.log (plus core/plugins/tests/main_thread.rs, which panicked at line 78; list every failure the log shows). For each: the test, the assertion that fails and why on Linux (read the code; re-run single tests with RUST_BACKTRACE=1 and --nocapture through tw when the log is not enough), which survey piece (OS-nn) it is, and the fix (a platform-layer piece, or a test that must change, with the reason). Watch for failures the surveys did not predict (e.g. tx::stage_tests::a_rollback_that_fails_is_reported_at_once, tx::kill_tests::killed_at_every_commit_step_the_project_recovers, graph::tests::a_lane_written_in_two_halves_never_plays_empty, graph::preview_tests::an_interrupted_commit_after_its_preview_points_to_needs_attention) and explain them.
2. Clippy on Linux for the WHOLE workspace including music2-desktop: cargo clippy --workspace --all-targets --keep-going -- -D warnings (own target dir). List every lint with file:line and the fix.
3. cargo test -p music2-desktop on Linux (its appkit test prints "macOS only" elsewhere): does it pass?
4. cargo build --release for the workspace: any Linux-only problem?
Write ${G}/tests.md (create the folder): a table per item (test or lint | root cause | OS-nn | fix), "In short" first. Return its path and a summary that lists every failure with its one-line root cause.`, { label: 'ground:tests', phase: 'Ground', schema: NOTES }),

  () => agent(`${RULES}
YOUR TASK (label ground-e2e): root-cause every browser suite that is not ok on Linux. The lead's run (${BASE}/e2e.log, one line per suite; per-suite logs in ${BASE}/e2e/) may still be finishing when you start: wait until e2e.log has a line for all 29 runs (the list is SUITES in e2e/run.mjs) or the file ${BASE}.out shows "=== baseline". For each FAIL or PART run: the failing checks (read its log), the cause on Linux (Ctrl vs ⌘ labels from isMac/navigator.platform, the case-sensitive filesystem e.g. "exports" vs "exports 2", the file watcher with no event source, fonts, timing, something else), which survey piece (OS-nn) and the fix. Re-run single suites to confirm, using the binaries the lead already built in the repo: from ${REPO}, $SCRATCH/tw e2e-<suite> 'E2E_CHROME=<chrome path above> M2_HEADLESS=target/debug/examples/headless node e2e/run.mjs <suite> [kind] --out $SCRATCH/scratch-ground-e2e/<suite>'. Do not run cargo in the repo's own target dir. Also note any suite that passed only by luck (timing-sensitive checks; HANDOFF lists known flakes: s5-knobs, live3a, G3/G11 timing). Note what live3b's plugin part (CLAP_PATH) would need on Linux.
Write ${G}/e2e.md (create the folder if needed): a table (suite | checks failing | cause | OS-nn | fix), "In short" first. Return its path and a summary.`, { label: 'ground:e2e', phase: 'Ground', schema: NOTES }),

  () => agent(`${RULES}
YOUR TASK (label ground-facts): check on THIS machine the facts the platform design depends on, especially every claim the surveys mark (verify) or *(verify)*. Write small probe programs (C with cc, or a scratch Rust crate) in $SCRATCH/scratch-ground-facts/ and run them. At least:
1. renameat2 RENAME_EXCHANGE and RENAME_NOREPLACE: on / (ext4 here), on /dev/shm (tmpfs), and what errno an unsupported flag gives. FICLONE on ext4 (expected EOPNOTSUPP/EXDEV?). link(2) on ext4. The errno of fsync on a directory fd here.
2. Durability cost on this disk: time fsync vs fdatasync of a small file, fsync of a directory, and a commit-like sequence (write 3 files + fsync each + rename + dir fsync), idle and while another process writes 1 GB. Report p50/p95 over 50 runs. (The contract's PLANE-5 budget: a journaled data edit heard within 20 ms; on the Mac the barrier is F_FULLFSYNC.)
3. inotify: is an event readable (non-blocking read of the inotify fd) immediately after the writer's syscall returns, in the same process and from another process? Run it 100000 times and report any miss. The limits here (/proc/sys/fs/inotify/*). Behaviour on IN_Q_OVERFLOW. What a rename into a watched dir from an unwatched dir reports.
4. flock: per open file description (two opens in one process conflict?), released at exit; statfs f_type for / and /dev/shm; /proc/self/mountinfo fs type of /; /etc/machine-id present?; gethostname.
5. Audio: is there any sound device (aplay -l, /dev/snd, /proc/asound/cards)? Is PipeWire or PulseAudio running? What does cpal's default_host().default_output_device() give here (a tiny Rust probe using the workspace's cpal 0.18 version, own target dir)? Does opening a stream fail, and with what error/stderr noise? Is the user in the audio group, RLIMIT_RTPRIO, RLIMIT_RTTIME, rtkit-daemon present?
6. Floating point: which host-side render-path math differs between glibc and the pure-Rust libm crate? The calls are listed in survey-code §8 (core/runtime/src/audio.rs: cosf, powf, sin/cos; core/play/src/resample.rs). Write a probe that compares glibc vs the libm crate (the version already in Cargo.lock) over the inputs these calls really see (or a dense sweep), and count mismatches per function. Compare ${BASE}/renders/SHA256SUMS with the Mac hashes in HANDOFF.md: which renders differ? (Expected: only examples/resample.)
7. std.wasm reproducibility: build std/kernels-src for wasm32-unknown-unknown here (copy std/ to scratch first; see std/build.sh; own target dir) and compare with the committed std/kernels/std.wasm: same bytes? If not, what differs (strings with paths, rustc hash)? Would --remap-path-prefix make it equal to a Mac build? (You can't build on the Mac; reason from what differs.)
8. wasmtime on x86_64: vm.max_map_count, overcommit settings, ulimit -v; anything that limits many 4 GiB reservations.
9. The libraries the desktop app links on Linux (ldd target/debug/music2-desktop if present in the repo's target; read-only) and what a Linux user needs installed at runtime.
Write ${G}/facts.md (create the folder if needed): one section per item with the measured result, the command or probe, and what it means for the design. Return its path and a summary with the key numbers.`, { label: 'ground:facts', phase: 'Ground', schema: NOTES }),

  () => agent(`${RULES}
YOUR TASK (label ground-ci): the facts for checking the Mac with GitHub Actions, and for plugins on Linux. Use the web (current docs, 2026) and say where each fact comes from.
1. GitHub-hosted macOS runners: which labels are Apple silicon (macos-14/15/26 arm64?), their CPU/RAM/disk, whether they are virtualized M1s, the per-minute price for a PRIVATE repo of an organization in 2026 and the free-minute multiplier, job time limits. How the job gets: Rust 1.99.0 pinned with the wasm32-unknown-unknown target (rust-toolchain.toml or rustup), Node 24, chrome-headless-shell 133.0.6943.141 mac-arm64 (npx @puppeteer/browsers install), the cargo cache (Swatinem/rust-cache or actions/cache) and its size limit (10 GB per repo?).
2. How the lead (on this server, no gh CLI yet) starts a run and reads its result: workflow_dispatch needs the workflow file on the default branch; a push to a dedicated branch (e.g. check/mac) also triggers; reading run status and logs needs auth (gh auth login, a fine-grained token with actions:read, or the REST API). What exactly the owner must do once (install gh from Ubuntu's apt or GitHub's apt repo, then gh auth login), and the least-privilege token if they prefer.
3. What the macOS job should run so it proves the Mac is still green: build, cargo test (all), clippy -D warnings, cargo test -p music2-desktop (including its AppKit main-thread test: can it run on a headless runner, is there a window server/GUI session?), the web build, every browser suite with M2_AUDIO=none, and the 8 offline renders compared with the recorded hashes. Estimate minutes per full run and the cost.
4. Linux CLAP plugins for later gates (live3b's plugin part, examples/with-plugins): Surge XT and Airwindows Consolidated Linux x86_64 CLAP builds: official download URLs, package formats, how to install per-user WITHOUT sudo into a private folder for CLAP_PATH (e.g. extract the .deb), and their runtime library needs. Do not download or install anything now.
5. Optional and cheap: does a GitHub ubuntu-24.04 job add anything the server doesn't already give? (Probably not; say so.)
Write ${G}/ci.md (create the folder if needed): "In short" first, then a draft .github/workflows/mac.yml (inside the doc, not as a file) with comments, and the owner's one-time steps. Return its path and a summary.`, { label: 'ground:ci', phase: 'Ground', schema: NOTES }),

  () => agent(`${RULES}
YOUR TASK (label ground-branches): how the platform layer on main will meet the three unmerged branches. For each of origin/wip/step7, origin/taste, origin/sounds (git diff main...origin/<branch>, git log main..origin/<branch>): list every OS-specific piece it ADDS or CHANGES (cfg(target_os), libc/Mach/AppKit calls, macOS paths, ⌘ in web or e2e, chrome paths in e2e, host math on the render path, std.wasm), which survey piece (OS-nn) or later-step piece it relates to, and the files where a platform-layer change on main will conflict, with a merge note (who moves what where). Note especially: wip/step7's core/runtime/src/audio.rs changes (the libm change touches the same math), crates/gates/src/store.rs (Mach task_info, QoS), core/plugins changes (main thread, probe); taste's menu.rs and e2e suites (⌘ strings, testids); sounds' std.wasm and new render fixtures. Also say what each branch's own checks would need on Linux once merged (new tests that assume macOS).
Write ${G}/branches.md (create the folder if needed): "In short" first, a table per branch. Return its path and a summary.`, { label: 'ground:branches', phase: 'Ground', schema: NOTES }),
])
const ground = grounds.filter(Boolean)
log(`ground: ${ground.length}/5 notes`)
const GROUND = ground.map(g => `- ${g.file}: ${g.summary}`).join('\n')
const UPDATE = `
OWNER UPDATE (this overrides every GitHub Actions line above and in the ground notes): the owner can't pay for GitHub macOS runners now. There is NO GitHub Actions workflow in this step. Linux (this server) is the platform where everything must be green. The owner checks the Mac BY HAND on their MacBook now and then. So the design must give ONE script the owner runs on the Mac with one command (e.g. ci/check.sh, the same script on both OSes, choosing paths per OS). It runs the full check: build, every cargo test, clippy -D warnings, the desktop tests, the web build, every browser suite with the Mac's chrome path, and the 8 offline renders compared with the recorded hashes. It ends with a short plain pass/fail summary the owner can paste back. Say exactly when a Mac run is worth asking for (as few as possible: at least once at the end of this step, and before merging anything that changes macOS-only code). Keep macOS code changes small and mechanical. Compile-check macOS code from Linux where possible (e.g. cargo check --target aarch64-apple-darwin for crates that need no macOS SDK, if that works here; probe it, and say what it catches and what it cannot). Until the owner runs the Mac check, macOS is "unverified since <commit>", recorded honestly in the step's commit message and in HANDOFF.md. From the ground note ci.md, use only what the Mac check must run and the Linux plugin facts.`
const RULES2 = RULES + UPDATE

phase('Design')
const DESIGN_TASK = (label, file, angle) => `${RULES2}
GROUND NOTES (read the files; they are measured on this machine and outrank the surveys):
${GROUND}
YOUR TASK (label ${label}): write a complete design for the platform layer to ${file}. ANGLE: ${angle}
The design must contain, in this order:
1. "In short" (plain words, for the owner: what changes, what they will notice, what they must do once).
2. Where the layer lives and its shape: crate(s) and modules, the exact public API (Rust signatures), what stays behind cfg in its own crate and why. Explain why this shape and not the alternative (free functions per module vs one trait with associated types; one crate vs per-crate cfg modules).
3. Every OS piece: walk ALL 85 survey pieces (OS-01..OS-85), grouped: for each, what this step does (built now on Linux and macOS / designed only / deferred to step N with the reason / no change needed), its Linux implementation with honest durability and real-time notes, and the Windows plan in one line. Use the ground notes' measured facts.
4. Every failing Rust test, clippy lint and browser suite from the ground notes: which change fixes it. Linux green must mean: all cargo tests (workspace, desktop included), clippy -D warnings on the whole workspace, the desktop app builds, every browser suite ok (29 runs; live3b's plugin part may be PART without plugins, say how to make it ok), offline renders identical on Linux and the Mac.
5. Determinism: the libm change (exact calls, and that it lands as one deliberate change of the recorded hashes on both OSes), std.wasm reproducibility (remap-path-prefix, rust-toolchain.toml pin) and what it means for kernel hashes, x86 vs arm wasm float behaviour, denormals on x86 (OS-49) as a later 8a/gate item.
6. Contract edits: exact text changes for rules that name macOS mechanisms (PLANE-5, WRITE-15/17/19, FMT-16/20, RENDER-30, HOST-2/3, the acceptance gates' "reference Mac"), a new rule family for the platform (do NOT call it OS-, which the survey ids use; e.g. PLAT-n), and the order-of-work entry for this step.
7. Verification: what runs on this server (one script; tw/tmux), the by-hand Mac check (the full draft script, one command, plain summary), when to ask the owner for a Mac run, the Linux-side compile checks of macOS code, and what counts as the Mac being green. Timings on Linux are informational; judged timings stay on the reference Mac.
8. Build slices: each slice small, green on Linux at its end (all tests, clippy, suites it touches), with byte-identical offline renders except the one deliberate libm slice, and which slices need a by-hand Mac run (as few as possible; at least the end of the step). Order them so that the merges of wip/step7, sounds and taste stay easy; say which conflicts to expect and how to resolve them.
9. How 8a, 8b and 9 plug in later (which module each fills in, without designing them).
10. Owner questions: only real decisions, each with a recommended answer, in plain words.
Keep it as small as the angle allows; no abstraction for its own sake. Return the path and a summary.`
const designs = await parallel([
  () => agent(DESIGN_TASK('design-small', `${P}/design-small.md`, 'SMALLEST. The least code that makes Linux green and keeps macOS identical (except the deliberate libm change), with one obvious place per OS for what differs. Prefer free functions in a few cfg-selected modules; add structure only where today\'s code needs it. Design 8a/9 pieces only as far as needed so they will not force a rewrite.'), { label: 'design:small', phase: 'Design', schema: NOTES }),
  () => agent(DESIGN_TASK('design-shaped', `${P}/design-shaped.md`, 'SHAPED FOR WHAT COMES. Still lean, but shaped so steps 8a (realtime engine threads, workgroups, core topology, wake), 9 (plugin host process, plugin windows, recording, disk streaming, MIDI) and a later Windows build each fill in one obvious place for every OS without moving code again (survey-later §5 is a starting point to challenge, not to copy). Show the Windows compile check if it is cheap.'), { label: 'design:shaped', phase: 'Design', schema: NOTES }),
])
const D = designs.filter(Boolean)

phase('Synthesize')
const syn = await agent(`${RULES2}
GROUND NOTES:\n${GROUND}
TWO DESIGNS: ${D.map(d => `${d.file}: ${d.summary}`).join('\n')}
YOUR TASK (label synthesize): read both designs fully and write ONE design, ${P}/design.md (revision 1), taking the better answer section by section: the smallest shape that does not force 8a/9/Windows to move code again. Keep the ten sections and their order (In short; shape and API; all 85 OS pieces; every failure → fix; determinism; contract edits; verification incl. the by-hand Mac check; build slices; how 8a/8b/9 plug in; owner questions). Add a short "Choices" table: each place the two designs differed, which answer was taken, and why. Check every factual claim you keep against the ground notes. Return the path and a summary.`, { label: 'synthesize', phase: 'Synthesize', schema: NOTES })

phase('Attack')
const ATTACK = { type: 'object', properties: { problems: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, where: { type: 'string' }, scenario: { type: 'string' }, fix: { type: 'string' } }, required: ['title', 'severity', 'where', 'scenario', 'fix'] } } }, required: ['problems'] }
const LENSES = [
  ['durability', 'crash safety and durability of the Tx journal and the write path on Linux: fsync/fdatasync semantics and their cost (measured), directory syncs, renameat2 support gaps and the fallbacks (tmpfs, overlayfs, NFS, SMB, FUSE, vfat/exfat), the clone fallback, the project lock (flock on NFS/SMB, two sessions in one process, read_holder), host identity (machine-id missing, containers, cloned VMs), the local-volume check, case-insensitive volumes and stored spelling. Anything accepted before it is durable?'],
  ['watcher', 'the file watcher on Linux: the inotify design (one watch per directory, new and moved-in directories, races between a new directory and its first files, IN_Q_OVERFLOW and rescans, limits and ENOSPC, the root moved or deleted, the session\'s own writes and own_stamp, the "flush" claim vs what ground-facts measured, idle CPU, remote volumes), its tests, and the four watch tests plus the two graph tests that fail today.'],
  ['macos', 'macOS SAFETY, since nobody can run the Mac locally anymore: anything in the design that could silently change macOS behaviour or break its build (cfg mistakes, moved code, the AppKit main thread and menus, the plugin main thread, FSEvents, F_FULLFSYNC, renamex_np), whether the by-hand Mac check script really proves the Mac green (all tests, clippy, the desktop tests, every browser suite, renders), what it cannot prove (timing gates), and what the Linux-side compile checks catch in macOS code before the owner runs it, and the determinism story (libm, std.wasm reproducibility and its effect on kernel hashes and renders.json, wasm float semantics on x86_64 vs aarch64, NaN canonicalisation off).'],
  ['simplicity', 'simplicity and fit: abstractions not needed today, an API that is too big or too small, code moved for no gain, things that will not fit 8a\'s realtime threads and wake, step 9\'s host process, plugin windows (X11 on Linux), recording, MIDI, disk streaming, or a later Windows build; merges with wip/step7, taste and sounds made harder than needed; contract edits that weaken a guarantee or blur macOS as the reference.'],
  ['verification', 'verification and slices: does the plan really make ALL cargo tests, clippy -D warnings (whole workspace incl. the desktop), and all 29 browser-suite runs pass on this Linux server, and keep them passing on the Mac? Can each slice land green and be checked here with tw/tmux? Is the by-hand Mac check script realistic for the owner (one command, plain summary, no paid services, the Mac\'s chrome path and toolchain)? Linux audio with no device, the Linux desktop app (builds; no display here), plugins on Linux for later gates, timing flakes. Missing or untestable acceptance criteria.'],
]
const attacks = await parallel(LENSES.map(([key, lens]) => () => agent(`${RULES2}
GROUND NOTES:\n${GROUND}
DESIGN: ${P}/design.md (read it fully; the two source designs are ${P}/design-small.md and ${P}/design-shaped.md).
YOUR TASK (label attack-${key}): attack the design on ${lens}
Report only REAL problems, each with a concrete scenario (inputs, state, what goes wrong) and a fix that keeps the design simple. Check claims against the code and the ground notes; probe on this machine when a claim is cheap to test. Do not pad the list: three solid problems beat ten weak ones. Also write them to ${P}/attack-${key}.md for the record.`, { label: `attack:${key}`, phase: 'Attack', schema: ATTACK })))
const problems = attacks.filter(Boolean).flatMap((a, i) => a.problems.map(p => ({ lens: LENSES[i][0], ...p })))
log(`attack: ${problems.length} problems (${problems.filter(p => p.severity === 'high').length} high)`)

phase('Revise')
const final = await agent(`${RULES2}
GROUND NOTES:\n${GROUND}
DESIGN: ${P}/design.md
PROBLEMS FROM FIVE ATTACKS (also in ${P}/attack-*.md):
${JSON.stringify(problems, null, 1)}
YOUR TASK (label revise): judge each problem skeptically (check it against the code, the ground notes and the design; reject it, with the reason, if it is wrong or not worth its cost) and revise ${P}/design.md in place to revision 2. Fold each accepted problem into the right section. Add an "Attack response" table (problem | lens | severity | accepted or rejected | what changed or why not). Keep it simple: a fix that adds a layer for a rare case must earn it. End with "Build slices" (final) and "Owner questions" (only real decisions, each with a recommended answer, in plain words). Make sure "In short" is plain and still true. Return the summary and the owner questions.`, { label: 'revise', phase: 'Revise', schema: { type: 'object', properties: { summary: { type: 'string' }, owner_questions: { type: 'array', items: { type: 'string' } }, rejected: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'owner_questions', 'rejected'] } })

phase('Critic')
const critic = await agent(`${RULES2}
GROUND NOTES:\n${GROUND}
DESIGN: ${P}/design.md (revision 2), the surveys, the attack files ${P}/attack-*.md.
YOUR TASK (label critic): be the completeness critic and skeptic. Check that:
(a) every survey piece OS-01..OS-85 has a decision (built now / designed / deferred to a named step with a reason / no change) and none is silently dropped;
(b) every failing Rust test, clippy lint and browser suite from the ground notes maps to a slice that fixes it, and the slice's done-criteria are checkable on this server;
(c) every accepted attack problem is really reflected in the text, and the rejections are sound;
(d) the build slices are in a sensible order, each lands green, the deliberate render-hash change is isolated in one slice, and the by-hand Mac runs are few and placed where they must be;
(e) the contract edits keep every guarantee (nothing weakened silently) and keep the Mac as the timing reference;
(f) "In short" and "Owner questions" are plain, short and true.
Fix small gaps directly in ${P}/design.md (note each fix in a "Critic pass" list at the end). List bigger ones as open issues for the lead. Return them.`, { label: 'critic', phase: 'Critic', schema: { type: 'object', properties: { fixed: { type: 'array', items: { type: 'string' } }, open_issues: { type: 'array', items: { type: 'string' } } }, required: ['fixed', 'open_issues'] } })

return { ground: ground.map(g => g.summary), designs: D.map(d => d.summary), synthesis: syn && syn.summary, problems: problems.map(p => `${p.severity} [${p.lens}] ${p.title}`), final, critic }

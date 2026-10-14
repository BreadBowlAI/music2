export const meta = {
  name: 'step7-engine-build',
  description: 'Build step 7 (the engine) from .git/m2-step7/design-7.md in its four green slices, verify every gate, review, confirm, fix',
  phases: [
    { title: 'Slice 1', detail: 'movable instances (own Store per kernel), Plan/Lanes in Renderer, two fixes, G7f measured' },
    { title: 'Slice 2', detail: 'the arena: boxes moved at activation, ghosts and edge fades, identity Activates' },
    { title: 'Slice 3', detail: 'names that survive edits (token diff, call by "(", ComponentId, key() cut, ID-14 counts)' },
    { title: 'Slice 4', detail: 'plugins persist: state loads into kept instances, export reuses them (Hold)' },
    { title: 'Verify', detail: 'G7a-G7f, all earlier gates, tests, identity, RENDER-33, suites' },
    { title: 'Review', detail: 'real time and identity; a producer listening; conformance and simplicity' },
    { title: 'Confirm', detail: 'skeptics refute findings' },
    { title: 'Fix', detail: 'fix confirmed findings, rerun everything' },
  ],
}

const REPO = '/Users/jerryxu/dev/suite2/music2'
const S = '/private/tmp/claude-501/-Users-jerryxu-dev-suite2-music2/07b91ec3-d8c4-49ce-985f-b33193c681f7/scratchpad'
const D = `${REPO}/.git/m2-step7`
const TGT = `${S}/target-s7`
const W = `${S}/s7`

const RULES = `
Repository: ${REPO} (music2: a DAW where TypeScript defines the music; Rust core core/{ir,script,runtime,song,session,play,plugins,kernel}, std/, crates/{server,gates,editor}, web/, apps/{music2-desktop,m2}). Notes and logs for this job go in ${W}/ (create it).
THE CONTRACT ${REPO}/docs/architecture.md is normative. Steps 1-6 and the G11 fix are BUILT and committed (HEAD 8aa301e: "Fast note edits": play-first lane previews via SwapLane in crates/server/src/graph/preview.rs, a faster Tx path, an FSEvents watcher). YOU ARE BUILDING STEP 7 ("The engine") from THE DESIGN ${D}/design-7.md (revision 2; read §0-§2 and the sections your slice names, §13 tests, §14 deletions, §15 contract edits, §17 attack response, §18 build slices; earlier notes in ${D}/understand-*.md). The design was written while the G11 workflow was editing: it says to leave SwapLane, reach, delta_between and the value fast path as that workflow writes them — they are now committed; build on them (the G11 preview path must keep working: G11 tx-data stays under 20 ms at p95 and e2e g11-play passes).
Owner decisions (taken): SiteId is a point (file + the call's "("), never a counter; a poly is kept or replaced whole; fades on changed edges into kept nodes and on the output (ghosts ring out 5 ms); reordered kept effects restart; a plain number inside an instrument body is part of its shape; export with plugins is a tape pause (Hold); after export each instance holds the committed blob; no LoadState/SetArrangement messages and activations never coalesce; std mixer keyed strips are a follow-up outside step 7.
THE OWNER wants a tasteful, professional DAW that is "simple yet elegant": prefer the simplest code that meets each gate, DELETE what the design lists as deleted (§14), and keep core small (report core line counts before/after for core/*). Never weaken a guarantee a producer would hear (no clicks, no stuck notes, tails of untouched tracks intact, exports exact, no audio-thread allocation or locks).
Hard rules:
- Do NOT commit, push, stash, reset, rebase or create branches; leave changes uncommitted. Never touch refs/backup/* or the "taste" branch/worktree (another workflow restyles web/ in ${S}/wt-taste; never edit it; avoid gratuitous web/ edits here).
- Do NOT launch music2-desktop; never kill music2-desktop processes. Never use the real ~/.music2 or ~/Music: HOME=${S}/home. Cargo needs RUSTUP_HOME=/Users/jerryxu/.rustup CARGO_HOME=/Users/jerryxu/.cargo PATH=/Users/jerryxu/.cargo/bin:$PATH when HOME is changed (set HOME only for the commands that need it). CLAP: CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP when needed.
- DISK: ONE shared cargo target dir CARGO_TARGET_DIR=${TGT}. Worktrees or scratch copies (e.g. for G7f's HEAD measurement) share it or use ${TGT}-scratch, deleted when done. No other target dirs.
- Every slice lands GREEN before the next starts: build, tests, clippy, offline byte-identity, RENDER-33, the allocation trap, the existing gates. Never weaken a test or gate; when a test's expectation must change because behaviour intentionally improves (e.g. tails now survive), say exactly which and why.
- Apply each slice's contract edits (§15) in the same slice, with Today lines that match the code.
- Match the surrounding code; short "why" comments; rule ids in comments on tests; Rust up to 140 cols.
`
const TESTING = `
TESTING (CARGO_TARGET_DIR=${TGT}):
- cargo build --workspace; cargo test --workspace; cargo clippy --workspace --all-targets -- -D warnings (0 warnings); std/kernels-src tests if you touch kernels.
- Fixtures: for d in examples/* templates/*; do HOME=${S}/home cargo run -q -p m2 -- check $d; done.
- Offline byte-identity: HOME=${S}/home e2e/baseline.sh --tree . <out> (7 cases; names add -for6) plus templates/empty --from 4 --seconds 6 by hand; compare with ${S}/baseline/SHA256SUMS (8 cases, names without -for6).
- Gates (release): cargo run -p m2-gates --release --features engine -- --quick; cargo run -p m2-gates --release -- --server (G1 G3 G5 G8 G9 G11 tx-data and watcher); CLAP_PATH=... cargo run -p m2-gates --release -- plugins --quick; and the new step 7 gates as the design defines them (G7a-G7f) on p50; once a dev-build --server run too. G3 sits near its line (p95 16-22 ms at n=10 under load): report every run honestly.
- Web (only if touched): cd web && npx tsc --noEmit -p . && npm run build.
- Browser suites: cargo build -p m2-server --example headless && cargo build -p m2; M2_HEADLESS=${TGT}/debug/examples/headless node e2e/run.mjs all (all ok except live3b PART without CLAP_PATH), then CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP ... node e2e/run.mjs live3b (51/51). New suites go into e2e/run.mjs SUITES and the e2e/README.md table.
`
const OUT = { type: 'object', properties: { summary: { type: 'string' }, files_changed: { type: 'array', items: { type: 'string' } }, gates: { type: 'string', description: 'every gate run with numbers' }, checks: { type: 'string' }, deleted: { type: 'array', items: { type: 'string' } }, contract_changes: { type: 'array', items: { type: 'string' } }, core_lines: { type: 'string' }, caveats: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'files_changed', 'gates', 'checks', 'deleted', 'contract_changes', 'core_lines', 'caveats'] }

const slices = [
  { phase: 'Slice 1', key: 's1', task: 'SLICE 1 of §18: movable instances (a Store per kernel instance, PolyNode pool box, AudioPlayer per lane, engine-wide note ids), the two one-line fixes (jumps win over Enter; changed constants are sent to plugins), Plan and Lanes inside Renderer, and G7f part 1 measured with the rule written in advance (record HEAD 8aa301e as S from a worktree sharing the target dir, removed after); decide the store layout per §11.3 and apply it now if P1 fails.' },
  { phase: 'Slice 2', key: 's2', task: 'SLICE 2 of §18: the arena (boxes moved at activation by a move list computed off the audio thread, mem::swap only, ghosts for one 5 ms window, linear edge fades exact when equal, the cycle rule, poisoned boxes reset at entry, identity Activates for tempo/meter/audio/asset/state/arrangement changes and scheduler growth, keyed plugins joined by (module, state path) first, continues() over the step 6 delta names), with its tests and the gates it proves (G7a, G7b for untouched tracks, G7c, G7d, G7f part 2); delete what §14 lists for this slice.' },
  { phase: 'Slice 3', key: 's3', task: 'SLICE 3 of §18: names that survive edits: the token-level diff inside line hunks, calls named by their "(", the ComponentId in chain segments, the key() cut, ID-14 occurrence counts, so wrapping an instrument in an effect keeps the instrument (G7b wrapped instrument); the optional deletions (editor carry_text/carry_ends/carry_lines, m2_ir::control_id) if they now fall out cleanly.' },
  { phase: 'Slice 4', key: 's4', task: 'SLICE 4 of §18: plugins persist: state loads into kept instances in prepare (main-thread calls), restart from the committed blob, export of a song with plugins pauses playback (Hold), reuses the live instances in offline mode and resumes with their state intact (G7e); with-plugins new_state rewritten as the design says.' },
]
const reports = {}
for (const sl of slices) {
  phase(sl.phase)
  const r = await agent(`${RULES}\n${TESTING}\nEARLIER SLICES: ${JSON.stringify(reports)}\nYOUR TASK: ${sl.task}\nLand it green (TESTING), with its contract edits. Report honestly with numbers.`, { label: `build:${sl.key}`, phase: sl.phase, schema: OUT })
  reports[sl.key] = r
  log(`${sl.key}: ${r ? r.gates.slice(0, 300) : 'FAILED'}`)
}

phase('Verify')
const VERIFY = { type: 'object', properties: { cargo: { type: 'string' }, offline_identity: { type: 'string' }, gates: { type: 'string', description: 'G7a-G7f and every earlier gate, release and dev, with numbers' }, web: { type: 'string' }, e2e: { type: 'array', items: { type: 'object', properties: { suite: { type: 'string' }, passed: { type: 'number' }, failed: { type: 'number' } }, required: ['suite', 'passed', 'failed'] } }, core_size: { type: 'string' }, fixes: { type: 'array', items: { type: 'string' } }, open_issues: { type: 'array', items: { type: 'string' } } }, required: ['cargo', 'offline_identity', 'gates', 'web', 'e2e', 'core_size', 'fixes', 'open_issues'] }
const verified = await agent(`${RULES}\n${TESTING}\nStep 7 is built. Reports: ${JSON.stringify(reports)}\nYOUR JOB: prove it independently. Run EVERY step 7 gate from the contract's Acceptance gates row 7 (p50 structural edit at 3:00 audible within 200 ms after the compile with untouched tails intact; live with no edits bit-identical to offline; a rebuilt kernel module with the same id and a new hash starts from init; an export of examples/with-plugins pauses playback, reuses its instances and resumes with their state intact; the Store cost at 6,176 instances measured and the layout chosen) plus every earlier gate, all tests, clippy, fixtures, offline identity, RENDER-33, the allocation trap and every browser suite. Check the contract's Today lines match the code and mark step 7 "Built." in the order of work only if every gate passes. Update README.md and docs/gaps.md for user-visible changes (edits keep other tracks sounding). Report core size (git show 8aa301e vs now for core/*). Fix bugs (never weaken tests). Report honestly.`, { label: 'verify', phase: 'Verify', schema: VERIFY })
log(`verify: ${verified ? verified.gates.slice(0, 400) : 'FAILED'}`)

phase('Review')
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, file: { type: 'string' }, rule: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, scenario: { type: 'string' }, evidence: { type: 'string' } }, required: ['title', 'file', 'severity', 'scenario', 'evidence'] } } }, required: ['findings'] }
const LENSES = [
  { key: 'realtime', prompt: 'Real time and identity: trace every path from the audio callback for allocation, locks, IO, waits (prepare, activate, moves, ghosts, fades, retire, Hold); instance matching that keeps the wrong state (moved calls, duplicated tracks, swapped identities, new wasm hash, changed shape/channels/voices, keyed plugins swapped); memory growth from retired boxes; plugin threading (HOST rules) and state loads.' },
  { key: 'listening', prompt: 'A producer listening: capture live output (silent player capture) around structural edits on p16/p50 and the fixtures while playing (add/remove effect, edit inside a track, wrap an instrument, add a call above, tempo change, audio clip edit, plugin edits, export with plugins and resume), and compare tails, clicks (sample-to-sample steps), stuck or doubled notes, dropped notes and drift against offline; drive the app with puppeteer for transport/edit regressions.' },
  { key: 'conformance', prompt: 'Conformance and simplicity: rule by rule for every rule step 7 touches and the contract edits made; re-run a sample of the gate measurements and check they measure what they claim; anything the design listed as deleted that still exists, or new machinery that is not needed for a gate; core line growth and whether it is justified.' },
]
const reviews = await parallel(LENSES.map(l => () => agent(`${RULES}\n${TESTING}\nStep 7 is implemented (uncommitted). Verify report: ${JSON.stringify(verified)}\nReview through ONE lens; report only REAL problems you confirmed (reproduce them). Lens: ${l.prompt}\nDo not modify repository files (scratch copies build into ${TGT}-scratch; delete it when done).`, { label: `review:${l.key}`, phase: 'Review', schema: FINDINGS })))

phase('Confirm')
const VERDICTS = { type: 'object', properties: { verdicts: { type: 'array', items: { type: 'object', properties: { index: { type: 'number' }, real: { type: 'boolean' }, reasoning: { type: 'string' } }, required: ['index', 'real', 'reasoning'] } } }, required: ['verdicts'] }
const groups = LENSES.map((l, i) => ({ key: l.key, items: (reviews[i] || { findings: [] }).findings }))
const judged = await parallel(groups.filter(g => g.items.length).map(g => () => agent(`${RULES}\nReviewers claim these problems in the uncommitted step 7 change (lens ${g.key}):\n${JSON.stringify(g.items.map((f, i) => ({ index: i, ...f })), null, 1)}\nFor EACH, try hard to REFUTE it by reading the code and reproducing (scratch copies build into ${TGT}-scratch; delete it when done). Default to real=false if it doesn't hold up, is taste, or belongs to a later step. Do not modify repo files. One verdict per index.`, { label: `confirm:${g.key}`, phase: 'Confirm', schema: VERDICTS }).then(v => g.items.map((f, i) => ({ ...f, verdict: v ? v.verdicts.find(x => x.index === i) : null })))))
const all = judged.filter(Boolean).flat()
const confirmed = all.filter(f => f.verdict && f.verdict.real)
log(`${confirmed.length} of ${all.length} confirmed`)

phase('Fix')
let fixed = null
if (confirmed.length) {
  fixed = await agent(`${RULES}\n${TESTING}\nFix these confirmed problems (tests where practical, rule ids in comments):\n${JSON.stringify(confirmed, null, 1)}\nThen rerun everything in TESTING, every step 7 gate and every browser suite. Report honestly.`, { label: 'fix', phase: 'Fix', schema: VERIFY })
}
return { reports, verified, findings: all.map(f => ({ title: f.title, severity: f.severity, real: f.verdict && f.verdict.real })), fixed }

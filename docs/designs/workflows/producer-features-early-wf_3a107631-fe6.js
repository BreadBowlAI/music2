export const meta = {
  name: 'producer-features-early',
  description: 'On the new-look branch: bring in the note-edit fix, then build the first producer features (add/duplicate/delete/reorder/recolour tracks, the song bar with tempo and time signature, the Mix view with faders) beside step 7; verify, review, fix',
  phases: [
    { title: 'Merge', detail: 'merge main (8aa301e) into the taste branch; everything green' },
    { title: 'Tracks', detail: '8b.1: + Track with an instrument, duplicate, delete, reorder, recolour' },
    { title: 'Song bar', detail: '8b.2: tempo, time signature, title in the top bar' },
    { title: 'Mix', detail: '8b.3: the Mix view with volume and pan faders' },
    { title: 'Verify', detail: 'all tests, every suite, taste lint, screenshots' },
    { title: 'Review', detail: 'a producer uses it; a correctness check of the Tx builders' },
    { title: 'Fix', detail: 'fix confirmed problems; rerun everything' },
  ],
}

const MAIN = '<repo>'
const S = '<scratchpad>'
const WT = `${S}/wt-taste`
const TGT = `${S}/target-taste`
const D = `${MAIN}/.git/m2-step8`
const OUT8 = `${S}/ux8-8b`

const RULES = `
You work in the git worktree ${WT} (branch "taste": steps 1-6 plus the new look, commit 9c6af26 on top of cbf5eb9). Edit ONLY files under ${WT}. Never edit ${MAIN}'s working tree (another workflow is building step 7, the engine, there) or ${S}/wt-sounds.
music2 is a DAW where TypeScript defines the music (Rust core core/*, std/, crates/{server,gates,editor}, web/ React+Tailwind UI in a Tauri app). The contract ${MAIN}/docs/architecture.md is normative. THE DESIGNS: ${D}/design-8b.md (§0-§5 for slices 8b.1-8b.3, §2 shared pieces, §12 what waits for step 7, §14 contract edits, §18 attack response, §19 build slices) and ${D}/taste.md (the look every feature follows; its slice table §8). The owner wants a tasteful, professional DAW, "simple yet elegant"; they are a beginner; plain words in the UI.
Owner decisions: the level slider lives only on the Mix strip (no header slider); tempo drag lets audio drift until release (that is 8b.7, not now); no tap tempo; MIDI keyboard later (8b.10).
These slices are built BEFORE step 7 lands (design-8b §12 says 8b.1-8b.3 can): until step 7, an add or delete of a track builds a new renderer (tails cut) and a tempo change rebuilds; that is expected now; do not try to build step 7's arena here. Keep every engine-facing change minimal so the later merge with step 7 (which rewrites core/runtime, core/play and parts of crates/server/src/graph/actor.rs and graph.rs) stays easy: prefer Tx builders in crates/editor, routes in crates/server/src/routes.rs, and web/.
Hard rules:
- You MAY commit in ${WT} on branch "taste" (one commit per slice, clear message ending with the line "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"), but never push, never rebase, never reset --hard, never touch other branches or refs/backup/*.
- Do NOT launch music2-desktop; never kill music2-desktop processes; never use the real ~/.music2 or ~/Music (HOME=${S}/home-8b for servers and the CLI; cargo then needs RUSTUP_HOME=/Users/<user>/.rustup CARGO_HOME=/Users/<user>/.cargo PATH=/Users/<user>/.cargo/bin:$PATH).
- ONE cargo target dir CARGO_TARGET_DIR=${TGT}; run cargo with nice -n 10 (the machine is shared with two other workflows).
- Every slice lands green. Never weaken a test. Offline renders of the fixtures stay byte-identical (${S}/baseline/SHA256SUMS; e2e/baseline.sh --tree ${WT} <out> plus templates/empty --from 4 --seconds 6) unless the slice intentionally changes a template, which these slices must not.
- Match the surrounding code; short "why" comments; rule ids in comments on tests.
`
const TESTING = `
TESTING (in ${WT}, CARGO_TARGET_DIR=${TGT}):
- PER SLICE (fast): cargo build --workspace; cargo test -p m2-editor -p m2-server -p m2-session -p m2-song (and any crate you touched); cargo clippy --workspace --all-targets -- -D warnings; (cd web && npx tsc --noEmit -p . && npm run build); the slice's own new browser suite (e2e/s8-*.mjs, registered in e2e/run.mjs SUITES and the README table) plus e2e taste (the lint) and the suites the slice's UI touches.
- AT VERIFY (full): cargo test --workspace; fixtures (m2 check on examples/* templates/*); offline byte-identity; M2_HEADLESS=${TGT}/debug/examples/headless node e2e/run.mjs all --repo ${WT} (all ok except live3b PART without CLAP_PATH) and CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP ... live3b; build the headless server and m2 first (cargo build -p m2-server --example headless && cargo build -p m2).
- Screenshots at 1440x900 with puppeteer-core from ${WT}/e2e/node_modules against a headless server from ${WT}'s build (see ${S}/ux8-taste/scripts/ for shot scripts); put them in ${OUT8}/ and LOOK at them.
`
const OUT = { type: 'object', properties: { summary: { type: 'string' }, commit: { type: 'string' }, files_changed: { type: 'array', items: { type: 'string' } }, checks: { type: 'string' }, contract_changes: { type: 'array', items: { type: 'string' } }, caveats: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'commit', 'files_changed', 'checks', 'contract_changes', 'caveats'] }

phase('Merge')
const merged = await agent(`${RULES}\n${TESTING}\nYOUR TASK: in ${WT}, run git merge 8aa301e (main: "Fast note edits": play-first previews, a faster Tx path, an FSEvents watcher, a g11-play e2e suite, toasts for a taken-back or slow save in web/src/App.tsx and lib/store.ts). Resolve every conflict so BOTH sides hold: the new look's shell and toast rules (taste.md §2.14: toasts above the dock, one at a time; the status pill) and the fix's messages (taken back, not saved yet, saved) shown in the new style; e2e/run.mjs SUITES and the README table keep both sides' suites and counts (g11-play included; suites the new look changed keep its changes). Then run the FULL checks from AT VERIFY and commit the merge. Report.`, { label: 'merge', phase: 'Merge', schema: OUT })
log(`merge: ${merged ? merged.checks.slice(0, 300) : 'FAILED'}`)

const SLICES = [
  { phase: 'Tracks', key: '8b.1', task: 'design-8b §3, slice 8b.1 Tracks: + Track (pick an instrument from std; writes tracks/<key>.ts, imports it in song.ts, a mixer channel and an empty notes lane, one Tx with one undo step), duplicate, delete, reorder and recolour tracks (colours by name in layout.json per taste §2.7, with its freeze rules), the header menu, the Empty template comment only if the design says so (it must not change the template render), its toasts and undo words; the CLI commands the design lists; suite e2e/s8-tracks.mjs.' },
  { phase: 'Song bar', key: '8b.2', task: 'design-8b §4, slice 8b.2 Song bar: tempo, time signature and title in the top bar as editable fields (Tx builders on data/arrangement.json / the manifest as the design says; Home::Arrangement), New Song\'s tempo field (taste NS-3), the transport leftovers the design lists; no tap tempo, no tempo lease (8b.7 later); suite e2e/s8-songbar.mjs.' },
  { phase: 'Mix', key: '8b.3', task: 'design-8b §5, slice 8b.3 Faders: the Mix view (one strip per track plus the master) with volume faders with a -inf foot and pan, writing into the code as the design specifies (std mixer entries in the object\'s style, the first pan at the drag threshold), double-click to reset, Option-drag fine; no mute/solo overlays yet (8b.4 needs step 7) and no meters yet (8b.8); suite e2e/s8-mix.mjs.' },
]
const built = {}
for (const sl of SLICES) {
  phase(sl.phase)
  const r = await agent(`${RULES}\n${TESTING}\nEARLIER: ${JSON.stringify({ merged, built })}\nYOUR TASK: build ${sl.task}\nFollow taste.md for its UI; look at screenshots of it. Land it green with the PER SLICE checks, apply its contract edits (§14), and commit it on branch taste. Report honestly.`, { label: `build:${sl.key}`, phase: sl.phase, schema: OUT })
  built[sl.key] = r
  log(`${sl.key}: ${r ? r.checks.slice(0, 250) : 'FAILED'}`)
}

phase('Verify')
const VER = { type: 'object', properties: { cargo: { type: 'string' }, identity: { type: 'string' }, e2e: { type: 'array', items: { type: 'object', properties: { suite: { type: 'string' }, passed: { type: 'number' }, failed: { type: 'number' } }, required: ['suite', 'passed', 'failed'] } }, shots: { type: 'string' }, fixes: { type: 'array', items: { type: 'string' } }, open_issues: { type: 'array', items: { type: 'string' } }, commit: { type: 'string' } }, required: ['cargo', 'identity', 'e2e', 'shots', 'fixes', 'open_issues', 'commit'] }
const v = await agent(`${RULES}\n${TESTING}\nBUILT: ${JSON.stringify(built)}\nYOUR JOB: run the FULL checks (AT VERIFY), take screenshots of every new feature and look at them, update README.md and docs/gaps.md for what a user can now do (plain words; remove what is done from gaps), fix what is broken (never weaken a test), and commit. Report honestly.`, { label: 'verify', phase: 'Verify', schema: VER })

phase('Review')
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, where: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, evidence: { type: 'string' }, fix: { type: 'string' } }, required: ['title', 'where', 'severity', 'evidence', 'fix'] } } }, required: ['findings'] }
const reviews = await parallel([
  () => agent(`${RULES}\nVERIFY: ${JSON.stringify(v)}\nReview AS A. G. COOK starting a new song from Empty with music2: start a headless server from ${WT}'s build (HOME=${S}/home-8b), add tracks with instruments, rename, reorder, recolour, delete, undo, set tempo and time signature, mix with faders; take screenshots and look at them. Report only REAL problems (slow, confusing, ugly, broken) with evidence. Do not modify files.`, { label: 'review:producer', phase: 'Review', schema: FINDINGS }),
  () => agent(`${RULES}\nVERIFY: ${JSON.stringify(v)}\nReview the Tx builders and routes of 8b.1-8b.3 for correctness: each is one Tx with one undo step; refusals are clear; the planner assertions hold (Confined ranges, the mixer call declared whole as design-8b says); keys stay identity (ID-*); a track delete then undo restores bytes exactly; outside edits between steps; read-only lock respected; property-style tests where cheap. Reproduce real problems in scratch copies (${TGT}-scratch, deleted after). Do not modify files.`, { label: 'review:tx', phase: 'Review', schema: FINDINGS }),
])
const findings = reviews.filter(Boolean).flatMap(r => r.findings)
log(`${findings.length} findings`)

phase('Fix')
let fixed = null
if (findings.length) {
  fixed = await agent(`${RULES}\n${TESTING}\nReviewers found:\n${JSON.stringify(findings, null, 1)}\nCheck each; fix the real ones (tests where practical); skip taste disagreements with taste.md/design-8b unless clearly right. Rerun the FULL checks, look at screenshots, commit. Report each finding fixed / skipped (why).`, { label: 'fix', phase: 'Fix', schema: VER })
}
return { merged, built, v, findings: findings.map(f => `${f.severity}: ${f.title}`), fixed }

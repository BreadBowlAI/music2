export const meta = {
  name: 'taste-tokens-and-shell',
  description: 'Apply the taste direction to the app UI in a separate worktree: slice T (design tokens) then S (the shell), verified with screenshots, the taste lint and every browser suite; reviewed by a producer and a designer',
  phases: [
    { title: 'Tokens', detail: 'slice T: the @theme tokens, system font, one accent, sizes, radii, port shapes, track colours' },
    { title: 'Shell', detail: 'slice S: top bar, dock, status pill, toasts, headers, browser, shortcuts, D1 D2 D4 D12' },
    { title: 'Verify', detail: 'screenshots side by side, taste lint, every suite, tests' },
    { title: 'Review', detail: 'a pro producer and a product designer look at the new screens' },
    { title: 'Fix', detail: 'fix what they confirmed; rerun everything' },
  ],
}

const MAIN = '/Users/jerryxu/dev/suite2/music2'
const S = '/private/tmp/claude-501/-Users-jerryxu-dev-suite2-music2/07b91ec3-d8c4-49ce-985f-b33193c681f7/scratchpad'
const WT = `${S}/wt-taste`
const TGT = `${S}/target-taste`
const D = `${MAIN}/.git/m2-step8`
const U = `${S}/ux8`
const OUT8 = `${S}/ux8-taste`

const RULES = `
You work in a SEPARATE git worktree: ${WT} (branch "taste", at cbf5eb9 = steps 1-6 built). Edit ONLY files under ${WT}. The main checkout ${MAIN} is being edited by another workflow: never edit it (reading ${MAIN}/.git/m2-step8/* is fine).
music2 is a DAW where TypeScript defines the music; its UI is web/ (React + Tailwind v4 + Monaco) inside a native Tauri app (apps/music2-desktop, native menus in its menu.rs). The owner wants a tasteful, PROFESSIONAL DAW, good enough for A. G. Cook, "simple yet elegant". Light and muted, no dark mode, every tool visible, no built-in AI.
THE TASTE DIRECTION ${D}/taste.md is your spec (read all of it: principles §1, design system §2 with the paste-ready @theme block §2.15, the main window §3, the change list §4, shortcuts §5, deletions §6, attack response §7, slices §8). It was written from the audit ${D}/audit-ui.md and 98 screenshots in ${U}/shots/ (look at them). design-8b.md in the same folder describes the 8b features that come later: do NOT build 8b features (add track, tempo bar, mixer faders, mute/solo overlays, clip editing, audition, meters); leave clean places for them where taste.md's shell has them (e.g. the Mix view can say what it will hold, or be omitted until 8b.3; decide per taste.md and say so).
Owner decisions already made: the level slider leaves the track header (taste Q1: yes); keep the device's own buffer size.
Hard rules:
- Do NOT commit, push, stash, reset, rebase, or create branches; leave changes uncommitted in ${WT}.
- Do NOT launch music2-desktop; never kill music2-desktop processes. Never use the real ~/.music2 or ~/Music: HOME=${S}/home-taste for servers and CLI.
- DISK/CPU: ONE cargo target dir CARGO_TARGET_DIR=${TGT}; run cargo with nice -n 19 (a timing-sensitive workflow runs beside you). No other target dirs.
- Behaviour stays the same unless taste.md says to change it; every e2e suite keeps its meaning. The suites select by data-testid: keep every testid on the element that replaces the old one, and update a suite in the same change only where taste.md moves or removes an element (say which).
- Match the surrounding code; short "why" comments. Use lucide-react icons as taste.md names them.
`
const TESTING = `
TESTING (in ${WT}, CARGO_TARGET_DIR=${TGT}, cargo niced):
- Web: cd ${WT}/web && npm ci (once) && npx tsc --noEmit -p . && npm run build.
- Rust (if you touch Rust, e.g. menu.rs): nice -n 19 cargo build --workspace && nice -n 19 cargo clippy --workspace --all-targets -- -D warnings; cargo test -p music2-desktop (and whatever crate you touched).
- Browser suites: nice -n 19 cargo build -p m2-server --example headless && nice -n 19 cargo build -p m2; then (cd ${WT}/e2e && npm ci) once; M2_HEADLESS=${TGT}/debug/examples/headless node ${WT}/e2e/run.mjs all --repo ${WT} (expected: every suite ok except live3b PART without CLAP_PATH), and CLAP_PATH=~/Library/Audio/Plug-Ins/CLAP ... node ${WT}/e2e/run.mjs live3b --repo ${WT}.
- Screenshots: the audit's shot scripts are ${U}/{a-start,b-first,c-edit,d-states,e-templates,f-detail,g-more}.mjs with helpers ${U}/lib.mjs (they read ${U}/session.json for the server URL/token and write into ${U}/shots). Copy them into ${OUT8}/scripts/, point them at a headless server you start from the worktree's binary (headless writes the session file you give it) and at ${OUT8}/shots/, keep the same shot names, and run them at 1440x900. LOOK at every new shot beside its old one in ${U}/shots/.
- The taste lint e2e/taste.mjs (taste.md §0): fails on a computed font-size outside {11,12,13,15} px (outside .monaco-editor), a border-radius outside {0,2,4,8} that isn't >= 9999, any gradient background-image, any backdrop-filter, mono font outside [data-code]. Register it in e2e/run.mjs SUITES and the README table.
`
const OUT = { type: 'object', properties: { summary: { type: 'string' }, files_changed: { type: 'array', items: { type: 'string' } }, checks: { type: 'string' }, decisions: { type: 'array', items: { type: 'string' } }, suites_changed: { type: 'array', items: { type: 'string' } }, caveats: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'files_changed', 'checks', 'decisions', 'suites_changed', 'caveats'] }

phase('Tokens')
const t = await agent(`${RULES}\n${TESTING}\nYOUR TASK: build taste slice T (tokens) exactly as taste.md §2 and §8 describe: the @theme block (§2.15) replacing Tailwind's defaults, the system font (no fetched fonts), tabular numbers, the four type sizes and three weights, the spacing scale and control heights, radii 2/4/8, the grey ramp and the one accent (move the brand mauve out), status colours only for problems, the 12 track colours from the OKLCH formula as a TypeScript constant (colours by name; the layout.json freeze rule only if it is needed for T, otherwise leave to 8b.1 and say so), drawn graphics per §2.8 (grid, clips, playhead, loop, port SHAPES instead of port colours, cables, knobs), states (§2.9), home markers instead of italics (§2.10), one focus ring, Lucide at 1.5 stroke, motion tokens; the old-to-new class mapping applied across web/src; Monaco's theme to match. Mechanical, no behaviour change. Add e2e/taste.mjs (the lint). Run the shot scripts and LOOK at old vs new. Run TESTING. Report honestly.`, { label: 'build:tokens', phase: 'Tokens', schema: OUT })
log(`tokens: ${t ? t.checks.slice(0, 300) : 'FAILED'}`)

phase('Shell')
const s = await agent(`${RULES}\n${TESTING}\nSLICE T REPORT: ${JSON.stringify(t)}\nYOUR TASK: build taste slice S (the shell) as taste.md §3, §4, §5, §6 and §8 describe: the 44 px top bar (§3.3); the dock with Device/Notes/Mix/Code in place of the module tabs, its view one choice for the window and selection driving its content, never its view (§3.5, §3.7); one status pill in place of the banners, keeping testids attention, compile-banner, read-only-banner (§2.14, ST); toasts above the dock, only for structural results; track headers without captions or a level slider (§3.4); the side browser (§3.6); native accelerators only for Cmd and Ctrl chords, with focus routing in appCommand and the Opt-K shortcuts sheet (§5.3; apps/music2-desktop menu.rs); defects D1, D2, D4, D12 from audit-ui.md; the small-window rules (§3.8); the deletions of §6 that belong to the shell (keep §6.1's good parts). No 8b features. Run the shot scripts and LOOK at every new shot beside the old. Run TESTING; update suites only where an element moved (list them). Report honestly.`, { label: 'build:shell', phase: 'Shell', schema: OUT })
log(`shell: ${s ? s.checks.slice(0, 300) : 'FAILED'}`)

phase('Verify')
const VER = { type: 'object', properties: { web: { type: 'string' }, rust: { type: 'string' }, e2e: { type: 'array', items: { type: 'object', properties: { suite: { type: 'string' }, passed: { type: 'number' }, failed: { type: 'number' } }, required: ['suite', 'passed', 'failed'] } }, lint: { type: 'string' }, shots: { type: 'string', description: 'where the new shots are, and what you saw comparing them' }, fixes: { type: 'array', items: { type: 'string' } }, open_issues: { type: 'array', items: { type: 'string' } } }, required: ['web', 'rust', 'e2e', 'lint', 'shots', 'fixes', 'open_issues'] }
const v = await agent(`${RULES}\n${TESTING}\nREPORTS: ${JSON.stringify({ t, s })}\nYOUR JOB: verify independently. Run everything in TESTING. Regenerate every shot into ${OUT8}/shots/ and LOOK at each beside the old one; check every taste.md principle and the §4 change list item by item (done / not done / wrong). Fix what is wrong (never weaken a suite; a suite changes only where an element moved). Report honestly.`, { label: 'verify', phase: 'Verify', schema: VER })
log(`verify: ${v ? v.shots.slice(0, 300) : 'FAILED'}`)

phase('Review')
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, where: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, evidence: { type: 'string', description: 'shot file names and what is wrong in them' }, fix: { type: 'string' } }, required: ['title', 'where', 'severity', 'evidence', 'fix'] } } }, required: ['findings'] }
const reviews = await parallel([
  () => agent(`${RULES}\nThe new look is built in ${WT}; new shots in ${OUT8}/shots/, old in ${U}/shots/. Verify report: ${JSON.stringify(v)}\nReview AS A. G. COOK producing a pop record: start a headless server from the worktree build (HOME=${S}/home-taste) and use the app with puppeteer on fresh fixture copies; take your own shots and look at them. What slows you down, looks amateur, is hard to read, is in the wrong place, or regressed versus the old shots? Only real problems with evidence; do not ask for 8b features. Do not modify files.`, { label: 'review:producer', phase: 'Review', schema: FINDINGS }),
  () => agent(`${RULES}\nThe new look is built in ${WT}; new shots in ${OUT8}/shots/, old in ${U}/shots/. Verify report: ${JSON.stringify(v)}\nReview AS A SENIOR PRODUCT DESIGNER for pro tools (Ableton, Bitwig, Logic, Figma polish): alignment to the 4 px grid, type sizes, contrast (check with numbers), consistency of controls and icons, density, empty states, focus, hover; anything that is not simple and elegant, or deviates from taste.md. Zoom into crops (2x) where needed. Only real problems with evidence. Do not modify files.`, { label: 'review:designer', phase: 'Review', schema: FINDINGS }),
])
const findings = reviews.filter(Boolean).flatMap(r => r.findings)
log(`${findings.length} findings`)

phase('Fix')
let fixed = null
if (findings.length) {
  fixed = await agent(`${RULES}\n${TESTING}\nReviewers found these (a producer and a designer):\n${JSON.stringify(findings, null, 1)}\nFor each: check it in the shots and the code; fix it if it is real and within taste.md (skip taste disagreements with taste.md unless the reviewer is clearly right, and say why). Then rerun everything in TESTING, regenerate the shots into ${OUT8}/shots/ and look at them. Report honestly, listing each finding as fixed / skipped (why).`, { label: 'fix', phase: 'Fix', schema: VER })
}
return { t, s, v, findings: findings.map(f => `${f.severity}: ${f.title}`), fixed }

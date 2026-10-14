export const meta = {
  name: 'std-sounds',
  description: 'Grow std into a pro sound set (a polysynth with lead/keys/strings/pad/supersaw presets, FM keys and bells, a drum kit with snare/clap/hats/toms/perc) in a separate worktree; analyse renders; producer and sound-designer review',
  phases: [
    { title: 'Plan', detail: 'survey std and the kernel ABI; design a small, elegant sound set' },
    { title: 'Kernels', detail: 'new Rust/wasm kernels with tests' },
    { title: 'Instruments', detail: 'TypeScript components with descriptors; a demo song; renders analysed' },
    { title: 'Review', detail: 'a sound designer measures; a pop producer judges' },
    { title: 'Fix', detail: 'fix what they confirmed; rerun everything' },
  ],
}

const MAIN = '/Users/jerryxu/dev/suite2/music2'
const S = '/private/tmp/claude-501/-Users-jerryxu-dev-suite2-music2/07b91ec3-d8c4-49ce-985f-b33193c681f7/scratchpad'
const WT = `${S}/wt-sounds`
const TGT = `${S}/target-sounds`
const W = `${S}/sounds`

const RULES = `
You work in a SEPARATE git worktree: ${WT} (branch "sounds", at 8aa301e). Edit ONLY files under ${WT}; never edit ${MAIN} (another workflow is rebuilding the engine there) or ${S}/wt-taste. Notes, renders and analysis go in ${W}/ (create it).
music2 is a DAW where TypeScript defines the music. std/ is an ordinary package (no package is privileged; the core has no instruments or DSP): std/m2pkg.toml declares kernels (ports, units, defaults) compiled from std/kernels-src (Rust -> wasm32, built by std/build.sh into the checked-in std/kernels/std.wasm); std/src/*.ts declares components with descriptors (controls with units, ranges, defaults, smooth; settings; editor metadata) built from kernels: instruments.ts (bass, kick, hat, pluck, pad today), dsp.ts, filter.ts, fx.ts, math.ts, mix.ts, control.ts. Read the contract ${MAIN}/docs/architecture.md sections on packages, components/descriptors, the kernel ABI, voices/poly, RENDER-2 determinism and the input-memory check, and README.md's "Your own components".
THE OWNER wants a tasteful, PROFESSIONAL DAW, good enough for A. G. Cook (hyperpop/pop: bright supersaws, glassy FM bells and keys, punchy 808/909-style drums, metallic percussion, big pads, plucks, sub bass). "Simple yet elegant": prefer ONE excellent polysynth engine with a few named, well-voiced instruments built on it over many half-good ones; a drum kit as a single component that maps note pitches to voices (General MIDI numbers: 36 kick, 38 snare, 39 clap, 42 closed hat, 46 open hat with choke, 41/45/48 toms, 37 rim, 56 cowbell or a metallic perc) plus each drum usable alone; few, well-named controls with musical ranges and good defaults; sounds that are mix-ready (sensible levels, no DC, no clicks, no aliasing you can hear, tails that end).
Hard rules:
- Do NOT commit, push, stash, reset, rebase or create branches; leave changes uncommitted in ${WT}.
- EXISTING components and kernels must not change behaviour: every fixture's offline render stays byte-identical (baseline hashes ${S}/baseline/SHA256SUMS; render with HOME=${S}/home e2e/baseline.sh --tree ${WT} <out> plus templates/empty --from 4 --seconds 6 by hand). Templates and examples stay unchanged; add a NEW example (examples/sounds/: a short pop demo using every new sound, format v1 with keys, data files canonical) instead.
- Determinism (RENDER-2): same input, same bits; noise seeded; no time or randomness from outside; every kernel passes the debug input-memory check; no allocation in process; denormals handled.
- Do NOT launch music2-desktop; never use the real ~/.music2 or ~/Music (HOME=${S}/home-sounds). Cargo with HOME changed needs RUSTUP_HOME=/Users/jerryxu/.rustup CARGO_HOME=/Users/jerryxu/.cargo PATH=/Users/jerryxu/.cargo/bin:$PATH.
- DISK/CPU: ONE cargo target dir CARGO_TARGET_DIR=${TGT} for the workspace (kernels-src may use ${TGT}-kernels); run cargo and std/build.sh with nice -n 19 (a timing-sensitive workflow runs beside you).
- Match the surrounding code and comment style; short "why" comments.
`
const TESTING = `
TESTING (in ${WT}):
- Kernels: (cd std/kernels-src && nice -n 19 cargo test) and nice -n 19 std/build.sh (needs the wasm32-unknown-unknown target; it is installed if earlier steps built std).
- Workspace: nice -n 19 cargo build --workspace && nice -n 19 cargo test --workspace && nice -n 19 cargo clippy --workspace --all-targets -- -D warnings; the web build typechecks std's TypeScript: (cd web && npm ci && npx tsc --noEmit -p . && npm run build).
- Fixtures: for d in examples/* templates/*; do HOME=${S}/home-sounds cargo run -q -p m2 -- check $d; done (the new example included). Offline byte-identity of the old fixtures as in RULES.
- Renders: HOME=${S}/home-sounds cargo run -q --release -p m2 -- render examples/sounds ${W}/sounds.wav, and per-sound renders (small scratch projects in ${W}/proj-*) at several pitches and velocities; analyse with Python (numpy/scipy if available, else write the maths): peak and RMS/LUFS-ish loudness, DC offset, spectrum and aliasing above 16 kHz for high notes, click detection (sample-to-sample steps vs the signal's own), tail length to -60 dB, CPU per voice (time the render).
`
const OUT = { type: 'object', properties: { summary: { type: 'string' }, files_changed: { type: 'array', items: { type: 'string' } }, sounds: { type: 'array', items: { type: 'string' }, description: 'each new component: name, what it is, its controls' }, measurements: { type: 'string' }, checks: { type: 'string' }, caveats: { type: 'array', items: { type: 'string' } } }, required: ['summary', 'files_changed', 'sounds', 'measurements', 'checks', 'caveats'] }

phase('Plan')
const plan = await agent(`${RULES}\nYOUR TASK: survey std (every kernel in m2pkg.toml and kernels-src, every component, how poly voices, envelopes, filters and descriptors work, what the kernel ABI allows) and write ${W}/plan.md: the smallest elegant set of new kernels and components that gives a pop producer a pro palette: (1) a polysynth engine (e.g. unison/supersaw oscillator with detune and spread, sub, noise, a resonant ladder or improved SVF lowpass with key tracking and an envelope, amp envelope, glide, velocity), and the named instruments on it (lead, supersaw, keys, strings, pad, pluck2 or bass2 only if they add something); (2) FM keys/bells (2-4 operators, electric piano and glassy bell); (3) the drum kit (synthesized 808/909-style kick, snare, clap with its multi-burst, closed/open hats with choke from 6 metallic squares through bandpasses, toms, rim, a metallic perc), as one kit component mapping GM pitches plus each drum alone. For each: kernels needed (new Rust kernels and their ports/units), controls (few, musical, with ranges and defaults), voices, CPU budget, and how its quality will be measured. Keep existing components untouched. Return the plan.`, { label: 'plan', phase: 'Plan', schema: { type: 'object', properties: { file: { type: 'string' }, summary: { type: 'string' } }, required: ['file', 'summary'] } })

phase('Kernels')
const kernels = await agent(`${RULES}\n${TESTING}\nPLAN: ${plan ? plan.file : 'MISSING'} (read it)\nYOUR TASK: build the new kernels the plan needs in std/kernels-src and declare them in std/m2pkg.toml: band-limited (PolyBLEP/BLAMP or better) and alias-checked, denormal-safe, deterministic, allocation-free, with Rust unit tests (frequency accuracy, aliasing at high notes, DC, stability at extreme settings, determinism, reset). Rebuild std.wasm with std/build.sh. Keep every existing kernel's code and output unchanged (offline byte-identity). Report with measurements.`, { label: 'build:kernels', phase: 'Kernels', schema: OUT })
log(`kernels: ${kernels ? kernels.summary.slice(0, 300) : 'FAILED'}`)

phase('Instruments')
const inst = await agent(`${RULES}\n${TESTING}\nPLAN: ${plan ? plan.file : 'MISSING'}\nKERNELS: ${JSON.stringify(kernels)}\nYOUR TASK: build the components in std/src (new file(s), e.g. synth.ts and drums.ts, exported from std's entry the way the existing ones are) with full descriptors (labels, controls with units/ranges/defaults/smooth, settings, editor metadata so they appear right in the app), voiced with care: levels matched (each instrument's typical note around -12 dBFS peak), envelopes and filters tuned for pop, velocity response, glide where it matters. Add examples/sounds (a 16-bar pop demo using every new sound) and document the new sounds in README.md's sound list (plain words). Render and analyse each sound per TESTING; iterate on the voicing until the numbers are clean. Report with measurements.`, { label: 'build:instruments', phase: 'Instruments', schema: OUT })
log(`instruments: ${inst ? inst.summary.slice(0, 300) : 'FAILED'}`)

phase('Review')
const FINDINGS = { type: 'object', properties: { findings: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, sound: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, evidence: { type: 'string' }, fix: { type: 'string' } }, required: ['title', 'sound', 'severity', 'evidence', 'fix'] } } }, required: ['findings'] }
const reviews = await parallel([
  () => agent(`${RULES}\n${TESTING}\nThe new sounds are built: ${JSON.stringify(inst)}\nReview AS A SOUND DESIGNER: render each new sound yourself at low, mid and high notes and several velocities and measure (aliasing, DC, clicks at note on/off and voice steal, envelope shapes, filter self-oscillation and stability, unison phase, stereo/mono compatibility, loudness consistency across the set, tails, CPU per voice, determinism across two renders). Compare each against what the classic it imitates is known to do (808/909 drums, Juno/JP-8000 supersaw, DX7 EP and bell). Only real problems with numbers. Do not modify files.`, { label: 'review:sound', phase: 'Review', schema: FINDINGS }),
  () => agent(`${RULES}\nThe new sounds are built: ${JSON.stringify(inst)}\nReview AS A. G. COOK making a pop record with music2: read the components and their descriptors as they appear in the app (names, controls, ranges, defaults), the demo song examples/sounds, and the analysis in ${W}/. Is this the palette you need to start a track; what is missing that every producer reaches for first; are controls named and ranged musically; are defaults usable as is; is anything redundant or not simple? Only real problems. Do not modify files.`, { label: 'review:producer', phase: 'Review', schema: FINDINGS }),
])
const findings = reviews.filter(Boolean).flatMap(r => r.findings)
log(`${findings.length} findings`)

phase('Fix')
let fixed = null
if (findings.length) {
  fixed = await agent(`${RULES}\n${TESTING}\nReviewers (a sound designer and a producer) found:\n${JSON.stringify(findings, null, 1)}\nFor each: check it (render and measure); fix it if real; skip taste calls that conflict with simplicity, saying why. Then rerun everything in TESTING (offline byte-identity of the old fixtures included). Report honestly, each finding fixed / skipped (why).`, { label: 'fix', phase: 'Fix', schema: OUT })
}
return { plan: plan && plan.summary, kernels, inst, findings: findings.map(f => `${f.severity}: ${f.sound}: ${f.title}`), fixed }

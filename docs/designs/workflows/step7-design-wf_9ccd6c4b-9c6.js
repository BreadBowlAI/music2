export const meta = {
  name: 'step7-design',
  description: 'Design step 7 (the engine: Plan/DataSet split, SiteIds, the arena, persistent plugin instances) as simply as possible, attack it, revise it. Read-only on the working tree.',
  phases: [
    { title: 'Understand', detail: 'runtime/play internals, identity in ir/script/song, the contract rules for step 7' },
    { title: 'Design', detail: 'two independent designs: simplest-that-passes, and robust-first' },
    { title: 'Judge', detail: 'pick and merge into one design' },
    { title: 'Attack', detail: 'real-time/identity correctness; simplicity and scope' },
    { title: 'Revise', detail: 'final design-7.md with what gets deleted' },
  ],
}

const REPO = '<repo>'
const D = `${REPO}/.git/m2-step7`

const RULES = `
Repository: ${REPO} (music2: a DAW where TypeScript defines the music; Rust core core/{ir,script,runtime,song,session,play,plugins,kernel}, std/, crates/{server,gates,editor}, web/, apps/{music2-desktop,m2}).
THE CONTRACT ${REPO}/docs/architecture.md is normative; read its Order of work and Acceptance gates first. Steps 1-6 are BUILT (HEAD cbf5eb9). STEP 7 is "The engine" (4-6 weeks): Plan split from DataSet; SiteId matching, InstancePath and RuntimeId; the arena with prepare and activate, keeping instances whose RuntimeId and shape (wasm hash included) match; arrangement switches prepared off the audio thread; persistent CLAP instances behind PluginHost with LoadState, and offline renders that reuse them. Step 7's gate: in p50, a structural edit at 3:00 is audible within 200 ms after the compile, and the tails of untouched tracks are intact; live playback with no edits is bit-identical to offline; a kernel module rebuilt with the same kernel id but a new hash starts from init; an export of examples/with-plugins pauses playback, reuses its instances, and playback resumes with their state intact; the per-instance Store cost at 6,176 instances is measured and the store layout chosen from it. Earlier steps' designs are in ${REPO}/.git/m2-step6-resume/ (step 6) for style.
THE OWNER'S DIRECTION: a tasteful, professional DAW that is "simple yet elegant"; "don't be afraid to go back and change things up". So prefer the simplest design that meets the gates; where step 7's arena subsumes earlier machinery (e.g. step 3a's crossfaded activation of whole renderers, SwapLane special cases, step 3b's carried plugin instances matched by module/plugin id/ordinal/state hash, step 5's per-renderer cell tables), say what can be DELETED or unified, and propose contract edits (cite rule ids) where a rule makes things more complex than needed. Never weaken a guarantee a producer would notice (no clicks, no stuck notes, tails intact, export exact, no audio-thread allocation).
IMPORTANT: another workflow is editing the working tree right now (the data-plane fix: play-first lane previews, a faster Tx path, an FSEvents watcher, in core/session, crates/server, crates/gates, web). You are READ-ONLY on the working tree: do NOT edit, build into, or run cargo/tests in ${REPO} (no cargo commands at all, to keep the CPU free for their timing measurements). Read code with cat/grep/sed. Write only into ${D}/ (create it). Do not commit or touch git refs.
Write plainly and concretely: file paths, types, functions, data flow, the audio-thread handoff, what is tested how.
`
const NOTES = { type: 'object', properties: { file: { type: 'string', description: 'path of the notes you wrote in the design folder' }, summary: { type: 'string' }, key_facts: { type: 'array', items: { type: 'string' } } }, required: ['file', 'summary', 'key_facts'] }

phase('Understand')
const READERS = [
  { key: 'runtime', prompt: `Map today's engine: core/runtime and core/play (Renderer, Flat graph, the planner, kernel instances and their Store/linear memory, voices/poly, lanes and schedulers, event buffers, cells and CellMap, activation with the 5 ms crossfade, SwapLane, jumps, the two clocks, range render, the allocation trap), core/kernel's ABI, and how core/plugins' carried instances work. For each: what is rebuilt on a structural edit today and what it costs (allocation of stores, wasm instantiation, plugin creation), and what state is lost. Write ${D}/understand-runtime.md.` },
  { key: 'identity', prompt: `Map identity today: Origins on IR nodes, call-site instrumentation in core/script, key() frames, ComponentIds and (instance, slot) origins from step 5, Module { key, label }, how a node in one revision can be matched to a node in the next (what is stable across an insertion above it, a rename of a label, an edit inside a track), and the GraphDelta node names step 6 introduced (core/session/src/tx/delta.rs) which "an insertion can't shift". Identify what SiteId, InstancePath and RuntimeId should be built from with the least new machinery. Write ${D}/understand-identity.md.` },
  { key: 'contract', prompt: `Extract every contract rule and Today line that mentions step 7, the arena, Plan, DataSet, SiteId, InstancePath, RuntimeId, prepare/activate, LoadState, persistent plugin instances, arrangement switches, or says "until step 7" / "the arena"; list what earlier steps deferred to step 7; list the step 7 gate items and how each could be measured with crates/gates (read crates/gates/src). Also list rules whose complexity looks unnecessary for a simple, elegant engine, with the reason. Write ${D}/understand-contract.md.` },
]
const notes = await parallel(READERS.map(r => () => agent(`${RULES}\nYOUR TASK: ${r.prompt}`, { label: `read:${r.key}`, phase: 'Understand', schema: NOTES })))
const ctx = JSON.stringify(notes.filter(Boolean))
log(`understood: ${notes.filter(Boolean).length}/3`)

phase('Design')
const DES = { type: 'object', properties: { file: { type: 'string' }, summary: { type: 'string' }, deletes: { type: 'array', items: { type: 'string' } }, contract_edits: { type: 'array', items: { type: 'string' } }, risks: { type: 'array', items: { type: 'string' } } }, required: ['file', 'summary', 'deletes', 'contract_edits', 'risks'] }
const ANGLES = [
  { key: 'simple', prompt: 'SIMPLEST-THAT-PASSES: the smallest set of concepts that meets every step 7 gate; reuse existing identity (origins/delta names) rather than inventing parallel ids where possible; unify activation, lane swaps and plugin carrying into one arena mechanism if that is simpler; aggressively list what is deleted.' },
  { key: 'robust', prompt: 'ROBUST-FIRST: start from the failure cases a producer would hear (tails cut on untouched tracks, clicks, stuck notes, a plugin losing its state, an export that differs from live, a kernel with a new hash keeping stale memory, allocation on the audio thread, memory growth from retired instances) and design the arena so each is impossible by construction; then simplify.' },
]
const designs = await parallel(ANGLES.map(a => () => agent(`${RULES}\nUNDERSTANDING NOTES: ${ctx}\nRead the notes' files. YOUR TASK: write a complete step 7 design from this angle to ${D}/design-${a.key}.md. Angle: ${a.prompt}\nCover: the Plan vs DataSet split; identity (SiteId, InstancePath, RuntimeId or a simpler equivalent); the arena (what it owns, prepare off the audio thread, activate at a block boundary, matching and keeping instances whose identity and shape match, retiring the rest off-thread, crossfades only where needed); arrangement switches; persistent CLAP instances with LoadState and offline renders reusing them (export pauses playback, reuses instances, resumes with state intact); the Store layout decision and how it is measured at 6,176 instances; how each gate is measured; the build order in 2-4 slices each landing green; the tests; and what is deleted.`, { label: `design:${a.key}`, phase: 'Design', schema: DES })))

phase('Judge')
const judged = await agent(`${RULES}\nNOTES: ${ctx}\nTWO DESIGNS: ${JSON.stringify(designs.filter(Boolean))}\nRead both design files fully. YOUR TASK: judge them on (1) meets every step 7 gate, (2) simplicity (fewest concepts, most deleted), (3) a producer never hears a regression, (4) buildable in green slices. Pick the base and graft the best ideas from the other. Write the merged design to ${D}/design-7.md (complete, self-contained, concrete), with a section "What this deletes" and a section "Contract edits" (rule ids, old vs new wording in brief).`, { label: 'judge', phase: 'Judge', schema: DES })

phase('Attack')
const ATTACK = { type: 'object', properties: { problems: { type: 'array', items: { type: 'object', properties: { title: { type: 'string' }, severity: { type: 'string', enum: ['high', 'medium', 'low'] }, scenario: { type: 'string' }, fix: { type: 'string' } }, required: ['title', 'severity', 'scenario', 'fix'] } } }, required: ['problems'] }
const ATTACKERS = [
  { key: 'correctness', prompt: 'real-time and identity correctness: audio-thread allocation/locks/IO in prepare-activate-retire; instance matching that keeps the wrong state (a moved call, a duplicated track, two instances swapping identity, a kernel with a new wasm hash, a changed shape or channel count, voices count changes); tails of untouched tracks; plugin threading (HOST rules) and LoadState; export reusing instances while playback is paused and resuming exactly; bit-identity live vs offline; memory growth.' },
  { key: 'simplicity', prompt: 'simplicity and scope: concepts that are not needed for any gate, duplicated identity schemes, machinery kept that the arena makes redundant, slices that cannot land green, gate measurements that would not actually prove the claim, and contract edits that weaken a guarantee. Propose the simpler alternative for each.' },
]
const attacks = await parallel(ATTACKERS.map(a => () => agent(`${RULES}\nDESIGN: ${D}/design-7.md (read it fully, and the code it changes).\nAttack it through one lens. Report only real problems with a concrete scenario and a fix. Lens: ${a.prompt}`, { label: `attack:${a.key}`, phase: 'Attack', schema: ATTACK })))
const problems = attacks.filter(Boolean).flatMap(a => a.problems)
log(`${problems.length} problems raised`)

phase('Revise')
const final = await agent(`${RULES}\nDESIGN: ${D}/design-7.md\nPROBLEMS RAISED: ${JSON.stringify(problems)}\nYOUR TASK: revise ${D}/design-7.md in place: fold in each real problem (append a table "Attack response": problem, accept/reject, why, where it changed). Keep it as simple as possible. End with "Build slices" (each slice: what lands, its tests, how it stays green, which gate items it proves) and "Owner questions" (only real decisions). Return the summary.`, { label: 'revise', phase: 'Revise', schema: DES })
return { notes: notes.filter(Boolean).map(n => n.summary), designs: designs.filter(Boolean).map(d => d.summary), judged: judged && judged.summary, problems: problems.map(p => `${p.severity}: ${p.title}`), final }

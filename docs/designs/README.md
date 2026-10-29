# Design notes

**In short**
- Each folder here holds the working notes of one step or track: its plan, the reviews of the plan, and its build notes.
- They are history. What holds today is in the contract, [docs/architecture.md](../architecture.md). Where a note and the contract disagree, the contract wins.
- Notes written before 2026-10-25 say "the manual" (`docs/manual.md`): it is now the guide, [docs/guide/](../guide/welcome.md), one page a topic.
- The look of the app is [step8/taste.md](step8/taste.md), the taste guide.
- A big change starts with a new note here, before the code (see [CONTRIBUTING.md](../../CONTRIBUTING.md)).

## Folders

| Folder | What it holds |
|---|---|
| `step6/` | the write path: every edit as a checked Tx |
| `step7/` | the engine: plans, data, the arena, persistent plugin instances |
| `kernel-layout/` | the speed work on how kernels hold their memory |
| `platform/` | the platform layer (`m2-os`), Linux beside the Mac, and the full check |
| `sounds/` | the sounds merge: std's new sounds |
| `step8/` | rendering on several cores (8a), the producer features (8b), the taste guide, and the tracks after them |
| `step9/` | one plan and one set of build notes per track (`io`, `ab`, `ph`, `sx`, `pm`, `ex`, `br`, `bus`) |
| `agents/` | agents beside the musician: the design, and the build notes of its five tracks (`steps`, `fwd`, `see`, `routes`, `listen`) |
| `release/` | the first release, 0.1.0: the downloads for the Mac and Linux, and the workflow that makes them from a tag |
| `packs/` | the plan for packs: the library, the registry of packs anyone can publish, and ⌘F search (not built yet) |
| `docs/` | the docs plan: the guide in `docs/guide/`, `m2 docs`, the website, and the `AGENTS.md` every new song gets |
| `workflows/` | the scripts that ran the AI agents who wrote many of these notes. They are kept as a record; nothing runs them |

## Paths in these notes

The notes were written on the maintainer's machines. A path outside the repo is written with one of these names:

| Name | What it stands for |
|---|---|
| `<repo>` | the music2 checkout |
| `$SCRATCH` | the maintainer's scratch folder on the Linux server, outside the repo: worktrees (`$SCRATCH/wt/<name>`), each check's logs, pictures of the app, and helper scripts such as `tw` (runs a long job in tmux) |
| `<scratchpad>` | an agent session's temporary folder on the Mac |
| `/home/<user>`, `/Users/<user>` | the maintainer's home folder on Linux or on the Mac |
| the Linux server | the maintainer's build server (Ubuntu 24.04, AMD Ryzen 9 7950X3D) |

Files under `$SCRATCH` and `<scratchpad>` are not in the repo.

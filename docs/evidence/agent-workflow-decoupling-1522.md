# Developer workflow decoupling — Issue #1522

Source revision before: `fe018bb75217a7a6853f53d037a23aade47873a0` (`master` at pickup).
Candidate source revision: `0118c19f8279f12e596a70b2ab888b8e630bcdf1` (pre-CI fix; final head must be rechecked).

## Duplicate instruction map

| Duplicated/externally delegated area | Prior sources | Seyal-owned authority now |
| --- | --- | --- |
| General principles, plan and readiness | AGENTS.md, AGENT-TOOLING.md, AI-SDLC create-issue/WORKING-LOOP, 3 Seyal facades | Short AGENTS.md; SEYAL-AGENT-WORKFLOW.md; ISSUE-PROTOCOL.md; on-demand refinement/readiness skills |
| GitHub owner, worktree, candidate stages | AGENTS.md, DEVELOPMENT.md, implement-issue facade, generic implement-issue | ISSUE-PROTOCOL.md authoritative; implement-issue applies it; shared local workflow summarizes |
| Verification criteria and specialist gates | AGENTS.md, generic verification, Seyal verification/milestone skills | SEYAL-AGENT-WORKFLOW.md and on-demand verification, terminal/domain skills |
| Review/remediation convergence | generic pr-review/process-review/WORKING-LOOP, AGENTS.md, multiple facades | One local review process; PR review and remediation skill entrypoints |
| Bootstrap/project context | generic project_context.py, ignored framework clone/pin, tracked context-index/meta, test-framework-pin | Repo-native `project-context` targeted search; `bootstrap-dev.sh --check-skills` (no clone) |
| Terminal/Metal/Rust state/hot-path rules | AGENTS.md plus accepted architecture/quality docs | TERMINAL-AGENT-GATES.md on demand; accepted architecture/ADRs still outrank it |

No product code, runtime hot-path, security rule, core test, conformance/fuzz suite, or existing quality command is deliberately changed.

## Source byte measurements

Sizes are the Git object blob sizes at the recorded revisions, **not measured model tokens**. Token proxy is bytes / 4, rounded up, and is an approximation only. For a skill load, the listed bundle includes `AGENTS.md`, the relevant Seyal skill, and its delegated external skill (before) or shared local workflow document (after). Other authority documents are demand-loaded and excluded from both totals.

| Instruction path | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| Always-on `AGENTS.md` | 17,350 B | 3,492 B | 79.9% |
| Implement Issue path | 33,175 B | 19,045 B | 42.6% |
| PR Review path | 29,925 B | 16,953 B | 43.3% |
| External `docs/WORKING-LOOP.md` alone | 32,373 B | 0 B required | Dependency removed |

The added on-demand readiness completeness rule preserves the retired generic checklist's duplicate, negative-case, design-congruence, verification and security criteria without reloading them for unrelated tasks.

Recompute after every edit via `python3 scripts/measure-agent-instructions.py`. If source size changes, update this table to match the exact PR head.

## Execution overhead

**Observable static overhead removed:** the previous generic-skill preflight required an extra `bash scripts/bootstrap-dev.sh --check-framework` invocation per worktree/revision, a git repository/origin/clean/pin scan and file existence tests. A missing checkout caused `git clone` + `git fetch` and derived-index validation. The native skill path performs **zero** framework preflights and requires **zero** external framework network requests. `make bootstrap-agents` remains opt-in MCP setup.

**Not measured:** end-to-end agent startup/model context tokens, local git operations, framework download elapsed time, exact runtime timing, and actual `make check` duration. The current execution environment has no checked-out Seyal repository and cannot resolve github.com; do not infer timings from source size. CI evidence must be read from the PR's exact head rather than claiming any local PASS.

## Representative workflow evidence targets

- Ready unowned Issue: human owner claimed, rechecked and authorized; existing candidate reused.
- Already owned/conflicting Issue: refuse before branch/editor operations; never steal an assignee.
- Complex change lacking a production path: refine and plan in the Issue body; no code.
- Terminal lifecycle code: load specialist invariants and adversarial matrix; no second PTY/VT.
- Review first pass clean: final review; no ceremonial second pass.
- Review returns blockers: one consolidated same-PR root-cause batch, affected checks and whole-candidate exact-head/base signoff.
- Exact head moves, required CI fails, or review evidence missing: no READY_TO_MERGE.
- Even with a clean independent review: **no merge without explicit human authorization**.

These are policy/fixture expectations until executed agent trials or independently reviewed PR evidence confirm actual client behavior.

## Observed CI evidence on candidate `688e4776` (before readiness wording update)

- **PASS:** on PR CI, native skills bootstrap check and complete `scripts/test-tooling.sh` passed. Docs `npm run check` passed; native-macos-compile passed.
- **FAILED (pre-existing, unchanged source):** `make check` fails at duplicate Xcode object IDs `C100...18` at project.pbxproj lines 62/67 and `C200...18` at lines 151/170. Same signatures are in pre-PR master CI logs.
- **FAILED (pre-existing, unchanged source):** repository policy runs its direct structural-debt gate and fails `crates/seyal-client/src/ffi/app/decode.rs` at 893 LOC against acknowledged 889. Neither the production file nor its debt baseline changed in this PR.
- **NOT_RUN / pending:** physical/local agent trials, local `make docs-build`, complete `make check` past pre-existing blockers, independent human review. No weakening/waiver of existing gates is authorized.

## Required verification and blockers

Run `bash scripts/bootstrap-dev.sh --check-skills`, `bash scripts/test-tooling.sh`, `bash -n scripts/*.sh`, `make check`, `make docs-check`, `make docs-build`, and inspect the PR's CI jobs. Independent human review must be requested; neither agent-authored notes nor this artifact can satisfy that approval gate. Record PASS/FAIL/NOT_RUN for each at the exact PR head, including environmental limitations. Do not merge.

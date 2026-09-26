# Seyal Engineering Quality Baseline (M002+)

**Authority role:** thin index and M001 carry-forward rules.  
**Not** a second constitution. The Product & Engineering Constitution, accepted architecture/ADRs/specs, and the documents linked below remain higher authority. If this index and a linked authority disagree, the linked authority wins.

Use this baseline when starting M002+ work so agents and humans land in the correct existing rules instead of inventing parallel standards.

## Authority map

| Domain | Canonical authority |
|---|---|
| Constitution / agent invariants | `AGENTS.md` |
| Development workflow / unit of work | `docs/engineering/DEVELOPMENT.md` |
| Issue Ready/Done | `docs/engineering/ISSUE-PROTOCOL.md` |
| PR / CI / review | `docs/engineering/GITHUB-WORKFLOW.md` |
| Testing / TDD / fuzz harness | `docs/engineering/TESTING.md` |
| Performance / hot-path / budgets | `docs/engineering/PERFORMANCE.md` |
| Security / privacy | `docs/engineering/SECURITY.md` |
| Repository layout | `docs/engineering/REPOSITORY-STRUCTURE.md` |
| Agent tooling / skills | `docs/engineering/AGENT-TOOLING.md` |
| OSS ↛ commercial | `AGENTS.md` + commercial overlay (never invert) |

## Baseline rules (derive; do not reinterpret)

### Rust ownership and public API

- One authoritative state owner per concern; no GUI VT mirror or second engine.
- Public API surface grows only with a Ready Issue and accepted architecture; prefer fail-closed errors over silent defaults.
- Panics are not control flow on production paths; FFI boundaries must not unwind across languages.

### unsafe / FFI / native boundary

- Every `unsafe` block has a local safety comment naming the invariant.
- FFI ABI and panic boundaries are tested (`ffi_misuse` / equivalent). Do not invent parallel bridge crates.

### Concurrency / resource lifecycle

- Bounded queues; backpressure is visible; no unbounded wake loops after persistent failure.
- Detach ≠ terminate. Explicit terminate must reap and return resources to baseline.
- Late lifecycle fixes require first-principles re-review of the affected state matrix (`AGENTS.md` merge gates).

### Hot path / performance

- Register new terminal hot-path functions in `scripts/check-hot-path.py` in the same PR.
- No avoidable alloc/copy/lock/IPC/JSON on canonical progress paths (`PERFORMANCE.md`).
- CI `make bench` with `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` is class `CI` only — never headed presentation proof. Headed acceptance uses `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=1` on a controlled host.
- Machine budget constants in scripts are authority; prose must not invent softer gates.

### Swift / AppKit / Metal

- Production Metal path: prepare → encode → present/commit with in-flight atlas/resource gates.
- Do not CPU-write GPU-sampled textures while frames are in flight.
- Fonts and host-surface config come from configuration authority, not hardcoded system monospace when a family is configured.
- UI source changes follow the same-PR XCTest / XCUI policy.

### Testing / fuzz / benchmarks

- Core behavior is test/evidence-first; no temporary production VT/renderer/runtime to green a gate.
- Fuzz registry (`fuzz/targets.toml`) must match real campaign targets; ci-smoke alone is not milestone §6.9 grade.
- Benchmarks record environment; do not weaken workloads or thresholds to pass.

### Security / privacy / dependencies

- Same-user UDS trust, socket ownership, bounds/version validation remain mandatory.
- Dependency and public-API growth needs explicit review; no secrets in tree.
- OSS must not depend on commercial crates or SKU knowledge.

### CI / reproducibility / evidence classes

Label every perf/presentation/fuzz claim: `CI` | `controlled-host` | `PLATFORM_LIMITED`.  
`PLATFORM_LIMITED` is not automatic PASS (`M001-PASS10-VALIDATION.md` verdict model).

### ADR triggers / docs / independent review

- Implementation PRs do not create or amend ADRs.
- Status docs must match machine truth (gates, freeze SHAs, Issue state).
- Core/high-risk work requires independent review; implementers do not self-approve.

## M001 carry-forward lessons (proven)

1. **Authority ↔ machine honesty** — closeout text that asserts a gate the repository does not enforce is a BLOCKING defect (Pass 10 #784: false `CLIENT_RSS_KIB=768` vs machine `1536`).
2. **Evidence class honesty** — do not cite Foundation display-link-off benches as headed presentation proof; do not cite socket-loss soak as GUI-process death (#787 lineage).
3. **Freeze invalidation** — production commits after a freeze SHA invalidate affected criterion evidence; re-freeze and re-validate.
4. **Finding-set freeze** — after Pass review freeze, new Issues only for FAIL/INCONCLUSIVE mandatory criteria, amended frozen findings, or new BLOCKING defects — not Issue factories.
5. **Post-M001 cohesion/CI debt** — #764–#768 are implemented and closed via their owning PRs (CI pin, block naming, client/runtime/FFI ownership splits). Later-work #663 (250-pane Metal) remains outside M001 / post-multipane.
6. **Harness ≠ product** — UITest helper packaging / ad-hoc codesign for diagnostic benches must not be confused with distributable Release identity or production behavior changes.
7. **Candidate-B / non-production comparators** — retained corpora are not production coverage; registry grades must stay honest.
8. **Issue-body hygiene** — closed Issues must not still say ACTIVE/IN PROGRESS; reopened Issues must state residuals with evidence.

## How to use for M002+

1. Read `AGENTS.md` then the linked domain doc for the change class.
2. For M002 remaining work, obey `docs/milestones/MILESTONE-002.md`: only #824/#672, #673, #837, and freeze-SHA validation remain; do not attach new product slices. #673 does not wait on #824 closure.
3. For M003 workspace work, obey `docs/milestones/MILESTONE-003.md`: parallel with late M002 only at stable terminal seams; do not treat #674 as one Ready PR; do not mutate TerminalState/VT/Unicode/reflow.
4. Open/refine one Ready Issue; one worktree; one scoped PR.
5. Prefer extending these authorities over adding competing “baseline” docs.
6. When M001 evidence documents are historical, treat them as precedent for honesty rules — not as a license to skip M002 acceptance gates.

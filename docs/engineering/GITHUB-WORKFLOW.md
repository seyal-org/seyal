# GitHub workflow configuration

Repository documents define policy; GitHub's control plane should enforce it where the platform supports enforcement.

## Project status model

Seyal uses these workflow states conceptually:

```text
Backlog
Refinement
Ready
In Progress
In Review
Validation
Blocked
Done
```

Only work explicitly marked **Ready** is eligible for implementation pickup.

A GitHub Project with these states is the preferred long-term control plane. While the repository remains under a personal account or the Project is not configured, the explicit `## State` field in each Issue is the approved temporary source of workflow status. Keep it current; do not infer readiness from an open Issue alone.

Recommended Project views once enabled:

- Ready queue: `Status = Ready`, grouped by milestone/parent.
- Active: In Progress + In Review + Validation.
- Blocked: Status = Blocked with dependency fields visible.
- M001: filtered to the M001 milestone/parent hierarchy.

## Issue hierarchy and dependencies

Native sub-issues and native blocked-by/blocking relationships are preferred when available.

Until those controls are available, the approved temporary fallback is explicit parent/dependency text in the Issue body. The relationship must be unambiguous and kept current; do not create a second spreadsheet or planning database merely to emulate GitHub-native relationships.

This fallback is sufficient for current M001 work and must not block terminal implementation. Migrate to native relationships when the repository moves to an organization/control plane that supports them cleanly.

## Issue types

Preferred native types once the repository is organization-owned:

```text
Epic     # custom organization issue type
Feature  # native/default organization issue type
Task     # native/default organization issue type
Bug      # native/default organization issue type
```

GitHub manages native Issue Types at the organization level. While Seyal remains under a personal account, Issue Forms plus clear titles are the approved temporary fallback. Organization transfer is recommended before contributor/team scale, but lack of native Issue Types is not an M001 implementation blocker.

## Labels

Keep labels orthogonal to native type. Recommended small set:

### Area

`area:terminal`, `area:vt`, `area:exec`, `area:runtime`, `area:render`, `area:macos`, `area:blocks`, `area:protocol`, `area:workspace`, `area:agents`

### Special state/risk

`blocked`, `needs-spec`, `needs-adr`, `performance-sensitive`, `security-sensitive`, `breaking-change`

Use `type:architecture`, `type:performance`, `type:security`, or `type:spike` only where native types do not express the distinction. Avoid duplicating Feature/Bug/Task as labels after organization Issue Types are available.

## Branch / pull-request protection

The policy is fixed even when repository settings cannot be inspected or configured from an automation client:

- no production feature work is pushed directly to `master`;
- every implementation change uses branch → pull request → validation/review → merge;
- required CI must be green before merge;
- core/high-risk changes require independent review when an independent reviewer is available;
- stale approvals must not be treated as valid after a material head change.

A GitHub ruleset/branch-protection configuration should enforce these rules when available. Lack of platform-level enforcement is a governance-hardening item, not permission to bypass the policy and not a blocker for the current owner-controlled M001 work.

### Residual risk (AUD-P2-004) — closed

As of the foundation P2/P3 closure (#811), the live `master` ruleset **requires**
status checks `repository-policy`, `rust-and-harness-quality`, and
`native-macos-smoke` with `strict_required_status_checks_policy: true`. Pull
requests remain mandatory. Independent review stays a process gate while the
repository is solo-owned (`required_approving_review_count: 0`); requiring a
numeric review count without a second trusted reviewer identity is weak.
Foundation Quality therefore runs on `pull_request` only; a post-merge
`push` re-run would duplicate those required jobs without adding merge
protection. Renaming any required job must update this document and the
ruleset together.

**AUD-P2-003** (scroll path keep-as-is after measurement) remains closed: no
scroll algorithm rewrite; see `crates/seyal-terminal/benches/vt_scroll_state.rs`.

## Public OSS repository CI

The canonical public Seyal repository owns the authoritative GitHub Actions quality gates. The `Foundation Quality` workflow uses minimal `contents: read` permissions, pins external actions by reviewed full commit SHA, cancels superseded runs on the same ref, and keeps fast PR responsibilities explicit.

### Required Foundation Quality jobs (every PR)

These stable job names are the Pass-1 required checks on `master`. Foundation Quality runs on `pull_request` only; post-merge `push` to `master` is intentionally omitted so merge does not re-spend the full suite after the same jobs already gated the PR. A renamed/replaced job must update this document and the ruleset together; a missing check must never be interpreted as a pass.

- **`repository-policy`** (ubuntu) — shell syntax, governance structure, local documentation links, architecture layering, hot-path/benchmark/UI-test contracts, handwritten production structural-debt ratchet, harness contracts, fuzz-registry smoke, and controlled negative fixtures proving repository validators reject invalid inputs.
- **`rust-and-harness-quality`** (ubuntu) — pinned Rust bootstrap, production Rust workspace build, `make check` (format, Clippy with warnings denied, unit tests, layering and harness checks), and `make bench` as a **portable harness smoke** (macOS-only native benches are skipped; no performance claim).
- **`native-macos-smoke`** (macos-15 + Xcode 16.4) — pinned Rust plus native toolchain bootstrap; Rust workspace `make build` (no headed `Seyal.app` until #883); `make check`; `make test` (Rust unit/PTY; native XCTest/XCUIAutomation skipped while `macos/Seyal` is absent); and `make bench` with:
  - `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` — hosted runners may be headless and cannot deliver `CAMetalDisplayLink` callbacks; presentation-proxy samples are recorded as `PLATFORM_LIMITED` rather than failing the job;
  - `SEYAL_CODESIGN_IDENTITY=-` — unsigned CI artifact only, not a release/signing proof.

`SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` is an honesty contract, not a weakened threshold. Interactive/local acceptance and Pass 6/10 headed presentation evidence must run with `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=1` (or an equivalent headed host that produces presentation-proxy samples). **Green Foundation CI with display-link off is not Pass 6 presentation proof and must not be cited as such in Pass 10 evidence.**

### Path-filtered and non-Foundation workflows

These are **not** Foundation required checks. A green Foundation run can therefore succeed without them:

| Workflow / gate | Trigger | What it proves | What it does **not** prove |
|---|---|---|---|
| `Docs` (`.github/workflows/docs.yml`) | path-filtered to `site/**`, docs skills, and itself | Astro docs build with SHA-pinned actions and `npm ci` against `site/package-lock.json` | product/runtime correctness |
| `Pass 5 Production Fuzz` (`.github/workflows/pass5-fuzz.yml`) | path-filtered to runtime/exec/protocol/fuzz surfaces (plus `workflow_dispatch`) | short libFuzzer campaigns (~30s) against locked fuzz workspace deps | continuous / milestone-length fuzz campaigns; Foundation already green without this workflow |
| Fuzz registry smoke inside `repository-policy` | every Foundation run | registry/corpus/adapter smoke via `scripts/fuzz-smoke.py` | libFuzzer campaign coverage or “fuzz clean” Pass 10 evidence |

Pass 10 continuous fuzz expectations: registry smoke is continuous on Foundation; path-filtered libFuzzer campaigns are targeted PR evidence only. Milestone “fuzz clean” / long-running campaign evidence is **controlled-host or explicit campaign evidence**, never inferred from a green Foundation run alone. Fuzz workspace dependencies are pinned in `fuzz/Cargo.lock`; the path-filtered workflow verifies `--locked` metadata before building.

### Controlled-host-only gates (not CI proof)

Shared CI absolute latency/CPU/RSS and headless display-link-off benches are diagnostic at best. The following remain controlled-host (or otherwise non-CI) evidence by design:

- headed Pass 6 presentation-proxy / `CAMetalDisplayLink` budgets (`SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=1`);
- Pass 9 five-cohort production budget artifacts validated by `scripts/check-pass9-production-budget.py` (CI/`make check` only runs the validator `--self-test`);
- absolute performance, RSS, thread, GPU, and reconnect/cleanup sign-off tables used for Pass 10 criterion `PASS`;
- long-running or corpus-expanding fuzz campaigns beyond the short path-filtered PR jobs.

### Host/image nondeterminism (classified, not silently claimed fixed)

`native-macos-smoke` and `production-macos-state-fuzz` pin `macos-15` and select `/Applications/Xcode_16.4.app` (#764) so Metal/terminfo smoke is less sensitive to silent GitHub image drift. That pin reduces host nondeterminism; it still does **not** make shared CI a substitute for controlled same-host Pass 10 measurements, and it must not be cited as bit-reproducible Metal/presentation evidence or absolute latency/RSS proof. Keep `SEYAL_REQUIRE_DISPLAY_LINK_BENCHMARK=0` on hosted runners; headed Pass 6/10 presentation evidence remains controlled-host only.

### Action pinning and docs supply chain

External GitHub Actions must be pinned by reviewed full commit SHA (with a human-readable version comment). Floating tags such as `@v4` are forbidden in repository workflows. The Docs workflow pins `actions/checkout` and `actions/setup-node` the same way Foundation does, and installs docs dependencies with `npm ci` against the committed `site/package-lock.json`.

Repository-owned validators are self-tested through safe temporary negative fixtures. The negative suite proves governance, local-link, architecture-layering, workspace and harness validators fail for controlled violations and for the expected reason. Rust compiler/formatter/Clippy and Xcode/native build failures are enforced by their own non-zero tool exits rather than fake production code.

Deeper scheduled/release or targeted gates are added only as their real production surfaces exist:

- retained VT conformance corpus;
- active fuzz campaigns and sanitizers beyond registry smoke / short path-filtered PR campaigns;
- deeper renderer/native validation;
- broad PTY/runtime failure matrix;
- performance/RSS/thread/GPU regression suites on controlled hosts;
- dependency/security scanning.

Pass 1 does not claim these deferred gates are active merely because their harness locations exist. Expensive/noisy checks should be targeted rather than making every PR unusable. Green Foundation CI is incomplete Pass 10 proof; see `docs/engineering/M001-PASS10-VALIDATION.md` for CI vs controlled-host vs `PLATFORM_LIMITED` provenance rules.


## Private `seyal-commercial` CI policy

`seyal-commercial` is a private superproject that consumes a pinned Seyal OSS revision at `oss/seyal`.

For now, do **not** add GitHub-hosted Actions workflows to the private repository because of private-repository CI cost. This is a temporary execution-cost decision, not permission to lower engineering standards.

Commercial PRs must still record the canonical local build/test/check/integration evidence relevant to the change. When commercial code becomes substantial, introduce private CI using self-hosted runners or paid hosted capacity rather than relying indefinitely on manual validation.

The public OSS workflow must never be moved into the private repository or made dependent on the private repository.

## Repository ownership note

The OSS repository is currently public under a personal GitHub account. Moving Seyal to an organization remains recommended before external contributor/team scale so native Issue Types, Projects, team reviewers, rulesets and future enterprise administration can be configured cleanly. That migration is governance hardening and does not block M001 under the temporary fallbacks above.

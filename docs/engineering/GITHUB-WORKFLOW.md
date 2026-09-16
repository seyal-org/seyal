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
- Milestone views filtered to the applicable milestone/parent hierarchy.

## Issue hierarchy and dependencies

Native sub-issues and native blocked-by/blocking relationships are preferred when available.

Until those controls are available, the approved temporary fallback is explicit parent/dependency text in the Issue body. The relationship must be unambiguous and kept current; do not create a second spreadsheet or planning database merely to emulate GitHub-native relationships.

## Issue types

Preferred native types once the repository is organization-owned:

```text
Epic
Feature
Task
Bug
```

While Seyal remains under a personal account, Issue Forms plus clear titles are the approved temporary fallback. Organization transfer is recommended before contributor/team scale, but lack of native Issue Types is not an implementation blocker.

## Labels

Keep labels orthogonal to native type.

Area labels include `area:terminal`, `area:vt`, `area:exec`, `area:runtime`, `area:render`, `area:macos`, `area:blocks`, `area:protocol`, `area:workspace`, and `area:agents`.

Special state/risk labels include `blocked`, `needs-spec`, `needs-adr`, `performance-sensitive`, `security-sensitive`, and `breaking-change`.

Use `type:architecture`, `type:performance`, `type:security`, or `type:spike` only where native types do not express the distinction.

## Branch / pull-request protection

The policy is fixed even when repository settings cannot be inspected or configured from an automation client:

- no production feature work is pushed directly to `master`;
- every implementation change uses branch → pull request → validation/review → merge;
- required CI must be green before merge;
- core/high-risk changes require independent review when an independent reviewer is available;
- stale approvals must not be treated as valid after a material head change.

A GitHub ruleset/branch-protection configuration should enforce these rules when available. Lack of platform-level enforcement is not permission to bypass policy.

### Required ruleset status

The live `master` ruleset requires status checks `repository-policy`, `rust-and-harness-quality`, and `native-macos-smoke` with strict required status checks. Pull requests remain mandatory. Independent review stays a process gate while the repository is solo-owned; requiring a numeric review count without a second trusted reviewer identity would be weak.

Renaming any required job must update this document and the ruleset together.

## Public OSS repository CI

The canonical public Seyal repository owns the authoritative GitHub Actions quality gates. The `Foundation Quality` workflow uses minimal permissions, pins external actions by reviewed full commit SHA, cancels superseded runs on the same ref, and keeps fast PR responsibilities explicit.

### Required Foundation Quality jobs

- **`repository-policy`** (ubuntu) — shell syntax, governance structure, local documentation links, architecture layering, hot-path/benchmark/UI-test contracts, harness contracts, fuzz-registry smoke, and controlled negative fixtures proving repository validators reject invalid inputs.
- **`rust-and-harness-quality`** (ubuntu) — pinned Rust bootstrap, production Rust workspace build, `make check`, and `make bench` as a portable harness smoke. macOS-only native benches are skipped and no performance claim is made.
- **`native-macos-smoke`** (macOS) — pinned Rust plus native toolchain bootstrap; Rust/native build/test/check surfaces and the hosted-runner benchmark mode defined by repository scripts.

Hosted-runner display-link-off evidence is not headed presentation proof. Interactive/local acceptance and milestone headed presentation evidence require a controlled headed host according to the owning milestone/specification.

### Path-filtered and non-Foundation workflows

These are not Foundation required checks, so a green Foundation run may succeed without them:

| Workflow / gate | Trigger | What it proves | What it does **not** prove |
|---|---|---|---|
| `Docs` | path-filtered to site/docs tooling | static documentation validation | product/runtime correctness |
| production fuzz workflow | path-filtered to affected runtime/exec/protocol/fuzz surfaces | short libFuzzer campaigns | milestone-length continuous fuzz evidence |
| fuzz registry smoke inside `repository-policy` | every Foundation run | registry/corpus/adapter smoke | full fuzz campaign coverage |

Long-running fuzz, headed presentation, absolute latency/CPU/RSS, GPU and reconnect/cleanup sign-off remain controlled-host or explicit campaign evidence where the owning milestone requires them.

### Host/image nondeterminism

Hosted macOS CI pins the repository-selected runner/toolchain configuration to reduce host nondeterminism. This still does not make shared CI a substitute for controlled same-host performance/presentation measurements, and it must not be cited as bit-reproducible Metal/presentation evidence or absolute latency/RSS proof.

### Action pinning and docs supply chain

External GitHub Actions must be pinned by reviewed full commit SHA with a human-readable version comment. Floating tags are forbidden in repository workflows. Documentation dependencies are installed against the committed lockfile.

Repository-owned validators are self-tested through safe temporary negative fixtures. Compiler/formatter/Clippy and native build failures are enforced by their own non-zero tool exits rather than fake production code.

Deeper scheduled/release or targeted gates are added only as their real production surfaces exist, including retained VT conformance, deeper fuzz/sanitizer campaigns, renderer/native validation, broad PTY/runtime failure matrices, controlled-host resource regression suites, and dependency/security scanning.

## Repository isolation

This public repository owns its own CI and quality authority. External/private consumers may validate their own composition separately, but this repository must not depend on or disclose their repositories, infrastructure, CI configuration, access model, or implementation details.

## Repository ownership note

The OSS repository is currently public under a personal GitHub account. Moving Seyal to an organization remains recommended before external contributor/team scale so native Issue Types, Projects, team reviewers and rulesets can be configured cleanly. That migration is governance hardening and does not block current milestone work under the temporary fallbacks above.

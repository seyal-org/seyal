# Seyal development workflow

## Authority chain

```text
Product & Engineering Constitution
→ accepted architecture
→ ADRs/rationale
→ specifications
→ milestone definition
→ Ready GitHub Issue
→ pull request
→ implementation
```

Issues and PRs cannot override higher authority. If implementation evidence contradicts an accepted architectural decision: stop implementation, record evidence, run architecture review/ADR, update affected specification and Issue, then resume.

For M002+ engineering expectations (ownership, unsafe/FFI, concurrency, hot-path, Metal, testing/fuzz, security, CI evidence classes, OSS↛commercial), start from the thin index `docs/engineering/ENGINEERING-QUALITY-BASELINE.md`. It points at existing authorities and records M001 carry-forward honesty rules; it does not replace this workflow or the Constitution.

The `.sdlc` context layer is deliberately **not** inserted into the authority chain. It is a compact navigation/provenance layer that helps agents find the relevant authoritative artifacts without rereading the repository.

## Unit of work

Default distributed-development unit:

```text
one Ready Issue
→ one authenticated **human** GitHub owner (assignee when assignable; acknowledged Owner claim otherwise)
→ one confirmed implementation plan
→ one deterministic <human-login>/issue/<number> branch
→ coding agent may act only as delegated tool/co-author
→ one isolated worktree
→ one scoped PR owned by the human
```

One Issue should produce one coherent outcome that can normally be tested, reviewed and merged independently. Large or cross-authority work is refined before implementation. Two active Issues must not mutate the same authoritative subsystem unless independence is explicit and reviewable.

The Issue has exactly one **human owner**. The sole assignee is the preferred owner record when GitHub permits it; otherwise an external contributor uses a maintainer-acknowledged `Owner: @login` Issue claim. This unique owner record is what prevents two people from owning the same implementation Issue at once. The exact `<human-login>/issue/<number>` branch is that owner's deterministic audit/resume backstop and makes ownership visible in Git history. New implementation branches do not use agent/vendor prefixes or short-name suffixes. Coding agents may contribute under the human-owned branch and be credited as co-authors/tooling provenance. Legacy plain `issue/<number>`, issue-only/slugged, or agent-named branches require explicit human-owner disposition before they continue.

## Mandatory flow

1. When project context beyond the Issue links is needed, use `project-context` to retrieve the smallest relevant node/relationship set, validate the derived index, and read the returned authoritative sources. A stale/no-match index routes to targeted source search; it never authorizes guessing.
2. Refine the Issue using `.agents/skills/issue-refinement/SKILL.md`.
3. Set Project status to **Ready** only after the readiness checklist in `ISSUE-PROTOCOL.md` passes.
4. Any request to implement/fix/finish/code a specific GitHub Issue must enter `.agents/skills/implement-issue/SKILL.md`. Resolve the authenticated **human GitHub owner** and fresh-read assignee/owner-claim state before planning or production work. Prefer sole assignment when GitHub permits it; otherwise require a maintainer-acknowledged external-contributor owner claim. A Cursor/Codex/Claude/Copilot/bot identity is never the work owner. Assigned-to-other, multiple-assignee, bot-owned, or identity-unavailable cases stop as `BLOCKED`.
5. Confirm the implementation plan in chat. Claim/Ready state is not permission to skip plan-first review.
6. After plan confirmation, create the exact branch `<human-login>/issue/<number>` from current accepted `master`, where the login is the sole human owner. The branch may live in the upstream repository or the contributor's fork; ownership identity is still the human login. If that branch already exists, stop unless the human owner explicitly requested resume/continue of that existing work. Re-read the Issue after branch creation and require the same human to remain the unique owner through sole assignment or the acknowledged external-owner claim before creating the worktree or editing production files.
7. Create one isolated worktree from the deterministic Issue branch.
8. Use tests/fixtures first for core behavior.
9. Implement only the Issue scope.
10. Assess **Documentation impact** before final validation. Run the `docs-authoring` skill and update the User Guide and/or Developer Guide in the same Issue/PR when applicable. If no documentation is needed, record a concrete `N/A` rationale in the PR.
11. Run `make check` plus issue-specific tests/benchmarks/security checks. When documentation changed, also run `make docs-check` and `make docs-build`.
12. Open a PR using the repository template, including documentation evidence or the `N/A` rationale.
13. Require CI evidence; high-risk/core work gets independent review.
14. Move to Validation where milestone/demo/performance evidence is required.
15. Merge only after required gates pass. Do not start a dependent milestone early.

Ownership handoff is explicit and human-to-human. The current owner stops editing and records branch/PR/check state. Transfer the **human owner record** explicitly: change the GitHub assignee when the recipient is assignable, otherwise replace the maintainer-acknowledged `Owner: @login` claim. The new human owner re-runs the full claim/readiness preflight; if the deterministic branch must move to the new owner's namespace, migrate the exact head and record both refs before deleting/retiring the old one. Switching coding agents alone never changes ownership. An agent must never self-clear or steal a claim because it appears stale.

## Human owner, agent assistance and attribution

Seyal accepts AI-assisted development, but repository ownership remains human.

- The Issue owner record (sole assignee when assignable, otherwise acknowledged `Owner: @login`) and branch namespace identify the responsible human GitHub contributor.
- New implementation branches are `<human-login>/issue/<number>`; `cursor/`, `codex/`, `claude/`, `copilot/` and other agent/vendor namespaces are forbidden for new work.
- Agents may implement, test, draft documentation, and assist reviews on behalf of the human owner.
- Agent contribution may be acknowledged in the PR body and/or with a real standard `Co-authored-by:` trailer. Do not fabricate attribution identities.
- Bot-authored reviews/comments are supplemental evidence only. Required independent review must be owned by a human GitHub reviewer and must not be represented as Cursor/Codex/etc. ownership.

## Documentation lifecycle

Documentation is part of feature completeness, not a default follow-up task.

Use `.agents/skills/docs-authoring/SKILL.md` whenever implementation adds or changes:

- user-visible behavior, commands, configuration, workflows, troubleshooting or interaction patterns;
- contributor setup, build/test workflow, architecture orientation, public extension points or engineering procedures;
- screenshots, diagrams or documentation media.

Choose the audience deliberately:

- **User Guide** for observable product behavior and tasks;
- **Developer Guide** for contributor orientation and development workflows;
- authoritative ADR/spec/architecture/engineering records remain under the repository `docs/` authority paths and must not be duplicated into the site as competing truth.

A change with no documentation impact must say why in the PR. Do not satisfy the gate by documenting planned behavior as shipped. Documentation should normally land with the implementation that makes it true so code and docs cannot drift immediately after merge.

## Scope discipline

Do not perform unrelated cleanup. If an out-of-scope problem is discovered, create/link another Issue and continue unless it blocks the current Issue. Do not turn implementation into architecture by precedent.

## Architecture changes

Use `docs/engineering/ISSUE-PROTOCOL.md` and the `architecture-change` skill. Creating, amending, reopening, or superseding an ADR must be a separate PR from implementation. Mixed ADR+implementation PRs are rejected; land and accept the ADR first, then implement against the accepted authority.

## Development prerequisites

The canonical repository bootstrap does not silently install host package managers or execute downloaded shell scripts.

Required before `make bootstrap`:

- Git;
- `make`;
- `rustup`, installed explicitly from the official Rust project;
- network access to the official Rust distribution when the pinned toolchain is not already installed.

On macOS, M001 now requires **full Xcode**, selected with `xcode-select`, because the permanent native app surface exists. `make bootstrap` validates `xcodebuild`, the macOS SDK, Swift compiler and Metal shader toolchain through `xcrun`; Command Line Tools alone are no longer sufficient for the canonical macOS build.

The repository pins Rust in `rust-toolchain.toml`. M001 currently uses Rust **1.98.0** with the `minimal` rustup profile plus `rustfmt` and `clippy`. Cargo is supplied by that same pinned Rust toolchain.

`make bootstrap` is idempotent where rustup permits: it validates host prerequisites, installs/verifies exactly the repository-pinned Rust toolchain/components through rustup, initializes repository-declared pinned submodules if any, and validates the result. It does not run `curl | sh`, invoke Homebrew, install optional MCP/agent tooling, or write credentials.

Optional developer-agent/MCP provisioning is deliberately separate:

```sh
make bootstrap-agents
```

That explicit opt-in command uses `scripts/bootstrap-dev.sh`, materializes the exact reviewed AI-SDLC developer-framework pin under ignored `.sdlc/framework/`, and may provision the other pinned developer tools documented in `docs/engineering/AGENT-TOOLING.md`. It is not part of product build/test/CI bootstrap and must not become a terminal/runtime dependency.

## Canonical task interface

The stable product human/agent/CI entry points are:

```sh
make bootstrap
make build
make test
make check
make bench
```

Documentation tooling is an opt-in development surface and remains outside the product runtime/build hot path:

```sh
make docs          # install docs dependencies and start the local documentation server
make docs-install  # install documentation dependencies only
make docs-build    # build the static documentation site
make docs-check    # run Starlight/Astro documentation validation
```

`make docs` requires Node.js 22.12 or later. Do not create competing undocumented command paths.

Current behavior after Passes 1–10 (M001 **Done / closed**; Pass 10 #727 and parent #5 closed on freeze `c536c54`):

- `make bootstrap` provisions/verifies the pinned Rust toolchain and, on macOS, validates full Xcode + Swift + macOS SDK + Metal tooling when that host tree exists;
- `make build` builds the Rust workspace and, on macOS, the thin `Seyal.app` host over Rust snapshots (`#883` one-pane slice);
- `make test` validates repository/tooling/workspace and harness invariants, validates the M001 fuzz registry/corpora, runs Rust workspace unit/integration tests, and on macOS runs native XCTest/XCUI (`make ui-test`);
- `make check` runs the deterministic repository checks, harness/fuzz validation, controlled negative fixtures proving custom validators actually reject bad inputs, Rust formatting/Clippy/tests, architecture layering, the handwritten production structural-debt ratchet (`scripts/check-structural-debt.py` + `docs/engineering/structural-debt-baseline.toml`), and on macOS requires the thin `Seyal.app` Metal/hot-path files. Cargo live Runtime fixtures and headed XCTest/XCUI (`make ui-test` / Foundation Quality `native-macos-smoke`) use an explicit isolated `--runtime-dir` namespace (socket directory and singleton lock) so they can run while a user-scoped `seyal-runtime` is already active. `make check` does not launch Seyal.app. Production discovery still uses `$(getconf DARWIN_USER_TEMP_DIR)/seyal-runtime/control.sock` and does not honor environment variables to relocate that endpoint;
- `make bench` records and round-trips benchmark environment metadata under `target/benchmarks/` and runs the real Cargo benchmark targets that exist for M001 passes;
- `make docs` starts the local Starlight documentation site after installing its isolated Node dependencies;
- `make docs-build` and `make docs-check` validate documentation without becoming dependencies of terminal production execution.

The public `Foundation Quality` workflow separates the fast PR gates into `repository-policy`, `rust-and-harness-quality`, and `native-macos-smoke` (Rust workspace build, `make check`, `make test`, and `make bench`; native `Seyal.app` / XCTest / XCUIAutomation run only when `macos/Seyal` exists). See `docs/engineering/GITHUB-WORKFLOW.md` for the exact responsibility, required-check contract, path-filtered Docs/fuzz workflows, and controlled-host-only gates. Linux remains a supported portable-core CI host; native AppKit/Metal build/test steps explicitly skip there instead of introducing a cross-platform GUI abstraction.

Canonical Cargo operations use the pinned toolchain and `--locked` where dependency resolution applies.

The physical Rust workspace is the Passes 1–10 / M001 production surface documented in `docs/engineering/REPOSITORY-STRUCTURE.md`. Crates exist only for justified ownership boundaries; do not pre-create empty diagram-driven packages.

The native host under `macos/Seyal` is a thin AppKit/Metal adapter over Rust `seyal_app_*` snapshots and Candidate-D `seyal_bridge_*` frames. It does not own Workspace/Tab/Pane/composer/chrome product state.

Harness locations under `tests/`, `fuzz/` and `benches/` hold real M001 fixtures, fuzz adapters and pass benchmarks. Pass 10 evidence/protocol docs live under `docs/engineering/M001-PASS10-EVIDENCE.md` and `docs/evidence/`; #727 and #5 are closed on the M001 freeze.

Issue #12 made the Pass-1 CI gates production-shaped: external workflow actions are pinned by reviewed commit SHA, workflow permissions remain minimal, repository validators are negative-fixture tested, and architecture layering is enforced in the public PR path. Later passes extended those gates without replacing the canonical root `make` interface.

## Clean-checkout workflow

From a new clone with the prerequisites above:

```sh
git clone https://github.com/mahboobmonnamd/seyal.git
cd seyal
make bootstrap
make build
make test
make check
make bench
```

For coding-agent/project-context tooling, explicitly opt in:

```sh
make bootstrap-agents
python3 .sdlc/framework/tools/project_context.py --root . validate
```

To preview the documentation locally (Node.js 22.12+):

```sh
make docs
```

On macOS, after `make build`, the one-pane host can be launched with:

```sh
open target/macos-derived-data/Build/Products/Debug/Seyal.app
```

### Diagnosing an apparently inert Return key

If the composer accepts text but Return, Command-C, or Command-V appears to do
nothing, do not assume that AppKit failed to deliver the key. Inspect the
terminal surface accessibility value first. A usable production path reports
non-`none` `runtime`, `execution`, and `attachment` identities together with
`connection=usable`. If it instead reports
`connection=disconnected runtime=none execution=none attachment=none`, input is
intentionally fenced because no Runtime-owned execution is attached. A focused
composer or a passing text-view unit test does not prove that end-to-end path.

One reproducible development-only trigger is terminating `seyal-runtime` while
its canonical control socket remains present. Connection then fails with
`ECONNREFUSED`: current reconnect authority treats that differently from an
absent endpoint, and only the Runtime may validate and remove its stale socket.
Do not make the GUI unlink the socket or broaden Runtime launch policy inside an
unrelated UI issue; that changes the accepted reconnect/process-lifecycle
contract and requires architecture/specification review first.

For user-visible keyboard regressions, retain a packaged-app
XCTest/XCUIAutomation case that starts the exact Runtime helper, asserts
`connection=usable`, sends a physical Return with no modifiers, observes the
command through the Runtime-owned PTY, and repeats the submission to exercise
Block reconciliation. In Flow mode also assert full-width, aligned Blocks and
`flow-paint=ok`; the right-edge black strip is a separate Block/Metal clipping
failure, not evidence that Return itself was dropped.

There are no required private repositories, `seyal-commercial` dependencies, shell-profile assumptions, Homebrew assumptions or hidden environment variables for this canonical product flow. AI-SDLC is an optional public developer-framework dependency materialized only by `make bootstrap-agents`.

## Generated and fixture data

Generated files must be clearly marked and reproducible. Fixtures live outside production code and record provenance where external/reference semantics matter. Benchmarks must record environment metadata and be reproducible locally and in CI where practical.

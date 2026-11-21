#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() {
  printf '[seyal tooling test] FAIL: %s\n' "$*" >&2
  exit 1
}

[[ -f rust-toolchain.toml ]] || fail "rust-toolchain.toml is missing"
grep -Eq 'channel[[:space:]]*=[[:space:]]*"1\.98\.0"' rust-toolchain.toml || fail "Rust channel is not pinned to 1.98.0"
grep -Eq 'components[[:space:]]*=[[:space:]]*\[[^]]*"rustfmt"' rust-toolchain.toml || fail "rustfmt is not pinned"
grep -Eq 'components[[:space:]]*=[[:space:]]*\[[^]]*"clippy"' rust-toolchain.toml || fail "clippy is not pinned"

for target in bootstrap bootstrap-agents build test check bench; do
  make -n "$target" >/dev/null || fail "canonical make target '${target}' does not resolve"
done

[[ -f scripts/bootstrap-dev.sh ]] || fail "agent bootstrap script is missing"
[[ -f docs/engineering/AGENT-TOOLING.md ]] || fail "agent tooling policy is missing"
grep -q 'XCODEBUILD_MCP_VERSION=' scripts/bootstrap-dev.sh || fail "XcodeBuildMCP is not pinned"
# Native bootstrapping must not download or preflight any external SDLC repository.
if grep -Eq 'AI_SDLC_(REPO|COMMIT|SKILLS)|ensure_ai_sdlc|framework_checkout|check_framework|framework-only' scripts/bootstrap-dev.sh; then
  fail "bootstrap still depends on the retired external framework"
fi
bash scripts/bootstrap-dev.sh --check-skills || fail "native Seyal skill discovery failed"
bash scripts/bootstrap-dev.sh --help | grep -Fq -- '--check-skills' || fail "native bootstrap usage missing"
if bash scripts/bootstrap-dev.sh --check-framework >/dev/null 2>&1; then
  fail "retired framework-check mode is still accepted"
fi
if bash scripts/bootstrap-dev.sh --framework-only >/dev/null 2>&1; then
  fail "retired framework-refresh mode is still accepted"
fi
grep -q 'github-mcp-server' scripts/bootstrap-dev.sh || fail "GitHub MCP bootstrap is missing"
grep -q 'mcpbridge' scripts/bootstrap-dev.sh || fail "official Xcode MCP bootstrap is missing"
grep -q 'xcodebuildmcp@${XCODEBUILD_MCP_VERSION}' scripts/bootstrap-dev.sh || fail "XcodeBuildMCP configuration is missing"
grep -q '^configure_copilot()' scripts/bootstrap-dev.sh || fail "GitHub Copilot MCP setup is missing"
grep -q 'configure_mcp_client copilot "GitHub Copilot CLI" builtin' scripts/bootstrap-dev.sh || fail "Copilot must use built-in GitHub MCP mode"
grep -q '^configure_cursor()' scripts/bootstrap-dev.sh || fail "Cursor MCP setup is missing"
grep -q 'SEYAL_CURSOR_MCP_CONFIG' scripts/bootstrap-dev.sh || fail "Cursor MCP config path is missing"
grep -q 'servers\["xcode"\]' scripts/bootstrap-dev.sh || fail "Cursor Xcode MCP setup is missing"
grep -q 'servers\["xcodebuild"\]' scripts/bootstrap-dev.sh || fail "Cursor XcodeBuildMCP setup is missing"
grep -q 'if has claude || has codex || has cursor; then' scripts/bootstrap-dev.sh || fail "external GitHub MCP should only be provisioned for clients that need it"

# Canonical local skills must be directly usable without a framework checkout.
for skill in project-context issue-refinement implementation-planning development-readiness implement-issue pr-review address-pr-review verification milestone-validation; do
  [[ -f ".agents/skills/$skill/SKILL.md" ]] || fail "missing Seyal-native $skill"
  [[ -f ".claude/skills/$skill/SKILL.md" ]] || fail "missing Claude discovery adapter $skill"
done
[[ ! -e .agents/skills/code-review/SKILL.md && ! -e .claude/skills/code-review/SKILL.md ]] || fail "duplicate code-review discovery must stay retired"
[[ -f docs/engineering/SEYAL-AGENT-WORKFLOW.md ]] || fail "native process is missing"
[[ -f docs/engineering/TERMINAL-AGENT-GATES.md ]] || fail "terminal/architecture specialist rules are missing"
# Preserve the retired local agent-claim removal and never touch private old state.
for retired in scripts/seyal-ai-sdlc-claim.py scripts/test-seyal-ai-sdlc-claim.py .cursor/hooks/claim-gate.py scripts/test-cursor-claim-hook.py; do
  [[ ! -e "$retired" ]] || fail "retired local claim component returned: $retired"
done
if [[ -f .cursor/hooks.json ]] && grep -Eq 'claim-gate\\.py|seyal-ai-sdlc-claim\\.py' .cursor/hooks.json; then
  fail "retired local agent claim hook remains configured"
fi
# Active instruction/skill/bootstrap docs must not delegate to remote framework paths.
active_workflow=(AGENTS.md docs/engineering/AGENT-TOOLING.md docs/engineering/SEYAL-AGENT-WORKFLOW.md docs/engineering/DEVELOPMENT.md docs/engineering/ISSUE-PROTOCOL.md)
for skill in .agents/skills/*/SKILL.md .claude/skills/*/SKILL.md; do active_workflow+=("$skill"); done
if grep -nE '\.sdlc/framework/|bootstrap-dev.sh --check-framework|bootstrap-dev.sh --framework-only|pinned AI-SDLC|canonical AI-SDLC' "${active_workflow[@]}"; then
  fail "active developer instructions still delegate to an external framework"
fi
[[ ! -e .sdlc/context/_meta.yaml && ! -e .sdlc/graph/context-index.json ]] || fail "stale framework-specific index/metadata tracked"
[[ ! -e scripts/test-framework-pin.sh ]] || fail "retired framework-pin regression still present"
grep -Fq 'plan before complex implementation' docs/engineering/SEYAL-AGENT-WORKFLOW.md || fail "plan gate missing"
for required in 'negative cases' 'duplicate-search' 'self-contained Issue body' 'security/secrets/privacy/failure' 'closed design/trust-boundary decisions'; do
  grep -Fq "$required" .agents/skills/development-readiness/SKILL.md || fail "readiness evidence requirement missing: $required"
done
grep -Fq 'one consolidated remediation batch' docs/engineering/SEYAL-AGENT-WORKFLOW.md || fail "consolidated remediation missing"
grep -Fq 'independent' docs/engineering/SEYAL-AGENT-WORKFLOW.md || fail "independent review missing"
grep -Fq 'No automatic merge' docs/engineering/SEYAL-AGENT-WORKFLOW.md || fail "explicit merge authorization missing"
grep -Fq 'full candidate' .agents/skills/pr-review/SKILL.md || fail "review must inspect whole PR"
grep -Fq 'exact-head' .agents/skills/verification/SKILL.md || fail "verification must be exact-head"
grep -Fq 'ISSUE-PROTOCOL.md' .agents/skills/implement-issue/SKILL.md || fail "human ownership authority missing"
python3 scripts/measure-agent-instructions.py --assert-lean || fail "always-loaded instruction budget exceeded"

# Parallel-development safety: implementation pickup must be an exclusive,
# verified GitHub owner before any production branch/worktree/edit path.
claim_skill=.agents/skills/implement-issue/SKILL.md
grep -Fq 'mandatory entrypoint for production implementation of a Seyal GitHub Issue' "$claim_skill" || fail "implement-issue must be the mandatory production entrypoint"
grep -Fq 'responsible **human GitHub owner**' "$claim_skill" || fail "implement-issue must resolve a human GitHub owner"
grep -Fq 'Issue #N is already taken by @login' "$claim_skill" || fail "implement-issue must report the existing assignee and stop"
grep -Fq 'multiple assignees or competing active owner claims' "$claim_skill" || fail "implement-issue must fail closed on conflicting human owner records"
grep -Fq 'exact branch name `<human-login>/issue/<number>`' "$claim_skill" || fail "implement-issue must use the human-owned deterministic issue branch"
grep -Fq 'unique owner through either sole assignment' "$claim_skill" || fail "implement-issue must keep external-owner fallback valid after branch creation"
grep -Fq 'unique human owner record coordinates contributors' "$claim_skill" || fail "implement-issue must make the human owner record authoritative across contributors"
grep -Fq 'Do **not** treat branch creation as the mechanism that prevents two humans from claiming the same Issue' "$claim_skill" || fail "implement-issue must scope branch collision to the same human namespace"
grep -Fq 'unique human owner-record check' "$claim_skill" || fail "implement-issue worktree gate must accept assignee or acknowledged external-owner records"
grep -Fq 'complete Issue owner-record state' "$claim_skill" || fail "implement-issue pickup must read the full owner record, not assignee state alone"
grep -Fq 'not sufficient for owner-record state' "$claim_skill" || fail "implement-issue fresh-read rule must cover the full owner record"
grep -Fq 'clear or transfer the human owner record' "$claim_skill" || fail "implement-issue abandonment must support both assignee and external-owner cleanup"
grep -Fq 'Report the current human owner record and branch state' "$claim_skill" || fail "implement-issue stale-claim reporting must not be assignee-only"
grep -Fq 'never overwrite another valid claim to win a race' "$claim_skill" || fail "implement-issue must not steal a concurrent claim"
grep -Fq 'add the human as assignee without replacing others' "$claim_skill" || fail "implementor pickup must add rather than replace assignees"
grep -Fq 'Recheck owner/candidate before push and publication' "$claim_skill" || fail "implementor must verify ownership before publication"
grep -Fq 'including self-authored comments not yet acknowledged by a maintainer' docs/engineering/ISSUE-PROTOCOL.md || fail "external claim collisions must include pending claims"
grep -Fq 'If the work item is a GitHub sub-issue, fetch its parent immediately' "$claim_skill" || fail "implement-issue must inspect the parent claim before a child slice"
grep -Fq 'planning/umbrella parent is not an ownership lock' "$claim_skill" || fail "implement-issue must allow independent child ownership under planning parents"
grep -Fqi 're-fetch both parent and child' "$claim_skill" || fail "implement-issue must re-fetch parent and child after claim and branch creation"
grep -Fq 'claim and branch that sub-issue only after the parent/slice overlap check' "$claim_skill" || fail "implement-issue must check parent/child slice overlap before claiming child work"
grep -Fq 'do not duplicate ownership of the same implementation slice' "$claim_skill" || fail "implement-issue must prevent duplicate slice ownership"
refine_skill=.agents/skills/issue-refinement/SKILL.md
grep -Fq 'recommend GitHub sub-issues (one per slice)' "$refine_skill" || fail "issue-refinement must recommend one GitHub sub-issue per slice"
grep -Fq 'parent and child must not represent the same implementation slice concurrently' "$refine_skill" || fail "issue-refinement must prevent duplicate parent/child implementation ownership"
grep -Fq 'Any request to **implement, fix, finish, code, or complete a specific GitHub Issue** must enter through' AGENTS.md || fail "AGENTS.md must route implementation requests through implement-issue"
grep -Fq 'one deterministic <human-login>/issue/<number> branch' docs/engineering/DEVELOPMENT.md || fail "development workflow must use the human-owned deterministic issue branch"
if grep -Eq '→ (issue/<number>-<short-name>|cursor/|codex/|claude/|copilot/)' docs/engineering/DEVELOPMENT.md; then
  fail "new development workflow must not use legacy or agent-owned branch conventions"
fi
grep -Fq 'Issue has exactly one **human owner**' docs/engineering/DEVELOPMENT.md || fail "development workflow must make human ownership authoritative"
grep -Fq 'unique owner record coordinates contributors' docs/engineering/DEVELOPMENT.md || fail "development workflow must make owner records authoritative across contributors"
grep -Fq 'Transfer the **human owner record** explicitly' docs/engineering/DEVELOPMENT.md || fail "development workflow must support assignee and external-owner handoff"
grep -Fq 'On an unassigned `help wanted` or `needs-refinement` issue, post a self-authored `Owner: @login` comment and re-fetch before continuing.' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must support non-assignable external human contributors"
grep -Fq 'durable ownership identity is always a human GitHub account' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must require human ownership"
grep -Fq 'Coding-agent/bot identities (Cursor, Codex, Claude Code, Copilot, or similar) are tools, not Seyal work owners.' AGENTS.md || fail "AGENTS.md must reject agent ownership"
grep -Fq 'New implementation branches are named `<human-login>/issue/<number>`' AGENTS.md || fail "AGENTS.md must human-namespace branches"
grep -Fq 'Agent assistance may be credited' AGENTS.md || fail "AGENTS.md must allow agent co-authorship/provenance"
grep -Fq 'A bot-authored review/comment is supplemental analysis only.' .agents/skills/implement-issue/SKILL.md || fail "implement-issue must not count bot review as human independent review"
grep -Fq 'Project status (`Ready`, `In Progress`, and so on) is lifecycle metadata, not an ownership lock' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must not use Project status as the ownership lock"
grep -Fq 'Status never overrides the **human-owner rule**' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must preserve the external-owner fallback"
grep -Fq 'single human owner record coordinates contributors' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must make the owner record authoritative across contributors"
grep -Fq 'branch is only that owner' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must define human-namespaced branches as audit/resume backstops"
grep -Fq 'The Issue body is the implementation plan' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must treat the Issue body as the plan"
grep -Fq 'A separate plan comment is not required.' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must not require a separate plan comment"
grep -Fq 'candidate_lifecycle_stage' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must define candidate lifecycle stage"
grep -Fq 'CLOSED_UNMERGED' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must keep a closed unmerged candidate"
grep -Fq 'A chat outline is not an implementation plan.' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must reject a chat-only plan"
grep -Fq 'The Issue body is the plan' .agents/skills/implement-issue/SKILL.md || fail "implement-issue must use the Issue body as the plan"
grep -Fq 'CLOSED_UNMERGED' .agents/skills/implement-issue/SKILL.md || fail "implement-issue must resume a closed unmerged candidate"
grep -Fq 'Refs` and `Part of` do not make a pull request the candidate.' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must not treat Refs or Part of as the implementation candidate"
grep -Fq 'Do not open a pull request while accepted scope is incomplete.' .agents/skills/implement-issue/SKILL.md || fail "implement-issue must not open a PR for unfinished scope"
grep -Fq 'Do not open a pull request while accepted scope is incomplete.' docs/engineering/ISSUE-PROTOCOL.md || fail "Issue protocol must not require a PR for unfinished scope"
grep -Fq 'A separate plan comment is not required.' .agents/skills/development-readiness/SKILL.md || fail "development-readiness must not require a separate plan comment"
grep -Fq 'A separate plan comment is not required.' .agents/skills/issue-refinement/SKILL.md || fail "issue-refinement must not require a separate plan comment"
grep -Fq 'Do not post a separate plan comment.' .agents/skills/implementation-planning/SKILL.md || fail "implementation-planning must not post a separate plan comment"
grep -Fq 'ready-for-review PR is `IN_REVIEW`' .agents/skills/address-pr-review/SKILL.md || fail "address-pr-review must map IN_REVIEW to ready-for-review"
grep -Fq 'CLOSED_UNMERGED' .agents/skills/address-pr-review/SKILL.md || fail "address-pr-review must resume a closed unmerged candidate"
if grep -Fq 'Confirm the implementation plan in chat.' docs/engineering/DEVELOPMENT.md \
  || grep -Fq 'Confirm the implementation plan in chat.' AGENTS.md \
  || grep -Fq 'Confirm the implementation plan in chat.' docs/engineering/ISSUE-PROTOCOL.md; then
  fail "chat plan confirmation must not replace the Issue body"
fi
if grep -Fq '<!-- seyal-plan-acceptance -->' docs/engineering/ISSUE-PROTOCOL.md \
  || grep -Fq '<!-- seyal-plan-acceptance -->' .agents/skills/implement-issue/SKILL.md \
  || grep -Fq '<!-- seyal-plan-acceptance -->' .agents/skills/development-readiness/SKILL.md; then
  fail "a seyal-plan-acceptance comment must not be an implementation gate"
fi
if grep -Fq 'accepted-plan comment' docs/engineering/DEVELOPMENT.md \
  || grep -Fq 'acceptance comment' docs/engineering/DEVELOPMENT.md \
  || grep -Fq 'accepted-plan comment' docs/engineering/AGENT-TOOLING.md \
  || grep -Fq 'The accepted-plan comment and the draft/ready candidate stage' docs/engineering/AGENT-TOOLING.md; then
  fail "DEVELOPMENT.md/AGENT-TOOLING.md must not require a removed accepted-plan comment gate"
fi
grep -Fq '→ the Issue body is the plan' site/src/content/docs/developer/index.mdx || fail "Developer Guide must use the Issue body as the plan"
if grep -Fq '→ confirmed plan' site/src/content/docs/developer/index.mdx; then
  fail "Developer Guide still says the plan is confirmed in chat"
fi
grep -Fq 'Exactly one human owns an implementation Issue at a time' site/src/content/docs/developer/index.mdx || fail "Developer Guide must document single-human ownership"
grep -Fq 'branch is only an audit/resume backstop for that human' site/src/content/docs/developer/index.mdx || fail "Developer Guide must not make the branch the ownership authority"

# Regression guards for the pre-human-owner wording that caused contradictory
# external-contributor and collision semantics.
for stale in   'Status never overrides the assignee rule'   'Branch creation is the collision backstop'   'both the assignee claim and deterministic branch checks pass'   'may never substitute for the human assignee'   'GitHub assignment is explicitly transferred'   'assignee state is the human-visible claim'   'deterministic implementation branch is the collision backstop'
do
  if grep -Fq "$stale" docs/engineering/ISSUE-PROTOCOL.md     || grep -Fq "$stale" "$claim_skill"     || grep -Fq "$stale" docs/engineering/DEVELOPMENT.md     || grep -Fq "$stale" site/src/content/docs/developer/index.mdx; then
    fail "stale assignee-era ownership wording remains: $stale"
  fi
done

# AGENTS must preserve the same dual human-owner record terminology.
if grep -Fq 'must not replace the human assignee' AGENTS.md; then
  fail "AGENTS.md drops the acknowledged external-owner path"
fi
grep -Fq 'must not replace the human owner record' AGENTS.md || fail "AGENTS.md must preserve the human owner-record authority"

# Targeted context search uses authoritative source, without stale generated graph.
[[ -f .agents/skills/project-context/SKILL.md ]] || fail "native context discovery missing"
grep -Fq 'rg' .agents/skills/project-context/SKILL.md || fail "native project-context must use targeted source discovery"

# Copilot project skills are loaded natively from .agents/skills; do not create a
# second Copilot-specific project skill tree that can diverge from canonical skills.
if [[ -d .copilot/skills || -d .github/skills ]]; then
  fail "duplicate Copilot project skill adapter tree detected"
fi

tooling_scope=(scripts/bootstrap-dev.sh docs/engineering/AGENT-TOOLING.md)
for forbidden in \
  'frontend-design' \
  'anthropics/skills' \
  'playwright' \
  'AppleDeepDocs' \
  'appledeepdoc' \
  'apple-deep-docs' \
  'SEYAL_ENABLE_APPLE_DEEP_DOCS'; do
  if grep -Fqi "$forbidden" "${tooling_scope[@]}"; then
    fail "non-project tooling returned to Seyal bootstrap/policy: ${forbidden}"
  fi
done

missing="$(mktemp)"
trap 'rm -f "$missing"' EXIT
if SEYAL_RUSTUP="${ROOT}/.definitely-missing-rustup" bash scripts/check-toolchain.sh >"$missing" 2>&1; then
  fail "missing rustup condition unexpectedly succeeded"
fi
grep -q 'rustup is required' "$missing" || fail "missing rustup failure is not actionable"

printf '[seyal tooling test] deterministic task/toolchain metadata tests passed.\n'

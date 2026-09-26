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
grep -q 'AI_SDLC_REPO=' scripts/bootstrap-dev.sh || fail "AI-SDLC repository is not declared"
grep -Eq 'AI_SDLC_COMMIT="[0-9a-f]{40}"' scripts/bootstrap-dev.sh || fail "AI-SDLC must be pinned by full commit SHA"
grep -q 'AI_SDLC_COMMIT="8d329477e41f00e82435fe47d49cfedd724aefc5"' scripts/bootstrap-dev.sh || fail "AI-SDLC pin must include merged working-loop revision"
grep -q '^AI_SDLC_SKILLS=(' scripts/bootstrap-dev.sh || fail "AI-SDLC skill manifest is missing"
grep -q '^ensure_ai_sdlc()' scripts/bootstrap-dev.sh || fail "AI-SDLC materialization is missing"
for generic_skill in project-context development-readiness work-item-design implementation code-review verification pr-review; do
  grep -q "  ${generic_skill}$" scripts/bootstrap-dev.sh || fail "AI-SDLC generic skill is not pinned: ${generic_skill}"
done
grep -q 'tools/project_context.py' scripts/bootstrap-dev.sh || fail "AI-SDLC project-context tool verification is missing"
grep -q 'project_context.py.*--root.*validate' scripts/bootstrap-dev.sh || fail "agent bootstrap must validate the derived context index"
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

for adapter in project-context development-readiness verification code-review; do
  [[ -f ".agents/skills/${adapter}/SKILL.md" ]] || fail "Seyal ${adapter} adapter is missing"
  [[ -f ".claude/skills/${adapter}/SKILL.md" ]] || fail "Claude ${adapter} adapter is missing"
done

[[ -f .agents/skills/pr-review/SKILL.md ]] || fail "Seyal pr-review facade is missing"
[[ -f .claude/skills/pr-review/SKILL.md ]] || fail "Claude pr-review adapter is missing"

grep -q '.sdlc/framework/skills/work-item-design/SKILL.md' .agents/skills/issue-refinement/SKILL.md || fail "issue-refinement must delegate to AI-SDLC work-item-design"
grep -q '.sdlc/framework/skills/implementation/SKILL.md' .agents/skills/implement-issue/SKILL.md || fail "implement-issue must delegate to AI-SDLC implementation"
grep -q '.sdlc/framework/skills/code-review/SKILL.md' .agents/skills/code-review/SKILL.md || fail "code-review must delegate to AI-SDLC code-review"
grep -q '.sdlc/framework/skills/pr-review/SKILL.md' .agents/skills/pr-review/SKILL.md || fail "pr-review must delegate to AI-SDLC pr-review"
if grep -q '.sdlc/framework/skills/code-review/SKILL.md' .agents/skills/pr-review/SKILL.md; then
  fail "pr-review must not regress to the focused AI-SDLC code-review authority"
fi
grep -q '.sdlc/framework/skills/verification/SKILL.md' .agents/skills/milestone-validation/SKILL.md || fail "milestone-validation must build on AI-SDLC verification"
grep -q '.sdlc/framework/skills/development-readiness/SKILL.md' .agents/skills/development-readiness/SKILL.md || fail "development-readiness adapter must delegate to AI-SDLC"
grep -q '.sdlc/framework/skills/verification/SKILL.md' .agents/skills/verification/SKILL.md || fail "verification adapter must delegate to AI-SDLC"

# Instruction layering for claim/ownership:
# - AGENTS routes implement → implement-issue and claim/closure → ISSUE-PROTOCOL
# - ISSUE-PROTOCOL owns complete owner-record semantics once
# - implement-issue keeps executable claim/re-fetch/parent-overlap/branch preflight
# - DEVELOPMENT + Developer Guide summarize and link; no sentence-level clones
claim_skill=.agents/skills/implement-issue/SKILL.md
issue_protocol=docs/engineering/ISSUE-PROTOCOL.md
dev_workflow=docs/engineering/DEVELOPMENT.md
dev_site=site/src/content/docs/developer/index.mdx

# AGENTS: mandatory implement route + claim/closure authority pointer
grep -Fq 'Any request to **implement, fix, finish, code, or complete a specific GitHub Issue** must enter through' AGENTS.md || fail "AGENTS.md must route implementation requests through implement-issue"
grep -Fq '.agents/skills/implement-issue/SKILL.md' AGENTS.md || fail "AGENTS.md must name the implement-issue skill path"
grep -Fq 'docs/engineering/ISSUE-PROTOCOL.md' AGENTS.md || fail "AGENTS.md must route claim/closure detail to ISSUE-PROTOCOL"
grep -Fq 'Coding-agent/bot identities (Cursor, Codex, Claude Code, Copilot, or similar) are tools, not Seyal work owners.' AGENTS.md || fail "AGENTS.md must reject agent ownership"
grep -Fq 'New implementation branches are named `<human-login>/issue/<number>`' AGENTS.md || fail "AGENTS.md must human-namespace branches"
grep -Fq 'Agent assistance may be credited' AGENTS.md || fail "AGENTS.md must allow agent co-authorship/provenance"
grep -Fq 'must not replace the human owner record' AGENTS.md || fail "AGENTS.md must preserve the human owner-record authority"
if grep -Fq 'must not replace the human assignee' AGENTS.md; then
  fail "AGENTS.md drops the acknowledged external-owner path"
fi

# ISSUE-PROTOCOL: complete owner-record semantics (single authority)
grep -Fq 'record `Owner: @login` in an Issue comment and require a maintainer acknowledgement' "$issue_protocol" || fail "Issue protocol must support non-assignable external human contributors"
grep -Fq 'durable ownership identity is always a human GitHub account' "$issue_protocol" || fail "Issue protocol must require human ownership"
grep -Fq 'Project status (`Ready`, `In Progress`, and so on) is lifecycle metadata, not an ownership lock' "$issue_protocol" || fail "Issue protocol must not use Project status as the ownership lock"
grep -Fq 'Status never overrides the **human-owner rule**' "$issue_protocol" || fail "Issue protocol must preserve the external-owner fallback"
grep -Fq 'single human owner record prevents two people from owning the same implementation Issue at once' "$issue_protocol" || fail "Issue protocol must make the owner record authoritative across contributors"
grep -Fq 'branch is only that owner' "$issue_protocol" || fail "Issue protocol must define human-namespaced branches as audit/resume backstops"
grep -Fq 'Closes #N' "$issue_protocol" || fail "Issue protocol must define Closes vs Refs closure honesty"
grep -Fq 'Refs #N' "$issue_protocol" || fail "Issue protocol must define non-closing Refs relationships"

# implement-issue: executable claim / re-fetch / parent-overlap / branch preflight
grep -Fq 'mandatory entrypoint for production implementation of a Seyal GitHub Issue' "$claim_skill" || fail "implement-issue must be the mandatory production entrypoint"
grep -Fq 'responsible **human GitHub owner**' "$claim_skill" || fail "implement-issue must resolve a human GitHub owner"
grep -Fq 'Issue #N is already taken by @login' "$claim_skill" || fail "implement-issue must report the existing owner and stop"
grep -Fq 'multiple assignees, multiple acknowledged owner claims' "$claim_skill" || fail "implement-issue must fail closed on conflicting human owner records"
grep -Fq 'exact branch name `<human-login>/issue/<number>`' "$claim_skill" || fail "implement-issue must use the human-owned deterministic issue branch"
grep -Fq 'unique owner through either sole assignment' "$claim_skill" || fail "implement-issue must keep external-owner fallback valid after branch creation"
grep -Fq 'unique human owner record prevents two people from owning the same implementation Issue at once' "$claim_skill" || fail "implement-issue must make the human owner record authoritative across contributors"
grep -Fq 'Do **not** treat branch creation as the mechanism that prevents two humans from claiming the same Issue' "$claim_skill" || fail "implement-issue must scope branch collision to the same human namespace"
grep -Fq 'unique human owner-record check' "$claim_skill" || fail "implement-issue worktree gate must accept assignee or acknowledged external-owner records"
grep -Fq 'complete Issue owner-record state' "$claim_skill" || fail "implement-issue pickup must read the full owner record, not assignee state alone"
grep -Fq 'not sufficient for owner-record state' "$claim_skill" || fail "implement-issue fresh-read rule must cover the full owner record"
grep -Fq 'clear or transfer the human owner record' "$claim_skill" || fail "implement-issue abandonment must support both assignee and external-owner cleanup"
grep -Fq 'Report the current human owner record and branch state' "$claim_skill" || fail "implement-issue stale-claim reporting must not be assignee-only"
grep -Fq 'never overwrite another valid claim to win a race' "$claim_skill" || fail "implement-issue must not steal a concurrent claim"
grep -Fq 'If the work item is a GitHub sub-issue, fetch its parent immediately' "$claim_skill" || fail "implement-issue must inspect the parent claim before a child slice"
grep -Fq 'planning/umbrella parent is not an ownership lock' "$claim_skill" || fail "implement-issue must allow independent child ownership under planning parents"
grep -Fq 're-fetch both parent and child' "$claim_skill" || fail "implement-issue must re-fetch parent and child after claim and branch creation"
grep -Fq 'claim and branch that sub-issue only after the parent/slice overlap check' "$claim_skill" || fail "implement-issue must check parent/child slice overlap before claiming child work"
grep -Fq 'do not duplicate ownership of the same implementation slice' "$claim_skill" || fail "implement-issue must prevent duplicate slice ownership"
grep -Fq 'A bot-authored review/comment is supplemental analysis only.' "$claim_skill" || fail "implement-issue must not count bot review as human independent review"
grep -Fq 'docs/engineering/ISSUE-PROTOCOL.md' "$claim_skill" || fail "implement-issue must point to ISSUE-PROTOCOL for claim/closure policy detail"

refine_skill=.agents/skills/issue-refinement/SKILL.md
grep -Fq 'recommend GitHub sub-issues (one per slice)' "$refine_skill" || fail "issue-refinement must recommend one GitHub sub-issue per slice"
grep -Fq 'parent and child must not represent the same implementation slice concurrently' "$refine_skill" || fail "issue-refinement must prevent duplicate parent/child implementation ownership"

# DEVELOPMENT + site: short workflow summary + authority link (not essay clones)
grep -Fq 'docs/engineering/ISSUE-PROTOCOL.md' "$dev_workflow" || fail "DEVELOPMENT.md must link claim/ownership detail to ISSUE-PROTOCOL"
grep -Fq '<human-login>/issue/<number>' "$dev_workflow" || fail "development workflow must use the human-owned deterministic issue branch"
grep -Fq 'human owner' "$dev_workflow" || fail "development workflow must require a human owner"
if grep -Eq '→ (issue/<number>-<short-name>|cursor/|codex/|claude/|copilot/)' "$dev_workflow"; then
  fail "new development workflow must not use legacy or agent-owned branch conventions"
fi
grep -Fq 'docs/engineering/ISSUE-PROTOCOL.md' "$dev_site" || fail "Developer Guide must link claim/ownership detail to ISSUE-PROTOCOL"
grep -Fq '<human-login>/issue/<number>' "$dev_site" || fail "Developer Guide must document human-namespaced branches"
grep -Fq 'human' "$dev_site" || fail "Developer Guide must document human ownership"

# Reject stale assignee-only / agent-owner / legacy branch-as-lock wording across routed surfaces
routed_surfaces=(AGENTS.md "$issue_protocol" "$claim_skill" "$dev_workflow" "$dev_site")
for stale in \
  'Status never overrides the assignee rule' \
  'Branch creation is the collision backstop' \
  'both the assignee claim and deterministic branch checks pass' \
  'may never substitute for the human assignee' \
  'GitHub assignment is explicitly transferred' \
  'assignee state is the human-visible claim' \
  'deterministic implementation branch is the collision backstop' \
  'agent owns the Issue' \
  'bot is the work owner'
do
  for surface in "${routed_surfaces[@]}"; do
    if grep -Fq "$stale" "$surface"; then
      fail "stale assignee-era or agent-owner wording remains in ${surface}: $stale"
    fi
  done
done

[[ -f .sdlc/context/_meta.yaml ]] || fail "Seyal SDLC context metadata is missing"
[[ -f .sdlc/graph/context-index.json ]] || fail "Seyal derived context index is missing"
python3 -m json.tool .sdlc/graph/context-index.json >/dev/null || fail "Seyal context index is not valid JSON"
python3 <<'PY' || fail "Seyal context index source fingerprints are stale"
import hashlib
import json
from pathlib import Path

root = Path('.')
with (root / '.sdlc/graph/context-index.json').open(encoding='utf-8') as handle:
    index = json.load(handle)

errors = []
for node in index.get('nodes', []):
    node_id = node.get('id', '<unknown>')
    for source in node.get('sources', []):
        rel = source.get('path')
        fingerprint = source.get('fingerprint')
        if isinstance(fingerprint, dict):
            expected = fingerprint.get('value')
        else:
            expected = fingerprint
        if not isinstance(rel, str) or not isinstance(expected, str):
            errors.append(f'{node_id}: malformed source fingerprint')
            continue
        path = root / rel
        if not path.is_file():
            errors.append(f'{node_id}: missing source {rel}')
            continue
        data = path.read_bytes()
        header = f'blob {len(data)}\0'.encode('utf-8')
        actual = hashlib.sha1(header + data).hexdigest()
        if actual != expected:
            errors.append(
                f'{node_id}: stale source {rel}: index={expected} current={actual}'
            )

if errors:
    for error in errors:
        print(f'[seyal tooling test] {error}')
    raise SystemExit(1)
PY
[[ ! -e scripts/project_context.py ]] || fail "generic project-context implementation must not be duplicated in Seyal"
grep -q '^/.sdlc/framework/' .gitignore || fail "materialized AI-SDLC framework must remain untracked"

ai_sdlc_commit="$(sed -n 's/^AI_SDLC_COMMIT="\([0-9a-f]\{40\}\)"$/\1/p' scripts/bootstrap-dev.sh)"
[[ -n "$ai_sdlc_commit" ]] || fail "could not read AI-SDLC pin"
grep -q "pinned_revision: \"${ai_sdlc_commit}\"" .sdlc/context/_meta.yaml || fail "SDLC metadata pin does not match bootstrap pin"
python3 - "$ai_sdlc_commit" <<'PY' || fail "context index pin does not match bootstrap pin"
import json
import sys

with open('.sdlc/graph/context-index.json', encoding='utf-8') as handle:
    value = json.load(handle)
if value.get('framework', {}).get('pinned_revision') != sys.argv[1]:
    raise SystemExit(1)
PY

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

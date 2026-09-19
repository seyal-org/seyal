#!/usr/bin/env bash
set -euo pipefail

SCRIPT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROOT="${SEYAL_VALIDATION_ROOT:-$SCRIPT_ROOT}"
cd "$ROOT"

required=(
  AGENTS.md
  CLAUDE.md
  .github/copilot-instructions.md
  docs/specs/README.md
  docs/engineering/DEVELOPMENT.md
  docs/engineering/ISSUE-PROTOCOL.md
  docs/engineering/TESTING.md
  docs/engineering/PERFORMANCE.md
  docs/engineering/SECURITY.md
  docs/engineering/REPOSITORY-STRUCTURE.md
  docs/engineering/OSS-COMMERCIAL-BOUNDARY.md
  docs/engineering/OSS-REPOSITORY-ISOLATION.md
  docs/engineering/COMPATIBILITY.md
  docs/engineering/RELEASES.md
  docs/engineering/M001-DISTRIBUTION.md
  docs/engineering/GITHUB-WORKFLOW.md
  docs/architecture/ADR-003-OSS-COMMERCIAL-REPOSITORY-BOUNDARY.md
  GOVERNANCE.md
  SUPPORT.md
  .github/pull_request_template.md
)

skills=(issue-refinement implement-issue architecture-change vt-tdd performance-gate security-review pr-review milestone-validation)

fail=0
for path in "${required[@]}"; do
  [[ -f "$path" ]] || { echo "missing required file: $path" >&2; fail=1; }
done
for skill in "${skills[@]}"; do
  [[ -f ".agents/skills/$skill/SKILL.md" ]] || { echo "missing canonical skill: $skill" >&2; fail=1; }
  [[ -f ".claude/skills/$skill/SKILL.md" ]] || { echo "missing Claude skill adapter: $skill" >&2; fail=1; }
done

production_paths=()
[[ -d crates ]] && production_paths+=(crates)
[[ -d macos ]] && production_paths+=(macos)

# OSS production code must not know about proprietary SKUs or entitlement state.
# Public extension seams, when required by a milestone, must be generic capabilities
# usable by any OSS consumer rather than hidden commercial hooks.
if [[ ${#production_paths[@]} -gt 0 ]] && grep -R -nE \
  'enterprise_license|pro_license|teams_license|commercial_license|commercial_entitlement|seyal_commercial' \
  "${production_paths[@]}" 2>/dev/null; then
  echo "forbidden commercial SKU/license coupling found in OSS production code" >&2
  fail=1
fi

if [[ -f AGENTS.md ]] && [[ $(wc -l < AGENTS.md) -gt 160 ]]; then
  echo "AGENTS.md is becoming an encyclopedia; keep it as a concise map" >&2
  fail=1
fi

if [[ -f CLAUDE.md ]] && ! grep -q '@AGENTS.md' CLAUDE.md; then
  echo "CLAUDE.md must import canonical AGENTS.md" >&2
  fail=1
fi

if [[ -f .github/copilot-instructions.md ]] && ! grep -q 'AGENTS.md' .github/copilot-instructions.md; then
  echo "Copilot adapter must point to AGENTS.md" >&2
  fail=1
fi

if [[ $fail -ne 0 ]]; then
  exit 1
fi

echo "Seyal governance validation passed."

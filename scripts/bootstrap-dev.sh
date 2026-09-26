#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${HOME}/.local/bin"
GITHUB_WRAPPER="${BIN_DIR}/seyal-github-mcp"
AI_SDLC_DIR="${ROOT}/.sdlc/framework"

# Reviewed/pinned developer-tool inputs. Update only through a normal Seyal PR.
XCODEBUILD_MCP_VERSION="2.7.0"
AI_SDLC_REPO="https://github.com/mahboobmonnamd/ai-sdlc.git"
AI_SDLC_COMMIT="21459b36b3ee351e35af9bfb613a8660033b7590"
AI_SDLC_SKILLS=(
  project-context
  development-readiness
  work-item-design
  implementation-planning
  implementation
  verification
  pr-review
  address-pr-review
)

info() { printf '[seyal bootstrap] %s\n' "$*"; }
warn() { printf '[seyal bootstrap] WARN: %s\n' "$*" >&2; }
has() { command -v "$1" >/dev/null 2>&1; }

ensure_submodules() {
  if [[ -f "${ROOT}/.gitmodules" ]]; then
    info "initializing pinned git submodules"
    git -C "${ROOT}" submodule update --init --recursive
  fi
}

ensure_ai_sdlc() {
  mkdir -p "${ROOT}/.sdlc"

  if [[ ! -d "${AI_SDLC_DIR}/.git" ]]; then
    rm -rf "${AI_SDLC_DIR}"
    info "cloning AI-SDLC developer framework"
    git clone --filter=blob:none --no-checkout "${AI_SDLC_REPO}" "${AI_SDLC_DIR}"
  fi

  info "materializing pinned AI-SDLC ${AI_SDLC_COMMIT}"
  git -C "${AI_SDLC_DIR}" fetch --depth 1 origin "${AI_SDLC_COMMIT}"
  local fetched
  fetched="$(git -C "${AI_SDLC_DIR}" rev-parse FETCH_HEAD)"
  if [[ "${fetched}" != "${AI_SDLC_COMMIT}" ]]; then
    echo "AI-SDLC pin mismatch: expected ${AI_SDLC_COMMIT}, fetched ${fetched}" >&2
    exit 1
  fi
  git -C "${AI_SDLC_DIR}" checkout --detach --force "${AI_SDLC_COMMIT}"

  local skill
  for skill in "${AI_SDLC_SKILLS[@]}"; do
    [[ -f "${AI_SDLC_DIR}/skills/${skill}/SKILL.md" ]] || {
      echo "pinned AI-SDLC revision is missing skills/${skill}/SKILL.md" >&2
      exit 1
    }
  done
  [[ -f "${AI_SDLC_DIR}/tools/project_context.py" ]] || {
    echo "pinned AI-SDLC revision is missing tools/project_context.py" >&2
    exit 1
  }

  info "validating Seyal derived project context"
  python3 "${AI_SDLC_DIR}/tools/project_context.py" --root "${ROOT}" validate
}

ensure_github_mcp_binary() {
  if has github-mcp-server; then
    return 0
  fi
  if [[ "$(uname -s)" == "Darwin" ]] && has brew; then
    info "installing official GitHub MCP server with Homebrew"
    brew install github-mcp-server
  else
    warn "github-mcp-server not found; install it manually to enable GitHub MCP"
    return 1
  fi
}

install_github_wrapper() {
  mkdir -p "${BIN_DIR}"
  cat >"${GITHUB_WRAPPER}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

if ! command -v github-mcp-server >/dev/null 2>&1; then
  echo "github-mcp-server is not installed" >&2
  exit 127
fi

if [[ -n "${GITHUB_PERSONAL_ACCESS_TOKEN:-}" ]]; then
  exec github-mcp-server stdio
fi

if [[ -n "${GITHUB_PAT_TOKEN:-}" ]]; then
  export GITHUB_PERSONAL_ACCESS_TOKEN="${GITHUB_PAT_TOKEN}"
  exec github-mcp-server stdio
fi

if command -v gh >/dev/null 2>&1; then
  token="$(gh auth token 2>/dev/null || true)"
  if [[ -n "${token}" ]]; then
    export GITHUB_PERSONAL_ACCESS_TOKEN="${token}"
    exec github-mcp-server stdio
  fi
fi

echo "GitHub MCP needs authentication. Set GITHUB_PAT_TOKEN or authenticate with 'gh auth login'." >&2
exit 78
EOF
  chmod 0755 "${GITHUB_WRAPPER}"
}

mcp_present() {
  local client="$1" name="$2"
  "$client" mcp list 2>/dev/null | grep -Eq "(^|[[:space:]])${name}([[:space:]:]|$)"
}

configure_mcp_client() {
  local client="$1" label="$2" github_mode="${3:-external}"

  if has xcrun && xcrun --find mcpbridge >/dev/null 2>&1; then
    if ! mcp_present "${client}" xcode; then
      info "configuring official Xcode MCP for ${label}"
      "${client}" mcp add xcode -- xcrun mcpbridge
    fi
  else
    warn "xcrun mcpbridge unavailable; Xcode MCP not configured for ${label}"
  fi

  if [[ "${github_mode}" == "external" ]]; then
    if [[ -x "${GITHUB_WRAPPER}" ]] && ! mcp_present "${client}" github; then
      info "configuring GitHub MCP for ${label}"
      "${client}" mcp add github -- "${GITHUB_WRAPPER}"
    fi
  elif [[ "${github_mode}" != "builtin" ]]; then
    echo "invalid GitHub MCP mode '${github_mode}' for ${label}" >&2
    return 2
  fi

  if has npx; then
    if ! mcp_present "${client}" xcodebuild; then
      info "configuring XcodeBuildMCP ${XCODEBUILD_MCP_VERSION} for ${label}"
      "${client}" mcp add xcodebuild -- npx -y "xcodebuildmcp@${XCODEBUILD_MCP_VERSION}" mcp
    fi
  else
    warn "npx not found; XcodeBuildMCP not configured for ${label}"
  fi
}

configure_claude() {
  has claude || { warn "Claude Code CLI not found; skipping Claude MCP setup"; return 0; }
  configure_mcp_client claude "Claude Code" external
}

configure_codex() {
  has codex || { warn "Codex CLI not found; skipping Codex MCP setup"; return 0; }
  configure_mcp_client codex "Codex" external
}

configure_copilot() {
  has copilot || { warn "GitHub Copilot CLI not found; skipping Copilot MCP setup"; return 0; }
  # Copilot CLI ships GitHub MCP itself. Only add the native-development MCPs Seyal needs.
  configure_mcp_client copilot "GitHub Copilot CLI" builtin
}

configure_cursor() {
  has cursor-agent || { warn "Cursor Agent CLI not found; skipping Cursor MCP setup"; return 0; }

  local config_path="${HOME}/.cursor/mcp.json"
  SEYAL_CURSOR_MCP_CONFIG="${config_path}" \
    SEYAL_GITHUB_WRAPPER="${GITHUB_WRAPPER}" \
    python3 - <<'PY'
import json
import os
import tempfile
from pathlib import Path

config_path = Path(os.environ["SEYAL_CURSOR_MCP_CONFIG"])
config_path.parent.mkdir(parents=True, exist_ok=True)
try:
    config = json.loads(config_path.read_text(encoding="utf-8"))
except FileNotFoundError:
    config = {}
except json.JSONDecodeError as exc:
    raise SystemExit(f"Cursor MCP config is not valid JSON: {exc}")
if not isinstance(config, dict):
    raise SystemExit("Cursor MCP config root must be an object")
servers = config.setdefault("mcpServers", {})
if not isinstance(servers, dict):
    raise SystemExit("Cursor MCP config mcpServers must be an object")
servers["xcode"] = {"command": "xcrun", "args": ["mcpbridge"]}
wrapper = Path(os.environ["SEYAL_GITHUB_WRAPPER"])
if wrapper.is_file():
    servers["github"] = {"command": str(wrapper), "args": []}
servers["xcodebuild"] = {
    "command": "npx",
    "args": ["-y", "xcodebuildmcp@2.7.0", "mcp"],
}
fd, temporary = tempfile.mkstemp(prefix="mcp.json.", dir=config_path.parent)
try:
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2)
        handle.write("\n")
    os.replace(temporary, config_path)
finally:
    try:
        os.unlink(temporary)
    except FileNotFoundError:
        pass
PY
  info "Cursor MCP configuration written to ${config_path}"
}

verify_repo_skills() {
  local required=(
    architecture-change implement-issue issue-refinement milestone-validation
    performance-gate pr-review address-pr-review implementation-planning
    code-review security-review vt-tdd project-context
    development-readiness verification
    macos-native-design macos-ui-testing macos-accessibility visual-regression
    terminal-conformance metal-renderer rust-fuzzing apple-platform-docs image-to-code
  )
  local skill
  for skill in "${required[@]}"; do
    [[ -f "${ROOT}/.agents/skills/${skill}/SKILL.md" ]] || {
      echo "missing canonical skill/adapter: ${skill}" >&2
      exit 1
    }
    [[ -f "${ROOT}/.claude/skills/${skill}/SKILL.md" ]] || {
      echo "missing Claude adapter: ${skill}" >&2
      exit 1
    }
  done
}

main() {
  verify_repo_skills
  ensure_submodules
  ensure_ai_sdlc

  # Claude Code and Codex need Seyal's local GitHub MCP wrapper. Copilot CLI
  # already provides GitHub MCP, so do not install a duplicate solely for it.
  if has claude || has codex || has cursor; then
    if ensure_github_mcp_binary; then
      install_github_wrapper
    fi
  fi

  configure_claude
  configure_codex
  configure_copilot
  configure_cursor

  info "complete"
  info "Seyal-specific skills stay in-repo; generic project-context and core development loop are pinned from AI-SDLC; no credentials were written"
}

main "$@"

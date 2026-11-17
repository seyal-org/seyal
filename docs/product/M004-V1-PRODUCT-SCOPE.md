# M004 V1 product scope — persistent agent-aware terminal workspace

**Status:** Proposed product-scope amendment for maintainer approval (#1510).
**Date:** 2026-11-17
**Release:** M004 / first market-ready v0.1 (called V1 in product discussions).
**Authority:** On acceptance, this addendum augments the launch requirements in MARKET-READY-M004.md. It does not accept an ADR, declare any functionality complete, change the version scheme, or authorize production pickup. Integrate these rows into the matrix after #1493 / PR #1507 is accepted, without weakening existing gates.

## Product promise

A native, high-performance, persistent and agent-aware macOS terminal workspace. A developer can organize multiple projects, run shells and existing coding agents, switch between them without interrupting work, quit/reopen the GUI and reconnect, and jump directly to a terminal requiring attention. The core remains account-free and entirely OSS.

## Mandatory launch outcomes

| Capability | Acceptance outcome | Existing ownership / refinement |
|---|---|---|
| Workspace containment | Window → multiple open workspaces → one split tree per workspace → multiple panes → multiple terminal tabs per pane. Switch in the same window; nonselected executions continue. Create/rename/reorder/close, quick switch, horizontal/vertical nested split, maximize/restore, resize, tab reorder, move/rebind existing sessions safely without respawn; explicit terminate separate from detach. | #1490 / #674, ADR-029 / SPEC-032, ADR-025 / SPEC-031 |
| Project context | Project root and cwd semantics remain distinct; show readable name, cwd/branch/worktree where known; no automatic worktree creation or forced directory changes. Basic reusable layout/cwd templates and safe launch defaults. | #674 / #676 / #262; ADR-028; F-014/F-019 |
| Local session inventory | Discover live, detached, attached and exited states from Runtime authority; search across workspaces, reveal/reattach, detach and explicitly terminate exact identity. No duplicate PTYs, silent killing or command replay. | #929 / #687 / #832; ADR-017/018/025 |
| Durable continuity | GUI close/quit/crash and reopen reattach same live executions and restore workspace/tab/pane layout, focus and bounded retained history. Runtime death and reboot restore available durable metadata/history but report lost processes; start replacement shells only explicitly. Corrupt state, mismatched versions and reconnect failure degrade visibly. | #687 / #832 / #677; proposed ADR-023 |
| Natural mouse and keyboard | Complete #1492: click-to-place in trusted editable Bash/Zsh/Fish command lines (wrapped/Unicode/multiline), selection including rectangular, paste/copy, search, scrolling, safe hyperlinks, native TUI mouse and keyboard arbitration; consistent customizable macOS shortcuts for navigation/creation/close/splits/search/attention and native IME. No prompt scraping or arbitrary historical-output cursor motion. | #1492 / #1498 / #1499 / #1500 / #1501 / #1502 / #676 |
| Standard terminal behavior | All existing M004 terminal compatibility, Unicode, reflow, VT conformance, config, rendering, startup and resource gates stay mandatory; ordinary Claude Code/Codex/other CLI tools must work as terminal workloads. | #664 / #672 / #673 / #665 |
| Local CLI orchestration | Bounded typed local operations: enumerate/create/select workspaces, create/split/navigate terminal surfaces, launch a new shell/process using explicit argv/cwd, list/detach/attach sessions, and submit notification events. GUI and CLI share existing Runtime execution authority and Rust-owned presentation authority; GUI-only operations must have defined absent-GUI behavior, not invent another layout owner. Scope requests to stable resource IDs, validate peer/session authority, distinguish inspection from mutation and destructive actions, prevent unintended input injection into busy shells; no arbitrary remote API. | #262 SY-010, #674, #929; refine contracts before implementation |
| Agent-aware notifications | Bounded documented integrations for Claude Code, Codex and OpenCode where supported vendor hooks exist, plus generic CLI notification ingress. Map events to exact live execution and workspace/tab/pane; attention badges, unread inbox, next-attention shortcut, reveal exact target, OS notification when appropriate; suppress duplicate focused notifications and rate-limit storms. Show needs-input, approval-needed, completion and failure only from supported evidence; unsupported or unknown events remain unknown. Notification is advisory, never trusted approval or command authorization. | #644 / #680, SPEC-028 and existing AttentionAuthority / chrome; refine source and trust mapping |
| Distribution and operational safety | Keep existing no-account, accessibility, privacy/redaction, diagnostic, signed/notarized install, update/rollback, reproducibility and exact-head release gates. Benchmark idle detached sessions, hidden workspaces and concurrent streaming plus notification load; avoid unbounded processes/queues/history/caches. | #673 / #677 / #688 |

## Boundaries and architecture obligations

- Raw-only user-facing resting presentation during Blocks suspension, subject to #1493 acceptance. Preserve Block code/evidence; no Flow switch, composer requirement or alternate VT/grid/PTY. Defer Blocks and composer until after launch. This product decision is an explicit proposed scope amendment, not unilateral modification of accepted technical authority.
- Runtime owns live execution identity and PTY lifecycle; Rust client owns product presentation and navigation under ADR-015. CLI requests must use approved ownership boundaries, including GUI absence. No second Runtime, presentation registry or notification authority; do not make terminal processing wait on agents, notifications, persistence, licensing or cloud.
- Use existing Attention contracts and implementation where appropriate; define minimal terminal-scoped notifications without requiring a WorkItem/Attempt/AgentRun for ordinary CLI processes. Do not fabricate lifecycle/approval events or turn arbitrary terminal text into trusted agent state. Verify hook availability, provenance, replay/deduplication, permissions, cross-session isolation and secret-safe previews. Remote/untrusted output cannot acquire local control rights. No native approval execution in V1.
- Session persistence is not process resurrection. Keep durable metadata and bounded history asynchronous; keep explicit termination separate from UI close and distinguish detached from exited. Reconcile ADR-023, the sidebar and session inventory before production durability work.
- Templates are declarative local layout/cwd defaults, not workflow DAGs; automatic command execution requires explicit trusted configuration/consent. Worktree awareness is read-only context, not a worktree orchestration engine.
- CLI syntax, precise shortcut defaults, notification hook mechanisms, data schema, transport and component ownership details require refinement against existing ADRs/specs; do not treat illustrative chat commands as accepted syntax.
- OSS never depends on commercial code; the commercial repository consumes OSS through its pinned submodule.

## Explicitly post-launch

Native Blocks/composer; provider login and managed agent execution; structured approval actions, smart routing, memory/context/evaluation, multi-agent orchestration, workflow DAGs, managed worktree automation, embedded browser/editor, cloud/remote Runtime product, collaboration, enterprise and Linux/Windows/mobile. SSH and coding agents running as normal terminal workloads remain launch requirements.

## Delivery order (existing owners; not implementation authorization)

1. Accept #1493, complete #1492 and M002 terminal acceptance including measured performance and actual packaged-app pixels.
2. Finish M003 workspace containment, tabs/splits, navigation and config; reconcile #1490 / #1494 and session inventory ownership before adding competing work.
3. Accept durability contracts (#832/#687); implement authoritative session inventory and restore/reattach; prove detach, crash and restart truth.
4. Refine and deliver bounded CLI operations, templates and project/worktree context using existing APIs and ownership.
5. Promote only the advisory notification subset from M005; deliver exact-target attention with security/privacy qualification. Provider integrations can be independently refined after target/session identities are stable.
6. Complete M004 release engineering, accessibility, diagnostics, documentation, security, physical performance, clean install/update and real developer daily-driver validation. No market-ready claim until every launch row passes on the same release candidate.

Independent docs/architecture refinement and release research can continue without mutating the terminal lane. Do not start overlapping implementation scopes or relax existing correctness/quality gates.

## Release evidence

An exact packaged candidate must demonstrate two or more workspaces with independent panes and terminal tabs, concurrent agent/shell/SSH workloads, workspace switching and CLI orchestration, targeted agent notifications from background sessions, GUI quit/reopen and same-execution reattach, safe explicit termination and honest recovery after Runtime loss. Test background resources, visual correctness, mouse/keyboard/IME, trust boundaries, malformed or storming notifications, CLI denial and wrong-target cases. Existing exact-head M004 conformance, performance, persistence, packaging, accessibility and security requirements still apply.

After acceptance, reconcile the launch matrix and roadmap milestone wording, M003 Block requirements, M005 promoted subset, and owning GitHub issues. Track incomplete decisions explicitly; no issue is marked Done or Ready merely by accepting this scope.

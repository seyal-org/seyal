# Seyal Product Feature Inventory

**Status:** Canonical product feature inventory  
**Scope:** Product capabilities and explicit rejected/superseded decisions. Architecture/ADR/spec/milestone documents remain implementation authority.

This file is the single product feature registry for Seyal OSS and its stable public extension seams. It is deliberately broader than the current implementation. A feature appearing here does **not** authorize implementation or bypass Issue → R&D/ADR/spec → milestone → tests/benchmarks/security review.

Legacy state is evidence only. A RILL/terminal issue being closed, merged or previously implemented never means the capability is implemented in the current Seyal architecture.

## Status vocabulary

- **Implemented** — current Seyal production capability has passed its owning milestone definition of done.
- **Foundation exists** — current Seyal has the necessary production foundation, but the complete user-facing capability is not yet shipped.
- **Accepted direction** — capability belongs in Seyal product direction; implementation may still need R&D/specification.
- **Deferred / decision required** — useful historical idea, but current Seyal has not committed to the exact capability or safety/UX contract.
- **Superseded** — product need remains covered by a newer Seyal model; do not implement the legacy shape.
- **Rejected** — deliberately not part of Seyal unless a future explicit decision supersedes this one.
- **Historical-only** — retained as evidence, not current product direction.

## Current implementation baseline

Current Seyal foundation includes the deterministic engineering/toolchain gates, Seyal-owned M001 VT/TerminalState subset, macOS PTY/child lifecycle, persistent headless Runtime composition with stable Runtime/Workspace/Execution identity, and the native Swift/AppKit/Metal application boundary. Those foundations do **not** imply that the broader UI, Blocks, remote, agent, collaboration or workflow features below are implemented.

## Seyal-native capabilities discovered after the RILL catalog

These capabilities were accepted/refined during Seyal architecture and product research and are not represented cleanly by one historical RILL `F-*` row.

| ID | Capability | Seyal status | Current product contract |
|---|---|---|---|
| SY-001 | Seyal Resource Addressing | Accepted direction | Stable Workspace/Session/Tab/Pane/Execution/Block/WorkItem/Attempt/AgentRun/Artifact references. References never grant authority. |
| SY-002 | Context-aware Seyal CLI | Accepted direction | Commands such as `seyal split` or `seyal link` resolve the current authorized context without forcing users to copy opaque IDs. |
| SY-003 | Exact teammate handoff | Accepted direction | Authorized teammate can open the exact workspace/execution/block/work item and observe, request control, fork investigation or open an independent shell. |
| SY-004 | Agent Worktree Awareness | Accepted direction | Expose agent worktree path/branch/base/changes/PR/lifecycle as first-class work context tied to WorkItem/Attempt/AgentRun. |
| SY-005 | Safe worktree shell transition | Accepted direction | Never silently `cd` a busy shell; open a new execution in the worktree or adopt only when trusted shell integration proves an idle prompt. |
| SY-006 | Tiered Agent Presence Detection | Accepted direction | Structured adapter → official hooks → process/shell signals → low-confidence terminal heuristics. Heuristics never become auth/approval/audit truth. |
| SY-007 | Provider-neutral SCM/CI adapters | Accepted direction | GitHub/GitLab/Bitbucket/Jenkins/Buildkite/custom systems consume a common capability seam for changes, PR/MR and checks. |
| SY-008 | Secure Remote Connection Multiplexing | Accepted direction | Reuse SSH transport only when host, user, auth identity, trust policy, jump chain and workspace policy are compatible; no implicit forwarding/credential sharing. |
| SY-009 | Stable workspace ordering + attention projection | Accepted direction | Do not auto-reorder workspaces for urgency. Use badges and Attention Stack; optional user-selected sort may exist. |
| SY-010 | Universal Seyal Integration CLI / Shell API | Accepted direction | Typed local integration actions for attention, artifacts, progress, diffs, navigation and structured metadata. |
| SY-011 | Capability-scoped Control API | Accepted direction | Authenticated external control is scoped by resource and capability; unrestricted arbitrary key injection is not the default API. |
| SY-012 | Block References | Accepted direction | Stable Block identity enables bookmark/pin/link/compare/rerun/save/promote-to-workflow without copying terminal authority. |
| SY-013 | Command Library | Accepted direction | Save commands from Blocks or manually with Personal, Project and Team scopes; local use does not require cloud. |
| SY-014 | Parameterized Commands | Accepted direction | Saved commands may expose safe visible parameters such as namespace/service/environment before execution. |
| SY-015 | Promote command sequence to Workflow/Runbook | Accepted direction | Turn proven procedures into reusable workflows using effect/replay/idempotency safety rather than blind transcript replay. |
| SY-016 | Selective local-first sync | Accepted direction | Optional sync for settings/themes/keybindings/commands/runbooks/preferences/selected metadata; raw history, secrets, SSH credentials and sensitive artifacts are excluded by default. |
| SY-017 | Key-to-photon latency contract | Accepted direction | Measure p50/p95/p99 input-to-display latency and compare on identical hardware; no reactive/agent/persistence/cloud work on the keystroke path. |
| SY-018 | Local Context Engine | Accepted direction | OSS local repository/docs/git/artifact retrieval with provenance, freshness, sensitivity, dedupe, token budgets and inspectable context. |
| SY-019 | Local capability/rule router | Accepted direction | Deterministic capability/provider/model routing with user rules, budgets, fallback chains and explainable decisions; learned/service-operated routing is outside this OSS contract unless separately accepted. |
| SY-020 | Durable local workflow DAG + effect/replay safety | Accepted direction | WorkflowRun/NodeRun DAG, bounded scheduling, typed handoffs, approvals, retries and reconciliation; ambiguous external side effects are never blindly retried. |
| SY-021 | Multi-agent orchestration + writer isolation | Accepted direction | Parallel agent runs, dedicated worktrees for concurrent writers, conflict/duplicate detection, independent evaluation and explicit reconciliation. |
| SY-022 | DevOps execution workspace | Accepted direction | Processes, agents, remotes, logs, results and typed operational actions compose around the same Runtime; Seyal does not become an IDE or a second terminal engine. |
| SY-023 | Working-tree Changes inspector | Accepted direction | Root-bounded git status + capped read-only diff, coalesced only while visible; edit in external IDE/editor and keep git work off the terminal hot path. |
| SY-024 | Agent evaluation, budgets and explainability | Accepted direction | Track attempts, tests/checks, elapsed time, provider token/cache/cost data, intervention and routing/status explanations without gating terminal progress. |
| SY-025 | Context Privacy Scope | Accepted direction | User/policy-controlled exclusion at Workspace/Execution/Block/File/Artifact scope prevents excluded content from agent retrieval, summaries, semantic indexes or sync unless explicitly re-enabled within policy. |
| SY-026 | Quick Agent Popover | Accepted direction | Compact non-modal contextual agent interaction may float over workspace chrome without resizing terminal/editor layout; it projects the same AgentRun and can expand into a full agent surface without duplicating authority. |

## Historical RILL feature catalog reconciliation

The original RILL competitive catalog contains **216** inventory rows. All 216 are accounted for below. This table preserves useful product discovery while recalculating status against **current Seyal**, not legacy completion state.

### 5.1 — Hierarchy, dashboards and navigation

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-001 | Host indicator | Accepted direction | Show local/remote execution host in workspace/pane chrome. |
| F-002 | Default session | Foundation exists | One persistent headless Runtime and stable default workspace foundation exist. |
| F-003 | Named sessions | Accepted direction | Keep stable Session identity; legacy per-session socket/state architecture is superseded. |
| F-004 | Workspace | Foundation exists | Runtime publishes stable Workspace association for executions. |
| F-005 | Workspace groups | Accepted direction | Collapsible/organizable workspace groups in navigation. |
| F-006 | Tab owns layout | Accepted direction | Tabs own layout while inactive executions remain live. |
| F-007 | Nested splits | Accepted direction | Nested horizontal/vertical split tree. |
| F-008 | Surface stack in a pane | Deferred / decision required | Multiple pane surface types fit the model, but exact in-pane surface stack UX is not yet fixed. |
| F-009 | Close narrowest context | Accepted direction | Close the narrowest focused UI context without killing unrelated executions. |
| F-010 | Workspace dashboard | Accepted direction | Workspace-wide execution/status inventory and jump navigation. |
| F-011 | Agent dashboard | Accepted direction | Cross-workspace agent/run census and navigation. |
| F-012 | Session/process switcher | Accepted direction | Find and jump to running executions/process contexts. |
| F-013 | Command palette | Accepted direction | Searchable command/action palette. |
| F-014 | Quick switcher | Accepted direction | Fast workspace switcher by stable identity/name. |
| F-015 | Focus history | Accepted direction | Back/forward focus navigation history. |
| F-016 | Reopen closed | Accepted direction | Reopen recently closed presentation objects without pretending dead processes survived. |
| F-017 | Drag rearrange | Accepted direction | Rearrange tabs/panes/presentations while preserving execution identity. |
| F-018 | Pane zoom / equalize | Accepted direction | Zoom focused pane and equalize split sizes. |
| F-019 | Layout templates | Accepted direction | Reusable workspace/tab/split/cwd launch layouts. |
| F-020 | Sidebar hide | Accepted direction | Hide/show sidebar without changing runtime ownership. |
| F-021 | Global summon | Deferred / decision required | Global summon hotkey is useful but not yet a Seyal product commitment. |
| F-022 | Deep links | Superseded | Replaced by richer Seyal Resource Addressing for exact local/shared targets. |
| F-023 | Canvas freeform layout | Rejected | Freeform canvas is not the canonical workspace/layout authority. |
| F-024 | Vertical workspace tabs | Accepted direction | Sidebar/workspace switcher may be vertical without creating a second workspace model. |
| F-025 | Task / resource manager | Accepted direction | Process/resource visibility fits the execution/DevOps workspace; exact scope remains later. |
| F-026 | Goto picker | Accepted direction | Searchable workspace/tab/pane/execution/agent goto surface. |
| F-027 | Custom interpreted sidebars | Deferred / decision required | Custom sidebars require the later capability-scoped extension model. |
| F-028 | Nested multiplexers allowed | Foundation exists | tmux/zellij/vim/other TUIs are supported as ordinary child workloads, not Seyal runtime authority. |
| F-029 | Agent transcript vault | Accepted direction | Searchable retained agent-run/provider-session history with explicit retention/privacy. |

### 5.2 — Persistence and recovery

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-030 | Detach keeps processes | Foundation exists | Runtime/execution ownership is independent from presentation attachment. |
| F-031 | Reattach same IDs | Foundation exists | Stable Runtime/Workspace/Execution identities support detach/reattach; full UI restore is later. |
| F-032 | Event replay | Accepted direction | Durable/idempotent event replay where persistence exists; never replay ambiguous terminal input blindly. |
| F-033 | Disconnect ≠ exit | Foundation exists | Attachment loss and child/process exit are distinct lifecycle states. |
| F-034 | Runtime crash honesty | Foundation exists | Current architecture explicitly refuses to claim journal-based live PTY resurrection. |
| F-035 | Layout snapshot | Accepted direction | Persist presentation/layout metadata separately from live PTY truth. |
| F-036 | Block/scrollback restore | Accepted direction | Persist bounded/redacted scrollback/Block history later; separate from live-process recovery. |
| F-037 | Agent session resume | Accepted direction | Resume capable agent harness sessions through provider-neutral continuity metadata. |
| F-038 | Live server handoff | Deferred / decision required | Live PTY handoff across Runtime binary replacement requires separate proven keeper/handoff architecture. |
| F-039 | Live session restore | Foundation exists | GUI detach/reconnect to live Runtime is the intended model; end-user restore UX remains later. |
| F-040 | Input delivery states | Accepted direction | Explicit delivery state for queued/dispatched/ambiguous input and effects. |
| F-041 | Multi-client attach | Foundation exists | Domain/runtime model supports multiple logical attachments and observer/controller policy seams. |
| F-042 | Quit vs detach | Accepted direction | GUI quit is detach; explicit Runtime/execution termination is separate and guarded. |
| F-043 | Nested-launch guard | Accepted direction | Prevent accidental nested Seyal Runtime launch unless explicitly requested. |
| F-044 | Protocol version handshake | Accepted direction | Versioned attach/control protocols fail visibly and recover safely on mismatch. |
| F-045 | Single-process escape hatch | Deferred / decision required | Debug-only nonpersistent/single-process escape hatch needs evidence before product exposure. |
| F-046 | Idle agent hibernation | Deferred / decision required | Only resumable agent workloads could hibernate; arbitrary shells must never be killed under this feature. |
| F-047 | Move pane without new PTY | Accepted direction | Move/reparent a live pane presentation without creating a new PTY/execution. |

### 5.3 — Mouse, input editor and Blocks

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-050 | Mouse-first chrome | Accepted direction | Mouse-first and keyboard-first navigation are both first-class. |
| F-051 | Click-to-place input | Accepted direction | Rich multiline composer/caret editing where composer owns input; raw/TUI semantics remain untouched. |
| F-052 | Multi-cursor / word ops | Deferred / decision required | Advanced multi-cursor editing is optional composer UX, not terminal foundation. |
| F-053 | Copy on select | Deferred / decision required | Optional copy-on-select policy; must not surprise users or break TUI selection. |
| F-054 | Smart / rectangular select | Accepted direction | Terminal/Block selection should support smart and rectangular selection where semantics allow. |
| F-055 | Clickable files & links | Accepted direction | Recognize/open safe URLs and paths, including OSC 8; preserve security prompts/policy. |
| F-056 | Path with line/column | Accepted direction | Open file:line:column in configured external editor. |
| F-057 | Syntax highlight + error underline | Accepted direction | Composer syntax/error affordances are additive and off terminal hot path. |
| F-058 | Alias expansion | Deferred / decision required | Alias expansion needs shell-truth integration; do not guess from static parsing. |
| F-059 | Command inspector | Deferred / decision required | Inline command/flag documentation is useful but not yet committed. |
| F-060 | Autosuggest / tab complete | Accepted direction | Local history/path/argument autosuggest and completion in composer. |
| F-061 | Quote/bracket autocomplete | Deferred / decision required | IDE-like quote/bracket pairing is optional composer behavior. |
| F-062 | Input Vim mode | Deferred / decision required | Optional modal editing for composer; raw/TUI input stays application-owned. |
| F-063 | Command history | Accepted direction | Secret-safe local command history with cwd/exit metadata when trusted signals exist. |
| F-064 | Unified command search | Accepted direction | Unified search can combine history, saved commands/workflows and other typed resources. |
| F-065 | Command corrections | Deferred / decision required | Post-failure correction suggestions are optional and must be nonblocking. |
| F-066 | Synchronized inputs | Deferred / decision required | Broadcast input is powerful/destructive and needs explicit target/confirmation semantics. |
| F-067 | Workflows / parameterized commands | Superseded | Folded into Saved Commands, parameterized commands and Workflow/Runbook model. |
| F-068 | Pin input top/bottom | Superseded | Current Seyal UI direction uses a per-pane bottom composer rather than arbitrary top/bottom placement. |
| F-069 | Classic/raw input | Accepted direction | Raw/classic path always available and sends application input directly to the same execution. |
| F-070 | Command Blocks | Accepted direction | Blocks are fundamental same-execution presentation objects, never a second PTY/grid. |
| F-071 | Copy command / output / both | Accepted direction | Copy command, output, or both with chrome/secret boundaries respected. |
| F-072 | Pending Block | Accepted direction | Blocks expose running/pending lifecycle; legacy spinner behavior is not normative. |
| F-073 | Prompt drain | Accepted direction | Block boundaries must exclude pre-command prompt/drain bytes. |
| F-074 | Background Blocks | Accepted direction | Unattributed/background output needs an honest presentation rather than attaching to wrong command. |
| F-075 | Sticky command header | Accepted direction | Keep relevant command identity visible while scrolling long Block output. |
| F-076 | Block find / filter | Accepted direction | Search/filter within and across retained Blocks. |
| F-077 | Rerun / edit-and-run | Accepted direction | Re-run/edit-and-run uses explicit execution context and safe quoting. |
| F-078 | Bookmark Blocks | Superseded | Covered by Block References: bookmark/pin/link/compare/rerun. |
| F-079 | Attach Block as context | Superseded | Covered by typed Context Attachments plus Block References. |
| F-080 | Share Block permalink | Superseded | Covered by Seyal Resource Addressing and exact authorized handoff/share. |
| F-081 | Compact Blocks / dividers | Accepted direction | Density/divider controls without sacrificing terminal readability. |
| F-082 | Live terminal Block | Superseded | Legacy second-live-surface concept is replaced by one execution with Block/raw/TUI projections. |
| F-083 | Raw mode same PTY | Foundation exists | Alternate-screen/raw presentation remains the same canonical TerminalExecution/TerminalState. |
| F-084 | Shift+mouse for UI vs TUI | Accepted direction | Explicit mouse arbitration lets users select UI text without breaking app mouse reporting. |
| F-085 | Rich input overlay for CLI agents | Accepted direction | Rich composer may submit to CLI agents only when input ownership is known and safe. |
| F-086 | Right-click menus | Accepted direction | Context menus for pane/tab/Block/agent actions. |
| F-087 | Keyboard copy mode | Accepted direction | Keyboard copy/navigation mode over retained terminal history. |
| F-088 | Warpify / in-band remote cmds | Rejected | No vendor helper that silently injects commands into SSH/subshell PTYs. |
| F-089 | Prompt chips vs PS1 | Accepted direction | Optional cwd/git/dev-tool context chips as host chrome; never replace/break PS1. |
| F-252 | Up/Down history recall | Accepted direction | Up/Down history recall in composer, prefix-aware; raw mode leaves keys to PTY. |
| F-253 | Ctrl+R history search | Accepted direction | Ctrl+R fuzzy history/search list with keyboard/mouse selection; raw mode leaves Ctrl+R to PTY. |
| F-254 | Natural caret navigation | Accepted direction | Natural macOS/readline caret and kill-key behavior in composer. |
| F-256 | Keyboard tab and pane cycle | Accepted direction | Keyboard tab/pane cycling with predictable focus-ring behavior. |

### 5.4 — Terminal fidelity

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-090 | GPU terminal engine / libghostty adapter | Superseded | Legacy libghostty production engine direction is replaced by Seyal-owned VT/state + Metal architecture. |
| F-091 | zsh / bash / fish | Foundation exists | PTY/VT foundation exists; broad zsh/bash/fish compatibility remains a conformance workload. |
| F-092 | Alternate screen | Foundation exists | M001 alternate-screen subset exists in canonical TerminalState; broader conformance grows incrementally. |
| F-093 | Mouse reporting to apps | Accepted direction | Application mouse-reporting protocols are terminal-fidelity work, with host override arbitration. |
| F-094 | Kitty keyboard protocol | Accepted direction | Kitty keyboard protocol when applications opt in, after tested compatibility. |
| F-095 | Unicode / IME / clipboard | Accepted direction | Full Unicode/grapheme/width, IME and clipboard correctness are terminal fundamentals. |
| F-096 | Graphics protocols | Accepted direction | Kitty/iTerm graphics protocols are later tested terminal-fidelity capabilities. |
| F-097 | Shell integration hooks | Accepted direction | Trusted shell integration for cwd/command/exit/duration/running boundaries. |
| F-098 | Ghostty config import | Superseded | Seyal TOML is canonical; an optional appearance-only Ghostty import may be considered without engine dependency. |
| F-099 | Scrollback compression | Accepted direction | Bounded/compact scrollback for large hidden/detached populations. |
| F-100 | Working directory policy | Accepted direction | Configurable cwd inheritance policy for new tab/pane/execution. |
| F-101 | Startup shell / env | Accepted direction | Per-workspace/execution startup shell and environment policy with secret boundaries. |
| F-102 | Nested tools in a pane | Foundation exists | Nested tmux/zellij/vim/htop/etc run as normal child workloads. |
| F-103 | Audible bell | Accepted direction | Optional BEL behavior, configurable and nondisruptive. |
| F-104 | Contrast / light theme | Accepted direction | Light/dark themes must preserve readable contrast and terminal colors. |

### 5.5 — Attention and notifications

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-110 | Attention queue | Accepted direction | One typed Attention model for needs-input, approval, completion, failure and disconnect. |
| F-111 | Sidebar badges / rings | Accepted direction | Unread/attention badges on stable workspace/tab/pane navigation. |
| F-112 | In-app mailbox | Accepted direction | Global Attention Stack/mailbox with filtering; not duplicate authoritative state. |
| F-113 | Jump to exact target | Accepted direction | Attention selection resolves exact Workspace/Execution/Block/Agent target. |
| F-114 | Next-attention shortcut | Accepted direction | Keyboard jump to next relevant AttentionItem. |
| F-115 | Native OS notifications | Accepted direction | Native OS notifications when backgrounded, policy-controlled. |
| F-116 | Suppress when focused | Accepted direction | Suppress redundant desktop notifications for actively focused target. |
| F-117 | OSC 9 / 99 / 777 | Accepted direction | OSC notify sequences may create untrusted notification input, never privileged approvals. |
| F-118 | CLI notify | Accepted direction | Typed CLI notification/attention entry point folds into universal Integration CLI. |
| F-119 | Notification hooks | Accepted direction | Trusted config may filter/group notification presentation without blocking terminal progress. |
| F-120 | Quiet / rate limit | Accepted direction | Storm grouping, cooldowns and quiet policies. |
| F-121 | Long-command complete | Accepted direction | Optional completion notification for long commands when user is elsewhere. |
| F-122 | Password-prompt notify | Deferred / decision required | Sensitive/password-prompt detection cannot rely on spoofable raw text as trusted state. |
| F-123 | Agent blocked rollup | Accepted direction | Parent workspace/run rolls up highest-urgency child attention without reordering navigation. |
| F-124 | Mark read on view | Accepted direction | Read/ack state updates when exact target is viewed, with explicit semantics. |

### 5.6 — Agents

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-130 | Task as runtime object | Superseded | Seyal uses WorkItem → Attempt → AgentRun → Execution/Artifact/Attention rather than legacy Task authority. |
| F-131 | Replay adapter | Accepted direction | Replay/fake harness adapter for deterministic tests and offline review. |
| F-132 | Section types | Accepted direction | Typed prompt/plan/tool/command/output/approval/diff/result/question sections. |
| F-133 | Approve / reject / cancel | Accepted direction | Typed approve/reject/cancel actions only for trusted structured requests. |
| F-134 | Question cards | Accepted direction | Structured question cards route through Attention and exact run identity. |
| F-135 | Detect CLI agents | Superseded | Expanded into Tiered Agent Presence Detection with explicit source/confidence. |
| F-136 | Lifecycle authority | Accepted direction | Structured provider/hook lifecycle beats terminal scraping; fallback stays low-confidence. |
| F-137 | Broad agent coverage | Accepted direction | Provider-neutral support for many CLI/agent harnesses. |
| F-138 | Native structured adapter | Accepted direction | Native structured adapters map provider events into Seyal's typed model. |
| F-139 | CLI-native path | Accepted direction | CLI-native agents remain first-class TerminalExecution workloads with additive metadata/UI. |
| F-140 | Prompt queue | Accepted direction | Queue/edit/reorder follow-up prompts within an AgentRun/workflow. |
| F-141 | Permission profiles | Accepted direction | Allow/deny/ask permission profiles scoped to workspace/run/capability. |
| F-142 | Task lists | Accepted direction | Agent/workflow task lists with explicit status and ownership. |
| F-143 | MCP servers | Accepted direction | User-configured MCP servers behind explicit capabilities and trust. |
| F-144 | Rules / skills | Accepted direction | Local project/user rules and skills; cloud is not required. |
| F-145 | Context attachments | Accepted direction | Scoped context attachments: files, Blocks, selections, URLs, images, artifacts. |
| F-146 | Diff review | Accepted direction | Diff review surface; mutating apply/reject actions require explicit safe semantics. |
| F-147 | Checkpoints | Accepted direction | Agent/workflow checkpoints for inspect/revert where underlying effects are actually reversible. |
| F-148 | Conversation fork | Accepted direction | Fork an investigation/conversation into a new Attempt/AgentRun/execution context. |
| F-149 | Direct agent attach | Accepted direction | Attach/associate an existing execution with an agent run without creating duplicate terminal state. |
| F-150 | Agent CLI orchestration | Accepted direction | Typed CLI orchestration for start/wait/prompt/read/cancel where harness supports it. |
| F-151 | Integrations install | Accepted direction | Install/configure official agent integrations/hooks explicitly. |
| F-152 | Custom labels / metadata | Accepted direction | Human labels plus provider-reported metadata such as tokens/cost/status. |
| F-153 | Worktrees for parallel tasks | Accepted direction | Parallel agent writers use dedicated worktrees; read-only work may share safe views. |
| F-154 | Model picker / BYOK / local providers | Accepted direction | Provider/model selection supports user-configured cloud models, local/private endpoints and generic compatible endpoints without requiring a Seyal-operated service; routing remains off terminal hot paths. |
| F-155 | Voice input | Deferred / decision required | Voice input is optional and outside terminal foundation. |
| F-156 | Computer / browser use | Deferred / decision required | Browser/computer use may be capability-gated later; never terminal hot-path authority. |
| F-157 | Remote/background agent execution | Accepted direction | Optional external/background agent execution may integrate through public seams; local use needs no account. |
| F-158 | Optional conversation sync | Accepted direction | Optional conversation synchronization may exist; complete local operation remains possible without it. |
| F-159 | Agent session sharing | Accepted direction | Explicit, revocable, authorization-checked agent/session sharing and teammate handoff. |
| F-160 | Vendor terminal UI lock-in | Rejected | Seyal will not clone a vendor terminal/agent UI as its core architecture. |
| F-161 | HITL feed cards | Superseded | Folded into typed Attention Stack/HITL actions bound to WorkItem/AgentRun. |
| F-162 | Agent explain | Accepted direction | Explain why an agent/run is classified waiting/blocked/idle, including signal source/confidence. |
| F-163 | Agent drives TUI | Deferred / decision required | Agent-driven TUI input requires explicit scoped authority; unrestricted key injection is not acceptable. |
| F-164 | NL autodetection | Rejected | Implicit shell-vs-agent natural-language classifier is not default input routing; keep explicit typed actions. |

### 5.7 — Remote

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-170 | Runtime on the host | Foundation exists | Headless Runtime owns executions on the host; remote hosts use the same authority model later. |
| F-171 | SSH attach | Accepted direction | Secure SSH attach to a remote Seyal Runtime/execution environment. |
| F-172 | Thin local client | Accepted direction | Thin local client keeps native input/clipboard/navigation while execution remains remote. |
| F-173 | SSH-then-attach | Accepted direction | Classic SSH-then-attach compatibility path may coexist with richer client flow. |
| F-174 | Reconnect + backoff | Accepted direction | Reconnect/backoff with identity/version checks and state resynchronization. |
| F-175 | Remote notify relay | Accepted direction | Remote structured attention/notification relay to authorized local client. |
| F-176 | Remote cwd / scp drop | Accepted direction | Remote cwd awareness and explicit file transfer/drop with policy controls. |
| F-177 | Browser via remote network | Deferred / decision required | Remote localhost/browser exposure requires explicit tunnel/security UX. |
| F-178 | Mosh transport | Deferred / decision required | Mosh/roaming transport is optional after SSH correctness and semantics are proven. |
| F-179 | Remote tmux mirror | Rejected | Do not mirror an external tmux hierarchy as Seyal-native panes; tmux remains a child workload. |
| F-180 | Host key / version check | Accepted direction | Host-key, server identity and protocol/version checks before attach. |

### 5.8 — Extra surfaces

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-190 | Development preview pane | Accepted direction | Sandboxed local dev-server and selected artifact preview is a cold non-terminal surface; detection should prefer trusted process/port/runtime signals and never block terminal progress. |
| F-191 | File explorer | Accepted direction | Root-bounded workspace file tree/search/navigation integrates with the focused editor and agent context; filesystem watching/index work stays asynchronous. |
| F-192 | Markdown viewer | Accepted direction | Markdown/notebook viewing and executable runbook content as cold surfaces. |
| F-193 | Diff viewer pane | Accepted direction | Read-only/capability-scoped diff review surface. |
| F-194 | Built-in editor + LSP as one IDE subsystem | Superseded | Replaced by separate bounded editor (`F-240`) and optional/lazy LSP (`F-241`) capabilities; do not create an IDE-scale always-resident subsystem. |
| F-195 | Git worktree UI | Accepted direction | Git worktree create/open/reveal/archive UX tied to WorkItem/Attempt. |
| F-196 | iOS / mobile companion | Accepted direction | Mobile companion/remote control later over the same secure attach/resource model. |
| F-197 | Simulator panes | Deferred / decision required | Simulator-as-pane is optional platform tooling, not core terminal scope. |
| F-198 | Dock panes | Deferred / decision required | Dock/auxiliary terminal presentations are low-priority platform UX. |
| F-199 | Sleepy / menubar keep-awake | Deferred / decision required | Keep-awake/menubar mode needs explicit power/user-intent policy. |
| F-200 | Popup floating terminals | Rejected | No untracked popup PTY; a popup may only present an existing tracked execution. |
| F-201 | Workspace pin / colors / icons | Accepted direction | Pin/color/icon metadata for stable workspace navigation. |
| F-202 | Workspace status lanes | Accepted direction | Workspace status labels may summarize state, but must not auto-reorder user navigation by default. |
| F-203 | Workspace todos | Accepted direction | Per-workspace checklist/todos as cold workflow metadata. |
| F-204 | Per-workspace env and ports | Accepted direction | Workspace environment metadata and advertised/listening ports with security boundaries. |
| F-205 | Warp Drive “workspaces” | Rejected | Do not overload Seyal Workspace with Warp-Drive-style cloud-space naming; knowledge/sync is separate. |

### 5.9 — Appearance, configuration, platform and security

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-210 | Canonical theme schema | Accepted direction | One canonical Seyal theme/look schema; importers are adapters. |
| F-211 | OS light/dark sync | Accepted direction | Follow system appearance when configured. |
| F-212 | Pane dimming / mouse focus | Accepted direction | Inactive-pane dimming and optional focus-follows-mouse. |
| F-213 | Opacity / blur | Accepted direction | Native opacity/blur effects only if they preserve readability/performance. |
| F-214 | Custom dock icons | Deferred / decision required | Custom dock icons are cosmetic and low priority. |
| F-215 | Settings sync via cloud | Superseded | Expanded into selective local-first sync; local config remains authoritative and sensitive data is excluded by default. |
| F-216 | Local settings file | Accepted direction | Canonical local TOML configuration works without an account. |
| F-217 | Project-local trusted config | Accepted direction | Project-local config requires explicit trust before actions/capabilities execute. |
| F-218 | Custom keybindings | Accepted direction | Custom keybindings/chords with conflict detection and raw/TUI correctness. |
| F-219 | Command palette custom actions | Accepted direction | User/project custom typed actions can appear in Command Palette. |
| F-220 | Plugins (out of process) | Accepted direction | Later out-of-process extensions/plugins with declared, reviewable capabilities. |
| F-221 | Plugin marketplace | Deferred / decision required | Marketplace/discovery comes only after a secure extension model exists. |
| F-222 | Socket + CLI | Superseded | Expanded into Universal Seyal Integration CLI plus capability-scoped Control API. |
| F-223 | Secret redaction | Accepted direction | Secrets/PII must not leak through logs, Blocks, diagnostics, sync or agent context. |
| F-224 | No account for local use | Accepted direction | Excellent local terminal and local agent workflows require no Seyal account. |
| F-225 | Signed updates | Accepted direction | Signed/notarized updates and safe rollback are release-security requirements. |
| F-226 | macOS native windowing | Foundation exists | Native Swift/AppKit/Metal macOS application boundary exists. |
| F-227 | Linux UI | Accepted direction | Linux later, reusing portable Rust semantics but proving platform-specific UI/renderer gates. |
| F-228 | Windows UI | Accepted direction | Windows later with the same portable core and native platform layer. |
| F-229 | Accessibility | Accepted direction | Keyboard accessibility, labels, focus order and VoiceOver are product requirements. |
| F-230 | Auto-update pill | Accepted direction | Native in-app update UX may sit above signed update mechanism. |
| F-231 | Multi-OS shared core | Foundation exists | Portable Rust core + native-per-platform UI principle is established; macOS is first. |

### 5.10 — Development / IDE boundary

| ID | Historical capability | Current Seyal disposition | Seyal mapping / constraint |
|---|---|---|---|
| F-240 | Focused code editor | Accepted direction | Bounded first-class editor supports normal editing, selection/search, syntax highlighting, formatting, exact path/line navigation, AI autocomplete/diff workflows and optional Vim-style editing; it must remain additive and resource-bounded rather than becoming full IDE authority. |
| F-241 | Optional/lazy language servers (LSP) | Accepted direction | Editor may use opt-in/lazy LSP diagnostics, navigation, completion and formatting; no always-resident IDE-scale language infrastructure and no dependency from terminal hot paths. |
| F-242 | Find and replace | Accepted direction | Workspace/file search fits; destructive project-wide replace needs an explicit undo/effect model. |
| F-243 | Code review panel | Accepted direction | Working-tree/PR diff and code-review inspector is a useful cold surface. |
| F-244 | Interactive review comments | Accepted direction | Structured review comments may steer an agent/run without turning Seyal into an editor. |
| F-245 | Open in external IDE | Accepted direction | Open exact path/line in configured external IDE/editor. |
| F-246 | Zero-state open / clone | Accepted direction | Zero-state create/open/clone project/workspace onboarding. |
| F-247 | Local codebase index | Accepted direction | Local codebase index is part of the Local Context Engine, with provenance and invalidation. |
| F-248 | Local notebooks | Accepted direction | Local runnable notebooks/runbooks as user-owned files; cloud not required. |
| F-249 | Browser DevTools / design | Deferred / decision required | Browser DevTools/design depends on a future accepted embedded-browser surface. |
| F-250 | Debugger / DAP | Rejected | No built-in debugger/DAP client as core product without a future explicit ADR. |

## Non-negotiable product decisions exposed by the registry

- Terminal fundamentals remain OSS and use the Seyal-owned PTY → VT/state → damage → Metal stack; no legacy libghostty production engine is revived.
- Blocks, raw terminal and TUI are presentations of the same TerminalExecution and canonical TerminalState. No second PTY/grid is created for Blocks.
- Seyal supports a bounded first-class editor, optional/lazy LSP, focused SCM/preview surfaces and agent interaction as additive capabilities; none may become terminal authority or require IDE-scale always-resident infrastructure. Debugger/DAP ownership remains a separate explicit decision.
- GUI close/detach is distinct from terminating a process. Persistence metadata never claims to resurrect a dead PTY.
- Agent and workflow features are additive. Terminal input/output/rendering never synchronously wait on agent, semantic, persistence, editor/LSP, SCM, preview, cloud or telemetry work.
- Raw terminal text is untrusted. It may support low-confidence detection/notifications but cannot become permission, approval, policy or audit authority.
- Stable navigation is the default. Attention uses badges and the global Attention Stack rather than constantly reordering the user's workspace list.
- Local use, local configuration and useful local agent/workflow capabilities do not require a Seyal account.
- Explicit Context Privacy Scope is stronger than pattern-based secret redaction: excluded resources must stay out of agent retrieval and derived context unless policy/user action permits them.

## Legacy evidence status

The one-time legacy import is complete: **511/511** RILL/terminal source issues were reconciled into Seyal as `historical-evidence`, with source-specific legacy labels and exact source markers. Their destination issue state preserves historical evidence only; it is not current implementation status.

The temporary export/import workflow, migration package and local helper scripts were consolidation tools and are intentionally not part of the product repository after this PR. Do not document or depend on those removed scripts as an ongoing workflow.

Imported Issues/comments do not preserve Git commit objects, PR diffs/reviews, releases or every repository artifact. Do not delete the legacy repositories solely because issue migration completed; archive full repository/PR history separately before any retirement decision.

`terminal` also contains implementation regressions, tests and obsolete architecture experiments. Those remain historical evidence unless they reveal a distinct current product behavior captured in this registry or its current backlog owner.

## Current backlog tracking

`docs/product/CURRENT_BACKLOG.md` and GitHub issue #650 are the current implementation-tracking entry points. Unfinished accepted/foundation/deferred capabilities remain open there until current Seyal evidence closes them. Rejected, superseded and historical-only shapes stay closed and map to their replacement/decision where applicable.

## Inventory maintenance rule

1. Add/update this file when a product capability or rejection is accepted.
2. Link/refine the owning R&D/ADR/spec in the same change when implementation behavior is being fixed.
3. Never mark a feature Implemented from a legacy issue state or a design document.
4. Keep competitive analysis, positioning and private product rationale outside this repository; record only the OSS product contract/disposition here.
5. New features must state whether they alter terminal hot paths, trust boundaries, persistence semantics, repository isolation or public extension seams.
6. Do not create a second competing product feature list; source-disposition/audit files may exist only as traceability ledgers pointing back here.

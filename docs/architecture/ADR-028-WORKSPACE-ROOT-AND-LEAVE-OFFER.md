# ADR-028 — Workspace root path attribute and leave-parent create/jump offer

- **Status:** Proposed; not implementation authority until accepted by a non-author maintainer.
- **Date:** 2026-11-01
- **Issue:** #1409
- **Scope:** Bound workspace root path as a workspace attribute (display + leave policy); user-confirmed create-or-jump when trusted foreground cwd leaves that root; reconcile with opaque `WorkspaceId` and deferred trusted CWD.
- **Depends on:** Accepted ADR-007, ADR-015, ADR-017, ADR-018, ADR-019, ADR-020; Accepted SPEC-003; UI `M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §5.1 / §5.5.1.
- **Coordinates with:** Proposed ADR-023 (#832) for nonzero WorkspaceId minting/persistence; #686 / ADR-009 trusted live CWD follow-on (leave detection must not fire on OSC 7 alone); legacy evidence #181 / #178 / #423 (historical only).
- **Numbering:** ADR-028. ADR-027 remains reserved for #1385 companion install consent and must not be reused. Rechecked 2026-11-01 against the architecture index.

## 1. Context

Product UX requires Workspaces to bind a real **location** (root path) when a workspace is set up, and when the shell’s parent directory leaves that root, to ask the user whether to **create** a workspace for the new path or **jump** to an existing one. **Yes** performs create or activate. **No** means navigation only: the shell continues; Seyal must not move PTYs and must not silently invent a workspace.

Accepted ADR-007 forbids deriving `WorkspaceId` from cwd, repo path, window, or provider session. Accepted ADR-017 / ADR-020 classify OSC 7 and other terminal-emitted path text as untrusted display metadata that must never become spawn, filesystem, or workspace authority. Trusted live CWD remains deferred under #686. M003 Runtime currently admits only `workspace_id == 0`; there is no production `CreateWorkspace` path.

Legacy RILL/terminal issues (#181, #178, #423) describe directory-identity and leave-prompt behavior. They are historical evidence only and do not authorize copying cwd-as-identity into Seyal.

## 2. Decision

### 2.1 Opaque identity + bound root attribute

1. `WorkspaceId` remains an opaque durable domain identity. It is never derived from cwd, repository path, OSC text, window, tab, pane, or display label (ADR-007 restated).
2. A Workspace record may carry a bound **root path** attribute used for:
   - helpful left-rail secondary detail (`Workspace.detail` / path when helpful);
   - leave-parent detection policy once a trusted CWD signal exists.
3. The root path is an attribute of the workspace, not its identity. Two workspaces must not be considered the same solely because their roots string-equal; Runtime-minted `WorkspaceId` remains authoritative.
4. When multi-workspace creation is authorized, Runtime mints nonzero `WorkspaceId` values (ADR-023 direction). The client never mints WorkspaceIds.

### 2.2 Create and jump

1. `CreateWorkspace` (name TBD in implementation Issue) creates a new Workspace with an explicit root path chosen by a typed product action (picker, confirmed leave offer, or equivalent). It does not move existing PTYs into the new workspace as a side effect of create.
2. `ActivateWorkspace` / navigate-to-workspace (existing shell direction) jumps to an existing Workspace whose bound root matches the offer path under the ADR’s matching rules (exact normalized path equality unless a later Accepted amendment defines worktree-aware grouping).
3. Matching for “jump if one already exists” uses the bound root attribute inventory owned by Rust product state, never a Swift-side path index.

### 2.3 Leave-parent offer

When **trusted** foreground cwd for the focused interactive execution leaves the active Workspace’s bound root (path is no longer equal to or under that root per the Accepted matching rule):

1. Rust product state may raise a typed leave offer: **create** a Workspace for the new path, or **jump** if one with that bound root already exists.
2. **Yes** → perform create or activate as appropriate through Rust-owned actions.
3. **No** → dismiss the offer; shell navigation continues; no PTY move, kill, rebind, or silent workspace create.
4. Optional sticky deny (“don’t ask again”) is a Rust-owned configuration/policy concern if introduced; Swift must not invent host-local prefs as product authority (ADR-015).
5. Leave detection **must not** treat OSC 7, Pane titles, prompt text, or other untrusted terminal path text as authority. Those signals remain display-only until an Accepted trusted CWD decision (#686 follow-on) amends ADR-009 / ADR-020 and this ADR’s dependency is satisfied.
6. Until trusted CWD is Accepted, this ADR authorizes the data model and offer semantics only; production leave detection must not ship on untrusted signals.

### 2.4 Presentation

- Workspaces mode shows name + helpful root path + tab count per existing C03/C04 contracts.
- Offer UI is thin host chrome over Rust offer state; the host cannot invent workspace rows or choose ExecutionIds.

## 3. Alternatives considered

1. **Derive WorkspaceId from directory path.** Rejected by ADR-007.
2. **Auto-create/switch without prompt.** Rejected: silent product mutation; contradicts legacy deny-sticky evidence and user control.
3. **Use OSC 7 as leave/create authority.** Rejected by ADR-017 / ADR-020; untrusted program output must not steer workspace mutation.
4. **Move PTYs into the new/jumped workspace on Yes.** Rejected for the initial decision: create/jump changes workspace association for new work and activation of existing workspace presentation; it does not relocate live PTY ownership as a side effect of the offer. Any future adopt/move semantics require a separate Accepted decision.
5. **Defer naming this decision until M004 persistence alone.** Rejected as a substitute for this ADR: UI and leave UX still need an Accepted identity/attribute/offer contract before Ready implementation Issues.

## 4. Consequences and implementation gates

- Substantial implementation is stopped until this ADR is Accepted by a non-author maintainer.
- Follow-on Ready Issues after acceptance:
  1. CreateWorkspace + root binding so Workspaces mode shows real locations;
  2. Leave-parent prompt UI/actions — **blocked** until trusted CWD is Accepted.
- ADR-023 acceptance (or an interim Accepted Runtime minting rule) is required before nonzero WorkspaceIds are legal on the wire.
- UI CORE-TERMINAL-REFERENCE workspace detail remains “path when helpful”; once roots are bound, redundant name/path suppression stays a presentation rule, not identity.
- No production Rust/Swift may ship leave detection on OSC 7 under this ADR.

## 5. Acceptance

This ADR becomes Accepted only when its Architecture PR receives independent non-author maintainer approval and is merged under #1409. Until merge, it remains a proposal and authorizes no implementation.

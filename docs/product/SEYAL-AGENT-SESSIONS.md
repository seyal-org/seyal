# Seyal Agent Sessions

**Status:** Accepted product terminology refinement  
**Scope:** Seyal OSS product behavior and stable integration seams  
**Owning issue:** #744  
**Related authority:** `docs/product/FEATURES.md`, `docs/product/AGENT-EXECUTION-WORKFLOW-REFINEMENT.md`, `docs/architecture/agent-rd/SEYAL-AGENT-DOMAIN-MODEL-RD-001.md`, `docs/architecture/agent-rd/SEYAL-HARNESS-PROTOCOL-RD-001.md`

## Purpose

Seyal has two distinct concepts that must never be conflated:

```text
Seyal Agent Sessions
= understand and manage agent sessions running in the execution workspace

Seyal AI Agent
= Seyal's separate first-party AI agent product/composition
```

This document defines **Seyal Agent Sessions** only. It refines the existing OSS capabilities around `SY-006`, `F-011`, `F-037` and the agent lifecycle/Attention contracts; it does not create a second agent domain model or authorize implementation ahead of the owning milestone.

## Product contract

A user may install and run coding/operations agents as ordinary terminal programs, including examples such as:

- Claude Code;
- Codex CLI (first-party OSS adapter on `StandaloneProcessHost` — see `docs/engineering/CODEX-ADAPTER.md`);
- Cursor Agent CLI;
- OpenCode;
- Gemini CLI;
- future or unknown agent CLIs.

Running an agent never requires Seyal to own that agent's model, reasoning loop, account or subscription.

### Unsupported or unknown agents

An unknown agent remains a normal terminal/TUI workload and must work with full terminal correctness.

```text
unknown agent CLI
    ↓
TerminalExecution
    ↓
PTY → VT → TerminalState → renderer
```

No adapter, semantic detection or cloud service is required for basic execution.

### Supported agents

When Seyal has reliable integration evidence, it may add a richer projection through the existing provider-neutral model:

```text
external agent process / harness session
              │
              ├─ TerminalExecution remains terminal authority
              │
              └─ HarnessAdapter / hooks / signals
                         ↓
                      AgentRun
                         │
                 HarnessSessionRef
                 ExecutionRef(s)
                 RunEvent(s)
                 Artifact(s)
                 CostEvent(s)
                 AttentionItem(s)
```

**Local first-party adapter (M005):** Claude Code is the first supported local CLI
adapter on `StandaloneProcessHost` (pipe-safe stream-json). Install/enable is a
trusted `admin.adapters` path; repository content never auto-enables it. See
`docs/engineering/CLAUDE-CODE-ADAPTER.md`. Codex remains a sibling adapter.

The upstream session/thread identifier remains an opaque adapter-scoped reference. It never replaces Seyal identity or becomes terminal authority.

## Agent Sessions experience

Subject to capability evidence and the owning milestone, Seyal Agent Sessions may provide:

- detection and registration of running agent sessions;
- lifecycle/status such as working, waiting, needs attention, needs review, failed and completed;
- workspace/session grouping and stable navigation;
- approvals, questions, failures and completion through the canonical Attention model;
- local notifications derived from Attention;
- resume/reconnect when the upstream harness explicitly supports it;
- changed-files, worktree, branch, artifact, diff or pull-request metadata when supported;
- provider-reported token/cache/cost metadata when supported;
- bounded session history and provider-session references under retention/privacy policy;
- capability/confidence provenance so heuristics are never presented as authoritative structured facts.

Feature absence is capability absence, not an excuse to fake provider behavior.

## Detection confidence

The existing tiered detection order remains:

```text
structured adapter
    > official hooks/events
    > trusted process/shell signals
    > low-confidence terminal heuristics
```

Low-confidence terminal heuristics may improve presentation only. They may not become authentication, authorization, approval, audit or billing truth.

## Relationship to Seyal AI Agent

The separate first-party **Seyal AI Agent** may use the same canonical `WorkItem` / `Attempt` / `AgentRun` / `Attention` / execution model and therefore appear in the same Agent Sessions UI.

```text
Claude Code ───┐
Codex ─────────┤
Cursor Agent ──┤
OpenCode ──────┼──> Seyal Agent Sessions UX
Seyal AI Agent ┘        │
                         └─ same canonical AgentRun/session projection
```

This does **not** make external agents part of the Seyal AI Agent product, and it does not give the Seyal AI Agent a competing session model.

## OSS/commercial boundary

Seyal Agent Sessions is an OSS workspace capability. It must remain useful without:

- a Seyal AI subscription;
- managed inference or bundled model credits;
- a Seyal account;
- hosted/cloud agents;
- proprietary routing;
- a Seyal-trained model;
- commercial entitlement checks.

OSS may own generic local primitives and capability seams that are independently useful to external agents and OSS consumers, including `AgentRun` identity, harness capability protocols, Attention integration, local context/evaluation/routing/workflow primitives and terminal-safe control seams.

First-party subscription packaging, managed inference, proprietary first-party agent policy, hosted execution, learned/managed routing, commercial entitlements and future proprietary Seyal-model IP belong above the OSS boundary in `seyal-commercial` when justified by commercial milestones.

## Performance and authority invariants

Seyal Agent Sessions is an additive control/presentation plane.

It must never:

- create a second PTY for an already-running agent merely to represent it;
- own or duplicate VT/grid/TerminalState;
- synchronously gate PTY input/output or rendering;
- require agent recognition for terminal correctness;
- infer an approval or security decision from raw terminal text;
- treat an upstream provider session ID as Seyal's durable identity.

## Non-goals

This document does not define:

- the Seyal AI Agent subscription or pricing model;
- managed model-provider contracts;
- proprietary harness policy;
- cloud worker architecture;
- model training;
- provider-specific feature promises.

Those belong to their owning commercial/product R&D and implementation gates.
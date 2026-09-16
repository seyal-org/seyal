# Seyal Agent Sessions

**Status:** Accepted product terminology refinement  
**Scope:** Seyal OSS product behavior and stable integration seams  
**Owning issue:** #744  
**Related authority:** `docs/product/FEATURES.md`, `docs/product/AGENT-EXECUTION-WORKFLOW-REFINEMENT.md`, `docs/architecture/agent-rd/SEYAL-AGENT-DOMAIN-MODEL-RD-001.md`, `docs/architecture/agent-rd/SEYAL-HARNESS-PROTOCOL-RD-001.md`

## Purpose

**Seyal Agent Sessions** means understanding and managing agent sessions running in the execution workspace without making those agents terminal authority.

This document defines only the public OSS session-management capability. It refines existing capabilities around `SY-006`, `F-011`, `F-037` and the agent lifecycle/Attention contracts; it does not create a second agent domain model or authorize implementation ahead of the owning milestone.

## Product contract

A user may install and run coding/operations agents as ordinary terminal programs, including examples such as:

- Claude Code;
- Codex CLI;
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

Low-confidence terminal heuristics may improve presentation only. They may not become authentication, authorization, approval, audit or cost truth.

## OSS independence

Seyal Agent Sessions must remain useful with ordinary locally installed agents and without requiring a Seyal-hosted service.

OSS may own generic local primitives and capability seams that are independently useful to external agents and OSS consumers, including `AgentRun` identity, harness capability protocols, Attention integration, local context/evaluation/routing/workflow primitives and terminal-safe control seams.

Provider/service implementation details outside the public repository are not part of this product contract.

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

- provider pricing or account terms;
- private service architecture;
- model training;
- provider-specific feature promises.

Those concerns are outside this OSS product contract.

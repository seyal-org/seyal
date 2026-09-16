# Seyal OSS Product Requirements

This document defines the durable product requirements for the **open-source Seyal repository**.

`AGENTS.md` remains higher authority for non-negotiable engineering principles. Accepted architecture/ADRs define structural decisions. Specifications define narrower observable behavior. Milestones define delivery slices and acceptance gates.

## Product problem

Modern development and operations work is fragmented across terminals, multiplexers, SSH sessions, coding agents, logs, editors and operational tools. At the same time, many terminal products still treat every workflow as an undifferentiated stream of characters.

Seyal OSS should provide a terminal workspace where **humans, shells, terminal applications and agents can work through one trustworthy execution foundation** without sacrificing terminal correctness, native performance or user control.

## Primary users

- software developers who live in shells, editors, CLIs and coding-agent tools;
- platform/SRE/operations engineers working across local and remote environments;
- advanced terminal users who need durable local execution and structured command history;
- contributors building or validating the terminal/runtime foundation.

## Product promise

A strong Seyal OSS release should make these statements true:

1. It is an excellent terminal even if the user never enables an agent feature.
2. Blocks and workspace structure improve command-line work without breaking normal shell/TUI semantics.
3. Closing the GUI does not have to mean terminating the work running underneath it.
4. Agent workflows can integrate with terminal execution without owning the PTY, VT, renderer or canonical terminal state.
5. The product remains fast and resource-efficient because performance constraints are architectural, not late optimization work.

## Core experiences

### Excellent terminal

Seyal OSS must ultimately handle real terminal workloads such as shells, SSH/nested SSH, Vim/Neovim, tmux as a child, ncurses/TUI applications, developer/operations CLIs, long-running commands and high-volume logs.

Correctness takes priority over cosmetic shortcuts.

### Native Blocks

Blocks represent real terminal execution and command/history structure.

A Block must not create:

- another PTY;
- another VT/state engine;
- another process merely for presentation;
- a copied transcript that becomes a second authority.

### Persistent local execution

GUI lifetime and execution lifetime are separate concerns.

The local Runtime may own executions beyond the lifetime of one GUI attachment, allowing detach/reconnect behavior according to accepted lifecycle contracts.

Live PTY survival across Runtime crash, durable history, crash recovery and reboot recovery remain distinct capabilities and must never be conflated.

### Agent-native local foundation

External coding-agent CLIs and future first-party agent workflows may consume Seyal execution/workspace capabilities.

Agents remain additive. Terminal input/output/rendering must never synchronously depend on an agent, model provider, semantic extractor, cloud service or telemetry path.

### Native high-performance UI

Portable product/UI state and behavior belong in Rust according to accepted architecture.

Platform hosts own only the native integration needed for the active platform. macOS uses AppKit/Metal; future platforms are introduced when they are actually developed rather than through a speculative cross-platform GUI abstraction.

## Repository scope

As introduced by accepted milestones, this repository owns generic capabilities required for a strong local terminal workspace, including:

- PTY/process lifecycle;
- Seyal-owned VT parser/state machine;
- authoritative terminal grid/history/damage semantics;
- Unicode/grapheme/width behavior;
- local Runtime/execution ownership;
- attach/detach/reconnect foundations;
- generic local Workspace/Block behavior;
- rendering architecture and native client foundations;
- public protocols or extension seams justified by real OSS use cases;
- tests, fixtures, fuzzing and performance/security evidence required to trust those capabilities.

## Product invariants

- Seyal owns its production terminal semantics rather than delegating VT authority to another terminal engine.
- One concern has one authoritative state owner.
- Rust owns portable terminal/runtime/product behavior; native code stays at the smallest justified platform boundary.
- Terminal hot paths do not synchronously depend on Blocks metadata, agents, persistence, cloud or telemetry.
- Avoid unnecessary IPC, serialization, JSON, copies, allocations, locks and language round-trips on canonical progress paths.
- Do not introduce temporary production terminal/rendering architectures intended to be replaced later.
- Do not claim persisted text/events can restore a live PTY/process after that PTY/process no longer exists.
- Do not create speculative platform/framework/package abstractions without an active requirement.

## Quality bar

A capability is not complete because it worked once.

For the layer being changed, completion requires the appropriate combination of:

```text
working
+ tested
+ demonstrable
+ benchmarked where relevant
+ security/failure reviewed where relevant
```

Terminal foundation work additionally requires regression resistance through conformance fixtures, fuzzing, resource bounds and real workload validation where applicable.

## Current status

Milestone 001 established the first production-shaped local execution foundation on macOS:

```text
Runtime
→ TerminalExecution
→ real PTY
→ shell/application
→ Seyal-owned VT / TerminalState
→ damage / derived projection
→ native client
→ Metal
→ pixels
```

It also proved the architectural separation between GUI lifetime and Runtime-owned execution lifetime.

Later milestones continue to extend correctness, history, Unicode, product presentation and other capabilities. The repository remains pre-release until release criteria say otherwise.

## Documentation map

```text
PRODUCT.md
  durable OSS product requirements

accepted architecture / ADR
  significant decisions, ownership and rationale

specification
  bounded observable/enforceable behavior

milestone
  delivery slice and acceptance gates

Ready Issue
  independently implementable work
```

Do not create a parallel `PRD.md` or another product-requirements file for the same OSS scope. Update this canonical owner when durable OSS product requirements change.

# Seyal Foundation Architecture R&D — Decision Package

**Document:** SEYAL-ARCH-FOUNDATION-RD-001  
**Date:** 2026-08-23  
**Status:** Accepted  
**Scope:** Foundation architecture only. No production implementation is authorized by this document beyond the milestone definitions.

**Companion rationale:** [`rationale/SEYAL-ARCH-FOUNDATION-RATIONALE-001.md`](rationale/SEYAL-ARCH-FOUNDATION-RATIONALE-001.md)

> Normative decisions and prohibitions use rationale IDs (`R-xxx`). The rationale record explains why each rule exists, the failure it prevents, and what evidence is required to reopen it.

---

## 1. Executive decision

1. Seyal uses one persistent, headless-capable **per-user Seyal Runtime** as the authority for local live terminal executions. (`R-001`, `R-004`)
2. Each `TerminalExecution` owns exactly one terminal endpoint (POSIX PTY on macOS/Linux; future ConPTY adapter on Windows), child lifecycle, and one canonical Seyal `TerminalState` containing VT/parser modes, primary/alternate screens, logical history, reflow and damage. Block **timeline** authority is separate Runtime/workspace metadata keyed by `ExecutionId`; accepted native-Block RegionGrids live inside that one `TerminalState` (`PrimaryBlockEngine`). (`R-005`, `R-006`, `R-029`, ADR-009)
3. The GUI never owns or mirrors a second authoritative VT or grid. (`R-002`)
4. The local desktop path must avoid synchronous IPC round trips / cross-process ping-pong on input, PTY output, VT mutation, damage, and presentation. (`R-003`)
5. Local clients use compact binary control/input plus a platform-local derived display projection; remote/cloud clients use compact binary snapshot/delta streams derived from the same canonical state. (`R-003`, `R-031`)
6. No daemon, process, I/O thread, Lua VM, glyph atlas, or renderer stack is created per pane. Runtime concurrency scales by measured load, while renderer resources scale mainly with visible surfaces. (`R-004`, `R-025`)
7. **Headless decision: IMPLEMENT FROM M001.** (`R-001`)
8. Blocks are Runtime timeline metadata plus canonical RegionGrid content inside one VT authority; a Block never implies another PTY, parser, VT engine, process, or transcript copy. (`R-006`, ADR-009)
9. Alternate-screen/TUI applications remain real terminal applications using the same canonical `TerminalState`. (`R-007`)
10. Agents and future multi-agent orchestration interact through stable capabilities/events and never own PTYs, terminal state, scrollback, or rendering. (`R-008`, `R-009`)
11. The ADR-015 target keeps Rust authoritative for portable terminal/runtime logic **and** portable product/UI state and behavior; Swift + AppKit remains the thin in-repo host for inherently macOS-specific windowing, native event collection, IME, accessibility, clipboard/drag-drop, appearance/accessibility observation, native color/font/material realization, Metal and CoreText integration, bundled-helper launch, and bounded disposable native view/resource/cache state. (`R-010`, ADR-015)
12. The portable Rust core is designed now for Linux, Windows, cloud, and embedding, but there is no premature universal GUI abstraction. (`R-012`, `R-013`)
13. iOS/Android are future first-class **remote controller/viewer clients** for terminals running on a user machine or in Seyal Cloud; mobile does not become terminal-state authority. (`R-014`, `R-031`)
14. TOML is the canonical static configuration format. Lua provides programmable customization only through cold typed config overlays and asynchronous typed actions; Lua never joins terminal hot paths. (`R-017`–`R-020`)
15. **Verdict: READY TO IMPLEMENT MILESTONE 001.**

---

## 2. Product architecture

Seyal is an execution workspace, not only a terminal emulator:

```text
human + shell + agent + infrastructure
                │
                ▼
        Seyal workspace model
                │
        ┌───────┼────────┐
        │       │        │
   terminal   agent   artifact/
 execution    task    operation
```

Terminal correctness remains foundational. Agent, collaboration, cloud, persistence, enterprise policy, and UI capabilities must be additive and may not synchronously gate PTY I/O, VT parsing, terminal-state mutation, shaping, or rendering.

---

## 3. Selected authority model

### 3.1 Canonical ownership

```text
Seyal Runtime
  ├─ Execution registry
  │    └─ TerminalExecution
  │         ├─ TerminalEndpoint
  │         │    ├─ PTY / ConPTY
  │         │    └─ child process/job
  │         └─ TerminalState                  ← authoritative
  │              ├─ VT parser/modes
  │              ├─ primary screen
  │              ├─ alternate screen
  │              ├─ Unicode/grapheme/width
  │              ├─ logical scrollback
  │              ├─ reflow
  │              └─ damage generations
  │
  └─ workspace/runtime metadata
       ├─ BlockTimeline
       │    └─ references ExecutionId + logical history anchors
       └─ semantic/event metadata
                │
                ├─ derived local display projection
                ├─ remote snapshot/delta projection
                ├─ persistence/cold-history consumers
                └─ agent/orchestrator observers
```

There is one authoritative VT/state machine per terminal execution.

`BlockTimeline` is authoritative Runtime/workspace metadata, not terminal infrastructure. A `TerminalExecution` may emit bounded asynchronous execution/history signals consumed by Block metadata, but PTY → VT → `TerminalState` → damage progress never waits for Block mutation or semantic processing.

The app renderer receives only derived presentation data. A renderer projection may be rebuilt at any time from canonical runtime state and therefore cannot become a second terminal authority.

### 3.2 Why runtime authority

Runtime authority is required because:

```text
close GUI
→ terminal process must continue

GUI crash
→ terminal process must continue

headless runtime
→ must behave like the same terminal engine

mobile/cloud attach
→ must observe the same terminal truth
```

A GUI-authoritative VT would require moving or reconstructing terminal ownership later. A GUI VT mirror would create competing state models and reconnect ambiguity.

---

## 4. Process and concurrency model

### 4.1 Local runtime

Default architecture:

```text
one logged-in user
  └─ one Seyal Runtime
       ├─ N TerminalExecution objects
       ├─ bounded PTY I/O shards
       ├─ bounded background work pools
       ├─ shared caches/indexes
       └─ client attachment manager
```

`N terminals` must not imply `N daemons`, `N renderer stacks`, or `N Lua runtimes`.

Independent live shells still require independent terminal endpoints and child processes. Seyal optimizes the layers around those unavoidable OS/process resources.

### 4.2 Scaling target

The design must remain practical for hundreds of panes spread across windows/tabs/workspaces.

Initial architectural budgets:

- hidden/detached idle terminal: target <= 256 KiB Seyal-owned hot resident memory before scrollback payload;
- 100 hidden idle terminals: target <= 25 MiB Seyal-owned execution overhead;
- 500 hidden idle terminals: target <= 125 MiB Seyal-owned execution overhead;
- thread count must not scale linearly with pane count;
- hidden/detached executions should hold no dedicated GPU render surface;
- glyph/font caches are shared across visible terminal surfaces where technically safe;
- scrollback is bounded/adaptive and may spill cold chunks without affecting live terminal semantics.

These are measurement targets, not excuses to reduce correctness.

### 4.3 Runtime crash survival

M001 guarantees GUI detach/crash survival because the Runtime owns the PTY.

It does **not** yet guarantee:

```text
Seyal Runtime crash
→ arbitrary live shell survives
```

A journal cannot resurrect a PTY. A future PTY keeper or worker/sharding design may be justified only by measured reliability/security requirements and must be evaluated against memory/context-switch/latency costs.

---

## 5. Hot-path architecture

### 5.1 Input

```text
NSEvent
→ Swift/AppKit input normalization
→ coarse native/Rust boundary
→ compact input/control queue
→ Seyal Runtime
→ canonical terminal mode/key encoding
→ PTY write
→ child
```

Rules:

- no synchronous GUI↔runtime acknowledgement is required before subsequent input;
- GUI must not encode terminal keys from stale mode state when correctness depends on canonical modes;
- config/Lua/agents/licensing/network/persistence never execute synchronously here;
- keybinding lookup is precompiled/cold, not parsed per key event.

### 5.2 Output

```text
PTY read
→ runtime I/O shard
→ Seyal VT parser/state machine
→ canonical TerminalState mutation
→ damage generation
→ derived display projection
→ one-way wake/signal
→ render preparation
→ Metal
→ pixels
```

No JSON, cell-by-cell FFI callback, terminal transcript serialization, or synchronous renderer acknowledgement belongs in this path.

### 5.3 Apple silicon local path

Apple silicon unified memory reduces CPU/GPU copy pressure compared with discrete-memory architectures, but it does **not** make process boundaries, scheduling, synchronization, or serialization free.

Therefore the Seyal rule is:

> Preserve the process boundary needed for persistent execution, but eliminate avoidable synchronous round trips, serialization, copies, and process/thread hops around it.

### 5.4 Resize

Resize is one logical Runtime transaction across two physical state holders: the kernel terminal endpoint geometry and the canonical `TerminalState`. `TerminalState` remains the semantic/presentation authority; physical syscall commit order does not create a second authority.

```text
pane/window geometry
→ client proposes rows/cols
→ controller authority validates resize ownership + geometry
→ runtime prepares all locally rejectable/infallible resize inputs
→ apply fallible PTY/ConPTY winsize
→ commit canonical TerminalState resize + reflow
→ new damage generation
→ renderer projection
```

The Runtime must not publish/reflow a new canonical geometry that the terminal endpoint rejected. Conversely, after the endpoint accepts the size, canonical commit must not be allowed to fail silently and leave endpoint/TerminalState divergence. If future `TerminalState::resize` gains a recoverable failure after validation, resize must use an explicit prepare/commit/rollback transaction before retaining endpoint-first physical commit order.

No renderer/client may observe the new geometry until canonical `TerminalState` commit completes.

Multi-client attach requires explicit resize authority. It must never degenerate into uncontrolled “last client packet wins.”

---

## 6. Blocks, history, scroll and TUI

### 6.1 Block model

Block **timeline** authority (IDs, lifecycle, trust/scope metadata) lives in
`seyal-workspace` / Runtime workspace metadata keyed by stable `ExecutionId`.
Accepted native-Block target (ADR-009 / #1480 / #1482): canonical prompt/command
and output **RegionGrids** live inside the one `TerminalState` `PrimaryBlockEngine`
through one `RegionStore` and non-owning `VTWindowMap`. A Block never owns an
independent PTY, parser, VT semantic engine, child, renderer, or copied transcript.

Typical Block fields may include:

```text
BlockId
ExecutionId
kind
command metadata
start/end logical line anchors
status / exit code
timestamps
cwd / shell-integration metadata
agent/activity association
presentation state
```

A Block must not own:

```text
independent PTY / child process
independent VT parser or mode/cursor engine
independent alternate-screen engine
independent renderer
private copied transcript / second writable primary grid
```

A Block **may** own RegionGrid content identity (prompt/command and output regions)
inside the single `PrimaryBlockEngine` / `RegionStore`. Block timeline metadata
observes lifecycle asynchronously. Terminal progress must not synchronously depend
on Block presentation, IPC, or semantic extraction.

### 6.2 Current mutable Block

A shell-integrated execution may have one current mutable terminal Block representing
the current command/prompt lifecycle. Completed Blocks retain RegionGrid content
through the shared HistoryStore once sealed under ADR-010 rules; lifecycle metadata
remains Runtime-owned. History-range-only cards are the legacy shipping model until
the single-owner native-Block cutover.

When shell integration is missing, Seyal still behaves as a correct raw terminal; Block boundaries degrade gracefully and may use coarse session/activity boundaries instead of guessed command truth.

### 6.3 History and virtualization

Canonical terminal history uses stable logical line identity independent of the currently visible renderer rows.

```text
Terminal history
→ stable LineId/chunk identity
→ Block line anchors
→ lightweight Block skeleton/height index
→ virtualized viewport
```

The Block skeleton is an index/layout aid, not copied terminal content.

Huge history must be bounded/adaptive. Cold history may be compressed/spilled/paged while the hot viewport remains inexpensive.

### 6.4 Live TUI / alternate screen

When the terminal enters alternate screen:

```text
same TerminalExecution
same PTY
same VT
same authoritative TerminalState
→ raw/full-pane TUI presentation
```

The Block model does not interrupt or emulate Vim, Neovim, htop, watch, tmux-as-child, Claude Code, Codex, Cursor CLI, OpenCode, or ncurses applications.

Seyal does not record every alternate-screen frame by default. Doing so would create unbounded memory, privacy, and persistence costs.

---

## 7. Workspace and presentation object model

Runtime identities and presentation identities are separate.

```text
Workspace
  ├─ Window
  │    ├─ Tab
  │    │    └─ Split tree / PaneView
  │    └─ Inspector / utility presentation
  ├─ Execution registry
  │    ├─ TerminalExecution
  │    └─ NonTerminalExecution / AgentTask
  ├─ Agent activity
  ├─ Artifact registry
  └─ Attention model
```

A window/tab/pane is a **view of an execution**, not the process owner.

This permits:

- one execution to survive all windows closing;
- remote/mobile clients to attach to the same execution;
- an execution to be moved/re-presented without changing its PTY;
- inspectors, diffs, artifacts and agent views that require no PTY.

Layout persistence is separate from execution persistence.

---

## 8. Global attention and approvals

Agent questions, approvals, command completions, policy prompts and important operational state use a global structured `AttentionItem` model.

An attention item includes stable identity and a target such as:

```text
workspace
execution
agent/task
artifact/change
approval request
```

The UI may present these in an **Attention Stack / popover** without forcing the user to navigate to a tab.

Typed agent approvals may be answered directly in the stack when the approval protocol supports it.

The stack must **not** fake arbitrary PTY keystrokes. Password prompts, terminal applications requiring spatial interaction, or unknown raw prompts must navigate/focus the exact execution instead.

OS notifications are projections of AttentionItems; they are not the canonical attention state.

---

## 9. Multi-agent orchestration seam

Future orchestration is first-class but outside terminal ownership.

```text
Agent Orchestrator
  ├─ planning
  ├─ smart routing
  ├─ scheduling
  ├─ policy/approval
  └─ coordination
        │ typed capabilities/events
        ▼
Execution Registry
  ├─ TerminalExecution A
  ├─ TerminalExecution B
  ├─ remote/cloud execution
  └─ non-terminal task
```

Invariant:

> An agent is an actor/consumer of executions, never the owner of terminal infrastructure.

Agents may:

- request/create an execution subject to policy;
- send authorized input/actions;
- observe permitted execution and Block events;
- inspect permitted derived terminal content;
- create artifacts/diffs;
- request approvals;
- hand work to another agent;
- route work to local/remote/cloud capabilities.

Agents may never own canonical PTY/ConPTY, VT/grid, scrollback, renderer, window/pane, or terminal damage state.

One execution may have zero, one, or many agents over its lifetime. One agent may coordinate many executions.

---

## 10. Local, remote, mobile and cloud clients

### 10.1 Local desktop

macOS initially; Linux and Windows later.

The desktop client uses the lowest-overhead local attachment mechanism supported by the platform while keeping Runtime authority intact.

### 10.2 Remote attach

Remote attach uses a versioned binary protocol carrying:

- execution/workspace metadata;
- terminal snapshot + generation;
- incremental damage/deltas;
- input/control messages;
- Block/attention/artifact metadata as separate typed channels.

A slow or disconnected remote client must never backpressure PTY→VT processing.

If a client misses a bounded delta window, it requests/resumes from a newer snapshot generation rather than forcing the runtime to retain unbounded per-client history.

### 10.3 Mobile

iOS/Android are remote-controller/viewer clients for Seyal Runtime running:

```text
on the user's Mac/Linux/Windows machine
or
in Seyal Cloud
```

Mobile may support:

- terminal viewing/history/Blocks;
- input/control when control authority is acquired;
- agent approval/question handling;
- attention notifications;
- artifacts/diffs/inspectors;
- workspace/session navigation.

Mobile does not become the canonical terminal runtime merely to satisfy product symmetry.

### 10.4 Cloud

Seyal Cloud uses the same portable execution/VT model, typically on Linux headless workers.

Cloud may introduce supervisor/worker isolation for tenant/security/failure reasons, but worker count must not mechanically equal terminal count.

---

## 11. Rendering architecture

macOS production path:

```text
TerminalState
→ damage
→ renderer projection
→ shaping/font fallback
→ glyph/image cache
→ instance buffers
→ Metal
→ presentation
```

Ownership:

- VT owns terminal semantics, not glyphs;
- renderer owns shaping, fallback resolution, glyph raster/cache policy, emoji/image presentation, scale/pixel conversion, and frame scheduling;
- glyph/font caches are shared where possible;
- renderer work is demand-driven by visible clients;
- hidden executions do not retain full renderer surfaces;
- decorative effects, inspectors and animations have lower scheduling priority than terminal cursor/input/output presentation.

No serialized cell JSON or per-cell Swift callback exists in the hot path.

---

## 12. macOS native-language decision

Choose:

```text
Rust
  → PTY/runtime/VT/history/Blocks/protocol/render preparation
  → Workspace/Tab/Pane, composer, command, focus/navigation, split/layout,
    Agent/Inspector and other portable product/UI behavior

Swift + AppKit (thin host in this repository; no separate Swift UI tree)
  → NSApplication/NSWindow lifecycle
  → native event collection
  → IME / NSTextInputClient bridge
  → accessibility adapter
  → clipboard / drag-drop
  → Metal drawable/surface creation
  → other inherently macOS-specific APIs

small Objective-C++/native bridge where Metal/C++ interop materially requires it
```

ADR-015 is the independent lifecycle for this product-vs-platform split.

Do not build the terminal surface with SwiftUI/NSTextView as a temporary renderer.

SwiftUI may be used later only for non-authoritative, non-hot-path chrome that still does not own product/UI state, and only when it does not compromise input, accessibility, rendering, or performance.

---

## 13. Configuration and Lua customization

### 13.1 TOML remains canonical static config

TOML is required because Seyal needs a deterministic, typed, diffable, portable configuration artifact suitable for dotfiles, backup, migration, enterprise policy composition, and optional future sync.

There is one versioned internal config schema.

Configuration is resolved cold at launch/reload, never walked per frame or per keypress.

### 13.2 Lua extends rather than replaces TOML

Lua supports full programmable customization but only through owned typed APIs.

```text
TOML
→ parsed typed config
→ optional Lua ConfigPatch at cold/reload boundary
→ validation
→ EffectiveConfig
```

Lua may also subscribe to semantic events and emit validated typed actions.

Lua may not synchronously inspect/mutate:

- PTY byte streams;
- VT parser internals;
- screen/grid cells;
- shaping/glyph atlas;
- damage tracking;
- renderer/frame scheduling;
- terminal locks/internal Rust objects.

Lua is lazy, bounded and not instantiated per pane.

The settings UI may show value provenance (`default`, TOML, project policy, Lua patch, managed policy) so customization never creates invisible competing truth.

Production configuration and Lua implementation are **not M001 scope**. M001 may introduce only minimal typed boundaries naturally required by the terminal slice; it must not implement the production Lua VM, final configuration system, config provenance UI, general Lua automation, or unrelated policy composition.

---

## 14. Embedding

Day-1 internal APIs must preserve an eventual opaque versioned ABI seam.

Progressive capability shape:

```text
terminal state capability
        ↓
terminal + render preparation capability
        ↓
headless execution runtime
        ↓
embedded/native consumer
        ↓
full Seyal application
```

Do not create crates solely to mirror this diagram. Boundaries must correspond to real ownership/portability/ABI needs.

Public embedding must never expose Rust/C++ internal structs as ABI contracts.

---

## 15. Minimal layer structure

Initial logical modules/crates should remain small in number:

### `seyal-terminal`

Owns VT parser/state, grid/screens, Unicode width/graphemes, logical history, reflow, damage, terminal input-mode semantics.

- headless: yes
- embeddable: yes
- hot path: yes

### `seyal-exec`

Owns terminal endpoint abstraction, PTY/ConPTY adapters, child lifecycle, execution identity, runtime orchestration primitives, attach/control state.

- headless: yes
- embeddable: yes
- hot path: yes for PTY transport

### `seyal-workspace`

Owns workspace/execution registry, Block timeline metadata, attention metadata and non-terminal execution identities. Must remain independent from native windows.

- headless: yes
- embeddable: yes
- hot path: no except tiny append-only signals

### `seyal-render`

Owns renderer-facing derived projection, shaping/cache preparation and renderer-independent draw data.

- headless: projection tests yes; GPU no
- embeddable: yes
- hot path: render path

### `seyal-protocol`

Owns versioned typed local/remote control, snapshots, deltas, capability messages and wire compatibility.

- headless: yes
- embeddable: yes
- hot path: local control/delta boundary only

### `Seyal.app`

Swift/AppKit/Metal macOS host.

Avoid further crate explosion until a distinct authority/process/ABI/portability boundary is demonstrated.

---

## 16. Ownership matrix

| Resource | Authoritative owner | Lifetime | Process/thread domain |
|---|---|---|---|
| PTY / ConPTY endpoint | `TerminalExecution` | execution | Seyal Runtime I/O shard |
| child process/job | `TerminalExecution` | execution/runtime/machine | OS + Runtime lifecycle |
| VT parser/modes | `TerminalState` | execution | Runtime |
| primary/alternate grid | `TerminalState` | execution | Runtime |
| logical scrollback | `TerminalState` | execution/persistence policy | Runtime + optional cold store |
| Block timeline | `seyal-workspace` / workspace-runtime metadata keyed by `ExecutionId` | execution/history | Runtime cold/semantic plane |
| renderer projection | derived attachment state | client attachment | Runtime writer / client reader |
| glyph/font cache | renderer | app/render device lifetime | client render subsystem |
| native renderer | client | visible surface | client render thread/device |
| workspace/execution registry | Seyal Runtime | runtime/persisted metadata | Runtime |
| window/tab/split/pane view | client UI | client/layout lifetime | App |
| agent metadata | orchestration plane | task/history | Runtime/service async domain |
| AttentionItem | workspace/runtime metadata | until resolved/expired | Runtime + client projections |
| Lua VM | customization service | lazy user/runtime config lifetime | cold/async domain |

---

## 17. Persistence contracts

Keep these separate:

### Detach persistence

```text
close Seyal.app
→ Seyal Runtime remains
→ PTY + child remain
→ reopen app
→ attach to same TerminalExecution
```

**M001 target.**

### GUI crash survival

Same mechanism as detach persistence. **M001 target.**

### Runtime crash recovery

M001 may restore metadata/history but cannot claim arbitrary live process continuity. A PTY keeper/worker solution is a separate future decision.

### Reboot recovery

Live processes are not resurrected. Seyal may restore workspace/layout/history metadata and explicitly restart/reconnect supported workloads.

### Scrollback/history persistence

Cold storage can preserve terminal history/Blocks without pretending it preserves a running PTY.

---

## 18. Security foundation seams

Foundation APIs must preserve explicit boundaries for:

- local socket/client authentication;
- controller vs observer permissions;
- clipboard reads/writes and OSC security;
- OSC hyperlinks/titles/CWD as untrusted input;
- secrets/environment redaction policy;
- SSH credentials and host verification;
- remote/client capability authorization;
- agent execution/approval permissions;
- Lua filesystem/network/process capabilities;
- enterprise policy injection without hot-path licensing checks.

Terminal fundamentals must work without cloud/licensing/telemetry.

---

## 19. Architecture invariants

1. Exactly one authoritative terminal state per execution.
2. A pane/window/tab never owns a PTY by virtue of presentation.
3. Blocks never imply a second PTY/parser/VT engine/process; Block timeline metadata remains Runtime-owned outside the PTY path, while accepted native Block RegionGrids live inside the one `TerminalState` / `PrimaryBlockEngine`.
4. Agents never own terminal infrastructure.
5. Terminal I/O/rendering never synchronously waits for agents, persistence, cloud, analytics, telemetry, licensing or Lua.
6. No synchronous IPC request/response ping-pong belongs in the local terminal hot path.
7. No JSON/display text protocol belongs in the local render path.
8. Runtime thread/process count must not scale one-for-one with panes without benchmark evidence.
9. Hidden executions do not retain dedicated full renderer resources.
10. Slow/remote clients cannot backpressure PTY→VT.
11. Multi-client control/resize authority is explicit.
12. Mobile is a client of runtime authority, not a competing runtime architecture.
13. TOML is canonical static configuration; Lua only produces validated cold patches/actions.
14. No temporary production VT or temporary terminal renderer may be introduced.
15. No premature universal GUI abstraction before another GUI platform is actively being developed.
16. Journaling/history is not live-PTY persistence.

---

## 20. Milestone sequence

### M001 — Foundation vertical slice

Goal:

```text
native macOS Seyal
→ one Block identity
→ real PTY
→ real shell
→ Seyal-owned VT
→ canonical terminal state
→ damage
→ Metal
→ pixels
```

The PTY/VT already live in the headless-capable Seyal Runtime.

M001 also establishes:

- attach/detach lifecycle;
- exact execution identity;
- primary + alternate screen path;
- stable logical line identity for Blocks/history;
- local binary input/control boundary;
- derived renderer projection seam;
- benchmark and memory measurement harness.

Production configuration and Lua remain later work; M001 must not expand to implement them.

### M002 — Terminal correctness expansion

Expand the same VT/state engine through conformance tests and real workloads: zsh/bash/fish, Vim/Neovim, ncurses, SSH/nested SSH, tmux-as-child, coding agents, resize/reflow, Unicode, selection/search, high-volume output.

### M003 — Durable Blocks/history + detach UX

Productionize Block virtualization, scroll/history persistence policies, reconnect UX, layout persistence and attention model while keeping all semantics derived from the same execution.

### Later, evidence-driven

Remote/mobile/cloud protocol, public embedding ABI, Linux/Windows GUI clients, runtime-crash process survival, advanced multi-agent orchestration and enterprise policy are separate milestones after the foundation is measured.

---

## 21. M001 acceptance criteria

M001 passes only when all are demonstrated:

### Correctness

- real shell launched through real PTY;
- all PTY bytes go through Seyal VT;
- no alternate terminal engine exists;
- primary and alternate screen transitions function for an explicit supported subset;
- `TerminalExecution` remains alive across app detach/reconnect;
- Block identity exists as Runtime/workspace metadata without owning another grid/PTY;
- missing VT behavior is recorded in a supported-feature matrix, not silently approximated.

### Rendering

- Metal is the first production terminal renderer;
- damage drives incremental presentation;
- no serialized per-cell JSON/text path;
- no NSTextView/SwiftUI terminal fallback.

### Performance

Capture repeatable baseline measurements for:

- key-to-PTY latency;
- PTY-read-to-terminal-state latency;
- terminal-state-to-present latency;
- sustained output throughput;
- idle CPU;
- active CPU;
- RSS for 1/10/50/100 idle executions;
- thread count for 1/10/50/100 executions;
- hidden execution renderer/GPU resource behavior.

No regression threshold should be invented without measurements, but measurement is mandatory before M001 passes.

### Engineering gates

M001 starts with:

- TDD for VT/core behavior;
- byte fixtures;
- retained reference/conformance fixtures for the claimed M001 VT subset with recorded provenance;
- parser/state property tests;
- fuzz harness for VT/parser/protocol boundaries;
- PTY integration tests;
- renderer deterministic tests/golden references where appropriate;
- focused local Runtime IPC/shared-memory threat review before local projection acceptance;
- sanitizers/static analysis/lints;
- crash/detach tests;
- deterministic/reproducible build plan;
- CI quality gate.

---

## 22. Restart-prevention checklist

Stop implementation and require architecture review if any of these appear:

| Failure pattern | Preventative rule |
|---|---|
| temporary renderer | first production frame remains Metal |
| temporary VT | every supported byte path uses Seyal VT |
| GUI-owned/mirrored VT | Runtime remains sole terminal-state authority |
| daemon added too late | M001 already launches through Runtime |
| daemon/process/thread per pane | bounded shared Runtime architecture |
| display JSON/serialized cells | binary/shared derived projection |
| Blocks own independent PTY/parser/VT engines | One TerminalExecution authority; RegionGrids only inside PrimaryBlockEngine |
| semantic transcript becomes canonical | terminal logical history remains canonical |
| agent/Lua/persistence on hot path | asynchronous/cold boundaries only |
| huge per-pane renderer allocation | resources scale by visible surfaces |
| slow client stalls output | bounded independent client queues/resync |
| hidden parallel config store | one typed config model + provenance |
| Lua becomes config source-of-truth | TOML canonical, Lua typed patch |
| platform-specific types leak into portable core | endpoint/native adapters remain narrow |
| premature cross-platform GUI framework | native platform host developed only when needed |
| persistence claimed from journal | live PTY survival and history restoration stay separate |

---

## 23. Final verdict

# READY TO IMPLEMENT MILESTONE 001

Fixed foundation for M001:

```text
runtime authority           = per-user Seyal Runtime
PTY/VT authority            = TerminalExecution in Runtime
BlockTimeline authority     = seyal-workspace / Runtime metadata keyed by ExecutionId
GUI VT mirror               = forbidden
headless                     = implemented from M001
local hot path               = no synchronous IPC ping-pong
per-pane daemon/thread       = forbidden by default
Blocks                       = Runtime timeline metadata + TerminalState RegionGrids (ADR-009)
live TUI                     = same PTY/VT alternate-screen state
agents                       = capability consumers, never terminal owners
attention/approvals          = structured global model
mobile                       = remote controller/viewer
cloud                        = same headless runtime model
config                       = typed TOML + cold typed Lua overlay; production implementation deferred beyond M001
Lua automation               = async typed events/actions only
renderer                     = Metal from first macOS production frame
portable core                = Rust, platform adapters narrow
runtime crash live-survival  = not guaranteed until separately proven
```

If measured local runtime attachment cannot meet latency/memory budgets, revise the transport/projection mechanics. Do not move terminal authority back into the GUI merely to avoid measuring the boundary.

---

## Source notes

Research references include Ghostty architecture/source (`libghostty`, `Surface`, embedding API), Warp Blocks/alt-screen implementation and architecture notes, Herdr headless/session-state model, WezTerm multiplexing architecture, tmux server/client model, iTerm2 shell-integration semantics, Kitty performance architecture, Unicode UAX #29/#11, Microsoft ConPTY documentation, Apple/Android platform lifecycle constraints, and the prior RILL configuration ADRs used only as configuration evidence.

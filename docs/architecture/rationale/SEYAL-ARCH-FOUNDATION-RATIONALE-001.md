# Seyal Foundation Architecture — Decision & Prohibition Rationale

**Document:** SEYAL-ARCH-FOUNDATION-RATIONALE-001  
**Date:** 2026-08-23  
**Status:** Companion rationale to [`../SEYAL-ARCH-FOUNDATION-RD-001.md`](../SEYAL-ARCH-FOUNDATION-RD-001.md)

This document explains **why** Seyal foundation rules exist. It is not a second architecture specification.

Each record contains:

- **Decision / prohibition**
- **Why**
- **Failure prevented**
- **Reopen only if**

Future ADRs that change a foundation rule should cite the relevant `R-xxx` record and provide new measurements, source evidence, platform requirements, or security evidence.

---

## Runtime and terminal authority

### R-001 — Runtime authority from Milestone 001

**Decision:** PTY and authoritative VT state live in the headless-capable Seyal Runtime from M001.

**Why:** GUI detach persistence is a foundation requirement. Moving PTY/VT ownership later would force execution lifetime and reconnect semantics to be rewritten.

**Failure prevented:** GUI-first prototype architecture that later requires daemon migration.

**Reopen only if:** a measured alternative preserves GUI-close persistence, headless use, remote attach, and single authoritative terminal state without migration.

### R-002 — No GUI VT mirror

**Prohibition:** The GUI must not keep a second VT/grid that attempts to mirror Runtime state.

**Why:** two independently mutable terminal models create mode, resize, cursor, scrollback and reconnect divergence.

**Failure prevented:** split-brain terminal state.

**Reopen only if:** formal proof/tests show a representation is strictly derived/read-only and cannot become authoritative; in that case it is a projection, not a VT mirror.

### R-003 — No synchronous IPC ping-pong on hot paths

**Decision:** Required process boundaries use one-way/batched control and display signaling; terminal progress does not wait for cross-process acknowledgements.

**Why:** process boundaries cost scheduling, cache disruption and synchronization even on unified-memory hardware.

**Failure prevented:** key/output latency dominated by request-response IPC.

**Reopen only if:** benchmarks demonstrate a synchronous exchange is required for correctness and remains inside the latency budget under load.

### R-004 — No process/thread/daemon per pane by default

**Decision:** one per-user Runtime manages many terminal executions through bounded I/O shards/tasks.

**Why:** hundreds of panes would otherwise multiply stacks, heaps, schedulable entities, wakeups, IPC endpoints and runtime metadata.

**Failure prevented:** excellent one-pane latency but unacceptable 100–500-pane RSS/CPU.

**Reopen only if:** measured crash/security isolation requirements justify sharding and the measured memory/context-switch cost is acceptable.

### R-005 — One terminal endpoint and one canonical TerminalState per execution

**Decision:** one independent live terminal execution owns one PTY/ConPTY endpoint and one canonical VT/state machine.

**Why:** independent shells/TUIs require independent kernel terminal semantics.

**Failure prevented:** sharing PTYs between panes or duplicating VT state per presentation.

**Reopen only if:** the object is no longer an independent terminal execution.

---

## Blocks, TUI and history

### R-006 — Blocks do not own independent PTY/VT engines

**Decision:** Block **timeline** metadata remains Runtime/workspace structure keyed by
`ExecutionId`. Accepted ADR-009 native-Block target (#1480 / #1482) allows Blocks to
own RegionGrid **content** inside one `PrimaryBlockEngine` / `RegionStore` with a
non-owning `VTWindowMap`. Blocks still must not own an independent PTY, parser,
mode/cursor engine, child, renderer, or copied transcript.

**Why:** a command boundary is semantic structure over one terminal stream; creating a
terminal engine per Block would break raw-mode continuity and explode memory. RegionGrids
inside one VT authority preserve structured UX without a second engine.

**Failure prevented:** Warp-like structured UX accidentally becoming many terminal engines;
also forbids treating history-range cards as the permanent content model once native
Blocks are cut over.

**Reopen only if:** a future Block represents a genuinely separate execution, in which
case it references a separate `ExecutionId` explicitly.

### R-007 — Alternate-screen/TUI remains raw terminal state

**Decision:** Vim, htop, Claude Code, Codex and other TUIs use the same execution's alternate screen and take over the pane presentation.

**Why:** full-screen applications rely on exact cursor/mode/mouse/resize semantics.

**Failure prevented:** trying to force a TUI into line-oriented Blocks.

**Reopen only if:** a specific application exposes a separate structured protocol that is additive to its real terminal behavior.

### R-008 — Agent identity is separate from execution identity

**Decision:** agents reference executions; they do not become executions automatically.

**Why:** one execution may outlive an agent, change agents, or be observed by multiple agents.

**Failure prevented:** agent lifecycle controlling shell/PTY lifetime.

**Reopen only if:** a task is explicitly modeled as a non-terminal agent execution.

### R-009 — Agents stay outside terminal hot paths

**Prohibition:** PTY→VT→damage→render never synchronously waits for agent reasoning, semantic extraction, routing or approvals.

**Why:** model/network latency is unbounded relative to terminal latency.

**Failure prevented:** terminal responsiveness depends on AI availability.

**Reopen only if:** never for canonical terminal progress; only optional derived features may wait.

### R-010 — Native macOS host + Rust portable core

**Decision:** Rust owns portable runtime/terminal logic and portable product/UI state/behavior. AppKit/Swift remains in this repository as a thin macOS host only (window/app lifecycle, native events, IME, accessibility, clipboard/drag-drop, Metal drawable/surface, macOS-specific APIs). Metal is native. The detailed MAY/MUST NOT lists live in ADR-015.

**Why:** this minimizes language crossings while preserving first-class Apple integration, without letting Swift become the Seyal product/UI authority.

**Failure prevented:** portable abstraction weakening IME/accessibility/input; Swift owning terminal state; Swift owning Workspace/Tab/Pane, Blocks, composer, commands, focus/layout or agent/inspector logic; a second Swift UI tree or deletion of the required native host.

**Reopen only if:** measured integration or maintenance evidence shows a different native boundary is superior. Product-vs-platform list changes go through ADR-015.

### R-011 — No temporary text renderer

**Prohibition:** no NSTextView/SwiftUI terminal renderer that is intended to be replaced later.

**Why:** renderer/state boundaries, damage, shaping and scheduling would be designed around the wrong abstraction.

**Failure prevented:** successful prototype with a production rewrite baked in.

**Reopen only if:** never for the production terminal surface; diagnostic tooling may use simple text output outside product architecture.

---

## Portability, mobile and cloud

### R-012 — Portable core now, cross-platform GUI later

**Decision:** platform-neutral Rust terminal/runtime/protocol boundaries are enforced from day one; a universal GUI framework is not.

**Why:** Windows/Linux/cloud/mobile are real future targets, but guessing their UI abstraction before implementation creates unnecessary constraints.

**Failure prevented:** macOS types leak into core, or premature framework compromises native quality.

**Reopen only if:** a second GUI platform is actively being developed and shared UI abstractions emerge from real duplication.

### R-013 — Terminal endpoint abstraction covers PTY and ConPTY

**Decision:** child-terminal transport is abstracted narrowly enough for POSIX PTY and Windows ConPTY.

**Why:** Windows does not expose POSIX PTY semantics directly, but its pseudoconsole exposes VT-oriented terminal streams.

**Failure prevented:** Unix process assumptions throughout terminal core.

**Reopen only if:** another platform requires an additional endpoint adapter.

### R-014 — Mobile is remote-first

**Decision:** iOS/Android act primarily as remote controllers/viewers for a Runtime on desktop/cloud.

**Why:** mobile sandbox/lifecycle constraints are incompatible with assuming durable arbitrary local shells.

**Failure prevented:** designing fake platform symmetry that weakens the real persistence model.

**Reopen only if:** platform capabilities and product requirements justify a separate local-mobile execution mode without changing remote authority.

### R-015 — Cloud uses the same terminal/runtime model

**Decision:** cloud execution reuses portable Runtime/VT semantics rather than creating a cloud-specific terminal engine.

**Why:** users should attach to the same execution model across local and cloud.

**Failure prevented:** local/cloud behavior drift and duplicate emulator implementations.

**Reopen only if:** never for terminal semantics; cloud may add supervisor/security layers around the same core.

### R-016 — Embedding uses opaque versioned ABI

**Decision:** future external embedding exposes opaque handles/versioned capabilities, not Rust/C++ layout structs.

**Why:** internal memory layouts must remain free to evolve.

**Failure prevented:** ABI freezes core implementation details.

**Reopen only if:** language-specific in-process APIs are offered in addition to, not instead of, the stable boundary.

---

## Configuration and customization

### R-017 — TOML remains canonical static configuration

**Decision:** one typed, versioned TOML model persists static settings.

**Why:** it is deterministic, readable, diffable, portable and suitable for dotfiles, backup, migration and policy composition.

**Failure prevented:** hidden GUI database or executable config becoming the only source of truth.

**Reopen only if:** a replacement provides the same deterministic/portable properties and a migration path; Lua alone does not.

### R-018 — Lua extends TOML; it does not replace it

**Decision:** Lua may return typed config patches and automation actions.

**Why:** programmable behavior is valuable, but persisted settings and executable code have different trust/performance properties.

**Failure prevented:** every setting evaluation becoming script execution.

**Reopen only if:** a future extension runtime preserves the same typed cold boundary.

### R-019 — Lua never enters PTY/VT/render hot paths

**Prohibition:** scripts do not run per byte, cell, key, damage event or frame.

**Why:** script execution can allocate, block, throw, recurse or perform I/O.

**Failure prevented:** user customization degrading terminal correctness/latency.

**Reopen only if:** never for canonical progress; precompiled immutable data produced by Lua may be consumed on hot paths.

### R-020 — EffectiveConfig is typed and provenance-aware

**Decision:** all sources resolve into one validated `EffectiveConfig`; the UI can explain value provenance.

**Why:** TOML, Lua, project policy and managed policy must not become competing hidden truth stores.

**Failure prevented:** “why is this setting active?” becoming unanswerable.

**Reopen only if:** another model retains one resolved typed authority and provenance.

---

## Rendering and memory

### R-021 — Shared/local display data is derived, not canonical

**Decision:** any shared-memory/local projection is a rebuildable renderer representation.

**Why:** sharing the canonical mutable terminal heap across processes complicates ownership, synchronization and crash recovery.

**Failure prevented:** renderer ABI and terminal state becoming one shared-memory data structure.

**Reopen only if:** a formally versioned immutable snapshot representation remains derived and single-writer.

### R-022 — Runtime is single writer of terminal state

**Decision:** clients cannot mutate terminal grids/modes directly.

**Why:** single-writer state dramatically simplifies correctness and generation ordering.

**Failure prevented:** data races and client-specific terminal truth.

**Reopen only if:** never for canonical terminal state.

### R-023 — Damage is generation-based

**Decision:** projections/deltas carry monotonically ordered generations and support snapshot resync.

**Why:** clients may miss frames, sleep, disconnect or lag.

**Failure prevented:** unbounded delta retention or visually corrupted reconnect.

**Reopen only if:** an equivalent deterministic resync mechanism exists.

### R-024 — Slow clients cannot backpressure PTY→VT

**Decision:** each client has bounded delivery state; lag leads to coalescing/resync, not parser blocking.

**Why:** mobile/network/hidden clients can be arbitrarily slow.

**Failure prevented:** one remote phone freezes a local shell.

**Reopen only if:** never for canonical PTY progress.

### R-025 — GPU resources scale with visible area, not execution count

**Decision:** hidden/detached executions do not retain dedicated full render surfaces/atlases.

**Why:** hundreds of persistent terminals may be invisible.

**Failure prevented:** GPU/RSS cost proportional to historical pane count.

**Reopen only if:** measured caching benefit exceeds memory cost under realistic 100+ execution tests.

### R-026 — Glyph/font caches are shared where safe

**Decision:** renderer-level glyph/font resources are reused across surfaces with matching device/font parameters.

**Why:** glyph atlases are expensive and identical glyphs recur across panes.

**Failure prevented:** per-pane atlas duplication.

**Reopen only if:** isolation/device constraints require separate caches and measurements justify them.

### R-027 — No per-cell native-language round trips

**Prohibition:** Rust/native host boundaries are batched/coarse.

**Why:** millions of tiny FFI calls erase GPU/render advantages.

**Failure prevented:** renderer dominated by language bridge overhead.

**Reopen only if:** measurements prove a specific call path is negligible and simpler.

### R-028 — Decorative UI cannot outrank terminal paint

**Decision:** cursor/input/output presentation has higher scheduler priority than animations, inspector effects and ornamental UI.

**Why:** futuristic UI is valuable only if terminal latency remains excellent.

**Failure prevented:** “beautiful but sluggish” terminal.

**Reopen only if:** never for user-visible terminal latency; priorities can be tuned from measurements.

---

## History, persistence and state

### R-029 — Logical history has stable identities

**Decision:** scrollback/history uses stable logical line/chunk identity independent of viewport rows.

**Why:** reflow and Blocks need anchors that survive resize.

**Failure prevented:** Blocks pointing at stale physical row numbers.

**Reopen only if:** an alternative stable-address model handles reflow equivalently.

### R-030 — History is bounded/adaptive

**Decision:** hot history has byte budgets; cold chunks may be compressed/spilled/paged.

**Why:** 1M-line logs across hundreds of panes cannot all stay hot in RAM.

**Failure prevented:** memory growth proportional only to lifetime output.

**Reopen only if:** user explicitly configures higher limits and measurements remain acceptable.

### R-031 — Local/remote/mobile are client projections of one Runtime

**Decision:** attachment transport differs, terminal authority does not.

**Why:** transport topology should not create new terminal engines.

**Failure prevented:** mobile/cloud/local divergence.

**Reopen only if:** never for canonical state; clients may have different presentation capabilities.

### R-032 — Layout persistence is separate from execution persistence

**Decision:** windows/tabs/splits can be restored independently of whether an execution is live.

**Why:** UI layout and process lifetime fail/recover differently.

**Failure prevented:** closing/rearranging a window accidentally terminates or duplicates execution.

**Reopen only if:** never conceptually; product UX may couple explicit user actions with confirmation.

### R-033 — Journaling does not equal PTY survival

**Decision:** live continuation is claimed only while an entity still owns the live PTY/process; journal/history restoration is a different contract.

**Why:** a file cannot reconstruct kernel PTY state or arbitrary process memory.

**Failure prevented:** false persistence claims.

**Reopen only if:** OS/container/worker mechanisms preserve the original process and PTY endpoint.

### R-034 — Persistence writes never block terminal progress

**Decision:** persistence consumes bounded asynchronous events/snapshots.

**Why:** disk stalls and fsync latency are unpredictable.

**Failure prevented:** terminal output freezes on storage I/O.

**Reopen only if:** a tiny metadata operation is proven nonblocking and required for correctness; PTY/VT still cannot depend on it.

---

## Workspace, attention and agents

### R-035 — Window/tab/pane is presentation, not process ownership

**Decision:** UI objects reference stable `ExecutionId` values.

**Why:** executions must survive window changes and may have multiple observers.

**Failure prevented:** GUI tree becomes runtime object graph.

**Reopen only if:** never for terminal ownership.

### R-036 — Inspectors/artifacts need no PTY by default

**Decision:** non-terminal panes are first-class presentation/data surfaces.

**Why:** attaching a PTY to every UI concept wastes processes and confuses lifecycle.

**Failure prevented:** “pane == PTY” everywhere.

**Reopen only if:** the specific inspector actually launches an interactive terminal execution.

### R-037 — Attention is a global structured model

**Decision:** approvals/questions/completions use stable `AttentionItem` identities independent of current tab.

**Why:** multi-agent work requires surfacing important events without navigation hunting.

**Failure prevented:** users miss blockers because the source tab is hidden.

**Reopen only if:** presentation changes; the underlying structured attention identity should remain.

### R-038 — OS notifications are projections, not authority

**Decision:** notification center/popover state derives from Runtime attention state.

**Why:** OS notifications can be dropped, dismissed or unavailable.

**Failure prevented:** workflow state disappears when an OS banner disappears.

**Reopen only if:** never for durable attention semantics.

### R-039 — Typed approvals can execute from the Attention Stack

**Decision:** a structured agent/policy approval may be answered without switching tabs.

**Why:** approval intent is semantic, not spatial terminal interaction.

**Failure prevented:** unnecessary context switching during multi-agent workflows.

**Reopen only if:** the target interaction requires terminal spatial context or secret/raw input.

### R-040 — Attention must not fake arbitrary PTY input

**Prohibition:** generic password/raw prompts are not converted into remote “approve” buttons by scraping text.

**Why:** terminal text is ambiguous and security-sensitive.

**Failure prevented:** wrong command/session receiving synthetic input.

**Reopen only if:** an explicit trusted structured protocol identifies the action and target.

### R-041 — Smart routing uses capabilities/metadata, not renderer ownership

**Decision:** routing chooses executions/hosts/agents through an execution registry and capability model.

**Why:** orchestration should scale independently of terminal presentation.

**Failure prevented:** agent router depends on active tabs/windows or renderer state.

**Reopen only if:** never for core routing authority.

### R-042 — Multi-agent conflict/authority must be explicit

**Decision:** multiple agents cannot concurrently mutate one execution simply because they can observe it; controller/write capability is explicit.

**Why:** parallel input can corrupt shells/TUIs and approvals.

**Failure prevented:** agent race conditions become terminal corruption.

**Reopen only if:** an execution type explicitly supports concurrent structured writers.

---

## Multi-client control and protocol

### R-043 — One active controller per terminal input stream by default

**Decision:** many observers may attach, but interactive write authority is leased/owned explicitly.

**Why:** desktop/mobile/remote input races are otherwise nondeterministic.

**Failure prevented:** simultaneous keyboards corrupting input.

**Reopen only if:** shared-control mode is explicitly enabled with deterministic rules.

### R-044 — Resize authority is explicit

**Decision:** one controlling presentation determines canonical PTY rows/cols at a time.

**Why:** terminal applications have one effective winsize.

**Failure prevented:** phone and desktop continuously resize the same TUI against each other.

**Reopen only if:** a future virtual-size/multi-viewport design is proven correct for real TUIs.

### R-045 — Protocol is versioned binary, not JSON display state

**Decision:** high-frequency terminal snapshot/delta/control messages use compact typed binary representation.

**Why:** JSON adds parsing/allocation/size overhead and weak schema evolution for cell-heavy data.

**Failure prevented:** remote/local display protocol becoming a CPU/memory bottleneck.

**Reopen only if:** JSON is used for cold/admin APIs, not high-frequency terminal display.

### R-046 — Reconnect uses snapshot + generation, not replay from process birth

**Decision:** a client attaches from a current snapshot and bounded subsequent deltas.

**Why:** replaying full raw output is expensive and may not reproduce state after truncation/protocol changes.

**Failure prevented:** reconnect cost proportional to session lifetime.

**Reopen only if:** a specialized forensic/history feature explicitly requests replay.

---

## Engineering and commercial boundaries

### R-047 — Terminal fundamentals remain OSS-capable and license-independent

**Decision:** PTY, VT, grid, rendering, local execution and basic Blocks do not require cloud/license checks.

**Why:** terminal correctness must remain trustworthy and the OSS product must be genuinely excellent.

**Failure prevented:** licensing/network failures affecting terminal use.

**Reopen only if:** never for foundational local terminal behavior.

### R-048 — Enterprise policy integrates at typed seams, not internal hot objects

**Decision:** policy can authorize actions/capabilities/config but cannot insert synchronous checks into every PTY byte/frame.

**Why:** governance must not contaminate latency-critical state machinery.

**Failure prevented:** enterprise edition becoming a slower terminal architecture.

**Reopen only if:** a security requirement mandates a check before a privileged action; checks remain action-boundary based.

### R-049 — Architecture changes require evidence, not implementation convenience

**Decision:** a foundation invariant is changed only through documented evidence/ADR, benchmark/security finding, or platform requirement.

**Why:** repeated restarts came from silently trading long-term ownership for short-term visibility.

**Failure prevented:** temporary implementation shortcuts becoming permanent architecture.

**Reopen only if:** the new evidence is documented and the affected acceptance tests/budgets are updated.

---

## Evidence used in this decision package

The architecture pass studied current official/source material from Ghostty, Warp, Herdr, WezTerm, Kitty, tmux, iTerm2, Unicode specifications, Microsoft ConPTY documentation, Apple/Android platform constraints, plus the previous RILL configuration ADRs as configuration-only evidence.

The prior RILL decisions are not inherited wholesale. In particular, Seyal restarts terminal architecture from first principles while retaining the useful configuration insight that typed static configuration and executable automation require different trust/performance boundaries.

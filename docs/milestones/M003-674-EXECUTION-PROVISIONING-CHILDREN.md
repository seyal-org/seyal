# M003 / #674 — execution provisioning child Issue drafts

**Status:** Draft Issue bodies produced by refinement Issue #994. **Not filed.** A
maintainer files these as GitHub Issues under parent [#674](https://github.com/seyal-org/seyal/issues/674).

**Authority:** [`../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md`](../architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md)
(Proposed), SPEC-003 §4.1/§5.2, SPEC-004 §18, SPEC-009 §8.2.1,
[`MILESTONE-003.md`](MILESTONE-003.md) §5–§6, ADR-005, ADR-006, ADR-007, ADR-008,
ADR-009, ADR-015.

**Hard gate:** no child below may be marked **Ready** before ADR-019 and the
SPEC-003 / SPEC-004 / SPEC-009 amendments are accepted on `master`. Until then
these are refinement artifacts, not work authorizations. Each child is one
independently reviewable outcome with one human owner, one
`<human-login>/issue/<number>` branch and one PR, per
`docs/engineering/ISSUE-PROTOCOL.md`.

## Dependency order

```text
ADR-019 + SPEC amendments accepted
  → P1 Runtime lifetime (zero-execution steady state)
  → P2 protocol encode/decode for types 35–38
  → P3 Runtime provisioning admission/creation
  → P4 Runtime explicit disposition
  → C1 portable client provisioning/binding/disposition authority
  → C2 headed Tab creation enablement
  → C3 headed split enablement                  (also requires #923)
  → M1 provisioning measurement/scaling evidence
```

P1 and P2 are independent of each other and may proceed in parallel. C3 also
depends on #923 split-tree projection. #936 multi-live Metal surfaces consumes
C1–C3 and must stay inside SPEC-004 §5 attachment maxima.

---

## P1 — Runtime process lifetime is independent of live-execution count

**In scope**

- `seyal-runtime` binary: zero live executions becomes a valid steady state; the
  process no longer exits when the live-execution count reaches zero.
- On the production client-launched path (empty argument list per SPEC-009
  §8.1.1) the Runtime creates no execution from its own startup.
- An explicit developer/test invocation with a command keeps creating that
  execution as Runtime's own composition.
- Exit only on explicit shutdown (SPEC-003 §16) or an OS signal.

**Out of scope**

- Any protocol change; any client change; provisioning itself; idle-timeout or
  self-termination policy.

**Acceptance**

- A Runtime launched with no command stays alive, keeps its singleton endpoint and
  `RuntimeId`, serves `ListExecutions` with zero entries and reports idle CPU
  indistinguishable from the existing idle baseline.
- A Runtime whose last execution finalizes stays alive and serves new attachments.
- Controlled shutdown still progresses all live executions and reports honestly.

**Tests**

- headless start with empty argv creates zero executions;
- `ListExecutions` returns an empty list without error;
- last-execution finalization does not exit the process, leaks no registration,
  descriptor or Workspace association;
- idle CPU/wake behavior at zero executions has no polling;
- explicit developer invocation with a command still creates exactly one
  execution;
- existing SPEC-003 §19 tests remain green.

**Dependencies:** SPEC-003 §4.1 accepted.

---

## P2 — Local protocol encoding for execution provisioning/disposition

**In scope**

- `seyal-protocol`: `MessageType` 35–38, `CAP_EXECUTION_PROVISIONING = 1 << 8`,
  and exact fixed-width encode/decode for `CreateExecutionRequest` (32 B),
  `CreateExecutionResult` (32 B), `TerminateExecutionRequest` (40 B),
  `TerminateExecutionResult` (32 B).
- Additive result codes `15 InvalidWorkspace`, `16 UnsupportedLaunchProfile`.
- Byte-exact fixtures and fuzz targets alongside the existing Pass 7/8 ones.

**Out of scope**

- Any Runtime or client behavior; any admission, authorization or spawn logic.

**Acceptance**

- Round-trip and rejection behavior match SPEC-004 §18.2–§18.5 exactly, including
  nonzero-reserved rejection and exact payload lengths.
- Framing version stays `1.0`; no existing message layout changes.

**Tests**

- byte-exact encode fixtures for all four payloads;
- truncated, oversized, nonzero-reserved and unknown-code decode rejection;
- unknown message type remains `UnknownMessage`, not a panic;
- fuzz targets registered in the existing harness/fuzz contract;
- existing protocol fixtures unchanged.

**Dependencies:** SPEC-004 §18 accepted.

---

## P3 — Runtime provisioning admission and execution creation

**In scope**

- Handle type 35 on the reactor owner as bounded control work; validate in the
  SPEC-004 §18.2 order; create through the existing SPEC-003 §7 transaction;
  queue exactly one type-36 result.
- Per-connection (4) and Runtime-wide (8) outstanding-request bounds; at most one
  creation per dispatch turn.
- Launch profile `0` resolves to the Runtime's default interactive shell with its
  existing `TERM`/terminfo (ADR-008) and shell-integration nonce (ADR-009)
  behavior. Reserved profile values fail closed.
- `workspace_id` validated against the owning Workspace association.

**Out of scope**

- Termination (P4); any client change; CWD inheritance; named/config launch
  profiles (#676); raising SPEC-004 §5 maxima; implicit attach on create.

**Acceptance**

- A negotiated client can create N distinct executions with distinct
  `ExecutionId`s, each with exactly one owning Workspace association, each
  enumerable and attachable.
- Every failure path emits exactly one failure result and leaves no execution,
  registration, descriptor, child or association behind.
- Unrelated PTY output and control work keep progressing during a burst.

**Tests**

- N concurrent requests → N distinct published executions, exact result
  correlation, FIFO per connection;
- capability absent → `UnknownMessage`; wrong state → `InvalidState`;
- `request_id` zero/duplicate/decreasing/wrapped rejection; reconnect resets the
  space;
- `InvalidWorkspace` and `UnsupportedLaunchProfile` fail closed with no fallback;
- zero/oversized geometry rejected before spawn;
- registry capacity → `CapacityExceeded` with no partial state;
- outstanding-budget exceeded → `Backpressure` before spawn work begins;
- injected spawn/registration/publication failure, repeated N times, proves
  bounded behavior, one result per request, no retry loop, no resource growth and
  continued progress for an unrelated streaming execution;
- one creation per dispatch turn under a burst, with read/write fairness evidence;
- a client that disconnects immediately after sending a request leaves either no
  execution or one live enumerable execution, never a half-created one;
- provisioning from a connection holding no attachment cannot observe or mutate
  another execution;
- created executions receive the same `TERM`/terminfo and shell-integration
  treatment as a Runtime-composed execution;
- logs contain no program/argv/environment/cwd/terminal content.

**Dependencies:** P1, P2, SPEC-003 §5.2 and SPEC-004 §18 accepted.

---

## P4 — Runtime explicit execution disposition

**In scope**

- Handle type 37 with SPEC-004 §18.4 validation order; require the current
  attached Controller; drive the existing SPEC-003 §11 termination state machine
  with the Runtime's own configured `TerminationPolicy`; queue exactly one
  type-38 result meaning `TerminationRequested`.

**Out of scope**

- Client-supplied grace/kill durations; synchronous completion; any change to
  §10 finalization or lifecycle notification; closing presentation semantics.

**Acceptance**

- Only the current Controller can terminate; Observer and stale/foreign
  attachment identities fail closed.
- Completion is observed only through the existing `Lifecycle` path; the reactor
  never blocks.
- Termination after primary reap sends no signal and finalizes exactly once.

**Tests**

- Controller terminate → `TerminationRequested` → SIGTERM → deadline → SIGKILL →
  finalize, without blocking unrelated execution output;
- Observer → `PermissionDenied`; stale/foreign `AttachmentId` → `StaleIdentity`
  or `InvalidAttachment`; mismatched `execution_id` → `StaleIdentity`;
- duplicate/interleaved terminate requests are idempotent and produce exactly one
  finalization;
- terminate raced against natural child exit sends no post-reap signal;
- terminate during `DrainingAfterPrimaryExit` is accepted or rejected
  deterministically and never double-finalizes;
- repeated create/terminate cycles return fd, registration, attachment,
  controller and association counters to baseline.

**Dependencies:** P2, P3, SPEC-004 §18 accepted.

---

## C1 — Portable client provisioning, binding and disposition authority

**In scope**

- `seyal-client` portable Rust: one provisioning intent per new terminal leaf;
  client-local request records correlating `request_id` → `PaneId`; requested
  geometry (Pane cell geometry, or the documented 80×24 bootstrap geometry
  followed by a correlated resize).
- Attach by explicit `ExecutionId`, then `ShellState::bind_execution`.
- Disposition policy of ADR-019 §6.3, including the never-bound orphan path
  (attach as Controller solely to dispose, then one terminate request).
- Unreferenced-live-execution record inside the live client session, so a closed
  Pane's execution stays re-attachable and is never silently forgotten.
- SPEC-009 §8.2.1 resolution: bind by recorded `ExecutionId`; retain
  single-survivor adoption; never guess with more than one survivor.
- Per-Pane client/connection ownership so the headed path no longer depends on
  single-running-execution resolution.
- Bounded non-secret failure state; no automatic provisioning retry.

**Out of scope**

- Flipping `allows_tab_creation` / `allows_pane_splitting` (C2/C3); chrome, menus
  or palette rows; any Swift change beyond forwarding existing typed actions;
  layout persistence; session inventory UI (#929).

**Acceptance**

- Deterministic portable state machine with no native product authority: a host
  cannot provision, choose an `ExecutionId` or retry a rejection.
- Every ADR-019 §7 row is represented by a test.

**Tests**

- intent → request → `Created` → attach → bind produces one Pane bound to one
  distinct execution;
- two intents in flight bind to the correct Panes; results are never swapped;
- result for an unknown/duplicate `request_id` fails closed and binds nothing;
- Pane closed while a request is outstanding → record retained → orphan
  disposition issues exactly one terminate and binds nothing;
- attach or bind failure after `Created` applies the same disposition and never
  leaks the execution silently;
- bound-then-closed Pane detaches only, leaves the execution live, and records it
  as unreferenced and re-attachable;
- provisioning failure surfaces a bounded non-secret state, leaves the Pane
  unbound and issues zero automatic retries;
- fresh session with exactly one survivor adopts it; with two survivors it
  provisions and adopts neither;
- reconnect binds by recorded `ExecutionId`, not list order;
- no provisioning or disposition work runs on the terminal hot path;
- no program/argv/environment/cwd/terminal content in client logs or error state.

**Dependencies:** P3, P4, SPEC-009 §8.2.1 accepted.

---

## C2 — Headed Tab creation on the real provisioning route

**In scope**

- `allows_tab_creation = true` on the production composition; the existing
  `CreateTab` action/palette/menu route reaches C1's provisioning path; each new
  Tab's terminal leaf binds its own distinct `ExecutionId`.
- Headed acceptance that closing a Tab does not terminate unrelated executions.
- Explicit "terminate execution" product action wired to P4 (distinct from
  closing chrome).

**Out of scope**

- Splits (C3); multiple simultaneously live Metal surfaces (#936); tab chrome
  fidelity (#922, #934); session inventory (#929).

**Acceptance**

- Creating tabs produces distinct `ExecutionId`s with no shared PTY/VT and no
  second grid.
- Closing a Tab detaches only; the closed Tab's execution is still live and
  enumerable; unrelated executions are untouched.
- Flow/Raw/TUI selection for a newly bound Pane uses the existing SPEC-008
  presentation fence unchanged.

**Tests**

- portable and headed tests for N tabs with N distinct executions;
- close-without-terminate and explicit-terminate headed cases;
- SPEC-004 §5 attachment/connection maxima produce a bounded honest failure at
  the limit rather than a crash or a silent no-op;
- native XCTest/XCUI required by the headed slice;
- `make bootstrap build test check bench` green on the exact head.

**Dependencies:** C1.

---

## C3 — Headed split-Pane creation on the real provisioning route

**In scope**

- `allows_pane_splitting = true`; split actions provision and bind one distinct
  execution per new terminal leaf; non-terminal Panes never provision.

**Out of scope**

- Split drag-resize ratios (#928); multi-live Metal surfaces (#936); split-tree
  projection itself (#923).

**Acceptance**

- Each terminal leaf in a split tree binds at most one execution; splitting never
  duplicates a VT/grid; closing one Pane leaves sibling executions untouched.

**Tests**

- 2×2 layout with four distinct executions and four independent composer states;
- closing/reparenting a Pane never terminates any execution;
- a non-terminal Pane provisions nothing;
- headed focus/input isolation across panes;
- SPEC-004 attachment-maxima behavior in a dense layout.

**Dependencies:** C1, C2, [#923](https://github.com/seyal-org/seyal/issues/923).

---

## M1 — Provisioning performance, resource and scaling evidence

**In scope**

- Latency for provisioning request → published execution and request → usable
  bound Pane.
- Per-dispatch spawn cost while another execution streams output, proving no
  read/write fairness regression.
- 1/10/50/100 live-execution scaling reported separately from presentation/Pane
  counts, per MILESTONE-003 §8.2.
- Repeated provision/dispose cycles with fd, registration, attachment,
  controller, Workspace-association and RSS counters returning to baseline.

**Out of scope**

- Changing any accepted M002 gate; re-baselining unrelated benchmarks.

**Acceptance**

- Evidence labelled `CI` / `controlled-host` / `PLATFORM_LIMITED` per
  MILESTONE-003 §8.1; the inherited `>5%` explain / `>10%` blocking policy passes.
- If measured spawn cost inside a dispatch violates the fairness gate, the
  finding is recorded against ADR-019 §16 rather than silently adding a worker.

**Tests/measurements**

- benchmark added under the existing benchmark contract;
- fairness measurement with one hot-output execution plus provisioning bursts;
- leak/counter evidence across at least 100 provision/dispose cycles.

**Dependencies:** P3, P4, C1 (C2/C3 for headed numbers).

---

## Explicitly not children of this decomposition

```text
CWD inheritance for new panes            → needs #686 trusted boundary + spec amendment
named/config launch profiles             → #676
session inventory / adoption UI          → #929
raising SPEC-004 connection/attachment maxima or attachment multiplexing
                                          → separate protocol decision + review
multiple live Metal surfaces per Tab      → #936
split-tree projection                     → #923
presentation/layout persistence            → ADR-007 class P4, deferred
agent/remote/cloud-created executions      → M005+ / ADR-016
```

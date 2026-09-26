# Runtime adversarial review contract

**Status:** Required engineering review gate for high-risk Runtime/reactor changes  
**Scope:** PTY lifecycle, child/process lifecycle, reactor readiness, local IPC, backpressure, persistence boundaries and Runtime scheduling

Seyal's normal acceptance tests prove represented behavior. They do not prove that adjacent Unix/macOS states are impossible. High-risk Runtime changes therefore require an explicit adversarial state review before merge.

This document elaborates the mandatory rules in the repository `AGENTS.md`. Architecture and accepted ADR/spec authority remain higher priority.

## 1. Orthogonal state matrix

Do not infer process truth from terminal-endpoint state, presentation state, attachment state or transport state.

At minimum, lifecycle reviews must consider combinations of:

```text
primary child     alive | exit-notified | reaped
PTY endpoint      open | EOF/HUP/closed
termination       not requested | graceful | forced
attachment        none | observer | controller
local transport   healthy | disconnected | resource pressured
presentation      attached | detached
```

A transition may collapse states only when the platform contract proves they cannot vary independently.

### Required PTY/child invariant

PTY EOF/HUP is terminal-I/O state. It does not prove that the primary child has exited. A primary child may close fd 0/1/2 and remain alive.

Therefore:

```text
PTY EOF + try_wait == None
→ disable terminal read/write/input activity
→ keep the primary child lifecycle Running
→ keep process-exit observation armed
→ preserve explicit terminate/shutdown authority
```

Only kernel-confirmed process-exit observation may enter a state whose semantic contract says primary exit is confirmed.

A bounded fallback reap probe may exist solely to cover a proven missed-notification race. It must have an explicit attempt/time bound and must stop; a live process may never cause a fixed-frequency reap loop forever.

For the Darwin inverse regression, do not use a shell command as the PTY-EOF/live-child fixture. Shell execution can introduce implementation-specific descriptor/job-control behavior and makes it unclear which process or descriptor still owns the slave side. Use a direct helper process whose PTY descriptor contract is explicit: close exactly the inherited PTY stdin/stdout/stderr descriptors, remain alive for a controlled interval, and let Runtime observe real PTY EOF while the same primary process is still running. Do not add unrelated controlling-terminal operations such as `TIOCNOTTY`; those are separate lifecycle actions and can themselves change process/session behavior. Where a production decision depends on a lower-level readiness property, pair the end-to-end fixture with a focused reactor/kernel regression for that property.

## 2. Termination invariant

While Seyal still owns a live primary child/process group, there must always be a valid path for:

- explicit execution termination;
- Runtime shutdown;
- eventual reap/finalization.

Loss of PTY I/O, GUI detachment, controller loss, local-IPC failure or presentation removal must not silently make the live child unsignallable.

Every new lifecycle state must answer: "If the child is still alive here, exactly how does shutdown kill and reap it?"

## 3. Level-triggered readiness progress invariant

Every level-triggered readiness handler must do at least one of:

1. make observable progress;
2. drain the condition to `WouldBlock`/equivalent;
3. disarm or throttle the source before returning.

A no-progress turn may not immediately return to a permanently/continually ready source.

This applies to PTYs, listener sockets, client sockets and future platform descriptors.

## 4. Persistent resource-pressure invariant

One-shot fault recovery is not sufficient evidence for an event-loop resource path.

Resource pressure such as descriptor/socket/memory exhaustion can be renewed continuously even when one failed Darwin `accept` drops the individual accepted socket. Runtime scheduling must therefore tolerate repeated no-progress listener readiness without burning a core or starving terminal work.

For the local listener, no-progress readiness is throttled through Runtime deadline scheduling. The listener read filter is temporarily disarmed and re-enabled after bounded exponential backoff; no second thread/event loop or polling loop is introduced.

Successful admission/rejection progress resets the backoff.

## 5. Required inverse regressions

Whenever a fix depends on `A -> B`, add the platform-valid inverse where the A-like signal occurs without B.

Required examples for the current Runtime include:

- PTY EOF while the primary child remains alive;
- PTY EOF followed by natural later primary exit;
- repeated listener resource-pressure/no-progress turns while an unrelated PTY continues producing output;
- explicit terminate and Runtime shutdown while terminal I/O is already closed.

## 6. Required evidence

For lifecycle/resource-pressure hardening, tests must prove as applicable:

- execution remains tracked in the correct public lifecycle;
- no permanent fixed-frequency retry/wake loop;
- unrelated PTY progress continues during local-IPC pressure;
- explicit terminate/shutdown still signals and reaps owned live children;
- natural exit remains observable after PTY read interest is disarmed;
- resource pressure clears and listener acceptance resumes;
- accepted input accounting returns to zero;
- FD/process/attachment capacity returns to baseline;
- no extra PTY, thread, terminal state authority or IPC hop is introduced.

## 7. Late-fix review restart

A late fix to lifecycle, readiness, security, backpressure or benchmark behavior invalidates any earlier statement that the affected area is fully reviewed.

Before marking the PR Ready again:

1. rerun exact-head correctness/quality CI;
2. rerun relevant fuzz/failure campaigns;
3. rerun production benchmark evidence required by the milestone;
4. review the affected orthogonal state matrix from first principles;
5. confirm no new unrepresented high-risk state remains;
6. only then restore final acceptance language in the PR/docs.

Green CI is necessary, not sufficient: it proves the cases we represented.

## 8. Exact-head evidence rule

For a PR that changes any Runtime lifecycle/readiness behavior, final acceptance evidence must belong to the final PR head or GitHub's exact current-master merge result for that head. Evidence from an earlier code SHA may remain useful architecture/performance history, but it cannot substitute for validating the new lifecycle/resource-pressure code. Documentation-only commits after a validated code head may be reconciled explicitly; production-code commits require a fresh run.
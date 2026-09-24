# SPEC-009 — M001 detach, reconnect and GUI-crash survival

- **Status:** Accepted and implemented for M001 Pass 9. Original refinement PR #718 merged as `465ee476124a6d6dd6f48b0485c834d550c684f9`; production implementation Issue #719 is closed Done. Production/acceptance landed via PR #743 as `78018027c9251dab09b100386a663c874d7e300b`; release qualification via #736 / PR #745 as `1005bc42397aac485b1aeff08cafd0f67790d969`.
- **Date:** 2026-08-28
- **Reconciled:** 2026-09-04 against closed #719 / Pass 10 review candidate `1005bc42397aac485b1aeff08cafd0f67790d969`
- **Presentation amendment:** accepted by #858 / PR #859 (`8d08f2f`) on 2026-09-11
- **Issue:** #717 refinement authority; #719 production implementation (Done)
- **Architecture authority:** accepted Seyal foundation architecture, ADR-001/004/005/006/007/015 (ADR-015 governs Rust/native ownership only); accepted ADR-009 including the #858 / PR #859 amendment governs presentation behavior
- **Depends on:** SPEC-002, SPEC-003, SPEC-004, SPEC-005, SPEC-006 and accepted SPEC-007
- **Pass 7 authority:** PR #707 merged as `4490d89fd32f96fe5ff04393a5470944c592f546`
- **Pass 8 authority:** reviewed head `54b3a1748effc7c47c409d1f7cfdcbd547e8d1cc`, merged by PR #721 as `d9d21187e8429bbd3dbeb3e1c7cc4d05c1d147e6`
- **Pass 9 authority:** #719 closed Done; PR #743 / PR #745; review candidate `1005bc42397aac485b1aeff08cafd0f67790d969`
- **Numbering note:** SPEC-008 is already the active M003 command-Blocks/composer specification and is intentionally not a Pass 9 dependency historically. SPEC-008 governs presentation selection/routing when this reconnect contract is used by Flow/Raw/TUI.
- **Proposed M003 amendment:** §8.2.1 multi-execution resolution under Issue #994; **normative only on ADR-017 acceptance** and not implemented.

## 0. Presentation-mode applicability of the accepted ADR-009 amendment

The historical Pass 9 continuity proof remains accepted: GUI loss must not kill the surviving Runtime, PTY, child, `ExecutionId` or authoritative `TerminalState`; reconnect uses a fresh `AttachmentId` and reconstructs disposable client state from Runtime authority.

The accepted #858 / PR #859 (`8d08f2f`) architecture amendment changes only the **presentation reconstruction interpretation** of this contract. References below to a recreated terminal surface are historical Pass 9 evidence for direct-terminal interaction; they do not require a permanently visible/focusable raw terminal surface underneath Flow.

A reconnecting terminal Pane must select exactly one active presentation before it assigns first responder, accessibility focus, IME/text-input, mouse or terminal-input ownership. The transition uses the accepted fence:

```text
1. Rust freezes the old route and increments the epoch.
2. Native revokes input, mouse capture, first responder, AX focus, and marked text.
3. Stale callbacks fail closed.
4. Rust validates current destination eligibility from fresh authenticated
   attachment and authoritative current state (`RuntimeId` / `ExecutionId` /
   fresh `AttachmentId` plus canonical display/projection).
5. Native realizes only that destination (`Flow | Raw | TUI`).
6. The destination route becomes active; focus / AX / IME / mouse / input are
   assigned only to that presentation.

Host completion may gate only local route activation (`Usable`). It must never
stall PTY, VT, or output progress.
```

Mode ownership after selection is:

- **Flow:** Block transcript plus Pane composer/explicit Block controls own Flow interaction. No hidden/coexisting raw terminal view is focusable, hit-testable, exposed as the competing accessibility focus target or eligible for arbitrary terminal input.
- **Raw:** the full-Pane primary-grid terminal presentation owns direct terminal focus, accessibility, IME/text-input, mouse and input routing.
- **TUI:** the full-Pane terminal-application presentation owns direct terminal focus, accessibility, IME/text-input, mouse and input routing.

Selection/admission is fenced to the exact current `ExecutionId`, fresh `AttachmentId` and relevant canonical/presentation generation or state. Any callback, eligibility result or event belonging to a dead attachment, older presentation generation or uncertain state fails closed. A reconnect transition must revoke the old route before enabling the new route; one physical/native event must never reach both routes or a stale route.

Renderer/compositor resources may be reused across these modes. Reuse of the Metal renderer is not authority to keep a conventional terminal input surface installed underneath Flow.

## 1. Purpose

M001 Pass 9 proves that Seyal presentation/client lifetime is independent from a live terminal-execution lifetime on macOS.

Required proof:

```text
live Seyal.app terminal Pane
→ attached Controller for ExecutionId E
→ close/detach GUI or abruptly kill/crash GUI
→ Runtime stays alive
→ same PTY + child + canonical TerminalState for E stay alive
→ reopen Seyal.app
→ discover same RuntimeId
→ attach to E with a fresh AttachmentId
→ rebuild disposable client/display/renderer/native-input state from Runtime authority
→ select current Flow / Raw / TUI presentation from canonical state and trusted integration
→ restore focus/accessibility/IME/input ownership only for that selected presentation
→ resume rendering + permitted input + resize
```

The proof uses the permanent Runtime, Candidate-D local transport, Pass 7 input/resize/focus/IME mechanics, Pass 8 Block metadata seam and reusable Metal renderer/compositor. No temporary reconnect terminal engine or second VT/grid is permitted. Under the accepted presentation amendment, the permanent terminal **authority** and reusable renderer do not imply a permanent raw-terminal viewport/input target in Flow.

Runtime-crash live-PTY survival is explicitly outside M001 Pass 9.

## 2. Non-negotiable invariants

1. `TerminalExecution` remains sole owner of PTY, child lifecycle and canonical `TerminalState`.
2. Runtime remains the live authority while every GUI is absent.
3. GUI window/process, local connection, `AttachmentId`, client display cache, renderer resources, IME state, input queue and resize-reconciliation state are disposable.
4. Losing every GUI/client attachment must not terminate a live `TerminalExecution` or stop the per-user Runtime.
5. Reconnect reconstructs from current Runtime/canonical state; historical PTY bytes are never replayed into a client VT/parser.
6. One surviving Runtime incarnation retains one stable `RuntimeId`.
7. A surviving live execution retains the same `ExecutionId`; a replacement process after execution death receives a new `ExecutionId`.
8. Every reconnect receives a fresh `AttachmentId`; stale attachment/controller/request identities never regain authority.
9. Reconnect never preempts a genuinely live Controller.
10. Detach/reconnect work never synchronously gates PTY → VT → canonical state → damage progress.
11. Persistence/journaling is not responsible for keeping the M001 live PTY alive.
12. Pass 8 Workspace association, Block identity, logical anchor and state remain continuous for the surviving execution; presentation loss/reappearance is not a Block lifecycle transition.
13. Native first-responder, accessibility and IME state are reconstructed as presentation state; they never become terminal authority.
14. Seyal OSS remains independent of commercial code.
15. Reconnect selects one current `Flow | Raw | TUI` presentation before focus/AX/IME/mouse/input ownership is enabled; stale or uncertain ownership fails closed and no raw terminal input target remains active underneath Flow.
16. Cmd-Q / application termination: Rust freezes input and requests bounded detach/cleanup before native application termination. Native must not terminate the process while that bounded cleanup is still owed. This never stalls PTY/VT progress for surviving executions.

## 3. Scope

Pass 9 covers:

- normal terminal-window close/detach;
- GUI application termination;
- abrupt GUI process death/crash;
- Runtime endpoint discovery, startup arbitration and verified stale-endpoint handling;
- Runtime connection-loss detection and idempotent cleanup;
- Controller-lease release and safe reacquisition;
- same-Runtime/same-execution identity proof;
- current-state reconstruction through SPEC-004 Candidate D;
- Pass 7 input/resize/focus/accessibility/IME usability after reattach, scoped to the active selected presentation;
- Pass 8 Block identity/state continuity;
- detached live output and detached execution-exit behavior;
- stale-authority and failure-injection coverage;
- repeated lifecycle/resource tests;
- controlled reconnect performance/resource evidence;
- native real-shell, accessibility/IME and minimal alternate-screen end-to-end proof.

## 4. Explicit non-goals

Pass 9 does not implement or claim:

- Runtime-crash PTY survival, PTY keeper/worker supervision or Runtime-restart continuity of live OS resources;
- reboot/login-session recovery;
- journal-based restoration of a dead PTY;
- durable terminal history, transcript persistence or million-line scrollback;
- durable layout/tab/split/workspace presentation restore;
- remote/network reconnect or cross-device continuation;
- M002 terminal-compatibility expansion;
- tabs/splits/rich workspace navigation;
- complete screen-reader transcript/text-range semantics beyond the accepted SPEC-006 accessibility seam;
- rich inline IME preedit UI beyond the accepted SPEC-006 seam;
- agent/cloud/mobile/commercial behavior.

A future durable metadata store may remember records after Runtime death, but it must never claim those records resurrect the old live PTY.

## 5. Identity and authority contract

For a successful continuity path:

```text
RuntimeId(before GUI loss) == RuntimeId(after GUI reopen)
ExecutionId(before GUI loss) == ExecutionId(after GUI reopen)
AttachmentId(before GUI loss) != AttachmentId(after GUI reopen)
```

If observed `RuntimeId` changes, the client must discard all old connection, attachment, Controller, request, display and resize-reconciliation state. It must not claim continuity of the old live PTY.

Disconnect or successful `Detach` revokes the old attachment and Controller lease. A later connection starts a fresh connection-local request-ID space as required by SPEC-004/006. Old `AttachmentId`, resize request IDs, applied-generation fences, IME state and accepted-but-unwritten local input are never transferred.

The surviving execution retains its Pass 8 Workspace association and M001 `BlockId`/logical start anchor/state until ordinary Block/execution lifecycle changes it. Detach or reattach alone does not create, complete or replace a Block.

Native accessibility object identity is presentation-local and may be recreated after process death. Logical continuity is represented by the same `ExecutionId` and selected Pane-presentation semantics, not by preserving an old `NSView`/AX object instance or requiring a terminal-surface AX object while Flow is active.

## 6. Normal detach semantics

Closing the M001 terminal window or terminating Seyal.app must stop that presentation from owning Controller authority.

Logical shutdown sequence:

```text
stop accepting new input on the active Pane route
→ cancel/discard ephemeral IME composition without emitting marked text
→ revoke first-responder / AX focus / mouse / input admission for that presentation
→ best-effort bounded Detach/Goodbye when healthy
→ close/release client connection and attachment
→ release disposable display/resize/input queues
→ release surface-dedicated renderer/GPU resources per SPEC-005
```

Correctness must not depend on a `Detached`, `Goodbye` or other acknowledgement. App/window shutdown must not block indefinitely waiting for Runtime response.

Normal presentation close must not terminate the `TerminalExecution` and must not stop Runtime merely because no GUI remains. If the macOS process remains alive after its final terminal window closes, it must not retain a hidden Controller lease or hidden direct-terminal input route.

## 7. Abrupt GUI death semantics

`SIGKILL`, crash, forced termination, socket EOF/reset or equivalent disappearance cannot perform a graceful handshake. Runtime therefore owns correctness.

On detected connection loss Runtime must, idempotently and in bounded work:

1. revoke every attachment bound to the dead connection;
2. release its Controller lease before a future attachment can mutate;
3. discard unsent per-client presentation/control state and per-client resource accounting;
4. retain the live `TerminalExecution`, PTY, child, canonical `TerminalState`, Runtime/workspace metadata and unrelated clients;
5. continue PTY output, child-exit observation and execution lifecycle independently.

Repeated or partially overlapping cleanup must not double-free, double-release or terminate the execution.

## 8. Reconnect state machine

A recreated M001 terminal Pane follows:

```text
Disconnected
→ DiscoverRuntime
→ Connect
→ Hello / RuntimeId validation
→ ResolveExecution
→ AttachController
→ AwaitCurrentState
→ CommitDisposableDisplayState
→ RendererReady
→ SelectPresentation(Flow | Raw | TUI)
→ RestoreSelectedPresentationInteractionState
→ Usable
```

Failure at any stage returns to a bounded disconnected/recovery state. It must not create another VT/grid or replay PTY bytes. Under #858, `RendererReady` does not itself select or authorize a raw-terminal input surface.

### 8.1 Runtime discovery, endpoint ownership and startup races

The existing Candidate-D Darwin endpoint remains under the verified per-user Runtime directory. Runtime-side endpoint rules in SPEC-004 and `seyal_protocol::discovery` remain authoritative: owner-only runtime directory, socket ownership/type checks, same-user trust validation, and an active connectable endpoint is never removed as stale.

Actor ownership is explicit:

- the GUI/client may resolve the endpoint path, attempt connection and request/perform the product's one-shot Runtime launch action when no Runtime is discoverable;
- the GUI/client must **never** unlink, replace, chmod, bind or otherwise repair `control.sock` itself;
- only a Runtime process attempting to become the singleton endpoint owner may verify and remove a stale socket and bind `control.sock`;
- a Runtime process must re-prove staleness immediately before removal: correct owner, actual socket, not symlink, and not connectable;
- if an endpoint becomes connectable during cleanup, it is active and must not be removed;
- a Runtime that loses singleton bind/startup arbitration exits/fails startup and never selects an alternate competing endpoint.

Observable discovery outcomes are:

| Observation | Required client/runtime behavior |
|---|---|
| endpoint connects | use that Runtime; validate `RuntimeId`/peer before continuity claim |
| endpoint missing | client may perform/request one Runtime launch for that foreground recovery episode; continuity of an old Runtime is not claimed |
| connection refused on an owned socket | treat as startup/stale ambiguity; use only the exact bounded discovery recovery below, never client-side unlink |
| verified stale owned socket | only the Runtime singleton contender may remove it and bind the canonical endpoint |
| symlink, non-socket, wrong owner or insecure runtime directory | fail closed with non-secret error; never repair by deletion |
| simultaneous Runtime startup | exactly one process may bind the canonical endpoint; losers exit/fail startup and clients converge on the winner |
| endpoint disappears during connect/cleanup | use only the exact bounded discovery recovery below; never create an alternate socket |

A foreground discovery recovery episode is exact and independently measurable:

- the episode starts with one immediate canonical-endpoint connect/discovery attempt at `t=0`;
- an endpoint-missing result may trigger **at most one Runtime launch action** in that episode;
- retryable `NotFound`, `ConnectionRefused`, or endpoint-disappearance outcomes permit **at most 6 connection retries**, for **7 total connection attempts** including the initial attempt;
- retry delays are exactly `10, 20, 40, 80, 160, 250 ms`; this is the complete Pass 9 schedule, with at most `560 ms` intentional scheduler delay;
- the entire automatic discovery/reconnect episode has a hard **1 s wall-clock ceiling** from the first failed connect/discovery attempt; reaching the ceiling cancels any remaining retry;
- successful connection to the canonical endpoint cancels remaining retry work;
- symlink, non-socket, wrong-owner, insecure-directory, peer-validation, or other fail-closed security outcomes stop immediately and do not consume/restart the startup-race retry schedule;
- a Runtime process that loses simultaneous singleton startup arbitration does not authorize another launch or alternate endpoint; the client continues, if budget remains, only against the canonical endpoint;
- exhaustion/ceiling stops automatic recovery with a bounded non-secret state and leaves no retry timer active;
- a later explicit user retry starts a new foreground recovery episode; automatic exhaustion never recursively starts a new episode.

A GUI must not launch multiple Runtime processes merely because the first connection races process startup. One launch action is permitted per foreground recovery episode; every automatic attempt remains within the exact schedule above.

### 8.1.1 Bundled Runtime helper and launch trust contract

The production macOS client launches Runtime only from the Mach-O helper at the
fixed bundle-relative path `Seyal.app/Contents/Helpers/seyal-runtime`. The URL is
derived from the running app bundle, not from configuration, the current
directory, `PATH` or an environment value. Before every launch action the client
canonicalizes the bundle and helper URLs, rejects any symlink or non-regular
helper leaf, and requires the canonical helper to remain below the canonical
`Contents/Helpers` directory at that exact name. A missing, substituted or
unverifiable helper is a non-retryable security failure.

The helper has code-signing identifier `dev.seyal.Seyal.runtime`, an empty
entitlement set for M001, and is signed before the outer app bundle is signed.
Package verification validates both the helper's signature and
the outer bundle seal. Immediately before launch, a distribution client validates
the helper with macOS Code Signing Services against its identifier, an Apple
code-signing anchor and the same Team Identifier as the containing app. A
Debug/test-only ad-hoc build may accept an ad-hoc helper only when the containing
app is also ad-hoc signed; that allowance is compile-time excluded from Release
and package acceptance. This trust check proves package integrity and the
expected release signer/identifier. It does not add isolation from already
trusted code running as the same effective user; that remains outside M001's
same-UID local IPC threat boundary.

The client launches the verified URL directly with no shell and an empty argument
list. The helper launch environment is constructed from scratch and contains
only:

- `HOME`: the absolute home directory returned by the effective user's account
  record;
- `USER` and `LOGNAME`: the effective user's account name;
- `SHELL`: the absolute executable shell path returned by that account record;
- `TMPDIR`: the already-validated absolute per-user Darwin temporary directory;
- `PATH`: exactly `/usr/bin:/bin:/usr/sbin:/sbin`;
- `LANG` and `LC_CTYPE`: copied independently from the GUI environment only when
  present, valid UTF-8, free of control characters and at most 128 bytes.

No other key is inherited. In particular all `DYLD_*`, `LD_*`, credential,
token, agent-socket, shell-hook, allocator/debug and application-private values
are absent. Runtime later constructs the child-shell environment under
ADR-005/ADR-008; helper launch does not grant the GUI authority over PTY
ownership or execution lifetime.

The spawned helper receives `/dev/null` as file descriptors 0, 1 and 2 and no
GUI-owned pipe, socket or log descriptor. Every descriptor at 3 or above is
closed in the child through close-all/default-close-on-exec spawn semantics; a
small hand-maintained close list is insufficient. Runtime creates its canonical
listener only after launch and never inherits it from the GUI. The helper is put
in an independent process group and has no parent-death signal, parent-monitoring
pipe or GUI-owned lifetime token. Logging therefore cannot block on or fail due
to a dead GUI.

The client owns at most one launch request per foreground recovery episode;
launch success only means the helper was started, never that old-PTY continuity
was restored. Runtime owns singleton arbitration, stale canonical-socket
validation/removal and endpoint binding. GUI exit or final-window close does
not terminate an already-running Runtime.

Pass 9 introduces no build-identity field and no protocol-wire change. Runtime
compatibility is determined only by SPEC-004's existing frame version and
capability negotiation: version `1.0` must be accepted and every capability
required by the reconnect path must be present. Compatible app/Runtime builds
may attach across an app update. A version mismatch or missing mandatory
capability maps to non-retryable `RuntimeMismatch`; the observed Runtime is not
killed, replaced or presented as continuity of the old execution. The UI exposes
a bounded non-secret update/restart-required state and explicit retry, but the
automatic recovery episode never restarts Runtime. Additive optional
capabilities remain ignorable under SPEC-004. This preserves ADR-001 and does
not require a new architecture decision.

Helper path, signature and launch failures are reported only as bounded
structured codes: `helperMissing`, `helperPathInvalid`, `helperTrustInvalid`,
`launchDenied` or `launchFailed`. `RuntimeMismatch` covers the compatibility
case. These paths must not log argv, environment names or values, descriptor
details, terminal cells, input, marked text, history, cwd, credentials, sensitive
paths, signer certificate contents or unredacted operating-system error text;
numeric error domain/code and the structured category are sufficient.

Required production evidence includes clean-checkout helper embedding and outer
bundle sealing, Release nested-signature/designated-requirement verification,
Debug-only ad-hoc gating, canonical no-symlink path resolution, direct empty-argv
launch, exact environment equality and poisoned-environment rejection. Separate
canary regular-file, directory, pipe-read, pipe-write, Unix-socket and kqueue
descriptors opened at numbers 3 or above prove close-all behavior. Evidence also
includes missing helper, non-regular/symlink/substituted helper, wrong signer,
wrong helper identifier, unexpected entitlement, duplicate launch race, active
and verified-stale canonical sockets, immediate GUI death while Runtime writes to
all standard streams, and proof that Runtime continues serving the canonical
socket without parent/process-group coupling. The compatibility matrix covers
same version with required capabilities, missing mandatory capability, framing
major/minor mismatch, and optional-capability skew in both client/Runtime age
directions. Every failure case asserts the structured redacted log contract.

Required discovery tests include missing endpoint, verified stale socket, active endpoint, connection refusal, endpoint disappearance between metadata check/connect/remove, two simultaneous Runtime starters, exact initial-plus-six-retry timing/count, one-second exhaustion, cancellation on success, zero surviving retry timer, and proof that only Runtime-side code removes/binds the endpoint.

### 8.2 Target execution resolution

M001 historically demonstrated one product terminal surface and did not add durable workspace/layout navigation. That does not require one permanent surface object after #858; the reconnect target is the eligible surviving `TerminalExecution` and its selected Pane presentation.

For the user-visible Pass 9 proof, exactly one eligible surviving interactive execution must be resolved automatically. If multiple eligible executions exist, the client must not guess, terminate extras or silently select by unstable list order. Tests/callers may specify an exact `ExecutionId`; richer selection belongs to later workspace UI.

If no eligible execution survives, continuity is not claimed. Creating a new execution is a separate path with a new `ExecutionId`.

#### 8.2.1 Multi-execution resolution (proposed M003 amendment)

- **Status:** proposed amendment; **normative only on ADR-017 acceptance** (Issue #994, ADR-017). It narrows resolution; it does not weaken any Pass 9 continuity requirement.

Once client-requested provisioning exists, several live executions are ordinary
rather than exceptional. Resolution therefore becomes:

- within one live client session, a Pane reconnects by the exact `ExecutionId`
  recorded in portable Rust product state. "First/only running execution"
  resolution must not be the headed production path;
- a fresh client process with exactly one eligible surviving execution still
  adopts it for its initial Pane, preserving the §8.2 proof above;
- a fresh client process with more than one eligible surviving execution must not
  guess, must not select by list order and must not terminate extras. It
  provisions a new execution for its initial Pane and leaves the survivors
  unreferenced, live and enumerable;
- presentation/layout persistence stays out of scope (ADR-007 class P4), so a
  fresh process cannot rebuild Pane→execution bindings; a truthful session
  inventory/adoption surface is separate later work (#929);
- closing a Pane, Tab or window remains §6 detach. Ending an execution is the
  explicitly requested, Controller-fenced operation in SPEC-004 §18 and is never
  a consequence of closing presentation.

### 8.3 Controller reacquisition race

A replacement GUI may arrive before Runtime processes EOF/reset for the dead Controller. In this narrow race `ControllerBusy` is valid.

The client must:

- remain non-usable for terminal mutation until Controller authority is obtained;
- never silently fall back to Observer input;
- never buffer unbounded user input;
- retry only through bounded non-spinning recovery.

The attempt contract is unambiguous:

- one immediate Controller attach is the **initial attempt**;
- after `ControllerBusy`, at most **6 retries** are permitted, for at most **7 total attach attempts**;
- retry delays are `10, 20, 40, 80, 160, 250 ms` respectively; these are the complete Pass 9 schedule, not lower bounds that implementations may extend arbitrarily;
- the retry scheduler adds at most `560 ms` of intentional delay;
- the entire automatic Controller-reacquisition episode has a hard `1 s` wall-clock ceiling from the first `ControllerBusy`; reaching the ceiling cancels any remaining retries;
- success cancels remaining retries; exhaustion/ceiling stops automatic retry and exposes a recoverable non-secret state;
- a genuinely live existing Controller is never preempted.

No timer remains active after success, exhaustion, disconnect or surface teardown.

## 9. Current-state reconstruction

Successful attach queues authoritative current display state as defined by SPEC-004. The new client builds a fresh disposable display cache and commits a complete valid current snapshot before the **selected Pane presentation** becomes `Usable`.

Reconnect must not reuse:

- PTY byte history;
- a prior GUI VT/parser/grid;
- prior committed display generation as current authority;
- old `appliedAwaitingProjection` resize fences;
- old IME composition;
- old accepted-but-unwritten client input queue;
- stale renderer/GPU state as terminal truth;
- prior native first-responder or accessibility object state as authority;
- a prior Flow/Raw/TUI presentation choice or input route as current authority.

Renderer resources may be rebuilt from newly committed disposable display state. Generation continuity and resync thereafter follow SPEC-004. Presentation selection is then derived from current canonical state and trusted integration under the owning presentation contract; it is never restored blindly from a dead GUI.

A reconnecting client may show bounded reconnect/loading chrome, but it must not display stale terminal content as if current or expose a direct-terminal input target before presentation selection completes.

## 10. Native focus, accessibility and IME reconstruction

Reconnect preserves the accepted SPEC-006 native mechanics by **recreating mode-appropriate presentation state**, not by carrying old state across the dead connection/process and not by always recreating/focusing a conventional terminal surface.

Before transition from `RendererReady` to `Usable` under the accepted #858 amendment, apply the ADR-009 six-step fence. The reconnect elaboration is:

1. validate the current `RuntimeId`, `ExecutionId`, fresh `AttachmentId`, committed projection generation and relevant canonical state used for presentation selection;
2. select exactly one `Flow | Raw | TUI` presentation from current canonical state and trusted integration; unknown/stale/unsafe Flow eligibility selects the safe non-Flow presentation required by SPEC-008 rather than guessing;
3. revoke or prove absent every stale previous route before enabling the new route: old first responder, text-input context/marked text, mouse reporting/hit testing, accessibility focused state and input admission;
4. create/attach only the native presentation objects required by the selected mode; reusable Metal renderer/compositor resources may be prepared without becoming input authority;
5. for **Flow**, expose Block/transcript/composer accessibility structure, make the eligible Pane composer (or other explicit Flow control) the focus/input owner, and expose no hidden/coexisting raw-terminal focus or hit target;
6. for **Raw/TUI**, attach the full-Pane terminal presentation, expose its accepted terminal accessibility semantics, make it first responder when the window is key/active and no modal application command owns focus, and activate its native text-input/IME context;
7. start any newly activated direct-terminal composition path with an empty bounded `CompositionDocument`; old marked/preedit text remains discarded;
8. after authoritative display commit supplies a valid cursor/cell anchor in Raw/TUI, `firstRect(forCharacterRange:)` returns finite current screen-coordinate candidate geometry from that selected presentation, never stale terminal/history text;
9. enable input only after the selected presentation owns focus/AX/IME/mouse routing and admission is fenced to the same `ExecutionId`, fresh `AttachmentId` and current presentation/canonical generation; stale callbacks fail closed;
10. prove ordinary Flow composer submission or, in Raw/TUI, committed text/dead-key/IME input works through the existing authorized path exactly once without duplicate event routing.

Accessibility continuity means semantic continuity, not native object reuse. A new process may expose new platform accessibility element instances, but they must represent the same logical terminal execution in the **selected current presentation** and must not silently expose an obsolete raw-terminal target while Flow is active.

Required deterministic/native evidence:

- reconnect selects Flow/Raw/TUI from current state before any first responder or terminal-input route is enabled;
- Flow reconnect focuses/exposes its Block/composer structure and has no focusable/hit-testable hidden raw-terminal accessibility/input target;
- Raw/TUI reconnect makes the selected full-Pane terminal presentation first responder when eligible;
- focus loss/reacquisition remains correct and no hidden old view retains focus;
- accessibility role/label/description and geometry are valid for the selected presentation;
- accessibility focused-state tracks the actual selected first-responder owner;
- the input-admission/reconnect failure state is exposed non-secretly;
- a VoiceOver smoke verifies Flow discovers Block/composer structure and Raw/TUI discovers the selected terminal presentation without exposing rejected input/marked text as transcript;
- fresh direct-terminal IME context begins with no old marked text;
- dead-key and one real IME path commit once after Raw/TUI reconnect;
- candidate-window anchor is finite and follows the current cursor/selected surface after authoritative snapshot commit;
- an intentionally delayed stale focus/IME/mouse/input callback from the prior attachment or prior presentation generation is rejected and cannot cross into the new route.

Pass 9 does not add a full accessibility terminal transcript API or a second editable text model.

## 11. Detached execution behavior

### 11.1 Output while detached

A live execution continues consuming PTY output and mutating canonical `TerminalState` while no GUI is attached. Reattach observes current authoritative state within M001's bounded screen/history contract; Pass 9 does not promise durable capture of every detached line.

### 11.2 Input while detached

With no GUI Controller, Runtime invents no input. Input that existed only in a dead client's local accepted-but-unwritten queue is lost with that client and must never be silently replayed after reconnect.

### 11.3 Execution exit while detached

If the child exits while detached, Runtime performs ordinary final drain and lifecycle completion. Pass 8 final display → Block Completed → Lifecycle Finalized ordering remains authoritative.

Reopening must not resurrect the dead PTY or create a replacement under the old `ExecutionId`. A retired finalized execution is absent from live resolution.

## 12. Runtime failure boundary

Pass 9 continuity requires the Runtime incarnation itself to survive GUI loss.

```text
Runtime dies
→ old RuntimeId ends
→ M001 does not claim old live-PTY continuity
→ old attachment/controller/request identities are invalid
→ later replacement execution has a new live execution lifetime
```

No journal, Block record, display snapshot or renderer cache may be presented as restoration of a live PTY. Runtime-crash survival requires a separately reviewed supervisor/keeper architecture.

## 13. Security and privacy

Required regressions include:

- stale old `AttachmentId` cannot input, resize or detach the new attachment;
- a second same-UID client cannot preempt a live Controller;
- malformed reconnect/attach frames are bounded and cannot retain leaked authority;
- old connection-local resize/request IDs cannot correlate against a new connection;
- RuntimeId mismatch invalidates cached authority/reconciliation state before mutation;
- GUI discovery cannot remove/replace an endpoint and insecure/symlink/non-socket paths fail closed;
- simultaneous startup cannot create two accepted singleton endpoints;
- reconnect/failure logs contain no terminal contents, input bytes, marked text, cwd, environment or secrets;
- accessibility/IME recovery cannot expose rejected input, marked text or terminal history through the composition document;
- crash cleanup does not weaken same-UID endpoint validation or attachment authorization;
- stale presentation/input-route generation cannot restore focus, AX, IME, mouse or input authority after reconnect;
- Flow reconnect cannot expose a hidden raw-terminal input/accessibility target.

## 14. Resource and hot-path constraints

Detach/reconnect is lifecycle/control work, never terminal hot-path work.

Forbidden:

- per-execution/per-client polling threads or timers that remain active while detached;
- synchronous persistence, agent, cloud, licensing or telemetry work on reconnect;
- unbounded discovery/controller retry, input or presentation queues;
- copied transcript/grid retained solely for reconnect;
- hidden renderer/GPU allocations retained after final surface detach contrary to SPEC-005;
- terminal progress waiting for GUI reconnect or acknowledgement.

Runtime retains only live execution state already required without a GUI plus bounded Runtime/workspace metadata.

## 15. Required tests and failure injection

### Identity/lifecycle

- normal window close keeps Runtime/PTY/child/`ExecutionId` alive;
- app quit keeps the same live execution alive after Rust freezes input and requests bounded detach/cleanup;
- GUI `SIGKILL`/forced crash keeps the same live execution alive;
- `RuntimeId` remains unchanged across surviving-Runtime reconnect;
- new `AttachmentId` differs and old identity is rejected;
- same `ExecutionId` resumes input and resize after reattach;
- same Pass 8 Workspace/Block identity/anchor/state survives detach/reconnect.

### Discovery/startup

- missing endpoint produces bounded Runtime-absent/start path, not competing endpoints;
- client never unlinks the endpoint;
- verified stale owned socket is removed only by Runtime-side singleton startup;
- active endpoint is never removed as stale;
- symlink/non-socket/wrong-owner/insecure path fails closed;
- connection-refused and cleanup races follow exactly the section 8.1 attempt/schedule/one-second ceiling and converge without alternate endpoint creation;
- simultaneous Runtime startup yields exactly one bound canonical endpoint and losing contenders exit/fail startup;
- successful discovery cancels every remaining retry/timer;
- exhaustion leaves no surviving retry timer and requires explicit user retry for a new episode;
- RuntimeId validation prevents claiming old continuity after Runtime replacement.

### Current-state/recovery

- output generated while detached is reflected after reconnect;
- initial reconnect snapshot is authoritative before `Usable`;
- generation gaps use ordinary bounded resync;
- stale display cache/resize fence/IME/input state is never reused;
- stale presentation choice/focus/input-route generation is never reused;
- client crash during attach or multi-chunk snapshot leaves Runtime/execution healthy;
- snapshot decode failure recovers through a fresh bounded attach/resync, never PTY replay.

### Focus/accessibility/IME

- presentation selection occurs before focus/input activation;
- Flow reconnect exposes/focuses Block/composer structure as applicable and no hidden raw-terminal focus/input target exists;
- Raw/TUI reconnect makes the selected terminal presentation first responder when eligible;
- accessibility role/label/focused state/geometry are valid for the selected presentation;
- fresh Raw/TUI IME context has empty composition state;
- dead-key and real IME commit paths work once after Raw/TUI reconnect;
- candidate geometry is finite/current and no stale view/context is consulted;
- VoiceOver smoke discovers the selected mode's structure without secret composition/rejected-input exposure;
- stale old-attachment or old-presentation callbacks fail closed and one event cannot reach both old/new routes.

### Controller races

- EOF cleanup releases Controller exactly once;
- new attach before cleanup observes `ControllerBusy`, not preemption;
- initial attempt + at most 6 retries follow exactly the declared schedule;
- total automatic reacquisition stops at the 1-second ceiling;
- bounded retry succeeds when cleanup completes;
- genuine persistent Controller occupancy exhausts bounded recovery without spin, hidden buffering or a surviving timer.

### Detached outcomes

- child continues running/outputting with zero GUI attachments;
- child exit while detached completes final drain/lifecycle correctly;
- retired execution is never resurrected/reused;
- explicit terminate remains distinct from detach.

### Resource/failure

- at least 100 graceful detach/reattach cycles;
- at least 100 abrupt client-loss/reconnect cycles in deterministic harnesses excluding process-launch cost;
- zero leaked Runtime attachment/controller records, sockets/fds and renderer resources;
- malformed/stale reconnect attempts remain bounded;
- disconnect during input backpressure, outstanding resize, display chunking and Block finalization remains correct.

### Native end-to-end

On the production macOS path demonstrate:

1. launch a real shell and long-lived fixture that changes visible state;
2. close the terminal window and verify Runtime, shell PID and PTY remain live;
3. reopen and interact with the same execution;
4. select the current Flow/Raw/TUI presentation before restoring its focus/accessibility/IME/input seam;
5. in Flow, verify no hidden raw-terminal focus/input target; in Raw/TUI, verify the selected terminal presentation owns direct input;
6. repeat with abrupt GUI process kill;
7. verify normal permitted input, Control-C where the selected route supports it, and resize after reconnect;
8. verify one accepted alternate-screen fixture can detach/reconnect without another PTY/VT authority.

## 16. Performance and measurement contract

Pass 9 implementation must preserve the final Pass 8 controlled baseline retained by PR #721 and `docs/engineering/M001-PASS8-BLOCK-METADATA.md`. The inherited Pass 8 regression policy remains normative: paired latency movement `>5%` requires root-cause explanation and `>10%` is blocking absent explicit re-review.

The previously proposed absolute `10 ms`, `25 ms` and `4 MiB` values are **not accepted blocking gates** because the refinement did not record a reproducible derivation. They may be retained only as non-binding investigation/reference values in historical review discussion. Before Issue #719 became Ready, a controlled pre-implementation calibration derived and recorded the exact absolute cleanup/reconnect/RSS budgets with independent review acceptance; those accepted budgets are retained under Pass 9 evidence and remain normative for reconnect work.

### 16.1 Required controlled methodology

The production implementation Issue must freeze all of the following before code starts:

- exact master/dependency SHAs and Pass 8 baseline values;
- exact Apple-Silicon hardware, macOS version, release-build configuration and benchmark commands;
- fixed 120×40 reconnect geometry plus any additional representative geometry;
- 20 warm-up lifecycle cycles excluded from measurement;
- five independent measured cohorts of 100 cycles for graceful reconnect and five cohorts of 100 cycles for abrupt-client-loss reconnect;
- per-cohort p50/p95/p99/max plus median-of-cohort p99;
- the exact definition of each timestamp boundary;
- exact RSS/fd/attachment/controller/GPU-resource sampling points;
- the exact accepted absolute budgets derived from calibration and why those budgets are appropriate.

For RSS/resource measurement, each cohort uses the same Runtime/execution across its 100 measured cycles. The pre-cohort baseline and post-cohort sample are taken only at a quiescent lifecycle point defined as:

```text
no GUI attachment/controller for the detached surface
+ client socket closed
+ surface renderer/GPU resources released
+ no pending reconnect/discovery/controller retry work
+ Runtime reactor has no queued lifecycle work attributable to that client
```

At each baseline/final point, take five RSS samples at fixed intervals and use the median. Repeat the cohort from a fresh Runtime process for the next independent cohort. Exact attachment/controller/fd/resource counters must return to baseline every cycle; any counter leak is blocking regardless of RSS noise.

RSS acceptance must be based on the independently accepted calibrated budget and repeated-cohort behavior, not one post-run sample. Any consistent positive growth trend across cycles/cohorts requires root-cause analysis even if it is numerically below the accepted budget.

### 16.2 Metrics that must be reported

Controlled measurements must report:

- Runtime disconnect-event dispatch → attachment/controller cleanup;
- local connect/hello/resolve/attach → complete authoritative 120×40 current-state client commit;
- committed client state → first renderer-ready update;
- renderer-ready → selected presentation interaction state ready where separately measurable;
- repeated-cycle CPU/RSS/fd/attachment/controller/renderer-resource counts;
- idle detached Runtime CPU with a live execution;
- paired Pass 8 input/output/render/resize regression attribution after reconnect work lands.

No benchmark may add a synchronous acknowledgement or logging dependency to the terminal hot path. Full macOS app process-launch time is measured separately because launch-services/cache state is noisy; it cannot substitute for deterministic Runtime/client reconnect measurement.

## 17. Acceptance criteria

Pass 9 production implementation is complete only when all are true:

- graceful close leaves the same Runtime and live execution running;
- abrupt Seyal.app death leaves the same Runtime and live execution running;
- runtime discovery/startup races follow section 8.1 exact attempt/deadline rules and never create a competing accepted Runtime endpoint;
- reopen observes same `RuntimeId`, same `ExecutionId`, fresh `AttachmentId` for the surviving-Runtime path;
- stale controller/attachment/request identities are rejected;
- current authoritative display is reconstructed without PTY replay or another VT/grid;
- reconnect selects one current Flow/Raw/TUI presentation before focus/AX/IME/mouse/input activation and rejects stale route callbacks;
- Flow reconnect has no hidden/coexisting raw-terminal focus/input target; Raw/TUI reconnect exposes the selected full-Pane terminal interaction surface;
- input, Control-C where applicable and authoritative resize work after reconnect;
- detached output advances canonical state and is reflected within M001 bounded-state limits;
- detached child exit is handled without resurrection;
- Pass 8 Workspace/Block continuity is preserved;
- repeated graceful/crash cycles are leak-free and bounded;
- security/privacy regressions pass;
- exact calibrated performance/resource gates recorded in #719 pass;
- inherited Pass 8 `>5%` explanation / `>10%` blocking regression policy passes;
- `make bootstrap`, `make build`, `make test`, `make check`, `make bench` are green on final head;
- native clean-checkout proof succeeds on the permanent production path;
- independent architecture/security/performance review has no unresolved blocker.

## 18. Documentation impact

Pass 9 production behavior exists on `master`. Pass 10 / M001 closeout documentation is complete (#727 / #5 closed). Do not reopen this SPEC's historical production gate. The accepted #858 amendment narrows how its native presentation wording is interpreted without invalidating the continuity evidence:

- **User Guide:** document only observable reconnect/lifecycle behavior that has passed its milestone gates; do not claim Runtime-crash PTY survival.
- **Developer Guide:** Runtime-vs-GUI lifetime, endpoint discovery ownership and reconnect/native interaction lifecycle are governed by this SPEC plus the Pass 9 evidence retained under `docs/evidence/pass9-*` and Issue #719. Current Flow/Raw/TUI presentation ownership additionally follows ADR-009/SPEC-008.
- **Authoritative engineering docs:** this SPEC, the specs index, and M001 Pass 10 protocols continue to present Pass 9 continuity as Done; Pass 10 / M001 closeout is also Done (#727 / #5 closed). The presentation amendment does not retroactively claim that historical Pass 9 tested a Block-first Flow UI.
- **Media/screenshots/video:** add only when the reconnect UX is stable enough for procedural user docs.

## 19. Historical refinement acceptance and Pass 9 readiness (superseded)

> **Superseded.** The Ready gates below were the pre-implementation production-start contract for Issue #719. They are retained as historical authority. Pass 9 production is **Done**: #719 closed; PR #743 merged production/acceptance as `78018027c9251dab09b100386a663c874d7e300b`; PR #745 retained release qualification at `1005bc42397aac485b1aeff08cafd0f67790d969`. Pass 10 (#727) and parent M001 (#5) are **closed**; M001 is Done.

Historical refinement provenance:

- Pass 7 implementation is merged as `4490d89fd32f96fe5ff04393a5470944c592f546`;
- SPEC-007 is accepted;
- Pass 8 implementation PR #721 is independently review-green and merged;
- Pass 8 reviewed head is `54b3a1748effc7c47c409d1f7cfdcbd547e8d1cc`;
- Pass 8 merge commit is `d9d21187e8429bbd3dbeb3e1c7cc4d05c1d147e6`;
- original SPEC-009 refinement PR #718 merged as `465ee476124a6d6dd6f48b0485c834d550c684f9`;
- SPEC-008 remains the active M003 command-Blocks authority;
- the corrective amendment made runtime-discovery failure semantics, native reconnect acceptance, retry bounds, measurement reproducibility and documentation impact explicit.

Historical production-start gate (no longer current). Before #719 could become Ready it had to record and independently validate all of the following:

1. the merged commit SHA containing this corrective SPEC-009 amendment;
2. the final Pass 8 merge SHA and exact retained Pass 8 baseline values;
3. current-master validation with no unresolved Runtime/client/native/Block lifecycle blocker;
4. the final discovery, lifecycle, accessibility/IME, failure-injection, security and native-E2E test matrix from this SPEC, including section 8.1's exact discovery schedule;
5. the controlled calibration methodology and independently accepted absolute cleanup/reconnect/RSS budgets required by section 16;
6. documentation impact classification and exact production-document updates expected;
7. no unresolved architecture/specification question.

Historically, no Pass 9 production code belonged in the corrective refinement PR, and production implementation could begin only after Issue #719 was explicitly moved to `Ready` with the evidence above. That gate is closed.

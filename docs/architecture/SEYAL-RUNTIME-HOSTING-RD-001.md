# Seyal Runtime Hosting and Process Continuity R&D

**Document:** SEYAL-RUNTIME-HOSTING-RD-001
**Date:** 2026-11-19
**Issue:** #1518
**Baseline:** `master` at `1ef183fa`
**Status:** Proposed architecture refinement. Not authority. No production implementation is authorized until a non-author maintainer accepts the resulting ADR (separate PR).
**Parents / siblings (read-only, not modified):** #1511 (V1 scope), #1515 / PR #1516 (session management; this is its WI-1), #1517 (notifications), #1494 (workspace containment), #1514 (recovery diagnostics), #832 / #687 (durable persistence), #924 (Runtime-crash live PTY R&D).

## 0. Summary

V1 requires that GUI Quit, GUI crash and GUI restart leave the same live executions reattachable (`M004-V1-PRODUCT-SCOPE.md`). The Runtime already supports this everywhere except one place: intentional headed Quit kills the Runtime process group (`BundledRuntimeLauncher.swift`), ending every PTY. The stated reason is a macOS LaunchServices artifact, not a Runtime limitation.

This document records a macOS experiment (section 4) showing that the artifact cannot be removed by changing how the GUI spawns the helper. The only mechanism that survived Quit with a clean Dock/LaunchServices state was a launchd job registered outside the app process.

**Recommendation (proposed, not accepted, conditional on gate S1):** keep the existing `seyal-runtime` binary and singleton unchanged; change only who owns its process lifetime. Host it as an on-demand per-user launchd LaunchAgent (registered through `SMAppService.agent`, no `RunAtLoad`, no `KeepAlive`, no root), started idempotently by the GUI or any CLI, and never spawned or killed by the GUI. No new daemon, protocol, registry or PTY manager is introduced.

**What is and is not established.** Established: a launchd job registered *outside* the app survives Quit with no LaunchServices subordinate state, and a job the app registers *itself* via `launchctl bootstrap` does not (E4 versus E6/E7). `SMAppService.register()` is an app-initiated call, so it may behave like E4 rather than E6/E7. That is unproven, and the whole recommendation fails if it does. Gate S1 (section 12) is therefore a **hard precondition for the ADR**, with the exact test "`register()` from the app, kickstart, Cmd+Q, `lsappinfo`". If S1 fails the design falls back to alternative D' (section 3): remove the Quit kill and accept the LaunchServices state, whose user-visible consequence is itself unmeasured.

## 1. Evidence discipline

Every claim below is tagged:

- **[V-src]** verified by reading source at `1ef183fa`.
- **[V-exp]** verified by the experiment in section 4 on this machine (macOS 27.2, arm64, ad-hoc signed throwaway bundles).
- **[V-meas]** measured, with the stated limitation.
- **[Inf]** inference from source or documentation, not executed.
- **[Proposed]** a design decision for review.
- **[Unverified]** needs a named gate before acceptance.

Behaviour of tmux, WezTerm, Zellij and cmux (section 5) is from documentation and prior knowledge, not re-verified in this session, and is tagged **[Unverified-secondary]**.

## 2. Existing implementation gap analysis

### 2.1 What already supports independence

- Runtime is a separate headless binary `crates/seyal-runtime` with no GUI-lifetime watcher: no parent-pid check, parent-death signal, stdin/pipe EOF watcher, idle-exit timer or exit-at-zero-attachments logic. **[V-src]**
- With empty argv the Runtime creates no execution and is resident at zero executions and zero attachments (SPEC-003 §4.1; `main.rs`; `runtime_lifetime_p1.rs`). **[V-src]**
- Connection loss revokes attachments, releases the Controller lease and retains execution, PTY, child and `TerminalState` (SPEC-009 §7; ADR-017 §6.1). **[V-src]**
- The helper is spawned with `/dev/null` standard streams, no inherited descriptors, its own process group, no parent-monitoring pipe (SPEC-009 §8.1.1). **[V-src]**
- Singleton is a Runtime-side `flock` guard; only the Runtime removes verified-stale sockets and binds `control.sock`; clients never repair endpoints (SPEC-003 §4, SPEC-009 §8.1). **[V-src]**
- Discovery has a bounded schedule: one launch per episode, 7 attempts, delays 10/20/40/80/160/250 ms, 1 s ceiling (SPEC-009 §8.1; `recovery.rs`). **[V-src]**
- GUI crash does not kill the Runtime: no parent watcher exists, and a helper whose parent exits is reparented to launchd. **[V-src]** Section 4 observed reparenting after a graceful Quit only (helper `ppid` became 1); an app `SIGKILL` was not run. **[Inf]**

### 2.2 Every assumption tying Runtime lifetime to GUI lifetime

| # | Assumption | Evidence | Effect |
|---|---|---|---|
| G1 | Intentional Quit kills the helper process group (`kill(-pid, SIGTERM)`, 0.4 s wait, then `SIGKILL`) | `BundledRuntimeLauncher.swift` `terminateLaunchedHelperForHeadedQuit`, called from `MultiWindowHostController.performQuitCleanup` **[V-src]** | Every live PTY ends on Cmd+Q. ADR-018 §4.0 (accepted text) explicitly accepts this as an interim pending a launchd/LSUIElement-packaged Runtime; it contradicts SPEC-003 §4.1, ADR-018 §8 invariant 6 and V1 scope |
| G2 | The kill exists to clear a LaunchServices state `(exited-with-subordinates)` | comments in the launcher; ADR-018 §4.0 **[V-src]**; reproduced **[V-exp]** | The cause is real; the remedy (kill Runtime) is wrong |
| G3 | The GUI is the only spawner; spawn is `posix_spawn` of `Bundle.main/Contents/Helpers/seyal-runtime` | launcher, `RecoveryEffect::LaunchHelper` **[V-src]** | The Runtime is a descendant of the GUI's launch context; a CLI without a GUI has no way to start it |
| G4 | Only the spawning GUI records `launchedHelperPid`; a second GUI that adopts an existing Runtime has no pid | launcher static **[V-src]** | Kill scope differs by who happened to spawn |
| G5 | Helper trust is anchored on the GUI's own Team ID and identifier at launch time | `evaluateHelperTrust` **[V-src]** | A launchd-hosted Runtime has no GUI at launch; trust moves to registration and peer validation (section 7) |
| G6 | Helper environment is built by the GUI, including `LANG`/`LC_CTYPE` copied from the GUI environment and a pinned `TMPDIR` | `launchEnvironment` **[V-src]** | A launchd-started Runtime gets launchd's environment; locale provenance changes (section 8, F4) |
| G7 | Singleton lock lives at `temp_dir()/seyal/runtime.lock`, the socket at `<DARWIN_USER_TEMP_DIR>/seyal-runtime/control.sock` | `config.rs:121`, `discovery.rs:67` **[V-src]**; difference is **[Inf]** | If the Runtime is started with a `TMPDIR` other than the Darwin user temp dir, lock and socket diverge and the singleton no longer arbitrates the socket. The launcher masks this by pinning `TMPDIR` |
| G8 | Last-window close: code returns `false` (app stays resident with zero windows); ADR-018 text says `true` at lines 230-232 and 275 and §13.3 (Proposed), while its own test line and the `AppDelegate` comment (§3.3a) say `false` | `AppDelegate.swift`, ADR-018 **[V-src]** | ADR-018 is internally inconsistent; independent of Runtime lifetime once G1 is removed; flagged for ADR-018 owners, not decided here. A3 is written to pass under either |
| G9 | Quit cleanup runs the 0.4 s wait plus blocking `waitpid` on the main thread inside a 500 ms backstop | `MultiWindowHostController.swift`, `ApplicationQuitCoordinator.swift` **[V-src]**; overrun is **[Inf]** | Disappears with G1 |
| G10 | Idle loop is `poll_once(30 s)` while SPEC-003 §4.1 says "an idle Runtime must not poll" | `main.rs` **[V-src]**; wake cost **[Unverified]** | A 30 s timeout wake is not a poll in the busy sense, but the contradiction should be resolved by measurement (section 9) |
| G11 | Update model says a GUI-only update leaves the old Runtime resident; Hello carries no build identity | ADR-022 context **[V-src]** | Needs the compatibility handling in section 8, row U2 |
| G12 | No shutdown path from clients. The Runtime installs **no signal handler**; `begin_shutdown` is reachable only in the explicit-argv developer mode (`main.rs`) and tests | `crates/seyal-runtime/src` (no handler found), `registry.rs`, #1515 §5.5 **[V-src, absence: grep of signal/sigaction/ctrlc in the crate]** | A SIGTERM from launchd (logout) kills the process by default disposition and runs **no** SPEC-003 §16 drain. Ending the Runtime deliberately has no product path; this document does not invent one (section 6.5) |

## 3. Alternatives

All four were evaluated against the same criteria.

| Criterion | A. Independently launched per-user Runtime | B. launchd on-demand LaunchAgent | C. SMAppService-managed agent | D. Existing bundled helper, corrected |
|---|---|---|---|---|
| Survives Quit with clean LaunchServices state | No when started by the GUI **[V-exp]** (E4, E5); would work only if started from outside the app | Yes when registered outside the app **[V-exp]** (E6, E7); No when the app registers it (E4) | **Unproven.** `register()` is app-initiated and may behave like E4 (tagged) rather than E6/E7 (clean). Decided by gate S1 | No **[V-exp]** (E1-E3) |
| Survives GUI crash/force quit | Yes | Yes | Yes | Yes (already) |
| Startup latency | One `posix_spawn`: Runtime reaches a connectable socket in ≈14 ms **[V-meas, debug]** | Same binary; plus launchd start (not separately measured; S3) | Same as B | Same as A |
| Idle memory / CPU | ≈3.4 MiB RSS, 0.0 % CPU, 1 thread **[V-meas, debug build, single run]** | Same process | Same process | Same process |
| Security | Same-UID only | Job definition is a file; signed-bundle path via SMAppService; launchd supplies a clean environment | Bundle-signature-bound job; user-visible and user-revocable in System Settings | Unchanged |
| Code signing | New trust anchor needed (no GUI at launch) | Plist path must be trusted; unsigned plists are editable by the same UID | Binds job to signed bundle (BTM) **[Inf]** | Unchanged |
| Installation | First-run action outside the app: none exists | Writing `~/Library/LaunchAgents` is an installer-like act | `register()` at first GUI launch; user may need to approve in Login Items & Extensions **[Inf]** | None |
| Updates | Process outlives bundle replacement | Same | Same; association survives bundle replacement **[Unverified]** (S5) | Same |
| AppKit/Dock | Tag remains if GUI starts it **[V-exp]** | Clean **[V-exp]** | Clean only if S1 passes | Tag remains **[V-exp]** |
| Operational complexity | Highest (own supervision, own cleanup) | Medium | Lowest of the working options | Lowest, but does not work |

Rejected:

- **D** cannot meet the requirement. Five different spawn/ownership variants all keep the app as `(exited-with-subordinates)` (section 4). Keeping the kill (status quo) violates V1.
- **A** requires a non-GUI launcher that is not a descendant of the GUI. That reinvents launchd poorly and still needs supervision, cleanup and login/logout handling that launchd provides.

**D'. Remove the Quit kill and accept the LaunchServices state.** If S1 fails, this is the fallback: the Runtime survives Quit and relaunch (E1-E3 show survival), but Seyal remains listed as `(exited-with-subordinates)`. Its user-visible effect (Dock indicator, relaunch behaviour, `open` semantics) was **not measured** (the experiment ran hidden with `open -g -j`) and must be observed in S1 before D' can be judged acceptable.

Not a new daemon: **B/C reuse the existing `seyal-runtime` binary and its singleton**. launchd owns only the process container (start, environment, domain lifetime). It adds no second authority.

B versus C: C is the recommended packaging because it registers a job tied to the signed bundle with user-visible consent and unregistration, and needs no writes to `~/Library/LaunchAgents`. If S1 shows C does not work for a supported distribution (self-built or ad-hoc), B-style behaviour for those builds is a maintainer decision (D2).

## 4. macOS lifecycle experiment (verified behaviour)

Purpose: distinguish alternatives A-D on the one question that decides them: after intentional Quit, does the helper survive, and does LaunchServices still treat the app as running?

Method: throwaway ad-hoc-signed `.app` bundles built with `swiftc`, each starting a trivial `sleep` loop helper from `Contents/Helpers`. The app is launched with `open -g -j -n`, quit with `osascript ... to quit`, then `lsappinfo list`, `ps` and `launchctl print` are inspected. A control with no surviving helper is included. Bundles and test processes were removed afterwards. Run on macOS 27.2, arm64, 2026-11-19. Source of the harness is small enough to recreate from the table; it is not committed.

| ID | Helper started how | Helper survived Quit | App state after Quit **[V-exp]** |
|---|---|---|---|
| E1 | `posix_spawn` with `POSIX_SPAWN_SETPGROUP` (current launcher flags) | Yes (`ppid` 1) | `(exited-with-subordinates)` for the full 9 s observed |
| E2 | E1 plus `responsibility_spawnattrs_setdisclaim` (current launcher) | Yes | `(exited-with-subordinates)` |
| E3 | `POSIX_SPAWN_SETSID` plus disclaim | Yes | `(exited-with-subordinates)` |
| E4 | App ran `launchctl bootstrap` of a plist for the helper | Yes | `(exited-with-subordinates)`, `loadedJobs:"dev.exp.launchd.helper"` shown on the app entry |
| E5 | App launched an `LSUIElement` helper app through `NSWorkspace.openApplication` | Yes | `(exited-with-subordinates)` |
| E6 | Job bootstrapped into `gui/<uid>` from outside the app; app only attached | Yes | No LaunchServices entry for the app |
| E7 | Same job registered outside the app with no `RunAtLoad`; the app started it with `launchctl kickstart gui/<uid>/<label>` | Yes (helper `ppid` 1) | No subordinate tag |
| E0 (control) | E1 helper killed before Quit | n/a | No LaunchServices entry |

Findings:

1. **[V-exp]** Process-group, session and responsibility attributes do not change the LaunchServices subordinate state. The current launcher's flags are exactly E2.
2. **[V-exp]** A job the app itself loads into launchd (E4) and an app launched through LaunchServices (E5) are still attributed to the app.
3. **[V-exp]** What matters is who registered the launchd job. A job registered outside the app is not attributed to it, even when the app later starts it (E7).
4. **[V-exp]** On-demand start of an already-registered job needs no `RunAtLoad`: an idempotent `launchctl kickstart` from the app started it and it survived Quit.
5. **[Unverified, decisive]** E6/E7 used a manual `launchctl bootstrap` from a shell, standing in for registration. `SMAppService.agent(...).register()` was not exercised: it needs a signed bundle, may show a user-facing notification and changes the user's Login Items, which this PR must not do on a developer machine. Because `register()` is called from inside the app, it may be attributed like E4. This is gate S1 and the recommendation is conditional on it.
6. **[Limit]** The helper was a `sleep` loop, not a Runtime with live PTY children, and the app was quit with `osascript`; no app `SIGKILL` was exercised. The reparenting-to-launchd on GUI crash is therefore **[Inf]**, supported by SPEC-009 §7 and source (no parent watcher), not by this experiment.

Measured Runtime footprint **[V-meas, indicative only]**: a debug build of the existing `seyal-runtime` (`--runtime-dir` temporary directory) reached a bound `control.sock` in 14.1 ms and, after 3 s idle, showed 3,520 KiB RSS, 0.0 % CPU, 1 thread. Single run, debug build, no executions. Not a performance claim; section 9 sets the measurement the implementation must freeze.

## 5. How comparable tools manage independent execution lifetime [Unverified-secondary]

| Tool | Server model | Relevance |
|---|---|---|
| tmux | A single server process per socket survives client exit; clients start it on demand; it ends when the last session ends unless configured otherwise | Confirms the server-is-truth model Seyal already has; on-demand start is the same idea as B/C |
| WezTerm | Optional `wezterm-mux-server`; by default local panes live inside the GUI process and die with it; unix-domain mux makes them survive | Shows survival requires a process boundary that is not the GUI, which Seyal already has |
| Zellij | Per-session server process; clients are disposable; session resurrection is a separate, lossy feature | Matches the rule that persistence must never be presented as live PTY continuity |
| cmux | Not independently verified in this session; no claim made | n/a |

None of these is a macOS GUI-app packaging precedent for avoiding the Dock subordinate state; the macOS-specific decision rests on section 4.

## 6. Proposed architecture

### 6.1 Ownership (one authority per concern)

```text
launchd (user domain gui/<uid>)         owns: the Runtime *process container*
                                          (start, clean environment, domain teardown)
                                          does NOT own: executions, attachments, identity

seyal-runtime (existing binary)          owns: RuntimeId, singleton, endpoint, registry,
                                          executions, PTYs, TerminalState, attachments,
                                          Controller leases, explicit termination

Seyal.app / CLI / future clients         own: presentation and one-shot "ensure running"
                                          request; never spawn, signal or kill the Runtime
```

The Runtime remains the sole authority for execution termination and attachment management. launchd authority is limited to starting the same binary and ending it with the user session.

### 6.2 Lifecycle and ownership diagram

```text
              register once                        kickstart (idempotent)
Seyal.app ───────────────► SMAppService agent ───────────────► launchd job "dev.seyal.Seyal.runtime"
   │                        (signed bundle, user-visible)             │ exec Contents/Helpers/seyal-runtime
   │                                                                   ▼
   │  connect control.sock (unchanged protocol) ┌──────────────────────────────────────┐
   └───────────────────────────────────────────►│ seyal-runtime: singleton, registry,   │
CLI ────────────────────────────────────────────►│ executions (PTY + TerminalState)      │
   (same discovery + at most one ensure-running) └──────────────────────────────────────┘
                                                      ▲ survives: GUI Quit, GUI crash, relaunch
                                                      ✗ ends with: logout/shutdown, crash, explicit end
```

### 6.3 Startup and singleton (proposed changes to SPEC-009 §8.1.1)

1. GUI/CLI performs the existing discovery attempt. If connectable and peer validation (section 7) passes, use it.
2. If the endpoint is missing, the client performs at most one **ensure-running** action per foreground recovery episode (the existing budget: 7 connection attempts, 1 s ceiling, one launch action). Ensure-running is: (a) verify the bundled program's signature exactly as `evaluateHelperTrust` does today (identifier, Team ID, no entitlements, Release rejects ad-hoc); (b) `launchctl kickstart gui/<uid>/<label>` without `-k`, so a running job is untouched. `launchctl` is `/bin/launchctl`, no shell, fixed argument vector, scrubbed environment, `<uid>` from `geteuid()` with euid 0 refused, only the exit status mapped to a structured code, stdout/stderr never logged. The label is not trusted by itself (finding: label squatting): the post-connect peer validation in section 7 is the real gate; a `launchctl print` program-path comparison before kickstart is advisory only because it is racy.
3. If the job is not registered (first run, CLI-only machine, user revoked it), ensure-running does **not** register it: registration needs the signed app and user consent, so only the GUI may do it. A CLI without registration reports `launchdRegistrationMissing` and tells the user to open Seyal once. No silent fallback spawn (L19).
4. launchd serializes starts of one label. The Runtime-side `flock` singleton and Runtime-only socket removal/bind stay as they are, but are **a collision backstop, not a security control**: the lock parent directory is created before it is owner/mode verified (`singleton.rs` before `create_verified_runtime_dir`), and stale-socket removal is a connect-then-unlink with a same-UID TOCTOU window. Slice 2 fixes the ordering.
5. A concurrent GUI and CLI both reach step 2; the second kickstart is a no-op; both converge on the same canonical socket.
6. The client no longer calls `posix_spawn`, records a helper pid, runs a reaper, or terminates the Runtime on Quit.
7. The start mechanism (kickstart versus a launchd `MachServices` lookup) and whether launchd start latency and `ThrottleInterval` fit the 1 s episode are **open until gate S2**; sections 6.3 and 12 change with its result.

### 6.4 Runtime job definition (proposed)

- Label `dev.seyal.Seyal.runtime`, bundle program `Contents/Helpers/seyal-runtime` (existing path and signing identifier). Debug, UI-test and multiple-app-copy builds must not share the Release label or collide with it: the test/Debug harness uses an isolated `--runtime-dir` (`IsolatedRuntimeDirectory`) that a static plist cannot express, so test builds keep a non-launchd, test-only start (never compiled into Release) or a build-scoped label. Which one is a slice-3 decision; the ADR must state that exactly one Release label exists per user.
- No `RunAtLoad`, no `KeepAlive`, no sockets, no `StartInterval`. Idle cost when never started: zero.
- `EnvironmentVariables` pins an explicit minimal environment (`PATH=/usr/bin:/bin:/usr/sbin:/sbin`). launchd's domain environment can carry values set by any same-UID process (`launchctl setenv`), so the Runtime **allowlists its own environment at startup** and re-verifies `TMPDIR` ownership and mode as the launcher does today, rather than trusting the plist. Locale (`LANG`/`LC_CTYPE`) can no longer be copied from the GUI; its source for child shells is gate S7 and an ADR-020 §3.8 note.
- No `StandardOutPath`/`StandardErrorPath` (default `/dev/null`, as the launcher uses today).
- `ProcessType`, `ThrottleInterval` and `ExitTimeOut` are to be set by S2/S3: a GUI-less PTY pump must not be classed as a background/low-priority process, and logout-time `SIGTERM` followed by `SIGKILL` after the exit timeout is the L10 behaviour.
- No root, no `LaunchDaemons`, no privileged helper.
- Crash: not auto-restarted (no `KeepAlive`). The next client's ensure-running starts a new Runtime. A Runtime crash loses live PTYs (SPEC-003 §4, #924 deferred); the UI says so. No replay or resurrection is claimed.

### 6.5 Termination ownership

- Quit, window close and GUI crash never terminate the Runtime or any execution (ADR-005, ADR-017 §6.1, ADR-018 §3.3). The Quit kill is removed.
- Explicit execution termination remains Controller-fenced `TerminateExecution` (ADR-017 §6.2).
- Ending the Runtime deliberately is **not designed here and no GUI action may end it by signal or launchd removal.** SPEC-003 §4.1 and ADR-022 §7 reserve that for the authenticated SPEC-003 §16 path, owned by #1515 (WI-3/AC16). Consequently the GUI offers at most **unregister, effective at next Runtime exit**, never "stop now".
- Ways the Runtime can still end without Seyal's consent, recorded as unprompted-loss modes rather than authorities: logout/shutdown (default-disposition SIGTERM, no §16 drain, G12), crash, and the user or Background Task Management revoking the background item in System Settings, which may boot the job out. S1 verifies what the toggle does to a running job; registration UX warns that disabling it can end running sessions.
- A same-UID process can `launchctl bootout` the job; this is inside the accepted same-UID boundary (SPEC-009 §13) and is stated, not mitigated.

### 6.6 Zero-execution idle behaviour

Keep SPEC-003 §4.1: the Runtime stays resident at zero executions. Idle exit is **rejected for now** because workspace identity is Runtime-owned in memory until #832/#687 land; exiting at zero would silently reset `RuntimeId` and Workspace state. Revisit when persistence is accepted (D3). On-demand launchd start makes a later idle exit cheap.

## 7. Security model

Boundary unchanged: same effective UID is the trust boundary for local IPC (SPEC-004 §4, SPEC-009 §13). **This design moves trust; it does not by itself strengthen it. Net effect is gated on S4.**

What exists today **[V-src]**: UID-only `getpeereid` on both sides (`auth.rs`, `verify_connected_peer_fd`), which gives no same-UID code identity; plus a GUI-side pre-launch gate: strict `SecStaticCodeCheckValidity`, helper identifier and Team ID requirement, no entitlements, Release rejects ad-hoc; plus a from-scratch helper environment with `TMPDIR` lstat checks.

| Concern | Today | Proposed |
|---|---|---|
| Who may start the Runtime | The GUI, after its own code-identity gate | Whoever can run `launchctl kickstart` in the user's domain (same UID). Client keeps the pre-start signature check on the bundled program; it does not rely on BTM re-verifying the binary at each start **[Inf]**, and the bundle may be user-writable (`~/Applications`) |
| Label squatting | n/a | A same-UID process could register the label first and be kickstarted. Mitigation is the post-connect check below, not the label |
| Runtime code identity | Checked before spawn | Checked **after connect** by every client: resolve the peer through `LOCAL_PEERTOKEN` (audit token) and verify with Code Signing Services against identifier and Team ID. **Release fails closed** if this is unavailable; only Debug/ad-hoc builds may degrade, mirroring the launcher's compile-time rule. No pid-based fallback (it would be the PID-reuse TOCTOU the design avoids). A CLI must be signed with the same Team ID to validate or be refused. Proven or rejected by S4 |
| Same-UID impostor holding `runtime.lock` and binding `control.sock` first | Not prevented; real Runtime exits `AlreadyRunning`, launchd shows a clean on-demand exit | Peer validation refuses the impostor with a fixed `RuntimeMismatch`-class error, never kills it. Residual: an impostor that passes no validation sees nothing; one that fails is refused before any input is sent. Must be shown in S4 |
| Stale socket | Runtime-only verified removal | Unchanged, with the lock-parent verification fix |
| PID reuse | Not relied on | Peer identity is a connected-socket property |
| Environment | Built from scratch by the GUI | **Different, not stronger**: launchd domain environment is not inherited from the GUI but can be modified by any same-UID process. Compensated by the plist pin and Runtime-side allowlist (6.4) |
| Root | Not used | Not used |
| User consent/visibility | None | Appears under Login Items & Extensions; user may disable it (6.5 unprompted-loss mode) |
| TCC / attribution | GUI spawn uses responsibility disclaim | Children of a launchd job are attributed to the Runtime binary, not `Seyal.app`; Desktop/Documents/Full Disk Access prompts and consent may change. **[Unverified]**, S1 observes |
| Security updates | Old Runtime keeps running | A compatible-protocol security fix never reaches a running Runtime until it exits. **Proposed:** Hello carries a security-minimum Runtime build/epoch; a GUI below it reports a `RuntimeMismatch`-family state and offers the user-confirmed restart path (ADR-022); a downgraded GUI cannot silently attach to a Runtime it cannot validate. Wire change is a separate protocol proposal, flagged here, not designed |
| Orphan Runtime after app deletion | n/a | Persists until logout with no UI (L16). Uninstall guidance must include unregistering first; manual cleanup commands documented; S6 observes |
| Multi-user | Per-user directory ownership | Separate `gui/<uid>` domains and sockets; a backgrounded user's Runtime and shells (which may hold secrets) keep running while another user is at the console |
| Logging | Redacted structured codes | Unchanged; new codes `launchdRegistrationMissing`, `launchdStartFailed` are structural only; no `launchctl` text logged |

## 8. Required behaviour (lifecycle and failure matrix)

"Same" means: same `RuntimeId`, same `ExecutionId` set, same PTY identity (slave device path, child controlling tty and process start time unchanged), no replacement PTY, and canonical `TerminalState` (grid hash and scrollback length) intact.

| ID | Scenario | Required behaviour | Evidence class |
|---|---|---|---|
| L1 | First GUI launch, no Runtime | GUI registers if absent (user consent), then one ensure-running; GUI provisions the initial Pane | **[Proposed]** |
| L2 | Close window / last window | Presentation torn down; attachments detached; Runtime and executions live | **[V-src]** |
| L3 | Intentional Cmd+Q (confirm per ADR-018 §4.0 if owed) | Bounded detach (existing §4.1), app exits; Runtime and executions live; no Dock entry or subordinate state | Survival and no tag with out-of-app registration **[V-exp]** E6/E7 (sleep helper); with real `register()` **[Unverified]** S1 |
| L4 | GUI crash or `SIGKILL` | Runtime revokes the dead connection's attachments and Controller lease, keeps executions (SPEC-009 §7) | **[V-src]**; reparenting to launchd **[Inf]** (no app SIGKILL was run in section 4) |
| L5 | GUI relaunch / same-execution reconnect | Discovery, Hello/`RuntimeId`, attach the same executions (inventory per #1515), no new PTY; input, Control-C and resize work after reattach | **[Proposed]** composition of paths covered by SPEC-009 §15 detach/reconnect tests; named in slice 7 |
| L6 | CLI with no GUI | Same discovery plus one ensure-running; if not registered, `launchdRegistrationMissing` (a CLI cannot register); never needs the GUI process | **[Proposed]**; CLI design belongs to #1515/#262 |
| L7 | GUI and CLI start concurrently | launchd serializes; singleton is a collision backstop; one Runtime process | **[Proposed]**; A5 |
| L8 | Runtime crash | PTYs gone. Next client starts a new Runtime with a **different `RuntimeId`**; UI states live sessions were lost; no claim of continuity; no auto-restart | **[V-src]** SPEC-003 §4 |
| L9 | Explicit Runtime shutdown | Not designed here (6.5); must use the SPEC-003 §16 drain and report incomplete shutdown honestly | #1515 WI-3 |
| L10 | User logout | The user's domain is torn down; the Runtime gets default-disposition SIGTERM then SIGKILL after the exit timeout; every PTY ends, no drain (G12). Not preventable by an app | **[Inf]**, S6 observes |
| L11 | Shutdown / reboot | All processes end. Durable restore (#832/#687) may start new executions with new ids, never the same PTYs | Honest limit |
| L12 | Fast user switching / multiple accounts | Separate domains, temp directories and sockets; a backgrounded user's Runtime continues until that user's logout **[Inf]**; no shared endpoint | S6 |
| L13 | GUI update, compatible protocol and security epoch | App replaced; running Runtime continues on its already-mapped image; new GUI attaches; no restart | **[Inf]** under this hosting model (older flow evidence in `docs/evidence/m004-688-sparkle-g2-g5.md`); S5 |
| L14 | Update, incompatible protocol, missing capability or below security epoch | `RuntimeMismatch`; Runtime untouched; user informed. **There is no accepted path that ends the old Runtime short of logout/reboot until SPEC-003 §16 exists**; ADR-022's `Deferred(incompatible_runtime)` stays the honest state | **[V-src]** existing contract; fix path is #1515 |
| L15 | Rollback to an older GUI | Same negotiation in the other direction; may refuse a Runtime it cannot validate; no auto downgrade or kill | **[Proposed]** |
| L16 | Application deleted while Runtime live | Orphan persists until logout (no UI to see or stop it). Guidance: unregister before deleting; documented manual cleanup. launchd dropping the dangling job at next login is **[Unverified]** (S6). The GUI offers unregister-at-next-exit only (6.5) | **[Proposed]** |
| L17 | Zero-execution idle | Resident, near-zero CPU, wakeups only the existing 30 s timeout | **[Unverified]**; one debug RSS/CPU sample exists, wakeups not measured |
| L18 | Stale endpoint, wrong owner, symlink | Unchanged fail-closed. **Same-UID forged endpoint** is *not* caught by UID checks; it is caught only by post-connect peer validation (section 7) | **[V-src]** + S4 |
| L19 | Registration missing, denied, revoked, or code signature invalid | Bounded non-secret state; explicit "enable background service" action; no silent fallback spawn | **[Proposed]** |
| L20 | Self-built / ad-hoc build | Maintainer decision D2. A12 is conditional on D2 | Decision |
| L21 | Kickstart fails, throttled, or races a job that is exiting | Counted within the single ensure-running; episode ends in a bounded state; no retry loop; no kill | S2 |
| L22 | Hung Runtime (holds lock, never serves) | No kill authority exists. Bounded diagnostic state; recovery only by logout or the §16 path | Gap recorded; #1515 |
| L23 | Debug / UI-test / second app copy | Never shares or hijacks the Release label (6.4) | Slice 3 |
| L24 | Quarantined or translocated first launch; Team ID change or re-sign | Registration fails or is revoked; handled as L19 | S1 |
| L25 | Quit while the Runtime is still starting | Quit proceeds; Runtime is left to start and idle; no kill | **[Proposed]** |
| L26 | CLI over SSH (no `gui/<uid>` domain) | Report unavailable honestly; `user/<uid>` fallback is S2 | S2 |

## 9. Numerical requirements

These are **proposed ceilings, not measurements and not gating until the ADR is accepted and the baseline is run**. Methodology reuses SPEC-009 §16.1 (20 warm-ups, 5 cohorts of 100, report p50/p95/p99/max; RSS as the median of five samples at a quiescent point). Existing M002 budgets (`M002-673-THRESHOLD-DECISIONS.md`: idle CPU 1/3/5 %; its startup budget measures `spawn`-to-first-byte, a different quantity) remain authoritative where they overlap; the idle-CPU figure below is a stricter target that must be stated as such when frozen.

| Quantity | Proposed target and clock | Basis |
|---|---|---|
| Start latency, release build, p95 | `kickstart` exec to a successful `connect()` ≤ 150 ms, cold (job not running, endpoint absent) and warm cohorts reported separately | Direct debug spawn measured 14.1 ms **[V-meas, single debug run]**; launchd cost unmeasured (S3) |
| GUI relaunch to usable | Endpoint connect to all prior executions attached and current state committed within the existing 1 s foreground recovery ceiling, clock starting at the first connect attempt | SPEC-009 §8.1 |
| Idle RSS, zero executions, release | median-of-5 ≤ 16 MiB | Debug 3.4 MiB, one sample, no executions |
| Idle CPU / wakeups | ≤ 0.1 % over a 10 min window measured with `top`/`ps`; wakeups counted with a named tool (`dtrace`/`powermetrics`) in S3; the 30 s loop is itself a known baseline (G10) | Unmeasured |
| Terminal hot path | No protocol or data-path change; verified by the unchanged throughput/latency benchmarks (>5 % explain, >10 % block per SPEC-009 §16) | Design |
| Added resident processes | 0 | Design |
| Launch actions per recovery episode | at most 1 (ensure-running); registration is a separate GUI-only, once-per-install action outside the episode budget | SPEC-009 §8.1 |

## 10. Integration boundaries

- #1515/#1516: consumes this lifetime contract (Runtime survives Quit/crash/relaunch; Runtime death loses PTYs). Inventory, reattach, controller handoff and shutdown stay with #1515.
- #1517: may assume the Runtime (and notification state it owns) outlives the GUI.
- #1494: workspace presentation containment untouched.
- #1514: the new `launchd*` codes must be coordinated with its code set; not edited here.
- #832/#687: untouched; this document only forbids claiming PTY resurrection.
- Commercial/cloud/agent/licensing: no dependency (ADR-003).

## 11. Proposed amendments (not applied in this PR)

For the follow-on ADR PR; per ISSUE-PROTOCOL not mixed with implementation.

1. **New ADR (the only new ADR):** "Runtime hosting by per-user launchd job". It owns the hosting model, the ownership split (6.1), the unprompted-loss modes, and the label/registration rules. It cites ADR-018 §4.0's own call for "a separate Architecture Issue" (this issue is that issue).
2. **ADR-018:** §4.0 final bullet (Quit no longer ends the Runtime), §12 non-goal "launchd/LSUIElement packaging" and the §13 item 4 text, the §3.3 close table row, and the internal `true`/`false` inconsistency for last-window close (G8).
3. **ADR-022:** non-goal wording ("login items", "launch daemons": a user LaunchAgent registered via `SMAppService` appears under Login Items & Extensions, so this is a real change, not a wording tweak); §2/§5 trust roots and artifact topology (the bundle gains a LaunchAgent plist; signing, notarization and package verification change); §7 (new GUI launches the bundled helper through SPEC-009 §8.1.1; update never restarts a running Runtime automatically).
4. **SPEC-029** §1/§20: the same non-goal and the release-record/package-verification text.
5. **SPEC-003 §4.1:** the "who may end the Runtime" list gains unprompted-loss modes (logout/shutdown, crash, background-item revocation); no new GUI authority.
6. **SPEC-009 §8.1.1:** replace GUI `posix_spawn` with ensure-running; add client peer identity validation; restate the environment contract (plist pin, Runtime allowlist, locale source); add the `launchd*` structured codes; drop helper-pid/reaper/kill text and the poisoned-environment/descriptor tests' GUI-spawn assumptions in favour of launchd-start equivalents.
7. **ADR-020 §3.8:** note on environment provenance for a launchd-started Runtime.
8. **Protocol (separate proposal, not designed here):** security-minimum Runtime build/epoch in Hello.

## 12. Gates and minimal ordered implementation slices

Gates that must pass **before the ADR is accepted** (none can be satisfied by this PR):

| Gate | Question | Method |
|---|---|---|
| S1 (hard precondition) | From a Developer ID signed, notarized bundle: does `SMAppService.agent(...).register()` start no subordinate state after kickstart + Cmd+Q; user-visible consent; what the Login Items toggle does to a running job; TCC attribution of child processes; self-built/ad-hoc behaviour; and, if it fails, the observable consequence of D' | `lsappinfo`, `launchctl print`, Dock observation on an authorized machine |
| S2 | kickstart versus `MachServices` trigger; `ThrottleInterval`, `ExitTimeOut`, `ProcessType`; `user/<uid>` fallback; latency inside the 1 s episode | Compare race, latency, failure codes |
| S3 | Cold/warm start latency, idle RSS/CPU/wakeups on a release build, using SPEC-009 §16.1 methodology | Measure |
| S4 | Peer code-identity validation on a connected Unix socket (`LOCAL_PEERTOKEN`), CLI signing, impostor-binds-first case | Prototype |
| S5 | Registration survives Sparkle bundle replacement and rollback with live PTYs | Update/rollback |
| S6 | Logout, backgrounded user, app deletion, dangling-job behaviour | Observe |
| S7 | Locale, `TMPDIR` and environment provenance for a launchd-started Runtime and its child shells vs ADR-020 | Compare and assert |

Slices (each independently reviewable; implementation begins only after the ADR is accepted). Order changed after review: baseline first, peer validation before the switch, and the switch and kill-removal are one slice.

1. **ADR-hosting** (docs only), after gates S1-S7.
2. **Baseline and Runtime hardening:** run the section 9 baseline on the current helper; make lock/socket derivation consistent regardless of `TMPDIR` (G7); verify the lock parent before locking; resolve G10.
3. **Peer identity validation** in discovery, shipped and enforced in Release before the hosting switch (section 7).
4. **Hosting switch (one slice):** bundle plist, registration, ensure-running, bounded `launchd*` failure states (no silent fallback), Debug/test label isolation, **and** removal of the Quit kill, helper pid, reaper and dependent ADR-018 text/tests. These cannot merge separately: removing the kill alone leaves a GUI-spawned Runtime in the subordinate state, and adding the plist alone leaves two lifecycle owners. The A1/A2/A5/A12 harnesses (section 13) are merge gates of this slice, not a later slice.
5. **Mismatch/security-epoch UX, registration UX and unregister-at-next-exit** (L14-L16, L19; protocol proposal for the epoch).
6. **Qualification and soak** (section 13); freezes section 9.

## 13. Acceptance and qualification scenarios

Execution classes: **R** = Rust/live-fixture in an isolated `--runtime-dir` (headless, CI-safe); **X** = isolated XCUI via the existing `make ui-test` harness; **H** = authorized headed macOS run on a dedicated machine or user account (never against a developer's live Runtime or desktop; production discovery cannot be relocated, so any H run affects the real endpoint and needs explicit authorization per #1486); **M** = manual. CI without a GUI session cannot run `launchctl gui/<uid>` scenarios, and that is stated rather than hidden.

Observable-to-failure map used by every scenario:

| Failure to detect | Observable |
|---|---|
| Replacement PTY | Per-execution slave device path, child pid, child process start time and controlling tty all unchanged; Runtime PTY master fd identity unchanged; total PTY count equal and no new PTY ever allocated (counter, not only equality) |
| ExecutionId/pid change | Full set equality, not a count |
| Lost scrollback | N numbered marker lines emitted before the event; after reattach, grid hash and scrollback length match, lines emitted while detached are contiguous with them |
| Double Runtime | Exactly one `seyal-runtime` process; launchd job pid, lock holder and socket listener all the same pid; one `RuntimeId` |
| Silent fallback spawn | Runtime parent pid is launchd (1); no GUI-parented helper exists; the client contains no `posix_spawn` of the Runtime (symbol/grep check plus an injected spawn counter) |

**A1 (central, class H, with R harness).** Launch Seyal → in separate panes start an interactive shell, an `ssh` session to a test host and a long-running agent process → record `RuntimeId`, every `ExecutionId`, child pids and start times, slave devices, Runtime pid → emit marker lines → Cmd+Q → verify (a) same pids and start times alive, Runtime pid unchanged, parent is launchd, (b) Seyal absent from the Dock and from `lsappinfo` with no subordinate entry → produce output while detached → reopen Seyal → verify the same `RuntimeId` and `ExecutionId` set attached, markers and detached output present and contiguous, input/Control-C/resize work in each pane, alternate-screen state intact, no new PTY allocated, one Runtime process.
**A2 (H+R).** A1 with `SIGKILL` of the GUI instead of Quit; also asserts the Controller lease is revoked and the new `AttachmentId` differs.
**A3 (H).** Close the last window; reopen from Dock; same checklist as A1 (valid under either ADR-018 last-window behaviour, G8).
**A4 (R+H).** CLI with no GUI running, registered job: list and attach, then start the GUI and see the same executions. Unregistered job: `launchdRegistrationMissing`.
**A5 (R + H).** (R) isolated cargo-level simultaneous-start test reusing SPEC-003 §19: one bound endpoint, losers exit. (H) 100 cold starts where "cold" means job booted out, socket removed, lock free; plus variants with a stale socket and kickstart racing the flock; assert one Runtime and zero client-surfaced errors; run on an authorized machine only.
**A6 (R).** Runtime `SIGKILL`: next client starts a Runtime with a different `RuntimeId`; UI states sessions were lost; no stale-socket failure; no continuity claim (reuses SPEC-009 §15 "RuntimeId validation prevents claiming old continuity").
**A7 (H).** Compatible GUI update with live PTYs: same Runtime pid and executions.
**A8 (R).** Incompatible protocol, forced with a fixture Runtime/version stub: `RuntimeMismatch`, Runtime untouched, no automatic action.
**A9 (H/M).** Logout/login and reboot: oracle is that the Runtime pid and every PTY child pid are gone, the new Runtime has a different `RuntimeId`, and the UI shows the lost-sessions state.
**A10 (H/M).** Two macOS users: independent Runtimes, no cross-access; the backgrounded user's Runtime and children are observed and recorded.
**A11 (H/M, provisional on S6).** App deleted while the Runtime is live: observe and document; unregister-at-next-exit behaves as 6.5.
**A12 (R+H, conditional on D2).** Registration missing, denied, revoked or signature invalid: bounded non-secret state; no silent fallback spawn. Becomes "documented fallback behaviour" if D2 keeps the helper for unsigned builds.
**A13 (H).** 24 h zero-execution soak sampled every 60 s: RSS, CPU, wakeups within section 9, no consistent positive RSS growth (a positive trend requires root cause per SPEC-009 §16.1); the host stays awake, power state recorded.
**A14 (R).** Existing terminal throughput/latency benchmarks unchanged: >5 % explained, >10 % blocks (SPEC-009 §16).
**A15 (R, 100 graceful + 100 abrupt cycles).** fd, attachment and Controller-lease leak counters return to baseline; a child that exits while detached is reported on reattach; descriptors are not inherited by child shells under a launchd-started Runtime (SPEC-003 §19 fd-hygiene tests re-run against the launchd start).
**A16 (R/H).** Failure rows: plist present but binary missing or signature invalid; kickstart non-zero or throttled; Runtime exits during the L1 episode; Quit while Starting; mid-exit race; no auto-restart after crash (no `KeepAlive`).

## 14. Verified versus proposed

| Verified (this PR) | Proposed or unverified |
|---|---|
| Source facts in section 2 | The recommended hosting model (sections 6-7), conditional on S1 |
| E1-E7 behaviour in section 4 (sleep helper, Quit via `osascript`, ad-hoc bundles) | `SMAppService` registration, consent UX, TCC attribution (S1) |
| Debug Runtime: 14.1 ms to socket, 3.4 MiB, 0 % CPU, 1 thread (one run, no wakeup count) | Every ceiling in section 9; idle wakeups |
| Helper survived Quit and was reparented to launchd in E1-E5 | Reparenting on app SIGKILL, logout, fast-user-switch, uninstall (S6) |
| Absence of GUI-lifetime watchers and signal handlers in the Runtime crates **[V-src, absence: grep for getppid/signal/sigaction/prctl/stdin-EOF watchers]** | Peer code-identity validation (S4), security epoch, start trigger (S2) |

## 15. Decisions for maintainers (not defects)

| ID | Decision | Recommendation |
|---|---|---|
| D1 | Adopt a per-user launchd job via `SMAppService` and amend ADR-022/SPEC-029 non-goals | Yes, only if S1 passes; otherwise D' |
| D2 | Behaviour for builds where registration is unavailable (self-built, ad-hoc) | Require registration and show L19 rather than silently keeping the killing helper; otherwise two lifecycle models must be maintained |
| D3 | Idle exit at zero executions | No until persistence lands |
| D4 | Quit UX: silent detach versus prompt | Silent detach by default; "End sessions" belongs to #1515 |
| D5 | Shutdown authority and wire path (also the only way to retire an incompatible old Runtime) | #1515 WI-3; out of scope here |
| D6 | Test/Debug start mechanism and Release label policy (6.4) | Test-only non-launchd start, one Release label |
| D7 | Security-minimum Runtime epoch in Hello | Yes, as a separate protocol proposal |

## 16. Defects found (confirmed against source, independent of decisions)

1. The Quit kill contradicts SPEC-003 §4.1 and ADR-018 §8 invariant 6 while ADR-018 §4.0 accepts it as an interim (G1). **[V-src]**
2. Lock path and socket path derive from different base directories (G7). **[V-src]**; impact **[Inf]**
3. ADR-018 is internally inconsistent on last-window close; `AppDelegate` follows the opposite of its accepted text (G8). **[V-src]**
4. The Quit helper-kill can exceed the 500 ms backstop (G9). **[Inf]**
5. SPEC-003 "must not poll" versus a 30 s timeout loop (G10). **[V-src]** / wake cost **[Unverified]**
6. The singleton lock parent directory is created before owner/mode verification (`singleton.rs` versus `create_verified_runtime_dir`). **[V-src]** per security review; fix in slice 2
7. No signal handler: logout SIGTERM ends the Runtime with no §16 drain (G12). **[V-src, absence]**

## 17. Non-goals

No second Runtime, PTY manager, session registry or daemon; no Runtime-crash PTY survival (#924); no durable persistence design; no workspace containment; no notification design; no inventory/CLI design; no GUI-initiated Runtime stop; no root; no cloud, agent or licensing dependency; no production code.

## 18. Review log

Three independent read-only reviews (architecture, security, test/qualification) were run against the first draft and consolidated into one revision: the recommendation was made conditional on gate S1 after the E4 versus E6/E7 distinction was challenged; a GUI-initiated Runtime stop was removed; the false "begin_shutdown reached by signal" claim was corrected (no handler exists); the security claims were restated as moving rather than strengthening trust, with client-side signature and peer validation kept; missing lifecycle states, amendments, execution classes, observable maps and evidence tags were added; and slices were reordered. Findings that remain open are gates S1-S7 and decisions D1-D7, not defects.

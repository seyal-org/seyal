import AppKit

struct PaneRecoveryPin: Sendable, Equatable {
    let version: UInt16
    let size: UInt16
    let flags: UInt16
    let paneLow: UInt64
    let paneHigh: UInt64
    let executionLow: UInt64
    let executionHigh: UInt64
    let bindingGeneration: UInt64
    let recoveryGeneration: UInt64

    init(_ target: SeyalAppPaneRecoveryTarget) {
        version = target.version
        size = target.size
        flags = target.flags
        paneLow = target.pane_lo
        paneHigh = target.pane_hi
        executionLow = target.execution_lo
        executionHigh = target.execution_hi
        bindingGeneration = target.binding_generation
        recoveryGeneration = target.recovery_generation
    }

    var ffiValue: SeyalAppPaneRecoveryTarget {
        var target = SeyalAppPaneRecoveryTarget()
        target.version = version
        target.size = size
        target.flags = flags
        target.reserved = 0
        target.pane_lo = paneLow
        target.pane_hi = paneHigh
        target.execution_lo = executionLow
        target.execution_hi = executionHigh
        target.binding_generation = bindingGeneration
        target.recovery_generation = recoveryGeneration
        return target
    }

    var executionIdentity: String {
        String(format: "%016llx%016llx", executionHigh, executionLow)
    }
}

enum RecoveryAttemptIdentity: Equatable {
    case primary(UInt64)
    case pane(PaneRecoveryPin)
}

/// Host effect executor for the Rust `RecoveryCoordinator`. Rust owns retry
/// delays, the episode deadline, launch-once and generation fencing; this
/// extension only opens/adopts handles, arms timers, launches the bundled
/// helper, disposes handles and reports truthful outcomes.
@MainActor
extension ProductChromeHostView {
    /// Recovery and presentation handoffs must honor the palette's focus
    /// ownership at the moment AppKit accepts the responder change.
    func requestComposerFocusIfPaletteAllows() -> Bool {
        guard !commandPalette.isOpen else { return false }
        return composer.focusEditor()
    }

    func commandPaletteDidDismiss() {
        guard pane.inputSurface.recoveryPresentationPending else {
            routeFocus()
            return
        }
        pane.inputSurface.syncInputRoutePresentation()
        pane.inputSurface.resetComposerRecoveryFocusRetries()
        _ = advanceRecoveryAfterPaletteFocus(
            requestFocus: { [weak self] in
                self?.pane.inputSurface.requestComposerFocusForRecovery() ?? false
            },
            advanceRecovery: { [weak self] in
                _ = self?.pane.inputSurface.advanceRecoveryPresentationIfReady()
            }
        )
    }

    @discardableResult
    func advanceRecoveryAfterPaletteFocus(
        requestFocus: () -> Bool,
        advanceRecovery: () -> Void
    ) -> Bool {
        guard requestFocus() else { return false }
        advanceRecovery()
        return true
    }

    func recoveryText(_ snapshot: SeyalAppSnapshot) -> String {
        let stage: String
        switch snapshot.recovery_stage {
        case UInt16(SEYAL_APP_RECOVERY_USABLE.rawValue):
            stage = "connected"
        case UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue):
            stage = "recovery exhausted"
        case UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue):
            stage = "blocked"
        case UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue):
            stage = "shell exited"
        case UInt16(SEYAL_APP_RECOVERY_RECONSTRUCTING.rawValue),
            UInt16(SEYAL_APP_RECOVERY_RESTORING.rawValue):
            stage = "restoring"
        case UInt16(SEYAL_APP_RECOVERY_DISCONNECTED.rawValue):
            stage = "disconnected"
        default:
            stage = "connecting"
        }
        if snapshot.recovery_stage == UInt16(SEYAL_APP_RECOVERY_USABLE.rawValue)
            || snapshot.recovery_stage == UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue)
        {
            return stage
        }
        return "\(stage) · attempts \(snapshot.recovery_attempts)"
    }

    /// #1379 Defect B: show one clear recovery action after primary shell exit
    /// and while a follow-on bootstrap attempt is blocked/exhausted so the user
    /// can retry without relaunching the app.
    func updateNewShellRecoveryAction(_ snapshot: SeyalAppSnapshot) {
        let stage = snapshot.recovery_stage
        let show = stage == UInt16(SEYAL_APP_RECOVERY_EXECUTION_ENDED.rawValue)
            || stage == UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue)
            || stage == UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue)
            || pane.inputSurface.preferFreshExecutionBootstrap
        newShellButton.isHidden = !show
        // Hide the Flow composer while the recovery action owns that slot.
        composer.isHidden = show
    }

    /// Explicit New shell after `exit` — bootstrap a fresh execution without
    /// re-pinning the dead identity (MetalSurfaceView+Recovery).
    ///
    /// Uses a direct `open_first` with a create-capable budget instead of the
    /// 1s reconnect episode: after shell exit the Runtime may need to retire a
    /// Controller lease and provision profile 0, which does not fit SPEC-009's
    /// short reconnect window.
    @objc func requestNewShellAfterExit() {
        pane.inputSurface.preferFreshExecutionBootstrap = true
        // CancelRecovery drops ExecutionEnded → Disconnected, which would let
        // startAutomaticBridgeRecoveryIfNeeded race a second 1s open against
        // this create-capable bootstrap (ControllerBusy / OPENED_REJECTED →
        // Blocked). Hold automatic recovery until adopt finishes.
        pane.inputSurface.suppressesAutomaticBridgeRecovery = true
        // stop() no-ops when already disconnected; shell-exit often leaves a
        // half-torn bridge (isConnected=false, clientHandle≠0, or
        // disconnectPending). Force the Controller lease drop before open.
        if pane.inputSurface.bridge?.clientHandle != 0
            || pane.inputSurface.bridge?.isConnected == true
        {
            pane.inputSurface.bridge?.completeEagerClientDisconnect()
            pane.inputSurface.bridge?.stop()
        } else {
            pane.inputSurface.bridge?.completeEagerClientDisconnect()
        }
        pane.inputSurface.isDetachingRuntimeConnection = false
        if pane.inputSurface.recoveryAppHandle != 0 {
            applyRuntimeRecoveryAction(
                pane.appHandle,
                kind: SEYAL_APP_ACTION_CANCEL_RECOVERY
            )
        }
        let identity = RecoveryAttemptIdentity.primary(
            seyal_app_snapshot(pane.appHandle).recovery_generation &+ 1
        )
        guard recoveryAttemptInFlight != identity else { return }
        recoveryAttemptInFlight = identity
        let issuedAt = ProcessInfo.processInfo.systemUptime
        recoveryLifecycleQueue.async { [weak self] in
            let outcome = openRuntimeRecoveryHandle(
                executionIdentity: nil,
                allowsImplicitExecutionBootstrap: true,
                remainingBudget: 15
            )
            DispatchQueue.main.async { [weak self] in
                seyalRunAsMainActorFromMainQueue {
                    guard let self else {
                        if case let .opened(opened) = outcome {
                            seyal_bridge_disconnect_handle(opened.handle)
                        }
                        return
                    }
                    self.finishDirectNewShellBootstrap(
                        outcome,
                        issuedAt: issuedAt,
                        identity: identity
                    )
                }
            }
        }
        reconcileChrome()
    }

    private func finishDirectNewShellBootstrap(
        _ outcome: RuntimeRecoveryAttemptOutcome,
        issuedAt: TimeInterval,
        identity: RecoveryAttemptIdentity
    ) {
        defer {
            if recoveryAttemptInFlight == identity {
                recoveryAttemptInFlight = nil
            }
        }
        let _ = issuedAt
        switch outcome {
        case let .opened(opened):
            // Drop any residual teardown fence that would reject adopt after
            // shell-exit disconnect (isConnected/disconnectPending).
            pane.inputSurface.bridge?.stop()
            pane.inputSurface.isDetachingRuntimeConnection = false
            // New shell is a fresh ExecutionId — clear the dead continuity pin
            // so commitRuntimeReconstruction does not fail closed as mismatch.
            applyRuntimeRecoveryAction(
                pane.appHandle,
                kind: SEYAL_APP_ACTION_DISCONNECT_RECONSTRUCTION
            ) {
                $0.reserved = 1
            }
            // Arm a fresh Rust episode, claim its PerformAttempt, then complete
            // with the handle we already opened (do not start a second open).
            let now = runtimeRecoveryNowMillis()
            applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_BEGIN_RECOVERY) {
                $0.target_generation = now
            }
            let generation = seyal_app_snapshot(pane.appHandle).recovery_generation
            if seyal_app_snapshot(pane.appHandle).recovery_effect
                == UInt32(SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT.rawValue)
            {
                ackRecovery()
            }
            var adopted = pane.inputSurface.adoptRecoveredHandle(opened)
            if !adopted {
                // One teardown retry: stop() may have raced a DispatchSource
                // cancel that re-armed disconnectPending after the first clear.
                pane.inputSurface.bridge?.completeEagerClientDisconnect()
                adopted = pane.inputSurface.adoptRecoveredHandle(opened)
            }
            if adopted {
                let result = completeRecovery(
                    generation: generation,
                    outcome: SEYAL_APP_RECOVERY_OPENED_ADOPTED,
                    handle: opened.handle
                )
                if result == 0 {
                    pane.inputSurface.preferFreshExecutionBootstrap = false
                    pane.inputSurface.suppressesAutomaticBridgeRecovery = false
                    pane.inputSurface.advanceRecoveryPresentationIfReady()
                } else {
                    seyal_bridge_disconnect_handle(opened.handle)
                    pane.inputSurface.preferFreshExecutionBootstrap = true
                    Self.logNewShellFailure("completeRecovery after adopt failed (\(result))")
                }
            } else {
                // Do NOT complete as RETRYABLE/OPENED_REJECTED: that burns the
                // 1s episode to Exhausted/Blocked while the Created shell may
                // already be Running. Cancel the episode and leave New shell
                // armed so the user can reattach without relaunching.
                seyal_bridge_disconnect_handle(opened.handle)
                applyRuntimeRecoveryAction(
                    pane.appHandle,
                    kind: SEYAL_APP_ACTION_CANCEL_RECOVERY
                )
                pane.inputSurface.preferFreshExecutionBootstrap = true
                Self.logNewShellFailure("adopt-rejected after open (episode cancelled)")
            }
        case .endpointMissing:
            // Missing Runtime: enter the launch-helper path, then let
            // driveRecovery open again after the helper binds.
            let now = runtimeRecoveryNowMillis()
            applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_BEGIN_RECOVERY) {
                $0.target_generation = now
            }
            let generation = seyal_app_snapshot(pane.appHandle).recovery_generation
            if seyal_app_snapshot(pane.appHandle).recovery_effect
                == UInt32(SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT.rawValue)
            {
                ackRecovery()
            }
            _ = completeRecovery(
                generation: generation,
                outcome: SEYAL_APP_RECOVERY_ENDPOINT_MISSING
            )
            pane.inputSurface.preferFreshExecutionBootstrap = true
        case .retryable, .controllerBusy, .executionEnded, .blocked:
            // Do not fold a failed create into the 1s reconnect episode — that
            // exhausts immediately and hides the real New shell failure mode.
            // Keep the explicit recovery action armed for another user click.
            Self.logNewShellFailure(String(describing: outcome))
            pane.inputSurface.preferFreshExecutionBootstrap = true
        }
        reconcileChrome()
    }

    private static func logNewShellFailure(_ detail: String) {
        let line = "seyal: New shell open_first failed (\(detail)); leaving New shell armed\n"
        NSLog("%@", line)
        if let data = line.data(using: .utf8) {
            let url = URL(fileURLWithPath: "/tmp/seyal-new-shell.log")
            if FileManager.default.fileExists(atPath: url.path) {
                if let handle = try? FileHandle(forWritingTo: url) {
                    defer { try? handle.close() }
                    _ = try? handle.seekToEnd()
                    try? handle.write(contentsOf: data)
                }
            } else {
                try? data.write(to: url)
            }
        }
    }

    func driveRecovery() {
        // A detached projection host has no window and must not open or launch
        // a Runtime. In-process tests construct those hosts beside the live window.
        guard window != nil else { return }
        let paneTarget = seyal_app_begin_next_pane_recovery(
            pane.appHandle,
            runtimeRecoveryNowMillis()
        )
        if paneTarget.size != 0 {
            drivePaneRecovery(paneTarget)
        } else if paneRecoveryTimerPin != nil {
            paneRecoveryTimer?.invalidate()
            paneRecoveryTimer = nil
            paneRecoveryTimerPin = nil
        }
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if recoveryTimer != nil, recoveryTimerGeneration != snapshot.recovery_generation {
            recoveryTimer?.invalidate()
            recoveryTimer = nil
            recoveryTimerGeneration = 0
        }
        switch snapshot.recovery_effect {
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT.rawValue):
            // CompleteRecovery replaces this effect; acking it would drop the
            // attempt. The in-flight fence keeps reconcile pulses from
            // starting a second open for the same generation.
            performRecoveryAttempt(generation: snapshot.recovery_generation)
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_SCHEDULE.rawValue):
            let delayMs = seyal_app_recovery_param(pane.appHandle)
            let generation = snapshot.recovery_generation
            scheduleRecoveryTimer(generation: generation, delayMs: delayMs)
            ackRecovery()
            driveRecovery()
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_LAUNCH_HELPER.rawValue):
            let generation = snapshot.recovery_generation
            guard seyal_app_recovery_param(pane.appHandle) == generation else {
                ackRecovery()
                driveRecovery()
                return
            }
            let launched = pane.inputSurface.bridge?.launchBundledRuntime() ?? false
            ackRecovery()
            // Any failed launch (missing, untrusted, denied, spawn failure)
            // blocks the episode; retrying cannot produce a Runtime.
            if !launched {
                completeRecovery(
                    generation: generation,
                    outcome: SEYAL_APP_RECOVERY_ENDPOINT_MISSING,
                    launch: SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING
                )
            }
            driveRecovery()
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_DISPOSE_HANDLE.rawValue):
            disposeRecoveryHandle(seyal_app_recovery_param(pane.appHandle))
            ackRecovery()
            driveRecovery()
        default:
            break
        }
    }

    private func drivePaneRecovery(_ target: SeyalAppPaneRecoveryTarget) {
        let effect = seyal_app_pane_recovery_effect(pane.appHandle, target)
        switch effect.kind {
        case UInt16(SEYAL_APP_RECOVERY_EFFECT_PERFORM_ATTEMPT.rawValue):
            performPaneRecoveryAttempt(target: target, remainingMs: effect.parameter)
        case UInt16(SEYAL_APP_RECOVERY_EFFECT_SCHEDULE.rawValue):
            let pin = PaneRecoveryPin(target)
            schedulePaneRecoveryTimer(pin: pin, delayMs: effect.parameter)
            _ = seyal_app_ack_pane_recovery_effect(pane.appHandle, target)
        case UInt16(SEYAL_APP_RECOVERY_EFFECT_LAUNCH_HELPER.rawValue):
            let launched = pane.inputSurface.bridge?.launchBundledRuntime() ?? false
            _ = seyal_app_ack_pane_recovery_effect(pane.appHandle, target)
            if !launched {
                _ = completePaneRecovery(
                    target: target,
                    outcome: SEYAL_APP_RECOVERY_ENDPOINT_MISSING,
                    launch: SEYAL_APP_RECOVERY_LAUNCH_HELPER_MISSING
                )
            }
            driveRecovery()
        case UInt16(SEYAL_APP_RECOVERY_EFFECT_DISPOSE_HANDLE.rawValue):
            disposeRecoveryHandle(effect.parameter)
            _ = seyal_app_ack_pane_recovery_effect(pane.appHandle, target)
            driveRecovery()
        default:
            break
        }
    }

    func performRecoveryAttempt(generation: UInt64) {
        let identity = RecoveryAttemptIdentity.primary(generation)
        guard recoveryAttemptInFlight != identity else { return }
        recoveryAttemptInFlight = identity
        // Capture the Rust-issued remaining budget BEFORE Ack. Acking pops
        // PerformAttempt from the pending queue; reading after Ack used to
        // return 0 and short-circuit every open to `.retryable` (no attach).
        let remainingMs = seyal_app_recovery_param(pane.appHandle)
        // Claim the level-triggered PerformAttempt so adopt → frame →
        // reconcile → driveRecovery cannot start a second open.
        ackRecovery()
        let issuedAt = ProcessInfo.processInfo.systemUptime
        let executionIdentity = pane.inputSurface.recoveryExecutionIdentity
        let allowsImplicit = pane.inputSurface.allowsImplicitExecutionBootstrap
        // #1379 Defect B: explicit New shell after exit needs a create+attach
        // budget larger than the 1s reconnect episode — open_first may need to
        // dispose a tombstoned Controller lease and provision profile 0.
        let openBudgetMs = pane.inputSurface.preferFreshExecutionBootstrap
            ? max(remainingMs, 15_000)
            : remainingMs
        recoveryLifecycleQueue.async { [weak self] in
            // Queue delay consumes the Rust-issued budget rather than resetting it.
            let elapsed = ProcessInfo.processInfo.systemUptime - issuedAt
            let outcome = openRuntimeRecoveryHandle(
                executionIdentity: executionIdentity,
                allowsImplicitExecutionBootstrap: allowsImplicit,
                remainingBudget: TimeInterval(openBudgetMs) / 1000 - elapsed
            )
            DispatchQueue.main.async { [weak self] in
                // Lifecycle work returns on the GCD main queue without a Swift
                // MainActor task; enter adopt/first-frame work via the hop.
                seyalRunAsMainActorFromMainQueue {
                    guard let self else {
                        if case let .opened(opened) = outcome {
                            seyal_bridge_disconnect_handle(opened.handle)
                        }
                        return
                    }
                    self.finishRecoveryAttempt(outcome, generation: generation)
                }
            }
        }
    }

    private func performPaneRecoveryAttempt(target: SeyalAppPaneRecoveryTarget, remainingMs: UInt64) {
        let pin = PaneRecoveryPin(target)
        guard target.size != 0, recoveryAttemptInFlight != .pane(pin) else { return }
        recoveryAttemptInFlight = .pane(pin)
        _ = seyal_app_ack_pane_recovery_effect(pane.appHandle, target)
        let issuedAt = ProcessInfo.processInfo.systemUptime
        recoveryLifecycleQueue.async { [weak self] in
            let elapsed = ProcessInfo.processInfo.systemUptime - issuedAt
            let outcome = openRuntimeRecoveryHandle(
                executionIdentity: pin.executionIdentity,
                allowsImplicitExecutionBootstrap: false,
                remainingBudget: TimeInterval(remainingMs) / 1000 - elapsed
            )
            DispatchQueue.main.async { [weak self] in
                seyalRunAsMainActorFromMainQueue {
                    guard let self else {
                        if case let .opened(opened) = outcome {
                            seyal_bridge_disconnect_handle(opened.handle)
                        }
                        return
                    }
                    self.finishPaneRecoveryAttempt(outcome, pin: pin)
                }
            }
        }
    }

    private func finishPaneRecoveryAttempt(
        _ outcome: RuntimeRecoveryAttemptOutcome,
        pin: PaneRecoveryPin
    ) {
        let identity = RecoveryAttemptIdentity.pane(pin)
        let ownedInFlight = recoveryAttemptInFlight == identity
        defer {
            if ownedInFlight, recoveryAttemptInFlight == identity {
                recoveryAttemptInFlight = nil
            }
        }
        let active = seyal_app_active_pane_recovery_target(pane.appHandle)
        let current = active.size == pin.size
            && active.pane_lo == pin.paneLow
            && active.pane_hi == pin.paneHigh
            && active.execution_lo == pin.executionLow
            && active.execution_hi == pin.executionHigh
            && active.binding_generation == pin.bindingGeneration
            && active.recovery_generation == pin.recoveryGeneration
        guard current else {
            if case let .opened(opened) = outcome {
                seyal_bridge_disconnect_handle(opened.handle)
            }
            reconcileChrome()
            return
        }
        var result: Int32
        var publishedHandle: UInt64?
        switch outcome {
        case let .opened(opened):
            guard seyal_bridge_adopt_handle(opened.handle) == 0 else {
                seyal_bridge_disconnect_handle(opened.handle)
                result = completePaneRecovery(
                    target: pin.ffiValue,
                    outcome: SEYAL_APP_RECOVERY_BLOCKED_OUTCOME
                )
                reconcileChrome()
                return
            }
            result = completePaneRecovery(
                target: pin.ffiValue,
                outcome: SEYAL_APP_RECOVERY_OPENED_ADOPTED,
                handle: opened.handle
            )
            if result == 0 {
                result = seyal_app_commit_pane_recovery(
                    pane.appHandle,
                    pin.ffiValue,
                    opened.handle
                )
            }
            if result == 0 {
                pane.inputSurface.bridge?.armAuxiliaryReadSources()
                publishedHandle = opened.handle
            } else {
                // A candidate rejected at the Rust ownership boundary must not
                // remain registered or paint after the Pane binding changed.
                seyal_bridge_disconnect_handle(opened.handle)
            }
        case .endpointMissing:
            result = completePaneRecovery(target: pin.ffiValue, outcome: SEYAL_APP_RECOVERY_ENDPOINT_MISSING)
        case .retryable:
            result = completePaneRecovery(target: pin.ffiValue, outcome: SEYAL_APP_RECOVERY_RETRYABLE)
        case .controllerBusy:
            result = completePaneRecovery(target: pin.ffiValue, outcome: SEYAL_APP_RECOVERY_CONTROLLER_BUSY)
        case .executionEnded:
            result = completePaneRecovery(
                target: pin.ffiValue,
                outcome: SEYAL_APP_RECOVERY_EXECUTION_ENDED_OUTCOME
            )
        case .blocked:
            result = completePaneRecovery(target: pin.ffiValue, outcome: SEYAL_APP_RECOVERY_BLOCKED_OUTCOME)
        }
        reconcileChrome()
        if let publishedHandle {
            pane.inputSurface.bridge?.onRegistryHandlePolled?(publishedHandle)
        }
    }

    func finishRecoveryAttempt(_ outcome: RuntimeRecoveryAttemptOutcome, generation: UInt64) {
        // Keep the in-flight fence until CompleteRecovery replaces PerformAttempt.
        // Clearing it before adopt let publishCurrentFrame → chrome reconcile →
        // driveRecovery start a second open for the same effect (main-thread
        // spin, empty Blocks, inactive composer).
        let identity = RecoveryAttemptIdentity.primary(generation)
        let ownedInFlight = recoveryAttemptInFlight == identity
        defer {
            if ownedInFlight, recoveryAttemptInFlight == identity {
                recoveryAttemptInFlight = nil
            }
        }
        let current = seyal_app_snapshot(pane.appHandle).recovery_generation == generation
        var adopted = false
        let result: Int32
        switch outcome {
        case let .opened(opened):
            // A superseded generation never adopts; Rust answers the stale
            // completion with DisposeHandle.
            adopted = current && pane.inputSurface.adoptRecoveredHandle(opened)
            if !adopted, current, pane.inputSurface.preferFreshExecutionBootstrap {
                // #1379: teardown races after shell exit must not permanently
                // Block New shell — dispose and retry within the episode.
                seyal_bridge_disconnect_handle(opened.handle)
                result = completeRecovery(
                    generation: generation,
                    outcome: SEYAL_APP_RECOVERY_RETRYABLE,
                    handle: 0
                )
            } else {
                result = completeRecovery(
                    generation: generation,
                    outcome: adopted
                        ? SEYAL_APP_RECOVERY_OPENED_ADOPTED
                        : SEYAL_APP_RECOVERY_OPENED_REJECTED,
                    handle: opened.handle
                )
            }
        case .endpointMissing:
            result = completeRecovery(generation: generation, outcome: SEYAL_APP_RECOVERY_ENDPOINT_MISSING)
        case .retryable:
            result = completeRecovery(generation: generation, outcome: SEYAL_APP_RECOVERY_RETRYABLE)
        case .controllerBusy:
            result = completeRecovery(generation: generation, outcome: SEYAL_APP_RECOVERY_CONTROLLER_BUSY)
        case .executionEnded:
            result = completeRecovery(
                generation: generation,
                outcome: SEYAL_APP_RECOVERY_EXECUTION_ENDED_OUTCOME
            )
        case .blocked:
            result = completeRecovery(generation: generation, outcome: SEYAL_APP_RECOVERY_BLOCKED_OUTCOME)
        }
        if current, result != 0 {
            // A rejected completion would leave PerformAttempt queued and
            // re-open immediately; end the episode instead of spinning.
            applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
        }
        if adopted {
            pane.inputSurface.advanceRecoveryPresentationIfReady()
        }
        reconcileChrome()
    }

    /// Rust may dispose a handle the surface already adopted (deadline passed
    /// during the MainActor hop); that must tear down the live client too.
    func disposeRecoveryHandle(_ handle: UInt64) {
        guard handle != 0 else { return }
        if let bridge = pane.inputSurface.bridge, bridge.clientHandle == handle {
            bridge.stop()
        } else {
            seyal_bridge_disconnect_handle(handle)
        }
    }

    @discardableResult
    func completeRecovery(
        generation: UInt64,
        outcome: SeyalAppRecoveryOutcome,
        launch: SeyalAppRecoveryLaunch = SEYAL_APP_RECOVERY_LAUNCH_NONE,
        handle: UInt64 = 0
    ) -> Int32 {
        let now = runtimeRecoveryNowMillis()
        return applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_COMPLETE_RECOVERY) {
            $0.target_execution_lo = generation
            $0.target_generation = now
            $0.target_attachment_lo = handle
            $0.reserved = UInt32(outcome.rawValue) | (UInt32(launch.rawValue) << 8)
        }
    }

    func fireRecovery(generation: UInt64) {
        guard recoveryTimerGeneration == generation else { return }
        recoveryTimer = nil
        recoveryTimerGeneration = 0
        guard seyal_app_snapshot(pane.appHandle).recovery_generation == generation else { return }
        let now = runtimeRecoveryNowMillis()
        applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_FIRE_RECOVERY) {
            $0.target_execution_lo = generation
            $0.target_generation = now
        }
        reconcileChrome()
    }

    func ackRecovery() {
        applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_ACK_RECOVERY)
    }

    private func firePaneRecovery(pin: PaneRecoveryPin) {
        guard paneRecoveryTimerPin == pin else { return }
        paneRecoveryTimer = nil
        paneRecoveryTimerPin = nil
        _ = seyal_app_fire_pane_recovery(pane.appHandle, pin.ffiValue, runtimeRecoveryNowMillis())
        driveRecovery()
    }

    func scheduleRecoveryTimer(generation: UInt64, delayMs: UInt64) {
        recoveryTimer?.invalidate()
        recoveryTimerGeneration = generation
        recoveryTimer = Timer.scheduledTimer(
            withTimeInterval: TimeInterval(delayMs) / 1000,
            repeats: false
        ) { [weak self] _ in
            DispatchQueue.main.async { self?.fireRecovery(generation: generation) }
        }
    }

    func schedulePaneRecoveryTimer(pin: PaneRecoveryPin, delayMs: UInt64) {
        paneRecoveryTimer?.invalidate()
        paneRecoveryTimerPin = pin
        paneRecoveryTimer = Timer.scheduledTimer(
            withTimeInterval: TimeInterval(delayMs) / 1000,
            repeats: false
        ) { [weak self] _ in
            DispatchQueue.main.async { self?.firePaneRecovery(pin: pin) }
        }
    }

    @discardableResult
    private func completePaneRecovery(
        target: SeyalAppPaneRecoveryTarget,
        outcome: SeyalAppRecoveryOutcome,
        launch: SeyalAppRecoveryLaunch = SEYAL_APP_RECOVERY_LAUNCH_NONE,
        handle: UInt64 = 0
    ) -> Int32 {
        seyal_app_complete_pane_recovery(
            pane.appHandle,
            target,
            UInt32(outcome.rawValue),
            UInt32(launch.rawValue),
            handle,
            runtimeRecoveryNowMillis()
        )
    }
}

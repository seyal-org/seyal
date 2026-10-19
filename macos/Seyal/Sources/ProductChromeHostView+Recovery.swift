import AppKit

private struct PaneRecoveryPin: Sendable {
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

    func driveRecovery() {
        let paneTarget = seyal_app_begin_next_pane_recovery(
            pane.appHandle,
            runtimeRecoveryNowMillis()
        )
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
            if paneTarget.size != 0,
                paneTarget.recovery_generation == snapshot.recovery_generation
            {
                performPaneRecoveryAttempt(generation: snapshot.recovery_generation, target: paneTarget)
            } else {
                performRecoveryAttempt(generation: snapshot.recovery_generation)
            }
        case UInt32(SEYAL_APP_RECOVERY_EFFECT_SCHEDULE.rawValue):
            let delayMs = seyal_app_recovery_param(pane.appHandle)
            let generation = snapshot.recovery_generation
            recoveryTimer?.invalidate()
            recoveryTimerGeneration = generation
            recoveryTimer = Timer.scheduledTimer(
                withTimeInterval: TimeInterval(delayMs) / 1000,
                repeats: false
            ) { [weak self] _ in
                DispatchQueue.main.async {
                    self?.fireRecovery(generation: generation)
                }
            }
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

    func performRecoveryAttempt(generation: UInt64) {
        guard recoveryAttemptInFlight != generation else { return }
        recoveryAttemptInFlight = generation
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
        recoveryLifecycleQueue.async { [weak self] in
            // Queue delay consumes the Rust-issued budget rather than resetting it.
            let elapsed = ProcessInfo.processInfo.systemUptime - issuedAt
            let outcome = openRuntimeRecoveryHandle(
                executionIdentity: executionIdentity,
                allowsImplicitExecutionBootstrap: allowsImplicit,
                remainingBudget: TimeInterval(remainingMs) / 1000 - elapsed
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

    func performPaneRecoveryAttempt(generation: UInt64, target: SeyalAppPaneRecoveryTarget) {
        guard target.size != 0,
            target.recovery_generation == generation,
            recoveryAttemptInFlight != generation
        else { return }
        recoveryAttemptInFlight = generation
        let remainingMs = seyal_app_recovery_param(pane.appHandle)
        ackRecovery()
        let issuedAt = ProcessInfo.processInfo.systemUptime
        let pin = PaneRecoveryPin(target)
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
                    self.finishPaneRecoveryAttempt(outcome, pin: pin, generation: generation)
                }
            }
        }
    }

    private func finishPaneRecoveryAttempt(
        _ outcome: RuntimeRecoveryAttemptOutcome,
        pin: PaneRecoveryPin,
        generation: UInt64
    ) {
        let ownedInFlight = recoveryAttemptInFlight == generation
        defer {
            if ownedInFlight, recoveryAttemptInFlight == generation {
                recoveryAttemptInFlight = nil
            }
        }
        let active = seyal_app_active_pane_recovery_target(pane.appHandle)
        let current = seyal_app_snapshot(pane.appHandle).recovery_generation == generation
            && active.size == pin.size
            && active.pane_lo == pin.paneLow
            && active.pane_hi == pin.paneHigh
            && active.execution_lo == pin.executionLow
            && active.execution_hi == pin.executionHigh
            && active.binding_generation == pin.bindingGeneration
            && active.recovery_generation == pin.recoveryGeneration
        var result: Int32
        var publishedHandle: UInt64?
        switch outcome {
        case let .opened(opened):
            guard current, seyal_bridge_adopt_handle(opened.handle) == 0 else {
                seyal_bridge_disconnect_handle(opened.handle)
                result = completeRecovery(
                    generation: generation,
                    outcome: SEYAL_APP_RECOVERY_OPENED_REJECTED,
                    handle: opened.handle
                )
                if current, result != 0 {
                    applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
                }
                reconcileChrome()
                return
            }
            result = completeRecovery(
                generation: generation,
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
                if current {
                    applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
                }
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
            applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
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
        let ownedInFlight = recoveryAttemptInFlight == generation
        defer {
            if ownedInFlight, recoveryAttemptInFlight == generation {
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
            result = completeRecovery(
                generation: generation,
                outcome: adopted
                    ? SEYAL_APP_RECOVERY_OPENED_ADOPTED
                    : SEYAL_APP_RECOVERY_OPENED_REJECTED,
                handle: opened.handle
            )
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
            $0.target_pty_generation = now
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
            $0.target_pty_generation = now
        }
        reconcileChrome()
    }

    func ackRecovery() {
        applyRuntimeRecoveryAction(pane.appHandle, kind: SEYAL_APP_ACTION_ACK_RECOVERY)
    }
}

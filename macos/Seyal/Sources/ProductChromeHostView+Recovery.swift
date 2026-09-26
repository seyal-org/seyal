import AppKit

/// Host effect executor for the Rust `RecoveryCoordinator`. Rust owns retry
/// delays, the episode deadline, launch-once and generation fencing; this
/// extension only opens/adopts handles, arms timers, launches the bundled
/// helper, disposes handles and reports truthful outcomes.
@MainActor
extension ProductChromeHostView {
    func recoveryText(_ snapshot: SeyalAppSnapshot) -> String {
        let stage: String
        switch snapshot.recovery_stage {
        case 6: stage = "connected"
        case 7: stage = "recovery exhausted"
        case 8: stage = "blocked"
        case 4, 5: stage = "restoring"
        case 0: stage = "disconnected"
        default: stage = "connecting"
        }
        if snapshot.recovery_stage == 6 {
            return stage
        }
        return "\(stage) · attempts \(snapshot.recovery_attempts)"
    }

    func driveRecovery() {
        let snapshot = seyal_app_snapshot(pane.appHandle)
        if recoveryTimer != nil, recoveryTimerGeneration != snapshot.recovery_generation {
            recoveryTimer?.invalidate()
            recoveryTimer = nil
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
        let remainingMs = seyal_app_recovery_param(pane.appHandle)
        let issuedAt = ProcessInfo.processInfo.systemUptime
        let executionIdentity = pane.inputSurface.requestedExecutionIdentity
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

    func finishRecoveryAttempt(_ outcome: RuntimeRecoveryAttemptOutcome, generation: UInt64) {
        if recoveryAttemptInFlight == generation {
            recoveryAttemptInFlight = nil
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

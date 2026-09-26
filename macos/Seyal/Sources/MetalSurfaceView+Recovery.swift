import AppKit
import Foundation

extension MetalSurfaceView {
  /// Snapshot recovery stage (`SeyalAppRecoveryStage`). Zero handle → disconnected.
  var runtimeRecoveryStage: UInt16 {
    guard recoveryAppHandle != 0 else {
      return UInt16(SEYAL_APP_RECOVERY_DISCONNECTED.rawValue)
    }
    return seyal_app_snapshot(recoveryAppHandle).recovery_stage
  }

  var isRustRecoveryEpisodeActive: Bool {
    guard recoveryAppHandle != 0 else { return false }
    let snap = seyal_app_snapshot(recoveryAppHandle)
    if snap.recovery_effect != 0 { return true }
    return runtimeRecoveryStageIsEpisodeActive(snap.recovery_stage)
  }

  func cancelBridgeReconnect() {
    guard recoveryAppHandle != 0 else { return }
    recoveryPresentationPending = false
    _ = applyRuntimeRecoveryAction(recoveryAppHandle, kind: SEYAL_APP_ACTION_CANCEL_RECOVERY)
    onRecoveryEffectsPending?()
  }

  func startAutomaticBridgeRecoveryIfNeeded() {
    guard !suppressesAutomaticBridgeRecovery,
      !isDetachingRuntimeConnection,
      shouldAttachRuntime,
      bridge?.isConnected == false,
      // stop() keeps the old clientHandle until both dispatch-source cancel
      // handlers complete. Waiting for zero prevents consuming a retry on our
      // own in-progress detach/controller cleanup.
      bridge?.clientHandle == 0,
      recoveryAppHandle != 0,
      !isRustRecoveryEpisodeActive,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_EXHAUSTED.rawValue),
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue)
    else { return }
    beginRustRecoveryEpisode()
  }

  /// Explicit user retry starts a new bounded foreground recovery episode.
  /// Automatic exhaustion never invokes this method recursively.
  @discardableResult
  func retryRuntimeConnection() -> Bool {
    guard shouldAttachRuntime,
      bridge?.isConnected != true,
      bridge?.clientHandle == 0,
      recoveryAppHandle != 0,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue)
    else { return bridge?.isConnected == true }
    beginRustRecoveryEpisode()
    return bridge?.isConnected == true
  }

  /// A command entered while disconnected is an explicit recovery action, but
  /// it must never synchronously connect/handshake/attach on the AppKit thread.
  @discardableResult
  func ensureTerminalBridgeConnected() -> Bool {
    guard bridge?.isConnected != true else { return true }
    guard !isDetachingRuntimeConnection,
      shouldAttachRuntime,
      bridge?.clientHandle == 0,
      recoveryAppHandle != 0
    else { return false }
    if !isRustRecoveryEpisodeActive,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_BLOCKED.rawValue)
    {
      beginRustRecoveryEpisode()
    }
    return false
  }

  /// Sole BeginRecovery requester for this surface. ProductChrome drains effects
  /// via `onRecoveryEffectsPending` → `driveRecovery` (open+adopt).
  func beginRustRecoveryEpisode() {
    guard recoveryAppHandle != 0 else { return }
    let now = runtimeRecoveryNowMillis()
    _ = applyRuntimeRecoveryAction(recoveryAppHandle, kind: SEYAL_APP_ACTION_BEGIN_RECOVERY) {
      $0.target_pty_generation = now
    }
    onRecoveryEffectsPending?()
  }

  @discardableResult
  func adoptRecoveredHandle(_ opened: RuntimeRecoveryOpenedHandle) -> Bool {
    let adopted = bridge?.adoptRecoveredHandle(opened) ?? false
    if adopted {
      recoveryPresentationPending = true
    }
    return adopted
  }

  /// SPEC-009 §10: Restoring then Usable after connect + native restore.
  @discardableResult
  func advanceRecoveryPresentationIfReady() -> Bool {
    guard recoveryAppHandle != 0,
      bridge?.isConnected == true,
      hasPreparedState,
      recoveryPresentationPending || runtimeRecoveryStage
        == UInt16(SEYAL_APP_RECOVERY_RECONSTRUCTING.rawValue)
        || runtimeRecoveryStage == UInt16(SEYAL_APP_RECOVERY_RESTORING.rawValue)
    else { return true }

    if runtimeRecoveryStage == UInt16(SEYAL_APP_RECOVERY_RECONSTRUCTING.rawValue) {
      _ = applyRuntimeRecoveryAction(
        recoveryAppHandle,
        kind: SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE
      ) {
        $0.reserved = UInt32(SEYAL_APP_RECOVERY_RESTORING.rawValue)
      }
    }

    guard shouldRender,
      runtimeRecoveryStage != UInt16(SEYAL_APP_RECOVERY_USABLE.rawValue)
    else {
      if runtimeRecoveryStage == UInt16(SEYAL_APP_RECOVERY_USABLE.rawValue) {
        recoveryPresentationPending = false
      }
      return true
    }

    guard restoreNativeInteractionAfterRendererReady() else {
      return false
    }
    _ = applyRuntimeRecoveryAction(
      recoveryAppHandle,
      kind: SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE
    ) {
      $0.reserved = UInt32(SEYAL_APP_RECOVERY_USABLE.rawValue)
    }
    recoveryPresentationPending = false
    refreshRecoveryAccessibilityValue()
    onRecoveryEffectsPending?()
    return true
  }
}

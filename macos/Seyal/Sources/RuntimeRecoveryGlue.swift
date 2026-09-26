import Foundation

/// Monotonic host clock for Rust recovery actions. Wall-clock changes must not
/// move the Rust-owned episode deadline.
func runtimeRecoveryNowMillis() -> UInt64 {
  UInt64(ProcessInfo.processInfo.systemUptime * 1000)
}

/// Applies one Rust `RecoveryCoordinator` action. The host executes effects;
/// retry, deadline and launch policy stay in Rust.
@discardableResult
func applyRuntimeRecoveryAction(
  _ appHandle: UInt64,
  kind: SeyalAppActionKind,
  configure: (inout SeyalAppAction) -> Void = { _ in }
) -> Int32 {
  guard appHandle != 0 else { return -1 }
  var action = SeyalAppAction()
  action.version = UInt16(SEYAL_APP_ABI_VERSION)
  action.size = UInt16(MemoryLayout<SeyalAppAction>.size)
  action.kind = UInt16(kind.rawValue)
  configure(&action)
  return seyal_app_apply(appHandle, &action)
}

func runtimeRecoveryStageIsEpisodeActive(_ stage: UInt16) -> Bool {
  stage == UInt16(SEYAL_APP_RECOVERY_DISCOVERING.rawValue)
    || stage == UInt16(SEYAL_APP_RECOVERY_STARTING.rawValue)
    || stage == UInt16(SEYAL_APP_RECOVERY_WAITING_CONTROLLER.rawValue)
}

/// Lifecycle-queue-only FFI attempt. It owns no AppKit state and returns only
/// a pending Rust handle; MainActor later adopts that handle into its Pane.
/// `remainingBudget` is the Rust `RecoveryCoordinator` episode remainder
/// carried by the `PerformAttempt` effect, so queue delay cannot reset the
/// episode deadline.
func openRuntimeRecoveryHandle(
  executionIdentity: String?,
  allowsImplicitExecutionBootstrap: Bool,
  remainingBudget: TimeInterval
) -> RuntimeRecoveryAttemptOutcome {
  guard remainingBudget.isFinite, remainingBudget > 0 else { return .retryable }
  let microsDouble = min(remainingBudget * 1_000_000, Double(UInt64.max))
  let budgetMicros = max(UInt64(1), UInt64(microsDouble.rounded(.down)))

  let handle: UInt64
  if let executionIdentity,
    let words = runtimeRecoveryExecutionWords(executionIdentity)
  {
    handle = seyal_bridge_open_execution_until(words.low, words.high, budgetMicros)
  } else if allowsImplicitExecutionBootstrap {
    handle = seyal_bridge_open_first_until(budgetMicros)
  } else {
    return .blocked
  }
  guard handle != 0 else {
    let result = seyal_bridge_last_recovery_result()
    // Rust owns failure_class → outcome mapping (ADR-015 / #1065).
    switch seyal_bridge_classify_open_result(result.failure_class, result.retryable) {
    case UInt32(SEYAL_APP_RECOVERY_ENDPOINT_MISSING.rawValue):
      return .endpointMissing
    case UInt32(SEYAL_APP_RECOVERY_CONTROLLER_BUSY.rawValue):
      return .controllerBusy
    case UInt32(SEYAL_APP_RECOVERY_RETRYABLE.rawValue):
      return .retryable
    default:
      return .blocked
    }
  }
  // `LAST_RECOVERY_RESULT` is executor-local in the Rust bridge. Capture the
  // accepted attachment identities on this lifecycle queue and carry them to
  // the MainActor adopter with the handle; reading them again there returns an
  // empty thread-local result and falsely looks like an identity mismatch.
  let result = seyal_bridge_last_recovery_result()
  return .opened(RuntimeRecoveryOpenedHandle(
    handle: handle,
    stage: result.stage,
    failureClass: result.failure_class,
    retryable: result.retryable != 0,
    connectionOrigin: result.connection_origin,
    runtimeIDLow: result.runtime_id_low,
    runtimeIDHigh: result.runtime_id_high,
    executionIDLow: result.execution_id_low,
    executionIDHigh: result.execution_id_high,
    attachmentIDLow: result.attachment_id_low,
    attachmentIDHigh: result.attachment_id_high
  ))
}

/// Asks Rust `ReconstructionState` to fence continuity identity. Returns true
/// only when the commit succeeds; identity mismatch fails closed in Rust.
@discardableResult
func commitRuntimeReconstruction(
  _ appHandle: UInt64,
  runtimeLow: UInt64,
  runtimeHigh: UInt64,
  executionLow: UInt64,
  executionHigh: UInt64,
  attachmentLow: UInt64,
  attachmentHigh: UInt64
) -> Bool {
  guard appHandle != 0 else { return false }
  _ = applyRuntimeRecoveryAction(appHandle, kind: SEYAL_APP_ACTION_BEGIN_RECONSTRUCTION)
  let result = applyRuntimeRecoveryAction(appHandle, kind: SEYAL_APP_ACTION_COMMIT_RECONSTRUCTION) {
    $0.fence_execution_lo = runtimeLow
    $0.fence_execution_hi = runtimeHigh
    $0.target_execution_lo = executionLow
    $0.target_execution_hi = executionHigh
    $0.target_attachment_lo = attachmentLow
    $0.target_attachment_hi = attachmentHigh
    // bit0 = controller authority, bit1 = authoritative snapshot.
    $0.reserved = 1 | 2
  }
  return result == 0
}

func disconnectRuntimeReconstruction(_ appHandle: UInt64) {
  guard appHandle != 0 else { return }
  _ = applyRuntimeRecoveryAction(appHandle, kind: SEYAL_APP_ACTION_DISCONNECT_RECONSTRUCTION)
}

private func runtimeRecoveryExecutionWords(_ value: String) -> (low: UInt64, high: UInt64)? {
  let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
    .lowercased()
    .replacingOccurrences(of: "0x", with: "")
  guard normalized.count == 32,
    let high = UInt64(normalized.prefix(16), radix: 16),
    let low = UInt64(normalized.suffix(16), radix: 16)
  else { return nil }
  return (low, high)
}

struct RuntimeRecoveryOpenedHandle: Equatable, Sendable {
  let handle: UInt64
  let stage: UInt8
  let failureClass: UInt8
  let retryable: Bool
  let connectionOrigin: UInt8
  let runtimeIDLow: UInt64
  let runtimeIDHigh: UInt64
  let executionIDLow: UInt64
  let executionIDHigh: UInt64
  let attachmentIDLow: UInt64
  let attachmentIDHigh: UInt64
}

enum RuntimeRecoveryAttemptOutcome: Equatable, Sendable {
  case opened(RuntimeRecoveryOpenedHandle)
  case endpointMissing
  case retryable
  case controllerBusy
  case blocked
}

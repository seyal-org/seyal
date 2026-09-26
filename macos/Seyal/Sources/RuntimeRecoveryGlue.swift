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
    // Class 1 is a missing leaf. Class 2 is refused/disappeared: a dead
    // `control.sock` looks like an unready listener, and only a Runtime
    // singleton contender may replace it (SPEC-009). Map both to the
    // one-launch-per-episode path; Rust launch-once accounting still prevents
    // a second spawn while a just-started helper binds the canonical endpoint.
    if result.retryable != 0, result.failure_class == 1 || result.failure_class == 2 {
      return .endpointMissing
    }
    if result.failure_class == 3, result.retryable != 0 { return .controllerBusy }
    return result.retryable != 0 ? .retryable : .blocked
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

struct RuntimeContinuityIdentity: Equatable {
  let low: UInt64
  let high: UInt64

  static let none = RuntimeContinuityIdentity(low: 0, high: 0)
  var isValid: Bool { self != .none }
}

enum ReconnectReconstructionStage: Equatable {
  case disconnected
  case awaitingAuthoritativeSnapshot
  case usable
  case blockedIdentityMismatch
}

/// Pins the Runtime/execution continuity claim while treating every attachment
/// and all client-side reconstruction state as disposable.
struct ReconnectReconstructionState: Equatable {
  private(set) var stage: ReconnectReconstructionStage = .disconnected
  private(set) var expectedRuntime: RuntimeContinuityIdentity?
  private(set) var expectedExecution: RuntimeContinuityIdentity?
  private(set) var lastAttachment: RuntimeContinuityIdentity?

  var canMutate: Bool { stage == .usable }

  mutating func beginAttempt() {
    stage = .awaitingAuthoritativeSnapshot
  }

  mutating func commit(
    runtime: RuntimeContinuityIdentity,
    execution: RuntimeContinuityIdentity,
    attachment: RuntimeContinuityIdentity,
    controllerAuthorityCommitted: Bool,
    authoritativeSnapshotCommitted: Bool
  ) -> Bool {
    guard runtime.isValid, execution.isValid, attachment.isValid,
      expectedRuntime.map({ $0 == runtime }) ?? true,
      expectedExecution.map({ $0 == execution }) ?? true,
      lastAttachment.map({ $0 != attachment }) ?? true
    else {
      stage = .blockedIdentityMismatch
      return false
    }
    guard controllerAuthorityCommitted, authoritativeSnapshotCommitted else {
      stage = .awaitingAuthoritativeSnapshot
      return false
    }

    expectedRuntime = runtime
    expectedExecution = execution
    lastAttachment = attachment
    stage = .usable
    return true
  }

  mutating func disconnect() {
    stage = .disconnected
  }
}

import AppKit
import Metal
@preconcurrency import QuartzCore

final class RuntimeRecoveryTimerBox: @unchecked Sendable {
  let timer: Timer

  init(timer: Timer) {
    self.timer = timer
  }
}

enum RuntimeRecoveryStage: UInt8, Equatable {
  case disconnected = 0
  case discovering = 1
  case startingRuntime = 2
  case waitingForController = 3
  case reconstructing = 4
  case restoringInteraction = 5
  case usable = 6
  case exhausted = 7
  case blocked = 8
}

struct RuntimeRecoveryState: Equatable {
  private(set) var stage: RuntimeRecoveryStage = .disconnected
  private(set) var generation: UInt64 = 0

  mutating func begin() {
    generation &+= 1
    stage = .discovering
  }

  mutating func transition(to next: RuntimeRecoveryStage) {
    stage = next
  }

  mutating func cancel() {
    generation &+= 1
    stage = .disconnected
  }

  mutating func retry() {
    begin()
  }
}

struct PresentationRetryBudget: Equatable {
  private static let delays: [TimeInterval] = [1.0 / 60.0, 0.05, 0.20, 0.75]
  static let maximumAutomaticRetries = 4
  private var nextAttempt = 0

  mutating func reset() {
    nextAttempt = 0
  }

  mutating func claimNextDelay() -> TimeInterval? {
    guard nextAttempt < Self.delays.count else { return nil }
    defer { nextAttempt += 1 }
    return Self.delays[nextAttempt]
  }

  var exhausted: Bool {
    nextAttempt >= Self.delays.count
  }
}

struct PresentationOpportunityState: Equatable {
  private(set) var pending = false
  private(set) var armed = false

  mutating func request() {
    pending = true
  }

  mutating func armIfNeeded() -> Bool {
    guard pending, !armed else { return false }
    armed = true
    return true
  }

  mutating func consumeOpportunity() -> Bool {
    guard armed, pending else { return false }
    armed = false
    return true
  }

  mutating func markPresented() {
    pending = false
    armed = false
  }

  mutating func markFailed() {
    pending = true
    armed = false
  }

  mutating func cancel() {
    pending = false
    armed = false
  }
}

struct PresentationRecoveryState: Equatable {
  private(set) var opportunity = PresentationOpportunityState()
  private(set) var retryBudget = PresentationRetryBudget()
  private(set) var exhausted = false

  var pending: Bool { opportunity.pending }
  var armed: Bool { opportunity.armed }

  mutating func request() {
    guard !exhausted else { return }
    opportunity.request()
  }

  mutating func armIfNeeded() -> Bool {
    guard !exhausted else { return false }
    return opportunity.armIfNeeded()
  }

  mutating func consumeOpportunity() -> Bool {
    opportunity.consumeOpportunity()
  }

  mutating func recordSubmissionFailure() -> TimeInterval? {
    opportunity.markFailed()
    guard let delay = retryBudget.claimNextDelay() else {
      exhausted = true
      opportunity.cancel()
      return nil
    }
    return delay
  }

  mutating func recordSubmissionSuccess() {
    opportunity.markPresented()
    retryBudget.reset()
    exhausted = false
  }

  mutating func cancel() {
    opportunity.cancel()
    retryBudget.reset()
    exhausted = false
  }

  mutating func cancelPending() {
    opportunity.cancel()
  }

  mutating func resetForLifecycleRecovery() {
    cancel()
  }
}

struct PreparationRecoveryState: Equatable {
  private(set) var retryBudget = PresentationRetryBudget()
  private(set) var exhausted = false

  var canAttemptPreparation: Bool { !exhausted }

  mutating func recordFailure() -> TimeInterval? {
    guard !exhausted else { return nil }
    guard let delay = retryBudget.claimNextDelay() else {
      exhausted = true
      return nil
    }
    return delay
  }

  mutating func recordSuccess() {
    retryBudget.reset()
    exhausted = false
  }

  mutating func resetForLifecycleRecovery() {
    recordSuccess()
  }
}

/// Owns the run-loop registration independently of the view's actor-isolated
/// lifetime. Releasing a surface therefore also invalidates its display link.
final class MetalDisplayLinkLease {
  let link: CAMetalDisplayLink

  init(layer: CAMetalLayer) {
    link = CAMetalDisplayLink(metalLayer: layer)
  }

  deinit {
    link.delegate = nil
    link.invalidate()
  }
}

import Foundation

final class RustBridgeHandleBox: @unchecked Sendable {
  var value: UInt64 = 0
}

final class RustBridgeTeardownCoordinator: @unchecked Sendable {
  private let disconnect: () -> Void
  private let lock = NSLock()
  private var activeSourceCountStorage = 0
  private var disconnectPendingStorage = false
  private var disconnectScheduled = false
  var onDisconnected: (() -> Void)?

  var activeSourceCount: Int {
    lock.lock()
    defer { lock.unlock() }
    return activeSourceCountStorage
  }

  var disconnectPending: Bool {
    lock.lock()
    defer { lock.unlock() }
    return disconnectPendingStorage
  }

  init(disconnect: @escaping () -> Void) {
    self.disconnect = disconnect
  }

  func sourceCreated() {
    lock.lock()
    defer { lock.unlock() }
    activeSourceCountStorage += 1
  }

  func sourceCancelled() {
    lock.lock()
    guard activeSourceCountStorage > 0 else {
      lock.unlock()
      return
    }
    activeSourceCountStorage -= 1
    let shouldSchedule = disconnectPendingStorage && activeSourceCountStorage == 0
    lock.unlock()
    if shouldSchedule { scheduleDisconnectOnMain() }
  }

  func requestDisconnect() {
    lock.lock()
    guard !disconnectPendingStorage else {
      lock.unlock()
      return
    }
    disconnectPendingStorage = true
    let shouldSchedule = activeSourceCountStorage == 0
    lock.unlock()
    if shouldSchedule { scheduleDisconnectOnMain() }
  }

  /// Eager CLIENT disconnect already dropped the handle; clear accounting so
  /// `start()` is not blocked and late DispatchSource cancel handlers no-op.
  func resetAfterEagerDisconnect() {
    lock.lock()
    activeSourceCountStorage = 0
    disconnectPendingStorage = false
    disconnectScheduled = false
    lock.unlock()
  }

  private func scheduleDisconnectOnMain() {
    lock.lock()
    guard disconnectPendingStorage, activeSourceCountStorage == 0, !disconnectScheduled else {
      lock.unlock()
      return
    }
    disconnectScheduled = true
    lock.unlock()

    if Thread.isMainThread {
      seyalRunAsMainActorFromMainQueue {
        self.finishDisconnectOnMain()
      }
      return
    }

    DispatchQueue.main.async { [self] in
      seyalRunAsMainActorFromMainQueue {
        self.finishDisconnectOnMain()
      }
    }
  }

  private func finishDisconnectOnMain() {
    lock.lock()
    guard disconnectPendingStorage, activeSourceCountStorage == 0 else {
      disconnectScheduled = false
      lock.unlock()
      return
    }
    disconnectPendingStorage = false
    disconnectScheduled = false
    let callback = onDisconnected
    lock.unlock()

    // CLIENT is thread-local, so this must remain on the AppKit/main queue.
    dispatchPrecondition(condition: .onQueue(.main))
    disconnect()
    callback?()
  }
}

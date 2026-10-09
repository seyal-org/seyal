import Foundation

@MainActor
final class RustDisplayBridge {
  typealias FrameHandler = @MainActor (SeyalPreparedFrame) -> Void
  typealias TimelineHandler = @MainActor () -> Void
  typealias HistoryHandler = @MainActor (NativeHistoryRange) -> Void
  typealias ComposerResultHandler = @MainActor (NativeComposerResult) -> Void
  typealias ComposerStatusHandler = @MainActor (NativeComposerStatus) -> Void
  typealias ErrorHandler = @MainActor (Int32) -> Void
  typealias StatusHandler = @MainActor () -> Void

  struct RecoveryResult: Equatable {
    let handle: UInt64
    let runtimeIDLow: UInt64
    let runtimeIDHigh: UInt64
    let executionIDLow: UInt64
    let executionIDHigh: UInt64
    let attachmentIDLow: UInt64
    let attachmentIDHigh: UInt64

    init(
      handle: UInt64,
      runtimeIDLow: UInt64,
      runtimeIDHigh: UInt64,
      executionIDLow: UInt64,
      executionIDHigh: UInt64,
      attachmentIDLow: UInt64,
      attachmentIDHigh: UInt64
    ) {
      self.handle = handle
      self.runtimeIDLow = runtimeIDLow
      self.runtimeIDHigh = runtimeIDHigh
      self.executionIDLow = executionIDLow
      self.executionIDHigh = executionIDHigh
      self.attachmentIDLow = attachmentIDLow
      self.attachmentIDHigh = attachmentIDHigh
    }

    static func current() -> RecoveryResult {
      let result = seyal_bridge_last_recovery_result()
      return RecoveryResult(
        handle: result.handle,
        runtimeIDLow: result.runtime_id_low,
        runtimeIDHigh: result.runtime_id_high,
        executionIDLow: result.execution_id_low,
        executionIDHigh: result.execution_id_high,
        attachmentIDLow: result.attachment_id_low,
        attachmentIDHigh: result.attachment_id_high
      )
    }

    init(opened: RuntimeRecoveryOpenedHandle) {
      self.init(
        handle: opened.handle,
        runtimeIDLow: opened.runtimeIDLow,
        runtimeIDHigh: opened.runtimeIDHigh,
        executionIDLow: opened.executionIDLow,
        executionIDHigh: opened.executionIDHigh,
        attachmentIDLow: opened.attachmentIDLow,
        attachmentIDHigh: opened.attachmentIDHigh
      )
    }
  }

  let onFrame: FrameHandler
  let onTimeline: TimelineHandler
  let onHistory: HistoryHandler
  let onComposerResult: ComposerResultHandler
  let onComposerStatus: ComposerStatusHandler
  let onError: ErrorHandler
  var onStatusChanged: StatusHandler
  var onCopiedText: ((String) -> Void)?
  /// Notifies the multi-live host after a non-primary registry handle is polled
  /// (#936) so a secondary Metal surface can present without changing focus.
  var onRegistryHandlePolled: ((UInt64) -> Void)?
  var readSource: DispatchSourceRead?
  var writeSource: DispatchSourceWrite?
  var auxiliaryWriteSources: [UInt64: DispatchSourceWrite] = [:]
  var auxiliaryReadSources: [UInt64: DispatchSourceRead] = [:]
  var provisioningWakeupSource: DispatchSourceRead?
  var socketFileDescriptor: Int32 = -1
  let handleBox = RustBridgeHandleBox()
  var teardown: RustBridgeTeardownCoordinator!
  var clientHandle: UInt64 = 0
  var isConnected = false
  var lastRecoveryResult = RecoveryResult.current()
  /// The last bundled-helper failure is retained for the recovery coordinator
  /// to classify as blocked. Do not silently discard trust or spawn errors.
  var lastLaunchError: BundledRuntimeLaunchError?
  var runtimeIdentityWords: (low: UInt64, high: UInt64) = (0, 0)
  var attachmentIdentityWords: (low: UInt64, high: UInt64) = (0, 0)
  /// ApplicationRoot that owns Rust `ReconstructionState` fencing for this Pane.
  var continuityAppHandle: UInt64 = 0
  var runtimeBlockMetadata: RuntimeBlockMetadata?
  let runtimeLauncher = BundledRuntimeLauncher()
  var lastTimelineRevision: UInt64 = 0
  let paneID: String
  let requestedExecutionIdentity: String?
  let allowsImplicitExecutionBootstrap: Bool
  var requestedHistoryRanges:
    [PaneHistoryRequestKey: (blockID: UInt64, startLine: UInt64, endLine: UInt64)] = [:]
  var historyRevisions: [PaneHistoryRequestKey: (revision: UInt64, requestID: UInt64)] = [:]
  var historyContinuations:
    [PaneHistoryRequestKey: (startUnit: UInt32, range: NativeHistoryRange)] = [:]
  var lastComposerResultRequestID: UInt64 = 0
  var lastComposerStatusRevision: UInt64 = 0

  init(
    onFrame: @escaping FrameHandler,
    onError: @escaping ErrorHandler,
    onStatusChanged: @escaping StatusHandler = {},
    onTimeline: @escaping TimelineHandler = {},
    onHistory: @escaping HistoryHandler = { _ in },
    onComposerResult: @escaping ComposerResultHandler = { _ in },
    onComposerStatus: @escaping ComposerStatusHandler = { _ in },
    paneID: String = "unbound",
    executionIdentity: String? = nil,
    allowsImplicitExecutionBootstrap: Bool = true
  ) {
    self.onFrame = onFrame
    self.onTimeline = onTimeline
    self.onHistory = onHistory
    self.onComposerResult = onComposerResult
    self.onComposerStatus = onComposerStatus
    self.onError = onError
    self.onStatusChanged = onStatusChanged
    self.paneID = paneID
    self.requestedExecutionIdentity = executionIdentity
    self.allowsImplicitExecutionBootstrap = allowsImplicitExecutionBootstrap
    teardown = RustBridgeTeardownCoordinator { [handleBox] in
      guard handleBox.value != 0 else { return }
      seyal_bridge_disconnect_handle(handleBox.value)
      handleBox.value = 0
    }
    teardown.onDisconnected = { [weak self] in
      self?.clientHandle = 0
      self?.teardownCompleted()
    }
  }

  @discardableResult
  func start() -> Bool {
    guard !isConnected else { return true }
    // Explicit self-test seams may still call start(), but production recovery
    // never queues a second open while the previous socket is tearing down.
    guard !teardown.disconnectPending else { return false }
    lastLaunchError = nil

    let handle: UInt64
    if let executionIdentity = requestedExecutionIdentity,
      let (low, high) = Self.executionWords(from: executionIdentity)
    {
      handle = seyal_bridge_open_execution(low, high)
    } else if allowsImplicitExecutionBootstrap {
      handle = seyal_bridge_open_first()
    } else {
      // A split production Pane without a Runtime identity is not allowed to
      // attach to whichever execution happens to be first. This keeps pane
      // ownership explicit and makes the missing execution a visible bridge
      // failure until the Runtime supplies one.
      onError(-6)
      onStatusChanged()
      return false
    }
    guard handle != 0 else {
      lastRecoveryResult = RecoveryResult.current()
      onError(-6)
      onStatusChanged()
      return false
    }
    lastRecoveryResult = RecoveryResult.current()
    return adoptOpenedHandle(handle, recoveryResult: lastRecoveryResult)
  }

  /// Called only on the MainActor after the lifecycle executor has completed
  /// the disposable Rust connection. Adoption moves the client into this
  /// Pane's executor-local registry before AppKit registers socket sources.
  @discardableResult
  func adoptRecoveredHandle(_ opened: RuntimeRecoveryOpenedHandle) -> Bool {
    let handle = opened.handle
    guard !isConnected, !teardown.disconnectPending else {
      seyal_bridge_disconnect_handle(handle)
      return false
    }
    lastLaunchError = nil
    guard seyal_bridge_adopt_handle(handle) == 0 else {
      seyal_bridge_disconnect_handle(handle)
      onError(-1)
      onStatusChanged()
      return false
    }
    let recoveryResult = RecoveryResult(opened: opened)
    lastRecoveryResult = recoveryResult
    return finishAdoptedHandle(handle, recoveryResult: recoveryResult)
  }

  @discardableResult
  func adoptOpenedHandle(_ handle: UInt64, recoveryResult: RecoveryResult) -> Bool {
    guard seyal_bridge_adopt_handle(handle) == 0 else {
      seyal_bridge_disconnect_handle(handle)
      onError(-1)
      onStatusChanged()
      return false
    }
    return finishAdoptedHandle(handle, recoveryResult: recoveryResult)
  }

  @discardableResult
  func finishAdoptedHandle(_ handle: UInt64, recoveryResult: RecoveryResult) -> Bool {
    // A Rust client handle is published only after finish_attach has validated
    // Controller authority and atomically committed the complete initial
    // snapshot. Identity drift or attachment reuse fails closed in Rust
    // ReconstructionState before AppKit can submit input or expose stale
    // presentation.
    guard commitRuntimeReconstruction(
      continuityAppHandle,
      runtimeLow: recoveryResult.runtimeIDLow,
      runtimeHigh: recoveryResult.runtimeIDHigh,
      executionLow: recoveryResult.executionIDLow,
      executionHigh: recoveryResult.executionIDHigh,
      attachmentLow: recoveryResult.attachmentIDLow,
      attachmentHigh: recoveryResult.attachmentIDHigh
    ) else {
      seyal_bridge_disconnect_handle(handle)
      onError(-4)
      onStatusChanged()
      return false
    }
    clientHandle = handle
    handleBox.value = handle
    runtimeIdentityWords = (recoveryResult.runtimeIDLow, recoveryResult.runtimeIDHigh)
    attachmentIdentityWords = (recoveryResult.attachmentIDLow, recoveryResult.attachmentIDHigh)

    let fileDescriptor = seyal_bridge_socket_fd()
    guard fileDescriptor >= 0 else {
      seyal_bridge_disconnect_handle(handle)
      clientHandle = 0
      handleBox.value = 0
      onError(fileDescriptor)
      onStatusChanged()
      return false
    }

    socketFileDescriptor = fileDescriptor
    isConnected = true
    runtimeBlockMetadata = currentBlockMetadata()
    let source = DispatchSource.makeReadSource(fileDescriptor: fileDescriptor, queue: .main)
    source.setEventHandler { [weak self] in
      seyalRunAsMainActorFromMainQueue {
        self?.drainReadyDisplayWork()
      }
    }
    source.setCancelHandler { [teardown = teardown!] in
      teardown.sourceCancelled()
    }
    teardown.sourceCreated()
    readSource = source
    source.resume()

    publishCurrentFrame()
    synchronizeWriteReadinessSource()
    armAuxiliaryReadSources()
    armProvisioningWakeupSource()
    onStatusChanged()
    return true
  }

  /// Starts only the trusted helper packaged inside Seyal.app. Called only
  /// when executing a Rust `RecoveryCoordinator` LaunchHelper effect, which
  /// owns episode-level launch-once accounting.
  @discardableResult
  func launchBundledRuntime() -> Bool {
    let result = runtimeLauncher.launch()
    if case let .failure(error) = result {
      lastLaunchError = error
      onError(error.nativeCode)
      onStatusChanged()
      return false
    }
    lastLaunchError = nil
    return true
  }

  static func executionWords(from value: String) -> (UInt64, UInt64)? {
    let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
      .lowercased()
      .replacingOccurrences(of: "0x", with: "")
    guard normalized.count == 32,
      let high = UInt64(normalized.prefix(16), radix: 16),
      let low = UInt64(normalized.suffix(16), radix: 16)
    else { return nil }
    return (low, high)
  }

  /// Abrupt client-loss path for merge-acceptance soak. Shuts the live socket
  /// then owns a single disconnect. This is fault injection for socket-loss
  /// recovery, not GUI-process death or the production poll→-18 observation
  /// path; do not treat it as full abrupt-GUI-death coverage. Runtime observes
  /// EOF/reset; replacement opening remains coordinator-owned.
  func forceAbruptSocketLossForAcceptance() {
    guard isConnected || socketFileDescriptor >= 0 || clientHandle != 0 else { return }
    if socketFileDescriptor >= 0 {
      // shutdown (not close): Runtime sees EOF while Rust still owns the fd and
      // can drop it safely exactly once in disconnect_handle.
      _ = Darwin.shutdown(socketFileDescriptor, SHUT_RDWR)
    }
    isConnected = false
    disconnectRuntimeReconstruction(continuityAppHandle)
    runtimeBlockMetadata = nil
    requestedHistoryRanges.removeAll(keepingCapacity: false)
    historyRevisions.removeAll(keepingCapacity: false)
    historyContinuations.removeAll(keepingCapacity: false)
    lastTimelineRevision = 0
    lastComposerResultRequestID = 0
    clearComposerStatus()
    runtimeIdentityWords = (0, 0)
    attachmentIdentityWords = (0, 0)
    if let readSource {
      self.readSource = nil
      readSource.cancel()
    }
    if let writeSource {
      self.writeSource = nil
      writeSource.cancel()
    }
    cancelAuxiliaryReadSources()
    socketFileDescriptor = -1
    if clientHandle != 0 {
      let handle = clientHandle
      clientHandle = 0
      handleBox.value = 0
      seyal_bridge_disconnect_handle(handle)
    }
    onStatusChanged()
  }

  /// Tears down only the disposable socket/client side of the Pane. The
  /// `reconnect` spelling is retained for source compatibility with older
  /// callers, but replacement scheduling/opening belongs exclusively to the
  /// Rust `RecoveryCoordinator` and its host effect loop.
  func stop(reconnect _: Bool = false) {
    if teardown.disconnectPending {
      // Prior stop armed teardown; finish CLIENT drop on this MainActor turn.
      completeEagerClientDisconnect()
      return
    }
    guard isConnected || socketFileDescriptor >= 0 else { return }

    isConnected = false
    disconnectRuntimeReconstruction(continuityAppHandle)
    runtimeBlockMetadata = nil
    // All request/display correlations are connection-local. A reconnect
    // receives a fresh attachment and must never reuse pending history,
    // composer, timeline, or generation state from the dead socket.
    requestedHistoryRanges.removeAll(keepingCapacity: false)
    historyRevisions.removeAll(keepingCapacity: false)
    historyContinuations.removeAll(keepingCapacity: false)
    lastTimelineRevision = 0
    lastComposerResultRequestID = 0
    clearComposerStatus()
    runtimeIdentityWords = (0, 0)
    attachmentIdentityWords = (0, 0)

    if let readSource {
      self.readSource = nil
      readSource.cancel()
    }
    if let writeSource {
      self.writeSource = nil
      writeSource.cancel()
    }
    cancelAuxiliaryReadSources()
    socketFileDescriptor = -1

    // Disconnect CLIENT on this MainActor turn (same ownership hand-off as
    // abrupt). Waiting for DispatchSource cancel-handler hops alone cannot
    // meet SPEC-009 §16.2 cleanup_p99 (250µs). Cancel handlers remain
    // idempotent via teardown.resetAfterEagerDisconnect().
    completeEagerClientDisconnect()
    onStatusChanged()
  }

  /// Drops the live CLIENT handle immediately and clears teardown so `start()`
  /// is not blocked on async DispatchSource cancel hops.
  func completeEagerClientDisconnect() {
    if clientHandle != 0 {
      let handle = clientHandle
      clientHandle = 0
      handleBox.value = 0
      seyal_bridge_disconnect_handle(handle)
    }
    teardown.resetAfterEagerDisconnect()
  }

  private func teardownCompleted() {
    // Completion only releases the old executor-local handle. It intentionally
    // never opens a replacement; the owning surface observes disconnected
    // status and starts one bounded coordinator episode if still renderable.
    onStatusChanged()
  }

  @discardableResult
  func selectClient() -> Bool {
    guard clientHandle != 0 else { return false }
    return seyal_bridge_select(clientHandle) == 0
  }

  /// One DispatchSourceRead per live Controller fd. CreateTab's second
  /// client must not steal the first connection's readiness handler.
  func armAuxiliaryReadSources() {
    guard isConnected else { return }
    var live = Set<UInt64>()
    var cursor: UInt64 = 0
    while true {
      let handle = seyal_bridge_next_handle(cursor)
      if handle == 0 { break }
      cursor = handle
      live.insert(handle)
      if handle == clientHandle { continue }
      if auxiliaryReadSources[handle] != nil { continue }
      let fd = seyal_bridge_socket_fd_for(handle)
      guard fd >= 0 else { continue }
      let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: .main)
      source.setEventHandler { [weak self] in
        seyalRunAsMainActorFromMainQueue {
          guard let self, self.isConnected else { return }
          let result = seyal_bridge_poll_for(handle)
          if result < 0 {
            // Level-trigger progress: EOF/disconnect must disarm this fd's
            // sources and drop the dead client. Tab 1 stays up (do not stop()).
            self.cancelAuxiliarySources(for: handle)
            seyal_bridge_disconnect_handle(handle)
            _ = self.selectClient()
            self.publishCurrentFrame()
            self.synchronizeWriteReadinessSource()
            self.armAuxiliaryReadSources()
            self.onStatusChanged()
            return
          }
          self.onRegistryHandlePolled?(handle)
          _ = self.selectClient()
          self.publishCurrentFrame()
          self.synchronizeWriteReadinessSource()
        }
      }
      source.setCancelHandler { [teardown = teardown!] in
        teardown.sourceCancelled()
      }
      teardown.sourceCreated()
      auxiliaryReadSources[handle] = source
      source.resume()
    }
    for (handle, source) in auxiliaryReadSources where !live.contains(handle) {
      source.cancel()
      auxiliaryReadSources.removeValue(forKey: handle)
    }
  }

  /// Wake `seyal_bridge_poll` when an off-thread second-Controller connect finishes.
  func armProvisioningWakeupSource() {
    guard isConnected, provisioningWakeupSource == nil else { return }
    let fd = seyal_bridge_provisioning_wakeup_fd()
    guard fd >= 0 else { return }
    let source = DispatchSource.makeReadSource(fileDescriptor: fd, queue: .main)
    source.setEventHandler { [weak self] in
      seyalRunAsMainActorFromMainQueue {
        guard let self, self.isConnected else { return }
        _ = seyal_bridge_poll()
        self.armAuxiliaryReadSources()
        self.publishCurrentFrame()
        self.synchronizeWriteReadinessSource()
      }
    }
    source.setCancelHandler { [teardown = teardown!] in
      teardown.sourceCancelled()
    }
    teardown.sourceCreated()
    provisioningWakeupSource = source
    source.resume()
  }

  func cancelAuxiliarySources(for handle: UInt64) {
    if let source = auxiliaryReadSources.removeValue(forKey: handle) {
      source.cancel()
    }
    if let source = auxiliaryWriteSources.removeValue(forKey: handle) {
      source.cancel()
    }
  }

  func cancelAuxiliaryReadSources() {
    for source in auxiliaryReadSources.values {
      source.cancel()
    }
    auxiliaryReadSources.removeAll()
    for source in auxiliaryWriteSources.values {
      source.cancel()
    }
    auxiliaryWriteSources.removeAll()
    if let wakeup = provisioningWakeupSource {
      provisioningWakeupSource = nil
      wakeup.cancel()
    }
  }

  deinit {
    // The coordinator is retained by cancellation handlers, so teardown
    // completes even if the owning surface destroys this bridge first.
    teardown.requestDisconnect()
    readSource?.cancel()
    writeSource?.cancel()
    for source in auxiliaryReadSources.values {
      source.cancel()
    }
    for source in auxiliaryWriteSources.values {
      source.cancel()
    }
    provisioningWakeupSource?.cancel()
  }
}

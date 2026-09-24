import Foundation

struct RuntimeBlockMetadata: Equatable, Sendable {
  enum State: UInt8, Sendable {
    case current = 1
    case completed = 2
  }

  let blockIDLow: UInt64
  let blockIDHigh: UInt64
  let revision: UInt64
  let startLineID: UInt64
  let state: State
}

/// UI registries must include the Pane namespace because Runtime block and
/// history request numbers are only unique within their owning execution.
struct PaneBlockKey: Hashable, Sendable {
  let paneID: String
  let blockID: UInt64

  init(paneID: String, blockID: UInt64) {
    self.paneID = paneID
    self.blockID = blockID
  }

  var accessibilityIdentifier: String {
    "pane.\(paneID).block.\(blockID)"
  }
}

struct PaneHistoryRequestKey: Hashable, Sendable {
  let paneID: String
  let requestID: UInt64

  init(paneID: String, requestID: UInt64) {
    self.paneID = paneID
    self.requestID = requestID
  }
}

struct NativeBlockRecord: Equatable {
  enum State: Equatable {
    case running
    case completed
  }

  let id: UInt64
  let command: String
  let state: State
  let startLine: UInt64
  let endLine: UInt64?
  let exitStatus: Int32
}

struct NativeHistoryRange: Equatable {
  struct Cell: Equatable {
    let scalar: UInt32
    let foreground: UInt32
    let background: UInt32
    let flags: UInt16
    let graphemeUtf8: Data

    init(
      scalar: UInt32,
      foreground: UInt32,
      background: UInt32,
      flags: UInt16,
      graphemeUtf8: Data = Data()
    ) {
      self.scalar = scalar
      self.foreground = foreground
      self.background = background
      self.flags = flags
      self.graphemeUtf8 = graphemeUtf8
    }
  }

  let startLine: UInt64
  let endLine: UInt64
  let blockID: UInt64
  let requestID: UInt64
  let revision: UInt64
  let rows: [[Cell]]
  let status: UInt32

  init(
    startLine: UInt64,
    endLine: UInt64,
    blockID: UInt64,
    requestID: UInt64,
    revision: UInt64,
    rows: [[Cell]],
    status: UInt32 = 0
  ) {
    self.startLine = startLine
    self.endLine = endLine
    self.blockID = blockID
    self.requestID = requestID
    self.revision = revision
    self.rows = rows
    self.status = status
  }
}

private func mergeHistoryRange(
  _ previous: NativeHistoryRange?,
  _ chunk: NativeHistoryRange
) -> NativeHistoryRange {
  guard let previous else { return chunk }
  if previous.rows.isEmpty {
    return NativeHistoryRange(
      startLine: previous.startLine,
      endLine: chunk.endLine,
      blockID: chunk.blockID,
      requestID: chunk.requestID,
      revision: chunk.revision,
      rows: chunk.rows,
      status: chunk.status
    )
  }
  if chunk.rows.isEmpty {
    return NativeHistoryRange(
      startLine: previous.startLine,
      endLine: previous.endLine,
      blockID: chunk.blockID,
      requestID: chunk.requestID,
      revision: chunk.revision,
      rows: previous.rows,
      status: chunk.status
    )
  }
  var rows = previous.rows
  if previous.endLine == chunk.startLine {
    rows[rows.count - 1] = rows[rows.count - 1] + chunk.rows[0]
    rows.append(contentsOf: chunk.rows.dropFirst())
  } else {
    rows.append(contentsOf: chunk.rows)
  }
  return NativeHistoryRange(
    startLine: previous.startLine,
    endLine: chunk.endLine == 0 ? previous.endLine : chunk.endLine,
    blockID: chunk.blockID,
    requestID: chunk.requestID,
    revision: chunk.revision,
    rows: rows,
    status: chunk.status
  )
}

private func historyLeadCount(_ rows: [[NativeHistoryRange.Cell]]) -> UInt32 {
  rows.reduce(into: 0) { count, row in
    count += UInt32(row.filter { $0.scalar != 0 || !$0.graphemeUtf8.isEmpty }.count)
  }
}

private let historyCellSidecarFlag: UInt16 = 1 << 7

private func historyGraphemeUtf8(cell: SeyalHistoryCell, sidecar: Data) -> Data {
  guard cell.flags & historyCellSidecarFlag != 0 else { return Data() }
  let offset = Int(cell.reserved)
  guard offset + 2 <= sidecar.count else { return Data() }
  let length = Int(sidecar[offset]) | (Int(sidecar[offset + 1]) << 8)
  let start = offset + 2
  let end = start + length
  guard length > 0, end <= sidecar.count else { return Data() }
  return sidecar.subdata(in: start..<end)
}

struct NativeComposerResult: Equatable {
  enum Code: Equatable {
    case accepted
    case busy
    case unsupported
    case backpressure
    case invalid
  }

  let requestID: UInt64
  let blockID: UInt64
  let code: Code
}

/// Runtime-published composer eligibility, relayed verbatim to the Rust
/// application root (#978). `eligibility` is `SeyalAppComposerEligibility`;
/// `.cleared` (revision 0) means the transport is gone and Rust must treat
/// the composer as busy until Runtime republishes.
struct NativeComposerStatus: Equatable {
  let eligibility: UInt8
  let revision: UInt64

  static let cleared = NativeComposerStatus(eligibility: 0, revision: 0)
}

/// Geometry for one canonical Block projection on the Pane-owned Metal
/// surface. The surface consumes a complete frame, so lifecycle updates never
/// replace one history buffer at a time or expose a partially updated order.
struct NativeTranscriptRegion: Equatable {
  let id: UInt64
  let origin: NSPoint
  let clip: NSRect
}

struct NativeTranscriptFrame: Equatable {
  let revision: UInt64
  let regions: [NativeTranscriptRegion]
  let surfaceIdentity: ObjectIdentifier?

  init(
    revision: UInt64,
    regions: [NativeTranscriptRegion],
    surfaceIdentity: ObjectIdentifier? = nil
  ) {
    self.revision = revision
    self.regions = regions
    self.surfaceIdentity = surfaceIdentity
  }

  var regionIDs: [UInt64] { regions.map(\.id) }

  var isValid: Bool {
    guard regions.allSatisfy({ $0.id != 0 && $0.clip.width >= 0 && $0.clip.height >= 0 }) else {
      return false
    }
    return Set(regionIDs).count == regionIDs.count
  }

  func applyingDuplicateID(_ id: UInt64) -> NativeTranscriptFrame {
    NativeTranscriptFrame(
      revision: revision,
      regions: regions + [NativeTranscriptRegion(id: id, origin: .zero, clip: .zero)],
      surfaceIdentity: surfaceIdentity
    )
  }
}

/// Composer acceptance is correlated by the Runtime request ID. A command
/// string is intentionally not an identity: two successive submissions may be
/// identical and must still settle independently.
struct ComposerRequestCorrelation {
  private(set) var pendingRequestID: UInt64?
  private var nextRequestID: UInt64 = 1

  var isSettled: Bool { pendingRequestID == nil }

  mutating func begin(command: String) -> UInt64 {
    let requestID = nextRequestID
    nextRequestID = requestID == UInt64.max ? 1 : requestID + 1
    pendingRequestID = requestID
    _ = command
    return requestID
  }

  mutating func accepts(requestID: UInt64) -> Bool {
    guard let pendingRequestID, pendingRequestID == requestID
    else { return false }
    self.pendingRequestID = nil
    return true
  }
}

// Cancellation handlers may outlive RustDisplayBridge and deinit is
// nonisolated in Swift 6. The coordinator serializes its accounting and
// schedules the thread-local Rust disconnect on the main queue.
private final class RustBridgeHandleBox: @unchecked Sendable {
  var value: UInt64 = 0
}

private final class RustBridgeTeardownCoordinator: @unchecked Sendable {
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
    let stage: UInt8
    let failureClass: UInt8
    let retryable: Bool
    let connectionOrigin: UInt8
    let handle: UInt64
    let runtimeIDLow: UInt64
    let runtimeIDHigh: UInt64
    let executionIDLow: UInt64
    let executionIDHigh: UInt64
    let attachmentIDLow: UInt64
    let attachmentIDHigh: UInt64

    init(
      stage: UInt8,
      failureClass: UInt8,
      retryable: Bool,
      connectionOrigin: UInt8,
      handle: UInt64,
      runtimeIDLow: UInt64,
      runtimeIDHigh: UInt64,
      executionIDLow: UInt64,
      executionIDHigh: UInt64,
      attachmentIDLow: UInt64,
      attachmentIDHigh: UInt64
    ) {
      self.stage = stage
      self.failureClass = failureClass
      self.retryable = retryable
      self.connectionOrigin = connectionOrigin
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
        stage: result.stage,
        failureClass: result.failure_class,
        retryable: result.retryable != 0,
        connectionOrigin: result.connection_origin,
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
        stage: opened.stage,
        failureClass: opened.failureClass,
        retryable: opened.retryable,
        connectionOrigin: opened.connectionOrigin,
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

  private let onFrame: FrameHandler
  private let onTimeline: TimelineHandler
  private let onHistory: HistoryHandler
  private let onComposerResult: ComposerResultHandler
  private let onComposerStatus: ComposerStatusHandler
  private let onError: ErrorHandler
  var onStatusChanged: StatusHandler
  var onCopiedText: ((String) -> Void)?
  private var readSource: DispatchSourceRead?
  private var writeSource: DispatchSourceWrite?
  private var socketFileDescriptor: Int32 = -1
  private let handleBox = RustBridgeHandleBox()
  private var teardown: RustBridgeTeardownCoordinator!
  private(set) var clientHandle: UInt64 = 0
  private(set) var isConnected = false
  private(set) var lastRecoveryResult = RecoveryResult.current()
  /// The last bundled-helper failure is retained for the recovery coordinator
  /// to classify as blocked. Do not silently discard trust or spawn errors.
  private(set) var lastLaunchError: BundledRuntimeLaunchError?
  private(set) var runtimeIdentityWords: (low: UInt64, high: UInt64) = (0, 0)
  private(set) var attachmentIdentityWords: (low: UInt64, high: UInt64) = (0, 0)
  private(set) var reconstructionState = ReconnectReconstructionState()
  private(set) var runtimeBlockMetadata: RuntimeBlockMetadata?
  private let runtimeLauncher = BundledRuntimeLauncher()
  private var lastTimelineRevision: UInt64 = 0
  private let paneID: String
  private let requestedExecutionIdentity: String?
  private let allowsImplicitExecutionBootstrap: Bool
  private var requestedHistoryRanges:
    [PaneHistoryRequestKey: (blockID: UInt64, startLine: UInt64, endLine: UInt64)] = [:]
  private var historyRevisions: [PaneHistoryRequestKey: (revision: UInt64, requestID: UInt64)] = [:]
  private var historyContinuations:
    [PaneHistoryRequestKey: (startUnit: UInt32, range: NativeHistoryRange)] = [:]
  /// Block Copy (#1010) requests: Rust-built text accumulated per chunk and
  /// the last line it ended on. Delivered to `onHistoryCopy`, never rendered.
  private var historyCopyRequests: [PaneHistoryRequestKey: (text: String, lastLine: UInt64)] = [:]
  var onHistoryCopy: ((UInt64, String) -> Void)?
  private var lastComposerResultRequestID: UInt64 = 0
  private var lastComposerStatusRevision: UInt64 = 0

  static func teardownReconnectStateSelfTest() -> Bool {
    var disconnects = 0
    let coordinator = RustBridgeTeardownCoordinator {
      disconnects += 1
    }
    coordinator.sourceCreated()
    coordinator.requestDisconnect()
    coordinator.requestDisconnect()
    guard coordinator.disconnectPending, disconnects == 0 else { return false }
    coordinator.sourceCancelled()
    let deadline = Date().addingTimeInterval(1)
    while disconnects == 0 && Date() < deadline {
      RunLoop.current.run(until: Date().addingTimeInterval(0.01))
    }
    return !coordinator.disconnectPending
      && coordinator.activeSourceCount == 0
      && disconnects == 1
  }

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
    reconstructionState.beginAttempt()

    let handle: UInt64
    if let executionIdentity = requestedExecutionIdentity,
      let (low, high) = Self.executionWords(from: executionIdentity)
    {
      handle = seyal_bridge_open_execution(low, high)
    } else if let execution = reconstructionState.expectedExecution {
      handle = seyal_bridge_open_execution(execution.low, execution.high)
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
    reconstructionState.beginAttempt()
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
  private func adoptOpenedHandle(_ handle: UInt64, recoveryResult: RecoveryResult) -> Bool {
    guard seyal_bridge_adopt_handle(handle) == 0 else {
      seyal_bridge_disconnect_handle(handle)
      onError(-1)
      onStatusChanged()
      return false
    }
    return finishAdoptedHandle(handle, recoveryResult: recoveryResult)
  }

  @discardableResult
  private func finishAdoptedHandle(_ handle: UInt64, recoveryResult: RecoveryResult) -> Bool {
    let runtime = RuntimeContinuityIdentity(
      low: recoveryResult.runtimeIDLow,
      high: recoveryResult.runtimeIDHigh
    )
    let execution = RuntimeContinuityIdentity(
      low: recoveryResult.executionIDLow,
      high: recoveryResult.executionIDHigh
    )
    let attachment = RuntimeContinuityIdentity(
      low: recoveryResult.attachmentIDLow,
      high: recoveryResult.attachmentIDHigh
    )
    // A Rust client handle is published only after finish_attach has validated
    // Controller authority and atomically committed the complete initial
    // snapshot. Identity drift or attachment reuse fails closed here before
    // AppKit can submit input or expose stale presentation.
    guard reconstructionState.commit(
      runtime: runtime,
      execution: execution,
      attachment: attachment,
      controllerAuthorityCommitted: true,
      authoritativeSnapshotCommitted: true
    ) else {
      seyal_bridge_disconnect_handle(handle)
      onError(-4)
      onStatusChanged()
      return false
    }
    clientHandle = handle
    handleBox.value = handle
    runtimeIdentityWords = (runtime.low, runtime.high)
    attachmentIdentityWords = (attachment.low, attachment.high)

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
    onStatusChanged()
    return true
  }

  /// Starts only the trusted helper packaged inside Seyal.app. Episode-level
  /// launch-once ownership belongs to RuntimeLifecycleRecoveryCoordinator.
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
    reconstructionState.disconnect()
    runtimeBlockMetadata = nil
    requestedHistoryRanges.removeAll(keepingCapacity: false)
    historyRevisions.removeAll(keepingCapacity: false)
    historyContinuations.removeAll(keepingCapacity: false)
    historyCopyRequests.removeAll(keepingCapacity: false)
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
  /// callers, but replacement scheduling/opening belongs exclusively to
  /// RuntimeLifecycleRecoveryCoordinator.
  func stop(reconnect _: Bool = false) {
    if teardown.disconnectPending {
      // Prior stop armed teardown; finish CLIENT drop on this MainActor turn.
      completeEagerClientDisconnect()
      return
    }
    guard isConnected || socketFileDescriptor >= 0 else { return }

    isConnected = false
    reconstructionState.disconnect()
    runtimeBlockMetadata = nil
    // All request/display correlations are connection-local. A reconnect
    // receives a fresh attachment and must never reuse pending history,
    // composer, timeline, or generation state from the dead socket.
    requestedHistoryRanges.removeAll(keepingCapacity: false)
    historyRevisions.removeAll(keepingCapacity: false)
    historyContinuations.removeAll(keepingCapacity: false)
    historyCopyRequests.removeAll(keepingCapacity: false)
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
  private func completeEagerClientDisconnect() {
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
  private func selectClient() -> Bool {
    guard clientHandle != 0 else { return false }
    return seyal_bridge_select(clientHandle) == 0
  }

  func currentFrame() -> SeyalPreparedFrame? {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return nil }
    let frame = seyal_bridge_frame()
    guard frame.cells != nil, frame.cell_count > 0 else { return nil }
    return frame
  }

  /// Builds the initial PreparedSurface after attach. Idempotent.
  @discardableResult
  func ensurePreparedSurface() -> Bool {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return false }
    return seyal_bridge_ensure_prepared() == 0
  }

  func publishCurrentFrame() {
    guard let frame = currentFrame() else { return }
    onFrame(frame)
  }

  /// Minimal read-only Pass 8 presentation seam. The rich command transcript
  /// remains the independent Pass 7.1 timeline above.
  func currentBlockMetadata() -> RuntimeBlockMetadata? {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return nil }
    let value = seyal_bridge_execution_block_metadata()
    guard value.revision > 0,
      value.start_line_id > 0,
      let state = RuntimeBlockMetadata.State(rawValue: value.state)
    else { return nil }
    return RuntimeBlockMetadata(
      blockIDLow: value.block_id_low,
      blockIDHigh: value.block_id_high,
      revision: value.revision,
      startLineID: value.start_line_id,
      state: state
    )
  }

  func currentTimeline() -> [NativeBlockRecord] {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return [] }
    let count = Int(seyal_bridge_block_count())
    return (0..<count).compactMap { index in
      let record = seyal_bridge_block_record(UInt32(index))
      guard record.id != 0, record.command != nil else { return nil }
      let command =
        String(
          bytes: UnsafeBufferPointer(
            start: record.command,
            count: Int(record.command_len)
          ),
          encoding: .utf8
        ) ?? ""
      return NativeBlockRecord(
        id: record.id,
        command: command,
        state: record.state == 0 ? .running : .completed,
        startLine: record.start_line,
        endLine: record.end_line == 0 ? nil : record.end_line,
        exitStatus: record.exit_status
      )
    }
  }

  func nextComposerRequestID() -> UInt64 {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return 0 }
    return seyal_bridge_next_composer_request_id()
  }

  /// Relay a newer Runtime composer eligibility. Only the revision decides
  /// novelty; the host never interprets the eligibility code.
  private func publishComposerStatus() {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return }
    let status = seyal_bridge_composer_status()
    guard status.revision != 0, status.revision != lastComposerStatusRevision else { return }
    lastComposerStatusRevision = status.revision
    onComposerStatus(NativeComposerStatus(eligibility: status.eligibility, revision: status.revision))
  }

  /// Transport lost: the relayed fact no longer describes a live attachment.
  private func clearComposerStatus() {
    guard lastComposerStatusRevision != 0 else { return }
    lastComposerStatusRevision = 0
    onComposerStatus(.cleared)
  }

  private func publishComposerResult() {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return }
    let result = seyal_bridge_composer_result()
    guard result.request_id != 0,
      result.request_id != lastComposerResultRequestID
    else { return }
    lastComposerResultRequestID = result.request_id
    let code: NativeComposerResult.Code
    switch result.code {
    case 0: code = .accepted
    case 1: code = .busy
    case 2: code = .unsupported
    case 3: code = .backpressure
    default: code = .invalid
    }
    onComposerResult(
      NativeComposerResult(
        requestID: result.request_id,
        blockID: result.block_id,
        code: code
      ))
  }

  @discardableResult
  func requestHistoryRange(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient(),
      blockID > 0, startLine > 0, endLine >= startLine
    else { return -4 }
    let requestID = seyal_bridge_next_history_request_id()
    guard requestID != 0 else { return -4 }
    let result = finishMutation(
      seyal_bridge_request_history_range(blockID, startLine, endLine, 512, 131_072, 0))
    if result == 0 {
      let requestKey = PaneHistoryRequestKey(paneID: paneID, requestID: requestID)
      requestedHistoryRanges[requestKey] = (blockID, startLine, endLine)
    }
    return result
  }

  /// Request one Block's history for pasteboard copy (#1010). The reply is
  /// delivered as Rust-built text through `onHistoryCopy`, not to the renderer.
  @discardableResult
  func requestHistoryCopy(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    let requestID = seyal_bridge_next_history_request_id()
    let result = requestHistoryRange(startLine: startLine, endLine: endLine, blockID: blockID)
    if result == 0, requestID != 0 {
      historyCopyRequests[PaneHistoryRequestKey(paneID: paneID, requestID: requestID)] = ("", 0)
    }
    return result
  }

  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    requestedHistoryRanges = requestedHistoryRanges.filter { blockIDs.contains($0.value.blockID) }
    historyRevisions = historyRevisions.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
    historyContinuations = historyContinuations.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
    historyCopyRequests = historyCopyRequests.filter { requestKey, _ in
      requestedHistoryRanges[requestKey] != nil
    }
  }

  private func publishHistoryRanges() {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return }
    for (requestKey, request) in Array(requestedHistoryRanges) {
      let metadata = seyal_bridge_history_range_peek_for(request.blockID, requestKey.requestID)
      guard metadata.block_id != 0,
        metadata.request_id != 0,
        metadata.block_id == request.blockID,
        metadata.request_id == requestKey.requestID,
        metadata.revision > 0,
        historyRevisions[requestKey]?.revision != metadata.revision
          || historyRevisions[requestKey]?.requestID != metadata.request_id
      else { continue }
      let sidecar = seyal_bridge_history_range_sidecar_for(metadata.block_id, metadata.request_id)
      let sidecarData: Data
      if sidecar.len > 0, let bytes = sidecar.bytes {
        sidecarData = Data(bytes: bytes, count: Int(sidecar.len))
      } else {
        sidecarData = Data()
      }
      let rows = (0..<Int(metadata.row_count)).compactMap { index -> [NativeHistoryRange.Cell]? in
        let row = seyal_bridge_history_range_row_for(
          metadata.block_id,
          metadata.request_id,
          UInt32(index)
        )
        guard row.line_id != 0, let cells = row.cells else { return nil }
        return Array(UnsafeBufferPointer(start: cells, count: Int(row.cell_count))).map {
          NativeHistoryRange.Cell(
            scalar: $0.scalar,
            foreground: $0.foreground,
            background: $0.background,
            flags: $0.flags,
            graphemeUtf8: historyGraphemeUtf8(cell: $0, sidecar: sidecarData)
          )
        }
      }
      historyRevisions[requestKey] = (metadata.revision, metadata.request_id)
      let chunk = NativeHistoryRange(
        startLine: metadata.start_line == 0 ? request.startLine : metadata.start_line,
        endLine: metadata.end_line == 0 ? request.endLine : metadata.end_line,
        blockID: metadata.block_id,
        requestID: metadata.request_id,
        revision: metadata.revision,
        rows: rows,
        status: metadata.reserved
      )
      let previous = historyContinuations[requestKey]
      let merged = mergeHistoryRange(previous?.range, chunk)
      let leads = historyLeadCount(rows)
      let copy = historyCopyRequests.removeValue(forKey: requestKey).map { prior in
        let chunk = seyal_bridge_history_range_text_for(metadata.block_id, metadata.request_id)
        let text = chunk.len > 0 && chunk.bytes != nil
          ? String(decoding: UnsafeBufferPointer(start: chunk.bytes, count: Int(chunk.len)), as: UTF8.self)
          : ""
        // A truncated reply can split one line across chunks; only a new
        // line starts a new text line.
        let separator = prior.text.isEmpty || metadata.start_line == prior.lastLine ? "" : "\n"
        return (text: prior.text + separator + text, lastLine: metadata.end_line)
      }
      _ = seyal_bridge_history_range_consume(metadata.block_id, metadata.request_id)
      requestedHistoryRanges.removeValue(forKey: requestKey)
      historyRevisions.removeValue(forKey: requestKey)
      historyContinuations.removeValue(forKey: requestKey)
      let truncated = metadata.reserved == 1
      if truncated, leads > 0, reconstructionState.canMutate, selectClient() {
        let nextStart = (previous?.startUnit ?? 0) + leads
        let nextID = seyal_bridge_next_history_request_id()
        if nextID != 0 {
          let continued = finishMutation(
            seyal_bridge_request_history_range(
              request.blockID, request.startLine, request.endLine, 512, 131_072, nextStart))
          if continued == 0 {
            let nextKey = PaneHistoryRequestKey(paneID: paneID, requestID: nextID)
            requestedHistoryRanges[nextKey] = request
            historyContinuations[nextKey] = (nextStart, merged)
            if let copy {
              historyCopyRequests[nextKey] = copy
              continue
            }
          }
        }
      }
      if let copy {
        onHistoryCopy?(metadata.block_id, copy.text)
        continue
      }
      onHistory(merged)
    }
  }

  @discardableResult
  func submitCommittedText(_ text: String) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = text.utf8.count
    guard byteCount <= Int(UInt32.max) else {
      onStatusChanged()
      return -14
    }
    let count = UInt32(byteCount)
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_utf8(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_utf8(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  /// Empty paste is invalid (`-4`); UTF-8 above `MAX_INPUT_BYTES` is CommitTooLarge (`-14`).
  static func pasteAdmissionCode(_ text: String) -> Int32 {
    let byteCount = text.utf8.count
    if byteCount == 0 { return -4 }
    if byteCount > 65_536 { return -14 }
    return 0
  }

  static func pasteAdmissionSelfTest() -> Bool {
    pasteAdmissionCode("") == -4
      && pasteAdmissionCode("a") == 0
      && pasteAdmissionCode(String(repeating: "x", count: 65_536)) == 0
      && pasteAdmissionCode(String(repeating: "x", count: 65_537)) == -14
  }

  @discardableResult
  func submitPaste(_ text: String) -> Int32 {
    let admission = Self.pasteAdmissionCode(text)
    guard admission == 0 else {
      onStatusChanged()
      return admission
    }
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = text.utf8.count
    let count = UInt32(byteCount)
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_paste(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_paste(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  @discardableResult
  func submitHostSelection(
    action: UInt8,
    kind: UInt8 = 0,
    startCol: UInt16 = 0,
    startRow: UInt16 = 0,
    endCol: UInt16 = 0,
    endRow: UInt16 = 0
  ) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(
      seyal_bridge_submit_host_selection(action, kind, startCol, startRow, endCol, endRow)
    )
  }

  @discardableResult
  func submitHostSearch(_ needle: String, forward: Bool) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = needle.utf8.count
    guard byteCount <= Int(UInt32.max) else {
      onStatusChanged()
      return -14
    }
    let count = UInt32(byteCount)
    let result =
      needle.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_host_search(buffer.baseAddress, count, forward ? 1 : 0)
      }
      ?? Array(needle.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_host_search(buffer.baseAddress, count, forward ? 1 : 0)
      }
    return finishMutation(result)
  }

  func copiedText() -> String? {
    guard selectClient() else { return nil }
    let copied = seyal_bridge_copied_text()
    guard copied.len > 0, let utf8 = copied.utf8 else { return nil }
    let buffer = UnsafeBufferPointer(start: utf8, count: Int(copied.len))
    return String(decoding: buffer, as: UTF8.self)
  }

  @discardableResult
  func consumeCopiedText() -> Int32 {
    guard selectClient() else { return -1 }
    return seyal_bridge_copied_text_consume()
  }

  @discardableResult
  func submitComposerCommand(_ text: String) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    let byteCount = text.utf8.count
    guard byteCount <= Int(UInt32.max) else {
      onStatusChanged()
      return -14
    }
    let count = UInt32(byteCount)
    let result =
      text.utf8.withContiguousStorageIfAvailable { buffer -> Int32 in
        seyal_bridge_submit_composer(buffer.baseAddress, count)
      }
      ?? Array(text.utf8).withUnsafeBufferPointer { buffer in
        seyal_bridge_submit_composer(buffer.baseAddress, count)
      }
    return finishMutation(result)
  }

  @discardableResult
  func submitKey(kind: UInt16, scalar: UInt32) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_key(kind, scalar))
  }

  func supportsKeyV2() -> Bool {
    guard isConnected, selectClient() else { return false }
    return seyal_bridge_supports_key_v2() != 0
  }

  @discardableResult
  func submitKeyV2(kind: UInt16, modifiers: UInt16, value: UInt32, event: UInt8, shiftedASCII: UInt32, actionID: UInt32) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_key_v2(kind, modifiers, value, event, shiftedASCII, actionID))
  }

  func mouseCell(
    pixelX: Double,
    pixelYFromTop: Double,
    viewportWidth: Double,
    viewportHeight: Double,
    horizontalInsets: Double,
    verticalInsets: Double,
    cellWidth: Double,
    cellHeight: Double
  ) -> (UInt16, UInt16)? {
    var col: UInt16 = 0
    var row: UInt16 = 0
    let ok = seyal_bridge_mouse_cell(
      pixelX,
      pixelYFromTop,
      viewportWidth,
      viewportHeight,
      horizontalInsets,
      verticalInsets,
      cellWidth,
      cellHeight,
      &col,
      &row
    )
    return ok != 0 ? (col, row) : nil
  }

  @discardableResult
  func submitMouse(
    kind: UInt8,
    button: UInt8,
    modifiers: UInt16,
    col: UInt16,
    row: UInt16,
    actionID: UInt32
  ) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_submit_mouse(kind, button, modifiers, col, row, actionID))
  }

  @discardableResult
  func proposeGeometry(
    viewportWidth: Double,
    viewportHeight: Double,
    horizontalInsets: Double,
    verticalInsets: Double,
    cellWidth: Double,
    cellHeight: Double,
    meaningfulLayoutEpoch: Bool
  ) -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(
      seyal_bridge_propose_geometry(
        viewportWidth,
        viewportHeight,
        horizontalInsets,
        verticalInsets,
        cellWidth,
        cellHeight,
        meaningfulLayoutEpoch ? 1 : 0
      )
    )
  }

  @discardableResult
  func retryResize() -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else {
      onStatusChanged()
      return -10
    }
    return finishMutation(seyal_bridge_retry_resize())
  }

  func inputFailureCode() -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return 4 }
    return seyal_bridge_input_failure()
  }

  func resizeFailureCode() -> Int32 {
    guard isConnected, reconstructionState.canMutate, selectClient() else { return 201 }
    return seyal_bridge_resize_failure()
  }

  private func finishMutation(_ result: Int32) -> Int32 {
    synchronizeWriteReadinessSource()
    onStatusChanged()
    if result == -18 {
      onError(result)
      stop(reconnect: true)
    } else if result == -3 || result == -10 {
      onError(result)
      stop()
    }
    return result
  }

  private func drainReadyDisplayWork() {
    guard isConnected, selectClient() else { return }
    defer {
      synchronizeWriteReadinessSource()
      onStatusChanged()
    }

    // Rust bounds each poll by frame count and bytes. A small outer bound
    // prevents one high-volume terminal from monopolizing the AppKit queue;
    // unread socket data will immediately retrigger this dispatch source.
    for _ in 0..<8 {
      let result = seyal_bridge_poll()
      runtimeBlockMetadata = currentBlockMetadata()
      publishHistoryRanges()
      publishComposerResult()
      publishComposerStatus()
      if let text = copiedText() {
        onCopiedText?(text)
        _ = consumeCopiedText()
      }
      let revision = seyal_bridge_block_timeline_revision()
      if revision != lastTimelineRevision {
        lastTimelineRevision = revision
        onTimeline()
      }
      if result == 1 {
        publishCurrentFrame()
        continue
      }
      if result == 0 {
        return
      }

      onError(result)
      stop(reconnect: result == -18)
      return
    }
  }

  private func synchronizeWriteReadinessSource() {
    guard isConnected, selectClient() else {
      writeSource?.cancel()
      writeSource = nil
      return
    }

    let wantsWrite = seyal_bridge_wants_write()
    if wantsWrite < 0 {
      onError(wantsWrite)
      stop()
      return
    }
    guard wantsWrite == 1 else {
      writeSource?.cancel()
      writeSource = nil
      return
    }
    guard writeSource == nil, socketFileDescriptor >= 0 else { return }

    let source = DispatchSource.makeWriteSource(
      fileDescriptor: socketFileDescriptor,
      queue: .main
    )
    source.setEventHandler { [weak self] in
      seyalRunAsMainActorFromMainQueue {
        self?.flushReadyControlWork()
      }
    }
    source.setCancelHandler { [teardown = teardown!] in
      teardown.sourceCancelled()
    }
    teardown.sourceCreated()
    writeSource = source
    source.resume()
  }

  deinit {
    // The coordinator is retained by cancellation handlers, so teardown
    // completes even if the owning surface destroys this bridge first.
    teardown.requestDisconnect()
    readSource?.cancel()
    writeSource?.cancel()
  }

  private func flushReadyControlWork() {
    guard isConnected, selectClient() else { return }
    let result = seyal_bridge_flush_writable()
    guard result == 0 else {
      onError(result)
      stop(reconnect: result == -18)
      return
    }
    synchronizeWriteReadinessSource()
    onStatusChanged()
  }
}

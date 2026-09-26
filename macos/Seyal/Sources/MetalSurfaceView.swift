import AppKit
import Metal
@preconcurrency import QuartzCore

class MetalSurfaceView: NSView, CAMetalDisplayLinkDelegate {
  /// How AppKit installs the surface presenter.
  enum Installation: Equatable {
    /// Production Metal display path (`CAMetalLayer` + Runtime bridge).
    case fullDisplay
    /// SPEC-009 §10 first-responder / AX / IME only: plain layer, no Runtime
    /// bridge, no CAMetalLayer drawables. Used by Pass 9 native_ready probe so
    /// the soak's MetalTerminalRenderer remains the sole display presenter.
    case nativeInteractionProbe
  }

  let paneID: String
  let requestedExecutionIdentity: String?
  let allowsImplicitExecutionBootstrap: Bool
  let installation: Installation
  let metalDevice: any MTLDevice
  let renderer: MetalTerminalRenderer
  var bridge: RustDisplayBridge?
  /// Rust application root whose `RecoveryCoordinator` owns this surface's
  /// recovery episodes. Zero means the surface has no recovery owner and never
  /// starts an episode.
  var recoveryAppHandle: UInt64 { 0 }
  /// Rust accepted a recovery action; the host effect executor must drain
  /// the pending recovery effects.
  var onRecoveryEffectsPending: (() -> Void)?
  /// An adopted handle still has to report SPEC-009 §10 Restoring/Usable
  /// presentation progress to Rust.
  var recoveryPresentationPending = false
  /// When true, the surface still supports first-responder / AX / IME restore
  /// (SPEC §10) but does not begin automatic Runtime recovery. Used by the
  /// Pass 9 native_ready probe so it does not open a second client alongside
  /// the qualification soak bridge.
  var suppressesAutomaticBridgeRecovery = false
  var forceNextFrame = false
  var hasPreparedState = false
  var presentationState = PresentationRecoveryState()
  var presentationRetryScheduled = false
  var presentationRetryTimer: Timer?
  var presentationRetryGeneration: UInt64 = 0
  var renderable = false
  var metalDisplayLinkLease: MetalDisplayLinkLease?
  /// Identity for CAMetalDisplayLink hops without capturing `@MainActor self`
  /// in a way that inserts `assumeIsolated` under Xcode 16.4.
  nonisolated(unsafe) var displayLinkHopTarget: Unmanaged<MetalSurfaceView>?
  var preparationRetryTimer: Timer?
  var preparationRetryGeneration: UInt64 = 0
  var preparationRetryScheduled = false
  var preparationState = PreparationRecoveryState()
  var lastAlternateScreen: Bool?
  var isDetachingRuntimeConnection = false
  var lastBridgeError: Int32?
  var lastRenderError: Error?
  var historyRanges: [PaneBlockKey: NativeHistoryRange] = [:]
  var lastProposedGeometry = CGRect.null
  var proposingGeometry = false

  override convenience init(frame frameRect: NSRect) {
    self.init(frame: frameRect, paneID: "unbound")
  }

  init(
    frame frameRect: NSRect,
    paneID: String,
    executionIdentity: String? = nil,
    allowsImplicitExecutionBootstrap: Bool = true,
    terminalFont: SeyalResolvedFontSpec = .canonicalTerminal,
    installation: Installation = .fullDisplay
  ) {
    self.paneID = paneID
    self.requestedExecutionIdentity = executionIdentity
    self.allowsImplicitExecutionBootstrap = allowsImplicitExecutionBootstrap
    self.installation = installation
    guard let device = MTLCreateSystemDefaultDevice() else {
      fatalError("Seyal requires a Metal-capable macOS device")
    }
    let renderer: MetalTerminalRenderer
    do {
      renderer = try MetalTerminalRenderer(device: device, terminalFont: terminalFont)
    } catch {
      fatalError("Seyal permanent Metal renderer initialization failed: \(error)")
    }

    metalDevice = device
    self.renderer = renderer
    super.init(frame: frameRect)
    displayLinkHopTarget = Unmanaged.passUnretained(self)
    wantsLayer = true

    switch installation {
    case .fullDisplay:
      guard let metalLayer = layer as? CAMetalLayer else {
        fatalError("MetalSurfaceView backing layer must be CAMetalLayer")
      }
      metalLayer.device = device
      metalLayer.pixelFormat = .bgra8Unorm
      metalLayer.framebufferOnly = true
      metalLayer.maximumDrawableCount = 2
      metalLayer.presentsWithTransaction = false
      metalLayer.isOpaque = true
      updateDrawableSize()

      // No dedicated GPU surface resources are retained before the view is
      // actually visible. Candidate-D state may still advance independently.
      renderer.setVisible(false)
      let hopTarget = displayLinkHopTarget!
      renderer.onNeedsCurrentFrame = {
        seyalRunAsMainActorFromMainQueue {
          hopTarget.takeUnretainedValue().bridge?.publishCurrentFrame()
        }
      }
      renderer.onPersistentDisplayFailure = { error in
        seyalRunAsMainActorFromMainQueue {
          hopTarget.takeUnretainedValue().lastRenderError = error
        }
      }

      let bridge = RustDisplayBridge(
        onFrame: { [weak self] frame in
          self?.consumeBridgeFrame(frame)
        },
        onError: { [weak self] code in
          self?.lastBridgeError = code
          self?.terminalBridgeDidFail(code)
        },
        onStatusChanged: { [weak self] in
          self?.terminalBridgeStatusDidChange()
        },
        onTimeline: { [weak self] in
          self?.onTimelineChanged?()
        },
        onHistory: { [weak self] range in
          guard let self else { return }
          // Retain rows before chrome publishes clips. `setTranscriptFrame`
          // only re-encodes ranges already stored here; dropping this store
          // leaves Block bodies empty even after Runtime history arrives.
          self.retainHistoryRange(range)
          if let onHistoryRangeChanged {
            onHistoryRangeChanged(range)
          } else {
            self.renderHistoryRange(range)
          }
        },
        onComposerResult: { [weak self] result in
          self?.onComposerResultChanged?(result)
        },
        onComposerStatus: { [weak self] status in
          self?.onComposerStatusChanged?(status)
        },
        paneID: paneID,
        executionIdentity: executionIdentity,
        allowsImplicitExecutionBootstrap: allowsImplicitExecutionBootstrap
      )
      bridge.onCopiedText = { text in
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
      }
      self.bridge = bridge
      bridge.continuityAppHandle = recoveryAppHandle
      // A production surface must not perform a synchronous pre-attempt on the
      // AppKit thread. Visibility starts the one authoritative recovery episode
      // so startup, retries and cancellation share the exact seven-attempt/
      // one-second contract instead of creating an eighth attempt with a fresh
      // timeout before the Rust RecoveryCoordinator episode begins.

    case .nativeInteractionProbe:
      // SPEC §10 probe: InteractiveMetalSurfaceView restore only. The Pass 9
      // soak owns the live Runtime/Metal presenter; this view must not allocate
      // CAMetalLayer drawables or a second client bridge.
      suppressesAutomaticBridgeRecovery = true
      renderer.setVisible(false)
    }
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    fatalError("Seyal uses a programmatic AppKit/Metal surface")
  }

  override func makeBackingLayer() -> CALayer {
    switch installation {
    case .fullDisplay:
      CAMetalLayer()
    case .nativeInteractionProbe:
      CALayer()
    }
  }

  /// Narrow subclass hooks for Pass 7 presentation-only failure/focus state.
  /// They never transfer PTY, VT, grid or renderer authority into AppKit.
  override func hitTest(_ point: NSPoint) -> NSView? {
    // Flow paints terminal pixels over Block bodies. Chrome owns scrolling
    // and composer hit-testing, so the compositor must not swallow events.
    if inspectRendererPresentation().drawsLiveGrid {
      return super.hitTest(point)
    }
    return nil
  }

  func inspectRendererPresentation() -> RendererPresentationInspection {
    renderer.inspectPresentation()
  }

  func inspectFlowPaint(sampleGPU: Bool = false) -> FlowPaintInspection {
    guard sampleGPU else {
      return renderer.inspectFlowPaint()
    }
    let size = convertToBacking(bounds).size
    let width = max(1, Int(size.width.rounded()))
    let height = max(1, Int(size.height.rounded()))
    guard let texture = renderer.renderOffscreenAndWait(width: width, height: height) else {
      return renderer.inspectFlowPaint()
    }
    return renderer.inspectFlowPaint(from: texture)
  }

  func setTranscriptFrame(_ frame: NativeTranscriptFrame) {
    guard frame.isValid,
      frame.surfaceIdentity == nil || frame.surfaceIdentity == ObjectIdentifier(self)
    else {
      return
    }
    let regionIDs = Set(frame.regionIDs)
    historyRanges = historyRanges.filter { regionIDs.contains($0.key.blockID) }
    renderer.removeHistoryRegions(except: regionIDs)
    renderer.setHistoryRegionOrder(frame.regionIDs)
    // Body intrinsic growth moves every following Block. Re-encode all
    // retained canonical ranges against this complete frame so no region
    // retains its previous clip or origin.
    for region in frame.regions {
      guard let range = historyRanges[PaneBlockKey(paneID: paneID, blockID: region.id)] else {
        continue
      }
      renderHistoryRange(range, region: region)
    }
  }

  func removeTranscriptRegions(except ids: Set<UInt64>) {
    historyRanges = historyRanges.filter { ids.contains($0.key.blockID) }
    renderer.removeHistoryRegions(except: ids)
  }

  var terminalExecutionIdentity: String? {
    guard terminalBridgeIsConnected, let bridge else { return nil }
    return Self.identityString((
      low: bridge.lastRecoveryResult.executionIDLow,
      high: bridge.lastRecoveryResult.executionIDHigh
    ))
  }

  var terminalRuntimeIdentity: String? {
    guard terminalBridgeIsConnected, let bridge else { return nil }
    return Self.identityString(bridge.runtimeIdentityWords)
  }

  var terminalAttachmentIdentity: String? {
    guard terminalBridgeIsConnected, let bridge else { return nil }
    return Self.identityString(bridge.attachmentIdentityWords)
  }

  /// Makes the authoritative recovery state observable to VoiceOver and to
  /// native acceptance automation without introducing a second terminal
  /// model or changing the Runtime protocol.
  func refreshRecoveryAccessibilityValue() {
    let connection = terminalBridgeIsConnected ? "usable" : "disconnected"
    let runtime = terminalRuntimeIdentity ?? "none"
    let execution = terminalExecutionIdentity ?? "none"
    let attachment = terminalAttachmentIdentity ?? "none"
    let alternate = lastAlternateScreen == true ? "true" : "false"
    let flowPaint = renderer.inspectFlowPaint().accessibilityToken
    setAccessibilityValue(
      "process=\(ProcessInfo.processInfo.processIdentifier) connection=\(connection) "
        + "runtime=\(runtime) execution=\(execution) "
        + "attachment=\(attachment) alternate-screen=\(alternate) "
        + "flow-paint=\(flowPaint)"
    )
  }

  static func identityString(
    _ words: (low: UInt64, high: UInt64)
  ) -> String? {
    guard words.low != 0 || words.high != 0 else { return nil }
    return String(format: "%016llx%016llx", words.high, words.low)
  }

  /// Logical cell metrics come from the permanent renderer's font/atlas metric
  /// source. Resize code must not independently remeasure fonts.
  func terminalLogicalCellSize() -> CGSize {
    let pixels = renderer.cellPixelSize(backingScale: 1)
    return CGSize(width: CGFloat(pixels.width), height: CGFloat(pixels.height))
  }

  /// Candidate-window anchoring needs logical points for the current screen.
  func terminalPresentationCellSize() -> CGSize {
    let scale = max(window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1, 1)
    let pixels = renderer.cellPixelSize(backingScale: scale)
    return CGSize(
      width: CGFloat(pixels.width) / scale,
      height: CGFloat(pixels.height) / scale
    )
  }

  override func layout() {
    super.layout()
    if suppressesAutomaticBridgeRecovery {
      // SPEC §10 probe: keep a 1×1 drawable so key-window/first-responder
      // restore does not allocate full-size CAMetalLayer backings each cycle.
      if let metalLayer = layer as? CAMetalLayer {
        metalLayer.drawableSize = CGSize(width: 1, height: 1)
      }
      return
    }
    updateDrawableSize()
    proposeCurrentGeometry()
    guard shouldRender,
      hasPreparedState,
      renderer.persistentDisplayFailure == nil,
      !presentationState.exhausted
    else { return }
    renderer.requestPresent()
    beginPresentationAttemptSeries()
    armMetalDisplayLink()
  }

  override func viewDidChangeBackingProperties() {
    super.viewDidChangeBackingProperties()
    updateDrawableSize()
    forceNextFrame = true
    if shouldRender {
      bridge?.publishCurrentFrame()
    }
  }

  override func viewDidHide() {
    super.viewDidHide()
    updateVisibility()
  }

  override func viewDidUnhide() {
    super.viewDidUnhide()
    updateVisibility()
  }

  override func viewWillMove(toWindow newWindow: NSWindow?) {
    if let window {
      NotificationCenter.default.removeObserver(
        self,
        name: NSWindow.didChangeOcclusionStateNotification,
        object: window
      )
      NotificationCenter.default.removeObserver(
        self,
        name: NSWindow.didBecomeKeyNotification,
        object: window
      )
      NotificationCenter.default.removeObserver(
        self,
        name: NSWindow.didBecomeMainNotification,
        object: window
      )
    }
    if newWindow == nil {
      // Suppress status-driven reconnect before stop() publishes its
      // disconnected transition. Teardown is detach-only and must not create
      // a replacement foreground recovery episode.
      detachRuntimeConnectionForApplicationTermination()
    }
    super.viewWillMove(toWindow: newWindow)
  }

  /// Detaches the disposable GUI-side Runtime client before the application
  /// exits. The Runtime/helper intentionally survives GUI lifetime, but its
  /// controller lease must be released before a later Seyal launch attempts
  /// to reacquire the same execution.
  func detachRuntimeConnectionForApplicationTermination() {
    isDetachingRuntimeConnection = true
    renderable = false
    renderer.setVisible(false)
    invalidatePreparedPresentation()
    invalidateMetalDisplayLink()
    cancelBridgeReconnect()
    bridge?.stop()
  }

  override func viewDidMoveToWindow() {
    super.viewDidMoveToWindow()
    if window != nil {
      isDetachingRuntimeConnection = false
    }
    if let window {
      NotificationCenter.default.addObserver(
        self,
        selector: #selector(windowOcclusionChanged),
        name: NSWindow.didChangeOcclusionStateNotification,
        object: window
      )
      NotificationCenter.default.addObserver(
        self,
        selector: #selector(windowActivationChanged),
        name: NSWindow.didBecomeKeyNotification,
        object: window
      )
      NotificationCenter.default.addObserver(
        self,
        selector: #selector(windowActivationChanged),
        name: NSWindow.didBecomeMainNotification,
        object: window
      )
    }
    updateDrawableSize()
    updateVisibility()
    // The production shell installs its content view before ordering the
    // window front. Recheck after AppKit completes that ordering so the
    // visibility-gated Runtime recovery episode cannot be stranded at
    // `disconnected` during launch.
    DispatchQueue.main.async { [weak self] in
      self?.updateVisibility()
    }
  }

  @objc func windowOcclusionChanged() {
    updateVisibility()
  }

  @objc func windowActivationChanged() {
    updateVisibility()
  }

  var shouldRender: Bool {
    guard shouldAttachRuntime else { return false }
    guard let window else { return false }
    return window.occlusionState.contains(.visible)
  }

  /// Runtime attachment is a lifecycle concern, not a Metal presentation
  /// concern. AppKit may report a stale/non-visible occlusion state while a
  /// newly reopened window is already visible and focusable. Gating attach on
  /// that state strands the pane with no Runtime, no Metal frame, and no
  /// working Enter key until an unrelated window/occlusion notification.
  /// A surface already installed in a non-miniaturized window is an eligible
  /// foreground pane even while AppKit is still settling visibility or an
  /// ancestor's occlusion bookkeeping.
  var shouldAttachRuntime: Bool {
    guard let window else { return false }
    return !window.isMiniaturized
  }

  /// AppKit can finish attaching a scroll-view sibling to its window after
  /// `viewDidMoveToWindow` has already run. Re-evaluate the lifecycle boundary
  /// after the shell's window has been ordered front so Runtime discovery is
  /// never left stranded in `.disconnected`.
  func activateRuntimeAfterWindowPresentation() {
    updateVisibility()
    startAutomaticBridgeRecoveryIfNeeded()
    refreshRecoveryAccessibilityValue()
  }

  func updateVisibility() {
    let renderable = shouldRender
    let becameRenderable = renderable && !self.renderable
    self.renderable = renderable
    if suppressesAutomaticBridgeRecovery {
      // SPEC §10 probe surfaces: first-responder / AX / IME only — no Metal
      // display-link or automatic Runtime recovery alongside the soak bridge.
      invalidateMetalDisplayLink()
      renderer.setVisible(false)
      return
    }
    if renderable {
      if let metalLayer = layer as? CAMetalLayer {
        installMetalDisplayLink(on: metalLayer)
      }
      forceNextFrame = true
      startAutomaticBridgeRecoveryIfNeeded()
    } else {
      invalidateMetalDisplayLink()
      invalidatePreparedPresentation()
    }

    renderer.setVisible(renderable)
    if renderable {
      // Showing is the explicit recovery boundary for an exhausted GPU
      // completion failure series. Reconstruct from the latest committed
      // Candidate-D state; never request PTY-byte replay.
      if becameRenderable {
        presentationState.resetForLifecycleRecovery()
        lastRenderError = nil
      } else if renderer.persistentDisplayFailure == nil,
        !presentationState.exhausted
      {
        lastRenderError = nil
      }
      bridge?.publishCurrentFrame()
      if hasPreparedState, !presentationState.exhausted {
        renderer.requestPresent()
        beginPresentationAttemptSeries()
        armMetalDisplayLink()
      }
    } else if shouldAttachRuntime {
      // The window can be attachable before AppKit publishes a reliable
      // occlusion state. Keep Runtime recovery independent from presentation;
      // the renderer remains hidden until a real visible-frame opportunity.
      startAutomaticBridgeRecoveryIfNeeded()
    }
  }

  func consumeBridgeFrame(_ bridgeFrame: SeyalPreparedFrame) {
    guard !presentationState.exhausted,
      preparationState.canAttemptPreparation
    else {
      forceNextFrame = true
      return
    }
    guard let frame = NativePreparedFrame(bridgeFrame: bridgeFrame) else {
      return
    }
    onFrameChanged?(frame)
    if lastAlternateScreen != frame.alternateScreen {
      lastAlternateScreen = frame.alternateScreen
      onAlternateScreenChanged?(frame.alternateScreen)
    }
    refreshRecoveryAccessibilityValue()
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    do {
      let result = try renderer.update(
        frame: frame,
        backingScale: scale,
        forceFullRebuild: forceNextFrame
      )
      if result == .updated {
        forceNextFrame = false
        hasPreparedState = true
        // Candidate-D can continue advancing while an exhausted GPU
        // display failure is latched. A successful CPU preparation must
        // not erase that asynchronous display diagnostic.
        if renderer.persistentDisplayFailure == nil,
          !presentationState.exhausted
        {
          lastRenderError = nil
        }
        resetPreparationRecovery()
        guard advanceRecoveryPresentationIfReady() else {
          return
        }
        if shouldRender,
          renderer.persistentDisplayFailure == nil,
          !presentationState.exhausted
        {
          beginPresentationAttemptSeries()
          armMetalDisplayLink()
        }
      }
    } catch {
      lastRenderError = error
      // Renderer preparation is incremental for damage efficiency.  A
      // failed replacement must not leave a partially updated live
      // buffer eligible for a later present.
      hasPreparedState = false
      // Keep the preparation recovery series alive across failures. The
      // lifecycle invalidation path resets it; resetting here would make
      // a persistent resource failure retry forever at the first delay.
      cancelPresentationOpportunity()
      renderer.invalidatePreparedState()
      forceNextFrame = true
      guard let delay = preparationState.recordFailure() else {
        lastRenderError = MetalTerminalRendererError.preparationFailuresExhausted
        return
      }
      schedulePreparationRetry(after: delay)
    }
  }

  func schedulePreparationRetry(after delay: TimeInterval) {
    guard shouldRender,
      !hasPreparedState,
      !preparationRetryScheduled,
      preparationState.canAttemptPreparation
    else {
      return
    }

    preparationRetryScheduled = true
    let generation = preparationRetryGeneration
    preparationRetryTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) {
      [weak self] _ in
      seyalRunAsMainActorFromMainQueue {
        self?.runPreparationRetry(generation: generation)
      }
    }
  }

  func runPreparationRetry(generation: UInt64) {
    guard generation == preparationRetryGeneration else { return }
    preparationRetryTimer = nil
    preparationRetryScheduled = false
    guard shouldRender, !hasPreparedState else { return }
    // Publish only after the current update call has unwound. This keeps
    // retry recovery asynchronous and avoids re-entering renderer.update.
    bridge?.publishCurrentFrame()
  }

  func resetPreparationRetries() {
    preparationRetryTimer?.invalidate()
    preparationRetryTimer = nil
    preparationRetryGeneration &+= 1
    preparationRetryScheduled = false
  }

  func resetPreparationRecovery() {
    resetPreparationRetries()
    preparationState.resetForLifecycleRecovery()
  }

  func terminalBridgeDidFail(_ code: Int32) {
    _ = code
  }

  func terminalBridgeStatusDidChange() {
    refreshRecoveryAccessibilityValue()
    guard !isDetachingRuntimeConnection else { return }
    if bridge?.isConnected == true {
      // Propose from `layout()` only. `proposeGeometry` always finishes with
      // `onStatusChanged`, so calling it here re-enters this method until the
      // stack overflows (EXC_BAD_ACCESS on the guard page).
      needsLayout = true
      return
    }
    lastProposedGeometry = .null
    recoveryPresentationPending = false
    // History/composer/display correlations are disposable connection state;
    // logical pane and Block identity remain owned by Runtime and are not
    // cleared here.
    historyRanges.removeAll(keepingCapacity: false)
    invalidatePreparedPresentation()
    startAutomaticBridgeRecoveryIfNeeded()
  }


  /// SPEC-009 §10: renderer-ready → native interaction before `Usable`.
  /// Base Metal surface has no text-input/first-responder seam; subclasses that
  /// own `NSTextInputClient` must restore focus/AX/IME here.
  @discardableResult
  func restoreNativeInteractionAfterRendererReady() -> Bool {
    true
  }

  /// Presentation-only notification. Runtime/Metal remains authoritative;
  /// AppKit uses this to switch the surrounding Pane chrome.
  var onAlternateScreenChanged: ((Bool) -> Void)?
  var onFrameChanged: ((NativePreparedFrame) -> Void)?
  var onTimelineChanged: (() -> Void)?
  var onHistoryRangeChanged: ((NativeHistoryRange) -> Void)?
  var onComposerResultChanged: ((NativeComposerResult) -> Void)?
  var onComposerStatusChanged: ((NativeComposerStatus) -> Void)?

  var terminalBridgeIsConnected: Bool {
    bridge?.isConnected == true
  }

  @discardableResult
  func terminalSubmitCommittedText(_ text: String) -> Int32 {
    bridge?.submitCommittedText(text) ?? -10
  }

  @discardableResult
  func terminalSubmitPaste(_ text: String) -> Int32 {
    bridge?.submitPaste(text) ?? -10
  }

  @discardableResult
  func terminalSubmitHostSelection(
    action: UInt8,
    kind: UInt8 = 0,
    startCol: UInt16 = 0,
    startRow: UInt16 = 0,
    endCol: UInt16 = 0,
    endRow: UInt16 = 0
  ) -> Int32 {
    bridge?.submitHostSelection(
      action: action,
      kind: kind,
      startCol: startCol,
      startRow: startRow,
      endCol: endCol,
      endRow: endRow
    ) ?? -10
  }

  func terminalSubmitComposerCommand(_ text: String) -> Int32 {
    bridge?.submitComposerCommand(text) ?? -10
  }

  func terminalNextComposerRequestID() -> UInt64 {
    bridge?.nextComposerRequestID() ?? 0
  }

  func requestHistoryRange(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    bridge?.requestHistoryRange(startLine: startLine, endLine: endLine, blockID: blockID) ?? -10
  }

  func retainHistoryRange(_ range: NativeHistoryRange) {
    historyRanges[PaneBlockKey(paneID: paneID, blockID: range.blockID)] = range
  }

  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    bridge?.discardHistoryRequests(except: blockIDs)
  }

  @discardableResult
  func terminalSubmitKey(kind: UInt16, scalar: UInt32) -> Int32 {
    bridge?.submitKey(kind: kind, scalar: scalar) ?? -10
  }

  func terminalSupportsKeyV2() -> Bool {
    bridge?.supportsKeyV2() ?? false
  }

  /// Stop the current attachment so the Rust RecoveryCoordinator can
  /// establish a fresh connection. Used when V2 action IDs are exhausted.
  func terminalStopForProtocolRecovery() {
    bridge?.stop()
  }

  @discardableResult
  func terminalSubmitKeyV2(kind: UInt16, modifiers: UInt16, value: UInt32, event: UInt8, shiftedASCII: UInt32, actionID: UInt32) -> Int32 {
    bridge?.submitKeyV2(kind: kind, modifiers: modifiers, value: value, event: event, shiftedASCII: shiftedASCII, actionID: actionID) ?? -10
  }

  func terminalMouseCell(for event: NSEvent) -> (UInt16, UInt16)? {
    let point = convert(event.locationInWindow, from: nil)
    let cell = terminalPresentationCellSize()
    guard cell.width > 0, cell.height > 0 else { return nil }
    return bridge?.mouseCell(
      pixelX: Double(point.x),
      pixelYFromTop: Double(bounds.height - point.y),
      viewportWidth: Double(bounds.width),
      viewportHeight: Double(bounds.height),
      horizontalInsets: 0,
      verticalInsets: 0,
      cellWidth: Double(cell.width),
      cellHeight: Double(cell.height)
    )
  }

  @discardableResult
  func terminalSubmitMouse(
    kind: UInt8,
    button: UInt8,
    modifiers: UInt16,
    col: UInt16,
    row: UInt16,
    actionID: UInt32
  ) -> Int32 {
    bridge?.submitMouse(
      kind: kind,
      button: button,
      modifiers: modifiers,
      col: col,
      row: row,
      actionID: actionID
    ) ?? -10
  }

  @discardableResult
  func terminalProposeGeometry(
    viewportWidth: Double,
    viewportHeight: Double,
    horizontalInsets: Double,
    verticalInsets: Double,
    cellWidth: Double,
    cellHeight: Double,
    meaningfulLayoutEpoch: Bool
  ) -> Int32 {
    bridge?.proposeGeometry(
      viewportWidth: viewportWidth,
      viewportHeight: viewportHeight,
      horizontalInsets: horizontalInsets,
      verticalInsets: verticalInsets,
      cellWidth: cellWidth,
      cellHeight: cellHeight,
      meaningfulLayoutEpoch: meaningfulLayoutEpoch
    ) ?? -10
  }

  @discardableResult
  func terminalRetryResize() -> Int32 {
    bridge?.retryResize() ?? -10
  }

  func terminalInputFailureCode() -> Int32 {
    bridge?.inputFailureCode() ?? 4
  }

  func terminalResizeFailureCode() -> Int32 {
    bridge?.resizeFailureCode() ?? 201
  }

  func terminalCurrentFrame() -> SeyalPreparedFrame? {
    bridge?.currentFrame()
  }

  /// Republishes the latest committed frame after a presentation consumer
  /// installs its callback. The bridge may publish once during initialization
  /// before the surrounding Block body is attached.
  func publishCurrentTerminalFrame() {
    bridge?.publishCurrentFrame()
  }

  /// Installs the bounded Runtime history projection into this Pane's one
  /// Metal renderer. The callback is intentionally asynchronous at the
  /// bridge boundary but preparation itself remains main-thread confined with
  /// the rest of AppKit/Metal ownership.
  func renderHistoryRange(_ range: NativeHistoryRange, region: NativeTranscriptRegion? = nil) {
    retainHistoryRange(range)
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    do {
      let rendererRegion: NativeTranscriptRegion
      if let region {
        // AppKit uses a bottom-left origin while the terminal shader
        // consumes top-left pixel coordinates.
        let pixelClip = NSRect(
          x: region.clip.minX * scale,
          y: (bounds.height - region.clip.maxY) * scale,
          width: region.clip.width * scale,
          height: region.clip.height * scale
        )
        rendererRegion = NativeTranscriptRegion(
          id: region.id,
          origin: pixelClip.origin,
          clip: pixelClip
        )
      } else {
        rendererRegion = NativeTranscriptRegion(
          id: range.blockID,
          origin: .zero,
          clip: NSRect(
            x: 0,
            y: 0,
            width: bounds.width * scale,
            height: bounds.height * scale
          )
        )
      }
      try renderer.update(
        historyRange: range,
        region: rendererRegion,
        backingScale: scale
      )
      if shouldRender {
        renderer.requestPresent()
        beginPresentationAttemptSeries()
        armMetalDisplayLink()
      }
      refreshRecoveryAccessibilityValue()
    } catch {
      lastRenderError = error
    }
  }

  func applyRendererPresentation(_ plan: RendererPresentationPlan) {
    renderer.setPresentationPlan(plan)
    layer?.isOpaque = plan.drawsFullGridBackground
    if let metalLayer = layer as? CAMetalLayer {
      metalLayer.isOpaque = plan.drawsFullGridBackground
      // Flow composites glyphs over AppKit Block chrome. The compositor must
      // be able to sample alpha; framebuffer-only drawables skip that path.
      metalLayer.framebufferOnly = plan.drawsFullGridBackground
    }
  }

  override var isOpaque: Bool {
    inspectRendererPresentation().drawsFullGridBackground
  }
}

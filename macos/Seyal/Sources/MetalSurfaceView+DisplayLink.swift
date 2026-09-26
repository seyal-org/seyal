import AppKit
import Metal
@preconcurrency import QuartzCore

@MainActor
extension MetalSurfaceView {
  func installMetalDisplayLink(on metalLayer: CAMetalLayer) {
    guard metalDisplayLinkLease == nil else { return }
    let lease = MetalDisplayLinkLease(layer: metalLayer)
    let link = lease.link
    link.delegate = self
    link.isPaused = true
    link.add(to: .main, forMode: .common)
    metalDisplayLinkLease = lease
  }

  func invalidateMetalDisplayLink() {
    metalDisplayLinkLease = nil
  }

  func armMetalDisplayLink() {
    guard shouldRender,
      hasPreparedState,
      presentationState.pending,
      !presentationState.exhausted,
      renderer.hasPresentablePreparedState,
      !renderer.hasFrameInFlight
    else {
      return
    }
    if presentationState.armIfNeeded() {
      metalDisplayLinkLease?.link.isPaused = false
    }
  }

  nonisolated func metalDisplayLink(
    _ link: CAMetalDisplayLink,
    needsUpdate update: CAMetalDisplayLink.Update
  ) {
    // CAMetalDisplayLink fires on the main run loop without a Swift MainActor
    // task. Avoid capturing `@MainActor self` directly (assumeIsolated trap);
    // hop via the init-time Unmanaged identity. `MetalTerminalRenderer.present`
    // is main-queue / non-MainActor so present itself no longer Trace/BPTs.
    guard let displayLinkHopTarget else { return }
    let view = displayLinkHopTarget.takeUnretainedValue()
    let drawable = update.drawable
    seyalRunAsMainActorFromMainQueue {
      view.handleMetalDisplayLink(link, drawable: drawable)
    }
  }

  func handleMetalDisplayLink(
    _ link: CAMetalDisplayLink,
    drawable: any CAMetalDrawable
  ) {
    // The callback may already be queued when the view is detached or the
    // display link is replaced. Never let an old link present into a new
    // surface lifecycle.
    guard metalDisplayLinkLease?.link === link else { return }
    link.isPaused = true
    renderer.drainGPUCompletionsIfNeeded()

    guard shouldRender,
      hasPreparedState,
      renderer.persistentDisplayFailure == nil,
      !presentationState.exhausted,
      presentationState.consumeOpportunity()
    else {
      return
    }

    if renderer.present(drawable: drawable) {
      presentationState.recordSubmissionSuccess()
      cancelPresentationRetryTimer()
    } else {
      guard let delay = presentationState.recordSubmissionFailure() else {
        lastRenderError = MetalTerminalRendererError.presentationSubmissionFailuresExhausted
        return
      }
      schedulePresentationRetry(after: delay)
    }
  }

  func beginPresentationAttemptSeries() {
    presentationState.request()
  }

  func cancelPresentationRetryTimer() {
    presentationRetryTimer?.invalidate()
    presentationRetryTimer = nil
    presentationRetryGeneration &+= 1
    presentationRetryScheduled = false
  }

  func cancelPresentationRetries() {
    cancelPresentationRetryTimer()
    presentationState.cancel()
  }

  func cancelPresentationOpportunity() {
    cancelPresentationRetryTimer()
    presentationState.cancelPending()
  }

  func invalidatePreparedPresentation() {
    hasPreparedState = false
    cancelPresentationRetries()
    resetPreparationRecovery()
  }

  func schedulePresentationRetry(after delay: TimeInterval) {
    guard shouldRender,
      hasPreparedState,
      presentationState.pending,
      !presentationRetryScheduled,
      !presentationState.exhausted
    else {
      return
    }

    presentationRetryScheduled = true
    let generation = presentationRetryGeneration
    presentationRetryTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) {
      [weak self] _ in
      seyalRunAsMainActorFromMainQueue {
        self?.runPresentationRetry(generation: generation)
      }
    }
  }

  func runPresentationRetry(generation: UInt64) {
    guard generation == presentationRetryGeneration else { return }
    presentationRetryTimer = nil
    presentationRetryScheduled = false
    guard shouldRender, hasPreparedState, presentationState.pending else { return }
    armMetalDisplayLink()
  }

  func updateDrawableSize() {
    guard let metalLayer = layer as? CAMetalLayer else { return }
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    metalLayer.contentsScale = scale
    metalLayer.drawableSize = convertToBacking(bounds).size
  }

  func proposeCurrentGeometry() {
    guard !proposingGeometry else { return }
    guard terminalBridgeIsConnected, bounds.width > 8, bounds.height > 8 else { return }
    let rounded = bounds.integral
    guard rounded != lastProposedGeometry else { return }
    let cell = terminalPresentationCellSize()
    guard cell.width > 0, cell.height > 0 else { return }
    proposingGeometry = true
    defer { proposingGeometry = false }
    let result = terminalProposeGeometry(
      viewportWidth: Double(rounded.width),
      viewportHeight: Double(rounded.height),
      horizontalInsets: 0,
      verticalInsets: 0,
      cellWidth: Double(cell.width),
      cellHeight: Double(cell.height),
      meaningfulLayoutEpoch: true
    )
    if result == 0 {
      lastProposedGeometry = rounded
    }
  }
}

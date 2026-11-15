import AppKit

struct NativeHistoryRenderPageKey: Equatable {
  let displayHandle: UInt64
  let startLine: UInt64
  let endLine: UInt64
  let totalRows: Int
  let requestID: UInt64
  let revision: UInt64
  let rows: Range<Int>
  let cellSize: CGSize
  let backingScale: CGFloat
}

extension MetalSurfaceView {
  func requestHistoryRange(
    startLine: UInt64,
    endLine: UInt64,
    blockID: UInt64,
    startUnit: UInt32 = 0
  ) -> Int32 {
    guard let bridge else { return -10 }
    let handle = bridge.windowFocusedDisplayHandle != 0
      ? bridge.windowFocusedDisplayHandle
      : bridge.clientHandle
    return bridge.requestHistoryRange(
      forDisplayHandle: handle,
      startLine: startLine,
      endLine: endLine,
      blockID: blockID,
      startUnit: startUnit,
      unitLimit: 0
    )
  }

  func liveTailBlockIDs() -> Set<UInt64> {
    Set(renderer.liveTail.clips.keys)
  }

  /// Plan-owned transcript regions from the last successful BlockViewportPlan.
  func currentTranscriptRegions() -> [NativeTranscriptRegion] {
    renderer.activeFlowDisplayHandle != 0
      ? renderer.activeFlowRegions
      : lastTranscriptFrameRegions ?? []
  }

  func currentTranscriptRegion(blockID: UInt64) -> NativeTranscriptRegion? {
    currentTranscriptRegions().first(where: { $0.id == blockID })
  }

  func retainHistoryRange(_ range: NativeHistoryRange) {
    // Finding E/D: store the latest plan-bounded sealed clip only — never a
    // dual-key legacy retain or monotonic full-Block accumulation.
    let handle = range.sourceDisplayHandle
    let key = PaneBlockKey(
      paneID: paneID,
      displayHandle: handle,
      blockID: range.blockID
    )
    let incomingAx = range.accessibilityTranscript()
    if incomingAx.isEmpty { return }
    // Viewport pages are addressed by sealed-row index. Scrolling toward the
    // head legitimately requests a lower startLine, so page freshness follows
    // the monotonically increasing request ID rather than content position.
    if let previous = historyRanges[key], previous.requestID > range.requestID {
      return
    }
    historyRanges[key] = range
  }

  func retainedHistoryRange(blockID: UInt64, displayHandle: UInt64 = 0) -> NativeHistoryRange? {
    let exact = PaneBlockKey(paneID: paneID, displayHandle: displayHandle, blockID: blockID)
    if let range = historyRanges[exact] { return range }
    if displayHandle != 0 {
      return historyRanges[PaneBlockKey(paneID: paneID, displayHandle: 0, blockID: blockID)]
    }
    return historyRanges.first(where: {
      $0.key.paneID == paneID && $0.key.blockID == blockID
    })?.value
  }

  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    bridge?.discardHistoryRequests(except: historyExceptPreservingRawScrollback(blockIDs))
  }

  /// Installs a bounded Runtime history projection in this Pane's Metal renderer.
  func renderHistoryRange(_ range: NativeHistoryRange, region: NativeTranscriptRegion? = nil) {
    let pendingRegion = renderer.pendingFlowRegion(
      id: range.blockID,
      displayHandle: range.sourceDisplayHandle,
      requestID: range.requestID
    )
    if renderer.presentationPlan.mode == .flow {
      guard pendingRegion != nil else {
        // A late reply or disconnect cache has no matching Rust layout. Keep
        // complete cells for AX/reconnect, never invent a Metal position.
        if range.status == 0 { retainHistoryRange(range) }
        return
      }
      if range.status != 0 || range.rows.isEmpty {
        rejectInvalidFlowPage(requestID: range.requestID)
        return
      }
    }
    if renderer.shouldIgnoreFlowHistory(
      id: range.blockID,
      displayHandle: range.sourceDisplayHandle,
      requestID: range.requestID
    ) { return }
    retainHistoryRange(range)
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    let rows = pendingRegion?.historyRows ?? region?.historyRows ?? HistoryRenderWindow(
      totalRows: range.rows.count,
      firstVisibleRow: max(range.rows.count - HistoryRenderWindow.maximumRows, 0),
      visibleRowCount: HistoryRenderWindow.maximumRows
    ).rows
    let cellSize = terminalPresentationCellSize()
    let lowerBound = max(0, min(rows.lowerBound, range.rows.count))
    let upperBound = max(lowerBound, min(rows.upperBound, range.rows.count))
    let boundedRows = lowerBound..<upperBound
    let page = NativeHistoryRange(
      startLine: range.startLine,
      endLine: range.endLine,
      blockID: range.blockID,
      requestID: range.requestID,
      revision: range.revision,
      rows: Array(range.rows[boundedRows]),
      status: range.status,
      sourceDisplayHandle: range.sourceDisplayHandle
    )
    let key = NativeHistoryRenderPageKey(
      displayHandle: range.sourceDisplayHandle,
      startLine: range.startLine,
      endLine: range.endLine,
      totalRows: range.rows.count,
      requestID: range.requestID,
      revision: range.revision,
      rows: boundedRows,
      cellSize: cellSize,
      backingScale: scale
    )
    do {
      let currentRegion = pendingRegion ?? region ?? lastTranscriptFrameRegions?.first(where: {
        $0.id == range.blockID
          && ($0.displayHandle == 0 || $0.displayHandle == range.sourceDisplayHandle)
      })
      let rendererRegion: NativeTranscriptRegion
      if let currentRegion {
        // AppKit uses a bottom-left origin; the terminal shader uses top-left pixels.
        let pixelClip = NSRect(
          x: currentRegion.clip.minX * scale,
          y: (bounds.height - currentRegion.clip.maxY) * scale,
          width: currentRegion.clip.width * scale,
          height: currentRegion.clip.height * scale
        )
        let pageOffset = CGFloat(boundedRows.lowerBound)
          * cellSize.height * scale
        let pixelOrigin = NSPoint(x: pixelClip.minX, y: pixelClip.minY + pageOffset)
        rendererRegion = NativeTranscriptRegion(
          id: currentRegion.id,
          origin: pixelOrigin,
          clip: pixelClip,
          historyRows: boundedRows,
          historyRequestID: range.requestID,
          historyRevision: range.revision
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
      if pendingRegion != nil {
        guard renderer.stageFlowHistory(page, region: rendererRegion, backingScale: scale) else {
          rejectInvalidFlowPage(requestID: range.requestID)
          return
        }
        if shouldRender || hasRetainedHistoryPresentation {
          beginPresentationAttemptSeries()
          armMetalDisplayLink()
        }
        return
      }
      if historyRenderPageKeys[range.blockID] == key,
         renderer.setHistoryRegionGeometry(
           id: range.blockID,
           origin: rendererRegion.origin,
           clip: rendererRegion.clip
         )
      {
        // Geometry-only scrolls reuse the prepared page; content/page changes
        // below prepare a fresh bounded buffer even when the clip is unchanged.
      } else {
        // Defers while a drawable is in flight; GPU completion applies it and
        // requests a fresh frame. Never zero framesInFlight on this path (#1484).
        try renderer.update(historyRange: page, region: rendererRegion, backingScale: scale)
        historyRenderPageKeys[range.blockID] = key
      }
      // Direct prepare (applyHistoryRange) must register order — inspectFlowPaint
      // only counts historyRegionOrder entries (#1484 clear→exit false-green).
      if !renderer.historyRegionOrder.contains(range.blockID) {
        renderer.setHistoryRegionOrder(renderer.historyRegionOrder + [range.blockID])
      }
      // History-only Flow has no Candidate-D live frame and cannot mark the renderer Usable.
      hasRetainedHistoryPresentation = true
      if shouldRender || hasRetainedHistoryPresentation {
        renderer.requestPresent()
        beginPresentationAttemptSeries()
        armMetalDisplayLink()
      }
      refreshRecoveryAccessibilityValue()
    } catch {
      historyRenderPageKeys.removeValue(forKey: range.blockID)
      lastRenderError = error
    }
  }

  private func rejectInvalidFlowPage(requestID: UInt64) {
    let shouldRetry = renderer.rejectPendingFlowFrame(requestID: requestID)
    lastAppliedViewportPlanSignature = nil
    guard shouldRetry else {
      lastRenderError = MetalTerminalRendererError.invalidFrame
      return
    }
    // One event-driven retry asks Rust for a fresh plan; no timer or poll loop.
    DispatchQueue.main.async { [weak self] in
      self?.renderer.onNeedsCurrentFrame?()
    }
  }
}

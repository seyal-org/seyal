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
  func requestHistoryRange(startLine: UInt64, endLine: UInt64, blockID: UInt64) -> Int32 {
    bridge?.requestHistoryRange(startLine: startLine, endLine: endLine, blockID: blockID) ?? -10
  }

  func retainHistoryRange(_ range: NativeHistoryRange) {
    historyRanges[
      PaneBlockKey(
        paneID: paneID,
        displayHandle: range.sourceDisplayHandle,
        blockID: range.blockID
      )
    ] = range
  }

  func retainedHistoryRange(blockID: UInt64, displayHandle: UInt64 = 0) -> NativeHistoryRange? {
    historyRanges[PaneBlockKey(paneID: paneID, displayHandle: displayHandle, blockID: blockID)]
  }

  func discardHistoryRequests(except blockIDs: Set<UInt64>) {
    bridge?.discardHistoryRequests(except: historyExceptPreservingRawScrollback(blockIDs))
  }

  /// Installs a bounded Runtime history projection in this Pane's Metal renderer.
  func renderHistoryRange(_ range: NativeHistoryRange, region: NativeTranscriptRegion? = nil) {
    retainHistoryRange(range)
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    let rows = region?.historyRows ?? HistoryRenderWindow(
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
      let currentRegion = region ?? lastTranscriptFrameRegions?.first(where: {
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
        try renderer.update(historyRange: page, region: rendererRegion, backingScale: scale)
        historyRenderPageKeys[range.blockID] = key
      }
      // History-only Flow has no Candidate-D live frame and cannot mark the renderer Usable.
      hasRetainedHistoryPresentation = true
      if shouldRender {
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
}

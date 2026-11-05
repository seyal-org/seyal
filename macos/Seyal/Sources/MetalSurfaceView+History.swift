import AppKit

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
    do {
      let rendererRegion: NativeTranscriptRegion
      if let region {
        // AppKit uses a bottom-left origin; the terminal shader uses top-left pixels.
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
      // History-only Flow has no Candidate-D live frame and cannot mark the renderer Usable.
      hasRetainedHistoryPresentation = true
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
}

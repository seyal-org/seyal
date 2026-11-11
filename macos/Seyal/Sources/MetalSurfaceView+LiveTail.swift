import AppKit
import Metal

extension MetalSurfaceView {
  func setTranscriptFrame(_ frame: NativeTranscriptFrame) {
    guard frame.isValid,
      frame.surfaceIdentity == nil || frame.surfaceIdentity == ObjectIdentifier(self)
    else {
      return
    }
    let displayHandles = Set(frame.regions.map(\.displayHandle).filter { $0 != 0 })
    guard displayHandles.count <= 1 else { return }
    if let displayHandle = displayHandles.first, displayHandle != transcriptDisplayHandle {
      return
    }
    // #1379: a transient empty region publish (zero-height Block bodies during
    // document-size sync) must not filter away retained history paint and leave
    // Flow titles with white bodies. Intentional clears use
    // `removeTranscriptRegions(except:)`.
    if frame.regions.isEmpty, !historyRanges.isEmpty {
      return
    }
    // History replies can arrive after the first geometry publish. Allow an
    // identical frame through until each retained range has a renderer region;
    // otherwise the Block keeps AX text but its Metal body stays blank.
    let hasUnpreparedHistory = frame.regions.contains { region in
      let key = PaneBlockKey(
        paneID: paneID,
        displayHandle: region.displayHandle,
        blockID: region.id
      )
      return historyRanges[key] != nil && renderer.historyRegions[region.id] == nil
    }
    guard lastTranscriptFrameRegions != frame.regions || hasUnpreparedHistory else { return }
    lastTranscriptFrameRegions = frame.regions
    let regionIDs = Set(frame.regionIDs)
    // Do not drop `historyRanges` here when a body clip is briefly omitted —
    // that erased AX transcript + Metal source data after clear→exit while
    // cards still existed. Lifetime stays with removeTranscriptRegions /
    // disconnect retain policy.
    renderer.removeHistoryRegions(except: regionIDs)
    renderer.setHistoryRegionOrder(frame.regionIDs)
    let pixelRegions = liveTailPixelRegions(in: frame)
    renderer.setTranscriptRegions(pixelRegions)
    // History pages are bounded per Pane viewport. Geometry-only movement
    // reuses a page, while a page/revision change rebuilds its GPU instances.
    for region in frame.regions {
      guard
        let range = historyRanges[
          PaneBlockKey(
            paneID: paneID,
            displayHandle: region.displayHandle,
            blockID: region.id
          )
        ]
      else {
        continue
      }
      renderHistoryRange(range, region: region)
    }
  }

  /// Running Flow Blocks: clip the prepared primary frame into Block regions.
  func setLiveTailBlocks(_ clipsByBlock: [UInt64: LiveTailClip]) {
    renderer.setLiveTailBlocks(clipsByBlock)
  }

  var lastPreparedRowCount: Int {
    renderer.lastPreparedRowCount
  }

  func clearLiveTailWhenTranscriptEmpty(_ ids: Set<UInt64>) {
    guard ids.isEmpty else { return }
    renderer.clearLiveTailPresentation()
  }

  /// End running primary clips without touching retained history regions.
  func clearLiveTailPresentation() {
    renderer.clearLiveTailPresentation()
  }

  /// AppKit bottom-left → terminal top-left pixel clips for live-tail.
  private func liveTailPixelRegions(in frame: NativeTranscriptFrame) -> [NativeTranscriptRegion] {
    let scale = window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1
    return frame.regions.map { region in
      let pixelClip = NSRect(
        x: region.clip.minX * scale,
        y: (bounds.height - region.clip.maxY) * scale,
        width: region.clip.width * scale,
        height: region.clip.height * scale
      )
      return NativeTranscriptRegion(
        id: region.id,
        displayHandle: region.displayHandle,
        origin: pixelClip.origin,
        clip: pixelClip,
        suppressesCursor: region.suppressesCursor,
        historyRows: region.historyRows,
        historyRequestID: region.historyRequestID,
        historyRevision: region.historyRevision
      )
    }
  }
}

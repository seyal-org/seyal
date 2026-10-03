import AppKit
import Metal

extension MetalSurfaceView {
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
    renderer.setTranscriptRegions(liveTailPixelRegions(in: frame))
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

  /// Running Flow Blocks: clip the prepared primary frame into Block regions.
  func setLiveTailBlocks(_ clipsByBlock: [UInt64: LiveTailClip]) {
    renderer.setLiveTailBlocks(clipsByBlock)
  }

  var lastPreparedRowCount: Int {
    renderer.lastPreparedRowCount
  }

  func clearLiveTailWhenTranscriptEmpty(_ ids: Set<UInt64>) {
    guard ids.isEmpty else { return }
    renderer.setLiveTailBlocks([:])
    renderer.setTranscriptRegions([])
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
      return NativeTranscriptRegion(id: region.id, origin: pixelClip.origin, clip: pixelClip)
    }
  }
}

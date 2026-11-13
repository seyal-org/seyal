import AppKit
import Foundation

extension MetalSurfaceView {
  /// Consume the Rust-owned `BlockViewportPlan` for this display (Finding C).
  /// Builds transcript regions from plan geometry — not AppKit card clips —
  /// and requests sealed-index HISTORY pages for each clipping range (Finding E).
  @discardableResult
  func applyBlockViewportPlan(displayHandle: UInt64) -> Bool {
    guard displayHandle != 0 else { return false }
    var header = SeyalBlockViewportPlanHeader()
    guard seyal_bridge_block_viewport_plan_header_for(displayHandle, &header) != 0,
          header.visible_count > 0
    else {
      return false
    }

    let cell = terminalPresentationCellSize()
    let surfaceHeight = bounds.height
    var regions: [NativeTranscriptRegion] = []
    regions.reserveCapacity(Int(header.visible_count))

    for index in 0..<header.visible_count {
      var item = SeyalBlockViewportVisibleItem()
      guard seyal_bridge_block_viewport_visible_at_for(displayHandle, index, &item) != 0,
            item.block_id != 0,
            item.visible_rows > 0
      else { continue }

      // Viewport-relative row = absolute document row − plan scroll_y.
      let docFirst = item.y.saturatingAdding(UInt64(item.first_visible_row))
      let viewportRow = docFirst >= header.scroll_y
        ? docFirst - header.scroll_y
        : 0
      let heightPoints = CGFloat(item.visible_rows) * cell.height
      let originYPoints = surfaceHeight
        - (CGFloat(viewportRow) * cell.height)
        - heightPoints
      let clip = NSRect(
        x: 0,
        y: originYPoints,
        width: bounds.width,
        height: heightPoints
      )
      guard clip.width > 0, clip.height > 0 else { continue }

      var region = NativeTranscriptRegion(
        id: item.block_id,
        origin: .zero,
        clip: clip,
        historyRows: 0..<Int(item.visible_rows),
        historyRequestID: 0,
        historyRevision: header.presentation_epoch
      )
      region.displayHandle = displayHandle
      regions.append(region)
    }

    guard !regions.isEmpty else { return false }
    let frame = NativeTranscriptFrame(
      revision: header.presentation_epoch,
      regions: regions,
      surfaceIdentity: ObjectIdentifier(self)
    )
    setTranscriptFrame(frame)

    // Bounded sealed retrieval per clipping range — not full-Block HISTORY.
    for index in 0..<header.clipping_count {
      var clip = SeyalBlockViewportClip()
      guard seyal_bridge_block_viewport_clip_at_for(displayHandle, index, &clip) != 0,
            clip.block_id != 0,
            clip.row_count > 0
      else { continue }
      _ = requestHistoryRange(
        startLine: 0,
        endLine: 0,
        blockID: clip.block_id,
        startUnit: clip.first_row
      )
    }

    return true
  }
}

private extension UInt64 {
  func saturatingAdding(_ other: UInt64) -> UInt64 {
    self.addingReportingOverflow(other).partialValue
  }
}

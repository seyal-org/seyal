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
    // Address the plan's displayHandle directly (not window-focused), so
    // multipane Flow clips cannot hit the wrong Controller (#1426 / Finding E).
    // Running LiveTail Blocks get one AX seed page only (paint stays LiveTail);
    // completed clips use the plan window with dedup so publish retries cannot
    // flood HISTORY during high-volume output (#1484 100k / clear→exit AX).
    for index in 0..<header.clipping_count {
      var clip = SeyalBlockViewportClip()
      guard seyal_bridge_block_viewport_clip_at_for(displayHandle, index, &clip) != 0,
            clip.block_id != 0,
            clip.row_count > 0
      else { continue }
      let liveTailPainted = renderer.liveTail.clips[clip.block_id] != nil
      let key: HistoryProjectionRequestKey
      let startUnit: UInt32
      if liveTailPainted {
        // Stable key: one seed for the whole Running lifetime.
        key = HistoryProjectionRequestKey(
          displayHandle: displayHandle,
          blockID: clip.block_id,
          startLine: 0,
          endLine: 0
        )
        startUnit = 0
      } else {
        key = HistoryProjectionRequestKey(
          displayHandle: displayHandle,
          blockID: clip.block_id,
          startLine: UInt64(clip.first_row),
          endLine: UInt64(clip.first_row).saturatingAdding(UInt64(clip.row_count))
        )
        startUnit = clip.first_row
      }
      guard planClipRequestBook.begin(key) else { continue }
      let result = bridge?.requestHistoryRange(
        forDisplayHandle: displayHandle,
        startLine: 0,
        endLine: 0,
        blockID: clip.block_id,
        startUnit: startUnit
      ) ?? -10
      if result != 0 {
        planClipRequestBook.retry(key)
      }
    }

    return true
  }

  /// Settle plan-owned clip requests after a HISTORY page arrives (or fails).
  func notePlanClipHistoryResult(_ range: NativeHistoryRange) {
    let handle = range.sourceDisplayHandle != 0
      ? range.sourceDisplayHandle
      : transcriptDisplayHandle
    let key = HistoryProjectionRequestKey(
      displayHandle: handle,
      blockID: range.blockID,
      startLine: 0,
      endLine: 0
    )
    planClipRequestBook.receive(
      key,
      status: range.status,
      deliveredRows: range.rows.count,
      minimumRows: range.rows.isEmpty ? 1 : nil
    )
  }
}

private extension UInt64 {
  func saturatingAdding(_ other: UInt64) -> UInt64 {
    self.addingReportingOverflow(other).partialValue
  }
}

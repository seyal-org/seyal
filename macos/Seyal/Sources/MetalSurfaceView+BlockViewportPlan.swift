import AppKit
import Foundation

extension MetalSurfaceView {
  /// Consume the Rust-owned `BlockViewportPlan` for this display (Finding C).
  /// Plan owns Block identity + visible row windows + sealed retrieval (E).
  /// Optional `screenClips` are AppKit body rects in this surface's coordinates
  /// (ADR-015 OS adapter) so Metal glyphs land under transparent card bodies —
  /// not a second content authority (#1484 clear→exit ink).
  @discardableResult
  func applyBlockViewportPlan(
    displayHandle: UInt64,
    screenClips: [UInt64: NSRect] = [:],
    /// Pane hosts intentionally clear `bridge` when unfocused; they still must
    /// fetch sealed HISTORY through the window-borrowed bridge (#1484 / #1426).
    historyBridge: RustDisplayBridge? = nil
  ) -> Bool {
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

      let clip: NSRect
      if let screen = screenClips[item.block_id], screen.width > 1, screen.height > 1 {
        // Thin adapter: map plan-selected Block onto the laid-out body hit target.
        clip = screen
      } else {
        // Viewport-relative row = absolute document row − plan scroll_y.
        let docFirst = item.y.saturatingAdding(UInt64(item.first_visible_row))
        let viewportRow = docFirst >= header.scroll_y
          ? docFirst - header.scroll_y
          : 0
        let heightPoints = CGFloat(item.visible_rows) * cell.height
        let originYPoints = surfaceHeight
          - (CGFloat(viewportRow) * cell.height)
          - heightPoints
        clip = NSRect(
          x: 0,
          y: originYPoints,
          width: bounds.width,
          height: heightPoints
        )
      }
      guard clip.width > 0, clip.height > 0 else { continue }

      // historyRows indexes the retained page buffer (often already windowed
      // by sealed-index start_unit), not absolute Block rows.
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
    // LiveTail still paints Running Blocks, but AX needs a plan-window HISTORY
    // page (Metal glyphs are not AX). Always pass unitLimit so Truncate cannot
    // walk the whole Block (#1484 100k). Completed clips use the same window.
    for index in 0..<header.clipping_count {
      var clip = SeyalBlockViewportClip()
      guard seyal_bridge_block_viewport_clip_at_for(displayHandle, index, &clip) != 0,
            clip.block_id != 0,
            clip.row_count > 0
      else { continue }
      let key = HistoryProjectionRequestKey(
        displayHandle: displayHandle,
        blockID: clip.block_id,
        startLine: UInt64(clip.first_row),
        endLine: UInt64(clip.first_row).saturatingAdding(UInt64(clip.row_count))
      )
      if !planClipRequestBook.begin(key) {
        // Never reopen while a request is in flight — that floods HISTORY and
        // trips ClientError backpressure (-3), dropping the AX page (#1484).
        // Only reopen a settled empty retain after LiveTail has cleared.
        if renderer.liveTail.clips[clip.block_id] != nil
          || planClipRequestBook.hasInFlight(
            displayHandle: displayHandle,
            blockID: clip.block_id
          )
        {
          continue
        }
        guard planClipRequestBook.hasSettled(
          displayHandle: displayHandle,
          blockID: clip.block_id
        ) else { continue }
        let retained = retainedHistoryRange(
          blockID: clip.block_id,
          displayHandle: displayHandle
        )
        let axEmpty = retained?.accessibilityTranscript().isEmpty ?? true
        guard axEmpty else { continue }
        planClipRequestBook.retry(key)
        guard planClipRequestBook.begin(key) else { continue }
      }
      let transport = bridge ?? historyBridge
      let result = transport?.requestHistoryRange(
        forDisplayHandle: displayHandle,
        startLine: 0,
        endLine: 0,
        blockID: clip.block_id,
        startUnit: clip.first_row,
        unitLimit: clip.row_count
      ) ?? -10
      if result != 0 {
        planClipRequestBook.retry(key)
      }
    }

    return true
  }

  /// When BlockViewportPlan has no visible clips (native-regions lag / empty
  /// index) but Flow already hosts Block cards, fetch one bounded sealed
  /// LiveEnd page per Block for AX/Metal retain. Plan remains paint authority
  /// once extent is non-zero; this only covers the multipane gap (#1484 FocusChurn).
  func requestNativeCardHistoryFallback(
    displayHandle: UInt64,
    blocks: [(blockID: UInt64, sealedRows: UInt32)],
    historyBridge: RustDisplayBridge? = nil
  ) {
    guard displayHandle != 0, let transport = bridge ?? historyBridge else { return }
    for block in blocks {
      guard block.blockID != 0, block.sealedRows > 0 else { continue }
      // LiveTail may still paint; AX still needs a bounded sealed page.
      let unitLimit = min(block.sealedRows, 128)
      let first = block.sealedRows > unitLimit ? block.sealedRows &- unitLimit : 0
      let key = HistoryProjectionRequestKey(
        displayHandle: displayHandle,
        blockID: block.blockID,
        startLine: UInt64(first),
        endLine: UInt64(first).saturatingAdding(UInt64(unitLimit))
      )
      if !planClipRequestBook.begin(key) {
        if planClipRequestBook.hasInFlight(
          displayHandle: displayHandle,
          blockID: block.blockID
        ) {
          continue
        }
        let retained = retainedHistoryRange(
          blockID: block.blockID,
          displayHandle: displayHandle
        )
        let axEmpty = retained?.accessibilityTranscript().isEmpty ?? true
        guard axEmpty,
              planClipRequestBook.hasSettled(
                displayHandle: displayHandle,
                blockID: block.blockID
              )
        else { continue }
        planClipRequestBook.retry(key)
        guard planClipRequestBook.begin(key) else { continue }
      }
      let result = transport.requestHistoryRange(
        forDisplayHandle: displayHandle,
        startLine: 0,
        endLine: 0,
        blockID: block.blockID,
        startUnit: first,
        unitLimit: unitLimit
      )
      if result != 0 {
        planClipRequestBook.retry(key)
      }
    }
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
    // Blank cell pages (or all-padding rows) must not settle — AX stays empty
    // and tall LiveEnd markers never appear (#1484 FocusChurn).
    let axEmpty = range.accessibilityTranscript().isEmpty
    planClipRequestBook.receive(
      key,
      status: range.status,
      deliveredRows: axEmpty ? 0 : range.rows.count,
      minimumRows: axEmpty ? 1 : nil
    )
  }
}

private extension UInt64 {
  func saturatingAdding(_ other: UInt64) -> UInt64 {
    self.addingReportingOverflow(other).partialValue
  }
}

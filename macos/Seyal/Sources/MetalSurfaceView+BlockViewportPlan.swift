import AppKit
import Foundation

extension MetalSurfaceView {
  /// Consume the Rust-owned `BlockViewportPlan` for this display (Finding C / ADR-015).
  /// Plan owns Block identity, visible row windows, and sealed retrieval (E).
  /// Optional `chromeBodyClips` only **place** plan-selected Blocks onto AppKit
  /// chrome body rects (thin adapter). They never invent Blocks, rows, or HISTORY.
  /// When clips are absent, plan row geometry is bottom-aligned into the surface
  /// so short transcripts match Warp-style docked cards (#1484 manual).
  @discardableResult
  func applyBlockViewportPlan(
    displayHandle: UInt64,
    /// Plan-selected Block → chrome body rect in surface coordinates.
    chromeBodyClips: [UInt64: NSRect] = [:],
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
    // Quantize chrome body placement so taller/shorter card bodies after
    // setOutputLines rebuild plan→body clips (diagnosis residual: header≥22+4).
    var chromeClipFingerprint: UInt64 = 0
    for (blockID, rect) in chromeBodyClips.sorted(by: { $0.key < $1.key }) {
      let qx = UInt64(max(0, rect.origin.x).rounded(.down))
      let qy = UInt64(max(0, rect.origin.y).rounded(.down))
      let qw = UInt64(max(0, rect.width).rounded(.down))
      let qh = UInt64(max(0, rect.height).rounded(.down))
      chromeClipFingerprint = chromeClipFingerprint
        &+ (blockID &* 1_000_003)
        &+ (qx &* 17)
        &+ (qy &* 31)
        &+ (qw &* 47)
        &+ (qh &* 61)
    }
    let signature = (
      handle: displayHandle,
      epoch: header.presentation_epoch,
      scrollY: header.scroll_y,
      visible: header.visible_count,
      clipping: header.clipping_count,
      totalExtent: header.total_document_extent,
      clipCount: UInt32(chromeBodyClips.count),
      chromeClipFingerprint: chromeClipFingerprint
    )
    // Identical geometry: skip region rebuild + HISTORY reopen. Do NOT key on
    // content_generation — type-48 bumps native revision every tick even when
    // height/identity are unchanged; that was the remaining echo lag (#1484).
    // Height-only growth is covered by totalExtent; seal by presentation_epoch.
    // LiveTail cells still advance via damage prepare.
    if lastAppliedViewportPlanSignature?.handle == signature.handle,
       lastAppliedViewportPlanSignature?.epoch == signature.epoch,
       lastAppliedViewportPlanSignature?.scrollY == signature.scrollY,
       lastAppliedViewportPlanSignature?.visible == signature.visible,
       lastAppliedViewportPlanSignature?.clipping == signature.clipping,
       lastAppliedViewportPlanSignature?.totalExtent == signature.totalExtent,
       lastAppliedViewportPlanSignature?.clipCount == signature.clipCount,
       lastAppliedViewportPlanSignature?.chromeClipFingerprint == signature.chromeClipFingerprint,
       let published = lastTranscriptFrameRegions,
       !published.isEmpty
    {
      return true
    }

    let cell = terminalPresentationCellSize()
    let surfaceHeight = bounds.height
    // Warp bottom-dock: when terminal content is shorter than the surface,
    // shift plan geometry up from the bottom so glyphs sit with docked cards
    // instead of at the top of the pane (#1484 manual).
    let contentHeightPoints = CGFloat(header.total_document_extent) * cell.height
    let bottomAlignOffset = max(surfaceHeight - contentHeightPoints, 0)
    var regions: [NativeTranscriptRegion] = []
    regions.reserveCapacity(Int(header.visible_count))

    for index in 0..<header.visible_count {
      var item = SeyalBlockViewportVisibleItem()
      guard seyal_bridge_block_viewport_visible_at_for(displayHandle, index, &item) != 0,
            item.block_id != 0,
            item.visible_rows > 0
      else { continue }

      let clip: NSRect
      if let chrome = chromeBodyClips[item.block_id],
         chrome.width > 1,
         chrome.height > 1
      {
        // Thin adapter: place plan-selected content into chrome body.
        clip = chrome
      } else {
        // Viewport-relative row = absolute document row − plan scroll_y.
        let docFirst = item.y.saturatingAdding(UInt64(item.first_visible_row))
        let viewportRow = docFirst >= header.scroll_y
          ? docFirst - header.scroll_y
          : 0
        let heightPoints = CGFloat(item.visible_rows) * cell.height
        // AppKit y grows up; viewportRow 0 is the top of the visible window.
        // Subtract bottomAlignOffset so short content docks to the composer.
        let originYPoints = surfaceHeight
          - bottomAlignOffset
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
      // historyRevision stays 0 on plan geometry: presentation_epoch must not
      // defeat LiveTail region equality during seq streaming (#1484 P0 flicker).
      var region = NativeTranscriptRegion(
        id: item.block_id,
        origin: .zero,
        clip: clip,
        historyRows: 0..<Int(item.visible_rows),
        historyRequestID: 0,
        historyRevision: 0
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

    // Bounded sealed retrieval per plan clipping range — not full-Block HISTORY
    // and not card-driven fallback (#1484 Finding E / ADR-015).
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
        // Defer while LiveTail owns running paint; reopen only empty retains.
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

    lastAppliedViewportPlanSignature = signature
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
    addingReportingOverflow(other).partialValue
  }
}

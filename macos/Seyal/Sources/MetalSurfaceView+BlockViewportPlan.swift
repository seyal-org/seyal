import AppKit
import Foundation

extension MetalSurfaceView {
  /// Consume the Rust-owned `BlockViewportPlan` for this display (Finding C / ADR-015).
  /// Plan owns Block identity, complete content heights, visible row windows,
  /// and output clips. AppKit card geometry never participates in Metal placement.
  @discardableResult
  func applyBlockViewportPlan(
    displayHandle: UInt64,
    /// Pane hosts intentionally clear `bridge` when unfocused; they still must
    /// fetch sealed HISTORY through the window-borrowed bridge (#1484 / #1426).
    historyBridge: RustDisplayBridge? = nil
  ) -> Bool {
    guard displayHandle != 0 else { return false }
    var header = SeyalBlockViewportPlanHeader()
    guard seyal_bridge_block_viewport_plan_header_for(displayHandle, &header) != 0 else {
      return false
    }
    if header.visible_count == 0 {
      if renderer.activeFlowDisplayHandle == displayHandle,
         renderer.activeFlowRegions.isEmpty,
         renderer.pendingFlowFrame == nil,
         renderer.blockSurfaceDecorations.isEmpty,
         renderer.liveTail.order.isEmpty {
        return true
      }
      renderer.stageFlowFrame(
        displayHandle: displayHandle,
        presentationEpoch: header.presentation_epoch,
        requestID: 0,
        regions: [],
        decorations: [],
        liveClips: [:],
        liveRegions: []
      )
      lastAppliedViewportPlanSignature = nil
      return true
    }
    if renderer.flowRetryDisplayHandle == displayHandle,
       renderer.flowRetryEpoch == header.presentation_epoch,
       renderer.flowInvalidPageRetries >= 2 {
      return false
    }
    var planHasher = Hasher()
    for index in 0..<header.visible_count {
      var item = SeyalBlockViewportVisibleItem()
      guard seyal_bridge_block_viewport_visible_at_for(displayHandle, index, &item) != 0 else {
        continue
      }
      planHasher.combine(item.block_id)
      planHasher.combine(item.y)
      planHasher.combine(item.height)
      planHasher.combine(item.first_visible_row)
      planHasher.combine(item.visible_rows)
      planHasher.combine(item.sealed_rows)
      planHasher.combine(item.header_rows)
      planHasher.combine(item.live_first_row)
      planHasher.combine(item.live_row_count)
    }
    let scale = max(window?.backingScaleFactor ?? NSScreen.main?.backingScaleFactor ?? 1, 1)
    let cell = terminalPresentationCellSize()
    let viewportSize = bounds.size
    let signature = (
      handle: displayHandle,
      epoch: header.presentation_epoch,
      geometry: planHasher.finalize(),
      scrollY: header.scroll_y,
      visible: header.visible_count,
      clipping: header.clipping_count,
      totalExtent: header.total_document_extent,
      viewportSize: viewportSize,
      cellSize: cell,
      backingScale: scale,
      canvasColor: renderer.defaultTerminalColors.y,
      resourceGeneration: renderer.presentationResourceGeneration
    )
    // Identical geometry: skip region rebuild + HISTORY reopen. Do NOT key on
    // content_generation — type-48 bumps native revision every tick even when
    // height/identity are unchanged; that was the remaining echo lag (#1484).
    // Height-only growth is covered by totalExtent; seal by presentation_epoch.
    // LiveTail cells still advance via damage prepare.
    if lastAppliedViewportPlanSignature?.handle == signature.handle,
       lastAppliedViewportPlanSignature?.epoch == signature.epoch,
       lastAppliedViewportPlanSignature?.geometry == signature.geometry,
       lastAppliedViewportPlanSignature?.scrollY == signature.scrollY,
       lastAppliedViewportPlanSignature?.visible == signature.visible,
       lastAppliedViewportPlanSignature?.clipping == signature.clipping,
       lastAppliedViewportPlanSignature?.totalExtent == signature.totalExtent,
       lastAppliedViewportPlanSignature?.viewportSize == signature.viewportSize,
       lastAppliedViewportPlanSignature?.cellSize == signature.cellSize,
       lastAppliedViewportPlanSignature?.backingScale == signature.backingScale,
       lastAppliedViewportPlanSignature?.canvasColor == signature.canvasColor,
       lastAppliedViewportPlanSignature?.resourceGeneration == signature.resourceGeneration,
       (renderer.activeFlowDisplayHandle == displayHandle || renderer.pendingFlowFrame != nil)
    {
      return true
    }

    let surfaceHeight = bounds.height
    // Warp bottom-dock: when terminal content is shorter than the surface,
    // shift plan geometry up from the bottom so glyphs sit with docked cards
    // instead of at the top of the pane (#1484 manual).
    let bottomAlignOffset = CGFloat(header.alignment_padding_rows) * cell.height
    var clips: [SeyalBlockViewportClip] = []
    clips.reserveCapacity(Int(header.clipping_count))
    for index in 0..<header.clipping_count {
      var clip = SeyalBlockViewportClip()
      if seyal_bridge_block_viewport_clip_at_for(displayHandle, index, &clip) != 0,
         clip.region_id != 0, clip.row_count > 0
      {
        clips.append(clip)
      }
    }

    let transport = bridge ?? historyBridge
    let requestID = transport?.requestBlockViewportContent(
      forDisplayHandle: displayHandle,
      planGeneration: header.content_generation,
      clips: clips.map { (regionID: $0.region_id, firstRow: $0.first_row, rowCount: $0.row_count) }
    ) ?? 0
    var regions: [NativeTranscriptRegion] = []
    regions.reserveCapacity(clips.count)
    var liveRegions: [NativeTranscriptRegion] = []
    liveRegions.reserveCapacity(Int(header.visible_count))
    for contentClip in clips {
      // Keep the signed offset for the bounded overscan rows. Clamping an
      // above-viewport RegionGrid page to row zero stacked adjacent Blocks on
      // one clip and produced overlapping bands during scroll (#1484).
      let viewportRow = CGFloat(contentClip.presentation_y) - CGFloat(header.scroll_y)
      let heightPoints = CGFloat(contentClip.row_count) * cell.height
      let originYPoints = surfaceHeight
        - bottomAlignOffset
        - (viewportRow * cell.height)
        - heightPoints
      let clip = NSRect(
        x: 0,
        y: originYPoints,
        width: bounds.width,
        height: heightPoints
      )
      guard clip.width > 0, clip.height > 0 else { continue }
      var region = NativeTranscriptRegion(
        id: contentClip.region_id,
        origin: .zero,
        clip: clip,
        historyRows: 0..<Int(contentClip.row_count),
        historyRequestID: requestID,
        historyRevision: 0
      )
      region.displayHandle = displayHandle
      regions.append(region)
    }

    // Running rows remain on the prepared-frame damage path. Rust supplies the
    // Block-local placement and the exact primary-frame row slice; Swift only
    // converts that plan geometry to AppKit points for the Metal adapter.
    var liveClips: [UInt64: LiveTailClip] = [:]
    for index in 0..<header.visible_count {
      var item = SeyalBlockViewportVisibleItem()
      guard seyal_bridge_block_viewport_visible_at_for(displayHandle, index, &item) != 0,
            item.block_id != 0, item.live_row_count > 0
      else { continue }
      // Rust has already clipped this row count to the Block's output extent.
      let liveRows = item.live_row_count
      guard liveRows > 0 else { continue }
      guard item.output_region != 0 else { continue }
      liveClips[item.output_region] = LiveTailClip(
        firstRow: item.live_first_row,
        rowCount: liveRows
      )
      let liveDocumentRow = item.y
        &+ UInt64(item.header_rows)
        &+ UInt64(item.sealed_rows)
      let viewportRow = CGFloat(liveDocumentRow) - CGFloat(header.scroll_y)
      let heightPoints = CGFloat(liveRows) * cell.height
      let originYPoints = surfaceHeight
        - bottomAlignOffset
        - (viewportRow * cell.height)
        - heightPoints
      var region = NativeTranscriptRegion(
        id: item.output_region,
        origin: .zero,
        clip: NSRect(x: 0, y: originYPoints, width: bounds.width, height: heightPoints)
      )
      region.displayHandle = displayHandle
      if region.clip.width > 0, region.clip.height > 0 {
        liveRegions.append(region)
      }
    }
    let pixelLiveRegions = liveTailPixelRegions(in: NativeTranscriptFrame(
      revision: header.presentation_epoch,
      regions: liveRegions,
      surfaceIdentity: ObjectIdentifier(self)
    ))

    let cellPixelHeight = max(cell.height * scale, 1)
    let surfacePixelWidth = max(bounds.width * scale, 1)
    let canvas = signature.canvasColor
    let red = Double(canvas & 0xff) / 255
    let green = Double((canvas >> 8) & 0xff) / 255
    let blue = Double((canvas >> 16) & 0xff) / 255
    let lightCanvas = red * 0.2126 + green * 0.7152 + blue * 0.0722 > 0.5
    let headerColor = lightCanvas
      ? packRGBA(red: 232, green: 235, blue: 239)
      : packRGBA(red: 38, green: 40, blue: 45)
    let cardColor = lightCanvas
      ? packRGBA(red: 242, green: 244, blue: 247)
      : packRGBA(red: 31, green: 33, blue: 37)
    var decorations: [MetalBlockSurfaceDecoration] = []
    decorations.reserveCapacity(Int(header.surface_count))
    for index in 0..<header.surface_count {
      var surface = SeyalBlockViewportSurface()
      guard seyal_bridge_block_viewport_surface_at_for(displayHandle, index, &surface) != 0,
        surface.block_id != 0, surface.row_count > 0
      else { continue }
      let documentRow = surface.presentation_y
      let viewportRow = documentRow >= header.scroll_y
        ? documentRow - header.scroll_y
        : 0
      let topRows = UInt64(header.alignment_padding_rows) &+ viewportRow
      let color = surface.role == 1 ? headerColor : cardColor
      decorations.append(MetalBlockSurfaceDecoration(
        rect: CGRect(
          x: 0,
          y: CGFloat(topRows) * cellPixelHeight,
          width: surfacePixelWidth,
          height: CGFloat(surface.row_count) * cellPixelHeight
        ),
        color: color
      ))
    }
    // Do not publish any part of this generation yet. HISTORY arrives through
    // the bounded request above; the renderer retains the previous complete
    // frame until every requested clip and the live prepared frame are ready.
    guard regions.isEmpty || requestID != 0 else { return false }
    renderer.stageFlowFrame(
      displayHandle: displayHandle,
      presentationEpoch: header.presentation_epoch,
      requestID: requestID,
      regions: regions,
      decorations: decorations,
      liveClips: liveClips,
      liveRegions: pixelLiveRegions
    )

    lastAppliedViewportPlanSignature = signature
    return !regions.isEmpty || !liveClips.isEmpty || !decorations.isEmpty
  }

}

private extension UInt32 {
  func saturatingSubtract(_ other: UInt32) -> UInt32 {
    self > other ? self - other : 0
  }
}

private extension UInt64 {
  func saturatingAdding(_ other: UInt64) -> UInt64 {
    addingReportingOverflow(other).partialValue
  }
}

import Foundation
import Metal

/// Rust-owned primary-frame clip for one running Flow Block (#865).
/// `firstRow`/`rowCount` map `start_line` onto the prepared viewport; hosts
/// must not invent a history end such as `start + 511`.
struct LiveTailClip: Equatable {
    var startLine: UInt64
    var firstRow: UInt16
    var rowCount: UInt16
}

struct LiveTailRegionGeometry: Equatable {
    var rowCount: Int
    var columns: Int
    var cellWidth: Int
    var cellHeight: Int
    var backingScale: CGFloat
    var atlasResetGeneration: UInt64
}

/// Renderer-local live-tail draw state. Clip rows come only from Rust; this
/// holds GPU instance buffers for the prepared-frame slice per running Block.
struct LiveTailRenderState {
    var regions: [UInt64: HistoryRenderRegion] = [:]
    var regionGeometry: [UInt64: LiveTailRegionGeometry] = [:]
    var order: [UInt64] = []
    var clips: [UInt64: LiveTailClip] = [:]
    /// A row-map mismatch may retain a same-geometry buffer for one submitted
    /// frame while Runtime and the prepared frame converge. Never retry the
    /// fallback until that Block has a valid map or its clip changes.
    var invalidMappingObserved: Set<UInt64> = []
    var pendingTransientFallbacks: Set<UInt64> = []
    /// Clip membership changed while a command buffer still samples live-tail
    /// instance buffers. Refresh after GPU completion (same gate as history).
    var deferredForceRefresh = false
    var transcriptRegions: [UInt64: NativeTranscriptRegion] = [:]
    var lastPreparedFrame: NativePreparedFrame?
}

extension MetalTerminalRenderer {
    /// Moves a prepared history region by replacing only its draw metadata.
    @discardableResult
    func setHistoryRegionGeometry(id: UInt64, origin: CGPoint, clip: CGRect) -> Bool {
        guard let existing = historyRegions[id] else { return false }
        let nextOrigin = SIMD2<Float>(Float(origin.x), Float(origin.y))
        guard existing.origin != nextOrigin || existing.clip != clip else { return true }
        historyRegions[id] = HistoryRenderRegion(
            buffer: existing.buffer,
            instanceCount: existing.instanceCount,
            origin: nextOrigin,
            clip: clip,
            historySource: existing.historySource,
            atlasGeneration: existing.atlasGeneration
        )
        needsPresent = true
        return true
    }

    /// Sealed HISTORY then LiveTail. Never dual-draw the same Block — LiveTail
    /// owns Running paint until sealed HISTORY is prepared (#1484 Finding E).
    var orderedBlockRegions: [HistoryRenderRegion] {
        let liveIDs = Set(liveTail.order)
        let historyOnly = historyRegionOrder.compactMap { id -> HistoryRenderRegion? in
            guard !liveIDs.contains(id) else { return nil }
            return historyRegions[id]
        }
        return historyOnly + orderedLiveTailRegions
    }

    /// True when sealed HISTORY GPU instances exist for `blockID`.
    func hasPreparedHistoryRegion(blockID: UInt64) -> Bool {
        historyRegions[blockID] != nil
    }

    /// Re-encodes sealed HISTORY whose baked atlas UVs predate the latest
    /// atlas reset. Steady state is one generation compare per region; only a
    /// reset re-uploads. Requires no command buffer sampling the atlas.
    func reencodeHistoryRegionsForCurrentAtlas() {
        let generation = glyphAtlas.stats.resets
        guard historyRegions.values.contains(where: { $0.atlasGeneration != generation }) else {
            return
        }
        flushDeferredHistoryPrepares()
        for (id, region) in historyRegions where region.atlasGeneration != generation {
            guard let source = region.historySource else {
                historyRegions.removeValue(forKey: id)
                continue
            }
            let placed = NativeTranscriptRegion(
                id: id,
                origin: CGPoint(x: CGFloat(region.origin.x), y: CGFloat(region.origin.y)),
                clip: region.clip
            )
            do {
                try applyHistoryPrepare(
                    historyRange: source.historyRange,
                    region: placed,
                    backingScale: source.backingScale,
                    cells: source.historyRange.rows.reduce(0) { $0 + min($1.count, 512) }
                )
            } catch {
                // Blank beats sampling another glyph's atlas slot.
                historyRegions.removeValue(forKey: id)
            }
        }
    }

    /// Relocate LiveTail draw metadata without rewriting instance buffers.
    @discardableResult
    func setLiveTailRegionGeometry(id: UInt64, origin: CGPoint, clip: CGRect) -> Bool {
        guard let existing = liveTail.regions[id] else { return false }
        let nextOrigin = SIMD2<Float>(Float(origin.x), Float(origin.y))
        guard existing.origin != nextOrigin || existing.clip != clip else { return true }
        liveTail.regions[id] = HistoryRenderRegion(
            buffer: existing.buffer,
            instanceCount: existing.instanceCount,
            origin: nextOrigin,
            clip: clip
        )
        needsPresent = true
        return true
    }

    var orderedLiveTailRegions: [HistoryRenderRegion] {
        liveTail.order.compactMap { liveTail.regions[$0] }
    }

    var liveTailRegionCount: Int {
        liveTail.regions.count
    }

    var lastPreparedRowCount: Int {
        liveTail.lastPreparedFrame?.rows ?? 0
    }

    /// Instance count for one live-tail Block (0 when absent / fail closed).
    func liveTailInstanceCount(for blockID: UInt64) -> Int {
        liveTail.regions[blockID]?.instanceCount ?? 0
    }

    /// Non-zero painted instances for one live-tail Block (excludes dense
    /// zero-size placeholders outside the region clip).
    func liveTailPaintedInstanceCount(for blockID: UInt64) -> Int {
        guard let region = liveTail.regions[blockID] else { return 0 }
        let pointer = region.buffer.contents().bindMemory(
            to: TerminalInstance.self,
            capacity: region.instanceCount
        )
        var count = 0
        for index in 0..<region.instanceCount where pointer[index].size.x > 0 && pointer[index].size.y > 0 {
            count += 1
        }
        return count
    }

    /// Block-local origin Y of the first painted instance for `blockID`.
    func liveTailFirstPaintedOriginY(for blockID: UInt64) -> Float? {
        guard let region = liveTail.regions[blockID] else { return nil }
        let pointer = region.buffer.contents().bindMemory(
            to: TerminalInstance.self,
            capacity: region.instanceCount
        )
        for index in 0..<region.instanceCount where pointer[index].size.x > 0 && pointer[index].size.y > 0 {
            return pointer[index].origin.y
        }
        return nil
    }

    /// Register running Flow Blocks that clip the prepared primary frame.
    /// Empty clears all live-tail regions. On refresh failure, live-tail draws
    /// are cleared (fail closed) so stale instances cannot remain on screen.
    func setLiveTailBlocks(_ clipsByBlock: [UInt64: LiveTailClip]) {
        if clipsByBlock == liveTail.clips {
            return
        }
        for (blockID, clip) in clipsByBlock where liveTail.clips[blockID] != clip {
            liveTail.invalidMappingObserved.remove(blockID)
            liveTail.pendingTransientFallbacks.remove(blockID)
        }
        liveTail.clips = clipsByBlock
        liveTail.order = clipsByBlock.keys.sorted()
        let keep = Set(liveTail.order)
        liveTail.regions = liveTail.regions.filter { keep.contains($0.key) }
        liveTail.regionGeometry = liveTail.regionGeometry.filter { keep.contains($0.key) }
        liveTail.invalidMappingObserved.formIntersection(keep)
        liveTail.pendingTransientFallbacks.formIntersection(keep)
        forceLiveTailRefreshWhenIdle()
    }

    func setTranscriptRegions(_ regions: [NativeTranscriptRegion]) {
        var map: [UInt64: NativeTranscriptRegion] = [:]
        for region in regions {
            map[region.id] = region
        }
        if map == liveTail.transcriptRegions {
            return
        }
        liveTail.transcriptRegions = map
        guard !liveTail.clips.isEmpty else {
            needsPresent = true
            return
        }
        // Geometry-only moves: relocate existing LiveTail buffers. Cell updates
        // stay on the damage path — avoid a second force:true rewrite in the
        // same tick as setLiveTailBlocks (#1484 P0 seq flicker).
        var relocatedAny = false
        var missingBuffer = false
        for (blockID, region) in map where liveTail.clips[blockID] != nil {
            if setLiveTailRegionGeometry(
                id: blockID,
                origin: region.origin,
                clip: region.clip
            ) {
                relocatedAny = true
            } else {
                missingBuffer = true
            }
        }
        if missingBuffer {
            forceLiveTailRefreshWhenIdle()
        } else if relocatedAny {
            needsPresent = true
        }
    }

    /// Damage-driven refresh after a successful Candidate-D prepare.
    func refreshLiveTailAfterPrepare(frame: NativePreparedFrame, damage: DamageMask, scale: CGFloat) {
        liveTail.lastPreparedFrame = frame
        do {
            try refreshLiveTailClips(backingScale: scale, damage: damage, force: false)
        } catch {
            failClosedLiveTail()
        }
    }

    func flushDeferredLiveTailRefresh() {
        guard framesInFlight == 0, liveTail.deferredForceRefresh else { return }
        liveTail.deferredForceRefresh = false
        guard liveTail.lastPreparedFrame != nil else { return }
        do {
            try refreshLiveTailClips(
                backingScale: currentScale > 0 ? currentScale : 1,
                damage: DamageMask(),
                force: true
            )
        } catch {
            failClosedLiveTail()
        }
    }

    private func forceLiveTailRefreshWhenIdle() {
        needsPresent = true
        guard liveTail.lastPreparedFrame != nil else { return }
        // Never CPU-write shared live-tail buffers while GPU may still sample.
        if framesInFlight > 0 {
            liveTail.deferredForceRefresh = true
            return
        }
        do {
            try refreshLiveTailClips(
                backingScale: currentScale > 0 ? currentScale : 1,
                damage: DamageMask(),
                force: true
            )
        } catch {
            failClosedLiveTail()
        }
    }

    private func refreshLiveTailClips(
        backingScale: CGFloat,
        damage: DamageMask,
        force: Bool
    ) throws {
        guard presentationPlan.mode == .flow,
              !presentationPlan.drawsLiveGrid,
              let frame = liveTail.lastPreparedFrame
        else {
            liveTail.regions.removeAll(keepingCapacity: false)
            return
        }
        let scale = max(backingScale, 1)
        let metrics = glyphAtlas.metrics(backingScale: scale)
        var next: [UInt64: HistoryRenderRegion] = [:]
        var nextGeometry: [UInt64: LiveTailRegionGeometry] = [:]
        for blockID in liveTail.order {
            guard let clip = liveTail.clips[blockID],
                  clip.startLine > 0,
                  clip.rowCount > 0,
                  let region = liveTail.transcriptRegions[blockID]
            else {
                continue
            }
            let firstRow = Int(clip.firstRow)
            let rowCount = Int(clip.rowCount)
            let geometry = LiveTailRegionGeometry(
                rowCount: rowCount,
                columns: frame.columns,
                cellWidth: metrics.cellWidth,
                cellHeight: metrics.cellHeight,
                backingScale: scale,
                atlasResetGeneration: glyphAtlas.stats.resets
            )
            guard firstRow < frame.rows, firstRow + rowCount <= frame.rows else {
                // Runtime and renderer updates may land on adjacent frames.
                // Retain one submitted frame only when the previous buffer's
                // Block-local instances still match its row/column, font-metric,
                // scale, and atlas generation. The current origin and clip are
                // draw-time metadata, so the retained cells follow a moved
                // Block without rewriting a buffer the GPU may be sampling.
                let firstInvalidMapping = liveTail.invalidMappingObserved.insert(blockID).inserted
                if (firstInvalidMapping || liveTail.pendingTransientFallbacks.contains(blockID)),
                   let existing = liveTail.regions[blockID],
                   let previousGeometry = liveTail.regionGeometry[blockID],
                   previousGeometry == geometry,
                   existing.instanceCount == rowCount * frame.columns
                {
                    next[blockID] = HistoryRenderRegion(
                        buffer: existing.buffer,
                        instanceCount: existing.instanceCount,
                        origin: SIMD2<Float>(Float(region.origin.x), Float(region.origin.y)),
                        clip: region.clip
                    )
                    nextGeometry[blockID] = geometry
                    liveTail.pendingTransientFallbacks.insert(blockID)
                } else {
                    liveTail.pendingTransientFallbacks.remove(blockID)
                }
                continue
            }
            liveTail.invalidMappingObserved.remove(blockID)
            liveTail.pendingTransientFallbacks.remove(blockID)
            // Damage-driven reuse: skip buffer rebuild when the clip row slice
            // is untouched and membership/geometry did not force a refresh.
            if !force,
               let existing = liveTail.regions[blockID],
               liveTail.regionGeometry[blockID] == geometry,
               !(firstRow..<(firstRow + rowCount)).contains(where: { damage.contains(row: $0) })
            {
                next[blockID] = HistoryRenderRegion(
                    buffer: existing.buffer,
                    instanceCount: existing.instanceCount,
                    origin: SIMD2<Float>(Float(region.origin.x), Float(region.origin.y)),
                    clip: region.clip
                )
                nextGeometry[blockID] = geometry
                continue
            }
            let cells = rowCount * frame.columns
            guard cells > 0 else { continue }
            let byteCount = cells * MemoryLayout<TerminalInstance>.stride
            // Reuse capacity-stable buffers; allocate only on growth / first use.
            let buffer: MTLBuffer
            let hadReusableBuffer: Bool
            if let existing = liveTail.regions[blockID],
               liveTail.regionGeometry[blockID] == geometry,
               existing.buffer.length >= byteCount,
               existing.instanceCount == cells
            {
                buffer = existing.buffer
                hadReusableBuffer = true
            } else {
                guard let allocated = device.makeBuffer(
                    length: byteCount,
                    options: .storageModeShared
                ) else {
                    throw MetalTerminalRendererError.unavailableBuffer
                }
                allocated.label = "Seyal Flow Live-Tail Instances"
                buffer = allocated
                stats.instanceBufferAllocations &+= 1
                hadReusableBuffer = false
            }
            let pointer = buffer.contents().bindMemory(to: TerminalInstance.self, capacity: cells)
            let prepared = instanceBuffer.map {
                $0.contents().bindMemory(to: TerminalInstance.self, capacity: instanceCount)
            }
            // Dense row×col layout enables partial-damage row rewrites. Cells
            // outside the region clip are written as zero-size so scissor still
            // owns paint bounds and inspectFlowPaint stays clean.
            let rewriteAll = force || !hadReusableBuffer
            let size = SIMD2<Float>(Float(metrics.cellWidth), Float(metrics.cellHeight))
            for rowOffset in 0..<rowCount {
                let row = firstRow + rowOffset
                if !rewriteAll, !damage.contains(row: row) {
                    continue
                }
                for column in 0..<frame.columns {
                    let index = row * frame.columns + column
                    let outputIndex = rowOffset * frame.columns + column
                    let origin = SIMD2<Float>(
                        Float(column * metrics.cellWidth),
                        Float(rowOffset * metrics.cellHeight)
                    )
                    let surfaceOrigin = origin + SIMD2<Float>(
                        Float(region.origin.x),
                        Float(region.origin.y)
                    )
                    let painted = CGRect(
                        x: CGFloat(surfaceOrigin.x),
                        y: CGFloat(surfaceOrigin.y),
                        width: CGFloat(size.x),
                        height: CGFloat(size.y)
                    )
                    stats.liveTailCellsRewritten &+= 1
                    guard region.clip.intersects(painted.insetBy(dx: 0.5, dy: 0.5)) else {
                        pointer[outputIndex] = TerminalInstance(
                            origin: .zero,
                            size: .zero,
                            uvRect: SIMD4<Float>(repeating: 0),
                            foreground: 0,
                            background: 0,
                            flags: 0,
                            atlasSlice: 0
                        )
                        continue
                    }
                    var instance: TerminalInstance
                    if let prepared, index < instanceCount {
                        instance = prepared[index]
                        instance.origin = origin
                        instance.size = size
                    } else {
                        let source = frame.cells[index]
                        instance = TerminalInstance(
                            origin: origin,
                            size: size,
                            uvRect: SIMD4<Float>(repeating: 0),
                            foreground: resolveTerminalColor(source.foreground, defaultRGBA: 0),
                            background: resolveTerminalColor(source.background, defaultRGBA: 0),
                            flags: 0,
                            atlasSlice: 0
                        )
                    }
                    // Cursor only inside the running Block clip.
                    if frame.cursorVisible,
                       !region.suppressesCursor,
                       row == frame.cursorRow,
                       column == frame.cursorColumn,
                       !presentationPlan.drawsCursorOutsideBlockRegions
                    {
                        instance.flags |= instanceCursorFlag
                    }
                    pointer[outputIndex] = instance
                }
            }
            next[blockID] = HistoryRenderRegion(
                buffer: buffer,
                instanceCount: cells,
                origin: SIMD2<Float>(Float(region.origin.x), Float(region.origin.y)),
                clip: region.clip
            )
            nextGeometry[blockID] = geometry
        }
        liveTail.regions = next
        liveTail.regionGeometry = nextGeometry
        needsPresent = true
    }

    /// A retained stale-map buffer is valid for one submitted frame only. The
    /// next draw clears it unless a valid Runtime mapping arrived in between.
    func retireSubmittedLiveTailFallbacks() {
        let retired = liveTail.pendingTransientFallbacks
        guard !retired.isEmpty else { return }
        for blockID in retired {
            liveTail.regions.removeValue(forKey: blockID)
            liveTail.regionGeometry.removeValue(forKey: blockID)
        }
        liveTail.pendingTransientFallbacks.removeAll(keepingCapacity: true)
        needsPresent = true
        // The fallback was encoded into the submitted drawable. Once that
        // drawable completes, schedule a fresh frame to clear it; the display
        // link is paused while the command buffer is in flight, so waiting for
        // another PTY frame would leave stale pixels visible indefinitely.
        needsCurrentFrameWhenIdle = true
    }

    /// Fail closed: drop live-tail draws so the next Flow present clears stale
    /// clips. Do not latch `persistentDisplayFailure` — that would freeze the
    /// last presented pixels on screen.
    private func failClosedLiveTail() {
        liveTail.regions.removeAll(keepingCapacity: false)
        liveTail.regionGeometry.removeAll(keepingCapacity: false)
        liveTail.invalidMappingObserved.removeAll(keepingCapacity: false)
        liveTail.pendingTransientFallbacks.removeAll(keepingCapacity: false)
        needsPresent = true
    }

    /// Drop running primary-frame clips and the cached prepared frame.
    /// Shell-exit retain must keep completed history regions only — leaving
    /// `lastPreparedFrame` lets clear→exit keep painting a loose viewport
    /// (cursor + later Block output) over retained cards (#1379).
    func clearLiveTailPresentation() {
        liveTail.clips.removeAll(keepingCapacity: false)
        liveTail.order.removeAll(keepingCapacity: false)
        liveTail.regions.removeAll(keepingCapacity: false)
        liveTail.regionGeometry.removeAll(keepingCapacity: false)
        liveTail.invalidMappingObserved.removeAll(keepingCapacity: false)
        liveTail.pendingTransientFallbacks.removeAll(keepingCapacity: false)
        liveTail.transcriptRegions.removeAll(keepingCapacity: false)
        liveTail.lastPreparedFrame = nil
        liveTail.deferredForceRefresh = false
        needsPresent = true
    }
}

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

/// Renderer-local live-tail draw state. Clip rows come only from Rust; this
/// holds GPU instance buffers for the prepared-frame slice per running Block.
struct LiveTailRenderState {
    var regions: [UInt64: HistoryRenderRegion] = [:]
    var order: [UInt64] = []
    var clips: [UInt64: LiveTailClip] = [:]
    /// Clip membership changed while a command buffer still samples live-tail
    /// instance buffers. Refresh after GPU completion (same gate as history).
    var deferredForceRefresh = false
    var transcriptRegions: [UInt64: NativeTranscriptRegion] = [:]
    var lastPreparedFrame: NativePreparedFrame?
}

extension MetalTerminalRenderer {
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
        liveTail.clips = clipsByBlock
        liveTail.order = clipsByBlock.keys.sorted()
        let keep = Set(liveTail.order)
        liveTail.regions = liveTail.regions.filter { keep.contains($0.key) }
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
        forceLiveTailRefreshWhenIdle()
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
            guard firstRow < frame.rows, firstRow + rowCount <= frame.rows else {
                // Stale mapping vs prepared frame: fail closed for this Block.
                continue
            }
            // Damage-driven reuse: skip buffer rebuild when the clip row slice
            // is untouched and membership/geometry did not force a refresh.
            if !force,
               let existing = liveTail.regions[blockID],
               !(firstRow..<(firstRow + rowCount)).contains(where: { damage.contains(row: $0) })
            {
                next[blockID] = existing
                continue
            }
            let cells = rowCount * frame.columns
            guard cells > 0 else { continue }
            let byteCount = cells * MemoryLayout<TerminalInstance>.stride
            // Reuse capacity-stable buffers; allocate only on growth / first use.
            let buffer: MTLBuffer
            let hadReusableBuffer: Bool
            if let existing = liveTail.regions[blockID],
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
                        Float(region.origin.x) + Float(column * metrics.cellWidth),
                        Float(region.origin.y) + Float(rowOffset * metrics.cellHeight)
                    )
                    let painted = CGRect(
                        x: CGFloat(origin.x),
                        y: CGFloat(origin.y),
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
                            foreground: resolveTerminalColor(source.foreground, defaultRGBA: 0xffe9_e1d8),
                            background: resolveTerminalColor(source.background, defaultRGBA: 0xff10_0d0b),
                            flags: 0,
                            atlasSlice: 0
                        )
                    }
                    // Cursor only inside the running Block clip.
                    if frame.cursorVisible,
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
                clip: region.clip
            )
        }
        liveTail.regions = next
        needsPresent = true
    }

    /// Fail closed: drop live-tail draws so the next Flow present clears stale
    /// clips. Do not latch `persistentDisplayFailure` — that would freeze the
    /// last presented pixels on screen.
    private func failClosedLiveTail() {
        liveTail.regions.removeAll(keepingCapacity: false)
        needsPresent = true
    }
}

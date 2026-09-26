import Foundation
import Metal
import QuartzCore

extension MetalTerminalRenderer {
    func allocateInstanceBuffer(rows: Int, columns: Int) throws {
        let count = rows * columns
        let byteCount = count * MemoryLayout<TerminalInstance>.stride
        guard let buffer = device.makeBuffer(length: max(byteCount, 1), options: .storageModeShared) else {
            throw MetalTerminalRendererError.unavailableBuffer
        }
        buffer.label = "Seyal Terminal Instances"
        instanceBuffer = buffer
        instanceCount = count
        stats.instanceBufferAllocations &+= 1
        stats.instanceBytes = UInt64(byteCount)
    }

    func rebuildRows(
        frame: NativePreparedFrame,
        damage: DamageMask,
        metrics: TerminalFontMetrics,
        backingScale: CGFloat
    ) throws {
        guard let instanceBuffer else {
            throw MetalTerminalRendererError.unavailableBuffer
        }
        let pointer = instanceBuffer.contents().bindMemory(
            to: TerminalInstance.self,
            capacity: instanceCount
        )
        let cellSize = SIMD2<Float>(Float(metrics.cellWidth), Float(metrics.cellHeight))
        let preparedRoleContinuation: UInt16 = 2
        let preparedHasGrapheme: UInt16 = 1 << 4

        // Grapheme sidecar is length-prefixed payloads in physical-cell order
        // for every multi-scalar lead on the surface.
        var graphemeCursor = 0
        let graphemeBytes = frame.graphemeUtf8
        var rowGraphemeOffsets = [Int](repeating: 0, count: frame.rows + 1)
        for index in 0..<frame.cells.count {
            if index.isMultiple(of: frame.columns) {
                rowGraphemeOffsets[index / frame.columns] = graphemeCursor
            }
            let reserved = frame.cells[index].reserved
            let role = reserved & 0b11
            let width = (reserved >> 2) & 0b11
            let hasGrapheme = reserved & preparedHasGrapheme != 0
            let knownBits = 0b11 | (0b11 << 2) | preparedHasGrapheme
            guard reserved & ~knownBits == 0,
                  role <= 2,
                  (role == 1 ? (width == 1 || width == 2) : width == 0),
                  !hasGrapheme || role == 1
            else {
                throw MetalTerminalRendererError.invalidFrame
            }
            if reserved & preparedHasGrapheme != 0 {
                guard graphemeCursor + 2 <= graphemeBytes.count else {
                    throw MetalTerminalRendererError.invalidFrame
                }
                let len = Int(graphemeBytes[graphemeCursor])
                    | (Int(graphemeBytes[graphemeCursor + 1]) << 8)
                graphemeCursor += 2
                guard graphemeCursor + len <= graphemeBytes.count else {
                    throw MetalTerminalRendererError.invalidFrame
                }
                graphemeCursor += len
            }
        }
        rowGraphemeOffsets[frame.rows] = graphemeCursor
        graphemeCursor = 0

        for row in 0..<frame.rows where damage.contains(row: row) {
            for column in 0..<frame.columns {
                let index = row * frame.columns + column
                let cell = frame.cells[index]
                let role = cell.reserved & 0b11
                let width = (cell.reserved >> 2) & 0b11

                var flags: UInt32 = 0
                var uvRect = SIMD4<Float>(repeating: 0)
                var atlasSlice: UInt32 = 0

                // Start from the validated row offset. This keeps sparse damage
                // preparation linear in the damaged rows instead of rescanning
                // every preceding cell for each row.
                if column == 0 {
                    graphemeCursor = rowGraphemeOffsets[row]
                }

                var graphemePayload: Data?
                if cell.reserved & preparedHasGrapheme != 0 {
                    let len = Int(frame.graphemeUtf8[graphemeCursor])
                        | (Int(frame.graphemeUtf8[graphemeCursor + 1]) << 8)
                    graphemeCursor += 2
                    graphemePayload = frame.graphemeUtf8.subdata(
                        in: graphemeCursor..<(graphemeCursor + len)
                    )
                    graphemeCursor += len
                }

                if role != preparedRoleContinuation {
                    if let graphemePayload,
                       let text = String(data: graphemePayload, encoding: .utf8),
                       !text.isEmpty
                    {
                        let entry = try glyphAtlas.lookupGrapheme(
                            text: text,
                            bold: cell.flags & preparedBoldFlag != 0,
                            backingScale: backingScale,
                            cellMetrics: metrics
                        )
                        flags |= instanceGlyphFlag
                        if width == 2 {
                            flags |= instanceWideGlyphFlag
                        }
                        uvRect = entry.uvRect
                        atlasSlice = entry.slice
                    } else if cell.scalar != 0 && cell.scalar != 32 {
                        let entry = try glyphAtlas.lookup(
                            scalar: cell.scalar,
                            bold: cell.flags & preparedBoldFlag != 0,
                            backingScale: backingScale,
                            cellMetrics: metrics
                        )
                        flags |= instanceGlyphFlag
                        if width == 2 {
                            flags |= instanceWideGlyphFlag
                        }
                        uvRect = entry.uvRect
                        atlasSlice = entry.slice
                    }
                }
                if cell.flags & preparedUnderlineFlag != 0 {
                    flags |= instanceUnderlineFlag
                }
                if frame.cursorVisible,
                   row == frame.cursorRow,
                   column == frame.cursorColumn
                {
                    flags |= instanceCursorFlag
                }

                pointer[index] = TerminalInstance(
                    origin: SIMD2<Float>(
                        Float(column * metrics.cellWidth),
                        Float(row * metrics.cellHeight)
                    ),
                    size: cellSize,
                    uvRect: uvRect,
                    foreground: resolveTerminalColor(
                        cell.foreground,
                        defaultRGBA: 0xffe9_e1d8
                    ),
                    background: resolveTerminalColor(
                        cell.background,
                        defaultRGBA: 0xff10_0d0b
                    ),
                    flags: flags,
                    atlasSlice: atlasSlice
                )
            }
        }
    }

    func makeCommandBuffer(
        target: MTLTexture,
        instanceBuffer: MTLBuffer?,
        atlasTexture: MTLTexture?,
        historyRegions: [HistoryRenderRegion] = []
    ) -> MTLCommandBuffer? {
        guard let commandBuffer = commandQueue.makeCommandBuffer() else { return nil }
        commandBuffer.label = "Seyal Terminal Frame"
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = target
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        if presentationPlan.drawsFullGridBackground {
            pass.colorAttachments[0].clearColor = MTLClearColor(
                red: 0.043,
                green: 0.051,
                blue: 0.063,
                alpha: 1
            )
        } else {
            pass.colorAttachments[0].clearColor = MTLClearColor(
                red: 0,
                green: 0,
                blue: 0,
                alpha: 0
            )
        }
        guard let encoder = commandBuffer.makeRenderCommandEncoder(descriptor: pass) else {
            return nil
        }
        encoder.label = "Seyal Terminal Encoder"
        encoder.setRenderPipelineState(pipeline)
        var viewport = SIMD2<Float>(Float(target.width), Float(target.height))
        encoder.setVertexBytes(
            &viewport,
            length: MemoryLayout<SIMD2<Float>>.stride,
            index: 1
        )
        var renderMode: UInt32 = 0
        encoder.setVertexBytes(
            &renderMode,
            length: MemoryLayout<UInt32>.stride,
            index: 2
        )
        encoder.setFragmentBytes(
            &renderMode,
            length: MemoryLayout<UInt32>.stride,
            index: 2
        )
        if let atlasTexture {
            encoder.setFragmentTexture(atlasTexture, index: 0)
            encoder.setFragmentSamplerState(sampler, index: 0)
        }
        let drawLiveGrid = presentationPlan.drawsLiveGrid
            && instanceCount > 0
            && instanceBuffer != nil
            && atlasTexture != nil
        if drawLiveGrid, let instanceBuffer {
            encoder.setVertexBuffer(instanceBuffer, offset: 0, index: 0)
            encoder.drawPrimitives(
                type: .triangle,
                vertexStart: 0,
                vertexCount: 6,
                instanceCount: instanceCount
            )
            // Wide grapheme glyphs span the lead and continuation cells. Draw
            // every cell background first, then draw glyphs in a second pass so a
            // continuation cell's background cannot cover the glyph's second
            // half. The glyph pass discards non-glyph instances in the fragment
            // stage and keeps the existing fixed-size instance buffer layout.
            renderMode = 1
            encoder.setVertexBytes(
                &renderMode,
                length: MemoryLayout<UInt32>.stride,
                index: 2
            )
            encoder.setFragmentBytes(
                &renderMode,
                length: MemoryLayout<UInt32>.stride,
                index: 2
            )
            encoder.drawPrimitives(
                type: .triangle,
                vertexStart: 0,
                vertexCount: 6,
                instanceCount: instanceCount
            )
        }
        for region in historyRegions where region.instanceCount > 0 && atlasTexture != nil {
            encoder.setVertexBuffer(region.buffer, offset: 0, index: 0)
            let x = max(0, Int(region.clip.minX.rounded(.down)))
            let y = max(0, Int(region.clip.minY.rounded(.down)))
            let maxX = min(target.width, Int(region.clip.maxX.rounded(.up)))
            let maxY = min(target.height, Int(region.clip.maxY.rounded(.up)))
            guard maxX > x, maxY > y else { continue }
            encoder.setScissorRect(MTLScissorRect(
                x: x,
                y: y,
                width: maxX - x,
                height: maxY - y
            ))
            // History uses the same two-pass order as the live surface:
            // cell-sized backgrounds first, then glyphs. A single composited
            // mode-2 pass would let a continuation instance cover the right
            // half of a width-two lead.
            for pass: UInt32 in [0, 1] {
                var historyMode = pass
                encoder.setVertexBytes(
                    &historyMode,
                    length: MemoryLayout<UInt32>.stride,
                    index: 2
                )
                encoder.setFragmentBytes(
                    &historyMode,
                    length: MemoryLayout<UInt32>.stride,
                    index: 2
                )
                encoder.drawPrimitives(
                    type: .triangle,
                    vertexStart: 0,
                    vertexCount: 6,
                    instanceCount: region.instanceCount
                )
            }
        }
        encoder.setScissorRect(MTLScissorRect(x: 0, y: 0, width: target.width, height: target.height))
        encoder.endEncoding()
        return commandBuffer
    }

    func commandCompleted(failed: Bool) {
        framesInFlight = 0
        stats.completedFrames &+= 1

        if failed {
            stats.commandCompletionFailures &+= 1
            deferredNeedsFullRebuild = true

            if gpuCompletionRetryState.recordFailureAndClaimRetry() {
                needsCurrentFrameWhenIdle = true
            } else {
                // A persistent asynchronous GPU failure is a terminal display
                // condition for this visible lifecycle. Stop all automatic GPU
                // resubmission while preserving disposable prepared/deferred
                // state. Hiding then showing the surface explicitly resets the
                // recovery state and reconstructs from committed Candidate-D.
                needsCurrentFrameWhenIdle = false
                needsPresent = false
                let failure = MetalTerminalRendererError.gpuCommandCompletionFailuresExhausted
                persistentDisplayFailure = failure
                stats.commandCompletionFailureExhaustions &+= 1
                onPersistentDisplayFailure?(failure)
            }
        } else {
            gpuCompletionRetryState.recordSuccess()
            persistentDisplayFailure = nil
        }

        if !visible {
            releaseWhenIdle = false
            releaseDedicatedResources()
            return
        }
        releaseWhenIdle = false

        // Do not let deferred damage or new terminal output bypass an exhausted
        // GPU completion series. The current Candidate-D/client authority keeps
        // advancing independently; an explicit lifecycle recovery can rehydrate
        // the renderer later.
        guard persistentDisplayFailure == nil else { return }

        flushDeferredHistoryPrepares()
        guard persistentDisplayFailure == nil else { return }

        if !deferredDamage.isEmpty || deferredNeedsFullRebuild || needsCurrentFrameWhenIdle {
            requestCurrentFrameIfNeeded()
        }
    }

    func countOpaquePixels(
        in texture: MTLTexture,
        clips: [CGRect]
    ) -> (outside: Int, inside: Int) {
        let width = texture.width
        let height = texture.height
        guard width > 0, height > 0 else { return (0, 0) }
        let bytesPerRow = width * 4
        var bytes = [UInt8](repeating: 0, count: bytesPerRow * height)
        texture.getBytes(
            &bytes,
            bytesPerRow: bytesPerRow,
            from: MTLRegionMake2D(0, 0, width, height),
            mipmapLevel: 0
        )
        var outside = 0
        var inside = 0
        var offset = 0
        for y in 0..<height {
            for x in 0..<width {
                let alpha = bytes[offset + 3]
                let maxRGB = max(bytes[offset], max(bytes[offset + 1], bytes[offset + 2]))
                offset += 4
                guard alpha > 8 || maxRGB > 8 else { continue }
                let point = CGPoint(x: CGFloat(x) + 0.5, y: CGFloat(y) + 0.5)
                if clips.contains(where: { $0.contains(point) }) {
                    inside += 1
                } else {
                    outside += 1
                }
            }
        }
        return (outside, inside)
    }

    func requestCurrentFrameIfNeeded() {
        guard persistentDisplayFailure == nil else {
            needsCurrentFrameWhenIdle = false
            return
        }
        guard framesInFlight == 0 else {
            needsCurrentFrameWhenIdle = true
            return
        }
        needsCurrentFrameWhenIdle = false
        onNeedsCurrentFrame?()
    }

    func releaseDedicatedResources() {
        instanceBuffer = nil
        instanceCount = 0
        historyRegions.removeAll()
        historyRegionOrder.removeAll()
        deferredHistoryPrepares.removeAll()
        currentRows = 0
        currentColumns = 0
        currentMetrics = nil
        currentScale = 0
        stats.instanceBytes = 0
        glyphAtlas.releaseResourcesWhenGPUIdle()
        deferredNeedsFullRebuild = true
    }
}

func resolveTerminalColor(_ packed: UInt32, defaultRGBA: UInt32) -> UInt32 {
    let tag = packed & 0xff00_0000
    if tag == 0 {
        return defaultRGBA
    }
    if tag == 0x0200_0000 {
        return packRGBA(
            red: UInt8((packed >> 16) & 0xff),
            green: UInt8((packed >> 8) & 0xff),
            blue: UInt8(packed & 0xff)
        )
    }
    if tag == 0x0100_0000 {
        return indexedColor(UInt8(packed & 0xff))
    }
    return defaultRGBA
}

func indexedColor(_ index: UInt8) -> UInt32 {
    let base: [(UInt8, UInt8, UInt8)] = [
        (0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16),
        (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
        (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67),
        (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255),
    ]
    let value = Int(index)
    if value < 16 {
        let rgb = base[value]
        return packRGBA(red: rgb.0, green: rgb.1, blue: rgb.2)
    }
    if value < 232 {
        let cube = value - 16
        let red = cube / 36
        let green = (cube % 36) / 6
        let blue = cube % 6
        func component(_ value: Int) -> UInt8 {
            value == 0 ? 0 : UInt8(55 + value * 40)
        }
        return packRGBA(
            red: component(red),
            green: component(green),
            blue: component(blue)
        )
    }
    let gray = UInt8(8 + (value - 232) * 10)
    return packRGBA(red: gray, green: gray, blue: gray)
}

func packRGBA(red: UInt8, green: UInt8, blue: UInt8) -> UInt32 {
    UInt32(red)
        | (UInt32(green) << 8)
        | (UInt32(blue) << 16)
        | 0xff00_0000
}

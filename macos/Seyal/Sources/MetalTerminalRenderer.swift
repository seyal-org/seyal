import Foundation
import Metal
import QuartzCore

/// Main-queue-only Metal presenter.
///
/// Intentionally **not** `@MainActor`: CAMetalDisplayLink and other AppKit
/// run-loop callbacks invoke `present` / `drainGPUCompletionsIfNeeded` on the
/// GCD main queue without a Swift MainActor task. Xcode 16.4 Trace/BPTs on
/// MainActor method entry (`_taskIsCurrentExecutor`) even though main-queue
/// mutual exclusion still holds. Callers must only touch this type from the
/// main queue (AppKit/`dispatchPrecondition`).
final class MetalTerminalRenderer: @unchecked Sendable {
    static let maximumFramesInFlight = 1

    let device: MTLDevice
    let commandQueue: MTLCommandQueue
    let pipeline: MTLRenderPipelineState
    let sampler: MTLSamplerState
    let glyphAtlas: GlyphAtlas

    var instanceBuffer: MTLBuffer?
    var instanceCount = 0
    /// Completed Block projections are keyed by lifecycle ID. The authoritative
    /// transcript frame replaces the order and membership atomically.
    var historyRegions: [UInt64: HistoryRenderRegion] = [:]
    var historyRegionOrder: [UInt64] = []
    var presentationPlan = RendererPresentationPlan.fullPane(.raw)
    var currentRows = 0
    var currentColumns = 0
    var currentMetrics: TerminalFontMetrics?
    var currentScale: CGFloat = 0
    var currentAlternateScreen = false
    var framesInFlight = 0
    var deferredDamage = DamageMask()
    var deferredNeedsFullRebuild = false
    /// History prepares that arrived while a command buffer still samples the
    /// shared glyph atlas. Latest prepare per Block ID wins.
    var deferredHistoryPrepares: [UInt64: DeferredHistoryPrepare] = [:]
    var releaseWhenIdle = false
    var visible = true
    var needsPresent = false
    var needsCurrentFrameWhenIdle = false
    var gpuCompletionRetryState = GPUCompletionRetryState()
    let gpuCompletionMailbox = GPUCompletionMailbox()
    /// Coalesces main-queue drain wakeups from GPU completion handlers.
    let gpuCompletionWakeScheduled = UnsafeMutablePointer<Int32>.allocate(capacity: 1)

    var stats = MetalRendererStats()
    var persistentDisplayFailure: MetalTerminalRendererError?
    var onNeedsCurrentFrame: (() -> Void)?
    var onPersistentDisplayFailure: ((MetalTerminalRendererError) -> Void)?

    init(device: MTLDevice, terminalFont: SeyalResolvedFontSpec = .canonicalTerminal) throws {
        gpuCompletionWakeScheduled.initialize(to: 0)
        self.device = device
        guard MemoryLayout<TerminalInstance>.stride == 48 else {
            throw MetalTerminalRendererError.invalidInstanceLayout
        }
        guard let commandQueue = device.makeCommandQueue() else {
            throw MetalTerminalRendererError.unavailableCommandQueue
        }
        self.commandQueue = commandQueue
        commandQueue.label = "Seyal Terminal Command Queue"

        let library: MTLLibrary
        do {
            library = try device.makeDefaultLibrary(bundle: .main)
        } catch {
            throw MetalTerminalRendererError.unavailableLibrary
        }
        guard let vertex = library.makeFunction(name: "seyal_terminal_vertex"),
              let fragment = library.makeFunction(name: "seyal_terminal_fragment")
        else {
            throw MetalTerminalRendererError.unavailableShader
        }
        let pipelineDescriptor = MTLRenderPipelineDescriptor()
        pipelineDescriptor.label = "Seyal Terminal Pipeline"
        pipelineDescriptor.vertexFunction = vertex
        pipelineDescriptor.fragmentFunction = fragment
        pipelineDescriptor.colorAttachments[0].pixelFormat = .bgra8Unorm
        pipelineDescriptor.colorAttachments[0].isBlendingEnabled = true
        pipelineDescriptor.colorAttachments[0].sourceRGBBlendFactor = .sourceAlpha
        pipelineDescriptor.colorAttachments[0].destinationRGBBlendFactor = .oneMinusSourceAlpha
        // Keep the render target opaque while glyph coverage controls only
        // RGB blending. Using sourceAlpha for the alpha channel would apply
        // coverage twice and leave normal glyph pixels partially transparent.
        pipelineDescriptor.colorAttachments[0].sourceAlphaBlendFactor = .one
        pipelineDescriptor.colorAttachments[0].destinationAlphaBlendFactor = .oneMinusSourceAlpha
        do {
            pipeline = try device.makeRenderPipelineState(descriptor: pipelineDescriptor)
        } catch {
            throw MetalTerminalRendererError.unavailablePipeline
        }

        let samplerDescriptor = MTLSamplerDescriptor()
        samplerDescriptor.minFilter = .linear
        samplerDescriptor.magFilter = .linear
        samplerDescriptor.sAddressMode = .clampToEdge
        samplerDescriptor.tAddressMode = .clampToEdge
        guard let sampler = device.makeSamplerState(descriptor: samplerDescriptor) else {
            throw MetalTerminalRendererError.unavailableSampler
        }
        self.sampler = sampler
        glyphAtlas = GlyphAtlas(
            device: device,
            fontResolver: TerminalFontResolver(spec: terminalFont)
        )
    }

    deinit {
        gpuCompletionWakeScheduled.deinitialize(count: 1)
        gpuCompletionWakeScheduled.deallocate()
    }

    var hasDedicatedSurfaceResources: Bool {
        instanceBuffer != nil || glyphAtlas.estimatedResidentBytes != 0
    }

    var hasPresentablePreparedState: Bool {
        persistentDisplayFailure == nil
            && needsPresent
            && (
                (presentationPlan.drawsLiveGrid
                    && instanceBuffer != nil
                    && instanceCount > 0
                    && glyphAtlas.texture != nil)
                    || !historyRegionOrder.isEmpty && glyphAtlas.texture != nil
                    || !presentationPlan.drawsLiveGrid
            )
    }

    var hasFrameInFlight: Bool {
        framesInFlight != 0
    }

    var hasDeferredHistoryPrepare: Bool {
        !deferredHistoryPrepares.isEmpty
    }

    var historyRegionCount: Int {
        historyRegions.count
    }

    var glyphStats: GlyphAtlasStats {
        glyphAtlas.stats
    }

    var estimatedDedicatedGPUBytes: Int {
        Int(stats.instanceBytes) + glyphAtlas.estimatedResidentBytes
    }

    var atlasResidentBytes: Int {
        glyphAtlas.estimatedResidentBytes
    }

    func cellPixelSize(backingScale: CGFloat) -> (width: Int, height: Int) {
        let metrics = glyphAtlas.metrics(backingScale: max(backingScale, 1))
        return (metrics.cellWidth, metrics.cellHeight)
    }

    func requestPresent() {
        dispatchPrecondition(condition: .onQueue(.main))
        drainGPUCompletionsIfNeeded()
        guard visible, persistentDisplayFailure == nil, instanceBuffer != nil else { return }
        needsPresent = true
    }

    func setVisible(_ value: Bool) {
        drainGPUCompletionsIfNeeded()
        guard visible != value else { return }
        visible = value
        if value {
            // A hide/show transition is an explicit lifecycle recovery event.
            // It is the only automatic way to clear an exhausted asynchronous
            // GPU completion failure series; ordinary terminal output cannot.
            gpuCompletionRetryState.resetForExplicitRecovery()
            persistentDisplayFailure = nil

            // A surface can become visible again before the last hidden-frame
            // command buffer completes.  Showing it cancels the deferred
            // release; completion must rebuild/publish the current frame.
            releaseWhenIdle = false
            deferredNeedsFullRebuild = true
            needsCurrentFrameWhenIdle = true
            if framesInFlight == 0 {
                requestCurrentFrameIfNeeded()
            }
        } else {
            needsPresent = false
            if framesInFlight == 0 {
                releaseDedicatedResources()
            } else {
                releaseWhenIdle = true
            }
        }
    }

    func update(
        frame: NativePreparedFrame,
        backingScale: CGFloat,
        forceFullRebuild: Bool = false
    ) throws -> RendererUpdateResult {
        drainGPUCompletionsIfNeeded()
        guard frame.rows > 0,
              frame.columns > 0,
              frame.rows <= 256,
              frame.columns <= 512,
              frame.cells.count == frame.rows * frame.columns,
              frame.cursorRow >= 0,
              frame.cursorRow < frame.rows,
              frame.cursorColumn >= 0,
              frame.cursorColumn < frame.columns
        else {
            throw MetalTerminalRendererError.invalidFrame
        }

        var incomingDamage = frame.damage
        if frame.fullRebuild || forceFullRebuild {
            incomingDamage.markAll(rows: frame.rows)
        }

        if persistentDisplayFailure != nil {
            // Candidate-D/client state remains authoritative and continues to
            // advance while the display is failed. Do not spend CPU reshaping or
            // rebuilding every generation for a GPU lifecycle that is latched
            // off; coalesce enough invalidation to rebuild from current state on
            // the explicit lifecycle recovery.
            deferredNeedsFullRebuild = true
            deferredDamage.formUnion(incomingDamage)
            stats.coalescedFrames &+= 1
            return .deferred
        }

        if !visible {
            deferredNeedsFullRebuild = true
            deferredDamage.formUnion(incomingDamage)
            stats.coalescedFrames &+= 1
            return .deferred
        }

        if framesInFlight >= Self.maximumFramesInFlight {
            deferredDamage.formUnion(incomingDamage)
            deferredNeedsFullRebuild = deferredNeedsFullRebuild
                || frame.fullRebuild
                || forceFullRebuild
            needsCurrentFrameWhenIdle = true
            stats.coalescedFrames &+= 1
            return .deferred
        }

        var preparationSucceeded = false
        defer {
            if !preparationSucceeded {
                invalidatePreparedState()
            }
        }

        let scale = max(backingScale, 1)
        let metrics = glyphAtlas.metrics(backingScale: scale)
        let geometryChanged = currentRows != frame.rows || currentColumns != frame.columns
        let scaleChanged = currentScale != scale || currentMetrics != metrics
        let screenChanged = currentAlternateScreen != frame.alternateScreen
        var fullRebuild = forceFullRebuild
            || frame.fullRebuild
            || geometryChanged
            || scaleChanged
            || screenChanged
            || deferredNeedsFullRebuild
            || instanceBuffer == nil

        var damage = incomingDamage
        damage.formUnion(deferredDamage)
        if fullRebuild {
            damage.markAll(rows: frame.rows)
        }

        if scaleChanged {
            glyphAtlas.resetWhenGPUIdle()
        }
        if geometryChanged || instanceBuffer == nil {
            try allocateInstanceBuffer(rows: frame.rows, columns: frame.columns)
            fullRebuild = true
            damage.markAll(rows: frame.rows)
        }

        if damage.isEmpty {
            // The committed Candidate-D frame can advance without changing
            // any rows. Reuse the prepared instance/sidecar state and avoid
            // rescanning or copying the full grapheme payload.
            currentRows = frame.rows
            currentColumns = frame.columns
            currentMetrics = metrics
            currentScale = scale
            currentAlternateScreen = frame.alternateScreen
            deferredDamage = DamageMask()
            deferredNeedsFullRebuild = false
            needsCurrentFrameWhenIdle = false
            needsPresent = true
            preparationSucceeded = true
            return .updated
        }

        do {
            try rebuildRows(
                frame: frame,
                damage: damage,
                metrics: metrics,
                backingScale: scale
            )
        } catch GlyphAtlasError.capacityExceeded {
            glyphAtlas.resetWhenGPUIdle()
            var allRows = DamageMask()
            allRows.markAll(rows: frame.rows)
            do {
                try rebuildRows(
                    frame: frame,
                    damage: allRows,
                    metrics: metrics,
                    backingScale: scale
                )
            } catch let error as GlyphAtlasError {
                throw MetalTerminalRendererError.glyphAtlas(error)
            }
            fullRebuild = true
            damage = allRows
        } catch let error as GlyphAtlasError {
            throw MetalTerminalRendererError.glyphAtlas(error)
        }

        do {
            _ = try glyphAtlas.ensureTextureForRendering()
        } catch let error as GlyphAtlasError {
            throw MetalTerminalRendererError.glyphAtlas(error)
        }

        currentRows = frame.rows
        currentColumns = frame.columns
        currentMetrics = metrics
        currentScale = scale
        currentAlternateScreen = frame.alternateScreen
        deferredDamage = DamageMask()
        deferredNeedsFullRebuild = false
        needsCurrentFrameWhenIdle = false
        needsPresent = true

        if fullRebuild {
            stats.fullRebuilds &+= 1
        }
        for row in 0..<frame.rows where damage.contains(row: row) {
            stats.rebuiltRows &+= 1
            stats.rebuiltCells &+= UInt64(frame.columns)
        }
        preparationSucceeded = true
        return .updated
    }

    /// Prepares a bounded canonical primary-history projection for the same
    /// Pane surface as the live terminal. No text conversion or second
    /// renderer is introduced; the range remains styled cells until the Metal
    /// fragment stage consumes it.
    ///
    /// History must not CPU-write the shared glyph atlas while a command buffer
    /// is still sampling it (`framesInFlight > 0`). Defer and coalesce until
    /// GPU completion, matching the live `update(frame:)` in-flight gate.
    @discardableResult
    func update(
        historyRange: NativeHistoryRange,
        region: NativeTranscriptRegion,
        backingScale: CGFloat
    ) throws -> RendererUpdateResult {
        guard historyRange.blockID != 0,
              historyRange.requestID != 0,
              region.id == historyRange.blockID,
              historyRange.rows.count <= 512
        else {
            throw MetalTerminalRendererError.invalidFrame
        }

        let cells = historyRange.rows.reduce(0) { $0 + min($1.count, 512) }
        guard cells <= 131_072 else {
            throw MetalTerminalRendererError.invalidFrame
        }

        if framesInFlight >= Self.maximumFramesInFlight {
            deferredHistoryPrepares[historyRange.blockID] = DeferredHistoryPrepare(
                historyRange: historyRange,
                region: region,
                backingScale: backingScale
            )
            stats.coalescedFrames &+= 1
            return .deferred
        }

        try applyHistoryPrepare(
            historyRange: historyRange,
            region: region,
            backingScale: backingScale,
            cells: cells
        )
        return .updated
    }

    func applyHistoryPrepare(
        historyRange: NativeHistoryRange,
        region: NativeTranscriptRegion,
        backingScale: CGFloat,
        cells: Int
    ) throws {
        deferredHistoryPrepares.removeValue(forKey: historyRange.blockID)
        guard cells > 0 else {
            historyRegions.removeValue(forKey: historyRange.blockID)
            needsPresent = instanceBuffer != nil || !presentationPlan.drawsLiveGrid
            return
        }
        let metrics = glyphAtlas.metrics(backingScale: max(backingScale, 1))
        let byteCount = cells * MemoryLayout<TerminalInstance>.stride
        guard let buffer = device.makeBuffer(length: byteCount, options: .storageModeShared) else {
            throw MetalTerminalRendererError.unavailableBuffer
        }
        buffer.label = "Seyal Canonical History Instances"
        let pointer = buffer.contents().bindMemory(to: TerminalInstance.self, capacity: cells)
        var outputIndex = 0
        for (rowIndex, row) in historyRange.rows.enumerated() {
            for (columnIndex, cell) in row.prefix(512).enumerated() {
                let origin = SIMD2<Float>(
                    Float(region.origin.x) + Float(columnIndex * metrics.cellWidth),
                    Float(region.origin.y) + Float(rowIndex * metrics.cellHeight)
                )
                let size = SIMD2<Float>(Float(metrics.cellWidth), Float(metrics.cellHeight))
                let painted = CGRect(
                    x: CGFloat(origin.x),
                    y: CGFloat(origin.y),
                    width: CGFloat(size.x),
                    height: CGFloat(size.y)
                )
                // Flow owns no full-grid canvas. Do not even prepare cells
                // outside the Block body clip: Metal scissoring prevents
                // pixels from escaping, but retaining those instances both
                // wastes hot-path work and violates the observable contract.
                guard region.clip.intersects(painted.insetBy(dx: 0.5, dy: 0.5)) else {
                    continue
                }
                var flags: UInt32 = 0
                var uvRect = SIMD4<Float>(repeating: 0)
                var atlasSlice: UInt32 = 0
                let continuation = cell.flags & (1 << 3) != 0
                let width = (cell.flags >> 4) & 0b11
                if !continuation,
                   (cell.scalar != 0 && cell.scalar != 32) || !cell.graphemeUtf8.isEmpty
                {
                    let entry: GlyphAtlasEntry
                    if !cell.graphemeUtf8.isEmpty,
                       let text = String(data: cell.graphemeUtf8, encoding: .utf8),
                       !text.isEmpty
                    {
                        entry = try glyphAtlas.lookupGrapheme(
                            text: text,
                            bold: cell.flags & 1 != 0,
                            backingScale: max(backingScale, 1),
                            cellMetrics: metrics
                        )
                    } else {
                        entry = try glyphAtlas.lookup(
                            scalar: cell.scalar,
                            bold: cell.flags & 1 != 0,
                            backingScale: max(backingScale, 1),
                            cellMetrics: metrics
                        )
                    }
                    flags |= instanceGlyphFlag
                    if width == 2 {
                        flags |= instanceWideGlyphFlag
                    }
                    uvRect = entry.uvRect
                    atlasSlice = entry.slice
                }
                if cell.flags & 2 != 0 { flags |= instanceUnderlineFlag }
                pointer[outputIndex] = TerminalInstance(
                    origin: origin,
                    size: size,
                    uvRect: uvRect,
                    foreground: resolveTerminalColor(cell.foreground, defaultRGBA: 0xffe9_e1d8),
                    background: resolveTerminalColor(cell.background, defaultRGBA: 0xff10_0d0b),
                    flags: flags,
                    atlasSlice: atlasSlice
                )
                outputIndex += 1
            }
        }
        historyRegions[historyRange.blockID] = HistoryRenderRegion(
            buffer: buffer,
            instanceCount: outputIndex,
            clip: region.clip
        )
        needsPresent = instanceBuffer != nil || !presentationPlan.drawsLiveGrid
    }

    func flushDeferredHistoryPrepares() {
        guard framesInFlight == 0, !deferredHistoryPrepares.isEmpty else { return }
        let pending = deferredHistoryPrepares
        deferredHistoryPrepares.removeAll(keepingCapacity: true)
        for prepare in pending.values {
            let cells = prepare.historyRange.rows.reduce(0) { $0 + min($1.count, 512) }
            do {
                try applyHistoryPrepare(
                    historyRange: prepare.historyRange,
                    region: prepare.region,
                    backingScale: prepare.backingScale,
                    cells: cells
                )
            } catch {
                // Preserve deferred damage/live recovery path; surface failure
                // through the same persistent-display latch used elsewhere.
                deferredNeedsFullRebuild = true
                needsCurrentFrameWhenIdle = true
                let failure: MetalTerminalRendererError
                if let atlas = error as? GlyphAtlasError {
                    failure = .glyphAtlas(atlas)
                } else if let metal = error as? MetalTerminalRendererError {
                    failure = metal
                } else {
                    failure = .invalidFrame
                }
                persistentDisplayFailure = failure
                onPersistentDisplayFailure?(failure)
                return
            }
        }
    }

    func setPresentationPlan(_ plan: RendererPresentationPlan) {
        presentationPlan = plan
        // Flow must be allowed to submit a clear-only frame so a prior live
        // grid cannot remain on the drawable after the mode fence.
        needsPresent = instanceBuffer != nil
            || !historyRegionOrder.isEmpty
            || !plan.drawsLiveGrid
    }

    func inspectPresentation() -> RendererPresentationInspection {
        RendererPresentationInspection(
            mode: presentationPlan.mode,
            drawsFullGridBackground: presentationPlan.drawsFullGridBackground,
            drawsLiveGrid: presentationPlan.drawsLiveGrid,
            drawsCursorOutsideBlockRegions: presentationPlan.drawsCursorOutsideBlockRegions,
            blockRegionIDs: historyRegionOrder
        )
    }

    func inspectFlowPaint(from texture: MTLTexture? = nil) -> FlowPaintInspection {
        var instancesOutsideClips = 0
        var historyInstanceCount = 0
        for region in orderedHistoryRegions {
            let pointer = region.buffer.contents().bindMemory(
                to: TerminalInstance.self,
                capacity: region.instanceCount
            )
            for index in 0..<region.instanceCount {
                historyInstanceCount += 1
                let origin = pointer[index].origin
                let size = pointer[index].size
                let painted = CGRect(
                    x: CGFloat(origin.x),
                    y: CGFloat(origin.y),
                    width: CGFloat(size.x),
                    height: CGFloat(size.y)
                )
                if !region.clip.intersects(painted.insetBy(dx: 0.5, dy: 0.5)) {
                    instancesOutsideClips += 1
                }
            }
        }
        var opaqueOutside = 0
        var opaqueInside = 0
        if let texture {
            let sampled = countOpaquePixels(in: texture, clips: orderedHistoryRegions.map(\.clip))
            opaqueOutside = sampled.outside
            opaqueInside = sampled.inside
        }
        return FlowPaintInspection(
            mode: presentationPlan.mode,
            liveGridSubmitted: presentationPlan.drawsLiveGrid,
            fullGridBackgroundSubmitted: presentationPlan.drawsFullGridBackground,
            historyInstanceCount: historyInstanceCount,
            instancesOutsideClips: instancesOutsideClips,
            opaquePixelsOutsideClips: opaqueOutside,
            opaquePixelsInsideClips: opaqueInside
        )
    }

    func setHistoryRegionOrder(_ ids: [UInt64]) {
        guard Set(ids).count == ids.count, ids.allSatisfy({ $0 != 0 }) else { return }
        let keep = Set(ids)
        historyRegions = historyRegions.filter { keep.contains($0.key) }
        historyRegionOrder = ids
        needsPresent = instanceBuffer != nil
    }

    func removeHistoryRegions(except ids: Set<UInt64>) {
        historyRegions = historyRegions.filter { ids.contains($0.key) }
        historyRegionOrder.removeAll { !ids.contains($0) }
        needsPresent = instanceBuffer != nil
    }

    private var orderedHistoryRegions: [HistoryRenderRegion] {
        historyRegionOrder.compactMap { historyRegions[$0] }
    }

    /// Submit a frame to a drawable supplied by the platform frame scheduler.
    /// Production presentation must not call `CAMetalLayer.nextDrawable()`
    /// here because that API can wait while all drawables are in use.
    ///
    /// - Parameter presentsToDisplay: When false, encode/commit only (no
    ///   `commandBuffer.present`). Deterministic self-tests on headless
    ///   Xcode 16.4 Trace/BPT when presenting into a compositor-backed
    ///   drawable; validation still covers in-flight coalescing via the same
    ///   completion mailbox.
    @discardableResult
    func present(drawable: any CAMetalDrawable, presentsToDisplay: Bool = true) -> Bool {
        dispatchPrecondition(condition: .onQueue(.main))
        drainGPUCompletionsIfNeeded()
        guard visible,
              persistentDisplayFailure == nil,
              needsPresent,
              framesInFlight == 0,
              hasPresentablePreparedState
        else {
            return false
        }
        guard let commandBuffer = makeCommandBuffer(
            target: drawable.texture,
            instanceBuffer: instanceBuffer,
            atlasTexture: glyphAtlas.texture,
            historyRegions: orderedHistoryRegions
        ) else {
            deferredNeedsFullRebuild = true
            needsCurrentFrameWhenIdle = true
            return false
        }

        framesInFlight = 1
        needsPresent = false
        stats.submittedFrames &+= 1
        if presentsToDisplay {
            commandBuffer.present(drawable)
        }
        // Metal completion runs off the main queue. Publish into the mailbox
        // and schedule a coalesced main-queue drain — do not use
        // `Task { @MainActor }` (Xcode 16.4 Trace/BPT). Renderer is main-queue
        // `@unchecked Sendable`, so `DispatchQueue.main.async` is safe.
        let mailbox = gpuCompletionMailbox
        commandBuffer.addCompletedHandler { [weak self] completed in
            mailbox.push(failed: completed.status == .error)
            self?.scheduleGPUCompletionDrainWakeup()
        }
        commandBuffer.commit()
        return true
    }

    /// Bounded event-driven wakeup: at most one main-queue drain is queued
    /// while completions are outstanding.
    func scheduleGPUCompletionDrainWakeup() {
        guard OSAtomicCompareAndSwap32Barrier(0, 1, gpuCompletionWakeScheduled) else {
            return
        }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            OSAtomicCompareAndSwap32Barrier(1, 0, self.gpuCompletionWakeScheduled)
            self.drainGPUCompletionsIfNeeded()
        }
    }

    /// Apply any GPU completions published off the main queue. Safe to call
    /// reentrantly from main-queue entry points (display link, update, timers).
    func drainGPUCompletionsIfNeeded() {
        dispatchPrecondition(condition: .onQueue(.main))
        for failed in gpuCompletionMailbox.drain() {
            commandCompleted(failed: failed)
        }
    }

    /// Deterministic validation substitute used only when a real drawable
    /// cannot be obtained. Prefer `present(drawable:presentsToDisplay:)` for
    /// production-path regression coverage.
    @discardableResult
    func beginValidationFrameInFlight() -> Bool {
        drainGPUCompletionsIfNeeded()
        guard visible,
              persistentDisplayFailure == nil,
              needsPresent,
              framesInFlight == 0,
              instanceBuffer != nil || !presentationPlan.drawsLiveGrid || !orderedHistoryRegions.isEmpty
        else {
            return false
        }
        framesInFlight = 1
        needsPresent = false
        stats.submittedFrames &+= 1
        return true
    }

    /// Completes a validation in-flight frame started by
    /// `beginValidationFrameInFlight()`.
    func endValidationFrameInFlight(failed: Bool = false) {
        guard framesInFlight > 0 else { return }
        commandCompleted(failed: failed)
    }

    /// Deterministic offscreen validation only. Production presentation never
    /// waits for GPU completion.
    func renderOffscreenAndWait(width: Int, height: Int) -> MTLTexture? {
        guard width > 0, height > 0 else {
            return nil
        }
        if presentationPlan.drawsLiveGrid {
            guard instanceBuffer != nil, instanceCount > 0, glyphAtlas.texture != nil else {
                return nil
            }
        } else if !orderedHistoryRegions.isEmpty {
            guard glyphAtlas.texture != nil else { return nil }
        }
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm,
            width: width,
            height: height,
            mipmapped: false
        )
        descriptor.usage = [.renderTarget]
        descriptor.storageMode = .shared
        guard let texture = device.makeTexture(descriptor: descriptor),
              let commandBuffer = makeCommandBuffer(
                  target: texture,
                  instanceBuffer: instanceBuffer,
                  atlasTexture: glyphAtlas.texture,
                  historyRegions: orderedHistoryRegions
              )
        else {
            return nil
        }
        commandBuffer.commit()
        commandBuffer.waitUntilCompleted()
        return commandBuffer.status == .completed ? texture : nil
    }

    /// Deterministic benchmark-only timing split. Target allocation is outside
    /// the measured interval; the first value covers command creation/encoding
    /// through commit, and the second covers commit through GPU completion.
    func renderOffscreenAndMeasureSubmission(width: Int, height: Int) -> MetalSubmissionTiming? {
        guard width > 0,
              height > 0,
              let instanceBuffer,
              instanceCount > 0,
              let atlasTexture = glyphAtlas.texture
        else {
            return nil
        }
        let descriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm,
            width: width,
            height: height,
            mipmapped: false
        )
        descriptor.usage = [.renderTarget]
        descriptor.storageMode = .shared
        guard let texture = device.makeTexture(descriptor: descriptor) else {
            return nil
        }
        let preparedToCommitStarted = DispatchTime.now().uptimeNanoseconds
        guard let commandBuffer = makeCommandBuffer(
                  target: texture,
                  instanceBuffer: instanceBuffer,
                  atlasTexture: atlasTexture,
                  historyRegions: orderedHistoryRegions
              )
        else {
            return nil
        }

        commandBuffer.commit()
        let commitFinished = DispatchTime.now().uptimeNanoseconds
        commandBuffer.waitUntilCompleted()
        guard commandBuffer.status == .completed else { return nil }
        let completed = DispatchTime.now().uptimeNanoseconds
        return MetalSubmissionTiming(
            preparedToCommitNanoseconds: commitFinished - preparedToCommitStarted,
            commitToCompletionNanoseconds: completed - commitFinished
        )
    }

    /// Make the current prepared state non-presentable after a failed
    /// replacement update.  `rebuildRows` writes into the live shared buffer
    /// for damage efficiency, so a later failure must never leave that
    /// partially updated buffer eligible for presentation.
    func invalidatePreparedState() {
        needsPresent = false
        deferredNeedsFullRebuild = true
        needsCurrentFrameWhenIdle = true
    }
}

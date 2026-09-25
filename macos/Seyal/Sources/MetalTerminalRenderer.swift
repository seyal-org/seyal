import Foundation
import Metal
import QuartzCore

/// Run `operation` while already on the GCD main queue, without
/// `MainActor.assumeIsolated`.
///
/// Apple platforms serialize MainActor work on the main dispatch queue, but
/// Swift 6.0 / Xcode 16.4's `assumeIsolated` checks `_taskIsCurrentExecutor`
/// rather than the queue. AppKit run-loop callbacks (CAMetalDisplayLink,
/// Timer) and GCD main-queue work therefore Trace/BPT if they call
/// `assumeIsolated` or enter a MainActor-isolated protocol witness, even
/// though mutual exclusion still holds. Use only after
/// `dispatchPrecondition(condition: .onQueue(.main))`.
func seyalRunAsMainActorFromMainQueue(_ operation: @MainActor () -> Void) {
    dispatchPrecondition(condition: .onQueue(.main))
    withoutActuallyEscaping(operation) { (fn: @escaping @MainActor () -> Void) in
        let raw = unsafeBitCast(fn, to: (() -> Void).self)
        raw()
    }
}

/// Single-slot GPU completion mailbox (`maximumFramesInFlight == 1`).
///
/// Metal completion handlers run off the main queue; main-queue code drains.
/// Uses a lock-free atomic slot instead of `NSLock` so present/update hot paths
/// do not take a blocking Foundation lock.
private final class GPUCompletionMailbox: @unchecked Sendable {
    /// 0 = empty, 1 = success pending, 2 = failure pending
    private let slot = UnsafeMutablePointer<Int32>.allocate(capacity: 1)

    init() {
        slot.initialize(to: 0)
    }

    deinit {
        slot.deinitialize(count: 1)
        slot.deallocate()
    }

    func push(failed: Bool) {
        let value: Int32 = failed ? 2 : 1
        while true {
            let current = slot.pointee
            if OSAtomicCompareAndSwap32Barrier(current, value, slot) {
                return
            }
        }
    }

    func drain() -> [Bool] {
        while true {
            let current = slot.pointee
            if current == 0 {
                return []
            }
            if OSAtomicCompareAndSwap32Barrier(current, 0, slot) {
                return [current == 2]
            }
        }
    }
}

private let preparedBoldFlag: UInt16 = 1 << 0
private let preparedUnderlineFlag: UInt16 = 1 << 1
private let instanceGlyphFlag: UInt32 = 1 << 0
private let instanceUnderlineFlag: UInt32 = 1 << 1
private let instanceCursorFlag: UInt32 = 1 << 2
private let instanceWideGlyphFlag: UInt32 = 1 << 3

private struct TerminalInstance {
    var origin: SIMD2<Float>
    var size: SIMD2<Float>
    var uvRect: SIMD4<Float>
    var foreground: UInt32
    var background: UInt32
    var flags: UInt32
    var atlasSlice: UInt32
}

struct DamageMask: Equatable {
    var word0: UInt64 = 0
    var word1: UInt64 = 0
    var word2: UInt64 = 0
    var word3: UInt64 = 0

    var isEmpty: Bool {
        word0 == 0 && word1 == 0 && word2 == 0 && word3 == 0
    }

    mutating func formUnion(_ other: DamageMask) {
        word0 |= other.word0
        word1 |= other.word1
        word2 |= other.word2
        word3 |= other.word3
    }

    mutating func markAll(rows: Int) {
        word0 = 0
        word1 = 0
        word2 = 0
        word3 = 0
        for row in 0..<min(rows, 256) {
            mark(row: row)
        }
    }

    mutating func mark(row: Int) {
        guard row >= 0, row < 256 else { return }
        let bit = UInt64(1) << UInt64(row & 63)
        switch row >> 6 {
        case 0: word0 |= bit
        case 1: word1 |= bit
        case 2: word2 |= bit
        default: word3 |= bit
        }
    }

    func contains(row: Int) -> Bool {
        guard row >= 0, row < 256 else { return false }
        let bit = UInt64(1) << UInt64(row & 63)
        switch row >> 6 {
        case 0: return word0 & bit != 0
        case 1: return word1 & bit != 0
        case 2: return word2 & bit != 0
        default: return word3 & bit != 0
        }
    }
}

struct NativePreparedFrame {
    /// Owned cell copy. Bridge frames are copied at construction so Rust
    /// `PreparedCell` storage never escapes into long-lived Swift state.
    let cells: [SeyalPreparedCell]
    /// Length-prefixed UTF-8 payloads for multi-scalar lead cells (SPEC-011 §12).
    let graphemeUtf8: Data
    let generation: UInt64
    let rows: Int
    let columns: Int
    let cursorRow: Int
    let cursorColumn: Int
    let cursorVisible: Bool
    let alternateScreen: Bool
    let fullRebuild: Bool
    let damage: DamageMask

    init?(bridgeFrame: SeyalPreparedFrame) {
        let rows = Int(bridgeFrame.rows)
        let columns = Int(bridgeFrame.columns)
        let count = Int(bridgeFrame.cell_count)
        guard rows > 0,
              columns > 0,
              rows <= 256,
              columns <= 512,
              count == rows * columns,
              let pointer = bridgeFrame.cells
        else {
            return nil
        }
        // Synchronous consume: copy before any later poll can invalidate Rust.
        cells = Array(UnsafeBufferPointer(start: pointer, count: count))
        if bridgeFrame.grapheme_utf8_len > 0, let graphemePtr = bridgeFrame.grapheme_utf8 {
            graphemeUtf8 = Data(
                bytes: graphemePtr,
                count: Int(bridgeFrame.grapheme_utf8_len)
            )
        } else {
            graphemeUtf8 = Data()
        }
        generation = bridgeFrame.generation
        self.rows = rows
        self.columns = columns
        cursorRow = Int(bridgeFrame.cursor_row)
        cursorColumn = Int(bridgeFrame.cursor_column)
        cursorVisible = bridgeFrame.cursor_visible != 0
        alternateScreen = bridgeFrame.alternate_screen != 0
        fullRebuild = bridgeFrame.full_rebuild != 0
        damage = DamageMask(
            word0: bridgeFrame.damage_word0,
            word1: bridgeFrame.damage_word1,
            word2: bridgeFrame.damage_word2,
            word3: bridgeFrame.damage_word3
        )
    }

    init(
        cells: UnsafeBufferPointer<SeyalPreparedCell>,
        generation: UInt64,
        rows: Int,
        columns: Int,
        cursorRow: Int = 0,
        cursorColumn: Int = 0,
        cursorVisible: Bool = false,
        alternateScreen: Bool = false,
        fullRebuild: Bool = true,
        damage: DamageMask = DamageMask(),
        graphemeUtf8: Data = Data()
    ) {
        self.cells = Array(cells)
        self.graphemeUtf8 = graphemeUtf8
        self.generation = generation
        self.rows = rows
        self.columns = columns
        self.cursorRow = cursorRow
        self.cursorColumn = cursorColumn
        self.cursorVisible = cursorVisible
        self.alternateScreen = alternateScreen
        self.fullRebuild = fullRebuild
        self.damage = damage
    }

    init(
        cells: [SeyalPreparedCell],
        generation: UInt64,
        rows: Int,
        columns: Int,
        cursorRow: Int = 0,
        cursorColumn: Int = 0,
        cursorVisible: Bool = false,
        alternateScreen: Bool = false,
        fullRebuild: Bool = true,
        damage: DamageMask = DamageMask(),
        graphemeUtf8: Data = Data()
    ) {
        self.cells = cells
        self.graphemeUtf8 = graphemeUtf8
        self.generation = generation
        self.rows = rows
        self.columns = columns
        self.cursorRow = cursorRow
        self.cursorColumn = cursorColumn
        self.cursorVisible = cursorVisible
        self.alternateScreen = alternateScreen
        self.fullRebuild = fullRebuild
        self.damage = damage
    }
}

private struct HistoryRenderRegion {
    let buffer: MTLBuffer
    let instanceCount: Int
    let clip: CGRect
}

enum MetalTerminalRendererError: Error {
    case unavailableCommandQueue
    case unavailableLibrary
    case unavailableShader
    case unavailablePipeline
    case unavailableSampler
    case unavailableBuffer
    case invalidFrame
    case invalidInstanceLayout
    case gpuCommandCompletionFailuresExhausted
    case presentationSubmissionFailuresExhausted
    case preparationFailuresExhausted
    case glyphAtlas(GlyphAtlasError)
}

enum RendererUpdateResult: Equatable {
    case updated
    case deferred
}

private struct DeferredHistoryPrepare {
    let historyRange: NativeHistoryRange
    let region: NativeTranscriptRegion
    let backingScale: CGFloat
}

struct GPUCompletionRetryState: Equatable {
    static let maximumAutomaticRetries = 4

    private(set) var retriesUsed = 0
    private(set) var exhausted = false

    mutating func recordSuccess() {
        retriesUsed = 0
        exhausted = false
    }

    /// Returns true only when another automatic GPU submission is permitted.
    /// The initial submission is not counted here, so four retries means a
    /// persistent failure series can produce at most five failed completions.
    mutating func recordFailureAndClaimRetry() -> Bool {
        guard !exhausted else { return false }
        guard retriesUsed < Self.maximumAutomaticRetries else {
            exhausted = true
            return false
        }
        retriesUsed += 1
        return true
    }

    mutating func resetForExplicitRecovery() {
        recordSuccess()
    }
}

struct MetalRendererStats: Equatable {
    var submittedFrames: UInt64 = 0
    var completedFrames: UInt64 = 0
    var commandCompletionFailures: UInt64 = 0
    var commandCompletionFailureExhaustions: UInt64 = 0
    var coalescedFrames: UInt64 = 0
    var fullRebuilds: UInt64 = 0
    var rebuiltRows: UInt64 = 0
    var rebuiltCells: UInt64 = 0
    var instanceBufferAllocations: UInt64 = 0
    var instanceBytes: UInt64 = 0
}

struct MetalSubmissionTiming {
    let preparedToCommitNanoseconds: UInt64
    let commitToCompletionNanoseconds: UInt64
}

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
    private let commandQueue: MTLCommandQueue
    private let pipeline: MTLRenderPipelineState
    private let sampler: MTLSamplerState
    private let glyphAtlas: GlyphAtlas

    private var instanceBuffer: MTLBuffer?
    private var instanceCount = 0
    /// Completed Block projections are keyed by lifecycle ID. The authoritative
    /// transcript frame replaces the order and membership atomically.
    private var historyRegions: [UInt64: HistoryRenderRegion] = [:]
    private var historyRegionOrder: [UInt64] = []
    /// Running Flow Blocks: damage-driven primary-frame clips keyed by Block ID.
    private var liveTailRegions: [UInt64: HistoryRenderRegion] = [:]
    private var liveTailOrder: [UInt64] = []
    private var liveTailStartLines: [UInt64: UInt64] = [:]
    private var transcriptRegions: [UInt64: NativeTranscriptRegion] = [:]
    private var lastPreparedFrame: NativePreparedFrame?
    private var presentationPlan = RendererPresentationPlan.fullPane(.raw)
    private var currentRows = 0
    private var currentColumns = 0
    private var currentMetrics: TerminalFontMetrics?
    private var currentScale: CGFloat = 0
    private var currentAlternateScreen = false
    private var framesInFlight = 0
    private var deferredDamage = DamageMask()
    private var deferredNeedsFullRebuild = false
    /// History prepares that arrived while a command buffer still samples the
    /// shared glyph atlas. Latest prepare per Block ID wins.
    private var deferredHistoryPrepares: [UInt64: DeferredHistoryPrepare] = [:]
    private var releaseWhenIdle = false
    private var visible = true
    private var needsPresent = false
    private var needsCurrentFrameWhenIdle = false
    private var gpuCompletionRetryState = GPUCompletionRetryState()
    private let gpuCompletionMailbox = GPUCompletionMailbox()
    /// Coalesces main-queue drain wakeups from GPU completion handlers.
    private let gpuCompletionWakeScheduled = UnsafeMutablePointer<Int32>.allocate(capacity: 1)

    private(set) var stats = MetalRendererStats()
    private(set) var persistentDisplayFailure: MetalTerminalRendererError?
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
                    || !liveTailOrder.isEmpty && glyphAtlas.texture != nil
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
            lastPreparedFrame = frame
            try refreshLiveTailClips(backingScale: scale)
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
        lastPreparedFrame = frame
        try refreshLiveTailClips(backingScale: scale)
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

    private func refreshLiveTailClips(backingScale: CGFloat) throws {
        guard presentationPlan.mode == .flow,
              !presentationPlan.drawsLiveGrid,
              let frame = lastPreparedFrame
        else {
            liveTailRegions.removeAll(keepingCapacity: false)
            return
        }
        let scale = max(backingScale, 1)
        let metrics = glyphAtlas.metrics(backingScale: scale)
        var next: [UInt64: HistoryRenderRegion] = [:]
        for blockID in liveTailOrder {
            guard liveTailStartLines[blockID] != nil,
                  let region = transcriptRegions[blockID]
            else {
                continue
            }
            let cells = frame.rows * frame.columns
            guard cells > 0 else { continue }
            let byteCount = cells * MemoryLayout<TerminalInstance>.stride
            guard let buffer = device.makeBuffer(length: byteCount, options: .storageModeShared)
            else {
                throw MetalTerminalRendererError.unavailableBuffer
            }
            buffer.label = "Seyal Flow Live-Tail Instances"
            let pointer = buffer.contents().bindMemory(to: TerminalInstance.self, capacity: cells)
            var outputIndex = 0
            for row in 0..<frame.rows {
                for column in 0..<frame.columns {
                    let index = row * frame.columns + column
                    let source = frame.cells[index]
                    let origin = SIMD2<Float>(
                        Float(region.origin.x) + Float(column * metrics.cellWidth),
                        Float(region.origin.y) + Float(row * metrics.cellHeight)
                    )
                    let size = SIMD2<Float>(Float(metrics.cellWidth), Float(metrics.cellHeight))
                    let painted = CGRect(
                        x: CGFloat(origin.x),
                        y: CGFloat(origin.y),
                        width: CGFloat(size.x),
                        height: CGFloat(size.y)
                    )
                    guard region.clip.intersects(painted.insetBy(dx: 0.5, dy: 0.5)) else {
                        continue
                    }
                    // Copy glyph/color from the already-prepared Pane instance when
                    // available; otherwise synthesize from the prepared cell.
                    var flags: UInt32 = 0
                    var uvRect = SIMD4<Float>(repeating: 0)
                    var atlasSlice: UInt32 = 0
                    if let instanceBuffer, index < instanceCount {
                        let prepared = instanceBuffer.contents().bindMemory(
                            to: TerminalInstance.self,
                            capacity: instanceCount
                        )[index]
                        flags = prepared.flags
                        uvRect = prepared.uvRect
                        atlasSlice = prepared.atlasSlice
                        pointer[outputIndex] = TerminalInstance(
                            origin: origin,
                            size: size,
                            uvRect: uvRect,
                            foreground: prepared.foreground,
                            background: prepared.background,
                            flags: flags,
                            atlasSlice: atlasSlice
                        )
                    } else {
                        pointer[outputIndex] = TerminalInstance(
                            origin: origin,
                            size: size,
                            uvRect: uvRect,
                            foreground: resolveTerminalColor(source.foreground, defaultRGBA: 0xffe9_e1d8),
                            background: resolveTerminalColor(source.background, defaultRGBA: 0xff10_0d0b),
                            flags: flags,
                            atlasSlice: atlasSlice
                        )
                    }
                    // Cursor only inside the running Block clip.
                    if frame.cursorVisible,
                       row == frame.cursorRow,
                       column == frame.cursorColumn,
                       presentationPlan.drawsCursorOutsideBlockRegions == false
                    {
                        var inst = pointer[outputIndex]
                        inst.flags |= instanceCursorFlag
                        pointer[outputIndex] = inst
                    }
                    outputIndex += 1
                }
            }
            if outputIndex > 0 {
                next[blockID] = HistoryRenderRegion(
                    buffer: buffer,
                    instanceCount: outputIndex,
                    clip: region.clip
                )
            }
        }
        liveTailRegions = next
        needsPresent = true
    }

    private func applyHistoryPrepare(
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

    private func flushDeferredHistoryPrepares() {
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
            || !liveTailOrder.isEmpty
            || !plan.drawsLiveGrid
    }

    func inspectPresentation() -> RendererPresentationInspection {
        RendererPresentationInspection(
            mode: presentationPlan.mode,
            drawsFullGridBackground: presentationPlan.drawsFullGridBackground,
            drawsLiveGrid: presentationPlan.drawsLiveGrid,
            drawsCursorOutsideBlockRegions: presentationPlan.drawsCursorOutsideBlockRegions,
            blockRegionIDs: historyRegionOrder + liveTailOrder
        )
    }

    /// Register running Flow Blocks that clip the prepared primary frame.
    /// Empty clears all live-tail regions. Hosts must not invent history ranges.
    func setLiveTailBlocks(_ startLinesByBlock: [UInt64: UInt64]) {
        liveTailStartLines = startLinesByBlock
        liveTailOrder = startLinesByBlock.keys.sorted()
        let keep = Set(liveTailOrder)
        liveTailRegions = liveTailRegions.filter { keep.contains($0.key) }
        if lastPreparedFrame != nil {
            try? refreshLiveTailClips(backingScale: currentScale > 0 ? currentScale : 1)
        }
        needsPresent = true
    }

    func setTranscriptRegions(_ regions: [NativeTranscriptRegion]) {
        var map: [UInt64: NativeTranscriptRegion] = [:]
        for region in regions {
            map[region.id] = region
        }
        transcriptRegions = map
        if lastPreparedFrame != nil {
            try? refreshLiveTailClips(backingScale: currentScale > 0 ? currentScale : 1)
        }
        needsPresent = true
    }

    var liveTailRegionCount: Int {
        liveTailRegions.count
    }

    var lastPreparedRowCount: Int {
        lastPreparedFrame?.rows ?? 0
    }

    func inspectFlowPaint(from texture: MTLTexture? = nil) -> FlowPaintInspection {
        var instancesOutsideClips = 0
        var historyInstanceCount = 0
        for region in orderedHistoryRegions + orderedLiveTailRegions {
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
            let sampled = countOpaquePixels(in: texture, clips: (orderedHistoryRegions + orderedLiveTailRegions).map(\.clip))
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

    private var orderedLiveTailRegions: [HistoryRenderRegion] {
        liveTailOrder.compactMap { liveTailRegions[$0] }
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
            historyRegions: orderedHistoryRegions + orderedLiveTailRegions
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
    private func scheduleGPUCompletionDrainWakeup() {
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
                  historyRegions: orderedHistoryRegions + orderedLiveTailRegions
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
                  historyRegions: orderedHistoryRegions + orderedLiveTailRegions
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

    static func gpuCompletionFailureRecoverySelfTest() -> Bool {
        // This is the exact state machine used by asynchronous production
        // command-completion handling. It intentionally contains no terminal,
        // client-cache or attachment authority, so exhausting it cannot mutate
        // canonical state.
        var recovery = GPUCompletionRetryState()
        var automaticRetries = 0

        // Initial failed submission + four automatic retries. The fifth failed
        // completion exhausts the series and must not claim a fifth retry.
        for _ in 0...GPUCompletionRetryState.maximumAutomaticRetries {
            if recovery.recordFailureAndClaimRetry() {
                automaticRetries += 1
            }
        }
        guard automaticRetries == GPUCompletionRetryState.maximumAutomaticRetries,
              recovery.exhausted,
              !recovery.recordFailureAndClaimRetry()
        else {
            return false
        }

        // Ordinary repeated failure calls cannot restart an exhausted series.
        for _ in 0..<1_000 where recovery.recordFailureAndClaimRetry() {
            return false
        }

        // A lifecycle-driven explicit recovery can restart a finite series.
        recovery.resetForExplicitRecovery()
        guard !recovery.exhausted,
              recovery.retriesUsed == 0,
              recovery.recordFailureAndClaimRetry()
        else {
            return false
        }

        // A genuine successful GPU completion also clears the consecutive
        // failure accounting.
        recovery.recordSuccess()
        return !recovery.exhausted && recovery.retriesUsed == 0
    }

    private func allocateInstanceBuffer(rows: Int, columns: Int) throws {
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

    private func rebuildRows(
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

    private func makeCommandBuffer(
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

    private func commandCompleted(failed: Bool) {
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

    private func countOpaquePixels(
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

    private func requestCurrentFrameIfNeeded() {
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

    private func releaseDedicatedResources() {
        instanceBuffer = nil
        instanceCount = 0
        historyRegions.removeAll()
        historyRegionOrder.removeAll()
        liveTailRegions.removeAll()
        liveTailOrder.removeAll()
        liveTailStartLines.removeAll()
        transcriptRegions.removeAll()
        lastPreparedFrame = nil
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

private func resolveTerminalColor(_ packed: UInt32, defaultRGBA: UInt32) -> UInt32 {
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

private func indexedColor(_ index: UInt8) -> UInt32 {
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

private func packRGBA(red: UInt8, green: UInt8, blue: UInt8) -> UInt32 {
    UInt32(red)
        | (UInt32(green) << 8)
        | (UInt32(blue) << 16)
        | 0xff00_0000
}

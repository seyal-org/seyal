import Foundation
import Metal

/// Disposable GPU-adapter state for one Rust viewport request. No part of it
/// enters the active draw order until every requested RegionGrid clip is ready.
struct PendingFlowFrame {
    let displayHandle: UInt64
    let presentationEpoch: UInt64
    let requestID: UInt64
    let regions: [NativeTranscriptRegion]
    let decorations: [MetalBlockSurfaceDecoration]
    let liveClips: [UInt64: LiveTailClip]
    let liveRegions: [NativeTranscriptRegion]
    var history: [UInt64: DeferredHistoryPrepare] = [:]

    var isComplete: Bool {
        regions.allSatisfy { history[$0.id] != nil }
    }
}

extension MetalTerminalRenderer {
    func clearFlowFrame() {
        pendingFlowFrame = nil
        activeFlowDisplayHandle = 0
        activeFlowRequestID = 0
        activeFlowRegions = []
        flowRetryDisplayHandle = 0
        flowRetryEpoch = 0
        flowInvalidPageRetries = 0
        flowCommitFailures = 0
        setBlockSurfaceDecorations([])
    }

    func stageFlowFrame(
        displayHandle: UInt64,
        presentationEpoch: UInt64,
        requestID: UInt64,
        regions: [NativeTranscriptRegion],
        decorations: [MetalBlockSurfaceDecoration],
        liveClips: [UInt64: LiveTailClip],
        liveRegions: [NativeTranscriptRegion]
    ) {
        dispatchPrecondition(condition: .onQueue(.main))
        guard regions.isEmpty || requestID != 0 else { return }
        if flowRetryDisplayHandle != displayHandle || flowRetryEpoch != presentationEpoch {
            flowRetryDisplayHandle = displayHandle
            flowRetryEpoch = presentationEpoch
            flowInvalidPageRetries = 0
            flowCommitFailures = 0
        }
        if let pending = pendingFlowFrame,
           pending.displayHandle == displayHandle,
           pending.presentationEpoch == presentationEpoch,
           pending.requestID == requestID,
           pending.regions == regions,
           pending.decorations == decorations,
           pending.liveClips == liveClips,
           pending.liveRegions == liveRegions {
            return
        }
        pendingFlowFrame = PendingFlowFrame(
            displayHandle: displayHandle,
            presentationEpoch: presentationEpoch,
            requestID: requestID,
            regions: regions,
            decorations: decorations,
            liveClips: liveClips,
            liveRegions: liveRegions
        )
        needsPresent = true
        requestPresent()
    }

    func pendingFlowRegion(id: UInt64, displayHandle: UInt64, requestID: UInt64) -> NativeTranscriptRegion? {
        guard let pending = pendingFlowFrame,
              pending.displayHandle == displayHandle,
              pending.requestID == requestID else { return nil }
        return pending.regions.first { $0.id == id }
    }

    /// A late response must never mutate an already published Flow generation.
    func shouldIgnoreFlowHistory(id: UInt64, displayHandle: UInt64, requestID: UInt64) -> Bool {
        guard presentationPlan.mode == .flow else { return false }
        if let pending = pendingFlowFrame,
           pending.displayHandle == displayHandle,
           pending.requestID == requestID,
           pending.regions.contains(where: { $0.id == id }) {
            return false
        }
        return activeFlowDisplayHandle != 0 || pendingFlowFrame != nil
    }

    @discardableResult
    func stageFlowHistory(
        _ range: NativeHistoryRange,
        region: NativeTranscriptRegion,
        backingScale: CGFloat
    ) -> Bool {
        guard var pending = pendingFlowFrame,
              pending.displayHandle == range.sourceDisplayHandle,
              pending.requestID == range.requestID,
              let expected = pending.regions.first(where: { $0.id == range.blockID }),
              range.status == 0,
              !range.rows.isEmpty,
              range.rows.count == expected.historyRows?.count else { return false }
        pending.history[range.blockID] = DeferredHistoryPrepare(
            historyRange: range,
            region: region,
            backingScale: backingScale
        )
        pendingFlowFrame = pending
        if pending.isComplete {
            needsPresent = true
            requestPresent()
        }
        return true
    }

    /// One retry for an invalid bounded page. A second stale result waits for
    /// a new Rust presentation epoch instead of creating an IPC retry storm.
    func rejectPendingFlowFrame(requestID: UInt64) -> Bool {
        guard pendingFlowFrame?.requestID == requestID else { return false }
        pendingFlowFrame = nil
        flowInvalidPageRetries += 1
        return flowInvalidPageRetries == 1
    }

    /// The display link calls this only with no GPU reader in flight. The
    /// setters are synchronous on the main queue, so a drawable sees either
    /// the old complete generation or the new complete generation.
    func commitPendingFlowFrameIfReady() {
        guard framesInFlight == 0,
              let pending = pendingFlowFrame,
              pending.isComplete,
              pending.liveClips.isEmpty || liveTail.lastPreparedFrame != nil else { return }
        let oldHistory = historyRegions
        let oldHistoryOrder = historyRegionOrder
        let oldDeferred = deferredHistoryPrepares
        let oldLive = liveTail
        let oldDecorations = blockSurfaceDecorations
        let oldSurfaceBuffer = blockSurfaceBuffer
        let oldSurfaceCount = blockSurfaceInstanceCount
        deferredHistoryPrepares = [:]
        do {
            for region in pending.regions {
                guard let prepare = pending.history[region.id] else {
                    throw MetalTerminalRendererError.invalidFrame
                }
                let cells = prepare.historyRange.rows.reduce(0) { $0 + min($1.count, 512) }
                try applyHistoryPrepare(
                    historyRange: prepare.historyRange,
                    region: prepare.region,
                    backingScale: prepare.backingScale,
                    cells: cells
                )
            }
            let visibleRegionIDs = Set(pending.regions.map(\.id))
            historyRegions = historyRegions.filter { visibleRegionIDs.contains($0.key) }
            if !pending.regions.isEmpty {
                _ = try glyphAtlas.ensureTextureForRendering()
            }
            guard pending.decorations.count <= 512 else {
                throw MetalTerminalRendererError.unavailableBuffer
            }
            setBlockSurfaceDecorations(pending.decorations)
            guard pending.decorations.isEmpty
                    || (blockSurfaceDecorations == pending.decorations && blockSurfaceBuffer != nil) else {
                throw MetalTerminalRendererError.unavailableBuffer
            }
            setLiveTailBlocks(pending.liveClips)
            setTranscriptRegions(pending.liveRegions)
            guard pending.liveClips.keys.allSatisfy({ liveTail.regions[$0] != nil }) else {
                throw MetalTerminalRendererError.invalidFrame
            }
            historyRegionOrder = pending.regions.map(\.id)
            activeFlowDisplayHandle = pending.displayHandle
            activeFlowRequestID = pending.requestID
            activeFlowRegions = pending.regions
            pendingFlowFrame = nil
            flowInvalidPageRetries = 0
            flowCommitFailures = 0
            needsPresent = true
        } catch {
            historyRegions = oldHistory
            historyRegionOrder = oldHistoryOrder
            deferredHistoryPrepares = oldDeferred
            liveTail = oldLive
            blockSurfaceDecorations = oldDecorations
            blockSurfaceBuffer = oldSurfaceBuffer
            blockSurfaceInstanceCount = oldSurfaceCount
            flowCommitFailures += 1
            if flowCommitFailures == 1 {
                // One event-driven retry can recover from an atlas/resource
                // transition. The old complete frame remains drawable.
                needsPresent = false
                DispatchQueue.main.async { [weak self] in
                    self?.onNeedsCurrentFrame?()
                    self?.needsPresent = true
                }
            } else {
                // A repeated preparation failure must be visible to the host,
                // not leave an unserviceable pending generation indefinitely.
                pendingFlowFrame = nil
                let failure = (error as? MetalTerminalRendererError)
                    ?? (error as? GlyphAtlasError).map(MetalTerminalRendererError.glyphAtlas)
                    ?? .invalidFrame
                persistentDisplayFailure = failure
                needsPresent = false
                onPersistentDisplayFailure?(failure)
            }
        }
    }
}

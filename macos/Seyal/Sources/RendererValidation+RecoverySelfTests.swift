import Foundation
import AppKit
import Darwin
import Metal
@preconcurrency import QuartzCore

@MainActor
extension RendererValidation {
    static func inFlightVisibilityRecoverySelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let cells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: cellSize.width,
                height: cellSize.height
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }
            // Hide while in flight and do not issue another update/input —
            // completion wakeup alone must release dedicated resources.
            renderer.setVisible(false)
            let released = waitForGPUCompletion(renderer, after: completedBefore)
                && !renderer.hasFrameInFlight
                && !renderer.hasDedicatedSurfaceResources
            _ = keepAlive
            return released
        } catch {
            return false
        }
    }

    /// Deferred Candidate-D update while a frame is in flight must be woken by
    /// GPU completion delivery alone — no further input/update.
    static func deferredFrameCompletionWakeupSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let cells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: cellSize.width,
                height: cellSize.height
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            var currentFrameRequests = 0
            renderer.onNeedsCurrentFrame = { currentFrameRequests += 1 }
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }
            let deferred = try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 2,
                        rows: 1,
                        columns: 1,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 2
                )
            }
            guard deferred == .deferred else { return false }
            // No further updates — only completion wakeup may request the frame.
            let woken = waitForGPUCompletion(renderer, after: completedBefore)
                && currentFrameRequests >= 1
                && !renderer.hasFrameInFlight
            _ = keepAlive
            return woken
        } catch {
            return false
        }
    }

    static func failedReplacementInvalidationSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            var cells = [
                preparedCell(scalar: UInt32(ascii: "A")),
                preparedCell(scalar: UInt32(ascii: "B"))
            ]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try cells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 2,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated
            }), renderer.hasPresentablePreparedState else { return false }

            cells[1].reserved = 1
            do {
                _ = try cells.withUnsafeBufferPointer { buffer in
                    try renderer.update(
                        frame: NativePreparedFrame(
                            cells: buffer,
                            generation: 2,
                            rows: 1,
                            columns: 2,
                            damage: damage
                        ),
                        backingScale: 1,
                        forceFullRebuild: true
                    )
                }
                return false
            } catch {
                guard !renderer.hasPresentablePreparedState else { return false }
            }

            cells[1].reserved = 0
            return try cells.withUnsafeBufferPointer { buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 3,
                        rows: 1,
                        columns: 2,
                        damage: damage
                    ),
                    backingScale: 1,
                    forceFullRebuild: true
                ) == .updated && renderer.hasPresentablePreparedState
            }
        } catch {
            return false
        }
    }

    /// #786: history prepare must not CPU-write the shared glyph atlas while a
    /// command buffer is still sampling it.
    static func historyPrepareDefersWhileFrameInFlightSelfTest() -> Bool {
        guard let device = MTLCreateSystemDefaultDevice() else { return false }
        do {
            let renderer = try MetalTerminalRenderer(device: device)
            let liveCells = [preparedCell(scalar: UInt32(ascii: "A"))]
            var damage = DamageMask()
            damage.mark(row: 0)
            guard try liveCells.withUnsafeBufferPointer({ buffer in
                try renderer.update(
                    frame: NativePreparedFrame(
                        cells: buffer,
                        generation: 1,
                        rows: 1,
                        columns: 1,
                        fullRebuild: true,
                        damage: damage
                    ),
                    backingScale: 1
                ) == .updated
            }) else { return false }

            let cellSize = renderer.cellPixelSize(backingScale: 1)
            let hosted = makePresentationLayer(
                device: device,
                width: max(cellSize.width, 8),
                height: max(cellSize.height, 8)
            )
            let layer = hosted.layer
            let keepAlive = hosted.keepAlive
            let completedBefore = renderer.stats.completedFrames
            guard presentOnLayerForValidation(renderer: renderer, layer: layer),
                  renderer.hasFrameInFlight
            else {
                return false
            }

            let uploadsBefore = renderer.glyphStats.uploads
            let history = NativeHistoryRange(
                startLine: 1,
                endLine: 1,
                blockID: 42,
                requestID: 7,
                revision: 1,
                rows: [[
                    NativeHistoryRange.Cell(
                        scalar: UInt32(ascii: "Z"),
                        foreground: 0xffe9_e1d8,
                        background: 0xff10_0d0b,
                        flags: 0
                    )
                ]]
            )
            let region = NativeTranscriptRegion(
                id: 42,
                origin: .zero,
                clip: NSRect(
                    x: 0,
                    y: 0,
                    width: CGFloat(cellSize.width),
                    height: CGFloat(cellSize.height)
                )
            )
            let deferred = try renderer.update(
                historyRange: history,
                region: region,
                backingScale: 1
            )
            guard deferred == .deferred,
                  renderer.hasDeferredHistoryPrepare,
                  renderer.historyRegionCount == 0,
                  renderer.glyphStats.uploads == uploadsBefore
            else {
                return false
            }

            guard waitForGPUCompletion(renderer, after: completedBefore),
                  !renderer.hasFrameInFlight
            else {
                return false
            }
            let ok = !renderer.hasDeferredHistoryPrepare
                && renderer.historyRegionCount == 1
                && renderer.glyphStats.uploads > uploadsBefore
            _ = keepAlive
            return ok
        } catch {
            return false
        }
    }

}

import AppKit
import Metal
import XCTest

@testable import Seyal

final class FlowFramePublicationTests: XCTestCase {
    @MainActor
    func testPublishesOnlyCompleteFlowGenerationAndRejectsLateHistory() throws {
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw XCTSkip("Metal device unavailable")
        }
        let renderer = try MetalTerminalRenderer(device: device)
        renderer.setPresentationPlan(.flow())
        let handle: UInt64 = 7
        let oldSurface = MetalBlockSurfaceDecoration(
            rect: CGRect(x: 0, y: 0, width: 100, height: 20), color: 0xffeeeeee)
        let nextSurface = MetalBlockSurfaceDecoration(
            rect: CGRect(x: 0, y: 0, width: 100, height: 40), color: 0xffdddddd)

        let oldRegion = region(id: 11, handle: handle, requestID: 1)
        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 1, requestID: 1,
            regions: [oldRegion], decorations: [oldSurface],
            liveClips: [:], liveRegions: [])
        renderer.stageFlowHistory(history(id: 11, handle: handle, requestID: 1),
                                  region: oldRegion, backingScale: 1)
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.activeFlowRequestID, 1)
        XCTAssertEqual(renderer.historyRegionOrder, [11])
        XCTAssertEqual(renderer.blockSurfaceDecorations, [oldSurface])

        let first = region(id: 21, handle: handle, requestID: 2)
        let second = region(id: 22, handle: handle, requestID: 2)
        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 2, requestID: 2,
            regions: [first, second], decorations: [nextSurface],
            liveClips: [:], liveRegions: [])
        renderer.stageFlowHistory(history(id: 21, handle: handle, requestID: 2),
                                  region: first, backingScale: 1)
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.activeFlowRequestID, 1,
                       "one RegionGrid reply cannot expose the next generation")
        XCTAssertEqual(renderer.historyRegionOrder, [11])
        XCTAssertEqual(renderer.blockSurfaceDecorations, [oldSurface])

        renderer.stageFlowHistory(history(id: 22, handle: handle, requestID: 2),
                                  region: second, backingScale: 1)
        renderer.framesInFlight = 1
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.activeFlowRequestID, 1,
                       "an in-flight drawable retains its complete old buffers")
        renderer.framesInFlight = 0
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.activeFlowRequestID, 2)
        XCTAssertEqual(renderer.historyRegionOrder, [21, 22])
        XCTAssertEqual(renderer.blockSurfaceDecorations, [nextSurface])
        XCTAssertNil(renderer.pendingFlowFrame)
        XCTAssertTrue(renderer.shouldIgnoreFlowHistory(
            id: 11, displayHandle: handle, requestID: 1))
        renderer.stageFlowHistory(history(id: 11, handle: handle, requestID: 1),
                                  region: oldRegion, backingScale: 1)
        XCTAssertEqual(renderer.historyRegionOrder, [21, 22])

        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 3, requestID: 0,
            regions: [], decorations: [], liveClips: [:], liveRegions: [])
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertTrue(renderer.activeFlowRegions.isEmpty)
        XCTAssertTrue(renderer.historyRegionOrder.isEmpty)
        XCTAssertTrue(renderer.blockSurfaceDecorations.isEmpty)
        XCTAssertNil(renderer.blockSurfaceBuffer)
    }

    @MainActor
    func testInvalidViewportPagesDoNotCompleteFrameAndRetryIsBounded() throws {
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw XCTSkip("Metal device unavailable")
        }
        let renderer = try MetalTerminalRenderer(device: device)
        renderer.setPresentationPlan(.flow())
        let expected = region(id: 31, handle: 7, requestID: 4)
        renderer.stageFlowFrame(
            displayHandle: 7, presentationEpoch: 9, requestID: 4,
            regions: [expected], decorations: [], liveClips: [:], liveRegions: [])
        XCTAssertFalse(renderer.stageFlowHistory(
            history(id: 31, handle: 7, requestID: 4, status: 2),
            region: expected, backingScale: 1))
        XCTAssertFalse(renderer.pendingFlowFrame?.isComplete ?? true)
        XCTAssertTrue(renderer.rejectPendingFlowFrame(requestID: 4))

        let retry = region(id: 31, handle: 7, requestID: 5)
        renderer.stageFlowFrame(
            displayHandle: 7, presentationEpoch: 9, requestID: 5,
            regions: [retry], decorations: [], liveClips: [:], liveRegions: [])
        XCTAssertFalse(renderer.stageFlowHistory(
            history(id: 31, handle: 7, requestID: 5, status: 1),
            region: retry, backingScale: 1))
        XCTAssertFalse(renderer.rejectPendingFlowFrame(requestID: 5))
        XCTAssertEqual(renderer.flowInvalidPageRetries, 2)
        XCTAssertNil(renderer.pendingFlowFrame)
        XCTAssertTrue(renderer.historyRegionOrder.isEmpty)
    }

    @MainActor
    func testRepeatedPreparationFailureSurfacesPersistentError() throws {
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw XCTSkip("Metal device unavailable")
        }
        let renderer = try MetalTerminalRenderer(device: device)
        renderer.setPresentationPlan(.flow())
        let decoration = MetalBlockSurfaceDecoration(
            rect: CGRect(x: 0, y: 0, width: 10, height: 10), color: 0xffeeeeee)
        renderer.stageFlowFrame(
            displayHandle: 7, presentationEpoch: 1, requestID: 0,
            regions: [], decorations: Array(repeating: decoration, count: 513),
            liveClips: [:], liveRegions: [])
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.flowCommitFailures, 1)
        XCTAssertNotNil(renderer.pendingFlowFrame)
        XCTAssertNil(renderer.persistentDisplayFailure)
        renderer.commitPendingFlowFrameIfReady()
        XCTAssertEqual(renderer.flowCommitFailures, 2)
        XCTAssertNil(renderer.pendingFlowFrame)
        guard case .unavailableBuffer? = renderer.persistentDisplayFailure else {
            return XCTFail("repeated renderer preparation failure must reach the host")
        }
    }

    @MainActor
    func testFlowHistoryWaitsForRustPlanWhileRawHistoryCanPrepareDirectly() throws {
        let surface = MetalSurfaceView(frame: CGRect(x: 0, y: 0, width: 100, height: 40))
        let page = history(id: 41, handle: 7, requestID: 1)
        surface.renderer.setPresentationPlan(.flow())
        surface.renderHistoryRange(page)
        XCTAssertTrue(surface.renderer.historyRegions.isEmpty,
                      "Flow cannot invent a clip before the Rust plan arrives")

        surface.renderer.setPresentationPlan(.fullPane(.raw))
        surface.renderHistoryRange(page)
        XCTAssertNotNil(surface.renderer.historyRegions[41],
                        "Raw scrollback keeps its direct history renderer path")
    }

    @MainActor
    func testPendingRustClipWinsOverOldHostRegionWhenPageArrives() throws {
        let surface = MetalSurfaceView(frame: CGRect(x: 0, y: 0, width: 100, height: 40))
        surface.renderer.setPresentationPlan(.flow())
        let pending = region(id: 51, handle: 7, requestID: 2)
        var old = region(id: 51, handle: 7, requestID: 1)
        old.historyRows = 0..<0
        surface.renderer.stageFlowFrame(
            displayHandle: 7, presentationEpoch: 2, requestID: 2,
            regions: [pending], decorations: [], liveClips: [:], liveRegions: [])

        surface.renderHistoryRange(history(id: 51, handle: 7, requestID: 2), region: old)

        XCTAssertTrue(surface.renderer.pendingFlowFrame?.isComplete ?? false,
                      "the Rust pending clip sets the row window, not the old host region")
        XCTAssertTrue(surface.renderer.historyRegions.isEmpty,
                      "the page is staged and must not paint before commit")
    }

    @MainActor
    func testUnchangedHistoryReusesInstancesAcrossMovedFlowGenerations() throws {
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw XCTSkip("Metal device unavailable")
        }
        let renderer = try MetalTerminalRenderer(device: device)
        renderer.setPresentationPlan(.flow())
        let handle: UInt64 = 7
        let first = region(
            id: 61, handle: handle, requestID: 1,
            origin: CGPoint(x: 0, y: 20),
            clip: CGRect(x: 0, y: 20, width: 100, height: 20)
        )
        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 1, requestID: 1,
            regions: [first], decorations: [], liveClips: [:], liveRegions: [])
        renderer.stageFlowHistory(history(id: 61, handle: handle, requestID: 1),
                                  region: first, backingScale: 1)
        renderer.commitPendingFlowFrameIfReady()
        let original = try XCTUnwrap(renderer.historyRegions[61])

        let moved = region(
            id: 61, handle: handle, requestID: 2,
            origin: CGPoint(x: 0, y: 40),
            clip: CGRect(x: 0, y: 40, width: 100, height: 20)
        )
        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 2, requestID: 2,
            regions: [moved], decorations: [], liveClips: [:], liveRegions: [])
        renderer.stageFlowHistory(history(id: 61, handle: handle, requestID: 2, revision: 2),
                                  region: moved, backingScale: 1)
        renderer.commitPendingFlowFrameIfReady()
        let reused = try XCTUnwrap(renderer.historyRegions[61])
        XCTAssertEqual(renderer.activeFlowRequestID, 2)
        XCTAssertTrue(reused.buffer === original.buffer,
                      "unchanged visible RegionGrid rows must keep their instance buffer")
        XCTAssertEqual(reused.origin, SIMD2<Float>(0, 40))
        XCTAssertEqual(reused.clip, moved.clip)
        XCTAssertEqual(reused.historySource?.historyRange.requestID, 2)
        XCTAssertEqual(reused.historySource?.historyRange.revision, 2)

        let changed = region(id: 61, handle: handle, requestID: 3)
        renderer.stageFlowFrame(
            displayHandle: handle, presentationEpoch: 3, requestID: 3,
            regions: [changed], decorations: [], liveClips: [:], liveRegions: [])
        renderer.stageFlowHistory(history(id: 61, handle: handle, requestID: 3, scalar: 66),
                                  region: changed, backingScale: 1)
        renderer.commitPendingFlowFrameIfReady()
        let rebuilt = try XCTUnwrap(renderer.historyRegions[61])
        XCTAssertFalse(rebuilt.buffer === reused.buffer,
                       "changed row payload must rebuild GPU instances")
    }

    private func region(
        id: UInt64, handle: UInt64, requestID: UInt64,
        origin: NSPoint = .zero,
        clip: NSRect = CGRect(x: 0, y: 0, width: 100, height: 20)
    ) -> NativeTranscriptRegion {
        var value = NativeTranscriptRegion(
            id: id, origin: origin, clip: clip)
        value.displayHandle = handle
        value.historyRows = 0..<1
        value.historyRequestID = requestID
        return value
    }

    private func history(
        id: UInt64, handle: UInt64, requestID: UInt64, status: UInt32 = 0,
        revision: UInt64 = 1, scalar: UInt32 = 65
    ) -> NativeHistoryRange {
        NativeHistoryRange(
            startLine: 0, endLine: 1, blockID: id, requestID: requestID,
            revision: revision,
            rows: [[NativeHistoryRange.Cell(
                scalar: scalar, foreground: 0, background: 0, flags: 0)]],
            status: status, sourceDisplayHandle: handle)
    }
}

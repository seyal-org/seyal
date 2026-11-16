import AppKit
import XCTest

@testable import Seyal

/// #1442: XCTest coverage for Raw presentation-local scrollback math and
/// Flow isolation helpers (UI policy requires SeyalTests for Sources changes).
final class RawScrollbackViewportTests: XCTestCase {
    func testRawScrollbackViewportMathSelfTests() {
        XCTAssertTrue(RawScrollbackViewportSelfTests.runAll())
    }

    func testCorrelationBlockIDIsDistinctFromOrdinaryBlocks() {
        XCTAssertEqual(RawScrollbackViewport.correlationBlockID, UInt64.max)
    }

    func testViewportClippingDoesNotSplitWideGraphemeAtRightEdge() {
        let row = [
            NativeHistoryRange.Cell(scalar: 65, foreground: 0, background: 0, flags: 0),
            NativeHistoryRange.Cell(
                scalar: 0x4E00, foreground: 0, background: 0, flags: 0x20),
            NativeHistoryRange.Cell(scalar: 0, foreground: 0, background: 0, flags: 0x08),
        ]

        let clipped = RawScrollbackViewport.padRowsForViewport(
            rows: [row], columns: 2, targetRowCount: 1)

        XCTAssertEqual(clipped.count, 1)
        XCTAssertEqual(clipped[0].count, 2)
        XCTAssertEqual(clipped[0][0].scalar, 65)
        XCTAssertEqual(clipped[0][1].scalar, 32)
    }

    func testOnlyTheExactLatestRawHistoryRequestCanReplaceTheVisiblePage() {
        let expected = RawScrollbackRequest(
            displayHandle: 42, requestID: 9, startLine: 100, endLine: 145)
        let current = NativeHistoryRange(
            startLine: 100,
            endLine: 145,
            blockID: RawScrollbackViewport.correlationBlockID,
            requestID: 9,
            revision: 1,
            rows: [],
            sourceDisplayHandle: 42
        )
        XCTAssertTrue(RawScrollbackViewport.isCurrentResponse(current, expected: expected))

        let stalePosition = NativeHistoryRange(
            startLine: 101,
            endLine: 146,
            blockID: RawScrollbackViewport.correlationBlockID,
            requestID: 8,
            revision: 1,
            rows: [],
            sourceDisplayHandle: 42
        )
        XCTAssertFalse(RawScrollbackViewport.isCurrentResponse(stalePosition, expected: expected))

        let wrongExecution = NativeHistoryRange(
            startLine: 100,
            endLine: 145,
            blockID: RawScrollbackViewport.correlationBlockID,
            requestID: 9,
            revision: 1,
            rows: [],
            sourceDisplayHandle: 43
        )
        XCTAssertFalse(RawScrollbackViewport.isCurrentResponse(wrongExecution, expected: expected))
        XCTAssertFalse(RawScrollbackViewport.isCurrentResponse(current, expected: nil))
    }

    func testAppKitWheelDeltaKeepsItsDirectionAndUsesTheRightUnit() {
        var remainder: CGFloat = 0
        XCTAssertEqual(
            InteractiveMetalSurfaceView.rawScrollRows(
                deltaY: 1, precise: false, cellHeight: 14, remainder: &remainder),
            1,
            "a conventional wheel-up delta must request older Raw history"
        )
        XCTAssertEqual(
            InteractiveMetalSurfaceView.rawScrollRows(
                deltaY: -2, precise: false, cellHeight: 14, remainder: &remainder),
            -2,
            "wheel-down line units must move toward LiveEnd"
        )

        remainder = 0
        XCTAssertEqual(
            InteractiveMetalSurfaceView.rawScrollRows(
                deltaY: 7, precise: true, cellHeight: 14, remainder: &remainder),
            nil,
            "a sub-row trackpad movement must accumulate without jumping"
        )
        XCTAssertEqual(
            InteractiveMetalSurfaceView.rawScrollRows(
                deltaY: 7, precise: true, cellHeight: 14, remainder: &remainder),
            1,
            "two half-cell precise deltas should accumulate into one row"
        )
    }

    @MainActor
    func testPrefersRawLocalScrollbackOnlyForRawNormalScreen() {
        let surface = MetalSurfaceView(frame: NSRect(x: 0, y: 0, width: 320, height: 200))
        let raw = UInt16(SEYAL_APP_ELIGIBILITY_RAW.rawValue)
        let flow = UInt16(SEYAL_APP_ELIGIBILITY_FLOW.rawValue)
        XCTAssertTrue(surface.prefersRawLocalScrollback(eligibility: raw, alternateScreen: false))
        XCTAssertFalse(surface.prefersRawLocalScrollback(eligibility: raw, alternateScreen: true))
        XCTAssertFalse(surface.prefersRawLocalScrollback(eligibility: flow, alternateScreen: false))
    }

    @MainActor
    func testClearRawScrollbackClearsPresentationCachesWithoutHostViewportAuthority() {
        let surface = MetalSurfaceView(frame: NSRect(x: 0, y: 0, width: 320, height: 200))
        surface.rawScrollPaintedStartLineID = 40
        surface.rawScrollRequestedHistory = RawScrollbackRequest(
            displayHandle: 7, requestID: 4, startLine: 30, endLine: 75)
        surface.rawScrollDidRefillLeadingBlanks = true

        let kept = surface.historyExceptPreservingRawScrollback([7])
        XCTAssertEqual(kept, [7], "an unbound host must not invent a Raw viewport")

        surface.clearRawScrollbackFollowTail()
        XCTAssertEqual(surface.rawScrollOffset, 0)
        XCTAssertEqual(surface.rawScrollPaintedStartLineID, 0)
        XCTAssertNil(surface.rawScrollRequestedHistory)
        XCTAssertFalse(surface.rawScrollDidRefillLeadingBlanks)

        let afterClear = surface.historyExceptPreservingRawScrollback([7])
        XCTAssertEqual(afterClear, [7])
    }
}

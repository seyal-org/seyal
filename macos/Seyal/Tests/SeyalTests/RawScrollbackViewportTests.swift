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
    func testClearRawScrollbackResetsOffsetAndKeepsOrdinaryBlockExcept() {
        let surface = MetalSurfaceView(frame: NSRect(x: 0, y: 0, width: 320, height: 200))
        surface.rawScrollOffset = 12
        surface.rawScrollPaintedStartLineID = 40
        surface.rawScrollDidRefillLeadingBlanks = true

        let kept = surface.historyExceptPreservingRawScrollback([7])
        XCTAssertTrue(kept.contains(7))
        XCTAssertTrue(kept.contains(RawScrollbackViewport.correlationBlockID))

        surface.clearRawScrollbackFollowTail()
        XCTAssertEqual(surface.rawScrollOffset, 0)
        XCTAssertEqual(surface.rawScrollPaintedStartLineID, 0)
        XCTAssertFalse(surface.rawScrollDidRefillLeadingBlanks)

        let afterClear = surface.historyExceptPreservingRawScrollback([7])
        XCTAssertEqual(afterClear, [7])
    }
}

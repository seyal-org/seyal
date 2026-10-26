import XCTest

/// Issue #923 headed evidence: with production pane splitting (C3), Split
/// Right/Down projects one region per Rust leaf, clicking a region focuses that
/// Pane, closing a non-last Pane removes its region, the surviving Pane still
/// offers hierarchical close, and exactly one live terminal surface exists,
/// inside the focused region. This file also covers #1219 zoom.
@MainActor
final class SeyalPaneSplitUITests: XCTestCase {
    override func setUp() {
        continueAfterFailure = false
    }

    private func hostedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchIsolatedHost()
        return app
    }

    func testSplitProjectsRegionsAndClickFocusesTheOtherPane() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertFalse(element(app, "seyal-pane-region-1").exists, "one Pane before splitting")

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12), "Split Right projects a second region")
        XCTAssertTrue(element(app, "seyal-pane-divider-0").exists, "the Split projects its divider")
        XCTAssertEqual(app.state, .runningForeground, "splitting must not crash Seyal.app")
        let first = try focusedRegionIndex(app, count: 2)
        assertSingleLiveSurface(in: app, focusedRegion: first)

        let other = first == 0 ? 1 : 0
        element(app, "seyal-pane-region-\(other)").click()
        waitForFocus(app, region: other)
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), other)
        assertSingleLiveSurface(in: app, focusedRegion: other)

        click(app, "seyal-split-down")
        XCTAssertTrue(element(app, "seyal-pane-region-2").waitForExistence(timeout: 12), "Split Down projects a third region")
        XCTAssertFalse(element(app, "seyal-pane-region-3").exists)
        let focused = try focusedRegionIndex(app, count: 3)
        assertSingleLiveSurface(in: app, focusedRegion: focused)
    }

    func testCloseRemovesNonLastPaneAndLastPaneOffersNoClose() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        XCTAssertTrue(
            element(app, "seyal-close-pane").waitForExistence(timeout: 5),
            "the sole Pane offers hierarchical Close Pane"
        )

        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        click(app, "seyal-close-pane")
        let gone = expectation(
            for: NSPredicate(format: "exists == false"),
            evaluatedWith: element(app, "seyal-pane-region-1"),
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 8), .completed, "closing a non-last Pane removes its region")
        XCTAssertEqual(app.state, .runningForeground)
        XCTAssertFalse(element(app, "seyal-pane-divider-0").exists, "no divider without a Split")
        XCTAssertEqual(try focusedRegionIndex(app, count: 1), 0)
        XCTAssertTrue(
            element(app, "seyal-close-pane").waitForExistence(timeout: 5),
            "the surviving Pane still offers hierarchical Close Pane"
        )
        waitForUsablePty(in: app, timeout: 12)
        assertSingleLiveSurface(in: app, focusedRegion: 0)
    }

    /// #1219 headed evidence: ⌘⇧↩ (`pane.zoom_toggle`) shows the focused leaf
    /// full-Tab without destroying its sibling; a second ⌘⇧↩ restores the split.
    func testZoomToggleFillsTabAndRestoresSplit() throws {
        let app = hostedApp()
        waitForUsablePty(in: app)
        click(app, "seyal-split-right")
        XCTAssertTrue(element(app, "seyal-pane-region-1").waitForExistence(timeout: 12))
        XCTAssertTrue(element(app, "seyal-pane-divider-0").exists)
        if element(app, "seyal-pane-region-1").value as? String != "focused" {
            app.typeKey(.rightArrow, modifierFlags: [.command, .option])
        }
        waitForFocus(app, region: 1)
        let sibling = element(app, "seyal-secondary-live-pane")
        XCTAssertTrue(sibling.waitForExistence(timeout: 12), "bound sibling has a live surface before zoom")
        let tab = element(app, "seyal-pane-region-0").frame.union(element(app, "seyal-pane-region-1").frame)
        let splitFrame = element(app, "seyal-pane-region-1").frame

        app.typeKey("\r", modifierFlags: [.command, .shift])
        waitForAbsence(element(app, "seyal-pane-region-0"), "zoom hides the occluded region")
        waitForAbsence(sibling, "the occluded leaf presents no surface")
        XCTAssertFalse(element(app, "seyal-pane-divider-0").exists, "no divider while zoomed")
        XCTAssertEqual(element(app, "seyal-pane-region-1").value as? String, "focused")
        assertFrame(element(app, "seyal-pane-region-1").frame, matches: tab, "zoomed leaf fills the Tab")
        assertSingleLiveSurface(in: app, focusedRegion: 1)
        XCTAssertEqual(app.state, .runningForeground)

        app.typeKey("\r", modifierFlags: [.command, .shift])
        XCTAssertTrue(element(app, "seyal-pane-region-0").waitForExistence(timeout: 8), "unzoom restores the sibling region")
        XCTAssertTrue(element(app, "seyal-pane-divider-0").waitForExistence(timeout: 8))
        XCTAssertTrue(sibling.waitForExistence(timeout: 12), "the sibling execution survived zoom")
        XCTAssertEqual(try focusedRegionIndex(app, count: 2), 1)
        assertFrame(element(app, "seyal-pane-region-1").frame, matches: splitFrame, "split ratio unchanged")
        waitForUsablePty(in: app)
    }

    // MARK: - Assertions

    private func focusedRegionIndex(_ app: XCUIApplication, count: Int) throws -> Int {
        let focused = (0..<count).filter { element(app, "seyal-pane-region-\($0)").value as? String == "focused" }
        XCTAssertEqual(focused.count, 1, "exactly one focused region among \(count)")
        return try XCTUnwrap(focused.first)
    }

    /// One live terminal surface exists, and it sits inside the focused region.
    private func assertSingleLiveSurface(in app: XCUIApplication, focusedRegion index: Int) {
        let terminals = app.descendants(matching: .any).matching(identifier: "terminal-input")
        let live = expectation(for: NSPredicate(format: "count == 1"), evaluatedWith: terminals, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [live], timeout: 12), .completed, "exactly one live terminal surface")
        let region = element(app, "seyal-pane-region-\(index)").frame
        let terminal = terminals.firstMatch.frame
        XCTAssertTrue(
            region.insetBy(dx: -1, dy: -1).contains(terminal),
            "live surface \(terminal) must sit inside focused region \(region)"
        )
    }

    // MARK: - Helpers

    private func element(_ app: XCUIApplication, _ identifier: String) -> XCUIElement {
        app.descendants(matching: .any)[identifier].firstMatch
    }

    private func click(_ app: XCUIApplication, _ identifier: String) {
        let control = element(app, identifier)
        XCTAssertTrue(control.waitForExistence(timeout: 8), "\(identifier) is shown")
        control.click()
    }

    private func waitForFocus(_ app: XCUIApplication, region index: Int) {
        let focused = expectation(
            for: NSPredicate(format: "value == 'focused'"),
            evaluatedWith: element(app, "seyal-pane-region-\(index)"),
            handler: nil
        )
        XCTAssertEqual(XCTWaiter.wait(for: [focused], timeout: 8), .completed, "region \(index) takes Rust focus")
    }

    private func waitForUsablePty(in app: XCUIApplication, timeout: TimeInterval = 20) {
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 10))
        let terminal = app.descendants(matching: .any)["terminal-input"]
        XCTAssertTrue(terminal.waitForExistence(timeout: 10), "terminal surface missing")
        let usable = NSPredicate(format: "value CONTAINS 'connection=usable'")
        let arrived = expectation(for: usable, evaluatedWith: terminal, handler: nil)
        let result = XCTWaiter.wait(for: [arrived], timeout: timeout)
        XCTAssertEqual(app.state, .runningForeground, "Seyal.app crashed while waiting for PTY attach")
        XCTAssertEqual(result, .completed, "PTY/runtime never became usable; terminal AX=\(terminal.value ?? "nil")")
    }

    private func waitForAbsence(_ element: XCUIElement, _ message: String) {
        let gone = expectation(for: NSPredicate(format: "exists == false"), evaluatedWith: element, handler: nil)
        XCTAssertEqual(XCTWaiter.wait(for: [gone], timeout: 8), .completed, message)
    }

    private func assertFrame(_ actual: CGRect, matches expected: CGRect, _ message: String) {
        XCTAssertEqual(actual.minX, expected.minX, accuracy: 1, message)
        XCTAssertEqual(actual.minY, expected.minY, accuracy: 1, message)
        XCTAssertEqual(actual.width, expected.width, accuracy: 1, message)
        XCTAssertEqual(actual.height, expected.height, accuracy: 1, message)
    }
}
